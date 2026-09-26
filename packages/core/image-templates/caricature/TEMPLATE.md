---
name: Caricature
description: A fun caricature surrounded by the things someone loves.
category: Trending
order: 4
icon: smile
color: '#a9cfee'
image: required
image-label: Photo of the person
size: portrait
settings:
  - id: loves
    label: Things they love
    placeholder: books, travel, tennis
  - id: exaggeration
    label: Exaggeration
    options:
      - label: Gentle
        prompt: Exaggerate their features only a little and keep it flattering.
      - label: Classic
        prompt: Exaggerate their most distinctive features playfully, like a street-fair caricature artist.
---

Draw a colorful caricature of the person in the attached photo, clearly recognizable, with a big head on a small body. {{exaggeration}}
{{#loves}}Surround them with the things they love: {{loves}}.{{/loves}}
A polished digital painting, cheerful and kind, never mean. No text.
