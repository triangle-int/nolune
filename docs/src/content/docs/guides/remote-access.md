---
title: Remote access
description: Reach nolune from phones and other computers, at home and away.
---

The gateway listens on `127.0.0.1:5780`. Put a tunnel in front of it, e.g.
[Tailscale Funnel](https://tailscale.com/kb/1223/funnel),
[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/),
or your own VPS, and tell nolune the public URL:

```sh
nolune config set origin https://nolune.example.com
nolune service restart
```

Keep the host awake when the family needs access. On a Mac, that's System Settings > Energy.
