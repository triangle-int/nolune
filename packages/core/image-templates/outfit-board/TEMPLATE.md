---
name: Outfit board
title: Put together a look
sentence: 'Put together {{format}} of a {{aesthetic}} outfit{{#occasion}} for {{occasion}}{{/occasion}}{{#image}}, built around {{image}}{{/image}}.'
description: A styled flat-lay, mood board or lookbook photo of an outfit in any aesthetic.
order: 11
icon: shirt
color: '#e6c9dc'
image: optional
max-images: 3
image-label: A piece or an inspiration
size: portrait
settings:
  - id: format
    label: Format
    options:
      - label: a mood board
        prompt: A mood board collage of the outfit pieces with fabric swatches, a color palette, textures and a few scene photos that capture the vibe, pinned together.
      - label: a flat-lay
        prompt: A styled flat-lay photographed from above on clean linen, every piece neatly arranged with a little space around it.
      - label: a lookbook photo
        prompt: A fashion lookbook photo of a model wearing the whole outfit in a fitting place, in natural light.
  - id: aesthetic
    label: Aesthetic
    options:
      - label: cottagecore
        prompt: cottagecore, with flowing floral dresses, linen, lace, straw hats and soft earthy colors
      - label: Y2K
        prompt: Y2K, with low-rise jeans, baby tees, butterfly clips, glossy pinks and chrome
      - label: old money
        prompt: old money, with tailored neutrals, cashmere, loafers, pearls and quiet luxury
      - label: streetwear
        prompt: streetwear, with oversized layers, bold sneakers, caps and graphic details
      - label: minimalist
        prompt: minimalist, with clean lines, a neutral palette and a few perfect pieces
      - label: dark academia
        prompt: dark academia, with tweed, plaid, knitwear, loafers and rich browns
  - id: occasion
    label: Occasion
    placeholder: a summer picnic
---

{{format}}
The look: {{aesthetic}}.
{{#image}}Build the outfit around the piece or inspiration in the pictures, keeping its colors and shapes.{{/image}}
Clothes, shoes, a bag and accessories that go together, in a cohesive palette, with clean editorial styling. No brand logos, no text.
