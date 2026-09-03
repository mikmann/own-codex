# 03 – Der erweiterte Workflow

## Grundidee

Der Workflow trennt Beratung, Umsetzung und Prüfung in klar erkennbare Phasen:

- **Experten** beraten den Hauptagenten bei Konzepten, Entscheidungen und
  Tickets.
- **Der Hauptagent** integriert die Ergebnisse und ist der einzige Agent, der
  Änderungen umsetzt.
- **Reviewer** prüfen die tatsächliche Änderung unabhängig und ausschließlich
  lesend.
- **Der Validator** prüft zusätzlich Schema, Pflichtfelder, Evidenz,
  Größenlimit, Agentenidentität sowie Aufgaben- und gegebenenfalls
  Digest-Bindung. Diese Prüfung garantiert nicht die Wahrheit jeder
  fachlichen Aussage.

Die Anfrage wird lokal und deterministisch klassifiziert. Dafür wird kein
zusätzlicher Modellaufruf verbraucht.

## Ablaufdiagramme in Mermaid und ASCII

Der Gesamtprozess ist bewusst auf drei inhaltliche Diagramme verteilt. Ein einzelner Graph
mit allen drei Klassen, Korrekturschleifen und Rücksprüngen erzeugt bei Mermaid
ein sehr hohes, schlecht lesbares Layout. Das erste Diagramm zeigt die
Klassifizierung, das zweite den eigenständigen Konzeptpfad und das dritte den
vollständigen Arbeitsauftrag einschließlich Same-Digest-Abschluss. Jede Sicht
steht in Mermaid und mit derselben Logik als reine ASCII-Fassung zur Verfügung.

### Klassifizierung und Hauptpfade

```mermaid
flowchart TD
    A["Benutzeranfrage"] --> B{"Lokale Klassifizierung"}
    B -->|"simple_answer"| S0{"Echte Mehrdomänen-Komplexität?"}
    S0 -->|"Nein"| S1["Direkte Antwort durch den Hauptagenten"]
    S0 -->|"Ja"| S2["Optionale passende Experten"]
    S2 --> S1

    B -->|"concept"| C1["Passende Experten gemäß min und max"]
    C1 --> C2["Validiertes Konzept"]
    C2 --> C3["Konzept-Turn abgeschlossen"]

    B -->|"work_order"| W1["Passende Experten gemäß min und max"]
    W1 --> W2["Validiertes Ticket oder Ticket-Serie"]
    W2 --> W3["Hauptagent implementiert und testet"]
    W3 --> W4["Reviewer prüfen den gebundenen Diff"]
    W4 --> W5["Same-Digest-Gate oder Benutzerentscheidung"]
```

### Klassifizierung und Hauptpfade als ASCII

```text
[BENUTZERANFRAGE]
        |
        v
{LOKALE KLASSIFIZIERUNG}
        |
        +-- simple_answer --> {ECHTE MEHRDOMÄNEN-KOMPLEXITÄT?}
        |                           |
        |                           +-- Nein --> [DIREKTE ANTWORT]
        |                           |
        |                           +-- Ja --> [OPTIONALE PASSENDE EXPERTEN]
        |                                          |
        |                                          v
        |                                   [DIREKTE ANTWORT]
        |
        +-- concept --------> [PASSENDE EXPERTEN GEMÄSS MIN/MAX]
        |                           |
        |                           v
        |                    [VALIDIERTES KONZEPT]
        |                           |
        |                           v
        |                    [KONZEPT-TURN FERTIG]
        |
        +-- work_order -----> [PASSENDE EXPERTEN GEMÄSS MIN/MAX]
                                    |
                                    v
                             [VALIDIERTES TICKET
                              ODER TICKET-SERIE]
                                    |
                                    v
                             [HAUPTAGENT IMPLEMENTIERT
                              UND TESTET]
                                    |
                                    v
                             [REVIEWER PRÜFEN DEN
                              GEBUNDENEN DIFF]
                                    |
                                    v
                             [SAME-DIGEST-GATE ODER
                              BENUTZERENTSCHEIDUNG]
```

