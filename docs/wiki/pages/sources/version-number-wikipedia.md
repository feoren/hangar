---
title: "Wikipedia: version number"
type: source
tags: [version]
sources: []
raw:
url: https://de.wikipedia.org/wiki/Versionsnummer
retrieved: 2026-11-07
created: 2026-11-07
updated: 2026-11-07
---

# Wikipedia: version number

Summary of the German Wikipedia article "Versionsnummer".

## Key points
- Common scheme: **major version** (fundamental or incompatible change), **minor version**
  (functional extension), **revision number** (patch level, bug fixes), **build number**
  (progress in single steps, e.g. `5.0.0-3242` = the 3242nd build).
- With version control, a number that uniquely identifies the sources in the version control system
  is often used instead of a build number.
- When the major or minor version goes up, the following parts except the build number are reset to 0.
  Many projects never reset the build number.
- Each part can have several digits: 0.9 is followed by 0.10, not 1.0. Major version 0 allows bigger
  jumps even in the minor version.

## Significance for Hangar
Basis for [[0002-version-scheme]].
