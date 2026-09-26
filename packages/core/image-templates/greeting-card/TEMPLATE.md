---
name: Greeting card
description: A card for a birthday, a holiday or a thank-you.
category: Templates
order: 7
icon: gift
color: '#f1b9c7'
image: optional
image-label: Photo to put on the card
size: portrait
quality: high
settings:
  - id: occasion
    label: Occasion
    options:
      - label: Birthday
        prompt: a birthday, with balloons, cake or confetti
      - label: Christmas
        prompt: Christmas, with snowy winter and festive details
      - label: New Year
        prompt: New Year, with fireworks and sparkle
      - label: Wedding
        prompt: a wedding, elegant and romantic, with flowers
      - label: Thank you
        prompt: a thank-you, warm and heartfelt, with flowers
      - label: New baby
        prompt: a new baby, soft and tender, in pastel colors
      - label: Get well
        prompt: a get-well wish, cheerful and comforting, with sunshine and flowers
  - id: message
    label: Message
    placeholder: Happy birthday, Grandma!
  - id: style
    label: Style
    options:
      - label: Watercolor
        prompt: soft watercolor with delicate gold accents
      - label: Cut paper
        prompt: a layered cut-paper collage with gentle shadows
      - label: Vintage
        prompt: a vintage postcard with muted colors and ornate details
      - label: Playful
        prompt: bright, playful hand-drawn doodles
---

Design the front of a greeting card for {{occasion}}.
{{#image}}Feature the people or scene from the attached photo, redrawn in the card's style and still recognizable.{{/image}}
{{#message}}Write "{{message}}" on the card in beautiful lettering, spelled exactly as written.{{/message}}
{{^message}}No text.{{/message}}
Style: {{style}}.
Just the card front, filling the whole image: no border, no table, no mockup.
