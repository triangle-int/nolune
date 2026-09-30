---
title: Memory
description: What nolune remembers about the family, how it finds it, and how to change it.
---

nolune keeps what it learns about the family (preferences, who's who, where things are) in small
notes. Each profile has its own, and each person has a card that goes with them into all their
profiles. Files: `~/.nolune/profiles/<profile>/memories` and `~/.nolune/cards`.

![Editable family memory in nolune](../../../../screenshots/memory.png)

## Notes and categories

Notes are in the same categories in every profile: core, people (a note per person, in the family
or not), home, health, plans, routines, pets, places, projects and other. Each member has their
note under people, so nolune knows who "I" is. When you add someone memory may know already
(grandma, before she got an account), you're asked which note is theirs.

The pinned `core` note (who's who, languages, allergies, anything you want it to always keep in
mind) is in every chat from the start, so keep it short: at most 4,000 characters.

## Your card

If you're in several profiles (your own, the family's, one with a partner, one with friends), you
don't have to tell each of them who you are. What you say about yourself that you'd tell anyone
(the languages you speak, what you eat, your allergies, how you like answers) goes on your card,
which goes with you into every profile you're in. Every chat there starts with it.

Profiles still keep their circles apart: nothing moves from one profile to another by itself.

- Only your own messages put things on your card. What someone else says about you, what you
  share with one circle, and anything you ask nolune to keep there stays in your note in that
  profile. When in doubt, nolune keeps it in the profile.
- Everyone in any of your profiles can read your card, but only you see which profiles those
  are, and only you can change it.
- **Your card**, in the menu under your name, is where you edit it, see what nolune put on it in
  any of your profiles, and undo that, or keep it only in the profile it came from.
- Start it with **Bring in from your notes** there: it shows what your notes in each profile
  already say about you and checks what several of them repeat. What you add moves onto your card.
- A card holds a few things, not everything: at most 2,000 characters.

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
