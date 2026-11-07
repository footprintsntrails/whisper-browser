#!/usr/bin/env python3
"""Icons and version details of Windows programs (PE files): read, check, replace.

Whisper's Windows programs and its installer must show Whisper's icon, never Firefox's.
Most of them get it at build time from the branding folder (prepare.sh copies
assets/icon/whisper.ico there under Firefox's file names, patches/0030 points the rest
at it). One cannot: the 7-Zip self-extractor that Firefox's installer starts with
(other-licenses/7zstub/firefox/7zSD.Win32.sfx) is a ready-made program in Firefox's
source with Firefox's icon and "Firefox"/"Mozilla" file details inside. `rebrand-sfx`
writes a copy of it with Whisper's icon and file details into the branding folder,
which is what Windows shows for Whisper-Setup.exe.

  winres.py icons FILE...
      list the icon groups of each file (resource id, image sizes)
  winres.py check --ico ICO [--ico ICO...] [--extract DIR] FILE...
      every icon group of every FILE must be one of the ICOs, image for image; exits 1
      and names each one that is not (a Firefox icon, say). FILE may also be a .zip
      (every .exe and .dll inside is checked) or a Firefox-style installer .exe (the
      self-extractor itself, then the setup.exe and every program in the archive it
      carries, unpacked with 7-Zip). --extract writes every icon group found as an
      .ico file, and a PNG of its largest PNG image, for the record.
  winres.py rebrand-sfx --ico ICO --product NAME --version V --filename NAME IN OUT
      a copy of the self-extractor IN with ICO as its icon and Whisper's file details

Only the Python standard library is used, so it runs on any CI runner.
"""

import argparse
import hashlib
import io
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile

RT_ICON = 3
RT_GROUP_ICON = 14
RT_VERSION = 16
LANG_EN_US = 0x409


class PEError(Exception):
    pass


def u16(b, o):
    return struct.unpack_from("<H", b, o)[0]


def u32(b, o):
    return struct.unpack_from("<I", b, o)[0]


def align(n, a):
    return (n + a - 1) // a * a


class PE:
    """The parts of a PE file this script needs: sections and the resource tree."""

    def __init__(self, data):
        self.data = bytearray(data)
        d = self.data
        if len(d) < 0x40 or d[:2] != b"MZ":
            raise PEError("not a Windows program (no MZ header)")
        self.pe = u32(d, 0x3C)
        if d[self.pe:self.pe + 4] != b"PE\0\0":
            raise PEError("not a PE file")
        coff = self.pe + 4
        self.nsections = u16(d, coff + 2)
        opt_size = u16(d, coff + 16)
        self.opt = coff + 20
        magic = u16(d, self.opt)
        if magic == 0x10B:
            self.dirs = self.opt + 96
        elif magic == 0x20B:
            self.dirs = self.opt + 112
        else:
            raise PEError(f"unknown optional header magic {magic:#x}")
        self.ndirs = u32(d, self.dirs - 4)
        self.sec_table = self.opt + opt_size
        self.sections = []
        for i in range(self.nsections):
            o = self.sec_table + 40 * i
            name = bytes(d[o:o + 8]).rstrip(b"\0").decode("ascii", "replace")
            vsize, va, rsize, rptr = struct.unpack_from("<IIII", d, o + 8)
            self.sections.append({"index": i, "offset": o, "name": name, "vsize": vsize, "va": va,
                                  "rsize": rsize, "rptr": rptr})

    def directory(self, i):
        if i >= self.ndirs:
            return 0, 0
        return struct.unpack_from("<II", self.data, self.dirs + 8 * i)

    def rva_to_offset(self, rva):
        for s in self.sections:
            if s["va"] <= rva < s["va"] + max(s["vsize"], s["rsize"]):
                off = s["rptr"] + rva - s["va"]
                if off >= len(self.data):
                    break
                return off
        raise PEError(f"RVA {rva:#x} is in no section")

    def resources(self):
        """{(type, name, language): bytes}; type and name are ints or strings."""
        rva, size = self.directory(2)
        if not rva or not size:
            return {}
        base = self.rva_to_offset(rva)
        d = self.data
        out = {}

        def name_of(entry_name):
            if entry_name & 0x80000000:
                o = base + (entry_name & 0x7FFFFFFF)
                n = u16(d, o)
                return bytes(d[o + 2:o + 2 + 2 * n]).decode("utf-16-le")
            return entry_name

        def walk(offset, path, depth):
            if depth > 3:
                raise PEError("resource tree deeper than three levels")
            named, ids = u16(d, offset + 12), u16(d, offset + 14)
            for i in range(named + ids):
                e = offset + 16 + 8 * i
                name, target = u32(d, e), u32(d, e + 4)
                key = path + (name_of(name),)
                if target & 0x80000000:
                    walk(base + (target & 0x7FFFFFFF), key, depth + 1)
                else:
                    data_rva, data_size = struct.unpack_from("<II", d, base + target)
                    o = self.rva_to_offset(data_rva)
                    if len(key) != 3:
                        raise PEError(f"resource leaf at depth {len(key)}")
                    out[key] = bytes(d[o:o + data_size])

        walk(base, (), 1)
        return out

    def replace_resources(self, resources):
        """Rewrites the resource section (it must be the last section, with nothing after
        it in the file) with `resources`, as returned by resources()."""
        rva, _ = self.directory(2)
        sec = next((s for s in self.sections if s["va"] == rva), None)
        if sec is None:
            raise PEError("the resource directory does not start a section")
        if sec["index"] != self.nsections - 1:
            raise PEError(f"the resource section {sec['name']} is not the last section")
        if sec["rptr"] + sec["rsize"] != len(self.data):
            raise PEError("there is data after the resource section")
        cert_rva, cert_size = self.directory(4)
        if cert_rva or cert_size:
            raise PEError("the file is signed; a signed file cannot be changed")
        blob = build_resource_section(resources, sec["va"])
        file_align = u32(self.data, self.opt + 36)
        sect_align = u32(self.data, self.opt + 32)
        raw = blob + b"\0" * (align(len(blob), file_align) - len(blob))
        d = self.data
        del d[sec["rptr"]:]
        d += raw
        struct.pack_into("<II", d, sec["offset"] + 8, len(blob), sec["va"])
        struct.pack_into("<I", d, sec["offset"] + 16, len(raw))
        struct.pack_into("<II", d, self.dirs + 16, sec["va"], len(blob))
        size_of_image = align(sec["va"] + len(blob), sect_align)
        struct.pack_into("<I", d, self.opt + 56, size_of_image)
        # Windows ignores the checksum for programs, but keep it right.
        struct.pack_into("<I", d, self.opt + 64, 0)
        struct.pack_into("<I", d, self.opt + 64, pe_checksum(d, self.opt + 64))
        self.__init__(bytes(d))


