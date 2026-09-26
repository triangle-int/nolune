---
name: Poster
title: Make a poster
sentence: 'Make a {{style}} poster of {{subject}} {{image}} with the headline {{headline}}.'
description: A bold printed poster built around a photo or an idea.
category: Templates
order: 1
icon: frame
color: '#e8956b'
image: optional
image-label: Photo or picture to feature
size: portrait
quality: high
settings:
  - id: subject
    label: What it shows
    placeholder: a red poppy
  - id: headline
    label: Headline
    placeholder: BLOOM
  - id: style
    label: Style
    options:
      - label: Swiss
        prompt: Swiss International Typographic Style, with a strict grid, clean sans-serif type, flat shapes and lots of white space
      - label: Art Deco
        prompt: Art Deco, with geometric ornament, gold and deep jewel colors and an elegant symmetric layout
      - label: Risograph
        prompt: a risograph print in two or three bright spot inks, with visible grain, slight misregistration and playful overprinting
      - label: Bauhaus
        prompt: Bauhaus, with primary colors, circles, squares and triangles and a bold asymmetric layout
      - label: Vintage travel
        prompt: a 1950s vintage travel poster, with a painted scene, warm sunlit colors and flat lithographic shading
      - label: Minimal
        prompt: modern minimalism, with one bold shape, one accent color and a quiet off-white background
---

Design a striking printed poster, ready to hang on a wall.
{{#image}}Build it around the main subject of the attached picture, redrawn to fit the poster's style.{{/image}}
{{#subject}}The poster shows {{subject}}.{{/subject}}
Style: {{style}}.
{{#headline}}Set the headline "{{headline}}" in large, bold type as a key part of the composition, spelled exactly as written.{{/headline}}
{{^headline}}No text on the poster.{{/headline}}
Strong composition with generous margins, a limited color palette and a subtle paper texture. No watermark, no signature, no mockup: just the poster, filling the whole image.
