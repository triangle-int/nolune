---
name: generate-images
description: Make or change pictures with `btw generate image`, from a description or from photos (restyle, edit, combine, turn into a poster, sticker, sketch...). Use whenever someone asks to make, draw, design, generate or edit a picture, and for "Make an image with the ... template" messages from the Images page.
---

# Making pictures

`btw generate image` sends a prompt, and any pictures you give it, to an image model and saves
what it makes. It prints one line per picture:

```
Saved /Users/anna/.btw-agent/profiles/family/images/2026-09-26-143512-design-a-striking-printed.png (1024×1536, PNG, 1.8 MB)
```

Pictures go to the profile's `images/` folder unless you pass `--out`. It usually takes 20 to 90
seconds, so always run it with `timeout_seconds` set to 300.

To show the result, put it in your reply as a picture, with the exact path it printed:
`![Poster with the headline BLOOM](/Users/anna/.btw-agent/profiles/family/images/2026-09-26-143512-design-a-striking-printed.png)`.
Don't paste the command or the prompt unless someone asks.

## Requests from the Images page

The family picks a template on the Images page (Poster, Stickers, Sketch...), chooses its
settings and a photo, and the page sends a message with the photo attached and the finished
prompt:

```
[Anna attached IMG_0142.jpg, saved at /Users/anna/.btw-agent/profiles/family/attachments/IMG_0142.jpg]
(the photo)
Anna: Make an image with the Poster template.
Size: portrait
Quality: high

Prompt:
Design a striking printed poster, ready to hang on a wall.
Build it around the main subject of the attached picture, redrawn to fit the poster's style.
Style: a risograph print in two or three bright spot inks, with visible grain...

Also: make it pink
```

Run it exactly as asked: the lines before `Prompt:` are options (`Size` is `--size`, `Quality`
`--quality`, `Background` `--background`, `Format` `--format`), everything after `Prompt:` is
the prompt, and each attached picture is an `--image`, by the path it was saved at.
Don't reword the prompt: it's the template's, tested to work. Pass it on stdin so its quotes
can't break the command:

```sh
btw generate image - --size portrait --quality high \
  --image /Users/anna/.btw-agent/profiles/family/attachments/IMG_0142.jpg <<'PROMPT'
Design a striking printed poster, ready to hang on a wall.
Build it around the main subject of the attached picture, redrawn to fit the poster's style.
Style: a risograph print in two or three bright spot inks, with visible grain...

Also: make it pink
PROMPT
```

Messages that start with `Make an image:` come from the page's own text box: that's a
description in the person's words, so write the prompt yourself (below).

## Writing a prompt

Write it like a brief for an illustrator or photographer, in English, whatever language you
speak with the family:

- What's in it, where, and what it's doing; then the style or medium (photo, watercolor, 3D
  render, flat vector), the composition and camera, the light and the mood, the colors.
- Put words that must appear in the picture in quotes and say they must be spelled exactly.
- Say what to leave out ("no text", "no watermark", "plain background").

```sh
btw generate image "A cozy watercolor of a ginger cat asleep on a sunny windowsill, soft morning light, pastel colors, no text" --size landscape
```

## Changing a picture

Pass the pictures with `--image` (up to 16) and say what to change and what to keep the same:

```sh
btw generate image --image photo.jpg "Replace the grey sky with a warm sunset. Keep the people, their faces and the house exactly as they are."
btw generate image --image room.jpg --image sofa.png "Put the sofa from the second picture in the room from the first."
```

For follow-ups ("make it bluer", "bigger headline"), run it again with the last result as the
`--image` and describe only the change. To start over with different choices, run the original
prompt again with the change worked in.

## Options

- `--size square|portrait|landscape|auto` or `WIDTHxHEIGHT` (multiples of 16, like 1536x1024).
  `auto` lets the model pick; for edits it keeps the photo's shape.
- `--quality low|medium|high|xhigh|max|auto`: higher is slower and costs more. Leave it out
  unless the person wants a draft (`low`) or a print (`high` or more).
- `--background transparent` for stickers, logos, icons and sprites (PNG or WebP).
- `--format png|jpeg|webp`, `--count 1-4`, `--out <folder or file>`.
- `--model <provider/model>` to use another model than the configured one (`btw config`).
- `--dry-run` prints the prompt and settings without making anything.

## Things to know

- Every picture costs money. Make one unless the person asks for more, and don't retry more than
  once without asking.
- If you need to check a result (text spelled right, the right number of things), look at it
  with `btw view <path>` before showing it; otherwise just show it.
- The image model gets the pictures you pass with `--image` itself, so your prompt doesn't need to
  describe what's in them, only what to make of them.
- **No API key**: tell the person that an admin has to run `btw key set openai` on this computer.
- **Refused by the safety system**: say so plainly. Don't reword the prompt to get around it.
- **Photos of people**: the model keeps faces close, but not perfectly. Say so if it matters.

This skill ships with btw and is replaced on updates. To adapt it for this profile, copy the folder
into the profile's skills folder and edit the copy; it takes precedence.