### Konzept als eigenständiger Workflow

#### ASCII

```text
[KONZEPTANFRAGE ODER WORKFLOW-TYP concept]
                    |
                    v
        [Root-Aufgabe eindeutig binden]
                    |
                    v
       [passende Experten gemäß min/max]
                    |
                    v
         {Berichte vom Validator akzeptiert?}
              |                    |
            Nein                   Ja
              |                    |
 [genau ein Korrekturversuch]      v
              |          [Findings auswerten und auflösen]
              v                    |
 {Korrektur akzeptiert?}           v
       |          |        [Hauptagent erstellt Konzept]
     Nein         Ja               |
       |          +----------------+
    [STOP]                         v
                       {Konzeptvertrag validiert?}
                           |               |
                         Nein              Ja
                           |               |
             [genau ein Korrekturversuch]  |
                           |               |
                           v               |
                  {Korrektur gültig?}      |
                     |           |         |
                   Nein          Ja -------+
                     |                     |
                  [STOP]                    v
                                  [Konzept abgeschlossen]
                                            |
                                            v
                                    optional: Tickets ableiten
```

#### Mermaid

```mermaid
flowchart TD
    A[Konzeptanfrage oder Workflow-Typ concept] --> B[Root-Aufgabe eindeutig binden]
    B --> C[Passende Experten gemäß min und max]
    C --> D{Berichte vom Validator akzeptiert?}
    D -->|Nein| E[Genau ein Korrekturversuch]
    E --> F{Korrektur akzeptiert?}
    F -->|Nein| G[STOP]
    F -->|Ja| H[Findings auswerten und auflösen]
    D -->|Ja| H
    H --> I[Hauptagent erstellt Konzept]
    I --> J{Konzeptvertrag validiert?}
    J -->|Nein| K[Genau ein Korrekturversuch]
    K --> K2{Korrektur gültig?}
    K2 -->|Nein| G
    K2 -->|Ja| L
    J -->|Ja| L[Konzept abgeschlossen]
    L --> M[Optional Tickets ableiten]
```

Beide Diagramme zeigen denselben Konzeptablauf. Ein Konzept-Turn verändert
keine Dateien allein aufgrund seiner Klasse. Eine spätere Umsetzung benötigt
einen ausdrücklichen Arbeitsauftrag oder den Workflow-Typ `work-order`.

### Arbeitsauftrag mit Reviewer- und Same-Digest-Gate

