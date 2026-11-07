#!/usr/bin/env python3
"""A local stand-in for every server the browser might contact, for the network test.

The network test (network_test.py, run-network.sh) runs the browser in a network
namespace with no way out. dnsstub.py --answer points every host name at 127.0.0.1;
this server accepts the connections there and writes down every request, so the test
also sees what the browser does after a server answers, not only the names it looks up
first:

  - port 443: TLS for any name. A certificate for the name the browser asks for (SNI)
    is made on the fly, signed by a test certificate authority that only the test
    profile trusts (written to --ca, installed through an enterprise policy by
    network_test.py). Only HTTP/1.1 is offered.
  - port 80: plain HTTP.
  - DNS over HTTPS (any path ending in /dns-query): a real answer, every name pointing
    here again, and the name is logged; so lookups over DoH are counted too.
  - Remote Settings (paths under /v1/): just enough of the protocol that the browser
    goes on as it would with Mozilla's server: the server info names the attachments
    host, the change list names collections, and a collection's changeset carries a
    signature whose certificate chain lives on the signature host. Signatures do not
    verify, so nothing is ever stored as if it came from Mozilla.
  - anything else: an empty 200 reply.

Log: one JSON object per request (--log), with the time, port, TLS server name, Host,
method, path and, for DoH, the name looked up. Nothing leaves the machine.

Usage: sink.py --log FILE --ca DIR [--listen 127.0.0.1] [--http-port 80] [--https-port 443]
"""

import argparse
import base64
import json
import os
import socket
import ssl
import struct
import subprocess
import sys
import threading
import time
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dnsstub import TYPES, answer_a, parse_question  # noqa: E402

RS_ATTACHMENTS = "https://firefox-settings-attachments.cdn.mozilla.net/"
RS_SIGNATURE_CHAIN = ("https://content-signature-2.cdn.mozilla.net/chains/"
                      "remote-settings.content-signature.mozilla.org-whisper-test.chain")
# Collections Firefox 159 keeps in Remote Settings (services/settings/dumps and the
# clients created in browser/ and toolkit/); listing them in the change list makes the
# browser sync each one it uses.
RS_COLLECTIONS = [
    ("security-state", "cert-revocations"), ("security-state", "intermediates"),
    ("security-state", "onecrl"), ("blocklists", "addons-bloomfilters"),
    ("blocklists", "gfx"), ("main", "query-stripping"),
    ("main", "anti-tracking-url-decoration"), ("main", "url-classifier-skip-urls"),
    ("main", "search-config-v2"), ("main", "search-config-icons"),
    ("main", "search-config-overrides-v2"), ("main", "top-sites"),
    ("main", "password-recipes"), ("main", "password-rules"),
    ("main", "remote-permissions"), ("main", "hijack-blocklists"),
    ("main", "language-dictionaries"), ("main", "translations-models"),
    ("main", "translations-wasm"), ("main", "doh-config"), ("main", "doh-providers"),
    ("main", "fingerprinting-protection-overrides"), ("main", "tracking-protection-lists"),
    ("main", "partitioning-exempt-urls"), ("main", "third-party-cookie-blocking-exempt-urls"),
    ("main", "cookie-banner-rules-list"), ("main", "bounce-tracking-protection-exceptions"),
    ("main", "websites-with-shared-credential-backends"), ("main", "sites-classification"),
    ("main", "moz-essential-domain-fallbacks"), ("main", "newtab-wallpapers-v2"),
    ("main", "url-parser-default-unknown-schemes-interventions"),
]


class CertMaker:
    """A throw-away CA, and a certificate per host name signed by it."""

    def __init__(self, folder):
        self.folder = folder
        os.makedirs(folder, exist_ok=True)
        self.ca_key = os.path.join(folder, "ca.key")
        self.ca_pem = os.path.join(folder, "ca.pem")
        self.lock = threading.Lock()
        self.contexts = {}
        self._run(["openssl", "req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1",
                   "-nodes", "-keyout", self.ca_key, "-out", self.ca_pem, "-days", "3",
                   "-subj", "/CN=Whisper network test CA (local, throw-away)",
                   "-addext", "basicConstraints=critical,CA:TRUE",
                   "-addext", "keyUsage=critical,keyCertSign,cRLSign"])

    @staticmethod
    def _run(cmd):
        subprocess.run(cmd, check=True, capture_output=True)

    def context(self, name):
        name = (name or "localhost").lower()
        with self.lock:
            if name in self.contexts:
                return self.contexts[name]
            safe = "".join(c if c.isalnum() or c in ".-" else "_" for c in name)[:200]
            key = os.path.join(self.folder, f"{safe}.key")
            csr = os.path.join(self.folder, f"{safe}.csr")
            pem = os.path.join(self.folder, f"{safe}.pem")
            ext = os.path.join(self.folder, f"{safe}.ext")
            with open(ext, "w") as f:
                f.write(f"subjectAltName=DNS:{name}\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n")
            self._run(["openssl", "req", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes",
                       "-keyout", key, "-out", csr, "-subj", f"/CN={name}"])
            self._run(["openssl", "x509", "-req", "-in", csr, "-CA", self.ca_pem, "-CAkey", self.ca_key,
                       "-CAcreateserial", "-out", pem, "-days", "3", "-extfile", ext])
            ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            ctx.load_cert_chain(pem, key)
            ctx.set_alpn_protocols(["http/1.1"])
            self.contexts[name] = ctx
            return ctx


class Log:
    def __init__(self, path):
        self.f = open(path, "a", buffering=1)
        self.lock = threading.Lock()

    def write(self, **entry):
        entry["t"] = round(time.time(), 3)
        with self.lock:
            self.f.write(json.dumps(entry, sort_keys=True) + "\n")


