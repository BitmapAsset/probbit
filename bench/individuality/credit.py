"""Metric c: credit accuracy (Python 3.9, standard library only).

Individuals 0-9 run fixtures/dispatch.yaml for 300 decisions each: every event is a decision on `action` (act or ask) with a
`blast` reading as a plain input. A host drives one `probbit live` per run event by event: it reads each stance's action,
draws its outcome from the frozen draws (fixtures/credit/, good with probability 0.8 after act, 0.4 after ask) and sends it
back later. Five runs per individual: stream 1 (each outcome on the next event, with `for`) and, on stream 2 (delays 0-6
events, 10 % of decisions cancelled instead, 5 % of outcomes sent twice, 2 % of events paying a turn never opened; one message
per event, a second one due on the same event moves to the next), (i) addressed (`for`), (ii) unaddressed, (iii) 0.8
semantics (fixtures/dispatch-noblock.yaml, outcomes unaddressed on arrival, cancels not sent) and (iv) the 0.8 workaround (an
outcome is kept only when the previous stance took the paid decision's level). A refused event is followed by the same
decision without its message, so every run makes 300 decisions. An independent learner replays RFC 0001 section B.2 from the
stance documents (levels, odds) and the events, and is compared with the state the strand checkpoints after every event.
"""
import json
import os
import shutil
import subprocess
import tempfile
from concurrent.futures import ThreadPoolExecutor

import common

BLOCK = os.path.join(common.FIXTURES, "dispatch.yaml")
NOBLOCK = os.path.join(common.FIXTURES, "dispatch-noblock.yaml")
SEEDS = list(range(10))
DECISIONS = 300
RUNS = ["stream1", "addressed", "unaddressed", "v08", "workaround"]
P_GOOD = {"act": 0.8, "ask": 0.4}
LEVELS = ["act", "ask"]
RATE, STEP_CAP, TOTAL_CAP, HALF_LIFE, MAX_OPEN = 0.3, 0.05, 0.4, 32, 8
TOL = 1e-6 + 1e-12
WORKERS = 50  # the host waits on each stance: the runs are latency-bound, so all 50 run side by side


def r6(x):
    """6 decimals as the engine rounds (the exact binary value, printed to 6 places)"""
    v = float("%.6f" % x)
    return 0.0 if v == 0 else v


def clip(x, lo, hi):
    return max(lo, min(hi, x))


def draws(seed):
    return common.read_jsonl(os.path.join(common.FIXTURES, "credit", "draws-%d.jsonl" % seed))


