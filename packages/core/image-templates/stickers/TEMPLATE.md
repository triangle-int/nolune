---
name: Stickers
description: Die-cut stickers of people, pets or things from a photo.
category: Trending
order: 2
icon: sticker
color: '#f3dc86'
image: required
image-label: Photo of who or what to turn into stickers
size: square
background: transparent
format: png
settings:
  - id: count
    label: How many
    options:
      - label: One sticker
        prompt: one large sticker
      - label: Sticker sheet
        prompt: a sheet of four different stickers, each with a different expression or pose, evenly spaced
  - id: style
    label: Style
    options:
      - label: Cartoon
        prompt: a bold cartoon with clean outlines and flat colors
      - label: Kawaii
        prompt: super cute kawaii, with big eyes and soft pastel colors
      - label: Photo cut-out
        prompt: the real photo cut out and slightly enhanced, like a photo sticker
---

Turn the main subject of the attached photo into {{count}}.
Style: {{style}}.
Each sticker has a thick white die-cut border and a subtle shadow, on a transparent background. Keep the subject recognizable. No text.
