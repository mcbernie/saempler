# Website

Statische Werbeseite für Sämpler. Keine Abhängigkeiten, kein Build-Schritt —
drei Dateien, die jeder Webserver ausliefern kann.

```
index.html   Inhalt
style.css    Gestaltung, Farben aus crates/saempler-ui/src/theme.rs
favicon.svg  Logo
```

## Ansehen

```sh
python -m http.server -d website 8000
# http://localhost:8000
```

## Veröffentlichen

Der Workflow `.github/workflows/pages.yml` schiebt den Ordner bei jedem Push
auf `main` zu GitHub Pages. Einmalig in den Repository-Einstellungen unter
*Pages* als Quelle *GitHub Actions* auswählen.

Für eine eigene Domain die Datei `CNAME` mit dem Hostnamen danebenlegen.

## Pflege

Die Farbwerte in `style.css` sind aus dem Theme des Plugins übernommen. Ändert
sich dort die Palette, gehört sie hier nachgezogen, sonst sehen Seite und
Instrument nach zwei Produkten aus.
