---
name: Pixel art
title: Make game-ready pixel art
sentence: 'Create a {{palette}} pixel art {{kind}}{{#subject}} of {{subject}}{{/subject}}{{#image}} based on {{image}}{{/image}}.'
description: Game-ready pixel art, from a single sprite to a whole scene.
category: Templates
order: 8
icon: gamepad-2
color: '#9dd4c6'
image: optional
image-label: Photo or drawing to turn into pixels
size: square
settings:
  - id: palette
    label: Palette
    options:
      - label: 16-bit
        prompt: 16-bit console era, with a rich but limited palette
      - label: NES
        prompt: 8-bit NES era, with a very limited palette and chunky pixels
      - label: Game Boy
        prompt: only four shades of green, like the original Game Boy
      - label: PICO-8
        prompt: the 16-color PICO-8 palette, bold and cute
  - id: kind
    label: Kind
    options:
      - label: character sprite
        prompt: a single full-body character sprite in a three-quarter front view, centered, on a transparent background
      - label: item icon
        prompt: a single game item icon with a bold dark outline, centered, on a transparent background
      - label: scene
        prompt: a complete game scene with a detailed background, like a screenshot from a side-scrolling game
  - id: subject
    label: What to draw
    placeholder: a knight with a lantern
---

Make {{kind}}.
{{#image}}Base it on the picture, keeping its main shapes and colors recognizable.{{/image}}
Palette and era: {{palette}}.
Crisp square pixels on a clear grid, no anti-aliasing, no blur, no smooth gradients, no text.
