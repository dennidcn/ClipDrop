---
name: ClipDrop
description: A first-party Windows 11 utility that turns "copy link" into "video saved", built on Fluent at PowerToys craft.
colors:
  accent: "#005fb8"
  accent-dark: "#60cdff"
  accent-text: "#003e92"
  accent-text-dark: "#99ebff"
  on-accent: "#ffffff"
  on-accent-dark: "#000000"
  solid-ground: "#f3f3f3"
  solid-ground-dark: "#202020"
  text-primary: "rgba(0, 0, 0, 0.896)"
  text-primary-dark: "#ffffff"
  text-secondary: "rgba(0, 0, 0, 0.606)"
  text-secondary-dark: "rgba(255, 255, 255, 0.786)"
  text-tertiary: "rgba(0, 0, 0, 0.45)"
  text-tertiary-dark: "rgba(255, 255, 255, 0.544)"
  text-disabled: "rgba(0, 0, 0, 0.3614)"
  text-disabled-dark: "rgba(255, 255, 255, 0.3628)"
  layer-card: "rgba(255, 255, 255, 0.7)"
  layer-card-dark: "rgba(255, 255, 255, 0.0512)"
  layer-card-hover: "rgba(249, 249, 249, 0.5)"
  layer-card-hover-dark: "rgba(255, 255, 255, 0.0837)"
  hairline-stroke: "rgba(0, 0, 0, 0.0578)"
  hairline-stroke-dark: "rgba(255, 255, 255, 0.0698)"
  control-fill: "rgba(255, 255, 255, 0.7)"
  control-fill-dark: "rgba(255, 255, 255, 0.0605)"
  control-stroke-bottom: "rgba(0, 0, 0, 0.1622)"
  control-stroke-bottom-dark: "rgba(255, 255, 255, 0.093)"
  control-strong: "rgba(0, 0, 0, 0.6)"
  control-strong-dark: "rgba(255, 255, 255, 0.5442)"
  control-input: "#ffffff"
  control-input-dark: "#1e1e1e"
  subtle-hover: "rgba(0, 0, 0, 0.0373)"
  subtle-hover-dark: "rgba(255, 255, 255, 0.0605)"
  subtle-press: "rgba(0, 0, 0, 0.0241)"
  subtle-press-dark: "rgba(255, 255, 255, 0.0419)"
  success: "#0f7b0f"
  success-dark: "#6ccb5f"
  critical: "#c42b1c"
  critical-dark: "#ff99a4"
  critical-bg: "#fde7e9"
  critical-bg-dark: "#442726"
  caution: "#9d5d00"
  caution-dark: "#fce100"
  caution-bg: "#fff4ce"
  caution-bg-dark: "#433519"
  caption-close: "#c42b1c"
typography:
  title:
    fontFamily: "Segoe UI Variable Display, Segoe UI, system-ui, sans-serif"
    fontSize: "28px"
    fontWeight: 600
    lineHeight: "36px"
  subtitle:
    fontFamily: "Segoe UI Variable Display, Segoe UI, system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 600
    lineHeight: "28px"
  body-strong:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: "20px"
  body:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: "20px"
  caption:
    fontFamily: "Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: "16px"
rounded:
  control: "4px"
  card: "6px"
  pill: "10px"
