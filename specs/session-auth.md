# specs/session-auth: the lock auth helper, lock events, and greeter mode

**Status:** rev 4 (2026-09-24, **helper hardening**: security review of the D5 helper absorbed —
nonce moved off argv into `MURA_AUTHD_NONCE`, process hardening on the kscreenlocker-worker
model (§2.1), strict response handling, a `mura-lock[-*]` service allowlist, a threat model
(§2.5), and §6 item 9 VM-verified). Rev 3 (D5 landed): `mura-authd` exists — `pkgs/mura-authd`,
Rust with a hand-written Linux-PAM FFI — and §6 items 1, 2, 3, 7 are VM-verified against the
sway stand-in by `mura-authd-harness`; items 4–6 wait for zxr. Rev 2 was the specification workstream's draft
(rev 1 findings from the PAM/greetd-persona review absorbed — greetd is the sole login PAM
authority, conversation nonces close the grace race, batched conversations added).
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
(`--fd N`) and a compositor-generated 64-bit **conversation nonce**, passed in the helper's
environment as `MURA_AUTHD_NONCE=HEX` (16 hex digits). The nonce is **never on argv**:
`/proc/<pid>/cmdline` is world-readable, `/proc/<pid>/environ` is not (it needs ptrace-read
access, which the hardening below denies to every same-uid process). The legacy `--nonce HEX`
argument is accepted with a stderr warning for one release so an older compositor build keeps
working, then removed. Each complete seqpacket record is one UTF-8 JSON object; there is no
in-band length prefix. Records above 64 KiB, truncated reads (`MSG_TRUNC`), zero-length records,
invalid UTF-8/JSON, or unknown `type` values terminate the conversation as `failure(internal)`.
Every message in both directions carries `"nonce"`; a message with a stale nonce is ignored
(§2.4) — and any response text it carried is zeroed before it is dropped.

**Response strictness (rev 4).** A `respond_batch` is handed to PAM only if it is well-formed:
every `index` is in range and unique, and no response contains U+0000 (C strings cannot carry
it). Any violation ends the conversation as `failure(internal)` after zeroing everything already
copied — never a silent skip, an overwrite of an earlier slot, or an empty answer standing in for
the one the wearer typed (an empty answer would otherwise be refused by
`PAM_DISALLOW_NULL_AUTHTOK` and mis-reported as `auth`).

**Process hardening (rev 4; kscreenlocker's PAM worker is the precedent —
`references/kscreenlocker/greeter/worker/prctls.h:30-44`, `main.cpp:365-376`).** Before any
secret is touched the helper: sets `PR_SET_DUMPABLE=0` (no core file; no same-uid ptrace attach or
`/proc/<pid>/{mem,environ,maps}` read under Yama), `RLIMIT_CORE=0`, `PR_SET_PDEATHSIG=SIGKILL`
with a `getppid()==1` check for the race (an orphaned helper exits at once — the compositor's
death ends every conversation, §2.4), `FD_CLOEXEC` on the conversation fd (pam_unix execs the
setuid `unix_chkpwd`; the socket must not follow it), and `mlockall(MCL_CURRENT|MCL_FUTURE)`.
**Which failures are fatal (rev 4.1, [research/56 §8](../docs/research/56-defaults-from-comparables.md)):**
the *lifecycle* prctl — `PDEATHSIG` — is fatal, as in kscreenlocker's worker (`main.cpp:365-368`,
"Failed to set death signal on parent, exiting") and systemd's fork helper; the *secrecy*
hardening — dumpable, core limit, `mlockall` — is best-effort (logged, continue), as in
kscreenlocker (`main.cpp:373-376`, "We'll continue but it is a bit unexpected") and systemd's
`(void) set_dumpable(...)`. The reasoning transfers: a helper that could outlive its compositor
holds a half-answered conversation, while a same-uid attacker already has the session and the
secrecy prctls only narrow what it can do. It **deliberately does not** set `PR_SET_NO_NEW_PRIVS`
or install a seccomp filter: `unix_chkpwd` is setuid and would break; sandboxing the helper
belongs to the compositor's spawn side (§7).

**Service allowlist (rev 4).** `--service` must be `mura-lock` or `mura-lock-*` (the latter
exists for test stacks such as `mura-lock-slow`/`mura-lock-batched`). Anything else exits 2
before `pam_start`. The caller is the same uid, so this is caller-bug containment, not a
security boundary: it stops a misconfigured compositor from driving, say, the `login` stack
(with `pam_faillock` counting against the wrong service and `pam_unix` prompting for password
changes) through a helper that only understands the lock conversation.

