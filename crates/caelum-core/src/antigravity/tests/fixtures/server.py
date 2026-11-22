#!/usr/bin/env python3
import json
import sys
import threading

state = {
    "sessions": {},
    "counter": 0,
    "pending_permissions": {},
}
write_lock = threading.Lock()


def send(obj):
    with write_lock:
        sys.stdout.write(json.dumps(obj) + "\n")
        sys.stdout.flush()


def notify(session_id, update):
    send({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {"sessionId": session_id, "update": update},
    })


def session_payload(session_id, model="gemini-4-neon", effort="high", mode="default"):
    return {
        "sessionId": session_id,
        "models": {
            "currentModelId": f"{model}-{effort}",
            "availableModels": [
                {"modelId": "gemini-4-neon-low", "name": "Gemini 4 Neon (Low)"},
                {"modelId": "gemini-4-neon-medium", "name": "Gemini 4 Neon (Medium)"},
                {"modelId": "gemini-4-neon-high", "name": "Gemini 4 Neon (High)"},
                {"modelId": "claude-opus-5-5-medium", "name": "Claude Opus 5.5 (Medium)"},
                {"modelId": "claude-opus-5-5-high", "name": "Claude Opus 5.5 (High)"},
            ],
        },
        "modes": {
            "currentModeId": mode,
            "availableModes": [
                {"id": "default", "name": "Default", "description": "Ask before edits"},
                {"id": "plan", "name": "Plan", "description": "Plan before executing"},
            ],
        },
        "configOptions": [
            {
                "id": "model",
                "name": "Model",
                "category": "model",
                "type": "select",
                "currentValue": model,
                "options": [
                    {"value": "gemini-4-neon", "name": "Gemini 4 Neon"},
                    {"value": "claude-opus-5-5", "name": "Claude Opus 5.5"},
                ],
            },
            {
                "id": "thought_level",
                "name": "Thinking Level",
                "category": "thought_level",
                "type": "select",
                "currentValue": effort,
                "options": [
                    {"value": "low", "name": "Low"},
                    {"value": "medium", "name": "Medium"},
                    {"value": "high", "name": "High"},
                ],
            },
            {
                "id": "mode",
                "name": "Mode",
                "category": "mode",
                "type": "select",
                "currentValue": mode,
                "options": [
                    {"value": "default", "name": "Default"},
                    {"value": "plan", "name": "Plan"},
                ],
            },
        ],
    }


def run_prompt(req_id, session_id, blocks):
    text = " ".join(b.get("text", "") for b in blocks if b.get("type") == "text").strip()
    sess = state["sessions"][session_id]
    sess["history"].append({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": text}})

    if "need-permission" in text:
        perm_id = f"perm-{req_id}"
        ev = threading.Event()
        box = {}
        state["pending_permissions"][perm_id] = (ev, box)
        send({
            "jsonrpc": "2.0",
            "id": perm_id,
            "method": "session/request_permission",
            "params": {
                "sessionId": session_id,
                "toolCall": {
                    "toolCallId": "call-perm",
                    "name": "run_command",
                    "title": "Run shell command",
                    "rawInput": {"command": "echo hi"},
                },
                "options": [
                    {"optionId": "allow_once", "name": "Allow once", "kind": "allow_once"},
                    {"optionId": "reject_once", "name": "Reject", "kind": "reject_once"},
                ],
            },
        })
        ev.wait(timeout=10)
        outcome = box.get("result", {}).get("outcome", {})
        reply = f"permission:{outcome.get('optionId', outcome.get('outcome', 'unknown'))}"
    else:
        thought = {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Checking files."}}
        tool_start = {
            "sessionUpdate": "tool_call",
            "toolCallId": "call-1",
            "name": "read_file",
            "title": "Read README.md",
            "status": "in_progress",
            "rawInput": {"path": "README.md"},
        }
        tool_done = {
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call-1",
            "status": "completed",
            "content": [{"type": "content", "content": {"type": "text", "text": "ok"}}],
        }
        notify(session_id, thought)
        notify(session_id, tool_start)
        notify(session_id, tool_done)
        sess["history"].extend([thought, tool_start, tool_done])
        reply = f"echo:{sess['model']}:{sess['effort']}:{sess['mode']}:{text}"

    msg = {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply}}
    usage = {"sessionUpdate": "usage_update", "used": 1234, "size": 1048576}
    notify(session_id, msg)
    notify(session_id, usage)
    sess["history"].extend([msg, usage])
    send({"jsonrpc": "2.0", "id": req_id, "result": {"stopReason": "end_turn"}})


for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    if "method" not in msg and "id" in msg:
        perm_id = str(msg["id"])
        if perm_id in state["pending_permissions"]:
            ev, box = state["pending_permissions"].pop(perm_id)
            box["result"] = msg.get("result", {})
            ev.set()
        continue

    method = msg.get("method")
    req_id = msg.get("id")
    params = msg.get("params") or {}

    if method == "initialize":
        send({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {"loadSession": True, "sessionCapabilities": {"list": {}, "load": {}, "close": {}}},
                "agentInfo": {"name": "Fake Antigravity ACP", "version": "1.0.0"},
            },
        })
    elif method == "session/new":
        state["counter"] += 1
        sid = f"acp-sess-{state['counter']}"
        state["sessions"][sid] = {
            "cwd": params.get("cwd", ""),
            "title": "New session",
            "updatedAt": "2026-11-22T01:00:00Z",
            "model": "gemini-4-neon",
            "effort": "high",
            "mode": "default",
            "history": [],
        }
        send({"jsonrpc": "2.0", "id": req_id, "result": session_payload(sid)})
        notify(sid, {
            "sessionUpdate": "available_commands_update",
            "availableCommands": [
                {"name": "plan", "description": "Plan before executing"},
                {"name": "compact", "description": "Compact conversation"},
            ],
        })
    elif method == "session/list":
        cwd = params.get("cwd", "")
        sessions = [
            {
                "sessionId": sid,
                "cwd": info["cwd"],
                "title": info["title"],
                "updatedAt": info["updatedAt"],
            }
            for sid, info in state["sessions"].items()
            if not cwd or info["cwd"] == cwd
        ]
        send({"jsonrpc": "2.0", "id": req_id, "result": {"sessions": sessions}})
    elif method == "session/load":
        sid = params["sessionId"]
        info = state["sessions"][sid]
        for update in info["history"]:
            notify(sid, update)
        send({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": session_payload(sid, info["model"], info["effort"], info["mode"]),
        })
    elif method == "session/close":
        state["sessions"].pop(params.get("sessionId"), None)
        send({"jsonrpc": "2.0", "id": req_id, "result": {}})
    elif method == "session/set_config_option":
        sid = params["sessionId"]
        if params.get("configId") == "model":
            state["sessions"][sid]["model"] = params["value"]
        elif params.get("configId") == "thought_level":
            state["sessions"][sid]["effort"] = params["value"]
        send({"jsonrpc": "2.0", "id": req_id, "result": {}})
    elif method == "session/set_mode":
        sid = params["sessionId"]
        state["sessions"][sid]["mode"] = params["modeId"]
        send({"jsonrpc": "2.0", "id": req_id, "result": {}})
    elif method == "session/prompt":
        sid = params["sessionId"]
        blocks = params.get("prompt", [])
        threading.Thread(target=run_prompt, args=(req_id, sid, blocks), daemon=True).start()
    elif method == "session/cancel":
        pass
    elif req_id is not None:
        send({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32601, "message": f"Unknown method {method}"}})
