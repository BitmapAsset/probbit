# RFC 0011: an amplitude register, an exact simulated qubit register as an optional solver tier for small stance programs

Status: **draft, round 1** (research). Nothing in this RFC changes a release or `main`; the tier is off unless a persona asks
for it, and it may be removed by the round that measures it against the default tier (B4 below).

## 1. What it is, in one paragraph

A register of q qubits (q ≤ 16 on the host, q ≤ 12 on a small microcontroller) simulated **exactly** on a CPU as a state
vector of 2^q complex amplitudes: unitary gates with phase, interference between basis states, measurement by the Born rule,
every step logged so the strand replays byte for byte. It is a simulation of a tiny quantum system and nothing else: no
quantum hardware, no speedup claim, no scaling past what 2^q amplitudes allow (10 qubits 16 KB, 16 qubits 1 MB, 20 qubits
16 MB; a microcontroller with 8 MB of PSRAM reaches about 16; a laptop about 25-28). Sampling a thermal distribution cannot
produce this behaviour (amplitudes carry signs and phases that probabilities do not); the register gets it by paying 2^q
memory and time, which is fine for the programs probbit actually solves (tens of two-valued variables, most of them not
coupled all at once).

## 2. Why bother (the question the measurement answers)

probbit's stance is one exact program over coupled p-bits per turn: biases and couplings → exact marginals → a level per
trait with its odds → a stance line, habits as hard rules on top. The p-bit tier gives a thermal (Boltzmann) distribution.
An amplitude register fed the same program as a Hamiltonian gives a **coherent** distribution: options can cancel (destructive
interference) as well as add, and a short unitary evolution (a transverse-field sweep or a few QAOA-style layers) reaches its
distribution by dynamics, not by equilibrium. Two concrete things that might matter to an agent:

1. **Conflict handling.** When two drives push a trait opposite ways, the thermal answer is a flattened mixture; a coherent
   answer can be a sharpened choice or a cancellation. Whether that is better for a stance is exactly what has to be measured.
2. **Reflex arcs.** A sub-turn surge needs a cheap, deterministic, bounded state shift. A q-qubit register evolved by a fixed
   small circuit is O(2^q · gates) with no sampling noise and no convergence question, which is a candidate mechanism for the
   arc, to be compared with a rule-based register.

It might also matter to nobody: the register may reproduce the p-bit marginals within noise, in which case it is a toy and the
RFC is closed with that number. Both outcomes are publishable with receipts.

## 3. Design (minimal)

- Module `probbit-decide::amp` (std, and no_std with `alloc`; the state vector on the heap): `Register::new(q)`, gates
  `H, X, Z, RX(θ), RZ(θ), CNOT, CZ, RZZ(θ)` (enough for Ising-type programs and QAOA layers), `evolve_ising(program,
  schedule)` (a Trotterised transverse-field sweep, fixed step count), `qaoa(program, p, params)`, `marginals()` (exact, no
  sampling), `measure(seed)` (the Born rule with the engine's deterministic RNG), `digest()` (sha256 of the canonical state at
  6 decimals, like every other emitted float). Amplitudes are two f64: 16 · 2^q bytes exactly.
- A solver tier `amplitude` beside `components` / `knapsack` / `exact`: selected by the persona flag `engine.tier: amplitude`
  (off when absent) and only when every variable of the program has at most two allowed values and the two-valued ones number
  at most `engine.q_max` (default 12); otherwise the existing tiers run and the stance document says why
  (`engine.amplitude: "declined: ..."`).
- How a program becomes a register (round 1): one qubit per two-valued variable (bit 0 = its first value). The program's log
  weight over the 2^q basis states is written exactly as an Ising program (`log w = c + Σ a_i z_i + Σ b_ij z_i z_j`, by its
  Walsh transform; a program with a term past pairwise declines). The register starts in |+…+> and runs the fixed default
  schedule `tf-linear-16x4` (16 Trotter steps, total time 4, `H(s) = -(1-s) Σ X_i - s (Σ a_i Z_i + Σ b_ij Z_i Z_j)`). The rules
  (habits, caps, clamps) are a projection: the Born probabilities of the plans that break one are set to 0 and the rest
  renormalised, so a stance obeys every habit in force exactly as on the other tiers. Plan = the most probable feasible plan.
- Receipt fields in the stance document: `engine.tier = "amplitude"`, `engine.qubits`, `engine.circuit` (digest of the gate
  list), `engine.schedule` (digest) and `engine.schedule_name`, `engine.state` (digest of the final amplitudes at 6 decimals),
  `engine.feasible_mass` (the Born mass on plans that obey the rules), and the marginals at 6 decimals (the stance odds).
  `live verify` recomputes all of it from the strand alone (the same rule as every other tier). An engine without the tier
  reads the persona's `engine.tier` as a field it does not know: `unsupported by this engine` (exit 5), never a divergence.
- No change to habits (hard rules), the bound certificate (it is computed from the persona file, not from the sampler), the
  strand format for other tiers, or any public wording. Public text, if any: "an exact simulated qubit register as an optional
  solver tier for small programs; measured against the default tier; no quantum hardware".
- Round 1 scope: the tier answers the turn's solves (the stance and its habit checks). The rest position at the start, the
  expectation's members and the trajectory stay on the existing tiers.

## 4. Bars (registered before any run; PASS/FAIL per bar, failures printed as failures)

- **B1 exactness.** Marginals from the register agree with an independent reference (a NumPy state-vector implementation
  outside the repo) within 1e-9 for q ≤ 12 on 200 random programs and 20 designed ones; gates unitary to 1e-12.
- **B2 replay.** 100 strands with the amplitude tier replay byte for byte on x86_64 and on the no_std build; the 6-decimal
  rounding rule holds for every emitted float; a strand recorded with the tier verifies on an engine without it with the
  documented `unsupported by this engine` exit (5), never a false divergence.
- **B3 cost.** Host: µs per gate and per `evolve_ising` for q = 8, 12, 16; memory = 16 · 2^q bytes exactly; no_std stack
  high-water for q = 8 and 12 ≤ 8 KB (the vector on the heap). Silicon numbers come later.
- **B4 agreement with the p-bit tier (the honest comparison).** On the recorded corpus (653 programs): the level chosen per
  trait agrees with the exact p-bit tier in ≥ 90 % of trait decisions under the default schedule, and the cases that differ are
  listed with both odds. KEEP as an optional tier if ≥ 90 %; DISCARD as a product tier if < 70 % (kept as a lab experiment);
  in between, the verdict is "needs a schedule study", stated as such.
- **B5 interference exists and is bounded.** Three designed programs where two paths cancel: the register's odds differ from the
  thermal odds by ≥ 0.2 at the designed node, and the persuadability bound still holds on every trait (0 exceedances on the
  247-program certificate set).
- **B6 the language.** Every public sentence uses "simulated", "exact", "on a CPU"; no claim of quantum hardware or of an
  advantage over classical computation; no claim of experience.

## 6. What this RFC does not claim

That a CPU can do what quantum hardware does at scale (it cannot: section 1), that the register has experiences (it is
arithmetic), or that it beats the p-bit tier (B4 decides). What it can honestly give: superposition with real phases and
interference inside the agent's decision, exact and replayable, simulated on a CPU at a size a laptop and a small chip can
afford.
