//! Zed extension for `Blastfile`: the grammar's queries, plus the `blastit-lsp`
//! language server.
//!
//! The extension resolves the server itself, so a developer configures nothing.
//! The order is the design's (§8.3a) and nothing else: the
//! `lsp.blastit-lsp.binary.path` setting, then `blastit-lsp` on the shell
//! `PATH`, then `blastit` on it as `blastit lsp`, and last one GitHub Release
//! download for this platform, unpacked once and then used from disk.
//!
//! Zed spawns the returned command — the wasm never executes anything — and the
//! child gets an environment allowlist of two names (`PATH`, and `SYSTEMROOT` on
//! Windows), never the whole shell environment Zed captured: the server reads no
//! environment, so the token a developer exported must not reach it.

use zed_extension_api as zed;

/// The language server's id: the `extension.toml` table key, and the name the
/// `lsp` settings are looked up under.
const LANGUAGE_SERVER_ID: &str = "blastit-lsp";

/// The repository the release assets come from. The browser URL redirects to a
/// CDN, and no other host is named: the download gate is checked before the
/// redirect (§8.3a).
const REPOSITORY: &str = "OffworldNexus/blastit";

/// Real releases only, and they must carry assets.
const RELEASE_OPTIONS: zed::GithubReleaseOptions = zed::GithubReleaseOptions {
    require_assets: true,
    pre_release: false,
};

/// The one directory the download extracts into and the relaunch path starts
/// with, relative to the extension's work directory. It carries **no** version:
/// the cache has to answer before any network call, and `<version>` exists only
/// in GitHub's reply — so a versioned name would put the network in front of a
/// binary that is already on disk. `download_file` and the relaunch path still
/// share this one name, which is what keeps them from diverging.
const EXTRACT_DIR: &str = "blastit-lsp";

/// The only environment names any step hands the server. `SYSTEMROOT` is what
/// Windows resolves its own DLLs with and is absent on Unix, so this is one list
/// and not a `cfg` fork.
const ENV_ALLOWLIST: [&str; 2] = ["PATH", "SYSTEMROOT"];

/// The sentence every failure ends with: what is built, and the way past it.
const FAILURE_HINT: &str = "releases cover Linux x86_64/aarch64, macOS \
     x86_64/aarch64 and Windows x86_64; set `lsp.blastit-lsp.binary.path` to a \
     `blastit-lsp` you built";

/// One release asset, and how it unpacks (§8.3a's table).
struct Asset {
    /// The asset's file name on the release page.
    name: &'static str,
    /// How `download_file` extracts it: `Zip` only on Windows.
    file_type: zed::DownloadedFileType,
    /// The executable inside the archive's one top-level directory.
    executable: &'static str,
}

/// The asset for one `current_platform()` pair, or `None` when no leg is built.
///
/// Five arms and no default: a pair this platform table does not name has no
/// release artifact, and a silent fallback would download another machine's
/// binary.
fn asset_for(os: zed::Os, architecture: zed::Architecture) -> Option<Asset> {
    match (os, architecture) {
        (zed::Os::Linux, zed::Architecture::X8664) => Some(Asset {
            name: "blastit-lsp-x86_64-unknown-linux-gnu.tar.gz",
            file_type: zed::DownloadedFileType::GzipTar,
            executable: "blastit-lsp",
        }),
        (zed::Os::Linux, zed::Architecture::Aarch64) => Some(Asset {
            name: "blastit-lsp-aarch64-unknown-linux-gnu.tar.gz",
            file_type: zed::DownloadedFileType::GzipTar,
            executable: "blastit-lsp",
        }),
        (zed::Os::Mac, zed::Architecture::Aarch64) => Some(Asset {
            name: "blastit-lsp-aarch64-apple-darwin.tar.gz",
            file_type: zed::DownloadedFileType::GzipTar,
            executable: "blastit-lsp",
        }),
        (zed::Os::Mac, zed::Architecture::X8664) => Some(Asset {
            name: "blastit-lsp-x86_64-apple-darwin.tar.gz",
            file_type: zed::DownloadedFileType::GzipTar,
            executable: "blastit-lsp",
        }),
        (zed::Os::Windows, zed::Architecture::X8664) => Some(Asset {
            name: "blastit-lsp-x86_64-pc-windows-msvc.zip",
            file_type: zed::DownloadedFileType::Zip,
            executable: "blastit-lsp.exe",
        }),
        _ => None,
    }
}

/// The executable's path, relative to the extension's work directory:
/// `blastit-lsp/blastit-lsp-<target>/blastit-lsp(.exe)`.
///
/// The archive keeps its own top-level directory (`download_file` extracts
/// verbatim), so the path is the extract directory, that directory, and the
/// executable.
fn executable_path(asset: &Asset) -> String {
    let directory = asset
        .name
        .strip_suffix(".tar.gz")
        .or_else(|| asset.name.strip_suffix(".zip"))
        .unwrap_or(asset.name);
    format!("{EXTRACT_DIR}/{directory}/{}", asset.executable)
}

