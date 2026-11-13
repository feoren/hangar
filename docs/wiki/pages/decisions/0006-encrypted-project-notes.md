---
title: "ADR 0006: Encrypted project notes in the repository"
type: decision
status: accepted
superseded_by:
tags: [notes, e2e, git, teams, keychain, xchacha20poly1305, hkdf]
sources: ["[[note-gitattributes-measurement]]"]
created: 2026-11-10
updated: 2026-11-13
---

# ADR 0006: Encrypted project notes in the repository

## Context
Today every note is private to one computer: it lives in the app data folder, keyed by the
working folder ([[0004-notes-per-working-folder]], contract in [[notes]]). ADR 0004 rejected
storing notes in the project because they would end up in commits in plain text.

The owner now wants **project notes**: notes that live inside the project in a dedicated folder,
travel with the repository (so the team, and the owner's other computers, get them through Git),
and are **encrypted**, with the key created by the project's owner in Hangar, or on the website if
possible. The notes area must stay absolutely simple to use.

Building blocks that already exist:
- Team end-to-end crypto ([[team-crypto]]): device keys in the OS keychain, a signed membership
  log, a random 256-bit secret per key epoch wrapped to every confirmed device, a new epoch after
  every member or device removal, sealed and signed records (`EpochCipher`, XChaCha20-Poly1305),
  and a printable recovery code. A device keeps only the newest 8 epoch secrets
  (`team::KEPT_EPOCHS`).
- A shared project on the website is a team; a clone is linked to it explicitly by root commit and
  folder (`team::LinkedRepo`), so nothing is shared from a repository the person did not tick.
- The notes UI with the chips "This project", "All", "General", search, trash with undo and the
  detached window ([[notes-and-start-usability]]).

## Options

### Where the key comes from
1. **A password per project.** Everyone types it; weak passwords, sharing it over chat, no way to
   lock out a person who leaves. Rejected.
2. **Derive the notes key from the team's epoch secrets.** Nothing new to store, but a note
   written in epoch 3 needs epoch 3's secret: a new member holds only the current one (history is
   opt-in), a device keeps only 8, and every rotation would mean re-encrypting every note, which is
   churn in Git for every member. Rejected.
3. **A notes key ring of its own, distributed with the team crypto** (chosen for teams). Random
   notes keys, sealed as one signed team record under the current epoch; the device that rotates
   the epoch re-seals it. New members get every key at once, Git sees no key churn.
4. **Key ring sealed under epoch keys inside the repository.** Works without a server change, but
   every rotation needs a commit before a new member can read anything, and that commit must come
   from an admin who has the clone. Rejected for teams; the same file idea is used for solo mode,
   where nothing rotates.
5. **Key created on the website.** The website would either hold the key or serve the code that
   handles it; either way the operator could read the notes. Contradicts end-to-end encryption.
   Rejected (see "Team server and website").

### Where the files live
1. `.note/` as the owner suggested.
2. **`.hangar/notes/`** (chosen): one hidden top level folder per tool is the convention (`.github`,
   `.vscode`, `.idea`), it cannot collide with another tool's `.note`/`.notes`, and later Hangar
   project files fit under the same folder.
3. A separate Git ref (`refs/hangar/notes`): no working tree churn, but invisible to hosters, not
   pushed or fetched by default, and fragile with every Git client. Rejected.

## Decision

### 1. Storage layout
```
.hangar/
  notes/
    .gitattributes        written by Hangar: "* -text -diff -merge"
    README.md             plaintext: "Encrypted Hangar project notes", no project data
    vault.json            plaintext vault header (below)
    <note id>.note        one file per note, encrypted
```
- **One file per note**, named by a random note id (UUID v4, lower case). Titles never appear in
  file names. Separate files mean two people editing different notes never conflict, and adding a
  note never touches another file.
- **No shared index or order file.** Project notes sort by their last change (the panel already
  sorts that way); a manual order would rewrite many files on every drag. The trash is local:
  deleting removes the file from the working tree and keeps the encrypted file in the scope's local
  `.trash/` in the app data folder, so Undo and "Recently deleted" work as today; Git history keeps
  it as well.
