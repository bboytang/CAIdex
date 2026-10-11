#!/usr/bin/env python3
"""I-4.2: independent Host/client, actual Broker HTTP, pinned Runtime source, no real keys."""
import argparse
import concurrent.futures
import http.server
import importlib
import json
import os
from pathlib import Path
import secrets
import socket
import socketserver
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time

h1 = importlib.import_module("h1-demo")
h2 = importlib.import_module("h2-demo")
KEY = "synthetic-i42-host-owned-credential"


class Fixture(http.server.BaseHTTPRequestHandler):
    calls = []
    errors = []
    lock = threading.Lock()
    started = threading.Event()
    release = threading.Event()

    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            assert self.headers.get("Authorization") == f"Bearer {KEY}"
            size = int(self.headers["Content-Length"])
            assert 0 < size <= 65536
            request = json.loads(self.rfile.read(size))
            assert KEY not in json.dumps(request)
            with self.lock:
                self.calls.append((self.path, request))
            if self.path == "/embeddings":
                assert request["model"] == "embedding-synthetic" and request["dimensions"] == 2
                body = {"data": [{"index": 0, "embedding": [1.0, 0.0]}]}
            else:
                assert self.path == "/responses" and request["model"] == "integration-synthetic"
                assert request["tools"] == [] and request["tool_choice"] == "none"
                assert request["stream"] is False and request["store"] is False
                text = request["input"][-1]["content"]
                if "SLOW" in text:
                    self.started.set()
                    assert self.release.wait(30), "synthetic HTTP release timeout"
                body = {"id": "i42-fixture", "status": "completed", "output": [{
                    "type": "message", "role": "assistant", "content": [{
                        "type": "output_text", "text": json.dumps([{"text": "Use vim", "tags": ["editor"]}])}]}]}
            data = json.dumps(body).encode()
            try:
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                # The two SLOW scenarios explicitly cancel/kill Host after this POST was observed.
                if self.path != "/responses" or "SLOW" not in request["input"][-1]["content"]:
                    raise
        except Exception as error:
            with self.lock:
                self.errors.append(type(error).__name__)
            self.send_error(500)


