#!/usr/bin/env python3
"""The README and desktop README describe the install model that shipped (#121, #130).

Required: both install paths, the CLI the installers delegate to, the opt-in
background service, and the update and uninstall stories. Forbidden: copy from
the old flow where the script registered a service and users copied tokens out
of config.toml.
"""
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[2]
readme = (root / "README.md").read_text()
desktop = (root / "desktop" / "README.md").read_text()
landing = (root / "landing" / "src" / "lib" / "components" / "Install.svelte").read_text()
failures: list[str] = []


def require(doc: str, name: str, phrases: list[str]) -> None:
    for phrase in phrases:
        if phrase not in doc:
            failures.append(f"{name}: missing {phrase!r}")


def forbid(doc: str, name: str, phrases: list[str]) -> None:
    for phrase in phrases:
        if phrase in doc:
            failures.append(f"{name}: still contains {phrase!r}")


require(readme, "README.md", [
    "curl -fsSL https://nolune.dev/install.sh | bash",
    "Install on this computer",
    "nolune onboard",
    "nolune gateway",
    "nolune gateway install",
    "nolune gateway uninstall",
    "nolune gateway status",
    "nolune gateway logs",
    "nolune uninstall --keep-data",
    "nolune uninstall --yes",
    "~/.nolune/bin/update",
    "nolune gateway restart",
    "No background service",
])
forbid(readme, "README.md", [
    "sets up the native Nolune server and service",
    "copy the token",
    "Token from config.toml",
])

require(desktop, "desktop/README.md", [
    "Install on this computer",
    "nolune onboard",
    "nolune gateway",
    "Run in background",
    "Show logs",
    "nightly",
])

forbid(landing, "landing Install.svelte", ["Creates the local service"])
require(landing, "landing Install.svelte", ["desktop app"])

# The one-liner must be documented as foreground-first; the service is opt-in.
install_section = readme.split("## Install", 1)[1].split("\n## ", 1)[0]
require(install_section, "README.md Install section", [
    "Desktop app",
    "nolune gateway install",
    "foreground",
])

if failures:
    print("\n".join(failures), file=sys.stderr)
    sys.exit(1)
print("Install-flow docs are consistent with the shipped behaviour.")
