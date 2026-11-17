# DepotDownloaderMod

The download backend, vendored as a prebuilt release.

WallForge does not download wallpapers itself. It runs this tool, which
speaks the Steam content protocol, and reads its progress. The files here
are the upstream build, unchanged, and they are compiled into `wallforge.exe`
by `include_bytes!` (see `webapp/src-tauri/src/depot_bundle.rs`). The app
writes them out to `%LOCALAPPDATA%\WallForge\depot` on first run, which is
what lets the app ship as one file.

Two things to know before changing anything here:

- **The file names are load bearing.** The .NET host resolves
  `DepotDownloaderMod.dll` from the name of the executable next to it, so the
  exe, the dll, the `.deps.json` and the `.runtimeconfig.json` all have to
  keep the names they have.
- **The licence travels with them.** DepotDownloaderMod is GPL-2.0, and
  `LICENSE` in this folder is the copy that ships with the app. It cannot be
  removed, and the credit in the README cannot be dropped.

To update the tool, replace the files here with a newer upstream release and
keep the names. The build fails loudly if a file is missing, rather than
shipping an app that cannot download.
