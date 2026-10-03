<#
.SYNOPSIS
Baut das portable Windows-Paket von IAP unter dist\iap-windows.

.DESCRIPTION
Führt Release-Builds von pa-launcher und optional der Tauri-Anwendung aus,
kopiert Runtime, Modell und Konfiguration in eine flache Verzeichnisstruktur,
die direkt vom USB-Stick gestartet werden kann. Nutzt bewusst den in
docs/mvp-schritt3-cli.md dokumentierten ASCII-Junction-Pfad, damit der reale
`ü`-Pfad die vendored-OpenSSL-Skripte nicht mehr aus dem Tritt bringt.

.PARAMETER Junction
Absoluter Pfad des ASCII-Junctions. Standard: %TEMP%\portableai-usb-ascii.

.PARAMETER OutDir
Zielverzeichnis für das Bundle. Standard: <Junction>\dist\iap-windows.

.PARAMETER SkipTauri
Baut nur pa-launcher.exe und lässt die Tauri-Binary aus. Nützlich, wenn npm
nicht offline verfügbar ist oder das Frontend separat gebaut wurde.

.PARAMETER SkipModel
Kopiert AI\models nicht (nur Kernbundle, für Größenmessung).
#>
[CmdletBinding()]
param(
    [string]$Junction = (Join-Path $env:TEMP 'portableai-usb-ascii'),
    [string]$OutDir,
    [switch]$SkipTauri,
    [switch]$SkipModel
)

$ErrorActionPreference = 'Stop'

function Assert-Path {
    param([string]$Path, [string]$Label)
    if (-not (Test-Path $Path)) {
        throw "$Label fehlt: $Path"
    }
}

Assert-Path -Path $Junction -Label 'ASCII-Junction'
if (-not $OutDir) {
    $OutDir = Join-Path $Junction 'dist\iap-windows'
}
Write-Host "== IAP Windows-Bundle" -ForegroundColor Cyan
Write-Host "   Junction: $Junction"
Write-Host "   Ziel    : $OutDir"

$strawberryPerl = Join-Path $Junction 'tools\build\strawberry\perl\bin'
Assert-Path -Path $strawberryPerl -Label 'Strawberry Perl'

$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'iap-target-ascii'
$env:PATH = $strawberryPerl + ';' + $env:PATH
$env:LC_ALL = 'C'
$env:LANG = 'C'
try { Remove-Item Env:PERL5LIB -ErrorAction Stop } catch {}

$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
Assert-Path -Path $cargo -Label 'cargo.exe'

Push-Location $Junction
try {
    Write-Host "== cargo build -p pa-launcher --release --offline"
    & $cargo build -p pa-launcher --release --offline
    if ($LASTEXITCODE -ne 0) { throw "pa-launcher-Release-Build ist mit Exitcode $LASTEXITCODE gescheitert." }
    $launcherRelease = Join-Path $env:CARGO_TARGET_DIR 'release\pa-launcher.exe'
    Assert-Path -Path $launcherRelease -Label 'pa-launcher.exe (Release)'

    Write-Host "== cargo build -p pa-stop --release --offline"
    & $cargo build -p pa-stop --release --offline
    if ($LASTEXITCODE -ne 0) { throw "pa-stop-Release-Build ist mit Exitcode $LASTEXITCODE gescheitert." }
    $stopRelease = Join-Path $env:CARGO_TARGET_DIR 'release\pa-stop.exe'
    Assert-Path -Path $stopRelease -Label 'pa-stop.exe (Release)'

    Write-Host "== cargo build -p pa-pci --release --offline (PCI-Begleiter)"
    & $cargo build -p pa-pci --release --offline
    if ($LASTEXITCODE -ne 0) { throw "pa-pci-Release-Build ist mit Exitcode $LASTEXITCODE gescheitert." }
    $pciRelease = Join-Path $env:CARGO_TARGET_DIR 'release\iap-pci.exe'
    Assert-Path -Path $pciRelease -Label 'iap-pci.exe (Release)'
} finally {
    Pop-Location
}