def pe_checksum(data, checksum_offset):
    total = 0
    n = len(data)
    padded = bytes(data) + (b"\0" if n % 2 else b"")
    for (word,) in struct.iter_unpack("<H", padded):
        total += word
        total = (total & 0xFFFF) + (total >> 16)
    # The checksum field itself counts as zero (it is zero here already).
    total = (total & 0xFFFF) + (total >> 16)
    return (total & 0xFFFF) + n


def build_resource_section(resources, section_rva):
    """A resource section (directory tables, data entries, names, data) for `resources`."""
    tree = {}
    for (rtype, name, lang), data in resources.items():
        tree.setdefault(rtype, {}).setdefault(name, {})[lang] = data

    def order(keys):
        # Named entries first (by name), then numeric ids (ascending), as the format wants.
        named = sorted((k for k in keys if isinstance(k, str)), key=lambda s: s.upper())
        return named + sorted(k for k in keys if isinstance(k, int))

    # Breadth-first: the root table, one table per type, one table per (type, name).
    tables = [((), order(tree))]
    for t in order(tree):
        tables.append(((t,), order(tree[t])))
    for t in order(tree):
        for n in order(tree[t]):
            tables.append(((t, n), order(tree[t][n])))
    table_off = {}
    off = 0
    for path, keys in tables:
        table_off[path] = off
        off += 16 + 8 * len(keys)
    leaves = [(t, n, lang) for t in order(tree) for n in order(tree[t]) for lang in order(tree[t][n])]
    entry_off = {}
    for leaf in leaves:
        entry_off[leaf] = off
        off += 16
    strings = sorted({k for path, keys in tables for k in keys if isinstance(k, str)})
    string_off = {}
    for s in strings:
        string_off[s] = off
        off += 2 + 2 * len(s)
    off = align(off, 8)
    data_off = {}
    for leaf in leaves:
        data_off[leaf] = off
        off = align(off + len(tree[leaf[0]][leaf[1]][leaf[2]]), 8)

    out = bytearray(off)
    for path, keys in tables:
        o = table_off[path]
        named = sum(isinstance(k, str) for k in keys)
        struct.pack_into("<IIHHHH", out, o, 0, 0, 4, 0, named, len(keys) - named)
        for i, k in enumerate(keys):
            name = (0x80000000 | string_off[k]) if isinstance(k, str) else k
            child = path + (k,)
            target = entry_off[child] if len(child) == 3 else (0x80000000 | table_off[child])
            struct.pack_into("<II", out, o + 16 + 8 * i, name, target)
    for leaf in leaves:
        data = tree[leaf[0]][leaf[1]][leaf[2]]
        struct.pack_into("<IIII", out, entry_off[leaf], section_rva + data_off[leaf], len(data), 0, 0)
        out[data_off[leaf]:data_off[leaf] + len(data)] = data
    for s in strings:
        struct.pack_into("<H", out, string_off[s], len(s))
        enc = s.encode("utf-16-le")
        out[string_off[s] + 2:string_off[s] + 2 + len(enc)] = enc
    return bytes(out)