```mermaid
flowchart TD
    A["1 · Arbeitsauftrag"] --> B["2 · Passende Experten gemäß min und max beraten read-only"]
    B --> C{"Akzeptiert der Validator Schema, Bindung und Evidenz?"}
    C -->|Ja| C1["Gültig: Beiträge und Findings auswerten"]
    C -->|Nein| C2["Ungültig:<br/>genau ein Korrekturversuch"]
    C2 --> C3{"Korrigierter Bericht gültig?"}
    C3 -->|Ja| C1
    C3 -->|Nein| Z1["Weiterhin ungültig:<br/>Workflow stoppt"]

    C1 --> D["3 · Konzept, falls verlangt, und<br/>aktuelles Ticket erstellen"]
    D --> D1{"Artefaktvertrag gültig?"}
    D1 -->|Ja| E["4 · Nur der Hauptagent<br/>implementiert und testet"]
    D1 -->|Nein| D2["Genau ein Artefakt-Korrekturversuch"]
    D2 --> D3{"Korrigiertes Artefakt gültig?"}
    D3 -->|Ja| E
    D3 -->|Nein| Z1
    E --> F["5 · Finalen Working-Tree-Diff<br/>binärsensitiv hashen"]
    F --> G["6 · Passende Reviewer gemäß min und max prüfen exakt diesen Digest read-only"]
    G --> H{"Reviewerberichte formal gültig?"}
    H -->|Ja| H1["Gültig:<br/>inhaltliches Review auswerten"]
    H -->|Nein| H2["Ungültig:<br/>genau ein gezielter Berichtskorrekturversuch"]
    H2 --> H3{"Korrigierter Bericht gültig?"}
    H3 -->|Ja| H1
    H3 -->|Nein| Z1

    H1 --> I{"PASS und leere Befundliste?"}
    I -->|Ja| I1["PASS:<br/>Reviewer-Nachweis ist zulässig"]
    I -->|Nein| J["Befunde vorhanden:<br/>Auflösung nach Schweregrad"]
    J --> J1["High oder Critical:<br/>zwingend beheben"]
    J --> J2["Low oder Medium:<br/>beheben oder konkret disponieren"]
    J1 --> K["Code korrigieren<br/>und Tests erneut ausführen"]
    J2 -->|beheben| K
    J2 -->|disponieren| L["Konkrete Disposition<br/>ohne Diff-Änderung"]
    K --> M["Diff geändert:<br/>neuer Digest und neue Reviewrunde"]
    M --> G
    L --> N["Derselbe Reviewer liefert<br/>genau eine Re-Review-Ausgabe"]
    N --> O{"Befunde geschlossen?"}
    O -->|Ja| I1
    O -->|Nein| Z2["Weiterhin offen:<br/>Benutzerentscheidung erforderlich"]

    I1 --> P["7 · Aktuellen Working-Tree-Digest<br/>beim Abschluss erneut berechnen"]
    P --> Q{"Alle benötigten Rollen PASS<br/>auf demselben aktuellen Digest?"}
    Q -->|Ja| X["Workflow erfolgreich abgeschlossen"]
    Q -->|Nein| Y["Nachweis fehlt oder ist veraltet:<br/>neue Reviewrunde oder Benutzerentscheidung"]
```

### Arbeitsauftrag mit Reviewer- und Same-Digest-Gate als ASCII

```text
[1. ARBEITSAUFTRAG]
          |
          v
[2. PASSENDE EXPERTEN GEMÄSS MIN/MAX BERATEN READ-ONLY]
          |
          v
{AKZEPTIERT DER VALIDATOR SCHEMA, BINDUNG UND EVIDENZ?}
          |
          +-- Nein --> [genau ein gezielter Korrekturversuch]
          |                         |
          |                         v
          |             {KORRIGIERTER BERICHT GÜLTIG?}
          |                         |
          |                         +-- Nein --> [STOP]
          |                         |
          |                         +-- Ja ------+
          |                                      |
          +-- Ja --------------------------------+
                                                 |
                                                 v
[3. KONZEPT, FALLS VERLANGT, UND AKTUELLES TICKET ERSTELLEN]
          |
          v
{IST DER ARTEFAKTVERTRAG GÜLTIG?}
          |
          +-- Nein --> [genau ein Artefakt-Korrekturversuch]
          |                         |
          |                         v
          |             {KORRIGIERTES ARTEFAKT GÜLTIG?}
          |                         |
          |                         +-- Nein --> [STOP]
          |                         |
          |                         +-- Ja ------+
          |                                      |
          +-- Ja --------------------------------+
                                                 |
                                                 v
[4. NUR DER HAUPTAGENT IMPLEMENTIERT UND TESTET]
          |
          v
[5. FINALEN WORKING-TREE-DIFF BINÄRSENSITIV HASHEN]
          |
          v
[6. PASSENDE REVIEWER GEMÄSS MIN/MAX PRÜFEN EXAKT DIESEN DIGEST READ-ONLY]
          |
          v
{SIND ALLE REVIEWERBERICHTE FORMAL GÜLTIG?}
          |
          +-- Nein --> [genau ein gezielter Berichtskorrekturversuch]
          |                         |
          |                         v
          |             {KORRIGIERTER BERICHT GÜLTIG?}
          |                         |
          |                         +-- Nein --> [STOP]
          |                         |
          |                         +-- Ja ------+
          |                                      |
          +-- Ja --------------------------------+
                                                 |
                                                 v
{VERDICT = PASS UND BEFUNDLISTE LEER?}
          |
          +-- Ja --> [Reviewer-Nachweis ist zulässig] ----------------+
          |                                                            |
          +-- Nein --> [BEFUNDE NACH SCHWERE AUFLÖSEN]                 |
                             |                                         |
                             +-- High/Critical --> zwingend beheben     |
                             |              |                           |
                             +-- Low/Medium -----> beheben              |
                             |              |                           |
                             |              v                           |
                             |     [Tests + neuer Diff-Digest]          |
                             |              |                           |
                             |              +--> [neue Reviewrunde]     |
                             |                     zurück zu Schritt 6  |
                             |                                          |
                             +-- Low/Medium --> konkrete Disposition    |
                                            ohne Diff-Änderung          |
                                                   |                    |
                                                   v                    |
                                      [derselbe Reviewer: eine Re-Review]
                                                   |                    |
                                                   v                    |
                                      {BEFUNDE GESCHLOSSEN?}            |
                                                   |                    |
                                                   +-- Nein -->         |
                                                   |   [Benutzer-       |
                                                   |    entscheidung]   |
                                                   |                    |
                                                   +-- Ja --------------+
                                                                        |
                                                                        v
[7. AKTUELLEN WORKING-TREE-DIGEST BEIM ABSCHLUSS ERNEUT BERECHNEN]
          |
          v
{ALLE BENÖTIGTEN REVIEWERROLLEN PASS AUF DEMSELBEN AKTUELLEN DIGEST?}
          |
          +-- Ja   --> [WORKFLOW ERFOLGREICH ABGESCHLOSSEN]
          |
          +-- Nein --> [neue Reviewrunde oder Benutzerentscheidung]
```

