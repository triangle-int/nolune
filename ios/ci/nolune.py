"""
A pretend family's nolune for the iOS app's screens in CI (.github/workflows/ios.yml): the JSON
and event streams the native screens read (DESIGN.md, "The API for apps"), with one profile, a few
chats, the bell, and a chat with a reply that ran a command. Signed out, it only says which nolune
it is, for the sign-in. Run with the runner's own python3: `python3 nolune.py <port> [signed-in]`.
A profile's pages answer as a real nolune did (fixtures/, from packages/web's endpoints).
"""

import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SIGNED_IN = len(sys.argv) > 2 and sys.argv[2] == "signed-in"
FIXTURES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")


def fixture(name):
    with open(os.path.join(FIXTURES, name + ".json"), encoding="utf-8") as f:
        return json.load(f)

NOW = int(time.time() * 1000)
MINUTE = 60_000

ICONS = {
    "cloud-rain": [["path", {"d": "M4 14.899A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.5 8.242"}], ["path", {"d": "M16 14v6"}], ["path", {"d": "M8 14v6"}], ["path", {"d": "M12 16v6"}]],
    "luggage": [["path", {"d": "M6 20a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2"}], ["path", {"d": "M8 18V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v14"}], ["path", {"d": "M10 20h4"}], ["circle", {"cx": "16", "cy": "20", "r": "2"}], ["circle", {"cx": "8", "cy": "20", "r": "2"}]],
    "bell": [["path", {"d": "M10.268 21a2 2 0 0 0 3.464 0"}], ["path", {"d": "M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"}]],
    "calendar": [["path", {"d": "M8 2v3"}], ["path", {"d": "M16 2v3"}], ["rect", {"x": "3", "y": "3", "width": "18", "height": "18", "rx": "2"}], ["path", {"d": "M3 9h18"}]],
    "search": [["path", {"d": "m21 21-4.34-4.34"}], ["circle", {"cx": "11", "cy": "11", "r": "8"}]],
}

ME = {"id": "anna", "name": "Anna Smith", "email": "anna@example.com", "isAdmin": True, "picture": None}
PROFILE = {
    "slug": "smiths",
    "name": "Smiths",
    "avatar": "moon",
    "members": [{"id": "anna", "name": "Anna Smith", "picture": None}, {"id": "max", "name": "Max Smith", "picture": None}],
}


def chat(id, title, minutes, folder=None, running=False):
    return {"id": id, "title": title, "presetName": "Sonnet", "folderId": folder, "updatedAt": NOW - minutes * MINUTE, "running": running}


CHATS = [
    chat("trip", "Packing for Kyoto", 2, folder="japan", running=True),
    chat("rain", "Will it rain tomorrow?", 5),
    chat("groceries", "Weekly groceries", 60),
    chat("printer", "Fix the printer", 60 * 26),
    chat("birthday", "Max's birthday party", 60 * 50),
]

BELL = {
    "items": [
        {"id": "n2", "title": "Umbrellas tomorrow", "body": "Rain from **3pm** in Berlin, 12°C.", "level": "info", "createdAt": NOW - 3 * MINUTE, "profile": {"slug": "smiths", "name": "Smiths", "avatar": "moon"}, "conversationId": None},
        {"id": "n1", "title": "Backup finished", "body": "Photos are backed up to the external drive.", "level": "info", "createdAt": NOW - 90 * MINUTE, "profile": {"slug": "smiths", "name": "Smiths", "avatar": "moon"}, "conversationId": "printer"},
    ],
    "seenAt": NOW - 60 * MINUTE,
}

OPTIONS = {
    "suggestions": [
        {"icon": ICONS["bell"], "label": "Set a reminder", "text": "Remind me tomorrow at 9:00 to "},
        {"icon": ICONS["cloud-rain"], "label": "Daily weather check", "text": "Every weekday at 7:30, check the weather and tell us if we need umbrellas."},
        {"icon": ICONS["search"], "label": "Find a file", "text": "Find the file on this computer called "},
    ],
    "suggestionsStale": False,
    "presets": [{"id": "sonnet", "name": "Sonnet", "provider": "anthropic"}, {"id": "opus", "name": "Opus", "provider": "anthropic"}],
    "defaultPresetId": "sonnet",
    "efforts": ["low", "medium", "high", "xhigh", "max"],
    "commandMode": "auto",
}

REPLY = """Yes, rain from **3pm**, so pack an umbrella. Here's the list for the trip:

### Clothes
- [x] Rain jacket
- [ ] Two sweaters
- [ ] Comfortable shoes for the temples

| Day | Weather | High |
|:----|:--------|-----:|
| Friday | Rain from 3pm | 14°C |
| Saturday | Cloudy | 16°C |
| Sunday | Sunny | 19°C |

To check again later:

```sh
curl wttr.in/Kyoto?format=3
```

More on [the forecast](https://wttr.in/Kyoto)."""

