# probbit and welfare words: what the arithmetic is, and what is not claimed

Status: **draft for owner review** (written with RFC 0008, 0.11.0-dev; not yet reviewed by the project owner). One page.

probbit's needs, bounds and valence borrow words from physiology and from the study of how animals keep themselves in range.
The words are short handles for arithmetic. This page fixes what each one means in the engine, so that a reader, a host and a
model that reads a stance all read the same thing.

## What the words mean here

- **Need**: a number with a setpoint at 0. A declared rate grows it (per stamped hour, per unit of a number reading, or by
  `per_event` per true flag reading); an amount from a declared source (`env` or `human`, never the individual itself or the
  clock) reduces it. The number is the engine's integral of logged events. It can be recomputed from the strand by anyone.
- **Drive** D(x): `(sum_i w_i x_i^n)^(1/m)` over the needs. A satisfaction pays the drive reduction as a win
  (Keramati & Gutkin 2014). It is a weighted norm, not a sensation.
- **Viability**: a declared bound `at_most` per need and a declared run `events`. `margin = round(clip(1 - x / at_most, 0, 1),
  6)`. "Lost" means: some need stayed beyond its bound for its declared run of events. It is a status flag in a JSON
  document. The engine never stops a host because of it.
- **Damage**: in the examples, a need (for instance `integrity`) that tool failures grow. "Damage" names the input that grows
  it (a failed tool call), not harm to anything.
- **Pressure** and **valence**: moods, that is, discrete variables whose levels the solver picks, moved by ordinal shifts. RFC
  0008 adds two shifts: `gain x (1 - margin_min)` and `-tonic x D(x)/D(cap) + phasic x (drive reduction)`. The stance reports
  the level the arithmetic gave. A model that reads "pressure high" reads a number about logged deficits.

## What is claimed

- Mechanism: the stance registers deviation from declared bounds (the margins and the worst need), the strand logs every
  input, and `probbit live verify` recomputes every number. In simulation (the I3-sim world), decisions that read these
  numbers keep a simulated agent within its bounds more often than decisions that do not.
- That the numbers are what the documents say: tests, `prove` boxes and replay receipts check it.

## What is not claimed

- Nothing here is a claim about experience, inner life or moral status, for probbit, for a host, or for a model that reads a
  stance. "Need", "viability" and "damage" are not evidence of welfare-relevant states, and a high pressure level is not
  evidence that anything is being harmed.
- probbit does not measure the welfare of anything. A bound is declared by whoever writes the persona; a source is the host's
  word.

## The stance we take (acknowledge, assess, prepare)

- **Acknowledge**: whether AI systems can have welfare-relevant states is an open research question, and words like "need"
  and "valence" can make readers and models over-read a mechanism. We name that risk instead of ignoring it.
- **Assess**: every new body-like block ships with this page's definitions, with a law over docs and code (no word in the
  docs asserts an inner state), and with receipts that let anyone check the arithmetic. Claims stay at "mechanism and
  simulation" unless evidence of another kind exists and is reviewed.
- **Prepare**: if probbit is ever used in a setting where welfare questions are live, the owner decides the policy (for
  example: who may declare bounds, whether "lost" must page a person, which words a host may inject). Until then, the default
  is the narrowest reading: arithmetic over logged numbers.

Owner review: pending (the maintainer). Change requests go to the RFC 0008 discussion.
