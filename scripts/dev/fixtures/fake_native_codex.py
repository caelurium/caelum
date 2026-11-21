#!/usr/bin/env python3
"""Minimal Codex app-server fixture for the native SSH smoke test."""
import base64
import hashlib
import json
import os
import socket
import socketserver
import struct

GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def read_exact(stream, size):
    data = bytearray()
    while len(data) < size:
        part = stream.recv(size - len(data))
        if not part:
            raise EOFError
        data.extend(part)
    return bytes(data)


def send_frame(stream, payload, opcode=1):
    payload = payload.encode() if isinstance(payload, str) else payload
    size = len(payload)
    if size < 126:
        header = bytes((0x80 | opcode, size))
    elif size <= 0xFFFF:
        header = bytes((0x80 | opcode, 126)) + struct.pack("!H", size)
    else:
        header = bytes((0x80 | opcode, 127)) + struct.pack("!Q", size)
    stream.sendall(header + payload)


def response(stream, request_id, result):
    send_frame(stream, json.dumps({"id": request_id, "result": result}))


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        stream = self.request
        headers = bytearray()
        while not headers.endswith(b"\r\n\r\n"):
            part = stream.recv(1)
            if not part:
                return
            headers.extend(part)
        request = bytes(headers).decode("ascii", "replace")
        first = request.split("\r\n", 1)[0]
        if first == "GET /readyz HTTP/1.1":
            stream.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            return
        key = next(
            (line.split(":", 1)[1].strip() for line in request.split("\r\n")
             if line.lower().startswith("sec-websocket-key:")),
            None,
        )
        if key is None:
            stream.sendall(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")
            return
        accept = base64.b64encode(hashlib.sha1((key + GUID).encode()).digest()).decode()
        stream.sendall(
            (
                "HTTP/1.1 101 Switching Protocols\r\n"
                "Upgrade: websocket\r\nConnection: Upgrade\r\n"
                f"Sec-WebSocket-Accept: {accept}\r\n\r\n"
            ).encode()
        )
        while True:
            try:
                first, second = read_exact(stream, 2)
                opcode = first & 0x0F
                masked = second & 0x80
                size = second & 0x7F
                if size == 126:
                    size = struct.unpack("!H", read_exact(stream, 2))[0]
                elif size == 127:
                    size = struct.unpack("!Q", read_exact(stream, 8))[0]
                mask = read_exact(stream, 4) if masked else b""
                payload = read_exact(stream, size)
                if masked:
                    payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
                if opcode == 8:
                    send_frame(stream, b"", 8)
                    return
                if opcode == 9:
                    send_frame(stream, payload, 10)
                    continue
                if opcode != 1:
                    continue
                message = json.loads(payload)
                method = message.get("method")
                request_id = message.get("id")
                params = message.get("params") or {}
                if request_id is None:
                    continue
                if method == "initialize":
                    response(stream, request_id, {"userAgent": "codex/0.143.0 (fixture)"})
                elif method == "thread/list":
                    response(stream, request_id, {
                        "data": [{
                            "id": "fixture-working-thread",
                            "name": "Working thread",
                            "preview": "An unrelated active turn",
                            "status": {"type": "active"},
                        }],
                        "nextCursor": None,
                    })
                elif method == "thread/resume":
                    send_frame(stream, json.dumps({
                        "id": request_id,
                        "error": {"code": -32000, "message": "The existing turn is still active."},
                    }))
                elif method == "model/list":
                    response(stream, request_id, {"data": [{
                        "model": "fixture-model",
                        "displayName": "Fixture model",
                        "isDefault": True,
                        "defaultReasoningEffort": "medium",
                        "supportedReasoningEfforts": [{"reasoningEffort": "medium"}],
                    }], "nextCursor": None})
                elif method == "thread/start":
                    response(stream, request_id, {
                        "thread": {
                            "id": "fixture-thread",
                            "cwd": params["cwd"],
                            "name": None,
                            "preview": "",
                            "status": {"type": "idle"},
                        },
                        "model": params.get("model") or "fixture-model",
                        "reasoningEffort": "medium",
                    })
                elif method == "turn/start":
                    thread = params["threadId"]
                    send_frame(stream, json.dumps({
                        "method": "turn/started",
                        "params": {"threadId": thread, "turn": {"id": "fixture-turn"}},
                    }))
                    send_frame(stream, json.dumps({
                        "method": "item/agentMessage/delta",
                        "params": {
                            "threadId": thread,
                            "turnId": "fixture-turn",
                            "itemId": "fixture-item",
                            "delta": "Codex ran over the VM WebSocket.",
                        },
                    }))
                    send_frame(stream, json.dumps({
                        "method": "turn/completed",
                        "params": {
                            "threadId": thread,
                            "turn": {"id": "fixture-turn", "status": "completed"},
                        },
                    }))
                    response(stream, request_id, {
                        "turn": {"id": "fixture-turn", "status": "inProgress"},
                    })
                else:
                    send_frame(stream, json.dumps({
                        "id": request_id,
                        "error": {"code": -32601, "message": f"Unsupported fixture method: {method}"},
                    }))
            except (EOFError, OSError, ValueError, KeyError, json.JSONDecodeError):
                return


class Server(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


if __name__ == "__main__":
    with Server(("127.0.0.1", int(os.environ["CAELUM_CODEX_PORT"])), Handler) as server:
        server.serve_forever()
