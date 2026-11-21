//! Records the CPython minor this shim is built for.
//!
//! `blastit_py_minor()` must report the minor of the interpreter `make py-sapi`
//! built against (`PYO3_PYTHON`), never a literal in the source: the same crate
//! produces `libblastit_py313-*.so` and `libblastit_py314-*.so`, and a literal
//! would make one of them lie about the vtable it exports. The value is read
//! from the interpreter itself, which is the same one pyo3's own build script
//! validated, and written into the crate as an environment variable.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");

    let interpreter = std::env::var_os("PYO3_PYTHON").unwrap_or_else(|| "python3".into());
    let output = Command::new(&interpreter)
        .args([
            "-c",
            "import sys; print(sys.version_info[0] * 100 + sys.version_info[1])",
        ])
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "blastit-py-shim: cannot run PYO3_PYTHON ({}): {err}; set PYO3_PYTHON to the \
                 CPython this shim is built for",
                interpreter.to_string_lossy()
            )
        });
    if !output.status.success() {
        panic!(
            "blastit-py-shim: PYO3_PYTHON ({}) did not answer `sys.version_info`; set PYO3_PYTHON \
             to the CPython this shim is built for",
            interpreter.to_string_lossy()
        );
    }
    let minor = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if minor.parse::<u32>().is_err() {
        panic!(
            "blastit-py-shim: PYO3_PYTHON ({}) answered {minor:?}, not a minor encoding",
            interpreter.to_string_lossy()
        );
    }
    println!("cargo:rustc-env=BLASTIT_PY_MINOR={minor}");

    if std::env::var_os("CARGO_FEATURE_EXTENSION_MODULE").is_some() {
        // An `extension-module` link target has no `libpython` to resolve `Py*`
        // against (that is the point of the feature), so the `cdylib` is linked
        // with those references left unresolved on purpose — `nm -uD` must show
        // them. A *test* build in this configuration compiles the
        // interpreter-backed half out (see `src/lib.rs`), so this only ever
        // applies to the artifact.
        println!("cargo:rustc-link-arg=-Wl,--unresolved-symbols=ignore-all");
    }
}
