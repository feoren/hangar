# Claude Code prompt suggestion: binary reading

- Date: 2026-11-15
- Platform: Windows 11 x64
- Program: Claude Code 2.1.325 (`claude --version` prints `2.1.325 (Claude Code)`), native binary
- Method: `grep -a -o -E ".{0,250}(uggestion|ghost|Ghost)[^,;]{0,40}(dimColor|dim\b|inverse).{0,250}"` over the binary

## Output (excerpt, verbatim)

```
ql=p(()=>u({subtype:T("ui_prompt_suggest"),surface:ae(),client_id:Q(),text:o().max(ds).describe("@internal The suggestion to show dim in the empty composer for Tab to take, as the `prompt.suggest` hooks left it.")})
```

Further strings found: `promptSuggestionEnabled` ("When false, prompt suggestions are disabled. When absent or
true, prompt suggestions are enabled."), `prompt_suggestion` ("Predicted next user prompt, emitted after each
turn when promptSuggestions is enabled.").

## Not measured

The exact SGR attributes on screen (SGR 2 faint versus a grey foreground, whether the first character sits under
a reverse-video cursor) were not recorded live in Hangar. That Right arrow takes the suggestion is the user's
report, not read from the code.
