# codex-extended-workflow: deutsche Anleitung

Diese Dokumentation beschreibt die Installation und Verwendung von
`codex-extended-workflow` in der Version `v0.147.1-alpha-9`. Die CLI verwendet
die Codex-Laufzeit, ergänzt sie aber um einen verbindlichen, konfigurierbaren
Experten- und Reviewer-Prozess.

> **Alpha-Hinweis:** Die hier beschriebenen Befehle und Konfigurationsschlüssel
> gelten für Alpha‑9. Vor einem späteren Upgrade sollten die Release Notes und
> `codex-extended-workflow --help` geprüft werden.

## Schnell zum Ziel

1. [CLI installieren](01-installation.md)
2. [Repository vorbereiten und ersten Workflow starten](02-schnellstart-und-vorbereitung.md)
3. [Ablauf und Abschlussbedingungen verstehen](03-workflow.md)
4. [Experten, Reviewer und Tokenbudgets konfigurieren](04-konfiguration.md)

## Gesamte Dokumentation

| Dokument | Inhalt |
|---|---|
| [01 – Installation](01-installation.md) | Geprüfte Windows-Installation, Update und Diagnose |
| [02 – Schnellstart und Vorbereitung](02-schnellstart-und-vorbereitung.md) | `.codex/config.toml` sicher erstellen und die CLI im Projekt starten |
| [03 – Workflow](03-workflow.md) | Klassifizierung, Experten, Tickets, Implementierung, Reviewer und Abschluss-Gate |
| [04 – Konfiguration](04-konfiguration.md) | Profile, Modelle, Reasoning-Stärke, Min/Max-Werte, Token- und Kontextlimits |
| [05 – Vergleich mit normalem Codex](05-vergleich-mit-codex.md) | Exakte Unterschiede sowie Vor- und Nachteile beider Varianten |
| [06 – Bedienung und Fehlerbehebung](06-bedienung-und-fehlerbehebung.md) | Befehlsreferenz, Statusanzeigen und typische Fehlermeldungen |

## Kurzüberblick

```text
codex                            codex-extended-workflow
  |                                        |
  +-- flexible Agentenarbeit               +-- Anfrage klassifizieren
  +-- Subagents bei Bedarf                 +-- Experten bei Konzept/Auftrag
  +-- Review auf Anforderung               +-- Ticket/Konzept validieren
                                           +-- Hauptagent implementiert
                                           +-- Reviewer prüfen denselben Diff
                                           +-- Abschluss nur bei erfülltem Gate
```

Der erweiterte Workflow ist besonders sinnvoll, wenn eine Änderung mehrere
Fachgebiete betrifft, Akzeptanzkriterien nachvollziehbar sein müssen oder ein
unabhängiger Review vor dem Abschluss vorgeschrieben ist. Für eine kurze,
einfache Frage ist der normale Codex-Ablauf meist schneller und günstiger.

## Wichtige Abgrenzungen

- `codex-extended-workflow` ist kein anderes Sprachmodell und keine zweite
  Implementierung der Codex-Laufzeit.
- Experten und Reviewer arbeiten ausschließlich lesend. Nur der Hauptagent darf
  die eigentliche Änderung durchführen.
- Ein fachlich positiver Text ist erst dann ein gültiger Experten- oder
  Reviewer-Beitrag, wenn auch Struktur, Aufgabenbindung und Evidenz validiert
  wurden.
- Das Standard-Datenverzeichnis ist von normalem Codex getrennt:
  `~/.codex-extended-workflow` statt `~/.codex`.
- Ein Tokenlimit von `0` bedeutet unbegrenzt; ein positives Workflowlimit gilt
  gemeinsam für Hauptagent, Experten, Reviewer und Kompaktierungen.

## Weiterführende Quellen

- [Technischer Vertrag dieses Forks](../../EXTENDED_WORKFLOW.md)
- [Offizielle Codex-CLI-Dokumentation](https://learn.chatgpt.com/de-DE/docs/codex/cli)
- [Offizielle Dokumentation zu Codex-Subagents](https://learn.chatgpt.com/de-DE/docs/agent-configuration/subagents)
- [Offizielle Grundlagen zur Codex-Konfiguration](https://learn.chatgpt.com/de-DE/docs/config-file/config-basic)
