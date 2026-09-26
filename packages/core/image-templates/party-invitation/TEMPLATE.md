---
name: Party invitation
title: Invite everyone to the party
sentence: 'Make a {{theme}} party invitation for {{name}}{{#age}}, turning {{age}}{{/age}}{{#when}}, on {{when}}{{/when}}{{#where}}, at {{where}}{{/where}}{{#image}}, starring the kid in {{image}}{{/image}}.'
description: A birthday invitation with the name, age, date and place, in the theme they love.
order: 1
icon: party-popper
color: '#f6b7c1'
image: optional
image-label: Photo of the birthday kid
size: portrait
settings:
  - id: theme
    label: Theme
    options:
      - label: dinosaur
        prompt: friendly dinosaurs, volcanoes and big jungle leaves
      - label: outer space
        prompt: rockets, planets, stars and a smiling moon
      - label: unicorn
        prompt: unicorns, rainbows, clouds and sparkles
      - label: pirate
        prompt: a pirate ship, treasure maps, gold coins and a parrot
      - label: under-the-sea
        prompt: colorful fish, coral, bubbles and a friendly octopus
      - label: superhero
        prompt: capes, comic-book bursts and a city skyline at night
      - label: construction
        prompt: diggers, cranes, dump trucks and traffic cones
  - id: name
    label: Name
    required: true
    placeholder: Aisha
  - id: age
    label: Age
    placeholder: '6'
  - id: when
    label: When
    placeholder: Saturday, May 3 at 2 pm
  - id: where
    label: Where
    placeholder: our house
---

Design a printed party invitation card, filling the whole image, themed around {{theme}}.
Lettering, bold, playful and easy to read, spelled exactly as written: "You're invited!", "{{name}}"{{#age}}, "turns {{age}}"{{/age}}{{#when}}, "{{when}}"{{/when}}{{#where}}, "{{where}}"{{/where}}.
{{#image}}Draw the kid from the picture as the hero of the theme, clearly recognizable.{{/image}}
A bright, joyful illustration with plenty of space around the text. No other text.
