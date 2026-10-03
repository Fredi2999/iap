<#
.SYNOPSIS
  Baut die optionalen Zusatzpakete (Sprache, Vorlesen, Bildverständnis, Git)
  aus den heruntergeladenen Originaldateien in einen Ordner `AI\packs`.

.DESCRIPTION
  Jedes Paket liegt in einem eigenen Ordner mit `PACK.toml` (Kennung, Version,
  Lizenz, Dateiliste mit Größe und SHA-256). IAP prüft diese Liste vor der
  Nutzung; fehlt ein Paket oder stimmt ein Hash nicht, ist die Funktion
  sichtbar "nicht eingerichtet". Die Pakete sind getrennt vom Kernsystem und
  einzeln abschaltbar (Konzept 10.5). Es wird nichts heruntergeladen.

  Erwartete Quelle (`-Source`, Standard `%USERPROFILE%\IAPDev\packs`):
    _zips\whisper-bin-x64.zip, _zips\piper_windows_amd64.zip,
    _zips\MinGit-*-64-bit.zip, whisper\ggml-base.bin,
    piper\voices\*.onnx(.json|.MODEL_CARD.txt), vision\mmproj-*.gguf
#>
param(
    [string]$Source = (Join-Path $env:USERPROFILE 'IAPDev\packs'),
    [Parameter(Mandatory = $true)][string]$Target,
    [string]$WhisperTag = 'b5130',
    [string]$PiperTag = '2023.11.14-2',
    [string]$GitVersion = '2.56.0'
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem

function Reset-Dir([string]$Path) {
    if (Test-Path $Path) { Remove-Item $Path -Recurse -Force }
    New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

function Extract-Zip([string]$Zip, [string]$Dest) {
    if (-not (Test-Path $Zip)) { throw "Fehlt: $Zip" }
    [System.IO.Compression.ZipFile]::ExtractToDirectory($Zip, $Dest)
}

function Write-Manifest([string]$Dir, [string]$Id, [string]$Name, [string]$Version, [string]$License, [string]$Extra) {
    $lines = @(
        "id = `"$Id`"",
        "name = `"$Name`"",
        "version = `"$Version`"",
        "license = `"$License`""
    )
    if ($Extra) { $lines += $Extra }
    $root = (Resolve-Path $Dir).Path.TrimEnd('\')
    Get-ChildItem $Dir -Recurse -File | Where-Object { $_.Name -ne 'PACK.toml' } | Sort-Object FullName | ForEach-Object {
        $rel = $_.FullName.Substring($root.Length + 1).Replace('\', '/')
        $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
        $lines += ''
        $lines += '[[file]]'
        $lines += "path = `"$rel`""
        $lines += "size_bytes = $($_.Length)"
        $lines += "sha256 = `"$hash`""
    }
    Set-Content -Path (Join-Path $Dir 'PACK.toml') -Value $lines -Encoding UTF8
    $total = (Get-ChildItem $Dir -Recurse -File | Measure-Object Length -Sum).Sum
    Write-Host ("   {0,-8} {1,8:N1} MB" -f $Id, ($total / 1MB))
}

Write-Host "== Zusatzpakete bauen: $Source -> $Target" -ForegroundColor Cyan
Reset-Dir $Target
$staging = Join-Path $env:TEMP 'iap-pack-staging'
Reset-Dir $staging

# --- Spracherkennung: whisper.cpp (MIT) + Modell ggml-base (MIT, OpenAI Whisper) ---
$whisper = Join-Path $Target 'whisper'
New-Item -ItemType Directory -Path $whisper | Out-Null
$stage = Join-Path $staging 'whisper'
Extract-Zip (Join-Path $Source '_zips\whisper-bin-x64.zip') $stage
foreach ($name in @('whisper-server.exe', 'whisper.dll', 'ggml.dll', 'ggml-base.dll')) {
    Copy-Item (Join-Path $stage "Release\$name") $whisper
}
Get-ChildItem (Join-Path $stage 'Release') -Filter 'ggml-cpu-*.dll' | Copy-Item -Destination $whisper
Copy-Item (Join-Path $Source 'whisper\ggml-base.bin') $whisper
Set-Content (Join-Path $whisper 'LICENSE.txt') -Encoding UTF8 -Value @(
    'whisper.cpp (ggml-org/whisper.cpp), Version ' + $WhisperTag + ': MIT-Lizenz, Copyright (c) 2023-2026 The ggml authors.',
    'Modell ggml-base.bin: Gewichte von OpenAI Whisper (MIT-Lizenz, Copyright (c) 2022 OpenAI), konvertiert im Format von whisper.cpp.',
    'Quelle: https://github.com/ggml-org/whisper.cpp und https://huggingface.co/ggerganov/whisper.cpp'
)
Write-Manifest $whisper 'whisper' 'Spracherkennung (whisper.cpp, Modell base)' "$WhisperTag / ggml-base" 'MIT' 'requires_restart = false'

# --- Sprachausgabe: Piper (MIT) + Stimmen ---
$piper = Join-Path $Target 'piper'
$stage = Join-Path $staging 'piper'
Extract-Zip (Join-Path $Source '_zips\piper_windows_amd64.zip') $stage
Copy-Item (Join-Path $stage 'piper') $piper -Recurse
$voices = Join-Path $piper 'voices'
New-Item -ItemType Directory -Path $voices | Out-Null
Copy-Item (Join-Path $Source 'piper\voices\*') $voices
Set-Content (Join-Path $piper 'LICENSE.txt') -Encoding UTF8 -Value @(
    'Piper (rhasspy/piper), Version ' + $PiperTag + ': MIT-Lizenz.',
    'Stimmen (Quelle https://huggingface.co/rhasspy/piper-voices), Lizenzen je Stimme siehe voices\*.MODEL_CARD.txt:',
    '  de_DE-thorsten-medium: CC0 | es_ES-davefx-medium: CC0 | fr_FR-siwis-medium: CC-BY 4.0 | en_GB-alba-medium: CC-BY 4.0',
    'Bei CC-BY 4.0 ist die Namensnennung Pflicht: Siwis (Universität Edinburgh) und Alba (Universität Edinburgh).'
)
Write-Manifest $piper 'piper' 'Sprachausgabe (Piper, Stimmen de/en/es/fr)' $PiperTag 'MIT; Stimmen CC0 und CC-BY 4.0' ''

# --- Bildverständnis: Projektor zu Gemma 4 E2B ---
$vision = Join-Path $Target 'vision'
New-Item -ItemType Directory -Path $vision | Out-Null
$mmproj = Get-ChildItem (Join-Path $Source 'vision') -Filter 'mmproj-*.gguf' | Select-Object -First 1
if (-not $mmproj) { throw 'Kein mmproj im Quellordner vision\' }
Copy-Item $mmproj.FullName $vision
Set-Content (Join-Path $vision 'LICENSE.txt') -Encoding UTF8 -Value @(
    'Bildprojektor (mmproj) zu Gemma 4 E2B, bereitgestellt von ggml-org (https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF).',
    'Es gelten die Gemma-Nutzungsbedingungen von Google, wie für das Textmodell.'
)
Write-Manifest $vision 'vision' 'Bildverständnis (Projektor zu Gemma 4 E2B)' $mmproj.Name 'Gemma-Nutzungsbedingungen' 'for_model = "gemma-4-e2b-q4-k-m"'

# --- Git: MinGit (GPLv2) ---
$git = Join-Path $Target 'git'
New-Item -ItemType Directory -Path $git | Out-Null
Extract-Zip (Join-Path $Source "_zips\MinGit-$GitVersion-64-bit.zip") $git
Set-Content (Join-Path $git 'LICENSE-Hinweis.txt') -Encoding UTF8 -Value @(
    "MinGit $GitVersion (Git for Windows, https://github.com/git-for-windows/git), GNU General Public License v2.",
    'Der Quelltext ist unter https://github.com/git-for-windows/git/releases verfügbar. IAP ruft git.exe nur als eigenen Prozess auf.'
)
Write-Manifest $git 'git' "Git fuer Worktrees (MinGit $GitVersion)" $GitVersion 'GPL-2.0' ''

Remove-Item $staging -Recurse -Force
Write-Host '== Pakete fertig.' -ForegroundColor Green
