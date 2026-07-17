# Android via Capacitor (`server.url`) - implementation plan

Status: **planning only** (not built). An alternative to the Tauri S2 plan
(`docs/plans/s2-android.md`) that reuses 100% of the web app and **avoids the server auth change**,
by pointing a native WebView at the live home server instead of bundling assets locally.

Best fit: a **personal / family instance with a stable Tailscale hostname**.

## 1. The idea in one paragraph

Capacitor wraps a system WebView + a JS-native bridge into an installable APK. With `server.url`
set, the WebView **loads your real origin** (`https://digestly.<tailnet>.ts.net`) instead of a
bundled copy. Because the app is then **same-origin** with the API, your existing session-cookie
auth, service worker, offline reading, and the write-sync outbox all work **unchanged - no server
changes at all**. The same `web/dist` the server already serves is what the app renders, so there
is no second bundle and no UI fork. Native features (splash, status bar, back button, local
notifications, biometric app-lock) are optional add-ons via plugins, guarded so the browser/PWA
build is untouched.

## 2. Two decisions that shape everything

### 2.1 Fixed host vs. user-editable host

`server.url` is read **before** your JS runs, so it's a **compile-time constant** - your React
"enter your server" screen can't set it. Options:

- **(Recommended) Bake the host in.** Build the APK with your tailnet hostname. To change it, you
  rebuild. Perfect for a one-server personal/family setup. **Keeps the whole "no server change"
  win.**
- **Editable host** - you'd bundle assets locally + use a runtime base URL, which makes the app
  cross-origin - back to needing **bearer-token auth on the server** (the Tauri cost). Only worth
  it if you genuinely need to switch servers from the UI.

This plan assumes **fixed host**.

### 2.2 Online-to-launch

Unlike a bundled app or an installed PWA (which cache a shell), `server.url` **needs the origin
reachable on cold start** - no local fallback screen. So the phone must be on Tailscale and the
server up to _open_ the app. After first load, the service worker still serves cached items for
**reading** offline, and the write-sync outbox still queues writes. If cold-start-offline matters,
use the bundled+bearer path instead (or the installed PWA, which does cache its shell). For a
home reader, "online to launch" is usually acceptable.

## 3. Auth & passkeys (important nuance)

- **Password login: works as-is.** Same-origin cookies persist in the WebView; `CurrentUser` reads
  them exactly like the browser. No server change.
- **Passkeys: risky in a raw WebView.** WebAuthn support in Android's System WebView (what
  Capacitor uses) is inconsistent, unlike full Chrome (what a TWA uses). Passkey login may not
  work in the Capacitor WebView. Mitigations: rely on password login in the app, and/or add a
  **biometric app-lock plugin** (fingerprint/face to open the app) instead of passkey login. If
  passkey login on mobile is a hard requirement, **TWA is the safer wrapper**.

## 4. Files & config to add

Nothing in the existing app changes behaviour; additions are guarded by `isNativePlatform()`.

### 4.1 Dependencies (in `web/`)

```
npm i @capacitor/core @capacitor/android @capacitor/app @capacitor/status-bar @capacitor/splash-screen
npm i -D @capacitor/cli
# optional later: @capacitor/local-notifications, a biometric plugin
```

`@capacitor/core` adds a small amount to the served bundle; in a normal browser
`Capacitor.isNativePlatform()` is `false`, so nothing native activates.

### 4.2 `capacitor.config.ts` (repo root or `web/`)

```ts
import type { CapacitorConfig } from "@capacitor/cli";

const config: CapacitorConfig = {
    appId: "net.ts.digestly",
    appName: "Digestly",
    webDir: "web/dist", // required by CLI; not used for loading when server.url is set
    server: {
        url: "https://digestly.<tailnet>.ts.net", // <- your fixed tailnet origin
        androidScheme: "https",
        allowNavigation: ["digestly.<tailnet>.ts.net"],
        cleartext: false,
    },
};
export default config;
```

### 4.3 A single native-glue module - `web/src/lib/native.ts`

Keeps all Capacitor calls in one guarded place so the web build stays clean:

```ts
import { Capacitor } from "@capacitor/core";

export const isNativeApp = () => Capacitor.isNativePlatform();

/** Wire up native niceties once, only inside the Capacitor app. Lazy-imports so browser/PWA
 *  builds don't pull plugin code into the main path. */
export async function initNative() {
    if (!isNativeApp()) return;
    const [{ App }, { StatusBar, Style }, { SplashScreen }] = await Promise.all(
        [
            import("@capacitor/app"),
            import("@capacitor/status-bar"),
            import("@capacitor/splash-screen"),
        ],
    );
    // Android hardware back -> in-app history, exit at the root.
    App.addListener("backButton", ({ canGoBack }) =>
        canGoBack ? window.history.back() : App.exitApp(),
    );
    StatusBar.setStyle({ style: Style.Dark }).catch(() => {});
    SplashScreen.hide().catch(() => {});
}
```

