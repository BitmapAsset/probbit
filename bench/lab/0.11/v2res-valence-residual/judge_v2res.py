"""V2-res bars R1-R3 as registered (LOOP.md §1), read from the tables only (no engine, no network).

usage: judge_v2res.py  -> tables/rbars_v2res.tsv (+ stdout)
R1: A and A-t0 rows equal the v-run's arms_vrun.tsv (10 shared columns) and tracking_vrun.tsv rows, every printed decimal.
R2: every arm's survival, prio_acc and median_sp_pressure equal A's exactly (as printed).
R3: carrier = smallest knob set (among the single-carrier arms T0-P0, T0-D0, T0-S0, then pairs if any were run, then
    T0-all) whose removal from A-t0 gives median <= +0.10; T0-all expected |median| <= its null(i) 97.5th pct + 0.02.
"""
import csv
import os

LAB = os.path.dirname(os.path.abspath(__file__))
V = os.path.join(LAB, "..", "v011-viability", "tables")
T = os.path.join(LAB, "tables")


def rows(path):
    with open(path) as fh:
        return list(csv.reader(fh, delimiter="\t"))


def main():
    av = {r[0]: r for r in rows(os.path.join(V, "arms_vrun.tsv"))}
    tv = {(r[0], r[1]): r for r in rows(os.path.join(V, "tracking_vrun.tsv"))}
    ar = rows(os.path.join(T, "arms_v2res.tsv"))
    hdr, a = ar[0], {r[0]: r for r in ar[1:]}
    tr = {(r[0], r[1]): r for r in rows(os.path.join(T, "tracking_v2res.tsv"))}
    out = []
    diffs = []
    for arm in ["A", "A-t0"]:
        if a[arm][:10] != av[arm][:10]:
            diffs.append("arms %s: v2res %s vs vrun %s" % (arm, a[arm][:10], av[arm][:10]))
        for m in ["sp_pressure", "sp_valence"]:
            if tr[(arm, m)] != tv[(arm, m)]:
                diffs.append("tracking %s %s: v2res %s vs vrun %s" % (arm, m, tr[(arm, m)], tv[(arm, m)]))
    ph = {arm: a[arm][hdr.index("persona")] for arm in ["A", "A-t0"]}
    ph_ok = ph["A"] == "persona-07fd3405cc40bd05.json" and ph["A-t0"] == "persona-b71a32d4f6ca14e8.json"
    out.append(("R1", "PASS" if not diffs and ph_ok else "FAIL",
                "A and A-t0 rows vs v-run arms/tracking: %d differences; persona A %s, A-t0 %s (v-run 07fd3405…, b71a32d4…)"
                % (len(diffs), ph["A"], ph["A-t0"]) + ("; " + " | ".join(diffs) if diffs else "")))
    i_s, i_p, i_m = hdr.index("survival"), hdr.index("prio_acc"), hdr.index("median_sp_pressure")
    r2 = [arm for arm in a if (a[arm][i_s], a[arm][i_p], a[arm][i_m]) != (a["A"][i_s], a["A"][i_p], a["A"][i_m])]
    out.append(("R2", "PASS" if not r2 else "FINDING",
                "arms whose survival/prio_acc/pressure median differ from A (%s/%s/%s): %s"
                % (a["A"][i_s], a["A"][i_p], a["A"][i_m], ",".join(r2) if r2 else "none")))
    iv, inl = hdr.index("median_sp_valence"), hdr.index("null975_sp_valence")
    med = {arm: float(a[arm][iv]) for arm in a}
    single = [("phasic", "T0-P0"), ("drives.valence", "T0-D0"), ("sense effects", "T0-S0")]
    hit = [name for name, arm in single if arm in med and med[arm] <= 0.10]
    if hit:
        carrier = " or ".join(hit) + " (single knob)"
    elif "T0-all" in med and med["T0-all"] <= 0.10:
        carrier = "no single knob; all three removed together reach <= +0.10 (pairs not run)"
    else:
        carrier = "not on the list (T0-all > +0.10)"
    tall = med.get("T0-all")
    tall_ok = tall is not None and abs(tall) <= float(a["T0-all"][inl]) + 0.02
    out.append(("R3", "PASS" if hit or (tall is not None and tall <= 0.10) else "FAIL",
                "medians: A-t0 %+.4f; T0-P0 %+.4f; T0-D0 %+.4f; T0-S0 %+.4f; T0-all %+.4f (null(i) 97.5th %s, at-null %s); carrier: %s"
                % (med["A-t0"], med["T0-P0"], med["T0-D0"], med["T0-S0"], tall, a["T0-all"][inl], tall_ok, carrier)))
    with open(os.path.join(T, "rbars_v2res.tsv"), "w") as fh:
        fh.write("bar\tresult\tevidence\n")
        for b, r, e in out:
            fh.write("%s\t%s\t%s\n" % (b, r, e))
            print(b, r, e)


if __name__ == "__main__":
    main()
