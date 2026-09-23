# ADR 0012: Desktop-environment modularity and spin-out seams

**Status:** accepted (draft)
**Date:** 2026-09-22
**Context sources:** [30-wayland-de-anatomy-protocol-seams](../../research/30-wayland-de-anatomy-protocol-seams.md)
(protocol inventory, KWin/COSMIC precedent, per-candidate verdicts),
[desktop-environment.md](../desktop-environment.md) (the plane model this ADR applies),
[component-registry.md](../component-registry.md) (the inventory the decisions cover),
[17-sharing-capture-stack](../../research/17-sharing-capture-stack.md) (portal/capture design).
Composes with [ADR 0006](0006-compositor-strategy.md) (the zxr compositor) and
[ADR 0007](0007-session-greeter-lock.md) (lock/greeter placement, the factoring exemplar).

## Context

ADR 0006 creates one program — the zxr compositor — that could absorb the whole desktop (the
GNOME Shell shape: everything in the compositor process) or shed everything (the wlroots/sway
shape: compositor as kernel, all UI external). Both extremes are coherent; neither is what
spatial-os wants. We need decided seams *before* the shell components are written, because the
seam determines each component's process model, protocol surface, and whether third parties can
replace it.

The evidence base ([research 30](../../research/30-wayland-de-anatomy-protocol-seams.md)) says the
standardized surface is now real: `ext-workspace-v1` (staging since wayland-protocols 1.39;
COSMIC/Hyprland/niri/labwc/Sway adopted), `ext-foreign-toplevel-list-v1` (identity/enumeration
only — control is deliberately unstandardized), `wlr-layer-shell` (deployed everywhere but Mutter;
ext successor still a draft), ratified capture (`ext-image-capture-source-v1` +
`ext-image-copy-capture-v1`), `ext-data-control-v1`, `xdg-activation-v1`. COSMIC is the closest
implementation precedent (smithay compositor + separate Rust shell processes on these protocols);
KWin is the precedent for the seams that must stay *in-process* (KDecoration plugins, QML switcher
layouts, scripting — presentation/policy externalized, authority never). None of these protocols
knows what an OpenXR view, world anchor, depth buffer, or spatial thumbnail is.

## Decision

The gating rule is [desktop-environment.md §4](../desktop-environment.md): mechanism stays in the
authority plane; policy and presentation are spun out **where a seam exists**, kept as an
in-process *plugin* where the seam would have to carry authority, and kept *compositor-internal*
where latency, lock, or capture invariants forbid delegation.

### 1. Separate clients / daemons on standard seams

