# Peak working set of `pbit decide --budget-ms 300` on the 300-task demo, read with Get-Process every ~5 ms until the process
# exits: a lower bound (the last sample before exit). Cross-check of the exact value matrix_bench.py reads with
# K32GetProcessMemoryInfo on the process handle.
#   pwsh -File scripts/bench/peak_rss_windows.ps1 [-Exe target\release\pbit.exe] [-Out peak-rss-powershell.json] [-Runs 3]
param([string]$Exe = 'target\release\pbit.exe', [string]$Out = 'peak-rss-powershell.json', [int]$Runs = 3)
$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path -LiteralPath $Exe).Path
$doc = Join-Path ([IO.Path]::GetTempPath()) 'pbit-demo300.json'
& $exePath demo --tasks 300 | Set-Content -Encoding ascii -LiteralPath $doc
$rows = foreach ($i in 1..$Runs) {
    $o = Join-Path ([IO.Path]::GetTempPath()) "pbit-d300-$i.json"
    $p = Start-Process -FilePath $exePath -ArgumentList 'decide', '--budget-ms', '300' -RedirectStandardInput $doc `
        -RedirectStandardOutput $o -NoNewWindow -PassThru
    $peak = [long]0; $samples = 0
    while (-not $p.HasExited) {
        $g = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
        if ($g) { $samples++; if ($g.PeakWorkingSet64 -gt $peak) { $peak = $g.PeakWorkingSet64 } }
        Start-Sleep -Milliseconds 5
    }
    $verdict = (Get-Content -Raw -LiteralPath $o | ConvertFrom-Json).verdict
    [pscustomobject]@{ peak_working_set_bytes = $peak; samples = $samples; verdict = $verdict }
}
[pscustomobject]@{
    method = 'Get-Process PeakWorkingSet64, polled every ~5 ms until exit (lower bound)'
    cmd = 'pbit decide --budget-ms 300 < demo300.json'
    powershell = "$($PSVersionTable.PSVersion)"
    runs = @($rows)
} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $Out
Get-Content -LiteralPath $Out
