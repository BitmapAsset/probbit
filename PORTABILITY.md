# probbit portability (0.2.0)

What builds, what runs and what each binary links, per platform. Measured on 2026-10-01 (PDT) from the 0.2.0 source
(main `d33e781`; this branch adds scripts and docs, no Rust source change) in GitHub Actions bench runs
[36969399069](https://github.com/BitmapAsset/probbit/actions/runs/36969399069) (run 1) and
[36970631022](https://github.com/BitmapAsset/probbit/actions/runs/36970631022) (run 2) on standard runners, plus one
local Apple M4.
Evidence: [scripts/bench/results/2026-10-01/](scripts/bench/results/2026-10-01/) (one JSON per job, and the transcripts).
Speed and memory per platform: [BENCHMARK-MATRIX.md](BENCHMARK-MATRIX.md). Workflow: `.github/workflows/bench.yml`.

## Targets

| target | built on | `cargo test --release` | `probbit demo --tasks 12 \| probbit decide` | links | binary |
|---|---|---|---|---|---|
| `aarch64-apple-darwin` | `macos-latest` (M1 VM), local M4 | pass: 127, 1 ignored (`macos-latest`, runs 1 and 2) | exit 0, `exact` | `libSystem.B.dylib` only | 1.15 MB |
| `x86_64-apple-darwin` | `macos-15-intel` (native); cross-built on `macos-latest` | 1 failure in 48 (a timing bound, see Findings 4; run 1; the test run stopped at that binary) | exit 0 natively on the Intel runner, and under Rosetta 2 on the M1 runner | `libSystem.B.dylib` only | 1.27 MB |
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` (Ubuntu 24.04) | pass: 127, 1 ignored (runs 1 and 2) | exit 0 | `libc.so.6`, `libm.so.6`, `libgcc_s.so.1`; needs **glibc >= 2.34** | 1.40 MB |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | not run (release build only) | exit 0 | none: `static-pie linked`, `ldd`: statically linked | 1.55 MB |
| `aarch64-unknown-linux-musl` | `ubuntu-latest`, linked with `gcc-aarch64-linux-gnu` | not run | exit 0 under `qemu-aarch64-static` | none: `statically linked` | 1.48 MB |
| `x86_64-pc-windows-msvc` | `windows-latest` (Server 2025) | pass: 127, 1 ignored (run 2, `--no-fail-fast`); run 1: 1 failure, a timing bound (Findings 4) | exit 0 in Git Bash and PowerShell 7.6; Windows PowerShell 5.1 adds a byte-order mark to the pipe (Findings 1): pipe through `cmd` | `KERNEL32`, `ntdll`, `bcryptprimitives`, **`VCRUNTIME140`** and the UCRT (`api-ms-win-crt-*`) | 0.97 MB |
| `wasm32-wasip1` | `ubuntu-latest` | n/a | not built or run | `cargo check -p probbit-ir -p probbit-decide`: pass | n/a |
| `wasm32-unknown-unknown` (0.4.0) | local M4; CI job `wasm` (`ubuntu-latest`) | `probbit-wasm` tests run natively | 300-task demo `diagnostics_passed` in Chrome and Node (`playground/check.mjs`) | the page's `probbit.now_ms` import only | 830,233 bytes (`probbit-wasm`) |

Not reached tonight: Linux x86_64 on WSL2 (a Windows desktop with an RTX 4070) and a Linux VirtualBox VM (CPU only):
pending, both machines offline. No Linux aarch64 machine was run natively (only `qemu-aarch64-static`), and no large or
ARM GitHub runners were used (billed on a private repository).

## What each binary links

Exact tool output from the runs above (`file`, `otool -L`, `ldd`, `objdump -T`, `dumpbin /dependents`).

```text
aarch64-apple-darwin (macos-latest)    Mach-O 64-bit executable arm64
  otool -L:  /usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1356.0.0)
x86_64-apple-darwin (macos-15-intel)   Mach-O 64-bit executable x86_64
  otool -L:  /usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)
x86_64-unknown-linux-gnu (ubuntu-latest)
  file:      ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), dynamically linked,
             interpreter /lib64/ld-linux-x86-64.so.2, for GNU/Linux 3.2.0, not stripped
  ldd:       linux-vdso.so.1, libgcc_s.so.1, libm.so.6, libc.so.6, /lib64/ld-linux-x86-64.so.2
  GLIBC symbol versions (objdump -T): 2.2.5 2.3 2.3.4 2.14 2.16 2.17 2.18 2.25 2.28 2.29 2.30 2.32 2.33 2.34
x86_64-unknown-linux-musl (ubuntu-latest)
  file:      ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, not stripped
  ldd:       statically linked
aarch64-unknown-linux-musl (ubuntu-latest, cross-linked)
  file:      ELF 64-bit LSB executable, ARM aarch64, version 1 (SYSV), statically linked, not stripped
