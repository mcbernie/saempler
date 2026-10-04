# Lizenzierung

Sämpler steht unter einer **doppelten Lizenz**. Du wählst, welche für dich gilt.

## 1. GPL-3.0-or-later (Standard)

Der Quellcode in diesem Repository steht unter der GNU General Public License,
Version 3 oder später. Der vollständige Text liegt in [LICENSE](LICENSE).

Das heißt für dich:

- Du darfst Sämpler benutzen, auch beruflich und auch für Musik, die du
  verkaufst. Was du damit produzierst, gehört dir; die GPL betrifft den Code,
  nicht deine Musik.
- Du darfst den Quellcode lesen, ändern und weitergeben.
- Gibst du eine geänderte Fassung weiter, muss sie ebenfalls unter der GPL-3
  stehen und ihr Quellcode verfügbar sein.

## 2. Kommerzielle Lizenz (auf Anfrage)

Wenn du Sämpler oder Teile davon in ein Produkt einbauen willst, dessen
Quellcode geschlossen bleiben soll, passt die GPL nicht. Für diesen Fall gibt
es eine kommerzielle Lizenz.

Anfragen: <https://github.com/mcbernie/saempler/issues> oder direkt an den
Betreuer des Projekts.

## Warum GPL-3

Nicht aus Überzeugungsgründen allein, sondern weil das VST3-SDK von Steinberg
es verlangt. Das SDK ist selbst doppelt lizenziert: GPL-3 oder eine
kommerzielle Vereinbarung mit Steinberg. Ohne eine solche Vereinbarung muss
alles, was VST3 ausliefert, unter der GPL-3 stehen.

Die CLAP- und Standalone-Fassungen haben diese Auflage nicht, werden aber aus
demselben Quellbaum gebaut und stehen deshalb unter derselben Lizenz.

## Fremde Bestandteile

| Bestandteil | Lizenz | Zweck |
| --- | --- | --- |
| [nih-plug](https://github.com/robbert-vdh/nih-plug) | ISC | Plugin-Rahmen, VST3- und CLAP-Export |
| [egui](https://github.com/emilk/egui) | MIT / Apache-2.0 | Oberfläche |
| [Symphonia](https://github.com/pdeljanov/symphonia) | MPL-2.0 | Dekodieren der Samples |
| [rtrb](https://github.com/mgeier/rtrb) | MIT / Apache-2.0 | Echtzeitsichere Warteschlangen |
| [rfd](https://github.com/PolyMeilex/rfd) | MIT | Dateidialog |
| VST3 SDK (über nih-plug) | GPL-3 oder kommerziell (Steinberg) | VST3-Ziel |

VST3 ist eine eingetragene Marke der Steinberg Media Technologies GmbH.
