# Contributing to Hangar

Thanks for your interest. Bug reports, ideas and pull requests are welcome.

## Before you start

- For larger changes, open an issue first so we can agree on the approach.
- The rules below apply to every change; design decisions are recorded in [`docs/wiki`](docs/wiki/index.md).
  Please write issues and pull requests in English.

## Development

```sh
pnpm install
pnpm tauri dev
```

The whole check suite must pass before a pull request is merged:

```sh
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path crates/hangar-team-protocol/Cargo.toml --check
cargo clippy --manifest-path crates/hangar-team-protocol/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/hangar-team-protocol/Cargo.toml
cargo fmt --manifest-path crates/hangar-team-crypto/Cargo.toml --check
cargo clippy --manifest-path crates/hangar-team-crypto/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/hangar-team-crypto/Cargo.toml
python3 crates/hangar-team-protocol/test_vectors.py  # Windows: py -3
python3 crates/hangar-team-crypto/test_vectors.py
pnpm format:check && pnpm lint && pnpm test && pnpm build
```

`pnpm tauri build` builds an installer. Custom programs are defined under **Settings > Terminals &
agents** (stored in `profiles.toml`; **Help > About Hangar** shows the path), API keys go to the
system keychain.

## Guidelines

- **Commits:** [Conventional Commits](https://www.conventionalcommits.org) in English
  (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`), one logical change per commit.
- **Tests:** new behavior comes with a test. A bug gets a failing test first, then the fix. Tests are
  never weakened to make them pass.
- **Rust:** no `unwrap`/`expect` outside tests unless an invariant is documented; `thiserror` in
  modules; `unsafe` only with a `// SAFETY:` comment.
- **TypeScript:** `strict`, no `any`, no unchecked casts.
- **Processes:** always start programs as an argv array, never as a shell string.
- **Shortcuts:** never bind `Ctrl+<letter>` alone; those keys belong to shells and agent CLIs.
  Shortcuts live in `src/shortcuts.ts` and have tests.
- **Dependencies:** add one only with a justification covering maintenance status, license
  (MIT, Apache-2.0 or BSD) and size.
- **UI text:** add every new string to `src/i18n.ts` in all supported languages.

## License

By contributing you agree that your contribution is licensed under the [MIT License](LICENSE).
