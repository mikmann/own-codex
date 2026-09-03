# codex-extended-workflow: deutsche Anleitung

Diese Dokumentation beschreibt den tatsächlich implementierten Quellstand für
`codex-extended-workflow v0.147.1-alpha-10`. Die CLI verwendet die
Codex-Laufzeit, ergänzt sie aber um einen verbindlichen, konfigurierbaren
Experten- und Reviewer-Prozess.

> **Releasegrenze:** Die Installationsbefehle funktionieren nur, wenn der Tag
> `v0.147.1-alpha-10` den dokumentierten Release-Workflow erfolgreich
> durchlaufen hat und die Assets veröffentlicht sind. Andernfalls liefert die
> Release-URL erwartungsgemäß HTTP 404.

Die im ursprünglichen Dokumentationsauftrag genannte Nummer
`v0.141.1-alpha-10` ist mit Workspace und Releasefolge nicht vereinbar. Die
korrekte Zielversion lautet `v0.147.1-alpha-10`.

## Schnell zum Ziel

1. [CLI installieren](01-installation.md)
2. [Repository vorbereiten und ersten Workflow starten](02-schnellstart-und-vorbereitung.md)
3. [Ablauf und Abschlussbedingungen verstehen](03-workflow.md)
4. [Experten, Reviewer und Tokenbudgets konfigurieren](04-konfiguration.md)

## Gesamte Dokumentation

| Dokument | Inhalt |
| --- | --- |
| [01 – Installation](01-installation.md) | Geprüfte Windows-Installation, Update und Diagnose |
| [02 – Schnellstart und Vorbereitung](02-schnellstart-und-vorbereitung.md) | `.codex/config.toml` sicher erstellen und die CLI im Projekt starten |
| [03 – Workflow](03-workflow.md) | Klassifizierung, Experten, Tickets, Implementierung, Reviewer und Abschluss-Gate |
| [04 – Konfiguration](04-konfiguration.md) | Profile, Modelle, Reasoning-Stärke, Min/Max-Werte, Token- und Kontextlimits |
| [05 – Vergleich mit normalem Codex](05-vergleich-mit-codex.md) | Exakte Unterschiede sowie Vor- und Nachteile beider Varianten |
| [06 – Bedienung und Fehlerbehebung](06-bedienung-und-fehlerbehebung.md) | Befehlsreferenz, Statusanzeigen und typische Fehlermeldungen |
| [07 – `.codex/config.toml`-Praxisanleitung](07-config-toml-praxisanleitung.md) | Speicherorte, Vorrang, TOML-Syntax, gültige Beispiele, sichere Anpassung und Fehlerbehebung |
| [08 – Konfigurierbare Workflow-Rollen](08-konfigurierbare-workflow-rollen.md) | `additional_profiles`, `selection_keywords`, sichere Rollendateien, Preflight und Rollenfingerabdruck |

Architekturentscheidungen und ihre Abnahmekriterien liegen getrennt von der
Bedienungsanleitung:

| Konzept | Inhalt |
| --- | --- |
| [Konfigurierbare Workflow-Rollen](concepts/01-konfigurierbare-workflow-rollen.md) | Sicherheits- und Implementierungsgrundlage der Alpha‑10-Rollenerweiterung |
| [Workflow-Typ und Statusausgaben](concepts/02-workflow-typ-und-statusausgaben.md) | Auswahl `auto`, `inactive`, `concept`, `work-order` und eindeutige Gate-Anzeigen |

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

## Diagrammabdeckung

Jeder eigenständige mehrstufige Ablauf besitzt eine inhaltlich gleiche ASCII-
und Mermaid-Fassung:

| Ablauf | Dokument |
| --- | --- |
| Installation und Upgrade | [01 – Installation](01-installation.md#installations--und-upgrade-workflow) |
| Repository-Vorbereitung | [02 – Schnellstart](02-schnellstart-und-vorbereitung.md#vorbereitungsworkflow) |
| Klassifizierung und Hauptpfade | [03 – Workflow](03-workflow.md#klassifizierung-und-hauptpfade) |
| Konzept-Workflow | [03 – Workflow](03-workflow.md#konzept-als-eigenständiger-workflow) |
| Arbeitsauftrag, Review und Same-Digest | [03 – Workflow](03-workflow.md#arbeitsauftrag-mit-reviewer--und-same-digest-gate) |
| Workflow-Tokenbudget | [04 – Konfiguration](04-konfiguration.md#tokenbudget-workflow-als-ascii) |
| Normaler Codex gegenüber Extended Workflow | [05 – Vergleich](05-vergleich-mit-codex.md#vergleich-als-ascii-diagramm) |
| Diagnose | [06 – Fehlerbehebung](06-bedienung-und-fehlerbehebung.md#diagnoseworkflow) |
| Konfigurationsvorrang | [07 – `config.toml`](07-config-toml-praxisanleitung.md#konfigurationsvorrang) |
| Rollenwahl und Rollen-Preflight | [08 – Rollen](08-konfigurierbare-workflow-rollen.md#rollenworkflow) |
| Explizite Workflow-Typwahl und ihre Hauptpfade | [Konzept: Workflow-Typ](concepts/02-workflow-typ-und-statusausgaben.md#ablauf) |

## Weiterführende Quellen

- [Technischer Vertrag dieses Forks](../../EXTENDED_WORKFLOW.md)
- [Offizielle Codex-CLI-Dokumentation](https://learn.chatgpt.com/de-DE/docs/codex/cli)
- [Offizielle Dokumentation zu Codex-Subagents](https://learn.chatgpt.com/de-DE/docs/agent-configuration/subagents)
- [Offizielle Grundlagen zur Codex-Konfiguration](https://learn.chatgpt.com/de-DE/docs/config-file/config-basic)