spacing:
  xxs: "2px"
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
components:
  button-standard:
    backgroundColor: "{colors.control-fill}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
  button-standard-hover:
    backgroundColor: "{colors.layer-card-hover}"
  button-accent:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
  button-subtle:
    backgroundColor: "transparent"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
  button-subtle-hover:
    backgroundColor: "{colors.subtle-hover}"
  button-icon:
    backgroundColor: "transparent"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.control}"
    size: "32px"
  button-small:
    typography: "{typography.caption}"
    rounded: "{rounded.control}"
    padding: "0 8px"
    height: "28px"
  textbox:
    backgroundColor: "{colors.control-fill}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.control}"
    padding: "0 11px"
    height: "32px"
  textbox-focus:
    backgroundColor: "{colors.control-input}"
  combo-box:
    backgroundColor: "{colors.control-fill}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.control}"
    padding: "0 32px 0 11px"
    height: "32px"
  settings-card:
    backgroundColor: "{colors.layer-card}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.card}"
    padding: "12px 16px"
    height: "68px"
  job-row:
    backgroundColor: "{colors.layer-card}"
    rounded: "{rounded.card}"
    padding: "10px 8px 10px 12px"
    height: "60px"
  job-row-hover:
    backgroundColor: "{colors.layer-card-hover}"
  toggle-switch:
    backgroundColor: "{colors.subtle-hover}"
    rounded: "{rounded.pill}"
    width: "40px"
    height: "20px"
  toggle-switch-on:
    backgroundColor: "{colors.accent}"
  caption-button:
    backgroundColor: "transparent"
    textColor: "{colors.text-primary}"
    width: "46px"
    height: "40px"
  caption-button-close-hover:
    backgroundColor: "{colors.caption-close}"
    textColor: "#ffffff"
  infobar-error:
    backgroundColor: "{colors.critical-bg}"
    rounded: "{rounded.card}"
    padding: "8px 8px 8px 16px"
    height: "48px"
---

# Design System: ClipDrop

## Overview

**Creative North Star: "The First-Party Utility"**

ClipDrop is drawn as if Microsoft shipped it in PowerToys. There is no metaphor and no themed world: the Windows 11 Fluent category standard is played straight, and the craft lives in getting the conventions exactly right (WinUI 3 resource values, the Fluent type ramp, double-stroke focus, settings-card anatomy, 46×40 caption buttons). The window is a receipt, glanced at for a few seconds to paste a link and check what arrived, so it is dense, quiet, and immediately legible.

Depth comes from the material stack rather than shadow: a Mica backdrop (applied natively; the body goes transparent under `html.mica`, otherwise a solid neutral ground), translucent layer cards over it, and hairline 1px strokes. The theme follows the system light/dark setting through `prefers-color-scheme`; every token has a light and dark value. Color is almost entirely neutral; the Windows default accent blue appears only on the primary action, checked toggles, progress, focus underline, and selection.

The system rejects the generic web-app look: no web header, no centered hero input, no footer drawer, no decorative shadows, no brand-color washes.

**Key Characteristics:**
- Native Windows 11 Fluent anatomy: custom 40px title bar, caption buttons at the right, settings cards, InfoBar, ToggleSwitch, ProgressBar, ComboBox.
- WinUI 3 light/dark resource values, switched by the system theme.
- Mica or solid ground, translucent cards, 1px hairlines; no drop shadows.
- Segoe UI Variable on the Fluent ramp (Caption 12, Body 14, Subtitle 20, Title 28).
- Segoe Fluent Icons as the only icon set.
- Fluent motion: 83ms linear state fades, decelerating `cubic-bezier(0, 0, 0, 1)` entrances, all collapsed under reduced motion.

## Colors

A neutral Fluent palette of translucent layers and alpha text, with one system accent used sparingly.

### Primary
- **Windows Default Accent** (accent / accent-dark): the Download button, checked ToggleSwitch track, ProgressBar indicator, TextBox focus underline, text selection, and the info glyph disc. Hover and press are the same hue at 90% and 80% alpha.
- **Accent Text** (accent-text / accent-text-dark): the darker/lighter accent step reserved for accent-colored text on a neutral ground.

### Tertiary
- **Status hues** (success, critical, caution and their -bg fills): job-tile glyph color per state (downloading uses accent, done uses success, error uses critical), error text, and InfoBar severity fills. Caution is defined for InfoBar severity but not yet shown in the build.
- **Caption Close Red** (caption-close): the close caption button hover, fixed across both themes as Windows does.

