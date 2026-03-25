<#
.SYNOPSIS
  SmartSorter 发布脚本 — 编译可执行文件、同步版本号、提交代码、打 Git Tag

.DESCRIPTION
  用法：.\scripts\release.ps1 -Version "1.1.0" [-Message "发布说明"] [-Push] [-SkipBuild]
  
  执行步骤：
  1. 更新 package.json、Cargo.toml、tauri.conf.json 中的版本号
  2. 编译前端 + Tauri 生产版本
  3. git add + commit
  4. 创建 git tag (v1.1.0)
  5. 可选推送到远程（push tag 后 GitHub Actions 会自动创建 Release 并上传产物）

.PARAMETER Version
  目标版本号，如 "1.1.0"（不带 v 前缀）

.PARAMETER Message
  Tag 说明 / Commit message（可选，默认 "release: vX.X.X"）

.PARAMETER Push
  是否自动推送到远程（默认 $false）

.PARAMETER SkipBuild
  跳过编译步骤（仅更新版本号和打 tag）
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,

    [string]$Message = "",

    [switch]$Push = $false,

    [switch]$SkipBuild = $false
)

$ErrorActionPreference = "Stop"

# 校验版本号格式
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    Write-Error "版本号格式不正确，应为 X.Y.Z（如 1.1.0）"
    exit 1
}

if (-not $Message) {
    $Message = "release: v$Version"
}

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not (Test-Path "$root\package.json")) {
    $root = Split-Path -Parent $PSScriptRoot
}
if (-not (Test-Path "$root\package.json")) {
    $root = $PSScriptRoot
}
# 兜底：脚本可能直接在项目根目录运行
if (-not (Test-Path "$root\package.json")) {
    $root = Get-Location
}

Write-Host "=== SmartSorter Release v$Version ===" -ForegroundColor Cyan
Write-Host "项目根目录: $root"

# ---- 1. 更新 package.json ----
Write-Host "`n[1/5] 更新 package.json ..." -ForegroundColor Yellow
$pkgPath = Join-Path $root "package.json"
$pkg = Get-Content $pkgPath -Raw
$pkg = $pkg -replace '"version"\s*:\s*"[^"]*"', "`"version`": `"$Version`""
Set-Content -Path $pkgPath -Value $pkg -NoNewline -Encoding UTF8

# ---- 2. 更新 Cargo.toml ----
Write-Host "[2/5] 更新 Cargo.toml ..." -ForegroundColor Yellow
$cargoPath = Join-Path $root "src-tauri\Cargo.toml"
$cargo = Get-Content $cargoPath -Raw
$cargo = $cargo -replace '(?m)^version\s*=\s*"[^"]*"', "version = `"$Version`""
Set-Content -Path $cargoPath -Value $cargo -NoNewline -Encoding UTF8

# ---- 3. 更新 tauri.conf.json ----
Write-Host "[3/5] 更新 tauri.conf.json ..." -ForegroundColor Yellow
$tauriPath = Join-Path $root "src-tauri\tauri.conf.json"
$tauri = Get-Content $tauriPath -Raw
$tauri = $tauri -replace '"version"\s*:\s*"[^"]*"', "`"version`": `"$Version`""
Set-Content -Path $tauriPath -Value $tauri -NoNewline -Encoding UTF8

# ---- 4. 编译 ----
if (-not $SkipBuild) {
    Write-Host "`n[4/7] 编译前端 ..." -ForegroundColor Yellow
    Push-Location $root
    pnpm build
    if ($LASTEXITCODE -ne 0) { Write-Error "前端编译失败"; exit 1 }

    Write-Host "[5/7] 编译 Tauri 生产版本 ..." -ForegroundColor Yellow
    Push-Location (Join-Path $root "src-tauri")
    cargo tauri build
    Pop-Location
    if ($LASTEXITCODE -ne 0) { Write-Error "Tauri 编译失败"; exit 1 }
    Pop-Location

    # 查找产物
    $bundleDir = Join-Path $root "src-tauri\target\release\bundle"
    $msi = Get-ChildItem "$bundleDir\msi\*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1
    $nsis = Get-ChildItem "$bundleDir\nsis\*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    $exe = Join-Path $root "src-tauri\target\release\smart-sorter.exe"

    Write-Host "`n构建产物：" -ForegroundColor Green
    if (Test-Path $exe) { Write-Host "  EXE:  $exe" }
    if ($msi)  { Write-Host "  MSI:  $($msi.FullName)" }
    if ($nsis) { Write-Host "  NSIS: $($nsis.FullName)" }
} else {
    Write-Host "`n[4/7] 跳过编译 (SkipBuild)" -ForegroundColor DarkGray
}

# ---- 5. Git commit ----
Write-Host "`n[6/7] Git commit ..." -ForegroundColor Yellow
Push-Location $root
git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json CHANGELOG.md
git commit -m $Message

# ---- 6. Git tag ----
Write-Host "[7/7] 创建 Git Tag v$Version ..." -ForegroundColor Yellow
git tag -a "v$Version" -m $Message
Pop-Location

Write-Host "`n=== 完成! ===" -ForegroundColor Green
Write-Host "已创建 tag: v$Version"

if ($Push) {
    Write-Host "`n推送到远程 ..." -ForegroundColor Yellow
    Push-Location $root
    git push
    git push --tags
    Pop-Location
    Write-Host "推送完成!" -ForegroundColor Green
} else {
    Write-Host "`n提示：运行以下命令推送到远程：" -ForegroundColor Gray
    Write-Host "  git push && git push --tags"
}
