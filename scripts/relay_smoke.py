#!/usr/bin/env python3
"""Smoke-test a deployed kaku-relay end to end, the way a Mac and a page use it.

    python3 scripts/relay_smoke.py                    # wss://r.kaku.fun, page origin https://t.kaku.fun
    python3 scripts/relay_smoke.py ws://127.0.0.1:8080 --origin ''   # a local relay without a page origin

Registers a throwaway Mac under a fresh random identity (it disappears when
the script ends; no real Mac, device or pairing is touched), then checks
health, status, the page redirect, a browser joining, data both ways with its
round trip, the close codes the page relies on (4403, 4409, 4429) passing
through, and a foreign Origin refused. When the relay asks for an access code
it uses KAKU_RELAY_ACCESS, else this Mac's saved code
(~/.local/share/kaku/remote-access); the code is never printed.

Run it after every `fly deploy`. Exit status is the number of failed checks.
Standard library only.
"""

import argparse
import base64
import hashlib
import json
import os
import socket
import ssl
import struct
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ACCESS_FILE = os.path.expanduser("~/.local/share/kaku/remote-access")
KIND_DATA = 0


class Refused(Exception):
    def __init__(self, status, body):
        super().__init__(f"{status} {body}")
        self.status = status
        self.body = body


class Ws:
    """A minimal WebSocket client: enough for handshakes, binary and text
    messages, and reading a close code."""

    def __init__(self, url, headers=None, timeout=10):
        u = urllib.parse.urlsplit(url)
        tls = u.scheme == "wss"
        port = u.port or (443 if tls else 80)
        raw = socket.create_connection((u.hostname, port), timeout=timeout)
        self.sock = ssl.create_default_context().wrap_socket(raw, server_hostname=u.hostname) if tls else raw
        key = base64.b64encode(os.urandom(16)).decode()
        lines = [
            f"GET {u.path or '/'} HTTP/1.1",
            f"Host: {u.netloc}",
            "Upgrade: websocket",
            "Connection: Upgrade",
            f"Sec-WebSocket-Key: {key}",
            "Sec-WebSocket-Version: 13",
        ] + [f"{k}: {v}" for k, v in (headers or {}).items()]
        self.sock.sendall(("\r\n".join(lines) + "\r\n\r\n").encode())
        self.buf = b""
        while b"\r\n\r\n" not in self.buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise Refused(0, "connection closed during handshake")
            self.buf += chunk
        head, self.buf = self.buf.split(b"\r\n\r\n", 1)
        status = int(head.split(b" ", 2)[1])
        if status != 101:
            body = self.buf.decode(errors="replace").strip()[:120]
            raise Refused(status, body)

    def _need(self, n):
        while len(self.buf) < n:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise EOFError
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def send(self, payload, text=False):
        if isinstance(payload, str):
            payload = payload.encode()
        head = bytes([0x80 | (1 if text else 2)])
        n = len(payload)
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        mask = os.urandom(4)
        self.sock.sendall(head + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def recv(self, timeout=5):
        """('text' | 'binary', bytes), ('close', code), or None on timeout."""
        self.sock.settimeout(timeout)
        try:
            while True:
                h = self._need(2)
                op, n = h[0] & 0x0F, h[1] & 0x7F
                if n == 126:
                    n = struct.unpack(">H", self._need(2))[0]
                elif n == 127:
                    n = struct.unpack(">Q", self._need(8))[0]
                payload = self._need(n)
                if op == 1:
                    return ("text", payload)
                if op == 2:
                    return ("binary", payload)
                if op == 8:
                    return ("close", struct.unpack(">H", payload[:2])[0] if len(payload) >= 2 else 1005)
                # ping, pong: keep reading
        except (socket.timeout, TimeoutError):
            return None
        except EOFError:
            return ("close", 1006)

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass


def http_get(url, headers=None):
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None

    req = urllib.request.Request(url, headers=headers or {})
    try:
        with urllib.request.build_opener(NoRedirect).open(req, timeout=10) as r:
            return r.status, dict(r.headers), r.read()
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), e.read()


