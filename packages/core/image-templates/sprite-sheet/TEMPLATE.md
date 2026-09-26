---
name: Sprite sheet
title: Make game-ready sprites
sentence: 'Make a {{style}} sprite sheet of {{subject}}: {{kind}}{{#image}}, based on {{image}}{{/image}}.'
description: A character in every pose, or a set of item icons, ready to drop into a game.
order: 16
icon: gamepad-2
color: '#a9dccd'
image: optional
image-label: Sketch or reference
size: landscape
settings:
  - id: style
    label: Style
    custom: true
    options:
      - label: pixel art
        prompt: 16-bit pixel art with crisp square pixels, a limited palette and no anti-aliasing
      - label: hand-painted
        prompt: hand-painted 2D game art with soft shading and clean silhouettes
      - label: flat vector
        prompt: clean flat vector game art with bold shapes and simple shading
      - label: low-poly
        prompt: a rendered low-poly 3D look with flat-shaded facets
  - id: subject
    label: Subject
    required: true
    placeholder: a fox knight
  - id: kind
    label: Kind
    options:
      - label: a walk cycle
        prompt: an eight-frame side-view walk cycle in one row
      - label: idle, run and jump
        prompt: idle, run, jump, attack and hurt poses, one per cell
      - label: item icons
        prompt: a 4×4 grid of matching item icons (weapons, potions, food, tools)
      - label: a turnaround
        prompt: front, three-quarter, side and back views of the same character
---

A game-ready sprite sheet: {{kind}}, every frame the same size and scale.
Style: {{style}}.
{{#image}}Base the character or items on the picture, keeping their main shapes and colors.{{/image}}
Evenly spaced on a clean grid, nothing touching or overlapping, on a transparent background. No labels, no text.
