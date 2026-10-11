"""H-1 failure peer, never evidence for a real Runtime or tool execution."""
import json
import os
import sys
import time

mode, marker = sys.argv[1:]


def emit(value):
    print(json.dumps(value), flush=True)


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        emit({"id": request["id"], "result": {"userAgent": "fixture"}})
    elif method == "thread/start" and mode.startswith("task-"):
        with open(marker, "a", encoding="utf-8") as stream:
            stream.write("thread/start\n")
        if mode == "task-drop-thread":
            sys.exit(0)
        if mode == "task-delay-thread":
            time.sleep(0.2)
        result = {"thread": {"id": "task-thread"}, "model": request["params"]["model"], "modelProvider": request["params"]["modelProvider"], "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandbox": {"type": "readOnly"}, "cwd": request["params"]["cwd"]}
        if mode == "task-wrong-policy":
            result["approvalPolicy"] = "never"
        emit({"id": request["id"], "result": result})
        emit({"method": "thread/started", "params": {"thread": result["thread"]}})
    elif method == "turn/start" and mode.startswith("task-"):
        with open(marker, "a", encoding="utf-8") as stream:
            stream.write("turn/start\n")
        if mode == "task-drop-turn":
            sys.exit(0)
        if mode == "task-reject-turn":
            emit({"id": request["id"], "error": {"code": -32000, "message": "synthetic rejection"}})
            continue
        turn = {"id": "task-turn", "status": "inProgress"}
        emit({"method": "turn/started", "params": {"threadId": "task-thread", "turn": turn}})
        emit({"id": request["id"], "result": {"turn": turn}})
        emit({"id": 77, "method": "item/commandExecution/requestApproval", "params": {"threadId": "task-thread", "turnId": "task-turn", "itemId": "task-item", "availableDecisions": ["accept", "cancel"], "extension": "retain"}})
    elif method == "turn/interrupt" and mode.startswith("task-"):
        with open(marker, "a", encoding="utf-8") as stream:
            stream.write("turn/interrupt\n")
        emit({"id": request["id"], "result": {}})
    elif method is None and mode.startswith("task-"):
        with open(marker, "a", encoding="utf-8") as stream:
            stream.write("approval/reply\n")
        if mode == "task-drop-approval":
            sys.exit(0)
        emit({"method": "turn/completed", "params": {"threadId": "task-thread", "turn": {"id": "task-turn", "status": "completed"}}})
    elif method == "thread/start":
        with open(marker, "a", encoding="utf-8", newline="\n") as stream:
            stream.write("thread/start\n")
            stream.flush()
            os.fsync(stream.fileno())
        if mode == "exit":
            sys.exit(0)
        if mode == "delay":
            time.sleep(60)
        emit({"id": request["id"], "result": {"thread": {"id": "fixture-thread"}}})
        emit({"method": "thread/started", "params": {"thread": {"id": "fixture-thread", "unknown": "keep"}}})
    elif method == "config/read":
        # Acknowledge injection before an event can deliberately stop the Host.
        emit({"id": request["id"], "result": {}})
        for event in request.get("params", {}).get("fixtureEvents", []):
            emit(event)
