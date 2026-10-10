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
        for event in request.get("params", {}).get("fixtureEvents", []):
            emit(event)
        emit({"id": request["id"], "result": {}})
