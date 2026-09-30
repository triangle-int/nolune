#!/usr/bin/env bash
# Builds nolune.app and a DMG for one architecture: the Swift launcher (macos/), Node, and the npm
# package with its production dependencies. Signed with a Developer ID and notarized when given
# one; ad-hoc signed otherwise, which runs on the Mac that built it.
#
#   macos/scripts/build-app.sh
#
# Environment:
#   ARCH              arm64 or x86_64 (default: this Mac's)
#   NODE_VERSION      the Node to bundle, e.g. 24.11.1 (default: the newest 24.x)
#   SIGN_IDENTITY     "Developer ID Application: Your Name (TEAMID)"
#   NOTARY_PROFILE    a notarytool keychain profile (`xcrun notarytool store-credentials`); with
#                     SIGN_IDENTITY, the app and the DMG are notarized and stapled
#   SKIP_WEB_BUILD=1  use build/ and dist/ from an earlier `pnpm build`
#
# Out: macos/dist/<arch>/nolune.app and macos/dist/nolune-<version>-<arch>.dmg
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MACOS="$ROOT/macos"
ARCH="${ARCH:-$(uname -m)}"
case "$ARCH" in
	arm64) NODE_ARCH=arm64 ;;
	x86_64) NODE_ARCH=x64 ;;
	*)
		echo "ARCH must be arm64 or x86_64, not $ARCH" >&2
		exit 1
		;;
esac
IDENTITY="${SIGN_IDENTITY:--}"
VERSION="$(node -p "require('$ROOT/package.json').version")"
BUILD="$(git -C "$ROOT" rev-list --count HEAD 2>/dev/null || echo 1)"
OUT="$MACOS/dist"
WORK="$OUT/work-$ARCH"
APP="$OUT/$ARCH/nolune.app"
DMG="$OUT/nolune-$VERSION-$ARCH.dmg"

step() { printf '\n==> %s\n' "$*"; }

# Checked first: signing comes after a few minutes of building.
if [ "$IDENTITY" != - ] && ! security find-identity -v -p codesigning | grep -qF -- "$IDENTITY"; then
	echo "No signing identity \"$IDENTITY\" in the keychain. It has:" >&2
	security find-identity -v -p codesigning >&2
	echo "Make a Developer ID Application certificate in Xcode (Settings > Accounts > Manage" >&2
	echo "Certificates), or leave SIGN_IDENTITY out for an ad-hoc build." >&2
	exit 1
fi

rm -rf "$WORK" "$OUT/$ARCH" "$DMG"
mkdir -p "$WORK" "$OUT/$ARCH"

# The web app (build/) and the CLI (dist/), packed as npm would publish them.
if [ "${SKIP_WEB_BUILD:-}" != 1 ]; then
	step "Building nolune $VERSION"
	(cd "$ROOT" && pnpm install --frozen-lockfile && pnpm build)
fi
step "Packing"
(cd "$ROOT" && npm pack --ignore-scripts --pack-destination "$WORK" >/dev/null)
TARBALL="$WORK/nolune-$VERSION.tgz"

# Node, checked against its published checksums.
if [ -z "${NODE_VERSION:-}" ]; then
	NODE_VERSION="$(curl -fsSL https://nodejs.org/dist/index.json | node -e '
		let json = "";
		process.stdin.on("data", (d) => (json += d)).on("end", () => {
			console.log(JSON.parse(json).find((r) => r.version.startsWith("v24.")).version.slice(1));
		});
	')"
fi
NODE_DIR="node-v$NODE_VERSION-darwin-$NODE_ARCH"
step "Node $NODE_VERSION ($NODE_ARCH)"
curl -fsSL -o "$WORK/$NODE_DIR.tar.gz" "https://nodejs.org/dist/v$NODE_VERSION/$NODE_DIR.tar.gz"
curl -fsSL -o "$WORK/SHASUMS256.txt" "https://nodejs.org/dist/v$NODE_VERSION/SHASUMS256.txt"
(cd "$WORK" && grep " $NODE_DIR.tar.gz\$" SHASUMS256.txt | shasum -a 256 -c -)
tar -xzf "$WORK/$NODE_DIR.tar.gz" -C "$WORK"
NODE_BIN="$WORK/$NODE_DIR/bin"

