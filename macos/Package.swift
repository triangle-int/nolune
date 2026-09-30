// swift-tools-version: 5.9
import PackageDescription

// The macOS app: a first-run onboarding, the gateway's supervisor and a menu bar extra, in one
// executable. `scripts/build-app.sh` wraps it, Node and the npm package into nolune.app.
let package = Package(
	name: "Nolune",
	platforms: [.macOS(.v13)],
	targets: [
		.executableTarget(
			name: "Nolune",
			path: "Sources/Nolune"
		)
	]
)
