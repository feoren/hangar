# git behavior behind the timeline's jump back

Date: 2026-11-15. Windows 11 Pro 10.0.26300, git 2.56.0.windows.4, rustc 1.100.0, Git Bash,
folders under `tests/.tmp/exp-timeline/` (removed afterwards) and the Rust tests of
`src-tauri/src/git_snapshots.rs`.

## 1. `checkout-index --prefix` with an absolute path

Setup: repository with `core.autocrlf=true`, `.gitattributes` `* text=auto`, files `a.txt`
(`a\nb\n`) and `d/e/f.txt` (`x\n`), committed. Then, with a temporary index:

```
printf 'a.txt\0d/e/f.txt\0' | GIT_INDEX_FILE=<tmp>/idx sh -c \
  'git read-tree HEAD && git checkout-index --force -z --stdin --prefix="E:/.../exp-timeline/stage/"'
rc=0
stage/a.txt:     a \r \n b \r \n
stage/d/e/f.txt: x \r \n
```

A drive letter path as prefix works, the folders below it are created, and line endings are
converted as for the work tree. With `d/.gitattributes` `*.txt -text` added to the tree:

```
stage/d/e/f.txt: x \n
stage/a.txt:     a \r \n b \r \n
```

Attributes are looked up by the entry's path, not by the prefixed path. The work tree is not
touched. Test `restore_writes_line_endings_as_git_does` (`*.txt text eol=crlf`) passes.

## 2. assume-unchanged entries and `add --all`

```
git update-index --assume-unchanged a.txt; printf 'changed\n' > a.txt
cp .git/index idx4; GIT_INDEX_FILE=idx4 git add --all; GIT_INDEX_FILE=idx4 git ls-files -s a.txt
100644 422c2b7ab3b3c668038da977e4e93a5fc623169c 0	a.txt      # the old blob: change lost
cp .git/index idx3; printf 'a.txt\0' | GIT_INDEX_FILE=idx3 git update-index --no-assume-unchanged -z --stdin
GIT_INDEX_FILE=idx3 git ls-files -v a.txt   -> H a.txt
GIT_INDEX_FILE=idx3 git add --all; GIT_INDEX_FILE=idx3 git ls-files -s a.txt
100644 5ea2ed416fbd4a4cbe227b75fe255dd7fa6bd4d6 0	a.txt      # = git hash-object a.txt
git ls-files -v a.txt   -> h a.txt                                # the user's index keeps its bit
```

`ls-files -v` tags an assume-unchanged entry with a lower case letter; `update-index
--no-assume-unchanged -z --stdin` applies the option to every path read. Test
`assume_unchanged_entries_are_saved_with_their_content` passes.

## 3. `gc` in the side repository with an alternate

Test `clean_up_runs_gc_and_keeps_the_folder_below_its_cap`: after four snapshots and
`git gc --quiet --prune=1.hour.ago` (run with `GIT_ALTERNATE_OBJECT_DIRECTORIES=<user objects>`),
`count-objects -v` in the side repository shows `count: 0` (everything packed), and
`cat-file -e <blob of an unchanged file>` in the side repository without the alternate fails:
the pack holds only the snapshots' own objects, none borrowed from the user's repository.

## 4. Cross-process lock (`std::fs::File::lock`, `try_lock`, `lock_shared`)

Stable since Rust 1.89 (the crate's `rust-version`). Test `clean_up_runs_only_without_another_instance`:
a second handle of `.lock` in the same process gets `TryLockError::WouldBlock` while the first
holds it shared, so locks conflict per handle (LockFileEx on Windows).

## 5. NTFS short names

```
fsutil 8dot3name query E:
Der Volumestatus ist "1" (Erstellung von 8dot3-Namen ist DISABLED).
```

No short names exist on the test volume, so the `GIT~1` branch of
`install_checks_every_folder_right_before_writing` did not run here. The path into `.git` by its
long name is refused through the canonical path.
