"""H-2 independent Host demo: synthetic Responses, real pinned Codex execution."""
import argparse
import json
import os
from pathlib import Path
import secrets
import sqlite3
import subprocess
import sys
import tempfile
import time

from importlib import import_module
h1 = import_module("h1-demo")
Client, launch = h1.Client, h1.launch


def wait(client, operation, status):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        result = client.call("task/status", operation_id=operation)
        if result["task"]["status"] == status:
            return result["task"]
        time.sleep(0.02)
    raise AssertionError((operation, status, result))


def submit(client, operation, prompt, **fields):
    value = {"prompt": prompt, "model": "gpt-5.5", "provider": "caidex_h2_a", **fields}
    return client.call("task/submit", operation_id=operation, submission=value), value


def approve(client, task, operation):
    pending = [item for item in task["pending"].values() if item["status"] == "pending"]
    assert len(pending) == 1, task
    client.call("task/approval", task_id=task["task_id"], operation_id=operation,
                request_id=pending[0]["request_id"], decision="accept")


def demo(binary, root):
    root = root.resolve()
    token = secrets.token_hex(32)
    trace_path = root / "trace.json"
    fixture = subprocess.Popen([sys.executable, "runtime/bridge/tests/fixtures/responses_server.py",
                                "host-tasks", str(trace_path)], stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    process = None
    clients = []
    try:
        port = json.loads(fixture.stdout.readline())["port"]
        endpoint = f"http://127.0.0.1:{port}/v1"
        directory = root / "host"
        process, ready = launch(binary, directory, token, endpoint)
        a, b = Client(ready, token), Client(ready, token)
        clients += [a, b]
        initial = b.call("attach", host_id=ready["host_id"])["snapshot"]
        payload = {"prompt": "H2_EXEC isolated native tool", "model": "gpt-5.5", "provider": "caidex_h2_a"}
        # Drop a real submitter process before it reads the accepted response.
        lost = subprocess.run([sys.executable, __file__, "--lost-submit"],
                              input=json.dumps({"ready": ready, "token": token, "submission": payload}),
                              text=True, capture_output=True, timeout=20)
        assert lost.returncode == 0, lost.stderr
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            b.identity += 1
            b.send({"id": b.identity, "method": "task/status", "operation_id": "lost-submit"})
            while True:
                response = b.receive()
                if response.get("id") == b.identity:
                    break
                if "event" in response:
                    b.events.append(response["event"])
            if "result" in response:
                break
            time.sleep(0.02)
        assert "result" in response, response
        blocked = wait(b, "lost-submit", "blocked")
        marker = directory / "probe-project/h2-marker.txt"
        assert not marker.exists(), "native approval must precede actual execution"
        assert process.poll() is None, "client exit must not stop Host"
        assert blocked["actual"]["policy"] == "on-request"
        assert blocked["actual"]["reviewer"] == "user"
        assert blocked["actual"]["sandbox"]["type"] == "readOnly"
        same = a.call("task/submit", operation_id="lost-submit", submission=payload)
        assert same["task"]["task_id"] == blocked["task_id"]
        approve(b, blocked, "approve-one")
        completed = wait(b, "lost-submit", "completed")
        assert marker.read_text().splitlines() == ["H2_EXECUTED"]
        # Both observers recover from the same committed cursor.
        a.close(); b.close()
        a, b = Client(ready, token), Client(ready, token)
        clients += [a, b]
        replay_a = a.call("attach", host_id=ready["host_id"], after=initial["seq"])
        replay_b = b.call("attach", host_id=ready["host_id"], after=initial["seq"])
        assert replay_a == replay_b
        recovered = a.call("snapshot")
        assert recovered["tasks"][completed["task_id"]]["status"] == "completed"
        db = sqlite3.connect(f"file:{directory / 'journal.sqlite3'}?mode=ro", uri=True)
        assert db.execute("SELECT MAX(seq) FROM events").fetchone()[0] >= recovered["seq"]
        assert db.execute("SELECT COUNT(*) FROM events WHERE method='item/completed'").fetchone()[0] > 0
        db.close()
        second, _ = submit(a, "same-provider", "Boundary continuation", model="gpt-5.4",
                           parent_task_id=completed["task_id"], continue_thread=True)
        assert second["accepted"] and second["task"]["status"] == "submitted"
        continued = wait(a, "same-provider", "completed")
        assert continued["thread_id"] == completed["thread_id"]
        assert continued["submission"]["model"] == "gpt-5.4"
        assert json.loads(trace_path.read_text())["hostRequests"][-1]["model"] == "gpt-5.4"
        submit(a, "cross-provider", "Explicit visible text only: CAIdex local fixture complete",
               provider="caidex_h2_b", parent_task_id=continued["task_id"])
        target = wait(a, "cross-provider", "completed")
        assert target["thread_id"] != continued["thread_id"]
        assert target["submission"]["parent_task_id"] == continued["task_id"]
        submit(a, "cancel-me", "H2_EXEC cancellation fixture")
        cancel_task = wait(a, "cancel-me", "blocked")
        cancelled = a.call("task/cancel", task_id=cancel_task["task_id"], operation_id="cancel-one")
        assert cancelled["task"]["status"] == "cancel-requested"
        wait(a, "cancel-me", "cancelled")
        assert marker.read_text().splitlines() == ["H2_EXECUTED"]
        submit(a, "unknown-tool", "H2_EXEC_LOST hold result after actual native side effect")
        unknown = wait(a, "unknown-tool", "blocked")
        approve(a, unknown, "approve-unknown")
        deadline = time.monotonic() + 30
        while not (root / "h2-result-held").exists():
            assert time.monotonic() < deadline, "actual tool result was not captured by local fixture"
            time.sleep(0.02)
        assert marker.read_text().splitlines() == ["H2_EXECUTED", "H2_EXECUTED"]
        before = json.loads(trace_path.read_text())["requests"]
        a.close(); b.close()
        process.kill(); process.wait(timeout=15)
        process.stdout.close(); process.stderr.close()
        process, restarted = launch(binary, directory, token, endpoint)
        assert restarted["host_id"] == ready["host_id"]
        a, b = Client(restarted, token), Client(restarted, token)
        clients += [a, b]
        lost_task = wait(a, "unknown-tool", "unknown")
        original = a.call("task/submit", operation_id="unknown-tool", submission=lost_task["submission"])
        assert original["task"]["task_id"] == unknown["task_id"]
        assert a.call("attach", host_id=ready["host_id"]) == b.call("attach", host_id=ready["host_id"])
        assert a.call("task/status", operation_id="lost-submit")["task"]["status"] == "completed"
        trace = json.loads(trace_path.read_text())
        assert trace["requests"] == before, "restart/retry must not submit a model turn"
        assert marker.read_text().splitlines() == ["H2_EXECUTED", "H2_EXECUTED"]
        assert not trace["authorizationSeen"]
        snapshot = a.call("snapshot")
        a.call("shutdown")
        assert process.wait(timeout=15) == 0, process.stderr.read()
        report = {"status": "ok", "codex_version": "0.160.1", "clients": 2, "evidence_directory": str(root),
                  "submitted_tasks": len(snapshot["tasks"]), "native_marker_writes": 2,
                  "restart_model_requests": trace["requests"] - before,
                  "model_requests": trace["requests"], "native_tool_offers": len(trace["hostCommands"]),
                  "lost_response_same_operation": True, "native_approval_execution": True,
                  "native_cancelled": True, "same_provider_model_boundary": True,
                  "cross_profile_fresh_thread_association": True,
                  "replay_or_snapshot_identical": True, "executed_unconfirmed_task": lost_task["status"],
                  "authorization_header_seen": trace["authorizationSeen"],
                  "evidence_basis": {"counts": "measured local HTTP trace, marker lines and snapshot",
                                     "process_and_contract": "assertions against real independent Host and pinned Runtime",
                                     "commercial_calls_user_keys": "offline scenario declaration, no independent global telemetry"}}
        print(json.dumps(report))
    except BaseException:
        if process is not None and process.poll() is not None:
            print(process.stderr.read(), file=sys.stderr)
        raise
    finally:
        for client in clients:
            client.close()
        for child in [process, fixture]:
            if child is not None:
                if child.poll() is None:
                    child.kill(); child.wait(timeout=15)
                child.stdout.close(); child.stderr.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host-bin", default=str(Path("target/debug/caidex-host.exe" if os.name == "nt" else "target/debug/caidex-host").resolve()))
    parser.add_argument("--directory", type=Path, help="new demo directory, retained with journal/HTTP trace")
    parser.add_argument("--lost-submit", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.lost_submit:
        value = json.load(sys.stdin)
        client = Client(value["ready"], value["token"])
        client.send({"id": 1, "method": "task/submit", "operation_id": "lost-submit", "submission": value["submission"]})
        client.close()
        return
    assert os.environ.get("CAIDEX_CODEX_BIN"), "set CAIDEX_CODEX_BIN to pinned Codex"
    if args.directory:
        args.directory.mkdir(mode=0o700, parents=False, exist_ok=False)
        demo(args.host_bin, args.directory)
    else:
        # Preserve evidence; Windows can deny deletion of Runtime clone files.
        demo(args.host_bin, Path(tempfile.mkdtemp(prefix="caidex-h2-demo-")))


if __name__ == "__main__":
    main()
