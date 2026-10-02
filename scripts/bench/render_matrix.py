#!/usr/bin/env python3
"""Render the BENCHMARK-MATRIX.md tables from the JSON files in a results directory (every cell comes from one of them).

    python3 scripts/bench/render_matrix.py scripts/bench/results/2026-10-01 > /tmp/tables.md
"""
import json, os, sys

D = sys.argv[1]
RUN1 = "run-36969399069/"
ROWS = [  # (row label, bench JSON, test JSON or None, key inside cross-linux-x86_64.json "bench" when nested); paths relative to D
    ("macOS arm64, Apple M4 (local)", "local-m4/bench-macos-arm64-m4-local.json", None, None),
    ("Linux x86_64 (`ubuntu-latest`)", RUN1 + "bench-linux-x86_64.json", RUN1 + "test-linux-x86_64.json", None),
    ("Linux x86_64, static musl (`ubuntu-latest`)", RUN1 + "cross-linux-x86_64.json", None, "linux-x86_64-musl"),
    ("Linux x86_64, glibc, same VM as musl", RUN1 + "cross-linux-x86_64.json", None, "linux-x86_64-gnu-samevm"),
    ("macOS arm64 (`macos-latest`)", RUN1 + "bench-macos-arm64.json", RUN1 + "test-macos-arm64.json", None),
    ("Windows x86_64 (`windows-latest`)", RUN1 + "bench-windows-x86_64.json", RUN1 + "test-windows-x86_64.json", None),
    ("macOS x86_64 (`macos-15-intel`)", RUN1 + "bench-macos-x86_64.json", RUN1 + "test-macos-x86_64.json", None),
]
PENDING = ["Linux x86_64, WSL2 on a Windows desktop with an RTX 4070", "Linux x86_64, VirtualBox VM (CPU only)"]


def load(name, key=None):
    p = os.path.join(D, name)
    if not os.path.exists(p):
        return None
    j = json.load(open(p))
    return j.get("bench", {}).get(key) if key else j


def e(x):  # 2.227e+07 -> 2.23e7
    if x is None:
        return "n/a"
    m, _, ex = ("%.3g" % x).partition("e")
    return m + ("e" + str(int(ex)) if ex else "")


def s(v, f=e):
    if not isinstance(v, dict):
        return "n/a"
    return "%s [%s-%s]" % (f(v["median"]), f(v["q1"]), f(v["q3"]))


def ms(x):
    return "%.1f" % x


def mib(x):
    return "%.1f" % (x / 1048576)


def table(title, head, rows):
    out = ["#### " + title, "", "| platform | " + " | ".join(head) + " |", "|---|" + "---|" * len(head)]
    out += ["| %s | %s |" % (r[0], " | ".join(r[1:])) for r in rows]
    out += ["| %s | %s |" % (p, " | ".join(["pending: machine offline"] + [""] * (len(head) - 1))) for p in PENDING]
    return "\n".join(out) + "\n"


data = [(lab, load(b, k), load(t) if t else None, b if not k else "%s → bench.%s" % (b, k)) for lab, b, t, k in ROWS]
data = [d for d in data if d[1]]

rows = []
for lab, b, t, src in data:
    m, tc = b["machine"], b["toolchain"]
    mem = "%.0f GiB" % (m["memory_bytes"] / 2 ** 30) if m.get("memory_bytes") else "n/a"
    cores = str(m["logical_cores"]) + (" (%dP+%dE)" % (m["perf_cores"], m["efficiency_cores"]) if m.get("efficiency_cores") else "")
    build = next((n.split("=", 1)[1] + " s" for n in b.get("notes", []) if n.startswith("build_s=")), "n/a")
    if t:
        tests = "%s: %d passed, %d failed, %d ignored, %d s" % ("pass" if t["passed"] else "FAIL", t["totals"]["passed"],
                                                               t["totals"]["failed"], t["totals"]["ignored"], t["seconds"])
    else:
        tests = "not run in this job"
    rows.append([lab, m.get("cpu_model") or "n/a", cores, mem, m.get("os_version") or m.get("os_release") or "n/a",
                 tc.get("release", "n/a"), build, "%.2f MB" % (b["binary"]["size_bytes"] / 1e6), tests, "`%s`" % src])
print(table("Machines, build and tests", ["CPU", "logical cores", "memory", "OS", "rustc", "release build", "binary", "cargo test --release", "source"], rows))

