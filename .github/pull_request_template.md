## Summary

Describe the problem and what changes for users.

## Linked issue

<!-- Add Closes #123 with the issue number, or explain why no issue is needed. -->

## Validation

Check what you ran; explain any skipped checks and report results.

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings`
- [ ] `cargo test --locked --workspace`
- [ ] `npm --prefix app/ui run check`
- [ ] `npm --prefix app/ui run check:tests`
- [ ] `npm --prefix app/ui run test`
- [ ] `npm --prefix app/ui run test:ui`
- [ ] `npm --prefix app/ui run build`
- [ ] If Rust types changed, regenerated and committed `app/ui/src/lib/bindings` with `cargo test --locked -p maxed-types --test export_bindings`.

Windows live testing performed or still needed:

## Privacy

- [ ] No raw recordings/captures, `live.sqlite3`, account details, or player names in the diff, logs, or attachments.
- [ ] Any capture fixtures are scrubbed, verified, and stored only under `tests/fixtures/scrubbed/` (or no fixtures changed).
