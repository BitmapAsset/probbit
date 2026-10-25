"""Re-measure the loop's endpoints (baseline and best) on the loop's held-out seeds 100-199; writes tables/loop_check.tsv."""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import loop as L  # noqa: E402
from multiprocessing import Pool  # noqa: E402


def main():
    pool = Pool(8)
    rows = []
    for label in ["baseline", "best", "best_coupling"]:
        cfg = json.load(open(os.path.join(HERE, "config_%s.json" % label)))
        for name, seeds in [("tune_0-99", L.TUNE), ("heldout_100-199", L.HELD)]:
            m = L.measure(cfg, seeds, pool, {})
            rows.append("%s\t%s\t%.2f\t%.3f\t%.3f\t%.4f\t%.4f\n" % (label, name, m["surv"], m["value"],
                        0.8 * m["c_value"], m["track"], m["prio"]))
    pool.close()
    pool.join()
    with open(os.path.join(os.path.dirname(HERE), "tables", "loop_check.tsv"), "w") as fh:
        fh.write("config\tseeds\tA_survival\tA_value\t0.8xC_value\tA_tracking_median\tA_prio_acc\n")
        fh.writelines(rows)


if __name__ == "__main__":
    main()
