# nolune for macOS

`nolune.app`: nolune set up and running without a Terminal. It carries its own Node and the npm
package, walks through a first-run onboarding in space (the web welcome's intro, then who's
setting it up, Full Disk Access, and the gateway started in the background), and stays in the menu
bar after.

## Build it

On a Mac with Xcode's command line tools, Node 22+ and pnpm:

```sh
macos/scripts/build-app.sh
```

That's `macos/dist/<arch>/nolune.app` and `macos/dist/nolune-<version>-<arch>.dmg`, ad-hoc signed:
they run on the Mac that built them. For one to hand out, sign it with a Developer ID and notarize
it:

```sh
# Once: store an app-specific password for notarytool in the keychain.
xcrun notarytool store-credentials nolune --apple-id you@example.com --team-id TEAMID

SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" NOTARY_PROFILE=nolune \
  macos/scripts/build-app.sh
```

`ARCH=x86_64` builds for Intel Macs; `NODE_VERSION` picks the Node to bundle (the newest 24.x by
default). The script's header lists the rest.

## Work on it

`swift run --package-path macos` runs the onboarding from a checkout, without building the app. It
uses the checkout's CLI (`packages/cli/src/index.ts`) and the first `node` on your PATH, and skips
registering the background service, which needs the app bundle; the font and the song come with the
bundle too, so it's in the system font and silent. `NOLUNE_HOME` points it at other
data; `defaults delete dev.nolune.app` (or `defaults delete Nolune` from `swift run`) starts the
onboarding over.

## How it works

- **One executable, three jobs** (`Sources/Nolune/NoluneApp.swift`): the app people open,
  `--gateway` for launchd, and `--probe-disk-access`, a one-shot Full Disk Access check.
- **The gateway runs as the app.** The LaunchAgent (`Resources/dev.nolune.gateway.plist`,
  registered with `SMAppService`) runs the app's own executable, which starts `node cli.js start`
  as its child (`Gateway.swift`). macOS charges a process's file access to the app that launchd
  started, so the gateway and every command it runs count as nolune: one Full Disk Access switch,
  named nolune, covers them, and it holds across updates because the app's signature doesn't
  change. It keeps the label `nolune service install` uses, so `nolune service restart|status|logs`
  work the same; the app removes a LaunchAgent left by `nolune service install` before registering.
- **Full Disk Access** (`DiskAccess.swift`). There's no API to ask for it. The app opens the right
  pane, reads a protected file so it usually shows up in the list switched off, offers its icon to
  drag in when it doesn't, and checks in a new process every second until the switch is on: a
  running process may not see the grant before it relaunches.
- **Signing** (`scripts/build-app.sh`): every Mach-O in the package, then Node with the
  entitlements V8 needs (`Resources/node.entitlements`, as Node's own builds have), then the app.
- **The intro** (`Sky.swift`, `IntroView.swift`) is IntroSky.svelte and the welcome's wordmark
  animation, redrawn with SwiftUI's `Canvas` on the web's timings and song. The wordmark's path is
  copied from `src/lib/assets/logo.svg`; keep them in step.

The CI workflow `macos.yml` builds the app on every change here and checks the bundle starts a
gateway that answers.
