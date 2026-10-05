<#
.SYNOPSIS
  AlphaNotch installer for Windows 10/11: Coucou Lite (the notch island) + the YASB "adaptive island" bar.

.DESCRIPTION
  One command, run in PowerShell:

    irm https://raw.githubusercontent.com/xdukunx/coucou-lite/lite-native/install.ps1 | iex

  It FIRST analyses your PC (YASB, fonts, Rust/Visual Studio Build Tools, Claude Code, Antigravity, OpenCode...),
  shows the plan, and only then asks once before changing anything. Nothing is hidden: the whole script is this file.

  What it can do, depending on what it finds:
    - install or upgrade YASB (winget, AmN.yasb) and the JetBrainsMono Nerd Font it uses
    - install the Rust toolchain + Visual Studio Build Tools if they are missing (needed to BUILD Coucou Lite)
    - download the source from GitHub and build Coucou Lite on THIS PC (no prebuilt binary is downloaded)
    - install it to %LOCALAPPDATA%\Coucou and start it at every login
    - apply the AlphaNotch YASB layout (your old YASB config is backed up first)
    - add Coucou's hooks to Claude Code / Antigravity (shows what changes, backs up first, never touches other tools' hooks)

  With parameters (use a script block):
    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/xdukunx/coucou-lite/lite-native/install.ps1))) -DryRun

.PARAMETER DryRun       Analyse and print the plan, change nothing.
.PARAMETER Yes          Do not ask; accept the plan.
.PARAMETER Yasb         auto (default) | apply | keep | skip. apply = install/upgrade YASB and apply the layout; keep = leave YASB's config alone.
.PARAMETER NoHooks      Do not touch Claude Code / Antigravity hooks.
.PARAMETER NoAutostart  Do not start Coucou at login.
.PARAMETER NoStart      Do not start Coucou at the end.
.PARAMETER NoSettings   Do not write %APPDATA%\Coucou\settings.json.
.PARAMETER Name         The name the island greets you with. Asked during install when not given.
.PARAMETER InstallDir   Where Coucou Lite goes (default %LOCALAPPDATA%\Coucou).
.PARAMETER SourceDir    Use this local checkout (the folder that contains "windows" and "yasb") instead of downloading.
.PARAMETER Branch       Branch to download (default lite-native).
#>
[CmdletBinding()]
param(
    [switch]$DryRun,
    [switch]$Yes,
    [ValidateSet('auto', 'apply', 'keep', 'skip')][string]$Yasb = 'auto',
    [switch]$NoHooks,
    [switch]$NoAutostart,
    [switch]$NoStart,
    [switch]$NoSettings,
    [string]$Name = '',
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Coucou'),
    [string]$SourceDir = '',
    [string]$Branch = 'lite-native',
    [string]$Repo = 'xdukunx/coucou-lite'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 } catch {}

$LogFile = Join-Path $env:TEMP 'alphanotch-install.log'
try { Start-Transcript -Path $LogFile -Force | Out-Null } catch {}

$YasbMinVersion = [version]'2.0.7'
$ConfigDir = if ($env:YASB_CONFIG_HOME) { $env:YASB_CONFIG_HOME } else { Join-Path $env:USERPROFILE '.config\yasb' }
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'

# ---------------------------------------------------------------- output helpers

function Say($msg, $color = 'Gray') { Write-Host $msg -ForegroundColor $color }
function Step($msg) { Write-Host ''; Write-Host ('==> ' + $msg) -ForegroundColor Cyan }
function Ok($msg) { Write-Host ('    [ok] ' + $msg) -ForegroundColor Green }
function Warn($msg) { Write-Host ('    [!]  ' + $msg) -ForegroundColor Yellow }
function Fail($msg) { Write-Host ('    [x]  ' + $msg) -ForegroundColor Red }

function Ask([string]$question, [bool]$default = $true) {
    if ($Yes) { return $true }
    $hint = if ($default) { '[Y/n]' } else { '[y/N]' }
    while ($true) {
        $a = Read-Host ($question + ' ' + $hint)
        if ([string]::IsNullOrWhiteSpace($a)) { return $default }
        if ($a -match '^(y|ya|yes)$') { return $true }
        if ($a -match '^(n|no|tidak)$') { return $false }
    }
}

