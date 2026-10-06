"""Deterministic protocol peer. Never contacts a model or executes user commands."""

import json
import sys
import time


def emit(message):
    print(json.dumps(message), flush=True)


held = None
for line in sys.stdin:
    request = json.loads(line)
    if "method" not in request:
        emit({"method": "fixture/replied", "params": request})
        continue
    method = request["method"]
    identity = request.get("id")
    if method == "test/first":
        held = identity
    elif method == "test/second":
        emit({"method": "future/unknown", "params": {"opaque": "signature"}, "extension": [1, 2]})
        emit({"id": identity, "result": "second"})
        emit({"id": held, "result": "first"})
    elif method == "test/approval":
        emit({"id": identity, "result": {}})
        emit({"id": "approval-7", "method": "item/commandExecution/requestApproval", "params": {"command": "fixture only"}, "extension": True})
    elif method == "test/error":
        emit({"id": identity, "error": {"code": -32001, "message": "overloaded", "data": {"retry": True}}})
    elif method == "test/close":
        break
    elif method == "test/hang":
        emit({"method": "fixture/waiting", "params": {}})
        time.sleep(60)
    elif method == "test/malformed":
        print("{bad json", flush=True)
    elif method == "test/missingResult":
        emit({"id": identity})
    elif method == "test/burst":
        emit({"method": "fixture/event", "params": 1})
        emit({"method": "fixture/event", "params": 2})
        emit({"id": identity, "result": None})
    else:
        emit({"id": identity, "result": request.get("params")})