| Component | Seam | Notes |
|---|---|---|
| Pager / places overview | `ext-workspace-v1` + zxr spatial extension | Workspace groups/lifecycle/activation over the standard protocol; the zxr extension adds space type, metric transform/bounds, anchor binding (ADR 0009), and compositor-rendered stereo previews (a client cannot re-render a 3D place). Coordinates stay integers — never reinterpreted as metres. The model consumed, the groups-as-frames mapping, and the `active`-bit honesty rule are now defined by [places-model.md §6](../places-model.md) / ADR 0016. |
| Task-switcher UI | `ext-foreign-toplevel-list-v1` + capture previews; `wlr-foreign-toplevel-management` for actions until an ext management protocol merges | Compositor-rendered previews; activation is a *request* — zxr keeps the focus decision and `xdg-activation` semantics. |
| Notification daemon | `org.freedesktop.Notifications` (D-Bus) + layer-shell presentation + zxr placement metadata | COSMIC's shape (`cosmic-notifications`). zxr caps angular size/depth and enforces DND/capture-suppression policy. |
| Portal backend | xdg-desktop-portal backend D-Bus API + ext capture protocols + PipeWire | Per [17 §8](../../research/17-sharing-capture-stack.md): `xdg-desktop-portal-wlr` unmodified on day one, native `xdg-desktop-portal-spatial` (SpatialCast source types, in-space consent) as the replacement. The backend *presents* consent; zxr *enforces* it. |
| App launcher | layer-shell + `xdg-activation-v1` | Search/results in the client; the activation token carries the launching gesture; focus transition stays in zxr. |
| OSD framework | layer-shell + a narrow event channel | Non-focus-stealing; zxr constrains angular size/distance/persistence. OSDs never receive raw global input — values arrive over the service channel (e.g. ADR 0011 IPD-motor events). |
| Display-config UI | `wlr-output-management` (non-HMD outputs only) | UI separate, zxr validates/applies atomically. **HMD configuration is explicitly not this** — see §4. |
| Clipboard manager | `ext-data-control-v1`, binding-gated | Privileged global filtered to the trusted client; previews follow visibility policy (never floated into a shared/spectated space). |
| Session restore manager | `xdg-session-management-v1` (served by zxr) + the `.desktop` database | The protocol restores window state for returning app instances but deliberately excludes relaunching — the manager owns relaunch and binds restored toplevels to **place IDs** (ADR 0009 anchors), so a room's workspace comes back apps-and-all. Evidence: doc 30 addendum. |
| SNI watcher + host | StatusNotifierItem/Watcher D-Bus (de-facto spec, formally still draft 0.1) | **Decision: host SNI, don't drop it.** A supervised watcher service owns the bus name; the host renders items as typed badges in a panel applet/component (COSMIC's `cosmic-applet-status-area` + socket-activated watcher; Plasma's systemtray applet + KDED watcher — [30 §A3](../../research/30-wayland-de-anatomy-protocol-seams.md)). Dropping tray compatibility was rejected: too many long-running apps (chat, sync, audio) signal only through SNI. Never the sole route to critical/safety controls. |
| Greeter, lock *auth*, PAM | greetd IPC / `spatial-authd` socketpair | Already decided in ADR 0007; listed for completeness as system-plane spin-outs. |

### 2. Pluggable in-process (plugin seam, not a protocol)

- **Decoration renderer** — KDecoration3's model: theme/chrome as an in-process module; frame
  geometry, hit-testing, move/resize grabs, and scene insertion stay zxr code. In 3D the
  decoration *is* a set of trusted hit volumes (grab/rotate/close affordances); an out-of-process
  renderer would put an untrusted client inside input dispatch. `xdg-decoration` still negotiates
  SSD/CSD for the 2D tier.
- **Window-management policy** (placement, tiling, rules) — a restricted in-process rules/scripting
  surface (KWin scripting precedent), *not* an external protocol. No portable protocol expresses
  initial placement, tiling trees, or focus-prevention; in XR an all-powerful external policy
  client could steer content into the user's face or across the boundary. External tools may write
  validated *configuration*; they do not hold a live policy socket.
- **Lock scene (appliance profile)** — compositor-internal per ADR 0007 (the lock surface must
  exist when every client is dead); `ext-session-lock-v1` remains the dev/desktop-profile seam.
- **Effects/animation module** — window open/close/move/switch transitions as an in-process
  module (KWin-effects precedent: effects get paint hooks and the window list, never input or
  protocol objects), under **authority-owned comfort caps**: in XR, sudden motion or scaling of
  large surfaces is a vestibular-safety property, so maximum angular velocity/scale-rate limits
  are compositor policy an effect cannot exceed. Never an external client (it runs inside the
  frame loop). The caps are normative for *all* autonomous motion, including window-management
  policies like follow mode — [composition §7.3](../zxr-shell-v2-composition.md) interaction
  constraint 6, mined from KWin VR's uncapped follow-mode slerp ([31 §2.10](../../research/31-kwin-vr.md)).

### 3. Authority-only (never delegated)

