# Nolune · Little Moon design system

Use this guide before changing the client, landing, onboarding, or brand assets. The approved direction is **Little Moon**. The product name is Nolune (wordmark: `nolune`).

## Source of truth

- Client color tokens: `client/src/lib/styles/tokens.css`.
- Shared application CSS primitives: `client/src/routes/layout.css` (`nl-button`, `nl-button-secondary`, `nl-panel`, `nl-input`, `nl-eyebrow`).
- Interactive examples: client route `/design-system`, including the real message, composer, and tool components. Examples are explicitly sample content and do not require a backend.
- Landing: `landing/src/app.css` and `landing/src/lib/components/`. The self-hosting docs under `landing/src/routes/docs/` reuse `.section-shell`, `.eyebrow` and `.section-title`; their section nav and long-form prose rules (headings divided by 1px borders, mono code and tables that scroll in their own container) live in `docs/+layout.svelte`, and every section page is wrapped in `DocsPage.svelte`.
- Desktop (Tauri): `desktop/src/app.css` mirrors the client tokens and `nl-*` primitives; fonts are bundled from Fontsource because the app CSP only allows same-origin assets. Desktop avatar: `desktop/src/lib/components/Moon.svelte`.
- Avatar: `client/static/skins/moon/character.svg`; thinking expression beside it. Landing copy: `landing/static/assets/nolune-moon.svg`. Desktop copy: the inline `Moon.svelte` component and the app icon source generated from the same path.
- Animated marketing avatar: `landing/src/lib/components/MoonCompanion.svelte`.

Update the guide and reference page when intentionally changing this system. Reuse existing components and tokens before inventing another variant. Do not automatically create a new palette for each feature.

## Character and tone

Calm, capable, personal. Generous space, simple forms, and readable text. Use flat plum surfaces, lavender actions, and ivory type. The landing may use ivory and pale-lavender section backgrounds. The working client uses a consistent dark plum theme.

Little Moon is the only skin. Its persisted identifier is `moon`. Minty and the golden liquid orb are retired. Do not restore glass-orb imagery, blue-black surfaces, glowing buttons, grain overlays, or translucent message bubbles.

Write clear sentence-case labels: “Create your companion”, “Retry connection”. Never present disconnected or failed data loading as an empty account, a successful action, or a live example.

## Color roles

| Role | CSS token | Value |
| --- | --- | --- |
| App background | `--background` | `#201D29` |
| Panel | `--card` | `#282432` |
| Elevated surface | `--popover` | `#312C3C` |
| Main text | `--foreground` | `#F6F3EC` |
| Secondary text | `--text-secondary` | `#D0C9DC` |
| Muted text | `--text-muted` | `#B5ADC2` |
| Primary action / focus | `--primary`, `--ring` | `#B7A9E7` |
| Text on lavender | `--primary-foreground` | `#201D29` |
| Selected surface | `--accent` | `#3B334D` |
| Border | `--border` | `#51485F` |
| Input border | `--input` | `#625770` |
| Destructive text | `--destructive` | `#F39B9B` |

Use semantic tokens rather than literal hex values in client components. `--warm` and `--glass-*` are existing component aliases mapped to the new system; they do not authorize amber colors or glass effects. Status colors must be accompanied by text, never used as the sole signal. Check text contrast against the actual background; do not dim essential labels with extra opacity.

## Typography

- **Fraunces 400**: welcome headings and editorial moments. Use upright type; italic only as a deliberate short emphasis. Heading line height 1.1–1.2, tracking no tighter than -0.035em.
- **Bricolage Grotesque**: navigation, controls, body, messages, settings. Body 16px, compact body 14px, helper text 12–13px. Line height 1.5–1.7.
- **JetBrains Mono**: code, commands, technical identifiers; never the default navigation font.
- Heading sizes: 40–72px for welcoming screens, 28–36px for sections, 18px for card titles. Reduce welcome headings on phones.

## Spacing and shape

Use the 4px spacing scale: 4, 8, 12, 16, 24, 32, 48. Typical panel padding 24px, page gutter 40px desktop / 20px mobile. Controls use 8px radius, panels 16px, chat inputs 12px. Use thin solid borders and minimal shadows. Avoid nested decorative cards.

## Component rules

