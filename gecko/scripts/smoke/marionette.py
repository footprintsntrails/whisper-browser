"""A minimal Marionette client: enough to drive Whisper's smoke test.

Marionette is the remote protocol built into Firefox (and Whisper); it is only active
when the browser is started with --marionette. Messages are length-prefixed JSON
("<length>:<json>"): commands are [0, id, name, params], replies [1, id, error, result].
"""

import base64
import json
import socket
import time


class MarionetteError(Exception):
    pass


class Marionette:
    def __init__(self, host="127.0.0.1", port=2828, timeout=90, alive=lambda: True):
        deadline = time.monotonic() + timeout
        last = None
        while time.monotonic() < deadline:
            if not alive():
                raise MarionetteError("the browser exited before Marionette was ready")
            try:
                self.sock = socket.create_connection((host, port), timeout=5)
                self.sock.settimeout(120)
                hello = self._recv()
                if hello.get("applicationType") != "gecko":
                    raise MarionetteError(f"unexpected greeting {hello}")
                break
            except (OSError, ValueError) as e:
                last = e
                time.sleep(0.5)
        else:
            raise MarionetteError(f"no Marionette server on {host}:{port}: {last}")
        self._id = 0

    def _recv(self):
        size = b""
        while not size.endswith(b":"):
            chunk = self.sock.recv(1)
            if not chunk:
                raise MarionetteError("connection closed")
            size += chunk
        length = int(size[:-1])
        data = b""
        while len(data) < length:
            chunk = self.sock.recv(length - len(data))
            if not chunk:
                raise MarionetteError("connection closed")
            data += chunk
        return json.loads(data)

    def command(self, name, params=None):
        self._id += 1
        body = json.dumps([0, self._id, name, params or {}]).encode()
        self.sock.sendall(str(len(body)).encode() + b":" + body)
        while True:
            reply = self._recv()
            if isinstance(reply, list) and reply[0] == 1 and reply[1] == self._id:
                if reply[2]:
                    err = reply[2]
                    raise MarionetteError(f"{name}: {err.get('error')}: {err.get('message')}")
                return reply[3]

    def start_session(self):
        session = self.command("WebDriver:NewSession", {"strictFileInteractability": True})
        self.command("Marionette:SetContext", {"value": "chrome"})
        self.command("WebDriver:SetTimeouts", {"script": 90000})
        return session

    def run(self, body, *args):
        """Runs `body` as an async function in the browser window (chrome) and returns
        its JSON result. Exceptions in the script come back as MarionetteError."""
        script = (
            "const resolve = arguments[arguments.length - 1];\n"
            "const args = Array.from(arguments).slice(0, -1);\n"
            "(async (...args) => {\n" + body + "\n})(...args).then(\n"
            "  value => resolve({ ok: true, value }),\n"
            "  error => resolve({ ok: false, error: String(error) + '\\n' + (error && error.stack || '') })\n"
            ");"
        )
        result = self.command("WebDriver:ExecuteAsyncScript", {"script": script, "args": list(args)})
        value = result.get("value") if isinstance(result, dict) else result
        if not value or not value.get("ok"):
            raise MarionetteError((value or {}).get("error", f"no result: {result}"))
        return value.get("value")

    def screenshot(self, path):
        data = self.command("WebDriver:TakeScreenshot", {"full": False, "hash": False, "scroll": False})
        with open(path, "wb") as f:
            f.write(base64.b64decode(data["value"] if isinstance(data, dict) else data))

    def quit(self):
        return self.command("Marionette:Quit", {"flags": ["eAttemptQuit"]})