/// The child's environment: the allowlist, read out of the shell environment Zed
/// captured. The filter is the point — everything else the developer exported is
/// dropped.
fn env_allowlist(worktree: &zed::Worktree) -> zed::EnvVars {
    worktree
        .shell_env()
        .into_iter()
        .filter(|(name, _)| ENV_ALLOWLIST.contains(&name.as_str()))
        .collect()
}

/// Step 1: the `lsp.blastit-lsp.binary.path` setting, if the developer set one.
///
/// Its `arguments` default to `[]`, because the portable server takes none; a
/// `path` pointing at the Linux `blastit` binary needs `["arguments": ["lsp"]]`.
/// The settings' `env` map is deliberately ignored: the child gets the allowlist.
fn from_settings(worktree: &zed::Worktree) -> Option<zed::Command> {
    let settings = zed::settings::LspSettings::for_worktree(LANGUAGE_SERVER_ID, worktree).ok()?;
    let binary = settings.binary?;
    let command = binary.path?;
    let args = binary.arguments.unwrap_or_default();
    Some(zed::Command {
        command,
        args,
        env: env_allowlist(worktree),
    })
}

/// Step 2: a server already on the shell `PATH`, the better name first.
fn from_path(worktree: &zed::Worktree) -> Option<zed::Command> {
    let env = env_allowlist(worktree);
    if let Some(command) = worktree.which("blastit-lsp") {
        return Some(zed::Command {
            command,
            args: Vec::new(),
            env,
        });
    }
    let command = worktree.which("blastit")?;
    Some(zed::Command {
        command,
        args: vec!["lsp".to_string()],
        env,
    })
}

/// Step 3: the server this extension unpacked earlier, then one download.
fn from_release(id: &zed::LanguageServerId, worktree: &zed::Worktree) -> zed::Result<zed::Command> {
    let (os, architecture) = zed::current_platform();
    let Some(asset) = asset_for(os, architecture) else {
        return Err(failed(
            id,
            format!("no `blastit-lsp` build for {os:?} {architecture:?}; {FAILURE_HINT}"),
        ));
    };
    let command = executable_path(&asset);
    // The cache answers before any network call: a machine that already
    // unpacked this target needs no GitHub at all, so an offline or
    // rate-limited editor start still gets a server (§8.3a).
    if std::fs::metadata(&command).is_ok_and(|entry| entry.is_file()) {
        return Ok(zed::Command {
            command,
            args: Vec::new(),
            env: env_allowlist(worktree),
        });
    }
    zed::set_language_server_installation_status(
        id,
        &zed::LanguageServerInstallationStatus::Downloading,
    );
    let release = match zed::latest_github_release(REPOSITORY, RELEASE_OPTIONS) {
        Ok(release) => release,
        Err(error) => {
            return Err(failed(
                id,
                format!("no release from {REPOSITORY}: {error}; {FAILURE_HINT}"),
            ));
        }
    };
    let Some(asset_url) = release
        .assets
        .iter()
        .find(|candidate| candidate.name == asset.name)
    else {
        return Err(failed(
            id,
            format!(
                "release {} carries no {}; {FAILURE_HINT}",
                release.version, asset.name
            ),
        ));
    };
    if let Err(error) = zed::download_file(&asset_url.download_url, EXTRACT_DIR, asset.file_type) {
        return Err(failed(
            id,
            format!("downloading {} failed: {error}; {FAILURE_HINT}", asset.name),
        ));
    }
    // Unconditional: it is a no-op off Unix, so there is no `cfg` and no second
    // code path (§8.3a).
    if let Err(error) = zed::make_file_executable(&command) {
        return Err(failed(
            id,
            format!("{command} was unpacked but is not executable: {error}; {FAILURE_HINT}"),
        ));
    }
    Ok(zed::Command {
        command,
        args: Vec::new(),
        env: env_allowlist(worktree),
    })
}

/// Reports `message` to Zed as the installation failure and returns it, so every
/// `Err` in this extension is one sentence the developer reads in one line.
fn failed(id: &zed::LanguageServerId, message: String) -> String {
    zed::set_language_server_installation_status(
        id,
        &zed::LanguageServerInstallationStatus::Downloading,
    );
    zed::set_language_server_installation_status(
        id,
        &zed::LanguageServerInstallationStatus::Failed(message.clone()),
    );
    message
}

struct Blastit;

impl zed::Extension for Blastit {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        // The whole resolution order, once: an explicit path, then a file
        // already on `$PATH`, then the cache and one download. Each step
        // returns early, so the network is only touched when nothing on this
        // machine can answer.
        if let Some(command) = from_settings(worktree) {
            return Ok(command);
        }
        if let Some(command) = from_path(worktree) {
            return Ok(command);
        }
        from_release(language_server_id, worktree)
    }
}