function Run([string]$file, [string[]]$arguments, [switch]$AllowFail) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $file @arguments
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $old }
    if ($code -ne 0 -and -not $AllowFail) { throw ("'" + $file + " " + ($arguments -join ' ') + "' exited with code " + $code) }
    return $code
}

function Refresh-Path {
    $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $env:Path = (($machine, $user) -join ';') + ';' + (Join-Path $env:USERPROFILE '.cargo\bin')
}

# ---------------------------------------------------------------- analysis

function Find-Yasb {
    $c = @("$env:ProgramFiles\YASB\yasb.exe")
    $g = Get-Command yasb.exe -ErrorAction SilentlyContinue
    if ($g) { $c += $g.Source }
    foreach ($key in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*') {
        Get-ItemProperty $key -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like 'YASB*' -and $_.InstallLocation } | ForEach-Object { $c += (Join-Path $_.InstallLocation 'yasb.exe') }
    }
    foreach ($p in $c) { if ($p -and (Test-Path $p)) { return $p } }
    return $null
}

function Find-Cargo {
    $g = Get-Command cargo.exe -ErrorAction SilentlyContinue
    if ($g) { return $g.Source }
    $p = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (Test-Path $p) { return $p }
    return $null
}

function Has-Msvc {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $false }
    $p = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    return [bool]$p
}

function Has-Font {
    foreach ($k in 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts', 'HKCU:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts') {
        $props = Get-ItemProperty $k -ErrorAction SilentlyContinue
        if ($props -and ($props.PSObject.Properties.Name | Where-Object { $_ -like '*JetBrainsMono*Nerd*' -or $_ -like '*JetBrainsMono*NF*' })) { return $true }
    }
    return [bool](Get-ChildItem "$env:LOCALAPPDATA\Microsoft\Windows\Fonts" -Filter 'JetBrainsMono*NF*' -ErrorAction SilentlyContinue | Select-Object -First 1)
}

function Analyse {
    $a = [ordered]@{}
    $os = Get-CimInstance Win32_OperatingSystem
    $a.OsName = $os.Caption
    $a.OsBuild = [int]$os.BuildNumber
    $a.Is64 = [Environment]::Is64BitOperatingSystem
    $a.IsAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    $a.Winget = [bool](Get-Command winget.exe -ErrorAction SilentlyContinue)

    $a.FreeGB = [math]::Round((Get-PSDrive -Name ($env:SystemDrive.TrimEnd(':'))).Free / 1GB, 1)

    $y = Find-Yasb
    $a.YasbPath = $y
    $a.YasbVersion = $null
    if ($y) { try { $a.YasbVersion = [version](Get-Item $y).VersionInfo.ProductVersion } catch { $a.YasbVersion = [version]'0.0.0' } }
    $a.YasbRunning = [bool](Get-Process yasb -ErrorAction SilentlyContinue)
    $a.YasbConfig = Test-Path (Join-Path $ConfigDir 'config.yaml')
    $a.YasbOurs = $false
    if ($a.YasbConfig) { $a.YasbOurs = [bool](Select-String -Path (Join-Path $ConfigDir 'config.yaml') -Pattern 'AlphaNotch YASB layout' -SimpleMatch -Quiet) }
    $a.Font = Has-Font

    $a.Cargo = Find-Cargo
    $a.Msvc = Has-Msvc

    $a.CoucouExe = Join-Path $InstallDir 'coucou-lite.exe'
    $a.CoucouInstalled = Test-Path $a.CoucouExe
    $a.CoucouRunning = [bool](Get-Process coucou-lite -ErrorAction SilentlyContinue)
    $a.TauriCoucou = [bool](Get-Process coucou -ErrorAction SilentlyContinue)

    $a.Claude = Test-Path (Join-Path $env:USERPROFILE '.claude')
    $a.Antigravity = (Test-Path (Join-Path $env:USERPROFILE '.gemini')) -or (Test-Path (Join-Path $env:LOCALAPPDATA 'Programs\Antigravity'))
    $a.OpenCode = [bool](Get-Command opencode -ErrorAction SilentlyContinue) -or (Test-Path (Join-Path $env:USERPROFILE '.config\opencode'))
    return $a
}

