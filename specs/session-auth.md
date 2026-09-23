# specs/session-auth: the auth helper framing, lock events, and greeter mode

**Status:** draft normative spec (specification workstream, wave 3).
**Design source:** [ADR 0007](../docs/architecture/adr/0007-session-greeter-lock.md) — this
document transcribes its ratified semantics into interfaces; it decides nothing new. Where ADR
0007 is silent, items are marked *open* rather than invented.
**Grounding:** greetd's IPC is the prior art for the framing style (length-prefixed JSON,
`$GREETD_SOCK`); PAM message types are POSIX-PAM's four; no "XDG" sense applies beyond
`$XDG_RUNTIME_DIR` (basedir spec) for socket paths.
**Budget impact** ([overview.md](../docs/architecture/overview.md) inv. 9): all interfaces here
are auth-time/event-rate; nothing touches the frame path. Negligible.

## 1. `spatial-authd`: the out-of-process PAM conversation

One helper process per authentication conversation (the swaylock fork model), spawned by the
compositor (lock) or the greeter mode, speaking over an inherited **socketpair** (`SOCK_SEQPACKET`;
fd number passed via `--fd N`). The compositor never links libpam; a hung PAM module can never
stall `xrWaitFrame` (ADR 0007 §PAM).

### 1.1 Framing

Each message is one seqpacket datagram: a 4-byte native-endian length is NOT used (seqpacket
preserves boundaries); payload is UTF-8 JSON, one object per datagram, `type` field mandatory.
Maximum payload 64 KiB; larger is a protocol error (connection closed, conversation failed).

### 1.2 Messages, compositor → authd

| `type` | Fields | Semantics |
|---|---|---|
| `start` | `service` (e.g. `"spatial-lock"`, `"spatial-greeter"`), `user` (string; empty = PAM decides), `tty`/`seat` context strings | Begin the PAM conversation for the named NixOS-owned service (`security.pam.services.*`). Exactly one per conversation. |
| `respond` | `id` (int, echoes prompt id), `response` (string), `cancelled` (bool) | Answer to a prompt. `cancelled: true` aborts the conversation (maps to PAM_CONV_ERR). |

### 1.3 Messages, authd → compositor

| `type` | Fields | Semantics |
|---|---|---|
| `prompt` | `id` (int, monotonic per conversation), `style` = `secret` \| `visible` \| `info` \| `error`, `text` | The four POSIX PAM message styles, verbatim; the lock/greeter scene renders **generic** prompts (ADR 0007: PIN pad fast path keys off `style=secret` + service config, never off prompt text parsing). `info`/`error` require no response. |
| `success` | — | PAM conversation succeeded (account+session stacks included for the greeter service; auth-only for the lock service). authd exits 0 after sending. |
| `failure` | `reason` = `auth` \| `maxtries` \| `abort` \| `internal`, `delay_ms` (int, from pam_faillock) | Conversation failed. authd exits nonzero after sending. Compositor enforces `delay_ms` before allowing retry UI. |

### 1.4 Lifecycle rules

- One conversation per process; retry = new spawn (fresh PAM state, no reuse).
- authd death without `success`/`failure` ⇒ treated as `failure(internal)`; the lock stays locked
  (invariant I3).
- The compositor may kill authd on doff-timeout/cancel; that is `failure(abort)`.
- Biometric paths (iris, ADR 0011) are *parallel* PAM-adjacent verifiers: they emit the same
  `success`/`failure` datagram shape over their own socketpair and never replace the PAM path
  (ADR 0007). Their spec is deferred with the iris verifier design.

## 2. The lock state machine: externally visible surface

Internal states (ADR 0007 §lock model): `unlocked`, `locked`, `verifying`, plus the boot-locked
entry. Externally observable contract:

### 2.1 Ordering invariants (normative, from I1–I3)

- **L1 (= I1):** on entering `locked`, client input delivery ceases *before* the state is
  reported anywhere; no client buffer is sampled into any subsequent composed frame.
- **L2 (= I2):** logind `SetLockedHint(true)` and any suspend-sequencer signal are emitted only
  **after** the first client-free composition has been submitted via `xrEndFrame`.
- **L3 (= I3):** `unlocked` is entered only from `verifying` on authd `success`, or at boot when
  no credential is enrolled. Compositor/runtime crash ⇒ restart into `locked`.

### 2.2 Event surface (for the session bus / shell components; names normative)

`org.spatialos.Session1` (system-bus peer or user-bus — *open: bus placement*, tracked with the
settings daemon design):

- `LockedChanged(b locked)` — emitted per L2 ordering.
- `PresenceChanged(b present, t since_usec)` — from `XR_EXT_user_presence` in the compositor's
  OpenXR loop (doff/don).
- `GraceState(s state)` — `none` | `doff_grace` | `expired` (the ADR 0007 doff ladder; timings
  from settings, defaults doffGraceSeconds).
- Method `Lock()` — explicit lock request (shell, idle daemon); always honored.
- Method `Unlock()` — **does not exist.** Unlock happens only via the authd conversation (L3).

### 2.3 Idle ladder integration

The compositor serves `ext-idle-notify-v1` and honors `zwp_idle_inhibit_v1` (ADR 0007);
idle→lock policy is configuration (`spatial.xr.session.lock.triggers`), evaluated
compositor-side. Docked-mode branches (no auto-lock while docked-in-use) per ADR 0015.

## 3. `--greeter` restricted mode

The zxr binary in greeter mode (multi-user profile, ADR 0007 §profiles). The contract:

**Disabled** (hard, not configuration): the client Wayland listening socket (no `WAYLAND_DISPLAY`
export; no client ever connects); all privileged globals; capture/injection subsystems; the
places/persistence store (no user context exists); perception services beyond the IMU tier
(cameras remain off pre-auth — the ADR 0007 privacy property).

**Enabled**: the OpenXR loop on IMU-only tracking; per-unit calibration from system state
(`spatial.xr.calibration.paths`); the built-in auth scene (composed internally — greeter UI is
not a client); `$GREETD_SOCK` as a greetd client speaking greetd's own IPC
(`create_session` → `post_auth_message_response` → `start_session`), sessions enumerated from the
module system (`spatial.xr.shell` values), never `.desktop` scanning.

**Exit**: on greetd `start_session` acknowledgment, the greeter tears down its Monado session and
exits 0; greetd's exit-then-start sequencing guarantees no two-compositor DRM contention
(research/11 §handoff). Nonzero exit ⇒ greetd restarts it (its normal supervision).

**Docked variant** (ADR 0015): the auth scene additionally presents flat on the external
connector; same conversation, same framing.

## 4. Conformance checklist (for the implementation milestone)

1. authd killed mid-`secret` prompt ⇒ lock remains locked, retry allowed after spawn (no state
   leak).
2. PAM module sleeping 60 s ⇒ compositor frame loop unaffected (L1 path never blocks on the
   socketpair; reads are event-driven).
3. `SetLockedHint` ordering verified against a captured `xrEndFrame` trace (L2).
4. Crash-restart lands in `locked` with a credential enrolled; in `unlocked` without (L3, boot
   rule).
5. Greeter mode: `ss`/`lsof` shows no Wayland listening socket; camera device nodes unopened.

## 5. Open items

Bus placement for `org.spatialos.Session1` (with the settings daemon design); the biometric
verifier framing (with the iris design); whether `GraceState` timings surface as properties
(settings-schema dependent).