### Neutral
- **Solid Ground** (solid-ground / solid-ground-dark): window background when Mica is unavailable.
- **Text Primary / Secondary / Tertiary / Disabled**: alpha text ramp. Secondary carries descriptions, meta lines and placeholders; Tertiary carries the inactive-window title and the empty-state glyph.
- **Layer Card** and **Layer Card Hover**: translucent fills of settings cards and job rows.
- **Hairline Stroke**: the 1px border on every card and control.
- **Control Fill / Control Stroke Bottom / Control Strong**: standard control fills, the slightly darker bottom edge that gives Fluent controls their weight, and the strong stroke used by the TextBox bottom edge, toggle track outline and progress track.
- **Subtle Hover / Subtle Press**: fills for transparent (subtle) buttons, caption buttons, and the job-tile backing.

### Named Rules
**The System Accent Rule.** The only chromatic UI color is the Windows default accent. The violet of the placeholder app icon is not a UI color and never appears outside the icon.

**The Paired Theme Rule.** Every color is defined twice, light and dark, from the WinUI 3 resources; a value without its dark twin is not a token.

## Typography

**Display Font:** Segoe UI Variable Display (with Segoe UI, system-ui)
**Body Font:** Segoe UI Variable Text (with Segoe UI, system-ui)
**Icon Font:** Segoe Fluent Icons (with Segoe MDL2 Assets)

**Character:** The operating system's own voice. Display optical size for Title and Subtitle, Text optical size for everything else; weights are only 400 and 600.

### Hierarchy
- **Title** (600, 28px, 36px): page headings ("Downloads", "Settings").
- **Subtitle** (600, 20px, 28px): empty-state headline.
- **Body Strong** (600, 14px, 20px): settings group titles and the list heading.
- **Body** (400, 14px, 20px): card titles, job titles, buttons, inputs; the base `font` of the window.
- **Caption** (400, 12px, 16px): window title, card descriptions, job meta (tabular numerals), field errors, footer, small buttons.

### Named Rules
**The Fluent Ramp Rule.** Sizes come only from the Fluent ramp (12, 14, 20, 28). No in-between sizes, no letter-spacing, no uppercase labels.

## Layout

A single column inside a small window (about 440×640, minimum 360×420). A fixed 40px title bar sits above a scrolling page with 16px side padding (4px top, 20px bottom). Each page opens with a 40px-tall head row: Title heading, with any page action (the Settings icon button) at the right.

Spacing is a 4px rhythm: 4px between stacked cards in a group and between job rows, 8px between controls in a row, 12px between an icon tile and text or between form rows, 16px card side padding and icon-to-text gap, 24px above each settings group or list heading. Text columns truncate with ellipsis rather than wrap (paths truncate from the left), except error meta, which wraps and is selectable.

Pages swap in place; the Settings page replaces Downloads, and a back button appears in the title bar's left edge.

## Elevation & Depth

Flat. There are no drop shadows anywhere in the build. Depth is the Fluent material stack: Mica (or a solid ground) at the bottom, translucent layer cards with 1px hairline strokes above it, controls with a slightly heavier bottom stroke on top. Hover lightens a layer's fill rather than lifting it. The only `box-shadow` in the system is the 1px inner ring of the focus visual.

### Named Rules
**The Material Not Shadow Rule.** Separation comes from fill alpha and hairline strokes. If an element needs to stand apart, change its layer, not its shadow.

**The Double Stroke Focus Rule.** Keyboard focus is a 2px outer stroke (text-primary contrast) offset 1px with a 1px inner contrast ring; inside the title bar the outline is inset (-2px). Focus is never shown on pointer click.

## Shapes

Small, consistent corners: controls (buttons, inputs, combo box, job tile) at 4px, layers (cards, job rows, InfoBar) at 6px, the ToggleSwitch track as a 10px pill with a circular knob. Caption buttons are square-cornered and full title-bar height. Every card and control has a 1px border; standard controls carry a darker bottom edge, and the TextBox's bottom edge becomes a 2px accent underline on focus.

## Components

### Buttons
Quiet, compact, and native.
- **Shape:** gently rounded (4px), 32px tall, 12px horizontal padding, 1px hairline with a darker bottom edge.
- **Standard:** control fill, primary text. Hover lightens the fill; press dims text to secondary and flattens the bottom edge.
- **Accent:** accent fill with on-accent text, white 8% hairline and a 40% black bottom edge. One per view: the Download button.
- **Subtle:** transparent with no border; subtle-hover / subtle-press fills. Used for icon buttons (32×32, 16px glyph) and "Clear finished".
- **Small:** 28px tall, 8px padding, Caption size; used for the per-row "Download" action.
- **Transitions:** background-color 83ms linear.

