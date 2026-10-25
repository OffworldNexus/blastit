//! Makes the binary able to find `libpython` with an empty environment.
//!
//! Every blastit process needs the dynamic loader to find `libpython`: one
//! binary plays every role, and the view role embeds CPython. dbproxy is
//! `execve`'d with an **empty** environment (§4), so the loader has no
//! `LD_LIBRARY_PATH` to fall back on there. When the interpreter is a bare
//! prefix outside the loader's default search path — a CI tool cache, a
//! `pyenv` build — dbproxy dies before it can report ready, and init fails the
//! whole boot.
//!
//! The fix belongs in the binary: an `rpath` pointing at the interpreter's
//! library directory. It is emitted only when `PYO3_PYTHON` names an
//! interpreter that actually ships a shared `libpython` under `<prefix>/lib` or
//! `<prefix>/lib64`, which is what a non-system distribution looks like, and a
//! path that does not exist at runtime is harmless: the loader then falls back
//! to its own search path.
//!
//! `pyo3-build-config`'s `add_libpython_rpath_link_args()` does the same thing
//! more thoroughly, but taking it as a build dependency would touch
//! `Cargo.lock`, which every other crate in flight also edits.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");
    let Some(python) = env::var_os("PYO3_PYTHON") else {
        return;
    };
    let interpreter = PathBuf::from(python);
    // <prefix>/bin/python → <prefix>.
    let Some(prefix) = interpreter.parent().and_then(Path::parent) else {
        return;
    };
    for candidate in ["lib", "lib64"] {
        let dir = prefix.join(candidate);
        if holds_libpython(&dir) {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
        }
    }
}

/// Whether `dir` holds a shared `libpython`.
fn holds_libpython(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.starts_with("libpython") && name.contains(".so")
    })
}
