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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_request_is_the_defaults() {
        let init: InitPaths = parse(b"").expect("empty is the default");
        assert!(init.sys_path.is_empty());
        assert!(
            init.site_packages,
            "an embedded interpreter needs its paths"
        );

        let preload: Preload = parse(b"").expect("empty is the default");
        assert_eq!(preload.report_fd, -1, "no socket unless named");
        assert_eq!(preload.immortalize, Immortalize::Auto);

        let serve: Serve = parse(b"").expect("empty is the default");
        assert_eq!((serve.view_fd, serve.loaded_fd), (-1, -1));
    }

    #[test]
    fn the_boot_document_is_read_field_by_field() {
        let init: InitPaths = parse(
            br#"{"sys_path":[[47,115,114,118]],"cwd":[47,97,112,112],"expected_minor":313,
                 "app_module":"app","site_packages":false}"#,
        )
        .expect("a full request");
        assert_eq!(init.sys_path, vec![b"/srv".to_vec()]);
        assert_eq!(init.cwd, Some(b"/app".to_vec()));
        assert_eq!(init.expected_minor, 313);
        assert!(!init.site_packages);

        let preload: Preload = parse(
            br#"{"app":"app:wsgi","preload":["django"],"warmup":"app:warm",
                 "report_fd":7,"root":[47,97,112,112],"immortalize":"off"}"#,
        )
        .expect("a full request");
        assert_eq!(preload.app.as_deref(), Some("app:wsgi"));
        assert_eq!(preload.preload, ["django"]);
        assert_eq!(preload.report_fd, 7);
        assert_eq!(preload.immortalize, Immortalize::Off);
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let err = parse::<Serve>(br#"{"view_fd":3,"not_a_field":true}"#)
            .expect_err("an unknown field must not be dropped");
        assert!(err.contains("not_a_field"), "{err}");
    }

    #[test]
    fn malformed_json_names_itself() {
        let err = parse::<Preload>(b"{").expect_err("a truncated document");
        assert!(err.contains("not valid JSON"), "{err}");
    }

    #[test]
    fn the_immortalise_token_maps_to_the_interpreter_setting() {
        assert_eq!(Immortalize::Auto.setting(), Setting::Auto);
        assert_eq!(Immortalize::On.setting(), Setting::On);
        assert_eq!(Immortalize::Off.setting(), Setting::Off);
    }
}
