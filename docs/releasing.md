# Release

## Versionsschema

Semantisch, als einzelne Zahl im Workspace gepflegt:

```toml
# Cargo.toml
[workspace.package]
version = "0.1.0"
```

Alle Crates erben sie über `version.workspace = true`, und das About-Fenster
liest sie über `env!("CARGO_PKG_VERSION")`. Es gibt keine zweite Stelle, an der
eine Versionsnummer steht, und damit auch keine, die abweichen kann.

Solange die Hauptzahl `0` ist:

| Änderung | Teil |
| --- | --- |
| Projektformat bricht, Host-Parameter fallen weg oder ändern ihre ID | Minor (`0.2.0`) |
| Neue Funktionen, neue Parameter am Ende der Liste, Fehlerbehebungen | Patch (`0.1.1`) |

Ab `1.0.0` gilt das übliche Semver. **Host-Parameter-IDs sind ab dem ersten
öffentlichen Release unveränderlich**: eine gespeicherte Automationsspur in
einem Ableton-Projekt zeigt auf die ID, nicht auf den Namen. Eine ID zu
entfernen oder umzuwidmen zerstört fremde Arrangements.

Dasselbe gilt für `PROJECT_VERSION` in `saempler-model`. Ändert sich die
Struktur des gespeicherten Projekts, wird die Zahl erhöht und
`ProjectFile::migrate` bekommt den Schritt — nicht umgekehrt.

## Ablauf

1. Arbeitsstand sauber, `just verify` grün.
2. Version in `Cargo.toml` setzen, `cargo check` laufen lassen, damit
   `Cargo.lock` nachzieht.
3. `CHANGELOG.md` ergänzen.
4. Beides committen: `git commit -m "Version 0.2.0"`.
5. Tag setzen und schieben:

   ```sh
   git tag -a v0.2.0 -m "Sämpler 0.2.0"
   git push origin main v0.2.0
   ```

6. Der Workflow `.github/workflows/release.yml` baut Windows, macOS und Linux,
   erzeugt die Installer und legt einen **Entwurf** einer Release-Seite an.
7. Entwurf prüfen, Text ergänzen, veröffentlichen.

Der Entwurf ist Absicht: der letzte Schritt gehört einem Menschen, und ein
fehlgeschlagener Build soll keine halbe Veröffentlichung hinterlassen.

## Was gebaut wird

| Ziel | Windows | macOS | Linux |
| --- | --- | --- | --- |
| VST3 | ✓ | ✓ (universal) | ✓ |
| CLAP | ✓ | ✓ (universal) | ✓ |
| Standalone | ✓ | ✓ (universal) | ✓ |
| Installer | Inno Setup `.exe` | `.pkg` | — |

Linux bekommt keinen Installer: dort ist das Entpacken nach `~/.vst3` der
übliche Weg, und ein Paket pro Distribution wäre mehr Pflege als Nutzen.

## Signieren und Beglaubigen

Beides fehlt noch, bewusst:

- **macOS**: Die `.pkg` ist weder signiert noch notariell beglaubigt.
  Gatekeeper warnt beim ersten Öffnen. Dafür braucht es ein
  Apple-Developer-Konto (99 USD/Jahr), ein „Developer ID"-Zertifikat als
  Secret und `--sign` in `packaging/macos/build-pkg.sh` sowie einen
  `notarytool`-Schritt im Workflow.
- **Windows**: Die `.exe` ist unsigniert, SmartScreen meldet sich. Dafür
  braucht es ein Code-Signing-Zertifikat und `signtool` im Workflow.

Bis dahin steht der Weg um die Warnung herum in den Release-Notizen.

## Updates

Das Plugin prüft nichts im Netz. Das About-Fenster zeigt seine eigene Version
und verlinkt die Projektseite; dort steht, welche die aktuelle ist. Das ist
eine bewusste Entscheidung: ein Plugin, das beim Öffnen eines Projekts
unaufgefordert nach Hause telefoniert, ist in einem Studio unerwünscht und in
manchen Häusern verboten.

Soll sich das ändern, gehört der Aufruf in einen Hintergrund-Task mit einem
Schalter, der ihn abstellt, und niemals in den Audio- oder den Editor-Thread.
