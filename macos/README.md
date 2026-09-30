# nolune for macOS

`nolune.app`: nolune set up and running without a Terminal. It carries its own Node and the npm
package, walks through a first-run onboarding (a big bang, then who's setting it up, an address
that works from anywhere, Full Disk Access, and the gateway started), and stays in the menu bar
after, running nolune while it's there.

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

With an App Store Connect API key instead of a keychain profile, set `NOTARY_KEY` (the .p8 file),
`NOTARY_KEY_ID` and `NOTARY_ISSUER`: that's how the Publish workflow signs and notarizes the app
for each release (see Publishing in [the development guide](../docs/development.md)).

To test on your own Mac, an `Apple Development` identity works as well, and keeps the Full Disk
Access switch on across builds, as an ad-hoc signature doesn't; `security find-identity -v -p
codesigning` lists yours. `ARCH=x86_64` builds for Intel Macs; `NODE_VERSION` picks the Node to bundle (the newest 24.x by
default). The script's header lists the rest.

## Work on it

`swift run --package-path macos` runs the onboarding from a checkout, without building the app. It
uses the checkout's CLI (`packages/cli/src/index.ts`) and the first `node` on your PATH, and
doesn't start a gateway (run `nolune start` or `pnpm dev` for one); the font and the song come with the
bundle too, so it's in the system font and silent. `NOLUNE_HOME` points it at other
data; `defaults delete dev.nolune.app` (or `defaults delete Nolune` from `swift run`) starts the
onboarding over.

## How it works

- **One executable, three jobs** (`Sources/Nolune/NoluneApp.swift`): the app people open,
  `--gateway`, the gateway's keeper, and `--probe-disk-access`, a one-shot Full Disk Access check.
- **The gateway runs with the app.** When the app opens it starts its own executable with
  `--gateway` as its child (`Service.swift`), which runs `node cli.js start` and keeps it running
  (`Gateway.swift`); quitting the app stops it. The app opens at login, so nolune is up whenever
  it's in the menu bar, and only then. macOS charges a process's file access to the app it runs
  under, so the gateway and every command it runs count as nolune: one Full Disk Access switch,
  named nolune, covers them, and it holds across updates because the app's signature doesn't
  change. `nolune service restart|status|logs` work with the app's gateway (the keeper's pid is in
  `$NOLUNE_HOME/gateway.pid`); the app removes a LaunchAgent left by `nolune service install`,
  which would fight it for the port.
- **An address from anywhere** (`Relay.swift`). The onboarding's second step gets one through
  nolune's relay (`packages/relay`), like `https://smiths.nolune.family`, with
  `nolune relay enable [--name]`: the family opens it on any phone or laptop, with nothing to set
  up on the router. "Only on this Mac for now" skips it, and the menu bar's "Open it from
  anywhere…" shows the same step later, then restarts the gateway to connect. The step is skipped
  when nolune has an address already (the relay's, or a public URL of its own). While there's one,
  it's what "Open nolune" opens and the menu shows (`Runtime.origin`, as `publicOrigin` in core).
- **Full Disk Access** (`DiskAccess.swift`). There's no API to ask for it. The app opens the right
  pane, tries a few protected files so it usually shows up in the list switched off, offers its
  icon to drag in when it doesn't, and checks in a new process every second until the switch is on:
  a running process may not see the grant before it relaunches. Any one protected file it can read
  is the answer, since none is on every Mac (macOS 27 moved the user's privacy database).
- **Signing** (`scripts/build-app.sh`): every Mach-O in the package, then Node with the
  entitlements V8 needs (`Resources/node.entitlements`, as Node's own builds have), then the app.
- **The intro** (`IntroView.swift`, `Sky.swift`) is a big bang of about four seconds: a point of
  light bursts and the stars and the eight colors fly out of it into the web welcome's sky
  (IntroSky.svelte, redrawn with SwiftUI's `Canvas`). The full intro, with the wordmark, is the
  web welcome's, which comes next.
- **It bursts out of the window** (`Outburst.swift`): a see-through window over each screen, above
  the onboarding and the menu bar, that clicks pass through. While the light gathers, the desktop
  dims and specks of light spiral in from all over the screen; at the bang the window's knocked
  about, the flash lights up the desktop, the shock waves roll out across the screen and sparks
  fly out past the window's edges, falling as they burn out. With no screen to draw on, the window
  draws the bang itself. It's heard a moment after it's seen, far away: unfa's Big Boom, made
  distant (`Resources/Sounds`, with its credit).

The CI workflow `macos.yml` builds the app on every change here, checks the bundle keeps a
gateway that answers, restarts and stops, and keeps every screen as a PNG (`Nolune --snapshot <folder>`, which draws
the intro at fixed moments, in the window and out over a stand-in desktop, each step and the menu without doing
anything).
