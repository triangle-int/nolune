---
name: Tattoo
title: Design a tattoo
sentence: 'Design a {{style}} tattoo of {{idea}}{{#image}}, based on {{image}}{{/image}}, shown {{show}}.'
description: A tattoo design from an idea, as a clean stencil or on skin.
order: 14
icon: feather
color: '#cfc3e6'
image: optional
image-label: Sketch or reference
size: portrait
settings:
  - id: style
    label: Style
    options:
      - label: fine-line
        prompt: delicate fine-line work with thin, even lines
      - label: botanical
        prompt: botanical, with detailed leaves and flowers
      - label: minimalist
        prompt: minimalist, drawn as a single continuous line
      - label: traditional
        prompt: American traditional, with bold outlines and solid red, green, yellow and black
      - label: watercolor
        prompt: watercolor, with soft color bleeding behind thin linework
  - id: idea
    label: Idea
    required: true
    placeholder: a moth with a crescent moon
  - id: show
    label: Shown
    options:
      - label: as a stencil
        prompt: as a clean black design on plain white paper, like a tattoo flash sheet, centered
      - label: on skin
        prompt: healed on the inner forearm, photographed in soft daylight
---

A tattoo design of {{idea}}, balanced and elegant, with lines that would age well.
Style: {{style}}.
{{#image}}Base it on the picture, keeping its main shapes.{{/image}}
Show it {{show}}.
No text unless it's part of the idea.
