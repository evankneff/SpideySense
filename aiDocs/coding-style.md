# Coding Style

Last updated 2026-08-20

- **`cargo clippy --all-targets -- -D warnings` and `cargo fmt` must both be clean.** Not
  a suggestion; this is the bar for every commit.
- **No `unwrap()` or `expect()` on runtime paths.** Config parsing at startup may fail
  loudly. Everything after startup degrades and logs. `unwrap_or_else`, `if let`, and
  `let ... else` cover almost every case; for a genuinely impossible branch, use
  `unwrap_or_else` with a sane fallback and a comment saying why it cannot happen.
  Tests may use `expect` with a descriptive message.
- **One module per concern, and keep pure logic free of I/O.** `events.rs` and
  `lifecycle.rs` hold the interesting decisions and touch no MQTT, Tauri, clock, or
  filesystem — that is what makes them testable. Push side effects to the caller.
- **Time is a parameter, never read inside logic.** Pass `now: Instant` in. Anything
  calling `Instant::now()` inside a decision function is untestable.
- **Comments explain *why*, only where the logic is non-obvious.** Event debouncing,
  window positioning math, and lock ordering deserve comments. Getters do not. Prefer a
  comment that records a decision or a gotcha over one that restates the code.
- **Logging is `tracing` with structured fields**, never `println!`. Use
  `info!(camera = %name, ...)` rather than interpolating into the message. Match the
  level to the volume: per-event `update` traffic is `trace`/`debug`, decisions are
  `info`.
- **Never hold a `Mutex` across a window call or an `await`.** Compute under the lock,
  act outside it. Recover poisoned locks rather than propagating a panic.
- **Check `aiDocs/architecture.md` before adding a dependency.** Several were considered
  and rejected with reasons. Two flags do not justify a CLI parser.
- **Regression tests test the mechanism, not the output.** A test asserting a generated
  script *contains* the right CSS passed while the script was throwing before it ever
  ran. Assert the behaviour that broke.
- **Test fixtures use real values from this deployment** — `front_doorbell`, not
  `doorbell`. Fixtures that drift from reality hide bugs. Use `testutil::config_from`,
  never a hand-rolled temp directory.
- **Resist over-engineering.** No abstraction before its second use, no trait "for
  flexibility", no config knob nobody will turn. Delete dead code rather than commenting
  it out.
