"""world -> engine inputs, using only what the shipped engine accepts (docs/persona.md §2.3, §2.11, §2.12, §5.7).

The host never sends a resource level or a deficit (N-E4 refuses a deficit). It sends what it measured this step:
number senses spend / damage / strain (raw readings, S3), the flag sense tool_fail when a tool ran, the standard
input time_pressure, satisfying amounts under `needs` (src env:*), and elapsed_hours on the fixed clock. The engine's
deficit is its own integral of those events.
"""
import json
import os
import subprocess

from world import RES

BIN = os.environ["PROBBIT"]  # the probbit binary; the run used the 0.11 development build 152d005 (PR #50 head)
SENSE_OF = {"budget": "spend", "integrity": "damage", "recovery": "strain"}
SRC_OF = {"budget": "env:owner_grants", "integrity": "env:inspector", "recovery": "env:idle_meter"}


def r6(x):
    return round(x + 0.0, 6)


def make_event(rec, urgency, prev_deficit, bcfg):
    ev = {"elapsed_hours": bcfg["dt_hours"], "src": "env:host"}
    if bcfg["send_time_pressure"]:
        ev["time_pressure"] = r6(urgency)
    senses = {"spend": rec["spend"], "damage": rec["damage"], "strain": rec["strain"]}
    if rec["acted"]:
        senses["tool_fail"] = bool(rec["fail"])
    ev["senses"] = senses
    needs = {}
    for r in RES:
        a = rec["restore"][r]
        if a <= 0:
            continue
        # the host clips the amount to the deficit it can account for (the engine's last deficit + this step's
        # reading), so |x - a| never overshoots the setpoint: a full resource is a deficit of 0, not a negative one
        x_after_growth = min(1.0, r6(prev_deficit[r] + senses[SENSE_OF[r]]))
        a = r6(min(a, x_after_growth))
        if a > 0:
            needs[r] = {"amount": a, "src": SRC_OF[r]}
    if needs:
        ev["needs"] = needs
    return ev


class Engine:
    """one resident individual per episode: `probbit live --clock fixed`, one stance per event"""

    def __init__(self, persona_path, seed, strand=None):
        cmd = [BIN, "live", persona_path, "--clock", "fixed", "--seed", str(seed)]
        if strand:
            cmd += ["--strand", strand]
        self.p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  text=True, bufsize=1)

    def send(self, ev):
        self.p.stdin.write(json.dumps(ev, sort_keys=True) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        doc = json.loads(line)
        if "error" in doc:
            raise RuntimeError("engine refused event: %s :: %s" % (line.strip(), json.dumps(ev)))
        return doc

    def close(self):
        self.p.stdin.close()
        self.p.wait()
        return self.p.returncode
