# 75 — The shell plane from comparables: how the shipping shells are built, and Mura's toolkit

**Status:** research, 2026-09-27. Phase 1 of the plan `shell-plane_research_and_design`: the
evidence [docs/architecture/shell-plane.md](../architecture/shell-plane.md) is written from.
Follows [research/30](30-wayland-de-anatomy-protocol-seams.md) (the seams), [research/36](36-vr-shell-interaction-patterns.md)
(the XR interaction patterns) and [research/60](60-de-abstractions-mapped-to-xr.md) (each desktop
abstraction mapped to XR); it does not repeat them. What those three did not do — and the component
registry's shell rows say so (`missing`, `none decided`, "no doc found",
[component-registry.md §5](../architecture/component-registry.md)) — is study *how the shipping shells
are built*: what programs, on what toolkit, binding which protocols, started and supervised how, at
what cost, and why. That is this document, plus the toolkit question it subsumes (§5), measured.

Rules applied: every claim cites `references/<clone>/<path>:<line>` (pinned in
`references/MANIFEST.json`; eleven clones added for this pass — cosmic-{greeter,launcher,osd,
notifications,applets,session}, waybar, fuzzel, maliit-keyboard, slint v1.18.0, accesskit) or is
marked **[external]**. Consumer XR platforms appear only through research/36's pattern evidence.
Numbers measured here are host numbers, labelled.

## 0. Summary

1. **Every shipping shell but one is a set of separate client processes over layer-shell + a few
   privileged seams; the one exception (GNOME Shell) is in-process because its compositor is a
   toolkit host.** COSMIC: one process per component (`cosmic-session` spawns them). Plasma: one
   `plasmashell` process for panels/desktop/applets/OSD/notification server, separate krunner,
   kded (tray watcher), kscreenlocker greeter, maliit; KWin supervises the IM and the locker.
   phosh: one GTK3 shell process beside phoc, with squeekboard as a separate OSK. wlroots world:
   one tool per job (waybar, fuzzel, mako/dunst, wvkbd, swaylock/hyprlock). §2, §3.
2. **Start and supervision split two ways, and the split is about the same thing.** GNOME, Plasma
   and phosh run shell components as **systemd user units** under `graphical-session.target`
   (`Restart=on-failure`, `OnFailure=…shutdown.target`, `OOMScoreAdjust=-1000`); COSMIC runs its
   own `ProcessManager` (unlimited restarts, exponential backoff) and only *optionally* wraps
   children in transient scopes; wlroots compositors leave it to `exec`/`spawn-at-startup` or
   D-Bus activation. Mura's session already is systemd units (`mura-session.target`, D4); the
   evidence favours that shape. §4.1.
3. **Binding policy converges:** privileged globals (layer-shell, session-lock, IM/VK,
   foreign-toplevel, workspace, data-control, screencopy) are hidden from sandboxed /
   `security-context` clients and shown to the shell's own processes — niri per-global filters,
   Hyprland a static whitelist, cosmic-comp a `not_sandboxed()` check with one engine name
   whitelisted, wlroots a `wl_display_set_global_filter`. Nobody uses a per-process allow-list of
   binaries; identity comes from the connection (security-context) or from *how the fd was
   handed over* (kscreenlocker, KWin's IM, cosmic-panel's applets: a pre-connected socketpair).
   §4.2.
4. **Anchoring is layer-shell's**, and the XR shells that have no layer-shell place their UI by
   hand (WayVR: dash floating at z = −0.9, keyboard anchored below, watch on the wrist; WiVRn:
   head-locked compact UI, world-locked settings). zxr's `zxr-layer-anchoring-v1` (head / body /
   world / docked frames + exclusive angular bands) is the layer-shell-native form of what they
   hard-code. §4.3.
5. **Accessibility:** GTK/Qt shells expose AT-SPI trees through their toolkits; the Rust shells do
   it through **AccessKit** (Slint always, libcosmic behind an `a11y` feature that cosmic-greeter,
   cosmic-panel, cosmic-osd and cosmic-launcher **do not enable** — only cosmic-notifications
   does); cosmic-comp's `a11y_manager_v1` is magnifier/filters, not AT-SPI; cosmic-session starts
   Orca. The pre-login screen reader is therefore a real gap in the one Rust comparable. §4.4.
6. **Toolkit (§5), measured:** Slint 1.18.0 with the software renderer, `backend-winit-wayland`
   and `accessibility`, built feature-minimised: **12.48 MB** stripped release binary; **RSS
   20.5 MB / PSS 12.6 MB / private-dirty 3.0 MB** steady with the auth-scene probe mapped on
   nested zxr, **6 threads** (main, winit `blocking-1`, `async-io`, `zbus::Connection`, two
   more), start → plane mapped **31 ms** (warm); idle **0 CPU and 0 wake-ups over 10 s** when no
   field is focused *and the compositor sends no motion* (see the compositor-side finding);
   caret blink **2 commits/s**; text input over `text-input-v3`/`input-method-v2`: focus,
   `content_type` (password ⇒ `Password`+`SensitiveData`; **number ⇒ `Normal`**), preedit,
   `commit_string` and seat keys all correct; **`delete_surrounding_text` ignored** (winit
   0.30.13 `// Not handled.`); an AT-SPI tree with **Entry / PasswordText / Button (Action) /
   Label / ListBox** roles, an AT-invoked `DoAction` fires the callback; **Latin, Greek,
   Cyrillic, Arabic, Hebrew, Devanagari, CJK, Hangul, Thai and emoji all render** (the "western
   scripts only" note describes the `no_std` font path; on `std` the stack is
   fontique + parley + swash). Native runtime set: `libfontconfig` (`NEEDED`) + freetype, expat,
   brotli, bz2, libpng, zlib behind it; **dlopen'd** `libwayland-client` (+ libffi) and
   `libxkbcommon` — a Nix package carries them as an rpath. **Qualifies**, with the two gaps
   recorded and the comparator (libcosmic/tiny-skia) not built. §5.4, §5.5.