# --- Icons ------------------------------------------------------------------------------

def read_ico(data):
    """The images of an .ico file: [(width, height, colours, planes, bits, image bytes)]."""
    if len(data) < 6 or u16(data, 0) != 0 or u16(data, 2) != 1:
        raise PEError("not an .ico file")
    images = []
    for i in range(u16(data, 4)):
        w, h, colours, _, planes, bits, size, offset = struct.unpack_from("<BBBBHHII", data, 6 + 16 * i)
        images.append((w, h, colours, planes, bits, bytes(data[offset:offset + size])))
    return images


def write_ico(images):
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for w, h, colours, planes, bits, image in images:
        entries += struct.pack("<BBBBHHII", w, h, colours, 0, planes, bits, len(image), offset)
        offset += len(image)
        blobs += image
    return head + entries + blobs


def icon_groups(resources):
    """{(group name, language): [images]} from RT_GROUP_ICON and RT_ICON resources."""
    icons = {(n, lang): data for (t, n, lang), data in resources.items() if t == RT_ICON}
    icons_by_id = {}
    for (n, lang), data in icons.items():
        icons_by_id.setdefault(n, data)
    groups = {}
    for (t, n, lang), data in resources.items():
        if t != RT_GROUP_ICON:
            continue
        images = []
        for i in range(u16(data, 4)):
            w, h, colours, _, planes, bits, _size, icon_id = struct.unpack_from("<BBBBHHIH", data, 6 + 14 * i)
            image = icons.get((icon_id, lang), icons_by_id.get(icon_id))
            if image is None:
                raise PEError(f"icon group {n} names icon {icon_id}, which is missing")
            images.append((w, h, colours, planes, bits, image))
        groups[(n, lang)] = images
    return groups


def image_size(img):
    w, h = img[0] or 256, img[1] or 256
    return f"{w}x{h}"


def fingerprint(images):
    """What makes two icons the same: the multiset of their image bytes."""
    return tuple(sorted(hashlib.sha256(img[5]).hexdigest() for img in images))


def set_icon(resources, ico_images):
    """Replaces every icon of `resources` with ico_images, under the first group's id."""
    groups = sorted(((n, lang) for (t, n, lang) in resources if t == RT_GROUP_ICON),
                    key=lambda k: (isinstance(k[0], str), str(k[0])))
    if not groups:
        raise PEError("the file has no icon to replace")
    group_name, lang = groups[0]
    kept = {k: v for k, v in resources.items() if k[0] not in (RT_ICON, RT_GROUP_ICON)}
    entries = b""
    for i, (w, h, colours, planes, bits, image) in enumerate(ico_images, start=1):
        kept[(RT_ICON, i, lang)] = image
        entries += struct.pack("<BBBBHHIH", w, h, colours, 0, planes, bits, len(image), i)
    kept[(RT_GROUP_ICON, group_name, lang)] = struct.pack("<HHH", 0, 1, len(ico_images)) + entries
    return kept


# --- Version details --------------------------------------------------------------------

def version_node(key, value=b"", text=False, children=b""):
    """One node of a VS_VERSIONINFO tree."""
    key_bytes = key.encode("utf-16-le") + b"\0\0"
    head_len = 6 + len(key_bytes)
    pad1 = align(head_len, 4) - head_len
    if text:
        value_bytes = value.encode("utf-16-le") + b"\0\0"
        value_len = len(value_bytes) // 2
    else:
        value_bytes = value
        value_len = len(value_bytes)
    body = b"\0" * pad1 + value_bytes
    if children:
        body += b"\0" * (align(head_len + len(body), 4) - head_len - len(body)) + children
    length = head_len + len(body)
    node = struct.pack("<HHH", length, value_len, 1 if text or (not value and children) else 0) + key_bytes + body
    return node + b"\0" * (align(len(node), 4) - len(node))