rows = []
for lab, b, t, src in data:
    c = b["cells"]
    rows.append([lab, s(c.get("stats_1t")), s(c.get("stats_default")), ", ".join(map(str, c.get("stats_default_threads", [])))])
print(table("`pbit stats` self-test: 400-spin ring, IR sampler, site updates/s (N = 5)", ["1 thread", "default threads", "threads (= min(cores, 4))"], rows))

rows = []
for lab, b, t, src in data:
    c = b["cells"]; r = c.get("native_over_portable")
    if isinstance(r, dict):
        nat = "%.2f / %.2f / %.2f" % (r.get("fast T0 multispin, splitmix", 0), r.get("fast multispin splitmix x 4 thr", 0), r.get("fast T1 f32 LUT, splitmix", 0))
    else:
        nat = "n/a: " + str(r)
    rows.append([lab, s(c.get("kernel_multispin_fast_1t")), s(c.get("kernel_multispin_fast_4t")), s(c.get("kernel_multispin_fast_10t")),
                 s(c.get("kernel_heatbath_fast_1t")), nat])
print(table("Lattice kernels (`pbit-core/examples/kernels`, portable build), updates/s (N = 5)",
            ["multispin fast, 1 thread", "multispin fast, 4 threads", "multispin fast, 10 threads", "heat-bath f32 fast, 1 thread",
             "native / portable (multispin 1t / 4t / heat-bath)"], rows))

rows = []
for lab, b, t, src in data:
    c = b["cells"]
    rows.append([lab, s(c.get("decide_12_wall_ms"), ms), s(c.get("decide_12_pbit_ms"), ms), ", ".join(c.get("decide_12_verdicts", [])),
                 ", ".join(map(str, c.get("decide_12_exit_codes", [])))])
print(table("`pbit demo --tasks 12 | pbit decide`, ms (N = 5)", ["wall, both processes", "pbit's own `ms`", "verdict", "exit"], rows))

rows = []
for lab, b, t, src in data:
    c = b["cells"]; rel = c.get("decide_300_released") or {}
    rows.append([lab, s(c.get("decide_300_wall_ms"), ms), s(c.get("decide_300_pbit_ms"), ms),
                 "%d-%d of 300" % (rel.get("min", 0), rel.get("max", 0)) if rel else "n/a", str(c.get("decide_300_violations_max")),
                 ", ".join(c.get("decide_300_verdicts", [])), s(c.get("decide_300_site_updates_per_s"))])
print(table("`pbit demo --tasks 300 | pbit decide --budget-ms 300`, ms (N = 5)",
            ["wall, both processes", "pbit's own `ms`", "released (min-max)", "violations (max)", "verdict", "sampler site updates/s"], rows))

rows = []
for lab, b, t, src in data:
    c = b["cells"]; v = c.get("decide_300_peak_rss_bytes")
    selfr = [x for x in b.get("peak_rss_300", {}).get("self_reported_peak_rss_mb", []) if x is not None]
    ps = (b.get("attached") or {}).get("peak_rss_powershell")
    psv = ", ".join(mib(r["peak_working_set_bytes"]) for r in ps["runs"]) if isinstance(ps, dict) else ""
    rows.append([lab, s(v, mib) if v else "n/a", c.get("decide_300_peak_rss_method", "n/a"),
                 ", ".join("%.1f" % x for x in selfr) or "n/a (Windows: `sys::usage()` is Unix-only)", psv or "-"])
print(table("Peak memory of the 300-task `pbit decide --budget-ms 300`, MiB (N = 3)",
            ["peak RSS", "how", "pbit's own `peak_rss_mb`", "Get-Process polling (Windows, lower bound)"], rows))

rows = []
for lab, b, t, src in data:
    th = b["cells"].get("threads_rows")
    if isinstance(th, dict) and th:
        rows.append([lab] + ["%.1f [%.1f-%.1f]" % (th[k]["wall_ms"], th[k]["wall_iqr"][0], th[k]["wall_iqr"][1]) if k in th else "n/a"
                             for k in ("threads_1", "threads_2", "threads_4")] + [", ".join(sorted({th[k]["verdict"] for k in th if k.startswith("threads_")}))])
    else:
        rows.append([lab, "n/a: " + str(th)[:120], "", "", ""])
print(table("`bench/decide_threads.py`: 300-task decide, fixed work (4 chains x 400 sweeps, polish 0), wall ms (N = 5)",
            ["--threads 1", "--threads 2", "--threads 4", "verdict"], rows))
