# Glossary

One English term per German domain term. Code, comments and documentation use these terms only.
UI texts in `src/i18n.ts` are translated per language and are not bound by this list.

| German                                          | English                           | Note                                                |
| ----------------------------------------------- | --------------------------------- | --------------------------------------------------- |
| Session                                         | session                           |                                                     |
| Agenten-Sitzung (aufrufende)                    | calling agent session             | the agent that launched Hangar's dev instance       |
| Agent, Agenten-CLI                              | agent, agent CLI                  |                                                     |
| Agenten-Katalog, Katalog                        | agent catalog, catalog            | `agent_catalog.toml`                                |
| Profil, Profildatei                             | profile, profile file             | `profiles.toml`                                     |
| Programm                                        | program                           | terminals and agents together                       |
| Terminal (Shell-Programm)                       | terminal                          |                                                     |
| Arbeitsordner                                   | working folder                    | not "working directory" in prose; `cwd` in code     |
| Arbeitsbereich                                  | workspace                         | `workspace.json`                                    |
| Verlauf, Terminalverlauf                        | scrollback                        |                                                     |
| Fortsetzen                                      | resume                            | `resume_args`                                       |
| Rückfall                                        | fallback                          | `resumeFallback`                                    |
| Notiz                                           | note                              |                                                     |
| Bereich „Allgemein“                             | General area                      | notes without a folder                              |
| Papierkorb                                      | trash                             |                                                     |
| Aufmerksamkeit, Aufmerksamkeits-Puls            | attention, attention pulse        |                                                     |
| wartet (auf dich)                               | waiting (for you)                 |                                                     |
| Taskleiste blinken                              | taskbar flash                     |                                                     |
| Hintergrund-Session                             | background session                |                                                     |
| Neu bei Hangar                                  | What's New                        | dialog title                                        |
| Einstellungen                                   | Settings                          |                                                     |
| Bereich (in den Einstellungen)                  | section                           |                                                     |
| Sidebar, Seitenleiste                           | sidebar                           |                                                     |
| Gruppe (Sidebar)                                | group                             |                                                     |
| Trennlinie (ziehbar)                            | splitter                          |                                                     |
| Trennlinie (statisch)                           | divider                           |                                                     |
| Tastenkürzel, Kürzel                            | shortcut                          |                                                     |
| Reihenfolge                                     | order                             |                                                     |
| Sichtbarkeit, ein-/ausblenden                   | visibility, show/hide             |                                                     |
| Platzhalter                                     | placeholder                       |                                                     |
| Klickschutz                                     | click guard                       |                                                     |
| Rückgängig                                      | undo                              |                                                     |
| Gegendruck                                      | backpressure                      |                                                     |
| Bündeln                                         | batching                          |                                                     |
| Prozessbaum                                     | process tree                      |                                                     |
| Starter (`.cmd`-Starter, npm-Shim)              | launcher script, npm shim         |                                                     |
| Programmversion                                 | program version                   |                                                     |
| Versionsquelle                                  | version source                    | `latest`                                            |
| Update-Hinweis, Update-Befehl                   | update command                    | field `update_hint` keeps its name                  |
| Programm-Update                                 | program update                    |                                                     |
| Installationsbefehl                             | install command                   |                                                     |
| Buildnummer                                     | build number                      |                                                     |
| kommende Version                                | upcoming version                  |                                                     |
| Versionsschema                                  | version scheme                    |                                                     |
| Signatur (Updater)                              | updater signature                 | minisign, `.sig`                                    |
| Signatur (Authenticode), Signieren              | code signing                      |                                                     |
| Zeitstempeldienst                               | timestamp server                  |                                                     |
| Installer                                       | installer                         |                                                     |
| Prüfkette                                       | check suite                       | fmt, clippy, tests, lint, build                     |
| Abnahme                                         | acceptance review                 | role `manager`                                      |
| Nacharbeit                                      | rework                            |                                                     |
| Sichtprüfung                                    | visual check                      |                                                     |
| Freigabe                                        | approval                          |                                                     |
| Wiki: Quelle, Synthese, Entscheidung            | source, synthesis, decision (ADR) |                                                     |
| Ingest                                          | ingest                            |                                                     |
| Messung                                         | measurement                       |                                                     |
| Vertrag (Befehls-)                              | contract                          |                                                     |
| Ungeprüft                                       | unverified                        | `[UNVERIFIED]`                                      |
| Wortmarke                                       | wordmark                          |                                                     |
| Gefaltetes H (Logo)                             | folded H                          |                                                     |
| Bereich (Notizen: Arbeitsordner oder Allgemein) | scope                             | `NoteScope`                                         |
| Notizbereich                                    | notes panel                       |                                                     |
| Bereichsauswahl                                 | scope picker                      |                                                     |
| Benutzerordner                                  | home folder                       |                                                     |
| Platzhalter-Session                             | placeholder session               |                                                     |
| Update-Punkt                                    | update dot                        |                                                     |
| Trennzeile (Wiederherstellen)                   | divider line                      | marker in restored scrollback                       |
| Kontextmenü                                     | context menu                      |                                                     |
| Programmauswahl                                 | program picker                    |                                                     |
| Drosselung, gedrosselt                          | throttling, throttled             |                                                     |
| Platz (gemerkte Position einer Session)         | slot                              |                                                     |
| Ende (Prozess), Beenden (App)                   | exit, quit                        |                                                     |
| Oberfläche                                      | UI                                |                                                     |
| Favorit (Ordner), Lieblingsordner               | favorite folder                   | `folders.ts`                                        |
| Zuletzt genutzte Ordner                         | recent folders                    | at most 8                                           |
| Startordner-Menü                                | folder menu                       | "Starts in" menu                                    |
| Suchleiste, Treffer                             | search bar, match                 | `SearchBar.tsx`                                     |
| Farbschema                                      | color scheme                      | `terminalLook.ts`                                   |
| Erscheinungsbild des Terminals                  | terminal look                     | scheme, font, cursor                                |
| Ziehen und Ablegen                              | drag and drop                     | files onto a pane                                   |
| Bereich (Terminal-Fläche einer Session)         | pane                              | `TerminalPane.tsx`                                  |
| Maskieren (Pfade für eine Shell)                | quoting                           | `pathQuote.ts`                                      |
| Exportieren (Session)                           | export                            | `sessionExport.ts`                                  |
| Schlüsselbund (des Betriebssystems)             | keychain                          | `secrets.rs`; Credential Manager on Windows         |
| Geheime Variable                                | secret variable                   | `secret_env`; only the name is in the file          |
| Infobereich, Tray-Symbol                        | notification area, tray icon      | `tray.rs`                                           |
| WSL-Distribution                                | WSL distribution                  | `wsl.rs`                                            |
| Geteilte Ansicht                                | split view                        | `split.ts`; one, two side by side or four in a grid |
| Fokussierter Bereich                            | focused pane                      | the pane with the active session                    |
| Schnellzugriff                                  | quick access, launcher            | `launcher.ts`; "launcher" in code                   |
| Textbaustein                                    | snippet                           | `snippets.ts`                                       |
| An mehrere senden                               | broadcast                         | `broadcast.ts`                                      |
| Markierung (Session)                            | session marker                    | `marker.ts`; color and emoji                        |