## Phase 1: Anfrage binden und klassifizieren

Die unveränderliche Aufgabenbindung verhindert, dass ein späterer Experte oder
Reviewer an einer anderen Fragestellung arbeitet. Kurze Fortsetzungen wie
`ja`, `weiter` oder `Option 1` werden mit dem vorherigen Arbeitskontext
verbunden. Ist die Fortsetzung nicht eindeutig, muss der Workflow nachfragen
oder geschlossen stoppen.

Die Klassen haben folgende Wirkung:

| Klasse | Typischer Ablauf |
| --- | --- |
| Einfache Antwort (`simple_answer`) | Hauptagent antwortet direkt; Experten nur bei echter zusätzlicher Komplexität |
| Konzept (`concept`) | Experten beraten; Hauptagent erstellt ein validiertes Konzept, optional danach Tickets |
| Arbeitsauftrag (`work_order`) | Experten beraten; Hauptagent erstellt ein oder mehrere umsetzbare Tickets |

## Konkrete Alpha‑10-Beispiele für die drei Klassen

Die Klassifizierung folgt dem Inhalt und der gewünschten Aktion, nicht nur
einzelnen Schlüsselwörtern. Die folgenden Beispiele zeigen den erwarteten
Ablauf des Alpha‑10-Quellkandidaten bei aktivierter Standardkonfiguration.
`auto` wählt die Klasse lokal; `concept` und `work-order` können sie über
`--workflow-type` oder `/workflow-type` ausdrücklich erzwingen. `inactive`
umgeht die Extended-Workflow-Orchestrierung für neue Root-Anfragen.

### Beispiel 1: Einfache Antwort (`simple_answer`)

Benutzerprompt:

```text
Erkläre mir kurz den Unterschied zwischen `git fetch` und `git pull`.
Ändere keine Dateien.
```

Erwarteter Ablauf:

1. Die lokale Klassifizierung ergibt `simple_answer`.
2. Der Hauptagent beantwortet die Frage direkt.
3. Da die Frage nur ein klar begrenztes Fachgebiet betrifft, werden keine
   Experten benötigt.