def version_resource(strings, version):
    parts = [int(p) for p in version.split(".")] + [0, 0, 0, 0]
    ms, ls = (parts[0] << 16) | parts[1], (parts[2] << 16) | parts[3]
    fixed = struct.pack("<13I", 0xFEEF04BD, 0x00010000, ms, ls, ms, ls, 0x3F, 0, 0x00040004, 1, 0, 0, 0)
    table = b"".join(version_node(k, v, text=True) for k, v in strings)
    string_info = version_node("StringFileInfo", children=version_node("040904b0", children=table))
    var_info = version_node("VarFileInfo",
                            children=version_node("Translation", struct.pack("<HH", LANG_EN_US, 1200)))
    return version_node("VS_VERSION_INFO", fixed, children=string_info + var_info)


def version_strings(resources):
    """The text values of the version resource (for the record and the checks)."""
    out = {}
    for (t, _n, _lang), data in resources.items():
        if t != RT_VERSION:
            continue

        def walk(o, end):
            while o + 6 <= end:
                length, value_len, vtype = struct.unpack_from("<HHH", data, o)
                if length == 0:
                    break
                k = o + 6
                key_end = k
                while key_end + 1 < len(data) and data[key_end:key_end + 2] != b"\0\0":
                    key_end += 2
                key = data[k:key_end].decode("utf-16-le", "replace")
                v = align(key_end + 2, 4)
                if vtype == 1 and value_len and key not in ("StringFileInfo", "VarFileInfo"):
                    # wValueLength counts characters, but some resource compilers write
                    # bytes; the value ends at its NUL either way.
                    text = data[v:v + 2 * value_len].decode("utf-16-le", "replace")
                    out[key] = text.split("\0", 1)[0]
                    child = align(v + 2 * value_len, 4)
                else:
                    child = align(v + value_len, 4)
                if child < o + length:
                    walk(child, o + length)
                o = align(o + length, 4)

        walk(0, len(data))
    return out


# --- Commands ---------------------------------------------------------------------------

def sevenzip():
    for name in (os.environ.get("SEVENZIP"), "7zz", "7z", "7za"):
        if name and shutil.which(name):
            return shutil.which(name)
    return None


def programs_in(path, label, work):
    """[(label, PE bytes)] for a file: itself, or what a zip or an installer holds."""
    data = open(path, "rb").read()
    if data[:4] == b"PK\x03\x04":
        out = []
        with zipfile.ZipFile(io.BytesIO(data)) as z:
            for info in z.infolist():
                if info.filename.lower().endswith((".exe", ".dll")):
                    out.append((f"{label}:{info.filename}", z.read(info)))
        return out
    out = [(label, data)]
    marker = data.find(b"7z\xbc\xaf\x27\x1c")
    if data[:2] == b"MZ" and marker > 0:
        tool = sevenzip()
        if not tool:
            raise PEError(f"{label} carries a 7-Zip archive, but no 7z/7zz is installed to look inside")
        target = tempfile.mkdtemp(dir=work)
        subprocess.run([tool, "x", "-y", f"-o{target}", path], check=True, stdout=subprocess.DEVNULL)
        for root, _dirs, files in os.walk(target):
            for f in sorted(files):
                if f.lower().endswith((".exe", ".dll")):
                    full = os.path.join(root, f)
                    out.append((f"{label}:{os.path.relpath(full, target)}", open(full, "rb").read()))
        if not any(name.endswith(":setup.exe") for name, _ in out):
            raise PEError(f"{label}: no setup.exe in its archive")
    return out


def cmd_icons(a):
    for path in a.files:
        pe = PE(open(path, "rb").read())
        groups = icon_groups(pe.resources())
        print(f"{path}: {len(groups)} icon group(s)")
        for (n, lang), images in sorted(groups.items(), key=lambda g: str(g[0])):
            print(f"  {n} (language {lang}): {', '.join(image_size(i) + '/' + str(i[4]) for i in images)}")
        for k, v in version_strings(pe.resources()).items():
            print(f"  {k}: {v}")


def save_png(images, path):
    """The largest PNG-encoded image of an icon, if it has one."""
    pngs = [i for i in images if i[5][:8] == b"\x89PNG\r\n\x1a\n"]
    if pngs:
        with open(path, "wb") as f:
            f.write(max(pngs, key=lambda i: len(i[5]))[5])


