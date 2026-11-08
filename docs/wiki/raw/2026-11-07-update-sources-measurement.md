# Measurement: version sources and OpenCode packages

- Date: 2026-11-07
- Platform: Windows 11 Pro 10.0.26300, x64
- Read-only queries, without sign-in, User-Agent `Hangar/0.1.0`.

## Locally installed npm packages

```
> npm ls -g --depth=0
C:\Users\Feo\AppData\Roaming\npm
+-- @earendil-works/pi-coding-agent@0.79.9
+-- @openai/codex@0.156.1
+-- opencode-ai@1.15.7
+-- ... (weitere ohne Bezug)
```

## OpenCode: two packages in the npm registry

```
GET https://registry.npmjs.org/opencode-ai/latest   -> 200, version 1.19.5
GET https://registry.npmjs.org/@opencode%2fcli/latest -> 200, version 2.2.6,
    homepage https://github.com/anomalyco/opencode#readme
GET https://registry.npmjs.org/opencode-ai   dist-tags.latest 1.19.5, time.modified 2026-10-28,
    not marked as deprecated, bin opencode
GET https://registry.npmjs.org/@opencode%2fcli  dist-tags.latest 2.2.6, time.created 2026-09-02,
    time.modified 2026-11-07, repository git+https://github.com/anomalyco/opencode.git,
    bin opencode and opencode2
```

Both packages ship a program `opencode`. The catalog research (raw/2026-11-07-agent-clis-research.md)
names `npm install -g @opencode/cli`, per opencode.ai/docs.

## Help texts

```
> opencode --help   (1.15.7, excerpt)
  opencode upgrade [target]    upgrade opencode to the latest or a specific version
> pi --help   (0.79.9, excerpt)
  pi update [source|self|pi]   Update pi (use --all for pi and extensions)
```

On start, pi 0.79.9 itself reported: "New version 1.5.0 is available. Run pi update"
(raw/2026-11-07-resume-measurement.md).

## Live test of the three sources via the Hangar backend (reqwest 0.13.5, rustls with ring)

```
> cargo test --lib live -- --ignored --nocapture
Npm("@anthropic-ai/claude-code"): Some("2.1.319")
Pypi("aider-chat"): Some("0.86.2")
Github { repo: "PowerShell/PowerShell", tag_prefix: "v" }: Some("7.6.9")
test result: ok. 1 passed
```

Unencoded (`/@anthropic-ai/claude-code/latest`) and encoded (`/@opencode%2fcli/latest`), the registry
answered 200 both times; Hangar uses the encoded form.
