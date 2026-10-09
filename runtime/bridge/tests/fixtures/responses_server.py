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
Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")


def event(kind, **fields):
    return {"type": kind, **fields}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        if not mode.startswith("native-anthropic-") or self.path != "/v1/organizations/me":
            self.send_error(404)
            return
        assert self.headers.get("x-api-key") == "CAIDEX_GATEWAY_PROVIDER_TEST_KEY"
        trace["organizationLookups"] = trace.get("organizationLookups", 0) + 1
        data = json.dumps({"type": "organization", "id": "org-fixture", "name": "fixture"}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def native_anthropic(self, body):
        assert self.path == "/v1/messages"
        trace["gatewayCredentialMatched"] = self.headers.get("x-api-key") == "CAIDEX_GATEWAY_PROVIDER_TEST_KEY"
        assert trace["gatewayCredentialMatched"]
        beta = "thinking-binding-controls-2026-08-01"
        if mode == "native-anthropic-discovery":
            beta += ",inline-tools-2026-09-15"
        assert self.headers.get("anthropic-beta") == beta
        trace.setdefault("nativeRequests", []).append(body)
        native = {"type": "message", "id": f"native-{trace['requests']}", "model": "native-fixture", "role": "assistant", "content": [
            {"type": "thinking", "thinking": "Native fixture thinking", "signature": "signed+/==\n", "future": "retain"},
            {"type": "redacted_thinking", "data": "opaque+/==\n"},
            {"type": "text", "text": "CAIdex local fixture complete", "citations": [{"type": "future_citation", "opaque": "retain"}]},
            {"type": "future_block", "number": 18446744073709551616}],
            "stop_reason": "end_turn", "input_transformations": [], "usage": {"input_tokens": 2, "output_tokens": 3}}
        if mode == "native-anthropic-tools-lite" and trace["requests"] == 1:
            tool = next(tool for tool in body["tools"] if tool["description"].startswith("Tool identity: functions::exec."))
            command = {"cmd": "echo CAIDEX_NATIVE_CODE_MODE > caidex-native-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex native fixture marker only", "yield_time_ms": 1000}
            script = "const result = await tools.exec_command(" + json.dumps(command) + "); text(result); text('CAIDEX_NATIVE_CODE_MODE');"
            native["content"][2] = {"type": "tool_use", "id": "native-code-mode-one", "name": tool["name"], "input": {"input": script}}
            native["stop_reason"] = "tool_use"
        if mode == "native-anthropic-discovery" and trace["requests"] <= 2:
            if trace["requests"] == 1:
                tool = next(tool for tool in body["tools"] if tool["description"].startswith("Tool identity: tool_search."))
                identity, arguments = "native-discovery-search", {"query": "fixture echo", "limit": 1}
            else:
                definitions = [block["tool"]["definition"] for message in body["messages"] if message["role"] == "system" for block in message["content"] if block["type"] == "tool_addition" and block["tool"]["type"] == "tool_definition"]
                tool = next(tool for tool in definitions if tool["description"].startswith("Tool identity: mcp__fixture::echo."))
                identity, arguments = "native-discovery-echo", {}
            native["content"][2] = {"type": "tool_use", "id": identity, "name": tool["name"], "input": arguments}
            native["stop_reason"] = "tool_use"
        start = {**native, "content": [], "stop_reason": None}
        events = [event("message_start", message=start)]
        if mode == "native-anthropic-stall":
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            self.protocol_version = "HTTP/1.1"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("anthropic-organization-id", "org-fixture")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            data = f"event: message_start\ndata: {json.dumps(events[0])}\n\n".encode()
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            self.wfile.flush()
            Path(trace_path).with_name("gateway-streaming").touch()
            try:
                disconnected = self.connection.recv(1) == b""
            except OSError:
                disconnected = True
            trace["gatewayDisconnected"] = disconnected
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            Path(trace_path).with_name("gateway-disconnected").touch()
            self.close_connection = True
            return
        for index, block in enumerate(native["content"]):
            if mode == "native-anthropic-discovery" and block["type"] == "tool_use":
                events.append(event("content_block_start", index=index, content_block={**block, "input": {}}))
                arguments = json.dumps(block["input"])
                split = len(arguments) // 2
                for partial in [arguments[:split], arguments[split:]]:
                    events.append(event("content_block_delta", index=index, delta={"type": "input_json_delta", "partial_json": partial}))
            else:
                events.append(event("content_block_start", index=index, content_block=block))
            events.append(event("content_block_stop", index=index))
        events.extend([event("message_delta", delta={"stop_reason": native["stop_reason"]}, usage={"output_tokens": 3}), event("message_stop")])
        trace.setdefault("nativeResponses", []).append(native)
        Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
        data = "".join(f"event: {item['type']}\ndata: {json.dumps(item)}\n\n" for item in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("anthropic-organization-id", "org-fixture")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def native_google(self, body):
        assert self.path == "/v1beta/models/native-fixture:streamGenerateContent?alt=sse"
        trace["gatewayCredentialMatched"] = self.headers.get("x-goog-api-key") == "CAIDEX_GATEWAY_PROVIDER_TEST_KEY"
        assert trace["gatewayCredentialMatched"]
        assert all(name not in self.headers for name in ["Authorization", "session_id", "x-client-request-id", "x-codex-turn-metadata", "x-codex-turn-state"])
        trace.setdefault("nativeRequests", []).append(body)
        parts = [{"thought": True, "text": "Native fixture thinking", "thoughtSignature": "signed+/==\n", "future": "retain"},
                 {"text": "CAIdex local ", "thoughtSignature": "text-a"},
                 {"text": "fixture complete", "thoughtSignature": "text-b"},
                 {"futurePart": {"number": 18446744073709551616, "opaque": "retain"}}]
        if mode in ["native-google-tools-lite", "native-google-multi-lite", "native-google-mcp"] and trace["requests"] == 1:
            declarations = body["tools"][0]["functionDeclarations"]
            if mode == "native-google-mcp":
                tool = next(tool for tool in declarations if "Tool identity: mcp__fixture::echo." in tool["description"])
                arguments = {}
            else:
                tool = next(tool for tool in declarations if "Tool identity: functions::exec." in tool["description"])
                command = {"cmd": "echo CAIDEX_NATIVE_CODE_MODE > caidex-native-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex native fixture marker only", "yield_time_ms": 1000}
                script = "const result = await tools.exec_command(" + json.dumps(command) + "); text(result); text('CAIDEX_NATIVE_CODE_MODE');"
                arguments = {"input": script}
            call = {"functionCall": {"id": "google-tool-one", "name": tool["name"], "args": arguments}, "thoughtSignature": "tool-signed+/==\n", "future": "keep-call"}
            parts[1:3] = [call]
            if mode == "native-google-multi-lite":
                parts.insert(2, {**call, "functionCall": {**call["functionCall"], "id": "google-tool-two"}})
        content = {"role": "model", "parts": parts}
        metadata = {"responseId": "reused-native-id", "modelVersion": "native-serving-version",
                    "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "thoughtsTokenCount": 4, "totalTokenCount": 9}}
        chunks = [{"candidates": [{"index": 0, "content": {"role": "model", "parts": parts[:2]}}]},
                  {"candidates": [{"index": 0, "content": {"role": "model", "parts": parts[2:]}, "finishReason": "STOP"}]}, metadata]
        if mode == "native-google-stall":
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            self.protocol_version = "HTTP/1.1"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            data = f"data: {json.dumps(chunks[0])}\n\n".encode()
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            self.wfile.flush()
            Path(trace_path).with_name("gateway-streaming").touch()
            try:
                disconnected = self.connection.recv(1) == b""
            except OSError:
                disconnected = True
            trace["gatewayDisconnected"] = disconnected
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            Path(trace_path).with_name("gateway-disconnected").touch()
            self.close_connection = True
            return
        trace.setdefault("nativeResponses", []).append({"candidates": [{"index": 0, "content": content, "finishReason": "STOP"}], **metadata})
        trace.setdefault("nativeChunks", []).append(chunks)
        Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
        data = "".join(f"data: {json.dumps(chunk)}\n\n" for chunk in chunks).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def native_ollama(self, body):
        assert self.path == "/v1/responses"
        trace["gatewayCredentialMatched"] = self.headers.get("Authorization") == "Bearer CAIDEX_GATEWAY_PROVIDER_TEST_KEY"
        assert trace["gatewayCredentialMatched"]
        trace.setdefault("nativeRequests", []).append(body)
        trace.setdefault("liteHeaders", []).append(self.headers.get("x-openai-internal-codex-responses-lite"))
        identity = f"native-{trace['requests']}"
        thinking = {"type": "reasoning", "id": f"rs_{identity}", "status": "completed", "encrypted_content": "Native fixture thinking", "summary": [{"type": "summary_text", "text": "Native fixture thinking"}], "future": {"n": 18446744073709551616}}
        if mode != "native-ollama-discovery" and trace["requests"] == 1:
            namespace = next(tool for tool in body["tools"] if tool["type"] == "namespace" and tool["name"] == "functions")
            tool = next(tool for tool in namespace["tools"] if tool["type"] == "function" and tool["name"] == "exec")
            command = {"cmd": "echo CAIDEX_NATIVE_CODE_MODE > caidex-native-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex native fixture marker only", "yield_time_ms": 1000}
            script = "const result = await tools.exec_command(" + json.dumps(command) + "); text(result); text('CAIDEX_NATIVE_CODE_MODE');"
            item = {"type": "function_call", "id": f"fc_{identity}", "status": "completed", "namespace": namespace["name"], "name": tool["name"], "call_id": "ollama-code-mode-one", "arguments": " { \"input\" : " + json.dumps(script) + " } "}
        elif trace["requests"] == 1:
            item = {"type": "tool_search_call", "id": f"ts_{identity}", "status": "completed", "execution": "client", "call_id": "native-discovery-search", "arguments": {"query": "fixture echo", "limit": 1}}
        elif mode == "native-ollama-discovery" and trace["requests"] == 2:
            loaded = [tool for result in body["input"] if result.get("type") == "tool_search_output" for tool in result["tools"]]
            namespace = next(tool for tool in loaded if tool.get("type") == "namespace" and tool["name"] == "mcp__fixture")
            member = next(tool for tool in namespace["tools"] if tool["name"] == "echo")
            item = {"type": "function_call", "id": f"fc_{identity}", "status": "completed", "namespace": namespace["name"], "name": member["name"], "call_id": "native-discovery-echo", "arguments": " { } "}
        else:
            item = {"type": "message", "id": f"msg_{identity}", "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": "CAIdex local fixture complete"}]}
        native = {"id": identity, "object": "response", "model": "native-fixture", "status": "completed", "output": [thinking, item], "future": {"n": 18446744073709551616}, "usage": {"input_tokens": 2, "output_tokens": 3, "total_tokens": 5}}
        if mode == "native-ollama-multi-lite":
            native["output"].append({**item, "id": "fc_ollama_two", "call_id": "ollama-code-mode-two"})
        events = [event("response.created", response={"id": identity, "status": "in_progress", "output": []}), event("response.output_item.added", output_index=0, item={**thinking, "summary": [], "encrypted_content": ""}), event("response.reasoning_summary_text.delta", output_index=0, item_id=thinking["id"], summary_index=0, delta="Native fixture thinking")]
        if item["type"] == "message":
            events.extend([event("response.output_item.added", output_index=1, item={**item, "content": []}), event("response.content_part.added", output_index=1, item_id=item["id"], content_index=0, part={"type": "output_text", "text": ""}), event("response.output_text.delta", output_index=1, item_id=item["id"], content_index=0, delta=item["content"][0]["text"])])
        if mode == "native-ollama-stall-lite":
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            self.protocol_version = "HTTP/1.1"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            data = "".join(f"event: {item['type']}\ndata: {json.dumps(item)}\n\n" for item in events).encode()
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            self.wfile.flush()
            Path(trace_path).with_name("gateway-streaming").touch()
            try:
                disconnected = self.connection.recv(1) == b""
            except OSError:
                disconnected = True
            trace["gatewayDisconnected"] = disconnected
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            Path(trace_path).with_name("gateway-disconnected").touch()
            self.close_connection = True
            return
        events.append(event("response.completed", response=native))
        trace.setdefault("nativeResponses", []).append(native)
        Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
        data = "".join(f"event: {item['type']}\ndata: {json.dumps(item)}\n\n" for item in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def native_deepseek(self, body):
        assert self.path == "/v1/responses"
        trace["gatewayCredentialMatched"] = self.headers.get("Authorization") == "Bearer CAIDEX_GATEWAY_PROVIDER_TEST_KEY"
        assert trace["gatewayCredentialMatched"]
        trace.setdefault("nativeRequests", []).append(body)
        trace.setdefault("liteHeaders", []).append(self.headers.get("x-openai-internal-codex-responses-lite"))
        identity = f"native-{trace['requests']}"
        thinking = {"type": "reasoning", "id": f"rs_{identity}", "status": "completed", "summary": [], "content": [{"type": "reasoning_text", "text": "Native DeepSeek fixture thinking"}], "future": {"n": 18446744073709551616}}
        if trace["requests"] == 1 and "stall" not in mode:
            if mode.endswith("-lite"):
                tool = next(tool for tool in body["tools"] if tool.get("parameters", {}).get("properties", {}).get("input", {}).get("type") == "string")
                command = {"cmd": "echo CAIDEX_NATIVE_DEEPSEEK > caidex-native-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex DeepSeek fixture marker only", "yield_time_ms": 1000}
                script = "const result = await tools.exec_command(" + json.dumps(command) + "); text(result);"
                arguments = " { \"input\" : " + json.dumps(script) + " } "
            else:
                tool = next(tool for tool in body["tools"] if "cmd" in tool.get("parameters", {}).get("properties", {}))
                arguments = json.dumps({"cmd": "echo CAIDEX_NATIVE_DEEPSEEK > caidex-native-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex DeepSeek fixture marker only", "yield_time_ms": 1000})
            item = {"type": "function_call", "id": f"fc_{identity}", "status": "completed", "name": tool["name"], "call_id": "deepseek-tool-one", "arguments": arguments}
        else:
            item = {"type": "message", "id": f"msg_{identity}", "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": "CAIdex local fixture complete"}]}
        native = {"id": identity, "object": "response", "model": "native-fixture", "status": "completed", "output": [thinking, item], "future": {"n": 18446744073709551616}, "usage": {"input_tokens": 2, "output_tokens": 3, "total_tokens": 5}}
        if mode == "native-deepseek-multi-lite":
            native["output"].append({**item, "id": "fc_deepseek_two", "call_id": "deepseek-tool-two"})
        events = [event("response.created", response={"id": identity, "status": "in_progress", "output": []})]
        if "stall" in mode:
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
                disconnected = True
            trace["gatewayDisconnected"] = disconnected
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            Path(trace_path).with_name("gateway-disconnected").touch()
            self.close_connection = True
            return
        events.extend([
            event("response.output_item.added", output_index=0, item={**thinking, "status": "in_progress", "content": []}),
            event("response.content_part.added", output_index=0, item_id=thinking["id"], content_index=0, part={"type": "reasoning_text", "text": ""}),
            event("response.reasoning_text.delta", output_index=0, item_id=thinking["id"], content_index=0, delta=thinking["content"][0]["text"]),
            event("response.reasoning_text.done", output_index=0, item_id=thinking["id"], content_index=0, text=thinking["content"][0]["text"]),
            event("response.content_part.done", output_index=0, item_id=thinking["id"], content_index=0, part=thinking["content"][0]),
            event("response.output_item.done", output_index=0, item=thinking),
        ])
        for index, output in enumerate(native["output"][1:], 1):
            if output["type"] == "function_call":
                events.extend([
                    event("response.output_item.added", output_index=index, item={**output, "status": "in_progress", "arguments": ""}),
                    event("response.function_call_arguments.delta", output_index=index, item_id=output["id"], delta=output["arguments"]),
                    event("response.function_call_arguments.done", output_index=index, item_id=output["id"], arguments=output["arguments"]),
                ])
            else:
                events.extend([
                    event("response.output_item.added", output_index=index, item={**output, "status": "in_progress", "content": []}),
                    event("response.content_part.added", output_index=index, item_id=output["id"], content_index=0, part={"type": "output_text", "text": ""}),
                    event("response.output_text.delta", output_index=index, item_id=output["id"], content_index=0, delta=output["content"][0]["text"]),
                    event("response.output_text.done", output_index=index, item_id=output["id"], content_index=0, text=output["content"][0]["text"]),
                    event("response.content_part.done", output_index=index, item_id=output["id"], content_index=0, part=output["content"][0]),
                ])
            events.append(event("response.output_item.done", output_index=index, item=output))
        events.append(event("response.completed", response=native))
        for sequence, output in enumerate(events):
            output["sequence_number"] = sequence
        trace.setdefault("nativeResponses", []).append(native)
        trace.setdefault("nativeChunks", []).append(events)
        Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
        data = "".join(f"event: {item['type']}\ndata: {json.dumps(item)}\n\n" for item in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def native_qwen(self, body):
        assert self.path == "/compatible-mode/v1/responses"
        trace["gatewayCredentialMatched"] = self.headers.get("Authorization") == "Bearer CAIDEX_GATEWAY_PROVIDER_TEST_KEY"
        assert trace["gatewayCredentialMatched"]
        trace.setdefault("nativeRequests", []).append(body)
        trace.setdefault("liteHeaders", []).append(self.headers.get("x-openai-internal-codex-responses-lite"))
        identity = f"native-{trace['requests']}"
        thinking = {"type": "reasoning", "id": f"rs_{identity}", "status": "completed", "summary": [{"type": "summary_text", "text": "Native Qwen fixture "}, {"type": "summary_text", "text": "thinking", "future": "retain"}], "future": {"n": 18446744073709551616}}
        if trace["requests"] == 1:
            if mode.endswith("-lite"):
                tool = next(tool for tool in body["tools"] if tool.get("parameters", {}).get("properties", {}).get("input", {}).get("type") == "string")
                command = {"cmd": "echo CAIDEX_NATIVE_QWEN > caidex-native-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex Qwen fixture marker only", "yield_time_ms": 1000}
                script = "const result = await tools.exec_command(" + json.dumps(command) + "); text(result);"
                arguments = " { \"input\" : " + json.dumps(script) + " } "
            else:
                tool = next(tool for tool in body["tools"] if "cmd" in tool.get("parameters", {}).get("properties", {}))
                arguments = json.dumps({"cmd": "echo CAIDEX_NATIVE_QWEN > caidex-native-marker.txt", "sandbox_permissions": "require_escalated", "justification": "Isolated CAIdex Qwen fixture marker only", "yield_time_ms": 1000})
            item = {"type": "function_call", "id": f"fc_{identity}", "status": "completed", "name": tool["name"], "call_id": "qwen-tool-one", "arguments": arguments}
        else:
            item = {"type": "message", "id": f"msg_{identity}", "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": "CAIdex local fixture complete"}]}
        native = {"id": identity, "object": "response", "model": "native-fixture", "status": "completed", "output": [thinking, item], "future": {"n": 18446744073709551616}, "usage": {"input_tokens": 2, "output_tokens": 3, "total_tokens": 5}}
        if mode == "native-qwen-multi-lite":
            native["output"].append({**item, "id": "fc_qwen_two", "call_id": "qwen-tool-two"})
        events = [event("response.created", response={"id": identity, "status": "in_progress", "output": []})]
        events.extend([
            event("response.output_item.added", output_index=0, item={**thinking, "status": "in_progress", "summary": []}),
            event("response.reasoning_text.delta", output_index=0, item_id=thinking["id"], delta="Native Qwen fixture thinking"),
            event("response.reasoning_text.done", output_index=0, item_id=thinking["id"], text="Native Qwen fixture thinking"),
            event("response.output_item.done", output_index=0, item=thinking),
        ])
        for index, output in enumerate(native["output"][1:], 1):
            if output["type"] == "function_call":
                events.extend([
                    event("response.output_item.added", output_index=index, item={**output, "status": "in_progress", "arguments": ""}),
                    event("response.function_call_arguments.delta", output_index=index, item_id=output["id"], delta=output["arguments"]),
                    event("response.function_call_arguments.done", output_index=index, item_id=output["id"], arguments=output["arguments"]),
                ])
            else:
                events.extend([
                    event("response.output_item.added", output_index=index, item={**output, "status": "in_progress", "content": []}),
                    event("response.content_part.added", output_index=index, item_id=output["id"], content_index=0, part={"type": "output_text", "text": ""}),
                    event("response.output_text.delta", output_index=index, item_id=output["id"], content_index=0, delta=output["content"][0]["text"]),
                    event("response.output_text.done", output_index=index, item_id=output["id"], content_index=0, text=output["content"][0]["text"]),
                    event("response.content_part.done", output_index=index, item_id=output["id"], content_index=0, part=output["content"][0]),
                ])
            events.append(event("response.output_item.done", output_index=index, item=output))
        events.append(event("response.completed", response=native))
        for sequence, output in enumerate(events):
            output["sequence_number"] = sequence
        if "stall" in mode:
            events.pop()
            trace.setdefault("nativeChunks", []).append(events)
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            self.protocol_version = "HTTP/1.1"
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            data = "".join(f"event: {item['type']}\ndata: {json.dumps(item)}\n\n" for item in events).encode()
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            self.wfile.flush()
            Path(trace_path).with_name("gateway-streaming").touch()
            try:
                disconnected = self.connection.recv(1) == b""
            except OSError:
                disconnected = True
            trace["gatewayDisconnected"] = disconnected
            Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
            Path(trace_path).with_name("gateway-disconnected").touch()
            self.close_connection = True
            return
        trace.setdefault("nativeResponses", []).append(native)
        trace.setdefault("nativeChunks", []).append(events)
        Path(trace_path).write_text(json.dumps(trace), encoding="utf-8")
        data = "".join(f"event: {item['type']}\ndata: {json.dumps(item)}\n\n" for item in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        trace["requests"] += 1
        trace["authorizationSeen"] |= "Authorization" in self.headers
        if mode.startswith("native-qwen-"):
            self.native_qwen(body)
            return
        if mode.startswith("native-deepseek-"):
            self.native_deepseek(body)
            return
        if mode in ["native-ollama-discovery", "native-ollama-tools-lite", "native-ollama-multi-lite", "native-ollama-stall-lite"]:
            self.native_ollama(body)
            return
        if mode.startswith("native-google-"):
            self.native_google(body)
            return
        if mode.startswith("native-anthropic-"):
            self.native_anthropic(body)
            return
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
