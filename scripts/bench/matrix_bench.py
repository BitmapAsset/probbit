#!/usr/bin/env python3
"""One row of BENCHMARK-MATRIX.md: machine facts, build facts and probbit timings for one OS/arch, written as one JSON file.

    python3 scripts/bench/matrix_bench.py --bin target/release/probbit --label linux-x86_64 --out bench-linux-x86_64.json \
        [--native-dir target-native/release] [--runs 5] [--attach KEY=FILE ...]

Stdlib only, Python >= 3.8, Linux / macOS / Windows. Run from the repo root after
`cargo build --release --locked --workspace` and `cargo build --release --locked -p probbit-core --example kernels`.
`--native-dir` names a `-C target-cpu=native` build of the same two targets; with it, bench/portable_vs_native.py runs too.

Every timing is N runs (default 5): median [IQR] (statistics.quantiles, n=4, as in bench/*.py) plus every raw value.
Wall clock is taken outside probbit (time.perf_counter around the processes); `probbit_ms` is what probbit reports itself.
Peak RSS of the 300-task decide: /usr/bin/time -l (macOS, "maximum resident set size", bytes), /usr/bin/time -v (Linux,
"Maximum resident set size", KiB), PeakWorkingSetSize from K32GetProcessMemoryInfo on the process handle (Windows; the
handle is still open after the wait). Absolute paths of this machine are scrubbed from the output.
"""
import argparse, json, os, platform, re, shutil, statistics, subprocess, sys, tempfile, time

KLINE = re.compile(r"^(.+?)\s*:\s*([0-9.]+e[+-]?[0-9]+|[0-9.]+) updates/s")
NUM = r"([0-9.]+(?:e[+-]?[0-9]+)?)"
PVN = re.compile(r"^(.+?): portable " + NUM + r" \[" + NUM + "-" + NUM + r"\] \| native " + NUM + r" \[" + NUM + "-" + NUM
                 + r"\] \| ratio ([0-9.]+)$")
DT = re.compile(r"^--threads (\d+): wall ms ([0-9.]+) \[([0-9.]+)-([0-9.]+)\], CPU ms ([0-9.]+), site updates/s ([0-9.]+) M;"
                r" answer identical: yes; verdict (\S+)$")
DD = re.compile(r"^default path \(--budget-ms 300, 4 chains, threads (\d+)\): wall ms ([0-9.]+) \[([0-9.]+)-([0-9.]+)\],"
                r" sweeps (\d+), verdict (\S+)$")
# BENCHMARKS.md §1 rows, from probbit-core/examples/kernels.rs
KERNEL_CELLS = {"kernel_multispin_fast_4t": "fast multispin splitmix x 4 thr", "kernel_multispin_fast_10t": "fast multispin splitmix x10 thr",
                "kernel_multispin_fast_1t": "fast T0 multispin, splitmix", "kernel_heatbath_fast_1t": "fast T1 f32 LUT, splitmix"}


