---
name: IAP
description: Ruhige lokale Arbeitskonsole mit kühlem Glas und rechter Navigation
colors:
  canvas: "#080f15"
  elevated: "#111416"
  panel: "#15191c"
  chat-panel: "rgba(20,35,45,.58)"
  text-primary: "#f1f3f4"
  text-secondary: "#c5c9cc"
  text-muted: "#939ba1"
  border-subtle: "rgba(255,255,255,.075)"
  border-default: "rgba(255,255,255,.12)"
  border-strong: "rgba(255,255,255,.24)"
  surface-hover: "rgba(255,255,255,.055)"
  surface-active: "rgba(255,255,255,.09)"
  button-primary: "#d7dee2"
  button-primary-hover: "#edf1f3"
  button-primary-text: "#111619"
typography:
  title:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif'
    fontSize: ".98rem"
    fontWeight: 580
    lineHeight: 1.35
    letterSpacing: "-.012em"
  body:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif'
    fontSize: ".9rem"
    fontWeight: 350
    lineHeight: 1.45
    letterSpacing: ".005em"
  label:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif'
    fontSize: ".7rem"
rounded:
  control: ".65rem"
  chat: "1rem"
  prompt: "1.375rem"
  dock: "1.5rem"
spacing:
  dock-gap: ".18rem"
  dock-inset: ".26rem"
  message-gap: "1.25rem"
components:
  button-primary:
    backgroundColor: "{colors.button-primary}"
    textColor: "{colors.button-primary-text}"
  button-primary-hover:
    backgroundColor: "{colors.button-primary-hover}"
    textColor: "{colors.button-primary-text}"
  chat-container:
    backgroundColor: "{colors.chat-panel}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.chat}"
  dock-item-active:
    backgroundColor: "{colors.surface-active}"
    textColor: "{colors.text-primary}"
---

# Design System: IAP

## Overview

**Creative North Star: "Die ruhige Arbeitskonsole"**

IAP presents local work in a dark blue glass workspace. A vertical navigation rail sits on the right and keeps every area directly reachable. The conversation and composer occupy the remaining space.

The rail, composer, chat frame and module surfaces use translucent layers with restrained blur and a brighter top edge. AuroraBackdrop drifts three soft colour fields slowly behind the content. The existing IAP logo has a restrained electric halo, and the wordmark rises letter by letter and lifts under the cursor. Monochrome SVGs support controls.

**Key Characteristics:**
- Centered, readable conversation without message bubbles.
- Visible model, Vault, and connection state; the status text follows actual connector state.
- Eighteen destinations in a scrollable right rail, compact icons below 900px.
- Visible Normal, Planmodus, Agent and Denken controls beside the prompt.
- Blue pointer responsive borders on navigation and selected inner cards, a visible processing line in chat, and restrained side notifications.

## Colors

The palette is monochrome and cool, with a small tonal range between canvas, work surface, and controls.

### Primary

- **Pale action:** `button-primary` and its hover state identify the strongest action; dark text preserves contrast.

### Neutral

- **Deep canvas:** `canvas` surrounds the console.
- **Raised work surfaces:** `elevated`, `panel`, and `chat-panel` distinguish the main work area from its backdrop.
- **Text ladder:** `text-primary`, `text-secondary`, and `text-muted` separate content, supporting copy, and status.
- **Hairline edges:** `border-subtle`, `border-default`, and `border-strong` define surfaces without heavy outlines.
- **Selection layers:** `surface-hover` and `surface-active` mark dock and menu states.

**The State Truth Rule.** Show “Offline” only when the connector setting reports offline mode. The header Air Gap control also shows “Status unbekannt” when the setting cannot be read and “Vorschau” when the local connector preview is enabled. The preview does not imply external network access.

## Typography

**Display Font:** Segoe UI Variable Text, then Segoe UI and platform system sans serif.
**Body Font:** The same system stack.
**Label/Mono Font:** Cascadia Code, then Consolas and monospace where code is shown.

**Character:** Modest size and weight differences make content legible without ornamental display type.

### Hierarchy

