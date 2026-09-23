# 38 — Desktop Linux security landscape and the Mura consolidation

**Status:** research and decision index, 2026-09-23.
**Scope:** how deployed Linux desktops compose security, where NixOS changes the mechanics, and
where Mura has fixed policy. This proposes no new framework; the index names each owner.

## Executive result

Desktop Linux security is a stack: Unix credentials separate users; logind tracks sessions, seats,
and device ACLs; PAM composes authentication; polkit authorizes named actions; D-Bus policy
constrains mainly the system bus; Wayland removes ambient display authority; portals broker
user-selected powers; sandboxes and systemd reduce process authority; MAC and encryption add
independent layers. None substitutes for another. [W01][W02][W03][W04][W05][W06]

GNOME and KDE are different presentations over this substrate. GNOME is portal-first, dropped a
general app tray, and removed its X11 session while retaining Xwayland; KDE integrates polkit
helpers through KAuth and ships its own portal backend. Both retain same-UID, shared-bus, Xwayland,
and accessibility-bus risks unless an app sandbox intervenes. [W17][W18][W19][W20][W21]

NixOS makes the stack declarative and reviewable rather than replacing it. Its immutable store and
signed substitutes protect deployment, while impermanence can discard undeclared state. It does
**not** supply comprehensive MAC by default, and a normal input-addressed closure hash is not proof
of output contents or reproducibility. [W22][W24][W25][W26][W27]

Mura is therefore an **appliance profile assembled from ordinary Linux parts**. Its XR
sensor, capture, lock, sharing, and per-unit-state controls are unusually concrete. Its real gaps
are the default app sandbox, MAC stance, secrets/keyring ownership, AT-SPI boundary, general
at-rest encryption, and device-specific verified boot. [L01][L15]

## 1. The desktop Linux composition

### 1.1 Unix credentials are the floor, not an app sandbox

Linux DAC compares process credentials with object ownership, mode bits, and optional POSIX ACLs;
capabilities split some historical root powers into per-thread privileges. This separates users,
but desktop apps normally share one UID, home, and session services. Service users meaningfully
isolate daemons; interactive apps need namespaces, filtered IPC, device denial, and grants—or
separate UIDs/domains—to be isolated from one another. [W01]

### 1.2 logind sessions and seats are lifecycle and device boundaries

`systemd-logind` binds a login session to at most one seat, tracks the active session, and manages
ACLs for seat devices; `pam_systemd` registers it in a scope. This is a DRM/input lifecycle
boundary, not a same-user sandbox: `user@UID.service` is shared across that user's sessions.
Mura correctly uses greetd/PAM for registration and logind for active-compositor device
access. [W02][L16]

### 1.3 PAM answers “did this conversation authenticate?”

PAM stacks `auth`, `account`, `password`, and `session` modules using controls such as `required`,
`requisite`, `sufficient`, `optional`, or exact bracketed rules. It does not define UI: the caller
renders generic conversation messages. Keeping PAM in `mura-authd`, away from the XR deadline,
and placing iris beside rather than instead of the credential is conventional separation adapted
to XR. [W03][L02][L03][L16]

### 1.4 polkit authorizes actions after login

Polkit is neither PAM nor a sandbox. Its system-bus authority checks a named action for a subject,
and a per-session agent may collect credentials. Ordered authorization rules are ECMA-262
edition-5 JavaScript intended for administrators and special-purpose OS environments. That fits
reviewed Mura appliance policy, but not a blanket “wheel may do everything” shortcut. KDE
KAuth demonstrates the right split: unprivileged UI requests an action; a small helper validates
the D-Bus caller through polkit and performs it. [W04][W19]

### 1.5 D-Bus policy protects the system bus better than the session bus

The system bus is normally default-deny for method calls and name ownership, with service policy
opening required paths; the same-user session bus is broadly permissive. Services must still
validate callers and arguments. Flatpak interposes `xdg-dbus-proxy`, but proxy and portal bugs have
caused real escapes; a 2026 filter bug leaked session/AT-SPI broadcasts until version 0.1.8.
AT-SPI is the hardest exception because it intentionally supports cross-app inspection, keystroke
listeners, and synthetic input; unrestricted access is unsafe on GNOME, KDE, and Mura alike.
[W05][W09][W16]

### 1.6 Wayland removes ambient authority and makes the compositor the TCB

