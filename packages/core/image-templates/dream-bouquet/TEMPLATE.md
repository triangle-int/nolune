---
name: Dream bouquet
title: Arrange a dream bouquet
sentence: 'Arrange {{flowers}}{{#colors}} in {{colors}}{{/colors}} as {{format}}.'
description: Her favorite flowers as a bouquet, a painting or a pressed-flower frame.
order: 9
icon: flower-2
color: '#f9d2c4'
image: optional
image-label: Photo of flowers she likes
size: portrait
settings:
  - id: flowers
    label: Flowers
    required: true
    placeholder: peonies, ranunculus and eucalyptus
  - id: colors
    label: Colors
    placeholder: blush pink and cream
  - id: format
    label: Format
    options:
      - label: a real bouquet
        prompt: a lush hand-tied bouquet wrapped in paper, photographed in soft window light
      - label: a vase on a table
        prompt: an arrangement in a ceramic vase on a sunny kitchen table
      - label: a painting
        prompt: an oil painting of the bouquet in a vase, in the Dutch still-life tradition
      - label: a pressed-flower frame
        prompt: pressed flowers arranged flat and delicate in a glass frame
---

A flower arrangement of {{flowers}}{{#colors}} in shades of {{colors}}{{/colors}}, full, fresh and generous, with natural variation in every bloom.
{{#image}}Take the flowers and colors from the picture.{{/image}}
Show it as {{format}}.
No text.
