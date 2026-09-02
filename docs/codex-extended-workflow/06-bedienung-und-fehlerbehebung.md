# 06 – Bedienung und Fehlerbehebung

## Befehlsübersicht

### Installation und Laufzeit prüfen

```powershell
Get-Command codex-extended-workflow
codex-extended-workflow --version
codex-extended-workflow --help
codex-extended-workflow doctor
```

### Repository vorbereiten

```powershell
# Vorschau im aktuellen Git-Worktree
codex-extended-workflow prepare-extend-workflow

# Vorschau anwenden
codex-extended-workflow prepare-extend-workflow --apply

# Explizites Repository aus beliebigem Arbeitsordner
codex-extended-workflow prepare-extend-workflow --cd D:\Pfad\zum\Repository
codex-extended-workflow prepare-extend-workflow --cd D:\Pfad\zum\Repository --apply

# Globale Extended-Workflow-Konfiguration
codex-extended-workflow prepare-extend-workflow --global
codex-extended-workflow prepare-extend-workflow --global --apply
```

### Sitzung starten oder fortsetzen

```powershell
codex-extended-workflow
codex-extended-workflow resume <SITZUNGS-ID>
codex-extended-workflow --shared-state
codex-extended-workflow --shared-state resume <SITZUNGS-ID>
```

### Temporäre Prozesswerte

```powershell
codex-extended-workflow `
  --workflow-token-limit 50000 `
  --expert-min 2 `
  --expert-max 4 `
  --reviewer-min 2 `
  --reviewer-max 4
