# Glossary

One English term per German domain term. Code, comments and documentation use these terms only.
UI texts in `src/i18n.ts` are translated per language and are not bound by this list.

| German                                          | English                           | Note                                                                   |
| ----------------------------------------------- | --------------------------------- | ---------------------------------------------------------------------- |
| Session                                         | session                           |                                                                        |
| Agenten-Sitzung (aufrufende)                    | calling agent session             | the agent that launched Hangar's dev instance                          |
| Agent, Agenten-CLI                              | agent, agent CLI                  |                                                                        |
| Agenten-Katalog, Katalog                        | agent catalog, catalog            | `agent_catalog.toml`                                                   |
| Profil, Profildatei                             | profile, profile file             | `profiles.toml`                                                        |
| Programm                                        | program                           | terminals and agents together                                          |
| Terminal (Shell-Programm)                       | terminal                          |                                                                        |
| Arbeitsordner                                   | working folder                    | not "working directory" in prose; `cwd` in code                        |
| Arbeitsbereich                                  | workspace                         | `workspace.json`                                                       |
| Verlauf, Terminalverlauf                        | scrollback                        |                                                                        |
| Fortsetzen                                      | resume                            | `resume_args`                                                          |
| Rückfall                                        | fallback                          | `resumeFallback`                                                       |
| Unterhaltungs-ID                                | conversation id                   | the agent CLI's own session id, `cliSessionId`                         |
| Sub-Agent                                       | sub-agent                         | runs inside the agent's process, no terminal of its own                |
| Sub-Agent-Ansicht                               | sub-agent view                    | read-only, `subagents/`                                                |
| Textauszug                                      | excerpt                           | a sub-agent's newest text, never stored                                |
| Notiz                                           | note                              |                                                                        |
| Bereich „Allgemein“                             | General area                      | notes without a folder                                                 |
| Papierkorb                                      | trash                             |                                                                        |
| Aufmerksamkeit, Aufmerksamkeits-Puls            | attention, attention pulse        |                                                                        |
| wartet (auf dich)                               | waiting (for you)                 |                                                                        |
| Status-Chip                                     | status chip                       | state shape at the right end of a session row                          |
| fragt (Glocke, BEL)                             | asks (bell)                       | waiting reason after BEL, `AttentionReason`                            |
| Hinweiston                                      | chime                             | synthesized two-tone sound, `chime.ts`                                 |
| Taskleiste blinken                              | taskbar flash                     |                                                                        |
| Hintergrund-Session                             | background session                |                                                                        |
| Neu bei Hangar                                  | What's New                        | dialog title                                                           |
| Einstellungen                                   | Settings                          |                                                                        |
| Bereich (in den Einstellungen)                  | section                           |                                                                        |
| Sidebar, Seitenleiste                           | sidebar                           |                                                                        |
| Gruppe (Sidebar)                                | group                             |                                                                        |
| Trennlinie (ziehbar)                            | splitter                          |                                                                        |
| Trennlinie (statisch)                           | divider                           |                                                                        |
| Tastenkürzel, Kürzel                            | shortcut                          |                                                                        |
| Reihenfolge                                     | order                             |                                                                        |
| Sichtbarkeit, ein-/ausblenden                   | visibility, show/hide             |                                                                        |
| Platzhalter                                     | placeholder                       |                                                                        |
| Klickschutz                                     | click guard                       |                                                                        |
| Rückgängig                                      | undo                              |                                                                        |
| Gegendruck                                      | backpressure                      |                                                                        |
| Bündeln                                         | batching                          |                                                                        |
| Prozessbaum                                     | process tree                      |                                                                        |
| Starter (`.cmd`-Starter, npm-Shim)              | launcher script, npm shim         |                                                                        |
| Programmversion                                 | program version                   |                                                                        |
| Versionsquelle                                  | version source                    | `latest`                                                               |
| Update-Hinweis, Update-Befehl                   | update command                    | field `update_hint` keeps its name                                     |
| Programm-Update                                 | program update                    |                                                                        |
| Update-Plan                                     | update plan                       | `run`, `copy` or `none`, with `planId`                                 |
| Weitere Kopie (eines Programms)                 | second copy                       | warning `second-copy`                                                  |
| Update-Ergebnis                                 | update outcome                    | event `program-update-result`                                          |
| Installationsbefehl                             | install command                   |                                                                        |
| Buildnummer                                     | build number                      |                                                                        |
| kommende Version                                | upcoming version                  |                                                                        |
| Versionsschema                                  | version scheme                    |                                                                        |
| Signatur (Updater)                              | updater signature                 | minisign, `.sig`                                                       |
| Signatur (Authenticode), Signieren              | code signing                      |                                                                        |
| Zeitstempeldienst                               | timestamp server                  |                                                                        |
| Installer                                       | installer                         |                                                                        |
| Prüfkette                                       | check suite                       | fmt, clippy, tests, lint, build                                        |
| Abnahme                                         | acceptance review                 | role `manager`                                                         |
| Nacharbeit                                      | rework                            |                                                                        |
| Sichtprüfung                                    | visual check                      |                                                                        |
| Freigabe                                        | approval                          |                                                                        |
| Wiki: Quelle, Synthese, Entscheidung            | source, synthesis, decision (ADR) |                                                                        |
| Ingest                                          | ingest                            |                                                                        |
| Messung                                         | measurement                       |                                                                        |
| Vertrag (Befehls-)                              | contract                          |                                                                        |
| Ungeprüft                                       | unverified                        | `[UNVERIFIED]`                                                         |
| Wortmarke                                       | wordmark                          |                                                                        |
| Gefaltetes H (Logo)                             | folded H                          |                                                                        |
| Bereich (Notizen: Arbeitsordner oder Allgemein) | scope                             | `NoteFolder`; the list shows one scope or all (`NotesScope`)           |
| Notizbereich                                    | notes panel                       |                                                                        |
| Bereichs-Chips                                  | scope chips                       | "This project", "All", "General" above the notes list; `notesScope.ts` |
| Schnelle Notiz                                  | quick note                        | Ctrl+Shift+N; created once it has content, an empty one is dropped     |
| Eigenes Notizfenster                            | detached window                   | window label `notes`; `NotesWindow.tsx`, `notesBridge.ts`              |
| Rechter Bereich                                 | right panel                       | tabs Notes and Changes; `SidePanel.tsx`                                |
| Wartehinweis am Listenrand                      | waiting edge hint                 | "1 waiting" above or below the session list; `waitingHint.ts`          |
| Benutzerordner                                  | home folder                       |                                                                        |
| Platzhalter-Session                             | placeholder session               |                                                                        |
| Update-Punkt                                    | update dot                        |                                                                        |
| Trennzeile (Wiederherstellen)                   | divider line                      | marker in restored scrollback                                          |
| Kontextmenü                                     | context menu                      |                                                                        |
| Programmauswahl                                 | program picker                    |                                                                        |
| Drosselung, gedrosselt                          | throttling, throttled             |                                                                        |
| Platz (gemerkte Position einer Session)         | slot                              |                                                                        |
| Snap-Layout                                     | snap layout                       | preset arrangement of the panes, `snapLayouts.ts`                      |
| Zone (Snap-Layout)                              | zone                              | the area of one pane in a snap layout                                  |
| Layout-Auswahl, Snap-Leiste                     | layout picker, snap bar           | `SnapBar.tsx`                                                          |
| Ablagelinie                                     | drop line                         | where a dragged session lands in the sidebar                           |
| Ende (Prozess), Beenden (App)                   | exit, quit                        |                                                                        |
| Oberfläche                                      | UI                                |                                                                        |
| Favorit (Ordner), Lieblingsordner               | favorite folder                   | `folders.ts`                                                           |
| Zuletzt genutzte Ordner                         | recent folders                    | at most 8                                                              |
| Startordner-Menü                                | folder menu                       | "Starts in" menu                                                       |
| Startziel                                       | start target                      | where the next session starts, `startTarget.ts`                        |
| Laufende Projekte                               | running projects                  | project folders of running sessions, folder menu                       |
| Suchleiste, Treffer                             | search bar, match                 | `SearchBar.tsx`                                                        |
| Farbschema                                      | color scheme                      | `terminalLook.ts`                                                      |
| Erscheinungsbild des Terminals                  | terminal look                     | scheme, font, cursor                                                   |
| Ziehen und Ablegen                              | drag and drop                     | files onto a pane                                                      |
| Bereich (Terminal-Fläche einer Session)         | pane                              | `TerminalPane.tsx`                                                     |
| Maskieren (Pfade für eine Shell)                | quoting                           | `pathQuote.ts`                                                         |
| Exportieren (Session)                           | export                            | `sessionExport.ts`                                                     |
| Schlüsselbund (des Betriebssystems)             | keychain                          | `secrets.rs`; Credential Manager on Windows                            |
| Geheime Variable                                | secret variable                   | `secret_env`; only the name is in the file                             |
| Infobereich, Tray-Symbol                        | notification area, tray icon      | `tray.rs`                                                              |
| WSL-Distribution                                | WSL distribution                  | `wsl.rs`                                                               |
| Geteilte Ansicht                                | split view                        | `split.ts`; one, two side by side or four in a grid                    |
| Fokussierter Bereich                            | focused pane                      | the pane with the active session                                       |
| Schnellzugriff                                  | quick access, launcher            | `launcher.ts`; "launcher" in code                                      |
| Textbaustein                                    | snippet                           | `snippets.ts`                                                          |
| An mehrere senden                               | broadcast                         | `broadcast.ts`                                                         |
| Markierung (Session)                            | session marker                    | `marker.ts`; color and emoji                                           |
| Desktop-Benachrichtigung                        | desktop notification              | `desktopNotify.ts`; `notify_session`                                   |
| Statuszeile (Session)                           | status line                       | `statusLine.ts`; status, folder, branch, run time                      |
| Laufzeit (Session)                              | run time                          | since the process started                                              |
| Taskleisten-Zähler                              | taskbar badge                     | `taskbarBadge.ts`; overlay icon on Windows                             |
| Eigener Worktree (Session)                      | own worktree                      | `worktree.ts`; branch `hangar/<slug>` per session                      |
| Änderungen (Bereich)                            | changes panel                     | `gitDiff.ts`, `ChangesPanel.tsx`; Ctrl+Shift+D                         |
| Anspruch (geänderte Datei einer Session)        | claim                             | `claims.rs`; path, kind of change, line ranges on the base side, area  |
| Überschneidung                                  | overlap                           | `OverlapLevel`: same area, same file, overlapping lines                |
| Basis-Commit                                    | base commit                       | HEAD of the session's worktree; lines are compared on this side        |
| Bereich (Claims: erster Ordner eines Pfads)     | area                              | `Claim.area`; configurable per repository later                        |
| Geteilter Ordner                                | shared folder                     | several sessions in one folder share its changes                       |
| Programm-Editor                                 | profile editor                    | `ProfileEditor.tsx`; Settings, Programs                                |
| Anpassen (eingebauter Eintrag)                  | customize                         | saving a catalog or WSL entry as the user's own profile                |
