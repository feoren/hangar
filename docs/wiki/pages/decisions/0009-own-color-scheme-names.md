---
title: "ADR 0009: Own names for the color schemes"
type: decision
status: accepted
superseded_by:
tags: [terminal, file-viewer, color-schemes, licenses, trademarks]
sources: []
created: 2026-11-13
updated: 2026-11-13
---

# ADR 0009: Own names for the color schemes

## Context
Up to 0.6.0 the terminal color schemes (`src/terminalSchemes.ts`) and the file viewer themes
(`src/viewerThemes.ts`) were shown under the names of the projects their palettes come from
(One Dark Pro, GitHub, VS Code, Darcula, Solarized, Catppuccin, Monokai, Dracula, Nord, Gruvbox,
Tokyo Night, Rosé Pine), and the stored keys were those names (`oneDarkPro`, `github`, ...).
Product names in a picker can read as an endorsement or affiliation, and some of them are
trademarks. Besides, the Monokai scheme was drawn from Monokai Pro and its light filter, a
commercial product.

## Options
1. Keep the names, add a notice. Leaves the endorsement reading and the Monokai Pro colors.
2. Rename every scheme with an own name, keep the colors, document the origins. Chosen.
3. Drop the third-party palettes. Loses popular, well-tested palettes for no legal need: almost
   all are under permissive licenses.

## Decision
- Every scheme except Hangar and Dusk gets an own name, the same in all 12 languages (proper
  names): Harbor (GitHub), Meridian (One Dark Pro and One Light), Studio (VS Code Dark+ and
  Light+), Anvil (Darcula), Sundial (Solarized), Velvet (Catppuccin), Ember (classic Monokai),
  Nocturne (Dracula), Fjord (Nord), Ochre (gruvbox), Neon (Tokyo Night), Heather (Rosé Pine). The
  file viewer families use the same names as the matching terminal schemes.
- Keys follow the names. `RENAMED_SCHEMES` (terminal) and `RENAMED_THEMES` (viewer) map every key
  released in 0.6.0 to the new key, so a stored choice carries over. `harbor` and `meridian`, the
  keys before 0.6.0, are current keys again with the palettes they always had.
- Colors stay, except Ember: its dark variant is now derived from the publicly documented classic
  2006 Monokai TextMate values (background `#272822`, foreground `#F8F8F2`, pink `#F92672`, green
  `#A6E22E`, yellow `#E6DB74`, cyan `#66D9EF`, purple `#AE81FF`, orange `#FD971F`, comment
  `#75715E`); the light variant is Hangar's own (same hues deepened on `#f9f8f1`). No Monokai Pro
  value is used.
- The light variant of Nocturne is not named after any Dracula product.
- Code comments describe each palette neutrally as "after <origin> (<license>)".
- `THIRD_PARTY_NOTICES.md` in the repository root lists per origin the source, copyright line,
  license name and full license text, says the colors are adapted and modified, and states that
  the names are trademarks or names of their owners and Hangar is not affiliated with or endorsed
  by them. The README's license section links it. The app has no About or licenses page, so none
  is linked in the UI.
- What's New items of earlier versions that named the origins were rewritten without the names;
  0.7.0 has an item about the rename.

## Consequences
- Users keep their choice across the update; Ember users see a different palette (classic Monokai instead of the former
  colors, and a new light variant).
- New palettes later need an own name, a row in `THIRD_PARTY_NOTICES.md` and a rename entry only
  if a released key changes.

## Evidence
Licenses as read from the repositories' license files on 2026-11-13 (GitHub API `repos/<repo>/license`):

| Origin | Repository | License | Copyright line |
| --- | --- | --- | --- |
| GitHub theme | primer/github-vscode-theme | MIT | Copyright (c) 2020 Primer |
| Dark+ and Light+ | microsoft/vscode | MIT | Copyright (c) 2015 - present Microsoft Corporation |
| One Light | atom/atom | MIT | Copyright (c) 2011-2022 GitHub Inc. |
| One Dark Pro | Binaryify/OneDark-Pro | MIT | Copyright (c) 2013-2022 Binaryify |
| Darcula | JetBrains/intellij-community | Apache-2.0 | Copyright (C) JetBrains s.r.o. (NOTICE.txt) |
| Solarized | altercation/solarized | MIT | Copyright (c) 2011 Ethan Schoonover |
| Catppuccin | catppuccin/catppuccin | MIT | Copyright (c) 2021 Catppuccin |
| Dracula | dracula/dracula-theme | MIT | Copyright (c) 2023 Dracula Theme |
| Nord | nordtheme/nord | MIT | Copyright (c) 2016-present Sven Greb |
| gruvbox | morhetz/gruvbox | MIT/X11 (README only, no license file) | author Pavel Pertsev (morhetz) |
| Tokyo Night | folke/tokyonight.nvim | Apache-2.0 | no holder named in the license file |
| Rosé Pine | rose-pine/rose-pine-theme | MIT | Copyright (c) 2023 Rosé Pine |

- Monokai Pro's license (<https://monokai.pro/license>, read 2026-11-13): "Monokai Pro may not be
  sub-licensed, resold, or redistributed", and no rights to "any compilation, derivative or
  collective work containing Monokai Pro" without written consent. Hence its colors are no
  longer used.
- The classic Monokai (Wimer Hazenberg, 2006) has no license file of its own; Ember uses only the
  widely published color values listed above, not a theme file.

> [!question] Unverified
> No trademark registry search was done for the new names (Harbor, Meridian, Studio, Anvil,
> Sundial, Velvet, Ember, Nocturne, Fjord, Ochre, Neon, Heather) in the software class. They are
> common words used as scheme names inside the app only; a registry search is still open.
