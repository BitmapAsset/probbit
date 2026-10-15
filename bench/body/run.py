"""bench/body: run the parts, check every gated bar (Python 3.9, standard library only).

    python3 run.py [--only needs,gate,pace,dispositions,valence,rest,load,replay,habits,p0] [--out result.json]

The binary is PROBBIT_BIN, else `probbit` on PATH. Exit 0: every gated bar of the parts run passes; 1: one fails; 2: a setup
error (no binary, a file that differs from fixtures/MANIFEST, or an engine that refuses a part's block). An engine without
a part's block is reported per part, `<part>  setup error: this engine does not support <block>`; the supported parts are
still measured, result.json records both, and the exit is 2.
"""
import json
import os
import shutil
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402
import manifest_check  # noqa: E402

PARTS = ["needs", "gate", "pace", "dispositions", "valence", "rest", "load", "replay", "habits", "p0"]
DERIVED = {"replay", "habits"}   # read the lives the parts above wrote in this run; no block of their own


def main(argv):
    only, out = PARTS, os.path.join(HERE, "result.json")
    args = list(argv)
    while args:
        a = args.pop(0)
        if a == "--only" and args:
            only = [x.strip() for x in args.pop(0).split(",") if x.strip()]
        elif a == "--out" and args:
            out = args.pop(0)
        else:
            common.fail("usage: python3 run.py [--only %s] [--out FILE]" % ",".join(PARTS))
    unknown = [m for m in only if m not in PARTS]
    if unknown:
        common.fail("unknown part(s): " + ", ".join(unknown))
    t0 = time.time()
    try:
        b = common.binary()
        bad = manifest_check.check()
        if bad:
            raise common.SetupError("files differ from fixtures/MANIFEST: " + ", ".join(bad))
        bars = common.bars()
    except common.SetupError as e:
        common.fail(str(e))
    result = {"probbit": common.version(b), "parts": {}, "setup_errors": {}}
    tmp = common.tmpdir()
    try:
        for name in [p for p in PARTS if p in only]:
            try:
                mod = __import__(name)
                if name not in DERIVED:
                    mod.check()
                res = mod.metric(bars, tmp)
                if name in DERIVED and not res["pass"]:
                    raise common.SetupError("no part it reads ran in this run")
                result["parts"][name] = res
            except common.SetupError as e:
                result["setup_errors"][name] = str(e)
                print("%-13s setup error: %s" % (name, e))
            except Exception as e:   # a part that breaks is a setup error of this run, never a crash
                result["setup_errors"][name] = "%s: %s" % (type(e).__name__, e)
                print("%-13s setup error: %s: %s" % (name, type(e).__name__, str(e)[:200]))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    wall = round(time.time() - t0, 1)
    result["wall_seconds"] = wall
    gates = {k: v for r in result["parts"].values() for k, v in r["pass"].items()}
    result["pass"] = gates
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(result, indent=1, sort_keys=True) + "\n")
    for name, r in sorted(result["parts"].items()):
        shown = {k: v for k, v in r.items() if k not in ("pass", "scope")}
        print("%-13s %s" % (name, json.dumps(shown, sort_keys=True)[:2000]))
        print("              scope: %s" % r["scope"])
    failed = sorted(k for k, ok in gates.items() if not ok)
    print("bars: %d/%d pass%s · setup errors: %d · wall %.1f s · probbit %s · %s" % (
        len(gates) - len(failed), len(gates), "" if not failed else " (failed: %s)" % ", ".join(failed),
        len(result["setup_errors"]), wall, result["probbit"], os.path.relpath(out, os.getcwd())))
    if result["setup_errors"]:
        return 2
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