function Ask-Name {
    # The island greets you by name. Asked once; Enter keeps the Windows account name.
    if ($Name.Trim()) { return $Name.Trim() }
    $guess = $env:USERNAME
    if ($Yes) { return $guess }
    Write-Host ''
    $n = Read-Host ('Siapa namamu? (dipakai island untuk menyapa: "Selamat malam, <nama>") [Enter = ' + $guess + ']')
    if ([string]::IsNullOrWhiteSpace($n)) { return $guess }
    return $n.Trim()
}

function Show-Analysis($a) {
    Step 'Analisis PC kamu'
    $rows = @(
        @('Windows', ('{0} (build {1}){2}' -f $a.OsName, $a.OsBuild, $(if ($a.Is64) { '' } else { ' - BUKAN 64-bit' }))),
        @('winget', $(if ($a.Winget) { 'ada' } else { 'TIDAK ADA (dibutuhkan untuk memasang YASB/Rust)' })),
        @('Ruang kosong', ('{0} GB' -f $a.FreeGB)),
        @('YASB', $(if ($a.YasbPath) { ('terpasang v{0}{1}' -f $a.YasbVersion, $(if ($a.YasbVersion -lt $YasbMinVersion) { ' (perlu upgrade ke 2.0.7+)' } else { '' })) } else { 'belum terpasang' })),
        @('Konfigurasi YASB', $(if ($a.YasbOurs) { 'sudah memakai tema AlphaNotch' } elseif ($a.YasbConfig) { 'ada (milikmu sendiri)' } else { 'belum ada' })),
        @('Font JetBrainsMono Nerd', $(if ($a.Font) { 'ada' } else { 'belum ada' })),
        @('Rust (cargo)', $(if ($a.Cargo) { 'ada' } else { 'belum ada' })),
        @('Visual Studio Build Tools (C++)', $(if ($a.Msvc) { 'ada' } else { 'belum ada' })),
        @('Coucou Lite', $(if ($a.CoucouInstalled) { 'sudah terpasang (akan diperbarui)' } else { 'belum terpasang' })),
        @('Claude Code / Desktop', $(if ($a.Claude) { 'terdeteksi' } else { 'tidak terdeteksi' })),
        @('Antigravity', $(if ($a.Antigravity) { 'terdeteksi' } else { 'tidak terdeteksi' })),
        @('OpenCode', $(if ($a.OpenCode) { 'terdeteksi' } else { 'tidak terdeteksi' }))
    )
    foreach ($r in $rows) { Write-Host ('    {0,-34} {1}' -f $r[0], $r[1]) }
    if ($a.IsAdmin) { Warn 'PowerShell berjalan sebagai Administrator: file akan masuk ke profil admin itu. Sebaiknya jalankan sebagai user biasa.' }
    if ($a.TauriCoucou) { Warn 'Coucou versi Tauri sedang berjalan. Dua versi memakai pipe yang sama; tutup salah satunya.' }
}

# ---------------------------------------------------------------- plan

function Decide-Plan($a) {
    $p = [ordered]@{}
    $p.Errors = @()
    if ($a.OsBuild -lt 17763) { $p.Errors += 'Windows 10 1809 / Windows 11 atau lebih baru dibutuhkan.' }
    if (-not $a.Is64) { $p.Errors += 'Windows 64-bit dibutuhkan.' }

    # YASB
    $mode = $Yasb
    if ($mode -eq 'auto') {
        if (-not $a.YasbPath) { $mode = 'apply' }
        elseif ($a.YasbVersion -lt $YasbMinVersion) { $mode = 'apply' }
        elseif (-not $a.YasbConfig -or $a.YasbOurs) { $mode = 'apply' }
        else { $mode = 'ask' }
    }
    $p.YasbMode = $mode
    $p.YasbInstall = ($mode -ne 'skip') -and (-not $a.YasbPath) -and ($mode -ne 'keep')
    $p.YasbUpgrade = ($mode -ne 'skip') -and $a.YasbPath -and ($a.YasbVersion -lt $YasbMinVersion)
    $p.Font = ($mode -in @('apply', 'ask')) -and (-not $a.Font)

    # build tools
    $p.Rust = -not $a.Cargo
    $p.Msvc = -not $a.Msvc
    $p.NeedWinget = $p.YasbInstall -or $p.YasbUpgrade -or $p.Font -or $p.Rust -or $p.Msvc
    if ($p.NeedWinget -and -not $a.Winget) { $p.Errors += 'winget tidak ada. Pasang "App Installer" dari Microsoft Store lalu ulangi.' }
    $need = 2.5 + $(if ($p.Msvc) { 5.0 } else { 0 }) + $(if ($p.Rust) { 1.0 } else { 0 })
    if ($a.FreeGB -lt $need) { $p.Errors += ('Ruang kosong kurang: butuh sekitar {0} GB, ada {1} GB.' -f $need, $a.FreeGB) }

    $p.Hooks = @()
    if (-not $NoHooks) {
        if ($a.Claude) { $p.Hooks += 'claude' }
        if ($a.Antigravity) { $p.Hooks += 'antigravity' }
    }
    $p.Pills = @()
    if ($a.Antigravity) { $p.Pills += 'antigravity' }
    if ($a.OpenCode) { $p.Pills += 'opencode' }
    return $p
}

