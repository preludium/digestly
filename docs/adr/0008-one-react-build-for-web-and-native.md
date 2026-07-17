# One React build serves browser, PWA, and future native wrappers

**Status: accepted.**

The `web/` directory is the single source of truth for all UI. The same build output is served by
the Rust binary (browser access), cached by the service worker (PWA and offline reading), and
would be loaded by a native wrapper (Tauri or Capacitor) without modification. Native-specific
capabilities, if added, live behind a thin guard (`isTauri()` / `isNativePlatform()`) in the
shared source - they do not fork the codebase.

## Considered options

**Fork the codebase per target (web + native).** Two codebases that diverge over time, doubling
maintenance of every UI change. Rejected.

**Separate native bundle with a different API client.** Native targets could use a different fetch
layer or auth mechanism. This was considered as part of the Tauri S2 plan (`docs/plans/s2-android.md`):
cross-origin auth requires bearer tokens rather than same-site cookies, but the solution is an
additive guard in the shared `api.ts`, not a fork. Rejected as the base approach; the bearer-token
path is an addition, not a split.

## Consequences

The installed PWA (via HTTPS and the service worker) works today with zero additional code. The
app shell is cached; offline read and queued-write replay work from the same bundle. Future native
wrappers (Tauri v2 Android, Capacitor) load this build from their WebView and guard any
native-specific calls with a platform check, keeping the browser/PWA build byte-for-byte
unaffected.

See `docs/plans/s2-android.md` and `docs/plans/capacitor-android.md` for the native implementation
plans (both planning-only, not yet built).
