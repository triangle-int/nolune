# Nolune cutover record

The product is Nolune and its domain is `nolune.dev`. The project was
renamed from Bolly (`triangle-int/bolly`, `bollyai.dev`) under #68; the code
rename landed in #69. There were no existing installations, so the rename
ships no Bolly configuration aliases, data migration, or desktop upgrade
compatibility. `server/tests/nolune_rename.rs` fails CI if a new reference to
the old name or its configuration variables appears outside the remnants it
lists.

## Completed

### Repository and code

- The GitHub repository is `triangle-int/nolune`. The installer, the server
  update check, the desktop updater endpoint, and the landing `/install.sh`
  and `/uninstall.sh` routes all target it.
- The server binary is `nolune`, its data directory is `~/.nolune`, and the
  background service is the launchd label `dev.nolune.nolune` or the systemd
  user unit `nolune.service`. Configuration variables are `NOLUNE_HOME`,
  `NOLUNE_AUTH_TOKEN` and `NOLUNE_PUBLIC_URL`; there are no third-party
  sign-in origins or callback URLs to update because the product has none.
- The desktop bundle is `Nolune` with identifier
  `com.triangle-int.nolune-desktop`. The updater signing key was kept.
- README, CONTRIBUTING, SECURITY, the landing pages, onboarding copy and the
  default companion label say Nolune and link to `nolune.dev`.

### Domain and hosting (verified 2026-09-20)

- `nolune.dev` and `www.nolune.dev` are attached to the Vercel project and
  verified. The apex answers 308 to `https://www.nolune.dev/`; the homepage,
  `/install.sh` and `/uninstall.sh` return 200 on `www.nolune.dev`.
- `bollyai.dev` and `www.bollyai.dev` answer 308 to `https://www.nolune.dev`,
  preserving paths and query strings.
- Vercel keeps deploying `main` after the repository rename: the production
  deployment of `576b01f` reports `githubOrg=triangle-int`,
  `githubRepo=nolune` and state `READY`.

### Releases

- v0.34.0 was the first Nolune release; v0.36.0 is current. `latest.json`
  lists signed `darwin-aarch64`, `linux-x86_64` and `windows-x86_64`
  entries, and the server assets are named `nolune-server-<target>`.
- The macOS Apple Silicon server artifact was verified end to end:
  `nolune --version`, a fresh data directory, `/healthz`, the web UI and
  authenticated `/api/meta`.

## Outstanding external work

None of this is code in this repository; it is the reason #68 stays open.

- Mailboxes. `support@nolune.dev` and `security@nolune.dev` are published in
  SECURITY.md and on the landing privacy and terms pages, but `nolune.dev`
  has no MX record and no SPF TXT record (dns.google, 2026-09-20), so mail
  to either address bounces. Pick a mail provider, add its MX, SPF and DKIM
  records at Cloudflare, and send a test message to both addresses.
- Docs. `landing/vercel.json` rewrites `/docs` to the Mintlify site built
  from the separate `triangle-int/docs` repository, which still titles
  itself "Bolly Docs" and links to `bollyai.dev` (last pushed 2026-04-04).
  Nothing in the landing app or README links to `/docs` today. #32 replaces
  the rewrite with in-repo docs in the landing app; rename the Mintlify
  content only if #32 slips.
- Vercel project name. The project is still called `bollyai`, which only
  shows in `*.vercel.app` preview hostnames and the inspector URL. Rename it
  in the Vercel project settings when convenient.
- Manual smoke tests on Linux and Windows: fresh install via `/install.sh`,
  `nolune onboard`, `nolune gateway install`, update, `nolune uninstall`, and
  desktop pairing with a local server. Only the macOS Apple Silicon server
  artifact has been exercised.

## External skills registry

The separate `triangle-int/bolly-skills` repository keeps its current URLs;
`server/src/config.rs` and the landing skills page point at it on purpose.
Renaming that repository is a separate operation, and changing its URLs here
first would break skill discovery.