function Show-Plan($a, $p) {
    Step 'Rencana'
    $n = 1
    $line = { param($t) Write-Host ('    {0}. {1}' -f $script:n, $t); $script:n++ }
    $script:n = 1
    if ($p.Font) { & $line 'Memasang font JetBrainsMono Nerd Font (untuk ikon bar).' }
    if ($p.YasbInstall) { & $line 'Memasang YASB (winget AmN.yasb).' }
    if ($p.YasbUpgrade) { & $line ('Meng-upgrade YASB dari v{0} ke versi terbaru (perlu 2.0.7+ untuk bar "adaptive island").' -f $a.YasbVersion) }
    if ($p.Rust) { & $line 'Memasang Rust (winget Rustlang.Rustup), dibutuhkan untuk membangun Coucou Lite.' }
    if ($p.Msvc) { & $line 'Memasang Visual Studio Build Tools + C++ (besar: +-3-5 GB, ada prompt UAC).' }
    & $line ('Mengunduh source dari github.com/{0} ({1}) dan MEMBANGUN Coucou Lite di PC ini (+-5-10 menit).' -f $Repo, $Branch)
    & $line ('Memasang ke {0}.' -f $InstallDir)
    if (-not $NoSettings) { & $line 'Menulis pengaturan awal Coucou (tetap di layar, pill agent: sesuai yang terdeteksi).' }
    if (-not $NoAutostart) { & $line 'Mengaktifkan autostart Coucou saat login.' }
    switch ($p.YasbMode) {
        'apply' { & $line 'Menerapkan tema YASB "adaptive island" (konfigurasi lama dibackup dulu), lalu autostart YASB.' }
        'ask' { & $line 'Menanyakan apakah tema YASB "adaptive island" diterapkan (konfigurasi YASB-mu sekarang akan dibackup).' }
        'keep' { & $line 'YASB dibiarkan apa adanya (konfigurasi tidak disentuh).' }
        'skip' { & $line 'YASB dilewati.' }
    }
    foreach ($h in $p.Hooks) { & $line ('Menawarkan hook Coucou untuk {0} (ditampilkan dulu, backup dibuat, hook lain tidak disentuh).' -f $h) }
    & $line 'Menjalankan Coucou.'
    Write-Host ''
    Say ('    Log lengkap: ' + $LogFile) DarkGray
    Say '    Pencabutan kapan saja: uninstall.ps1 (di repo yang sama).' DarkGray
}

# ---------------------------------------------------------------- steps

function Winget-Install([string]$id, [string]$extra = '') {
    $wargs = @('install', '--id', $id, '-e', '--silent', '--accept-package-agreements', '--accept-source-agreements')
    if ($extra) { $wargs += @('--override', $extra) }
    $code = Run 'winget.exe' $wargs -AllowFail
    # 0 = ok, -1978335189 = already installed / no applicable update
    if ($code -ne 0 -and $code -ne -1978335189 -and $code -ne -1978335212) { throw ("winget gagal memasang $id (kode $code)") }
}

function Install-Font { Step 'Font JetBrainsMono Nerd Font'; Winget-Install 'DEVCOM.JetBrainsMonoNerdFont'; Ok 'font terpasang' }

