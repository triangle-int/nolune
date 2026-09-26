---
name: Nail art
title: Try a new nail design
sentence: 'Design {{shape}} nails with {{idea}}{{#image}}, inspired by {{image}}{{/image}}.'
description: See a nail design on a hand before you book it.
order: 6
icon: hand
color: '#f7c6d9'
image: optional
image-label: Inspiration
size: portrait
settings:
  - id: shape
    label: Shape
    options:
      - label: almond
        prompt: medium almond-shaped
      - label: square
        prompt: square
      - label: coffin
        prompt: long coffin-shaped
      - label: short round
        prompt: short, rounded
  - id: idea
    label: Design
    required: true
    placeholder: tiny strawberries on milky pink
---

A close-up beauty photo of a hand with a fresh manicure: {{shape}} nails painted with {{idea}}, every nail part of one matching set, with neat, glossy detail.
{{#image}}Take the colors and motifs from the picture.{{/image}}
Soft, flattering light on a clean pastel background. No text.
