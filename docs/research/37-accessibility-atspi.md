# 37-accessibility-atspi — Accessibility: AT-SPI2/Newton architecture and the XR mapping

**Status:** research complete; architecture input, not an implementation decision. **Date:** 2026-09-23. **Question:** how would AT-SPI be used or extended in spatial-os?

## Executive answer

AT-SPI2 is the shipping Linux desktop accessibility substrate, and spatial-os should adopt it before inventing an XR replacement. Applications expose semantic widget trees over a dedicated D-Bus accessibility bus; Orca consumes names, roles, states, text, focus, relations, and actions and turns them into speech or Braille ([S1], [S2]).
That application path is independent of X11 versus Wayland; Wayland does, however, remove global geometry and unrestricted global input from clients, making the compositor necessary for cross-window accessibility context ([S12], [S17], [S18]).

“Newton” is GNOME's experimental Wayland-native successor architecture. It uses AccessKit's serialized, push-updated trees: apps attach accessibility updates to surface commits, the compositor supplies trusted surface focus/identity, and assistive technologies query local cached trees instead of repeatedly walking remote D-Bus objects ([S9]–[S11]).

As of September 2026, Newton is **not a shipping replacement**. Its end-to-end Wayland, Mutter, Orca, and consumer-library path remains prototype/unmerged.
GTK 4.18 merged an AccessKit backend, but GTK's February 2026 report says Linux still defaults to AT-SPI and GTK-side AccessKit work has seen little movement ([S9], [S13], [S14]). Newton therefore changes the seam spatial-os should preserve, not the initial adoption answer:

1. ship AT-SPI2 and Orca now;
2. make zxr's global accessibility bridge transport-neutral;
3. follow Newton upstream rather than making an unmerged prototype mandatory;
4. design XR spatial semantics beside either transport.

The sharp XR-specific gap is **scene semantics**, not another screen-reader protocol. Neither AT-SPI nor AccessKit represents windows and places in metric 3D, reference frames, field-of-view entry, spatial occlusion, boundary state, or gaze/ray hover.
AT-SPI `Component` geometry is 2D and screen/window/parent-relative; AccessKit has a 2D affine transform and rectangle in window physical-pixel coordinates ([S6], [S15]).
zxr must be the authoritative accessibility source for that spatial layer without replacing each app's content tree.

## 1. Grounding: which “XDG”?

AT-SPI2 is a freedesktop **cross-desktop interoperability stack**: a D-Bus protocol and shared infrastructure used by GNOME, KDE/Qt, GTK, WebKit, LibreOffice, Firefox, and others ([S1], [S2]). That is “XDG” in [desktop-environment §2](../architecture/desktop-environment.md)'s sense (a): freedesktop specifications and conventions.
It is not an `xdg_*` Wayland protocol and not an xdg-desktop-portal API.

AT-SPI wire names use `org.a11y.*`. New compositor helpers use `org.freedesktop.a11y.*` on the ordinary session bus; that is a separate channel and trust boundary from the dedicated AT-SPI bus ([S3], [S17], [S18]).

The local plane vocabulary gives the placement rule:

- applications/toolkits supply widget semantics;
- zxr's authority plane owns global focus, surfaces, transforms, input arbitration, and final-scene effects;
- the service plane owns the accessibility bus, Orca, speech, Braille, audio policy, and settings state;
- the shell plane presents settings, scanning UI, and an on-screen keyboard;
- the perception plane supplies head/hand/eye inputs but never raw gaze through AT-SPI.

This follows [desktop-environment §§1–4](../architecture/desktop-environment.md), [ADR 0011](../architecture/adr/0011-eye-tracking-ipd.md)'s gaze privacy boundary, and [ADR 0012](../architecture/adr/0012-de-modularity-spinout-seams.md)'s module seams.

## 2. How AT-SPI2 actually works

### 2.1 Processes and the separate bus