- Primary action: lavender fill, plum text, minimum 44px height, 14px medium sans text.
- Secondary action: panel fill with border and ivory text. Preserve disabled and pending states.
- Selects: use the shared shadcn-svelte `Select` components, with a 44px trigger and option targets. Menus use popover tokens and visible selected/focus states. The composer model selector is the reference: each option is the preset name over a muted 11px line with the model id and, when the model lacks vision, documents or tools, the chips (`no documents`) after it; the same limitation is stated in a secondary-text sentence under the composer while that model is the conversation's (#28, see the Chat example in `/design-system`).
- Inputs: visible label, 16px text, solid panel surface, focus outline. Placeholder text is not a label.
- Chat: lavender-tinted user bubble, plum companion bubble. Keep text selection, file links, streaming, and tool interactions intact.
- Navigation: clear active surface and lavender indicator; scroll horizontally when space is limited rather than hiding destinations.
- Empty states: one clear next step and brief explanation. Keep loading, disconnected, authenticated-empty, and populated states distinct.
- Icons: existing Lucide components or simple outlined SVGs, 1.5–2px stroke. Do not substitute ambiguous Unicode glyphs for document/action icons.

## Avatar and motion

Use the approved lavender crescent path and small dark eyes. Keep the face minimal. Use the thinking SVG for actual thinking state, never an unrelated randomized video.

On the landing, the moon can float, blink, glance, and change expression on activation. Keep motion gentle and avoid changing animation duration on hover (this shifts phase and visibly snaps). Supply pause for continuous decorative motion and honor `prefers-reduced-motion`. Essential UI must remain usable without animation. Do not hide the avatar in reduced-motion mode.

## Companion state

Little Moon reflects what Nolune is really doing, never a decorative mood. The state comes from the pure reducer in `client/src/lib/companion/state.js`, held by the scene store (`scene.companion`, `scene.companionStatus`) and fed by the root layout from websocket events only. Every state has an accessible sentence that names the related action, computer, request, or blocker, so it reads without motion; when several facts hold at once the highest row wins.