**Implementation notes (D5, from the VM):** the helper is spawned with `--fd N [--user NAME]
[--service NAME]` and `MURA_AUTHD_NONCE` in its environment (`--service` exists for the
test-only stacks; production is `mura-lock`). The fail-delay callback is installed with `pam_set_item(PAM_FAIL_DELAY, fn)`; the
helper never sleeps itself — `delay_ms` is the compositor's to enforce (Linux-PAM's default
fail delay of ~2 s was observed as `delay_ms: 1876` on a wrong password). `security.pam.services.mura-lock`
carries **no `nullok`** — `PAM_DISALLOW_NULL_AUTHTOK` makes it inert, and "no credential ⇒ no lock
engages" is T2's rule, not PAM's — and the faillock ladder; because authd runs *as the user*,
`pam_faillock` reaches the tally only through a traversable directory (`state/faillock` is
`0755`; the user's tally is `0660 user:root`), updates it, and cannot create it (root callers
do). This is Linux-PAM's own design for exactly this caller, not a Mura trade: *"Individual files
with the failure records are created as owned by the user. This allows pam_faillock.so module to
work correctly when it is called from a screensaver"* (`pam_faillock.8`, [external, linux-pam];
`pam_faillock.c` returns `PAM_SUCCESS` on `EACCES`/`ENOENT`), and it is what kscreenlocker,
swaylock and hyprlock — all running PAM as the user — inherit
([research/56 §7](../docs/research/56-defaults-from-comparables.md)). Responses handed to PAM are `calloc`/`strdup`'d as the
Linux-PAM contract requires (PAM frees them); the helper's own copies are zeroed.

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

### 2.5 Threat model (helper)

What the helper is and is not defending against, so that reviews argue about the right things.