```text
GTK/Qt/WebKit/app accessibility implementation
        │  org.a11y.atspi.* objects, methods, signals
        ▼
dedicated accessibility D-Bus
        ├── at-spi2-registryd (applications + event subscriptions)
        └── Orca / Accerciser / automation clients
                    ├── Speech Dispatcher → synthesizer → audio
                    └── Braille output
```

`at-spi-bus-launcher` owns `org.a11y.Bus` on the normal session bus. Clients call `GetAddress` at `/org/a11y/bus`; the launcher starts and returns the address of a **separate** `dbus-daemon` or `dbus-broker` accessibility bus ([S3]).
AT-SPI object traffic does not normally use the main session bus; the separate bus exists partly because the protocol is very chatty ([S1], [S3]).

`at-spi2-registryd` owns `org.a11y.atspi.Registry` on the accessibility bus and tracks accessible applications and event-listener registrations.
Assistive technologies register event classes globally or for one app; providers can observe registrations and suppress unused event production ([S4]).
The registry coordinates discovery and events; it is not a central copy of every app's tree.

`org.a11y.Status.IsEnabled` tells applications to activate accessibility dynamically. `ScreenReaderEnabled` is an autostart preference and is not a reliable test for a manually launched AT, so providers should key enablement from `IsEnabled` ([S2]).

### 2.2 Toolkit providers and bridges

An accessible application is a D-Bus server. Its bus name owns object paths forming a tree; callers query objects and receive signals when semantic state changes ([S2]).

Legacy GTK 2/3, GNOME Shell's St toolkit, Firefox, LibreOffice, and Java paths historically expose ATK interfaces in process and use `atk-adaptor`/`at-spi2-atk` to translate ATK into AT-SPI D-Bus ([S1]).
ATK is an in-process toolkit abstraction; AT-SPI is the inter-process contract.
`at-spi2-atk` is therefore one bridge, not the whole accessibility stack.

Modern GTK 4, Qt 5/6, and WebKit map their internal models directly to AT-SPI without ATK ([S1]).
Qt's `QAccessible` bridge activates from AT-SPI status and is independent of whether Qt renders through Wayland or X11 QPA ([S8]).

AccessKit already has a Rust Unix adapter that publishes an AccessKit tree as ordinary AT-SPI with `zbus` ([S11]). Using AccessKit in a Rust client today does **not** imply using Newton.
This gives a smithay/Rust-leaning spatial-os app ecosystem a usable baseline before Newton ships.

### 2.3 Accessible trees and interfaces

`org.a11y.atspi.Accessible` is the base object. It supplies parent/children, name, description, role, state, attributes, supported interfaces, and relations ([S6]).
This hierarchy describes semantic UI structure, not Wayland surfaces or pixels.

Optional interfaces add capabilities:

- `Action`: list and invoke semantic actions such as activate or toggle;
- `Component`: 2D bounds, hit testing, focus, layer, and scrolling;
- `Text`/`EditableText`: text runs, caret, selection, attributes, and edits;
- `Selection`: inspect or alter selected children;
- `Value`: inspect or set a scalar control;
- `Table`/`TableCell`, `Hypertext`/`Hyperlink`, `Image`, and `Document`: domain structure;
- `Collection`: provider-side matching rather than a full linear walk;
- `Cache`: bulk common fields plus add/remove updates ([S6], [S7]).

Relations such as `LABELLED_BY`, `CONTROLLER_FOR`, `FLOWS_TO`, `DESCRIBED_BY`, and `ERROR_MESSAGE` express semantics outside parent/child order and let an AT associate a control with its label, controller, continuation, description, or error without guessing from layout ([S16]).

AT-SPI events cover focus, objects, windows, documents, keyboard, mouse, and terminals. Listeners register strings like `EventClass:major_type:minor_type:detail`; providers signal changes instead of retransmitting the whole tree ([S4]).

### 2.4 Consumers