x86_64-pc-windows-msvc (windows-latest)   PE32+ executable for MS Windows 6.00 (console), x86-64, 5 sections
  dumpbin /dependents (dumpbin is not on the runner's PATH; found with vswhere):
             bcryptprimitives.dll  api-ms-win-core-synch-l1-2-0.dll  KERNEL32.dll  ntdll.dll  VCRUNTIME140.dll
             api-ms-win-crt-{math,string,runtime,stdio,locale,heap}-l1-1-0.dll
```

No binary needs anything outside the OS except two cases: the glibc build needs glibc 2.34 or later (Ubuntu 22.04+,
Debian 12+, RHEL 9+; not Ubuntu 20.04, Debian 11, RHEL 8 or Amazon Linux 2), and the Windows build needs
`VCRUNTIME140.dll` from the Visual C++ 2015-2022 redistributable (present on most machines, absent on a clean install).
The musl builds have no runtime dependency at all.

## Building for each target

```sh
# Linux x86_64, static (no glibc requirement):
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked -p probbit-cli --target x86_64-unknown-linux-musl

# Linux aarch64, static, cross-linked from x86_64 Linux:
sudo apt-get install -y gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-musl
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc \
  cargo build --release --locked -p probbit-cli --target aarch64-unknown-linux-musl
sudo apt-get install -y qemu-user-static          # to run it on x86_64: qemu-aarch64-static ./probbit version

# macOS Intel from Apple silicon (runs under Rosetta 2 with `arch -x86_64`):
rustup target add x86_64-apple-darwin
cargo build --release --locked -p probbit-cli --target x86_64-apple-darwin

# WebAssembly (WASI), feasibility check only:
rustup target add wasm32-wasip1
cargo check --target wasm32-wasip1 -p probbit-ir -p probbit-decide
```

Build times on the runners: 12-28 s for `cargo build --release --locked --workspace` (fat LTO, one codegen unit), 86 s on
the Intel runner; the musl and cross builds took 18-19 s each. `apt-get install gcc-aarch64-linux-gnu` took 13 s and
`qemu-user-static` 4 s on `ubuntu-latest`.

## Minimum Rust version

Checked on `ubuntu-latest` with debug builds of `probbit-cli` (run 1, `transcripts/msrv.txt`):

| toolchain | result |
|---|---|
| 1.72 | fails: `error[E0658]: use of unstable library feature 'int_roundings'` (`div_ceil`) |
| 1.73 | builds (with `Cargo.lock` set aside: cargo before 1.78 cannot read lockfile version 4) |
| 1.77 | fails: `failed to parse lock file` (the committed lockfile is version 4) |
| 1.78 | builds with `--locked` |

So the source needs Rust 1.73 and the repository as committed needs 1.78. `Cargo.toml` declares no `rust-version`; tests
and examples were not built on these toolchains. The runners used 1.99.0, the local M4 1.98.1.

## WebAssembly

`cargo check --target wasm32-wasip1 -p probbit-ir -p probbit-decide` passes (exit 0, no warnings shown, 0.69 s; this run predates
0.4.0). Both crates use `std::thread` (the chains and the gate's worker pool) and `std::time::Instant` (budgets and deadlines):
on `wasm32-wasip1` a thread spawn fails at run time, and on `wasm32-unknown-unknown` (a browser) `Instant::now()` panics.

0.4.0 adds both missing pieces (`probbit_core::rt`): a per-thread switch that runs every parallel section of the command paths in
order on the calling thread, and the engine's `Instant`, which is std's natively and on `wasm32-unknown-unknown` reads a clock
the embedder installs (`performance.now()` in a page). `probbit-wasm` builds for `wasm32-unknown-unknown` (`playground/build.sh`;
830,233 bytes, release, symbol names stripped, an 8 MiB stack: `.cargo/config.toml`) and runs in Chrome and Node. Measured on an
Apple M4 (medians of 5, load 3.4-3.6): the 300-task demo at `--sweeps 3200 --polish-ms 0` took 531.4 ms in headless Chrome and
533.3 ms in Node 26, against 346.2 ms native at `--threads 1` and 132.7 ms at `--threads 4`; the documents equal the native ones
(timings and `telemetry.threads` aside; `process_cpu_ms`, `peak_rss_mb` and `nice` are null there, as on Windows). Not run:
`wasm32-wasip1`, other browsers, other machines.
## Installers and packages

No release exists yet, so every installer was tested against a local server that serves an archive packed the way
`release.yml` packs it (`scripts/bench/package_like_release.sh`: same names, layout and `.sha256`).

| what | where it passed | checks |
|---|---|---|
| `install.sh` (POSIX sh) | `ubuntu-latest` (`sh` = dash, `bash`, `dash`), `macos-latest` and `macos-15-intel` (`sh`, `bash`, `dash`), local M4 | piped into the shell as `curl \| sh` does; default directory under a scratch `HOME`; a wrong `.sha256` fails and installs nothing; `PROBBIT_DOWNLOAD_BASE` without `PROBBIT_VERSION` fails |
| `install.ps1` | `windows-latest`: PowerShell 7.6 (all checks, runs 1 and 2); Windows PowerShell 5.1 (download, checksum, install, `probbit version`; the test then stopped at the demo pipeline, Findings 1, and now pipes it through `cmd`) | `-DownloadBase`, `-AddToPath` (user PATH), the `irm \| iex` form with `PROBBIT_*` variables, a wrong `.sha256` fails and installs nothing, `-DownloadBase` without `-Version` fails |
| npm wrapper (`npm/`) | `ubuntu-latest`, `windows-latest`, local M4 | `npm pack`, `npm install -g` (scratch prefix), postinstall fetch and SHA-256 check, `which probbit`, exit codes 0 / 1 / 2 / 3 passed through, `npm uninstall -g`; an `--ignore-scripts` install fetches on first run; `PROBBIT_BINARY`; a wrong `.sha256` fails the install |
| `docs/agents.md` recipes | shell, Python (`python/test_probbit.py`), Node (`examples/node/decide.mjs`): Linux, macOS arm64 and x86_64, Windows (Git Bash); PowerShell 7.6: Windows | each recipe as written, plus one call per exit code |

Install locations: `install.sh` writes `/usr/local/bin` when it can (every runner image above: their user can write it)
and `~/.local/bin` otherwise (the local M4, where `/usr/local/bin` belongs to root), and never uses sudo; `install.ps1`
writes `$HOME\.local\bin`, adds it to the session's PATH, and to the user PATH only with `-AddToPath`. Names: `probbit` is
free on npm and `probbit`, `probbit-core`, `probbit-ir`, `probbit-decide` and `probbit-cli` are free on crates.io (checked 2026-10-01
22:31 PDT); nothing was published.

## Findings that need a source, test or release change (not made here)

1. **A UTF-8 byte-order mark on stdin is rejected** (`bad JSON: unexpected character 'ï' at byte 0`, exit 2). Windows
   PowerShell 5.1 on the `windows-latest` image adds one to text it pipes into a native program even with
   `$OutputEncoding` at its `us-ascii` default (runs 1 and 2), so `probbit demo | probbit decide` exits 2 there; PowerShell 7
   adds one when a profile sets `$OutputEncoding = [Text.Encoding]::UTF8` (seen with 7.6 locally). RFC 8259 lets a parser
   ignore a leading BOM: `read_stdin()` in `probbit-cli/src/main.rs` could strip `U+FEFF`. Today: PowerShell 7, or pipe
   through `cmd /c` (docs/agents.md).
2. **The Linux release binary needs glibc 2.34.** `release.yml` builds `x86_64-unknown-linux-gnu` on `ubuntu-latest`.
   The static musl build above runs on any x86_64 Linux and was within 4% (run 2) and 7% (run 1) of the glibc build on
   the same VM, with 2.5-3.2 MiB less peak memory (BENCHMARK-MATRIX.md). Shipping it, plus `aarch64-unknown-linux-musl`,
   would also give `install.sh` something for Alpine and arm64 Linux, which it refuses today.
3. **The Windows binary needs `VCRUNTIME140.dll`.** Linking the CRT statically (`-C target-feature=+crt-static` for the
   msvc release build) would remove that; not built or measured here.
4. **Two wall-clock test bounds failed on shared runners** in run 1: `run_deadline_ms_bounds_the_whole_call` on
   `macos-15-intel` (990 ms for `--deadline-ms 600`, bound 900 ms) and `acc1_tv_le_001_within_10ms` on `windows-latest`
   (p50 13.1 ms, bound 10 ms; one seed's maximum 37 ms). In run 2 (`--no-fail-fast`) all 127 tests passed on Linux,
   macOS arm64 and Windows; the Intel row was not re-run, and its run-1 test job stopped after the failing binary (48 of
   the 128 tests ran). The same class as the `d33e781` CI tolerance.
5. **`--deadline-ms` is not a hard bound on a slow CPU**: 990 ms for a 600 ms deadline on the Intel runner (65% over).
6. **`bench/*.py` assume a Unix machine.** `bench/decide_threads.py` stops with a `TypeError` on Windows
   (`process_cpu_ms` is `null` there: `sys::usage()` is Unix-only); `bench/portable_vs_native.py` runs `<dir>/probbit`
   without `.exe` (the harness adds extensionless copies on Windows); both print a hard-coded "Apple M4" header line.
7. **npm 11.17 warns about install scripts** (`npm warn allow-scripts ... not yet covered by allowScripts`). They still
   ran; if a future npm blocks them, the wrapper fetches the binary on first run (tested with `--ignore-scripts`).
