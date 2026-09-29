# The shell plane — components as processes, the compositor's shell-layer half, and the toolkit

**Status: DRAFT, rev 0.7 (2026-09-29; rev 0.6 + the body frame withdrawn — §2.6 the **world** fallback on the shell's anchor, seeded where the scene appears and re-seated only by recenter; every `body` frame in §3 is `world`; no comparable had a body frame, research/78 §9 F23). Rev 0.6 (2026-09-28; rev 0.5 + §3.2 `mura-osk` built — the OSK on the platform's `input-method` feature, squeekboard's shape, measured nested at G2). Rev 0.5 (2026-09-28; rev 0.4 + the body frame as the **default placement** — nothing head-locked unless it asks (§2.6, §3.1, §3.4, §3.6; spatial-input §13) — and the input floor as one rule for every surface). Rev 0.4 (2026-09-28; rev 0.3 + the body frame built and the OSK bound to the surface it types into — §2.6 the `typed` seed and the body frame's definition, §3.2 the OSK's frame, §6 the seed-provenance item; spec §4/§5 rev 3.14). Rev 0.3 (2026-09-28; rev 0.2 + ADR 0007 amendment 2 and research/78 — §2.3 greeter
mode is the socketpair kiosk, the lock is an `ext-session-lock` client under a user unit; §3.1 the
program's modes, unit, seams and surface roles; §3.2 the OSK as zxr's child in every mode (KWin's IM
shape); §4 the sctk platform with the AccessKit bridge and the Stage B gate; §6 items).
Rev 0.2 (2026-09-27; rev 0.1 + §2 the compositor's half made mechanical from
[research/77](../research/77-shell-layer-mechanics-from-comparables.md) — arrangement per frame in
frame-pixel space, the initial configure, the focus rules, the trusted connection as the gate's
exception, the filter as `ClientData` bits at insert, the still-pointer rule's placement; §2.6 the
wearer's placement table `shell.place:<namespace>` — owner ruling 2026-09-27; §3.5 OSD and
notifications may be separate, Mura's own likely merged — owner ruling; §6 the items the rulings
leave).** Derived from [research/75](../research/75-shell-plane-from-comparables.md)
(how the shipping shells are built; the toolkit measured) on top of [research/30](../research/30-wayland-de-anatomy-protocol-seams.md)
(the seams), [research/36](../research/36-vr-shell-interaction-patterns.md) (the XR interaction
patterns) and [research/60](../research/60-de-abstractions-mapped-to-xr.md) (each abstraction mapped
to XR), under [ADR 0012](adr/0012-de-modularity-spinout-seams.md) as amended ("every shell component
is its own process over a standard seam, placed through anchoring frames") and
[ADR 0007](adr/0007-session-greeter-lock.md) as amended (the greeter/lock scene is a trusted client).
The five owner rulings of research/75 §7 (2026-09-27) are written in. Design docs specify; ordering
lives only in [implementation-path.md §5](implementation-path.md).

**What this document is.** The shell plane ([desktop-environment.md §3](desktop-environment.md)) is
the presentation layer of the desktop: the greeter and lock scene, the on-screen keyboard, panels
and status surfaces, the OSD, notifications, the launcher, the task switcher, the places overview,
and the decoration chrome. This document fixes, for every component, the same contract — what
process it is, how it is started, which seams it binds, where it is anchored, what it owes
accessibility and the input floor, which settings it reads, what it costs — and fixes the
**compositor's half**: what zxr must serve and enforce for any of them to exist. It names the
toolkit Mura's own components are written with and the components Mura carries instead.

**Grounding.** Wayland terms are the protocols' own: `wlr-layer-shell-unstable-v1` (layers
`background`/`bottom`/`top`/`overlay`, *exclusive zone*, *keyboard interactivity*
`none`/`exclusive`/`on_demand`, *namespace*), `ext-session-lock-v1`, `zwp_input_method_v2`,
`zwp_virtual_keyboard_v1`, `zwp_text_input_v3`, `ext-foreign-toplevel-list-v1`, `ext-workspace-v1`,
`xdg-activation-v1`, `wp_security_context_v1`; D-Bus interfaces are the freedesktop specs'
(`org.freedesktop.Notifications`, StatusNotifierItem/Watcher/Host, `org.a11y.Bus` / AT-SPI2,
`org.freedesktop.PolicyKit1.AuthenticationAgent`); "XDG" here means the Wayland `xdg_*` namespace
for `xdg-activation`/`xdg-decoration` and the freedesktop *specifications* (desktop entries, base
directories) for entries and configuration — never xdg-desktop-portal, which is the service
plane's. Frames are [places-model.md](places-model.md)'s and `protocols/zxr-layer-anchoring-v1.xml`'s
(`head`, `hand_left`/`hand_right`, `world`, `docked`; XrSpace-grounded; rev 0.7: no `body`). systemd unit
vocabulary is systemd's (`PartOf=`, `Restart=`, `graphical-session.target`).

**Budget impact** (overview invariant 9; [budgets.md §3](budgets.md): the shell plane is
*damage-driven only; zero steady-state CPU wake-ups when idle; panels/OSDs never animate uncapped*).
Each Mura-written component is one Slint process: measured **12.5 MB stripped binary, 20.5 MB RSS /
12.6 MB PSS / 3.0 MB private-dirty** with a full auth scene mapped, 6 threads, 0 CPU and 0 wake-ups
idle when nothing is focused and the compositor sends no motion, 2 commits/s with a caret
(research/75 §5.4, host). The plane's process count is therefore the budget lever: **§3 fixes five
long-lived processes at most in a normal session** (greeter-or-lock is not resident; OSK, panel +
status + tray host, notifications (mako, carried) , OSD + polkit agent, launcher) ≈ 45–60 MB PSS
aggregate if nothing beyond glibc is shared — the same order as zxr. The compositor's half adds no
thread and no pass: layer-shell surfaces are members of bands 2/4/5 of the existing layer list
(spec §4), anchoring is a frame choice the scene already models, the binding filter is a
per-connection predicate on global advertisement, and the restricted-mode admission is one fd. The
one compositor cost found is a *saving*: motion dedupe for a still pointer (§2.5).