Orca is the principal shipping screen reader. It uses libatspi—historically through `pyatspi2`—to retain a local object view, follow focus/events, provide structural navigation and “where am I,” then send speech and Braille output ([S1], [S19]).
Accerciser inspects trees, Dogtail uses them for automation, and Rust screen reader Odilia speaks AT-SPI D-Bus directly ([S1]).

“Run Orca” is not equivalent to “implement accessibility.” Orca needs good app and shell trees, the accessibility bus, global Wayland input/surface context, speech/Braille output, and accessible setup/lock UI.

### 2.5 Chattiness, caching, and security

AT-SPI is primarily pull-oriented; a tree walk can trigger many small calls for role, parent, child, state, relations, text, and geometry.
libatspi and adaptors cache common properties; `Cache.GetItems` bulk-loads objects and then follows add/remove updates ([S1], [S7]).

A historical GNOME profiling workload peaked at 699 AT-SPI calls in 100 ms; that benchmark is old and not a current throughput claim, but it demonstrates the small-RPC failure mode ([S5]).
The current at-spi2-core guide still calls the protocol “very chatty” and notes the app→bus→AT context switches ([S1]).

The accessibility bus is privileged: clients can inspect text and invoke actions across applications, while legacy device-event paths can observe or synthesize input.
Flatpak therefore supplies a filtered accessibility-bus proxy rather than unrestricted access to sandboxed apps ([S20]).
An XR extension containing room, gaze-derived, or boundary state must be even more tightly capability-gated.

## 3. What Wayland makes the compositor responsible for

Wayland does **not** replace application content accessibility: a GTK or Qt Wayland client still exposes AT-SPI, and Orca still reads it.
Wayland removes the X11 assumption that an arbitrary client can discover global window geometry, monitor all keys, move the pointer, or identify the toplevel beneath it ([S12], [S17], [S18]).

AT-SPI `Component` still defines screen-, window-, and parent-relative coordinates. GTK on Wayland cannot provide true global screen coordinates and historically falls back to window-relative values with a warning ([S6], [S12]).
Only the compositor can map a surface-local point into the composed scene.

### Load-bearing duty 1: focus, identity, and geometry

The compositor must join an accessible app root to the actual focused/hovered surface and translate between surface-local and global scene context.
Mutter's `org.freedesktop.a11y.PointerLocator` returns the toplevel beneath the pointer, relative surface coordinates, and—when available—the app's accessibility-bus name/object path so Orca can continue hit testing inside the app tree ([S18]).
KWin's 2026 implementation identifies the same compositor bridge and initially reports process identity while awaiting the app↔surface protocol ([S37]).

For zxr this duty expands from a 2D output transform to window→world/place/head reference-frame transforms; it is the prerequisite for “what am I pointing at?”, “where is the focused window?”, and “bring that accessible object into view.”

### Load-bearing duty 2: privileged assistive input

A screen reader needs global commands while an app owns keyboard focus and may consume a command before it changes XKB state.
KWin's `org.freedesktop.a11y.KeyboardMonitor` exists specifically to intercept screen-reader keys before normal input filtering; Mutter has equivalent compositor integration ([S17], [S21]).
This is authority-plane mechanism, never an ordinary Wayland privilege.

Authorization is load-bearing too: a 2026 disclosure showed implementations trusting ownership of claimable `org.gnome.Orca.KeyboardMonitor`; zxr must authenticate a supervised process or portal-issued capability, not a self-asserted bus name ([S21]).

### Load-bearing duty 3: system-wide effects and shell semantics

Magnification, visual bells, pointer/ray emphasis, and global reduced-motion behavior need the final scene and execute in the compositor/effects layer.
GNOME's magnifier is built into GNOME Shell's compositor scene; KWin ships in-process Zoom and Magnifier effects ([S22], [S23]).

The compositor or trusted shell must also expose launcher, overview, decorations, lock, greeter, OSD, and boundary UI as accessible content.
Application AT-SPI trees cannot describe desktop-owned presentation.

