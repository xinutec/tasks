# tasks (Android)

The task list presented as a native-feeling app: a single full-screen
**WebView**, no address bar, no tabs, a home-screen icon. It avoids browser
chrome while showing the UI exactly as designed (the system WebView is Chromium,
so it renders like Chrome).

The site is **private** — its DNS points inside the WireGuard tunnel — and
**behind a Nextcloud sign-in**. The WebView keeps the session cookie, so it is a
**one-time login**; the app needs only `INTERNET`, since the VPN is set up at the
OS level and not by this app. A phone off the VPN cannot resolve the host at all.

⚠ An installed, signed-in copy is a **standing authenticated session** over the
list — and unlike the memory viewer beside it, this list is a *working surface*:
from it a task can be moved, finished or filed. That is the point of having it on
a phone, and it is worth knowing before the phone leaves the house.

## What it does

Almost nothing, which is the point: everything a wrapper does belongs to
`org.xinutec:shell` (see `~/Code/ui-harness/android`), so all that is left here is
the address and the login hop.

- Loads `https://tasks.xinutec.org/` — **hardcoded** (`MainActivity.TASKS_URL`);
  this app is single-purpose.
- `allowedHosts` names the app **and `dash.xinutec.org`**. Host confinement is on
  by default, and without the identity provider on that list the OAuth round-trip
  is ejected to the browser and the app can never sign in.
- The shell handles the rest: insets including the keyboard, bars painted from the
  page's own surface colour so they track light/dark, Back through the SPA
  history, and reopening on the last in-app page.

Runs on any Android 8+ (minSdk 26) device.

## The icon is blue on purpose

⚠ **Not memview's `#5E35B1`.** The two apps sit side by side on the same
launcher, and an icon that differs only by its mark is one you tap wrong at a
glance. The launcher background, the web icon (`frontend/public/icon.svg`), the
page's `theme-color` — which the shell paints the system bars from — and the
Angular Material palette are all `#1565C0`/azure, so the icon and the app it
opens are the same colour.

The mark is a ticked box with an arrow leaving it: the app's thesis is not the
list but the handover.

## Build & install

No toolchain lives in this repo — it borrows the recall project's `android` nix
dev shell (JDK 17 + Android SDK; the Gradle wrapper pins Gradle). It also needs
`~/Code/ui-harness` checked out beside this repo, which `app/build.gradle.kts`
asserts in a sentence rather than a stacktrace.

```sh
nix develop ~/Code/recall#android --command ./deploy.sh
```

`deploy.sh` builds and installs, and keys on the device **model** rather than an
IP: DHCP drifts, and a bare `adb install` could hit whichever other device happens
to be connected. Pass an `ip[:port]` if wireless debugging has rotated to a random
port.
