---
name: App icon
title: Design an app icon
sentence: 'Design a {{style}} app icon for {{name}}{{#idea}}: {{idea}}{{/idea}}{{#image}}, inspired by {{image}}{{/image}}.'
description: A polished icon for your app or game, from a name and an idea.
order: 17
icon: app-window
color: '#b7c8f5'
image: optional
image-label: Sketch or reference
size: square
settings:
  - id: style
    label: Style
    options:
      - label: glossy 3D
        prompt: glossy 3D with soft gradients and gentle highlights
      - label: flat
        prompt: flat design with two or three colors and crisp geometry
      - label: pixel art
        prompt: chunky pixel art with a limited palette
      - label: clay
        prompt: soft, clay-like 3D with rounded forms and matte materials
  - id: name
    label: Name
    required: true
    placeholder: Pocket Garden
  - id: idea
    label: Idea
    placeholder: a tiny plant in a pot
---

A single app icon: one bold, simple symbol centered in a rounded square that fills the image edge to edge, readable even at small sizes.
Style: {{style}}.
{{#image}}Take the idea and colors from the picture.{{/image}}
No text, no letters, no mockup, no device frame.
