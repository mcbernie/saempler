# Changelog

Alle nennenswerten Änderungen an Sämpler. Das Format folgt lose
[Keep a Changelog](https://keepachangelog.com/de/1.1.0/), die Versionen
[Semantic Versioning](https://semver.org/lang/de/).

## [Unveröffentlicht]

### Hinzugefügt

- Host-Automation: zwanzig feste Bänke, eine je Slice, mit Gain, Pitch, Speed,
  Cutoff und Reverb-Anteil, dazu die vier Send-Rückwege. Alle Werte sind
  Versatz auf die Einstellung der Cell, nicht ihr Ersatz.
- Modifier sind einstellbar: Länge von Stutter, Repeat und Brake sowie die
  Half-Time-Rate, gespeichert mit dem Projekt.
- Mod-Matrix erreicht Filter-Cutoff, Resonanz und Drive.
- Send-Fenster mit normalem und getriebenem Satz, Rückweg-Pegel je Effekt und
  einstellbarer Sättigung des getriebenen Weges.
- About-Fenster mit Versionsnummer, Lizenz- und Projektlink. Ein Klick auf das
  Namensschild öffnet es.
- Effekt-Modifier: vier Tasten werfen alles in Delay, Reverb, Phaser oder
  Flanger.
- Installer für Windows und macOS, Release-Workflow und Werbeseite.

### Geändert

- Neues Erscheinungsbild: helle Eurorack-Frontplatten statt dunkler Paneele.
  Platten, Regler, Kippschalter, Drehschalter, Tasten und Pads sind
  Texturen aus Renderings, Werte und Anzeigen werden darüber gezeichnet.
- Wellenform des LFOs, Playback-Modus, Mod-Quelle, Filtertyp und
  Drive-Kurve werden mit Drehschaltern gewählt.
- Lange Auswahllisten scrollen und öffnen beim aktuellen Wert.
- Das Fenster startet mit 1240 × 1060 und ist mindestens 1050 hoch.
- Repeat und Collapse umfassen standardmäßig den ganzen Slice. Der Notenwert
  steht weiter zur Wahl.
- Der Brake hält über einen Beat statt über eine ganze Note an, wie seine
  Beschreibung es immer schon sagte.
- Höchstens zwanzig Slices je Projekt, damit jeder einen Automationsplatz hat.
- Modifier beginnen auf C2 statt auf C1.
- Collapse spielt den Slice erst einmal ganz und faltet dann sein Ende
  zusammen, statt vom Anfang her zu schrumpfen.

### Behoben

- Reverb, Phaser und Flanger waren praktisch unhörbar: die Mitkopplung wurde
  nach Spitzenwert statt nach Energie ausgeglichen, beim Reverb elf Dezibel zu
  viel.
- Sends wurden ohne Rückweg-Pegel in voller Höhe auf das Trockensignal addiert.
- Eine kopierte Cell ließ beim Spielen auch das Original aufleuchten.
- Die Send-Einstellungen erreichten die Engine nach dem Laden eines Projekts
  nie.
- Das Vorhören per Pad-Klick schnitt Loop-Modi nach zwei Sekunden ab, sodass
  der zweite Durchlauf eines langen Slices nur zur Hälfte zu hören war.

## [0.1.0]

Erste Fassung.