Wayland makes the compositor the display server: it routes input from its scene graph and drives
KMS, while ordinary clients cannot inspect arbitrary windows or inject global input. The trade is
that the compositor becomes TCB for lock, capture, privileged globals, and injection. [W06][L11]

One normal Xwayland instance does not isolate its X11 clients from one another, though native
Wayland clients remain separate; per-app instances can restore separation. `security-context-v1`
attaches sandbox/app/instance identity to a connection, but the compositor must still authorize by
filtering globals, and must forbid dangerous nested contexts. [W07][W08][L11]

### 1.7 Portals are the app-facing grant layer

Portals expose stable APIs for selected files, URIs, capture, remote input, notifications, print,
and devices without ambient home/bus/device access. A document FUSE view and permission store bind
resources to apps. For capture, the portal owns request/consent, the compositor authorizes sources
and input, and PipeWire/libei transport the grant. Restore tokens restore a specific scope, not
ambient permission; XR scope language and active badging specialize this model. [W10][L06][L07]

### 1.8 Flatpak and Snap add app confinement in different ways

Flatpak uses bubblewrap namespaces, bind-mounted runtimes, no-new-privileges, seccomp, filtered
D-Bus, and portals. Snap strict confinement combines generated AppArmor, seccomp, mount namespaces,
device cgroups, capabilities, and interfaces; classic confinement is intentionally traditional.
Broad filesystem, device, X11, bus, or host-execution grants weaken either model, so the
distribution must choose a default app class and audit exceptions. [W09][W11]

### 1.9 systemd hardening is cheap, per-service containment

Systemd applies read-only filesystem views, private namespaces, device policy, capability bounds,
no-new-privileges, syscall filters, and kernel/control-group protections per service.
`systemd-analyze security` scores only these controls, so it is a review aid, not certification.
XR units need tailored closed device policy: Monado gets required DRM/input/camera/timing nodes,
while portal and map services generally do not. [W12]

### 1.10 MAC is distribution policy, not a generic Linux constant

Fedora ships SELinux enforcing; Ubuntu loads AppArmor; openSUSE Tumbleweed changed new installs to
SELinux enforcing in 2025 while Leap 15.x retains AppArmor. Many desktops have no comprehensive
app MAC. MAC can deny access that same-UID DAC allows, but needs maintained service, app, IPC,
device, and update policy: an enabled LSM is not a complete product policy. [W13]

### 1.11 At-rest encryption is another independent layer

LUKS/dm-crypt is the normal whole-filesystem choice. `fscrypt` supports per-directory/user keys and
encrypts contents and names but little metadata. Neither protects unlocked plaintext.
`LoadCredentialEncrypted=` instead decrypts authenticated TPM2/host-bound data into one unit's
credential directory: useful for map and service keys, but not a volume cipher or user keyring.
[W14][W15][W33]

## 2. How GNOME and KDE compose it in practice

### 2.1 GNOME

GNOME layers Mutter/GNOME Shell over logind, PAM/GDM, polkit, the user bus, PipeWire, and the GNOME
portal backend.
The portal frontend handles containment details while GNOME-specific backends and GNOME Shell
provide session UI.
This is portal-first integration, not a GNOME-specific permission API. [W10]

GNOME stopped showing application status icons by default in 3.26.
Its stated reasons include separating system from application status, preserving user control,
avoiding an incoherent dumping ground, and accessibility of tiny targets.
That is a UI choice, not additional process isolation. [W17]

GNOME 49 disabled its X11 session by default, and GNOME 50 removed GNOME's own X11-session support;
GDM can still launch another desktop's X11 session, and X11 applications continue through
Xwayland.
The security improvement is therefore “Wayland is the session,” not “all X11 compatibility risk
has disappeared.” [W18]

### 2.2 KDE Plasma

Plasma uses the same freedesktop foundations with KWin as compositor and
`xdg-desktop-portal-kde` as the desktop portal backend.
KDE documents portals as user-approved session-bus APIs and has shipped its backend with Plasma
since 5.10. [W20]

For privileged operations, KAuth keeps the normal application unprivileged and moves the operation
into a small helper, usually authorized through polkit.
That is a more explicit developer-facing helper pattern than GNOME commonly exposes, but the
underlying authorization substrate is shared. [W19]

KDE retains tray/SNI compatibility and more traditional extensibility.
That can improve compatibility but does not make a tray item trusted; the project must still avoid
making a third-party badge the only indication or control for capture, injection, or safety.
[L11]

### 2.3 Where both desktops punt