**Runs as:** the session user, unprivileged, no capabilities, one process per conversation,
lifetime = one unlock attempt. It holds, briefly: the nonce, the wearer's typed responses, and
the PAM handle. It never holds the password hash (that is `unix_chkpwd`'s, behind setuid).

| Adversary | Position | Held off by | Not addressed here |
|---|---|---|---|
| Another process of the **same uid** (a compromised session app) | can read `/proc/<pid>/cmdline`, spawn helpers itself, send it records if it has the fd | nonce in environ not argv + non-dumpable (no environ/mem read, no ptrace); the socketpair is inherited, never a named path, so only the spawner holds the compositor end | such a process can run `mura-authd` itself and *authenticate the same user* with a guessed password — that is `pam_faillock`'s ladder (§6 item 8), not the helper's; same-uid processes are not a boundary Linux offers without a sandbox (§7) |
| A **revoked or racing conversation** (§2.4) | a stale nonce, a late `success` | the compositor's atomic revocation; the helper ignores stale-nonce `respond_batch` (and zeroes its text) | — |
| **Malformed or hostile records** from the compositor end | oversize, truncated, empty, bad JSON, unknown type, NUL, duplicate/out-of-range index | every case → `failure(internal)` with zeroing; nothing partial reaches PAM (§2.1, §6 item 9) | — |
| A **PAM module** in the `mura-lock` stack | runs in the helper's address space by construction | out-of-process from the compositor: a module that hangs, crashes, or leaks affects one helper, not the frame loop (§6 item 2); the fd it might inherit on exec is `CLOEXEC` | a malicious module is root-installed configuration, out of scope (the administrator is the wearer, overview inv. 10) |
| **Secrets at rest after exit** | core files, swap, freed heap | `RLIMIT_CORE=0` + non-dumpable; best-effort `mlockall`; explicit zeroing of every buffer the helper owns | copies inside PAM modules and `strdup`'d responses PAM frees itself — Linux-PAM's contract, shared by every locker (swaylock, kscreenlocker, GDM) |
| **The compositor process** itself | spawns the helper, owns the socketpair | not a threat to the helper: it *is* the caller. Its own duties are §2.4 revocation and the heap-dump half of §6 item 1 | — |

Explicitly **not** goals: defending against root; hiding *that* an unlock attempt is under way
(`pgrep mura-authd` is fine); rate limiting inside the helper (that is `pam_faillock`, and
`delay_ms` is the compositor's to enforce so the helper never sleeps on the wearer).

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

Status per item (D5, `tests/vm/default-image.nix` / `multi-user.nix`, `mura-authd-harness`):

1. authd killed mid-`prompt_batch` ⇒ `locked`, retry allowed with a fresh nonce; zeroization
   verified (no secrets in the compositor heap dump). **Verified (helper half):** SIGKILL during
   the prompt → the caller sees EOF and no terminal message (⇒ `failure(internal)`), a fresh
   conversation with a fresh nonce succeeds. The heap-dump half is the compositor's (zxr).
2. PAM module sleeping 60 s ⇒ compositor frame loop unaffected (reads are event-driven; §3.1
   trace shows no stalls). **Verified** with a 5 s test module (`pam_mura_test.so sleep=5`): the
   caller's loop ticked 25× at 200 ms while the helper sat inside PAM; success arrived after
   the sleep. Out-of-process PAM is the mechanism.
3. **Race test:** hold a valid `success` datagram, expire grace, then deliver it ⇒ ignored;
   session stays `locked` (T5 beats T8 by nonce invalidation). **Demonstrated** by the harness
   as the compositor's rule: the nonce is invalidated before the terminal message is read, and a
   `success` carrying it does not unlock. Also verified: a `respond_batch` with a stale nonce is
   ignored by the helper (it keeps waiting); `cancel` → `failure(abort)`, non-zero exit.
4. L1–L3: for one `seq`, assert strict trace ordering *and* independently verify the
   `client_free_frame_submitted` frame contains no client samples (composition introspection).
   **Needs zxr.**
5. Crash-restart: T10 both branches. **Partial:** the compositor restarts inside the same login
   session (D4, `RestartMode=direct`); *into locked* needs the lock scene (zxr, D5→G3).
6. Greeter mode: no Wayland listening socket (`ss`/`lsof`); camera nodes unopened; **no PAM
   symbols loaded** in the greeter process (greetd owns login PAM). **Needs zxr (G1/G2).**
7. Batched conversation: a module issuing two prompts + one info in one callback round-trips as
   one `prompt_batch`/`respond_batch` pair. **Verified** with `pam_mura_test.so batched`:
   one `prompt_batch` with `secret`, `visible`, `info`; one `respond_batch` with an empty slot
   for the info entry; success.
8. *(added D5)* A passwordless account is refused by the lock (`PAM_DISALLOW_NULL_AUTHTOK` →
   `failure(auth)`); a wrong password → `failure(auth)` with `delay_ms`; the faillock ladder
   counts unlock failures and refuses the right password while locked. **Verified.**
9. *(added rev 4)* Framing and response strictness (§2.1): a 64 KiB + 1 record, a record larger
   than the receive buffer (`MSG_TRUNC`), a zero-length record, invalid JSON, an unknown `type`,
   a response containing U+0000, and a duplicate prompt index each end the conversation as
   `failure(internal)` with a non-zero exit. The helper is not dumpable (a same-uid `cat
   /proc/<pid>/environ` is refused while root's succeeds), its `cmdline` carries no nonce, and
   the conversation fd is `O_CLOEXEC`. A `--service` outside `mura-lock[-*]` exits 2 before
   `pam_start`; the legacy `--nonce` argv path still authenticates (with a warning).
   **Verified** (`tests/vm/default-image.nix`, scenarios `oversize` … `dup-index`,
   `bad-service`, `argv-nonce`).

## 7. Open items

- The biometric helper's verifier-specific fields (with the iris design); `GraceState` timing
  properties (settings-schema keys).
- `radio` prompts: rendered by the shipped stacks or rejected via `unsupported_prompt`
  (deployment policy). `binary` prompts are rejected with `unsupported_prompt` today (rev 4:
  `PAM_CONV_ERR` from the helper); a biometric helper (ADR 0011) that needs them will re-open
  this with its own message fields.
- **Sandboxing the helper** (seccomp/Landlock/`PR_SET_NO_NEW_PRIVS`): belongs on the
  compositor's spawn side, not inside the helper, because `pam_unix` execs the setuid
  `unix_chkpwd` and no-new-privs would break it. Decider: the zxr lock scene (G3), which knows
  what the shipped `mura-lock` stack execs. Until then the helper's hardening is §2.1's prctl set.
- Removing the legacy `--nonce` argv path: one release after a compositor that sets
  `MURA_AUTHD_NONCE` ships.
