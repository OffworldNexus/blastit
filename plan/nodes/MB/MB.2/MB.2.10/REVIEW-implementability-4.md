# Implementability review 4 — static-and-spa.md (final doc sync)

Code is truth; only the four last-round fixes are re-checked.

1. **FIXED** `slot_releasable` is `pub(crate)` (§4.6 l.724–729) and tabled by a `#[cfg(test)]` unit test in `scheduler.rs` (l.740–744, §9 l.1169); `lib.rs:70` has `mod scheduler;`, so that test reaches it without a `lib.rs` export. No text claims the integration suite calls it — l.741–744 says the flow cases prove it is *consulted*.
2. **FIXED** The three declared-route `.with_static` sites are named (§4.6 l.679–686, §4.7 l.788–791) matching `stage1.rs:280–283`/`285–292` and `route.rs:106`; the `LEARN_FALLBACK_ROUTE` builder (`stage1.rs:275–279`) is stated as having no `route` and keeping `RouteInfo::new` defaults. No dangling `route` expression.
3. **FIXED** `static_parts: Option<Vec<String>>` and `static_has_body: bool` are named `Request` fields (§4.6 l.692–696); the head sequence — `declares_body` pairs, `from_utf8`→`Refused(Encoding)`, `head_segments`, `Err`→`decide` before `admit_ready`, `Ok`→`static_pending`, `resolve`+`decide` in `dispatch_static` — is buildable with every callee already reachable (`static_asset.rs:615`, `rewrite.rs:79/143`, `route.rs:35/66`), so only `scheduler.rs` changes.
4. **FIXED** §8 l.1109–1118 forbids trimming either leaf's `critical_rewrite.rs` cases (shared 1,600-line ceiling, append-only) and l.1124–1131 truthfully records the un-re-synced `MB.2.5.2` brief, confirmed in its `NODE.md` l.23–24 (`static_reads`, `decide` before `plan_request_body`).

## Remaining

- **MINOR** §4.3 l.460 builds `&[(Vec<u8>, Vec<u8>)]` (copies every head header), while `begin_raw`'s doc at l.1034–1035 still promises "no header is copied into an owned `Vec`"; say the copy is policy-route-only and amend that sentence in the same diff.
- **MINOR** Answer the head refusal *after* `self.requests.insert` (as `Resolution::Bad`, l.1114): `reject`→`emit_request` (l.2808–2811) silently returns for an unknown stream, so l.459's "before the payload moves into the request" read literally drops the `request` event §7.2 asserts. Insert, then reject, still before `admit_ready`.
- **MINOR** §4.7 l.788 quotes `manifest.app().is_some()` where the landed `routes_from_manifest` passes `has_app` (`route.rs:98,106`); name `has_app` so the sentence diffs against the crate.

VERDICT: PASS
