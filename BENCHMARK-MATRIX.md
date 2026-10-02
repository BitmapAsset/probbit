# pbit benchmark matrix (0.2.0, measured 2026-10-01)

pbit 0.2.0 (main `d33e781`; this branch changes no Rust source), built and measured on every platform reachable on
2026-10-01 (PDT): three GitHub-hosted standard runners twice, the macOS Intel runner once, a static (musl) Linux build,
and one local Mac mini. Every cell below comes from a run made that night and traces to a JSON file in
[scripts/bench/results/2026-10-01/](scripts/bench/results/2026-10-01/) (named in the first table; the other tables keep
its row order). `python3 scripts/bench/render_matrix.py scripts/bench/results/2026-10-01` prints the tables from those
files. No number here comes from an older build. Format as in [BENCHMARKS.md](BENCHMARKS.md): median [interquartile
range] (`statistics.quantiles(n=4)`), N = 5 runs unless a header says otherwise. What builds and links where:
[PORTABILITY.md](PORTABILITY.md).

**Runs.** GitHub Actions `bench` run [36970631022](https://github.com/BitmapAsset/pbit/actions/runs/36970631022)
(2026-10-02 05:49 UTC) gives the Linux, macOS arm64, Windows and static-musl rows; run
[36969399069](https://github.com/BitmapAsset/pbit/actions/runs/36969399069) (05:32 UTC, same Rust source) gives the
macOS Intel row, which ran once (a slow runner billed at 10x; the workflow now runs it only on request), and the first
half of the run-to-run table. The runners compiled with rustc 1.99.0. The M4 row ran locally (rustc 1.98.1) while a
second worker was compiling and running pbit on the same machine: 1-minute load averages 8.5 at the start, 6.4 at the
end, recorded per block in its JSON. Runner VMs are shared; their load is not known.

**What runs** (`scripts/bench/matrix_bench.py`, stdlib Python, the same script on every OS):

```sh
cargo build --release --locked --workspace
cargo build --release --locked -p pbit-core --example kernels
RUSTFLAGS="-C target-cpu=native" cargo build --release --locked -p pbit-cli --target-dir target/native        # x86_64 rows, M4
RUSTFLAGS="-C target-cpu=native" cargo build --release --locked -p pbit-core --example kernels --target-dir target/native
python3 scripts/bench/matrix_bench.py --bin target/release/pbit --label <os-arch> --out bench-<os-arch>.json \
    [--native-dir target/native/release]
```

- `pbit stats`: `self_test.site_updates_per_s_1_thread` and `self_test.site_updates_per_s` at the default thread count
  (`min(cores, 4)`).
- `pbit-core/examples/kernels`: the BENCHMARKS.md §1 kernel rows, portable build.
- `pbit demo --tasks 12 | pbit decide` and `pbit demo --tasks 300 | pbit decide --budget-ms 300`: the wall clock of both
  processes, from Python (`time.perf_counter`), next to the `ms` pbit reports itself; released tasks, violations,
  verdicts and exit codes of every run.
- Peak memory of `pbit decide --budget-ms 300 < demo300.json` (N = 3): `/usr/bin/time -l` on macOS, `/usr/bin/time -v`
  on Linux, and on Windows `PeakWorkingSetSize` from `K32GetProcessMemoryInfo` on the exited process's handle,
  cross-checked by polling PowerShell `Get-Process` (`scripts/bench/peak_rss_windows.ps1`).
- `bench/portable_vs_native.py` (unchanged) where a native build exists: the native / portable ratio column.
- `bench/decide_threads.py` (unchanged).
- Tests: `cargo test --release --locked --workspace` in a separate job per OS (run 2 with `--no-fail-fast`).

The workflow is `.github/workflows/bench.yml` (pushes to its branch or a manual run; `[intel]` in the commit message
adds the Intel row).

## Tables

#### Machines, build and tests

| platform | CPU | logical cores | memory | OS | rustc | release build | binary | cargo test --release | source |
|---|---|---|---|---|---|---|---|---|---|
| macOS arm64, Apple M4 (local) | Apple M4 | 10 (4P+6E) | 16 GiB | 26.5.2 | 1.98.1 | n/a | 1.16 MB | not run in this job | `local-m4/bench-macos-arm64-m4-local.json` |
| Linux x86_64 (`ubuntu-latest`) | AMD EPYC 9V45 96-Core Processor | 2 | 8 GiB | Ubuntu 24.04.5 LTS | 1.99.0 | 13 s | 1.40 MB | pass: 127 passed, 0 failed, 1 ignored, 201 s | `run-36970631022/bench-linux-x86_64.json` |
| Linux x86_64, static musl (`ubuntu-latest`) | AMD EPYC 7763 64-Core Processor | 2 | 8 GiB | Ubuntu 24.04.5 LTS | 1.99.0 | n/a | 1.55 MB | not run in this job | `run-36970631022/cross-linux-x86_64.json → bench.linux-x86_64-musl` |
| Linux x86_64, glibc, same VM as musl | AMD EPYC 7763 64-Core Processor | 2 | 8 GiB | Ubuntu 24.04.5 LTS | 1.99.0 | n/a | 1.40 MB | not run in this job | `run-36970631022/cross-linux-x86_64.json → bench.linux-x86_64-gnu-samevm` |
| macOS arm64 (`macos-latest`) | Apple M1 (Virtual) | 3 | 7 GiB | 26.6.2 | 1.99.0 | 27 s | 1.15 MB | pass: 127 passed, 0 failed, 1 ignored, 195 s | `run-36970631022/bench-macos-arm64.json` |
| Windows x86_64 (`windows-latest`) | AMD EPYC 7763 64-Core Processor | 2 | 8 GiB | Microsoft Windows Server 2025 Datacenter (build 26100) | 1.99.0 | 37 s | 0.97 MB | pass: 127 passed, 0 failed, 1 ignored, 572 s | `run-36970631022/bench-windows-x86_64.json` |
| macOS x86_64 (`macos-15-intel`, run 1 only) | Intel(R) Core(TM) i7-8700B CPU @ 3.20GHz | 4 | 14 GiB | 15.7.9 | 1.99.0 | 86 s | 1.27 MB | FAIL: 47 passed, 1 failed, 0 ignored, 309 s | `run-36969399069/bench-macos-x86_64.json` |
| Linux x86_64, WSL2 on a Windows desktop with an RTX 4070 | pending: machine offline |  |  |  |  |  |  |  |  |
| Linux x86_64, VirtualBox VM (CPU only) | pending: machine offline |  |  |  |  |  |  |  |  |

#### `pbit stats` self-test: 400-spin ring, IR sampler, site updates/s (N = 5)

| platform | 1 thread | default threads | threads (= min(cores, 4)) |
|---|---|---|---|
| macOS arm64, Apple M4 (local) | 2.23e7 [2.16e7-2.24e7] | 7.3e7 [6.82e7-7.4e7] | 4 |
| Linux x86_64 (`ubuntu-latest`) | 1.76e7 [1.71e7-1.78e7] | 2.33e7 [2.31e7-2.37e7] | 2 |
| Linux x86_64, static musl (`ubuntu-latest`) | 1.12e7 [1.09e7-1.13e7] | 1.42e7 [1.4e7-1.42e7] | 2 |
| Linux x86_64, glibc, same VM as musl | 1.1e7 [1.09e7-1.1e7] | 1.4e7 [1.39e7-1.4e7] | 2 |
| macOS arm64 (`macos-latest`) | 1.32e7 [1.31e7-1.36e7] | 2.63e7 [2.57e7-2.64e7] | 3 |
| Windows x86_64 (`windows-latest`) | 1.03e7 [9.97e6-1.1e7] | 1.46e7 [1.28e7-1.46e7] | 2 |
| macOS x86_64 (`macos-15-intel`, run 1 only) | 5.66e6 [4.27e6-6.62e6] | 1.67e7 [1.34e7-1.86e7] | 4 |
| Linux x86_64, WSL2 on a Windows desktop with an RTX 4070 | pending: machine offline |  |  |
| Linux x86_64, VirtualBox VM (CPU only) | pending: machine offline |  |  |

#### Lattice kernels (`pbit-core/examples/kernels`, portable build), updates/s (N = 5)

| platform | multispin fast, 1 thread | multispin fast, 4 threads | multispin fast, 10 threads | heat-bath f32 fast, 1 thread | native / portable (multispin 1t / 4t / heat-bath) |
|---|---|---|---|---|---|
| macOS arm64, Apple M4 (local) | 3.64e10 [3.63e10-3.67e10] | 1.07e11 [1.06e11-1.29e11] | 1.22e11 [1.19e11-1.24e11] | 8.96e8 [8.89e8-8.98e8] | 0.99 / 0.94 / 1.00 |
| Linux x86_64 (`ubuntu-latest`) | 2.47e10 [2.43e10-2.6e10] | 2.22e10 [2.12e10-2.32e10] | 2.22e10 [2.18e10-2.26e10] | 5.03e8 [5.01e8-5.3e8] | 1.06 / 1.07 / 1.05 |
| Linux x86_64, static musl (`ubuntu-latest`) | n/a | n/a | n/a | n/a | n/a: no --native-dir |
| Linux x86_64, glibc, same VM as musl | n/a | n/a | n/a | n/a | n/a: no --native-dir |
| macOS arm64 (`macos-latest`) | 2e10 [1.62e10-2.1e10] | 4.6e10 [4.1e10-5.47e10] | 5.77e10 [4.83e10-5.87e10] | 4.95e8 [4.4e8-5.13e8] | n/a: no --native-dir |
| Windows x86_64 (`windows-latest`) | 1.32e10 [1.17e10-1.33e10] | 1.2e10 [1.08e10-1.26e10] | 1.21e10 [1.17e10-1.25e10] | 2.37e8 [1.44e8-2.43e8] | 0.98 / 1.04 / 1.00 |
| macOS x86_64 (`macos-15-intel`, run 1 only) | 7.83e9 [3.59e9-8.97e9] | 2.05e10 [1.89e10-2.36e10] | 2.35e10 [1.66e10-2.53e10] | 1.3e8 [7.83e7-1.48e8] | 1.00 / 0.85 / 1.12 |
| Linux x86_64, WSL2 on a Windows desktop with an RTX 4070 | pending: machine offline |  |  |  |  |
| Linux x86_64, VirtualBox VM (CPU only) | pending: machine offline |  |  |  |  |

#### `pbit demo --tasks 12 | pbit decide`, ms (N = 5)

| platform | wall, both processes | pbit's own `ms` | verdict | exit |
|---|---|---|---|---|
| macOS arm64, Apple M4 (local) | 10.9 [10.9-11.0] | 8.2 [8.2-8.3] | exact | 0 |
| Linux x86_64 (`ubuntu-latest`) | 10.5 [10.4-10.5] | 9.1 [9.1-9.1] | exact | 0 |
| Linux x86_64, static musl (`ubuntu-latest`) | 19.0 [19.0-19.1] | 17.6 [17.5-17.7] | exact | 0 |
| Linux x86_64, glibc, same VM as musl | 19.6 [19.5-19.7] | 17.9 [17.8-18.0] | exact | 0 |
| macOS arm64 (`macos-latest`) | 17.8 [17.7-18.2] | 13.0 [12.9-13.1] | exact | 0 |
| Windows x86_64 (`windows-latest`) | 30.8 [30.6-41.8] | 18.0 [17.9-21.5] | exact | 0 |
| macOS x86_64 (`macos-15-intel`, run 1 only) | 69.6 [63.2-78.3] | 47.8 [43.4-53.8] | exact | 0 |
| Linux x86_64, WSL2 on a Windows desktop with an RTX 4070 | pending: machine offline |  |  |  |
| Linux x86_64, VirtualBox VM (CPU only) | pending: machine offline |  |  |  |

#### `pbit demo --tasks 300 | pbit decide --budget-ms 300`, ms (N = 5)

| platform | wall, both processes | pbit's own `ms` | released (min-max) | violations (max) | verdict | sampler site updates/s |
|---|---|---|---|---|---|---|
| macOS arm64, Apple M4 (local) | 423.6 [422.1-426.9] | 418.0 [417.2-421.2] | 300-300 of 300 | 0 | diagnostics_passed | 4.5e7 [3.85e7-4.53e7] |
| Linux x86_64 (`ubuntu-latest`) | 422.5 [421.7-424.0] | 417.9 [417.3-419.5] | 300-300 of 300 | 0 | diagnostics_passed | 1.49e7 [1.47e7-1.51e7] |
| Linux x86_64, static musl (`ubuntu-latest`) | 478.5 [477.3-483.3] | 470.3 [469.2-475.2] | 300-300 of 300 | 0 | diagnostics_passed | 9.7e6 [9.61e6-9.8e6] |
| Linux x86_64, glibc, same VM as musl | 471.8 [469.7-476.0] | 465.5 [463.6-469.8] | 300-300 of 300 | 0 | diagnostics_passed | 9.37e6 [9.36e6-9.38e6] |
| macOS arm64 (`macos-latest`) | 439.2 [437.8-444.7] | 431.2 [429.7-436.0] | 300-300 of 300 | 0 | diagnostics_passed | 1.47e7 [1.34e7-1.48e7] |
| Windows x86_64 (`windows-latest`) | 493.0 [490.8-507.8] | 470.1 [467.7-490.4] | 296-300 of 300 | 0 | diagnostics_passed, partial | 8.69e6 [8.44e6-8.84e6] |
| macOS x86_64 (`macos-15-intel`, run 1 only) | 642.3 [576.8-667.2] | 616.3 [551.0-638.1] | 300-300 of 300 | 0 | diagnostics_passed | 1.06e7 [9.39e6-1.2e7] |
| Linux x86_64, WSL2 on a Windows desktop with an RTX 4070 | pending: machine offline |  |  |  |  |  |
| Linux x86_64, VirtualBox VM (CPU only) | pending: machine offline |  |  |  |  |  |

#### Peak memory of the 300-task `pbit decide --budget-ms 300`, MiB (N = 3)

| platform | peak RSS | how | pbit's own `peak_rss_mb` | Get-Process polling (Windows, lower bound) |
|---|---|---|---|---|
| macOS arm64, Apple M4 (local) | 47.6 [47.0-48.0] | /usr/bin/time -l (maximum resident set size, bytes) | 47.0, 47.6, 48.0 | - |
| Linux x86_64 (`ubuntu-latest`) | 21.0 [21.0-21.3] | /usr/bin/time -v (Maximum resident set size, KiB x 1024) | 21.0, 21.3, 21.0 | - |
| Linux x86_64, static musl (`ubuntu-latest`) | 17.5 [17.0-17.9] | /usr/bin/time -v (Maximum resident set size, KiB x 1024) | 17.5, 17.9, 17.0 | - |
| Linux x86_64, glibc, same VM as musl | 20.1 [19.7-21.1] | /usr/bin/time -v (Maximum resident set size, KiB x 1024) | 20.1, 19.7, 21.1 | - |
| macOS arm64 (`macos-latest`) | 29.8 [28.8-30.6] | /usr/bin/time -l (maximum resident set size, bytes) | 28.7, 29.7, 30.6 | - |
| Windows x86_64 (`windows-latest`) | 19.2 [19.2-20.3] | K32GetProcessMemoryInfo PeakWorkingSetSize (process handle, after exit) | n/a (Windows: `sys::usage()` is Unix-only) | 19.3, 19.9, 20.2 |
| macOS x86_64 (`macos-15-intel`, run 1 only) | 25.1 [23.4-25.5] | /usr/bin/time -l (maximum resident set size, bytes) | 23.4, 25.0, 25.4 | - |
| Linux x86_64, WSL2 on a Windows desktop with an RTX 4070 | pending: machine offline |  |  |  |
| Linux x86_64, VirtualBox VM (CPU only) | pending: machine offline |  |  |  |

#### `bench/decide_threads.py`: 300-task decide, fixed work (4 chains x 400 sweeps, polish 0), wall ms (N = 5)

| platform | --threads 1 | --threads 2 | --threads 4 | verdict |
|---|---|---|---|---|
| macOS arm64, Apple M4 (local) | 84.1 [83.1-85.8] | 65.2 [64.9-65.4] | 55.7 [55.6-55.8] | refused |
| Linux x86_64 (`ubuntu-latest`) | 99.1 [99.1-99.8] | 84.1 [82.4-85.1] | 83.2 [83.1-83.6] | refused |
| Linux x86_64, static musl (`ubuntu-latest`) | 176.4 [176.2-186.1] | 153.5 [152.4-154.6] | 156.6 [155.2-159.9] | refused |
| Linux x86_64, glibc, same VM as musl | 172.9 [172.3-173.7] | 153.6 [150.3-155.3] | 152.0 [150.6-152.6] | refused |
| macOS arm64 (`macos-latest`) | 135.6 [134.7-139.9] | 109.4 [106.1-133.6] | 119.4 [105.3-131.7] | refused |
| Windows x86_64 (`windows-latest`) | n/a: exit 1, TypeError: '<' not supported between instances of 'NoneType' and 'NoneType' |  |  |  |
| macOS x86_64 (`macos-15-intel`, run 1 only) | 295.4 [272.9-298.7] | 258.0 [225.4-310.7] | 304.1 [249.8-363.4] | refused |
| Linux x86_64, WSL2 on a Windows desktop with an RTX 4070 | pending: machine offline |  |  |  |
| Linux x86_64, VirtualBox VM (CPU only) | pending: machine offline |  |  |  |

#### Run to run: run 1 / run 2 medians, same source, two VMs per runner label

| platform | CPU (run 1 / run 2) | `pbit stats` 1 thread | multispin fast, 1 thread | 12-task wall ms | 300-task wall ms | peak RSS MiB |
|---|---|---|---|---|---|---|
| Linux x86_64 (`ubuntu-latest`) | both AMD EPYC 9V45 96-Core Processor | 1.91e7 / 1.76e7 (-8%) | 2.59e10 / 2.47e10 (-5%) | 9.6 / 10.5 (+9%) | 418.8 / 422.5 (+1%) | 21.1 / 21.0 (-0%) |
| macOS arm64 (`macos-latest`) | both Apple M1 (Virtual) | 1.25e7 / 1.32e7 (+6%) | 2.17e10 / 2e10 (-8%) | 15.5 / 17.8 (+15%) | 442.3 / 439.2 (-1%) | 30.5 / 29.8 (-3%) |
| Windows x86_64 (`windows-latest`) | AMD EPYC 9V74 80-Core Processor / AMD EPYC 7763 64-Core Processor | 1.26e7 / 1.03e7 (-19%) | 1.37e10 / 1.32e10 (-4%) | 20.6 / 30.8 (+49%) | 457.9 / 493.0 (+8%) | 20.2 / 19.2 (-5%) |

**What this lets us claim.** One source tree builds with `--locked` and gives the same answers on macOS arm64 and
x86_64, Linux x86_64 (glibc and static musl) and Windows x86_64. The 12-task decision is `exact` on every row; the
300-task decision ended with 0 violations on every one of the 35 timed runs and released all 300 tasks on every row
except two Windows runs (`partial`, 299 and 296 released; the Windows runner had 2 vCPUs and the fewest sampler updates
per second). `cargo test --release` passes in full (127 passed, 1 ignored) on the Linux, macOS arm64 and Windows runners
in run 2; the two failures seen (Intel in run 1, Windows in run 1 but not in run 2) are wall-clock bounds, not wrong
answers (PORTABILITY.md, Findings 4). A small exact decision costs 10-31 ms of wall clock including two process starts
on the standard runners (pbit's own `ms`: 9-18), 70 ms on the Intel runner; the 300-task decision with `--budget-ms 300`
returns in 420-495 ms on every standard runner and the M4 (the budget, then the gate and the 50 ms polish). Its peak
memory is 17-48 MiB and grew with the sweeps run inside the budget (median 45,008 sweeps and 47.6 MiB on the M4 with 4
threads; 8,720-14,960 sweeps and 17-30 MiB on the runners; `telemetry.sweeps` in each JSON). Binaries are 0.97-1.55 MB.
On x86_64, `-C target-cpu=native` moved the three kernel cells by -2% to +7% on the Linux and Windows runners in run 2
(-14% to +9% across both runs) and by -15% to +12% on the noisier Intel runner, the same size as the run-to-run noise:
the portable release builds give up little. The static musl build was within 4% of the glibc build on the same VM in run
2, and within 7% either way in run 1 (`run-36969399069/cross-linux-x86_64.json`), using 2.5-3.2 MiB less memory.

**What it does not let us claim.** A speed ranking of platforms: the runners are shared VMs with 2 (Linux, Windows), 3
(macOS arm64) or 4 (Intel) vCPUs, the same runner label landed on different CPUs from job to job (the two Linux jobs of
run 2: AMD EPYC 9V45 and 7763; Windows: EPYC 9V74 in run 1, 7763 in run 2), and the same cell moved by up to 19% in
`pbit stats` and 49% in the 12-task wall clock between the two runs (table above). Thread scaling: with 2-3 vCPUs the 4-
and 10-thread kernel rows oversubscribe the runner, and `pbit stats`' default thread count differs by row (last column).
Anything about the Intel runner beyond one run (its lower quartiles reach 54% below the median). The 12-task wall clocks
compare process start-up as much as pbit (Windows starts processes slowest). The M4 row ran under load 6-8. GPU, Linux
on aarch64 hardware (only a qemu smoke test, PORTABILITY.md), WSL2 and VM rows: not measured, machines offline. Rosetta
2 ran one `pbit stats` per run (1 thread 1.10e7 and 9.09e6 updates/s, 3 threads 2.00e7 and 1.42e7, runs 1 and 2;
`transcripts/cross-x86_64-apple-darwin.txt`), not enough for a row.

**Not run on every platform.** `bench/decide_threads.py` stops on Windows (`process_cpu_ms` is `null` there), and
`bench/portable_vs_native.py` needs extensionless copies of the Windows binaries, which the harness makes
(PORTABILITY.md, Findings 6). The kernels and native rows were not measured for the musl build (release binary only).