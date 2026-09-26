---
name: Storybook page
title: Star in a picture book
sentence: 'Make a {{style}} storybook page where the kid in {{image}} {{adventure}}.'
description: A picture-book page where your kid goes on any adventure you can think of.
order: 6
icon: book-open
color: '#f3d08a'
image: required
image-label: Photo of the kid
size: portrait
settings:
  - id: style
    label: Style
    options:
      - label: watercolor
        prompt: soft watercolor with loose washes and fine ink lines
      - label: gouache
        prompt: rich mid-century gouache with flat shapes and textured brushwork
      - label: paper-cut
        prompt: a layered paper cut-out collage with soft shadows
      - label: crayon
        prompt: bright crayon and colored pencil, like the best drawer in class made it
  - id: adventure
    label: Adventure
    required: true
    placeholder: rides a dragon to school
---

A full-page children's picture-book illustration of the adventure.
The kid from the picture is the hero, clearly recognizable: the same face, hair and favorite clothes, drawn in the book's style.
Style: {{style}}.
Warm, whimsical and full of little details to discover, with a clear focal point. No text.
