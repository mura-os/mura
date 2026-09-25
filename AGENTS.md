# Mura — read this before touching anything

## Why this project exists

Every shipping XR headset is a walled garden. Meta, Apple, Google, and the rest treat the
hardware as theirs and the person wearing it as a licensee — accounts capped, credentials
dictated, encryption withheld, software sources gated, root forbidden. **Mura exists
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
6. **Think embedded.** The target is a battery-powered SoC where CPU cycles, memory, power and
   latency are the budget, in everything the OS does. Every runtime, language, daemon and
   polling loop is judged on that budget first. Interpreter runtimes (Python, JavaScript,
   Perl…) are not banned; they are expensive, and the questions are always "is this normal in
   comparable projects?" and "is it the most efficient way on this class of device?" — so far
   the answer has always been no. POSIX shell for oneshots and thin supervisors is acceptable;
   a program that parses, serves, or holds state is Rust.
7. **Comparables before invention, with their reasoning.** Every default, threshold, mechanism
   and posture is derived from how comparable shipping projects solved the *same* problem for
   the *same kind of user and device*, from their source in `references/` (file:line) or marked
   [external] with the source named. A precedent is not evidence until the deliverable says
   what problem the source was solving, what it chose, **why** (its comments, history, docs),
   which assumptions it rested on (host ecosystem, user model, password posture, hardware),
   whether those reasons transfer to Mura, and what adopting it trades off. Prevalence or a
   matching number/API is not itself a decision. No comparable is a signal, not a license:
   say so and treat the design as a rethink candidate.
8. **Evidence gate.** Act without asking when the evidence is overwhelming and you are
   confident — one exact precedent, or converging ones, with reasons that transfer. Otherwise
   bring the decision to the owner, *after* rule 7 is exhausted, one item per question,
   stating: what is being decided, why it is a decision at all, the comparables and what each
   does and why, the options, and the consequence of each — enough to decide without
   re-reading the corpus. The options are the comparables' actual positions, never invented
   ones, and never an arbitrary binary where the real question is a framework one.

## Behaviours that have cost this project time (do not repeat)

Each of these happened. The rule it breaks is in brackets.

- Asserting a precedent that was not read ("pmOS ships X") — open the source first. [7]
- Stopping at the first comparable and reaching for a question to the owner, with options
  the agent invented — finish the research; the determination usually falls out of it. [7, 8]
- Deciding a contested default yourself and writing it into the plan or code. [4, 8]
- Asking a question whose answer precedent already gives. [8]
- Framing rules as absolutes ("never Python", "shell only for once-run glue") that shipped
  code already violates — rules here are procedural frameworks for judgment, not bans. [6, 7]
- Editing files before the owner has answered the question you asked them. [4]
- Reasoning from first principles about a user or device ("the passwordless wheel user…")
  when comparables exist — study them instead; good UX is what shipping projects converged
  on, not what an agent derives. [7]
- Laundering a judgment call as "the research recommends" or "the plan says". [4]

## Working conventions (established, do not relitigate)

- Commit with **explicit paths only** — parallel workstreams share this tree; never sweep the
  index. Fresh `git status` before editing shared files.
- `nix flake check` green before every commit.
- Research claims cite file paths and line numbers from the pinned clones in `references/`
  (MANIFEST.json), or are marked [external] with sources.
- New ADRs and design docs carry a budget-impact statement (overview invariant 9) and ground
  their vocabulary per the docs README rules (which "XDG", XrSpace terms, no-deferral).