- **`.gitattributes`** marks everything binary (`-text -diff -merge`): no CRLF conversion on
  Windows (the bytes stay stable), no useless ciphertext diffs, and on a conflict Git keeps "ours"
  in the working tree instead of writing conflict markers into ciphertext; Hangar resolves it
  (section 6). Measured on Windows with git 2.56 ([[note-gitattributes-measurement]]): the bytes
  stay unconverted under every `core.autocrlf`, and a conflict leaves `UU` (modify/delete `UD`,
  add/add `AA`) with the stages readable through `git cat-file blob :2:<path>` and `:3:<path>`.
  A rebase swaps what stages 2 and 3 mean, so Hangar decides by the note's `updated` time.
- **Committed on purpose.** Hangar never edits `.gitignore`. If `git check-ignore` says the folder
  is ignored, the notes panel says so ("Your .gitignore keeps shared notes out of Git") instead of
  silently not sharing.
- **Which checkout:** project notes are read and written in the clone's **main worktree** (the
  first entry of `git worktree list`), whichever worktree a session runs in. Otherwise every task
  worktree would fork the notes per branch and they would arrive only with the task's merge.
  Bare clones and a missing main worktree fall back to the session's worktree.
- **Hangar never commits or pushes notes in phase 1.** Notes travel with the person's normal
  commits; the panel shows "2 note changes not committed yet". A "Commit notes" action follows in
  phase 3.

### 2. File formats
**`vault.json`** (UTF-8, LF, keys in fixed order, pretty printed, rewritten only when its content
changes):
```json
{
  "format": 1,
  "vault": "<32 lower case hex digits, random>",
  "mode": "team",
  "team": "<16 hex digits: first 8 bytes of SHA-256(label, team id)>"
}
```
Solo mode has `"mode": "solo"` and `"codeSlot": "<base64url>"` instead of `"team"`. The team id
itself is not written: the repository may be public, and the hint is enough for a member's Hangar
to recognise one of its own teams. An unknown `format` makes Hangar show the notes as "made with a
newer Hangar", read only.

**`<note id>.note`** (ASCII, two lines, LF):
```
hangar-note 1 <key id, 16 hex digits>
<base64url without padding: nonce (24 bytes) || XChaCha20-Poly1305 ciphertext and tag>
```
Plaintext outside the ciphertext: the format version, the note id (file name) and which key
encrypted it. Nothing else; not the title, not the times, not the author.

**Plaintext inside** (before padding): one JSON line, a line feed, then the Markdown body.
```json
{"v":1,"title":"...","created":1762770000000,"updated":1762770000000,"by":"Display name"}
```
`by` is the display name of the last editor in team mode (empty in solo mode). The plaintext is
padded with zero bytes after a length prefix to the next multiple of 256 bytes, so the file size
reveals the length only roughly. Limits as today: Markdown at most 1 MiB UTF-8, title rules of
[[notes]].

**Associated data and key per file:**
```
file key = HKDF-SHA256(salt none, ikm = notes key,
                       info = field("hangar-notes/v1/file") || field(vault) || field(note id))
aad      = field("hangar-notes/v1/note") || field(format 1) || field(vault)
           || field(note id) || field(key id)
```
`field` is the length-prefixed framing of the crypto crate. A fresh random 192-bit nonce per
write. Binding vault and note id means a file renamed to another note's id, or copied into
another repository, does not open; binding the key id means the header cannot be changed to point
at another key.

**Churn:** encryption makes every write change the whole file, so Hangar writes a note only when
its decrypted content actually changed (not on open, not on a no-op save, debounced as today), and
never re-encrypts untouched notes, also not after a key change (section 3). A Git diff therefore
shows exactly the notes someone edited.

### 3. Key model
A **notes key** is a random 256-bit key with a random 64-bit **key id**. A **key ring** is the set
of all notes keys a project ever had; new writes always use the newest, reads use whichever the
file names. Keys are only ever added, never removed, so an old note stays readable.