function Install-Yasb($a) {
    Step 'YASB'
    Get-Process yasb -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    if ($a.YasbPath -and $a.YasbVersion -lt $YasbMinVersion) {
        $code = Run 'winget.exe' @('upgrade', '--id', 'AmN.yasb', '-e', '--silent', '--accept-package-agreements', '--accept-source-agreements') -AllowFail
        if ($code -ne 0) { Warn ('winget upgrade mengembalikan kode ' + $code + ' (kalau jendela installer YASB muncul, selesaikan dulu).') }
    } else {
        Winget-Install 'AmN.yasb'
    }
    $y = Find-Yasb
    if (-not $y) { throw 'YASB tidak ditemukan setelah pemasangan.' }
    Ok ('YASB v' + (Get-Item $y).VersionInfo.ProductVersion)
}

function Install-BuildTools($p) {
    if ($p.Rust) {
        Step 'Rust'
        Winget-Install 'Rustlang.Rustup'
        Refresh-Path
        $rustup = Get-Command rustup.exe -ErrorAction SilentlyContinue
        if (-not $rustup) { $rustup = Get-Item (Join-Path $env:USERPROFILE '.cargo\bin\rustup.exe') -ErrorAction SilentlyContinue }
        if ($rustup) {
            $path = if ($rustup.Source) { $rustup.Source } else { $rustup.FullName }
            Run $path @('default', 'stable-x86_64-pc-windows-msvc') | Out-Null
        }
        Refresh-Path
        if (-not (Find-Cargo)) { throw 'cargo tidak ditemukan setelah memasang Rust. Buka PowerShell baru lalu jalankan installer lagi.' }
        Ok 'Rust siap'
    }
    if ($p.Msvc) {
        Step 'Visual Studio Build Tools (C++). Ini besar; jendela UAC mungkin muncul.'
        Winget-Install 'Microsoft.VisualStudio.2022.BuildTools' '--wait --quiet --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
        if (-not (Has-Msvc)) { throw 'Komponen C++ belum terdeteksi. Pasang "Desktop development with C++" lewat Visual Studio Installer lalu ulangi.' }
        Ok 'Build Tools siap'
    }
}

function Get-Source {
    if ($SourceDir) {
        if (-not (Test-Path (Join-Path $SourceDir 'windows'))) { throw "SourceDir tidak berisi folder 'windows': $SourceDir" }
        Ok ('memakai source lokal: ' + $SourceDir)
        return (Resolve-Path $SourceDir).Path
    }
    $work = Join-Path $env:LOCALAPPDATA 'AlphaNotch\src'
    $zip = Join-Path $env:TEMP ('alphanotch-' + $Stamp + '.zip')
    $url = 'https://github.com/{0}/archive/refs/heads/{1}.zip' -f $Repo, $Branch
    Say ('    mengunduh ' + $url)
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
    if (Test-Path $work) { Remove-Item $work -Recurse -Force }
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    Expand-Archive -Path $zip -DestinationPath $work -Force
    Remove-Item $zip -Force -ErrorAction SilentlyContinue
    $root = Get-ChildItem $work -Directory | Select-Object -First 1
    if (-not $root -or -not (Test-Path (Join-Path $root.FullName 'windows'))) { throw 'Arsip GitHub tidak berisi folder windows.' }
    Ok ('source di ' + $root.FullName)
    return $root.FullName
}

function Build-Coucou($src) {
    Step 'Membangun Coucou Lite (beberapa menit; jangan ditutup)'
    $cargo = Find-Cargo
    if (-not $cargo) { throw 'cargo tidak ditemukan.' }
    $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'AlphaNotch\target'
    Push-Location (Join-Path $src 'windows')
    try { Run $cargo @('build', '--release', '-p', 'coucou-lite', '-p', 'coucou-hook') | Out-Null } finally { Pop-Location }
    $exe = Join-Path $env:CARGO_TARGET_DIR 'release\coucou-lite.exe'
    $hook = Join-Path $env:CARGO_TARGET_DIR 'release\coucou-hook.exe'
    if (-not (Test-Path $exe) -or -not (Test-Path $hook)) { throw 'Build selesai tapi exe tidak ditemukan.' }
    Ok 'build selesai'
    return @($exe, $hook)
}

function Install-Files($built) {
    Step ('Memasang ke ' + $InstallDir)
    $target = Join-Path $InstallDir 'coucou-lite.exe'
    Get-Process coucou-lite -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $target } | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 800
    New-Item -ItemType Directory -Path (Join-Path $InstallDir 'bin') -Force | Out-Null
    Copy-Item $built[0] (Join-Path $InstallDir 'coucou-lite.exe') -Force
    Copy-Item $built[1] (Join-Path $InstallDir 'coucou-hook.exe') -Force
    Copy-Item $built[1] (Join-Path $InstallDir 'bin\coucou-hook.exe') -Force
    Ok 'file terpasang'
}

