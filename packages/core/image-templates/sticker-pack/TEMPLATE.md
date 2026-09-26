---
name: Sticker pack
title: Make a sticker pack
sentence: 'Create a {{style}} sticker pack based on {{image}}, remixing with {{remix}}.'
description: Nine stickers of a person, a pet or a thing, each with its own expression or pose.
order: 5
icon: sticker
color: '#f3dc86'
image: required
max-images: 3
image-label: Photo of who or what to turn into stickers
size: square
settings:
  - id: style
    label: Style
    options:
      - label: 3D
        prompt: Use rounded, toy-like forms, smooth polished materials, soft studio lighting, subtle ambient occlusion.
      - label: Retro
        prompt: Use a 1970s retro cartoon look, with bold outlines, halftone shading, groovy rounded shapes and a warm, sun-faded palette.
      - label: Kawaii
        prompt: Use super cute kawaii forms, with big shiny eyes, blushing cheeks, soft pastel colors and thick clean outlines.
      - label: Pixel art
        prompt: Use crisp pixel art, with chunky square pixels, a limited palette and no anti-aliasing.
      - label: Embroidered
        prompt: Make them embroidered patches, with visible satin stitches, thread texture and a stitched edge.
  - id: remix
    label: Remix with
    options:
      - 💀🍓🛼💨
      - 🌻💥🍉🎨
      - 🌈🦄✨🍭
      - 🍄🌙🔮🐸
      - 🔥🎸⚡🕶️
      - 🌊🐚☀️🏄
---

{{style}}

Create a single {{#aspect}}{{aspect}} {{/aspect}}transparent sticker sheet with nine distinct stickers arranged in a 3×3 grid, each showing a different expression, pose, or reaction. Separate the stickers with wide, fully transparent gaps. No background, shadows, or overlapping elements.
