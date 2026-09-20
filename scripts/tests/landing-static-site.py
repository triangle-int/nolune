#!/usr/bin/env python3
"""The built landing site is static and every internal link resolves (#32).

Runs after `pnpm --dir landing build` and inspects landing/.vercel/output:

- no application route is a serverless function; the only entry the adapter
  may leave under functions/ is its internal `![-]` 404 catch-all, which
  adapter-vercel always emits so SvelteKit renders the error page for unknown
  paths, and config.json may route nothing else to a function;
- every page and both installer scripts are prerendered files, including the
  self-hosting docs index and its six sections;
- /install.sh and /uninstall.sh are byte-for-byte the repository scripts at the
  built commit, and landing/vercel.json serves them as text/plain (Vercel
  otherwise derives application/x-sh from the extension for a prerendered
  file, so a browser visit downloads the script instead of showing it);
- landing/vercel.json rewrites nothing off-site: every route is built here;
- the built /docs page names every section a self-hoster needs;
- every internal href, src and hash anchor in the prerendered HTML points at a
  prerendered file, and a hash names an id on the page it targets.
"""
from html.parser import HTMLParser
from pathlib import Path
import json
import posixpath
import sys
from urllib.parse import urlsplit

root = Path(__file__).resolve().parents[2]
output = root / "landing" / ".vercel" / "output"
static = output / "static"
functions = output / "functions"
failures: list[str] = []

if not static.is_dir():
    print(
        f"{static} is missing: run `pnpm --dir landing build` first",
        file=sys.stderr,
    )
    sys.exit(2)

# The adapter's private namespace; guaranteed not to collide with a route.
INTERNAL = "![-]"
CATCHALL = f"/{INTERNAL}/catchall"

# --- no application route runs as a function -------------------------------

if functions.is_dir():
    for entry in sorted(functions.iterdir()):
        if entry.name != INTERNAL:
            failures.append(f"route is a serverless function: functions/{entry.name}")
    internal = functions / INTERNAL
    if internal.is_dir():
        for entry in sorted(internal.iterdir()):
            if entry.name != "catchall.func":
                failures.append(f"unexpected function: functions/{INTERNAL}/{entry.name}")

config = json.loads((output / "config.json").read_text())
for route in config.get("routes", []):
    dest = route.get("dest", "")
    if dest.startswith(f"/{INTERNAL}/") and (dest != CATCHALL or route.get("src") != "/.*"):
        failures.append(f"config.json routes {route.get('src')!r} to a function: {dest}")

# --- required prerendered files --------------------------------------------


def resolve(path: str) -> Path | None:
    """Map a site path to the prerendered file Vercel would serve, if any."""
    relative = path.lstrip("/")
    if relative == "":
        relative = "index.html"
    candidates = [
        static / relative,
        static / f"{relative}.html",
        static / relative / "index.html",
    ]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


DOCS_SECTIONS = ["prerequisites", "install", "upgrade", "backup", "uninstall", "troubleshooting"]
DOCS_PAGES = ["/docs"] + [f"/docs/{section}" for section in DOCS_SECTIONS]

for page in ["/", "/skills", "/privacy", "/terms", "/install.sh", "/uninstall.sh", *DOCS_PAGES]:
    if resolve(page) is None:
        failures.append(f"{page} is not prerendered")

for served, script in [("/install.sh", "scripts/install.sh"), ("/uninstall.sh", "scripts/uninstall.sh")]:
    built = resolve(served)
    if built is not None and built.read_bytes() != (root / script).read_bytes():
        failures.append(f"{served} differs from {script}")

# --- vercel.json: scripts are text/plain, nothing is rewritten off-site ------

vercel = json.loads((root / "landing" / "vercel.json").read_text())

for rewrite in vercel.get("rewrites", []):
    destination = rewrite.get("destination", "")
    if urlsplit(destination).netloc:
        failures.append(f"vercel.json rewrites {rewrite.get('source')!r} off-site to {destination}")


def content_type(path: str) -> str | None:
    """The Content-Type header vercel.json sets for exactly this path, if any."""
    for rule in vercel.get("headers", []):
        if rule.get("source") != path:
            continue
        for header in rule.get("headers", []):
            if header.get("key", "").lower() == "content-type":
                return header.get("value")
    return None


for served in ["/install.sh", "/uninstall.sh"]:
    if content_type(served) != "text/plain; charset=utf-8":
        failures.append(f"vercel.json must serve {served} as text/plain; charset=utf-8")

# --- the docs index covers the self-hosting lifecycle ----------------------

docs = resolve("/docs")
if docs is not None:
    text = docs.read_text().lower()
    for word in ["prerequisites", "install", "backup", "uninstall", "troubleshooting"]:
        if word not in text:
            failures.append(f"/docs does not mention {word!r}")
    if "update" not in text and "upgrade" not in text:
        failures.append("/docs does not mention 'update' or 'upgrade'")

# --- link check ------------------------------------------------------------


class Links(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.ids: set[str] = set()
        self.refs: list[tuple[str, str]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        for name, value in attrs:
            if value is None:
                continue
            if name == "id":
                self.ids.add(value)
            elif name in ("href", "src"):
                self.refs.append((name, value))


pages: dict[Path, Links] = {}
for html in sorted(static.rglob("*.html")):
    parser = Links()
    parser.feed(html.read_text())
    pages[html] = parser

for html, parsed in pages.items():
    page = html.relative_to(static).as_posix()
    for attr, value in parsed.refs:
        parts = urlsplit(value)
        if parts.scheme or parts.netloc or value.startswith(("data:", "#!")):
            continue
        if parts.path == "":
            target_ids = parsed.ids
            target = page
        else:
            # SvelteKit writes `./_app/...` relative to the page's URL, which
            # the static tree mirrors, so resolve against the file's directory.
            path = parts.path
            if not path.startswith("/"):
                path = posixpath.normpath(posixpath.join("/" + posixpath.dirname(page), path))
            resolved = resolve(path)
            if resolved is None:
                failures.append(f"{page}: {attr}={value!r} does not resolve")
                continue
            target = resolved.relative_to(static).as_posix()
            target_ids = pages[resolved].ids if resolved in pages else set()
        if parts.fragment and parts.fragment not in target_ids:
            failures.append(f"{page}: {attr}={value!r}: no id {parts.fragment!r} on {target}")

if failures:
    print("\n".join(failures), file=sys.stderr)
    sys.exit(1)
print(
    f"landing is static: {len(pages)} prerendered pages, no route functions, all internal links resolve."
)