Final focus/activation decisions and global-shortcut/gesture interception; 2D/3D hit testing,
input routing, and grabs; the window + space model and placement invariants; decoration *policy*
(per-window/per-state chrome decisions — the renderer is the §2 plugin, the policy is not); lock
enforcement (ADR 0007 I1–I3) and every "presented safe frame" transition; capture/injection
authorization and passthrough privacy redaction; sort-last composition and frame pacing
(composition doc); the **colour pipeline** (serving `color-management-v1`/`color-representation-v1`
to clients is a protocol duty, but applying panel calibration, choosing composition colour space,
and matching passthrough to rendered content are composition-correctness mechanism that cannot
leave the process); boundary enforcement (dim/cut client content at the guardian fence).

### 4. The zxr-private protocol surface (kept minimal)

One small family of shell-integration extensions beside `zxr-shell-v2`, covering only genuinely
spatial semantics ([30 §6](../../research/30-wayland-de-anatomy-protocol-seams.md)):

1. spatial workspace metadata + compositor-rendered space-preview sources (extends ext-workspace);
2. spatial toplevel state/actions not expressible on upstream foreign handles;
3. world/head/body anchoring + angular/metric exclusion zones for layer surfaces (reinterpreting
   layer-shell's output-edge semantics per [desktop-environment.md §5](../desktop-environment.md));
4. spectator/stereo/depth capture source types + passthrough redaction (doc 17's SpatialCast);
5. HMD/runtime configuration (IPD, render scale, refresh, recentering, passthrough toggle) as a
   narrow settings API backed by Monado capability checks — never `wl_output` mode-setting.
   Completeness checklist: KWin VR's KCM taxonomy (general / input / headgaze / follow-mode /
   advanced pages, [31 §2.7](../../research/31-kwin-vr.md)) — the empirically user-tested set of
   knobs a 2D-in-XR tier needed (ADR 0013 §2).
6. toplevel export/delegation (`zext-toplevel-export-v1`, added by
   [ADR 0014](0014-toplevel-delegation-protocol.md)) — unlike items 1–5 this one is
   deliberately **XR-agnostic** and carries explicit upstream intent (`xx_`/`ext_` path); zxr is
   its consumer, foreign 2D compositors its producers
   ([foreign-session-integration.md](../foreign-session-integration.md)).
7. spatial accessibility semantics (`zext-a11y`, reserved by
   [spatial-a11y.md](../spatial-a11y.md)) — read-only spatial context (window poses/relations,
   place membership/currency per [places-model.md](../places-model.md), gaze context under the
   ADR 0011 privacy posture, boundary state) plus privileged navigation verbs, for allow-listed
   assistive clients; carrier may become an AccessKit payload if Newton matures (upstream-intent
   posture per ADR 0014's mold). Item 1's workspace extension fields are refined by
   places-model §6.

Rule: upstream base object + zxr extension; never silently give a 2D protocol's words new wire
meanings. Candidates for upstreaming follow COSMIC's path (their workspace protocol became
`ext-workspace-v1`).

### 5. Binding policy

Every privileged global (foreign-toplevel list/management, workspace manager, data-control,
capture, output-management, virtual-keyboard, input-method, the zxr shell-integration family) is
**filtered per connection**: trusted shell components and the portal backend get them; ordinary and
sandboxed clients (identified via `security-context-v1`) do not. `security-context` supplies
identity, not authorization — the allow-list is compositor policy, configured through the module
system.

## Rationale

- **The seams exist now.** In 2019 (wxrc's era) none of ext-workspace/foreign-toplevel-list/ext
  capture existed; a modular XR shell would have been all private protocol. Today the standard
  surface covers eight of the ten candidates, and adoption is broad enough (COSMIC, Hyprland,
  niri, Sway, Waybar-side clients) that implementing them buys ecosystem compatibility, not just
  internal structure.
- **COSMIC proves the exact target shape** — a smithay compositor with pager/panel/launcher/OSD/
  notifications/greeter/settings/portal as separate processes — including the migration pattern
  (private protocol → upstream ext + retained small extensions). This is the strongest available
  precedent for our smithay-leaning base (ADR 0006).
- **KWin proves the in-process seams.** Decorations and window-policy scripting are modular *and*
  in-process everywhere they've been done well; kglobalaccel's Wayland-era move *into* KWin is the
  ecosystem conceding that interception is authority.
- **XR sharpens, not weakens, the authority boundary.** Decoration hit volumes are input dispatch;
  workspace previews need compositor rendering; OSD placement is a comfort/safety property;
  policy clients could violate boundary safety. Each XR twist pushed a candidate *toward* the
  compositor or a zxr extension — none pushed toward looser coupling than the desktop precedent.
- **Spin-outs force the protocol surface to exist.** Building launcher/switcher/OSD as separate
  clients makes the privileged-protocol implementations direct dependencies of the first shell
  components (the seam-layer edges in [desktop-environment.md §6.3](../desktop-environment.md)),
  so the seams get exercised by real consumers rather than staying speculative.

## Consequences

- The zxr 2D tier's protocol surface includes: `ext-workspace-v1`, `ext-foreign-toplevel-list-v1` (+ wlr
  management shim), `wlr-layer-shell`, `xdg-activation`, `xdg-decoration`, `ext-idle-notify` +
  idle-inhibit, dev-profile `ext-session-lock`, `security-context` + global filtering,
  text-input-v3 / input-method-v2 / virtual-keyboard-v1, `ext-image-capture-source` +
  `ext-image-copy-capture`, `ext-data-control`, `wlr-output-management`, cursor-shape,
  `xdg-session-management-v1`, `xdg-toplevel-icon-v1`, and `color-management-v1` +
  `color-representation-v1`.
  ([30 §6](../../research/30-wayland-de-anatomy-protocol-seams.md) plus its addenda are the
  normative list.)
- New separate components to build (tracked in [component-registry.md](../component-registry.md)):
  launcher, switcher UI, OSD daemon, notification daemon, portal backend, settings UI,
  clipboard manager, session restore manager, and the panel's SNI host — each an ordinary
  client/daemon replaceable without recompiling zxr.
- **Non-goal recorded: desktop icons.** The environment is not an icon surface; the launcher (a
  phone-style app grid / "start menu" scene) owns application icons, fed by the desktop-entry +
  icon-theme XDG specs. No desktop-icons component will be built.
- The zxr shell-integration protocol family (§4) is a deliverable beside zxr-shell-v2, versioned
  and documented like it; its workspace/preview extension is an upstreaming candidate.
- Known gaps accepted: no merged ext foreign-toplevel *management* (we ship the wlr shim and
  migrate); layer-shell's ext successor is a draft (wlr-layer-shell is fine, Mutter-only tools
  won't run — acceptable); Plasma 6 hasn't adopted these seams (irrelevant to us; COSMIC/wlroots
  world has).
- Third-party replaceability is bounded: anyone can replace the launcher/switcher/pager/
  notifications/portal; nobody can replace focus policy, decorations' hit volumes, or the lock
  from outside the compositor. That is the intended trade.

## Alternatives considered

- **GNOME-style monolith** (shell inside the compositor process): rejected — contradicts the
  modularity goal, couples every shell iteration to the compositor's release/crash domain, and
  wastes the now-real standard seams. (GNOME itself pays this: js shell code in the frame loop.)
- **wlroots-style everything-external** (compositor as pure kernel, even policy external):
  rejected where XR safety/latency forbids it — decoration hit volumes, boundary enforcement,
  lock I1–I3, and capture authorization cannot ride an unprivileged protocol; and sort-last
  composition (one OpenXR layer) means the compositor is irreducibly a renderer, not a mux.
- **KDE/COSMIC-style split**: adopted — this ADR *is* that split, adapted to XR with the
  perception plane's privacy boundary and the five zxr-private extensions.
- **All-private protocol surface** (wxrc-era necessity, StardustXR's present shape): rejected —
  forfeits ecosystem clients (Waybar-class tools, portal consumers) and the upstreaming path.
- **External decoration-renderer process** (a "decoration client"): rejected — input-dispatch
  correctness would depend on an untrusted process's honesty about hit regions.
