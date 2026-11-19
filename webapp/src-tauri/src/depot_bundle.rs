//! The download tool, embedded in the app.
//!
//! DepotDownloaderMod and its .NET libraries are compiled into the binary and
//! written out on first run, so WallForge ships as a single executable.
//!
//! The files come from `vendor/DepotDownloaderMod`, which holds the upstream
//! build of the tool. The names must stay as they are: the .NET host looks
//! for `DepotDownloaderMod.dll` next to its own executable.

use std::fs;
use std::path::PathBuf;

pub struct DepotFile {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

const DEPOT_FILES: &[DepotFile] = &[
    DepotFile {
        name: "DepotDownloaderMod.exe",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/DepotDownloaderMod.exe"),
    },
    DepotFile {
        name: "DepotDownloaderMod.dll",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/DepotDownloaderMod.dll"),
    },
    DepotFile {
        name: "DepotDownloaderMod.runtimeconfig.json",
        bytes: include_bytes!(
            "../../../vendor/DepotDownloaderMod/DepotDownloaderMod.runtimeconfig.json"
        ),
    },
    DepotFile {
        name: "DepotDownloaderMod.deps.json",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/DepotDownloaderMod.deps.json"),
    },
    DepotFile {
        name: "protobuf-net.dll",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/protobuf-net.dll"),
    },
    DepotFile {
        name: "protobuf-net.Core.dll",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/protobuf-net.Core.dll"),
    },
    DepotFile {
        name: "QRCoder.dll",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/QRCoder.dll"),
    },
    DepotFile {
        name: "SteamKit2.dll",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/SteamKit2.dll"),
    },
    DepotFile {
        name: "System.IO.Hashing.dll",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/System.IO.Hashing.dll"),
    },
    DepotFile {
        name: "ZstdSharp.dll",
        bytes: include_bytes!("../../../vendor/DepotDownloaderMod/ZstdSharp.dll"),
    },
];

/// Where the tool is unpacked. Everything the app owns lives under
/// `%APPDATA%\WallForge`, so the tool sits beside the settings file.
pub fn depot_dir() -> PathBuf {
    crate::settings::data_dir().join("depot")
}

/// Write the tool out if it is missing or the wrong size, and return the
/// folder it lives in.
pub fn ensure_extracted() -> Result<PathBuf, String> {
    let dir = depot_dir();
    fs::create_dir_all(&dir).map_err(|err| format!("Could not create {}: {err}", dir.display()))?;
    for file in DEPOT_FILES {
        let target = dir.join(file.name);
        let current = fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0);
        if current == file.bytes.len() as u64 {
            continue;
        }
        fs::write(&target, file.bytes)
            .map_err(|err| format!("Could not write {}: {err}", target.display()))?;
    }
    Ok(dir)
}

/// The path of the download tool.
pub fn depot_exe_path() -> PathBuf {
    depot_dir().join("DepotDownloaderMod.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_is_embedded() {
        assert!(!DEPOT_FILES.is_empty());
        for file in DEPOT_FILES {
            assert!(!file.bytes.is_empty(), "{} is empty", file.name);
        }
    }

    #[test]
    fn the_tool_is_written_out() {
        let dir = ensure_extracted().expect("the tool must unpack");
        assert!(dir.join("DepotDownloaderMod.exe").is_file());
        assert!(dir.join("DepotDownloaderMod.runtimeconfig.json").is_file());
    }
}