4. Es wird weder ein Konzept noch ein Ticket erzeugt.
5. Da keine Arbeitsänderung beauftragt wurde, ist kein Reviewer-Gate
   erforderlich.

Bei einer einfachen Frage, die dennoch mehrere unabhängige Fachgebiete oder ein
erhöhtes Risiko berührt, darf der Hauptagent passende Experten hinzuziehen. Die
Klasse bleibt trotzdem `simple_answer`, solange weder ein Konzept noch eine
Umsetzung verlangt wird.

### Beispiel 2: Konzept (`concept`)

Benutzerprompt:

```text
Erstelle ein technisches Konzept für einen revisionssicheren Dokumentenimport
mit Virenprüfung, Dublettenerkennung und nachvollziehbarer Fehlerbehandlung.
Noch nichts implementieren und noch keine Dateien ändern.
```

Erwarteter Ablauf:

1. Die lokale Klassifizierung ergibt `concept`.
2. Gemäß Standardkonfiguration werden zwei bis vier passende Experten ausgewählt, zum
   Beispiel Architektur, Sicherheit sowie Testing und Kompatibilität.
3. Jeder Experte liefert einen vollständigen strukturierten Bericht. Ungültige
   Berichte erhalten höchstens einen gezielten Korrekturversuch.
4. Der Hauptagent integriert die akzeptierten Beiträge in ein strukturiertes
   Konzept mit Zielbild, Entscheidungen, Risiken und offenen Punkten.
5. Das Konzept wird als Workflow-Artefakt validiert.
6. Der Konzept-Turn ist damit abgeschlossen. Eine Implementierung beginnt erst,
   wenn der Benutzer daraus ausdrücklich einen Arbeitsauftrag macht.

Optional kann das Konzept bereits validierte Ticketvorschläge enthalten. Diese
werden aber in einem reinen Konzept-Turn nicht automatisch implementiert.

### Beispiel 3: Arbeitsauftrag (`work_order`)

Benutzerprompt:

```text
Implementiere für den Dokumentenimport ein konfigurierbares Request-Timeout.
Erstelle zuerst ein umsetzbares Ticket mit Akzeptanzkriterien, ändere den Code,
ergänze passende Tests und prüfe die fertige Änderung vollständig.
```

Erwarteter Ablauf:

1. Die lokale Klassifizierung ergibt `work_order`.
2. Gemäß Standardkonfiguration beraten zwei bis vier zur Änderung passende
   Experten den Hauptagenten.
3. Der Hauptagent erstellt mindestens ein validiertes Ticket mit
   Akzeptanzkriterien und Prüfschritten. Bei mehreren Tickets wird genau ein
   aktuelles Ticket ausgewählt.
4. Nur der Hauptagent verändert den Working Tree und führt die relevanten Tests
   aus.
5. Aus dem finalen binärsensitiven Working-Tree-Diff wird ein Digest gebildet.
6. Gemäß Standardkonfiguration prüfen zwei bis vier fachlich unterschiedliche
   Read-only-Reviewer exakt diesen Digest.
7. Low- und Medium-Befunde werden behoben oder konkret disponiert und erneut
   geprüft. High- und Critical-Befunde müssen behoben werden.
8. Ändert eine Korrektur den Diff, wird eine neue Reviewrunde an den neuen
   Digest gebunden.
9. Der Workflow ist erst abgeschlossen, wenn alle benötigten Reviewerrollen auf
   demselben aktuellen Digest gültig mit PASS und leerer Befundliste vorliegen.

## Phase 2: Expertenprozess

Der Hauptagent wählt abhängig von den betroffenen Fachgebieten zwischen dem
konfigurierten Minimum und Maximum passende Expertenprofile aus. Standardmäßig
sind das mindestens zwei und höchstens vier.