- **Title:** `title` styles the chat heading; the startup wordmark is a separate brand moment.
- **Body:** `body` styles conversation and rendered Markdown with a line height of 1.45; the message text is capped at 42rem.
- **Label:** `label` supports dock labels and small controls. Status and timestamps use similarly small sizes.

**The Conversation First Rule.** Give message text more contrast and width than its metadata; keep author and time in a separate quiet row.

## Layout

The app fills the viewport. A 3.8rem status bar sits above the workspace. The vertical rail occupies 12.5rem on the right; the workspace uses the remaining width. Messages are centered within a 60rem lane and their text within 42rem. The composer stays at the chat bottom. Presentation mode moves to a body-level overlay so the header and rail cannot cover it.

The rail groups all 18 routes into Dialog, Wissen, Werkzeuge and System. At 900px it becomes an icon rail. At 720px the message grid becomes one column; at 650px the prompt toolbar wraps and the model button is hidden. The area chooser remains accessible through the existing route search.

## Elevation & Depth

Depth comes from cool gradients, translucent panels, fine borders, specular top edges and soft shadows. The top bar uses 18px blur. The rail and composer use the `FrostedPanel` component with backdrop blur enabled, and solid fallbacks for reduced transparency.

### Shadow Vocabulary

- **Glass:** `0 12px 32px rgba(2,4,6,.24), inset 0 1px rgba(255,255,255,.08)` for dock and prompt glass.
- **Chat:** `0 24px 54px rgba(0,0,0,.16), inset 0 1px rgba(255,255,255,.025)` for the conversation frame.
- **Overlay:** `0 24px 70px rgba(0,0,0,.45)` for the area chooser above a dimmed backdrop.

**The Interactive Glass Rule.** Keep controls readable over translucent surfaces and make glass effects optional through reduced transparency.

## Shapes

Rounded rectangles distinguish the main console, composer, buttons, and dock. The chat and module frames use a 1rem corner, the prompt 1.25rem, and the dock 1.6rem. Hairline borders define most boundaries; dock items have 1rem corners and the send control has a tighter .72rem corner.

## Components

### Buttons

- **Primary:** Pale filled action with dark text; the global primary button uses `button-primary` and `button-primary-hover`.
- **Send:** A darker, raised 3.65rem by 3rem control at the end of the prompt; hover lightens it, active scales to .96, focus has a 2px outline, and disabled is dimmed.
- **Secondary:** Transparent or faintly filled controls with thin borders; hover increases the neutral surface tint.

### Cards / Containers

- **Chat frame:** Translucent dark blue gradient, 1rem corner, hairline border, blur and a soft outer shadow.
- **Area chooser:** Raised neutral panel over a dimmed backdrop; it holds grouped destinations in a three column grid, two on narrower screens.

### Inputs / Fields

- **Prompt:** An expanding textarea occupies its own row inside the glass surface. Normal, Planmodus and Agent remain direct controls above it; attachment, model and send controls sit below it. The Denken slider has five levels.
- **Paperclip control:** Opens the Files area through a small menu; it does not attach a file to the message.

### Navigation

- **Right rail:** All 18 destinations remain visible through vertical scrolling. Labels accompany monochrome SVG icons on desktop; the compact view shows icons with title tooltips.
- **Top bar:** Logo and IAP name lead; model, Vault, and connection status follow. Narrow layouts retain the connection state while hiding the first two status items.
- **Feedback:** StatusLine reports active work and elapsed time without revealing private reasoning. Toast confirms actions at the side; its X and downward swipe dismiss the notice. RecordButton displays real microphone levels and supports a deliberate swipe to discard.
- **Legal and credits:** A Marquee band marks a readable section at the end of Settings; the full text remains static below it.

## Do's and Don'ts

### Do:

- **Do** keep local, Vault, and connection status honest and visible.
- **Do** use thin neutral edges and tonal layering around reading content.
- **Do** preserve keyboard focus outlines and reduced transparency and motion fallbacks.
- **Do** use the existing IAP mark and monochrome line SVGs for navigation.

### Don't:

- **Don't** place decorative glass around every content block.
- **Don't** add permanent sidebars to the central console pattern.
- **Don't** imply that the paperclip attaches files; its current action opens Files and input options.
