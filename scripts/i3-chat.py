#!/usr/bin/env python3
"""Run the I-3 shared Chat client against a local synthetic Responses server."""
import argparse
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading


class Fixture(http.server.BaseHTTPRequestHandler):
    requests = []
    errors = []
    lock = threading.Lock()

    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            length = int(self.headers.get("Content-Length", "0"))
            assert 0 < length <= 12 * 1024 * 1024
            request = json.loads(self.rfile.read(length))
            assert self.path == "/responses"
            assert self.headers.get("Authorization") is None
            assert request["model"] == "synthetic"
            assert request["tools"] == [] and request["tool_choice"] == "none"
            assert request["stream"] is True and request["store"] is False
            assert "PRIVATE-CARRIER" not in json.dumps(request)
            with self.lock:
                self.requests.append(request)
            text = "Synthetic Chat reply"
            frames = [
                {"type": "response.output_text.delta", "delta": "Synthetic "},
                {"type": "response.output_text.delta", "delta": "Chat reply"},
                {"type": "response.completed", "response": {
                    "id": "synthetic-response", "status": "completed", "output": [
                        {"type": "reasoning", "encrypted_content": "PRIVATE-CARRIER"},
                        {"type": "message", "role": "assistant", "content": [
                            {"type": "output_text", "text": text}]},
                    ]}},
            ]
            body = "".join(f"event: {v['type']}\ndata: {json.dumps(v)}\n\n" for v in frames).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception as error:
            with self.lock:
                self.errors.append(type(error).__name__)
            self.send_error(500)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="caidex-i3-demo-") as temporary:
            private_root = Path(temporary) / "chat"
            result = subprocess.run([
                "cargo", "run", "-p", "caidex-chat", "--example", "local_chat", "--locked", "--",
                str(private_root), f"http://127.0.0.1:{server.server_port}/responses",
            ], capture_output=True, text=True, timeout=300, env=os.environ.copy())
            if result.returncode:
                raise RuntimeError(f"Internal Chat client failed ({result.returncode}):\n{result.stderr}")
            report = json.loads(result.stdout)
            assert not Fixture.errors, Fixture.errors
            assert len(Fixture.requests) == 4
            first = Fixture.requests[0]
            assert first["input"][0]["content"][1]["file_data"].startswith("data:text/plain;base64,")
            report["http_requests"] = len(Fixture.requests)
            report["fixture_request_assertions"] = "passed"
            report["platform"] = os.name
            if args.output:
                args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
            print(json.dumps(report, ensure_ascii=False, indent=2))
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    main()
