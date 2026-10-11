#!/usr/bin/env python3
"""Exercise the shared Memory Engine through actual local Responses/Embedding HTTP."""
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
            assert 0 < length <= 1024 * 1024
            request = json.loads(self.rfile.read(length))
            assert self.headers.get("Authorization") is None
            with self.lock:
                self.requests.append((self.path, request))
            if self.path == "/embeddings":
                assert request["model"] == "embedding-synthetic"
                assert request["dimensions"] == 2 and request["encoding_format"] == "float"
                wire = {"data": [{"index": i, "embedding": [1.0, 0.0]}
                                 for i, _ in enumerate(request["input"])]}
                media, body = "application/json", json.dumps(wire).encode()
            else:
                assert self.path == "/responses"
                assert request["tools"] == [] and request["tool_choice"] == "none"
                assert request["store"] is False
                if request["model"] == "integration-synthetic":
                    assert request["stream"] is False
                    text = json.dumps([{"text": "User prefers vim", "tags": ["editor"]}])
                else:
                    assert request["model"] == "chat-synthetic" and request["stream"] is True
                    text = "Synthetic memory-aware reply"
                wire = {"id": "i4-fixture", "status": "completed", "output": [
                    {"type": "message", "role": "assistant", "content": [
                        {"type": "output_text", "text": text}]}]}
                if request["stream"]:
                    frame = {"type": "response.completed", "response": wire}
                    media = "text/event-stream"
                    body = f"event: response.completed\ndata: {json.dumps(frame)}\n\n".encode()
                else:
                    media, body = "application/json", json.dumps(wire).encode()
            self.send_response(200)
            self.send_header("Content-Type", media)
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
        with tempfile.TemporaryDirectory(prefix="caidex-i4-demo-") as temporary:
            result = subprocess.run([
                "cargo", "run", "-p", "caidex-chat", "--example", "local_memory", "--locked", "--",
                str(Path(temporary) / "private"), f"http://127.0.0.1:{server.server_port}",
            ], capture_output=True, text=True, timeout=300, env=os.environ.copy())
            if result.returncode:
                raise RuntimeError(f"Memory client failed ({result.returncode}):\n{result.stderr}")
            report = json.loads(result.stdout)
            assert not Fixture.errors, Fixture.errors
            integrations = [r for p, r in Fixture.requests if r["model"] == "integration-synthetic"]
            embeddings = [r for p, r in Fixture.requests if p == "/embeddings"]
            chats = [r for p, r in Fixture.requests if r["model"] == "chat-synthetic"]
            assert len(integrations) == 1 and len(embeddings) == 3 and len(chats) == 2
            assert "Use emacs" in json.dumps(chats[0])
            assert "Use emacs" not in json.dumps(chats[1])
            report.update(http_requests=len(Fixture.requests), integration_http=1,
                          embedding_http=3, chat_http=2, fixture_assertions="passed", platform=os.name)
            output = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
            if args.output:
                args.output.write_text(output)
            print(output, end="")
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    main()
