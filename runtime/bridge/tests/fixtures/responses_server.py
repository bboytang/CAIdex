"""Loopback-only scripted Responses SSE server, never an AI provider or gateway.

Events follow the fixed upstream core/tests/common/responses.rs test format.
The only command offered by the approval cases writes a marker in a temp project.
"""

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from socketserver import TCPServer
import sys

mode, trace_path = sys.argv[1:]
trace = {"requests": 0, "authorizationSeen": False, "tool": None, "toolOutputs": []}


def event(kind, **fields):
    return {"type": kind, **fields}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        trace["requests"] += 1
        trace["authorizationSeen"] |= "Authorization" in self.headers
        if self.path != "/v1/responses":
            self.send_error(404)
            return
        for item in body.get("input", []):
            if item.get("type") in ["function_call_output", "custom_tool_call_output"]:
                trace["toolOutputs"].append(item.get("output"))
        identity = f"fixture-response-{trace['requests']}"
        events = [event("response.created", response={"id": identity})]
        if mode == "patch" and trace["requests"] == 1:
            trace["offeredTools"] = [{"name": tool.get("name"), "type": tool.get("type"), "nestedNames": [nested.get("name") for nested in tool.get("tools", [])]} for tool in body.get("tools", [])]
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            if not any(tool.get("name") == "apply_patch" for tool in body.get("tools", [])):
                self.send_error(500, "expected upstream patch tool; offered " + str([(tool.get("name"), tool.get("type")) for tool in body.get("tools", [])]))
                return
            marker = Path(trace_path).parent / "caidex-patch-marker.txt"
            patch = f"*** Begin Patch\n*** Add File: {marker.as_posix()}\n+CAIDEX_PATCH_APPLIED\n*** End Patch"
            trace["tool"] = "apply_patch"
            events.append(event("response.output_item.done", item={"type": "custom_tool_call", "call_id": "fixture-patch-1", "name": "apply_patch", "input": patch}))
        elif mode in ["approval", "questions"] and trace["requests"] == 1:
            tools = body.get("tools", [])
            names = [tool.get("name") for tool in tools]
            if mode == "questions" and "request_user_input" in names:
                name = "request_user_input"
                arguments = {"questions": [{"id": "choice", "header": "Fixture", "question": "Choose a local fixture option", "options": [{"label": "A", "description": "First fixture option"}, {"label": "B", "description": "Second fixture option"}]}]}
            elif mode == "questions":
                self.send_error(500, "expected upstream plan-mode input tool")
                return
            elif "exec_command" in names:
                name = "exec_command"
                arguments = {"cmd": "echo CAIDEX_TOOL_EXECUTED > caidex-tool-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex test marker only", "yield_time_ms": 1000}
            elif "shell_command" in names:
                name = "shell_command"
                arguments = {"command": "echo CAIDEX_TOOL_EXECUTED > caidex-tool-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex test marker only", "timeout_ms": 1000}
            else:
                self.send_error(500, "expected upstream shell tool")
                return
            trace["tool"] = name
            events.append(event("response.output_item.done", item={"type": "function_call", "call_id": "fixture-command-1", "name": name, "arguments": json.dumps(arguments)}))
        else:
            item = {"type": "message", "role": "assistant", "id": f"message-{identity}", "content": [{"type": "output_text", "text": "CAIdex local fixture complete"}]}
            events.append(event("response.output_item.added", item={**item, "content": []}))
            events.append(event("response.output_text.delta", delta="CAIdex local fixture complete"))
            events.append(event("response.output_item.done", item=item))
        events.append(event("response.completed", response={"id": identity, "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 0}}))
        Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
        data = "".join(f"event: {item['type']}\ndata: {json.dumps(item)}\n\n" for item in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


class LoopbackServer(ThreadingHTTPServer):
    def server_bind(self):
        # HTTPServer normally calls getfqdn; this offline fixture needs no DNS.
        TCPServer.server_bind(self)
        self.server_name = "localhost"
        self.server_port = self.server_address[1]


server = LoopbackServer(("127.0.0.1", 0), Handler)
print(json.dumps({"port": server.server_port}), flush=True)
server.serve_forever()