def sh(args, input=None, env=None, timeout=900):
    try:
        p = subprocess.run(args, input=input, capture_output=True, text=True, env=env, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except Exception as e:  # tool missing, timeout
        return None, "", "%s: %s" % (type(e).__name__, e)


def summary(xs):
    xs = [x for x in xs if isinstance(x, (int, float))]
    if not xs:
        return None
    q = statistics.quantiles(xs, n=4) if len(xs) > 1 else [xs[0]] * 3
    return {"median": statistics.median(xs), "q1": q[0], "q3": q[2], "min": min(xs), "max": max(xs), "n": len(xs), "values": xs}


def load():
    up = sh(["uptime"])[1].strip() if shutil.which("uptime") else None
    return {"loadavg": list(os.getloadavg()) if hasattr(os, "getloadavg") else None, "uptime": up, "t": time.strftime("%H:%M:%S")}


def exe(path):
    return path + ".exe" if platform.system() == "Windows" and not path.endswith(".exe") else path


def machine():
    m = {"system": platform.system(), "os_release": platform.release(), "arch": platform.machine(),
         "python": platform.python_version(), "logical_cores": os.cpu_count()}
    s = m["system"]
    if s == "Darwin":
        rc, out, _ = sh(["sysctl", "-n", "machdep.cpu.brand_string", "hw.ncpu", "hw.memsize"])
        v = out.strip().splitlines()
        m["cpu_cmd"] = "sysctl -n machdep.cpu.brand_string hw.ncpu hw.memsize"
        m["cpu_model"] = v[0] if v else None
        m["hw_ncpu"] = int(v[1]) if len(v) > 1 else None
        m["memory_bytes"] = int(v[2]) if len(v) > 2 else None
        rc, out, _ = sh(["sysctl", "-n", "hw.perflevel0.logicalcpu", "hw.perflevel1.logicalcpu"])
        if rc == 0:
            v = out.split(); m["perf_cores"], m["efficiency_cores"] = int(v[0]), (int(v[1]) if len(v) > 1 else 0)
        rc, out, _ = sh(["sysctl", "-n", "machdep.cpu.features", "machdep.cpu.leaf7_features"])
        if rc == 0:
            f = set(out.lower().split()); m["cpu_flags"] = {k: k in f for k in ["sse4.2", "avx1.0", "avx2", "fma", "bmi2", "avx512f"]}
        rc, out, _ = sh(["sysctl", "-n", "sysctl.proc_translated"])
        m["rosetta_translated"] = out.strip() == "1" if rc == 0 else None
        m["os_version"] = sh(["sw_vers", "-productVersion"])[1].strip()
    elif s == "Linux":
        rc, out, _ = sh(["lscpu"])
        kv = dict((a.strip(), b.strip()) for a, b in (l.split(":", 1) for l in out.splitlines() if ":" in l))
        m["cpu_cmd"] = "lscpu"
        m["cpu_model"] = kv.get("Model name")
        m["lscpu"] = {k: kv.get(k) for k in ["Architecture", "CPU(s)", "Thread(s) per core", "Core(s) per socket", "Socket(s)",
                                              "Vendor ID", "Model name", "Hypervisor vendor", "L3"] if k in kv}
        f = set(kv.get("Flags", "").split())
        m["cpu_flags"] = {k: k in f for k in ["sse4_2", "avx", "avx2", "fma", "bmi2", "avx512f", "avx512bw", "avx512vl"]}
        try:
            with open("/proc/meminfo") as fh:
                m["memory_bytes"] = int(re.search(r"MemTotal:\s+(\d+) kB", fh.read()).group(1)) * 1024
            with open("/etc/os-release") as fh:
                m["os_version"] = re.search(r'PRETTY_NAME="([^"]*)"', fh.read()).group(1)
        except Exception as e:
            m["meminfo_error"] = str(e)
        m["libc"] = " ".join(platform.libc_ver())
    elif s == "Windows":
        ps = ("$p = Get-CimInstance Win32_Processor | Select-Object -First 1; $c = Get-CimInstance Win32_ComputerSystem; "
              "$o = Get-CimInstance Win32_OperatingSystem; [pscustomobject]@{Name = $p.Name; Cores = $p.NumberOfCores; "
              "Logical = $p.NumberOfLogicalProcessors; Mem = $c.TotalPhysicalMemory; Os = $o.Caption; Build = $o.BuildNumber} | ConvertTo-Json")
        rc, out, err = sh(["powershell", "-NoProfile", "-NonInteractive", "-Command", ps])
        m["cpu_cmd"] = "Get-CimInstance Win32_Processor"
        try:
            j = json.loads(out)
            m.update(cpu_model=(j["Name"] or "").strip(), physical_cores=j["Cores"], cim_logical=j["Logical"],
                     memory_bytes=int(j["Mem"]), os_version="%s (build %s)" % (j["Os"], j["Build"]))
        except Exception as e:
            m["cim_error"] = "%s %s" % (e, err[:300])
    return m


def runner():
    keys = ["RUNNER_OS", "RUNNER_ARCH", "ImageOS", "ImageVersion", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_JOB", "GITHUB_SHA",
            "GITHUB_REF_NAME", "GITHUB_WORKFLOW"]
    return {k: os.environ[k] for k in keys if k in os.environ} or None


def toolchain():
    rc, out, _ = sh(["rustc", "-vV"])
    t = dict((a.strip(), b.strip()) for a, b in (l.split(":", 1) for l in out.splitlines() if ":" in l))
    t["rustc"] = out.splitlines()[0] if out else None
    t["cargo"] = sh(["cargo", "-V"])[1].strip()
    return t


def linkage(bin_):
    s = platform.system(); r = {}
    if shutil.which("file"):
        r["file"] = sh(["file", "-b", bin_])[1].strip()
    if s == "Darwin":
        r["otool_L"] = sh(["otool", "-L", bin_])[1].strip().splitlines()[1:]
        r["lipo_archs"] = sh(["lipo", "-archs", bin_])[1].strip()
    elif s == "Linux":
        rc, out, err = sh(["ldd", bin_]); r["ldd"] = (out + err).strip().splitlines()
        rc, out, _ = sh(["objdump", "-T", bin_])
        vers = sorted(set(re.findall(r"GLIBC_([0-9.]+)", out)), key=lambda v: [int(x) for x in v.split(".")])
        r["glibc_symbol_versions"] = vers
        r["min_glibc"] = vers[-1] if vers else None
    elif s == "Windows":
        dump, how = shutil.which("dumpbin"), "PATH"
        if not dump:
            vsw = os.path.join(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"), "Microsoft Visual Studio", "Installer", "vswhere.exe")
            if os.path.exists(vsw):
                rc, out, _ = sh([vsw, "-latest", "-products", "*", "-find", r"VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe"])
                cand = [l.strip() for l in out.splitlines() if l.strip().lower().endswith("dumpbin.exe")]
                dump, how = (cand[0], "vswhere (not on PATH)") if cand else (None, None)
        r["dumpbin_found_via"] = how if dump else "not found: dumpbin is not on PATH and vswhere found none"
        if dump:
            rc, out, err = sh([dump, "/nologo", "/dependents", bin_])
            r["dll_imports"] = [l.strip() for l in out.splitlines() if l.strip().lower().endswith(".dll")]
            r["dumpbin_rc"] = rc
    return r


def run_json(args, input=None):
    rc, out, err = sh(args, input=input)
    try:
        return rc, json.loads(out), err
    except Exception:
        return rc, None, (err or "")[-500:] + " | stdout: " + (out or "")[:300]


def pipeline(bin_, demo_args, decide_args):
    t0 = time.perf_counter()
    p1 = subprocess.Popen([bin_, "demo"] + demo_args, stdout=subprocess.PIPE)
    p2 = subprocess.Popen([bin_, "decide"] + decide_args, stdin=p1.stdout, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    p1.stdout.close()
    out, err = p2.communicate()
    rc1 = p1.wait()
    wall = (time.perf_counter() - t0) * 1e3
    try:
        d = json.loads(out.decode("utf-8"))
    except Exception:
        d = None
    return wall, rc1, p2.returncode, d, err.decode("utf-8", "replace")[-300:]


def win_peak(handle):
    import ctypes
    from ctypes import wintypes

    class PMC(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [(n, ctypes.c_size_t) for n in (
            "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
            "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.K32GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(PMC), wintypes.DWORD]
    k32.K32GetProcessMemoryInfo.restype = wintypes.BOOL
    pmc = PMC(); pmc.cb = ctypes.sizeof(PMC)
    if not k32.K32GetProcessMemoryInfo(int(handle), ctypes.byref(pmc), pmc.cb):
        raise OSError("K32GetProcessMemoryInfo failed, error %d" % ctypes.get_last_error())
    return pmc.PeakWorkingSetSize, pmc.PeakPagefileUsage


def peak_rss(bin_, doc, args):
    """One run of `probbit decide args < doc` under the OS tool: (bytes, method, probbit's own peak_rss_mb, verdict, extra)."""
    s = platform.system(); extra = {}
    with open(doc, "rb") as f:
        if s == "Windows":
            p = subprocess.Popen([bin_, "decide"] + args, stdin=f, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            out, err = p.communicate()
            peak, peak_commit = win_peak(p._handle)
            extra["peak_pagefile_bytes"] = peak_commit
            method = "K32GetProcessMemoryInfo PeakWorkingSetSize (process handle, after exit)"
            val = peak
        elif s == "Darwin" or os.path.exists("/usr/bin/time"):
            flag = "-l" if s == "Darwin" else "-v"
            p = subprocess.run(["/usr/bin/time", flag, bin_, "decide"] + args, stdin=f, capture_output=True)
            out, err = p.stdout, p.stderr
            if s == "Darwin":
                m = re.search(rb"(\d+)\s+maximum resident set size", err); val = int(m.group(1)) if m else None
                fp = re.search(rb"(\d+)\s+peak memory footprint", err)
                if fp: extra["peak_memory_footprint_bytes"] = int(fp.group(1))
                method = "/usr/bin/time -l (maximum resident set size, bytes)"
            else:
                m = re.search(rb"Maximum resident set size \(kbytes\): (\d+)", err); val = int(m.group(1)) * 1024 if m else None
                method = "/usr/bin/time -v (Maximum resident set size, KiB x 1024)"
        else:
            p = subprocess.run([bin_, "decide"] + args, stdin=f, capture_output=True)
            out, val, method = p.stdout, None, "none: /usr/bin/time missing"
    try:
        d = json.loads(out.decode("utf-8"))
    except Exception:
        d = {}
    tel = d.get("telemetry") or {}
    return val, method, tel.get("peak_rss_mb"), d.get("verdict"), extra


def parse_kernels(text):
    r = {}
    for line in text.splitlines():
        m = KLINE.match(line)
        if m:
            r[m.group(1).strip()] = float(m.group(2))
        if line.startswith("chk"):
            r["chk"] = line.split()[1]
    return r


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bin", required=True); ap.add_argument("--label", required=True); ap.add_argument("--out", required=True)
    ap.add_argument("--native-dir"); ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--attach", action="append", default=[], help="KEY=FILE: embed FILE (JSON, else text) under attached.KEY")
    ap.add_argument("--note", action="append", default=[])
    a = ap.parse_args()
    bin_ = exe(a.bin); pdir = os.path.dirname(bin_) or "."
    N = a.runs; t_start = time.time()
    R = {"schema": "probbit-bench-matrix/1", "label": a.label, "date_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
         "git_sha": sh(["git", "rev-parse", "HEAD"])[1].strip() or None, "runner": runner(), "notes": a.note,
         "machine": machine(), "toolchain": toolchain(), "load": {"start": load()}, "errors": []}
    print("machine:", R["machine"].get("cpu_model"), "| logical cores", R["machine"]["logical_cores"], "| load", R["load"]["start"]["loadavg"], flush=True)

    R["binary"] = {"path": a.bin, "size_bytes": os.path.getsize(bin_), "version": sh([bin_, "version"])[1].strip(), "linkage": linkage(bin_)}

    # probbit stats: the IR self-test (400-spin ring, fixed work), 1 thread and default threads
    st = []
    for _ in range(N):
        rc, j, err = run_json([bin_, "stats"])
        if j is None:
            R["errors"].append("stats rc=%s %s" % (rc, err)); continue
        st.append(j)
    if st:
        R["stats_first"] = {k: st[0].get(k) for k in ("engine", "machine", "build", "controls")}
        R["stats"] = {"site_updates_per_s_1_thread": summary([j["self_test"]["site_updates_per_s_1_thread"] for j in st]),
                      "site_updates_per_s": summary([j["self_test"]["site_updates_per_s"] for j in st]),
                      "threads": sorted(set(j["self_test"]["threads"] for j in st)),
                      "speedup": summary([j["self_test"]["speedup"] for j in st])}
    R["load"]["after_stats"] = load()

    # probbit demo --tasks 12 | probbit decide, and probbit demo --tasks 300 | probbit decide --budget-ms 300
    for key, demo, dec in [("decide_12", ["--tasks", "12"], []), ("decide_300", ["--tasks", "300"], ["--budget-ms", "300"])]:
        runs = []
        for _ in range(N):
            wall, rc1, rc2, d, err = pipeline(bin_, demo, dec)
            d = d or {}; tel = d.get("telemetry") or {}
            runs.append({"wall_ms": wall, "demo_exit": rc1, "decide_exit": rc2, "probbit_ms": d.get("ms"), "verdict": d.get("verdict"),
                         "tier": d.get("tier"), "released": len(d.get("released") or []), "escalated": len(d.get("escalated") or []),
                         "violations": d.get("violations"), "sweeps": tel.get("sweeps"), "site_updates_per_s": tel.get("site_updates_per_s"),
                         "threads": tel.get("threads"), "peak_rss_mb_self": tel.get("peak_rss_mb"), "stderr": err or None})
        R[key] = {"cmd": "probbit demo %s | probbit decide %s" % (" ".join(demo), " ".join(dec)), "runs": runs,
                  "wall_ms": summary([r["wall_ms"] for r in runs]), "probbit_ms": summary([r["probbit_ms"] for r in runs]),
                  "released": summary([r["released"] for r in runs]), "violations_max": max([r["violations"] or 0 for r in runs]),
                  "verdicts": sorted(set(str(r["verdict"]) for r in runs)), "exit_codes": sorted(set(r["decide_exit"] for r in runs)),
                  "site_updates_per_s": summary([r["site_updates_per_s"] for r in runs])}
    R["load"]["after_decide"] = load()

    # peak RSS of the 300-task decide (3 runs; the document is generated once)
    tmp = tempfile.mkdtemp(prefix="probbit-bench-")
    doc = os.path.join(tmp, "demo300.json")
    with open(doc, "w") as f:
        f.write(sh([bin_, "demo", "--tasks", "300"])[1])
    rss = []
    for _ in range(3):
        try:
            rss.append(peak_rss(bin_, doc, ["--budget-ms", "300"]))
        except Exception as e:
            R["errors"].append("peak_rss: %s: %s" % (type(e).__name__, e))
    if rss:
        R["peak_rss_300"] = {"cmd": "probbit decide --budget-ms 300 < demo300.json", "method": rss[0][1],
                             "bytes": summary([r[0] for r in rss]), "self_reported_peak_rss_mb": [r[2] for r in rss],
                             "verdicts": [r[3] for r in rss], "extra": [r[4] for r in rss]}

    # lattice kernels (probbit-core/examples/kernels.rs), portable build, N runs
    kbin = exe(os.path.join(pdir, "examples", "kernels"))
    if os.path.exists(kbin):
        ks = []
        for _ in range(N):
            rc, out, err = sh([kbin])
            if rc == 0:
                ks.append(parse_kernels(out))
            else:
                R["errors"].append("kernels rc=%s %s" % (rc, err[-300:]))
        if ks:
            R["kernels"] = {"cmd": os.path.relpath(kbin).replace(os.sep, "/"), "checksums": sorted(set(k.get("chk") for k in ks)),
                            "lines": {name: summary([k.get(name) for k in ks]) for name in ks[0] if name != "chk"}}
    else:
        R["kernels"] = {"skipped": "no kernels example at %s (build it with -p probbit-core --example kernels)" % os.path.relpath(kbin)}
    R["load"]["after_kernels"] = load()

    # bench/portable_vs_native.py (unchanged; its header line names the M4 whatever the machine)
    if a.native_dir:
        if platform.system() == "Windows":
            # the script runs <dir>/probbit and <dir>/examples/kernels; CreateProcess appends no .exe to a name with a path
            R["windows_shim"] = []
            for d in (pdir, a.native_dir):
                for name in ("probbit", os.path.join("examples", "kernels")):
                    src, dst = os.path.join(d, name + ".exe"), os.path.join(d, name)
                    if os.path.exists(src) and not os.path.exists(dst):
                        shutil.copyfile(src, dst); R["windows_shim"].append(os.path.relpath(dst).replace(os.sep, "/"))
        env = dict(os.environ, PROBBIT_DIR=pdir, PROBBIT_NATIVE_DIR=a.native_dir)
        rc, out, err = sh([sys.executable, "bench/portable_vs_native.py"], env=env, timeout=1800)
        rows = {}
        for line in out.splitlines():
            m = PVN.match(line)
            if m:
                g = m.groups()
                rows[g[0]] = {"portable": float(g[1]), "portable_iqr": [float(g[2]), float(g[3])], "native": float(g[4]),
                              "native_iqr": [float(g[5]), float(g[6])], "ratio": float(g[7])}
        R["portable_vs_native"] = {"cmd": "PROBBIT_DIR=%s PROBBIT_NATIVE_DIR=%s python3 bench/portable_vs_native.py" % (
            os.path.relpath(pdir).replace(os.sep, "/"), os.path.relpath(a.native_dir).replace(os.sep, "/")), "exit": rc,
            "rows": rows, "stdout": out.splitlines(), "stderr_tail": err[-1500:] or None,
            "note": "the script's first line is hard-coded to the M4; the machine is the one in `machine`"}
    else:
        R["portable_vs_native"] = {"skipped": "no --native-dir"}
    R["load"]["after_pvn"] = load()

    # bench/decide_threads.py (unchanged; same hard-coded header)
    rc, out, err = sh([sys.executable, "bench/decide_threads.py"], env=dict(os.environ, PROBBIT=bin_), timeout=1800)
    th = {}
    for line in out.splitlines():
        m = DT.match(line)
        if m:
            g = m.groups(); th["threads_%s" % g[0]] = {"wall_ms": float(g[1]), "wall_iqr": [float(g[2]), float(g[3])], "cpu_ms": float(g[4]),
                                                       "site_updates_per_s_M": float(g[5]), "verdict": g[6]}
        m = DD.match(line)
        if m:
            g = m.groups(); th["default"] = {"threads": int(g[0]), "wall_ms": float(g[1]), "wall_iqr": [float(g[2]), float(g[3])],
                                             "sweeps": int(g[4]), "verdict": g[5]}
    R["decide_threads"] = {"cmd": "PROBBIT=%s python3 bench/decide_threads.py" % a.bin, "exit": rc, "rows": th, "stdout": out.splitlines(),
                           "stderr_tail": err[-1500:] or None}
    R["load"]["end"] = load()

    for kv in a.attach:
        k, _, path = kv.partition("=")
        try:
            with open(path) as f:
                txt = f.read()
            try:
                R.setdefault("attached", {})[k] = json.loads(txt)
            except ValueError:
                R.setdefault("attached", {})[k] = txt.splitlines()
        except OSError as e:
            R.setdefault("attached", {})[k] = "missing: %s" % e

    # the cells BENCHMARK-MATRIX.md shows (each one also lives in full above)
    C = {"cpu_model": R["machine"].get("cpu_model"), "logical_cores": R["machine"]["logical_cores"],
         "binary_size_bytes": R["binary"]["size_bytes"]}
    if "stats" in R:
        C["stats_1t"] = R["stats"]["site_updates_per_s_1_thread"]; C["stats_default"] = R["stats"]["site_updates_per_s"]
        C["stats_default_threads"] = R["stats"]["threads"]
    for name, line in KERNEL_CELLS.items():
        C[name] = (R.get("kernels", {}).get("lines") or {}).get(line)
    for key in ("decide_12", "decide_300"):
        C[key + "_wall_ms"] = R[key]["wall_ms"]; C[key + "_probbit_ms"] = R[key]["probbit_ms"]; C[key + "_verdicts"] = R[key]["verdicts"]
        C[key + "_exit_codes"] = R[key]["exit_codes"]
    C["decide_300_released"] = R["decide_300"]["released"]; C["decide_300_violations_max"] = R["decide_300"]["violations_max"]
    C["decide_300_site_updates_per_s"] = R["decide_300"]["site_updates_per_s"]
    if "peak_rss_300" in R:
        C["decide_300_peak_rss_bytes"] = R["peak_rss_300"]["bytes"]; C["decide_300_peak_rss_method"] = R["peak_rss_300"]["method"]
    pv = R["portable_vs_native"].get("rows") or {}
    C["native_over_portable"] = {k: v["ratio"] for k, v in pv.items()} or R["portable_vs_native"].get("skipped") or "n/a: exit %s" % R["portable_vs_native"].get("exit")
    C["threads_rows"] = th or "n/a: exit %s, %s" % (rc, (err.strip().splitlines() or [""])[-1][:200])
    R["cells"] = C
    R["elapsed_s"] = round(time.time() - t_start, 1)

    text = json.dumps(R, indent=1)
    home = os.path.expanduser("~")
    for p, rep in ((os.getcwd(), "."), (home, "~")):  # no absolute paths of this machine in the output
        if len(p) > 3:
            text = text.replace(json.dumps(p)[1:-1], rep).replace(p, rep)
    with open(a.out, "w") as f:
        f.write(text + "\n")
    print(json.dumps(C, indent=1))
    print("wrote", a.out, "in", R["elapsed_s"], "s;", len(R["errors"]), "errors", flush=True)


if __name__ == "__main__":
    main()