These duties do not mean zxr should mirror every app widget.
Today it should preserve the app's accessible identity and broker global context.
Under Newton it additionally relays synchronized updates, while the toolkit remains the source of buttons, text, tables, and actions.

## 4. Newton and AccessKit

### 4.1 Protocol shape

The successor design is push-based.
Providers send an initial full tree and incremental updates; every AT keeps a local tree and queries it without synchronous provider round trips.
Only state-changing requests—focus, activate, edit, scroll—return to the provider ([S10]).

AccessKit supplies the cross-platform schema.
A `TreeUpdate` carries changed nodes, tree metadata, tree identity, and current focus.
Changed nodes are sent in full; graft nodes compose subtrees from multi-process applications ([S11]).
The canonical schema/adapters are Rust, but generated bindings let other toolkits produce the same updates ([S11]).

Newton's prototype has two legs:

```text
toolkit / AccessKit provider
    ── Wayland surface accessibility update ──► compositor
compositor
    ── privileged D-Bus + serialized updates ──► Orca / AT
```

An accessibility update is committed atomically with the corresponding `wl_surface` state.
The compositor tells providers when an AT is interested, avoiding tree cost while accessibility is inactive ([S9]).
Providers use surface-relative coordinates and cannot assert global focus; the compositor supplies surface enumeration, focus, transforms, and security ([S9], [S10]).

The prototype keeps ATs on privileged D-Bus rather than exposing global data to sandboxed Wayland apps.
Mutter passes updates without interpreting widget semantics ([S9]).
That is the right division for zxr.

### 4.2 Benefits and limits

Push removes tree-query round trips, lets an AT inspect the last snapshot while an app UI thread is hung, and synchronizes semantics with visuals ([S10]).
It risks large initial trees and update cost for documents, tables, and virtualized views.
The design acknowledges that some providers may remain on AT-SPI indefinitely ([S10]).

AccessKit is not a 3D schema.
Its geometry is a 2D `Rect` plus six-coefficient affine transform, resolved in physical pixels relative to a tree container ([S15]).
Newton's per-surface push model is an excellent transport shape, but cannot directly encode world pose, oriented 3D bounds, reference frame, solid angle, place, or boundary proximity.

### 4.3 September 2026 status

The 2024 prototype demonstrated Orca with real GTK 4 apps, keyboard commands, mouse review, flat review, and action invocation.
Only Orca was supported; in-shell ATs such as the magnifier were not ([S9]).
GNOME described the Wayland protocol, Mutter, Orca, AccessKit, and consumer-library branches as unmerged and said Newton was not ready to upstream ([S9]).

GTK 4.18 later shipped AccessKit, primarily enabling GTK accessibility on Windows/macOS.
On Linux it still maps through AT-SPI unless selected otherwise ([S13]).
GTK's February 2026 report says AT-SPI remains the Linux default, AccessKit GTK work has stalled, and AT-SPI itself lacks proper role/event feature negotiation ([S14]).

**Verdict:** package AT-SPI2 first, accept AccessKit clients through its AT-SPI adapter, preserve a Newton-compatible compositor seam, and do not block XR a11y design on Newton.

## 5. Assistive-feature inventory

AT-SPI is one subsystem, not the whole accessibility product.
Mature desktops combine semantic access with compositor effects, input filters, themes, settings, audio policy, and alternative-input clients.

### Vision

- **Screen reader/Braille:** Orca consumes AT-SPI; the compositor contributes focus/input/surface context ([S1], [S17], [S18]).
- **Magnification:** GNOME composes zoom regions in Shell; KWin has in-process Zoom and Magnifier effects with pointer tracking ([S22], [S23]).
- **High contrast and large text:** GNOME exposes both as settings spanning toolkit/theme, fonts, and shell chrome rather than AT-SPI calls ([S24]).
- **Reduced motion:** GNOME exposes a preference; GNOME Shell 51 replaces scale/translation window animations with fades when active ([S25]).
- **Pointer and color aids:** GNOME Zoom includes crosshairs, inversion, brightness, contrast, and color filters; KWin has track-mouse/effect equivalents ([S22]–[S24]).

