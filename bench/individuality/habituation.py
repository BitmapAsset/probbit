"""Metric b: habituation and the named past (Python 3.9, standard library only).

b1, b2 run on the copy with every sense gene spread at 0 and check the engine against an independent implementation of the
sense step below (RFC 0001 section A.3, every line rounded to 6 decimals). b3 runs on the persona with genes. b4 and b5 read
metric a's probe documents: a past clause in `why` is checked against its template rendered here from the document.
"""
import json
import math
import os
import shutil
import tempfile

import common
from fixtures import lives

NOGENES = os.path.join(common.FIXTURES, "sensed-nogenes.yaml")
GENES = os.path.join(common.FIXTURES, "sensed.yaml")
B3_SEEDS = list(range(100))
r6 = common.r6


def declared(persona):
    """Per sense (in declared order): the describe document's fields."""
    code, out, err = common.run(["persona", "describe", persona])
    if code:
        raise common.SetupError("describe: " + err)
    d = json.loads(out)
    order = [line.split("id:")[1].strip() for line in open(persona, encoding="utf-8")
             if line.strip().startswith("- id:") and line.split("id:")[1].strip() in d.get("senses", {})]
    return d, [(i, d["senses"][i]) for i in order]


def clip(x, lo, hi):
    return max(lo, min(hi, x))


def step(s, norm, reading, genes):
    """RFC 0001 A.3 for one reading of sense `s` against `norm` {mean, var, n}: (z, zd, the normal after)."""
    g_gain, g_thr, g_hab = genes
    x = (1.0 if reading else 0.0) if s["kind"] == "flag" else (r6(math.log1p(reading)) if s["scale"] == "log1p" else reading)
    h = r6(clip(s["half_life_events"] * g_hab, 1, 10000))
    a = r6(1 - 0.5 ** (1 / h))
    z = r6(clip((x - norm["mean"]) / math.sqrt(norm["var"] + s["var_floor"]), -s["zcap"], s["zcap"]))
    d = r6(clip(s["dead_zone"] + g_thr, 0, s["zcap"] - 0.000001))
    zd = r6(math.copysign(max(abs(z) - d, 0.0), z)) if z else 0.0
    pm, dc = s["prior"]["mean"], s["drift_cap"]
    mean = r6(clip(norm["mean"] + a * (x - norm["mean"]), pm - dc, pm + dc))
    var = r6(clip((1 - a) * (norm["var"] + a * (x - norm["mean"]) ** 2), 0, s["var_cap"]))
    return z, zd, {"mean": mean, "var": var, "n": min(norm["n"] + 1, 1000000)}


def reading_of(v):
    return v["v"] if isinstance(v, dict) else v


def check_life(persona, seed, events):
    """Run a life with a checkpoint after every event; compare every reading's z, zd, judged mean and n, and the normal after
    it, with step(). Returns (stances, values compared, values that differ)."""
    d, senses = declared(persona)
    tmp = os.path.join(tempfile.mkdtemp(prefix="pbit-hab-"), "life")
    try:
        st, strand = tmp + ".state.json", tmp + ".strand"
        common.init(persona, seed, st)
        code, docs, err = common.live(persona, events, state=st, strand=strand, checkpoint_every=1)
        if code:
            raise common.SetupError("live: " + err.strip()[-300:])
        states = [json.loads(line)["state"] for line in open(strand, encoding="utf-8") if line.startswith('{"checkpoint"')]
    finally:
        shutil.rmtree(os.path.dirname(tmp), ignore_errors=True)
    norm = {i: {"mean": s["prior"]["mean"], "var": s["prior"]["var"], "n": 0} for i, s in senses}
    compared = differ = 0
    for ev, doc, state in zip(events, docs, states):
        genes = state.get("senses", {}).get("genes", {})
        g = lambda k, i, dflt: genes.get(k, {}).get(i, dflt)  # noqa: E731
        for i, s in senses:
            if i not in ev.get("senses", {}):
                continue
            z, zd, after = step(s, norm[i], reading_of(ev["senses"][i]), (g("gain", i, 1), g("threshold", i, 0), g("habituation", i, 1)))
            got = doc["senses"][i]
            pairs = [(z, got["z"]), (zd, got["zd"]), (norm[i]["mean"], got["mean"]), (norm[i]["n"], got["n"])]
            pairs += [(after[k], state["senses"]["norm"][i][k]) for k in ("mean", "var", "n")]
            compared += len(pairs)
            differ += sum(1 for want, have in pairs if abs(want - have) > 5e-7)
            norm[i] = after
    return docs, compared, differ


def zd_series(persona, seed, events, sink):
    code, docs, err = common.live(persona, events, seed=seed)
    if code:
        raise common.SetupError("live: " + err.strip()[-300:])
    sink.extend(docs)
    return [doc["senses"]["tool_fail"] for doc in docs]


# ------------------------------------------------------------------------------------------------- the named past (b4, b5)
def plural(n, one):
    return "%d %s%s" % (n, one, "" if n == 1 else "s")


def z1(z):
    return "%+.1f" % z