### Cards / Containers
The settings card is the unit of the system.
- **Corner Style:** 6px.
- **Background:** layer card, 1px hairline stroke.
- **Anatomy:** 20px glyph, title (Body) over description (Caption, secondary), control at the right; minimum 68px tall, 12px 16px padding, 16px gaps.
- **Grouping:** cards stack with 4px gaps under a Body Strong group title.

### Inputs / Fields
- **TextBox:** 32px, 4px radius, control fill, hairline sides with a control-strong bottom edge; placeholder in secondary text. Focus swaps to the opaque input fill and a 2px accent bottom edge, no outline.
- **ComboBox:** same chrome as a standard button with a 12px chevron glyph inset 11px from the right; minimum 120px wide. The dropdown popup is the native select list.
- **Error:** Caption-size critical text 6px below the field.

### ToggleSwitch
40×20 pill with a state label ("On"/"Off") to its left. Off: control-strong outline, 12px secondary-text knob; hover grows the knob to 14px; press stretches it to 17px. On: accent fill, no outline, on-accent knob. Knob motion is 167ms on the decelerating curve.

### Job Row (signature)
A list card in a three-column grid: 32px glyph tile (subtle-hover backing, 4px radius, glyph colored by state), title and Caption meta (platform · percent · speed, tabular numerals), then subtle 32px icon actions. While downloading, a Fluent ProgressBar sits under the meta: 1px control-strong track, 3px accent indicator with 2px radius; indeterminate state slides a 40% indicator. Rows enter newest first with an 8px drop-in over 250ms; hover lightens the fill.

### InfoBar
6px radius, hairline, 48px minimum, 16px left padding. A 16px circular glyph disc (accent for informational, critical for error) leads bold title and message text, with an optional standard button at the right. Error severity uses the critical-bg fill.

### Title Bar
40px custom frame, draggable everywhere except buttons: 16px app icon, "ClipDrop" in Caption (12px left inset), and three 46×40 caption buttons with 10px glyphs. Hover uses subtle-hover; Close hover is caption-close red with white glyph. When the window is inactive, the title and resting caption glyphs drop to tertiary and the icon to 50% opacity. A 40px back button occupies the left edge on the Settings page.

### Empty State
Centered 40px tertiary glyph, Subtitle headline, secondary body limited to 300px.

## Do's and Don'ts

### Do:
- **Do** take every color from the WinUI 3 light/dark resources and define both themes.
- **Do** build new settings as settings cards (glyph, title, description, control at the right) in groups with a Body Strong title.
- **Do** use Segoe Fluent Icons code points for every glyph, with Segoe MDL2 Assets as fallback.
- **Do** keep state transitions at 83ms linear and entrances on `cubic-bezier(0, 0, 0, 1)`, and collapse all motion under `prefers-reduced-motion`.
- **Do** keep one accent button per view.
- **Do** keep the body transparent under `html.mica` so the native backdrop shows through.

### Don't:
- **Don't** add drop shadows, glows, or gradients; depth is layer fill and hairline only.
- **Don't** use colors outside the token set, including the app icon's violet.
- **Don't** introduce sizes off the Fluent ramp, uppercase labels, or letter-spacing.
- **Don't** add a web-style header, centered hero input, or footer drawer.
- **Don't** use radii other than 4px (controls), 6px (layers) and the toggle pill.

### Known gaps (not rules)
- The custom maximize button does not show the Snap Layouts flyout; that needs a native hit-test layer.
- The ComboBox dropdown and `title=` tooltips are browser-drawn, not Fluent flyouts or tooltips.
- Icons rely on the Segoe Fluent Icons font installed with Windows 11 (Segoe MDL2 Assets fallback on Windows 10); nothing is bundled.
- Visual checks render at `preview/*.html` with a mocked Tauri API, regenerated by `preview/build.py`.
