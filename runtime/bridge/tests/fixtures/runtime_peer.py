"""Stateful app-server protocol peer; no model, network, files, or shell actions."""

import json
import sys


def emit(message):
    print(json.dumps(message), flush=True)


def notify(method, params):
    emit({"method": method, "params": params})


initialized = False
handshake = False
threads = {}
active = None
for line in sys.stdin:
    request = json.loads(line)
    if "method" not in request:
        notify("fixture/replied", request)
        continue
    method = request["method"]
    identity = request.get("id")
    params = request.get("params", {})
    if method == "initialize":
        handshake = True
        emit({"id": identity, "result": {"userAgent": "fixture", "platformFamily": "fixture", "platformOs": "fixture", "codexHome": "/fixture"}})
        notify("fixture/initialize", params)
        continue
    if method == "initialized":
        initialized = handshake
        continue
    if not initialized:
        emit({"id": identity, "error": {"code": -32000, "message": "Not initialized"}})
        continue
    if method == "config/read" and "fixtureEvents" in params:
        for event in params["fixtureEvents"]:
            emit(event)
        emit({"id": identity, "result": {}})
        continue
    if method == "thread/start":
        thread = {"id": f"thread-{len(threads) + 1}", "turns": [], "extension": {"opaque": "retain"}}
        threads[thread["id"]] = thread
        emit({"id": identity, "result": {"thread": thread}})
        notify("thread/started", {"thread": thread})
        continue
    if method == "thread/read" or method == "thread/resume":
        emit({"id": identity, "result": {"thread": threads[params["threadId"]], "received": params}})
        continue
    if method == "thread/fork":
        thread = {"id": f"thread-{len(threads) + 1}", "turns": threads[params["threadId"]]["turns"][:]}
        threads[thread["id"]] = thread
        emit({"id": identity, "result": {"thread": thread, "received": params}})
        continue
    if method == "turn/start":
        active = {"id": "turn-1", "status": "inProgress", "items": []}
        threads[params["threadId"]]["turns"].append(active)
        emit({"id": identity, "result": {"turn": active, "received": params}})
        notify("turn/started", {"threadId": params["threadId"], "turn": active})
        notify("item/agentMessage/delta", {"threadId": params["threadId"], "turnId": active["id"], "itemId": "message-1", "delta": "fixture", "opaque": "retain"})
        continue
    if method == "turn/steer":
        if active and params["expectedTurnId"] == active["id"]:
            emit({"id": identity, "result": {"turnId": active["id"], "received": params}})
        else:
            emit({"id": identity, "error": {"code": -32602, "message": "active turn mismatch"}})
        continue
    if method == "turn/interrupt":
        active["status"] = "interrupted"
        emit({"id": identity, "result": {}})
        notify("turn/completed", {"threadId": params["threadId"], "turn": active})
        active = None
        continue
    emit({"id": identity, "result": {"wire": request}})