function Write-Settings($p) {
    Step 'Pengaturan Coucou'
    $dir = Join-Path $env:APPDATA 'Coucou'
    $path = Join-Path $dir 'settings.json'
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    if (Test-Path $path) {
        Copy-Item $path ($path + '.bak-' + $Stamp) -Force
        $cur = Get-Content $path -Raw | ConvertFrom-Json
        $cur | Add-Member -NotePropertyName stayVisible -NotePropertyValue $true -Force
        if (-not $NoAutostart) { $cur | Add-Member -NotePropertyName autostart -NotePropertyValue $true -Force }
        if ($script:DisplayName) { $cur | Add-Member -NotePropertyName displayName -NotePropertyValue $script:DisplayName -Force }
        if (-not ($cur.PSObject.Properties.Name -contains 'agentPills')) { $cur | Add-Member -NotePropertyName agentPills -NotePropertyValue @($p.Pills) -Force }
        [IO.File]::WriteAllText($path, ($cur | ConvertTo-Json -Depth 6), $utf8)
        Ok 'pengaturan lama dipertahankan (hanya stayVisible/autostart diubah; backup dibuat)'
        return
    }
    $s = [ordered]@{
        soundEnabled      = $true
        soundVolume       = 0.12
        autoCloseInterval = 6.0
        absenceInterval   = 180.0
        activeIntegrations = @()
        screen            = 'primary'
        autostart         = (-not $NoAutostart)
        hooksInstalled    = $false
        stayVisible       = $true
        agentPills        = @($p.Pills)
        displayName       = $script:DisplayName
        weatherCity       = 'auto'
        stocks            = @('^JKSE', 'BBCA.JK', 'BBRI.JK', 'BMRI.JK', 'TLKM.JK')
    }
    [IO.File]::WriteAllText($path, ($s | ConvertTo-Json -Depth 6), $utf8)
    Ok ('pengaturan awal ditulis ke ' + $path)
}

function Set-Autostart {
    Step 'Autostart Coucou'
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    Set-ItemProperty -Path $key -Name 'Coucou' -Value ('"' + (Join-Path $InstallDir 'coucou-lite.exe') + '"')
    Ok 'Coucou akan jalan otomatis saat login'
}

function Apply-YasbLayout($src, $a) {
    Step 'Tema YASB "adaptive island"'
    $tpl = Join-Path $src 'yasb'
    if (-not (Test-Path (Join-Path $tpl 'config.yaml'))) { throw 'Folder yasb/ tidak ada di source.' }
    New-Item -ItemType Directory -Path $ConfigDir -Force | Out-Null
    foreach ($f in 'config.yaml', 'styles.css') {
        $dst = Join-Path $ConfigDir $f
        if (Test-Path $dst) { Copy-Item $dst ($dst + '.bak-' + $Stamp) -Force }
        Copy-Item (Join-Path $tpl $f) $dst -Force
    }
    Ok ('konfigurasi diterapkan di ' + $ConfigDir + ' (yang lama: *.bak-' + $Stamp + ')')
    $y = Find-Yasb
    if ($y) {
        $yc = Join-Path (Split-Path $y) 'yasbc.exe'
        if (Test-Path $yc) { Run $yc @('enable-autostart') -AllowFail | Out-Null }
        Start-Process -FilePath $y
        Ok 'YASB dijalankan dan diatur autostart'
    }
}

