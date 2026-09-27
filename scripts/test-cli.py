#!/usr/bin/env python3
"""Exercise a dedicated running app: test-cli.py --socket PATH [--cli BINARY].

Use a disposable HOME/collections directory; this test changes app preferences.
Only Python's standard library is required. All HTTP traffic stays on loopback.
"""
import argparse
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--socket", required=True)
parser.add_argument("--cli", default="target/debug/request-eagle-cli")
args = parser.parse_args()


def call(command, *, succeeds=True, **fields):
    result = subprocess.run(
        [args.cli, "--socket", args.socket, "call", "-"],
        input=json.dumps({"command": command, **fields}),
        text=True, capture_output=True, timeout=10,
    )
    reply = json.loads(result.stdout)
    assert reply["ok"] == succeeds, (command, reply)
    assert result.returncode == (0 if succeeds else 1), result
    return reply.get("result", reply.get("error"))


# A local socket alone is not authorization, even for harmless reads.
for token in (None, "0" * 64):
    environment = dict(os.environ)
    environment.pop("REQUEST_EAGLE_CLI_TOKEN", None)
    if token is not None:
        environment["REQUEST_EAGLE_CLI_TOKEN"] = token
    output = subprocess.run([args.cli, "--socket", args.socket, "call", '{"command":"tabs.new"}'],
                            env=environment, capture_output=True, text=True, timeout=10)
    assert output.returncode == 1, output
    assert json.loads(output.stdout)["error"]["code"] == "unauthorized", output.stdout
assert len(call("tabs.list")) == 1

body = bytes(range(256)) * 2048
received = []


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        received.append(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
        self.send_response(201)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Set-Cookie", "first=one")
        self.send_header("Set-Cookie", "second=two")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
collection = call("collections.create")["path"]
try:
    call("settings.request", timeout_ms=3000)
    call("settings.proxy", mode="disabled")
    Path(collection, "environment.toml").write_text(f'base_url = "http://127.0.0.1:{server.server_port}"\n')
    folder = call("folders.create", parent=collection)["path"]
    folder = call("entries.rename", path=folder, name="Agent workflow")["path"]
    tab = call("tabs.new")["tab"]
    request = {"method": "POST", "url": "{{base_url}}/echo", "body": '{"hello":"agent"}',
               "post_response": 'pm.test("created", () => pm.expect(pm.response.code).to.eql(201)); console.log("CLI response test");'}
    call("drafts.set", tab=tab, request=request)
    call("tabs.close", tab=tab, succeeds=False)
    saved = call("drafts.save", tab=tab, parent=folder, name="Echo")
    assert call("drafts.get", tab=tab)["dirty"] is False
    assert call("requests.get", path=saved["path"])["request"]["url"] == request["url"]
    call("requests.send", tab=tab, succeeds=False)
    call("requests.send", tab=tab, trust_scripts=True)
    for _ in range(100):
        response = call("responses.get", tab=tab, limit=32768)
        if not response["loading"]:
            break
        time.sleep(.05)
    assert response.get("status") == 201, response
    assert response["failed"] is False
    assert received == [b'{"hello":"agent"}'], received
    assert len(response["cookies"]) == 2
    assert response["scripts"][0]["tests"][0]["error"] is None
    chunks = [base64.b64decode(response["body"]["data"])]
    while response["body"]["next_offset"] is not None:
        response = call("responses.get", tab=tab, offset=response["body"]["next_offset"], limit=32768)
        chunks.append(base64.b64decode(response["body"]["data"]))
    assert b"".join(chunks) == body
    request["url"] = "{{base_url}}/unsaved"
    call("drafts.set", tab=tab, request=request)
    call("requests.open", path=saved["path"])
    assert call("drafts.get", tab=tab)["request"]["url"] == request["url"]
    moved = call("entries.move", path=saved["path"], target=collection, placement="inside")["path"]
    call("drafts.save", tab=tab)
    assert call("requests.get", path=moved)["request"]["url"] == request["url"]
    call("entries.delete", path=moved, confirm=False, succeeds=False)
    call("tabs.close", tab=tab)
    for mode in ("light", "dark"):
        for font_size in (12, 16, 24):
            call("settings.appearance", mode=mode, interface_font_size=font_size)
            call("ui.show", page="general")
            settings = call("settings.get")
            assert settings["appearance"]["interface_font_size"] == font_size
            assert settings["request"]["timeout_ms"] == 3000
    call("settings.appearance", mode="dark", interface_font_size=16)
    assert call("keybindings.list")
    assert call("themes.list")
    assert call("fonts.list")
    call("ui.sidebar", visible=False)
    call("ui.sidebar", visible=True)
    print("CLI end-to-end passed: live drafts, persistence, movement, trust, variables, scripts, binary response paging, cookies, preferences, appearance, catalogs.")
finally:
    call("entries.delete", path=collection, confirm=True)
    server.shutdown()
