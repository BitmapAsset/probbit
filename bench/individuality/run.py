"""bench/individuality: run the metrics, check every gated bar (Python 3.9, standard library only).

    python3 run.py [--only a,b,c,d,e] [--out result.json]

The binary is PROBBIT_BIN, else `probbit` on PATH. Exit 0: every gated bar of the metrics run passes; 1: one fails (or the
wall time is over its bar); 2: a setup error (no binary, a fixture that differs from fixtures/MANIFEST). Metric f (lineage)
needs blocks this release does not have and is skipped with a note.
"""
import json
import os
import shutil
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "fixtures"))
sys.path.insert(0, HERE)
import common  # noqa: E402

KNOWN = ["a", "b", "c", "d", "e", "f"]
LATER = {"f": "lineage drift needs signed rewards and a lineage block (not in this release)"}


def manifest():
    """Every file fixtures/MANIFEST lists has its recorded sha256 and size."""
    bad = []
    with open(os.path.join(common.FIXTURES, "MANIFEST"), encoding="utf-8") as f:
        for line in f:
            if line.startswith("#") or not line.strip() or line.startswith("binary "):
                continue
            sha, size, path = line.split()
            full = os.path.join(common.FIXTURES, path)
            if not os.path.exists(full):
                bad.append(path + " (missing)")
                continue
            with open(full, "rb") as g:
                data = g.read().replace(b"\r\n", b"\n")
            if common.sha256_bytes(data) != sha or len(data) != int(size):
                bad.append(path)
    return bad


def main(argv):
    only, out = KNOWN, os.path.join(HERE, "result.json")
    args = list(argv)
    while args:
        a = args.pop(0)
        if a == "--only" and args:
            only = [x.strip() for x in args.pop(0).split(",") if x.strip()]
        elif a == "--out" and args:
            out = args.pop(0)
        else:
            common.fail("usage: python3 run.py [--only a,b,c,d,e] [--out FILE]")
    unknown = [m for m in only if m not in KNOWN]
    if unknown:
        common.fail("unknown metric(s): " + ", ".join(unknown))
    t0 = time.time()
    try:
        b = common.binary()
        bad = manifest()
        if bad:
            raise common.SetupError("fixtures differ from fixtures/MANIFEST: " + ", ".join(bad))
        with open(os.path.join(common.FIXTURES, "bars.json"), encoding="utf-8") as f:
            bars = json.load(f)
        with open(os.path.join(common.FIXTURES, "bars-c.json"), encoding="utf-8") as f:
            bars_c = json.load(f)
        import credit
        import habits
        import habituation
        import replay
        import twins
        result = {"probbit": common.version(b), "metrics": {}, "skipped": {}}
        tmp = tempfile.mkdtemp(prefix="pbit-bench-")
        try:
            lv = twins.run_lives(tmp) if set(only) & {"a", "b", "d", "e"} else {}
            stances = [d for life in lv.values() for d in life["stances"] + life["probes"]]
            if "a" in only:
                result["metrics"]["a"] = twins.metric(lv, bars)
            if "b" in only or "e" in only:
                rb = habituation.metric(lv, bars, stances)
                if "b" in only:
                    result["metrics"]["b"] = rb
            if "d" in only:
                result["metrics"]["d"] = replay.metric(lv, tmp, bars)
            if "e" in only:
                result["metrics"]["e"] = habits.metric(stances, bars)
            if "c" in only:
                result["metrics"]["c"] = credit.metric(bars_c, tmp)
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
        for m in only:
            if m in LATER:
                result["skipped"][m] = LATER[m]
    except common.SetupError as e:
        common.fail(str(e))
    wall = round(time.time() - t0, 1)
    result["wall_seconds"] = wall
    gates = {k: v for m in result["metrics"].values() for k, v in m["pass"].items()}
    gates["wall"] = wall <= bars["wall_seconds_max"]
    result["pass"] = gates
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(result, indent=1, sort_keys=True) + "\n")
    for m, r in sorted(result["metrics"].items()):
        shown = {k: v for k, v in r.items() if k not in ("pass", "scope")}
        print("%s  %s" % (m, json.dumps(shown, sort_keys=True)))
        print("   scope: %s" % r["scope"])
    for m, why in sorted(result["skipped"].items()):
        print("%s  skipped: %s" % (m, why))
    failed = sorted(k for k, ok in gates.items() if not ok)
    print("bars: %d/%d pass%s · wall %.1f s · probbit %s · %s" % (len(gates) - len(failed), len(gates),
          "" if not failed else " (failed: %s)" % ", ".join(failed), wall, result["probbit"], os.path.relpath(out, os.getcwd())))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
