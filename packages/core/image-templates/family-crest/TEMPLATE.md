---
name: Family crest
title: Give your family a coat of arms
sentence: 'Design a {{style}} family crest for the {{surname}} family{{#symbols}} with {{symbols}}{{/symbols}}{{#motto}} and the motto "{{motto}}"{{/motto}}.'
description: A coat of arms built from your surname, your pets and what you love doing together.
order: 12
icon: shield
color: '#c9b8e8'
image: none
size: square
settings:
  - id: style
    label: Style
    options:
      - label: classic
        prompt: classic medieval heraldry, with rich reds, blues and gold, fine engraved detail and flowing mantling
      - label: modern
        prompt: a modern flat vector crest in two or three colors, clean and minimal
      - label: cartoon
        prompt: a playful cartoon crest with bold outlines, bright colors and a wink of humor
  - id: surname
    label: Surname
    required: true
    placeholder: Turatbekov
  - id: symbols
    label: Symbols
    placeholder: a cat, bicycles and pancakes
  - id: motto
    label: Motto
    placeholder: Always one more episode
---

A coat of arms: a shield with the family's symbols arranged in its fields, a helmet or crown on top and a banner below.
Style: {{style}}.
{{#symbols}}The symbols are {{symbols}}, drawn as heraldic charges.{{/symbols}}
The banner says "{{surname}}"{{#motto}}, with "{{motto}}" beneath it{{/motto}}, spelled exactly as written.
Centered, on a transparent background. No other text.
