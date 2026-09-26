---
name: Logo
description: A clean logo for a club, a business or a project.
category: Templates
order: 3
icon: badge
color: '#c3cfdb'
image: optional
image-label: Sketch or idea to start from
size: square
quality: high
background: transparent
settings:
  - id: name
    label: Name
    required: true
    placeholder: Fish & Friends
  - id: about
    label: What it's for
    placeholder: a family fishing club
  - id: style
    label: Style
    options:
      - label: Minimal
        prompt: a minimal wordmark with a simple symbol, flat, in one or two colors
      - label: Emblem
        prompt: a round emblem or badge, with the name set along the ring and a symbol in the middle
      - label: Mascot
        prompt: a friendly mascot character with the name below it, with bold outlines
      - label: Hand-lettered
        prompt: the name hand-lettered with a brush, lively and warm
      - label: Geometric
        prompt: a geometric symbol built from simple shapes, with the name in a modern sans serif
  - id: colors
    label: Colors
    placeholder: navy and white
---

Design a professional logo for "{{name}}"{{#about}}, {{about}}{{/about}}.
Style: {{style}}.
{{#colors}}Colors: {{colors}}.{{/colors}}
{{#image}}Start from the attached sketch or reference and keep its main idea.{{/image}}
Spell the name exactly as written. One logo, centered, with plenty of empty space around it and crisp, vector-like edges. No mockup, no extra text, no watermark.
