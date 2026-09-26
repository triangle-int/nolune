---
name: Concept art
title: Paint a world for your game
sentence: 'Paint {{style}} concept art of {{world}} in {{mood}}{{#image}}, based on {{image}}{{/image}}.'
description: Environment concept art for a game world you describe.
order: 17
icon: mountain
color: '#f2b48f'
image: optional
image-label: Sketch or reference
size: landscape
settings:
  - id: style
    label: Style
    options:
      - label: painterly
        prompt: loose, painterly digital concept art with visible brushwork
      - label: anime background
        prompt: a hand-painted anime film background
      - label: matte painting
        prompt: a realistic matte painting with cinematic lighting
      - label: low-poly
        prompt: a stylized low-poly 3D render
  - id: world
    label: World
    required: true
    placeholder: a floating market above the clouds
  - id: mood
    label: Light
    options:
      - label: golden-hour light
        prompt: warm golden-hour light with long shadows
      - label: bright daylight
        prompt: bright, clear daylight
      - label: moonlight
        prompt: cool moonlight with glowing windows and lanterns
      - label: a storm
        prompt: a dramatic storm with rain and lightning
---

Game environment concept art: a wide establishing shot with strong depth, a clear focal point and a sense of scale.
Style: {{style}}.
Light and mood: {{mood}}.
{{#image}}Use the picture as the starting point for the layout and main shapes.{{/image}}
No characters in the foreground, no text, no watermark.
