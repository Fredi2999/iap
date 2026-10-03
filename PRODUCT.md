# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- Primär ein persönlicher Nutzer, der IAP von einem USB-Stick auf wechselnden Windows-Rechnern startet. **Vorläufige Annahme aus Konzept und Auftrag; Nutzerbestätigung steht aus.**
- Die Oberfläche muss auf der Untergrenze von 8 GB RAM ohne dedizierte GPU bedienbar bleiben.

## Product Purpose

IAP ist ein portabler lokaler KI-Agent. Er startet vom USB-Stick, führt die Inferenz auf dem Host aus und verwahrt Unterhaltungen und persönliche Daten verschlüsselt im Vault. Erfolg bedeutet, auf fremder Hardware nachvollziehbar und ohne Cloud arbeiten zu können.

## Positioning

Die Kombination aus lokalem Modell, verschlüsseltem portablem Vault und zentraler Policy für jeden Werkzeugaufruf ermöglicht KI-Arbeit mit nutzerkontrollierten Daten und Handlungen.

## Operating Context

- Windows ist die primäre Plattform; Linux und macOS sind spätere Phasen.
- Der Nutzer entsperrt einen von mehreren Vaults, chattet mit einem lokalen Modell und nutzt bei Bedarf Dateien, Memory, Skills, Agenten, Code, Kalender und weitere Werkzeuge.
- Langsame CPU-Inferenz macht Streaming, ehrlichen Status, Abbruch und klare Kontext- und Zeitbudgets wesentlich.
- Entwicklung und Release-Builds auf Windows laufen wegen des Umlauts im Quellpfad über die dokumentierte ASCII-Junction.

## Capabilities and Constraints

- Verbindlicher Stack: Rust, Tauri v2, Svelte 5, TypeScript, Vite, TailwindCSS, CodeMirror 6, llama.cpp als Loopback-Subprozess, SQLCipher, sqlite-vec, Wasmtime und gix.
- Kein ausgehender Netzwerkverkehr, keine Cloud oder Telemetrie; nur die eigene Inferenz auf `127.0.0.1`.
- Jeder Werkzeugaufruf läuft durch `pa-policy`. Das Modell erzeugt Patches; der Nutzer bestätigt sie nach Diff-Sicht.
- Keine Blockade des UI-Threads; lange Vorgänge brauchen Hintergrundstatus.
- Kernbundle ohne Modelle unter 100 MB. Die Zielhardware ist 8 GB RAM und CPU-only.
- Der bestehende Code enthält Konnektor-Oberflächen mit Netzwerkzuständen. Ihre Beziehung zur verbindlichen Offline-Invariante ist eine offene Produkt- und Implementierungsfrage; die Neugestaltung darf keinen zusätzlichen Netzwerkzugriff einführen.

## Brand Commitments

- Name: IAP. Das vorhandene monochrome IAP-Logo bleibt erkennbar.
- Deutsche UI-Texte und Kommentare, englische Bezeichner.
- Keine Emojis in der Oberfläche; monochrome SVG-Piktogramme.
- Für die Neugestaltung sind ein modernes schwarzes Glassmorphism-Design, Liquid-Glass-Buttons und ein neues App-Layout ausdrücklich vorgegeben.

## Evidence on Hand

- Architekturkonzept: `docs/konzept.md` mit Original `portabler-ki-agent-konzept.md`.
- Projektregeln: `AGENTS.md`.
- Aktuelle Übergabe: `docs/handover-2026-09-25.md`.
- Bestehende Svelte-Oberfläche: `app/src/`.
- Logo: `app/src/lib/assets/iap-logo.png`.

## Product Principles

1. Die nächste Handlung und der aktuelle Systemzustand sind auf einen Blick klar.
2. Der Nutzer behält Kontrolle über Daten, Modell, Werkzeuge und Freigaben.
3. Die Oberfläche bleibt auf schwacher Hardware flüssig und zeigt Wartezeiten ehrlich.
4. Portabilität und Offline-Betrieb gelten für jede Funktion, nicht nur für den Chat.
