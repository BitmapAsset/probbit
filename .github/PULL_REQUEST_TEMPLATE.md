## What this changes

<!-- One paragraph: the behaviour before, the behaviour after, and why. -->

## How it was checked

<!-- Commands you ran and what they printed. `cargo test --release --workspace` at minimum. -->

## Checklist

- [ ] `cargo build --release --workspace` has no warnings and `cargo test --release --workspace` passes
- [ ] No new external crates (the shipped path builds offline)
- [ ] Any number that moved is updated in the table it lives in (`BENCHMARKS.md`, `USE-CASES.md`, `docs/pbit-ir-json.md`), with machine, load and `N`
- [ ] Fixed-work output (`--sweeps`, `--polish-sweeps`) is still bit-identical across thread counts, or the RNG/move-order change is called out below and in `CHANGELOG.md`
- [ ] A new behaviour or control has a test, and the JSON contract / exit codes are unchanged or documented
- [ ] `CHANGELOG.md` has an entry under the unreleased version
