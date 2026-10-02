---
title: Images
description: Make pictures with the family, from templates or your own ideas.
---

The **Images** page in the sidebar has templates for the family (party invitations, storybook
pages, wanted posters, trip postcards, photo-booth strips, sticker packs, bouquets, nail art...) and
for making games (sprite sheets, textures, app icons, concept art). Tap one and take or choose a
photo, draw something, or press **Try it**; many ask for a few choices first.

nolune starts a new chat with the picture and the template's prompt, makes the picture with
`nolune generate image` and shows it there, where you can ask for changes. You can also just
describe a picture, there or in any chat.

## What it needs

An OpenAI or OpenRouter key (Models & keys, or `nolune key set openai`), or the
[nolune plan](/docs/guides/models/#the-nolune-plan). Pictures are made with `gpt-image-2.5-flare`
on the first of them nolune has, keys first, unless you pick another model with
`nolune config set image-model` (`openrouter/black-forest-labs/flux.2-pro`, say).

## Your own templates

Templates are folders with a `TEMPLATE.md`. Add your own in `~/.nolune/image-templates` or a
profile's `image-templates` folder (see
[DESIGN.md](https://github.com/triangle-int/nolune/blob/main/DESIGN.md)).
