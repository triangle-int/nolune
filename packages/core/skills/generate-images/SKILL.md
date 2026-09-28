---
name: generate-images
description: Make or change pictures with `nolune generate image`, from a description, a detailed prompt or photos (restyle, edit, combine, turn into stickers, a storybook page, a postcard...). Use whenever someone asks to make, draw, design, generate or edit a picture, including the prompts the Images page's templates send.
---

# Making pictures

`nolune generate image` sends a prompt, and any pictures you give it, to an image model and saves
what it makes. It prints one line per picture:

```
Saved /Users/anna/.nolune/profiles/family/images/2026-09-26-143512-make-a-watercolor-storybook.png (1024×1536, PNG, 1.8 MB)
```

Pictures go to the profile's `images/` folder unless you pass `--out`. It usually takes 20 to 90
seconds, so always run it with `timeout_seconds` set to 300.

To show the result, put it in your reply as a picture, with the exact path it printed:
`![Aisha riding a dragon to school](/Users/anna/.nolune/profiles/family/images/2026-09-26-143512-make-a-watercolor-storybook.png)`.
Don't paste the command or the prompt unless someone asks.

## Prompts from the Images page

The Images page has templates (Storybook page, Sticker pack, Trip postcard...). Applying one starts a new
chat with a finished prompt, and the photo attached when the template uses one:

```
[Anna attached IMG_0142.jpg, saved at /Users/anna/.nolune/profiles/family/attachments/IMG_0142.jpg]
(the photo)
Anna: Make a watercolor storybook page where the kid in the attached picture rides a dragon to school.

A full-page children's picture-book illustration of the adventure.
The kid from the picture is the hero, clearly recognizable...
Style: soft watercolor with loose washes and fine ink lines.
Make it portrait (2:3).

Give the dragon a backpack.
```

A detailed prompt like this, from the Images page or written by someone themselves, is used as
it is: don't reword, shorten or translate it. Pass everything after `Anna: ` on stdin, so its
quotes can't break the command, and each attached picture as an `--image`, by the path it was
saved at. Set the flags the prompt asks for in words:

- "square (1:1)" is `--size square`, "portrait (2:3)" `--size portrait`, "landscape (3:2)"
  `--size landscape`. Without a shape, leave `--size` out.
- A transparent background ("transparent background", "transparent sticker sheet", "no
  background") is `--background transparent`.

```sh
nolune generate image - --size portrait \
  --image /Users/anna/.nolune/profiles/family/attachments/IMG_0142.jpg <<'PROMPT'
Make a watercolor storybook page where the kid in the attached picture rides a dragon to school.

A full-page children's picture-book illustration of the adventure.
...
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
nolune generate image "A cozy watercolor of a ginger cat asleep on a sunny windowsill, soft morning light, pastel colors, no text" --size landscape
```

## Changing a picture

Pass the pictures with `--image` (up to 16) and say what to change and what to keep the same:

```sh
nolune generate image --image photo.jpg "Replace the grey sky with a warm sunset. Keep the people, their faces and the house exactly as they are."
nolune generate image --image room.jpg --image sofa.png "Put the sofa from the second picture in the room from the first."
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
- `--model <provider/model>` to use another model than the configured one (`nolune config`).
- `--dry-run` prints the prompt and settings without making anything.

## Things to know

- Every picture costs money. Make one unless the person asks for more, and don't retry more than
  once without asking.
- If you need to check a result (text spelled right, the right number of things), look at it
  with `nolune view <path>` before showing it; otherwise just show it.
- The image model gets the pictures you pass with `--image` itself, so your prompt doesn't need to
  describe what's in them, only what to make of them.
- **No API key**: tell the person that an admin can add an OpenAI key under Models & keys in nolune
  (or with `nolune key set openai` on this computer).
- **Refused by the safety system**: say so plainly. Don't reword the prompt to get around it.
- **Photos of people**: the model keeps faces close, but not perfectly. Say so if it matters.

This skill ships with nolune and is replaced on updates. To adapt it for this profile, copy the folder
into the profile's skills folder and edit the copy; it takes precedence.
