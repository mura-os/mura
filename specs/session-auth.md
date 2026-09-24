# specs/session-auth: the lock auth helper, lock events, and greeter mode

**Status:** draft rev 2 (specification workstream; rev 1 findings from the PAM/greetd-persona
review absorbed — greetd is the sole login PAM authority, conversation nonces close the grace
race, batched conversations added).
**Design source:** [ADR 0007](../docs/architecture/adr/0007-session-greeter-lock.md); the lock
transition table (§3) maps every row to its ADR invariant.
**Grounding:** greetd IPC is the login-path authority and prior art; PAM message semantics follow
Linux-PAM (not only the four POSIX styles); `$XDG_RUNTIME_DIR` (basedir spec) for socket paths.
**Budget impact** (inv. 9): auth-time and event-rate only; nothing on the frame path.

## 1. Authority split (normative)

- **Login (multi-user profile): greetd's session worker is the only PAM authority.** The zxr
  `--greeter` mode is an unprivileged greetd client: it renders greetd `auth_message` prompts and
  relays responses over `$GREETD_SOCK` (`create_session` → `post_auth_message_response` →
  `start_session`). It never spawns `mura-authd`, never links PAM, and performs no account,
  credential, or session management — greetd owns the entire login lifecycle.
- **In-session lock: `mura-authd` is the lock's PAM helper.** One helper process per unlock
  conversation, for the `mura-lock` PAM service only (auth stack; no session management —
  the session already exists).

## 2. `mura-authd`: the lock conversation

### 2.1 Process and framing

Spawned by the compositor per conversation with an inherited `SOCK_SEQPACKET` socketpair
(`--fd N`) and a compositor-generated 64-bit **conversation nonce** (`--nonce HEX`). Each
complete seqpacket record is one UTF-8 JSON object; there is no in-band length prefix. Records
above 64 KiB, truncated reads (`MSG_TRUNC`), zero-length records, invalid UTF-8/JSON, or unknown
`type` values terminate the conversation as `failure(internal)`. Every message in both directions
carries `"nonce"`; a message with a stale nonce is ignored (§2.4).

### 2.2 The PAM call sequence (helper side)

`pam_start("mura-lock", user, conv, &h)` → `pam_authenticate` (with `PAM_DISALLOW_NULL_AUTHTOK`)
→ `pam_acct_mgmt` → `pam_end`. No `pam_setcred`, no `pam_open_session`. The helper installs a
fail-delay callback (`pam_set_item(PAM_FAIL_DELAY, …)`); `delay_ms` reported on failure is the
maximum delay requested through that callback during the conversation, measured by authd — never
attributed to a particular module. PAM return codes map to coarse reasons (§2.3) chosen to leak
no account-existence information.

### 2.3 Messages

authd → compositor:

| `type` | Fields | Semantics |
|---|---|---|
| `prompt_batch` | `nonce`, `conversation` (int), `prompts`: array of `{index, style, text?, data?}` | One PAM conversation callback, delivered whole. `style` ∈ `secret` \| `visible` \| `info` \| `error` \| `radio` \| `binary`. `text` for textual styles; `data` (base64) with `mime` for `binary`. The UI renders **generic** prompts (the digit-pad fast path keys off `style=secret` + the user's non-secret `numeric-credential` hint per multi-user.md §3, never prompt-text parsing; the hint lookup happens only after the uid-window check). `info`/`error` entries require empty response slots. If a style is unsupported by the deployment, authd answers PAM with `PAM_CONV_ERR` itself and reports `failure(unsupported_prompt)`. |
| `success` | `nonce` | Authentication + account checks passed. authd exits 0 after sending. |
| `failure` | `nonce`, `reason` ∈ `auth` \| `maxtries` \| `abort` \| `unsupported_prompt` \| `internal`, `delay_ms` | Conversation failed; authd exits nonzero. The compositor enforces `delay_ms` before offering retry UI. |

compositor → authd:

| `type` | Fields | Semantics |
|---|---|---|
| `respond_batch` | `nonce`, `conversation`, `responses`: array of `{index, response?, data?}` (one slot per prompt, empty for info/error) | Completes exactly one `prompt_batch`; authd then returns the response array to PAM. |
| `cancel` | `nonce` | Abort: authd answers PAM with `PAM_CONV_ERR`, reports `failure(abort)`, exits. |

### 2.4 Nonce revocation (the grace race, closed)

On doff-grace expiry, explicit cancel, or a newer conversation starting, the compositor
**atomically**: (1) marks the nonce invalid, (2) stops reading the socketpair, (3) sends
`cancel` and closes its end, (4) kills the helper after a short grace (SIGTERM→SIGKILL), and
(5) zeroizes any buffered prompt/response data. A `success` (or any message) bearing an
invalidated nonce is ignored — the session cannot unlock from a revoked conversation. Helper
death without a terminal message ⇒ `failure(internal)`. All outcomes leave the lock in `locked`
(fail closed, invariant I3).

## 3. The lock state machine

States: `unlocked`, `locked`, `verifying` (a live conversation), plus the doff-grace overlay.
Transition table (each row cites its ADR 0007 source):

| # | From | Event | To | Source |
|---|---|---|---|---|
| T1 | boot | credential enrolled | `locked` | boot-locked rule |
| T2 | boot | no credential enrolled | `unlocked` | boot rule |
| T3 | `unlocked` | doff | `unlocked` + panels blanked + grace timer | doff ladder |
| T4 | grace | don within grace | `unlocked` (resume) | doff ladder |
| T5 | grace | grace expiry | `locked` (+ §2.4 revocation of any conversation) | doff ladder |
| T6 | `unlocked` | idle-past-lock / explicit `Lock()` / suspend / lid analog | `locked` | lock triggers |
| T7 | `locked` | unlock UI engaged | `verifying` (spawn authd, new nonce) | PAM out of process |
| T8 | `verifying` | authd `success` (valid nonce) | `unlocked` | I3 |
| T9 | `verifying` | authd `failure`/revocation | `locked` (retry after `delay_ms`) | I3 fail-closed |
| T10 | any | compositor/runtime crash-restart | `locked` if credential enrolled else T2 | I3 |

Presence never unlocks (a head ≠ the owner); biometric verifiers (iris, ADR 0011) are parallel
helpers using the §2 message shape and nonce rules over their own socketpair, gated beside — not
replacing — PAM. Docked-mode branch: while docked-in-use, T3/T5 are policy-suppressed (ADR 0015).

### 3.1 Ordering invariants and instrumentation (L1–L3)

Each lock transition carries a monotonically increasing **lock sequence number** `seq`, and the
implementation must emit these ordered trace points on one monotonic clock:
`input_withdrawn(seq)` → `client_free_frame_submitted(seq)` (the first `xrEndFrame` whose
composition sampled zero client buffers) → `SetLockedHint(true)` / suspend-ready →
`LockedChanged(seq)`.

- **L1 (= I1):** `input_withdrawn(seq)` precedes any external report of `seq`.
- **L2 (= I2):** `SetLockedHint` and suspend-readiness for `seq` follow
  `client_free_frame_submitted(seq)`.
- **L3 (= I3):** `unlocked` is entered only via T2 or T8.

## 4. The session event surface

`org.mura.Session1` on the **session bus** (aligned with
[settings-schema.md](settings-schema.md); the bus does not exist in greeter mode — greeter-time
tooling has no Session1 to talk to, by design):

- `LockedChanged(u seq, b locked)` — per §3.1 ordering.
- `PresenceChanged(b present, t since_usec)`.
- `GraceState(s state)` — `none` | `doff_grace` | `expired`.
- Method `Lock()` — always honored in-session (T6). No `Unlock()` method exists (L3).

## 5. `--greeter` restricted mode

**Disabled** (hard): the client Wayland listening socket; all privileged globals;
capture/injection; the places store; perception beyond the IMU tier (cameras off pre-auth).
**Enabled**: the OpenXR loop on IMU-only tracking; per-unit calibration from system state; the
built-in auth scene (internal, not a client); the greetd client conversation of §1, with sessions
enumerated from the module system (`mura.xr.shell` values); and — on the multi-user profile
only (amendment per [ADR 0018](../docs/architecture/adr/0018-multi-user-accounts.md) decision 9)
— exactly **one** `mura-provisiond` conversation, *create-guest*: gated server-side on the
root-owned owner-grant flag, answered with a single-use token consumed by the guest PAM gate
([multi-user.md §4](../docs/architecture/multi-user.md)). No other provisiond conversation is
reachable from greeter mode. **Exit**: on `start_session`
acknowledgment, tear down the Monado session and exit 0 (greetd's exit-then-start sequencing owns
the DRM handoff). **Docked** (ADR 0015): the auth scene additionally presents flat on the
external connector; identical conversation.

## 6. Conformance checklist

1. authd killed mid-`prompt_batch` ⇒ `locked`, retry allowed with a fresh nonce; zeroization
   verified (no secrets in the compositor heap dump).
2. PAM module sleeping 60 s ⇒ compositor frame loop unaffected (reads are event-driven; §3.1
   trace shows no stalls).
3. **Race test:** hold a valid `success` datagram, expire grace, then deliver it ⇒ ignored;
   session stays `locked` (T5 beats T8 by nonce invalidation).
4. L1–L3: for one `seq`, assert strict trace ordering *and* independently verify the
   `client_free_frame_submitted` frame contains no client samples (composition introspection).
5. Crash-restart: T10 both branches.
6. Greeter mode: no Wayland listening socket (`ss`/`lsof`); camera nodes unopened; **no PAM
   symbols loaded** in the greeter process (greetd owns login PAM).
7. Batched conversation: a module issuing two prompts + one info in one callback round-trips as
   one `prompt_batch`/`respond_batch` pair.

## 7. Open items

The biometric helper's verifier-specific fields (with the iris design); `GraceState` timing
properties (settings-schema keys); whether `radio`/`binary` styles are enabled in the shipped
PAM stacks or rejected via `unsupported_prompt` (deployment policy).
