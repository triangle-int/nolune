---
name: Interior design
title: Redesign your room
sentence: 'Redesign the room in {{image}} in {{style}} style, shown as {{look}}.'
description: See one of your rooms redone in a new style.
order: 7
icon: sofa
color: '#d4b6e6'
image: required
image-label: Photo of your room
size: auto
settings:
  - id: style
    label: Style
    options:
      - label: Scandinavian
        prompt: light wood, white walls, soft wool textiles, simple functional furniture and green plants
      - label: Japandi
        prompt: low natural-wood furniture, linen, muted earth tones, paper lamps and a calm, uncluttered feel
      - label: mid-century modern
        prompt: walnut furniture on tapered legs, mustard and teal accents and graphic prints
      - label: industrial loft
        prompt: exposed brick, black steel, leather, warm filament bulbs and a concrete floor
      - label: cozy cottage
        prompt: floral fabrics, painted wood, layered warm textiles, books and plants
      - label: minimalist
        prompt: a few perfect pieces, white and warm grey, hidden storage and nothing on the surfaces
      - label: boho
        prompt: rattan, macramé, layered rugs, terracotta tones and plants everywhere
  - id: look
    label: Look
    options:
      - label: a realistic photo
        prompt: Show it as a realistic interior photograph from the same camera angle as the original, in natural daylight.
      - label: an isometric 3D model
        prompt: Show it as a cut-away isometric 3D miniature of the room, like a detailed diorama, on a plain soft pastel background.
---

Keep the room's architecture exactly: walls, windows, doors and ceiling stay where they are, with the same proportions. Replace the furniture, colors, textiles, lighting and decoration to fit the style: {{style}}. Make it look cohesive and lived-in.
{{look}}
No text, no watermark.
