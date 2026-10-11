#!/usr/bin/env python3
"""Real H-3 Host: delegated approval race, native patch/Diff/Review, recovery."""
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
from concurrent.futures import ThreadPoolExecutor

from importlib import import_module
h1, h2 = import_module("h1-demo"), import_module("h2-demo")
Client, launch, submit, wait = h1.Client, h1.launch, h2.submit, h2.wait


def request(client, method, **fields):
    client.identity += 1
    client.send({"id": client.identity, "method": method, **fields})
    while True:
        value = client.receive()
        if value.get("id") == client.identity:
            return value
        if "event" in value:
            client.events.append(value["event"])


def rejected(ready, token):
    address, port = ready["address"].rsplit(":", 1)
    with socket.create_connection((address, int(port)), timeout=10) as connection:
        with connection.makefile("rwb") as stream:
            stream.write(json.dumps({"protocol": 1, "token": token}).encode() + b"\n"); stream.flush()
            value = json.loads(stream.readline())
            assert value.get("error") == "authorization/protocol rejected", value


def approval_fields(task, operation):
    pending = [r for r in task["pending"].values() if r["status"] == "pending"]
    assert len(pending) == 1, task
    return {"task_id": task["task_id"], "request_id": pending[0]["request_id"], "operation_id": operation, "decision": "accept"}


