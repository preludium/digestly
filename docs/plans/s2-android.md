# S2 - Tauri v2 Android: implementation plan

Status: **planning only** (not built). This document scopes what S2 would take, the pros/cons,
and the decisions to make before starting.

## 1. Goal (and non-goal)

**Goal:** ship a native Android app that is the **same React build** wrapped in a Tauri v2 shell,
acting as a **client to the home server** (the Rust binary running on the Pi/server behind
Tailscale). It adds only the things a browser PWA can't do well: OS-native secure token storage,
biometric unlock, and native notifications. First launch asks for the server's API base URL
(Tailscale hostname). Output: a **signable `.apk`** + a Mobile README.

**Non-goal:** the phone is **not** a server. No ingestion, no SQLite, no AI, no digest engine on
the device - all of that stays on the home server. The app only renders the UI and calls the API.
No UI fork: the exact same `web/` bundle powers browser, PWA, and Android.

## 2. Why this is more than "wrap the SPA"

The web app today is **same-origin**: it calls `/api/...` (relative) and authenticates with a
**signed session cookie** (`SameSite=Lax`, `HttpOnly`, not `Secure` - `src/auth/session.rs`). On
Android the app is loaded from Tauri's asset protocol (`tauri://localhost` / `http://tauri.localhost`),
and the API lives at a **different origin** (`https://digestly.<tailnet>.ts.net`). Two consequences
drive most of the work:

1. **Auth can't be cookie-based cross-origin.** `SameSite=Lax` cookies are **not sent** on
   cross-site requests, and Android WebView blocks third-party cookies. So the session cookie
   won't travel from the Tauri origin to the server. We need **bearer-token auth** in addition to
   the cookie.

2. **`/api` must become an absolute, configurable base URL.** The fetch client
   (`web/src/lib/api.ts`) hardcodes `fetch('/api' + path)`. In Tauri it must target the
   user-entered server origin.

Everything else (secure storage, biometrics, notifications, signing) is native-shell plumbing.

## 3. Target architecture

```
+---------------------------- Android device ----------------------------+
|  Tauri v2 app                                                          |
|  +-------------------------------+   Rust shell (src-tauri, per-target) |
|  |  WebView: the same web/        |   - secure storage (token)           |
|  |  React bundle                  |   - biometric unlock                 |
|  |  - base URL from config        |   - local notifications              |
|  |  - bearer token in header      |   - first-launch config command      |
|  +-------------+-----------------+                                       |
|                | HTTPS + Authorization: Bearer <token>                  |
+----------------+-------------------------------------------------------+
                 | (Tailscale)
          +------+--------------------------------------+
          |  Home server (existing Rust binary)         |
          |  - CORS allows tauri origin + headers       |
          |  - CurrentUser accepts cookie OR            |
          |    bearer token -> same sessions table      |
          +---------------------------------------------+
```

The token is the existing opaque **session id** (from the `sessions` table), delivered via
`Authorization: Bearer` instead of a cookie - reusing all existing session revocation
(logout / logout-all / user-delete). No new auth model, just a second transport.

## 4. Work breakdown

### 4.1 Server changes (small, but real - affects the shared binary)

- **Accept bearer tokens in `CurrentUser`** (`src/auth/extract.rs`): if no valid session cookie,
  fall back to `Authorization: Bearer <sid>` and look the session up the same way. One extractor,
  two transports. Keeps per-user scoping identical.
- **Return the session id on login** for token clients: add it to the `POST /api/auth/login`
  (and passkey login) response, or a dedicated `POST /api/auth/token`. Guard so the browser path
  still prefers the cookie (don't expose the sid to web JS unnecessarily - consider returning the
  token only when a header like `X-Client: tauri` is present, or from a separate endpoint).
- **CORS `allow_headers`** (`src/http.rs`): the current `CorsLayer` sets methods/credentials/origin
  but **not** `allow_headers`; a cross-origin `Authorization` + `Content-Type` request needs
  `.allow_headers([AUTHORIZATION, CONTENT_TYPE])`. Tauri origins are already allowed.
- **Logout for token clients**: `logout` currently clears the cookie; also delete the session row
  when the caller authenticated by bearer.
- Security review: bearer sid is equivalent to the cookie; keep it out of logs (already the
  pattern), keep 30-day TTL, ensure `logout-all` revokes it.