7. **Two compositor-side findings for zxr** fell out of the measurement: (a) zxr serves no
   layer-shell, so no shipping OSK (wvkbd, squeekboard — both layer-shell clients) can map on it
   today; the shell-layer half of the compositor is the prerequisite for every component here, not
   only the greeter; (b) with a head-ray-owned pointer over a client the seat delivers
   `wl_pointer.motion` at tick rate to a still client (62 wake-ups/s in the probe's main thread)
   — against budgets.md's "zero steady-state CPU wake-ups when idle" for the shell plane; the
   unchanged-position dedupe is the compositor's. §6.

## 1. Scope and method

Component classes (registry §5): greeter/lock UI, on-screen keyboard, panels/status surfaces, OSD,
notifications, launcher, task switcher, pager/overview, decoration chrome, and the tray (ruled
"carried", ADR 0012 amendment). Shells: **COSMIC** (Rust; cosmic-session, -panel/-applets,
-launcher, -osd, -notifications, -greeter; libcosmic/iced), **KDE Plasma** (plasma-workspace,
kscreenlocker, maliit-keyboard, KWin, SDDM), **GNOME Shell** (in-process, the counter-example),
**phosh** (the mobile appliance shell; phoc + phosh + squeekboard), the **wlroots ecosystem**
(waybar, fuzzel, mako, dunst, wvkbd, swaylock, hyprlock, greetd's greeters; niri and Hyprland as
the compositors), and the **XR shells** for engineering evidence (WayVR, Stardust/Flatland,
kwin-vr, Simula, WiVRn, Monado). For each row: program, toolkit/renderer, protocols and D-Bus
bound, start/supervision, config/theming, cost evidence, and what transfers.

## 2. The shells as shipped — one paragraph each, with the load-bearing cites

**COSMIC.** `cosmic-session` is the parent of every shell process: a `ProcessManager` with
`set_max_restarts(usize::MAX)` and `ExponentialBackoff(10 ms)` (`references/cosmic-session/src/main.rs:137-143`),
a **hard-coded** component list — `start_component("cosmic-app-library" | "cosmic-launcher" |
"cosmic-osd" | "cosmic-greeter" …)` (`:344-369`) — no config file names them; replacement is a
same-named binary on `PATH`. systemd is optional: `systemctl --user start --no-block
cosmic-session.target` (`src/systemd.rs:35-40`) and transient scopes after spawn; the shell apps are
**not** long-lived user units. The compositor's socket reaches the session over a `UnixStream::pair()`
in `COSMIC_SESSION_SOCK`, and cosmic-comp replies with `WAYLAND_DISPLAY` (+ `DISPLAY`) as JSON
`SetEnv` (`src/comp.rs:107-130`; `cosmic-comp/src/session.rs:59-72`) — children get the *display
name*, not an fd; the panel strips `WAYLAND_SOCKET` when restarting children (`src/notifications.rs:44`).
Binding policy: `not_sandboxed()` is "no security-context, or sandbox engine
`com.system76.CosmicPanel`" (`cosmic-comp/src/state.rs:165-172`); layer-shell, session-lock, IM/VK,
data-control, toplevel-info/management, workspace and a11y are filtered to non-sandboxed clients
(`:702-762`). Greeter mode: `cosmic-comp <cmd>` runs the remaining argv as a kiosk child with the
env from `session::get_env` (`cosmic-comp/src/lib.rs:85-105`); greetd's `default_session` is
`cosmic-greeter-start` as user `cosmic-greeter` (`cosmic-greeter/cosmic-greeter.toml:1-10`), the
script `exec cosmic-comp cosmic-greeter` (`cosmic-greeter-start.sh:1-2`). All components are
libcosmic/iced with **tiny-skia always on** (`libcosmic/Cargo.toml:191-201`) and **`wgpu` off by
default** in every shell crate; `a11y` on only in cosmic-notifications (`Cargo.toml:9-18`).

**KDE Plasma.** One `plasmashell` process hosts desktop views, panels (`PanelView`), applets, the
OSD object and the FDO notification *server* (`plasma-workspace/shell/main.cpp:135-219`,
`shellcorona.cpp:226`, `:1601-1613`; `libnotificationmanager/server_p.cpp:92-109` — only the
`_plasma_dbus_master` owns `org.freedesktop.Notifications`). Panels are **LayerShellQt** windows:
`LayerTop`, keyboard `None` (or `OnDemand` while accepting input), scope `dock`, exclusive zone =
thickness (`shell/panelview.cpp:63-67`, `:1432-1450`, `:1572-1598`); the desktop is `LayerBackground`
(`desktopview.cpp:46-49`). The task manager binds `org_kde_plasma_window_management`, not
ext-foreign-toplevel (`libtaskmanager/waylandtasksmodel.cpp:379-398`); activation tokens come from
`xdg-activation` for tray/notification/krunner launches (`applets/systemtray/systemtray.cpp:511+`,
`libnotificationmanager/server.cpp:80`). The OSD is in-process behind D-Bus `org.kde.osdService`
(`shell/osd.h:24-39`, `osd.cpp:39`). The tray *host* is a plasmashell applet, the *watcher* a kded
module (`statusnotifierwatcher/statusnotifierwatcher.cpp:19-27`). Session: `startplasma-wayland`
starts `plasma-workspace-wayland.target` (`startkde/startplasma.cpp:741-746`); the units are
`plasma-plasmashell.service` (`PartOf=graphical-session.target`, `Restart=on-failure`,
`StartLimitBurst=3`, `Type=dbus`), `plasma-krunner.service` (`Restart=no`), `plasma-kwin_wayland.service`
(no `Restart=`); targets `plasma-core.target` → `plasma-workspace.target` (`BindsTo=graphical-session.target`)
→ `plasma-workspace-wayland.target`. The lock: `KSldApp` is a library **inside kwin_wayland**
(`kwin/src/wayland_server.cpp:609-641`); it spawns `kscreenlocker_greet` with a **pre-connected
`WAYLAND_SOCKET`** (`kscreenlocker/ksldapp.cpp:377-422`), the greeter maps on `LayerTop`,
`KeyboardInteractivityExclusive`, exclusive zone −1 (`greeter/greeterapp.cpp:404-410`), restarts it
up to 4 times then shows the black `EmergencyWindow` (`ksldapp.cpp:199-210`, `emergencywindow.cpp:26-55`),
and PAM runs in a `kscreenlocker_worker` (`greeter/pamauthenticator.cpp:295-319`). The IM:
KWin starts the configured `X-KDE-Wayland-VirtualKeyboard` desktop entry (`kwinrc` `[Wayland]
InputMethod`) with `WAYLAND_SOCKET=<fd>`, `QT_QPA_PLATFORM=wayland`, ≤ 5 crash restarts
(`kwin/src/inputmethod.cpp:864-926`, `main_wayland.cpp:196-214`), advertising `zwp_input_method_v1` +
`zwp_input_panel_v1` to that client only (`wayland_server.cpp:141`). Decorations: an in-process
KDecoration3 plugin (`decorationbridge.cpp:130-146`); `zxdg_decoration_manager_v1` +
`org_kde_kwin_server_decoration_manager` served. Everything UI is Qt Quick/QML. Replaceability:
look-and-feel packages, `plasmashell -p <shell-plugin>` (`main.cpp:144-147`), applets by
`X-Plasma-Provides` alternatives (`alternativeshelper.cpp:26-74`), the IM by desktop entry. SDDM's
greeter is a separate `sddm-greeter` process as user `sddm` over a `QLocalSocket`
(`sddm/src/greeter/Greeter.cpp:83-119`).

**GNOME Shell.** The shell is a **mutter plugin in the compositor process**
(`gnome-shell/src/main.c:616-629`, `gnome-shell-plugin.c:24-28`); panel, overview, app grid, message
tray, OSD, screen shield/unlock dialog, on-screen keyboard and the GDM login dialog are St/Clutter
actors built in `js/ui/main.js:229-253` (GJS). It *is* the FDO notification server
(`js/ui/notificationDaemon.js:35-38`) and owns `org.gnome.Shell`; it hosts no StatusNotifier. Mutter
implements **no layer-shell** and **no `zwp_input_method`** — only `zwp_text_input_v3` bridged to
Clutter's IM (`mutter/src/wayland/meta-wayland-text-input.c:32`, `:129-134`); the OSK is an
in-process St widget driving a virtual keyboard device (`js/ui/keyboard.js:1031-1035`, `:1855-1856`).
The cost of in-process is written into its unit: `Restart=no`,
`OnFailure=org.gnome.Shell-disable-extensions.service gnome-session-shutdown.target`,
`OOMScoreAdjust=-1000` (`data/org.gnome.Shell@.service.in:2-4`, `:27-28`) — a crashing extension
takes the session down and the failure unit disables extensions (`data/org.gnome.Shell-disable-extensions.service:7-14`,
`js/ui/extensionSystem.js:54-57`). Greeter mode is the same binary, `--mode=gdm`
(`main.c:518-528`, `:636-637`; `gdm/data/gnome-login.session.conf:1-2` `Requires=org.gnome.Shell@gdm.service`),
talking to GDM over libgdm (`js/gdm/loginDialog.js:392`). a11y is ATK through Clutter's actor
accessibles (`st/st-widget.c:2421`). No in-tree rationale for in-process beyond "the shell is a
mutter plugin"; the premise is a compositor that already hosts a toolkit.

**phosh.** A separate **GTK3 + libhandy** client of phoc (`phosh/meson.build:156-157`, `:192-195`;
`README.md:15`, `:66-77`), started by `gnome-session` under phoc:
`exec "${COMPOSITOR}" … -E "${PHOSH_SESSION_BIN} --session=phosh"` (`data/phosh-session.in:60`);
the shell is a user unit `mobi.phosh.Shell.service` with `Restart=on-failure`,
`OnFailure=gnome-session-shutdown.target`, `OOMScoreAdjust=-1000` (`data/systemd/mobi.phosh.Shell.service.in`),
and `X-GNOME-AutoRestart=true` on its desktop entry. It binds `zwlr_layer_shell_v1`,
`zwlr_foreign_toplevel_manager_v1`, `phosh_private` v7 (thumbnails, keyboard-accelerator
subscription, startup tracker, shell-up state — `protocol/phosh-private.xml`), output-power,
gamma, xdg-output, `ext_idle_notifier_v1` (`src/phosh-wayland.c:43-151`). Layers: top panel on
`TOP`/`OVERLAY` by lock state (`shell.c:284`, `top-panel.c:1127-1130`), home bar `TOP`
(`home.c:866-869`), **lock screen on `OVERLAY`, full anchors, exclusive −1**
(`lockscreen.c:1218-1227`) — phosh does not use `ext-session-lock`; it authenticates with **PAM
directly**, service `phosh` (`src/auth.c:80-87`). It is the FDO notification server
(`notifications/notify-manager.c:48`, `:686-694`), implements `org.gnome.ScreenSaver`
(`screen-saver-manager.c:29-33`), lists apps through `GDesktopAppInfo` (`app-grid.c:172`), and
controls the OSK only as a *preference* over `sm.puri.OSK0` (`osk-manager.c:18-30`: "any text input
can make the keyboard show again"). **squeekboard** (GTK3 + Rust) is a separate process: layer
`TOP`, anchors bottom|left|right, namespace `osk` (`squeekboard/src/panel.c:76-85`); binds
`zwp_input_method_manager_v2` + `zwp_virtual_keyboard_manager_v1` + layer-shell
(`server-main.c:115-193`); owns `sm.puri.OSK0` (`:200`, `dbus.h:29-30`); 104 layouts
(`data/keyboards/*.yaml`); autostarted by the session (`sm.puri.Squeekboard.desktop.in.in:5-16`,
`X-GNOME-AutoRestart=true`), required as `sm.puri.OSK0` in the phosh session
(`phosh/data/meson.build:19-22`). Its typing path: `commit_string` for text; **Erase is a
virtual-keyboard Backspace, never `delete_surrounding_text`** — "takes byte offsets, so cannot work
without get_surrounding_text. This is a bug in the protocol." (`src/submission.rs:116-150`); it
never sends preedit.

**The wlroots ecosystem.** One tool per job, each with its own toolkit: **waybar** (gtkmm-3 +
gtk-layer-shell; `meson.build:86`, `:122`) — layer/exclusive/position from JSONC, CSS themes,
StatusNotifier watcher+host in its tray module (`modules/sni/watcher.cpp:11`, `host.cpp:20-22`),
`zwlr_foreign_toplevel` taskbar, UPower/login1 over D-Bus; **fuzzel** (fcft + pixman, no toolkit) —
`OVERLAY` + `EXCLUSIVE` keyboard by default (`config.c:1725-1726`), its own `.desktop` scanner
(`xdg.c:671-723`), `xdg_activation_v1` on launch (`wayland.c:575-673`), `text-input-v3` for its own
field; **mako** (cairo + pango) — the FDO server, layer `TOP` anchor top|right (`config.c:126-130`),
`fr.emersion.Mako` control interface, shipped `mako.service` `Type=dbus BusName=org.freedesktop.Notifications`
(`contrib/systemd/mako.service:1-15`), D-Bus activation preferred (`README.md:16-24`); **dunst**
(cairo) — layer-shell with xdg-shell fallback (`wl.c:336-341`, `:558-577`); **wvkbd** (cairo +
pango) — `OVERLAY`, anchors bottom|left|right, exclusive zone = height, keyboard interactivity
*none* (`main.c:63-66`, `:828-833`), types only through `zwp_virtual_keyboard_v1` (`:469-471`),
binds `input-method-v2` only to show/hide with `--auto` (`:525-533`), toggled by SIGUSR1/2
(`:1314-1319`); **swaylock** (cairo) — `ext_session_lock_v1` (`main.c:1206-1213`), PAM in a forked
child over a pipe (`pam.c:13-22`, `comm.c:86-140`), failure stays locked (`main.c:1045-1052`);
**hyprlock** (EGL + GLES3 shaders, cairo for text; `Renderer.cpp:11-13`) — same lock protocol.
Greeters: **greetd**'s IPC is length-prefixed JSON over `$GREETD_SOCK` — `CreateSession`,
`PostAuthMessageResponse`, `StartSession{cmd, env}`, `CancelSession` (`greetd_ipc/src/lib.rs:19-88`),
`[default_session] command … user = "greeter"` (`config.toml:7-16`); **gtkgreet** (GTK3) under
`cage -s`, sessions from `/etc/greetd/environments` (`config.c:9-34`); **regreet** (GTK4 + relm4,
`Cargo.toml:27-32`) globs `wayland-sessions/*.desktop` (`sysutil.rs:114-146`), CSS at
`/etc/greetd/regreet.css`; **tuigreet** (terminal). How they are started: compositor config
(`exec`, niri `spawn-at-startup`, Hyprland `exec-once`/Lua start hook) or D-Bus activation;
niri's own session is a user unit (`resources/niri-session:25-47`, `niri.service:11-14`) and its
docs name systemd *or* `spawn-at-startup` for mako/polkit (`docs/wiki/Important-Software.md:5`, `:34`).
Binding policy: niri's per-global `client_is_unrestricted` (`niri.rs:2388-2389`, `:2415-2485`;
security-context children are `restricted: true`, `handlers/mod.rs:504-508`); Hyprland's static
whitelist for sandboxed clients — everything not on it is privileged, layer-shell, session-lock,
foreign-toplevel and IME included (`ProtocolManager.cpp:348-403`, `Compositor.cpp:267-288`);
wlroots the library hook (`wlr_security_context_v1.h:14-19`).

**XR shells (engineering evidence only).** **WayVR** draws its dashboard, keyboard, watch and toasts
*in-process* with its own Vulkan `wgui` (`wgui/README.md:7-9`); placement is hand-coded — the dash
floating at z = −0.9 (`wayvr/src/overlays/dashboard.rs:365-383`), the keyboard anchored at
(0, −0.65, −0.5) (`overlays/keyboard/mod.rs:98-111`), the watch on the left wrist at scale 0.115
(`overlays/watch.rs:131-148`); it types through uinput / `zwp_virtual_keyboard_v1`
(`subsystem/hid/provider/wl_virtual.rs:221-225`) and listens to D-Bus notifications
(`subsystem/notifications.rs:71`). **Stardust** is a display server with no shell UI of its own —
Flatland and the launcher are separate clients (`stardustxr-server/README.md:3`, `flatland/README.md:1-14`);
Flatland's default placement is HMD-relative (0, 0, −0.25) (`src/initial_panel_placement.rs:42-57`).
**kwin-vr** brings Plasma's *existing* client windows into 3D under a grab handle at `-distance`,
filters OSD windows and draws them head-near (`windowmodelfilter.cpp:90-124`, `XrScene.qml:170-174`,
`:258-321`) — it has no layer-shell → VR mapper. **WiVRn**'s headset UI is in-process Dear ImGui:
head-locked compact/overlay, world-locked settings/apps (`client/scenes/stream_gui.cpp:725-755`), a
built-in virtual keyboard (`lobby_keyboard.cpp:33-34`); its PC dashboard is Qt6/Kirigami
(`dashboard/CMakeLists.txt:8-9`, `:71-83`). **Monado**'s only UI is the SDL2 + ImGui debug GUI
(`gui_sdl2_imgui.c:18`). None publishes a footprint number.

## 3. By component class

Legend: **proc** = separate process; **in** = in the compositor's process; toolkit; seams bound;
start/supervision; what transfers to Mura and what does not. Cites are §2's unless given.

### 3.1 Greeter and lock UI

| shell | shape | toolkit | seams | supervised by | transfers |
|---|---|---|---|---|---|
| COSMIC | **proc** `cosmic-greeter` = greetd frontend *or* session locker by user name (`src/main.rs:34-38`); root `cosmic-greeter-daemon` proxies user data | libcosmic/iced tiny-skia, **no a11y** | greeter: layer-shell `Top`, all anchors, `Exclusive` keyboard, zone −1 (`greeter.rs:1341-1359`); locker: `ext-session-lock` (`locker.rs:10`, `905+`); greetd IPC; PAM for the *lock* in-process (`locker.rs:133`) | greetd (system unit `Restart=always`); in-session by cosmic-session | one program for both modes; power menu over logind; sessions from `wayland-sessions`. Not: PAM inside the locker UI (Mura's `mura-authd` split is stronger, session-auth §2) |
| Plasma | **proc** `kscreenlocker_greet` over a pre-connected `WAYLAND_SOCKET`; `sddm-greeter` as user `sddm` | Qt Quick | layer-shell `Top` + `Exclusive` (kscreenlocker); QLocalSocket to sddm | KSldApp in kwin: ≤ 4 restarts then `EmergencyWindow`; PAM in `kscreenlocker_worker` | the fd channel, the worker split, exclusive keyboard on the top layer. Not: the hand-rolled restart loop and emergency UI (research/12 §3, ADR 0007 amendment) |
| GNOME | **in** (`--mode=gdm`, `ScreenShield`) | St/Clutter, GJS | libgdm D-Bus; `org.gnome.ScreenSaver` | the shell unit (`Restart=no`) | nothing structural — the premise (compositor = toolkit host) is not Mura's |
| phosh | **in the shell proc** (`PhoshLockscreen`, layer `OVERLAY`, exclusive −1); PAM direct, service `phosh` | GTK3 | layer-shell; no `ext-session-lock` | the shell unit | the overlay-layer lock surface; the direct-PAM shape is what session-auth §1 rejects |
| wlroots | **proc** swaylock/hyprlock; gtkgreet/regreet/tuigreet under cage as `greeter` | cairo / GLES / GTK3 / GTK4 | `ext_session_lock_v1`; `$GREETD_SOCK` | compositor `exec`; greetd | the protocol's lock semantics; greetd's "the greeter is any program" |

**Reads:** every shell but GNOME draws this scene as a client; the two that spawn it themselves
(KWin, cosmic-comp kiosk mode) hand it a channel and compose only it. ADR 0007's 2026-09-27
amendment is the majority shape. The session list comes from `wayland-sessions/*.desktop` in
regreet/tuigreet/cosmic-greeter — Mura's comes from `mura.xr.shell` (ADR 0007), so the greeter
program takes it from zxr or the module system, not a scan.

### 3.2 On-screen keyboard

| shell | shape | seams | show/hide | typing | transfers |
|---|---|---|---|---|---|
| phosh + **squeekboard** | proc, GTK3 + Rust | layer `TOP` bottom|left|right, ns `osk`; `input-method-v2` + `virtual-keyboard-v1`; `sm.puri.OSK0` D-Bus | IM `activate`/`deactivate`; `SetVisible` as a preference | `commit_string`; Erase = virtual-keyboard Backspace (protocol gap, `submission.rs:116-150`); no preedit | the whole shape: research/60 §10 already chose it; the Erase fact matters for the toolkit (§5.4) |
| wlroots **wvkbd** | proc, cairo + pango | layer `OVERLAY`, exclusive zone = height, keyboard interactivity none; `virtual-keyboard-v1`; `--auto` binds IM only for visibility | signals / IM activate | virtual-keyboard keys only | a smaller comparator; XKB layouts in headers |
| Plasma **maliit** | proc, QML; KWin spawns it with `WAYLAND_SOCKET`, `zwp_input_method_v1` + `input_panel_v1` to it only; ≤ 5 restarts | IM v1 | KWin `org.kde.kwin.VirtualKeyboard` | Maliit framework; **sends preedit** (`maliit-keyboard/src/plugin/editor.cpp:56-74`) | compositor-spawned IM over an fd — the same channel as the greeter; v1 is the old protocol |
| GNOME | in-process St keyboard, Clutter IM, mutter `text-input-v3` only | — | a11y key / touch mode | virtual keyboard device | not the shape |
| COSMIC | none in the pinned set (cosmic-osk is not a pinned clone) | — | — | — | — |
| XR | WayVR built-in (uinput / `virtual-keyboard`), WiVRn ImGui | — | — | keys | pattern evidence only (research/36 §7) |

**Reads:** a Mura OSK is a layer-shell client binding `input-method-v2` + `virtual-keyboard-v1`
(ADR 0012 amendment, squeekboard's shape), on a **body/hand frame** rather than `bottom` of an
output (research/60 §10). Both shipping wlroots OSKs type through `virtual-keyboard-v1` and use
`delete_surrounding_text` never (squeekboard) or not at all (wvkbd) — so winit's unhandled
`DeleteSurroundingText` (§5.4) does not break either of them; maliit's preedit would exercise a
path Slint handled correctly in the probe.

### 3.3 Panels and status surfaces

| shell | shape | seams | transfers |
|---|---|---|---|
| COSMIC **cosmic-panel** | proc: a nested smithay *server* hosting applets as clients over `WAYLAND_SOCKET` socketpairs (`wrapper_space.rs:599-601`), itself a layer-shell client of cosmic-comp | layer `Top` (config), `OnDemand` keyboard, `exclusive_zone: true` (`container_config.rs:139-171`, `panel_space.rs:954-955`); `foreign-toplevel-list` + `cosmic-toplevel-info`; `ext-workspace`; `wp_security_context` (engine `com.system76.CosmicPanel`); optional privileged fd `X_PRIVILEGED_WAYLAND_SOCKET` (`:579-582`) | applets as separate processes behind one panel surface; the security-context identity trick; `malloc_trim` hygiene (`main.rs:57-75`) — no numbers |
| Plasma | in `plasmashell`: `PanelView` per screen; `LayerTop`, zone = thickness, `None`/`OnDemand` | LayerShellQt; `org_kde_plasma_window_management` | the layer/zone/keyboard rules; not the monolith |
| GNOME | in-process `Panel` actor | — | — |
| phosh | in the shell proc: top panel `TOP`/`OVERLAY` by lock state; home bar `TOP` bottom-anchored | layer-shell | the lock-time promotion to `OVERLAY` |
| wlroots **waybar** | proc, gtkmm + gtk-layer-shell | `layer` `bottom|top|overlay`, `exclusive`, `position` from JSONC (`bar.cpp:65-104`, `:384-395`); tray = StatusNotifier watcher+host; UPower/login1 | config surface names; the tray host inside the panel (ADR 0012 ruled the watcher/host "carried") |

**Reads:** panels are `top`-layer clients with an exclusive zone; in XR the zone is the *exclusive
angular band* of `zxr-layer-anchoring-v1` on a body or head frame (research/60 §1, spec §4 band
4). COSMIC's nested-server panel (applets as its clients) is the one structural novelty; it buys
applet isolation at the cost of a second compositor in the process — flagged, not adopted here.

### 3.4 OSD

| shell | shape | trigger | layer | transfers |
|---|---|---|---|---|
| COSMIC **cosmic-osd** | proc, libcosmic tiny-skia, no a11y | owns `com.system76.CosmicOsd`; audio/brightness/keyboard-backlight/airplane subscriptions; **polkit agent** in the same process (`polkit_agent.rs:124`); CLI tasks (`app.rs:71-95`) | `Overlay`, keyboard `None`, bottom margin 48, auto-close 3 s (IDs 1 s); polkit dialog `Exclusive` (`osd_indicator.rs:201-230`, `polkit_dialog.rs:74-79`) | a D-Bus-fed overlay client; the 3 s timeout; polkit-agent co-location is a judgment |
| Plasma | in `plasmashell` behind `org.kde.osdService` (`osd.h:24-39`) | D-Bus | QML `Osd` | the D-Bus trigger interface shape |
| GNOME | in-process `OsdWindow` | — | — | — |
| phosh | in the shell proc as a `PhoshSystemModal` on `OVERLAY` (`osd-window.c:56`, `system-modal.c:144`) | — | overlay | overlay placement |

**Reads:** research/60 §10 already ruled "a layer-shell client fed over D-Bus, overlay band, head
frame"; every comparable that is a client fits it. The compositor's own volume/brightness/mode
events reach it over D-Bus (cosmic-osd's subscriptions are the model), never a private protocol.

### 3.5 Notifications

| shell | server | UI | placement | transfers |
|---|---|---|---|---|
| COSMIC **cosmic-notifications** | is `org.freedesktop.Notifications` (`subscriptions/notifications.rs:44-48`) | same process, layer-shell cards `exclusive_zone: 0`, keyboard `None`, ns `notifications` (`app.rs:306-325`); panel applet over a socketpair | anchor from panel config; defaults `max_notifications 3`, `max_per_app 2`, normal 5 s / low 3 s / urgent none, `do_not_disturb` (`cosmic-notifications-config/src/lib.rs:38-48`) | server + UI in one client; the config surface; xdg-activation tokens for actions |
| Plasma | in `plasmashell` (`_plasma_dbus_master`) | applet QML | — | not the monolith; DND via `Server::inhibited` |
| GNOME | in-process + `org.gnome.Shell.Notifications` helper | St | — | — |
| phosh | in the shell proc (`notify-manager.c:48`) | GTK | — | — |
| wlroots **mako** / **dunst** | proc, each is the FDO server | cairo/pango cards | mako `TOP` anchor top|right, timeout default 0, `fr.emersion.Mako` modes; dunst `overlay`, urgency timeouts 10/10/0 s, pause | `Type=dbus BusName=org.freedesktop.Notifications` unit (mako) — D-Bus activation as the start shape; mode/DND control interfaces |

**Reads:** research/36 §4 and /60 §7 — head-locked transient toasts on the overlay band, DND by
immersion, critical bypass. Whether Mura carries mako (cairo, tiny, D-Bus-activated, no spatial
awareness) or writes its own on the shell toolkit is §7 Q4.

### 3.6 Launcher, switcher, overview

| shell | launcher | switcher / overview | seams | transfers |
|---|---|---|---|---|
| COSMIC **cosmic-launcher** | proc, tiny-skia; `pop-launcher` IPC for search/desktop entries (`subscriptions/launcher.rs:35-116`); dummy `Bottom` surface + visible `Exclusive` top-anchored surface, width ≤ 600 (`app.rs:217-255`); `xdg-activation` tokens + `DESKTOP_STARTUP_ID` (`:587-599`); D-Bus single-instance activation | `cosmic-workspaces` / app-list applet over `cosmic-toplevel-info` + `ext-workspace` | layer-shell, activation, toplevel-info, workspace | the token path; the separate search service is a judgment |
| Plasma | kickoff applet in plasmashell; **krunner** separate proc (`Restart=no`) | task manager applet over `org_kde_plasma_window_management` | plasma protocols | not the protocols (KDE-private) |
| GNOME | in-process app grid / overview | in-process | — | — |
| phosh | in the shell proc: `PhoshAppGrid` over `GDesktopAppInfo` | overview in-process; `phosh_private` thumbnails via screencopy | `zwlr_foreign_toplevel`, `phosh_private` | `GDesktopAppInfo` for entries; thumbnails need a private protocol — Mura's is the compositor-rendered previews of places-model §6 |
| wlroots **fuzzel** | proc, fcft/pixman, its own `.desktop` scanner, `OVERLAY` + `EXCLUSIVE` keyboard, `xdg_activation_v1` | waybar taskbar over `zwlr_foreign_toplevel` | layer-shell, activation, `text-input-v3` | overlay + exclusive keyboard for a modal launcher |

**Reads:** research/36 §3 ("flat pinned grid + search on one reserved gesture is universal") and
/60 §9 stand. The launcher is the component native-openxr-apps §3 makes the scope owner of native
apps; it needs `xdg-activation` and the per-app scope, both already designed. Every OSS shell wrote
its own desktop-entry scanner (research/36 §3); GLib's `GDesktopAppInfo` and Rust's
`freedesktop-desktop-entry` (cosmic's `cosmic::desktop`) are the two library shapes.

### 3.7 Decoration chrome and the tray

**Chrome:** KWin loads a KDecoration3 plugin in-process (`decorationbridge.cpp:130-146`) and serves
`xdg-decoration`; GNOME draws SSD in mutter; wlroots compositors mostly leave CSD to clients.
ADR 0012 rules Mura's chrome compositor-drawn (the affordances are trusted hit volumes); the
registry's "zero design for what the chrome *is* in 3D" stands — §7 Q6.
**Tray:** StatusNotifier watcher + host: a plasmashell applet + kded module (Plasma), an applet in
cosmic-panel (`cosmic-applet-status-area/…/server.rs:18-27`), waybar's `sni` module; GNOME hosts
none. ADR 0012 ruled it carried as a separate host applet — every comparable that has one puts it
in the panel process.

## 4. Cross-cuts

### 4.1 Process model, start and supervision

| shell | who starts components | restart | on repeated failure |
|---|---|---|---|
| GNOME | systemd user units (`org.gnome.Shell@.service`, `PartOf`/`After=gnome-session-initialized.target`) | `Restart=no` | `OnFailure=…-disable-extensions.service gnome-session-shutdown.target` |
| Plasma | systemd user units under `plasma-core`/`plasma-workspace(-wayland).target` | plasmashell `Restart=on-failure`, `StartLimitBurst=3`; krunner `Restart=no`; kwin none | — ; KWin supervises its own children (locker ≤ 4, IM ≤ 5) |
| phosh | systemd user unit (`mobi.phosh.Shell.service`) + `X-GNOME-AutoRestart` | `Restart=on-failure` | `OnFailure=gnome-session-shutdown.target` |
| COSMIC | `cosmic-session`'s `ProcessManager` (hard-coded list) | unlimited, exponential backoff from 10 ms | none; optional transient scopes; `cosmic-session.target` started `--no-block` |
| wlroots | compositor `exec` / `spawn-at-startup` / `exec-once`; D-Bus activation for mako | none | — |

Three of the four desktops that supervise at all use **systemd user units with
`PartOf=graphical-session.target` and `Restart=on-failure`**; COSMIC's own manager is the outlier
and its `usize::MAX` restarts with backoff is exactly what `Restart=` + `StartLimit*` express
declaratively. Mura's session is already this shape (`mura-session.target`,
`specs/session-bootstrap.md:50-68`; research/30's autostart addendum, `30:870`: "native shell
under `mura-session.target`"). A component that must exist for the session to be usable
(compositor) is `Requires=`; shell components are `Wants=` + `PartOf=graphical-session.target` so a
failed panel never tears the session down (Plasma's krunner `Restart=no`, GNOME's `OnFailure` are
the two ways to say "the session survives / does not survive this component").

### 4.2 Binding policy (the trusted set)

Two identities exist in the comparables and both are needed: **connection identity** — a
`wp_security_context_v1` sandbox engine/app-id marks a client *restricted* and the compositor's
global filter hides the privileged set (niri `client_is_unrestricted`, Hyprland's whitelist,
cosmic-comp `not_sandboxed()`; research/30 `:191-205`, ADR 0012 §5); and **channel identity** —
the compositor (or panel) spawns the process itself and hands it a pre-connected fd
(`WAYLAND_SOCKET`: kscreenlocker's greeter, KWin's IM, cosmic-panel's applets, cosmic-panel's
optional privileged fd to the compositor), so what may bind is decided by *who was given the
socket*. The privileged set is stable across all four: layer-shell, session-lock, input-method +
virtual-keyboard, foreign-toplevel(-list/-management), workspace, data-control, screencopy/export,
(cosmic) toplevel-info/management, a11y managers. Mura: the greeter/lock program and the OSK are
channel-identified (spawned over a socketpair — ADR 0007's amendment; KWin's IM is the exact
precedent for the OSK); the session's own panel/launcher/OSD/notifications connect to the public
socket as unrestricted clients; anything under a `security-context` is restricted (research/30's
rule).

### 4.3 Anchoring and layers

Layer-shell's four layers keep their meanings (spec §4): panels `top` with an exclusive zone;
launcher/lock/greeter `top`/`overlay` with `Exclusive` keyboard interactivity and zone −1 (fuzzel,
kscreenlocker, cosmic-greeter, phosh lock); OSD/notifications `overlay`, keyboard `None`, zone 0;
OSK `top` (squeekboard) or `overlay` (wvkbd) with its height as the zone and keyboard `None`. In
XR the *frame* replaces the output edge: `zxr-layer-anchoring-v1`'s `head` (VIEW), `body`
(position + yaw, not pitch/roll), `hand`, `world`, `docked` (`protocols/zxr-layer-anchoring-v1.xml:59-74`),
with `set_exclusive_angle` as the exclusive zone (`:178+`) and **head** as the default for unaware
clients (`:118-120`). The XR shells hard-code the same taxonomy: WayVR watch = hand, keyboard =
anchored below the view, dash = floating/world; WiVRn compact UI head-locked, settings
world-locked; Flatland HMD-relative first placement. So: greeter/lock/OSD/notifications → head;
panel/status and the OSK → body (the keyboard within reach, the panel at the periphery; research/60
§10); launcher → head at spawn distance; overview → a place transition (places-model §6).

### 4.4 Accessibility

GTK and Qt shells get AT-SPI from their toolkits (phosh via GTK3; kscreenlocker/plasmashell via
Qt's AT-SPI bridge; GNOME via Clutter/ATK). The Rust shells depend on **AccessKit → AT-SPI over
zbus** (`accesskit/adapters/unix/Cargo.toml:19-25`): Slint enables it under `accessibility`
(`slint/internal/backends/winit/Cargo.toml:59`, `:112-113` — `accesskit_unix` + `async-io`);
libcosmic behind `a11y` (`libcosmic/Cargo.toml:22`) which **cosmic-greeter, -panel, -osd and
-launcher do not enable** (`cosmic-greeter/Cargo.toml:23-31`; cosmic-notifications does,
`Cargo.toml:9-18`). cosmic-comp's `cosmic_a11y_manager_v1` is magnifier/invert/colour-filter, not
AT-SPI (`cosmic-comp/src/wayland/protocols/a11y.rs:3-14`); cosmic-session starts Orca when the
screen reader is enabled (`cosmic-session/src/a11y.rs:8-41`). AccessKit's own completeness note:
"rough feature parity … single-line and multi-line text … don't yet support rich text or
hypertext" (`accesskit/README.md:35`); the pinned `accesskit_unix` 0.22 exposed `Text` but not
`EditableText` on the probe's fields (§5.4), 0.25 in the clone has the interface
(`adapters/atspi-common/src/node.rs:457-458`). The adapter connects to the a11y bus only when
`org.a11y.Status.IsEnabled` is true and the registry answers `Embed` (`adapters/unix/src/context.rs:184-200`,
`atspi/bus.rs:42-68`) — the a11y bus and `at-spi2-registryd` are session infrastructure Mura must
run (spatial-a11y §1 already requires AT-SPI). Pre-login, that is the greeter's session bus.

### 4.5 Replaceability and theming (rule 3, overview invariant 10)

greetd: the greeter is any program (`config.toml:7-16`). COSMIC: a same-named binary on `PATH`
replaces a component (hard-coded names); applets are config-driven. Plasma: look-and-feel
packages, `-p` shell plugins, applet alternatives, the IM by desktop entry. phosh: the OSK is
whichever process owns `sm.puri.OSK0`. wlroots: everything is a config line. Theming: GTK CSS
(waybar, regreet), QML packages (Plasma), cosmic-theme (COSMIC), INI (fuzzel, mako, dunst). For
Mura the mechanism is systemd's: a component is a user unit the administrator can `mask`,
override or replace with a drop-in (`ExecStart=`), and its config is settings keys declared when
the consumer lands (research/73's rule) — no in-compositor look.

### 4.6 Budget

No comparable publishes RSS, closure or start numbers for its shell components (searched READMEs,
docs, CHANGELOGs of every pinned clone; cosmic-panel and plasmashell only tune glibc
`mallopt`/`malloc_trim`, `cosmic-panel-bin/src/main.rs:57-75`, `plasma-workspace/shell/main.cpp:47-57`).
budgets.md §3 gives the shell/service plane one rule: **damage-driven only; zero steady-state CPU
wake-ups when idle; panels/OSDs never animate uncapped**. The measured probe (§5.4) is the first
number: ~20 MB RSS / 12.6 MB PSS per Slint process. A five-process shell (greeter-or-lock, OSK,
panel, notifications/OSD, launcher) at that size is ≈ 60–65 MB PSS if nothing is shared beyond
glibc — the same order as zxr itself (RSS anon 7.5 MB + binary + the ICD; host total 55–60 MB,
spec §12). Whether that is acceptable on a class-B device is a budgets.md §3 partition decision
the design doc must state; the per-process cost is why the design keeps the count low
(cosmic-osd's polkit-agent co-location and cosmic-notifications' server+UI co-location are the
comparables' answer to the same pressure).

## 5. The toolkit

### 5.1 What the comparables use, and why

| toolkit | used by | reasons given / evident | cost shape |
|---|---|---|---|
| **libcosmic / iced (pop-os fork)** | every COSMIC component; cosmic-greeter | one Rust crate family for the whole shell; tiny-skia software path always compiled, wgpu optional; applet helpers; cosmic-config theming | Rust, static; `a11y` optional and mostly off; the iced fork is not upstream iced (`libcosmic/.gitmodules:1-4`, gitlink `b2419520…`, submodule not checked out in the pin) |
| **GTK3/4 (+ libhandy/adwaita)** | phosh, squeekboard, waybar (gtkmm), gtkgreet, regreet (relm4) | mature a11y, CSS theming, `GDesktopAppInfo`; the mobile shell's choice | C, dynamic, large closure (GLib/GTK/pango/cairo/graphene/GSK) |
| **Qt Quick / QML** | plasmashell, kscreenlocker_greet, maliit, sddm, kwin-vr, WiVRn dashboard | KDE's stack; declarative UI | a JS engine in the greeter — excluded by rule 6 |
| **cairo + pango / fcft + pixman** | mako, dunst, wvkbd, swaylock, fuzzel | smallest possible; no widgets | no widgets, no a11y tree, no text input — a floor, not a competitor |
| **in-process (St/Clutter, ImGui, wgui, Godot)** | GNOME Shell, WiVRn, WayVR, Simula | the compositor/runtime is the toolkit host | rejected by ADR 0012 / ADR 0007's amendment |
| **Slint** | none of the pinned shells | designed for embedded: AOT-compiled `.slint`, selectable software renderer, `no_std` path, AccessKit | the candidate; measured below |

### 5.2 The web research absorbed [external], with what the corpus confirms or corrects

The owner-run toolkit review (2026-09-27, [external]: documentation and source review of Slint
1.18.x, winit 0.30.13, iced 0.14, libcosmic, AccessKit) recommended **Slint + software renderer as
the first candidate, libcosmic/tiny-skia + a11y as the comparator**, and made these corrections,
each checked here against the pinned sources or the probe:

- *Rust ≠ static.* Confirmed and measured: Slint's winit `wayland` feature enables
  `softbuffer?/wayland-dlopen` (`slint/internal/backends/winit/Cargo.toml:31-32`); the resolved graph
  carries `xkbcommon-dl 0.4.2` and `winit 0.30.13` (probe `cargo tree`); the built binary has
  `NEEDED libfontconfig.so.1 libgcc_s libm libc` and **dlopens `libwayland-client` and
  `libxkbcommon`** — without them on the library path it aborts with "The wayland library could
  not be loaded" (probe run 1). A Nix package supplies them as an rpath, as `pkgs/zxr` does for
  libvulkan. fontconfig is a link-time dependency through `yeslogic-fontconfig-sys` (fontique's
  `fontconfig-dlopen` feature notwithstanding, the sys crate links unless `RUST_FONTCONFIG_DLOPEN`
  is set at build).
- *`DeleteSurroundingText` unhandled in winit 0.30.13.* Confirmed in source
  (`winit-0.30.13/src/platform_impl/linux/wayland/seat/text_input/mod.rs:152-154`, `// Not
  handled.`) and **observed**: `delete_surrounding_text(1, 0)` between two commits left the field
  `abcéZ` instead of `abcZ`. Mitigated in practice: squeekboard and wvkbd never send it (§3.2).
- *Number/decimal → `ImePurpose::Normal`.* Confirmed in source
  (`slint/internal/backends/winit/winitwindowadapter.rs:2132-2138`) and observed (`content_type
  purpose=Normal` on the `input-type: number` field; `Password` + `SensitiveData` on the password
  field). An OSK cannot switch to a digit pad from the purpose alone; the digit-pad rendering is the
  greeter program's own (ADR 0007: selected by the non-secret hint), so this does not block it.
- *Software renderer "western scripts only".* The docs say so
  (`slint/docs/astro/src/content/docs/guide/backends-and-renderers/backends_and_renderers.mdx:85`),
  but with `std`/`systemfonts` the renderer uses fontique + parley + swash
  (`internal/renderers/software/Cargo.toml:19-31`) and the probe rendered every script tried
  (§5.4). The limitation describes the pre-rendered-font `no_std` path.
- *Licence.* Confirmed: royalty-free excludes embedded systems (`slint/LICENSE.md:17-22`, `:32`;
  `LICENSES/LicenseRef-Slint-Royalty-free-2.0.md:33`); Mura's route is **GPLv3**, which is the
  project's licence class anyway (`pkgs/zxr/default.nix` `gpl3Plus`).
- *iced has no `no_std`; Slint does.* Not re-verified here (upstream iced is not pinned; libcosmic's
  iced submodule is absent from the pin). Slint's `no_std` path is in the pinned tree
  (`internal/renderers/software/lib.rs:11` `#![no_std]`; `api/rs/slint/Cargo.toml:43`). The review's
  own caveat stands: reach is not a size proof.
- *egui idle redraw.* Not measured; egui is out for the renderer reason (no CPU-only Wayland
  configuration), not the redraw one.
- *libcosmic `wayland` implies `iced_wgpu/wayland`.* Confirmed (`libcosmic/Cargo.toml:85-96`) —
  the comparator would need a feature audit of its own.

### 5.3 Stage A — the probe

A minimal qualifying client, built and measured (host; scratch under `/tmp/mura-shell`, nothing in
the repo): Slint `=1.18.0`, `default-features = false`, features `std compat-1-18
backend-winit-wayland renderer-software accessibility`, `slint-build` AOT, release profile
`opt-level = "s"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`.
The scene: a title label in nine scripts, a status label, `LineEdit` user, `LineEdit
input-type: password`, `LineEdit input-type: number`, a `ListView` of three sessions, a `Button`
— all `std-widgets`. Native build inputs: pkg-config, fontconfig, freetype, expat, wayland,
libxkbcommon. Run under the nested harness of research/70 (Monado simulated HMD + `zxr`, the
public socket; `ZXR_NO_LIBINPUT=1`), driven by `zxr ctl` (pointer/keys) and a scripted
`zwp_input_method_v2` client standing in for the OSK (zxr serves `text-input-v3` /
`input-method-v2` / `virtual-keyboard-v1` but no layer-shell, so wvkbd/squeekboard cannot map —
§6 finding a), on a private session bus with at-spi2-core's launcher and registry for the a11y
tree. 440 crates in the resolved graph.

### 5.4 Measured (host, 2026-09-27; medians of 3 runs where they differed, else the one value)

| gate | result |
|---|---|
| **binary** | 12 482 448 B stripped (12.48 MB); `INTERP` glibc; `NEEDED`: libfontconfig.so.1, libgcc_s, libm, libc |
| **runtime libraries mapped** | brotli, bzip2, expat, fontconfig, freetype, libgcc, glibc, libffi, libpng, libxkbcommon, wayland-client, zlib — i.e. fontconfig's closure plus the two dlopen'd Wayland libraries; nothing GPU |
| **start → plane mapped on zxr** | 30–31 ms (warm; binary cached) |
| **memory, steady, scene mapped** | RSS 20.5–20.8 MB, PSS 12.6–12.8 MB, private dirty 3.0 MB, VmHWM = RSS; after interaction RSS 24.0 MB, PSS 14.4 MB |
| **threads** | 6: main, winit `blocking-1`, smol `async-io`, `zbus::Connection`, two unnamed |
| **idle, no field focused** | run with a fresh compositor: main thread **62 wake-ups/s**, 1–2 CPU ticks/5 s, zxr +2 ticks/5 s; run without pointer motion reaching the client: **0 wake-ups, 0 CPU ticks in 10 s**, every thread quiescent. Attribution: the head-ray-owned pointer over the plane makes zxr deliver `wl_pointer.motion` each tick (§6 b); the toolkit itself is idle-clean |
| **idle, a field focused (caret)** | 0 CPU ticks/5 s in the probe; 10 client commits/5 s (2 Hz blink); zxr unchanged within noise |
| **focus by ray click** | the click on the plane focused the field under it (the PIN row at the plane centre); `Tab` cycles Entry → Entry → Entry → Button; `Enter` on the button fires the callback |
| **`content_type`** | user: `Normal`/hint 0; password: **`Password`**, hint `SensitiveData` (128); number: **`Normal`** (the mapping above) |
| **preedit** | `set_preedit_string("pré")` accepted, no state loss |
| **`commit_string`** | `abcé` inserted correctly (multi-byte OK) |
| **`delete_surrounding_text(1,0)`** | **ignored** — field `abcéZ`, expected `abcZ` |
| **seat keys after IM traffic** | `k` appended correctly; `input-type: number` rejected letters, accepted digits |
| **AT-SPI tree** | present once `org.a11y.Status.IsEnabled` and the registry are up: Frame "mura shell probe"; Labels; **Entry** `user name` (Text), **PasswordText** `password` (Text), Entry `pin` (Text); **Button** `unlock` (Action); ListBox (Selection) with three children. `EditableText` not exposed by the pinned `accesskit_unix` 0.22.1. `DoAction 0` on the button from the bus fired `unlock` |
| **scripts (offscreen, host fonts)** | Latin, Greek, Cyrillic, Arabic, Hebrew, Devanagari, CJK, Hangul, Thai, emoji: all rendered (ink present, distinct from the no-glyph reference which rendered nothing); Arabic/Devanagari shaping correctness not visually verified |
| **compositor** | zxr RSS 62.3 → 69.4 MB with the one client plane (+7 MB: the panel swapchain and textures for an 800×600 plane); 4 threads |

### 5.5 Determination on the toolkit

Slint 1.18.0 / software renderer **qualifies for the greeter program** under the review's gates,
with two recorded gaps and one dependency note:

- Gap 1 — `delete_surrounding_text` (winit). Does not affect squeekboard/wvkbd (§3.2); would affect
  an OSK that uses it. Track upstream (winit) or carry a patch; the greeter's own digit pad and
  password field need only `commit_string` and keys.
- Gap 2 — `EditableText` absent on text fields in `accesskit_unix` 0.22 (via `accesskit_winit`
  0.33); present in AccessKit 0.25. A screen reader reads the field (Text) and can focus/act; it
  cannot edit through AT-SPI until the bump. Track.
- Dependency — fontconfig + freetype (+ expat, brotli, bz2, png, zlib) and the dlopen'd
  libwayland-client / libxkbcommon: ≈ 12 shared objects, all already in the image for any
  Wayland/GTK program; the Nix package carries them as rpath. Fonts are part of the closure the
  design must name.

The comparator (libcosmic/tiny-skia + `a11y`) is **not built**: no hard gate failed (the review's
"choose, then validate; not two greeters"). Whether Mura's *other* shell components inherit the
toolkit is §7 Q3, not decided here.

## 6. Determinations (what falls out without an owner question)

- **D1 — the shell plane's shape is settled by convergence:** separate processes over layer-shell
  + the privileged seams, started as systemd user units under `graphical-session.target`, with a
  compositor that hides the privileged globals from restricted clients and spawns the two
  pre-login/exclusive ones (greeter/lock, OSK) over a socketpair. GNOME is the sole in-process
  comparable and its premise does not hold for Mura (ADR 0007 amendment, ADR 0012).
- **D2 — zxr's shell-layer half is the prerequisite for every component, not only the greeter:**
  `wlr-layer-shell` + `zxr-layer-anchoring-v1` serving (bands 2/4/5, exclusive angular bands,
  head/body/docked defaults) and the per-connection binding filter are not built (spec §10 lists
  them for M1; `pkgs/zxr/src/input/mode.rs:24-25`). No shipping OSK or panel can run on zxr until
  they are. This orders the implementation path: compositor shell-layer half → greeter program +
  restricted mode (G1) → OSK → the rest.
- **D3 — a compositor-side idle defect surfaced:** a head-ray-owned pointer resting on a client
  plane produces `wl_pointer.motion` at tick rate (62/s) with no user movement — the client wakes
  for each. budgets.md's shell-plane rule ("zero steady-state CPU wake-ups when idle") is the
  compositor's to honour first: suppress motion when the plane-local position is unchanged (the
  simulated head's pose noise, if any, is below a pixel). A zxr item for the shell-layer work.
- **D4 — the toolkit ruling is scoped:** Slint for the greeter program (§5.5); the shell-wide
  question and the carried components are §7.
- **D5 — squeekboard's Erase note is protocol evidence:** `delete_surrounding_text` "cannot work
  without get_surrounding_text" — both shipping OSKs avoid it; a Mura OSK should too, whatever its
  toolkit, which also neutralises Gap 1.
- **D6 — the pre-login a11y stack is session infrastructure:** the a11y bus (`org.a11y.Bus`) and
  `at-spi2-registryd` must run in the greeter's session for any AccessKit or GTK client to expose a
  tree; the probe exposed nothing until both were up. The greeter unit set includes them
  (spatial-a11y §1).

## 7. Owner questions (rule 8 form; one item each)

**Q1 — Start and supervision shape.** *Decided:* how Mura's shell components are started and
restarted. *Comparables:* GNOME, Plasma, phosh — systemd user units, `PartOf=graphical-session.target`,
`Restart=on-failure` (krunner `Restart=no`), `OnFailure=…shutdown.target` for the ones the session
cannot live without; COSMIC — its own `ProcessManager` (unlimited restarts, backoff), systemd
optional; wlroots — compositor `exec`/D-Bus activation, no supervision. *Options:* (a) systemd user
units under `mura-session.target` (the D4 shape, three of four desktops); (b) a Mura session
manager process spawning them (COSMIC). *Consequence:* (a) is declarative, already built for the
compositor, and gives the administrator `systemctl --user mask/edit` replaceability; (b) adds a
supervisor process and re-implements `Restart=`. My read: (a) — the evidence converges and it is
rule 1's answer.

**Q2 — The trusted set and its identity.** *Decided:* which globals are privileged and how a
client qualifies. *Comparables:* niri/Hyprland/cosmic-comp — the same privileged list (layer-shell,
session-lock, IM/VK, foreign-toplevel, workspace, data-control, screencopy) hidden from
security-context clients, everything else unrestricted; KWin/cosmic-panel — channel identity for
spawned children. *Options:* (a) research/30's rule as is: unrestricted unless under a
security-context, plus channel identity for the socketpair-spawned greeter/lock and OSK; (b)
additionally an allow-list of the session's own unit names for the privileged set. *Consequence:*
(a) is every comparable; (b) has no precedent and would break third-party panels the
administrator installs (rule 3). My read: (a).

**Q3 — One toolkit for Mura's own shell components, or per component?** *Decided:* whether the
panel, OSD, notifications UI and launcher Mura writes use the greeter's toolkit. *Comparables:*
COSMIC — one (libcosmic) for all; Plasma/GNOME — one (Qt/St); wlroots — per tool; phosh — GTK for
the shell, GTK+Rust for the OSK. *Options:* (a) Slint for every component Mura writes (COSMIC's
shape; one theme, one a11y path, one build); (b) per component as each lands. *Consequence:* (a)
one closure and skill set, but every component pays Slint's ~12 MB binary and ~13 MB PSS unless
components are co-located (cosmic-osd/notifications' shape); (b) freedom at the cost of N toolkits
in the image. My read: (a), with co-location where the comparables co-locate (OSD + polkit agent;
notification server + cards).

**Q4 — Notifications: carry or write.** *Decided:* whether the FDO notification server + cards is
mako (cairo + pango, D-Bus-activated, a `Type=dbus` unit, no spatial awareness) or a Mura
component on the shell toolkit that knows the head frame, immersion DND and the critical bypass
(research/36 §4). *Comparables:* every desktop writes its own (COSMIC, Plasma, GNOME, phosh); the
wlroots world carries mako/dunst. *Options:* (a) carry mako now, its placement fixed by zxr's
default head anchoring for unaware clients (`zxr-layer-anchoring-v1.xml:118-120`), replace later;
(b) write it. *Consequence:* (a) ships sooner, DND-by-immersion needs mako's `SetMode` driven by
zxr over D-Bus; (b) is the comparables' shape for an integrated shell. My read: (a) first, as the
seam is the standard one either way.

**Q5 — The OSK: carry squeekboard/wvkbd or write on the shell toolkit.** *Comparables:* phosh
carries squeekboard (GTK3+Rust, 104 layouts, `sm.puri.OSK0`); wlroots carries wvkbd; COSMIC and
Plasma write/carry their own (cosmic-osk, maliit). *Options:* (a) carry squeekboard (layouts,
`input-method-v2` + `virtual-keyboard-v1`, proven Erase path) on a body/hand frame via the
anchoring default or a small patch; (b) carry wvkbd (smaller, virtual-keyboard only, C + cairo);
(c) write a Slint OSK for the ray (target sizes, dwell, spatial layout). *Consequence:* (a)/(b) are
GTK3-or-cairo processes in a Slint shell and were designed for a touch phone, not a 1.5° ray; (c)
is the XR shells' choice (WayVR, WiVRn built theirs) and research/36 §7's pattern. My read: (a) to
reach G1–G3 (the greeter's keyboard path), (c) as the shell component when the toolkit ruling
(Q3) is in.

**Q6 — What the decoration chrome is in 3D.** Unchanged from the registry's "zero design";
research/60 §14 and ADR 0012 fix only that it is compositor-drawn hit volumes. *Options* are the
XR comparables' affordances (Flatland: close button, grab ball, resize handles; kwin-vr: grab
handle, radial menu; research/36 §2 placement). A design item for window-workspace-management's
manipulation UI, listed here so the shell-plane design does not claim it.

## 8. Sources

Pinned clones (`references/MANIFEST.json`): cosmic-session, cosmic-comp, cosmic-panel,
cosmic-applets, cosmic-launcher, cosmic-osd, cosmic-notifications, cosmic-greeter, libcosmic,
plasma-workspace, kwin, kscreenlocker, maliit-keyboard, sddm, gnome-shell, mutter, gdm, phosh,
squeekboard, waybar, fuzzel, mako, dunst, wvkbd, swaylock, hyprlock, greetd, gtkgreet, regreet,
tuigreet, wlroots, niri, hyprland, wayvr, stardustxr-server, flatland, kwin-vr, simula, wivrn,
monado, envision, slint (v1.18.0), accesskit. Crate sources read from the probe's registry:
winit 0.30.13, accesskit_unix 0.22.1, atspi-common 0.13.0. Repo: research/30, /36, /60, /12,
ADR 0007 (amended 2026-09-27), ADR 0012, specs/zxr-core.md, specs/session-bootstrap.md,
budgets.md, component-registry.md, protocols/zxr-layer-anchoring-v1.xml. [external]: the
owner-run toolkit review of 2026-09-27 (Slint/iced/egui/LVGL documentation and source review),
absorbed in §5.2 with each claim checked against the pins or the probe.
