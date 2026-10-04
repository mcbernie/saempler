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

- Repeat und Collapse umfassen standardmäßig den ganzen Slice. Der Notenwert
  steht weiter zur Wahl.
- Der Brake hält über einen Beat statt über eine ganze Note an, wie seine
  Beschreibung es immer schon sagte.
- Höchstens zwanzig Slices je Projekt, damit jeder einen Automationsplatz hat.
- Modifier beginnen auf C2 statt auf C1.

### Behoben

- Reverb, Phaser und Flanger waren praktisch unhörbar: die Mitkopplung wurde
  nach Spitzenwert statt nach Energie ausgeglichen, beim Reverb elf Dezibel zu
  viel.
- Sends wurden ohne Rückweg-Pegel in voller Höhe auf das Trockensignal addiert.
- Eine kopierte Cell ließ beim Spielen auch das Original aufleuchten.
- Die Send-Einstellungen erreichten die Engine nach dem Laden eines Projekts
  nie.

## [0.1.0]

Erste Fassung.