def read_request(conn):
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = conn.recv(65536)
        if not chunk:
            return None
        data += chunk
        if len(data) > 1 << 20:
            return None
    head, body = data.split(b"\r\n\r\n", 1)
    lines = head.decode("latin-1").split("\r\n")
    try:
        method, target, _version = lines[0].split(" ", 2)
    except ValueError:
        return None
    headers = {}
    for line in lines[1:]:
        if ":" in line:
            k, v = line.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    length = int(headers.get("content-length", "0") or 0)
    while len(body) < min(length, 1 << 20):
        chunk = conn.recv(65536)
        if not chunk:
            break
        body += chunk
    return method, target, headers, body


def doh_answer(query):
    """A DNS answer for a DoH query: every A record points back here."""
    name, qtype, end = parse_question(query)
    return name, TYPES.get(qtype, qtype), answer_a(query, end, qtype, "127.0.0.1")


def remote_settings(path, query):
    now = int(time.time() * 1000)
    if path.rstrip("/").endswith("/v1"):
        return {"project_name": "Remote Settings (Whisper network test)",
                "capabilities": {"attachments": {"base_url": RS_ATTACHMENTS}}}
    if "/buckets/monitor/collections/changes/" in path:
        return {"metadata": {}, "timestamp": now,
                "changes": [{"id": f"{b}-{c}", "bucket": b, "collection": c, "last_modified": now}
                            for b, c in RS_COLLECTIONS]}
    if "/changeset" in path:
        parts = path.split("/")
        try:
            bucket = parts[parts.index("buckets") + 1]
            collection = parts[parts.index("collections") + 1]
        except (ValueError, IndexError):
            bucket, collection = "main", "unknown"
        return {"metadata": {"id": collection, "bucket": bucket, "last_modified": now,
                             "signature": {"x5u": RS_SIGNATURE_CHAIN, "mode": "p384ecdsa",
                                           "signature": "d2hpc3Blci10ZXN0"}},
                "timestamp": now,
                "changes": [{"id": "whisper-network-test", "last_modified": now,
                             "attachment": {"location": f"{bucket}/{collection}/whisper-network-test.bin",
                                            "filename": "whisper-network-test.bin", "size": 4,
                                            "hash": "0" * 64, "mimetype": "application/octet-stream"}}]}
    return {"data": []}


def respond(conn, status, body=b"", content_type="text/plain"):
    head = (f"HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {len(body)}\r\n"
            "Cache-Control: no-store\r\nConnection: close\r\n\r\n").encode()
    conn.sendall(head + body)


def handle(conn, port, sni, log):
    req = read_request(conn)
    if not req:
        return
    method, target, headers, body = req
    url = urllib.parse.urlsplit(target)
    host = headers.get("host", "").split(":")[0].lower()
    entry = {"port": port, "sni": sni, "host": host, "method": method, "path": url.path}
    if url.path.endswith("/dns-query"):
        query = body
        if method == "GET":
            q = urllib.parse.parse_qs(url.query).get("dns", [""])[0]
            query = base64.urlsafe_b64decode(q + "=" * (-len(q) % 4))
        try:
            name, qtype, reply = doh_answer(query)
            entry.update(doh=name, doh_type=qtype)
            log.write(**entry)
            respond(conn, "200 OK", reply, "application/dns-message")
        except (ValueError, struct.error, IndexError):
            log.write(**entry)
            respond(conn, "400 Bad Request")
        return
    log.write(**entry)
    if headers.get("upgrade", "").lower() == "websocket":
        respond(conn, "404 Not Found")
    elif url.path.startswith("/v1"):
        respond(conn, "200 OK", json.dumps(remote_settings(url.path, url.query)).encode(), "application/json")
    elif host.startswith("content-signature"):
        respond(conn, "200 OK", b"-----BEGIN CERTIFICATE-----\nd2hpc3Blcg==\n-----END CERTIFICATE-----\n",
                "application/x-pem-file")
    else:
        respond(conn, "200 OK")


def serve(addr, port, tls, certs, log):
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind((addr, port))
    srv.listen(64)
    base = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER) if tls else None
    if base:
        base.load_cert_chain(certs.ca_pem, certs.ca_key)  # replaced per name below

        def pick(sslsock, name, _ctx):
            sslsock.context = certs.context(name)
            sslsock.whisper_sni = name
        base.sni_callback = pick

    def one(raw, peer):
        try:
            raw.settimeout(30)
            if base:
                conn = base.wrap_socket(raw, server_side=True)
                sni = getattr(conn, "whisper_sni", None)
            else:
                conn, sni = raw, None
            handle(conn, port, sni, log)
        except (OSError, ssl.SSLError) as e:
            log.write(port=port, error=type(e).__name__, detail=str(e)[:200])
        finally:
            try:
                raw.close()
            except OSError:
                pass

    while True:
        raw, peer = srv.accept()
        threading.Thread(target=one, args=(raw, peer), daemon=True).start()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--log", required=True)
    ap.add_argument("--ca", required=True, help="folder for the test CA (ca.pem is what the profile trusts)")
    ap.add_argument("--listen", default="127.0.0.1")
    ap.add_argument("--http-port", type=int, default=80)
    ap.add_argument("--https-port", type=int, default=443)
    a = ap.parse_args()
    certs = CertMaker(a.ca)
    log = Log(a.log)
    threads = [threading.Thread(target=serve, args=(a.listen, a.http_port, False, certs, log), daemon=True),
               threading.Thread(target=serve, args=(a.listen, a.https_port, True, certs, log), daemon=True)]
    for t in threads:
        t.start()
    print(f"sink: listening on {a.listen}:{a.http_port} and :{a.https_port}, CA {certs.ca_pem}", flush=True)
    for t in threads:
        t.join()


if __name__ == "__main__":
    main()
