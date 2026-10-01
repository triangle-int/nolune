nolune now installs like any Mac app, and the family can open it from anywhere: on a phone at school, a laptop at work, or a tablet at grandma's. No Terminal, no router settings, no domain to buy.

{{downloads}}

## What's new

### 🌌 nolune for Mac, no Terminal needed

nolune.app sets everything up for you. It opens with a big bang that bursts out of its window across your desktop, then walks you through three steps:

- **your account**, with a strong password made for you;
- **an address for the family**, like `https://smiths.nolune.family`, with a copy button to share it;
- **Full Disk Access**, so nolune can help with Documents, Photos and Mail. The switch in the app flips when the real one in System Settings does.

After that nolune lives in the menu bar. It starts when your Mac does and runs while the app is open. It carries its own Node, so there's nothing else to install.

### 🌍 Open it from anywhere

Turn on nolune's relay and your family gets its own address that works on any device, at home or away. Your computer connects out to the relay, so there's no port to open and no tunnel to run.

- Offered in the Mac app, in `nolune setup`, and any time with `nolune relay enable --name smiths`.
- Free: each address can use 30 GB a month, far more than a family's chats need.
- When your computer is asleep, the address shows a friendly "nolune is offline" page that reloads itself when it's back. A restart doesn't show it.
- Pick a name you like, or get a random one like `cozy-otter-42`. Names nobody has used for 90 days become free again, and a nolune that comes back after a long break gets its name back by itself.

### 👪 Your family, managed from the browser

- **People**, in the user menu, lists everyone's accounts. Add people, reset passwords, make someone an admin, or remove an account.
- **Invite links** let someone create their own account, with their own name and password, so no password gets passed around. A link works once and expires after 7 days. You can also make one with `nolune user invite Anna`.
- **Your name and picture:** Settings now opens on your account. Change the name everyone sees and add a profile picture, cropped right in the browser.

### 💬 Chats that feel shared

- See who else is typing, with their picture and "Max is typing", right where their message will land.
- Profile pictures show next to messages, in the typing indicator and on memory notes.
- The new chat screen greets you by the time of day.

### 🛡️ Auto mode

Before the agent runs a command, a model checks it. It blocks what could do harm nobody asked for: deleting more than you meant, sending files or passwords out, buying or posting for you. Commands that only look run without a check. When something is blocked, nolune says what it wanted to do, and a "yes" from you lets it through. It's on by default; admins can turn it off under **Models & keys**.

### 🔑 ChatGPT Plus and Pro, with Sign in with ChatGPT

Use your ChatGPT plan with nolune by signing in with ChatGPT, right in a new profile's welcome or under **Models & keys**. You don't need Codex anymore. PDFs now reach the model on the plan too.

### 🐧 Linux, in the background

`nolune service install` now runs the gateway as a systemd user service, so it starts with your computer and keeps running after you log out.

## Upgrading from 0.1.0

- **From npm:** run `npm install -g nolune@latest`, then `nolune service restart`. Your chats, memory and settings carry over, and nolune updates its database the first time it starts.
- **If you used the ChatGPT plan:** it no longer goes through Codex. Sign in with ChatGPT once under **Models & keys**.
- **To get an address from anywhere:** run `nolune relay enable`, then `nolune service restart`.

## Also in this release

- The new site at [nolune.dev](https://nolune.dev), with the docs at [nolune.dev/docs](https://nolune.dev/docs).
