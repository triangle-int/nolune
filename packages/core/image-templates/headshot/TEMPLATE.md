---
name: Headshot
description: A polished profile photo from any snapshot of a person.
category: Templates
order: 5
icon: user-round
color: '#c5d8c3'
image: required
image-label: Photo of the person
size: square
quality: high
settings:
  - id: backdrop
    label: Backdrop
    options:
      - label: Studio grey
        prompt: a seamless mid-grey studio backdrop
      - label: Office
        prompt: a softly blurred, bright modern office
      - label: Outdoors
        prompt: softly blurred green trees in daylight
      - label: Bright color
        prompt: a solid, bright color that suits them
  - id: outfit
    label: Clothes
    options:
      - label: Keep theirs
        prompt: the clothes they wear in the photo, neatened up
      - label: Smart casual
        prompt: smart casual, a plain sweater or an open-collar shirt
      - label: Business
        prompt: business attire with a dark blazer
---

Turn the attached photo into a professional headshot of the same person. Keep their face, features, skin tone, hair and age exactly as they are: it must look like them.
Head and shoulders, facing the camera with a natural, friendly expression, sharp focus on the eyes and soft, flattering light, as if shot with an 85 mm portrait lens.
Background: {{backdrop}}.
Clothes: {{outfit}}.
Photorealistic, natural skin texture, no text.
