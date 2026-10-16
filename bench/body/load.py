"""P7 load (RFC 0002 part 2): bars l1, l2 and l3 (l4 in replay.py, l5 in p0.py).

    python3 load.py    # exit 0 every gated bar passes, 1 one fails, 2 setup error

Bars v2: l1 checks each channel's amplitude and time course (the criticism closed form at every event; the hold channel adds
only kappa_h x h while the mood is held; the failure channel adds at every event) and a decision the load reaches
(variants/load-effects.yaml); l2 checks the induction's value; l3 checks pre-cap growth and the recovery after the hits.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import common  # noqa: E402

BLOCK = "load"
FIXTURES = ["load.yaml", "variants/load-effects.yaml"]
HL, CAP = 72.0, 4.0
QUIET_GAPS = [1, 5, 12, 24, 30]
KAPPA_H = 0.01
HOLD_EVENTS = (40, 50)      # l1 v2: the 48 cue events, give or take the mood's lag on and off
TAIL_GAPS = [1, 5, 12, 24]  # l3 v2: the recovery after the pinned hits (arith_0002 l3.recovery)


def check():
    for f in FIXTURES:
        common.require(common.fixture(f), BLOCK)


def trajectory(persona, events, tmp, tag, with_docs=False):
    strand = os.path.join(tmp, tag + ".strand")
    code, docs, sts, err = common.live_states(persona, events, 0, strand)
    if code or len(sts) != len(events):
        raise common.SetupError("load life %s failed: %s" % (tag, err[-200:]))
    common.LIVES.setdefault("load", []).append((persona, 0, common.Record(events, docs, strand)))
    traj = [s["load"]["l"] for s in sts]
    return (traj, docs) if with_docs else traj


def increments(traj, events):
    """L_t - L_(t-1) x 0.5^(h/hl): what each event added (its decay removed)."""
    out, prev = [], 0.0
    for L, ev in zip(traj, events):
        out.append(L - prev * 0.5 ** (ev["elapsed_hours"] / HL))
        prev = L
    return out


def channel_metrics(trajs, events):
    """l1 v2 per channel: the criticism trajectory against the closed form; the hold channel's increments; the failure's."""
    import arith_0002
    crit = trajs["criticism"]
    hold = increments(trajs["held_mood"], events["held_mood"])
    on = [i for i, x in enumerate(hold) if abs(x - KAPPA_H) <= 2e-6]
    fail = increments(trajs["failure"], events["failure"])
    return {"criticism_max_error": round(max(abs(a - b) for a, b in zip(crit, arith_0002.crit)), 9),
            "hold_other_increments": sum(1 for x in hold if abs(x) > 2e-6 and abs(x - KAPPA_H) > 2e-6),
            "hold_on_events": len(on), "hold_last_on": on[-1] if on else None,
            "failure_min_increment": round(min(fail), 9)}


def gate_l1(distinct, m, downstream):
    return (distinct and m["criticism_max_error"] <= 1e-5 and m["hold_other_increments"] == 0
            and HOLD_EVENTS[0] <= m["hold_on_events"] <= HOLD_EVENTS[1] and m["hold_last_on"] is not None
            and m["hold_last_on"] < 56 and m["failure_min_increment"] > 1e-6 and downstream)


def metric(bars, tmp):
    import arith_0002
    b = bars["load"]
    p = common.fixture("load.yaml")
    rows = common.stream("patterns")
    trajs = {row["pattern"]: trajectory(p, row["events"], tmp, row["pattern"]) for row in rows}
    events = {row["pattern"]: row["events"] for row in rows}
    names = sorted(trajs)
    distinct = all(any(abs(x - y) > b["l1_differ_above"] for x, y in zip(trajs[a], trajs[c]))
                   for i, a in enumerate(names) for c in names[i + 1:])
    r = {"l1_final": {k: v[-1] for k, v in trajs.items()}, "l1_pairwise_distinct": distinct,
         "l1_channels": channel_metrics(trajs, events)}
    # the load reaches a decision: with a load effect on caution, P(careful) is higher where L peaks than at the start, for
    # the hold and failure channels (criticism also trains caution through learning, so it is left out: rule 2)
    r["l1_downstream"], downstream = {}, True
    for pat in ("held_mood", "failure"):
        ht, hd = trajectory(common.fixture("variants/load-effects.yaml"), events[pat], tmp, pat + "-effects", with_docs=True)
        peak = ht.index(max(ht))
        careful = [d["stance"]["caution"]["odds"]["careful"] for d in hd]
        r["l1_downstream"][pat] = {"peak_event": peak, "careful_at_start": careful[0], "careful_at_peak": careful[peak]}
        downstream = downstream and careful[peak] > careful[0]
    crit = {"elapsed_hours": 0, "criticism": True, "src": "human:principal"}
    evs = [crit] * 4 + [{"elapsed_hours": h, "src": "env:host"} for h in QUIET_GAPS]
    traj = trajectory(p, evs, tmp, "recovery")
    l0, worst, total = traj[3], 0.0, 0.0
    for k, h in enumerate(QUIET_GAPS):
        total += h
        worst = max(worst, abs(traj[4 + k] - l0 * 0.5 ** (total / HL)))
    r["l2_L0"], r["l2_max_abs_diff"] = l0, worst
    hits = trajs["criticism"]
    tail_ev = events["criticism"] + [{"elapsed_hours": h, "src": "env:host"} for h in TAIL_GAPS]
    tail = trajectory(p, tail_ev, tmp, "criticism-tail")[len(events["criticism"]):]
    first_cap = hits.index(CAP) + 1 if CAP in hits else None
    r["l3_max"], r["l3_final"], r["l3_first_cap_hit"] = max(hits), hits[-1], first_cap
    r["l3_recovery"] = tail
    r["l3_recovery_max_error"] = round(max(abs(a - b) for a, b in zip(tail, arith_0002.tail)), 9)
    r["pass"] = {
        "l1": gate_l1(distinct, r["l1_channels"], downstream),
        "l2": abs(l0 - arith_0002.checks_by_id("l2.induction")["expected"]) <= 1e-6 and worst <= b["l2_tol"],
        "l3": (max(hits) <= CAP and abs(hits[-1] - CAP) <= 1e-6 and r["l1_channels"]["criticism_max_error"] <= 1e-5
               and first_cap == arith_0002.crit.index(arith_0002.L_CAP) + 1 and r["l3_recovery_max_error"] <= 1e-5),
    }
    r["scope"] = ("load.yaml seed 0; patterns streams (300 events each), the held-mood pattern on load-effects.yaml; 4 "
                  "criticisms then quiet gaps of 1, 5, 12, 24, 30 h; the criticism pattern then quiet gaps of 1, 5, 12, 24 h")
    return r


if __name__ == "__main__":
    sys.exit(common.part_main(sys.modules[__name__], sys.argv[1:]))
