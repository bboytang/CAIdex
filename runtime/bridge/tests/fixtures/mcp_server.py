"""Local MCP handshake-era stdio fixture. No network, commands, or credentials."""

import json
from pathlib import Path
import sys

trace_path = Path(sys.argv[1])
trace = {"initialized": False, "toolCalls": [], "elicitationReplies": []}
pending = {}


def emit(message):
    print(json.dumps({"jsonrpc": "2.0", **message}), flush=True)


def result(identity, value):
    emit({"id": identity, "result": value})


def save():
    trace_path.write_text(json.dumps(trace), encoding="utf-8")


for line in sys.stdin:
    request = json.loads(line)
    identity = request.get("id")
    method = request.get("method")
    params = request.get("params", {})
    if method is None:
        original = pending.pop(identity)
        response = request.get("result", {})
        trace["elicitationReplies"].append(response)
        save()
        result(original, {"content": [{"type": "text", "text": json.dumps(response)}], "structuredContent": response, "_meta": {"fixtureOpaque": "preserve"}})
    elif method == "initialize":
        trace["clientElicitationCapability"] = params.get("capabilities", {}).get("elicitation")
        save()
        result(identity, {"protocolVersion": "2025-06-18", "serverInfo": {"name": "CAIdex fixture", "version": "1.0"}, "capabilities": {"tools": {}, "resources": {}}})
    elif method == "notifications/initialized":
        trace["initialized"] = True
        save()
    elif method == "ping":
        result(identity, {})
    elif method == "tools/list":
        result(identity, {"tools": [{"name": name, "description": f"Local {name} fixture", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False}, "annotations": {"readOnlyHint": True}} for name in ["echo", "choose"]]})
    elif method == "resources/list":
        result(identity, {"resources": [{"uri": "fixture://marker", "name": "fixture-marker", "mimeType": "text/plain"}]})
    elif method == "resources/templates/list":
        result(identity, {"resourceTemplates": []})
    elif method == "resources/read":
        result(identity, {"contents": [{"uri": params["uri"], "mimeType": "text/plain", "text": "CAIdex local MCP resource"}]})
    elif method == "tools/call":
        assert trace["initialized"], "tools called before handshake finished"
        trace["toolCalls"].append(params["name"])
        save()
        if params["name"] == "echo":
            result(identity, {"content": [{"type": "text", "text": "CAIdex local MCP tool"}], "structuredContent": {"fixture": True}, "_meta": {"fixtureOpaque": "preserve"}})
        else:
            pending["fixture-elicit-1"] = identity
            emit({"id": "fixture-elicit-1", "method": "elicitation/create", "params": {"mode": "form", "message": "Choose a local fixture option", "requestedSchema": {"type": "object", "properties": {"choice": {"type": "string", "enum": ["A", "B"]}}, "required": ["choice"]}}})
    elif identity is not None:
        emit({"id": identity, "error": {"code": -32601, "message": "unknown fixture method"}})