```

## Wichtige Slash-Befehle

| Befehl | Zweck |
|---|---|
| `/status` | aktuelle Sitzung, Modell, Version und Nutzung anzeigen |
| `/statusline` | Elemente wie Session- und Workflowtokens ein-/ausblenden |
| `/agent` | laufende oder abgeschlossene Agententhreads ansehen |
| `/workflow-reset-budget` | aktiven Workflow aufgeben und sein Budget schließen |
| `/prepare-extend-workflow` | Repository-Vorbereitung in der TUI anzeigen |
| `/prepare-extend-workflow apply` | sichere Vorbereitung anwenden |

`close_agent` ist kein normaler Benutzerbefehl. Der Workflow verwendet das
Agentenwerkzeug intern, um abgeschlossene Child-Slots freizugeben. Ein aktiver
Workflow-Agent darf erst geschlossen werden, nachdem sein Ergebnis validiert
oder endgültig verworfen wurde.

## Diagnose in sinnvoller Reihenfolge

Führe bei einem Start- oder Laufzeitproblem zuerst diese nicht destruktiven
Prüfungen aus:

```powershell
Get-Command codex-extended-workflow | Format-List Source, Version
codex-extended-workflow --version
codex-extended-workflow doctor
git rev-parse --show-toplevel
git status --short
```

Danach erst die konkrete Fehlerklasse bearbeiten. Datenbanken, Sitzungen oder
Konfigurationsverzeichnisse niemals auf Verdacht löschen.

## Fehler: nicht in einem Git-Worktree

Ausgabe:

```text
Repository preparation failed: not inside a Git worktree; run this command
from a repository or pass --cd <DIR>
```

Ursache: `prepare-extend-workflow` wurde außerhalb eines Git-Repositories
ausgeführt oder das falsche Ziel wurde gewählt.

Lösung:

```powershell
Set-Location D:\Pfad\zum\Repository
git rev-parse --show-toplevel
codex-extended-workflow prepare-extend-workflow
```

Alternativ:

```powershell
codex-extended-workflow prepare-extend-workflow --cd D:\Pfad\zum\Repository
```

## Fehler: Code Mode Host fehlt

Ausgabe ähnlich:

```text
Code Mode is unavailable because failed to spawn code-mode host ...
codex-code-mode-host.exe: host executable was not found.
```

Ursache: Meist wurde nur die CLI gebaut oder mit `cargo install` installiert,
ohne den separaten Host zu paketieren.

Lösung: Das vollständige Releasepaket mit dem offiziellen Installer dieses
Forks erneut installieren und danach prüfen:

```powershell
codex-extended-workflow --version
codex-extended-workflow doctor
```

Das bloße Aktivieren eines Features erzeugt keine fehlende EXE.

## Fehler: lokale Datenbank kann nicht initialisiert werden

Ausgabe ähnlich:

```text
Codex couldn't start because its local database appears to be damaged.
```

Vorgehen:

1. Alle laufenden Instanzen der erweiterten CLI beenden.
2. `codex-extended-workflow doctor` ausführen und die technische Ursache lesen.
3. Vor jeder Reparatur das gesamte Verzeichnis
   `~/.codex-extended-workflow` sichern.
4. Prüfen, ob die installierte Version zur Version passt, mit der die Datenbank
   zuletzt geöffnet wurde.
5. Keine `state_*.sqlite`-Datei löschen, solange Sitzungen oder Nachweise noch
   benötigt werden.

Eine erneute Installation ersetzt die Binärdateien, repariert aber nicht
automatisch einen bereits inkonsistenten Datenbankstand.

## Warnung: projektlokale Schlüssel werden ignoriert

Ausgabe ähnlich:

```text
Ignored unsupported project-local config keys ...: notify, profiles
```

Das bedeutet: Die Datei wurde gefunden, aber bestimmte allgemeine
Benutzereinstellungen dürfen aus Sicherheits- oder Vertrauensgründen nicht aus
einem Repository übernommen werden. Es ist kein Fehler der verschachtelten
`extended_workflow.experts.profiles` oder
`extended_workflow.reviewers.profiles`.

Verschiebe die genannten allgemeinen Schlüssel in die globale
Benutzerkonfiguration und lasse in der Repository-Datei nur die dort erlaubten
Projektwerte. Die Warnung verhindert, dass ein Checkout unbemerkt persönliche
Benachrichtigungen oder globale Profile umkonfiguriert.

## Meldung: Bericht ungültig, Rejected oder Excluded

Diese Statuswerte bedeuten nicht automatisch, dass der Agent die Änderung
fachlich abgelehnt hat. Der separate Validator konnte den Beitrag nicht als
gültigen Workflow-Nachweis akzeptieren.

Typische Reason Codes:

| Code | Bedeutung | Sinnvolle Reaktion |
|---|---|---|
| `task_binding_mismatch` | Subject oder Inhalt passt nicht zur gebundenen Aufgabe | konkrete Root-Aufgabe und Änderungsgegenstand verwenden |
| `schema_consistency` | Verdict, Befunde oder Schweregrad widersprechen einander | strukturierten Bericht konsistent korrigieren |
| `invalid_report_format` | erwartetes JSON/Schema nicht erfüllt | nur den geforderten Vertrag liefern |
| `missing_evidence` | Behauptung ohne benötigten Nachweis | Dateistellen, Tests oder konkrete Beobachtung ergänzen |
| `size_limit_exceeded` | Ergebnis überschreitet `max_result_bytes` | präziser und kürzer berichten |
| `terminal_output_mismatch` | validiertes Ergebnis ist nicht die endgültige Agentenausgabe | korrigiertes JSON als letzte Ausgabe liefern |
| `change_digest_mismatch` | Bericht gehört zu einem anderen Working-Tree-Stand | neuen Digest planen und Review erneut ausführen |
| `correction_exhausted` | einziger Korrekturversuch wurde verbraucht | Workflow stoppt; Benutzerentscheidung nötig |
| `technical_failure` | Nachweis konnte technisch nicht sicher geprüft werden | Laufzeitproblem zuerst beheben |

Kurze Folgeprompts wie nur `1`, `ja` oder `weiter` können für Menschen eindeutig
wirken, sind aber ohne gebundenen Kontext als Reviewgegenstand ungeeignet.
Formuliere eine umzusetzende Auswahl möglichst vollständig:

```text
Bitte Option 1 umsetzen: Ändere den Git-Abschnitt auf einen vollständigen
git diff --cached und teste ihn mit einer bereits vorgemerkten Fremddatei.
```

## Workflow stoppt wegen geändertem Digest

Wenn der Working-Tree-Diff nach der Reviewer-Planung verändert wurde, passt der
aktuelle Digest nicht mehr zum geprüften Stand. Das ist eine beabsichtigte
Sicherheitsfunktion.

Erforderlich ist:

1. Änderung und Tests fertigstellen,
2. neuen Reviewerplan auf dem aktuellen Diff erstellen,
3. alle erforderlichen Rollen auf diesem neuen Digest prüfen lassen,
4. Working Tree bis zum Abschluss-Gate unverändert lassen.

Ein alter PASS wird nicht auf den neuen Stand übertragen.

## Workflow-Tokenlimit erreicht

In der interaktiven TUI erscheint eine Auswahl:

```text
+100% des Basislimits
+75% des Basislimits
+50% des Basislimits
+25% des Basislimits
Stop
```

Eine Freigabe erhöht die Obergrenze; sie setzt den bisherigen Verbrauch nicht
zurück. Bei `exec` gibt es keine interaktive Nachfrage. Soll kein hartes Limit
gelten, verwende beim nächsten Start `--workflow-token-limit 0` oder setze
`workflow_budget_tokens = 0`.

`/workflow-reset-budget` ist kein kostenloses Fortsetzen desselben Workflows:
Der aktive Workflow wird dabei aufgegeben. Der nächste Workflow erhält ein
neues Budget, während die Session-Gesamttokens erhalten bleiben.

## Falsche oder alte Version wird gestartet

Prüfen:

```powershell
Get-Command codex-extended-workflow -All
codex-extended-workflow --version
```

Mehrere Treffer bedeuten, dass die `PATH`-Reihenfolge entscheidet. Ein lokales
`target\release\codex-extended-workflow.exe` wird durch einen Debug-Build nicht
automatisch aktualisiert. Für Endanwender ist die verwaltete Releaseinstallation
eindeutiger.

Der Befehl `codex` darf weiterhin auf die offizielle Codex-CLI zeigen. Für
diesen Fork ist ausschließlich `codex-extended-workflow` der eindeutige
Launcher.

## `--shared-state` bewusst verwenden

Standardmäßig sind State und Sitzungen getrennt. Mit `--shared-state` verwendet
die erweiterte CLI den normalen Codex-State und dessen Sitzungen, während die
Anmeldedaten der erweiterten Produktidentität isoliert bleiben.

Verwende den Schalter nur, wenn die gemeinsame Sitzungshistorie ausdrücklich
gewünscht ist. Kombiniere `--shared-state` nicht mit einem eigenen `CODEX_HOME`;
diese mehrdeutige Kombination wird abgewiesen.

## Wenn die Ursache danach noch unklar ist

Sammle ohne Geheimnisse zu veröffentlichen:

- vollständige `--version`-Ausgabe,
- Pfad aus `Get-Command`,
- Ausgabe von `doctor`,
- konkreten Startbefehl,
- relevante Fehlermeldung,
- ob normaler oder `--shared-state`-Modus verwendet wurde,
- Repository-Root und `git status --short`,
- bei Reviewproblemen den angezeigten Reason Code und die ersten Zeichen des
  Digest, aber keine Zugangsdaten.

Zurück zur [Übersicht](README.md).
