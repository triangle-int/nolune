---
name: Couple portrait
title: Make a picture of you two
sentence: 'Turn {{image}} into {{style}}{{#title}} called "{{title}}"{{/title}}.'
description: A photo of the two of you as a film poster, a fairy tale, a watercolor or a tarot card.
order: 2
icon: heart
color: '#f4a7b9'
image: required
max-images: 2
image-label: Photo of you two
size: portrait
settings:
  - id: style
    label: Style
    custom: true
    options:
      - label: a romantic film poster
        prompt: a romantic film poster with a dreamy sunset, a soft glow and elegant title lettering
      - label: a fairy tale
        prompt: a fairy-tale storybook illustration in soft gouache, the two of them as the heroes of an enchanted kingdom
      - label: a watercolor
        prompt: a delicate watercolor portrait with loose washes and touches of gold leaf
      - label: a tarot card
        prompt: an art-nouveau tarot card with an ornate border, flowing lines and symbols of love
  - id: title
    label: Title
    placeholder: The Lovers
---

Keep both people clearly recognizable: their faces, hair, features and the way they are together.
Make it {{style}}.
{{#title}}Include the title "{{title}}", spelled exactly as written.{{/title}}
{{^title}}No text.{{/title}}
Warm, romantic and a little magical.
