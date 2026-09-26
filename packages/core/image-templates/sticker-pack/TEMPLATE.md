---
name: Sticker pack
title: Make a sticker pack
sentence: 'Make a {{style}} sticker pack based on {{image}}{{#remix}}, remixing with {{remix}}{{/remix}}.'
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
      - label: 3D toy
        prompt: rounded, toy-like 3D forms with smooth polished materials, soft studio lighting and subtle ambient occlusion
      - label: kawaii
        prompt: super cute kawaii cartoons with big shiny eyes, blushing cheeks and soft pastel colors
      - label: retro cartoon
        prompt: 1930s rubber-hose cartoons with bouncy limbs, pie eyes and a warm, slightly faded palette
      - label: embroidered patch
        prompt: embroidered patches with visible satin stitches, thread texture and a stitched border
      - label: pixel art
        prompt: crisp pixel art with a limited palette and chunky pixels
  - id: remix
    label: Remix with
    placeholder: 💀🍓🛼💨
---

Style: {{style}}.
{{#remix}}Weave these into the stickers as props, outfits and moods: {{remix}}.{{/remix}}
Keep the subject of the pictures recognizable in every sticker.
A single sticker sheet with nine distinct stickers arranged in a 3×3 grid, each showing a different expression, pose or reaction, each with a thick white die-cut border. Separate the stickers with wide, fully transparent gaps, on a transparent background, with no shadows and nothing overlapping. No text.