Alpha‑10 berücksichtigt dabei optionale `selection_keywords`, anschließend den
bestehenden Domänen-/Risikoscore und zuletzt die Deklarationsreihenfolge. Vor
dem ersten Spawn werden alle ausgewählten Rollen, Modelle, Reasoning-Stufen,
Kontextgrenzen und Rollenfingerabdrücke gemeinsam geprüft. Scheitert ein
Profil, startet die gesamte Runde nicht teilweise.

Ein Expertenbeitrag muss:

- zur gebundenen Anfrage und zur zugewiesenen Rolle passen,
- eine vollständige, fachlich verwertbare Antwort liefern,
- dem erwarteten strukturierten Datenvertrag entsprechen,
- innerhalb des Ergebnis- und Kontextlimits bleiben.

Bei einem ungültigen Bericht erhält derselbe Agent genau einen
Korrekturversuch. Ein formal ungültiger Bericht bedeutet nicht automatisch,
dass seine fachliche Schlussfolgerung falsch war; er kann beispielsweise am
Schema, an fehlender Evidenz oder an einer unklaren Aufgabenbindung scheitern.

## Phase 3: Konzept, Ticket und Implementierung

Der Hauptagent fasst die gültigen Expertenbeiträge zusammen. Bei einem
Arbeitsauftrag entstehen daraus Tickets mit eindeutigem Gegenstand,
Akzeptanzkriterien und Prüfschritten. Anschließend setzt nur der Hauptagent das
aktuelle Ticket um.

Experten und Reviewer dürfen nicht parallel eigene Varianten in den
Arbeitsbaum schreiben. Diese Trennung verhindert konkurrierende Änderungen und
macht die spätere Prüfung eindeutig.

## Phase 4: Reviewer-Prozess

Nach der Umsetzung wird aus dem tatsächlichen, binärsensitiven Arbeitsbaum ein
Digest gebildet. Erst dann wählt der Hauptagent passende, voneinander
verschiedene Reviewer-Rollen. Jeder Reviewer erhält denselben gebundenen Stand
und arbeitet read-only.

Ein Reviewer-PASS ist nur gültig, wenn:

1. der Bericht strukturell valide ist,
2. die Aufgaben- und Digest-Bindung stimmt,
3. `verdict` auf `pass` steht,
4. die Liste der Befunde leer ist.

### Warum nur ein gezielter Berichtskorrekturversuch?

Die Begrenzung bedeutet nicht, dass insgesamt nur ein Bericht erlaubt ist. Der
Reviewer liefert zunächst seinen ursprünglichen Bericht. Lehnt der Validator
diesen formal ab, nennt er die konkreten Fehler. Danach darf derselbe Reviewer
genau eine neue, gezielt korrigierte Ausgaberevision einreichen. Es sind somit
höchstens zwei Einreichungen für denselben gebundenen Auftrag zulässig.

Eine formale Ablehnung kann mehr als einen reinen Darstellungsfehler betreffen.
Mögliche Ursachen sind ein ungültiges Schema, fehlende Pflichtfelder oder
Evidenz, eine falsche Aufgaben- beziehungsweise Digest-Bindung oder ein
überschrittenes Ergebnislimit. Deshalb verwendet die Anleitung den genaueren
Begriff **Berichtskorrekturversuch** statt **Format-Korrekturversuch**.

Die feste Grenze verhindert unbegrenzte automatische Schleifen, hält Laufzeit
und Tokenverbrauch berechenbar und bewahrt einen eindeutigen Auditpfad. Bleibt
auch die korrigierte Revision formal ungültig, wird sie mit
`correction_exhausted` ausgeschlossen. Der Workflow senkt den Prüfstandard
nicht stillschweigend ab, sondern stoppt für eine Benutzerentscheidung.

Davon zu unterscheiden sind fachliche Re-Reviews:

