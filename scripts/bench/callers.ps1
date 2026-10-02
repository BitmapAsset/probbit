# The PowerShell recipes of docs/agents.md, run as written, plus one call per exit code. `pbit` must be on PATH.
#   pwsh -File scripts/bench/callers.ps1      powershell -ExecutionPolicy Bypass -File scripts/bench/callers.ps1
# (No 2>$null on native calls: Windows PowerShell 5.1 turns redirected stderr into errors.)
$ErrorActionPreference = 'Continue'
"PowerShell $($PSVersionTable.PSVersion) ($($PSVersionTable.PSEdition)), pbit at $((Get-Command pbit).Source)"

'> pbit demo --tasks 12 | pbit decide | ConvertFrom-Json | Select-Object verdict, violations, ms'
pbit demo --tasks 12 | pbit decide | ConvertFrom-Json | Select-Object verdict, violations, ms | Format-List | Out-String -Width 200
if ($LASTEXITCODE -ne 0) { throw "demo pipeline: exit $LASTEXITCODE" }

pbit demo --tasks 24 | Set-Content -Encoding ascii router.json
'> Get-Content router.json -Raw | pbit decide --budget-ms 200 | ConvertFrom-Json; $LASTEXITCODE'
$d = Get-Content router.json -Raw | pbit decide --budget-ms 200 | ConvertFrom-Json
$code = $LASTEXITCODE
"verdict $($d.verdict), released $(@($d.released).Count) of $($d.tasks), violations $($d.violations); exit $code"
if ($code -ne 0) { throw "router.json: exit $code" }

'> one call per exit code (infeasible, bad input, declined)'
'{"workers":[{"id":"a","cap":1}],"tasks":[{"id":"t1","allowed":["a"],"scores":{"a":1}},{"id":"t2","allowed":["a"],"scores":{"a":1}}]}' | pbit decide | Out-Null
$r1 = $LASTEXITCODE
'{"workers": 1}' | pbit decide | Out-Null
$r2 = $LASTEXITCODE
Get-Content examples/denoise-8x12.json -Raw | pbit run --op exact --exact-ms 50 | Out-Null
$r3 = $LASTEXITCODE
"exit codes: infeasible $r1 (want 1), bad input $r2 (want 2), declined $r3 (want 3)"
if ($r1 -ne 1 -or $r2 -ne 2 -or $r3 -ne 3) { throw 'unexpected exit codes' }
'PowerShell recipes: all passed'
