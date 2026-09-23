# First run and onboarding: machine provisioning, the OOBE, and factory reset

**Status:** accepted design (2026-09-23). Decision record: [ADR 0017](adr/0017-first-run-provisioning.md).
**What this covers:** everything between "the image was flashed" and "the greeter can
authenticate someone": silent machine provisioning (F1), the in-headset onboarding wizard (F2),
the persistent-state classes both depend on, and factory reset as their inverse. Slots into the
boot chain as the F-track ([implementation-path.md §2](implementation-path.md)).
**Grounding:** "XDG" below means the Base Directory spec (per-user preference/state split,
[specs/settings-schema.md §2](../../specs/settings-schema.md)). Precedents: the Steam Deck OOBE
(fixed `deck` user, wizard inside the auto-logged-in session — the account is *not* created at
OOBE), Quest (device-local passcode created at setup; forgotten passcode ends in factory reset,
[research/12 §6](../research/12-lock-screens-and-appliance-login.md)), and
`gnome-initial-setup` (the wizard as a dedicated pre-user session under the display manager).
**Budget impact** (overview invariant 9): F1 is one-shot boot-time work off the frame path; F2
runs on the greeter-mode budget (IMU-tier tracking, no client tier); neither adds a steady-state
tenant.

## 1. The account model in one paragraph

There is no runtime user creation. The `owner` account is **declared** in the module system
(`users.mutableUsers = false`) and exists in every image; "setting up a user" means writing
per-unit *state* — a PIN hash, calibration, preferences — never mutating the account database.
This is the Steam Deck model, and it is what makes A/B updates and factory reset trivially safe:
the mutable surface is exactly the state classes of §2, nothing else. Guest/multi-account is
deferred with the multi-user profile (ADR 0017). Secrets (PIN hashes, Wi-Fi credentials, device
keys) are **never Nix option values** — the store is world-readable; they exist only as
runtime state written by `spatial-provisiond` (§4.2) under protected persistent storage.

## 2. Persistent-state classes (normative)

`/var/lib/spatial` binds onto `/persist/spatial` (pulled in by `spatial-persist-setup.service`;
[families/uefi-rauc](../../families/uefi-rauc/default.nix) is the first implementation). The
subtrees are **classes with different lifecycles**, and every consumer and reset path must
treat them by class, never the tree as one blob:

| Class | Contents | A/B update | Factory reset |
|---|---|---|---|
| `factory/` | factory calibration (panel/optics/camera intrinsics, per-unit, flashed at manufacture or bring-up) | survives | **survives** (invariant 4) |
| `identity/` | device keys, attestation material | survives | survives; regenerated only by explicit re-provisioning |
| `enrollment/` | PIN hash, user credentials, user calibration (§5), the provisioning marker | survives | **wiped** |
| `state/` | update/migration bookkeeping, quarantine records | survives | reset per settings-schema policy |
| machine-id | `/etc/machine-id`, persisted here and committed **before D-Bus/logind start** | **survives** (one identity per unit, not per slot) | **rotated** — privacy; machine identity is not hardware identity |

Per-user preferences and remembered state stay in `$XDG_CONFIG_HOME` / `$XDG_STATE_HOME` on
`/home` (settings-schema §2); factory reset wipes `/home` wholesale.

## 3. F1 — silent machine provisioning

One-shot systemd units, no UI, no XR. Work: data-partition growth where the device needs it,
per-unit key generation into `identity/`, the `/persist/spatial` skeleton (the setup service's
job), settings-store seeding (empty stores + the generation tag), nix-db rehydration where the
family requires it.

**The durable marker is authoritative, `ConditionFirstBoot` is not.** Installing a fresh root
slot via an A/B update presents an empty `/etc/machine-id` and looks like first boot to
`ConditionFirstBoot`; provisioning must not re-run there. Rules:

- F1 units gate on **absence of the provisioning marker** (`enrollment/provisioned`, §4.2) or
  their own per-task markers under `state/` — all on `/persist`, so they see through slot
  replacement.
- `ConditionFirstBoot` is used only for genuinely *slot-local* concerns (nix-db rehydration
  class — work that must re-run per new rootfs).
- machine-id: bound from `/persist` before D-Bus/logind start (early-boot, the standard
  image-based pattern), so identity is stable across updates and rotates only on factory reset.

**Interrupted first boot is recovered by construction:** every F1 unit is idempotent and its
marker is written atomically (`rename(2)`) after the work completes; a power cut mid-F1 re-runs
the incomplete units on the next boot. No unit depends on a *partially* provisioned sibling —
dependencies are on markers, not on unit start order.

## 4. F2 — the onboarding session

### 4.1 Dispatch

greetd cannot select a session from runtime state by itself. Its `default_session` command is a
small root-owned **dispatcher wrapper**: if the provisioning marker is absent it execs
`zxr --oobe`, otherwise `zxr --greeter` (implementation-path B3). The marker is runtime state
consumed at dispatch time — never a NixOS option, which cannot change after evaluation. On the
appliance profile the dispatcher sits in `initial_session` the same way (§7).

### 4.2 The UI/authority split

`zxr --oobe` is an **unprivileged wizard UI** — a third restricted compositor mode beside
`--greeter`, on the same scene machinery, IMU-tier tracking, no client Wayland socket, rendered
with factory calibration and the safe default IPD (`spatial.xr.ipd.defaultMeters`). It can draw,
read input, and talk to exactly two privileged surfaces:

- **`spatial-provisiond`** — a narrowly scoped root service on a private socket (the
  spatial-authd shape: SOCK_SEQPACKET, JSON records, one conversation). It owns every
  provisioning write: PIN-hash creation (argon2, into `enrollment/`), device-key operations,
  and the **provisioning marker**, which is root-owned and committed transactionally (write
  sidecar → fsync → rename) as the *last* act of onboarding. The UI cannot mint, modify, or
  delete credentials or the marker; deleting the marker to reopen enrollment is a root
  operation by construction.
- **NetworkManager** (via its own D-Bus policy) for the Wi-Fi step.

OOBE writes **only runtime state**: locale and preferences go into the settings stores
(preferences with provenance, settings-schema §3), network profiles into NetworkManager's
state, enrollment into `enrollment/` via provisiond. It never rewrites generated `/etc` files
or NixOS configuration.

### 4.3 The wizard ladder

Each step is skippable-or-repeatable until the final commit; the ladder re-enters at the first
incomplete step if interrupted:

1. **Language/locale** → settings store (session + per-unit preference).
2. **Wi-Fi** → NetworkManager. Offline continue is allowed; updates and account-layering (out
   of scope here) simply wait.
3. **Owner display identity** — display name only; the Unix account is fixed (§1).
4. **PIN enrollment** — doc 12 option (b): `pam_spatial_pin` verifies an argon2 hash in
   `enrollment/`; provisiond writes it. The lock-screen PIN pad and this enrollment share the
   digit-pad scene component (session-auth §2.3's `style=secret` fast path).
5. **User calibration** (§5): IPD (measured via the ADR 0011 fixation-target wizard where eye
   tracking exists; slider/hardware readback otherwise), floor height, first boundary draw
   (consumed by [spatial-mapping §7](spatial-mapping.md)), controller pairing per the device
   contract.
6. **Privacy defaults** — presence sharing, capture policy, telemetry: written as
   settings-schema *preferences with explicit provenance*; consent is recorded, never silently
   defaulted.
7. **Commit** — provisiond writes the marker; the wizard exits; the dispatcher's next run
   selects the greeter (multi-user) or the session proceeds (appliance).

## 5. Factory calibration vs user calibration (two stages, normatively distinct)

- **Factory calibration** (`factory/`): panel/optics/camera intrinsics. A **B1a precondition** —
  Monado does not start without it, and the OOBE itself renders *with* it plus the default IPD.
  It predates first boot (manufacture/bring-up flashing) and survives everything short of
  re-manufacture.
- **User calibration** (`enrollment/`-class): IPD preference, floor height, boundary. Produced
  by F2 step 5, refined any time later from settings; wiped by factory reset.

The two never share a store, a lifecycle, or a validity check. B1b's preflight validates
*factory* calibration; a missing *user* calibration simply routes to F2 (or to in-session
calibration UI after the MVP).

## 6. Factory reset

The inverse of provisioning, per the §2 class table: wipe `enrollment/`, wipe `/home`, reset
`state/` per policy, **rotate machine-id**, preserve `factory/` and `identity/`. The marker
goes with `enrollment/`, so the next boot dispatches into F2. Reset is a recovery-environment
operation (not an in-session `rm`), and the forgotten-PIN terminal fallback is exactly this
path (doc 12's Quest precedent: after PAM's faillock ladder is exhausted, the only way forward
is reset — on an appliance the final fallback is recovery/wipe, not a root shell).

## 7. The MVP path (appliance profile)

Steam Deck model: `spatial.xr.session.autoLogin = "owner"`, no greeter, and F2 runs as the
*first session content* instead of pre-login — same wizard, same provisiond authority, same
marker; the dispatcher's decision simply happens inside the session start. This is the **first
shipped profile** and requires none of the multi-user machinery. It is *not* what the G-track
verifies first: G2 deliberately exercises the multi-user greeter chain in the rung-2 VM with
fixture-seeded enrollment, because that path holds the hard ordering problems — the two "firsts"
are different axes, reconciled in [implementation-path.md §1](implementation-path.md). The
multi-user greeter-gated variant (§4.1) is the general case. A companion-phone enrollment tool
is recorded as an open alternative (not designed; the provisiond socket is the natural seam for
it).

## 8. Conformance checks

1. A/B update across an existing installation: F1 does not re-run (marker seen through slot
   replacement); machine-id unchanged; `ConditionFirstBoot`-gated slot-local units do re-run.
2. Power cut mid-F1 and mid-F2: next boot resumes at the first incomplete step; no
   half-written credentials (provisiond transactionality).
3. The OOBE UI process holds no capability to write `enrollment/` directly (fs permissions +
   no privileged sockets beyond provisiond/NetworkManager).
4. Factory reset: `factory/` and `identity/` byte-identical before/after; machine-id rotated;
   next boot lands in F2.
5. Deleting the marker as the unprivileged user fails; as root, next dispatch re-enters F2
   without touching existing `identity/`.
6. Multi-user G2 fixture: pre-seeded `enrollment/` authenticates without F2 ever running.

## 9. Open items

Recovery-environment design (where factory reset executes — ties to the family's recovery
story); the companion-tool enrollment alternative; account-layering (store accounts, cloud
identity) — explicitly out of OS scope today; guest mode (with the multi-user profile);
whether boundary drawing at F2 step 5 can be deferred to first passthrough use on devices
without controllers.
