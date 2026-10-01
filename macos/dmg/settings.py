# The DMG's window, for dmgbuild: nolune and Applications side by side over the planet's rim in
# background.svg, the arrow between them. scripts/build-app.sh passes the paths:
#
#   dmgbuild -s settings.py -D app=nolune.app -D icon=AppIcon.icns -D background=background.png \
#     nolune nolune.dmg
#
# background@2x.png, next to the background, is found and joined to it for Retina screens.

files = [defines["app"]]
symlinks = {"Applications": "/Applications"}
hide_extensions = ["nolune.app"]
# The volume's icon, on the desktop and in the window's title.
icon = defines["icon"]
background = defines["background"]

format = "UDZO"
# The background's size, about the middle of a laptop's screen.
window_rect = ((420, 260), (660, 400))
icon_size = 128
text_size = 13
# The icons' middles, where background.svg draws around them.
icon_locations = {"nolune.app": (170, 176), "Applications": (490, 176)}
