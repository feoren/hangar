# Linux AppImage: first CI build and first run in WSL (2026-11-18)

Own measurement.

## CI build (job linux-bundle, ubuntu-22.04, commit 39478d3)
- Bundles: `Hangar_0.11.0_amd64.AppImage` (94,431,736 bytes), `Hangar_0.11.0_amd64.deb`
  (17,205,770 bytes).
- Highest glibc symbol version over `usr/bin/*` of the AppImage: `GLIBC_2.34`.
- `.deb` Depends: `libasound2 | libasound2t64, libayatana-appindicator3-1, libwebkit2gtk-4.1-0,
  libgtk-3-0`.
- `hangar-send` is in both bundles. tauri-bundler 2.12.4 (`bundle/linux/debian.rs`,
  `create_tar_from_dir`) writes the .deb tar paths without a leading `./`, so `dpkg-deb -c` lists
  `usr/bin/hangar-send`; a check for `./usr/bin/hangar-send` reported it missing.

## Run in WSL 2 (Windows 11, WSLg, Ubuntu 26.04.1, glibc 2.43)
- Installed: libwebkit2gtk-4.1-0, libgtk-3-0t64, libayatana-appindicator3-1. Not installed:
  libfuse2/libfuse2t64, so the AppImage ran with `APPIMAGE_EXTRACT_AND_RUN=1`.
- Fresh `HOME` under `/tmp`; the app stayed up for 40 s and 90 s until stopped (no crash).
- Window rendered through WSLg (`DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-0`), title "Hangar".
- The sidebar listed Login shell (bash 5.3.9), Codex CLI 0.170.0, OpenCode and Pi 1.5.2 from the
  WSL `PATH`; CPU and RAM showed, no GPU entry.
- `hangar-send` was copied to `~/.local/share/com.feoren.hangar/bin/`.
- stderr: libEGL/MESA warnings about `/dev/dri/renderD128: Permission denied` (no GPU access in
  this WSL; software rendering), "account sign-in not read from the keychain: The keychain is not
  available on this system" (no Secret Service), an atk-bridge warning.
- The header showed `0.11.0-85`: the CI checkout is shallow, so the commit count is not the
  repository's; release builds take the build number from the version files.

Not covered: a real GNOME or KDE desktop, X11 session, Secret Service, tray, notifications,
dictation audio, self-update of the AppImage.
