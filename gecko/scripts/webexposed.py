#!/usr/bin/env python3
"""What web pages can load from a Firefox build: the chrome:// and resource:// files
that the build registers as content-accessible.

A web page can try to load chrome:// and resource:// URLs (as an image, stylesheet or
script); only packages registered with contentaccessible=yes answer, the rest fail. If
Whisper answered differently from Firefox of the same version (a file only Whisper has,
one it lacks, an image of another size), a page could tell Whisper apart. This lists
every such URL with what a page can measure about it, and compares two builds.

  webexposed.py list APP_DIR                  every exposed URL, one per line, as JSON
  webexposed.py compare WHISPER_DIR FIREFOX_DIR [--allow FILE]
      exits 1 if a page could tell them apart: a URL exposed in one build and not the
      other, or an exposed file that differs in what a page can measure (image size;
      for other files their bytes). --allow names URLs (one per line, # comments)
      whose bytes may differ as long as they are exposed in both and images keep
      their size: Whisper's own branding images.

APP_DIR is an unpacked Linux package (the folder with omni.ja and browser/omni.ja).
Standard library only.
"""

import argparse
import hashlib
import json
import os
import re
import struct
import sys
import zlib

JARS = ("omni.ja", "browser/omni.ja")


class Jar:
    """A minimal zip reader that also reads Mozilla's optimized jars (Firefox's own
    omni.ja keeps its central directory at the start, which zipfile cannot read)."""

    def __init__(self, path):
        self.data = open(path, "rb").read()
        d = self.data
        eocd = d.rfind(b"PK\x05\x06")
        if eocd < 0:
            raise ValueError(f"{path}: not a zip file")
        count, cd_size, cd_offset = struct.unpack_from("<HII", d, eocd + 10)
        self.entries = {}
        pos = cd_offset
        if d[pos:pos + 4] != b"PK\x01\x02":
            # Mozilla's optimized jars: the offsets count from after a 4-byte preamble.
            pos += 4
            self.base = 4
        else:
            self.base = 0
        for _ in range(count):
            if d[pos:pos + 4] != b"PK\x01\x02":
                raise ValueError(f"{path}: bad central directory entry")
            method, = struct.unpack_from("<H", d, pos + 10)
            csize, usize, nlen, elen, clen = struct.unpack_from("<IIHHH", d, pos + 20)
            local, = struct.unpack_from("<I", d, pos + 42)
            name = d[pos + 46:pos + 46 + nlen].decode("utf-8")
            self.entries[name] = (method, csize, usize, local + self.base)
            pos += 46 + nlen + elen + clen

    def namelist(self):
        return list(self.entries)

    def read(self, name):
        method, csize, _usize, local = self.entries[name]
        d = self.data
        nlen, elen = struct.unpack_from("<HH", d, local + 26)
        start = local + 30 + nlen + elen
        raw = d[start:start + csize]
        return raw if method == 0 else zlib.decompress(raw, -15)


def manifests(z, prefix=""):
    """(manifest path, lines) of every chrome.manifest reachable in a jar."""
    out = []
    todo = ["chrome.manifest"]
    seen = set()
    names = set(z.namelist())
    while todo:
        m = todo.pop()
        if m in seen or m not in names:
            continue
        seen.add(m)
        lines = z.read(m).decode("utf-8", "replace").splitlines()
        out.append((m, lines))
        base = os.path.dirname(m)
        for line in lines:
            parts = line.split()
            if parts[:1] == ["manifest"] and len(parts) > 1:
                todo.append(os.path.normpath(os.path.join(base, parts[1])))
    return out


def image_size(data):
    if data[:8] == b"\x89PNG\r\n\x1a\n":
        return list(struct.unpack(">II", data[16:24]))
    if data[:4] == b"\0\0\1\0":
        # An icon shows its largest image.
        count = struct.unpack_from("<H", data, 4)[0]
        return {"ico": max((data[6 + 16 * i] or 256) for i in range(count))}
    if data[:5] == b"<?xml" or b"<svg" in data[:2000]:
        head = data[:4000].decode("utf-8", "replace")
        svg = re.search(r"<svg\b[^>]*>", head, re.S)
        if svg:
            tag = svg.group(0)
            w = re.search(r'\swidth="([^"]+)"', tag)
            h = re.search(r'\sheight="([^"]+)"', tag)
            vb = re.search(r'\sviewBox="([^"]+)"', tag)
            # What a page measures: the intrinsic width and height (naturalWidth and
            # naturalHeight), and without them the aspect ratio of the viewBox.
            ratio = None
            if vb:
                parts = [float(v) for v in re.split(r"[\s,]+", vb.group(1).strip())]
                if len(parts) == 4 and parts[3]:
                    ratio = round(parts[2] / parts[3], 4)
            return {"width": w and w.group(1), "height": h and h.group(1),
                    "ratio": None if (w and h) else ratio}
    return None


def exposed(app):
    """{url: {"size": bytes, "sha256": ..., "image": dims}} of content-accessible files."""
    result = {}
    for jar in JARS:
        path = os.path.join(app, jar)
        if not os.path.isfile(path):
            continue
        z = Jar(path)
        names = z.namelist()
        for mpath, lines in manifests(z):
            base = os.path.dirname(mpath)
            for line in lines:
                parts = line.split()
                if len(parts) < 3 or parts[0] not in ("content", "resource"):
                    continue
                flags = parts[3:]
                if "contentaccessible=yes" not in flags:
                    continue
                kind, name, target = parts[0], parts[1], parts[2]
                if "://" in target and not target.startswith("jar:"):
                    continue  # points at another registration, not files in this jar
                rel = os.path.normpath(os.path.join(base, target)).rstrip("/") + "/"
                scheme = f"chrome://{name}/content/" if kind == "content" else f"resource://{name}/"
                for n in names:
                    if n.startswith(rel) and not n.endswith("/"):
                        data = z.read(n)
                        info = {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                        img = image_size(data)
                        if img:
                            info["image"] = img
                        result[scheme + n[len(rel):]] = info
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("list")
    p.add_argument("app")
    p = sub.add_parser("compare")
    p.add_argument("whisper")
    p.add_argument("firefox")
    p.add_argument("--allow")
    a = ap.parse_args()
    if a.cmd == "list":
        for url, info in sorted(exposed(a.app).items()):
            print(json.dumps({"url": url, **info}))
        return 0
    allow = set()
    if a.allow:
        for line in open(a.allow, encoding="utf-8"):
            line = line.split("#", 1)[0].strip()
            if line:
                allow.add(line)
    w, f = exposed(a.whisper), exposed(a.firefox)
    problems = []
    for url in sorted(set(w) - set(f)):
        problems.append(f"only Whisper exposes {url}")
    for url in sorted(set(f) - set(w)):
        problems.append(f"Firefox exposes {url}, Whisper does not")
    for url in sorted(set(w) & set(f)):
        a_, b_ = w[url], f[url]
        if a_.get("image") != b_.get("image"):
            problems.append(f"{url}: a page measures {a_.get('image')} in Whisper, {b_.get('image')} in Firefox")
        elif a_["sha256"] != b_["sha256"] and url not in allow:
            problems.append(f"{url}: differs from Firefox's ({a_['size']} vs {b_['size']} bytes)")
    for p in problems:
        print(f"::error::{p}")
    print(f"webexposed: {len(w)} URLs exposed in Whisper, {len(f)} in Firefox, {len(problems)} difference(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
