# The PowerShell recipes of docs/agents.md, run as written, plus one call per exit code. `probbit` must be on PATH.
#   pwsh -File scripts/bench/callers.ps1      powershell -ExecutionPolicy Bypass -File scripts/bench/callers.ps1
# (No 2>$null on native calls: Windows PowerShell 5.1 turns redirected stderr into errors.)
$ErrorActionPreference = 'Continue'
"PowerShell $($PSVersionTable.PSVersion) ($($PSVersionTable.PSEdition)), probbit at $((Get-Command probbit).Source)"
"`$OutputEncoding $($OutputEncoding.WebName) ($($OutputEncoding.GetPreamble().Length)-byte preamble), " +
"[Console]::InputEncoding $([Console]::InputEncoding.WebName) ($([Console]::InputEncoding.GetPreamble().Length)-byte preamble)"
# Windows PowerShell 5.1 adds a UTF-8 byte-order mark to text it pipes into a native program (runs 36969399069 and
# 36970631022, with $OutputEncoding us-ascii) and probbit 0.2.0 rejects it (exit 2). docs/agents.md: let cmd.exe pipe.
$desktop = $PSVersionTable.PSEdition -ne 'Core'
if ($desktop) {
    probbit demo --tasks 12 | probbit decide | Out-Null
    "as is: probbit demo --tasks 12 | probbit decide -> exit $LASTEXITCODE (2 = the byte-order mark was rejected)"
}

if ($desktop) {
    '> cmd /c "probbit demo --tasks 12 | probbit decide" | ConvertFrom-Json | Select-Object verdict, violations, ms'
    cmd /c "probbit demo --tasks 12 | probbit decide" | ConvertFrom-Json | Select-Object verdict, violations, ms | Format-List | Out-String -Width 200
} else {
    '> probbit demo --tasks 12 | probbit decide | ConvertFrom-Json | Select-Object verdict, violations, ms'
    probbit demo --tasks 12 | probbit decide | ConvertFrom-Json | Select-Object verdict, violations, ms | Format-List | Out-String -Width 200
}
if ($LASTEXITCODE -ne 0) { throw "demo pipeline: exit $LASTEXITCODE" }

probbit demo --tasks 24 | Set-Content -Encoding ascii router.json
if ($desktop) {
    '> cmd /c "probbit decide --budget-ms 200 < router.json" | ConvertFrom-Json; $LASTEXITCODE'
    $d = cmd /c "probbit decide --budget-ms 200 < router.json" | ConvertFrom-Json
} else {
    '> Get-Content router.json -Raw | probbit decide --budget-ms 200 | ConvertFrom-Json; $LASTEXITCODE'
    $d = Get-Content router.json -Raw | probbit decide --budget-ms 200 | ConvertFrom-Json
}
$code = $LASTEXITCODE
"verdict $($d.verdict), released $(@($d.released).Count) of $($d.tasks), violations $($d.violations); exit $code"
if ($code -ne 0) { throw "router.json: exit $code" }

'> one call per exit code (infeasible, bad input, declined)'
'{"workers":[{"id":"a","cap":1}],"tasks":[{"id":"t1","allowed":["a"],"scores":{"a":1}},{"id":"t2","allowed":["a"],"scores":{"a":1}}]}' | Set-Content -Encoding ascii infeasible.json
'{"workers": 1}' | Set-Content -Encoding ascii bad.json
if ($desktop) {
    cmd /c "probbit decide < infeasible.json" | Out-Null; $r1 = $LASTEXITCODE
    cmd /c "probbit decide < bad.json" | Out-Null; $r2 = $LASTEXITCODE
    cmd /c "probbit run --op exact --exact-ms 50 < examples\denoise-8x12.json" | Out-Null; $r3 = $LASTEXITCODE
} else {
    Get-Content infeasible.json -Raw | probbit decide | Out-Null; $r1 = $LASTEXITCODE
    Get-Content bad.json -Raw | probbit decide | Out-Null; $r2 = $LASTEXITCODE
    Get-Content examples/denoise-8x12.json -Raw | probbit run --op exact --exact-ms 50 | Out-Null; $r3 = $LASTEXITCODE
}
"exit codes: infeasible $r1 (want 1), bad input $r2 (want 2), declined $r3 (want 3)"
if ($r1 -ne 1 -or $r2 -ne 2 -or $r3 -ne 3) { throw 'unexpected exit codes' }

if ($desktop) {
    # informational: does a BOM-free console input encoding stop 5.1 from adding the mark?
    try {
        [Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false)
        probbit demo --tasks 12 | probbit decide | Out-Null
        "probe (informational): [Console]::InputEncoding = UTF8Encoding(`$false), then probbit demo --tasks 12 | probbit decide -> exit $LASTEXITCODE"
    } catch {
        "probe (informational): [Console]::InputEncoding cannot be set here: $($_.Exception.Message)"
    }
}
'PowerShell recipes: all passed'
