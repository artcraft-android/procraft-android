<#
.SYNOPSIS
  Packages the desktop app for Windows x64.
.DESCRIPTION
  Writes, into $env:DIST (default: dist\):
    procraft-<version>-windows-x64.msi            the installer (WiX v5), for every user
    procraft-<version>-windows-x64-portable.zip   ProCraft.exe and the licences, to run anywhere
  The C runtime is linked in (+crt-static), so nothing else needs installing. The version is
  $env:VERSION, else procraft.version in android\gradle.properties (the one a release is tagged
  with). Needs Rust (MSVC) and WiX v5 with its UI and Util extensions ($env:WIX can name another
  wix.exe):
    dotnet tool install --global wix --version 5.0.2
    wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2
  Modelled on SolidCraft's packaging (tangency-labs/solidcraft).
.EXAMPLE
  pwsh packaging/windows/package.ps1
  pwsh packaging/windows/package.ps1 -SkipBuild
#>
param([switch] $SkipBuild)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

$Version = $env:VERSION
if (-not $Version) {
  $line = Get-Content (Join-Path $Root 'android\gradle.properties') | Where-Object { $_ -match '^procraft\.version=' } | Select-Object -First 1
  $Version = ($line -replace '^procraft\.version=', '').Trim()
}
if (-not $Version) { throw 'no version: set VERSION or procraft.version in android\gradle.properties' }
# the MSI's own version is numbers only: a beta's suffix is left out
$MsiVersion = ($Version -split '-')[0]
$Target = 'x86_64-pc-windows-msvc'
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
$Wix = if ($env:WIX) { $env:WIX } else { 'wix' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Write-Output "ProCraft $Version for Windows x64"

if (-not $SkipBuild) {
  # the C runtime linked in; only for this target, so build scripts and proc-macros are as ever
  $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
  Invoke-Native 'cargo build' { cargo build --release --locked -p procraft-desktop --target $Target }
}
$Exe = Join-Path $TargetDir "$Target\release\procraft.exe"

# the .exe: for x64, a windowed program (no console), and needing no Visual C++ runtime
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$pe = [BitConverter]::ToInt32($bytes, 0x3C)
$machine = [BitConverter]::ToUInt16($bytes, $pe + 4)
$subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C)
if ($machine -ne 0x8664) { throw ('procraft.exe is for machine 0x{0:X}, not x64' -f $machine) }
if ($subsystem -ne 2) { throw "procraft.exe has PE subsystem $subsystem, not 2 (a window)" }
if ([System.Text.Encoding]::ASCII.GetString($bytes).Contains('VCRUNTIME140.dll')) {
  throw 'procraft.exe needs VCRUNTIME140.dll: the C runtime should be linked in (+crt-static)'
}
Write-Output 'ok procraft.exe: x64, a window, no Visual C++ runtime needed'

$Stage = Join-Path $TargetDir 'windows-package\stage'
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item $Exe (Join-Path $Stage 'ProCraft.exe')
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md') { Copy-Item (Join-Path $Root $f) $Stage }

# ---- MSI ----------------------------------------------------------------------------------------
$Msi = Join-Path $Dist "procraft-$Version-windows-x64.msi"
Invoke-Native 'wix build' {
  & $Wix build (Join-Path $PSScriptRoot 'procraft.wxs') -arch x64 `
    -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext -culture en-US `
    -d "Version=$MsiVersion" -d "DisplayVersion=$Version" -d "BinDir=$Stage" `
    -d "IconPath=$(Join-Path $Root 'assets\app-icon\procraft.ico')" -o $Msi
}
Invoke-Native 'wix msi validate' {
  & $Wix msi validate $Msi -intermediateFolder (Join-Path $TargetDir 'windows-package\msi-validation')
}
# its debug symbols (.wixpdb) aren't for the release
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))

# ---- portable zip -------------------------------------------------------------------------------
$Name = "procraft-$Version-windows-x64-portable"
$Portable = Join-Path $TargetDir "windows-package\$Name"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
Copy-Item -Recurse $Stage $Portable
$Zip = Join-Path $Dist "$Name.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

Get-Item $Msi, $Zip | Format-Table Name, Length