Call `initNative()` once from `web/src/main.tsx` (next to `registerServiceWorker()`), and open
external "read original" links via `@capacitor/browser` when `isNativeApp()` so they don't
navigate the app off your origin.

### 4.4 Generate the Android project

```
npx cap init Digestly net.ts.digestly --web-dir web/dist   # writes capacitor.config
npx cap add android                                         # creates ./android (Gradle project)
npm run build && npx cap sync android                       # copy config + web assets
npx cap open android                                        # open in Android Studio
```

## 5. Build, sign, install

- **Debug APK:** Android Studio - _Build > Build APK_, or `cd android && ./gradlew assembleDebug`.
- **Signed release:** create an upload keystore (`keytool`), add `signingConfigs` to
  `android/app/build.gradle` (or use Android Studio's _Generate Signed Bundle/APK_) - signed
  `.apk`/`.aab`. Never commit the keystore.
- **Install:** `adb install app-release.apk`, or sideload the APK to family devices.
- **Toolchain required (NOT present in this environment):** JDK 17 + Android SDK + platform-tools
  (Android Studio bundles these). **No Rust, no NDK** - the big difference from Tauri.
- **CI (optional):** GitHub Actions with `actions/setup-java` + `android-actions/setup-android` +
  Gradle. (PWABuilder is TWA-only; it can't package Capacitor.)

## 6. Optional native features (add only if wanted)

| Feature                           | Plugin                                                                | Notes                                                                                      |
| --------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| Splash / status bar / back button | `@capacitor/app`, `@capacitor/status-bar`, `@capacitor/splash-screen` | The core polish; small.                                                                    |
| External links in system browser  | `@capacitor/browser`                                                  | Keep "open original" off the app WebView.                                                  |
| Local notifications               | `@capacitor/local-notifications`                                      | In-app digest/health nudges. Closed-app push still best served by your **ntfy** app.       |
| Biometric app-lock                | e.g. `@aparajita/capacitor-biometric-auth`                            | Fingerprint/face to open the app (alternative to passkeys, which may not work in WebView). |
| Secure token storage              | community secure-storage plugin                                       | **Probably unnecessary** - same-origin cookies already live in the WebView.                |

## 7. Pros / cons

| Pros                                                          | Cons                                                                                    |
| ------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Same-origin - cookies/SW/offline work; **zero server change** | Host baked at build time (rebuild to change)                                            |
| 100% code reuse, one bundle, no fork                          | **Online required to cold-start** the app                                               |
| No Rust/NDK - lighter toolchain than Tauri                    | Still needs JDK + Android SDK + a device to build/test                                  |
| Native bridge available for later polish/plugins              | Raw WebView != full Chrome - **passkeys may not work** (use password + biometric plugin) |
| Real signable/sideloadable APK                                | App-store policies dislike thin web wrappers (irrelevant for sideload)                  |

## 8. Phasing & effort (rough)

1. **Phase 1 - minimal wrapper (~0.5-1 day on a prepared machine).** Add deps + `capacitor.config.ts`
   + `native.ts`; `cap add android`; build debug APK - install - **password login (cookie) - read**.
   That already satisfies the gate's spirit (installs, points at server, logs in, reads).
2. **Phase 2 - polish (~0.5 day).** Splash, status bar, back-button, external-link handling,
   `allowNavigation`.
3. **Phase 3 - optional plugins (~0.5-1 day).** Local notifications and/or biometric app-lock.
4. **Phase 4 - signing + Mobile README (+ optional CI) (~0.5 day).**

Total ~**2-3 focused days**, vs ~4-6 for Tauri - and no shared-server security change.

## 9. What's verifiable where

- **Doable now, headless, in this repo:** add `@capacitor/core` + `capacitor.config.ts` +
  `web/src/lib/native.ts` (guarded) and confirm `tsc --noEmit && vite build` + Vitest stay green -
  i.e. prove the additions don't disturb the web/PWA build. This is real Phase-1 prep.
- **Needs an Android machine + device (NOT here):** `cap add android`, the Gradle/APK build,
  install, and the on-device login/read smoke test.

## 10. Recommendation

- **Personal/family, stable tailnet host, want an APK with least fuss and no server change** -
  **Capacitor + `server.url` (fixed host)**. Accept "online to launch"; use password login (+
  optional biometric plugin) since passkeys are unreliable in the WebView.
- **Want passkey/biometric login specifically** - prefer **TWA** (full Chrome).
- **Don't want any build toolchain** - **install the PWA**.
