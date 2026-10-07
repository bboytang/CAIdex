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
        if mode.startswith("wire-"):
            trace["gatewayCredentialMatched"] = self.headers.get("Authorization") == "Bearer CAIDEX_GATEWAY_PROVIDER_TEST_KEY"
            trace.setdefault("wireRequests", []).append({"body": body, "liteHeader": self.headers.get("x-openai-internal-codex-responses-lite"), "accept": self.headers.get("Accept"), "organization": self.headers.get("OpenAI-Organization"), "project": self.headers.get("OpenAI-Project")})
        for item in body.get("input", []):
            if item.get("type") in ["function_call_output", "custom_tool_call_output"]:
                trace["toolOutputs"].append(item.get("output"))
        if mode == "compact":
            trace.setdefault("summarySeen", []).append("CAIDEX_COMPACT_SUMMARY" in json.dumps(body.get("input", [])))
        identity = f"fixture-response-{trace['requests']}"
        events = [event("response.created", response={"id": identity})]
        if mode == "wire-stall":
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            self.protocol_version = "HTTP/1.1"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            data = f"event: response.created\ndata: {json.dumps(events[0])}\n\n".encode()
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            self.wfile.flush()
            Path(trace_path).with_name("gateway-streaming").touch()
            try:
                disconnected = self.connection.recv(1) == b""
            except OSError:
                disconnected = True  # Reset and EOF both prove socket cancellation.
            trace["gatewayDisconnected"] = disconnected
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            Path(trace_path).with_name("gateway-disconnected").touch()
            self.close_connection = True
            return
        if mode.startswith("wire-"):
            reasoning = {"type": "reasoning", "id": f"rs_{identity}", "summary": [], "encrypted_content": "CAIDEX_OPAQUE_REASONING+/==", "provider_signature": "CAIDEX_FUTURE_SIGNATURE=="}
            if mode.startswith("wire-anthropic-"):
                native = {"type": "message", "id": identity, "model": "native-fixture", "role": "assistant", "content": [
                    {"type": "thinking", "thinking": "Native fixture thinking", "signature": "signed+/==\n", "future": "retain"},
                    {"type": "redacted_thinking", "data": "opaque+/==\n"},
                    {"type": "text", "text": "CAIdex local fixture complete", "citations": [{"type": "future_citation", "opaque": "retain"}]},
                    {"type": "future_block", "number": 18446744073709551616}],
                    "stop_reason": "end_turn", "usage": {"input_tokens": 0, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0, "output_tokens": 0}}
                reasoning["summary"] = [{"type": "summary_text", "text": "Native fixture thinking"}]
                reasoning["encrypted_content"] = "caidex.anthropic.native-message.v1:" + json.dumps({"provider": "anthropic", "version": 1, "message": native})
            events.append(event("response.output_item.done", item=reasoning))
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
        elif mode == "queue" or (mode in ["approval", "questions"] and trace["requests"] == 1):
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
            text = "" if mode == "goal-empty" else "CAIDEX_COMPACT_SUMMARY" if mode == "compact" and trace["requests"] == 2 else "CAIdex local fixture complete"
            item = {"type": "message", "role": "assistant", "id": f"message-{identity}", "content": [{"type": "output_text", "text": text}]}
            if mode.startswith("goal-") or mode.startswith("wire-"):
                item["phase"] = "final_answer"
            events.append(event("response.output_item.added", item={**item, "content": []}))
            if text:
                events.append(event("response.output_text.delta", delta=text))
            events.append(event("response.output_item.done", item=item))
        tokens = 100 if mode == "goal-budget" else 0
        events.append(event("response.completed", response={"id": identity, "usage": {"input_tokens": tokens, "output_tokens": 0, "total_tokens": tokens}}))
        if mode.startswith("wire-"):
            trace.setdefault("wireResponses", []).append(events)
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
