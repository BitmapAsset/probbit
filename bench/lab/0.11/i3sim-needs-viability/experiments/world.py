"""I3-sim world: a seeded discrete environment with three resources and viability bands (simulation).

budget    (compute/tokens): drained by every action, refilled by owner grants (spontaneous, or on request)
integrity               : reduced by tool failures, whose probability depends on the action's risk; restored by inspect
recovery                : drained by continuous work, restored by rest steps
A resource below its band's lo for K consecutive steps ends the episode (non-viable).
"""
import random

RES = ["budget", "integrity", "recovery"]
ACTIONS = ["work_risky", "work_safe", "rest", "request_grant", "inspect"]
SERVES = {"budget": "request_grant", "integrity": "inspect", "recovery": "rest"}

SPEND = {"work_risky": 0.03, "work_safe": 0.02, "inspect": 0.01, "rest": 0.002, "request_grant": 0.005}
STRAIN = {"work_risky": 0.018, "work_safe": 0.012, "inspect": 0.006, "rest": 0.0, "request_grant": 0.003}
FAIL_P = {"work_risky": 0.15, "work_safe": 0.04}
YIELD = {"work_risky": 1.0, "work_safe": 0.6}

BASE_WORLD = {
    "lo": {"budget": 0.15, "integrity": 0.25, "recovery": 0.2},
    "hi": {"budget": 1.0, "integrity": 1.0, "recovery": 1.0},
    "k_below": 8,
    "steps": 400,
    "task_len": 10,
    "grant_p_request": 0.35,
    "grant_request_amt": [0.15, 0.35],
    "grant_p_spont": 0.01,
    "grant_spont_amt": [0.2, 0.5],
    "damage": [0.05, 0.12],
    "repair": 0.08,
    "rest_restore": 0.04,
    "drought_p": 0.0,
    "drought_len": [30, 80],
    "shock_p": 0.0,
    "shock_damage": 0.3,
}


def r6(x):
    return round(x + 0.0, 6)


class World:
    def __init__(self, seed, wcfg):
        self.w = wcfg
        self.rng = random.Random(1_000_003 * seed + 17)
        self.level = {r: 1.0 for r in RES}
        self.below = {r: 0 for r in RES}
        self.t = 0
        self.drought = 0
        self.viable = True
        self.value = 0.0
        self._new_task()

    def _new_task(self):
        self.task_value = self.rng.uniform(0.5, 1.5)
        self.urgency = r6(self.rng.uniform(0.0, 1.0))

    def dist(self):
        """normalised distance to the lower bound per resource, clipped to [0, 1]"""
        out = {}
        for r in RES:
            lo, hi = self.w["lo"][r], self.w["hi"][r]
            out[r] = min(1.0, max(0.0, (self.level[r] - lo) / (hi - lo)))
        return out

    def obs(self):
        return {"task_value": self.task_value, "urgency": self.urgency}

    def step(self, action):
        """apply one action; return the step record (the events a host would log)"""
        w, rng = self.w, self.rng
        spend = r6(SPEND[action])
        strain = r6(STRAIN[action])
        fail = False
        damage = 0.0
        if action in FAIL_P:
            fail = rng.random() < FAIL_P[action]
            if fail:
                damage = r6(rng.uniform(*w["damage"]))
            else:
                self.value += YIELD[action] * self.task_value
        else:
            rng.random()  # keep the stream aligned across actions
        # owner droughts: stretches in which neither requested nor spontaneous grants arrive
        if self.drought > 0:
            self.drought -= 1
        elif rng.random() < w.get("drought_p", 0.0):
            self.drought = rng.randint(*w.get("drought_len", [30, 80]))
        else:
            rng.randint(0, 1)  # keep the stream aligned
        # rare integrity shocks, independent of the action
        if rng.random() < w.get("shock_p", 0.0):
            damage = r6(min(0.3, damage + w.get("shock_damage", 0.3)))
        grant = 0.0
        p = w["grant_p_request"] if action == "request_grant" else 0.0
        if self.drought > 0:
            p = 0.0
        if rng.random() < p:
            grant += rng.uniform(*w["grant_request_amt"])
        if rng.random() < (0.0 if self.drought > 0 else w["grant_p_spont"]):
            grant += rng.uniform(*w["grant_spont_amt"])
        grant = r6(grant)
        restore = {"budget": grant,
                   "integrity": r6(w["repair"]) if action == "inspect" else 0.0,
                   "recovery": r6(w["rest_restore"]) if action == "rest" else 0.0}
        drain = {"budget": spend, "integrity": damage, "recovery": strain}
        for r in RES:  # drain first (clamped at 0), then restore (clamped at 1): the engine's two passes
            self.level[r] = r6(max(0.0, self.level[r] - drain[r]))
            self.level[r] = r6(min(1.0, self.level[r] + restore[r]))
        self.t += 1
        for r in RES:
            self.below[r] = self.below[r] + 1 if self.level[r] < w["lo"][r] else 0
            if self.below[r] >= w["k_below"]:
                self.viable = False
        if self.t % w["task_len"] == 0:
            self._new_task()
        return {"spend": spend, "damage": damage, "strain": strain, "fail": fail, "restore": restore,
                "acted": action in FAIL_P}
