---
title: Memory
description: What nolune remembers about the family, how it finds it, and how to change it.
---

nolune keeps what it learns about the family (preferences, who's who, where things are) in small
notes. Files: `~/.nolune/profiles/<profile>/memories`.

![Editable family memory in nolune](../../../../screenshots/memory.png)

## Notes and categories

Notes are in the same categories in every profile: core, people (a note per person, in the family
or not), home, health, plans, routines, pets, places, projects and other. Each member has their
note under people, so nolune knows who "I" is. When you add someone memory may know already
(grandma, before she got an account), you're asked which note is theirs.

The pinned `core` note (who's who, languages, allergies, anything you want it to always keep in
mind) is in every chat from the start, so keep it short: at most 4,000 characters.

## How nolune finds facts

Each message comes with the facts from memory that match it, and nolune searches for more when a
request needs them (`nolune memory search wifi`).

With an OpenAI or OpenRouter key, it also finds facts by meaning ("where's the other key for the
car?" finds the spare key, a question in Russian finds notes in English): each fact is embedded
once with that provider's `text-embedding-3-small`. An admin can turn that off, or use a model of
one of your servers instead (Ollama, LM Studio, oMLX on your computer), under Memory search in
Models & keys or with `nolune config set embeddings custom-openai/local/nomic-embed-text`.

## Learning from chats

Besides what nolune saves as it goes, it looks over each chat once it has been quiet for a couple
of minutes and saves what it missed, with one short request to the chat's model. Turn that off on
the Memory page (**Learn from chats**) or with `nolune memory learning off`.

What it saves shows in the chat ("Saved 3 memories") and at the top of the Memory page, each with
Undo.

## The Memory page

The Memory page shows every fact as a dot, darker the newer it is, and lets you fix, move, merge or
delete a note. From the terminal, `nolune memory merge people/grandma people/olga` puts two notes
about one person together; [Commands](/docs/reference/commands/#memory) lists the rest.
