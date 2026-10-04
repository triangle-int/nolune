# nolune for iOS

nolune for iPhone and iPad: the family's nolune in an app of its own, with the bell's notifications
on the lock screen. The first screen asks which nolune to open (the family's name on nolune's
relay, its address, or a link from it, like an invite), and the app shows its web UI from then on.
[The iOS app](../DESIGN.md#the-ios-app) in the design notes says how it works.

## Build it

On a Mac with Xcode 26 or later, open `ios/Nolune.xcodeproj` and run the **Nolune** scheme on a
simulator. For an iPhone, pick your team under the target's Signing & Capabilities first. The app
runs on iOS 16 and later; built with iOS 26's SDK, it wears that version's look there, Liquid Glass.

To try it against nolune from a checkout, run `pnpm dev` and connect the simulator to
`http://localhost:5173` (on this network the app may use plain http; everywhere else it's https).

The tests (`NoluneTests`) run with ⌘U, or:

```sh
xcodebuild test -project ios/Nolune.xcodeproj -scheme Nolune \
  -destination 'platform=iOS Simulator,name=iPhone 17' CODE_SIGNING_ALLOWED=NO
```

The CI workflow `ios.yml` does the same on every change here, builds for a device too, and keeps
its screens as PNGs: the first one; the native sign-in, sidebar, chat, new chat, bell and a
profile's pages, from a pretend nolune (`ci/nolune.py`, which you can run the same way: `python3
ios/ci/nolune.py 5780 signed-in`, then launch with `-origin http://localhost:5780`); the note for
an older nolune; and today's web app with a page open. A link like a widget's, `xcrun simctl
openurl booted 'nolune://open?path=/p/<slug>/memory'`, asks first in the simulator (Open in
"nolune"?); a widget's own doesn't.

The app's screens are native ([#125](https://github.com/triangle-int/nolune/issues/125)). To see
today's web app instead, add `-native NO` under Arguments Passed On Launch in the scheme
(Product > Scheme > Edit Scheme > Run > Arguments); `-open /p/<slug>/c/<id>` opens a page first.

`Sky.swift` and `Theme.swift`, and the Figtree font, are the macOS app's (`macos/`): the first
screen wears its onboarding, and a change there shows in both apps. Xcode reads the `Nolune`,
`NoluneWidgets` (the widgets and the Live Activity), `NoluneShare` (the share sheet), `Shared`
(built into all three) and `NoluneTests` folders as they are, so a new file needs no change to the
project. The project itself is plain text; its extensions' settings are in `Config/`.

To see a widget, add it from the simulator's Home Screen once the app has signed in; to see the
share sheet, share a photo from Photos to nolune. Siri's "Ask nolune" works in the simulator too,
or run it from the Shortcuts app.

## Notifications

The bell's notifications reach an iPhone through nolune's relay, which holds the app's key from
Apple (see [packages/relay](../packages/relay/README.md)). A build run from Xcode gets them from
Apple's sandbox, and TestFlight and App Store builds from production; the relay sends each to the
right one.

To see how the app handles one without a relay, send the simulator a notification as the gateway
would, with the path it opens and the address it comes from:

```sh
cat > /tmp/notification.json <<'END'
{
  "Simulator Target Bundle": "dev.nolune.app",
  "aps": { "alert": { "title": "Umbrellas", "subtitle": "Family", "body": "Rain at 3pm." }, "sound": "default" },
  "path": "/p/family",
  "origin": "http://localhost:5173"
}
END
xcrun simctl push booted /tmp/notification.json
```

## Put it in the App Store

Once, in the [Apple Developer](https://developer.apple.com/account) account that publishes it:

1. **The app's IDs.** Under Certificates, Identifiers & Profiles > Identifiers, register the
   bundle ID `dev.nolune.app` with Push Notifications and App Groups, and its extensions',
   `dev.nolune.app.widgets` and `dev.nolune.app.share`, with App Groups; then the app group
   `group.dev.nolune.app`, and add it to all three (Xcode's automatic signing does all this too).
   The macOS app has the same ID, outside the Mac App Store, so the two can become one app later.
   With another ID, change `PRODUCT_BUNDLE_IDENTIFIER` in the project and `APNS_TOPIC` on the
   relay; with another group, `Shared.group` in `Shared/Shared.swift` and the three
   `.entitlements` in `Config/`.
2. **The key for notifications.** Under Keys, make one with Apple Push Notifications service
   (APNs) for Sandbox & Production, and give it to the relay (`APNS_KEY`, `APNS_KEY_ID`,
   `APNS_TEAM_ID`; see Running it in [packages/relay](../packages/relay/README.md)). The relay
   then answers `/api/gateways/<name>/push` instead of 501.
3. **The app in App Store Connect.** Apps > New App: iOS, the bundle ID above, and a name (the
   App Store's names are unique; "nolune" may need a few words after it). It runs on iPads too,
   so the listing needs iPad screenshots as well as iPhone ones. Its privacy policy is
   `https://nolune.dev/privacy` (`site/src/routes/privacy`), which the first screen links too, and
   under App Privacy: Data Not Collected (what people write goes to their own nolune, and the
   relay passes notifications on without keeping them).

For each version:

1. Set the version (`MARKETING_VERSION`, 1.0 to start) and raise the build number
   (`CURRENT_PROJECT_VERSION`) in the target's General tab. The app's version is its own; the
   gateway's releases don't need a new one.
2. Product > Archive, then in the Organizer: Distribute App > App Store Connect. Xcode signs it
   for distribution and turns notifications to production. `Info.plist` says the app uses only
   standard encryption, so App Store Connect doesn't ask.
3. Try it from TestFlight, then submit it for review. **Reviewers need a nolune to open**: in App
   Review Information, give the address of one that's up during the review, an account on it, and
   a line on what nolune is (a family's own assistant, which runs on their computer; the app opens
   it). An automation that runs every few minutes shows them the notifications.
