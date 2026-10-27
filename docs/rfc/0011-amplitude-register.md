# RFC 0011: an amplitude register, an exact simulated qubit register as an optional solver tier for small stance programs

Status: **draft, round 2** (research). Nothing in this RFC changes a release or `main`; the tier is off unless a persona asks
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
  (off when absent) and only when every variable has an allowed value and the largest connected component of the program
  has at most `engine.q_max` encoded qubits (default 12, 1-16); otherwise the existing tiers run and the stance document says
  why (`engine.amplitude: "declined: ..."`).
- How a program becomes registers (round 2). **Encoding:** a variable with d candidate values (allowed, and the clamp)
  takes ⌈log2 d⌉ qubits; code c < d is its c-th value, the codes ≥ d are unused (d = 3: codes 00, 01, 10; 11 unused). The start
  is the uniform superposition over the valid codes (for two-valued variables, H on every qubit). **Mixer** (`complete-graph`,
  fixed): per variable exp(i t A_d) with A_d = J − I, the adjacency of the complete graph on its d codes, and the identity on
  the unused codes; closed form e^{−it}(I + (e^{itd} − 1)/d · J), O(d) per block; for d = 2 it is exp(i t X), round 1's mixer
  exactly. **Phase layer:** one general diagonal layer, a_x ← e^{−iγE(x)} a_x with E = −log w of the plan x encodes, on every
  valid basis state (O(2^q) per layer). Pair tables between multi-level variables give 3- and 4-body terms in any qubit
  encoding; the diagonal layer takes them as they are, with no Walsh transform, no residual and no decline. Round 1's RZ/RZZ
  decomposition of the cost is gone: on a 16-variable two-valued chain one general phase layer took 333 µs against 1,938 µs
  for the RZ/RZZ layer, and the whole sweep 48.4 ms against 51.9 ms (x86_64, median of 20); the gates stay in the module. The unused codes keep amplitude exactly 0: the
  engine checks their mass (≤ 1e-15) after every layer and never renormalises. **Components:** the program is split into its
  connected components (two variables are linked by a pair or by sharing a capacity rule, as the `components` tier splits
  it) and each component runs its own register. This is exact: E is a sum over components and the mixer acts per variable,
  so the joint state is the product of the components' states (the repository's example personas need 52-70 encoded qubits
  as one register, 10-15 in their largest component). The sweep is the fixed default `tf-linear-16x4` (16 Trotter steps,
  total time 4, `H(s) = −(1−s) Σ A_i + s E`). The rules (habits, caps, clamps) are a projection per component: the Born
  probabilities of the plans that break one are set to 0 and the rest renormalised, so a stance obeys every habit in force
  exactly as on the other tiers. Plan = per component its most probable feasible plan.
- Receipt fields in the stance document: `engine.tier = "amplitude"`, `engine.qubits` (encoded qubits, summed over the
  components), `engine.components` (each component's qubits), `engine.encoding` (digest of the variable → qubits map with each
  variable's number of valid codes), `engine.mixer` (`complete-graph`), `engine.circuit` (digest of the layer list),
  `engine.schedule` (digest) and `engine.schedule_name`, `engine.state` (digest of the final amplitudes at 6 decimals),
  `engine.feasible_mass` (the Born mass on plans that obey the rules), and the marginals at 6 decimals (the stance odds).
  `live verify` recomputes all of it from the strand alone (the same rule as every other tier). An engine without the tier
  reads the persona's `engine.tier` as a field it does not know: `unsupported by this engine` (exit 5), never a divergence.
- No change to habits (hard rules), the bound certificate (it is computed from the persona file, not from the sampler), the
  strand format for other tiers, or any public wording. Public text, if any: "an exact simulated qubit register as an optional
  solver tier for small programs; measured against the default tier; no quantum hardware".
- Scope (rounds 1-2): the tier answers the turn's solves (the stance and its habit checks). The rest position at the start, the
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

## 5. Measurements so far

Round 1 (two-valued programs only) and round 2 (multi-level) results are recorded in the change log's Research entries and
the pull request; the verdict on B4 and B5 belongs to round 3.

Round 2 (multi-level programs, simulated exactly on a CPU; x86_64 host, median of 20):

| bar | measured |
|---|---|
| B1 exactness | 220 programs mixing two- and three-level variables (q ≤ 12 encoded; 20 designed, among them one where the unused code would be favoured if it were allowed) against an independent mixed-radix reference (one tensor axis of size d per variable, mixer from an eigendecomposition): max \|Δ marginal\| per level 2.7e-13 (bar 1e-9); unused-code mass after every layer 0 (bar 1e-15); mixer unitarity 8.9e-16 for d = 1-8 (bar 1e-12); round 1's 220 two-valued programs through the new layers: 5.3e-15 |
| B2 replay | 100 strands of the tutor example with the tier (three-level traits; 60 encoded qubits in 12 components, the largest 10): recorded twice byte-equal 100/100, verify 100/100 on x86_64 and on the no_std build, re-written byte-equal 100/100, 0 floats past 6 decimals; an engine without the tier: exit 5 on 100/100 |
| B3 cost | one sweep of 16 steps with level marginals: 42 µs (q 8, four three-level traits), 0.82 ms (q 12, six), 16.3 ms (q 16, eight); all two-valued: 97 µs, 2.1 ms, 43 ms; memory 16 · 2^q bytes of amplitudes plus 8 · 2^q of energies; no_std stack high-water 2,016 B at q 8 and 12 |

The Born mass on plans that obey the rules can be small after the default sweep (as low as 7.3e-05 on the tutor example:
the product over its 12 components; its four-step order component alone allows 24 of its 256 codes); the projection renormalises it. B4 (agreement with the default
tiers, its first pass recorded with the pull request) and B5 have their verdict in round 3.

## 6. What this RFC does not claim

That a CPU can do what quantum hardware does at scale (it cannot: section 1), that the register has experiences (it is
arithmetic), or that it beats the p-bit tier (B4 decides). What it can honestly give: superposition with real phases and
interference inside the agent's decision, exact and replayable, simulated on a CPU at a size a laptop and a small chip can
afford.
