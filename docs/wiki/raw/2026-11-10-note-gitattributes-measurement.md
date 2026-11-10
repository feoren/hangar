# Measurement: Git with `* -text -diff -merge` for encrypted note files (2026-11-10)

Platform: Windows 11 Pro, build 26300, local NTFS drive. git 2.56.0.windows.4. Run through Git Bash
(`sh`) from `tests/.tmp/`; each case in a fresh repository with `core.autocrlf` set to `true`,
`false` and `input`. macOS and Linux were not measured.

`.hangar/notes/.gitattributes` holds `* -text -diff -merge`. The note file is
`hangar-note 1 0011223344556677\nAAAA\r\nBBBB\n` (one CR on purpose), `outside.txt` a plain LF text
file without attributes for comparison. Cases: commit and fresh checkout; a merge where both
branches changed the note; resolving by writing the file and `git add`; a modify/delete merge; a
rebase with the same conflict; an add/add merge of `vault.json` (two branches turned notes on).

The line "blob bytes unchanged: 2 (1 = CR kept)" counts `od -c` output lines that contain `\r` and
is not a byte count; the later line "CR bytes kept in base blob" counts the CR bytes of the
committed blob with `tr -cd '\r' | wc -c` and is the valid check.

## Script

```sh
#!/bin/sh
# Measures Git's behaviour with ".hangar/notes/.gitattributes" = "* -text -diff -merge".
set -u
ROOT="$(cd "$(dirname "$0")" && pwd)/git-attr-measure"
rm -rf "$ROOT"; mkdir -p "$ROOT"; cd "$ROOT"
G="git -c user.name=T -c user.email=t@example.invalid -c commit.gpgsign=false"
for AUTOCRLF in true false input; do
  R="$ROOT/repo-$AUTOCRLF"; mkdir -p "$R"; cd "$R"
  git init -q --initial-branch=main
  git config core.autocrlf $AUTOCRLF
  mkdir -p .hangar/notes
  printf '* -text -diff -merge\n' > .hangar/notes/.gitattributes
  # A file with LF and a CRLF in it, plus a plain text file outside for comparison.
  printf 'hangar-note 1 0011223344556677\nAAAA\r\nBBBB\n' > .hangar/notes/a.note
  printf 'line1\nline2\n' > outside.txt
  $G add -A >/dev/null 2>&1; $G commit -qm base
  rm .hangar/notes/a.note outside.txt; git checkout -q -- .
  echo "autocrlf=$AUTOCRLF note bytes unchanged: $(printf 'hangar-note 1 0011223344556677\nAAAA\r\nBBBB\n' | cmp -s - .hangar/notes/a.note && echo yes || echo NO)"
  echo "autocrlf=$AUTOCRLF blob bytes unchanged: $(git cat-file blob HEAD:.hangar/notes/a.note | od -c | grep -c '\r')  (1 = CR kept)"
  echo "autocrlf=$AUTOCRLF outside.txt has CRLF in tree: $(od -c outside.txt | grep -c '\r')"
  echo "autocrlf=$AUTOCRLF check-attr: $(git check-attr text diff merge -- .hangar/notes/a.note | tr '\n' ';')"
  # Conflict on both sides
  $G checkout -qb other
  printf 'hangar-note 1 0011223344556677\nTHEIRS\n' > .hangar/notes/a.note
  $G commit -qam theirs
  $G checkout -q main
  printf 'hangar-note 1 0011223344556677\nOURS\n' > .hangar/notes/a.note
  $G commit -qam ours
  $G merge -q other >/dev/null 2>&1; echo "autocrlf=$AUTOCRLF merge exit: $?"
  echo "autocrlf=$AUTOCRLF status: $(git status --porcelain=v1 -z -- .hangar/notes | tr '\0' ';')"
  echo "autocrlf=$AUTOCRLF ls-files -u: $(git ls-files -u -- .hangar/notes | awk '{print $3}' | tr '\n' ',')"
  echo "autocrlf=$AUTOCRLF worktree file: $(sed -n 2p .hangar/notes/a.note)"
  echo "autocrlf=$AUTOCRLF :1: $(git cat-file blob :1:.hangar/notes/a.note | sed -n 2p) :2: $(git cat-file blob :2:.hangar/notes/a.note | sed -n 2p) :3: $(git cat-file blob :3:.hangar/notes/a.note | sed -n 2p)"
  echo "autocrlf=$AUTOCRLF conflict markers in file: $(grep -c '<<<<<<<' .hangar/notes/a.note)"
  echo "autocrlf=$AUTOCRLF diff output: $(git diff HEAD~1 HEAD -- .hangar/notes/a.note | grep -c 'Binary files')"
  # Resolve: write, add, then status
  printf 'hangar-note 1 0011223344556677\nRESOLVED\n' > .hangar/notes/a.note
  $G add -- .hangar/notes/a.note
  echo "autocrlf=$AUTOCRLF after add: $(git status --porcelain=v1 -z -- .hangar/notes | tr '\0' ';')"
  $G merge --abort 2>/dev/null; $G reset -q --hard
  # Modify/delete conflict
  $G checkout -qb del other~0 2>/dev/null || $G checkout -qb del
  git rm -q .hangar/notes/a.note; $G commit -qm del
  $G checkout -q main
  printf 'hangar-note 1 0011223344556677\nMODIFIED\n' > .hangar/notes/a.note; $G commit -qam mod
  $G merge -q del >/dev/null 2>&1
  echo "autocrlf=$AUTOCRLF modify/delete status: $(git status --porcelain=v1 -z -- .hangar/notes | tr '\0' ';') stages: $(git ls-files -u -- .hangar/notes | awk '{print $3}' | tr '\n' ',')"
  $G merge --abort 2>/dev/null; $G reset -q --hard
  # Rebase conflict (stage 2 = upstream, stage 3 = the commit being replayed)
  $G checkout -q other; $G rebase -q main >/dev/null 2>&1; echo "autocrlf=$AUTOCRLF rebase exit: $?"
  echo "autocrlf=$AUTOCRLF rebase status: $(git status --porcelain=v1 -z -- .hangar/notes | tr '\0' ';') worktree: $(sed -n 2p .hangar/notes/a.note) :2: $(git cat-file blob :2:.hangar/notes/a.note 2>/dev/null | sed -n 2p) :3: $(git cat-file blob :3:.hangar/notes/a.note 2>/dev/null| sed -n 2p)"
  $G rebase --abort 2>/dev/null
  # Pull with --rebase in a clone
  echo "autocrlf=$AUTOCRLF CR bytes kept in base blob: $(git cat-file blob $(git rev-list --max-parents=0 main):.hangar/notes/a.note | tr -cd '\r' | wc -c)"
  cd "$ROOT"
done
# Add/add: two branches turn on notes (same .gitattributes and README, different vault.json).
R="$ROOT/addadd"; mkdir -p "$R"; cd "$R"
git init -q --initial-branch=main; printf 'x\n' > x; $G add x; $G commit -qm root
$G checkout -qb a; mkdir -p .hangar/notes
printf '* -text -diff -merge\n' > .hangar/notes/.gitattributes; printf 'readme\n' > .hangar/notes/README.md
printf '{"vault":"aaaa"}\n' > .hangar/notes/vault.json; $G add -A; $G commit -qm a
$G checkout -q main; mkdir -p .hangar/notes
printf '* -text -diff -merge\n' > .hangar/notes/.gitattributes; printf 'readme\n' > .hangar/notes/README.md
printf '{"vault":"bbbb"}\n' > .hangar/notes/vault.json; $G add -A; $G commit -qm b
$G merge -q a >/dev/null 2>&1
echo "add/add status: $(git status --porcelain=v1 -z -- .hangar/notes | tr '\0' ';') stages: $(git ls-files -u -- .hangar/notes | awk '{print $3 $4}' | tr '\n' ',') worktree: $(cat .hangar/notes/vault.json)"
```