$tauriBinary = $null
if (-not $SkipTauri) {
    Write-Host "== Tauri-Build (iap.exe)"
    $appTauriRoot = Join-Path $Junction 'app\src-tauri'
    Push-Location $appTauriRoot
    try {
        $env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'iap-target-ascii-app'
        & $cargo build --release --offline
        if ($LASTEXITCODE -ne 0) { throw "Tauri-Release-Build ist mit Exitcode $LASTEXITCODE gescheitert. Wenn das Frontend fehlt, bitte zuerst `"npm install`" und `"npm run build`" im app-Verzeichnis." }
        $tauriBinary = Join-Path $env:CARGO_TARGET_DIR 'release\iap.exe'
        Assert-Path -Path $tauriBinary -Label 'iap.exe (Release)'
    } finally {
        Pop-Location
        $env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'iap-target-ascii'
    }
} else {
    Write-Host "== Tauri-Build wird übersprungen (--SkipTauri)"
}

if (Test-Path $OutDir) {
    Write-Host "== Bereinige altes Zielverzeichnis $OutDir"
    Remove-Item -Recurse -Force $OutDir
}
New-Item -ItemType Directory -Path $OutDir | Out-Null

Write-Host "== Kopiere pa-launcher.exe (CLI und Diagnose)"
Copy-Item -Path $launcherRelease -Destination (Join-Path $OutDir 'pa-launcher.exe')

Write-Host "== Kopiere IAP-Beenden.exe (Safe Eject Utility)"
Copy-Item -Path $stopRelease -Destination (Join-Path $OutDir 'IAP-Beenden.exe')

if ($tauriBinary) {
    Write-Host "== Kopiere iap.exe (GUI)"
    Copy-Item -Path $tauriBinary -Destination (Join-Path $OutDir 'iap.exe')
}

$aiSource = Join-Path $Junction 'AI'
Assert-Path -Path $aiSource -Label 'AI-Verzeichnis'
$aiTarget = Join-Path $OutDir 'AI'
New-Item -ItemType Directory -Path $aiTarget | Out-Null

$binSource = Join-Path $aiSource 'bin'
Assert-Path -Path $binSource -Label 'AI\bin'
Write-Host "== Kopiere AI\bin (llama.cpp-Runtime + Manifest)"
Copy-Item -Path $binSource -Destination $aiTarget -Recurse

Write-Host "== Lege PCI-Begleiter in AI\bin\win-x64 ab (iap-pci.exe)"
$pciTargetDir = Join-Path $aiTarget 'bin\win-x64'
New-Item -ItemType Directory -Path $pciTargetDir -Force | Out-Null
Copy-Item -Path $pciRelease -Destination (Join-Path $pciTargetDir 'iap-pci.exe') -Force

if ($SkipModel) {
    Write-Host "== AI\models wird ausgelassen (--SkipModel)"
} else {
    $modelsSource = Join-Path $aiSource 'models'
    Assert-Path -Path $modelsSource -Label 'AI\models'
    Write-Host "== Kopiere AI\models (Modell + Deskriptor)"
    Copy-Item -Path $modelsSource -Destination $aiTarget -Recurse
}

$dataTarget = Join-Path $aiTarget 'data'
New-Item -ItemType Directory -Path $dataTarget | Out-Null
Write-Host "== Vault-Verzeichnis AI\data leer angelegt"

$startCmd = @'
@echo off
setlocal
cd /d "%~dp0"
if exist "iap.exe" (
    start "" "iap.exe"
) else (
    echo Die grafische Oberflaeche fehlt in diesem Bundle.
    echo Falls du nur die CLI moechtest, fuehre stattdessen aus:
    echo   pa-launcher.exe --cli --root .
    pause
)
'@
$startCmd | Out-File -FilePath (Join-Path $OutDir 'Start.cmd') -Encoding ascii

$liesmich = @'
IAP - portabler lokaler KI-Agent
=======================================

Was das ist
-----------
IAP startet vom USB-Stick, misst die Hardware des Rechners, laedt
standardmaessig Gemma 4 E2B Q4_K_M und chattet mit dir. Ist ein weiteres
Modell im Paket installiert, kannst du es in der Chat-Promptbar waehlen und
seine Werte in den Einstellungen anpassen. Alle Daten bleiben
verschluesselt auf dem Stick. Es besteht kein Netzwerkverkehr ausser dem
Loopback zur eigenen Inferenz-Instanz (127.0.0.1).

Voraussetzungen
---------------
- Windows 10/11 (x64)
- Mind. 8 GB RAM (Tier 0), CPU-only reicht
- Keine Administratorrechte noetig
- WebView2 muss vorhanden sein (bei Windows 11 immer, bei aelteren Windows
  10 sonst einmalig installieren)

Starten
-------
Doppelklick auf "Start.cmd" oder direkt auf "iap.exe".
Beim ersten Start:
1. Passphrase fuer den neuen Vault vergeben (Haken "Neu anlegen" setzen).
2. Beim Bereich "Chat" beginnt IAP mit Streaming.

CLI-Modus (Diagnose)
--------------------
Wenn die grafische Oberflaeche zickt oder du im Klartext chatten willst:
    pa-launcher.exe --cli --root .
Endet mit /quit; Ctrl+C bricht eine laufende Antwort ab, der Teiltext bleibt
im Vault gespeichert.

Cache und Portabilitaet
-----------------------
Das Modell wird beim ersten Start eines neuen Rechners in
%LOCALAPPDATA%\IAP\model-cache\<sha256> kopiert und dort verifiziert
wiederverwendet. Nichts weiter wird ausserhalb des Sticks abgelegt.

Sicherer Auswurf
----------------
Vor dem Ziehen des Sticks entweder /quit im CLI oder das Fenster schliessen
(Rueksync erfolgt automatisch). Bei Absturz oder unsauberem Ziehen wird der
Vault beim naechsten Start automatisch aus der geprueften Hostkopie
wiederhergestellt.

Noch nicht durchgefuehrte manuelle Tests
----------------------------------------
- Start auf einem echten 8-GB-Rechner
- Start auf mehreren unterschiedlichen Windows-PCs
- SmartScreen-Verhalten
- Physisches Ziehen und Wiederanstecken des Sticks waehrend des Betriebs
- Externer Netzwerkmonitor (Nachweis "kein einziger ausgehender Aufruf")
Diese Punkte werden nicht geschaetzt; du kannst sie selbst ausfuehren und
in docs/mvp-schritt6-paket.md ergaenzen.

Lizenz und Datenschutz
----------------------
Der komplette Programmquelltext liegt neben dieser Datei im Projektstamm.
Es findet keinerlei Telemetrie statt. Deine Chats bleiben deine Chats.
'@
$liesmich | Out-File -FilePath (Join-Path $OutDir 'LIESMICH.txt') -Encoding utf8

# Groessen ermitteln
$coreItems = @('Start.cmd','LIESMICH.txt','pa-launcher.exe','IAP-Beenden.exe')
if ($tauriBinary) { $coreItems += 'iap.exe' }
$coreItems += 'AI\bin'
$coreBytes = 0
foreach ($item in $coreItems) {
    $path = Join-Path $OutDir $item
    if (Test-Path $path) {
        $coreBytes += (Get-ChildItem -Recurse -File $path | Measure-Object -Property Length -Sum).Sum
    }
}
$coreMb = [math]::Round($coreBytes / 1MB, 2)
Write-Host ""
Write-Host "== Kernbundle (ohne AI\models): $coreMb MB"

$launcherHash = (Get-FileHash -Algorithm SHA256 -Path (Join-Path $OutDir 'pa-launcher.exe')).Hash
Write-Host "   pa-launcher.exe SHA-256: $launcherHash"
if ($tauriBinary) {
    $guiHash = (Get-FileHash -Algorithm SHA256 -Path (Join-Path $OutDir 'iap.exe')).Hash
    Write-Host "   iap.exe SHA-256: $guiHash"
}

if ($coreBytes -gt (100 * 1MB)) {
    Write-Host "WARNUNG: Kernbundle ueberschreitet 100 MB." -ForegroundColor Yellow
} else {
    Write-Host "OK: Kernbundle unter der 100-MB-Grenze."
}

Write-Host ""
Write-Host "Bundle erstellt unter: $OutDir" -ForegroundColor Green
