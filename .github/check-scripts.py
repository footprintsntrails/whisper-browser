#!/usr/bin/env python3
"""Checks that every script that is run directly can be run (CI lint, ci.yml).

1. Every tracked file that starts with "#!" is executable in git (mode 100755).
2. Every repository script a workflow runs directly (not through bash, sh or python3)
   starts with "#!" and is executable in git.
Scripts started by other scripts (make-mar.sh from update_test.py, ...) are covered by
the first rule. Exits 1 and lists what is wrong.

Usage: check-scripts.py   (from anywhere inside the repository)
"""

import os
import re
import subprocess
import sys

ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True,
                      check=True).stdout.strip()
INTERPRETERS = ("python3", "python", "bash", "sh", "pwsh", "node")


def tracked():
    out = subprocess.run(["git", "ls-files", "-s"], cwd=ROOT, capture_output=True, text=True, check=True).stdout
    modes = {}
    for line in out.splitlines():
        meta, path = line.split("\t", 1)
        modes[path] = meta.split()[0]
    return modes


def has_shebang(path):
    with open(os.path.join(ROOT, path), "rb") as f:
        return f.read(2) == b"#!"


def main():
    modes = tracked()
    problems = []
    for path, mode in sorted(modes.items()):
        if mode in ("100644", "100755") and has_shebang(path) and mode != "100755":
            problems.append(f"{path}: starts with #! but is {mode}; run git update-index --chmod=+x {path}")

    script = re.compile(r"(?<![\w./-])((?:\./)?(?:gecko|crates|\.github)/[\w./-]+\.(?:sh|py))\b")
    workflows = os.path.join(ROOT, ".github", "workflows")
    for name in sorted(os.listdir(workflows)):
        if not name.endswith((".yml", ".yaml")):
            continue
        rel = f".github/workflows/{name}"
        for n, line in enumerate(open(os.path.join(workflows, name), encoding="utf-8"), start=1):
            # Comments, and path lists (on: push: paths:) are not commands.
            if line.lstrip().startswith("#") or re.fullmatch(r"\s*-\s+[\"']?!?[\w./*-]+[\"']?\s*", line):
                continue
            for m in script.finditer(line):
                before = line[:m.start()].rstrip().split()
                if before and before[-1] in INTERPRETERS:
                    continue
                path = m.group(1).removeprefix("./")
                if path not in modes:
                    continue  # a path in a message, or a file the job creates
                if modes[path] != "100755" or not has_shebang(path):
                    problems.append(f"{rel}:{n}: runs {path} directly, but it is {modes[path]}"
                                    f"{'' if has_shebang(path) else ' without #!'}")
    for p in problems:
        print(p)
    print(f"check-scripts: {len(problems)} problem(s)" if problems else "check-scripts: every script can be run")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
