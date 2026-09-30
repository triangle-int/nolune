---
title: Chats and files
description: What the chat shows, attaching files, photos, folders and languages.
---

## What nolune did

The chat keeps what nolune did folded under each reply ("Worked for 12s"), with plain-language
steps. To see the exact commands, token usage and prompt caching, turn on **Show technical
details** in Settings (click your name at the bottom of the sidebar).

## Files and photos

Attach files to a message with the paperclip, by pasting, or by dropping them on the message box.
nolune sees pictures and PDFs, and every file is saved in the profile's `attachments` folder for it
to work with.

nolune can look at photos, screenshots and scans on the computer (with `nolune view`, which converts
HEIC and shrinks big photos for it), and it can show pictures and hand over files in the chat
("show me the beach photos from August", "fill in this form and give it to me"). It keeps its own
copy of each file it shows, so they stay in the chat even if the original moves.

## Folders

Keep related chats together, like projects in ChatGPT: make one with **New folder** in the sidebar,
give it instructions and files on its page, and every chat in it gets them (files as paths on the
computer, which nolune opens when they matter). Start a chat in a folder from its page or the
folder chip in the composer, or drag chats onto a folder in the sidebar. Moving a chat makes its
next reply re-read the conversation once. Files: `~/.nolune/profiles/<profile>/folders`.

## Languages

The web interface comes in English, Russian, German, Spanish and French. It follows the browser's
language, or pick one in Settings (per device). Only menus, buttons and pages change: nolune answers
in whatever language you write in, whatever the setting.