class Host:
    """One `probbit live` process driven event by event: the host sees each stance before it sends the next event."""

    def __init__(self, persona, seed, strand, errf):
        self.err = open(errf, "w")
        self.p = subprocess.Popen([common.binary(), "live", persona, "--seed", str(seed), "--clock", "fixed", "--strand", strand,
                                   "--checkpoint-every", "1"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.err,
                                  cwd=common.REPO, universal_newlines=True, bufsize=1)
        self.events, self.lines = [], []

    def send(self, ev):
        self.p.stdin.write(common.canon(ev) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line:
            raise common.SetupError("live ended before its events")
        self.events.append(ev)
        self.lines.append(line)
        return json.loads(line)

    def close(self):
        self.p.stdin.close()
        self.p.stdout.read()
        code = self.p.wait()
        self.err.close()
        return code


def compose(run, base, m, k, level):
    """The event that carries message m at step k in this run, and whether the host sent the message or dropped it"""
    ev = dict(base)
    if m["kind"] == "cancel":
        if run in ("v08", "workaround"):
            return ev, "dropped"  # 0.8 has no cancel
        ev["cancel"] = [m["n"]]
        ev["src"] = "env:executor"
        return ev, "sent"
    if run == "workaround" and (k == 0 or m["n"] not in level or level[k - 1] != level[m["n"]]):
        return ev, "dropped"  # kept only when the previous stance took the paid decision's level
    ev["praise" if m["good"] else "criticism"] = True
    ev["src"] = "env:outcome"
    if run in ("stream1", "addressed"):
        ev["for"] = m["n"]
    return ev, "sent"


def one_run(tmp, seed, run):
    """One host-driven run: the events and stance lines, the messages with their fate, the strand, the levels per decision"""
    persona = NOBLOCK if run in ("v08", "workaround") else BLOCK
    name = "s%d-%s" % (seed, run)
    strand = os.path.join(tmp, name + ".strand")
    h = Host(persona, seed, strand, os.path.join(tmp, name + ".err"))
    dr, late = draws(seed), run != "stream1"
    queue, level, msgs = {}, {}, []
    try:
        for k in range(DECISIONS):
            d = dr[k]
            due = sorted(queue.pop(k, []), key=lambda m: (m["n"], m["order"]))
            if late and d["unknown"]:
                due.append({"kind": "unknown", "n": k + 1000, "order": 2, "good": d["unknown_good"]})
            if len(due) > 1:
                queue.setdefault(k + 1, []).extend(due[1:])  # the later ones move to the next event
            base = {"blast": d["blast"], "src": "env:host"}
            if persona == BLOCK:
                base["await"] = True
            ev, m = base, (due[0] if due else None)
            if m is not None:
                ev, m["fate"] = compose(run, base, m, k, level)
                m["step"] = k
                msgs.append(m)
            doc = h.send(ev)
            if m is not None:
                m["refused"] = "error" in doc
            if "error" in doc:
                doc = h.send(base)  # the decision without its message
                if "error" in doc:
                    raise common.SetupError("s%d %s step %d: the decision itself was refused: %s" % (seed, run, k, json.dumps(doc)))
            if doc.get("turn") != k:
                raise common.SetupError("s%d %s: turn %s at step %d" % (seed, run, doc.get("turn"), k))
            level[k] = doc["stance"]["action"]["level"]
            good = d["u_out"] < P_GOOD[level[k]]
            if not late:
                queue.setdefault(k + 1, []).append({"kind": "outcome", "n": k, "order": 0, "good": good})
            elif d["cancel"]:
                queue.setdefault(k + 1 + d["delay"], []).append({"kind": "cancel", "n": k, "order": 0, "good": good})
                if d["dup"]:
                    queue.setdefault(k + 2 + d["delay"], []).append({"kind": "stray", "n": k, "order": 1, "good": good})
            else:
                queue.setdefault(k + 1 + d["delay"], []).append({"kind": "outcome", "n": k, "order": 0, "good": good})
                if d["dup"]:
                    queue.setdefault(k + 2 + d["delay"], []).append({"kind": "dup", "n": k, "order": 1, "good": good})
    finally:
        code = h.close()
    if code not in (0, 2):
        raise common.SetupError("s%d %s: live exit %d" % (seed, run, code))
    return {"seed": seed, "run": run, "persona": persona, "block": persona == BLOCK, "strand": strand, "events": h.events,
            "lines": h.lines, "msgs": msgs, "level": level, "unsettled": sum(len(v) for v in queue.values())}


def oracle(r):
    """RFC 0001 B.2 in Python, over every event in order (a refused one changes nothing): per event the open decisions on
    arrival (after expiry) and, when accepted, the payment, its eligibility, Δ after it and the step it applied"""
    delta, credit, opened, out, t = [0.0, 0.0], {}, [], [], 0
    for ev, line in zip(r["events"], r["lines"]):
        doc = json.loads(line)
        live = [n for n in opened if t - n - 1 < 4 * HALF_LIFE] if r["block"] else []
        row = {"t": t, "accepted": "error" not in doc, "open": live}
        out.append(row)
        if not row["accepted"]:
            continue
        before, opened = list(delta), [n for n in live if n not in ev.get("cancel", [])]
        sign = (1 if ev.get("praise") else 0) - (1 if ev.get("criticism") else 0)
        paid = e = None
        if sign != 0 and t > 0:
            paid = ev.get("for", t - 1) if r["block"] else t - 1
            e = 1.0 if paid == t - 1 else r6(0.5 ** ((t - paid - 1) / HALF_LIFE))
            step = [e * clip(sign * RATE * x, -STEP_CAP, STEP_CAP) for x in credit[paid]]
            mean = (step[0] + step[1]) / 2
            delta = [r6(clip(dv + (x - mean), -TOTAL_CAP, TOTAL_CAP)) for dv, x in zip(delta, step)]
            opened = [n for n in opened if n != paid]
        a = doc["stance"]["action"]
        credit[t] = [r6((1.0 if lv == a["level"] else 0.0) - a["odds"][lv]) for lv in LEVELS] if a.get("released") else [0.0, 0.0]
        if r["block"] and ev.get("await"):
            opened.append(t)
            opened = opened[-MAX_OPEN:]
        row.update({"paid": paid, "e": e, "delta": delta, "step": [x - y for x, y in zip(delta, before)], "after": list(opened)})
        t += 1
    return out


def checkpoints(strand):
    """The state after every event line (a checkpoint after each)"""
    with open(strand, encoding="utf-8") as f:
        return [j["state"] for j in (json.loads(x) for x in f if x.strip()) if "checkpoint" in j]


def rerun_identical(tmp, r):
    """The same run with its refused events deleted: the same stance lines and the same strand, byte for byte"""
    kept = [ev for ev, line in zip(r["events"], r["lines"]) if not line.startswith('{"error"')]
    evf, strand2 = os.path.join(tmp, "s%d-%s.kept.jsonl" % (r["seed"], r["run"])), os.path.join(tmp, "s%d-%s.kept.strand" % (r["seed"], r["run"]))
    common.write_jsonl(evf, kept)
    code, out, err = common.run(["live", r["persona"], "--seed", str(r["seed"]), "--clock", "fixed", "--events", evf, "--strand", strand2,
                                 "--checkpoint-every", "1"])
    lines = "".join(x for x in r["lines"] if not x.startswith('{"error"'))
    with open(r["strand"], "rb") as a, open(strand2, "rb") as b:
        return code == 0 and out.decode("utf-8") == lines and a.read() == b.read()


def evaluate(tmp, r):
    """Per run: the oracle against the engine (c1, c2), the open outcomes paid (c3), the refusals and the re-run (c4)"""
    o, states = oracle(r), checkpoints(r["strand"])
    acc = [x for x in o if x["accepted"]]
    if len(states) != len(acc):
        raise common.SetupError("s%d %s: %d checkpoints for %d accepted events" % (r["seed"], r["run"], len(states), len(acc)))
    docs = [json.loads(x) for x in r["lines"] if not x.startswith('{"error"')]
    s = {"events": len(acc), "delta_equal": 0, "step_total": 0.0, "step_equal": 0.0, "payments": 0, "open_mismatch": 0}
    prev = [0.0, 0.0]
    for row, st, doc in zip(acc, states, docs):
        eng = st["learned"]["action"]
        s["delta_equal"] += all(abs(x - y) <= TOL for x, y in zip(eng, row["delta"]))
        if r["block"]:
            s["open_mismatch"] += sorted(x["turn"] for x in st["open_credit"]) != sorted(row["after"])
        if row["paid"] is not None:
            w = sum(abs(x) for x in row["step"])
            same = all(abs((x - y) - z) <= TOL for x, y, z in zip(eng, prev, row["step"]))
            if r["block"]:
                p = doc["credit"]["paid"] or {}
                same = same and p.get("turn") == row["paid"] and p.get("e") == row["e"]
            s["payments"] += 1
            s["step_total"] += w
            s["step_equal"] += w if same else 0.0
        prev = eng
    # the open decisions on arrival of each message: the oracle's row of the earliest event sent at its step (the one carrying it)
    at, step = {}, 0
    for row in o:
        at.setdefault(step, row)
        step += row["accepted"]
    for m in r["msgs"]:
        m["open_on_arrival"] = m["n"] in at[m["step"]]["open"]
        m["ambiguous"] = any(n != at[m["step"]]["t"] - 1 for n in at[m["step"]]["open"])
    s["refused_events"] = sum(1 for x in r["lines"] if x.startswith('{"error"'))
    # without a refused event the run without them is the same run (metric d replays every strand)
    s["identical"] = rerun_identical(tmp, r) if s["refused_events"] else True
    s["reran"] = bool(s["refused_events"])
    return s


def metric(bars_c, tmp=None):
    own = tmp is None
    tmp = tmp or tempfile.mkdtemp(prefix="pbit-credit-")
    try:
        with ThreadPoolExecutor(WORKERS) as ex:
            runs = list(ex.map(lambda sr: one_run(tmp, sr[0], sr[1]), [(s, r) for s in SEEDS for r in RUNS]))
            stats = list(ex.map(lambda r: evaluate(tmp, r), runs))
    finally:
        if own:
            shutil.rmtree(tmp, ignore_errors=True)
    tot = lambda k: sum(x[k] for x in stats)  # noqa: E731
    by = lambda run: [r for r in runs if r["run"] == run]  # noqa: E731
    msgs = lambda run, *kinds: [m for r in by(run) for m in r["msgs"] if m["kind"] in kinds and m["fate"] == "sent"]  # noqa: E731
    c1 = tot("step_equal") / tot("step_total") if tot("step_total") else 1.0
    c2 = tot("delta_equal") / tot("events")
    on_arrival = [m for run in ("stream1", "addressed") for m in msgs(run, "outcome") if m["open_on_arrival"]]
    c3 = sum(not m["refused"] for m in on_arrival) / len(on_arrival)
    bad = msgs("addressed", "dup", "stray", "unknown") + [m for m in msgs("unaddressed", "outcome", "dup", "stray", "unknown") if m["ambiguous"]]
    c4 = sum(m["refused"] for m in bad) / len(bad)
    pairs = sum(x["identical"] for x in stats)
    # reported
    v08 = [(r, m) for r in by("v08") for m in r["msgs"] if m["kind"] == "outcome" and m["fate"] == "sent"]
    wrong = sum(r["level"][m["step"] - 1] != r["level"][m["n"]] for r, m in v08 if m["step"] > 0)
    wk = [m for r in by("workaround") for m in r["msgs"] if m["kind"] == "outcome"]
    act_rate = {run: common.r6(sum(r["level"][k] == "act" for r in by(run) for k in range(DECISIONS - 100, DECISIONS)) / (100 * len(SEEDS)))
                for run in ("stream1", "addressed", "v08", "workaround")}
    closed = {run: {"evicted": 0, "expired": 0} for run in ("stream1", "addressed", "unaddressed")}
    ages = []
    for r in runs:
        if r["block"]:
            for line in r["lines"]:
                c = json.loads(line).get("credit") or {}
                for x in c.get("closed", []):
                    if x["as"] in closed[r["run"]]:
                        closed[r["run"]][x["as"]] += 1
                if r["run"] == "addressed" and c.get("paid"):
                    ages.append(json.loads(line)["turn"] - c["paid"]["turn"] - 1)
    decisions = len(SEEDS) * DECISIONS
    ages.sort()
    kinds = {k: len(msgs("addressed", k)) for k in ("outcome", "cancel", "dup", "stray", "unknown")}
    res = {
        "scope": "fixtures/dispatch.yaml (dispatch-noblock.yaml for iii and iv), individuals 0-9, 300 decisions each, 5 runs; "
                 "stream 2: outcomes 0-6 events late, 10 % cancelled, 5 % sent twice, 2 % of events paying a turn never opened",
        "c1": common.r6(c1), "c2": common.r6(c2), "c3": common.r6(c3), "c4": common.r6(c4),
        "c1_payments": tot("payments"), "c2_events": tot("events"), "c3_outcomes_open_on_arrival": len(on_arrival),
        "c4_refusable": len(bad), "c4_pairs_identical": "%d/%d" % (pairs, len(stats)), "c4_runs_rerun": tot("reran"),
        "open_set_mismatches": tot("open_mismatch"), "refused_events": tot("refused_events"),
        "addressed_messages": kinds,
        "reported": {
            "v08_outcomes_credited_at_another_level": common.r6(wrong / len(v08)) if v08 else None,
            "v08_duplicates_strays_unknowns_accepted": len(msgs("v08", "dup", "stray", "unknown")),
            "workaround_outcomes_kept": common.r6(sum(m["fate"] == "sent" for m in wk) / len(wk)) if wk else None,
            "act_rate_last_100": act_rate,
            "evicted_share": {k: common.r6(v["evicted"] / decisions) for k, v in closed.items()},
            "expired_share": {k: common.r6(v["expired"] / decisions) for k, v in closed.items()},
            "addressed_paid_age": {"max": ages[-1], "p50": ages[len(ages) // 2], "p95": ages[int(len(ages) * 0.95)],
                                   "share_at_most_6": common.r6(sum(a <= 6 for a in ages) / len(ages))} if ages else None,
            "addressed_outcomes_not_open_on_arrival": sum(not m["open_on_arrival"] for m in msgs("addressed", "outcome")),
            "unsettled_at_end_addressed": sum(r["unsettled"] for r in by("addressed")),
        },
    }
    res["pass"] = {"c1": c1 >= bars_c["c1"]["share_min"], "c2": c2 >= bars_c["c2"]["share_min"], "c3": c3 >= bars_c["c3"]["share_min"],
                   "c4": c4 >= bars_c["c4"]["share_min"] and (pairs == len(stats) or not bars_c["c4"]["pairs_identical"])}
    return res


if __name__ == "__main__":
    with open(os.path.join(common.FIXTURES, "bars-c.json"), encoding="utf-8") as f:
        print(json.dumps(metric(json.load(f)), indent=1, sort_keys=True))