**Team mode (preferred, owner's "create the key" = turn on shared notes for a team project):**
- An owner or admin turns on shared notes in a clone linked to the team project. Hangar creates
  the first notes key and seals the key ring as a new team record kind, **`notes-keys`**, with
  `EpochCipher::seal` under the current epoch: team, epoch, kind, record id, sender, log head and
  counter are bound, and the record is signed by the device. Members' devices open it with
  `open_verified` and accept it only from a device whose member is owner or admin in the verified
  log at the record's head. One key ring per team serves every repository linked to it; the vault
  id keeps their files apart.
- **Members get the keys through the existing device confirmation.** A confirmed device holds the
  current epoch, fetches the `notes-keys` record and unlocks. No extra step, no code to type.
- **Rotation:** the device that rotates the epoch (always an owner or admin) re-seals the key ring
  under the new epoch in the same step, from its keychain cache. If the epoch changed because a
  member or device was **removed**, it adds a new notes key first, so notes written afterwards are
  unreadable for the removed person. Untouched notes keep their old key: the removed person had
  that key and the Git history anyway, re-encrypting would protect nothing. If the rotating device
  has no cached key ring, the next owner or admin device that is online re-seals it.
- **Monotonic:** a device keeps the union of all keys it has accepted; an older record replayed by
  the relay cannot take keys away, and a record with fewer keys is ignored.
- **Cache:** the key ring is cached in the OS keychain (service `hangar-team`, entry per team,
  `v1 <key id>:<hex>...`). The Windows credential store holds at most 1280 UTF-16 units per entry
  (`secrets::MAX_VALUE_UNITS`), about 16 keys; older keys are fetched from the record on demand.

**Solo mode (projects without a team):**
- Turning on shared notes creates the first notes key, stores the key ring in the keychain (entry
  per vault) and shows a **notes recovery code** once: 128 random bits, Crockford base32 in eight
  groups of four with a checksum, the format of the team recovery code with its own version byte,
  so the two can never be mixed up.
- `vault.json` carries a **code slot**: the key ring sealed with XChaCha20-Poly1305 under
  `HKDF-SHA256(ikm = code entropy, info = field("hangar-notes/v1/code-slot") || field(vault))`.
  On another computer or after the keychain is lost, typing the code once unlocks the project.
  128 bits suffice without password hashing, as for the team recovery code: the code is random,
  not chosen by a person.
- **Solo to team:** sharing the project with a team later moves the key ring into the team's
  `notes-keys` record, adds a new notes key and removes the code slot from `vault.json`. The old
  code still opens the notes written before the switch through Git history; they were the owner's
  own.

**New clone:**
| Situation | What the person sees |
| --- | --- |
| Team mode, device confirmed, clone linked | nothing: unlocked automatically |
| Team mode, member of the hinted team, clone not linked | "These notes belong to *Project*. Link this folder?" (the existing tick) |
| Team mode, device not confirmed yet | the existing "waiting for confirmation" with its safety number |
| Team mode, not a member | "Shared notes of a team you are not in", locked |
| Solo mode, key ring in the keychain | nothing: unlocked |
| Solo mode, no key | "Enter the notes recovery code" |

**Lost keys:** team mode recovers through any other device or the team recovery code; solo mode
only through the notes recovery code. Without it the notes are gone for good; setup says so in one
sentence and offers "Copy" and "Save as file" before the switch can be turned on.

### 4. Algorithms and libraries
No new dependency. Everything is in the tree through `crates/hangar-team-crypto`:
XChaCha20-Poly1305 (`chacha20poly1305` 0.11, random 192-bit nonces make nonce reuse across
devices and branches negligible without coordination), HKDF-SHA256 (`hkdf` 0.13, `sha2` 0.11),
`getrandom` 0.4 (fallible), `data-encoding` 2 (canonical base64url), `zeroize`, and the existing
recovery code encoding. The sealing of `notes-keys` reuses `EpochCipher` unchanged.
The pure part (file format, key ring, code slot) goes into a new module `notes` of the crypto
crate, with test vectors in `test_vectors.py` like the other formats; the app part
(`src-tauri/src/project_notes.rs`) does the file I/O, Git calls and keychain access.

### 5. Team server and website
- **Relay (protocol crate, team server):** one new record kind and three messages:
  `notes.keys.set` (sealed, owner or admin ticket only, current epoch only, one per team, the
  newest replaces the older), `notes.keys.get` (any member) and the event `notes.keys.updated`.
  About 2 KiB per team. The relay stores an opaque blob; it never sees a notes key or a note.
- **Website:** nothing is required. Optional: a line on the project page "Shared notes: on, keys
  are created in the Hangar app" (the relay can tell from the stored record that notes exist).
  **Keys are never created on the website:** a key made in the browser comes from code the server
  delivers, so the operator could read every note; that breaks the promise of [[team-crypto]].
- **What stays out of the server:** notes themselves (they travel through Git), titles, the vault
  id, the repository.

### 6. User experience
- **Where:** the "This project" chip shows two groups: **Shared** (lock icon, "in the
  repository") and **Only on this computer** (today's notes). "All" and search include unlocked
  shared notes with the same icon. Each group has its own "+"; a note's More menu gets "Move to
  shared notes" and "Move to this computer". Decrypted notes are never written to disk; search
  decrypts in memory. *Amended 2026-11-13:* the two groups became a **Private | Shared** switch
  above the list; see [[notes]].
- **First-time setup in one step:** in a Git repository without `.hangar/notes`, the Shared group
  shows one line: "Share notes with everyone who has this repository, encrypted." and a button
  **Turn on**. One dialog: in a team project (owner or admin) it says who will be able to read
  them and has one button; in a project without a team it shows the recovery code with Copy and
  Save and the button enables after "I saved it". Members without the right see "Ask an owner or
  admin of *Project* to turn on shared notes".
- **States** of the Shared group: *Off* (setup line), *Unlocked* (the notes), *Locked* (one line
  with the reason and the one action from the table in section 3), *Newer format* (read only),
  *Conflict* (below). Never more than one action per state.
- **Merge conflicts:** one file per note makes them rare: only when two people changed the same
  note. When `git status` shows a conflicted `.note` file, Hangar resolves it without asking,
  losing nothing: the version with the later `updated` stays, the other becomes a new note
  "*Title* (other version, *Name*)" in the same vault; Hangar writes both and stages exactly these
  files (`git add -- <path>`), never commits. A notice says what happened. An in-app three-way
  text merge follows in phase 3. A conflict in `vault.json` (two people turned on notes on two
  branches) keeps the vault whose id sorts first; notes of the other vault are re-encrypted into
  it by a device that holds both key rings, otherwise they stay locked with "made in another
  vault".
- **Agents** see only ciphertext in `.hangar/notes`; "Send to terminal" works for shared notes as
  for private ones.

### 7. Threat model
- **Protected:** title, content, times and author against everyone with read access to the
  repository (a public host, forks, CI, backups), the team server operator, and removed members
  for notes written after their removal. Each file is authenticated: a changed, truncated,
  renamed or transplanted file does not open.
- **Not protected:** the number of notes, their rough size, which notes change and when, and who
  committed (Git metadata); rolling a note back to an older version or deleting it (anyone with
  write access can, as with code); a member altering another member's note or writing a false
  `by` (any key holder can; `by` is a hint, not a signature); a compromised computer, the
  keychain read by another process of the same user, the plaintext in the WebView's memory.
- **Relay:** cannot read or inject keys (`notes-keys` is signed and checked against the verified
  log's roles); can withhold the record, which only delays unlocking on new devices.
- **Removed members** keep every note up to their removal; the UI says so in the removal dialog,
  as it does for team content.

### 8. Non-goals
Per-note sharing or access lists (it is all or nothing per project); sharing with people outside
the team; live co-editing; a history browser beyond Git; encrypting other files of the repository;
keys created on the website; giving agents decrypted shared notes automatically.

## Consequences
- ADR 0004 stays valid for private notes; this ADR adds a second, shared kind next to them.
- The team protocol gains a record kind and three messages; the external crypto review
  ([[team-crypto]], "Questions for the external review") must cover the notes formats too.
- Shared notes depend on Git for transport: people see each other's notes after commit, push and
  pull, not live. That is the price of the owner's "in the project folder".
- Phased rollout:
  1. **Phase 1 (MVP):** crypto module with test vectors; layout, formats, solo mode with the
     notes recovery code; Shared group with Off, Unlocked and Locked; main worktree rule; automatic
     "keep both" conflict resolution; measurement of `-merge` stages and of the `.gitattributes`
     behaviour on Windows, macOS and Linux.
  2. **Phase 2 (teams):** `notes-keys` record and relay messages, re-seal on rotation with a new
     key after removals, link prompt for unlinked clones, solo to team switch.
  3. **Phase 3:** in-app three-way merge, "Commit notes", moving notes between vaults, signed
     authorship if the owner wants it.

## Owner decisions (2026-11-10)
1. Folder name: `.hangar/notes/` (chosen by the owner over `.note/`).
2. Only owners and admins turn on shared notes in a team project, matching who rotates keys.
3. New members read all earlier shared notes, like the repository's history.
4. The notes recovery code is mandatory in solo mode (chosen by the owner): setup does not finish
   until the code is confirmed.
5. Hangar never commits note changes by itself; phase 3 adds a "Commit notes" button.

## Phase 1 as built (2026-11-10)
- Crypto: `crates/hangar-team-crypto/src/notes.rs` (`KeyRing`, `seal_note`/`open_note`,
  `NotesRecoveryCode` with version byte `0xA8`, so every notes code starts with `N`,
  `seal_code_slot`/`open_code_slot`), test vectors recomputed by `test_vectors.py`. The code slot
  is `base64url(0x01 || nonce || ciphertext)` with associated data
  `field("hangar-notes/v1/code-slot") || field(0x01) || field(vault)`, the key ring inside is
  `0x01 || count || (key id || key)*`. The vault id is bound as its 16 raw bytes, the note id as
  its UUID text, the key id as 8 raw bytes.
- App: `src-tauri/src/project_notes.rs`; contract in [[notes]], "Shared notes". The key ring lives
  in the keychain service `hangar-notes` (account `notes/<vault id>`), one service for release and
  debug builds: a key ring is the person's data, not a device identity, and a dev build must open
  the same notes. Team vaults show as locked until phase 2.
- Setup is two calls (`shared_notes_prepare`, then `shared_notes_enable` after "I saved the
  code"); the code exists only in memory between them, at most 15 minutes, and goes when the
  dialog closes (`shared_notes_cancel`). A failure before a file is written (for example the
  keychain refusing) keeps the setup, so a retry seals the code the person already saved.
- The code is saved only outside any Git work tree (the save dialog starts in Documents; besides
  git's answer, any `.git` entry in the folder or above refuses the place, since git reports no
  repository it does not trust); Copy
  marks it as secret for clipboard histories and clears the clipboard after a minute if it still
  holds the code.
- `.hangar` and `.hangar/notes` must be real folders inside the repository: a link or junction is
  refused before anything is read or written. The main worktree counts only if its `.git` exists
  and it shares the folder's git common directory.
- The keychain entry is never overwritten when it cannot be read or names a newer format
  (`v2` and up), and a keychain read error stops the write.
- "Move to shared notes" and "Move to this computer" (section 6) are in phase 1 as well; they
  create the note in the target and move the source to its trash.
- A `vault.json` conflict is resolved only when both sides are vaults of this format and this
  computer holds both key rings: then the vault whose id sorts first stays and the other vault's
  notes are re-encrypted into it. A side of a newer format, a damaged side, an unreadable stage or
  a missing key ring leaves the conflict to the person, so a foreign vault never wins by its id.
- Conflict stages are read as the XY code promises them (`UU`/`AA`: 2 and 3, `UD`/`AU`: 2,
  `DU`/`UA`: 3); a stage that cannot be read whole leaves the conflict. `DD` (both sides deleted
  or renamed the note away) is cleared with `git rm --cached`.
- A scan and a search read at most 256 MiB each, check each file's header before reading it, and
  the scan remembers files that did not open until the file, the vault id or the ring's key ids
  change.
- A conflicted note whose either side does not open here is left to Git (nothing would be lost,
  but nothing can be copied either).

## Evidence
- [[note-gitattributes-measurement]]: `.gitattributes` behaviour and conflict stages on Windows.
- Existing building blocks read in the code on 2026-11-10: `crates/hangar-team-crypto/src/record.rs`
  (`EpochCipher`, `RecordKind`), `crates/hangar-team-crypto/src/epoch.rs` (epoch secrets, history
  link framing), `crates/hangar-team-crypto/src/recovery.rs` (code format),
  `src-tauri/src/team.rs` (`KEPT_EPOCHS`, `StoredKeys`, `LinkedRepo`, keychain service),
  `src-tauri/src/secrets.rs` (`MAX_VALUE_UNITS`), `src-tauri/src/notes.rs` and `src/notesScope.ts`.

> [!warning] Phase 2 blocker
> The Windows credential store holds at most 1280 UTF-16 units per entry
> (`secrets::MAX_VALUE_UNITS`); a key ring entry takes 2 + 82 units per key, so about 15 keys fit.
> `remember` refuses a larger ring (`TooManyKeys`). Solo mode has one key; team mode, which adds a
> key per removal, needs older keys fetched from the `notes-keys` record or a second entry.

> [!question] Unverified
> Not yet measured: the `.gitattributes` behaviour on macOS and Linux (Windows is measured), and
> that `git commit -- <paths>` commits only those paths (phase 3). The relay message sizes are
> estimates, not measured.
