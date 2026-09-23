# spatial-os — read this before touching anything

## Why this project exists

Every shipping XR headset is a walled garden. Meta, Apple, Google, and the rest treat the
hardware as theirs and the person wearing it as a licensee — accounts capped, credentials
dictated, encryption withheld, software sources gated, root forbidden. **spatial-os exists
because of that.** It is a Linux PC in headset form: Free Software, libre, no walled gardens.
The wearer is the administrator. The OS never treats its user as stupid, and never removes a
power user's choice to protect them from themselves.

This is not a style preference. It is the project's reason for existing, and it is
[overview.md invariant 10](docs/architecture/overview.md). Design work that violates it is
wrong even when it is well-executed.

## Binding rules for every agent in this repo

1. **Standard Linux mechanisms are the default answer to every solved problem.** Accounts are
   passwd/shadow + PAM. Enumeration is NSS. Admin is wheel + polkit per-action escalation —
   no session ever carries ambient root, and no bespoke "owner" role exists above ordinary
   Unix. Config surfaces are XDG. If GNOME/KDE/systemd/freedesktop already solved it, that is
   the design; deviate only for a genuine XR-physical reason, stated in writing.
2. **Consumer XR platforms (Quest, visionOS, Horizon, PICO…) are anti-patterns by default.**
   They may be cited as *engineering evidence* — mechanisms, measurements, failure modes —
   never as *policy authorities*. "Quest does X" is never, by itself, a reason to do X. If a
   design imports a behavior from a closed platform, it must say so explicitly and justify it
   on engineering grounds alone.
3. **Never import caps, lockdowns, or permission-gating.** No account limits. No mandatory
   wizards standing between the user and their machine. No credential schemes that replace the
   Unix password (convenience methods stack *beside* it — the fprintd model). No feature the
   hardware supports that is withheld from its administrator. Encryption, accounts, root
   access, and software sources are the user's choices, not the vendor's — offer choices,
   never adjudicate them away.
4. **Flag discretionary judgments; never silently default them.** When a design decision is
   not forced by the user's instructions, the evidence, or a hard constraint, say so in the
   deliverable and surface it for the user's adjudication. Do not launder judgment calls
   through "the research recommends" or "the plan says." The user decides policy; agents
   propose and label.
5. **Design docs specify; they never schedule.** A design is a decision, a condition-shaped
   rule ("X exists only when Y does"), a non-goal with reserved hooks, or an open question
   naming its decider. Ordering and deferral live only in
   [implementation-path.md §5](docs/architecture/implementation-path.md)
   (the docs README states this rule).

## Working conventions (established, do not relitigate)

- Commit with **explicit paths only** — parallel workstreams share this tree; never sweep the
  index. Fresh `git status` before editing shared files.
- `nix flake check` green before every commit.
- Research claims cite file paths and line numbers from the pinned clones in `references/`
  (MANIFEST.json), or are marked [external] with sources.
- New ADRs and design docs carry a budget-impact statement (overview invariant 9) and ground
  their vocabulary per the docs README rules (which "XDG", XrSpace terms, no-deferral).
