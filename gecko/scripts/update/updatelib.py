"""Shared helpers for Whisper's update tools (gecko/scripts/update/).

MAR (Mozilla ARchive) is the format of Firefox's updater. Only what Whisper's tools
check is read here: the signatures (algorithm and length), the product information
block (MAR channel and product version) and where the file data starts. Layout
(modules/libmar/src/mar_private.h): "MAR1", offset of the index (u32), file size
(u64), number of signatures (u32), then per signature its algorithm ID (u32), length
(u32) and bytes, then the number of additional blocks (u32) and per block its size
(u32, header included), ID (u32; 1 = product information: channel and version as two
NUL-terminated strings) and data. All numbers are big-endian. The index (at its
offset): its length (u32), then per file its data offset (u32), length (u32), mode
(u32) and NUL-terminated name; each file is packed with xz (make_full_update.sh).
"""

import configparser
import hashlib
import lzma
import os
import re
import struct
from xml.sax.saxutils import quoteattr

GECKO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
PRODUCT_INFO_BLOCK_ID = 1
SIGNATURE_ALGORITHMS = {1: "RSA-PKCS1-SHA1", 2: "RSA-PKCS1-SHA384"}


class MarError(Exception):
    pass


def read_env(name):
    """KEY=value lines of gecko/<name> (release.env, firefox.env)."""
    env = {}
    with open(os.path.join(GECKO, name), encoding="utf-8") as f:
        for line in f:
            m = re.match(r"^([A-Z0-9_]+)=(.*)$", line.strip())
            if m:
                env[m.group(1)] = m.group(2)
    return env


def app_ini(path):
    """(Version, BuildID) from a package's application.ini."""
    cp = configparser.RawConfigParser()
    cp.read(path, encoding="utf-8")
    return cp.get("App", "Version"), cp.get("App", "BuildID")


def read_info(path):
    with open(path, "rb") as f:
        data = f.read(1 << 16)
        f.seek(0, 2)
        size = f.tell()
    if data[:4] != b"MAR1":
        raise MarError(f"{path}: not a MAR file")
    index_offset, file_size = struct.unpack(">IQ", data[4:16])
    if file_size != size:
        raise MarError(f"{path}: header says {file_size} bytes, file has {size}")
    pos = 16
    (count,) = struct.unpack(">I", data[pos:pos + 4])
    pos += 4
    signatures = []
    for _ in range(count):
        algorithm, length = struct.unpack(">II", data[pos:pos + 8])
        pos += 8 + length
        signatures.append({"algorithm": SIGNATURE_ALGORITHMS.get(algorithm, algorithm), "bytes": length})
    (blocks,) = struct.unpack(">I", data[pos:pos + 4])
    pos += 4
    channel = version = None
    for _ in range(blocks):
        block_size, block_id = struct.unpack(">II", data[pos:pos + 8])
        if block_id == PRODUCT_INFO_BLOCK_ID:
            fields = data[pos + 8:pos + block_size].split(b"\0")
            channel, version = fields[0].decode(), fields[1].decode()
        pos += block_size
    if channel is None:
        raise MarError(f"{path}: no product information block")
    return {
        "size": size,
        "index_offset": index_offset,
        "content_offset": pos,
        "signatures": signatures,
        "channel": channel,
        "product_version": version,
    }


def extract_mar(path, dest):
    """Unpacks every file of a complete MAR into `dest`, as the updater would install
    them (the update manifests are left out). Returns the number of files."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"MAR1":
        raise MarError(f"{path}: not a MAR file")
    (index_offset,) = struct.unpack(">I", data[4:8])
    (index_size,) = struct.unpack(">I", data[index_offset:index_offset + 4])
    pos, end = index_offset + 4, index_offset + 4 + index_size
    count = 0
    while pos < end:
        offset, length, mode = struct.unpack(">III", data[pos:pos + 12])
        name_end = data.index(b"\0", pos + 12)
        name = data[pos + 12:name_end].decode("utf-8")
        pos = name_end + 1
        if name.startswith("updatev") and name.endswith(".manifest"):
            continue
        if name.startswith("/") or ".." in name.split("/"):
            raise MarError(f"{path}: unsafe file name {name!r}")
        target = os.path.join(dest, *name.split("/"))
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "wb") as out:
            out.write(lzma.decompress(data[offset:offset + length]))
        os.chmod(target, mode & 0o777)
        count += 1
    return count


def product_version(app_version, build_id):
    """The product version Whisper writes into a MAR (make-mar.sh) and its updater
    compares (patches/0022-updates-in-build-order.patch):
    159.0.1 + 20261106093414 -> 159.0.1.20261106.093414."""
    parts = app_version.split(".")
    parts += ["0"] * (3 - len(parts))
    return ".".join(parts + [build_id[:8], build_id[8:]])


def sha512(path):
    h = hashlib.sha512()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_mar(path, app_version, build_id, channel=None, signatures=None):
    """Problems (a list of strings) if the MAR does not describe this build."""
    info = read_info(path)
    problems = []
    expected = product_version(app_version, build_id)
    if info["product_version"] != expected:
        problems.append(f"MAR product version {info['product_version']}, the build gives {expected}")
    if channel and info["channel"] != channel:
        problems.append(f"MAR channel {info['channel']}, expected {channel}")
    if signatures is not None and len(info["signatures"]) != signatures:
        problems.append(f"{len(info['signatures'])} signatures, expected {signatures}")
    if any(s["algorithm"] != "RSA-PKCS1-SHA384" for s in info["signatures"]):
        problems.append(f"unexpected signature algorithm: {info['signatures']}")
    # The updater gives each file the mode the MAR records; everyone must be able to
    # read the program (an omni.ja readable by one user only stops Firefox).
    unreadable = [name for name, mode in mar_entries(path) if mode & 0o444 != 0o444]
    if unreadable:
        problems.append(f"files not readable by everyone: {unreadable[:10]}")
    return problems


def mar_entries(path):
    """(name, mode) of every file in a MAR, from its index."""
    with open(path, "rb") as f:
        data = f.read()
    (index_offset,) = struct.unpack(">I", data[4:8])
    (index_size,) = struct.unpack(">I", data[index_offset:index_offset + 4])
    pos, end = index_offset + 4, index_offset + 4 + index_size
    entries = []
    while pos < end:
        _, _, mode = struct.unpack(">III", data[pos:pos + 12])
        name_end = data.index(b"\0", pos + 12)
        entries.append((data[pos + 12:name_end].decode("utf-8"), mode))
        pos = name_end + 1
    return entries


def update_xml(app_version, build_id, display_version, url, size, sha512_hex, details_url=""):
    """One <update> with one complete <patch>, as Firefox's update service reads it."""
    update_attrs = [
        ("type", "minor"),
        ("displayVersion", display_version),
        ("appVersion", app_version),
        ("platformVersion", app_version),
        ("buildID", build_id),
    ]
    if details_url:
        update_attrs.append(("detailsURL", details_url))
    patch_attrs = [
        ("type", "complete"),
        ("URL", url),
        ("size", str(size)),
        ("hashFunction", "sha512"),
        ("hashValue", sha512_hex),
    ]

    def fmt(attrs):
        return " ".join(f"{k}={quoteattr(v)}" for k, v in attrs)

    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        "<updates>\n"
        f"  <update {fmt(update_attrs)}>\n"
        f"    <patch {fmt(patch_attrs)}/>\n"
        "  </update>\n"
        "</updates>\n"
    )