## 1. The rule

1. **A shell component is a process.** It renders into an ordinary Wayland client surface and binds
   exactly the privileged seams its job needs (ADR 0012 §5). The compositor draws no shell UI of its
   own: not the greeter, not the lock, not an OSD (ADR 0007 as amended; research/75 §2: every
   shipping shell but GNOME's, whose premise — the compositor *is* the toolkit host — is not ours).
   The exceptions ADR 0012 keeps are not UI: decoration *hit volumes* and the cursor layer are the
   compositor's (§3.9).
2. **It is a systemd user unit** (ruled Q1): `PartOf=graphical-session.target`,
   `After=graphical-session-pre.target`, `Restart=on-failure`, `OOMScoreAdjust` as the desktops set
   it, `WantedBy=` `mura-session.target` for the session set; the greeter's set is wanted by the
   greeter's own target. A component the session cannot be used without is `Requires=`d
   (the compositor only — [specs/session-bootstrap.md](../../specs/session-bootstrap.md)); every
   shell component is `Wants=`, so its failure never ends the session (Plasma's krunner `Restart=no`,
   GNOME's `OnFailure=` are the two shapes the comparables use for the two cases; Mura uses
   `Restart=on-failure` for all and `OnFailure=` for none). No Mura process supervises another
   process's restarts (not COSMIC's `ProcessManager`, not kscreenlocker's four tries).
3. **It qualifies by connection or by channel** (ruled Q2 — research/30's rule): a client on the
   public socket is *unrestricted* unless it presents a `wp_security_context_v1`, in which case the
   privileged globals are not advertised to it (niri, Hyprland, cosmic-comp); the two components that
   run before any public socket exists or must be the only client — the greeter/lock program and the
   OSK — are **spawned by zxr over a pre-connected socketpair** (`WAYLAND_SOCKET`; kscreenlocker's
   greeter, KWin's input method) and are trusted because zxr gave them the socket. No allow-list of
   binary names exists; a third-party panel the administrator installs is as unrestricted as Mura's.
4. **It is placed by a frame, not an output edge.** Layer-shell keeps its four layers and its
   exclusive-zone and keyboard-interactivity semantics; `zxr-layer-anchoring-v1` adds the frame
   (`head`, `hand_*`, `world`, `docked`) and the *exclusive angular band* that is the
   exclusive zone's XR form. A layer-shell client that knows nothing of anchoring is placed on the
   **head** frame (the protocol's default, `zxr-layer-anchoring-v1.xml:118-120`), so waybar, mako or
   squeekboard run unmodified.
5. **It is replaceable by the administrator** (rule 3, overview invariant 10): every component is a
   unit that can be masked, overridden with a drop-in `ExecStart=`, or substituted by any program
   that binds the same seams (greetd's "the greeter is any program"; phosh's OSK is whoever owns
   `sm.puri.OSK0`). Its look is its own — theming lives in the component and its settings keys, never
   in the compositor.
6. **Its settings are keys declared when it lands** (research/73's rule): a key exists exactly once
   and names its consumer; a component that is designed but not built has no keys.
7. **Mura's own components are written in Slint** (ruled Q3; research/75 §5.5): the software
   renderer, `backend-winit-wayland`, `accessibility`, AOT-compiled `.slint`, GPLv3. Components are
   co-located where the comparables co-locate (§3). Components Mura *carries* keep their toolkit
   (mako, squeekboard).

## 2. The compositor's half (zxr)

What zxr serves and enforces so that any component of §3 can exist. Spec [zxr-core.md](../../specs/zxr-core.md)
§4 (bands), §8, §9 and §10 are the normative home; this section is their shell-plane content.

### 2.1 Layer-shell with anchoring

zxr serves `zwlr_layer_shell_v1` (v5) and `zxr_layer_anchoring_v1` (spec §10; the mechanics are
spec §4 rev 3.12, from research/77). A layer surface is a member of band 2 (`bottom`), 4 (`top`)
or 5 (`overlay`) of the layer list (spec §4); `background` is the environment's band 1 and is the
wallpaper client's (accepted, not composed until the environment design admits it — spec §14).
Its frame is the anchoring request's or **head** by default. **Arrangement** is wlroots'
arithmetic in sway's pass order, run per frame in the frame's pixel rectangle on the surface's
commit/map/unmap and never per tick: the usable rectangle is per frame (a world-frame panel never
shrinks the head frame); a positive zone reserves `zone + margin` on the surface's one exclusive
edge; `set_exclusive_angle` is the same zone in degrees; the client is configured with the
arranged pixel size on its first commit *after* arranging, so unaware clients' own sizing
(squeekboard's height from `wl_output`, gtk-layer-shell's auto zone) holds unmodified. The window
tiers honour the head frame's usable rectangle at spawn and are not moved when a band appears.
**Keyboard interactivity** keeps the protocol's meaning as every comparable implements it
(spatial-input §6 rev 0.5): `exclusive` on `top`/`overlay` is the focus override while mapped
(greeter, lock), `on_demand` is a focus-stack member (launcher), `none` is never focused (panel,
OSD, notifications, OSK). Layer surfaces are **placeable, never tiled or resized**: the WM engines
ignore them (no resize grab, no arrangement), a grab on one writes its placement row (§2.6), and
they are never targets of `ext-foreign-toplevel-list`; they are hit-tested as planes with the
shell class (bands 4–5, `input/hit.rs`).

### 2.6 The placement table — where a shell surface sits is the wearer's

**Ruled (owner, 2026-09-27; research/77 §3.3a):** the wearer places shell surfaces and chooses
their frame; no component's position is hardcoded because a design discussion settled it. The
mechanism is Hyprland's layer rules transposed — a rule matches the surface's **namespace** (the
string every layer-shell client sends) and overrides the client (`hyprland/src/desktop/rule/layerRule/LayerRule.cpp:96-115`).
The table is the relocatable settings template **`shell.place:<namespace>`** with keys `frame`
(any frame the compositor advertises), `azimuth_deg`, `elevation_deg`, `distance_m`, `pitch_deg`,
`width_deg` (0 = compositor's choice). **Precedence:** a row wins over the client's
`zxr-layer-anchoring-v1` request; without a row the client's request applies; without either the
**world fallback** (rev 0.7, ruled 2026-09-29; rev 0.5 had said "body": the world frame's rectangle
is the head's extent at its distance, `shell.head.{extent_h_deg,extent_v_deg,distance_m}`, 90×70° at
0.5 m — the anchoring protocol's own example and WiVRn/WayVR's distance — hung off the shell's **world
anchor**: the head's position and heading captured when the scene first appears, re-seated only by
recenter, spec §5). Nothing is head-locked unless it asks, and nothing follows unless the wearer
turns following on: an aimed-at scene stays where it was summoned (visionOS, Android XR, HoloLens,
SteamVR's dashboard, wayvr's `Floating` — spatial-input §13); the lock surface is placed the same way. A grab on a shell plane (the WM
branch's grab mechanics) writes the row; `mura-settings set shell.place:osk.frame head` is the same
write by hand; `Reset`/`DeleteInstance` returns to the seed. **Seed rows** (the consumer's defaults
for an instance without a stored value, GNOME's shape): `osk` → **`typed`** — the frame of the
surface it types into (research/36 §7's convergence: WiVRn, xrdesktop, visionOS, Quest all bind
the keyboard to the focused panel; wayvr's body-anchored keyboard, the previous seed's source, was
the outlier — rev 0.4, spec §4 rev 3.14); `notifications` → head, upper-right (mako's anchor;
head-locked toasts, research/36 §4); a bar → world, bottom (research/60 §9 — *the same provenance
the OSK seed had; a rethink candidate, §6*). **There is no body frame** (rev 0.7; spec §5;
research/78 §9 F23): rev 0.4–0.6 built one — the head's position every tick, its yaw re-seated
lazily — and no comparable has it (OpenXR's spaces, visionOS, HoloLens, Android XR, SteamVR, wayvr
are all world + recenter, with following opt-in); the world anchor replaces it and `wm.follow.*`
stays the wearer's opt-in per surface (Q6). The frame set grows with the compositor (`frames` bitfield): a keyboard on the real desk is `frame = world` with
a pose now and a surface-detected frame when the perception plane offers one. Unaware clients
(squeekboard, mako, waybar) are placed by the same rows — the namespace is theirs already.

### 2.2 The binding filter

One predicate per connection decides whether the privileged set is advertised: **restricted** iff
the client carries a `wp_security_context_v1` (research/30 §binding; niri `client_is_unrestricted`,
Hyprland's whitelist, cosmic-comp `not_sandboxed()` — research/75 §4.2). The privileged set:
`zwlr_layer_shell_v1`, `zxr_layer_anchoring_v1`, `ext_session_lock_manager_v1`,
`zwp_input_method_manager_v2`, `zwp_virtual_keyboard_manager_v1`, `ext_foreign_toplevel_list_v1`,
`ext_workspace_manager_v1` + `zxr_workspace_v1`, `zwlr_data_control_manager_v1`,
`zxr_window_management_v1` (the WM seam), the export/capture managers, and the perception intake.
Everything else (`xdg_wm_base`, seat, shm/dmabuf, `xdg_activation_v1`, `zwp_text_input_manager_v3`,
`xdg_decoration`, viewporter, fractional scale, cursor shape) is advertised to every client.
`wp_security_context_manager_v1` itself is served only to clients without a security context.

### 2.3 Restricted-mode admission

**Greeter mode (rev 0.3; ADR 0007 amendment 2).** In `--greeter` mode zxr composes **trusted members
only**: the greeter program and the OSK, each spawned by zxr as a child with one end of a
`socketpair(AF_UNIX, SOCK_STREAM)` as `WAYLAND_SOCKET` (kscreenlocker `ksldapp.cpp:377-422`; KWin's
`InputMethod::startInputMethod`, `kwin/src/inputmethod.cpp:864-926`). No listening socket exists
(session-auth §5). The program speaks greetd itself over the inherited ``; when the
**primary** trusted client (the program) exits — after `start_session`, or by crashing — zxr exits
(cage's kiosk rule), and greetd starts the session or restarts the greeter (research/78 §4).

**Lock mode is not restricted admission.** The in-session lock is `ext-session-lock-v1` on the public
socket (§2.1's privileged set), from `mura-greeter --lock` running as a **user unit**
(`Restart=on-failure`) like every other component of §3 — cosmic-session's resident locker, swaylock
under sway (research/78 §3). While locked the mode gate routes input only to the lock surface and to
trusted members (the OSK spawned by zxr in the session, §3.2), the composition samples no window (I1),
`locked` follows the first such frame (I2), and the lock survives the client's death until the unit
brings it back (I3). The trusted bit built in research/77 §4.3 therefore serves greeter mode and
the OSK-on-lock case; the lock itself is the protocol's.

### 2.4 What the compositor tells the shell, and how

Compositor state a component presents (tracking state, presence, the active source of the input
tier, quiet mode, the summon affordance) reaches it over the **standard seams where one exists** and
otherwise over D-Bus signals on a Mura interface, never a private Wayland protocol: window lists
through `ext-foreign-toplevel-list`, places through `ext-workspace` + `zxr-workspace-v1`
(places-model §6), activation through `xdg-activation`, volume/brightness/mode toasts through the
OSD's D-Bus subscriptions (cosmic-osd's shape), notifications through `org.freedesktop.Notifications`,
the tray through StatusNotifier. phosh's `phosh_private` (thumbnails, accelerator subscription,
startup tracker) is the anti-pattern to avoid: each of its four requests has a standard seam here
(compositor-rendered previews, the reserved input, `xdg-activation`, the session's unit state).

### 2.5 Idle correctness

The shell plane's budget rule is the compositor's to honour first. Research/75 §5.4 found that a
head-ray-owned pointer resting on a client makes the seat deliver `wl_pointer.motion` every tick to
a still client (62 wake-ups/s). Rule: **no `motion` unless the plane-local position changed** by at
least one logical pixel; no `frame` without an event. *Built (rev 0.2; spec §8 rev 3.12, gate 8
measured):* a one-pixel dead band in the pointer transport — wlroots' `wl_fixed` resolution was
tried first and leaked 20 motions/s of numerical jitter on a head-anchored plane; the pixel is the
right unit. A layer surface with keyboard interactivity
`none` receives no keyboard events; a surface not under the ray receives nothing. Panels and OSDs
redraw on damage only; a component that animates does so under the comfort caps (composition §7.3
constraint 6) and stops when its animation ends.

## 3. The components

Each: process · unit · seams · frame/band · input floor · a11y · settings · cost. "Carried" means a
third-party program shipped as is (rule 5 makes it replaceable); "Mura" means written here in Slint.

### 3.1 Greeter and lock program — Mura

- **Process:** `mura-greeter`, one program for both modes, selected by `--lock` (cosmic-greeter's
  one-binary shape, by argument rather than by user name since zxr and the unit both know the mode). In greeter mode it renders greetd's `auth_message`
  prompts per session-auth §2.3's style set (the digit pad keyed off `style=secret` plus the
  non-secret numeric hint — ADR 0007/0018), the session list from the file the module system
  writes (`/etc/greetd/environments`, gtkgreet's list; a `wayland-sessions` scan if a second shell
  ever ships — research/78 §4), the standard furniture of multi-user.md §2 (power menu over logind, clock,
  session chooser, accessibility toggles — **no network menu**: ruled 2026-09-29, multi-user rev 3.8;
  Wi-Fi before a user exists is `mura-setup`'s, first-run-onboarding §5, and after login the
  panel's, §3.3), and the multi-user profile's create-guest flow (ADR 0018
  decision 9). In lock mode it is resident, waits for logind's `Session.Lock`, locks through
  `ext-session-lock-v1`, **spawns and reads `mura-authd`** (session-auth §2 rev 6; kscreenlocker's
  worker, swaylock's PAM child), renders the prompt batches, and unlocks with `unlock_and_destroy`
  after `success`; it sets logind's `LockedHint` on `locked` and re-locks on restart while the hint
  is set (cosmic-greeter's recovery). It never links PAM (session-auth §1).
- **Unit (rev 0.3, ADR 0007 amendment 2):** in the session, `mura-greeter-lock.service` — a user
  unit, `PartOf=graphical-session.target`, `Restart=on-failure`, on the public socket (cosmic-session
  runs its locker as a resident component the same way). In greeter mode, zxr's socketpair child;
  zxr exits with it and greetd supervises (cage's rule). *Ruled (research/78 §9):* the program
  speaks greetd itself; zxr relays nothing.
- **Seams:** greeter mode — the socketpair and the inherited `$GREETD_SOCK`; lock mode — the public
  socket, `ext_session_lock_manager_v1`, logind over the session bus (`Lock`/`Unlock`,
  `SetLockedHint`, power), its own `mura-authd` socketpair; both — `zwp_text_input_v3` for its
  fields (the OSK types into them); `xdg_activation` not needed.
  In greeter mode it maps as a layer surface on `overlay`, all anchors, keyboard `exclusive`,
  zone −1 (cosmic-greeter, gtkgreet `-l`, phosh's lock all do exactly this); in lock mode as
  `ext_session_lock_surface_v1` per output (swaylock, cosmic-greeter's locker), which zxr composes
  as a band-5 head-frame member (spec §9).
- **Frame:** `world` (the default — rev 0.7; the program asks for nothing), at the head config's distance on the shell's anchor: floating in front where it appeared, head free, brought back by recenter, so head-aim and dwell can reach every target (spatial-input §13); docked: additionally flat on the docked output (ADR 0015).
- **Input floor:** operable by head ray + `hmdButtons.<selectRole>` and by dwell alone; targets sized
  for a 1.5° ray (research/37); a physical keyboard types into it; the OSK (§3.2) is its keyboard
  path.
- **a11y:** an AT-SPI tree with Entry/PasswordText/Button roles and actions (measured, research/75
  §5.4); the a11y bus and registry run in the greeter's session (§3.10); the password field is
  `PasswordText`, never read back.
- **Settings:** none of its own at rev 0.1; theme keys are declared when a theme exists.
- **Cost:** the measured probe; not resident while a session runs (the lock is the same binary,
  spawned on lock).

### 3.2 On-screen keyboard — carried (squeekboard), then Mura

- **Process (ruled Q5; built at G2, rev 0.6):** **`mura-osk`** (`pkgs/mura-osk`), Slint on the sctk
  platform's `input-method` feature, in squeekboard's shape — layer `top`, anchors bottom|left|right,
  namespace `osk`, exclusive zone = its height (`squeekboard/src/panel.c:64,84`); `zwp_input_method_v2`
  + `zwp_virtual_keyboard_v1`; types with `commit_string`, erases with a virtual-keyboard Backspace,
  sends no preedit and never `delete_surrounding_text` (`submission.rs:116-150`); shows on `activate`,
  hides 200 ms after `deactivate` (`animation.rs:15`); the wearer's hide holds across one activation's
  updates and clears with the next field (`state.rs:292-318`); layouts letters/symbols/digit pad, the
  pad for digits/number/phone/pin/date/time purposes (`data/loading.rs:122-126`); every key an AT-SPI
  button; `sm.puri.OSK0` (`SetVisible`/`Visible`) served when a session bus exists, absent in greeter
  mode. Designed for the ray: keys ≥ 72 px (≈ 3.4° on the greeter frame), nothing on hover. Hidden =
  unmapped (a null buffer; the compositor treats the next commit as the initial one again).
  squeekboard was carried through G1 (104 layouts; `mura-osk` has `us` — layouts are an open item).
- **Unit (rev 0.3):** **zxr's socketpair child in every mode** — KWin's input-method shape exactly
  (`kwin/src/inputmethod.cpp:864-926`: spawned with `WAYLAND_SOCKET`, restarted on crash up to five
  times in 20 s, then stopped with a warning, `:88-96, 916-928`). It is the one component zxr
  supervises rather than systemd, for the reason KWin does: the lock screen needs a keyboard, and
  the protocols carry no trusted bit for an OSK (cosmic-comp invented `show_on_lock`, Hyprland
  `above_lock` — research/77 §2.4); Mura's trusted connection is that bit, and it exists only for a
  child. *Flagged (rule 4):* a trusted listening socket (research/78 §9 Q1 option b) would let the
  OSK be a unit; no comparable has one.
- **Frame (rev 0.4): the surface it types into** — the placement table's `typed` value, the seed
  for `osk`. Research/36 §7's convergence: WiVRn hangs its keyboard at a fixed offset below its
  GUI (`client/constants.h:87-88`: (0, −0.3, 0.1) m, pitch −0.6 rad), xrdesktop shows one per
  focused window, visionOS and Quest float it near the field; none anchors it to the body (wayvr's
  anchored keyboard was the one exception and the previous seed's source — research/78 §9 F8).
  So: under the greeter or a world/head-frame panel the OSK is that frame's bottom band (its own
  layer-shell anchors, zone respected — gate 9); under a world-frame window it is arranged against
  the window's rectangle (the window's width) and hangs below it with WiVRn's offset, sized at the
  window's distance, following the window when it moves (spec §4 rev 3.14; gate 9 (h)). A wearer's
  row (`shell.place:osk.frame head`, or a grab) still wins. `hand_*` when a hand is tracked stays a
  settings key for the Mura OSK.
- **Visibility:** the protocol's — shown on `activate`, hidden on `deactivate`; `sm.puri.OSK0`
  `SetVisible` is a preference (phosh's rule: "any text input can make the keyboard show again");
  `input.osk.enabled` (declared, research/73) is the permanent suppression, `suppress_after_key_s`
  the physical-keyboard hatch.
- **Above the surface it types into (rev 0.3, Stage B; phoc's rule):** a `top` OSK under an
  `overlay` greeter or lock scene on the same frame is occluded and cannot be hit (research/78
  §7a). phoc raises the `osk`-namespace surface to `overlay` while the focused layer surface's
  layer is ≥ the OSK's and the input method is enabled on it (`phoc/src/layer-shell.c:446-499`
  `phoc_layer_shell_update_osk`, "as otherwise keyboard input isn't possible"; re-evaluated on
  every arrange, `:290-293`). zxr does the same in its own terms: while the exclusive-focus
  override member has an active text input and its layer is ≥ the OSK member's, the OSK member
  composes and hit-tests in the band above the override's (capped at the foreground band); the
  OSK's own layer otherwise. Nothing is asked of the OSK client. Lock surfaces are above every
  layer by the protocol; the lock scene is a lock surface, so the rule there is the same with the
  lock member as the focused surface.
- **Design rule for the Mura OSK (D5):** never `delete_surrounding_text` — Backspace as a key; the
  purpose hint selects the layout where the toolkit sends one (password ⇒ masked; number arrives as
  `Normal` from Slint clients, so the digit-pad *layout* is the OSK's own state, toggled by the user
  or by `sm.puri.OSK0`).
- **a11y:** the OSK is itself an AT-SPI client; its keys are Buttons with actions.
- **Cost:** squeekboard GTK3 + Rust (not measured here); the Mura OSK one Slint process.

### 3.3 Panel and status surfaces (+ the tray host) — Mura

- **Process:** `mura-panel`: clock, battery, tracking/presence state, network, the tray host. The
  StatusNotifier **host** lives in the panel process (Plasma's applet, waybar's `sni`, cosmic's
  status-area applet); the **watcher** is a small separate unit (`org.kde.StatusNotifierWatcher`,
  kded's / waybar's shape) so a panel restart does not drop registered items — ADR 0012's "carried".
- **Unit:** `Wants=` by `mura-session.target`, `Restart=on-failure`.
- **Seams:** layer `top`, keyboard `none` (or `on_demand` while a popover is open), exclusive
  angular band = its height; `ext_foreign_toplevel_list` for a task strip if it has one;
  `xdg_activation` for launches; StatusNotifier, UPower, login1, NetworkManager over D-Bus.
- **Frame:** `world` at the periphery (research/60 §1: bands with windows between); docked: the
  docked output's edge.
- **Not** cosmic-panel's nested-server design: applets are not separate processes behind the panel
  (flagged in research/75 §3.3 as the one structural novelty; it costs a second compositor). A
  third-party status program is another `top`-layer client beside it.
- **Settings:** declared when it lands (position/band, items).

### 3.4 OSD (+ the polkit agent) — Mura

- **Process:** `mura-osd`: transient indicators for volume, brightness, IPD, mode/tracking changes,
  the recenter/summon affordance of native-openxr-apps §6 while the reserved input is held; **and the
  polkit authentication agent** (cosmic-osd co-locates them; rule 7's co-location) — the agent's
  dialog is the same class of head-locked exclusive-keyboard transient.
- **Unit:** `Wants=`, `Restart=on-failure`; owns a D-Bus name for CLI/D-Bus triggers.
- **Seams:** layer `overlay`, keyboard `none` for indicators (auto-close 3 s, cosmic-osd's), zone 0;
  the polkit dialog `exclusive` on `overlay`; triggers over D-Bus subscriptions (audio, backlight,
  the compositor's mode signals), never a private protocol (§2.4).
- **Frame:** `head` for the 3-s indicators only (small head-locked transients, exempt from the motion caps — research/36 §4; the one deliberate head placement besides toasts); the polkit dialog and anything the wearer must aim at: `world` (rev 0.7, spatial-input §13).
  (research/36 §4).

### 3.5 Notifications — carried (mako), then Mura

- **Process (ruled Q4):** **mako** carried: the FDO server + cards in one cairo process, layer `top`
  by default (set to `overlay` in its config), anchor top|right, D-Bus-activated (`Type=dbus
  BusName=org.freedesktop.Notifications`, its shipped unit). Placement: head frame by the anchoring
  default. DND by immersion and the critical bypass (research/36 §4): zxr drives mako's
  `fr.emersion.Mako.SetMode` over D-Bus when a native app is primary or a scene is exclusive;
  urgency `critical` is left through by mako's own criteria. Later a Mura component on the shell
  toolkit that knows the head frame directly; the seam (`org.freedesktop.Notifications`) does not
  change. Placement by the `notifications` row of §2.6 (seed: head, upper-right).
- **Unit:** mako's own `Type=dbus` unit, `PartOf=graphical-session.target`.
- **OSD and notifications, one process or two (ruled, owner 2026-09-27):** separate processes are
  **allowed** — the seam is the FDO interface and any daemon may own it (COSMIC's shape:
  `cosmic-osd` + `cosmic-notifications`); Mura's own components will in practice likely **merge**
  the two into one Slint process (plasmashell's and GNOME Shell's shape — one resident process,
  one toolkit instance, on the ≤ 5-process budget). The design admits both; nothing in zxr
  distinguishes them.

### 3.6 Launcher — Mura

- **Process:** `mura-launcher`: the flat pinned grid + search on the reserved gesture (research/36
  §3), desktop entries from the freedesktop desktop-entry spec (the Rust `freedesktop-desktop-entry`
  crate as cosmic uses, not a bespoke scanner), **and the native-app launch path of
  native-openxr-apps §3** (the launcher owns the per-app scope, requests primary through the session,
  and is the reader of `system.quit_timeout_s`).
- **Unit:** `Wants=`, `Restart=on-failure`; D-Bus single-instance activation for the summon toggle.
- **Seams:** layer `overlay`, keyboard `exclusive` while shown, zone −1 (fuzzel, cosmic-launcher);
  `xdg_activation` tokens on every launch; `zwp_text_input_v3` for its search field;
  `ext_foreign_toplevel_list` for running-app results.
- **Frame:** `world` where summoned (rev 0.7: an aimed-at scene is never head-locked and never follows unasked, spatial-input §13; fuzzel and cosmic-launcher float where they were summoned).

### 3.7 Task switcher and places overview — Mura, after M1

- **Process:** `mura-overview`: the places overview (pager) and the switcher are one component
  over `ext_workspace_manager_v1` + `zxr_workspace_v1` (places-model §6 enumerates exactly what it
  reads, including partial-transition offsets) and `ext_foreign_toplevel_list_v1`; previews are
  compositor-rendered (places-model §6; never `phosh_private`-style thumbnails or screencopy).
  Focus and activation stay the compositor's (spatial-input §6; a switcher *requests* activation
  with an interaction serial, window-workspace-management §11).
- **Frame:** a place transition rather than a surface; the overview UI itself on `head`.
- Exists only when there are places worth organizing (places-model §5; the registry keeps it
  `missing` until then).

### 3.8 Wallpaper / environment — carried or Mura, background layer

A `background`-layer client on the `world` frame is the wallpaper of band 1 (spec §4); the
perception producer supplies passthrough over the intake protocol instead. Any layer-shell
wallpaper program qualifies; not designed further here.

### 3.9 Decoration chrome — compositor (not a shell process)

Per ADR 0012 the grab/rotate/close affordances are trusted hit volumes drawn by zxr; *what they
are* in 3D is window-workspace-management's manipulation UI (research/75 §7 Q6, open there). Client
`xdg-decoration` negotiation keeps its meaning for the 2D tier. This document only records that no
shell process draws chrome.

### 3.10 Session infrastructure the plane needs

- The **a11y bus** (`org.a11y.Bus`, at-spi2-core's launcher) and **`at-spi2-registryd`** in every
  session including the greeter's — no client exposes a tree without them (research/75 D6);
  spatial-a11y §1's ATs (Orca) are service-plane units the session starts when
  `org.a11y.Status.ScreenReaderEnabled` is set (cosmic-session's shape).
- **Fonts and fontconfig** in the closure for every Slint component (the software renderer shapes
  through fontique/parley/swash over the system font database); the font set is the image's, named
  in the profile.
- `libwayland-client` and `libxkbcommon` on the components' rpath (dlopen'd; research/75 §5.2).

## 4. The toolkit (ruled Q3; research/75 §5)

**Slint 1.18.x**, software renderer, `backend-winit-wayland`, `accessibility`, `.slint` compiled
ahead of time by `slint-build`, `std` (the `no_std` path is not this plane's), GPLv3. Feature set
per component: `["std", "compat-1-18", "backend-winit-wayland", "renderer-software",
"accessibility"]` — no X11, no Qt backend, no GPU renderer, no system tray helper; release profile
`opt-level = "s"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, stripped.

**Measured qualification** (research/75 §5.4, host, 2026-09-27): 12.48 MB binary; 20.5 MB RSS /
12.6 MB PSS / 3.0 MB private dirty; 6 threads; 31 ms to a mapped plane (warm); idle-clean (0 CPU, 0
wake-ups over 10 s without focus and without compositor motion); 2 Hz caret commits; text-input
focus, `content_type`, preedit, `commit_string` and seat keys correct; AT-SPI tree with Entry /
PasswordText / Button + Action, an AT-invoked action fires; nine scripts and emoji render.

**Conditions recorded with the ruling (each a tracked item, decider named):**
- `delete_surrounding_text` is ignored by winit 0.30.13 — harmless for squeekboard/wvkbd and the
  Mura OSK (D5); an upstream winit item, decider: the OSK component's author when it lands.
- `EditableText` is absent from `accesskit_unix` 0.22 (present in 0.25): a screen reader reads and
  focuses fields but cannot edit through AT-SPI until the dependency moves — decider: the greeter
  program's first release.
- `input-type: number` is reported as `ContentPurpose::Normal`: the digit pad is the OSK's own layout
  state, not the purpose's — designed so in §3.2.
- Native runtime set: fontconfig (+ freetype, expat, brotli, bz2, png, zlib), dlopen'd
  libwayland-client and libxkbcommon — carried by the Nix package as rpath, the way `pkgs/zxr`
  carries libvulkan.

**The platform (rev 0.3; owner ruling 2026-09-28).** Slint's stock Wayland backend is winit, which
creates xdg toplevels only; the lock program must be an `ext-session-lock-v1` client and the
greeter a layer-shell one (§3.1), so Mura's shell components run Slint on **one sctk platform**
(`slint::platform::Platform` + `WindowAdapter` over `smithay-client-toolkit`: wl_shm buffers,
`SoftwareRenderer`, seat keyboard/pointer/touch, `zwlr_layer_surface_v1`, `ext_session_lock_surface_v1`,
`zwp_text_input_v3`) — the route libcosmic/iced took for the same reason; Slint's own non-winit
`linuxkms` backend is the in-tree template. **Accessibility is restored at that boundary, not
dropped:** Slint's AccessKit tree translation lives in its winit backend
(`references/slint/internal/backends/winit/accesskit.rs`, whose own comment says "If we wanted to
move this to corelib…", `:43-45`); Mura carries it as a **crate beside Slint**
(`pkgs/mura-greeter/accesskit`, depending on `i-slint-core`'s internals at the exact pinned
version — no fork, no patch) that drives `accesskit_unix::Adapter` from the platform, offered
upstream as the module that comment anticipates. Semantics (roles, names, values, actions, text runs) stay in the client;
zxr owns secure lock, input routing, spatial placement and compositor-level accessibility
(magnification, filters, assistive-technology input privileges). **Gate before the greeter is
written** (Stage B): a three-widget scene on the platform maps as layer-shell and as a lock
surface on nested zxr, takes ray/key/OSK input, and shows its AT-SPI tree; if the extraction is
not a contained adapter change, GTK4 + gtk4-layer-shell (layer-shell and session-lock, mature
AT-SPI) is the fallback brought to the owner. The `backend-winit-wayland` feature above is
replaced by the platform crate; the measured qualification is re-taken on it.

**Stage B passed (2026-09-28; research/78 §7a).** The extraction is a contained adapter change:
the translation is upstream's file unchanged apart from a `Host` trait (window adapter, keyboard
focus, deferred re-entry) in place of `Weak<WinitWindowAdapter>`, an mpsc + wake in place of
`accesskit_winit`'s event-loop proxy, and `set_window_focused` in place of winit's `Focused`
event; `accesskit_unix::Adapter` is driven directly. One further Slint hack matters: the
compiler's `EmbedTextures ⇒ accessibility off` rule (`internal/compiler/lib.rs:342-346`, "not
supported with backends that support the software renderer anyway") is removed — its assumption
is exactly what this platform breaks — sidestepped rather than patched: the scene carries no
images (`EmbedFiles`), so the package builds unpatched Slint from crates.io. Both are upstream
candidates. GTK4 is not brought to the owner. Two things the gate surfaced belong to zxr, not the platform: (1) **the OSK under a
full-frame greeter** — squeekboard (`top`) is occluded by an `overlay` greeter on the same head
frame, so its keys cannot be hit; phoc's rule is the precedent (`phoc/src/layer-shell.c:446-499`
`phoc_layer_shell_update_osk`: when the focused layer surface's layer ≥ the `osk` surface's and
the input method is enabled on it, the OSK is composed on `overlay` — "as otherwise keyboard
input isn't possible"); zxr adopts it as the band above the focused layer member (§3.2, G1).
(2) **The 2D pointer cannot cross members** — a relative pointer leaving its plane re-lands where
the head ray hits (spatial-input §8); the harness aims the controller ray instead. Not a defect.

**Not chosen, and why (research/75 §5.1):** libcosmic/iced (the comparator; one shipping greeter,
but a fork of iced, `a11y` off in the shipping greeter, `wayland` implies `iced_wgpu`); GTK4
(mature a11y, the functional reference; C, dynamic, a large closure); Qt/QML (a JS engine); egui
(no CPU-only Wayland configuration); cairo/pango by hand (no widgets, no text input, no tree).
The comparator was not built because no gate failed; it is rebuilt only if a gate fails at first
hardware.

## 5. Theming and replacement

A component's appearance is its own: Slint components read a Mura theme (colours, radii, type
scale) from settings keys declared when the first themable component lands (`ui.*`, research/73;
`ui.reduced_motion` exists already and every component honours it; `ui.{animation_speed,high_contrast}` are declared with their first consumer); carried
components keep their own config files (mako's `config`, squeekboard's layouts). Replacement is
systemd's: `systemctl --user mask mura-launcher.service` and a drop-in `ExecStart=` for another
launcher; a third-party program binding the same seams is unrestricted on the public socket (§2.2).
Nothing in the compositor knows a component's name.

## 6. Open items (each names its decider)

- **Who speaks greetd** — ruled 2026-09-28: the program (research/78 §9; session-auth §5 rev 6).
- **The Slint AccessKit crate** (§4): a crate beside Slint on `i-slint-core`'s internals at the
  pinned version until upstream exposes accessibility to custom platforms; re-pinned at each Slint
  upgrade. Decider: the owner, at each Slint upgrade.
- **The bar/panel seed row** (§2.6: world, bottom, −25°, pitch −5°, "research/60 §9's dock") has
  the provenance the OSK seed had — a taxonomy entry with the comparables fitted afterwards, none
  read for a dock's placement. Rethink candidate (rule 7): read the shells' dock placements
  (Horizon's Navigator, visionOS's Home View, xrdesktop, wayvr's watch) before `mura-panel` lands.
  Decider: the owner, with that reading.
- **A trusted listening socket** for the OSK (§3.2) so it can be a unit rather than zxr's child;
  no comparable has one (smithay's per-listener `ClientData` is the mechanism). Decider: the owner,
  when the Mura OSK lands.
- **`LockedHint` from zxr** when a trigger fires before any client has locked (session-auth §7).
  Decider: the owner, at G3.
- **The OSK's frame and its Slint successor's layout for a ray** (`world` vs `hand_*`, key size, dwell
  behaviour). Decider: the Mura OSK's design (research/36 §7 patterns), after the carried phase.
- **Decoration chrome in 3D** — window-workspace-management's manipulation UI (research/75 Q6).
- **The polkit agent's co-location** with the OSD: cosmic-osd's shape adopted; revisit if the agent
  needs to run while the OSD is masked. Decider: the OSD component's first release.
- **Aggregate budget on a class-B device**: the five-process ceiling and the measured per-process
  numbers are host numbers; the device partition is budgets.md §3's at first hardware. Decider: the
  budgets owner with the first device measurement.
- **A Mura notifications component** replacing mako, and when: decider the shell's second release,
  on the seam that does not change (merged with the OSD or not — §3.5's ruling admits both).
- **Seed placement rows in code vs a Nix option** (§2.6): the rows are the consumer's defaults in
  `shell/place.rs` keyed by namespace; a `mura.xr.shell.place.<namespace>` Nix option that seeds
  template instances would need the settings artifact to carry per-instance defaults, which
  GSettings relocatable schemas do not have. Decider: the settings design (settings-schema.md §10).
- **Grab-to-place on a shell plane**: lands when the WM branch's grab mechanics (research/76) merge;
  until then rows are written by `mura-settings set` and `zxr ctl`. Decider: the WM workstream's
  merge order (implementation-path §5).

## 7. Cross-references

[research/75](../research/75-shell-plane-from-comparables.md) (evidence and measurements),
[research/30](../research/30-wayland-de-anatomy-protocol-seams.md), [research/36](../research/36-vr-shell-interaction-patterns.md),
[research/60](../research/60-de-abstractions-mapped-to-xr.md), [ADR 0007](adr/0007-session-greeter-lock.md)
(amended), [ADR 0012](adr/0012-de-modularity-spinout-seams.md) (amended), [specs/session-auth.md](../../specs/session-auth.md)
rev 5, [specs/zxr-core.md](../../specs/zxr-core.md) §4/§8/§9/§10, [specs/session-bootstrap.md](../../specs/session-bootstrap.md),
[places-model.md](places-model.md) §6, [spatial-a11y.md](spatial-a11y.md), [spatial-input.md](spatial-input.md) §12–§14,
[native-openxr-apps.md](native-openxr-apps.md) §3/§6, [budgets.md](budgets.md) §3,
[component-registry.md](component-registry.md) §5, `protocols/zxr-layer-anchoring-v1.xml`.