function Install-Hooks($agent, $exe) {
    $tmp = Join-Path $env:TEMP ('alphanotch-hook-' + $agent + '.txt')
    Start-Process -FilePath $exe -ArgumentList @('--agent-hooks', $agent, 'preview', '--out', ('"' + $tmp + '"')) -Wait -NoNewWindow
    if (-not (Test-Path $tmp)) { Warn ($agent + ': preview gagal, dilewati.'); return }
    $txt = Get-Content $tmp
    $fp = ($txt | Where-Object { $_ -match '^fingerprint:\s*(\S+)' } | ForEach-Object { $Matches[1] } | Select-Object -First 1)
    $file = ($txt | Where-Object { $_ -match '^file:\s*(.+)$' } | ForEach-Object { $Matches[1] } | Select-Object -First 1)
    $added = @($txt | Where-Object { $_ -match '^\+ ' }).Count
    $err = $txt | Where-Object { $_ -match '^error:' }
    if ($err -or -not $fp) { Warn ($agent + ': ' + ($err -join ' ') + ' (dilewati)'); return }
    Say ('    ' + $agent + ': akan MENAMBAH sekitar ' + $added + ' baris di ' + $file + ' (backup dibuat; entri alat lain tidak disentuh).')
    if (-not (Ask ('Pasang hook Coucou untuk ' + $agent + '?') $true)) { Say '    dilewati.'; return }
    $out2 = Join-Path $env:TEMP ('alphanotch-hook-' + $agent + '-install.txt')
    Start-Process -FilePath $exe -ArgumentList @('--agent-hooks', $agent, 'install', '--fingerprint', $fp, '--out', ('"' + $out2 + '"')) -Wait -NoNewWindow
    if (Test-Path $out2) { Get-Content $out2 | ForEach-Object { Say ('    ' + $_) } }
}

# ---------------------------------------------------------------- main

try {
    Say ''
    Say '  AlphaNotch installer  (Coucou Lite + YASB adaptive island)' Cyan
    Say '  Menganalisis dulu, mengubah sesudah kamu setuju.' DarkGray

    $a = Analyse
    Show-Analysis $a
    $plan = Decide-Plan $a

    if ($plan.Errors.Count -gt 0) {
        Step 'Tidak bisa lanjut'
        $plan.Errors | ForEach-Object { Fail $_ }
        return
    }

    Show-Plan $a $plan

    if ($DryRun) {
        Step 'DryRun: tidak ada yang diubah.'
        return
    }

    # one question for an existing, personal YASB config
    if ($plan.YasbMode -eq 'ask') {
        Write-Host ''
        Say '    Kamu sudah punya konfigurasi YASB sendiri.' Yellow
        if (Ask 'Terapkan tema AlphaNotch "adaptive island"? (konfigurasi lama dibackup)' $true) { $plan.YasbMode = 'apply' } else { $plan.YasbMode = 'keep' }
    }
    $script:DisplayName = Ask-Name
    Write-Host ''
    if (-not (Ask 'Lanjutkan pemasangan?' $true)) { Say 'Dibatalkan. Tidak ada yang diubah.'; return }

    if ($plan.Font -and $plan.YasbMode -eq 'apply') { Install-Font }
    if (($plan.YasbInstall -or $plan.YasbUpgrade) -and $plan.YasbMode -eq 'apply') { Install-Yasb $a }
    Install-BuildTools $plan

    Refresh-Path
    $src = Get-Source
    $built = Build-Coucou $src
    Install-Files $built
    if (-not $NoSettings) { Write-Settings $plan }
    if (-not $NoAutostart) { Set-Autostart }
    if ($plan.YasbMode -eq 'apply') { Apply-YasbLayout $src $a }

    $exe = Join-Path $InstallDir 'coucou-lite.exe'
    if ($plan.Hooks.Count -gt 0) {
        Step 'Hook agent'
        foreach ($h in $plan.Hooks) { Install-Hooks $h $exe }
    }

    if (-not $NoStart) {
        Step 'Menjalankan Coucou'
        Start-Process -FilePath $exe
        Start-Sleep -Seconds 4
        if (Get-Process coucou-lite -ErrorAction SilentlyContinue) { Ok 'Coucou berjalan (island ada di tengah atas layar)' } else { Warn ('Coucou tidak terlihat berjalan. Cek log: ' + (Join-Path $env:LOCALAPPDATA 'Coucou\coucou.log')) }
    }

    Step 'Selesai'
    Say ('    Coucou Lite : ' + $InstallDir)
    Say '    Dashboard   : arahkan kursor ke tengah atas layar lalu klik; menu tab ada di atas island.'
    Say '    Opsional    : hubungkan Google Tasks/Calendar lewat docs/GOOGLE-SETUP.md (butuh OAuth client milikmu sendiri).'
    Say '    Mencabut    : jalankan uninstall.ps1 dari repo ini.'
}
catch {
    Fail $_.Exception.Message
    Say ('    Log lengkap: ' + $LogFile) DarkGray
}
finally {
    try { Stop-Transcript | Out-Null } catch {}
}