ENTRIES = [
    {"type": "human", "key": "m1", "message": {"id": 1, "kind": "human", "senderId": "anna", "senderName": "Anna Smith", "text": "Will it rain in Kyoto this weekend? Make me a packing list.", "attachments": [], "queued": False, "createdAt": NOW - 6 * MINUTE}},
    {
        "type": "reply",
        "key": "reply-1",
        "parts": [
            {"type": "activity", "key": "reply-1-0", "steps": [
                {"type": "thinking", "text": "Check the forecast for Kyoto first."},
                {"type": "command", "id": "t1", "command": "curl -s 'wttr.in/Kyoto?format=j1'", "cwd": "/Users/anna", "summary": "Checking the forecast for Kyoto", "icon": "cloud-rain"},
            ], "startedAt": NOW - 6 * MINUTE, "endedAt": NOW - 6 * MINUTE + 12_000},
            {"type": "text", "key": "reply-1-1", "text": REPLY, "media": {}},
        ],
        "messageIds": [2, 4],
        "usage": {"input": 1200, "cacheRead": 14000, "cacheWrite": 800, "output": 420},
        "stopReasons": ["tool_use", "end_turn"],
        "models": ["claude-sonnet-5"],
        "live": False,
    },
    {"type": "memory", "key": "memory-4", "look": {"after": 4, "createdAt": NOW - 5 * MINUTE, "changes": [{"id": 1, "op": "add", "note": "people/anna.md", "fact": "Anna is going to Kyoto this weekend.", "before": None, "createdAt": NOW - 5 * MINUTE, "undone": None, "card": None}]}},
    {"type": "human", "key": "m5", "message": {"id": 5, "kind": "human", "senderId": "max", "senderName": "Max Smith", "text": "Can you add the train times too?", "attachments": [], "queued": False, "createdAt": NOW - 2 * MINUTE}},
    {"type": "reply", "key": "reply-5", "parts": [
        {"type": "activity", "key": "reply-5-0", "steps": [{"type": "command", "id": "t2", "command": "open https://www.jr-odekake.net", "summary": "Looking up the trains to Kyoto", "icon": "search"}], "startedAt": NOW - MINUTE, "endedAt": NOW},
    ], "messageIds": [], "usage": None, "stopReasons": [], "models": [], "live": True},
]

SNAPSHOT = {
    "type": "snapshot",
    "snapshot": {
        "title": "Packing for Kyoto",
        "model": {"presetId": "sonnet", "presetName": "Sonnet", "provider": "anthropic", "effort": "medium", "contextWindow": 200000},
        "commands": {"mode": None, "fallback": "auto"},
        "toolChanges": None,
        "running": True,
        "error": None,
        "queued": [],
        "toolOutput": None,
        "background": [],
        "typing": [{"id": "max", "name": "Max Smith"}],
        "entries": ENTRIES,
        "results": {"t1": {"id": "t1", "output": "Friday: rain from 15:00, 14°C\nSaturday: cloudy, 16°C\nSunday: sunny, 19°C", "isError": False, "pictures": []}},
    },
}


class Nolune(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        sys.stderr.write("%s %s\n" % (self.command, self.path))

    def answer(self, body, status=200):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def stream(self, *events):
        """Server-sent events: these, then a comment now and then until the app goes."""
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        try:
            for event in events:
                self.wfile.write(("data: %s\n\n" % json.dumps(event)).encode())
            self.wfile.flush()
            while True:
                time.sleep(15)
                self.wfile.write(b": ping\n\n")
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/api/version":
            server = {"version": "0.6.0", "api": 1}
            if SIGNED_IN:
                server["capabilities"] = [
                    "chats", "notifications", "transcript", "memory", "automations",
                    "skills", "profile", "folders", "images", "activities",
                ]
            return self.answer(server)
        if not SIGNED_IN:
            return self.answer({"message": "Not signed in"}, 401)
        if path == "/api/me":
            return self.answer(ME)
        if path == "/api/profiles":
            return self.answer({"profiles": [PROFILE]})
        if path == "/api/notifications":
            return self.answer(BELL)
        if path == "/api/p/smiths/chats":
            return self.answer({"chats": CHATS, "next": None})
        if path == "/api/p/smiths/folders":
            return self.answer({"folders": [{"id": "japan", "name": "Trip to Japan"}]})
        if path == "/api/p/smiths/new-chat":
            return self.answer(OPTIONS)
        pages = {
            "/api/p/smiths/memory": "memory",
            "/api/p/smiths/automations": "automations",
            "/api/p/smiths/skills": "skills",
            "/api/p/smiths/settings": "settings",
            "/api/p/smiths/images": "images",
            "/api/p/smiths/folders/japan": "folder",
        }
        if path in pages:
            return self.answer(fixture(pages[path]))
        if path.startswith("/api/icons/"):
            icon = ICONS.get(path[len("/api/icons/"):])
            return self.answer(icon) if icon else self.answer({"message": "Unknown icon"}, 404)
        if path == "/api/events":
            return self.stream()
        if path == "/api/p/smiths/running":
            return self.stream({"running": ["trip"]})
        if path.startswith("/api/c/") and path.endswith("/transcript"):
            return self.stream(SNAPSHOT)
        return self.answer({"message": "Not found"}, 404)

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        return self.answer({"ok": True})

    do_PATCH = do_POST
    do_DELETE = do_POST


if __name__ == "__main__":
    ThreadingHTTPServer.daemon_threads = True
    ThreadingHTTPServer(("", int(sys.argv[1])), Nolune).serve_forever()
