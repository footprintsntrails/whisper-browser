#!/usr/bin/env python3
"""A DNS server that answers nothing and writes down every name it is asked for.

Used by the smoke test inside a network namespace with no way out: whatever host names
the browser tries to resolve on its first start end up in the log, one line per query
("<unix time> <type> <name>"), and every query gets NXDOMAIN, so nothing is contacted.

Usage: dnsstub.py --listen 127.0.0.53 [--listen 127.0.0.1] --log FILE [--port 53]
"""

import argparse
import socket
import struct
import threading
import time

TYPES = {1: "A", 2: "NS", 5: "CNAME", 6: "SOA", 12: "PTR", 15: "MX", 16: "TXT",
         28: "AAAA", 33: "SRV", 64: "SVCB", 65: "HTTPS"}


def parse_question(msg):
    labels, i = [], 12
    while i < len(msg):
        n = msg[i]
        if n == 0:
            i += 1
            break
        if n & 0xC0:
            raise ValueError("compressed name in question")
        labels.append(msg[i + 1:i + 1 + n].decode("ascii", "replace"))
        i += 1 + n
    qtype, _qclass = struct.unpack("!HH", msg[i:i + 4])
    return ".".join(labels).lower(), qtype, i + 4


def nxdomain(msg, question_end):
    ident = msg[:2]
    flags = struct.unpack("!H", msg[2:4])[0]
    # QR=1, same opcode and RD, RA=1, RCODE=3 (NXDOMAIN)
    out_flags = 0x8000 | (flags & 0x7900) | 0x0080 | 3
    return ident + struct.pack("!HHHHH", out_flags, 1, 0, 0, 0) + msg[12:question_end]


def serve(addr, port, log, lock):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind((addr, port))
    while True:
        msg, peer = sock.recvfrom(4096)
        try:
            name, qtype, end = parse_question(msg)
        except (ValueError, struct.error, IndexError):
            continue
        with lock:
            log.write(f"{time.time():.3f} {TYPES.get(qtype, qtype)} {name}\n")
            log.flush()
        sock.sendto(nxdomain(msg, end), peer)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--listen", action="append", required=True)
    ap.add_argument("--log", required=True)
    ap.add_argument("--port", type=int, default=53)
    a = ap.parse_args()
    log = open(a.log, "a", buffering=1)
    lock = threading.Lock()
    threads = [threading.Thread(target=serve, args=(addr, a.port, log, lock), daemon=True) for addr in a.listen]
    for t in threads:
        t.start()
    for t in threads:
        t.join()


if __name__ == "__main__":
    main()
