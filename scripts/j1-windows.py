#!/usr/bin/env python3
"""Actual Windows release/WebView and installer verification; synthetic model only."""
import argparse
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import urllib.request

SECRET = "synthetic-native-ui-j1-key"
checks = []


def check(name, condition):
    if not condition:
        raise AssertionError(name)
    checks.append(name)
    print(f"PASS {name}", flush=True)


class Model(http.server.BaseHTTPRequestHandler):
    requests = []

    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        assert self.path == "/responses"
        assert self.headers.get("Authorization") == f"Bearer {SECRET}"
        assert request["model"] == "synthetic"
        assert request["tools"] == [] and request["tool_choice"] == "none"
        assert request["store"] is False and request["stream"] is True
        assert SECRET not in json.dumps(request)
        self.requests.append(request)
        text = "Actual native Chat reply"
        slow = "cancel-test" in json.dumps(request["input"])
        frames = [
            {"type": "response.output_text.delta", "delta": "Actual native "},
            {"type": "response.output_text.delta", "delta": "Chat reply"},
            {"type": "response.completed", "response": {"id": "j1-response", "status": "completed", "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}]}},
        ]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            for index, frame in enumerate(frames):
                if index:
                    time.sleep(3 if slow else 0.2)
                self.wfile.write(f"event: {frame['type']}\ndata: {json.dumps(frame)}\n\n".encode())
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass


class Driver:
    def __init__(self, binary):
        self.session = None
        result = self.call("POST", "/session", {"capabilities": {"alwaysMatch": {"tauri:options": {"application": str(binary.resolve())}}}})
        self.session = result["sessionId"]

    def call(self, method, path, value=None):
        data = None if value is None else json.dumps(value).encode()
        req = urllib.request.Request("http://127.0.0.1:4444" + path, data=data, method=method, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=30) as response:
            output = json.load(response)
        result = output.get("value")
        if isinstance(result, dict) and result.get("error"):
            raise RuntimeError("native WebDriver command failed")
        return result

    def execute(self, source, args=None):
        return self.call("POST", f"/session/{self.session}/execute/sync", {"script": source, "args": args or []})

    def wait(self, source, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                result = self.execute(source)
                if result:
                    return result
            except Exception:
                pass
            time.sleep(0.1)
        try:
            diagnostic = self.execute("return {ready:document.readyState,title:document.title,text:document.body.innerText.slice(0,1200),textareas:document.querySelectorAll('textarea').length}")
            print("Native condition diagnostic: " + json.dumps(diagnostic, ensure_ascii=True).replace(SECRET, "[REDACTED]"), flush=True)
        except Exception:
            print("Native condition diagnostic unavailable", flush=True)
        raise TimeoutError("native UI condition did not become true")

    def click(self, css):
        self.wait(f"return !!document.querySelector({json.dumps(css)})")
        element = self.call("POST", f"/session/{self.session}/element", {"using": "css selector", "value": css})
        ident = element["element-6066-11e4-a52e-4f735466cecf"]
        self.call("POST", f"/session/{self.session}/element/{ident}/click", {})

    def fill(self, css, text):
        # React controlled inputs use the native setter before a bubbling input event.
        self.execute("const e=document.querySelector(arguments[0]); const p=e.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;Object.getOwnPropertyDescriptor(p,'value').set.call(e,arguments[1]);e.dispatchEvent(new Event('input',{bubbles:true}));", [css, text])

    def select(self, css, value):
        self.execute("const e=document.querySelector(arguments[0]);e.value=arguments[1];e.dispatchEvent(new Event('change',{bubbles:true}));", [css, value])

    def screenshot(self, path):
        encoded = self.call("GET", f"/session/{self.session}/screenshot")
        path.write_bytes(base64.b64decode(encoded))

    def close(self):
        if self.session:
            self.call("DELETE", f"/session/{self.session}")
            self.session = None


def run(args):
    output = Path(args.output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    binary = Path(args.binary).resolve()
    check("release_binary_exists", binary.is_file())
    model = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
    threading.Thread(target=model.serve_forever, daemon=True).start()
    process = subprocess.Popen([args.driver, "--native-driver", str(Path(args.native_driver).resolve())], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    driver = None
    installer_result = {}
    try:
        for _ in range(100):
            try:
                with urllib.request.urlopen("http://127.0.0.1:4444/status", timeout=1):
                    break
            except Exception:
                time.sleep(0.2)
        driver = Driver(binary)
        driver.wait("return document.querySelector('textarea') && document.body.textContent.includes('随时可以开始')")
        check("real_release_webview_started", True)
        driver.click("button[aria-label='设置']")
        driver.select("select[aria-label='主题']", "dark")
        driver.wait("return document.documentElement.dataset.theme==='dark'")
        driver.click("button[aria-label='关闭']")
        driver.screenshot(output.parent / "native-zh-dark.png")
        driver.click("button[aria-label='设置']")
        driver.select("select[aria-label='主题']", "light")
        driver.wait("return document.documentElement.dataset.theme==='light'")
        driver.select("select[aria-label='语言']", "en")
        driver.wait("return document.documentElement.lang==='en' && document.documentElement.dataset.theme==='light'")
        check("native_preferences_saved_and_rendered", True)
        driver.click("button[aria-label='Close']")
        driver.click(".setup-card button")
        driver.click(".section-heading button")
        driver.select("select[aria-label='Provider']", "custom_responses")
        driver.fill(".profile-form label:nth-of-type(2) input", "Native fixture")
        driver.fill(".profile-form label:nth-of-type(3) input", "synthetic")
        driver.fill(".profile-form label:nth-of-type(4) input", f"http://127.0.0.1:{model.server_port}/responses")
        driver.fill(".profile-form label:nth-of-type(5) input", SECRET)
        check("key_input_masked", driver.execute("return document.querySelector('.profile-form input[type=password]')!==null"))
        driver.click(".profile-form button[type=submit]")
        driver.wait("return document.body.textContent.includes('Key configured')")
        check("native_system_key_saved_without_echo", not driver.execute("return document.body.textContent.includes(arguments[0])", [SECRET]))
        driver.click("button[aria-label='Close']")
        driver.click("button[aria-label='Choose a model']")
        driver.click("[role=menuitem]")
        driver.fill("textarea", "Hello from native Windows")
        driver.click("button[aria-label='Send message']")
        driver.wait("return document.querySelector('.assistant .message-content')?.textContent==='Actual native Chat reply' && !document.querySelector('.streaming')")
        check("real_ipc_provider_sse_chat_completed", len(Model.requests) == 1)
        driver.screenshot(output.parent / "native-en-light-chat.png")
        driver.fill("textarea", "draft survives restart")
        time.sleep(0.7)
        driver.close()
        driver = Driver(binary)
        driver.wait("return document.documentElement.lang==='en' && document.querySelector('.history-item')")
        driver.click(".history-item")
        driver.wait("return document.querySelector('textarea').value==='draft survives restart' && document.body.textContent.includes('Actual native Chat reply')")
        check("actual_process_restart_preserves_chat_draft_theme_language", len(Model.requests) == 1)
        driver.fill("textarea", "cancel-test")
        driver.click("button[aria-label='Send message']")
        driver.wait("return document.querySelector('.streaming .message-content')?.textContent.includes('Actual native')")
        driver.click("button[aria-label='Stop generating']")
        driver.wait("return !document.querySelector('.streaming') && document.body.textContent.includes('Stopped')")
        check("native_stop_preserves_partial_output", len(Model.requests) == 2)
        driver.close()
        driver = Driver(binary)
        driver.wait("return document.querySelector('.history-item')")
        driver.click(".history-item")
        driver.wait("return document.body.textContent.includes('Stopped')")
        time.sleep(1)
        check("restart_never_replays_cancelled_model_post", len(Model.requests) == 2)
        driver.fill("textarea", "crash-test")
        driver.click("button[aria-label='Send message']")
        driver.wait("return document.querySelector('.streaming .message-content')?.textContent.includes('Actual native')")
        code = subprocess.run(["taskkill", "/F", "/IM", "CAIdex.exe"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20).returncode
        check("actual_process_killed_during_stream", code == 0)
        try:
            driver.close()
        except Exception:
            driver.session = None
        driver = Driver(binary)
        driver.wait("return document.querySelector('.history-item')")
        driver.click(".history-item")
        driver.wait("return document.body.textContent.includes('Output interrupted')")
        time.sleep(1)
        check("crash_restart_seals_unknown_turn_without_replay", len(Model.requests) == 3)
        driver.close()
        with tempfile.TemporaryDirectory(prefix="caidex-j1-install-") as temporary:
            install = Path(temporary) / "nsis"
            installers = list((binary.parent / "bundle" / "nsis").glob("*.exe"))
            check("real_nsis_installer_exists", len(installers) == 1)
            code = subprocess.run([str(installers[0]), "/S", f"/D={install}"], timeout=120).returncode
            check("nsis_installer_completed", code == 0)
            installed = list(install.rglob("CAIdex.exe"))
            check("nsis_installed_real_executable", len(installed) == 1)
            check("installed_binary_matches_release", hashlib.sha256(installed[0].read_bytes()).hexdigest() == hashlib.sha256(binary.read_bytes()).hexdigest())
            driver = Driver(installed[0])
            driver.wait("return document.querySelector('.history-item')")
            check("installed_application_webview_starts_with_preserved_history", True)
            driver.close()
            uninstaller = list(install.glob("*ninstall*.exe"))
            check("nsis_uninstaller_exists", len(uninstaller) == 1)
            code = subprocess.run([str(uninstaller[0]), "/S"], timeout=120).returncode
            check("nsis_uninstaller_completed", code == 0)
            for _ in range(100):
                if not installed[0].exists():
                    break
                time.sleep(0.1)
            check("nsis_uninstaller_removed_installed_executable", not installed[0].exists())
            installer_result = {"nsis_install_exit": 0, "nsis_uninstall_exit": 0, "msi": "built; interactive installation not exercised"}
        # No secret-bearing diagnostics, production endpoints, or real model requests.
        check("synthetic_requests_only", len(Model.requests) == 3)
        source = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        output.write_text(json.dumps({"source_sha": source, "passed": len(checks), "failed": 0, "checks": checks, "synthetic_model_posts": len(Model.requests), "installers": installer_result, "environment": "GitHub windows-2022 / real WebView2 / real native Broker and SQLite", "limits": ["Not Windows 11 human interaction or UAC/SmartScreen acceptance", "Unsigned installers", "No commercial model or production account traffic"]}, indent=2), encoding="utf-8")
    finally:
        if driver:
            try:
                driver.close()
            except Exception:
                pass
        process.terminate()
        process.wait(timeout=10)
        model.shutdown()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--driver", required=True)
    parser.add_argument("--native-driver", required=True)
    parser.add_argument("--output", required=True)
    run(parser.parse_args())