### Hearing/audio

- **Visual bell:** GNOME can flash a window/full screen; Plasma can invert or flash a chosen color ([S24], [S26]).
- **Wayland request:** `xdg_system_bell_v1.ring(surface)` leaves audible, visual, other, or no feedback to the compositor ([S27], [research 30 A5](30-wayland-de-anatomy-protocol-seams.md)).
- **Mono/balance:** WirePlumber gained force-mono output policy in 2026, but the DE still needs settings/UI ([S28]).
- **Directional captions:** when sound location matters, captions need source/direction; visionOS explicitly requires this ([S31]).

### Motor/alternative input

- **Sticky/slow/bounce keys:** under Wayland these are compositor input filters, not libinput features; KWin implements them in its input pipeline, with Slow Keys targeted at Plasma 6.6 ([S29]).
- **On-screen keyboard:** Squeekboard is a separate client requiring layer-shell and virtual-keyboard-v1 and recommending input-method-v2; Maliit is another deployed framework ([S30]).
- **Switch access:** archived/X11-era Caribou provided one/two-switch scanning; no established modern cross-desktop Wayland replacement exists ([S32]).
- **Voice control:** Numen provides local system-wide Linux voice control through `uinput`; useful precedent, but its broad device privilege is not the desired zxr seam ([S33]).

The inventory argues against one monolithic a11y daemon.
Semantic access, input mechanism, scene effects, theme policy, audio routing, and UI have different authority and failure domains.

## 6. XR precedent

visionOS treats accessibility as spatial platform behavior.
VoiceOver navigates spatial content with alternate pinches; apps label RealityKit entities and announce scene contents/meaningful changes ([S31]).
VoiceOver uses spatial audio for object location, while Direct Gesture mode arbitrates whether VoiceOver or the app receives hand gestures ([S31]).

visionOS Dwell Control provides eye-only tap, scroll, long press, and drag.
Pointer Control offers alternatives to eye-plus-hand input; Reduce Motion asks apps for static or crossfade alternatives ([S34]).
Zoom offers whole-view or movable-window magnification, may include apps, surroundings, or both, and can lock depth to the user's hands ([S35]).
Users can separately move windows closer or farther for comfort ([S35]).

Meta Quest exposes setup-time accessibility, text-to-speech/screen reader, contrast, text size, live captions, remapping, height adjustment, mono, and balance ([S36]).
Its screen-reader documentation has described it as experimental, warning that a feature list is not proof of complete app coverage ([S36]).

Transferable requirement: scene navigation, alternate pointing/activation, bounded motion, digital and surroundings magnification, and audio fallback when spatial hearing is unavailable are OS responsibilities.

## 7. Mapping to spatial-os

### 7.1 Keep application content accessibility unchanged

For ordinary Wayland applications:

- ship `at-spi-bus-launcher`, `at-spi2-registryd`, libatspi, toolkit bridges, Orca, Speech Dispatcher, and Braille integration;
- preserve the dedicated accessibility bus and filtered sandbox access;
- let GTK, Qt, WebKit, Firefox, LibreOffice, and AccessKit clients expose native AT-SPI trees;
- run Orca as a service-plane client, not inside zxr;
- preserve app-root identity across move, place switch, summon, dock, and restore;
- forward focus/surface context rather than copying every widget into zxr.

This fills the registry's “nothing exists anywhere” row while respecting [desktop-environment §3](../architecture/desktop-environment.md) and [component registry §6](../architecture/component-registry.md).

### 7.2 zxr supplies spatial semantics

