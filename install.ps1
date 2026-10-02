# pbit installer for Windows (Windows PowerShell 5.1 or PowerShell 7): fetches a prebuilt release archive, checks its SHA-256
# and installs pbit.exe. No administrator rights needed.
#
#   irm https://raw.githubusercontent.com/BitmapAsset/pbit/main/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/BitmapAsset/pbit/main/install.ps1))) -Version v0.4.0
#
# Parameters (each also read from the environment variable named after it):
#   -Version       PBIT_VERSION        release tag, e.g. v0.4.0 (default: the latest release)
#   -InstallDir    PBIT_INSTALL_DIR    where pbit.exe goes (default: $HOME\.local\bin)
#   -DownloadBase  PBIT_DOWNLOAD_BASE  the archive is fetched from <DownloadBase>/<tag>/pbit-<tag>-<target>.zip
#                                      (default: https://github.com/BitmapAsset/pbit/releases/download); needs -Version
#   -Target        PBIT_TARGET         Rust target triple to fetch instead of the detected one
#   -AddToPath     PBIT_ADD_TO_PATH=1  also add InstallDir to your user PATH (otherwise only this session's PATH)
param(
    [string]$Version = $env:PBIT_VERSION,
    [string]$InstallDir = $env:PBIT_INSTALL_DIR,
    [string]$DownloadBase = $env:PBIT_DOWNLOAD_BASE,
    [string]$Target = $env:PBIT_TARGET,
    [switch]$AddToPath = ($env:PBIT_ADD_TO_PATH -eq '1')
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue' # the progress bar makes Invoke-WebRequest many times slower on 5.1
$Repo = 'BitmapAsset/pbit'
if ($PSVersionTable.PSVersion.Major -lt 6) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
}

if (-not $Target) {
    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    switch ($arch) {
        'AMD64' { $Target = 'x86_64-pc-windows-msvc' }
        'ARM64' { $Target = 'x86_64-pc-windows-msvc'; Write-Host 'pbit install: no native arm64 build yet; installing the x86_64 build (Windows runs it under emulation)' }
        default { throw "pbit install: no prebuilt pbit for Windows on $arch" }
    }
}
$customBase = [bool]$DownloadBase
if (-not $DownloadBase) { $DownloadBase = "https://github.com/$Repo/releases/download" }
$DownloadBase = $DownloadBase.TrimEnd('/')
if (-not $Version) {
    if ($customBase) { throw 'pbit install: set -Version (or PBIT_VERSION), e.g. v0.4.0, together with -DownloadBase' }
    $Version = (Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$Repo/releases/latest").tag_name
    if (-not $Version) { throw 'pbit install: no published release found (set -Version)' }
}
if (-not $Version.StartsWith('v')) { $Version = "v$Version" }
if (-not $InstallDir) { $InstallDir = Join-Path $HOME '.local\bin' }

$name = "pbit-$Version-$Target"
$asset = "$name.zip"
$url = "$DownloadBase/$Version/$asset"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("pbit-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "pbit install: $Version for $Target"
    Write-Host "  fetching $url"
    $zip = Join-Path $tmp $asset
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zip
    Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256" -OutFile "$zip.sha256"
    $want = ((Get-Content -LiteralPath "$zip.sha256" -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
    $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToLowerInvariant()
    if ($want -notmatch '^[0-9a-f]{64}$') { throw "pbit install: unreadable checksum file $asset.sha256" }
    if ($want -ne $got) { throw "pbit install: checksum mismatch for ${asset}: expected $want, got $got (nothing was installed)" }
    Write-Host "  sha256 ok  $got"

    Expand-Archive -LiteralPath $zip -DestinationPath (Join-Path $tmp 'x') -Force
    $src = Join-Path $tmp "x\$name\pbit.exe"
    if (-not (Test-Path -LiteralPath $src)) { throw "pbit install: $asset has no $name\pbit.exe" }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $dest = Join-Path $InstallDir 'pbit.exe'
    Copy-Item -LiteralPath $src -Destination $dest -Force
    $ran = & $dest version
    if ($LASTEXITCODE -ne 0) { throw "pbit install: installed $dest, but it does not run here: $ran" }
    Write-Host "  installed  $dest ($ran)"
} finally {
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$full = (Resolve-Path -LiteralPath $InstallDir).Path.TrimEnd('\')
$onPath = { param($p) @($p -split ';' | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') }) -contains $full }
if (-not (& $onPath $env:Path)) { $env:Path = "$full;$env:Path" } # this session
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not (& $onPath $userPath)) {
    if ($AddToPath) {
        [Environment]::SetEnvironmentVariable('Path', ($(if ($userPath) { "$full;$userPath" } else { $full })), 'User')
        Write-Host "  added      $full to your user PATH (new terminals see it)"
    } else {
        Write-Host ''
        Write-Host "$full is on this session's PATH only. To keep it, run (or re-run this installer with -AddToPath):"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', '$full;' + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')"
    }
}
Write-Host ''
Write-Host 'Next:'
Write-Host '  pbit version'
# Windows PowerShell 5.1 adds a byte-order mark to text it pipes into a program: pbit 0.3.0 skips it (0.2.x rejected it:
# pipe through `cmd /c` there)
Write-Host '  pbit demo --tasks 12 | pbit decide --pretty'
Write-Host '  pbit stats --pretty'
