---
name: subagents
description: Hand a task to a subagent, another btw that works on it in the background in a conversation of its own and reports back, so you can do other things meanwhile or run several jobs at once. Use for big, independent jobs that would fill this conversation with reading (research across many pages, going through lots of files, emails or photos) and for long jobs you don't want to wait on.
---

# Subagents

`btw agent run` starts a subagent: another btw on this computer, with the same skills and memory,
working in a hidden conversation of its own. It starts with nothing but your prompt; it can't see
this conversation. When it's done, its last message is its result, and only that comes back here,
so the reading it did doesn't fill up this conversation.

## When to use one

- A task that needs a lot of reading or searching when you only need the conclusion: compare
  prices across sites, find every invoice from last year, summarize a folder of documents.
- Several independent jobs at once: start one subagent per job.
- A long job you don't want to sit in: start it and keep talking with the family.

Don't use one for what you can do in a few commands, or to check your own work: starting one and
reading its result has a cost of its own. If you delegate a job, don't redo it once the result
comes back.

## Starting one

Write the prompt so that someone who knows nothing about this conversation can do the job: the
goal, what you already know, file paths, who it's for, limits (don't buy anything, don't email
anyone), and what the result should contain. Pass long prompts on stdin with a quoted heredoc, so
quotes and `$` in them can't break the command:

```sh
btw agent run --prompt - <<'EOF'
Find the three cheapest direct flights from Berlin to Lisbon on 12-14 October for two adults.
Anna prefers mornings. Don't book anything. Report airline, times, price per person and a link
for each.
EOF
```

It prints the subagent's id (`agent-1`, or the one you give: `btw agent run flights --prompt ...`),
its model and reasoning level, and the path of its log.

## Model and reasoning

A subagent runs on this chat's model and reasoning level unless you choose others:

- `--preset <name>` picks one of the model presets this computer has. Run `btw preset list` first:
  it shows each preset's name, model and context window, and which is the default. Pass a name
  from that list exactly as it's written there; never make one up from a model you know of. A
  smaller, faster model often does well on simple jobs with a lot of reading (going through
  files, collecting prices from pages); keep this chat's model for jobs that need judgment.
- `--effort low|medium|high|xhigh|max` sets how hard it thinks. `low` suits simple, clearly
  described jobs and costs less; raise it for hard ones.

```sh
btw preset list
btw agent run invoices --preset "Haiku" --effort low --prompt - <<'EOF'
...
EOF
```

## Hearing back

Right after starting it, run `btw agent watch <id>` with `run_in_background: true`. Then keep
working, or end your turn and tell the person you'll get back to them. When the subagent is done,
a message starting with "[Background command finished" arrives with its last message, and you
carry on from there. Don't poll or sleep while you wait.

A background command is killed after an hour unless you pass `timeout_seconds` (up to 86400). If
`watch` was cut off, the subagent keeps working: run `watch` again.

## While it works

- `tail -n 40 <log>` shows what it has said and run so far (never its reasoning), when someone
  asks how it's going or you wonder whether it's on track.
- `btw agent steer <id> --prompt "..."` sends it a message it reads at its next step: a
  correction, or something new you learned. `--prompt -` reads stdin, like `run`.
- `btw agent stop <id>` stops it. `btw agent list` shows this conversation's subagents.

## Afterwards

A subagent that finished keeps its conversation. `btw agent run <id> --prompt "..."` gives it more
work that builds on what it did (then `watch` it again); a new id starts from nothing. It keeps
its model for good, so leave out `--preset` then (a different one is refused: start a new
subagent for another model). `--effort` can change, but then its next step rereads its whole
conversation without the cache once.

## Limits

- 5 subagents working at once per conversation.
- Subagents can't start subagents of their own.
- When the person presses Stop, the subagents of this conversation stop too.