zxr should expose a small compositor-owned **spatial accessibility scene**, not a duplicate app-widget tree:

```text
spatial scene
 ├─ active/inactive place summaries
 │   ├─ window host → app AT-SPI/AccessKit root
 │   └─ trusted decoration/action
 ├─ system surfaces (launcher, notifications, OSD, lock)
 └─ boundary state / safety affordance
```

A window host needs:

- stable toplevel identity and app accessible-root link;
- place membership and active/inactive state;
- world/head/body reference frame;
- metric pose, oriented bounds, angular size, and distance;
- visible, occluded, outside-view, minimized, or privacy-hidden state;
- keyboard focus and stabilized assistive ray/gaze-hover;
- focus, summon, comfortable-view, and describe-location actions.

Useful semantic events are:

- window entered or left view;
- focus moved between windows/system UI;
- active place changed;
- window was summoned, restored, or autonomously moved;
- boundary changed among safe/warning/breached;
- stabilized assistive hover target changed.

Never emit raw gaze vectors, samples, history, or confidence.
ADR 0011 requires opt-in gaze and confines eye data to perception; accessibility gets a derived, rate-limited target only after zxr stabilization/arbitration ([ADR 0011 §4](../architecture/adr/0011-eye-tracking-ipd.md)).

### 7.3 Extension shape

Do not mint private AT-SPI roles such as `XR_WINDOW` or reinterpret screen coordinates as metres.
AT-SPI's missing feature negotiation makes private enum growth brittle; changing 2D field meanings breaks existing consumers ([S14]).

Prefer a companion, versioned zxr interface keyed by app root and toplevel identity:

- AT-SPI2 phase: service-consumer D-Bus API plus zxr authority events;
- Newton phase: compositor-owned spatial tree/side metadata keyed by surface or AccessKit `TreeId`;
- common schema: 3D transform, reference frame, place, visibility, spatial actions;
- explicit capability negotiation and privacy classes;
- no general scene-control authority for the AT.

Newton can carry app trees and zxr can publish its own scene as another provider.
Because AccessKit lacks 3D fields, prototype side metadata instead of forking its core schema ([S10], [S15]).

### 7.4 Motor

Composition constraint 7 already requires deadzone, smoothing, dwell, magnetism, and class-aware arbitration before gaze/ray hover ([composition §7.3](../architecture/zxr-shell-v2-composition.md)).
Use that one stage for ordinary pointing, Dwell Control, head-only pointing, and switch scanning.

KWin VR proved headgaze viability but shipped raw distance-ordered picking without smoothing/magnetism; users explicitly requested both ([research 31 §2.11](31-kwin-vr.md)).
Support head-only and one-handed operation from the first interaction milestone.
Dwell progress belongs in trusted chrome and must be cancelable without precision motion.

Switch scanning should be service/shell policy over the spatial scene.
It requests next/previous/activate through a capability-limited zxr API, not unrestricted `uinput`.
Voice control should invoke named semantic actions, falling back to text/keys only when no action exists.

Constraint 8 requires follow/grab/recenter/summon/dwell to share one arbitration state machine.
Constraint 9 requires one settings declaration for dwell timing, magnetism, target size, and motion caps rather than divergent runtime/UI defaults ([composition §7.3](../architecture/zxr-shell-v2-composition.md)).

### 7.5 Vision

XR magnification needs three modes:

1. **window/content zoom** — enlarge one plane or a lens inside it while preserving app-local focus/coordinates;
2. **view lens** — magnify a bounded region of the composed digital scene with a stable reticle;
3. **surroundings zoom** — magnify passthrough with perception-layer sampling and depth handling.

visionOS establishes whole-view versus window-lens choice and apps/surroundings/both selection ([S35]).
Never implement world zoom by changing head-pose gain or scaling tracking motion.
Moving a window closer is a separate placement action under distance/angular-size caps.

