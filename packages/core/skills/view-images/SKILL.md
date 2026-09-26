---
name: view-images
description: Look at pictures yourself with `btw view`, which attaches them to a command's result. Photos, screenshots, scans, PDF pages, video frames, web pages or the screen. Use whenever what something looks like matters, e.g. reading a receipt or a letter, checking or finding a photo, or seeing what is on the screen.
---

# Seeing images

`btw view <file>...` attaches pictures to the result of the command that runs it, so you see them
in your next step. It prints a line for each one (`Attached receipt.heic (1500×2000 JPEG,
converted from HEIC).`) and exits with 1, saying why, if a file couldn't be attached.

```sh
btw view ~/Desktop/receipt.heic
btw view scan-1.png scan-2.png     # up to 10 per command
```

- JPEG, PNG, GIF and WebP are sent as they are. HEIC, TIFF and other formats are converted, big
  pictures are shrunk to 2000 px on the long side, and photos are turned upright.
- Only you see these pictures. To show one to the family, put it in your reply:
  `![what it shows](path)`.
- Every picture stays in this conversation and is sent again with each step; a conversation holds
  at most 100 (20 MB). Look at what the task needs, and at small copies when that's enough. When
  `btw view` says the conversation is full, say what you need in words or suggest a new chat.
- When only the text matters, reading it (`pdftotext doc.pdf -`) is cheaper than looking. If that
  prints nothing, the PDF is a scan: look at its pages.

The commands below are macOS's own (`sips`, `qlmanage`, `screencapture`) or optional tools. Check
an optional one with `command -v` first, and ask before installing anything.

## Small print

Shrinking to 2000 px loses detail in big scans and screenshots. Crop the part you need at full
resolution and look at that:

```sh
# 800 px high and 1200 wide, starting 1400 px from the top and 300 from the left
sips -c 800 1200 --cropOffset 1400 300 scan.png --out /tmp/crop.png && btw view /tmp/crop.png
magick scan.png -crop 1200x800+300+1400 +repage /tmp/crop.png   # ImageMagick (`convert` in v6)
```

## Many photos

Don't view them one by one. With ImageMagick, a contact sheet shows 25 photos with their file
names in one picture; more photos make `sheet-0.jpg`, `sheet-1.jpg` and so on:

```sh
rm -f /tmp/sheet*.jpg && magick montage -label '%f' ~/Pictures/Trip/*.jpg -auto-orient \
  -thumbnail 360x360 -tile 5x5 -geometry +4+4 /tmp/sheet.jpg && btw view /tmp/sheet*.jpg
# `montage` instead of `magick montage` in ImageMagick 6
```

Without it, make small copies and view them ten at a time, then open the interesting ones:

```sh
mkdir -p /tmp/small && for f in ~/Pictures/Trip/*; do
  sips -s format jpeg -Z 512 "$f" --out "/tmp/small/$(basename "${f%.*}").jpg" >/dev/null
done
```

## The screen

```sh
screencapture -x /tmp/screen.png && btw view /tmp/screen.png
```

If it shows only the desktop background and menu bar, btw isn't allowed to record the screen. The
person has to add btw's `node` (the path is in `cat "$BTW_HOME/bin/btw"`) in System Settings >
Privacy & Security > Screen Recording (Screen & System Audio Recording on newer macOS). A locked or
sleeping Mac shows nothing useful.

## PDF pages, videos and web pages

```sh
# The first page of a PDF (also a thumbnail of most documents and videos): writes /tmp/ql/doc.pdf.png
mkdir -p /tmp/ql && qlmanage -t -s 2000 -o /tmp/ql doc.pdf >/dev/null && btw view /tmp/ql/doc.pdf.png

# Any page, with poppler: page 3 into /tmp/page3.png
pdftoppm -png -r 150 -f 3 -singlefile doc.pdf /tmp/page3 && btw view /tmp/page3.png

# A video, with ffmpeg: the frame at 0:30, or nine frames 10 s apart in one picture
ffmpeg -loglevel error -y -ss 30 -i clip.mov -frames:v 1 /tmp/frame.jpg
ffmpeg -loglevel error -y -i clip.mov -vf "fps=1/10,scale=480:-1,tile=3x3" -frames:v 1 /tmp/frames.jpg

# A web page, with Google Chrome
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless \
  --screenshot=/tmp/page.png --window-size=1280,2000 "https://example.com"
```
