//! The combinations of values that only make sense together (§8.2, D5).

use blastit_frame::{Limits, is_header_name};

use super::invalid;
use super::parse::check_trusted_proxies;
use super::settings::{Config, MIN_DEBT_TTL};
use super::types::Warning;

impl Config {
    /// Checks the combinations of values that only make sense together, and
    /// clamps the ones that have a usable nearest value (§8.2, D5).
    ///
    /// # Errors
    ///
    /// [`super::ConfigError`] naming a variable.
    pub fn validate(&mut self) -> Result<(), super::ConfigError> {
        if self.max_workers == 0 {
            return Err(invalid(
                "BLASTIT_MAX_WORKERS",
                "must be at least 1, or no request can ever run".to_string(),
            ));
        }
        if let Some(0) = self.view_max_workers {
            return Err(invalid(
                "BLASTIT_VIEW_MAX_WORKERS",
                "must be at least 1, or the view could never serve a request".to_string(),
            ));
        }
        // The view's `RLIMIT_NPROC` is `view_max_workers + 16` (§7.2); it must
        // leave room for the interpreter's own threads.
        let cap = self.view_worker_cap();
        if cap.checked_add(16).is_none() {
            return Err(invalid(
                "BLASTIT_VIEW_MAX_WORKERS",
                "is too large to size RLIMIT_NPROC from".to_string(),
            ));
        }
        if self.health_path.is_empty() || !self.health_path.starts_with('/') {
            return Err(invalid(
                "BLASTIT_HEALTH_PATH",
                "must be an absolute path".to_string(),
            ));
        }
        if !(0.0..=100.0).contains(&self.mem_high_watermark_percent) {
            return Err(invalid(
                "BLASTIT_MEM_HIGH_WATERMARK",
                "must be a percentage between 0% and 100%".to_string(),
            ));
        }
        Limits::new(self.frame_max)
            .map_err(|error| invalid("BLASTIT_FRAME_MAX", error.to_string()))?;
        if self.backlog < 1 {
            return Err(invalid("BLASTIT_BACKLOG", "must be at least 1".to_string()));
        }
        if self.max_connections < 1 {
            return Err(invalid(
                "BLASTIT_MAX_CONNECTIONS",
                "must be at least 1".to_string(),
            ));
        }
        if let Some(app) = &self.app
            && !app.contains(':')
        {
            return Err(invalid(
                "BLASTIT_APP",
                "must be `module:attribute`".to_string(),
            ));
        }
        if let Some(0) = self.max_view_processes {
            return Err(invalid(
                "BLASTIT_MAX_VIEW_PROCESSES",
                "must be at least 1, or no view process could ever run".to_string(),
            ));
        }
        if self.view_mem_estimate == 0 {
            return Err(invalid(
                "BLASTIT_VIEW_MEM_ESTIMATE",
                "must be at least 1 byte, or the view process cap cannot be derived from \
                 memory (§8 Sizing)"
                    .to_string(),
            ));
        }
        if let Some(warmup) = &self.warmup
            && !warmup.contains(':')
        {
            return Err(invalid(
                "BLASTIT_WARMUP",
                "must be `module:callable`".to_string(),
            ));
        }
        // D5: the fair queue's knobs are clamped to something usable rather than
        // refused, the way the reference implementation does it — a /33 prefix is
        // a /32, and a table that holds nothing is a table that holds one — and
        // the operator is told once at boot.
        self.clamp_fair_queue();
        // A name that can never match a request is a setting that does nothing:
        // the fair queue's header and cookie lists are tokens (§3.1), so a name
        // with a space, a colon or a control byte in it is refused rather than
        // hashed into a key nothing will ever produce.
        for (variable, kind, names) in [
            ("BLASTIT_FQ_INNER_HEADERS", "header", &self.fq_inner_headers),
            ("BLASTIT_FQ_INNER_COOKIES", "cookie", &self.fq_inner_cookies),
        ] {
            if names.iter().any(|name| !is_header_name(name)) {
                return Err(invalid(
                    variable,
                    format!("every entry must be a {kind} name, never a value"),
                ));
            }
        }
        if !is_header_name(&self.client_ip_header) {
            return Err(invalid(
                "BLASTIT_CLIENT_IP_HEADER",
                "must be a header name, without its colon".to_string(),
            ));
        }
        if let Err(reason) = check_trusted_proxies(&self.trusted_proxies) {
            return Err(invalid("BLASTIT_TRUSTED_PROXIES", reason));
        }
        // `ADR-0010` §3: the host `libphp` and the shim directory are absolute
        // paths, and nothing else. A relative one would be resolved against a
        // working directory the stage-1 image does not share with whoever
        // started the boot, so the file that was inspected and the file that is
        // loaded could differ.
        if let Some(path) = &self.libphp
            && !path.is_absolute()
        {
            return Err(invalid(
                "BLASTIT_LIBPHP",
                "must be an absolute path to a libphp.so".to_string(),
            ));
        }
        if let Some(dir) = &self.php_shim_dir
            && !dir.is_absolute()
        {
            return Err(invalid(
                "BLASTIT_PHP_SHIM_DIR",
                "must be an absolute directory holding the libblastit_php*.so shims".to_string(),
            ));
        }
        Ok(())
    }

    /// Brings the `BLASTIT_FQ_*` knobs into the range the queue can use (D5).
    ///
    /// Each of these has an obvious nearest usable value — a prefix cannot be
    /// longer than the address it masks, a table cannot hold zero flows, and a
    /// debt TTL shorter than a second is a TTL that expires between two checks
    /// — so a value out of range is adjusted, not refused: a server that will
    /// not start over a prefix length is worse than one that starts and says
    /// what it did ([`Config::warnings`], printed once at boot).
    fn clamp_fair_queue(&mut self) {
        if self.fq_outer_prefix_v4 > 32 {
            self.fq_outer_prefix_v4 = 32;
            self.warnings.push(Warning::OuterPrefixV4Clamped);
        }
        if self.fq_outer_prefix_v6 > 128 {
            self.fq_outer_prefix_v6 = 128;
            self.warnings.push(Warning::OuterPrefixV6Clamped);
        }
        for (value, warning) in [
            (&mut self.fq_outer_max, Warning::OuterMaxClamped),
            (
                &mut self.fq_inner_max_per_outer,
                Warning::InnerMaxPerOuterClamped,
            ),
            (&mut self.fq_inner_max, Warning::InnerMaxClamped),
        ] {
            if *value == 0 {
                *value = 1;
                self.warnings.push(warning);
            }
        }
        if self.fq_debt_ttl < MIN_DEBT_TTL {
            self.fq_debt_ttl = MIN_DEBT_TTL;
            self.warnings.push(Warning::DebtTtlFloored);
        }
    }
}
