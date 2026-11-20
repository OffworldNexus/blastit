//! The JSON documents a stage-1 caller sends each vtable entry.
//!
//! One struct per boundary, deserialised with `deny_unknown_fields`: a caller
//! that passes a field this shim does not know is refused here rather than
//! silently having it ignored, which is what keeps the two halves of the ABI in
//! step without a version bump for every new boot datum.
//!
//! Every field is `#[serde(default)]`-able, so the smallest useful request is
//! `{}`. Paths travel as byte vectors because a Unix path is bytes, and the
//! shim hands them to `os.fsdecode` exactly as the composition root's
//! `sys.path` restore does.

use serde::Deserialize;

use blastit_view_python::immortal::Setting;

/// An `init_paths` request (design §1.2).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct InitPaths {
    /// The `sys.path` entries stage 0 recorded, inserted at the front in order.
    pub(crate) sys_path: Vec<Vec<u8>>,
    /// The directory the application's own module lives in, moved to the front.
    pub(crate) cwd: Option<Vec<u8>>,
    /// The CPython minor the boot discovered (`313`/`314`); `0` accepts any.
    pub(crate) expected_minor: u32,
    /// The application module, named in `VIEW_PATHS` so its own directory is
    /// part of the set a view must be able to read.
    pub(crate) app_module: Option<String>,
    /// Whether an embedded interpreter has to be pointed at the distribution's
    /// `site-packages` (false for the wheel's own interpreter).
    pub(crate) site_packages: bool,
    /// The installation prefix CPython must call home before it starts: the
    /// one `Config.python.python_home` recorded (design §2.4). This is the
    /// **only** way the prefix reaches the interpreter now — stage 0 exports no
    /// `PYTHONHOME` (`MB.42.2.1.4`).
    pub(crate) python_home: Option<Vec<u8>>,
}

impl Default for InitPaths {
    fn default() -> Self {
        Self {
            sys_path: Vec::new(),
            cwd: None,
            expected_minor: 0,
            app_module: None,
            // An embedded interpreter does not find `site-packages` on its own;
            // the wheel's own interpreter is the caller that turns this off.
            site_packages: true,
            python_home: None,
        }
    }
}

/// When §9.2 #7's heap immortalisation runs, as the boot's own token.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Immortalize {
    /// Immortalise wherever the marker is known, and skip it elsewhere.
    #[default]
    Auto,
    /// The same interpreters, asked for by an operator.
    On,
    /// Never immortalise, whatever the interpreter.
    Off,
}

impl Immortalize {
    /// This token as the interpreter crate's setting.
    pub(crate) fn setting(self) -> Setting {
        match self {
            Self::Auto => Setting::Auto,
            Self::On => Setting::On,
            Self::Off => Setting::Off,
        }
    }
}

/// A `preload` request (design §1.2).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Preload {
    /// `module:callable`, the WSGI application.
    pub(crate) app: Option<String>,
    /// Modules imported before the warm-up and inherited by every view.
    pub(crate) preload: Vec<String>,
    /// `module:callable`, called once after the preload imports.
    pub(crate) warmup: Option<String>,
    /// The dev-only loaded-file report socket, or a negative value for none.
    pub(crate) report_fd: i32,
    /// The project root the tracker canonicalises, required with a report fd.
    pub(crate) root: Option<Vec<u8>>,
    /// `BLASTIT_IMMORTALIZE` as stage 0 parsed it.
    pub(crate) immortalize: Immortalize,
    /// The path half of the boot, for a `Hosted` interpreter (a wheel), where
    /// `init_paths` is never called (design §1.2.1 item 1). An `Embed` boot
    /// applies this in `init_paths` and leaves it `None`.
    pub(crate) paths: Option<InitPaths>,
}

impl Default for Preload {
    fn default() -> Self {
        Self {
            app: None,
            preload: Vec::new(),
            warmup: None,
            report_fd: -1,
            root: None,
            immortalize: Immortalize::Auto,
            paths: None,
        }
    }
}

/// A `serve_view` request (design §1.2), sent by the child after the child
/// sequence has run.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Serve {
    /// The child's end of `view.chan`.
    pub(crate) view_fd: i32,
    /// The view's loaded-file report socket, or a negative value for none.
    pub(crate) loaded_fd: i32,
    /// The source id its batches name (the route index).
    pub(crate) loaded_source: u16,
    /// The route's name, for logs only.
    pub(crate) route: String,
    /// `SCRIPT_NAME`, already normalised by the composition root.
    pub(crate) script_name: String,
    /// The largest number of requests this process serves at once.
    pub(crate) max_threads: Option<u32>,
    /// The private scratch directory: `HOME`, `TMPDIR` and the working directory.
    pub(crate) home: Option<Vec<u8>>,
    /// `BLASTIT_FRAME_MAX`: the frame ceiling of this `view.chan` (§3.1). `None`
    /// keeps the transport's default.
    pub(crate) max_frame: Option<usize>,
}

impl Default for Serve {
    fn default() -> Self {
        Self {
            view_fd: -1,
            loaded_fd: -1,
            loaded_source: 0,
            route: String::new(),
            script_name: String::new(),
            max_threads: None,
            home: None,
            max_frame: None,
        }
    }
}

/// Parses one request, or returns the sentence a `deny_unknown_fields` refusal
/// becomes. An empty request is the struct's defaults.
pub(crate) fn parse<T>(req: &[u8]) -> Result<T, String>
where
    T: Default + serde::de::DeserializeOwned,
{
    if req.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice(req).map_err(|err| format!("the shim request is not valid JSON: {err}"))
}
