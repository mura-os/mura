# spatial-os architecture: spatial accessibility (design note)

**Status:** design note (places workstream) — the documented intent for the accessibility
component the registry marks missing, and the reservation of its protocol surface. Evidence:
[research/37](../research/37-accessibility-atspi.md) (AT-SPI2 anatomy, Newton/AccessKit 2026
status, XR mapping) and [research/38 §7](../research/38-desktop-linux-security-landscape.md)
(the a11y-bus filtering gap). This is a note, not a full design: it fixes the shape and the
seams so later work has a home; the assistive components themselves remain registry gaps.

## 1. The adopted baseline (from doc 37)

- **App content accessibility is unchanged**: Wayland/X apps expose AT-SPI2 trees over the
  accessibility D-Bus; screen readers (Orca) and other ATs run as **service-plane clients**.
  zxr's duty is not to break the bus and to provide what Wayland moved into the compositor.
- **Newton/AccessKit is tracked, not adopted** (experimental/unmerged in 2026). AccessKit's
  Rust, protocol-shaped design is the natural carrier candidate for the spatial extension below
  if/when it matures — re-evaluate at each release wave.
- **The three compositor duties** (doc 37 §7): (1) global focus + geometry context for ATs
  (which window/element has focus, where it is — on Wayland only the compositor knows);
  (2) privileged assistive input (dwell/switch/synthesized events — constraint-7's
  dwell/magnetism stage *is* motor access, one code path); (3) scene-wide semantics for
  effects/shell (reduced-motion = a constraint-6 cap profile; high-contrast = decoration/theme
  profile; visual bell = `xdg-system-bell` handling).

## 2. The spatial-semantics gap (the genuinely new surface)

No existing stack models what an AT needs when the "screen" is a scene
(doc 37's sharpest finding). The **zxr spatial-a11y extension** — reserved here as the third
zxr extension family beside zxr-shell-v2 and zspatial-toplevel-export — exposes, read-only, to
authorized ATs:

- **window poses and spatial relations** ("the terminal is left of the browser, 2 m away"),
  in user-relative terms suitable for speech;
- **place membership and currency** — now definable precisely via
  [places-model.md](places-model.md): the located place, the pager-current place, membership
  lists, entry-policy events ("you have entered the kitchen; recipe app opened");
- **gaze/ray context** (what the ray/gaze hovers, subject to the ADR 0011 privacy posture —
  AT access is a privileged grant, not an app capability);
- **boundary state** (distance/breach — safety-critical for non-visual users);
- **navigation verbs** (privileged: move-focus-to-window, summon-window-to-comfort-zone —
  the AT analog of the switcher's activation path, always through the compositor's authority).

Carrier: a zxr-namespace protocol in the ADR 0012 §4 family (upstream-intent posture per the
ADR 0014 mold), OR an AccessKit-protocol payload if Newton matures first — the *surface* above
is carrier-independent and is what this note fixes.

## 3. Security posture

The a11y bus is a known sandbox-escape surface on desktop Linux
([38 §7](../research/38-desktop-linux-security-landscape.md)); adopting AT-SPI2 imports that
risk. Posture: AT clients binding the spatial extension (and, once an app-sandbox default
exists, the a11y bus itself) are **allow-listed like shell components** (ADR 0012 §5
per-connection filtering; `security-context` identity); gaze context additionally rides the
ADR 0011 privacy boundary. The unresolved base-OS half (bus filtering for sandboxed apps) stays
an open item shared with doc 38's gap list.

## 4. Registry + budget

Registry: the accessibility row's plane placement is fixed by this note (service-plane ATs;
in-zxr duties in the authority plane; spatial extension in the protocol family); status stays
**missing** until components are designed. Budget impact (invariant 9): AT event traffic is
D-Bus-rate, off the frame path; dwell/magnetism already budgeted under the input stabilization
stage; no new frame-path cost.

## 5. Open items

Screen-reader TTS localization in spatial audio (doc 37 §7); magnification model (world-zoom vs
window-zoom vs move-closer — visionOS precedent); the bus-filtering gap (with doc 38); Newton
re-evaluation cadence.