Both desktops accept that same-UID native applications outside a sandbox retain broad home and
session access.
Both rely on Flatpak/portals or equivalent packaging for real app confinement.
Both retain Xwayland for compatibility, normally with shared-X-server trust among X11 clients.
[W07][W09]

Both also depend on AT-SPI for accessibility.
Its cross-application semantics are inherently privileged, so a sandbox cannot safely receive the
whole bus.
Filtering implementations reduce exposure, but the 2026 proxy bug demonstrates that this remains
live security code rather than a solved protocol property. [W16]

## 3. What NixOS changes

### 3.1 Easier: one reviewable system policy

NixOS declaratively defines users/groups with `users.users` and `users.groups`, PAM service stacks
with `security.pam.services`, and polkit with `security.polkit`.
Desktop modules can enable the agents and portal backends they require.
Generation rollback also rolls back this policy as a unit. [W22]

Systemd unit hardening is expressed directly under
`systemd.services.<name>.serviceConfig`.
The module system makes a common baseline, device-specific exceptions, assertions, and tests easier
to review than hand-edited unit drop-ins.
The actual controls remain systemd/kernel controls. [W23]

### 3.2 Store integrity: strong, but describe it precisely

Nix store objects are immutable after creation and form closed reference graphs.
Build outputs become valid store objects only after a successful build and output processing, so a
failed build is not installed as a partially valid package.
Upgrades add new paths and switch profiles instead of overwriting a live package tree. [W24]

Binary-cache signatures cover the store path, NAR hash, NAR size, and references.
The client verifies the downloaded NAR hash, and an untrusted cache is rejected unless a trusted
signature or content-addressed trust rule applies.
This protects artifact transport and cache substitution; it does not prove source review or
reproducibility. [W25]

Ordinary Nix outputs are usually **input-addressed**:
the path hash commits to derivation inputs, not directly to output bytes.
`nix store make-content-addressed` can rewrite a closure into content-addressed form, but current
whole-system TPM measurement/attestation work remains planned rather than a stable default.
Therefore Mura invariant 7 is correctly stronger: independent rebuild byte comparison is the
release evidence.
Calling the normal closure path an “attestation” would overclaim. [W26][W27][L01]

### 3.3 Impermanence and secrets

The community impermanence pattern wipes or rolls back the root filesystem on boot and persists
only declared files/directories.
It converts unnoticed mutable state into an explicit allowlist and is particularly suitable for an
appliance, but it is opt-in and does not itself encrypt persisted state. [W28]

Plaintext secrets must not enter the world-readable Nix store.
`sops-nix` and `agenix` store encrypted material with the deployment and decrypt it into runtime
paths; systemd credentials can further scope delivery to a unit.
This is good mechanism, but Mura has not selected ownership for user keyring secrets,
service secrets, recovery material, and signing keys as one policy. [W29][W33]

### 3.4 The missing default: MAC

NixOS has an AppArmor module with declarative policies and enforce/complain/disable state, but no
comprehensive application profile set is enabled by default.
The official NixOS security documentation still characterizes integration as incomplete.
SELinux integration is substantially less mature because conventional labels and the immutable
store model do not compose cleanly. [W30][W31]

This is the largest difference from Android and Fedora.
NixOS makes bespoke hardening easy to encode once chosen; it does not make the choice or maintain
the policy automatically.

## 4. Mobile and appliance contrast

Android assigns each app a unique Linux UID and process, layers SELinux MAC over all processes in
enforcing mode, and adds explicit platform permissions and brokers.
Verified Boot extends a hardware root of trust through the bootloader and verified partitions;
dm-verity can verify filesystem blocks as read. [W34][W35]

AVB also defines rollback indexes in tamper-evident storage.
A signed but older vulnerable image is rejected when its rollback index is below the stored value;
on A/B devices, index advancement must preserve a known-good fallback until the new slot is marked
successful. [W36]

SteamOS takes a pragmatic desktop-appliance position:
the OS filesystem is read-only, updates arrive as whole images, and Valve recommends Flatpak for
additional applications.
Users can disable read-only mode, and non-Flatpak changes may be lost at the next update.
That is robust deployment policy, not Android-equivalent per-app or MAC policy. [W32]

Mura sits between these precedents.
Its desired product posture is closer to Android—a fixed-function device with protected sensors,
state, and boot chain—while its implementation ingredients are NixOS, systemd, Wayland, portals,
and Linux application compatibility.
The honest label today is “appliance profile under construction,” not “hardened Android equivalent.”