_Alternative considered:_ use Tauri's native HTTP plugin (`@tauri-apps/plugin-http`, reqwest-based)
so requests bypass the browser CORS/cookie model entirely. Cleaner CORS story, but splits the
transport layer (native HTTP in Tauri vs browser `fetch` on web) and still needs the bearer token.
Recommended path is **bearer + CORS allow-headers** - one transport, minimal server change.

### 4.2 Frontend changes (shared `web/`, no fork)

- **Configurable base URL** in `web/src/lib/api.ts`: read a runtime base (empty string on web -
  relative `/api`; the configured origin in Tauri). Store it in `localStorage` / Tauri store.
- **Runtime environment detection**: `isTauri()` (checks `window.__TAURI__`). Only in Tauri does
  the app attach the bearer token and use the absolute base URL.
- **First-launch config screen**: if running in Tauri and no base URL is set, show a screen to
  enter the server URL (validate it's https, **reject localhost**), ping `/api/health`, then
  continue to login. Editable later in Settings.
- **Token handling**: on login, store the returned token via the secure-storage bridge (below);
  attach `Authorization: Bearer` on every request; clear on logout. Web path is unchanged (cookie).
- **Notifications hook**: on Tauri, surface digest/feed-health as **local notifications** via the
  plugin (see 4.4).
- These are additive and guarded by `isTauri()`, so the browser/PWA build is byte-for-byte
  unaffected - satisfies "no UI fork."

### 4.3 Tauri shell (`src-tauri/`, new)

- **Scaffold** a Tauri v2 project pointing `frontendDist` at `web/dist`; `tauri.conf.json` with
  the app id (e.g. `net.ts.digestly`), name, icons, and Android config.
- **`cargo tauri android init`** to generate the Gradle/Android project.
- **Rust commands** exposed to the webview:
  - secure storage get/set/delete for the token,
  - biometric prompt (gate app open / token read),
  - "set/get API base URL".
- **Plugins** (Tauri v2 official where possible):
  - **Secure storage**: `tauri-plugin-stronghold` (encrypted vault, password/biometric-gated) is
    the pragmatic path. Proper Android Keystore / `EncryptedSharedPreferences` needs a small
    custom Kotlin plugin - heavier; decide based on how strict "Keystore" must be.
  - **Biometric**: `tauri-plugin-biometric` (v2, Android supported).
  - **Notifications**: `tauri-plugin-notification` (local notifications).
- **Permissions/capabilities**: Tauri v2 capability files granting the webview access to the above
  plugin commands + `http` to the server origin.

### 4.4 Notifications - decide the scope

- **Local notifications** (recommended, in scope): the app, while running/backgrounded, raises OS
  notifications for new digests / feed-health. Light, no external infra.
- **True push (FCM)** when the app is closed: requires a Firebase project, a push token registered
  with the server, and a server-side FCM sender - a **large** addition and arguably outside
  "client to home server." Recommendation: **rely on the user's existing ntfy** (the official ntfy
  Android app already does real push) for closed-app delivery, and use local notifications in-app.
  Document this explicitly.

### 4.5 Build, sign, CI

- **Toolchain** (must be installed on the build machine - **none present in this environment**):
  JDK 17, Android SDK + platform-tools, **Android NDK**, and the Rust Android targets
  (`aarch64-linux-android`, `armv7-linux-androideabi`, `i686-...`, `x86_64-...`), plus the Tauri CLI.
- **Debug build**: `cargo tauri android build --debug` - an installable APK.
- **Signing**: generate an upload keystore (`keytool`), configure Gradle signing, produce a
  **signed release `.apk`/`.aab`**. Document the keystore handling (never commit it).
- **CI (optional)**: a GitHub Actions job with `android-actions/setup-android` + NDK + the Rust
  targets to produce signed artifacts. Emulated builds are slow; a real device/emulator is needed
  for the install+login smoke test.

### 4.6 Docs

- **Mobile README**: prerequisites, how to build/sign the `.apk`, install (`adb install` /
  sideload), and set the API base URL on first launch. Note the Tailscale-hostname requirement and
  that the phone must be on the tailnet.

## 5. Pros