class Proxy(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


class Forward(socketserver.StreamRequestHandler):
    def handle(self):
        try:
            with socket.create_connection(self.server.target, timeout=10) as upstream:
                remote = upstream.makefile("rwb")
                auth = self.rfile.readline(65537)
                remote.write(auth)
                remote.flush()
                self.wfile.write(remote.readline(65537))
                self.wfile.flush()
                raw = self.rfile.readline(65537)
                if not raw:
                    return
                request = json.loads(raw)
                remote.write(raw)
                remote.flush()
                reply = remote.readline(65537)
                if request.get("method") == "memory/submit" and self.server.drop_reply.is_set():
                    self.server.drop_reply.clear()
                    return  # Drop a real accepted response; status/recovery must never resubmit.
                self.wfile.write(reply)
                self.wfile.flush()
                remote.close()
        except (OSError, ValueError):
            pass  # Deliberate offline phase is represented to the client by a closed connection.


def read_json(process):
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(process.stdout.readline)
        try:
            line = future.result(timeout=45)
        except concurrent.futures.TimeoutError:
            process.kill()
            process.wait(timeout=10)
            raise AssertionError("internal process response timeout") from None
    if not line:
        raise AssertionError(process.communicate(timeout=10)[1])
    assert KEY not in line
    return json.loads(line)


def send(process, action, **fields):
    process.stdin.write(json.dumps({"action": action, **fields}) + "\n")
    process.stdin.flush()


def run_new(process, text):
    send(process, "run", text=text)
    operation = read_json(process)["operation_id"]
    return operation, read_json(process)


def terminate(process, tree=False):
    if process.poll() is None:
        if tree and os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
        else:
            process.kill()
    process.wait(timeout=15)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    assert os.environ.get("CAIDEX_CODEX_BIN"), "set the pinned Codex executable"
    version = subprocess.run([os.environ["CAIDEX_CODEX_BIN"], "--version"], capture_output=True,
                             text=True, timeout=15, env={name: os.environ[name] for name in
                             ["PATH", "HOME", "SystemRoot", "USERPROFILE", "TEMP", "TMP"] if name in os.environ})
    assert version.returncode == 0 and version.stdout.strip() == "codex-cli 0.160.1"
    binaries = {name: Path("target/debug/examples") / (name + (".exe" if os.name == "nt" else ""))
                for name in ["memory_host", "memory_client"]}
    subprocess.run(["cargo", "build", "-p", "caidex-host", "--examples", "--locked"], check=True)
    fixture = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    fixture_thread = threading.Thread(target=fixture.serve_forever, daemon=True)
    fixture_thread.start()
    proxy = Proxy(("127.0.0.1", 0), Forward)
    proxy.drop_reply = threading.Event()
    proxy_thread = threading.Thread(target=proxy.serve_forever, daemon=True)
    proxy_thread.start()
    processes, clients, hosts = [], [], []
    temporary = tempfile.TemporaryDirectory(prefix="caidex-i42-demo-")
    try:
        root = Path(temporary.name)
        runtime_fixture = subprocess.Popen([sys.executable, "runtime/bridge/tests/fixtures/responses_server.py",
            "host-tasks", str(root / "runtime-trace.json")], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        processes.append(runtime_fixture)
        runtime_port = read_json(runtime_fixture)["port"]
        token = secrets.token_hex(32)
        environment = os.environ.copy()
        environment.update(CAIDEX_HOST_TOKEN=token, CAIDEX_I42_SYNTHETIC_KEY=KEY)

        def launch(port):
            process = subprocess.Popen([str(binaries["memory_host"]), str(root / "host"),
                f"http://127.0.0.1:{fixture.server_port}", f"http://127.0.0.1:{runtime_port}", str(port)],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=environment)
            processes.append(process)
            hosts.append(process)
            return process, read_json(process)

        host, ready = launch(0)
        address = ready["address"].rsplit(":", 1)
        proxy.target = (address[0], int(address[1]))
        owner = h1.Client(ready, token)
        clients.append(owner)
        pair = owner.call("client/grant", client_id="memory-demo", scopes=["memory"])["token"]
        memory = h1.Client(ready, pair)
        clients.append(memory)
        client_env = os.environ.copy()
        client_env["CAIDEX_I42_PAIR_TOKEN"] = pair
        # No model Key is passed to the separate client process.
        for name in ["CAIDEX_I42_SYNTHETIC_KEY", "CAIDEX_HOST_TOKEN", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"]:
            client_env.pop(name, None)
        client = subprocess.Popen([str(binaries["memory_client"]), str(root / "client"),
            f"127.0.0.1:{proxy.server_address[1]}", ready["host_id"]], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=client_env)
        processes.append(client)
        assert read_json(client) == {"ready": True, "automatic_default": False}
        _, result = run_new(client, "default off vim")
        assert result["state"] == "waiting" and not Fixture.calls
        send(client, "enable")
        assert read_json(client)["enabled"]
        send(client, "resume")
        assert read_json(client)["resumed"] == 1
        send(client, "embedding")
        assert read_json(client)["semantic_results"] == 1
        assert len(Fixture.calls) == 3
        proxy.drop_reply.set()
        lost, result = run_new(client, "lost response vim")
        assert "unknown" in result["error"]
        send(client, "recover")
        assert read_json(client)["recovered"] == 1
        assert len(Fixture.calls) == 4
        memory.close()
        owner.call("shutdown")
        owner.close()
        assert host.wait(timeout=15) == 0, host.stderr.read()
        before = len(Fixture.calls)
        _, result = run_new(client, "offline vim")
        assert result["state"] == "waiting" and len(Fixture.calls) == before
        host, restarted = launch(int(address[1]))
        assert restarted["host_id"] == ready["host_id"]
        owner = h1.Client(restarted, token)
        memory = h1.Client(restarted, pair)
        clients += [owner, memory]
        send(client, "resume")
        assert read_json(client)["resumed"] == 1 and len(Fixture.calls) == 5
        h2.submit(owner, "i42-native-source", "H2_EXEC isolated source evidence")
        blocked = h2.wait(owner, "i42-native-source", "blocked")
        h2.approve(owner, blocked, "i42-native-approval")
        task = h2.wait(owner, "i42-native-source", "completed")
        assert (root / "host/probe-project/h2-marker.txt").read_text().splitlines() == ["H2_EXECUTED"]
        send(client, "source-denied", task_id=task["task_id"])
        assert read_json(client)["source_denied"]
        owner.call("memory/evidence-grant", host_id=ready["host_id"], client_id="memory-demo",
                   task_id=task["task_id"], scope={"type": "personal"})
        send(client, "source", task_id=task["task_id"])
        source = read_json(client)
        assert source["source_current"] and source["candidate_count"] == 1
        assert len(Fixture.calls) == 6
        Fixture.started.clear()
        Fixture.release.clear()
        send(client, "run", text="SLOW cancel vim")
        cancel = read_json(client)["operation_id"]
        assert Fixture.started.wait(15)
        memory.call("memory/cancel", host_id=ready["host_id"], operation_id=cancel)
        assert "cancelled" in read_json(client)["error"]
        Fixture.release.set()
        time.sleep(0.1)
        Fixture.started.clear()
        Fixture.release.clear()
        send(client, "run", text="SLOW restart vim")
        unknown = read_json(client)["operation_id"]
        assert Fixture.started.wait(15)
        terminate(host, tree=True)
        memory.close()
        owner.close()
        assert "unknown" in read_json(client)["error"]
        Fixture.release.set()
        before = len(Fixture.calls)
        host, restarted = launch(int(address[1]))
        owner = h1.Client(restarted, token)
        clients.append(owner)
        send(client, "recover")
        assert "unknown" in read_json(client)["error"]
        assert len(Fixture.calls) == before
        same, result = run_new(client, "SLOW restart vim")
        assert same == unknown and "already submitted" in result["error"]
        send(client, "source-denied", task_id=task["task_id"])
        assert read_json(client)["source_denied"]  # Operator evidence policy must be reconfigured after restart.
        owner.call("client/revoke", client_id="memory-demo")
        _, result = run_new(client, "revoked vim")
        assert "refused" in result["error"] and len(Fixture.calls) == before
        send(client, "exit")
        client.stdin.close()
        assert client.wait(timeout=15) == 0, client.stderr.read()
        owner.call("shutdown")
        owner.close()
        assert host.wait(timeout=15) == 0, host.stderr.read()
        db = sqlite3.connect(root / "host/memory-operations.sqlite3")
        states = dict(db.execute("SELECT id,state FROM operations"))
        assert states[cancel] == "cancelled" and states[unknown] == "unknown"
        assert db.execute("SELECT count(*) FROM operations").fetchone()[0] == 8
        db.close()
        for file in (root / "host").glob("memory-operations*"):
            raw = file.read_bytes()
            for value in [KEY, token, pair, "SLOW", "Use vim"]:
                assert value.encode() not in raw
        assert not Fixture.errors, Fixture.errors
        assert len(Fixture.calls) == 8
        trace = json.loads((root / "runtime-trace.json").read_text())
        report = {"milestone": "I-4.2", "status": "ok", "codex_version": "0.160.1",
                  "automatic_default": False, "host_memory_http": 8,
                  "integration_http": 6, "embedding_http": 2, "host_broker_authorization": "synthetic header only",
                  "lost_response_recovered_without_replay": True, "offline_waiting_explicit_resume": True,
                  "cancelled": states[cancel], "restart_original_operation": states[unknown],
                  "restart_memory_replays": 0, "runtime_http": trace["requests"],
                  "native_marker_lines": 1, "execution_source": "authorized completed pinned Runtime task summary",
                  "revocation_rejected": True, "uploads": 0, "platform": os.name}
        output = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        if args.output:
            args.output.write_text(output)
        print(output, end="")
    finally:
        Fixture.release.set()
        for client in clients:
            try:
                client.close()
            except OSError:
                pass
        for process in reversed(processes):
            terminate(process, tree=process in hosts)
        proxy.shutdown()
        proxy.server_close()
        proxy_thread.join(timeout=5)
        fixture.shutdown()
        fixture.server_close()
        fixture_thread.join(timeout=5)
        temporary.cleanup()


if __name__ == "__main__":
    main()
