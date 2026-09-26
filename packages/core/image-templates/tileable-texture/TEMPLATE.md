---
name: Tileable texture
title: Make a seamless texture
sentence: 'Make a seamless {{style}} texture of {{material}}{{#image}}, matching {{image}}{{/image}}.'
description: A texture that repeats without seams, for games, 3D or a pattern.
order: 18
icon: grid-3x3
color: '#d9cbb4'
image: optional
image-label: Photo of the material to match
size: square
settings:
  - id: style
    label: Style
    options:
      - label: photoreal
        prompt: a photorealistic albedo texture, crisp and detailed
      - label: hand-painted
        prompt: hand-painted, like classic stylized fantasy games
      - label: pixel art
        prompt: pixel art with crisp pixels and a limited palette
      - label: toon
        prompt: a stylized toon look with clean shapes and soft color bands
  - id: material
    label: Material
    required: true
    placeholder: mossy cobblestone
---

A seamless, tileable texture that repeats without visible seams on all four edges, seen straight on from above, with flat, even lighting, no perspective, no vignette and no cast shadows.
Style: {{style}}.
{{#image}}Match the look, colors and scale of the material in the picture.{{/image}}
Even detail across the whole image, with no single element standing out. No text.