# The package and its production dependencies, installed with the bundled Node's own npm.
step "Installing the package"
mkdir -p "$WORK/app"
echo '{ "private": true }' >"$WORK/app/package.json"
(
	cd "$WORK/app"
	PATH="$NODE_BIN:$PATH" "$NODE_BIN/node" "$NODE_BIN/../lib/node_modules/npm/bin/npm-cli.js" install \
		--omit=dev --ignore-scripts --no-audit --no-fund --no-package-lock --os=darwin --cpu="$NODE_ARCH" "$TARBALL"
)
# better-sqlite3 ships every platform's addon and SQLite's source: keep this Mac's addon.
SQLITE="$WORK/app/node_modules/better-sqlite3"
rm -rf "$SQLITE/deps" "$SQLITE/src"
find "$SQLITE/prebuilds" -type f ! -name "darwin-$NODE_ARCH.node" -delete
test -f "$WORK/app/node_modules/nolune/dist/cli.js"

step "Building the launcher"
swift build -c release --package-path "$MACOS" --arch "$ARCH"
LAUNCHER="$(swift build -c release --package-path "$MACOS" --arch "$ARCH" --show-bin-path)/Nolune"

step "Assembling $APP"
CONTENTS="$APP/Contents"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"
sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" "$MACOS/Resources/Info.plist" >"$CONTENTS/Info.plist"
cp "$LAUNCHER" "$CONTENTS/MacOS/Nolune"
cp "$NODE_BIN/node" "$CONTENTS/MacOS/node"
cp "$MACOS/Resources/Fonts/Figtree.ttf" "$CONTENTS/Resources/"
cp "$MACOS/Resources/Fonts/OFL.txt" "$CONTENTS/Resources/Figtree-OFL.txt"
cp "$WORK/$NODE_DIR/LICENSE" "$CONTENTS/Resources/Node-LICENSE.txt"
SOUNDS="$ROOT/packages/web/src/lib/assets/sounds/welcome"
cp "$SOUNDS/music.mp3" "$SOUNDS/confirm.mp3" "$CONTENTS/Resources/"
cp "$MACOS/Resources/Sounds/boom.mp3" "$CONTENTS/Resources/"
mv "$WORK/app" "$CONTENTS/Resources/app"

ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
	sips -z "$size" "$size" "$MACOS/Resources/AppIcon.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
	double=$((size * 2))
	sips -z "$double" "$double" "$MACOS/Resources/AppIcon.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$CONTENTS/Resources/AppIcon.icns"

# Signing, from the inside out: every Mach-O in the package, Node with the entitlements V8 needs,
# then the app, which seals the rest.
sign() {
	if [ "$IDENTITY" = - ]; then
		codesign --force --sign - "$@"
	else
		codesign --force --timestamp --options runtime --sign "$IDENTITY" "$@"
	fi
}
step "Signing ($([ "$IDENTITY" = - ] && echo ad-hoc || echo "$IDENTITY"))"
find "$CONTENTS/Resources/app" -type f -size +4k -print0 | while IFS= read -r -d '' file; do
	if file -b "$file" | grep -q 'Mach-O'; then sign "$file"; fi
done
sign --entitlements "$MACOS/Resources/node.entitlements" "$CONTENTS/MacOS/node"
sign "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

notarize() {
	xcrun notarytool submit "$1" --keychain-profile "$NOTARY_PROFILE" --wait
}
NOTARIZE=0
if [ -n "${NOTARY_PROFILE:-}" ] && [ "$IDENTITY" != - ]; then NOTARIZE=1; fi

if [ "$NOTARIZE" = 1 ]; then
	step "Notarizing the app"
	ditto -c -k --keepParent "$APP" "$WORK/nolune.zip"
	notarize "$WORK/nolune.zip"
	xcrun stapler staple "$APP"
fi

step "Making $DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/nolune.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname nolune -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
if [ "$IDENTITY" != - ]; then codesign --force --timestamp --sign "$IDENTITY" "$DMG"; fi
if [ "$NOTARIZE" = 1 ]; then
	step "Notarizing the DMG"
	notarize "$DMG"
	xcrun stapler staple "$DMG"
	spctl --assess --type execute --verbose "$APP"
fi

step "Done"
echo "  $APP"
echo "  $DMG"
