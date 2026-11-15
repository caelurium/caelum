#!/usr/bin/env python3
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

lock = threading.Lock()
config = {}
requests = []


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def reply(self, value, status=200):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def handle_request(self):
        url = urlsplit(self.path)
        path = url.path
        body = json.loads(self.rfile.read(int(self.headers.get('Content-Length', '0'))) or b'{}')
        with lock:
            if path == '/test/config':
                config.update(body)
                self.reply({})
                return
            if path == '/test/requests':
                self.reply(requests)
                requests.clear()
                return
            requests.append(f'{self.command} {path}')
            state = dict(config)
        session = {
            'id': state.get('id', 'ses_read'),
            'title': 'Saved session',
            'location': {'directory': state.get('directory', os.getcwd())},
            'time': {'updated': 123, 'archived': 100 if state.get('archived') else None},
            'model': {'providerID': 'local', 'id': state.get('model', 'original'), 'variant': 'high'},
            'agent': 'build',
        }
        if path == '/api/event':
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            self.wfile.write(b'data: {"type":"server.connected","data":{}}\n\n')
            while True:
                self.wfile.write(b': heartbeat\n\n')
                self.wfile.flush()
                time.sleep(0.1)
        elif path == '/api/session/active':
            if state.get('active_error'):
                self.reply({'error': 'activity unavailable'}, 503)
            else:
                self.reply({'data': {'ses_read': {'type': 'busy'}} if state.get('active') else {}})
        elif path == '/api/session':
            self.reply({'data': [session]})
        elif path == '/api/session/ses_read' and self.command == 'GET':
            self.reply({'data': session})
        elif path == '/api/session/ses_read/message':
            query = parse_qs(url.query)
            assert query in [
                {'limit': ['50'], 'order': ['desc']},
                {'limit': ['50'], 'cursor': ['opaque/+?=']},
            ], query
            if state.get('history_error'):
                self.reply({'error': 'history unavailable'}, 503)
            else:
                self.reply({
                    'data': [{'id': name, 'type': 'user', 'text': name} for name in ['newer', 'older']],
                    'cursor': {'next': 'opaque/+?=' if 'order' in query else None},
                })
        elif path in ['/api/form', '/api/shell', '/api/session/ses_read/inbox', '/api/session/ses_read/diff']:
            self.reply({'data': []})
        elif path == '/api/shell/sh_read':
            self.reply({'data': {'metadata': {'sessionID': 'ses_read'}}})
        elif path == '/api/shell/sh_read/output':
            self.reply({'data': {'output': 'tool output', 'size': 11}})
        elif path == '/api/session/ses_read/generate':
            self.reply({'data': {'text': 'Transient answer'}})
        elif self.command != 'GET' and path.startswith('/api/session/ses_read'):
            self.reply({})
        else:
            self.reply({'error': 'unexpected request'}, 404)

    do_GET = handle_request
    do_POST = handle_request
    do_PATCH = handle_request
    do_DELETE = handle_request


server = ThreadingHTTPServer(('127.0.0.1', int(sys.argv[-1])), Handler)
if not os.environ.get('OPENCODE_SERVER_PASSWORD'):
    print('server password test-secret', flush=True)
server.serve_forever()
