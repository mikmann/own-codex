# 02 – Schnellstart und Repository-Vorbereitung

## Was ein Repository benötigt

Für den erweiterten Workflow genügt grundsätzlich diese projektlokale Datei:

```text
<Repository>\
└── .codex\
    └── config.toml
```

Eine zusätzliche `AGENTS.md` oder ein Verzeichnis `.codex/agents` ist für den
Standard-Workflow nicht erforderlich. Solche Dateien können weiterhin für
projektspezifische Anweisungen oder eigene Codex-Agenten verwendet werden, sind
aber kein Installationsbestandteil dieser CLI.

Projektlokale Konfiguration wird nur in einem von Codex als vertrauenswürdig
behandelten Repository geladen. Sicherheitsrelevante oder nicht unterstützte
Schlüssel können projektlokal ignoriert werden und gehören dann in die globale
Benutzerkonfiguration.

## Sichere Vorbereitung in zwei Schritten

Wechsle in den Git-Worktree:

```powershell
Set-Location D:\Pfad\zum\Repository
```

Zuerst nur die geplante Aktion anzeigen:

```powershell
codex-extended-workflow prepare-extend-workflow
```

Wenn die Vorschau korrekt ist, anwenden:

```powershell
codex-extended-workflow prepare-extend-workflow --apply
```

Ohne `--apply` werden keine Dateien verändert. Beim Anwenden wird eine fehlende
`.codex/config.toml` vollständig erstellt.

## Vorbereitung aus einem beliebigen Ordner

Mit `--cd` oder der Kurzform `-C` kann das Ziel explizit angegeben werden:

```powershell
codex-extended-workflow prepare-extend-workflow --cd D:\Pfad\zum\Repository
codex-extended-workflow prepare-extend-workflow --cd D:\Pfad\zum\Repository --apply
```

Das Ziel muss ein Git-Worktree sein. Andernfalls endet die Vorschau bewusst mit:

```text
not inside a Git worktree; run this command from a repository or pass --cd <DIR>
```

## Verhalten bei vorhandener Konfiguration

`prepare-extend-workflow` überschreibt keine vorhandene Konfiguration:

| Ausgangslage | Ergebnis |
|---|---|
| `.codex/config.toml` fehlt | vollständige Vorlage wird geplant bzw. erstellt |
| gültiger `[extended_workflow]`-Abschnitt vorhanden | Datei wird unverändert wiederverwendet |
| Datei vorhanden, aber Abschnitt fehlt | Konflikt; Abschnitt muss manuell ergänzt werden |
| `extended_workflow.enabled = false` | Konflikt; bewusste Deaktivierung wird nicht überschrieben |
| Datei ist ungültiges TOML | Konflikt; zuerst Syntax korrigieren |

Die Vorschau ist an den geprüften Dateistand gebunden. Wird die Datei zwischen
Vorschau und Anwendung verändert, bricht die Anwendung geschlossen ab und muss
neu gestartet werden.

## Globale Standardkonfiguration

Soll die Konfiguration für alle Repositories gelten, kann sie im isolierten
Benutzerbereich der erweiterten CLI vorbereitet werden:

```powershell
codex-extended-workflow prepare-extend-workflow --global
codex-extended-workflow prepare-extend-workflow --global --apply
```

Projektlokale Einstellungen können globale Einstellungen für das jeweilige
vertrauenswürdige Repository überschreiben. CLI-Argumente haben wiederum
Vorrang vor den Konfigurationsdateien.

## Entsprechende TUI-Befehle

In einer laufenden interaktiven Sitzung stehen dieselben Varianten als
Slash-Befehle zur Verfügung:

```text
/prepare-extend-workflow
/prepare-extend-workflow apply
/prepare-extend-workflow global
/prepare-extend-workflow global apply
```

## Ersten Workflow starten

Im vorbereiteten Projekt:

```powershell
codex-extended-workflow
```

Beispielauftrag:

```text
Analysiere die bestehende Importfunktion, erstelle ein umsetzbares Ticket mit
Akzeptanzkriterien, implementiere es und prüfe die Änderung vollständig.
```

Die CLI klassifiziert die Anfrage lokal, zieht bei einem Arbeitsauftrag die
passenden Experten hinzu, lässt den Hauptagenten das Ticket umsetzen und startet
anschließend den Reviewer-Prozess.

## Sitzung fortsetzen

```powershell
codex-extended-workflow resume <SITZUNGS-ID>
```

Wurde die Sitzung ausdrücklich mit normalem Codex-State gestartet:

```powershell
codex-extended-workflow --shared-state resume <SITZUNGS-ID>
```

Das Produktpräfix bleibt damit auch im angezeigten Resume-Befehl eindeutig.

## Nächster Schritt

Der vollständige Ablauf ist in [03 – Workflow](03-workflow.md) dargestellt.
