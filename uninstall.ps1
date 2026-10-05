<#
.SYNOPSIS
  Removes what install.ps1 added: Coucou Lite, its autostart, its hooks, and (optionally) the YASB layout.

.DESCRIPTION
    irm https://raw.githubusercontent.com/xdukunx/alphanotch/main/uninstall.ps1 | iex

  It asks before each part. YASB itself is never uninstalled. Hooks are removed with the same
  preview + backup flow used to add them, and only Coucou's own entries are touched.

.PARAMETER Yes         Do not ask; remove everything this installer added.
.PARAMETER KeepData    Keep %APPDATA%\Coucou (settings, to-dos, teleprompter script).
.PARAMETER InstallDir  Where Coucou Lite was installed (default %LOCALAPPDATA%\Coucou).
#>
[CmdletBinding()]
param(
    [switch]$Yes,
    [switch]$KeepData,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Coucou')
)

$ErrorActionPreference = 'Stop'
$ConfigDir = if ($env:YASB_CONFIG_HOME) { $env:YASB_CONFIG_HOME } else { Join-Path $env:USERPROFILE '.config\yasb' }

function Say($m, $c = 'Gray') { Write-Host $m -ForegroundColor $c }
function Step($m) { Write-Host ''; Write-Host ('==> ' + $m) -ForegroundColor Cyan }
function Ask([string]$q, [bool]$d = $true) {
    if ($Yes) { return $true }
    $hint = if ($d) { '[Y/n]' } else { '[y/N]' }
    while ($true) {
        $a = Read-Host ($q + ' ' + $hint)
        if ([string]::IsNullOrWhiteSpace($a)) { return $d }
        if ($a -match '^(y|ya|yes)$') { return $true }
        if ($a -match '^(n|no|tidak)$') { return $false }
    }
}

$exe = Join-Path $InstallDir 'coucou-lite.exe'

try {
    Say ''
    Say '  AlphaNotch uninstaller' Cyan

    # 1. hooks first, while the exe that knows how to remove them still exists
    if (Test-Path $exe) {
        foreach ($agent in 'claude', 'antigravity') {
            $tmp = Join-Path $env:TEMP ('alphanotch-un-' + $agent + '.txt')
            Start-Process -FilePath $exe -ArgumentList @('--agent-hooks', $agent, 'status', '--out', ('"' + $tmp + '"')) -Wait -NoNewWindow
            if ((Test-Path $tmp) -and (Select-String -Path $tmp -Pattern 'installed: true' -SimpleMatch -Quiet)) {
                Step ('Hook Coucou di ' + $agent)
                if (Ask ('Hapus hook Coucou dari ' + $agent + '? (backup dibuat, entri lain tidak disentuh)') $true) {
                    $prev = Join-Path $env:TEMP ('alphanotch-un-' + $agent + '-prev.txt')
                    Start-Process -FilePath $exe -ArgumentList @('--agent-hooks', $agent, 'preview', '--out', ('"' + $prev + '"')) -Wait -NoNewWindow
                    $fp = (Get-Content $prev | Where-Object { $_ -match '^fingerprint:\s*(\S+)' } | ForEach-Object { $Matches[1] } | Select-Object -First 1)
                    if ($fp) {
                        $out = Join-Path $env:TEMP ('alphanotch-un-' + $agent + '-out.txt')
                        Start-Process -FilePath $exe -ArgumentList @('--agent-hooks', $agent, 'uninstall', '--fingerprint', $fp, '--out', ('"' + $out + '"')) -Wait -NoNewWindow
                        if (Test-Path $out) { Get-Content $out | ForEach-Object { Say ('    ' + $_) } }
                    }
                }
            }
        }
    }

    # 2. the app
    Step 'Coucou Lite'
    if (Ask 'Hentikan dan hapus Coucou Lite beserta autostart-nya?' $true) {
        Get-Process coucou-lite -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 800
        Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Coucou' -ErrorAction SilentlyContinue
        foreach ($f in 'coucou-lite.exe', 'coucou-hook.exe', 'bin\coucou-hook.exe') {
            $p = Join-Path $InstallDir $f
            if (Test-Path $p) { Remove-Item $p -Force }
        }
        Say '    aplikasi dan autostart dihapus.'
        $build = Join-Path $env:LOCALAPPDATA 'AlphaNotch'
        if ((Test-Path $build) -and (Ask 'Hapus juga folder build (source + cache kompilasi, bisa beberapa GB)?' $true)) { Remove-Item $build -Recurse -Force; Say '    folder build dihapus.' }
    }

    # 3. data
    $data = Join-Path $env:APPDATA 'Coucou'
    if ((Test-Path $data) -and -not $KeepData) {
        Step 'Data Coucou'
        if (Ask ('Hapus pengaturan, daftar tugas, dan naskah teleprompter di ' + $data + '?') $false) {
            Remove-Item $data -Recurse -Force
            Say '    data dihapus (token Google di Credential Manager tidak ikut terhapus: hapus entri "fr.louisraille.coucou" bila perlu).'
        } else { Say '    data dipertahankan.' }
    }

    # 4. YASB layout
    $cfg = Join-Path $ConfigDir 'config.yaml'
    if ((Test-Path $cfg) -and (Select-String -Path $cfg -Pattern 'AlphaNotch YASB layout' -SimpleMatch -Quiet)) {
        Step 'Tema YASB AlphaNotch'
        $bak = Get-ChildItem $ConfigDir -Filter 'config.yaml.bak-*' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($bak) {
            if (Ask ('Kembalikan konfigurasi YASB lama dari ' + $bak.Name + '?') $true) {
                Copy-Item $bak.FullName $cfg -Force
                $cssBak = Join-Path $ConfigDir ($bak.Name -replace '^config\.yaml', 'styles.css')
                if (Test-Path $cssBak) { Copy-Item $cssBak (Join-Path $ConfigDir 'styles.css') -Force }
                Say '    konfigurasi YASB lama dikembalikan (YASB memuat ulang sendiri).'
            }
        } else {
            Say '    tidak ada backup konfigurasi lama; tema dibiarkan. Hapus/ubah config.yaml dan styles.css secara manual bila perlu.'
        }
    }

    Step 'Selesai'
    Say '    YASB dan font tidak dihapus. Untuk menghapus YASB: winget uninstall AmN.yasb'
}
catch {
    Write-Host ('    [x]  ' + $_.Exception.Message) -ForegroundColor Red
}