| Pro                         | Notes                                                                           |
| --------------------------- | ------------------------------------------------------------------------------- |
| Native secure token storage | Token in Keystore/stronghold, not browser `localStorage` - better than the PWA. |
| Biometric unlock            | Face/fingerprint to open the app - not possible in a plain PWA.                 |
| Real app presence           | Launcher icon, task switcher, native notifications, no browser chrome/URL bar.  |
| Reuses everything           | Same React bundle, same API, same sessions - no second UI, no second backend.   |
| Robust offline base         | Ships the write-sync outbox + PWA caching inside a controlled WebView.          |
| Distribution                | A signable `.apk` you can sideload to family devices without an app store.      |

## 6. Cons / risks / costs

| Con / risk                                    | Notes                                                                                                                                                                                  |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Server auth change**                        | Touches the shared, security-critical `CurrentUser` + CORS + login. Must be reviewed carefully; a bearer-token bug affects the web app too.                                            |
| **Heavy toolchain**                           | JDK + Android SDK + NDK + Rust Android targets + Tauri CLI. Multi-GB; not installed here; finicky version matching (NDK vs Gradle vs Tauri).                                           |
| **Can't be built/tested in this environment** | No Android SDK/NDK/JDK and no device/emulator here - the `.apk`-builds/installs/logs-in gate is **not runnable** in this session. Needs a real dev machine.                            |
| **Tauri v2 mobile maturity**                  | Mobile is newer than desktop; Android plugin ecosystem (esp. Keystore) is thinner - secure-storage "done properly" may need a custom Kotlin plugin.                                    |
| **Notifications caveat**                      | True closed-app push needs FCM infra; realistic scope is local notifications + rely on ntfy.                                                                                           |
| **Signing/release ops**                       | Keystore generation, safe storage, and (if ever) Play Store policies add ongoing overhead.                                                                                             |
| **Maintenance surface**                       | A new build target, native deps, and Android OS/WebView churn to keep working over time.                                                                                               |
| **Marginal value vs. PWA**                    | The installed PWA already gives home-screen install + offline reading + queued writes. Tauri's net-new value is Keystore + biometric + native notifications - real, but incremental.    |

## 7. The honest alternative: the PWA you already have

Android Chrome can **install the existing PWA** ("Add to Home Screen") today: home-screen icon,
standalone window, offline reading, and the write-sync outbox - **zero** additional code,
toolchain, or server change. What you _don't_ get vs. Tauri: Keystore-backed token, biometric
unlock, and native (non-ntfy) notifications. If those three aren't must-haves, the PWA covers ~80%
of S2's user-facing value for ~0% of the cost. This is the main decision.

## 8. Suggested phasing (if we proceed)

1. **Phase A - server + client auth/base-URL (no Tauri yet).** Bearer-token auth in `CurrentUser`,
   login token, CORS allow-headers, configurable API base URL + `isTauri()` in the web app. Fully
   testable headless (Rust tests + curl with `Authorization: Bearer`). This is the reusable core
   and de-risks the hardest part first.
2. **Phase B - Tauri scaffold + first-launch config + debug APK.** On a machine with the toolchain:
   scaffold, wire base URL, build an unsigned debug APK, install, log in, read.
3. **Phase C - native plugins.** Secure storage (stronghold or custom Keystore) + biometric unlock.
4. **Phase D - notifications.** Local notifications; document ntfy for closed-app push.
5. **Phase E - signing + Mobile README (+ optional CI).** Signed release `.apk`; docs; gate.

Phase A is the only part fully doable in this environment; B-E need the Android toolchain and a
device/emulator.

## 9. Effort (rough, relative)

- Phase A: **~1 focused day** (small server change + client plumbing + tests).
- Phase B: **~1 day** on a prepared machine (much of it toolchain setup/debugging).
- Phase C: **~1-2 days** (stronghold is quicker; a proper Keystore Kotlin plugin is the long end).
- Phase D: **~0.5-1 day** (local notifications) - more if FCM is required.
- Phase E: **~0.5-1 day** (signing + README).

Total ballpark: **~4-6 focused days**, most of it gated on having the Android toolchain and a
device, plus ongoing maintenance of a native target.

## 10. Recommendation

- If the goal is "use Digestly comfortably on my phone" - **install the PWA** (nothing to build).
- If Keystore-backed auth + biometric unlock + native notifications are genuinely wanted - do
  **Phase A now** (it's safe, headless-testable, and independently useful for pointing any client
  at a remote server), then B-E later on a machine with the Android toolchain.
- Building B-E in _this_ environment isn't possible; the plan is structured so we get real,
  verified value (Phase A) without pretending the on-device gate ran.
