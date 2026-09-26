---
name: Interior design
title: Redesign your room
sentence: 'Redesign {{image}} in {{style}} style, shown as {{look}}.'
description: See one of your rooms redone in a new style.
category: Templates
order: 2
icon: sofa
color: '#d4b6e6'
image: required
image-label: Photo of your room
size: auto
quality: high
settings:
  - id: style
    label: Style
    options:
      - label: Scandinavian
        prompt: Scandinavian, with light wood, white walls, soft wool textiles, simple functional furniture and green plants
      - label: Japandi
        prompt: Japandi, with low natural-wood furniture, linen, muted earth tones, paper lamps and a calm, uncluttered feel
      - label: Mid-century modern
        prompt: mid-century modern, with walnut furniture on tapered legs, mustard and teal accents and graphic prints
      - label: Industrial loft
        prompt: industrial loft, with exposed brick, black steel, leather, warm filament bulbs and a concrete floor
      - label: Cozy cottage
        prompt: cozy cottage, with floral fabrics, painted wood, layered warm textiles, books and plants
      - label: Minimalist
        prompt: minimalist, with a few perfect pieces, white and warm grey, hidden storage and nothing on the surfaces
      - label: Boho
        prompt: boho, with rattan, macramé, layered rugs, terracotta tones and plants everywhere
  - id: look
    label: Look
    options:
      - label: a photo
        prompt: Show it as a realistic interior photograph from the same camera angle as the original, in natural daylight.
      - label: an isometric 3D model
        prompt: Show it as a cut-away isometric 3D miniature of the room, like a detailed diorama, on a plain soft pastel background.
---

Redesign the room in the attached photo. New style: {{style}}.
Keep the room's architecture exactly: walls, windows, doors and ceiling stay where they are, with the same proportions. Replace the furniture, colors, textiles, lighting and decoration to fit the style, and make it look cohesive and lived-in.
{{look}}
No text, no watermark.