## Output

```
autocrlf=true note bytes unchanged: yes
autocrlf=true blob bytes unchanged: 2  (1 = CR kept)
autocrlf=true outside.txt has CRLF in tree: 1
autocrlf=true check-attr: .hangar/notes/a.note: text: unset;.hangar/notes/a.note: diff: unset;.hangar/notes/a.note: merge: unset;
autocrlf=true merge exit: 1
autocrlf=true status: UU .hangar/notes/a.note;
autocrlf=true ls-files -u: 1,2,3,
autocrlf=true worktree file: OURS
autocrlf=true :1: AAAA :2: OURS :3: THEIRS
autocrlf=true conflict markers in file: 0
autocrlf=true diff output: 1
autocrlf=true after add: M  .hangar/notes/a.note;
autocrlf=true modify/delete status: UD .hangar/notes/a.note; stages: 1,2,
autocrlf=true rebase exit: 1
autocrlf=true rebase status: UU .hangar/notes/a.note; worktree: MODIFIED :2: MODIFIED :3: THEIRS
autocrlf=true CR bytes kept in base blob: 1
autocrlf=false note bytes unchanged: yes
autocrlf=false blob bytes unchanged: 2  (1 = CR kept)
autocrlf=false outside.txt has CRLF in tree: 0
autocrlf=false check-attr: .hangar/notes/a.note: text: unset;.hangar/notes/a.note: diff: unset;.hangar/notes/a.note: merge: unset;
autocrlf=false merge exit: 1
autocrlf=false status: UU .hangar/notes/a.note;
autocrlf=false ls-files -u: 1,2,3,
autocrlf=false worktree file: OURS
autocrlf=false :1: AAAA :2: OURS :3: THEIRS
autocrlf=false conflict markers in file: 0
autocrlf=false diff output: 1
autocrlf=false after add: M  .hangar/notes/a.note;
autocrlf=false modify/delete status: UD .hangar/notes/a.note; stages: 1,2,
autocrlf=false rebase exit: 1
autocrlf=false rebase status: UU .hangar/notes/a.note; worktree: MODIFIED :2: MODIFIED :3: THEIRS
autocrlf=false CR bytes kept in base blob: 1
autocrlf=input note bytes unchanged: yes
autocrlf=input blob bytes unchanged: 2  (1 = CR kept)
autocrlf=input outside.txt has CRLF in tree: 0
autocrlf=input check-attr: .hangar/notes/a.note: text: unset;.hangar/notes/a.note: diff: unset;.hangar/notes/a.note: merge: unset;
autocrlf=input merge exit: 1
autocrlf=input status: UU .hangar/notes/a.note;
autocrlf=input ls-files -u: 1,2,3,
autocrlf=input worktree file: OURS
autocrlf=input :1: AAAA :2: OURS :3: THEIRS
autocrlf=input conflict markers in file: 0
autocrlf=input diff output: 1
autocrlf=input after add: M  .hangar/notes/a.note;
autocrlf=input modify/delete status: UD .hangar/notes/a.note; stages: 1,2,
autocrlf=input rebase exit: 1
autocrlf=input rebase status: UU .hangar/notes/a.note; worktree: MODIFIED :2: MODIFIED :3: THEIRS
autocrlf=input CR bytes kept in base blob: 1
add/add status: AA .hangar/notes/vault.json; stages: 2.hangar/notes/vault.json,3.hangar/notes/vault.json, worktree: {"vault":"bbbb"}
```

## The app's own run

`src-tauri/src/project_notes/tests.rs`, test `merge_conflicts_are_resolved_without_loss` (cargo
test, same machine and git): a real merge of two edits of one shared note leaves `UU`; Hangar reads
`:2:` and `:3:` with `git cat-file blob`, keeps the newer version under the note id, writes the
other one as a new note and runs `git add -- <both paths>`. Afterwards `git status --porcelain=v1`
shows no unmerged entry and the copy as `A `, and `git commit` succeeds. Result: ok.
