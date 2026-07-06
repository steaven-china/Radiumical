#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Unified build script for Radiumical workspace.
.DESCRIPTION
    Builds tui, rpc, and tauri packages and copies binaries to
    target/{profile}/{tui,rpc,tauri}/ for organized output.
.PARAMETER Profile
    Build profile: dev (default), release, release-small
.PARAMETER Target
    Build target triple (optional, uses host default if omitted)
.PARAMETER Package
    Build specific package(s): tui, rpc, tauri, all (default: all)
.PARAMETER Clean
    Clean output directories before building.
.EXAMPLE
    ./build.ps1
    ./build.ps1 -Profile release
    ./build.ps1 -Profile release-small -Target x86_64-pc-windows-msvc
    ./build.ps1 -Package tui,rpc -Profile release
#>

param(
    [ValidateSet("dev", "release", "release-small")]
    [string]$Profile = "dev",

    [string]$Target = "",

    [string[]]$Package = @("all"),

    [switch]$Clean
)

$ErrorActionPreference = "Stop"

# ── Resolve packages ────────────────────────────────────────────
$packages = @()
foreach ($p in $Package) {
    foreach ($split in $p.Split(",").Trim()) {
        if ($split -eq "all") {
            $packages = @("tui", "rpc", "tauri")
            break
        } else {
            $packages += $split
        }
    }
    if ($packages.Count -eq 3) { break }
}
Write-Host "Packages: $($packages -join ', ')" -ForegroundColor DarkGray

# ── Resolve profile dir name ────────────────────────────────────
$profileDir = switch ($Profile) {
    "dev"           { "debug" }
    "release"       { "release" }
    "release-small" { "release-small" }
}

# ── Resolve target triple ───────────────────────────────────────
$targetSubdir = ""
if ($Target) {
    $targetSubdir = $Target
}

# ── Output root ─────────────────────────────────────────────────
if ($targetSubdir) {
    $outRoot = "target/$targetSubdir/$profileDir"
} else {
    $outRoot = "target/$profileDir"
}

# ── Package → crate name mapping ────────────────────────────────
$crateMap = @{
    "tui"   = @{ crate = "radiumical";     bin = "radiumical"     }
    "rpc"   = @{ crate = "radiumical-rpc";  bin = "radiumical-rpc" }
    "tauri" = @{ crate = "radiumical-tauri"; bin = "radiumical-tauri" }
}

# ── Clean ───────────────────────────────────────────────────────
if ($Clean) {
    foreach ($pkg in $packages) {
        $outDir = "$outRoot/$pkg"
        if (Test-Path $outDir) {
            Write-Host "Cleaning $outDir" -ForegroundColor Yellow
            Remove-Item -Recurse -Force $outDir
        }
    }
}

# ── Build ───────────────────────────────────────────────────────
$nonTauri = $packages | Where-Object { $_ -ne "tauri" }
$hasTauri = $packages -contains "tauri"

# Build non-Tauri packages together
if ($nonTauri) {
    $pkgSpecs = @()
    foreach ($p in $nonTauri) {
        $pkgSpecs += "--package"
        $pkgSpecs += $crateMap[$p].crate
    }
    $cmd = @("cargo", "build") + $pkgSpecs
    if ($Profile -ne "dev") { $cmd += @("--profile", $Profile) }
    if ($Target) { $cmd += @("--target", $Target) }

    Write-Host "`n▸ $($cmd -join ' ')" -ForegroundColor Cyan
    & $cmd[0] $cmd[1..($cmd.Length-1)]

    if ($LASTEXITCODE -ne 0) {
        Write-Host "Build failed." -ForegroundColor Red
        exit 1
    }
}

# Build Tauri separately: ensure frontend dist exists and enable custom protocol for release builds
if ($hasTauri) {
    $tauriDir = "radiumical-tauri"
    if (Test-Path "$tauriDir/package.json") {
        Write-Host "`n▸ npm run build (Tauri frontend)" -ForegroundColor Cyan
        Push-Location $tauriDir
        try {
            npm run build | Out-Host
            if ($LASTEXITCODE -ne 0) {
                Write-Host "Tauri frontend build failed." -ForegroundColor Red
                exit 1
            }
        } finally {
            Pop-Location
        }
    }

    $cmd = @("cargo", "build", "--package", $crateMap["tauri"].crate)
    if ($Profile -ne "dev") {
        $cmd += @("--profile", $Profile)
        $cmd += @("--features", "custom-protocol")
    }
    if ($Target) { $cmd += @("--target", $Target) }

    Write-Host "`n▸ $($cmd -join ' ')" -ForegroundColor Cyan
    & $cmd[0] $cmd[1..($cmd.Length-1)]

    if ($LASTEXITCODE -ne 0) {
        Write-Host "Build failed." -ForegroundColor Red
        exit 1
    }
}

# ── Copy binaries ───────────────────────────────────────────────
foreach ($pkg in $packages) {
    $info = $crateMap[$pkg]
    $srcBase = "$outRoot/$($info.bin)"

    # Resolve extension
    $src = if ($IsWindows -or $env:OS -eq "Windows_NT") {
        "$srcBase.exe"
    } else {
        $srcBase
    }

    $dstDir = "$outRoot/$pkg"
    $dst = "$dstDir/$($info.bin)"

    if ($IsWindows -or $env:OS -eq "Windows_NT") {
        $dst = "$dst.exe"
    }

    if (!(Test-Path $src)) {
        Write-Host "⚠ Binary not found: $src" -ForegroundColor Yellow
        continue
    }

    New-Item -ItemType Directory -Path $dstDir -Force | Out-Null
    Copy-Item $src $dst -Force
    $size = [math]::Round((Get-Item $dst).Length / 1MB, 2)
    Write-Host "✔ $pkg → $dst ($size MB)" -ForegroundColor Green
}

Write-Host "`nDone. Output in: $outRoot/{tui,rpc,tauri}/" -ForegroundColor Cyan