## 5. XR-specific threat surface

### 5.1 Sensor privacy

Room cameras reveal a home; eye images and gaze reveal biometric and attention data; face/body
signals can reconstruct identity and expression.
The existing plane design is strong: Monado-side services own frames, the compositor receives
finished layers or poses, and ordinary clients never receive room-camera frames, hand mattes, or
eye-camera images.
Gaze pose is exposed only through the OpenXR interface and remains subject to a still-open per-app
permission policy. [L04][L05][L03]

### 5.2 Spatial sharing and bystanders

An XR capture can expose more than a flat window:
observer-controlled views can look around an object, spectate follows the wearer's gaze, and
passthrough can reveal bystanders and the room.
Existing sharing invariants therefore attach consent to scope, exclude passthrough by default,
authorize each observer view, render capture before private composition, and keep active shares
badged in-space.
Recording/presentation to a room carries the same indicator duty. [L07]

Restore tokens are capabilities to restore a prior scoped choice, not consent to a broader source.
A workspace join, app volume, window, and spectated full scene need distinct language.
The local portal research grounds this in the standard ScreenCast lifecycle and permission store.
[L06][L07]

### 5.3 Delegation and input injection

A foreign compositor delegating a toplevel crosses privileged buffer, pacing, and input authority.
The accepted seam identifies objects by foreign-toplevel handles, filters the manager global,
allows the producer to deny or redact lock/protected/internal surfaces, and never trusts a foreign
activation token.
This is not governed by capture consent because the producer's binding policy is the authority.
[L08]

Remote input is an active attack surface even when capture is legitimate.
The EIS design keeps emulated devices distinct, maps them to the granted surface, visibly badges
them, and permits the compositor to pause/discard injection—for example while locked or while a
password prompt is focused.
No remote-control client should receive an undifferentiated global seat. [L06][L07]

### 5.4 Shoulder surfing and authentication

A headset hides the display from most observers but exposes controller motion, mirrored output,
spectator feeds, and the moment a user removes it.
The existing PIN pad reduces keyboard dependence, iris can provide a private parallel verifier,
and presence alone never unlocks.
The lock invariant must cover every presentation, including docked and shared outputs. [L02][L03]

### 5.5 Why microVMs are not the default

Spectrum demonstrates a strong niche:
one VM per untrusted app, an unprivileged jailed cross-domain backend, and only a host Wayland
connection crossing the boundary.
The local research also records the costs: guest copies without the mature virtgpu path, no guest
GPU in Spectrum's configuration, larger GPU attack surface if virgl/venus is enabled, and substantial
integration work for zero-copy color, depth, and explicit synchronization. [L12]

For Mura, default microVM-per-app isolation is rejected on resource-budget grounds.
A smartphone-class SoC is already sustaining camera perception, tracking, compositor work, and
72–120 Hz VR rendering; duplicate guest kernels, memory, buffer copies, and virtual GPU machinery
consume the same latency, thermal, and memory-bandwidth budget.
The default must be process/container-style confinement plus compositor/portal policy.

Spectrum-style microVMs remain the doc-19 niche for explicitly untrusted workloads whose risk
justifies the cost.
`waypipe --vsock` is the cheaper first experiment recorded there; a zero-copy proxied 3D client is
deferred behind missing dmabuf, drm-syncobj, and guest-GPU plumbing.
This is a scope constraint, not a claim that microVM isolation is weak. [L12]

## 6. The Mura consolidated security index

This is the reviewer-facing pointer; “owner” defines the decision and this doc only consolidates it.

