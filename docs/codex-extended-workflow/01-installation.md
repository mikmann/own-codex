# 01 – Installation

## Unterstützter Installationsweg

Für Endanwender unter Windows ist das vollständige, geprüfte Releasepaket der
empfohlene Weg. Es enthält nicht nur die CLI, sondern auch den separaten Code
Mode Host und die benötigten Hilfsprogramme.

Voraussetzungen:

- Windows x64
- Windows PowerShell 5.1 oder PowerShell 7
- Zugriff auf das Release-Repository
- ein ChatGPT- oder API-Zugang, mit dem Codex verwendet werden kann

Der Download des Installers kann aus **jedem Ordner** erfolgen. Erst die spätere
Repository-Vorbereitung muss innerhalb eines Git-Worktrees oder mit `--cd`
ausgeführt werden.

## Alpha‑9 installieren

```powershell
$tag = "v0.147.1-alpha-9"
$installer = Join-Path $env:TEMP "install-extended-workflow.ps1"
Invoke-WebRequest "https://github.com/mikmann/own-codex/releases/download/$tag/install-extended-workflow.ps1" -OutFile $installer
& $installer -Release $tag
```

Der Installer führt vor der Aktivierung folgende Prüfungen durch:

- SHA‑256-Prüfsumme des Releasepakets
- Vollständigkeit und erwartete Verzeichnisstruktur
- gemeldete CLI-Version
- Vorhandensein und Startfähigkeit von `codex-code-mode-host.exe`
- sichere gestaffelte Installation mit Rückfallmöglichkeit

Das Standardziel ist:

```text
%LOCALAPPDATA%\Programs\codex-extended-workflow
```

Der Installer ergänzt den Benutzer-`PATH` und den `PATH` der aktuellen
PowerShell-Sitzung. Mit `-NoPathUpdate` kann diese Änderung bewusst unterbunden
werden.

## Installation verifizieren

Öffne bei Bedarf eine neue PowerShell und führe aus:

```powershell
Get-Command codex-extended-workflow
codex-extended-workflow --version
codex-extended-workflow doctor
```

Für dieses Release muss die Versionsausgabe lauten:

```text
codex-extended-workflow v0.147.1-alpha-9
```

`Get-Command` sollte auf die verwaltete Installation unter
`%LOCALAPPDATA%\Programs\codex-extended-workflow` zeigen. Zeigt der Befehl auf
ein altes Build-Verzeichnis, muss die `PATH`-Reihenfolge korrigiert oder die
PowerShell neu geöffnet werden.

## Enthaltene Laufzeitdateien

Das vollständige Paket enthält mindestens:

```text
bin\codex-extended-workflow.exe
bin\codex-code-mode-host.exe
codex-path\rg.exe
codex-resources\codex-command-runner.exe
codex-resources\codex-windows-sandbox-setup.exe
```

Ein bloßes `cargo install` installiert nur das CLI-Crate und lässt den separat
gebauten Code Mode Host aus. Es ist deshalb kein vollständiger Installationsweg
für Endanwender.

## Anmeldung und erster Start

```powershell
codex-extended-workflow login
codex-extended-workflow
```

Ohne `--shared-state` verwendet die CLI ein eigenes Daten- und
Anmeldeverzeichnis:

```text
C:\Users\<Benutzer>\.codex-extended-workflow
```

Die normale `codex`-Installation verwendet dagegen standardmäßig
`C:\Users\<Benutzer>\.codex`. Dadurch vermischt die erweiterte CLI ihre
Sitzungen und lokalen Daten nicht unbemerkt mit normalem Codex.

## Auf eine spätere Alpha-Version aktualisieren

Setze `$tag` auf den neuen Release-Tag und führe denselben Installer erneut aus.
Danach immer erneut prüfen:

```powershell
codex-extended-workflow --version
codex-extended-workflow doctor
```

Ein Release-Tag allein beweist noch nicht, dass die lokale Shell bereits die
neue Binärdatei verwendet. Maßgeblich sind `Get-Command` und die tatsächlich
ausgegebene Version.

## Nächster Schritt

Weiter mit [02 – Schnellstart und Vorbereitung](02-schnellstart-und-vorbereitung.md).
