//! Zed extension for `Blastfile`: the grammar's queries, plus the `blastit lsp`
//! command.
//!
//! The extension resolves the language server itself, so a developer configures
//! nothing: `blastit` on the shell `PATH` -> `blastit lsp`; otherwise `uv` on the
//! shell `PATH` -> `uv run --frozen --no-sync --quiet blastit lsp`. Zed spawns the
//! returned command (the wasm never executes anything), and the environment is the
//! one Zed captured from the shell: a desktop launch has no login `PATH` of its own.
//!
//! `--frozen --no-sync` matters: a bare `uv run` syncs the project first, which
//! executes the worktree's declared build backend with this environment. Opening
//! somebody else's `Blastfile` must not build their code.

use zed_extension_api as zed;

struct Blastit;

impl zed::Extension for Blastit {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        // The shell's environment, as Zed captured it, so the child sees the same
        // `PATH` (and `UV_*`, `VIRTUAL_ENV`, `PYTHONPATH`) the resolution used.
        let env = worktree.shell_env();
        let (command, args) = match worktree.which("blastit") {
            Some(blastit) => (blastit, vec!["lsp".to_string()]),
            None => match worktree.which("uv") {
                Some(uv) => (
                    uv,
                    ["run", "--frozen", "--no-sync", "--quiet", "blastit", "lsp"]
                        .iter()
                        .map(|arg| arg.to_string())
                        .collect(),
                ),
                None => {
                    return Err("blastit is not installed: run `uv add blastit` \
                                (or `uv sync` once in this project), or \
                                `uv tool install blastit`"
                        .to_string());
                }
            },
        };
        Ok(zed::Command { command, args, env })
    }
}

zed::register_extension!(Blastit);
