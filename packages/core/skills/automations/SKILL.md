---
name: automations
description: Do things later, on a schedule, or when something happens (a webhook, new email, a price change) with background triggers whose results reach the family as notifications. Use for reminders, recurring checks and "tell me when ..." requests.
---

# Automations

A trigger runs you in the background, in a new conversation that starts with an
"[Automation ...]" message. Nobody watches it: your final reply there becomes a notification for
everyone in the profile, who can open it to continue the conversation with you. When there is
nothing worth telling them, that run replies only `NO_NOTIFICATION` and stays silent.

Triggers are managed with `nolune trigger` (`nolune trigger help` lists every option). `--profile` can
be left out; it defaults to this profile.

## When (pick one)

- `--cron "<minute hour day month weekday>"` repeats, in this computer's time zone:
  `30 7 * * 1-5` is weekdays at 7:30, `0 */2 * * *` every two hours, `0 9 1 * *` the 1st of each
  month at 9:00.
- `--at "YYYY-MM-DD HH:MM"` or `--in 30m` (m, h or d) runs once. Check the time with `date` first.
- `--webhook` prints a secret URL that other services (Home Assistant, IFTTT, Shortcuts) can POST
  JSON to; the request body is added to the prompt. Give the person the URL and tell them to keep
  it private.

## What (pick one)

- `--prompt "<instructions>"` wakes you. The prompt is all that run gets; it doesn't see this
  conversation. Make it self-contained: who asked, what to check or do, and when there is nothing
  worth reporting.
- `--script "<command>"` runs a shell command on the schedule without you, which costs nothing.
  Use it for frequent checks (new email, a price, a web page changing). The script calls
  `nolune wake "<what happened and what to do>"` only when you are needed; that starts a run of this
  trigger with the message as its prompt (`nolune wake -` reads the message from stdin).

## How it looks to the family

The profile's Automations page shows each trigger's name, its schedule in plain words ("Every
weekday at 07:30") and a calendar of what runs in the coming week. The prompt and script are
hidden behind Edit, so always give:

- `--summary "<one sentence>"`: what it does and for whom, in plain words and the language you
  are speaking with them. No URLs, commands, cron or instructions to yourself: "Tells Timur and
  Polina each weekday morning whether they need umbrellas, with the day's temperatures."
- `--icon <name>`: a [Lucide](https://lucide.dev/icons) icon name that fits, like `umbrella`,
  `bell`, `mail`, `package`, `cake` or `pill`.

## Examples

```sh
nolune trigger add "Umbrella check" --cron "30 7 * * 1-5" --icon umbrella \
  --summary "Tells the family each weekday morning whether they need umbrellas." \
  --prompt "Check today's weather for Berlin. If it will rain, tell the family to take umbrellas. Otherwise there is nothing to report."

nolune trigger add "Parcel" --at "2026-09-26 17:00" --icon package \
  --summary "Reminds Anna to pick up the parcel." \
  --prompt "Remind Anna to pick up the parcel at the post office."

nolune trigger add "School mail" --cron "*/10 * * * *" --icon mail \
  --summary "Watches the inbox and tells you when the school writes." \
  --script "./automations/school-mail.sh"
```

For a script trigger:

1. Write the script to a file in the profile folder, e.g. `automations/school-mail.sh`. Keep the
   state it needs (like the ids it has already seen) in files next to it, so it only wakes you for
   new things.
2. Run it by hand once and check it works before adding the trigger.
3. Scripts run in the profile folder with no keyboard input, with `NOLUNE_PROFILE`,
   `NOLUNE_TRIGGER_ID` and, for webhooks, `NOLUNE_PAYLOAD` set, and time out after 10 minutes. Exit
   non-zero only when the check itself failed: the family gets one notification when a script
   starts failing.

## Managing

- `nolune trigger list`, and `nolune trigger show <name>` for details, recent runs and the last script
  output.
- `nolune trigger run|pause|resume|rm <name>`.
- `nolune trigger edit <name> --prompt ... | --script ... | --cron ... | --at ... | --name ... |
--summary ... | --icon ...`. Triggers made before summaries existed have none; add one when
  you touch them.
- Runs use this conversation's model unless `--preset` is given, with reasoning `medium` unless
  `--effort` is given.
- Members see, edit, run, pause and delete triggers on the profile's Automations page.

When you are done, tell the person what you set up in plain words ("every weekday at 7:30 I'll
check the weather and tell you if you need umbrellas"), not the cron expression.

This skill ships with nolune and is replaced on updates. To adapt it for this profile, copy the folder
into the profile's skills folder and edit the copy; it takes precedence.
