"""fixtures/MANIFEST: the sha256 and size of every frozen file of bench/body (Python 3.9, standard library only).

    python3 manifest_check.py           # check: exit 0 every listed file matches, 1 one differs or is missing
    python3 manifest_check.py --write   # re-extract fixtures/bars.md from README.md, then rewrite MANIFEST (a freeze:
                                        # every later rewrite is logged in README.md with its reason)

Listed: every fixture, variant and stream, gen.py, bars.json, bars.md (the README's bar tables), run.py and every script.
Paths are relative to bench/body/. Line endings are read as LF, so a checkout with CRLF verifies too.
"""
import hashlib
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MANIFEST = os.path.join(HERE, "fixtures", "MANIFEST")
README = os.path.join(HERE, "README.md")
BARS_MD = os.path.join(HERE, "fixtures", "bars.md")
HEADER = """# bench/body: sha256, bytes and path (relative to bench/body/) of every frozen file.
# Frozen before any measurement on an engine with the RFC 0002 blocks. A later change is logged in README.md with its
# reason; run.py checks this file before any part (exit 2 when one differs). Check: python3 manifest_check.py
"""


def data(path):
    with open(path, "rb") as f:
        return f.read().replace(b"\r\n", b"\n")


def bar_tables():
    """The README from its "## Bars" heading up to the next "## " heading."""
    out, on = [], False
    for line in data(README).decode("utf-8").split("\n"):
        if line.startswith("## "):
            if on:
                break
            on = line.strip() == "## Bars"
        if on:
            out.append(line)
    return ("\n".join(out).rstrip("\n") + "\n").encode("utf-8")


def listed():
    names = []
    for root, dirs, files in os.walk(HERE):
        dirs[:] = sorted(d for d in dirs if d != "__pycache__")
        for f in sorted(files):
            rel = os.path.relpath(os.path.join(root, f), HERE).replace(os.sep, "/")
            if rel in ("fixtures/MANIFEST", "result.json", "README.md") or f.endswith(".pyc"):
                continue
            names.append(rel)
    return sorted(names)


def write():
    with open(BARS_MD, "wb") as f:
        f.write(bar_tables())
    lines = [HEADER]
    for rel in listed():
        d = data(os.path.join(HERE, rel))
        lines.append("%s %d %s\n" % (hashlib.sha256(d).hexdigest(), len(d), rel))
    with open(MANIFEST, "w", encoding="utf-8", newline="\n") as f:
        f.write("".join(lines))
    return len(lines) - 1


def check():
    """The files that differ from MANIFEST (empty when every one matches), and bars.md against the README."""
    if not os.path.exists(MANIFEST):
        return ["fixtures/MANIFEST (missing)"]
    bad, seen = [], set()
    with open(MANIFEST, encoding="utf-8") as f:
        for line in f:
            if line.startswith("#") or not line.strip():
                continue
            sha, size, rel = line.split()
            seen.add(rel)
            full = os.path.join(HERE, rel)
            if not os.path.exists(full):
                bad.append(rel + " (missing)")
                continue
            d = data(full)
            if hashlib.sha256(d).hexdigest() != sha or len(d) != int(size):
                bad.append(rel)
    bad += [rel + " (not in MANIFEST)" for rel in listed() if rel not in seen]
    if os.path.exists(BARS_MD) and data(BARS_MD) != bar_tables():
        bad.append("fixtures/bars.md (differs from README.md's bar tables)")
    return bad


def main(argv):
    if "--write" in argv:
        print("manifest_check.py: wrote %d entries" % write())
        return 0
    bad = check()
    n = sum(1 for l in open(MANIFEST, encoding="utf-8") if l.strip() and not l.startswith("#")) if os.path.exists(MANIFEST) else 0
    print("MANIFEST: %d entries, %d differ%s" % (n, len(bad), (": " + ", ".join(bad)) if bad else ""))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
