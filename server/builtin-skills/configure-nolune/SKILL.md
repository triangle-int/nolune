---
name: configure-nolune
description: Show or change your own settings (the user's timezone, your display name, API keys, the GitHub token, email accounts) with the `nolune config` command.
---

# Configuring Nolune

Your settings are read and written by the `nolune config` command. Run it with
run_command: your shell already points `nolune` and `$NOLUNE_HOME` at the
server you run under, so never pass `--profile` and never edit the config
files by hand. Changes take effect on the next message.

## See the settings

```sh
nolune config show
```

It prints the data root, your name, the timezone, the chat and background
models, which API keys are set, GitHub, extensions (MCP servers), and email
accounts. Secret values are never printed, only whether they are set.

## Timezone and name

```sh
nolune config set timezone Asia/Bishkek   # an IANA name
nolune config set name Luna
nolune config unset timezone              # back to UTC
```

The shell clock follows the timezone: `date` answers in the user's time.

## Secrets: API keys and the GitHub token

Never ask the user to paste a secret in chat, and never put one on a command
line: commands are logged and shown in the chat. `nolune config set` reads a
secret from stdin only. Ask for it with `request_secret`, which lets the user
type it privately into a file you never see, then feed that file to the
command and delete it in the same run:

1. `request_secret` with `file` = `tmp/secret` and no `key`.
2. run_command:

   ```sh
   nolune config set anthropic-key < "$NOLUNE_HOME/tmp/secret"; rm -f "$NOLUNE_HOME/tmp/secret"
   ```

The secret keys are `openai-key`, `anthropic-key`, `brave-search-key`, and
`github-token`. `nolune config unset <key>` clears one.

## Email accounts

Ask for the password the same way (`request_secret`, `file` = `tmp/secret`),
then add the account with its non-secret details as flags:

```sh
nolune config email add --smtp-host smtp.example.com --smtp-user me@example.com \
  --from me@example.com --imap-host imap.example.com \
  < "$NOLUNE_HOME/tmp/secret"; rm -f "$NOLUNE_HOME/tmp/secret"
```

`--smtp-port` defaults to 587, `--imap-port` to 993, and `--imap-user` to the
SMTP user; leave out `--imap-host` for a send-only account. When IMAP needs a
different password, request it to `tmp/imap-secret` and pipe both, one per
line:

```sh
{ cat "$NOLUNE_HOME/tmp/secret"; echo; cat "$NOLUNE_HOME/tmp/imap-secret"; } \
  | nolune config email add …; rm -f "$NOLUNE_HOME/tmp/secret" "$NOLUNE_HOME/tmp/imap-secret"
```

Remove one with `nolune config email remove me@example.com`.

## Everything else

Models, providers, extensions (MCP servers), and computer use are set in the
Settings page; point the user there. `nolune --help` lists the other
commands.