High contrast is a profile spanning toolkit settings, shell assets, compositor decorations, ray/cursor affordances, and optional final-scene filters.
It must preserve depth and boundary legibility, not merely swap a GTK theme.

Reduced motion maps directly to constraint 6 and ADR 0012's authority-owned effects caps.
Select a stricter cap profile, replace nonessential translation/scale with fades or instant transitions, expose the preference to apps, and prevent plugins/policies from exceeding it ([composition §7.3](../architecture/zxr-shell-v2-composition.md), [ADR 0012 §2](../architecture/adr/0012-de-modularity-spinout-seams.md)).

### 7.6 Hearing/audio

Default TTS should be head-locked/centered and intelligible, with app audio ducked or masked.
Short earcons may localize a source window, but navigation must not depend only on localization because mono removes the cue.

WirePlumber policy should expose mono downmix and left/right balance.
Under mono, verbalize relative location (“terminal, left, two metres”) or use nonspatial earcon differences.
Spatial captions need optional source/direction labels ([S28], [S31], [S36]).

Implement `xdg-system-bell-v1`.
For a surface-associated ring, outline that window, emit a bounded head-locked earcon, and optionally haptic-pulse the active controller.
Obey flash/luminance and reduced-motion caps; flashing the entire stereo view is a poor XR translation ([S27], [research 30 A5](30-wayland-de-anatomy-protocol-seams.md)).

### 7.7 Plane placement

| Piece | Plane | Reason |
|---|---|---|
| app AT-SPI/AccessKit provider | application | app owns widget meaning/actions |
| accessibility bus + registry | service | session IPC, no scene authority |
| Orca, speech, Braille | service | replaceable AT consumers |
| a11y settings controller | service | per-user policy/persistence |
| settings UI, scanner, OSK | shell | presentation over constrained seams |
| focus/surface/transform bridge | authority/zxr | only compositor has scene truth |
| spatial accessible scene | authority/zxr | places, windows, visibility, boundary |
| dwell/stabilization/input filters | authority/zxr | pre-dispatch arbitration |
| magnification/contrast/bell | authority effects | final-scene work under safety caps |
| mono/balance/TTS ducking | service/audio policy | audio graph policy |
| raw head/hand/eye sensing | perception | exports derived input, never raw gaze |

### 7.8 Registry rewrite implied

Replace the single accessibility row with:

1. **AT-SPI2 substrate + Orca/speech/Braille** — service daemons, standard a11y D-Bus, **adoptable now; partial until integrated/tested**.
2. **zxr global accessibility bridge** — focus, toplevel identity, PointerLocator/KeyboardMonitor-class capability, authority plane, **missing**.
3. **zxr spatial accessibility scene/schema** — authority source with service-consumer seam, **missing; requirements here, wire design absent**.
4. **assistive input policy** — authority mechanism plus replaceable scan/voice clients, **partial from constraints 6–9**.
5. **visual accessibility profile/effects** — authority effects plus settings/theme policy, **partial from ADR 0012; no magnifier design**.
6. **audio accessibility policy** — WirePlumber/service plane, **missing**, shared with audio row.
7. **accessibility settings UI/persistence** — shell + settings service, **missing**, shared with settings row.

This preserves ADR 0012:
ATs, settings UI, OSK, scanner, and voice controller are replaceable processes.
Global input, scene transforms, final effects, and comfort enforcement remain in zxr.

## 8. Open questions

