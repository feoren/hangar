# Linux bundling: reading of docs and source (2026-11-18)

Retrieved on 2026-11-18. Quotes are verbatim excerpts of the pages; nothing was run.

## Tauri updater docs

URL: https://v2.tauri.app/plugin/updater/

- "On Linux, Tauri will create the normal AppImage inside the `target/release/bundle/appimage/`
  folder." The page names no other Linux format for updates (no .deb).
- "Each platform key is in the `OS-ARCH` format, where `OS` is one of `linux`, `darwin` or
  `windows`." Example keys include `linux-x86_64`.
- `createUpdaterArtifacts`: "Setting this to `true` tells Tauri's app bundler to create updater
  artifacts." The page says the private key must be in the environment for that and that signing
  "cannot be disabled". It does not say what a build without the key does.

## Tauri AppImage docs

URL: https://v2.tauri.app/distribute/appimage/

- "Core libraries such as glibc frequently break compatibility with older systems."
- "you must build your Tauri application using the oldest base system you intend to support"
- "linuxdeploy, the AppImage tooling Tauri uses, currently does not support cross-compiling ARM
  AppImages."
- "GUI apps on macOS and Linux do not inherit the `$PATH` from your shell dotfiles." The page
  points to the fix-path-env-rs crate.
- `bundle.linux.appimage` keys: `files` (destinations "must currently begin with `/usr/`") and
  `bundleMediaFramework`.

## tauri-bundler, `Settings::package_types`

URL: https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/settings.rs

- Platform lists: `TargetPlatform::Linux => vec![PackageType::Deb, PackageType::Rpm,
  PackageType::AppImage]`, `TargetPlatform::Windows => vec![PackageType::WindowsMsi,
  PackageType::Nsis]`, `TargetPlatform::MacOS => vec![PackageType::MacOsBundle,
  PackageType::Dmg]`.
- Configured targets are kept only when the platform list contains them
  (`if platform_types ... .any(|t| t == package_type)`); without configured targets the platform
  list is used.

## tauri-utils 2.10.2, `DebConfig`

Local crate source (`tauri-utils-2.10.2/src/config.rs`): `depends`, `recommends`, `provides`,
`conflicts`, `replaces`, `files`, `section`, `priority`, `changelog`, `desktop_template`,
`pre_install_script` and more.

## linuxdeploy-plugin-gtk, AppRun hook

URL: https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh

The hook the plugin writes into the AppImage exports, unconditionally:

- `APPDIR` (fallback to the folder of AppRun for an extracted image)
- `GTK_DATA_PREFIX="$APPDIR"`
- `GTK_THEME="$APPIMAGE_GTK_THEME"` ("Custom themes are broken")
- `GDK_BACKEND=x11` ("Crash with Wayland backend on Wayland")
- `XDG_DATA_DIRS="$APPDIR/usr/share:/usr/share:$XDG_DATA_DIRS"`
- `GSETTINGS_SCHEMA_DIR`, `GI_TYPELIB_PATH`, `GTK_EXE_PREFIX`, `GTK_PATH`, `GTK_IM_MODULE_FILE`,
  `GDK_PIXBUF_MODULE_FILE`, all below `$APPDIR`

It exports no `GIO_*` and no `LD_LIBRARY_PATH`.