def demo(binary, root):
    token = secrets.token_hex(32)
    directory, trace_path = root / "host", root / "trace.json"
    clients, process, fixture = [], None, None
    try:
        env = {key: os.environ[key] for key in ["PATH", "SystemRoot", "SYSTEMROOT", "WINDIR", "TEMP", "TMP", "TMPDIR"] if key in os.environ}
        fixture = subprocess.Popen([sys.executable, "runtime/bridge/tests/fixtures/responses_server.py", "host-h3", str(trace_path)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8", env=env)
        port = json.loads(fixture.stdout.readline())["port"]
        endpoint = f"http://127.0.0.1:{port}/v1"
        process, ready = launch(binary, directory, token, endpoint)
        owner = Client(ready, token); clients.append(owner)
        project = directory / "probe-project"
        # Only the newly created, isolated demo project is initialized or modified.
        for arguments in [["init"], ["config", "user.name", "H3 Fixture"], ["config", "user.email", "h3-fixture@example.invalid"]]:
            subprocess.run(["git", "-C", str(project), *arguments], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        (project / "baseline.txt").write_text("private fixture baseline\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(project), "add", "baseline.txt"], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        subprocess.run(["git", "-C", str(project), "commit", "-m", "private fixture baseline"], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        grants = {}
        for name in ["device-a", "device-b"]:
            grants[name] = owner.call("client/grant", client_id=name, scopes=["observe", "approve"])["token"]
        a, b = Client(ready, grants["device-a"]), Client(ready, grants["device-b"])
        clients += [a, b]
        initial = a.call("attach", host_id=ready["host_id"])["snapshot"]
        b.call("attach", host_id=ready["host_id"])
        denied = request(a, "task/submit", operation_id="unauthorized-submit", submission={"prompt": "never execute", "model": "gpt-5.5", "provider": "caidex_h2_a"})
        assert "error" in denied
        submit(owner, "h3-patch", "H3_REQUEST_PATCH isolate and wait for file approval")
        blocked = wait(owner, "h3-patch", "blocked")
        marker = project / "h3-change.txt"
        assert not marker.exists(), "patch must wait for explicit native approval"
        assert process.poll() is None
        fields_a, fields_b = approval_fields(blocked, "race-a"), approval_fields(blocked, "race-b")
        with ThreadPoolExecutor(max_workers=2) as workers:
            first = workers.submit(request, a, "task/approval", **fields_a)
            second = workers.submit(request, b, "task/approval", **fields_b)
            ra, rb = first.result(), second.result()
        assert ("result" in ra) + ("result" in rb) == 1, (ra, rb)
        winner, winning_client = ("race-a", "device-a") if "result" in ra else ("race-b", "device-b")
        completed = wait(owner, "h3-patch", "completed")
        winning = next(r for r in completed["pending"].values() if r.get("operation_id") == winner)
        assert winning["client_id"] == winning_client
        assert marker.read_text(encoding="utf-8").strip() == "CAIDEX_H3_PATCH"
        # Native Diff can arrive after turn/completed; require it within a bound.
        deadline = time.monotonic() + 30
        while "diff" not in completed["artifacts"]:
            assert time.monotonic() < deadline, ("native Diff missing after completion", completed)
            time.sleep(0.02)
            completed = owner.call("task/status", operation_id="h3-patch")["task"]
        assert "CAIDEX_H3_PATCH" in completed["artifacts"]["diff"]["params"]["diff"]
        assert any(v.get("params", {}).get("item", {}).get("type") == "fileChange" for v in completed["artifacts"].values())
        retry_client, retry_fields = (a, fields_a) if winner == "race-a" else (b, fields_b)
        retry = retry_client.call("task/approval", **retry_fields)
        assert retry["operation"]["operation_id"] == winner
        late = request(a, "task/approval", **{**fields_a, "operation_id": "late-answer"})
        assert "error" in late
        owner.call("client/revoke", client_id="device-b")
        rejected(ready, grants["device-b"])
        a.close(); a = Client(ready, grants["device-a"]); clients.append(a)
        assert a.call("snapshot")["tasks"][completed["task_id"]]["artifacts"] == completed["artifacts"]
        submit(owner, "h3-review", "Review uncommitted fixture changes", review=True)
        reviewed = wait(owner, "h3-review", "completed")
        kinds = {v.get("params", {}).get("item", {}).get("type") for v in reviewed["artifacts"].values()}
        assert {"enteredReviewMode", "exitedReviewMode"} <= kinds, reviewed
        submit(owner, "h3-revoke", "H3_REQUEST_REVOKE explicit request revocation")
        revoke_task = wait(owner, "h3-revoke", "blocked")
        fields = approval_fields(revoke_task, "request-revoke")
        revoked = a.call("task/approval-revoke", **{k: v for k, v in fields.items() if k != "decision"})
        assert any(r["status"] == "revoked" for r in revoked["task"]["pending"].values())
        wait(owner, "h3-revoke", "cancelled")
        assert not (project / "h3-revoked.txt").exists()
        # Ensure an old committed cursor requires the bounded snapshot path.
        for index in range(16):
            snapshot = owner.call("snapshot")
            if snapshot["seq"] - initial["seq"] > 128:
                break
            submit(owner, f"gap-{index}", "No tool: isolated recovery watermark")
            wait(owner, f"gap-{index}", "completed")
        assert owner.call("snapshot")["seq"] - initial["seq"] > 128
        recovery = a.call("attach", host_id=ready["host_id"], after=initial["seq"])
        assert recovery["mode"] == "snapshot"
        assert recovery["snapshot"]["tasks"][completed["task_id"]]["artifacts"] == completed["artifacts"]
        submit(owner, "h3-unanswered", "H3_REQUEST_PENDING leave approval unanswered across restart")
        pending = wait(owner, "h3-unanswered", "blocked")
        assert not (project / "h3-unanswered.txt").exists()
        before = json.loads(trace_path.read_text())["requests"]
        process.kill(); process.wait(timeout=15)
        process.stdout.close(); process.stderr.close()
        for client in clients:
            client.close()
        clients = []
        process, restarted = launch(binary, directory, token, endpoint)
        owner, a = Client(restarted, token), Client(restarted, grants["device-a"])
        clients += [owner, a]
        unknown = wait(owner, "h3-unanswered", "unknown")
        assert "error" in request(a, "task/approval", **approval_fields(pending, "stale-answer"))
        snapshot = a.call("snapshot")
        assert snapshot["clients"]["device-b"]["revoked"]
        rejected(restarted, grants["device-b"])
        assert snapshot["tasks"][completed["task_id"]]["artifacts"] == completed["artifacts"]
        assert snapshot["tasks"][reviewed["task_id"]]["artifacts"] == reviewed["artifacts"]
        assert snapshot["tasks"][completed["task_id"]]["pending"] == completed["pending"]
        trace = json.loads(trace_path.read_text())
        assert trace["requests"] == before
        assert not trace["authorizationSeen"]
        assert not (project / "h3-unanswered.txt").exists()
        db = sqlite3.connect(f"file:{directory / 'journal.sqlite3'}?mode=ro", uri=True)
        assert db.execute("SELECT COUNT(*) FROM events WHERE method='item/agentMessage/delta'").fetchone()[0] > 0
        claimed = sum(1 for (data,) in db.execute("SELECT data FROM events WHERE method='host/task'") if (json.loads(data).get("operation") or {}).get("operation_id") == winner and json.loads(data)["operation"]["outcome"] == "sending")
        assert claimed == 1
        for secret in grants.values():
            assert db.execute("SELECT COUNT(*) FROM events WHERE instr(data,?)>0", (secret,)).fetchone()[0] == 0
        db.close()
        owner.call("shutdown")
        assert process.wait(timeout=15) == 0, process.stderr.read()
        print(json.dumps({"status": "ok", "codex_version": "0.160.1", "clients": 2, "evidence_directory": str(root), "approval_claims": claimed, "winning_operation": winner, "native_patch_contents": marker.read_text(encoding="utf-8").strip(), "native_diff_restored": True, "native_review_restored": True, "revoked_client_rejected": True, "revoked_request_not_executed": True, "snapshot_gap_recovered": True, "unanswered_task": unknown["status"], "restart_model_requests": trace["requests"] - before, "model_requests": trace["requests"], "native_patch_offers": len(trace["h3Patches"]), "authorization_header_seen": trace["authorizationSeen"], "evidence_basis": {"counts": "measured SQLite claim count and local HTTP trace", "native_flow": "real independent Host/Runtime, file contents and wire assertions", "commercial_calls_user_keys": "offline scenario declaration, no independent global telemetry"}}))
    finally:
        failed = sys.exc_info()[0] is not None
        if failed:
            print(json.dumps({"evidence_directory": str(root)}), file=sys.stderr)
        for client in clients:
            client.close()
        for child in [process, fixture]:
            if child is not None:
                if child.poll() is None:
                    child.kill(); child.wait(timeout=15)
                if failed:
                    print(child.stderr.read(), file=sys.stderr)
                child.stdout.close(); child.stderr.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host-bin", default=str(Path("target/debug/caidex-host.exe" if os.name == "nt" else "target/debug/caidex-host").resolve()))
    parser.add_argument("--directory", type=Path, help="new private demo directory retained for audit")
    args = parser.parse_args()
    assert os.environ.get("CAIDEX_CODEX_BIN"), "set CAIDEX_CODEX_BIN to pinned Codex"
    root = args.directory or Path(tempfile.mkdtemp(prefix="caidex-h3-demo-"))
    if args.directory:
        root.mkdir(mode=0o700, exist_ok=False)
    demo(args.host_bin, root)


if __name__ == "__main__":
    main()