zed::register_extension!(Blastit);

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pair `asset_for` answers, in the design's table order.
    const SUPPORTED: [(zed::Os, zed::Architecture); 5] = [
        (zed::Os::Linux, zed::Architecture::X8664),
        (zed::Os::Linux, zed::Architecture::Aarch64),
        (zed::Os::Mac, zed::Architecture::Aarch64),
        (zed::Os::Mac, zed::Architecture::X8664),
        (zed::Os::Windows, zed::Architecture::X8664),
    ];

    fn asset(pair: (zed::Os, zed::Architecture)) -> Asset {
        match asset_for(pair.0, pair.1) {
            Some(asset) => asset,
            None => panic!("{:?} must have an asset", pair),
        }
    }

    #[test]
    fn every_supported_pair_has_one_asset() {
        for pair in SUPPORTED {
            assert!(asset_for(pair.0, pair.1).is_some(), "{pair:?}");
        }
    }

    #[test]
    fn an_unbuilt_pair_has_no_asset() {
        // The verdict `MB.35.1` reached: no 32-bit, no musl, no Windows on ARM.
        assert!(asset_for(zed::Os::Linux, zed::Architecture::X86).is_none());
        assert!(asset_for(zed::Os::Windows, zed::Architecture::Aarch64).is_none());
    }

    #[test]
    fn only_windows_unpacks_a_zip_and_only_windows_ships_an_exe() {
        for pair in SUPPORTED {
            let asset = asset(pair);
            let windows = matches!(pair.0, zed::Os::Windows);
            assert_eq!(
                asset.file_type == zed::DownloadedFileType::Zip,
                windows,
                "{:?} file type",
                pair
            );
            assert_eq!(asset.name.ends_with(".zip"), windows, "{:?} name", pair);
            assert_eq!(
                asset.executable.ends_with(".exe"),
                windows,
                "{:?} exe",
                pair
            );
        }
    }

    #[test]
    fn every_asset_name_carries_its_own_target_triple() {
        for pair in SUPPORTED {
            let asset = asset(pair);
            let directory = asset
                .name
                .strip_suffix(".tar.gz")
                .or_else(|| asset.name.strip_suffix(".zip"));
            let Some(directory) = directory else {
                panic!("{} is neither a .tar.gz nor a .zip", asset.name);
            };
            assert!(asset.name.starts_with("blastit-lsp-"), "{}", asset.name);
            assert!(directory.starts_with("blastit-lsp-"), "{directory}");
            assert!(
                matches!(
                    directory,
                    "blastit-lsp-x86_64-unknown-linux-gnu"
                        | "blastit-lsp-aarch64-unknown-linux-gnu"
                        | "blastit-lsp-aarch64-apple-darwin"
                        | "blastit-lsp-x86_64-apple-darwin"
                        | "blastit-lsp-x86_64-pc-windows-msvc"
                ),
                "{directory} is not one of the five released targets"
            );
        }
    }

    #[test]
    fn the_relaunch_path_is_the_archives_own_directory() {
        for pair in SUPPORTED {
            let asset = asset(pair);
            let path = executable_path(&asset);
            let directory = asset
                .name
                .trim_end_matches(".tar.gz")
                .trim_end_matches(".zip");
            assert_eq!(
                path,
                format!("blastit-lsp/{directory}/{}", asset.executable),
                "{pair:?}"
            );
            assert!(
                !path.starts_with('/'),
                "{path} must be relative to the work directory"
            );
        }
    }

    #[test]
    fn the_cache_path_is_version_free_and_shared_with_the_extract_directory() {
        // The offline second start is the reason: a versioned directory name is
        // knowable only from GitHub, so the cache check must not need one.
        let asset = asset(SUPPORTED[0]);
        let path = executable_path(&asset);
        assert_eq!(
            path,
            "blastit-lsp/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp"
        );
        assert_eq!(EXTRACT_DIR, "blastit-lsp");
        let segments: Vec<&str> = path.split('/').collect();
        assert_eq!(segments.len(), 3, "{path} must not carry a version segment");
        assert_eq!(segments[0], EXTRACT_DIR);
    }

    #[test]
    fn the_failure_sentence_names_the_five_targets_and_the_setting() {
        for target in [
            "Linux x86_64/aarch64",
            "macOS x86_64/aarch64",
            "Windows x86_64",
            "lsp.blastit-lsp.binary.path",
        ] {
            assert!(FAILURE_HINT.contains(target), "{target} missing");
        }
    }

    #[test]
    fn the_environment_allowlist_is_exactly_path_and_systemroot() {
        assert_eq!(ENV_ALLOWLIST, ["PATH", "SYSTEMROOT"]);
    }
}
