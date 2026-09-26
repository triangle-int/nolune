---
name: Wanted poster
title: Catch the culprit
sentence: 'Make a wanted poster for the culprit in {{image}}{{#name}}, known as "{{name}}"{{/name}}{{#crime}}, wanted for {{crime}}{{/crime}}{{#reward}}, with a reward of {{reward}}{{/reward}}.'
description: An old-west WANTED poster for whoever ate the last cookie. Cats included.
order: 7
icon: scroll
color: '#e3c29b'
image: required
image-label: Photo of the culprit
size: portrait
settings:
  - id: name
    label: Name
    placeholder: Whiskers
  - id: crime
    label: Crime
    placeholder: stealing the last sausage
  - id: reward
    label: Reward
    placeholder: one belly rub
---

An old-west WANTED poster on aged, stained paper with torn edges, nailed to a weathered wooden wall.
In the middle, an engraving-style sepia portrait of the culprit from the picture, clearly recognizable, with a suitably guilty look.
Bold old-west wood type, spelled exactly as written: "WANTED"{{#name}}, "{{name}}"{{/name}}{{#crime}}, "for {{crime}}"{{/crime}}{{#reward}}, "REWARD: {{reward}}"{{/reward}}.
No other text.
