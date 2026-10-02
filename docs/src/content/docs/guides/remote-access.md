---
title: Remote access
description: Reach nolune from phones and other computers, at home and away.
---

The gateway listens on `127.0.0.1:5780`, so at first only this computer can open it. There are two
ways in from other devices.

## Through nolune's relay

The easiest: `nolune setup` offers it, and so does the macOS app's onboarding (or, later, **Open
it from anywhere…** in its menu bar). Or turn it on from a terminal:

```sh
nolune relay enable --name smiths   # https://smiths.nolune.family (a random name without --name)
nolune service restart              # the gateway connects to the relay; its address is nolune's now
nolune relay status                 # the address, whether nolune is connected, this month's traffic
nolune relay disable                # stop using it and give the name back
```

The gateway keeps a connection open to `relay.nolune.dev`, and the relay passes requests for the
address down it, so there's no port to open, no tunnel to run and no domain to buy. It works on any
phone or laptop, at home or away. While the relay is on, its address is nolune's origin (sign-in,
webhook URLs).

- **Free, within reason.** Each address may pass 30 GB a month through the relay, far more than a
  family's chats use. Past it, the address shows a page saying so until the 1st of next month.
- **Privacy.** The relay could read what passes through it, as any hosted tunnel could; it keeps
  none of it. If you'd rather nobody in between could, use a tunnel of your own (below), or
  [run your own relay](https://github.com/triangle-int/nolune/tree/main/packages/relay) and point
  nolune at it with `nolune relay enable --server https://relay.example.com`. Either way, the
  nolune app for iPhone gets no notifications (below).
- **Notifications on iPhones.** The nolune app for iPhone puts the bell's notifications on the
  lock screen through nolune's relay, the only one with the key Apple asks for.
- **When the computer is off or asleep**, the address shows a page saying nolune is offline, which
  reloads itself until it's back. A restart doesn't show it: the relay waits a few seconds for
  nolune to come back.
- **Away for months.** An address whose nolune hasn't connected in 90 days goes back to the relay,
  so names don't stay taken by computers that are gone. When nolune comes back, it asks for the
  same address again and gets it, unless someone else took the name in the meantime: then it stops
  using it, and `nolune relay enable` gets a new one.

## Through a tunnel of your own

Put a tunnel in front of the gateway, e.g.
[Tailscale Funnel](https://tailscale.com/kb/1223/funnel),
[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/),
or your own VPS, and tell nolune the public URL:

```sh
nolune config set origin https://nolune.example.com
nolune service restart
```

While the relay is on, its address wins: `nolune relay disable` switches to this one.

:::caution[No notifications on iPhones]
On a tunnel of your own, the nolune app for iPhone gets no notifications on the lock screen: they
only come through nolune's relay. The bell still shows them whenever the app is open.
:::

Either way, keep the host awake when the family needs access. On a Mac, that's System Settings >
Energy.