| Situation | Folge |
| --- | --- |
| Bericht ist formal ungültig | ursprünglicher Bericht plus höchstens ein gezielter Berichtskorrekturversuch |
| Bericht ist formal gültig, enthält aber Befunde | Befunde beheben oder disponieren und begrenzte Re-Review durch denselben Reviewer |
| Eine Behebung verändert den Working Tree | neuer Digest und vollständig neu gebundene Reviewrunde |

Ändert sich der Arbeitsbaum nach einem Review, ist der alte PASS nicht mehr der
Abschlussnachweis. Für den neuen Digest ist eine neue gebundene Reviewrunde
erforderlich.

## Befunde und Schweregrade

Reviewer beschreiben Befunde anhand von Auswirkung, Eintrittswahrscheinlichkeit
und Wiederherstellbarkeit. Daraus wird der Schweregrad konsistent abgeleitet:

| Punkte | Schweregrad | Bedeutung für den Workflow |
| ---: | --- | --- |
| 0–2 | Low | muss behoben oder ausdrücklich disponiert und erneut geprüft werden |
| 3–4 | Medium | muss behoben oder ausdrücklich disponiert und erneut geprüft werden |
| 5–6 | High | releasekritisch; muss behoben und erneut geprüft werden |
| 7–9 | Critical | releasekritisch; muss behoben und erneut geprüft werden |

Auch Low- und Medium-Befunde werden nicht stillschweigend ignoriert. Bei einer
begründeten Disposition ohne Diff-Änderung liefert derselbe Reviewer eine neue
Ausgaberevision und prüft sie einmal erneut. Verändert eine Fehlerkorrektur den
Working Tree, ist der bisherige Reviewerplan nicht mehr aktuell: Die CLI bindet
eine neue Reviewrunde an den neuen Digest, und alle für den Abschluss benötigten
Rollen müssen gültige Nachweise für diesen Stand liefern. Bleibt ein Befund nach
der zulässigen Re-Review offen, endet die automatische Schleife und der Benutzer
muss entscheiden.

## Sichtbare Agenten- und Reviewausgabe

Die TUI zeigt jeden ausgewählten Agenten in einer kompakten Zeile:

```text
reviewer_3: Reviewer | Testing and regression | gpt-5.5 | high | PASS
```

Dabei werden Name, Typ, Rolle, Modell, Reasoning-Stärke und Status getrennt
dargestellt. Farben unterstützen die Orientierung; der Text bleibt jedoch die
verbindliche Information, etwa bei `NO_COLOR` oder in einem Terminal ohne
Farbfähigkeit.

Typische Statusinformationen sind:

- `Validated`: ein Konzept oder Ticket erfüllt den strukturellen Vertrag; es
  ist dadurch noch nicht genehmigt, implementiert oder getestet
- Reviewer-`PASS`: ein formal gültiger, korrekt digest-gebundener Bericht
  enthält keine Befunde; das Gesamtquorum kann trotzdem noch offen sein
- `ELIGIBLE`: der einzelne Beitrag darf im nächsten Prozessschritt verwendet
  oder für das Quorum gezählt werden
- `Needs resolution`: Befund muss bearbeitet oder disponiert werden
- `Rejected` oder `Invalid`: Bericht hat den Daten- oder Bindungsvertrag nicht erfüllt
- `Low`, `Medium`, `High`, `Critical`: maximaler Schweregrad eines Befunds

Die vier Ebenen Agentenlaufzeit, formale Validierung, Einzel-Gate und
Gesamtquorum werden mit Beispielen unter
[Workflow-Ausgaben richtig lesen](06-bedienung-und-fehlerbehebung.md#workflow-ausgaben-richtig-lesen)
erklärt.

## Abschluss-Gate

Ein Arbeitsworkflow ist erst erfolgreich abgeschlossen, wenn alle ausgewählten
Reviewer auf demselben aktuellen Digest gültig mit PASS und ohne Befunde
geantwortet haben. Ein laufender Test, ein Commit oder ein einzelner grüner
Reviewer ersetzt dieses gemeinsame Gate nicht.

Weiter mit [04 – Konfiguration](04-konfiguration.md).
