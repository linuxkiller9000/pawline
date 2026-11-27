# Mock OpenAI-compatible chat server for the pawline local-LLM test.
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
        req = json.loads(body)
        auth = self.headers.get('Authorization')
        sys.stderr.write(f"REQUEST path={self.path} model={req.get('model')} auth={auth} msgs={len(req.get('messages', []))}\n")
        if self.path != "/v1/chat/completions":
            self.send_response(404); self.end_headers(); return
        out = {"choices": [{"message": {"content": f"mock reply to: {req['messages'][-1]['content']}"}}]}
        data = json.dumps(out).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

HTTPServer(("127.0.0.1", 18123), H).serve_forever()
