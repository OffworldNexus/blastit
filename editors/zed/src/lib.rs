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

    // --- the committed highlight queries and the grammar's fixtures ---------
    //
    // The grammar package's query and Zed's copy are different files **by
    // design**: the grammar one carries the patterns any consumer of
    // `node-types.json` needs, Zed's is the superset a bundled theme styles. The
    // property that must hold is containment, not equality — every `(node) @cap`
    // the grammar copy states must also be in Zed's copy, or the two render
    // differently. And no query or fixture may name a node the grammar stopped
    // declaring: a rename would otherwise silently drop the capture instead of
    // failing here.

    /// `editors/tree-sitter-blastfile`, from this crate's manifest directory.
    const GRAMMAR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tree-sitter-blastfile");

    /// The fifteen capture names the bundled Zed themes style
    /// (`docs/design/blastfile-lsp-zed.md` §10.1): the intersection of One Dark,
    /// Ayu and Gruvbox, plus `variable.special`, which is only ever the first
    /// half of the `@variable.special @variable` fallback pair.
    const CAPTURES: [&str; 15] = [
        "attribute",
        "comment",
        "constant",
        "function",
        "keyword",
        "property",
        "punctuation.bracket",
        "punctuation.delimiter",
        "string",
        "string.escape",
        "string.regex",
        "string.special",
        "type",
        "variable",
        "variable.special",
    ];

    fn read(relative: &str) -> String {
        let path = format!("{GRAMMAR}/{relative}");
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    /// The identifier right after a `(`, when a line opens a node.
    fn node_of(line: &str) -> Option<&str> {
        let rest = line.trim_start();
        let after = rest.strip_prefix('(')?;
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .unwrap_or(after.len());
        (end > 0).then(|| &after[..end])
    }

    /// The `@name` captures a line states, in order.
    fn captures_of(line: &str) -> Vec<&str> {
        let mut names = Vec::new();
        let mut rest = line;
        while let Some(at) = rest.find('@') {
            rest = &rest[at + 1..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                .unwrap_or(rest.len());
            if end > 0 {
                names.push(&rest[..end]);
            }
            rest = &rest[end..];
        }
        names
    }

    /// Every `(node, capture)` pair a highlight query states.
    fn query_pairs(query: &str) -> std::collections::BTreeSet<(String, String)> {
        let mut pairs = std::collections::BTreeSet::new();
        for line in query
            .lines()
            .filter(|line| !line.trim_start().starts_with(';'))
        {
            let capture = captures_of(line).into_iter().next().map(str::to_owned);
            let (Some(node), Some(capture)) = (node_of(line), capture) else {
                continue;
            };
            pairs.insert((node.to_owned(), capture));
        }
        pairs
    }

    /// Every `@name` a highlight query states.
    fn query_captures(query: &str) -> std::collections::BTreeSet<String> {
        query
            .lines()
            .filter(|line| !line.trim_start().starts_with(';'))
            .flat_map(captures_of)
            .map(str::to_owned)
            .collect()
    }

    /// Every node a highlight query names.
    fn query_nodes(query: &str) -> std::collections::BTreeSet<String> {
        query
            .lines()
            .filter(|line| !line.trim_start().starts_with(';'))
            .filter_map(node_of)
            .map(str::to_owned)
            .collect()
    }

    /// The named node types `src/node-types.json` declares.
    ///
    /// Read line by line rather than through a JSON dependency: an entry is
    /// `"type": "<id>",` immediately followed by `"named": true,`, and every
    /// field/child reference repeats that shape.
    fn declared_nodes() -> std::collections::BTreeSet<String> {
        let document = read("src/node-types.json");
        let mut declared = std::collections::BTreeSet::new();
        let mut previous: Option<String> = None;
        for line in document.lines() {
            let trimmed = line.trim();
            let bare = trimmed.trim_end_matches(',');
            if bare == "\"named\": true" {
                if let Some(id) = previous.take() {
                    declared.insert(id);
                }
                continue;
            }
            previous = bare
                .strip_prefix("\"type\": \"")
                .and_then(|rest| rest.strip_suffix('"'))
                .map(str::to_owned);
        }
        declared
    }

    /// Every literal the grammar tokenizes, from `grammar.js`'s `'...'` strings.
    ///
    /// Comments are stripped first: the file's prose carries apostrophes
    /// ("the grammar's"), and a naive quote split would pair the wrong ones.
    fn grammar_literals() -> std::collections::BTreeSet<String> {
        let source = read("grammar.js");
        let mut literals = std::collections::BTreeSet::new();
        for line in source.lines() {
            let code = line.split("//").next().unwrap_or_default();
            let mut pieces = code.split('\'');
            pieces.next();
            for (index, chunk) in pieces.enumerate() {
                // After the leading text, even indices are inside a literal.
                if index % 2 == 0 && !chunk.is_empty() && !chunk.contains(char::is_whitespace) {
                    literals.insert(chunk.to_owned());
                }
            }
        }
        literals
    }

    #[test]
    fn every_grammar_highlight_is_also_in_zeds_copy() {
        let grammar = read("highlights.scm");
        let zed = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/languages/blastfile/highlights.scm"
        ))
        .expect("zed highlights.scm");
        let (theirs, ours) = (query_pairs(&grammar), query_pairs(&zed));
        let missing: Vec<_> = theirs.difference(&ours).collect();
        assert!(
            missing.is_empty(),
            "the grammar query states {missing:?}, which Zed's copy does not: the two render differently"
        );
        let (theirs, ours) = (query_captures(&grammar), query_captures(&zed));
        let missing: Vec<_> = theirs.difference(&ours).collect();
        assert!(
            missing.is_empty(),
            "the grammar query captures {missing:?}, which Zed's copy does not"
        );
        assert!(
            !query_pairs(&zed).is_empty() && !query_pairs(&grammar).is_empty(),
            "an empty query would satisfy a subset check vacuously"
        );
    }

    #[test]
    fn every_query_names_a_declared_node_and_a_styled_capture() {
        let declared = declared_nodes();
        assert!(!declared.is_empty(), "node-types.json declares nothing");
        let styled: std::collections::BTreeSet<&str> = CAPTURES.into_iter().collect();
        let zed = concat!(env!("CARGO_MANIFEST_DIR"), "/languages/blastfile");
        let queries = [
            ("grammar highlights.scm", read("highlights.scm")),
            (
                "zed highlights.scm",
                std::fs::read_to_string(format!("{zed}/highlights.scm"))
                    .expect("zed highlights.scm"),
            ),
            (
                "zed brackets.scm",
                std::fs::read_to_string(format!("{zed}/brackets.scm")).expect("zed brackets.scm"),
            ),
            (
                "zed indents.scm",
                std::fs::read_to_string(format!("{zed}/indents.scm")).expect("zed indents.scm"),
            ),
            (
                "zed outline.scm",
                std::fs::read_to_string(format!("{zed}/outline.scm")).expect("zed outline.scm"),
            ),
        ];
        for (relative, text) in queries {
            for node in query_nodes(&text) {
                assert!(
                    declared.contains(&node),
                    "{relative} names `{node}`, which node-types.json does not declare"
                );
            }
            // `brackets.scm`/`indents.scm`/`outline.scm` carry editor-machinery
            // captures (`@close`, `@item`), not theme ones; only a highlight
            // query may claim a capture a bundled theme must style.
            if !relative.contains("highlights") {
                continue;
            }
            for capture in query_captures(&text) {
                assert!(
                    styled.contains(capture.as_str()),
                    "{relative} captures @{capture}, which no bundled theme styles"
                );
            }
        }
    }

    #[test]
    fn the_committed_corpus_and_fixtures_speak_the_grammars_vocabulary() {
        let declared = declared_nodes();
        let literals = grammar_literals();
        assert!(!literals.is_empty(), "grammar.js declares no literal");

        // `test/corpus/*.txt`: `===`, name, `===`, source, `---`, expected tree.
        // Splitting on the `===` banner and then on `---` reads that shape
        // directly: every odd chunk is a name, every chunk after one is its
        // source and expected tree.
        let mut entries = 0usize;
        let mut vocabulary = literals.clone();
        let mut corpus = std::fs::read_dir(format!("{GRAMMAR}/test/corpus"))
            .expect("test/corpus")
            .map(|entry| entry.expect("corpus entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
            .collect::<Vec<_>>();
        corpus.sort();
        for path in corpus {
            let name = path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path).expect("corpus file");
            let chunks: Vec<&str> = text.split("==================").collect();
            assert!(
                chunks.len() >= 3,
                "{name} states no `===` banner: the corpus format changed"
            );
            for pair in chunks[2..].chunks(2) {
                let Some(block) = pair.first() else { continue };
                let Some((source, expected)) = block.split_once("\n---\n") else {
                    continue;
                };
                entries += 1;
                for line in source.lines() {
                    let word: String = line
                        .trim()
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !word.is_empty() {
                        vocabulary.insert(word);
                    }
                }
                for token in expected.split(|c: char| {
                    !(c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '.')
                }) {
                    let node = token.trim_end_matches(':');
                    if node.is_empty() || node.contains(':') || node.contains('.') {
                        continue;
                    }
                    // The root of every tree is `source_file`; `node-types.json`
                    // lists the nodes, not the start rule.
                    if node == "source_file" {
                        continue;
                    }
                    assert!(
                        declared.contains(node),
                        "{name}: the expected tree names `{node}`, which node-types.json does not declare"
                    );
                }
            }
        }
        assert!(
            entries >= 10,
            "only {entries} corpus entries: fixtures shrank"
        );

        // `test/fixtures/*.Blastfile`: every leading word must be a token the
        // grammar tokenizes or a corpus source uses, so a fixture cannot carry a
        // construct no committed parse covers.
        let mut fixtures = std::fs::read_dir(format!("{GRAMMAR}/test/fixtures"))
            .expect("test/fixtures")
            .map(|entry| entry.expect("fixture entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "Blastfile"))
            .collect::<Vec<_>>();
        fixtures.sort();
        assert_eq!(fixtures.len(), 8, "the fixture set changed");
        for path in fixtures {
            let name = path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path).expect("fixture");
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let word: String = line
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if word.is_empty() {
                    continue;
                }
                assert!(
                    vocabulary.contains(&word),
                    "{name}: `{word}` is neither a grammar literal nor a word a committed corpus parse uses"
                );
            }
        }
    }
}
