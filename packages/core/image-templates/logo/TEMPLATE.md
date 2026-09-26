---
name: Logo
title: Design a logo
sentence: 'Design a {{style}} logo for "{{name}}"{{#about}}, {{about}}{{/about}}{{#colors}}, in {{colors}}{{/colors}}{{#image}}, based on {{image}}{{/image}}.'
description: A clean logo for a club, a business or a project.
order: 10
icon: badge
color: '#c3cfdb'
image: optional
image-label: Sketch or idea to start from
size: square
settings:
  - id: style
    label: Style
    options:
      - label: minimal
        prompt: a minimal wordmark with a simple symbol, flat, in one or two colors
      - label: emblem
        prompt: a round emblem or badge, with the name set along the ring and a symbol in the middle
      - label: mascot
        prompt: a friendly mascot character with the name below it, with bold outlines
      - label: hand-lettered
        prompt: the name hand-lettered with a brush, lively and warm
      - label: geometric
        prompt: a geometric symbol built from simple shapes, with the name in a modern sans serif
  - id: name
    label: Name
    required: true
    placeholder: Fish & Friends
  - id: about
    label: What it's for
    placeholder: a family fishing club
  - id: colors
    label: Colors
    placeholder: navy and white
---

Style: {{style}}.
{{#image}}Keep the main idea of the sketch or reference in the picture.{{/image}}
Spell the name exactly as written. One logo, centered, with plenty of empty space around it and crisp, vector-like edges, on a transparent background. No mockup, no extra text, no watermark.