def access_code():
    code = os.environ.get("KAKU_RELAY_ACCESS", "").strip()
    if not code and os.path.exists(ACCESS_FILE):
        with open(ACCESS_FILE) as f:
            code = f.read().strip()
    return code or None


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("relay", nargs="?", default="wss://r.kaku.fun")
    ap.add_argument("--origin", default="https://t.kaku.fun", help="the relay's page origin; '' when it has none")
    args = ap.parse_args()
    relay = args.relay.rstrip("/")
    http = relay.replace("wss://", "https://", 1).replace("ws://", "http://", 1)
    origin = args.origin or None
    failures = 0

    def check(name, ok, detail=""):
        nonlocal failures
        print(f"  {'ok  ' if ok else 'FAIL'} {name}{'' if ok or not detail else f': {detail}'}")
        if not ok:
            failures += 1
        return ok

    status, _, body = http_get(f"{http}/health")
    check("health", status == 200 and body.strip() == b"ok", f"{status} {body[:60]!r}")

    secret = os.urandom(32).hex()
    mac_id = hashlib.sha256(secret.encode()).hexdigest()[:32]
    headers = {"x-kaku-secret": secret}
    try:
        mac = Ws(f"{relay}/host/{mac_id}", headers)
    except Refused as e:
        if e.status != 401:
            check("throwaway Mac registers", False, str(e))
            return failures
        code = access_code()
        if not code:
            check("throwaway Mac registers", False, "the relay wants an access code; set KAKU_RELAY_ACCESS")
            return failures
        headers["x-kaku-access"] = code
        try:
            mac = Ws(f"{relay}/host/{mac_id}", headers)
        except Refused as e2:
            check("throwaway Mac registers", False, f"{e2.status} (access code refused?)")
            return failures
    check("throwaway Mac registers", True)

    status, _, _ = http_get(f"{http}/{mac_id}/status")
    check("status says the Mac is online", status == 200, str(status))
    status, _, _ = http_get(f"{http}/{os.urandom(16).hex()}/status")
    check("status says an unknown id is offline", status == 404, str(status))
    if origin:
        status, hdrs, _ = http_get(f"{http}/{mac_id}/")
        location = hdrs.get("Location") or hdrs.get("location") or ""
        check("page address redirects to the page origin", status == 308 and location == f"{origin}/{mac_id}/", f"{status} {location}")

    browser_headers = {"Origin": origin} if origin else {}

    def join():
        browser = Ws(f"{relay}/{mac_id}/ws", browser_headers)
        while True:
            msg = mac.recv(5)
            if msg is None:
                raise RuntimeError("the Mac never heard of the browser")
            if msg[0] == "text":
                note = json.loads(msg[1])
                if note.get("t") == "open":
                    return browser, note["c"]

    try:
        browser, c = join()
        check("browser joins and the Mac hears of it", True)
    except (Refused, RuntimeError, OSError) as e:
        check("browser joins and the Mac hears of it", False, str(e))
        mac.close()
        return failures

    probe = os.urandom(32)
    browser.send(probe)
    msg = mac.recv(5)
    check(
        "browser to Mac",
        msg is not None and msg[0] == "binary" and msg[1][0] == KIND_DATA
        and struct.unpack(">I", msg[1][1:5])[0] == c and msg[1][5:] == probe,
        repr(msg)[:80],
    )
    mac.send(json.dumps({"t": "ok", "c": c}), text=True)
    rtts = []
    for _ in range(5):
        payload = os.urandom(16)
        t0 = time.monotonic()
        mac.send(bytes([KIND_DATA]) + struct.pack(">I", c) + payload)
        got = browser.recv(5)
        if got and got[0] == "binary" and got[1] == payload:
            browser.send(payload)
            back = mac.recv(5)
            if back and back[0] == "binary" and back[1][5:] == payload:
                rtts.append((time.monotonic() - t0) * 1000)
    check("Mac to browser and back, 5 times", len(rtts) == 5, f"{len(rtts)}/5")
    if rtts:
        rtts.sort()
        print(f"       round trip through the relay: median {rtts[2]:.0f} ms, max {rtts[-1]:.0f} ms")
    browser.close()

    for code in (4403, 4409, 4429):
        try:
            b, cc = join()
        except (Refused, RuntimeError, OSError) as e:
            check(f"close {code} reaches the page", False, str(e))
            continue
        mac.send(json.dumps({"t": "close", "c": cc, "code": code}), text=True)
        got = b.recv(5)
        check(f"close {code} reaches the page", got == ("close", code), repr(got))
        b.close()

    if origin:
        try:
            b = Ws(f"{relay}/{mac_id}/ws", {"Origin": "https://example.com"})
            got = b.recv(5)
            check("a foreign Origin is closed with 4426", got == ("close", 4426), repr(got))
            b.close()
        except Refused as e:
            check("a foreign Origin is refused", True, str(e))

    mac.close()
    print(f"{'all checks passed' if not failures else f'{failures} failed'} against {relay}")
    return failures


if __name__ == "__main__":
    sys.exit(main())
