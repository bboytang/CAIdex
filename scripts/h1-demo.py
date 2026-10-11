"""Independent H-1 process demo: pinned Codex, no model turns or user secrets."""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time


class Client:
    def __init__(self, ready, token):
        host, port = ready["address"].rsplit(":", 1)
        self.socket = socket.create_connection((host, int(port)), timeout=20)
        self.stream = self.socket.makefile("rwb")
        self.identity = 0
        self.events = []
        self.send({"protocol": 1, "token": token})
        assert self.receive()["authorized"]

    def send(self, value):
        self.stream.write(json.dumps(value).encode() + b"\n")
        self.stream.flush()

    def receive(self):
        line = self.stream.readline()
        assert line, "Host disconnected"
        return json.loads(line)

    def call(self, method, **fields):
        self.identity += 1
        self.send({"id": self.identity, "method": method, **fields})
        while True:
            value = self.receive()
            if value.get("id") == self.identity:
                assert "error" not in value, value
                return value["result"]
            if "event" in value:
                self.events.append(value["event"])
            assert "gap" not in value, "reconnect from last committed cursor"

    def close(self):
        self.stream.close()
        self.socket.close()


def launch(binary, directory, token, offline_endpoint=None):
    environment = os.environ.copy()
    environment["CAIDEX_HOST_TOKEN"] = token
    arguments = [binary, "run", str(directory)]
    if offline_endpoint is not None:
        arguments += ["--offline-responses", offline_endpoint]
    process = subprocess.Popen(arguments, env=environment,
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True)
    # Reading readiness in a child with communicate gives a finite launch deadline.
    import concurrent.futures
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(process.stdout.readline)
        try:
            line = future.result(timeout=30)
        except concurrent.futures.TimeoutError:
            process.kill()
            process.wait(timeout=10)
            raise AssertionError("Host readiness timed out") from None
    if not line:
        raise AssertionError(process.communicate(timeout=10)[1])
    return process, json.loads(line)


def wait_threads(client, minimum):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        snapshot = client.call("snapshot")
        if len(snapshot["threads"]) >= minimum:
            return snapshot
        time.sleep(0.02)
    raise AssertionError("thread event was not captured")


def demo(binary, directory):
    directory = directory.resolve()
    token = secrets.token_hex(32)
    process, ready = launch(binary, directory, token)
    clients = []
    try:
        a, b = Client(ready, token), Client(ready, token)
        clients += [a, b]
        first = a.call("attach", host_id=ready["host_id"])["snapshot"]
        b.call("attach", host_id=ready["host_id"], after=first["seq"])
        a.call("probe")
        snapshot = wait_threads(b, 1)
        # A separate observer process connects, attaches, and exits. No Host stop.
        observer = subprocess.run([sys.executable, __file__, "--observer"],
                                  input=json.dumps({"ready": ready, "token": token}),
                                  text=True, capture_output=True, timeout=20)
        assert observer.returncode == 0, observer.stderr
        assert process.poll() is None
        a.call("detach")
        a.close()
        b.close()
        a, b = Client(ready, token), Client(ready, token)
        clients += [a, b]
        replay_a = a.call("attach", host_id=ready["host_id"], after=first["seq"])
        replay_b = b.call("attach", host_id=ready["host_id"], after=first["seq"])
        assert replay_a == replay_b
        assert replay_a["mode"] == "replay"
        assert any(event["method"] == "thread/started" for event in replay_a["events"])
        events = replay_a["events"]
        assert [event["seq"] for event in events] == list(range(first["seq"] + 1, replay_a["through_seq"] + 1))
        connection = sqlite3.connect(f"file:{directory / 'journal.sqlite3'}?mode=ro", uri=True)
        assert connection.execute("SELECT MAX(seq) FROM events").fetchone()[0] >= events[-1]["seq"]
        connection.close()
        cached = wait_threads(a, 1)
        a.close()
        b.close()
        assert process.poll() is None
        process.kill()  # Actual abrupt Host process death, not a mock restart.
        process.wait(timeout=15)
        process.stdout.close()
        process.stderr.close()
        connection = sqlite3.connect(f"file:{directory / 'journal.sqlite3'}?mode=ro", uri=True)
        cached = json.loads(connection.execute("SELECT data FROM snapshot WHERE id=1").fetchone()[0])
        submitted = connection.execute("SELECT COUNT(*) FROM events WHERE method='host/probeStarted'").fetchone()[0]
        connection.close()
        process, restarted = launch(binary, directory, token)
        assert restarted["host_id"] == ready["host_id"]
        a, b = Client(restarted, token), Client(restarted, token)
        clients += [a, b]
        recovered = a.call("snapshot")
        assert recovered["seq"] > cached["seq"]
        connection = sqlite3.connect(f"file:{directory / 'journal.sqlite3'}?mode=ro", uri=True)
        restarted_submitted = connection.execute("SELECT COUNT(*) FROM events WHERE method='host/probeStarted'").fetchone()[0]
        assert restarted_submitted == submitted, "restart must not replay probes"
        connection.close()
        assert recovered["stream"] == cached["stream"] + 1
        assert recovered["threads"]
        assert all(thread["runtime_state"] == "unknown" for thread in recovered["threads"].values())
        assert a.call("attach", host_id=restarted["host_id"], after=cached["seq"]) == b.call("attach", host_id=restarted["host_id"], after=cached["seq"])
        # Initial attach always offers an atomic, current snapshot.
        assert b.call("attach", host_id=restarted["host_id"])["snapshot"] == recovered
        a.call("shutdown")
        assert process.wait(timeout=15) == 0, process.stderr.read()
        report = {"status": "ok", "codex_version": "0.160.1", "host_id": ready["host_id"],
                  "before_restart_seq": cached["seq"], "after_restart_seq": recovered["seq"],
                  "clients": 2, "observer_process_exit_detaches": True,
                  "replay_identical": True, "snapshot_restored": True,
                  "old_runtime_state": "unknown", "restart_resubmissions": restarted_submitted - submitted,
                  "model_turns": 0, "commercial_calls": 0, "user_keys_read": False,
                  "evidence_basis": {
                      "restart_resubmissions": "measured SQLite probeStarted count difference",
                      "client_exit_replay_snapshot": "asserted process, socket and snapshot checks",
                      "model_turns_commercial_calls_user_keys_read": "fixed scenario declarations; no independent telemetry"}}
        print(json.dumps(report))
    finally:
        for client in clients:
            client.close()
        if process.poll() is None:
            process.kill()
            process.wait(timeout=15)
        process.stdout.close()
        process.stderr.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host-bin", default=str(Path("target/debug/caidex-host.exe" if os.name == "nt" else "target/debug/caidex-host").resolve()))
    parser.add_argument("--directory", type=Path, help="new private Host directory; retained after demo")
    parser.add_argument("--observer", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.observer:
        value = json.load(sys.stdin)
        client = Client(value["ready"], value["token"])
        client.call("attach", host_id=value["ready"]["host_id"])
        client.close()
        return
    assert os.environ.get("CAIDEX_CODEX_BIN"), "set CAIDEX_CODEX_BIN to pinned Codex 0.160.1"
    if args.directory:
        assert not args.directory.exists(), "demo requires a new directory; existing user data is never removed"
        demo(args.host_bin, args.directory)
    else:
        with tempfile.TemporaryDirectory(prefix="caidex-h1-demo-") as temporary:
            demo(args.host_bin, Path(temporary) / "host")


if __name__ == "__main__":
    main()
