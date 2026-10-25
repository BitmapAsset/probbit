"""Ablations on the baseline (no bar): A0 reads only the engine's deficits behind a host threshold (no mood);
A1 is A with pressure's non-need inputs (time_pressure, tool_fail, spend, strain gains) set to 0. Seeds 1000-1999."""
import copy
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run_arms as R  # noqa: E402


def main():
    base = json.load(open(os.path.join(HERE, "config_baseline.json")))
    seeds = list(range(1000, 2000))
    a1 = copy.deepcopy(base)
    for g in ["tp_gain", "fail_gain", "spend_gain", "strain_gain"]:
        a1["persona"][g] = 0.0
    res = {}
    res["A"] = R.evaluate(base, seeds, ["A"])["A"]
    res["A0"] = R.evaluate(base, seeds, ["A0"])["A0"]
    res["A1"] = R.evaluate(a1, seeds, ["A"])["A"]
    out = os.path.join(os.path.dirname(HERE), "tables", "ablation_baseline.tsv")
    with open(out, "w") as fh:
        fh.write("arm\tsurvival\tmean_steps\tmean_value\tprio_acc\tmedian_sp_pressure\tsurv_diff_vs_A\tci_lo\tci_hi\n")
        for k, eps in res.items():
            s = R.summary(eps)
            d = R.boot_diff(eps, res["A"], "survival")
            fh.write("%s\t%.4f\t%.2f\t%.3f\t%.4f\t%s\t%.4f\t%.4f\t%.4f\n" % (k, s["survival"], s["steps"], s["value"],
                     s["prio_acc"], R.f(s["sp_pressure_med"]), d[0], d[1], d[2]))
    print(open(out).read())


if __name__ == "__main__":
    main()
