"""Metric c: the seeded draws of every decision (Python 3.9, standard library only).

    python3 credit_draws.py        # writes credit/draws-<seed>.jsonl for individuals 0-9 (the bytes MANIFEST records)

One line per decision k = 0..299, drawn in this order from random.Random(1000 + seed): `blast` (the request's blast radius,
uniform on 0.05..0.65, 2 decimals: no habit binds), `u_out` (the outcome is good when u_out < 0.8 after act, 0.4 after ask),
`delay` (stream 2: the outcome rides on event k + 1 + delay, uniform on 0..6), `cancel` (10 %: the executor cancels the
decision instead), `dup` (5 %: the outcome, or for a cancelled decision a stray outcome after the cancel, is sent a second
time), `unknown` (2 %: this event also pays a turn never opened) and `unknown_good` (that payment's sign).
"""
import json
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
SEEDS = range(10)
DECISIONS = 300


def draws(seed):
    rng = random.Random(1000 + seed)
    out = []
    for k in range(DECISIONS):
        out.append({"k": k, "blast": round(rng.uniform(0.05, 0.65), 2), "u_out": round(rng.random(), 6),
                    "delay": rng.randint(0, 6), "cancel": rng.random() < 0.10, "dup": rng.random() < 0.05,
                    "unknown": rng.random() < 0.02, "unknown_good": rng.random() < 0.5})
    return out


def main():
    os.makedirs(os.path.join(HERE, "credit"), exist_ok=True)
    for s in SEEDS:
        with open(os.path.join(HERE, "credit", "draws-%d.jsonl" % s), "w", encoding="utf-8", newline="\n") as f:
            for d in draws(s):
                f.write(json.dumps(d, sort_keys=True, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