def normal_clause(doc, senses, genes):
    """The RFC A.5 normal clause rendered from the document's `senses`: (sense id, text) or None."""
    read = [(i, s) for i, s in senses if i in doc.get("senses", {})]
    gain = lambda i: genes.get("gain", {}).get(i, 1)  # noqa: E731
    best = None
    for i, s in read:
        push = abs(gain(i) * doc["senses"][i]["zd"])
        if push > 0 and (best is None or push > best[0]):
            best = (push, i, s)
    say = lambda i, s: s.get("say") or i.replace("_", " ")  # noqa: E731
    if best:
        _, i, s = best
        r = doc["senses"][i]
        side = ("far " if abs(r["z"]) >= 2 else "") + ("above" if r["z"] > 0 else "below")
        return i, "%s %s its normal (z %s over %s)" % (say(i, s), side, z1(r["z"]), plural(r["n"], "reading"))
    best = None
    for i, s in read:
        r = doc["senses"][i]
        raw = r["raw"]
        x = (1.0 if raw else 0.0) if s["kind"] == "flag" else (r6(math.log1p(raw)) if s["scale"] == "log1p" else raw)
        z0 = r6(clip((x - s["prior"]["mean"]) / math.sqrt(s["prior"]["var"] + s["var_floor"]), -s["zcap"], s["zcap"]))
        gap = abs(z0) - abs(r["z"])
        if gap >= 1 and (best is None or gap > best[0]):
            best = (gap, i, s, z0)
    if best:
        _, i, s, z0 = best
        r = doc["senses"][i]
        return i, "%s usual for it now (z %s over %s, %s against its prior)" % (say(i, s), z1(r["z"]), plural(r["n"], "reading"), z1(z0))
    return None


def learned_clause(doc, state, d):
    """The RFC A.5 learned clause rendered from the state's `learned` and `learned_from`: (trait, Δ, text) or None."""
    best = None
    for t, deltas in sorted((state.get("learned") or {}).items()):
        st = doc["stance"].get(t)
        if not st or not st.get("released"):
            continue
        k = d["traits"][t]["levels"].index(st["level"])
        if abs(deltas[k]) >= 0.05 and (best is None or abs(deltas[k]) > abs(best[1])):
            best = (t, deltas[k], st["level"])
    if not best:
        return None
    t, delta, level = best
    lf = state["learned_from"][t]
    return t, delta, "learned: %s %+.2f on %s (%s, %s)" % (t, delta, level, plural(lf["rewards"], "reward"), plural(lf["corrections"], "correction"))


def named_past(lv):
    """b4 and b5 over metric a's divergent probes."""
    d, senses = declared(GENES)
    decisions = backed = probes = cited_differ = 0
    for s in sorted({k[0] for k in lv}):
        a, b = lv[(s, "A")], lv[(s, "B")]
        sa, sb = (json.load(open(x["state"], encoding="utf-8")) for x in (a, b))
        for pa, pb in zip(a["probes"], b["probes"]):
            if pa["stance"]["caution"]["level"] == pb["stance"]["caution"]["level"]:
                continue
            probes += 1
            cited = []
            for doc, state in ((pa, sa), (pb, sb)):
                decisions += 1
                nc = normal_clause(doc, senses, state.get("senses", {}).get("genes", {}))
                lc = learned_clause(doc, state, d)
                why = doc.get("why", "")
                ok_n = nc is not None and doc.get("noticed") == nc[0] and nc[1] in why
                ok_l = lc is not None and lc[2] in why
                backed += 1 if (ok_n or ok_l) else 0
                r = doc["senses"].get(nc[0]) if nc else None
                cited.append(((nc[0], r["z"], r["mean"], r["n"]) if nc else None, (lc[0], lc[1]) if lc else None))
            cited_differ += 1 if cited[0] != cited[1] else 0
    return {"divergent_probes": probes, "decisions": decisions, "b4": r6(backed / decisions) if decisions else None,
            "b5": r6(cited_differ / probes) if probes else None}


def metric(lv, bars, sink):
    """Metric b; every stance it reads is appended to `sink` (metric e)."""
    hab = lives.load("habituation")
    docs, n1, bad1 = check_life(NOGENES, 0, hab)
    sink.extend(docs)
    zd = [doc["senses"]["tool_fail"]["zd"] for doc in docs]
    b1 = r6(zd[19] / zd[0])
    # every reading of every sense: the two rooms and the surprise streams on the no-genes copy
    n_all, bad_all = n1, bad1
    for evs in (lives.load("room-a") + lives.load("tail"), lives.load("room-b") + lives.load("tail"),
                lives.load("surprise-quiet"), lives.load("surprise-rate")):
        more, n, bad = check_life(NOGENES, 0, evs)
        sink.extend(more)
        n_all, bad_all = n_all + n, bad_all + bad
    zq = zd_series(NOGENES, 0, lives.load("surprise-quiet"), sink)[-1]["z"]
    zr = zd_series(NOGENES, 0, lives.load("surprise-rate"), sink)[-1]["z"]
    b2 = r6(zq / zr)
    below = 0
    for s in B3_SEEDS:
        series = zd_series(GENES, s, hab, sink)
        below += 1 if series[19]["zd"] < series[0]["zd"] else 0
    b3 = r6(below / len(B3_SEEDS))
    past = named_past(lv)
    r = {"scope": "b1 b2: fixtures/sensed-nogenes.yaml, individual 0; b3: fixtures/sensed.yaml, individuals 0-99; "
                  "b4 b5: metric a's divergent probes",
         "b1": b1, "b1_zd_reading_1": zd[0], "b1_zd_reading_20": zd[19], "b1_values_compared": n_all, "b1_values_differing": bad_all,
         "b2": b2, "b2_z_after_quiet": zq, "b2_z_after_rate_0_3": zr, "b3": b3}
    r.update(past)
    r["pass"] = {"b1": b1 <= bars["b1"]["ratio_max"] and bad_all <= bars["b1"]["differing_values_max"],
                 "b2": b2 >= bars["b2"]["ratio_min"], "b3": b3 >= bars["b3"]["share_min"],
                 "b4": past["b4"] is not None and past["b4"] >= bars["b4"]["share_min"],
                 "b5": past["b5"] is not None and past["b5"] >= bars["b5"]["share_min"]}
    return r