1. Which broker authenticates ATs without the claimable-name flaw: systemd supervision, portal capability, executable measurement, or inherited socket?
2. How is an AT-SPI root joined robustly to `xdg_toplevel`, multi-process browsers, Xwayland, popups, delegated toplevels, and restored sessions?
3. Does the spatial scene ride the accessibility bus, private zxr D-Bus, or future Newton AT channel?
4. Which spatial fields belong upstream in AccessKit versus a zxr companion schema?
5. What verbal vocabulary describes place, distance, clock direction, occlusion, and boundary warning?
6. Does assistive gaze require consent separate from ordinary app gaze, and what works before consent?
7. How are greeter, lock, shell, and boundary trees tested with no ordinary app?
8. What are the dwell latency, target expansion, angular motion, flash, contrast, and TTS interruption budgets?
9. How does surroundings zoom preserve passthrough latency, stereo/depth cues, and boundary visibility?
10. Where do TTS/earcons localize, and how does policy degrade under mono or unilateral hearing loss?
11. Which AT-SPI/Orca corpus becomes a NixOS qualification test, and which XR scene tests are added?
12. Can Newton's accessible remote-display goal compose with spectate/workspace privacy filtering ([S10])?

## Sources

[S1]: https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/architecture.html
[S2]: https://freedesktop.org/wiki/Accessibility/AT-SPI2/
[S3]: https://github.com/GNOME/at-spi2-core/blob/main/bus/README.md
[S4]: https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/doc-org.a11y.atspi.Registry.html
[S5]: https://wiki.gnome.org/Accessibility%282f%29Documentation%282f%29GNOME2%282f%29ATSPI2%282d%29Investigation.html
[S6]: https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/xml-interfaces.html
[S7]: https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/doc-org.a11y.atspi.Cache.html
[S8]: https://doc.qt.io/qt-6/qaccessible.html
[S9]: https://blogs.gnome.org/a11y/2024/06/18/update-on-newton-the-wayland-native-accessibility-project/
[S10]: https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/new-protocol.html
[S11]: https://github.com/AccessKit/accesskit/blob/main/ARCHITECTURE.md
[S12]: https://lists.gnome.org/archives/commits-list/2020-October/msg06788.html
[S13]: https://blogs.gnome.org/gtk/2025/05/12/an-accessibility-update/
[S14]: https://blogs.gnome.org/gtk/2026/02/06/gtk-hackfest-2026-edition/
[S15]: https://docs.rs/accesskit/latest/accesskit/struct.Node.html
[S16]: https://docs.gtk.org/atspi2/enum.RelationType.html
[S17]: https://invent.kde.org/plasma/kwin/-/merge_requests/7300
[S18]: https://github.com/GNOME/mutter/commit/62f6fa2e0c8b7e6e42326651fa58c0f67646d42b
[S19]: https://help.gnome.org/users/orca/stable/
[S20]: https://github.com/flatpak/flatpak/issues/79
[S21]: https://linnemanlabs.com/posts/hello-my-name-is-orca/
[S22]: https://wiki.gnome.org/Projects%282f%29GnomeShell%282f%29Magnification.html
[S23]: https://invent.kde.org/plasma/kwin/-/tree/master/src/plugins/zoom
[S24]: https://help.gnome.org/gnome-help/a11y.html
[S25]: https://github.com/GNOME/gnome-shell/commit/29dad874e0cd59522662982c9a1926c94b90996f
[S26]: https://docs.kde.org/stable_kf6/en/plasma-desktop/kcontrol/kcmaccess/
[S27]: https://wayland.app/protocols/xdg-system-bell-v1
[S28]: https://arunraghavan.net/2026/01/accessibility-update-enabling-mono-audio/
[S29]: https://invent.kde.org/plasma/kwin/-/merge_requests/8491
[S30]: https://gitlab.gnome.org/World/Phosh/squeekboard
[S31]: https://developer.apple.com/videos/play/wwdc2023/10034/
[S32]: https://packages.debian.org/bookworm/gnome/caribou
[S33]: https://git.sr.ht/~geb/numen
[S34]: https://support.apple.com/guide/apple-vision-pro/dwell-control-tan0ba69a1f1/visionos
[S35]: https://support.apple.com/guide/apple-vision-pro/zoom-tan563db5e24/visionos
[S36]: https://www.meta.com/help/quest/674999931400954/
[S37]: https://invent.kde.org/plasma/kwin/-/merge_requests/9825
