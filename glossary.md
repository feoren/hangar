# Glossary

One English term per German domain term. Code, comments and documentation use these terms only.
UI texts in `src/i18n.ts` are translated per language and are not bound by this list.

| German                                          | English                           | Note                                                                        |
| ----------------------------------------------- | --------------------------------- | --------------------------------------------------------------------------- |
| Session                                         | session                           |                                                                             |
| Agenten-Sitzung (aufrufende)                    | calling agent session             | the agent that launched Hangar's dev instance                               |
| Agent, Agenten-CLI                              | agent, agent CLI                  |                                                                             |
| Agenten-Katalog, Katalog                        | agent catalog, catalog            | `agent_catalog.toml`                                                        |
| Profil, Profildatei                             | profile, profile file             | `profiles.toml`                                                             |
| Programm                                        | program                           | terminals and agents together                                               |
| Terminal (Shell-Programm)                       | terminal                          |                                                                             |
| Arbeitsordner                                   | working folder                    | not "working directory" in prose; `cwd` in code                             |
| Arbeitsbereich                                  | workspace                         | `workspace.json`                                                            |
| Verlauf, Terminalverlauf                        | scrollback                        |                                                                             |
| Fortsetzen                                      | resume                            | `resume_args`                                                               |
| Rückfall                                        | fallback                          | `resumeFallback`                                                            |
| Unterhaltungs-ID                                | conversation id                   | the agent CLI's own session id, `cliSessionId`                              |
| Sub-Agent                                       | sub-agent                         | runs inside the agent's process, no terminal of its own                     |
| Sub-Agent-Ansicht                               | sub-agent view                    | read-only, `subagents/`                                                     |
| Textauszug                                      | excerpt                           | a sub-agent's newest text, never stored                                     |
| Aktivitätsprotokoll                             | activity log                      | console-like log of the focused sub-agent, `LogBatch`, never stored         |
| Fokus (Sub-Agent)                               | focus                             | the one sub-agent whose activity log is streamed, `focus_sub_agent`         |
| Entfernen (fertiger Sub-Agent)                  | dismiss                           | hides a finished sub-agent in the view only; transcripts stay               |
| Kontextanzeige                                  | context meter                     | context window use of an agent session on its row, `ContextMeter.tsx`       |
| Notiz                                           | note                              |                                                                             |
| Bereich „Allgemein“                             | General area                      | notes without a folder                                                      |
| Papierkorb                                      | trash                             |                                                                             |
| Aufmerksamkeit, Aufmerksamkeits-Puls            | attention, attention pulse        |                                                                             |
| wartet (auf dich)                               | waiting (for you)                 |                                                                             |
| Aussehen je Status                              | state look                        | color, pulse strength and duration per state, `stateLook.ts`                |
| Pause (Session läuft, ruhig)                    | pause                             | UI name of the `idle` state of a running session                            |
| Status-Chip                                     | status chip                       | state pill at the end of a session row's title line                         |
| fragt (Glocke, BEL)                             | asks (bell)                       | waiting reason after BEL, `AttentionReason`                                 |
| Hinweiston                                      | chime                             | synthesized two-tone sound, `chime.ts`                                      |
| Taskleiste blinken                              | taskbar flash                     |                                                                             |
| Hintergrund-Session                             | background session                |                                                                             |
| Neu bei Hangar                                  | What's New                        | dialog title                                                                |
| Einstellungen                                   | Settings                          |                                                                             |
| Bereich (in den Einstellungen)                  | section                           |                                                                             |
| Sidebar, Seitenleiste                           | sidebar                           |                                                                             |
| Gruppe (Sidebar)                                | group                             |                                                                             |
| Agentengruppe (Sidebar)                         | agent group                       | top-level group of one program's sessions, worktree or not; `agentGroupKey` |
| Trennlinie (ziehbar)                            | splitter                          |                                                                             |
| Trennlinie (statisch)                           | divider                           |                                                                             |
| Tastenkürzel, Kürzel                            | shortcut                          |                                                                             |
| Reihenfolge                                     | order                             |                                                                             |
| Sichtbarkeit, ein-/ausblenden                   | visibility, show/hide             |                                                                             |
| Platzhalter                                     | placeholder                       |                                                                             |
| Klickschutz                                     | click guard                       |                                                                             |
| Rückgängig                                      | undo                              |                                                                             |
| Gegendruck                                      | backpressure                      |                                                                             |
| Bündeln                                         | batching                          |                                                                             |
| Prozessbaum                                     | process tree                      |                                                                             |
| Starter (`.cmd`-Starter, npm-Shim)              | launcher script, npm shim         |                                                                             |
| Programmversion                                 | program version                   |                                                                             |
| Versionsquelle                                  | version source                    | `latest`                                                                    |
| Abfrageordner (Versionsabfrage)                 | probe folder                      | Hangar's own working folder for `--version` calls, `version::probe_folder`  |
| Ausführungsalias (Store-App)                    | execution alias                   | empty reparse-point file in `WindowsApps`, `exec_alias.rs`                  |
| Update-Hinweis, Update-Befehl                   | update command                    | field `update_hint` keeps its name                                          |
| Programm-Update                                 | program update                    |                                                                             |
| Update-Plan                                     | update plan                       | `run`, `copy` or `none`, with `planId`                                      |
| Weitere Kopie (eines Programms)                 | second copy                       | warning `second-copy`                                                       |
| Update-Ergebnis                                 | update outcome                    | event `program-update-result`                                               |
| Installationsbefehl                             | install command                   |                                                                             |
| Buildnummer                                     | build number                      |                                                                             |
| kommende Version                                | upcoming version                  |                                                                             |
| Versionsschema                                  | version scheme                    |                                                                             |
| Signatur (Updater)                              | updater signature                 | minisign, `.sig`                                                            |
| Signatur (Authenticode), Signieren              | code signing                      |                                                                             |
| Zeitstempeldienst                               | timestamp server                  |                                                                             |
| Installer                                       | installer                         |                                                                             |
| Prüfkette                                       | check suite                       | fmt, clippy, tests, lint, build                                             |
| Abnahme                                         | acceptance review                 | role `manager`                                                              |
| Nacharbeit                                      | rework                            |                                                                             |
| Sichtprüfung                                    | visual check                      |                                                                             |
| Freigabe                                        | approval                          |                                                                             |
| Wiki: Quelle, Synthese, Entscheidung            | source, synthesis, decision (ADR) |                                                                             |
| Ingest                                          | ingest                            |                                                                             |
| Messung                                         | measurement                       |                                                                             |
| Vertrag (Befehls-)                              | contract                          |                                                                             |
| Ungeprüft                                       | unverified                        | `[UNVERIFIED]`                                                              |
| Wortmarke                                       | wordmark                          |                                                                             |
| Gefaltetes H (Logo)                             | folded H                          |                                                                             |
| Bereich (Notizen: Arbeitsordner oder Allgemein) | scope                             | `NoteFolder`; the list shows one scope or all (`NotesScope`)                |
| Notizbereich                                    | notes panel                       |                                                                             |
| Bereichs-Chips                                  | scope chips                       | "This project", "All", "General" above the notes list; `notesScope.ts`      |
| Schnelle Notiz                                  | quick note                        | Ctrl+Shift+N; created once it has content, an empty one is dropped          |
| Eigenes Notizfenster                            | detached window                   | window label `notes`; `NotesWindow.tsx`, `notesBridge.ts`                   |
| Rechter Bereich                                 | right panel                       | tabs Notes and Changes; `SidePanel.tsx`                                     |
| Wartehinweis am Listenrand                      | waiting edge hint                 | "1 waiting" above or below the session list; `waitingHint.ts`               |
| Benutzerordner                                  | home folder                       |                                                                             |
| Platzhalter-Session                             | placeholder session               |                                                                             |
| Update-Punkt                                    | update dot                        |                                                                             |
| Trennzeile (Wiederherstellen)                   | divider line                      | marker in restored scrollback                                               |
| Kontextmenü                                     | context menu                      |                                                                             |
| Programmauswahl                                 | program picker                    |                                                                             |
| Drosselung, gedrosselt                          | throttling, throttled             |                                                                             |
| Platz (gemerkte Position einer Session)         | slot                              |                                                                             |
| Snap-Layout                                     | snap layout                       | preset arrangement of the panes, `snapLayouts.ts`                           |
| Zone (Snap-Layout)                              | zone                              | the area of one pane in a snap layout                                       |
| Aufteilung (Snap-Layout)                        | pane ratios                       | splitter positions of a snap layout as fractions, `paneRatios.ts`           |
| Layout-Miniatur                                 | layout glyph                      | snap layout in miniature on a sidebar row, `LayoutGlyph.tsx`                |
| Layout-Auswahl, Snap-Leiste                     | layout picker, snap bar           | `SnapBar.tsx`                                                               |
| Ablagelinie                                     | drop line                         | where a dragged session lands in the sidebar                                |
| Ende (Prozess), Beenden (App)                   | exit, quit                        |                                                                             |
| Oberfläche                                      | UI                                |                                                                             |
| Favorit (Ordner), Lieblingsordner               | favorite folder                   | `folders.ts`                                                                |
| Zuletzt genutzte Ordner                         | recent folders                    | at most 8                                                                   |
| Startordner-Menü                                | folder menu                       | "Project folder" menu                                                       |
| Startziel                                       | start target                      | where the next session starts, `startTarget.ts`                             |
| Laufende Projekte                               | running projects                  | project folders of running sessions, folder menu                            |
| Suchleiste, Treffer                             | search bar, match                 | `SearchBar.tsx`                                                             |
| Farbschema                                      | color scheme                      | `terminalLook.ts`                                                           |
| Erscheinungsbild des Terminals                  | terminal look                     | scheme, font, cursor                                                        |
| Ziehen und Ablegen                              | drag and drop                     | files onto a pane                                                           |
| Bereich (Terminal-Fläche einer Session)         | pane                              | `TerminalPane.tsx`                                                          |
| Maskieren (Pfade für eine Shell)                | quoting                           | `pathQuote.ts`                                                              |
| Exportieren (Session)                           | export                            | `sessionExport.ts`                                                          |
| Schlüsselbund (des Betriebssystems)             | keychain                          | `secrets.rs`; Credential Manager on Windows                                 |
| Geheime Variable                                | secret variable                   | `secret_env`; only the name is in the file                                  |
| Infobereich, Tray-Symbol                        | notification area, tray icon      | `tray.rs`                                                                   |
| Einzelinstanz (nur ein Hangar je Benutzer)      | single instance                   | `single_instance.rs`; release builds only, debug builds exempt              |
| WSL-Distribution                                | WSL distribution                  | `wsl.rs`                                                                    |
| Geteilte Ansicht                                | split view                        | `split.ts`; one, two side by side or four in a grid                         |
| Fokussierter Bereich                            | focused pane                      | the pane with the active session                                            |
| Schnellzugriff                                  | quick access, launcher            | `launcher.ts`; "launcher" in code                                           |
| Textbaustein                                    | snippet                           | `snippets.ts`                                                               |
| An mehrere senden                               | broadcast                         | `broadcast.ts`                                                              |
| Markierung (Session)                            | session marker                    | `marker.ts`; color and emoji                                                |
| Desktop-Benachrichtigung                        | desktop notification              | `desktopNotify.ts`; `notify_session`                                        |
| Statuszeile (Session)                           | status line                       | `statusLine.ts`; status, folder, branch, run time                           |
| Laufzeit (Session)                              | run time                          | since the process started                                                   |
| Taskleisten-Zähler                              | taskbar badge                     | `taskbarBadge.ts`; overlay icon on Windows                                  |
| Eigener Worktree (Session)                      | own worktree                      | `worktree.ts`; branch `hangar/<slug>` per session                           |
| Private Session                                 | private session                   | starts in the folder, no agent gate, never shared; ADR 0008                 |
| Team-Session                                    | team session                      | own worktree, agent gate, shared with the team; `SessionMode`               |
| Art der Session (Privat oder Team)              | session mode                      | `SessionMode`; switch below "Project folder", `startMode.ts`                |
| Hook-Format (Agent-Gate)                        | hook format                       | how the gate hands hooks to one program, `HookFormat`                       |
| Hook-Helfer (Agent-Gate)                        | gate helper                       | `hangar gate-hook`, `gate_hook.rs`; an agent's command hook                 |
| Änderungen (Bereich)                            | changes panel                     | `gitDiff.ts`, `ChangesPanel.tsx`; Ctrl+Shift+D                              |
| Commit-Verlauf (Änderungen)                     | commit history                    | `gitHistory.ts`, `CommitHistory.tsx`; "Commits" view of the changes panel   |
| Upstream (Branch)                               | upstream                          | the branch's tracking branch, e.g. `origin/main`                            |
| Nicht gepusht                                   | unpushed                          | on HEAD but not on the upstream; `CommitSummary.pushed`                     |
| Anspruch (geänderte Datei einer Session)        | claim                             | `claims.rs`; path, kind of change, line ranges on the base side, area       |
| Überschneidung                                  | overlap                           | `OverlapLevel`: same area, same file, overlapping lines                     |
| Basis-Commit                                    | base commit                       | HEAD of the session's worktree; lines are compared on this side             |
| Bereich (Claims: erster Ordner eines Pfads)     | area                              | `Claim.area`; configurable per repository later                             |
| Geteilter Ordner                                | shared folder                     | several sessions in one folder share its changes                            |
| Sperre vor dem Bearbeiten                       | gate                              | `gate.ts`; holds an agent's edit of a file another session is changing      |
| Offene Entscheidung                             | pending decision                  | `PendingDecision`; allow once, allow for this file or deny                  |
| Reservierung (Aufgabe)                          | reservation                       | `reservation.rs` (protocol crate); first wins, expires when idle            |
| Freigabe-Anfrage                                | release request                   | asks the holder of a reservation to hand the task over                      |
| Halter (einer Reservierung)                     | holder                            | member and device holding a reservation                                     |
| Schlüssel-Hash                                  | keyed hash                        | HMAC-SHA256 under the team's matching key; compared by equality only        |
| Abgleichschlüssel                               | matching key                      | `MatchKey`, derived per epoch from the team key                             |
| Epoche (Teamschlüssel)                          | epoch                             | generation of the team key; new on every leave                              |
| Chiffrat                                        | ciphertext                        | opaque encrypted bytes, base64url; format in step E                         |
| Ereignisprotokoll, Sequenznummer                | event log, sequence number        | per team; replay window, `resync_required`                                  |
| Operations-ID                                   | op id                             | per client intent; a retry carries the same one                             |
| Protokoll-Crate                                 | protocol crate                    | `crates/hangar-team-protocol`                                               |
| Programm-Editor                                 | profile editor                    | `ProfileEditor.tsx`; Settings, Programs                                     |
| Anpassen (eingebauter Eintrag)                  | customize                         | saving a catalog or WSL entry as the user's own profile                     |
| Konto (Hangar-Konto)                            | account                           | `account.rs`; optional, Hangar works without it                             |
| Anmeldung (Konto)                               | sign-in                           | OAuth device flow; the refresh token lives only in the keychain             |
| Bestätigungscode                                | user code                         | `XXXX-XXXX`, confirmed in the browser                                       |
| Serveradresse                                   | server address                    | API base of the official service or a self-hosted server; `BaseUrl`         |
| Projekt (Team)                                  | project, team                     | a project on the website is a team; `Project`                               |
| Platz (Lizenz)                                  | seat                              | counted per account of the project's owner (ADR 0005)                       |
| Tarif                                           | plan                              | `Project.plan`, as the server names it                                      |
| Einladung                                       | invitation                        | sent and accepted on the website                                            |
| Teamverbindung                                  | team connection                   | per project, turned on by "Share with team"; `TeamStatus` in `team.ts`      |
| Sicherheitsnummer                               | safety number                     | 6 groups of 5 digits, compared on both devices before a confirmation        |
| Bestätigung (neues Gerät)                       | confirmation                      | an own device or an owner/admin accepts a new device; `PendingConfirmation` |
| Anwesenheit (Teamgerät)                         | presence                          | `presence.set`; the device's sessions in linked repositories, sealed        |
| Verknüpftes Repository                          | linked repository                 | `LinkedRepo`; only its sessions are shared with a project                   |
| Teamansicht                                     | Team view                         | `TeamActivityView.tsx`; teammates' sessions per shared project              |
| Überschneidung mit dem Team                     | teammate overlap                  | `TeammateOverlap`; the relay's `claim.overlap`, refined locally             |
| Zustand (Session, fürs Team)                    | session state                     | working, waiting, idle; `SessionState`                                      |
| Anderes Konto (Teamgerät)                       | other account                     | `otherAccount`; a team device acts only for the account it was bound to     |
| Abweichende Historie (Teamgerät)                | divergent device                  | `DivergentDevice`; set aside after signing another history                  |
| Aufgabe (im Team reserviert)                    | task                              | a short title the team reserves; the relay sees only its keyed hash         |
| Reservierungsnotiz                              | reservation note                  | sealed title, name, area and session of a reservation, signed by the holder |
| Reservierungsversuch                            | attempt                           | `AttemptView`; this device's reservation the relay has not granted (yet)    |
| Reservierter Bereich                            | reserved area                     | a folder a reservation covers; teammates' agents pause there (gate)         |
| Dateibaum (rechter Bereich)                     | file tree                         | tab `files`; `FilesPanel.tsx`, `fileTree.ts`; folders listed lazily         |
| Dateiansicht (Fenster)                          | file viewer                       | read-only window for one file; `FileViewerWindow.tsx`, `file_viewer.rs`     |
| Dateiansicht-Thema                              | viewer theme                      | colors of the file viewer, dark and light; `viewerThemes.ts`                |
| Änderungspfad                                   | change path                       | folders down to each changed file, marked in the tree; `onChangePath`       |