def cmd_check(a):
    allowed = {}
    for ico in a.ico:
        allowed[fingerprint(read_ico(open(ico, "rb").read()))] = os.path.basename(ico)
    work = tempfile.mkdtemp(prefix="winres-")
    problems, checked = [], 0
    try:
        for path in a.files:
            for label, data in programs_in(path, os.path.basename(path), work):
                try:
                    pe = PE(data)
                    groups = icon_groups(pe.resources())
                except PEError as e:
                    problems.append(f"{label}: {e}")
                    continue
                checked += 1
                for (n, lang), images in sorted(groups.items(), key=lambda g: str(g[0])):
                    match = allowed.get(fingerprint(images))
                    sizes = ", ".join(image_size(i) for i in images)
                    print(f"{'ok ' if match else 'BAD'} {label} icon {n}: {match or 'not Whisper’s icon'} ({sizes})")
                    if not match:
                        problems.append(f"{label}: icon {n} ({sizes}) is not Whisper's")
                    if a.extract:
                        os.makedirs(a.extract, exist_ok=True)
                        stem = os.path.join(a.extract, f"{label.replace(':', '_').replace('/', '_')}-{n}")
                        with open(stem + ".ico", "wb") as f:
                            f.write(write_ico(images))
                        save_png(images, stem + ".png")
                details = version_strings(pe.resources())
                if a.product_version and label.endswith("whisper.exe"):
                    shown = details.get("ProductVersion", "")
                    ok = shown == a.product_version
                    print(f"{'ok ' if ok else 'BAD'} {label} product version: {shown or 'none'}")
                    if not ok:
                        problems.append(f"{label}: product version {shown!r}, expected {a.product_version!r}")
                bad = {k: v for k, v in details.items() if any(w in v for w in ("Mozilla", "Firefox"))}
                if bad and label.endswith((".exe", ":setup.exe")) and a.details:
                    problems.append(f"{label}: file details name Mozilla or Firefox: {bad}")
    finally:
        shutil.rmtree(work, ignore_errors=True)
    for p in problems:
        print(f"::error::{p}")
    print(f"winres: {checked} program(s) checked, {len(problems)} problem(s)")
    return 1 if problems or not checked else 0


def cmd_rebrand_sfx(a):
    pe = PE(open(a.input, "rb").read())
    resources = pe.resources()
    before = version_strings(resources)
    resources = set_icon(resources, read_ico(open(a.ico, "rb").read()))
    versions = [k for k in resources if k[0] == RT_VERSION]
    name, lang = (versions[0][1], versions[0][2]) if versions else (1, LANG_EN_US)
    for k in versions:
        del resources[k]
    resources[(RT_VERSION, name, lang)] = version_resource([
        ("CompanyName", a.company),
        ("FileDescription", f"{a.product} Setup"),
        ("FileVersion", a.version),
        ("InternalName", "setup"),
        ("LegalCopyright", a.copyright),
        ("OriginalFilename", a.filename),
        ("ProductName", a.product),
        ("ProductVersion", a.version),
    ], a.version)
    pe.replace_resources(resources)
    # Read back what was written, as Windows will.
    check = PE(bytes(pe.data))
    groups = icon_groups(check.resources())
    want = fingerprint(read_ico(open(a.ico, "rb").read()))
    if [fingerprint(g) for g in groups.values()] != [want]:
        raise PEError("the icon did not come out as written")
    after = version_strings(check.resources())
    if after.get("ProductName") != a.product or any("Mozilla" in v or "Firefox" in v for v in after.values()):
        raise PEError(f"the file details did not come out as written: {after}")
    with open(a.output, "wb") as f:
        f.write(pe.data)
    print(f"{a.output}: {a.product} self-extractor, icon from {os.path.basename(a.ico)} "
          f"({len(next(iter(groups.values())))} images); file details {after} (were {before})")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    p = sub.add_parser("icons")
    p.add_argument("files", nargs="+")
    p.set_defaults(func=cmd_icons)
    p = sub.add_parser("check")
    p.add_argument("--ico", action="append", required=True)
    p.add_argument("--extract")
    p.add_argument("--details", action="store_true",
                   help="also fail if a program's file details name Mozilla or Firefox")
    p.add_argument("--product-version",
                   help="also fail if whisper.exe's product version is not this (Whisper's version)")
    p.add_argument("files", nargs="+")
    p.set_defaults(func=cmd_check)
    p = sub.add_parser("rebrand-sfx")
    p.add_argument("--ico", required=True)
    p.add_argument("--product", required=True)
    p.add_argument("--version", required=True)
    p.add_argument("--filename", required=True)
    p.add_argument("--company", default="Whisper")
    p.add_argument("--copyright", default="License: MPL 2")
    p.add_argument("input")
    p.add_argument("output")
    p.set_defaults(func=cmd_rebrand_sfx)
    a = ap.parse_args()
    try:
        return a.func(a)
    except PEError as e:
        print(f"winres: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