| Priority | State | Derived from | Status text |
| --- | --- | --- | --- |
| 1 | Offline | Socket closed or not open yet | “Nolune is offline, reconnecting (attempt 3).” / “Nolune is connecting.” |
| 2 | Blocked by permissions | `run_command` output reporting a permission denial, in any shape it produces: `error: …`, `stderr: …`, or a raw PTY diagnostic line | “Nolune is blocked by permissions: running command (ls: /root: Permission denied).” |
| 3 | Waiting for approval | An unanswered `secret_request` or approval | “Nolune is waiting for you: a GitHub token for gh.” |
| 4 | Failed | The server’s `[system] <error>` assistant message that precedes `agent_stopped` (which carries no error field), or an `agent_stopped` that names one | “Nolune stopped with an error: something went wrong.” |
| 5 | Working on another computer | A tool call that names another machine (from #80’s trail) | “Nolune is working on studio-mac: opening Finder.” |
| 6 | Working locally | A tool call on this computer | “Nolune is working on this computer: reading notes/tea.md.” |
| 7 | Recalling | `memory_recall` before the first action of the run | “Nolune is recalling 3 memories.” |
| 8 | Thinking | `agent_running` with no recall or action yet, or a reply after the last action | “Nolune is thinking.” |
| 9 | Listening | The server accepted your message; no run yet | “Nolune is listening.” |
| 10 | Completed | `agent_stopped` without error, blocker, or open request | “Nolune finished.” |
| 11 | Idle | Connected, nothing in progress | “Nolune is idle.” |

Rules that keep animation honest:

- Offline beats everything and keeps the facts underneath, so reconnecting restores the state the runtime is still in.
- Blocked and waiting beat working. Both outlast `agent_stopped`: a blocked run or an open request is never shown as completed. A blocker clears when the next message or run starts; a request clears only when answered.
- Completed is claimed only for a run the client saw start, once its last run stops without an error. It is the one transient state: the scene store returns it to idle after a short hold; the reducer itself has no timers.
- A failure is recorded the moment the server reports it, so the `agent_stopped` that follows cannot claim completed. `[system]` status lines (mood, rhythm, routine, desktop connected) are neither replies nor failures and change nothing.
- Recalling never overrides working: once an action runs, later recalls only add to the count, and the pause between actions is thinking, not a stale recall.
- A message sent to a companion that is already working is heard without interrupting the work. One companion may run several chats; it is working while any run is active.
- Pass the companion’s name to `companionStatusText` when it is known; the product name is the fallback.

The `/design-system` page reduces one documented event sequence per state, so the gallery cannot drift from the reducer. Expressions and motion per state, and the desktop overlay port, follow in later slices of #86.

## Connected spaces

The Computers tab and the Computers section of Settings › Connections list every place the one companion can act (#80). The rows come from `GET /api/instances/{slug}/machines` viewed by the pure helpers in `client/src/lib/computers/spaces.js` (`client/tests/computers-spaces.test.mjs`) and rendered by `SpaceRow.svelte`; `ConnectedComputers.svelte` only loads, subscribes and renames.

- The server home is the first row, on the elevated surface with a “This server” pill and the moon glyph. Its note says where the companion runs (“Where Luna runs.”, from the companion name the caller passes); the desktops below are other places it can act, never separate companions. When the listing has no `server_local` record the row is synthesized from this browser’s connection: named after the companion (“Luna’s home”), no facts of its own, not renamed or forgotten, and a closed socket reads as “Reconnecting” with a hint about this browser, not as the server being offline. A `server_local` record the server lists (the Cua driver beside it) takes the home slot but reads like any machine: Offline, Not responding or Needs permission with its permissions, actions, Cua driver and hints, whose copy names the Cua driver on the server instead of the desktop app.
- Desktops follow, online by name and then offline by last seen. Each row pairs one state word with its color: Online (lavender), Not responding or Needs permission (destructive), Offline (muted). Offline beats not responding beats needing permission.
- Facts are what the desktop reported at its last registration: each permission with its state (`denied` and `not asked yet` block; `not available here` does not), the number of actions it accepts, and the Cua driver (“not reported” until #18 ships). Nothing is inferred beyond the record.
- Hints name the computer and the one thing to do: reopen the desktop app on an offline computer, check that a silent one is awake, grant a denied permission in System Settings, update an app that reports no actions. Informational hints (the Cua driver) are muted and hidden in the compact Settings copy.
- Rename is inline: the name field is labeled, Enter saves, Escape cancels, blank shows the hostname again. A renamed row keeps its hostname in mono beside the name.
- Forget appears beside Rename on an offline row only (the server refuses a connected one with 409 `machine_online`). It asks once, inline, in the same place the rename field appears: “Forget laptop? Its record and name are dropped; if it connects again it is listed as new.” with an outlined destructive Forget and a Keep button; focus moves to Forget, Escape keeps. A refusal is announced beside the row.
- Rows update live from `machine_updated` and `machine_forgotten`; routine heartbeats are silent on the socket, so the list is also polled every 15 s (and when the tab becomes visible) and a 5 s clock re-derives health from `last_seen` with the server’s 45 s threshold. A poll that was in flight when an event, a rename or a forget landed never reverts that row: `reconcileListing` folds the response in around the rows that changed since it was requested, and a response for a slug the surface has left is dropped. While a refresh fails the rows stay, a status line says how old they are, and the clock freezes so an unreachable server never reads as every computer going quiet.

The `/design-system` page renders the same component from sample records at a fixed clock (home, renamed desktop, not responding, permission denied, offline); its rename and forget apply the events locally and never reach a server.

The composer carries a computer selector (`TargetPicker.svelte`, helpers in `client/src/lib/computers/target.js`, tests in `client/tests/computer-target.test.mjs`) built on the same rows: a shared shadcn `Select` beside the model preset, 44px trigger and options, the monitor glyph, and the name of what the desktop tools will act on. "Ask me" is listed first and leaves the choice open (the only connected desktop; the companion asks when there are several); then the home, then every desktop with its state word in the same color as its row (Online lavender, Not responding and Needs permission destructive, Offline muted). Nothing chosen with several desktops connected reads "Choose a computer" in lavender; with none connected, "No desktop connected"; until the first listing arrives the trigger reads "Ask me" or "Remembered computer" and its title says it is checking which computers are connected. At desktop width the trigger is capped at 224px so its longest built-in label ("No desktop connected") fits. The choice is remembered per conversation in the browser, sent with each message, shown in the chat bar as "working on <name>" (or "at home") while the companion works, and named in every desktop tool's activity entry and in Activity runs ("On <name>"). On phones the two pickers share the row and truncate their names; the composer never scrolls sideways.

## Accessibility and responsive behavior

Visible keyboard focus; semantic buttons/links; meaningful labels for icon-only controls; decorative SVGs hidden from assistive technology. Main controls have 44px targets. Do not disable browser zoom. Support 390px mobile widths without page overflow. Long commands can scroll inside their own container. Announce asynchronous errors and success without stealing focus.

## Verification for future changes

1. Run `corepack pnpm@10.34.5 --dir client check` and `build` (likewise `landing` when affected).
2. Inspect affected screens on desktop and phone widths, including focus and disabled/error states.
3. Use `/design-system` to compare tokens, controls, and actual message rendering.
4. Verify live behavior with a running server; when unavailable, explicitly report that limitation. Never substitute mock data without labeling it.
5. Keep Minty removed. No existing installations required migration at the time of this redesign.

## Svelte AI Elements

The client vendors a focused subset from [Svelte AI Elements](https://svelte-ai-elements.vercel.app/docs/installation) under `client/src/lib/components/ai-elements/`. Upstream license and integration notes live alongside the source. These are editable source components, not a separate chat backend.

- `MessageBubble` composes Message and MessageContent, sanitizes assistant markdown, preserves file viewing and word-level voice reveal, and renders unfinished streams as plain text.
- `PromptComposer` composes Prompt Input, Textarea, Toolbar, and Submit. Reuse it for new conversation surfaces. It has no API dependencies; `ChatInput` supplies model preferences, upload progress, and the real send callback.
- `ChatView` uses Conversation/Content with `autoScroll={false}` because its existing WebSocket and voice code manages scrolling. Do not enable two competing scroll controllers.
- `StreamActivity` uses Tool/ToolHeader/ToolContent. Historical activity is labeled “Recorded”, not “Running” or “Completed”: the current activity record does not prove execution status.
- Preserve raw `File` uploads through Nolune's API. Do not introduce base64 encoding or an AI SDK backend just to use presentation components.
- A send callback can return `false` or reject to preserve the draft and files. Disable duplicate submissions; keep the Stop button available while the agent runs. Enter sends, Shift+Enter inserts a line, IME composition must not submit.
- Add Sources or Confirmation only when the backend supplies real citations or approval state. Never invent source links, execution status, or approval outcomes.

The `/design-system` conversation example explicitly simulates streaming, stop, and failure states without a backend. Its attachments are not uploaded. Use it to verify changes, then exercise real chat against a running server before claiming end-to-end validation.

## Application surfaces

The same tokens apply to settings, onboarding, authentication, skills, drops, activity, memory, computers, file previews, and notification surfaces. Keep collection errors distinct from empty results.

- Shared shadcn Button, Input, and Select triggers default to at least 44px targets. Inputs keep 16px type on phones.
- Destructive companion confirmation uses shadcn AlertDialog (Clear context in the chat bar, Replace on the Data settings page: title, a description that names what is replaced, the safe action focused first, the destructive action filled with `--destructive`). File previews and secret entry use Bits UI Dialog; keep focus trapping, Escape, and focus restoration intact. New-skill entry uses native dialog semantics.
- Navigation has `aria-current`, horizontal scrolling on phones, and a lavender active indicator. Primary tabs are Chat, Activity, Memory, Computers, Settings (`lib/companion/navigation.js`).
- Settings is five section pages under one layout (`routes/[slug]/settings/+layout.svelte`). The section nav is a segmented pill control (lavender active pill, plum text). Each page is one panel divided into sections by 1px borders; a section is a two-column row with the Fraunces title and one-line description on the left and its controls on the right, stacking below 768px. Sections are never separate cards, so uneven heights cannot leave gaps. Shared rules live in `lib/settings/settings.css`; raw server fields stay on Advanced and carry an owner badge. See [settings.md](settings.md) and the Settings example in `/design-system`.
- The working moon is centered within its scene element and capped at 200px on phones so it does not clip or compete with the composer.
- Memory receipts (#84): an assistant reply that recalled memories carries a “Why did Nolune remember this?” disclosure under its timestamp (`MemoryReceiptPanel`), and the memory library shows the same receipt rows on an open memory. Each row names the canonical source (media memories cite their bound text), quotes the recalled excerpt on a lavender rule, and states the retrieval reason, a confidence bucket and when it was recalled; raw scores never appear. A source forgotten since the reply keeps its row with a dashed border and a “Forgotten” badge and says so in words. Controls (`MemoryControls`: Inspect, Correct, Pin, Exclude from proactive use, Forget) are 44px secondary buttons; Correct always starts from the text the correction replaces (the memory body, or a media memory's bound text), never from a listing summary or a receipt excerpt. A correction that conflicts with an earlier one shows both statements side by side and asks which one stays, never merging them; when the conflict is an earlier one still waiting, the prompt says so, names the parked statement as the earlier correction and keeps the new statement in the editor to be saved afterwards. A memory cited once per chunk is shown once. A reply that used no memory says “No memories were used for this reply.” The `/design-system` sample is read-only and fetches nothing.
- Activity lists the companion's commitments (#85) above its receipts, in the same card style: a status word and who promised, the promise, one line of facts from the record (due, waiting, snoozed, next check), the last check with a link to that check-in, and a row of shared secondary buttons (Details, Edit, Snooze, Complete, Cancel; the open one is tinted with `--accent`) that open an inline form under the card instead of a dialog. The Open/History filter is the same 44px segmented pill control as the settings nav. Both controls are the ones shown in `/design-system`; no new variant. Complete asks for confirmation or evidence and keeps its primary action disabled until one is given; Cancel confirms inline with a destructive primary action. Commitment check-ins in the receipts list say what changed and why then and link to the commitment.
- Conversation history failures remain visible with Retry and Settings actions. Provider setup is required for live chat; the design-system example stays explicitly local and simulated.
- Companions (#108): the Companions section of Settings › Connections lists peer companions as rows built by `CompanionRow.svelte` from the pure views in `lib/federation/companions.js`: the companion id shortened in mono (the full id in its title), one state word beside its color (Paired lavender, Waiting for you or for its owner in ivory, Revoked muted), then who invited whom, where it is reached, and its last sighting in muted text, with a secondary note when there is something to do. Confirm is the primary action on a row this server invited; Revoke asks once inline with the outlined destructive button and a Keep button, focus on Revoke, Escape keeps. Invite a companion opens the lavender pairing panel with the one invite line in a read-only mono textarea, a countdown, Copy line, Done, and Withdraw; the panel is the only place the line is ever shown and it keeps nothing once dismissed. Accept an invite is a labeled textarea with an inline alert when the paste looks like a URL and a disabled submit until it does not. Rotate signing key asks once inline and reports in a status line. `/design-system` renders the four row states from sample records at a fixed clock.
- Handoff cards (`lib/components/continuity/HandoffCard.svelte`, listed by `HandoffCards.svelte` at the top of Activity) are plain panels built from the server's card: goal as the 18px title, the origin line and the decision line in secondary text, sectioned lists, then the four decisions as one primary (Continue here) and three secondary actions. The preview replaces the actions with the destination and its checks grouped as stops (destructive text with words), approvals, and notes, and a Back control; the picker is a list of full-width secondary buttons, unavailable computers disabled with the reason in their label. Copy comes from the pure helpers in `lib/continuity/handoff.js`. See the Handoff example in `/design-system`.
- The resume suggestion (#83, `lib/components/continuity/ResumeBanner.svelte`, loaded by `ResumeSuggestion.svelte` above the content of every companion tab) is one plain panel with the “Resume my work” eyebrow, the task as the 18px title, the server's why-now sentence and “Why this one” line, the next step, and a muted line saying when it was suggested and that nothing continues until the card is accepted. Review and continue is the one primary action and links to the task's handoff card under Activity (the list item `#handoff-card-<record_id>`, outlined with `--ring` when targeted; the card's heading keeps `handoff-<record_id>` as its accessible name); Not now, Snooze (three inline presets and Back), and Never this task are secondary. Copy comes from `lib/continuity/resume.js`. The Settings › Companion section uses the page's own switch and native selects. See the Resume my work example under Handoff in `/design-system`.

Onboarding uses `MoonBirth.svelte` after skin selection: the crescent gently grows into place, opens its eyes, blinks, and greets the user. It completes automatically, has a Continue control, and shows a still moon with a shorter hold for reduced motion. Replay the same component in `/design-system`.

## Desktop app

The Tauri desktop app (`desktop/`) is a connection shell, not a second chat client. It uses the same plum surfaces, lavender actions, Fraunces headings, and `nl-button`, `nl-button-secondary`, `nl-panel`, `nl-input`, `nl-eyebrow` primitives as the client.

- Splash: the crescent grows into place and opens its eyes (same keyframes as `MoonBirth.svelte`), then the `nolune` wordmark fades in. It has a Continue control and shows a still moon with a shorter hold for reduced motion. The golden orb video and its spark sound are retired; the splash plays a short CC0 chime (`desktop/static/splash.mp3`, from videoeditingsfx.com, no attribution required) at 0.4 gain, timed to the moon opening its eyes.
- Connect panel: visible labels, 16px inputs, one primary action (Save connection or Open companion) and secondary actions beside it. Errors use `--destructive` text with `role="alert"`; pending state shows a spinner with text.
- Settings: permissions are a bordered list on `--card`; a granted state is a text badge, an ungranted state is a lavender Grant button. Failed status checks show an error and Retry, never an empty list.
- Overlay: the working moon sits in a small card-colored disc with gentle float and blink; action flashes are card-colored chips with outlined SVG icons and JetBrains Mono text. No emoji glyphs.
- App icon: lavender crescent on a plum rounded square, regenerated with `pnpm tauri icon` from a 1024px PNG of the approved path.
