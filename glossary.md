# Glossary

One English term per German domain term. Code, comments and documentation use these terms only.
UI texts in `src/i18n.ts` are translated per language and are not bound by this list.

| German                                          | English                           | Note                                                                                 |
| ----------------------------------------------- | --------------------------------- | ------------------------------------------------------------------------------------ |
| Session                                         | session                           |                                                                                      |
| Agenten-Sitzung (aufrufende)                    | calling agent session             | the agent that launched Hangar's dev instance                                        |
| Agent, Agenten-CLI                              | agent, agent CLI                  |                                                                                      |
| Agenten-Katalog, Katalog                        | agent catalog, catalog            | `agent_catalog.toml`                                                                 |
| Profil, Profildatei                             | profile, profile file             | `profiles.toml`                                                                      |
| Programm                                        | program                           | terminals and agents together                                                        |
| Terminal (Shell-Programm)                       | terminal                          |                                                                                      |
| Arbeitsordner                                   | working folder                    | not "working directory" in prose; `cwd` in code                                      |
| Arbeitsbereich                                  | workspace                         | `workspace.json`                                                                     |
| Verlauf, Terminalverlauf                        | scrollback                        |                                                                                      |
| Fortsetzen                                      | resume                            | `resume_args`                                                                        |
| Rückfall                                        | fallback                          | `resumeFallback`                                                                     |
| Absturzmarke                                    | exit marker                       | `running.marker`; left behind, it means the last run did not quit cleanly            |
| Absturzhinweis                                  | crash notice                      | notice after restoring the sessions of a run that did not quit cleanly               |
| Unterhaltungs-ID                                | conversation id                   | the agent CLI's own session id, `cliSessionId`                                       |
| Sub-Agent                                       | sub-agent                         | runs inside the agent's process, no terminal of its own                              |
| Sub-Agent-Ansicht                               | sub-agent view                    | read-only, `subagents/`                                                              |
| Textauszug                                      | excerpt                           | a sub-agent's newest text, never stored                                              |
| Aktivitätsprotokoll                             | activity log                      | console-like log of the focused sub-agent, `LogBatch`, never stored                  |
| Fokus (Sub-Agent)                               | focus                             | the one sub-agent whose activity log is streamed, `focus_sub_agent`                  |
| Schließen (fertiger Sub-Agent)                  | dismiss                           | hides a finished sub-agent in the view only; transcripts stay                        |
| Sub-Agent-Definition                            | sub-agent definition              | Markdown file with a frontmatter in `.claude/agents`; `agent_defs.rs`                |
| Sub-Agent-Bibliothek                            | sub-agent library                 | the listed definitions, Settings > Sub-agents; `subAgentLibrary.ts`                  |
| Programm der Definition                         | definition program                | whose definitions: `agent_program`, `DefProgram` (`claude`, `codex`)                 |
| Standardmodell (Sub-Agenten)                    | default model                     | per program for every session Hangar starts; `agent_models.rs`                       |
| Denkaufwand                                     | reasoning effort                  | Codex CLI's `model_reasoning_effort`                                                 |
| Bereitstellen, Kopie (einer Definition)         | provide, copy                     | the user's definitions in another program's format; `agent_provision.rs`             |
| Markierung (Kopie)                              | marker                            | first line of a copy with source and SHA-256; `codex_agents.rs`                      |
| Sicherung (ersetzte Datei)                      | backup                            | `<name>.toml.bak`, `.bak.2`: a user's file a copy replaced                           |
| Figur (Sub-Agent)                               | figure                            | picture of a definition: bundled, own image or initial; `agent_figures.rs`           |
| Grundfigur                                      | built-in figure                   | bundled figure `src/assets/sub-agents/<id>.webp`, `tools/figures.py`                 |
| Aussehen der Session (Figur)                    | session look                      | a sub-agent session's figure in place of the program badge; `SessionLook.tsx`        |
| Crew-Spalte                                     | crew column                       | sub-agents as figures beside the terminal; `CrewColumn.tsx`, `crew.ts`               |
| Crew-Leiste                                     | crew strip                        | narrow-pane form of the crew column, above the terminal                              |
| Sockel (Crew)                                   | plinth                            | block under a figure, its color shows the state; `CrewState`                         |
| Sprechblase (Crew)                              | speech bubble                     | short text of what a figure does; `crewText.ts`                                      |
| Kontextanzeige                                  | context meter                     | context window use of an agent session on its row, `ContextLine.tsx`                 |
| Kostenübersicht                                 | cost overview                     | tokens and estimated cost per session and day, `cost_service.rs`, `CostDialog.tsx`   |
| Preistabelle                                    | price table                       | dollars per million tokens per model, `model_prices.toml`, user `model-prices.toml`  |
| Kostenbuch                                      | cost ledger                       | per conversation id and local day, `cost-ledger.json`                                |
| Tagesbudget                                     | daily budget                      | dollars per day, one notice per level and day                                        |
| Vorwarnung (Tagesbudget)                        | early warning                     | notice at a share of the daily budget, level `early`; `budgetEarlyPercent`           |
| Übergabe (Session)                              | handover                          | new session of another agent in the same folder with a handover text, `handover.ts`  |
| Partnerprojekt                                  | partner project                   | project linked for cross-project messages, ADR 0012; not a linked repository         |
| Projektnachricht                                | cross-project message             | text a session sends to a partner project's session, ADR 0012                        |
| Kurzname (Partnerprojekt)                       | short name                        | `[a-z0-9-]{1,32}` of a partner project, in the header line; `partners.rs`            |
| Wartende Nachricht                              | waiting message                   | an agent's message until the person pastes or discards it; `partner_inbox.rs`        |
| Sendeprogramm                                   | send client                       | `hangar-send`, run by agents to send a cross-project message, ADR 0012               |
| Sende-Token                                     | send token                        | token of scope `Send` on the hook endpoint, in `HANGAR_SEND`                         |
| Partnerliste                                    | partner book                      | the named projects and their pairs, `partner-projects.json`; `PartnerBook`           |
| Link-Gruppe                                     | link group                        | running agent sessions linked to message each other, ADR 0014; in memory, `links.rs` |
| Mitgliedsname                                   | member name                       | `[a-z0-9-]{1,32}` name of a session in its link group, `hangar-send <name>`          |
| Schleifenbremse                                 | loop brake                        | at most 6 automatic deliveries per link group and minute, then a click; `Brake`      |
| Zustellbudget                                   | budget (link group)               | at most 20 automatic deliveries per link group until the person acts; `AUTO_BUDGET`  |
| Agenten-Befehl                                  | agent command                     | text the session menu types into a running session, `agent_commands`                 |
| Kontext komprimieren                            | compact (context)                 | the agent summarizes its conversation (`/compact`), label key `compact`              |
| Notiz                                           | note                              |                                                                                      |
| Bereich „Allgemein“                             | General area                      | notes without a folder                                                               |
| Papierkorb                                      | trash                             |                                                                                      |
| Aufmerksamkeit, Aufmerksamkeits-Puls            | attention, attention pulse        |                                                                                      |
| wartet (auf dich)                               | waiting (for you)                 |                                                                                      |
| Aussehen je Status                              | state look                        | color, pulse strength and duration per state, `stateLook.ts`                         |
| Pause (Session läuft, ruhig)                    | pause                             | UI name of the `idle` state of a running session                                     |
| Status-Chip                                     | status chip                       | state pill at the end of a session row's title line                                  |
| Session-Zeile (Anordnung)                       | row layout                        | which elements a session row shows, where and in which form, `rowLayout.ts`          |
| Vorlage (Session-Zeile)                         | row preset                        | a ready-made row layout to start from, `ROW_PRESETS`                                 |
| Einzeilig (Session-Zeilen)                      | one line                          | `oneLineRows.ts`; every row on one compact line, the layout before is kept           |
| fragt (Glocke, BEL)                             | asks (bell)                       | waiting reason after BEL, `AttentionReason`                                          |
| Hinweiston                                      | chime                             | synthesized two-tone sound, `chime.ts`                                               |
| Taskleiste blinken                              | taskbar flash                     |                                                                                      |
| Hintergrund-Session                             | background session                |                                                                                      |
| Neu bei Hangar                                  | What's New                        | dialog title                                                                         |
| Einstellungen                                   | Settings                          |                                                                                      |
| Bereich (in den Einstellungen)                  | section                           |                                                                                      |
| Sidebar, Seitenleiste                           | sidebar                           |                                                                                      |
| Gruppe (Sidebar)                                | group                             |                                                                                      |
| Agentengruppe (Sidebar)                         | agent group                       | top-level group of one program's sessions, worktree or not; `agentGroupKey`          |
| Projektgruppe (Sidebar)                         | project group                     | top-level group of one project folder's sessions; `sessionGroupKey`                  |
| Gruppierung (Sidebar)                           | session grouping                  | by program or by project; `SessionGrouping`                                          |
| Trennlinie (ziehbar)                            | splitter                          |                                                                                      |
| Trennlinie (statisch)                           | divider                           |                                                                                      |
| Tastenkürzel, Kürzel                            | shortcut                          |                                                                                      |
| Reihenfolge                                     | order                             |                                                                                      |
| Sichtbarkeit, ein-/ausblenden                   | visibility, show/hide             |                                                                                      |
| Platzhalter                                     | placeholder                       |                                                                                      |
| Klickschutz                                     | click guard                       |                                                                                      |
| Rückgängig                                      | undo                              |                                                                                      |
| Gegendruck                                      | backpressure                      |                                                                                      |
| Bündeln                                         | batching                          |                                                                                      |
| Prozessbaum                                     | process tree                      |                                                                                      |
| Starter (`.cmd`-Starter, npm-Shim)              | launcher script, npm shim         |                                                                                      |
| Programmversion                                 | program version                   |                                                                                      |
| Versionsquelle                                  | version source                    | `latest`                                                                             |
| Abfrageordner (Versionsabfrage)                 | probe folder                      | Hangar's own working folder for `--version` calls, `version::probe_folder`           |
| Ausführungsalias (Store-App)                    | execution alias                   | empty reparse-point file in `WindowsApps`, `exec_alias.rs`                           |
| Update-Hinweis, Update-Befehl                   | update command                    | field `update_hint` keeps its name                                                   |
| Programm-Update                                 | program update                    |                                                                                      |
| Update-Plan                                     | update plan                       | `run`, `copy` or `none`, with `planId`                                               |
| Weitere Kopie (eines Programms)                 | second copy                       | warning `second-copy`                                                                |
| Update-Ergebnis                                 | update outcome                    | event `program-update-result`                                                        |
| Installationsbefehl                             | install command                   |                                                                                      |
| Buildnummer                                     | build number                      |                                                                                      |
| kommende Version                                | upcoming version                  |                                                                                      |
| Versionsschema                                  | version scheme                    |                                                                                      |
| Signatur (Updater)                              | updater signature                 | minisign, `.sig`                                                                     |
| Signatur (Authenticode), Signieren              | code signing                      |                                                                                      |
| Zeitstempeldienst                               | timestamp server                  |                                                                                      |
| Installer                                       | installer                         |                                                                                      |
| Prüfkette                                       | check suite                       | fmt, clippy, tests, lint, build                                                      |
| Abnahme                                         | acceptance review                 | role `manager`                                                                       |
| Nacharbeit                                      | rework                            |                                                                                      |
| Sichtprüfung                                    | visual check                      |                                                                                      |
| Freigabe                                        | approval                          |                                                                                      |
| Wiki: Quelle, Synthese, Entscheidung            | source, synthesis, decision (ADR) |                                                                                      |
| Ingest                                          | ingest                            |                                                                                      |
| Messung                                         | measurement                       |                                                                                      |
| Vertrag (Befehls-)                              | contract                          |                                                                                      |
| Ungeprüft                                       | unverified                        | `[UNVERIFIED]`                                                                       |
| Wortmarke                                       | wordmark                          |                                                                                      |
| Gefaltetes H (Logo)                             | folded H                          |                                                                                      |
| Bereich (Notizen: Arbeitsordner oder Allgemein) | scope                             | `NoteFolder`; the list shows one scope or all (`NotesScope`)                         |
| Notizbereich                                    | notes panel                       |                                                                                      |
| Bereichs-Chips                                  | scope chips                       | "This project", "All", "General" above the notes list; `notesScope.ts`               |
| Seite (Notizen: Privat oder Geteilt)            | notes side                        | `NotesSide`; switch above the notes list, `notesSide.ts`                             |
| Schnelle Notiz                                  | quick note                        | Ctrl+Shift+N; created once it has content, an empty one is dropped                   |
| Eigenes Notizfenster                            | detached window                   | window label `notes`; `NotesWindow.tsx`, `notesBridge.ts`                            |
| Rechter Bereich                                 | right panel                       | tabs Notes and Changes; `SidePanel.tsx`                                              |
| Wartehinweis am Listenrand                      | waiting edge hint                 | "1 waiting" above or below the session list; `waitingHint.ts`                        |
| Benutzerordner                                  | home folder                       |                                                                                      |
| Platzhalter-Session                             | placeholder session               |                                                                                      |
| Update-Punkt                                    | update dot                        |                                                                                      |
| Trennzeile (Wiederherstellen)                   | divider line                      | marker in restored scrollback                                                        |
| Kontextmenü                                     | context menu                      |                                                                                      |
| Programmauswahl                                 | program picker                    |                                                                                      |
| Drosselung, gedrosselt                          | throttling, throttled             |                                                                                      |
| Platz (gemerkte Position einer Session)         | slot                              |                                                                                      |
| Snap-Layout                                     | snap layout                       | preset arrangement of the panes, `snapLayouts.ts`                                    |
| Zone (Snap-Layout)                              | zone                              | the area of one pane in a snap layout                                                |
| Aufteilung (Snap-Layout)                        | pane ratios                       | splitter positions of a snap layout as fractions, `paneRatios.ts`                    |
| Layout-Miniatur                                 | layout glyph                      | snap layout in miniature on a sidebar row, `LayoutGlyph.tsx`                         |
| Layout-Auswahl, Snap-Leiste                     | layout picker, snap bar           | `SnapBar.tsx`                                                                        |
| Ablagelinie                                     | drop line                         | where a dragged session lands in the sidebar                                         |
| Ende (Prozess), Beenden (App)                   | exit, quit                        |                                                                                      |
| Oberfläche                                      | UI                                |                                                                                      |
| Favorit (Ordner), Lieblingsordner               | favorite folder                   | `folders.ts`                                                                         |
| Zuletzt genutzte Ordner                         | recent folders                    | at most 8                                                                            |
| Startordner-Menü                                | folder menu                       | "Project folder" menu                                                                |
| Startziel                                       | start target                      | where the next session starts, `startTarget.ts`                                      |
| Laufende Projekte                               | running projects                  | project folders of running sessions, folder menu                                     |
| Suchleiste, Treffer                             | search bar, match                 | `SearchBar.tsx`                                                                      |
| Farbschema                                      | color scheme                      | `terminalLook.ts`                                                                    |
| Erscheinungsbild des Terminals                  | terminal look                     | scheme, font, cursor                                                                 |
| Ziehen und Ablegen                              | drag and drop                     | files onto a pane                                                                    |
| Bereich (Terminal-Fläche einer Session)         | pane                              | `TerminalPane.tsx`                                                                   |
| Maskieren (Pfade für eine Shell)                | quoting                           | `pathQuote.ts`                                                                       |
| Exportieren (Session)                           | export                            | `sessionExport.ts`                                                                   |
| Bericht (Session)                               | session report                    | `report/`, the agent's transcript as Markdown, `export_report`                       |
| Transkript                                      | transcript                        | the agent CLI's own conversation file, not the scrollback                            |
| Schlüsselbund (des Betriebssystems)             | keychain                          | `secrets.rs`; Credential Manager on Windows                                          |
| Geheime Variable                                | secret variable                   | `secret_env`; only the name is in the file                                           |
| Infobereich, Tray-Symbol                        | notification area, tray icon      | `tray.rs`                                                                            |
| Einzelinstanz (nur ein Hangar je Benutzer)      | single instance                   | `single_instance.rs`; release builds only, debug builds exempt                       |
| WSL-Distribution                                | WSL distribution                  | `wsl.rs`                                                                             |
| Geteilte Ansicht                                | split view                        | `split.ts`; one, two side by side or four in a grid                                  |
| Fokussierter Bereich                            | focused pane                      | the pane with the active session                                                     |
| Schnellzugriff                                  | quick access, launcher            | `launcher.ts`; "launcher" in code                                                    |
| Suche in allen Sessions                         | search across sessions            | `sessionSearch.ts`; output and scrollback of every open terminal                     |
| Übersicht (alle Sessions als Kacheln)           | overview, tile                    | `overview.ts`; live tiles with the last lines of each session                        |
| Startansicht                                    | home view                         | house button in the sidebar; sessions put aside (`putAside`), they keep running      |
| Projekt-Übersicht (Startansicht)                | project overview                  | `ProjectOverview.tsx`; one card per project of the sessions, above the calendar      |
| Projektkarte                                    | project card                      | `projectCards.ts`; a repository with its worktrees, or a plain folder                |
| Checkout (einer Projektkarte)                   | checkout                          | a work tree of the project with sessions; `RepoPlace.workTree`                       |
| Hover-Vorschau (Session)                        | session peek                      | `peek.ts`, `SessionPeek.tsx`; last lines beside a sidebar row while the mouse rests  |
| Vorgemerkte Nachricht                           | queued message                    | `messageQueue.ts`; sent once the session is done, kept while it asks                 |
| Rückfragen-Schutz                               | question guard                    | `promptGuard.ts`; a question on screen holds the queued message back                 |
| Hänger-Hinweis                                  | stall hint                        | `stall.ts`; a working agent shows nothing new for a while, quiet text on its row     |
| Warte-Hinweis                                   | wait hint                         | `waitHint.ts`; a session waits for you past a threshold, its row says how long       |
| Ruhezeiten (Hinweise)                           | quiet hours                       | `quietHours.ts`; daily window without taskbar flash, chime and desktop notification  |
| Textbaustein                                    | snippet                           | `snippets.ts`                                                                        |
| Variable (Textbaustein)                         | snippet variable                  | `snippetVariables.ts`; `{{file}}` and others, filled per session when sending        |
| Workspace-Vorlage                               | workspace template                | `templates.ts`; named set of sessions started together, stored per machine           |
| An mehrere senden                               | broadcast                         | `broadcast.ts`                                                                       |
| Markierung (Session)                            | session marker                    | `marker.ts`; color and emoji                                                         |
| Desktop-Benachrichtigung                        | desktop notification              | `desktopNotify.ts`; `notify_session`                                                 |
| Ressourcen (Session)                            | resources                         | `resources.rs`; CPU share and resident memory of a session's process tree            |
| Ausreißer                                       | outlier                           | a session that keeps 0.9 cores busy for 30 s or holds more than 4 GiB of memory      |
| Systemleiste                                    | system bar                        | `SystemBar.tsx`, `system_usage.rs`; machine CPU, RAM and GPU above New session       |
| Energiesparen im Akkubetrieb                    | power saving                      | `power.ts`, `power.rs`; slower polls and still pulses while on battery               |
| Akkubetrieb, Netzbetrieb                        | on battery, on AC                 | `PowerState`; `noBattery` for machines without one                                   |
| Energiesparmodus (des Systems)                  | battery saver                     | Windows `SystemStatusFlag`; state `saver`                                            |
| Push aufs Handy                                 | push (to the phone)               | `push.rs`, `phonePush.ts`; ntfy or Pushover, opt-in                                  |
| Statuszeile (Session)                           | status line                       | `statusLine.ts`; status, folder, branch, run time                                    |
| Laufzeit (Session)                              | run time                          | since the process started                                                            |
| Taskleisten-Zähler                              | taskbar badge                     | `taskbarBadge.ts`; overlay icon on Windows                                           |
| Eigener Worktree (Session)                      | own worktree                      | `worktree.ts`; branch `hangar/<slug>` per session                                    |
| Worktree-Aufräumer                              | worktree cleanup                  | dialog listing Hangar's own worktrees; `worktree_cleanup.rs`, `worktreeCleanup.ts`   |
| Private Session                                 | private session                   | starts in the folder, no agent gate, never shared; ADR 0008                          |
| Team-Session                                    | team session                      | own worktree, agent gate, shared with the team; `SessionMode`                        |
| Art der Session (Privat oder Team)              | session mode                      | `SessionMode`; switch below "Project folder", `startMode.ts`                         |
| Hook-Format (Agent-Gate)                        | hook format                       | how the gate hands hooks to one program, `HookFormat`                                |
| Hook-Helfer (Agent-Gate)                        | gate helper                       | `hangar gate-hook`, `gate_hook.rs`; an agent's command hook                          |
| Änderungen (Bereich)                            | changes panel                     | `gitDiff.ts`, `ChangesPanel.tsx`; Ctrl+Shift+D                                       |
| Aktivitätskalender                              | activity calendar                 | the user's own commits of the last year in the empty window; `activity.ts`           |
| Tagesrückblick                                  | daily review                      | one local day's sessions, times and commits; `daily_review.rs`, ADR 0011             |
| Morgenzusammenfassung                           | morning summary                   | yesterday's short review on the first start or focus of a day; `morningReview.ts`    |
| Arbeitsprotokoll                                | work log                          | JSON Lines per UTC day under `activity/`, `work_log.rs`; not the activity log        |
| Arbeitsphase                                    | working phase                     | one stretch of the state "working", logged when it ends                              |
| Aktive Zeit                                     | active time                       | the union of the working phases; parallel sessions count once                        |
| Commit-Verlauf (Änderungen)                     | commit history                    | `gitHistory.ts`, `CommitHistory.tsx`; "Commits" view of the changes panel            |
| Zeitleiste (Änderungen)                         | timeline                          | `timeline.ts`, `TimelineView.tsx`; "Timeline" view of the changes panel              |
| Zwischenstand, Schnappschuss                    | snapshot                          | silent commit of the work tree after an agent step; side repository                  |
| Auf diesen Stand zurück                         | go back (restore)                 | work tree to a snapshot, the current state saved first; `snapshot_restore`           |
| Stagen, Unstagen                                | stage, unstage                    | a file into or out of the index; stage box per file, `git_stage`, `git_unstage`      |
| Verwerfen (Änderungen einer Datei)              | discard                           | back to HEAD, an untracked file is deleted; asks first; `git_discard`                |
| Commit-Feld                                     | commit box                        | message and commit button above the uncommitted changes; `CommitBox.tsx`             |
| Letzter Prompt (Leiste)                         | last prompt bar                   | the user's last prompt above an agent's terminal; click jumps there; `lastPrompt.ts` |
| Zeilenkommentar (Diff)                          | line comment                      | `@path:line: text` from a diff line into the agent's prompt; `lineComment`           |
| Fertig-Hinweis                                  | done notice                       | changed files after an agent session stops working; `doneNotice.ts`                  |
| Name aus dem ersten Prompt                      | prompt title                      | an agent session named after its first task; `promptTitle.ts`                        |
| Dev-Server (lokal)                              | dev server                        | local address found in a session's output; `devServer.ts`, globe on the row          |
| Upstream (Branch)                               | upstream                          | the branch's tracking branch, e.g. `origin/main`                                     |
| Nicht gepusht                                   | unpushed                          | on HEAD but not on the upstream; `CommitSummary.pushed`                              |
| Anspruch (geänderte Datei einer Session)        | claim                             | `claims.rs`; path, kind of change, line ranges on the base side, area                |
| Überschneidung                                  | overlap                           | `OverlapLevel`: same area, same file, overlapping lines                              |
| Basis-Commit                                    | base commit                       | HEAD of the session's worktree; lines are compared on this side                      |
| Bereich (Claims: erster Ordner eines Pfads)     | area                              | `Claim.area`; configurable per repository later                                      |
| Geteilter Ordner                                | shared folder                     | several sessions in one folder share its changes                                     |
| Sperre vor dem Bearbeiten                       | gate                              | `gate.ts`; holds an agent's edit of a file another session is changing               |
| Offene Entscheidung                             | pending decision                  | `PendingDecision`; allow once, allow for this file or deny                           |
| Gemerkte Antwort (Gate-Regel)                   | gate rule                         | `gate_rules.rs`; always or never, for a file or folder of one project                |
| Automatisch beantwortet (Gate-Protokoll)        | rule log                          | `RuleAnswer`; the last edits a gate rule answered, newest first                      |
| Schutz vor gefährlichen Befehlen                | command guard                     | `command_guard.rs`; asks before a team session's agent runs a matching command       |
| Befehlsregel                                    | guard rule                        | one `[[rule]]` of `command-guard.toml`: id, description, matcher                     |
| Eingebaute Befehlsregel                         | built-in rule                     | `hangar-*` rule not in the file: too complex, stdin, too long, Hangar's settings     |
| Gemerkte Antwort (Befehlsregel)                 | command rule                      | a gate rule with scope `command` for one guard rule in one project                   |
| Grund (nicht teamfähig)                         | team block                        | `TeamBlock`; why a program cannot be a team session on this machine                  |
| Reservierung (Aufgabe)                          | reservation                       | `reservation.rs` (protocol crate); first wins, expires when idle                     |
| Freigabe-Anfrage                                | release request                   | asks the holder of a reservation to hand the task over                               |
| Halter (einer Reservierung)                     | holder                            | member and device holding a reservation                                              |
| Schlüssel-Hash                                  | keyed hash                        | HMAC-SHA256 under the team's matching key; compared by equality only                 |
| Abgleichschlüssel                               | matching key                      | `MatchKey`, derived per epoch from the team key                                      |
| Epoche (Teamschlüssel)                          | epoch                             | generation of the team key; new on every leave                                       |
| Chiffrat                                        | ciphertext                        | opaque encrypted bytes, base64url; format in step E                                  |
| Ereignisprotokoll, Sequenznummer                | event log, sequence number        | per team; replay window, `resync_required`                                           |
| Operations-ID                                   | op id                             | per client intent; a retry carries the same one                                      |
| Protokoll-Crate                                 | protocol crate                    | `crates/hangar-team-protocol`                                                        |
| Programm-Editor                                 | profile editor                    | `ProfileEditor.tsx`; Settings, Programs                                              |
| Anpassen (eingebauter Eintrag)                  | customize                         | saving a catalog or WSL entry as the user's own profile                              |
| Konto (Hangar-Konto)                            | account                           | `account.rs`; optional, Hangar works without it                                      |
| Anmeldung (Konto)                               | sign-in                           | OAuth device flow; the refresh token lives only in the keychain                      |
| Bestätigungscode                                | user code                         | `XXXX-XXXX`, confirmed in the browser                                                |
| Serveradresse                                   | server address                    | API base of the official service or a self-hosted server; `BaseUrl`                  |
| Verbindungstest (eigener Server)                | connection test                   | account API, relay health check, live channel upgrade; `server_check.rs`             |
| Anleitung zum eigenen Server                    | self-hosting guide                | hangar-server's README, opened as `AccountPage::SelfHostingGuide`                    |
| Projekt (Team)                                  | project, team                     | a project on the website is a team; `Project`                                        |
| Platz (Lizenz)                                  | seat                              | counted per account of the project's owner (ADR 0005)                                |
| Tarif                                           | plan                              | `Project.plan`, as the server names it                                               |
| Einladung                                       | invitation                        | sent and accepted on the website                                                     |
| Teamverbindung                                  | team connection                   | per project, turned on by "Share with team"; `TeamStatus` in `team.ts`               |
| Sicherheitsnummer                               | safety number                     | 6 groups of 5 digits, compared on both devices before a confirmation                 |
| Bestätigung (neues Gerät)                       | confirmation                      | an own device or an owner/admin accepts a new device; `PendingConfirmation`          |
| Anwesenheit (Teamgerät)                         | presence                          | `presence.set`; the device's sessions in linked repositories, sealed                 |
| Verknüpftes Repository                          | linked repository                 | `LinkedRepo`; only its sessions are shared with a project                            |
| Teamansicht                                     | Team view                         | `TeamActivityView.tsx`; teammates' sessions per shared project                       |
| Überschneidung mit dem Team                     | teammate overlap                  | `TeammateOverlap`; the relay's `claim.overlap`, refined locally                      |
| Zustand (Session, fürs Team)                    | session state                     | working, waiting, idle; `SessionState`                                               |
| Anderes Konto (Teamgerät)                       | other account                     | `otherAccount`; a team device acts only for the account it was bound to              |
| Abweichende Historie (Teamgerät)                | divergent device                  | `DivergentDevice`; set aside after signing another history                           |
| Aufgabe (im Team reserviert)                    | task                              | a short title the team reserves; the relay sees only its keyed hash                  |
| Reservierungsnotiz                              | reservation note                  | sealed title, name, area and session of a reservation, signed by the holder          |
| Reservierungsversuch                            | attempt                           | `AttemptView`; this device's reservation the relay has not granted (yet)             |
| Reservierter Bereich                            | reserved area                     | a folder a reservation covers; teammates' agents pause there (gate)                  |
| Dateibaum (rechter Bereich)                     | file tree                         | tab `files`; `FilesPanel.tsx`, `fileTree.ts`; folders listed lazily                  |
| Dateiansicht (Fenster)                          | file viewer                       | read-only window for one file; `FileViewerWindow.tsx`, `file_viewer.rs`              |
| Dateiansicht-Thema                              | viewer theme                      | colors of the file viewer, dark and light; `viewerThemes.ts`                         |
| Änderungspfad                                   | change path                       | folders down to each changed file, marked in the tree; `onChangePath`                |
| Dateifilter                                     | tree filter                       | field above the file tree; `filterTree`, index from `list_session_paths`             |
| Erwähnung (@-Pfad)                              | mention                           | `@path ` typed into a terminal by right click or drag; `rowMention`                  |
| Vorschau (Dateiansicht)                         | preview                           | Markdown rendered or image drawn in the file viewer; `ViewerMode`                    |
| Vergleich mit dem letzten Commit                | compare with the last commit      | the file's diff against `HEAD` in the file viewer; `file_viewer_diff`                |
| Live-Aktualisierung                             | live refresh                      | the file viewer reads its file again when it changed; `pollStamps`                   |
| Aktivitätskarte                                 | activity map                      | Files tab; rows tinted in heat levels 1 to 4 (`heatLevel`); `fileActivity.ts`        |
| Änderungsereignis                               | change event                      | a file newly held, or held with other lines; `ActivityEvent`                         |
| Ereignisprotokoll (Aktivitätskarte)             | event log                         | the activity map's capped change events per project; `EventLog`, `activityStore.ts`  |
| Projekt (Aktivitätskarte)                       | activity project                  | a repository's main folder with its Hangar worktrees; `projectKeyOf`                 |
| Zeitraum (Aktivitätskarte)                      | time window                       | last hour, today, 7 days; `HeatWindow`                                               |
| Abspielen (Aktivitätskarte)                     | replay                            | the time window in `PLAY_STEPS` steps, play and pause                                |
| Halter (einer Datei)                            | file holder                       | a teammate session or a worktree of this device changing a file                      |
| Globale Anweisungen (HANGAR.md)                 | global instructions               | `HANGAR.md` in the config folder; `instructions.rs`, `instruction_args`              |
| Erster Prompt (beim Start)                      | first prompt                      | user message a fresh start begins with; `StartPrompt`, `prompt_args`                 |
| Upstream-Prüfung (Worktree)                     | upstream check                    | first prompt of a fresh worktree session; `[start_prompts] worktree`                 |
| Arena (Agenten-Arena)                           | agent arena, arena                | one task for 2 to 4 agents in worktrees from one commit; `arena.rs`, `arena.ts`      |
| Teilnehmer (Arena)                              | contestant                        | one agent of an arena with its session and worktree; `Contestant`                    |
| Übernehmen (Arena)                              | take over                         | the winner's changes into the project folder, uncommitted; `arena_apply`             |
| Diktieren                                       | dictation                         | hold a key, speak, the text lands in the prompt; `src-tauri/src/dictation/`          |
| Diktiertaste (gehalten)                         | dictation key                     | Ctrl+Shift+Space, held, not pressed; `KeyHold`, `DICTATION_SHORTCUT`                 |
| Sprachmodell                                    | speech model                      | a whisper model file, downloaded after consent; `model.rs`                           |
| Aufnahmeanzeige                                 | recording indicator               | the pill on the pane while the microphone is open; `DictationIndicator.tsx`          |
| Energieschwelle                                 | energy gate                       | drops silence before recognition; `has_speech`                                       |
| Erkannter Text (Diktieren)                      | transcript (dictation)            | in memory until taken once; `clean_transcript`, `dictation_take_text`                |
| Aufnahmefrist                                   | recording deadline                | a recording without stop ends on its own; `RECORDING_DEADLINE`                       |
| Login-Shell-PATH                                | login shell PATH                  | PATH of `$SHELL -ilc`, put in front at startup; `login_path.rs`                      |
| AppImage-Variablen                              | AppImage variables                | what AppRun sets for Hangar, removed for child processes; `appimage_fixes`           |
| Dateimanager                                    | file manager                      | Explorer, Finder or the Linux desktop's file manager; `revealLabelKey`               |
| Datenfluss (Baumlinien)                         | tree flow                         | optional light on the sidebar tree guides to working sessions; `treeFlow.ts`         |