| Decided control / status | Mechanism | Plane | Owning document |
|---|---|---|---|
| Lock invariant I1: locked frames sample no client color/depth, and no client receives input | Compositor lock state withdraws focus/seat and composes only the lock scene | Compositor/display | [ADR 0007 §Decision](../architecture/adr/0007-session-greeter-lock.md) |
| Lock invariant I2: externally report locked only after a zero-client-sample frame is submitted | Order `SetLockedHint`/suspend sequencing after `xrEndFrame` of the locked composition | Compositor/session | [ADR 0007 §Decision](../architecture/adr/0007-session-greeter-lock.md) |
| Lock invariant I3: only PAM or configured grace unlocks; crash returns locked | Boot/session supervision restarts into locked state; presence alone is insufficient | Session/auth | [ADR 0007 §Decision](../architecture/adr/0007-session-greeter-lock.md) |
| PAM cannot stall the XR frame loop | `mura-authd` owns libpam over a socketpair; generic PAM conversation; `pam_faillock` | Auth/system | [ADR 0007 §PAM out of process](../architecture/adr/0007-session-greeter-lock.md) |
| Biometrics sit beside PAM and never replace the credential | Iris/face is a parallel verifier within the sensor privacy boundary | Auth/perception | [ADR 0007](../architecture/adr/0007-session-greeter-lock.md), [ADR 0011 §4](../architecture/adr/0011-eye-tracking-ipd.md) |
| Persona/face data has three trust classes | Trusted local runtime owns asset; untrusted local apps get composition only; trusted remote runtime requires explicit Persona consent | Perception/app/sharing | [ADR 0010 §Consequences](../architecture/adr/0010-avatar-control-space-and-driver.md) |
| Room-camera frames and hand mattes never reach clients | Monado frameserver owns cameras; services publish finished dmabuf layers; clients declare policy only | Perception/runtime | [ADR 0008](../architecture/adr/0008-perception-services-placement.md), [perception design](../architecture/perception-passthrough-hands.md) |
| Eye images never leave the eye service; greeter has no eye frames | Session-scoped eye cameras; OpenXR exposes gaze pose, not pixels; cameras off pre-auth | Perception/auth | [ADR 0011 §4](../architecture/adr/0011-eye-tracking-ipd.md) |
| Eye pixels are prohibited; per-app gaze opt-in remains unresolved | `XR_EXT_eye_gaze_interaction` exposes pose only; permission policy is an ADR open question | App/perception | [ADR 0011 §Consequences](../architecture/adr/0011-eye-tracking-ipd.md) |
| Every privileged Wayland global is filtered per connection | Trusted shell/portal allowlist; ordinary and sandboxed clients excluded | Compositor/protocol | [ADR 0012 §5](../architecture/adr/0012-de-modularity-spinout-seams.md) |
| Security context supplies identity, not authorization | `security-context-v1` labels sandbox/app/instance; compositor policy decides globals | Compositor/protocol | [ADR 0012 §5](../architecture/adr/0012-de-modularity-spinout-seams.md) |
| Capture excludes private composition except explicitly authorized spectate | Capture tap precedes private composition; observer requests clipped/budgeted | Compositor/sharing | [spatial sharing §6](../architecture/spatial-sharing.md) |
| Capture consent is scope-specific and never silently inherited | Distinct language for window, app volume, spectate/gaze, and workspace join | Portal/sharing | [spatial sharing §6](../architecture/spatial-sharing.md), [research 17](17-sharing-capture-stack.md) |
| Passthrough is excluded from stills/streams unless explicitly included | Capture source taxonomy plus passthrough-redaction hook | Compositor/portal | [spatial sharing §2.2](../architecture/spatial-sharing.md) |
| Persistent capture restore remains a scoped capability | Frontend restore token maps to backend-owned, vendor-versioned restore data in permission store | Portal | [research 17 §1.3](17-sharing-capture-stack.md) |
| Active streams/shares are visibly badged; stills require shutter-moment consent | In-space system indicator for stream lifetime; interactive portal flow for a still | Shell/portal | [spatial sharing §2.2, §6](../architecture/spatial-sharing.md) |
| EIS input injection is distinct, badged, mapped, and pausable | Per-share libeis device/region joined by `mapping_id`; compositor discards or pauses at sensitive focus | Input/compositor | [research 17 §5](17-sharing-capture-stack.md), [spatial sharing §2](../architecture/spatial-sharing.md) |
| Foreign-session delegation is privileged and producer-redactable | Handle identity; connection-filtered manager; producer denies lock/protected/internal surfaces | Delegation/compositor | [foreign session §3.1](../architecture/foreign-session-integration.md) |
| Delegated activation and input do not import foreign authority | Producer owns focus/grabs and mints/validates activation tokens; DnD capability-gated | Delegation/input | [foreign session §3.5](../architecture/foreign-session-integration.md) |
| Spatial maps are sensitive, on-device, encrypted, and user-deletable | Descriptors-not-images target; per-map encryption; device/user wrapping with TPM2 + systemd credentials; deletion covers WAL/backups | Mapping/storage | [spatial mapping §6](../architecture/spatial-mapping.md), [ADR 0009](../architecture/adr/0009-spatial-mapping-architecture.md) |
| Per-unit calibration and identity are never copied and survive flashes | System state / protected partition classes; never `$HOME`; separate reviewed operation required to touch | Device/auth/perception | [overview invariant 4](../architecture/overview.md), [ADR 0007](../architecture/adr/0007-session-greeter-lock.md), [device contract](../architecture/device-contract.md) |
| Builds never write hardware | `nix build` is pure build; flashing is a separate human-confirmed stage with independent checks | Build/deployment | [overview invariant 1](../architecture/overview.md) |
| Every donor input is provenance-recorded and hash-pinned | Public `fetchurl`, non-redistributable `requireFile`, or explicit on-device extraction path | Build/supply chain | [overview invariant 2](../architecture/overview.md) |
| Donor/cache redistribution is strictest-input-wins | `publicRedistributable`, `privateSubstitutable`, `localOnly`; donor bytes never enter public cache | Build/release | [overview invariant 3](../architecture/overview.md) |
| Production signing keys never enter the Nix store | Store artifacts use test keys; release signing is external | Build/release | [overview invariant 5](../architecture/overview.md) |
| Reproducibility is evidence, not an assumption | Releases carry independent-rebuild byte comparisons | Build/release | [overview invariant 7](../architecture/overview.md) |
| AVB/verified-boot mode, chain, rollback, and key custody are not yet decided | Device mode enum and unsupported-mode fail-closed behavior are deferred to hardware spike | Boot/device | [design backlog #2](../architecture/design-backlog.md) |
| MicroVM-per-app is not the default; retained for selected untrusted workloads | Default process/container confinement; Spectrum/`waypipe --vsock` niche when risk pays SoC cost | App isolation | [research 19 §9.2](19-wayland-proxying.md); scope disposition recorded in this research |

## 7. Genuine gaps exposed by the index

These are questions, not decisions; each names the precedent that could answer it.

1. **What is the default confinement class for third-party applications?**
   Should every untrusted 2D app launch through a Flatpak-style bubblewrap profile with filtered
   D-Bus, `security-context-v1`, device denial, and portals, while explicitly trusted system apps
   use ordinary packaging?
   **Precedent:** Flatpak on GNOME/KDE and SteamOS's “read-only OS + Flatpak additions.”
   **Missing owner:** application packaging/launch policy.

2. **What is the MAC stance?**
   Is AppArmor required for the appliance profile, limited to high-risk daemons, or intentionally
   omitted in favor of namespaces and unit hardening?
   Which maintained profiles and denial tests make that claim real?
   **Precedent:** Ubuntu/Snap AppArmor; Fedora/Android SELinux shows the stronger product bar.
   **Missing owner:** base security profile and device kernel contract.

3. **Who owns secrets and keyrings?**
   Which data belongs in sops-nix/agenix, systemd encrypted credentials, a per-user Secret portal/
   keyring, TPM-sealed state, recovery media, or an offline release-signing process?
   **Precedent:** GNOME Secret portal/keyring selection, systemd credentials, and Android Keystore.
   **Missing owner:** system/user secret taxonomy and rotation/recovery policy.

4. **How is AT-SPI exposed across the app sandbox?**
   Which methods may ordinary apps call, which trusted accessibility tools receive control powers,
   and how is that identity bound to a sandbox connection?
   **Precedent:** Flatpak's method-filtered accessibility-bus proxy and its published escape history.
   **Missing owner:** accessibility architecture; research 37 had no accepted policy at review time.

5. **What is the general at-rest encryption profile?**
   Map encryption is decided, but owner home data, portal permissions, Persona assets, logs,
   calibration backups, and swap have no consolidated LUKS/fscrypt/key-lifecycle rule.
   **Precedent:** LUKS2 for device volumes plus fscrypt only where separate per-user keys are needed.
   **Missing owner:** storage/image architecture.

6. **Which privileged actions exist, and what are their exact polkit rules?**
   Update, flashing, protected-state maintenance, time/network changes, and developer-mode
   transitions need named actions and minimal helpers rather than shell-wide privilege.
   **Precedent:** KDE KAuth helper/polkit split and polkit's special-purpose-OS rule model.
   **Missing owner:** administrative action inventory.

7. **How does verified boot bind to Nix closure integrity?**
   Per-device AVB mode, vbmeta chain, rollback storage, A/B mark-success ordering, closure
   signatures/content verification, recovery, and key custody remain open.
   **Precedent:** Android AVB plus current Nix signed-substitute/content-addressed mechanisms;
   do not mislabel an input-addressed store path as output attestation.
   **Missing owner:** backlog #2 after the Lynx hardware spike.

8. **What is the Xwayland policy?**
   Is it absent in appliance images, opt-in per application, or shared for compatibility?
   If present, are high-risk apps assigned separate Xwayland instances?
   **Precedent:** GNOME's Wayland-only session with Xwayland compatibility and the upstream
   per-app-Xwayland isolation option.
   **Missing owner:** application compatibility profile.

9. **What audit evidence ships?**
   Unit exposure reports, portal grants, polkit decisions, sandbox denials, capture/injection
   lifecycle, and protected-state operations need privacy-bounded logging and release gates.
   **Precedent:** `systemd-analyze security`, MAC denial tests, Flatpak permission inspection, and
   Android CTS-style policy validation.
   **Missing owner:** qualification and incident-response policy.

## 8. Consistency check against desktop Linux

No accepted Mura decision contradicts the desktop-Linux composition.
PAM out of process follows greetd/screen-locker practice; polkit remains the right future action
authorizer; compositor-owned lock/capture/input policy is exactly where Wayland places authority;
portals remain the consent layer; systemd credentials and protected state fit NixOS deployment.

Several choices are deliberately nonstandard in presentation, not in security principle.
The built-in lock is compositor state rather than an `ext-session-lock-v1` client; camera and eye
privacy are stronger than ordinary desktop webcam mediation because clients receive derived planes
or poses, never frames; capture consent distinguishes observer viewpoints and passthrough; and
foreign-session delegation has a new privileged seam.
Each difference follows from XR geometry or timing and still preserves familiar authority splits.

The one wording correction reviewers should enforce is around Nix.
Nix store immutability, closure completeness, trusted cache signatures, content-addressed outputs,
reproducible builds, verified boot, and remote attestation are related but distinct properties.
Mura invariant 7 already states the defensible requirement—independent rebuild comparison—
and should not be weakened to “the closure hash attests the system.”

The one posture mismatch is incompleteness, not contradiction:
Mura wants Android-like appliance security, but its accepted documents do not yet select a
default app sandbox, enforcing MAC policy, broad storage encryption/key lifecycle, or AVB chain.
Until those owners exist, the compositor/sensor plane is better specified than the base OS plane.

## Sources

Local sources are the owners linked in §6 plus [L01 overview](../architecture/overview.md) and [L02 ADR 0007](../architecture/adr/0007-session-greeter-lock.md);
[L03 ADR 0011](../architecture/adr/0011-eye-tracking-ipd.md); [L04 ADR 0008](../architecture/adr/0008-perception-services-placement.md);
[L05 perception](../architecture/perception-passthrough-hands.md); [L06 capture/EIS](17-sharing-capture-stack.md);
[L07 sharing](../architecture/spatial-sharing.md); [L08 delegation](../architecture/foreign-session-integration.md);
[L11 ADR 0012](../architecture/adr/0012-de-modularity-spinout-seams.md); [L12 microVMs](19-wayland-proxying.md);
[L15 device/backlog](../architecture/device-contract.md); [L16 login](11-display-managers-greeters.md).

Web sources:

- [W01] Linux kernel, [Credentials in Linux](https://docs.kernel.org/security/credentials.html).
- [W02] systemd, [sd-login](https://www.freedesktop.org/software/systemd/man/latest/sd-login.html) and [pam_systemd](https://www.freedesktop.org/software/systemd/man/latest/pam_systemd.html).
- [W03] Linux-PAM, [pam.conf(5)](https://man7.org/linux/man-pages/man5/pam.conf.5.html).
- [W04] polkit, [polkit Reference Manual](https://freedesktop.org/software/polkit/docs/latest/polkit.8.html).
- [W05] D-Bus, [dbus-daemon policy manual](https://dbus.freedesktop.org/doc/dbus-daemon.1.html).
- [W06] Wayland, [architecture](https://wayland.freedesktop.org/docs/book/Architecture.html).
- [W07] Wayland, [Xwayland client isolation](https://wayland.freedesktop.org/docs/book/Xwayland.html).
- [W08] wayland-protocols, [security-context-v1](https://wayland.app/protocols/security-context-v1) and pinned [protocol XML](../../references/wayland-protocols/staging/security-context/security-context-v1.xml).
- [W09] Flatpak, [under the hood](https://github.com/flatpak/flatpak-docs/blob/master/docs/under-the-hood.rst) and [sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html).
- [W10] XDG Desktop Portal, [architecture/API](https://flatpak.github.io/xdg-desktop-portal/docs/index.html), [documents](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Documents.html), and pinned [source](../../references/xdg-desktop-portal/).
- [W11] Ubuntu, [Snap confinement mechanisms](https://documentation.ubuntu.com/security/security-features/privilege-restriction/snap-confinement/).
- [W12] systemd, [systemd.exec sandboxing](https://man7.org/linux/man-pages/man5/systemd.exec.5.html) and [systemd-analyze security](https://www.freedesktop.org/software/systemd/man/latest/systemd-analyze.html).
- [W13] Fedora, [SELinux configuration](https://fedoraproject.org/wiki/SELinux/Config); Ubuntu, [AppArmor](https://ubuntu.com/server/docs/how-to/security/apparmor/); openSUSE, [SELinux status](https://en.opensuse.org/Portal:SELinux).
- [W14] Linux kernel, [dm-crypt](https://docs.kernel.org/admin-guide/device-mapper/dm-crypt.html); [W15] [fscrypt](https://docs.kernel.org/filesystems/fscrypt.html).
- [W16] Flatpak, [AT-SPI sandbox issue](https://github.com/flatpak/flatpak/issues/79) and [2026 xdg-dbus-proxy advisory](https://www.openwall.com/lists/oss-security/2026/08/11/10).
- [W17] GNOME, [Status Icons and GNOME](https://blogs.gnome.org/aday/2017/08/31/status-icons-and-gnome/).
- [W18] GNOME, [X11 session removal](https://blogs.gnome.org/alatiera/2025/06/08/the-x11-session-removal/) and [GDM 50 release notes](https://download.gnome.org/sources/gdm/50/gdm-50.alpha.news).
- [W19] KDE, [KAuth overview](https://develop.kde.org/docs/features/kauth/) and [helper pattern](https://develop.kde.org/docs/features/kauth/using_kauth/).
- [W20] KDE, [Flatpak and portal integration](https://develop.kde.org/docs/packaging/flatpak/integration/).
- [W21] XDG Desktop Portal, [desktop backend composition](https://flatpak.github.io/xdg-desktop-portal/docs/for-desktop-developers.html).
- [W22] NixOS, [manual](https://nixos.org/manual/nixos/stable/), [PAM module](https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/security/pam.nix), and [polkit module](https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/security/polkit.nix).
- [W23] NixOS, [systemd services](https://github.com/NixOS/nixpkgs/blob/master/nixos/doc/manual/administration/service-mgmt.chapter.md).
- [W24] Nix, [store-object immutability](https://nix.dev/manual/nix/latest/store/store-object.html) and [build output registration](https://nix.dev/manual/nix/latest/store/building.html).
- [W25] Nix, [binary-cache store trust](https://nix.dev/manual/nix/latest/store/types/http-binary-cache-store) and [signed fingerprint fields](https://github.com/NixOS/nix/blob/master/src/libstore/include/nix/store/path-info.hh).
- [W26] Nix, [`nix store make-content-addressed`](https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-store-make-content-addressed).
- [W27] NixOS, [reproducible-build evidence](https://github.com/NixOS/reproducible.nixos.org/blob/main/index.html) and [end-to-end boot-security tracker](https://github.com/NixOS/nixpkgs/issues/549372).
- [W28] nix-community, [impermanence](https://github.com/nix-community/impermanence).
- [W29] [sops-nix](https://github.com/Mic92/sops-nix) and [agenix](https://github.com/ryantm/agenix).
- [W30] NixOS, [AppArmor module](https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/security/apparmor.nix); [W31] [security/MAC status](https://wiki.nixos.org/wiki/Security) and [SELinux discussion](https://github.com/NixOS/nix/pull/2670).
- [W32] Valve, [Steam Deck FAQ: immutable OS and Flatpak](https://partner.steamgames.com/doc/steamhardware/steamdeck/faq).
- [W33] systemd, [system and service credentials](https://systemd.io/CREDENTIALS/).
- [W34] Android, [application sandbox](https://source.android.com/docs/security/app-sandbox) and [SELinux](https://source.android.com/docs/security/features/selinux).
- [W35] Android, [Verified Boot](https://source.android.com/docs/security/features/verifiedboot/verified-boot).
- [W36] AOSP, [AVB rollback protection](https://android.googlesource.com/platform/external/avb/+/refs/heads/master/README.md).
