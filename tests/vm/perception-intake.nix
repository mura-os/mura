# The perception→compositor intake protocol (specs/perception-intake.md) proven in the VM with
# neither real end: `intake-fake-producer` (a perception service with no camera) and
# `intake-test-consumer` (zxr's intake with no compositor) from pkgs/mura-perception-intake,
# over real kernel objects — udmabuf dmabufs (CPU-visible, so the pixel stamps can be checked),
# DRM syncobj timelines on the VM's virtio-gpu render node, a memfd register, SEQPACKET with
# SCM_RIGHTS. The §8 conformance checklist, one subtest each, plus registration, epoch
# supersession and the §7 framing rules. Check 4 (structural never-block) traces the consumer's
# pass window with strace and judges it against a deny-list of blocking syscalls — the
# discretionary choice recorded in the plan (external tracer; an in-thread seccomp filter is the
# reserved hook for zxr).
# (Nix indented string: never write two consecutive single quotes inside the script.)
{ pkgs }:
(import ./lib.nix { inherit pkgs; }) {
  name = "mura-vm-perception-intake";
  profileModules = [ ../../profiles/default.nix ];
  extraModules = [
    ({ pkgs, ... }: {
      # TEST-ONLY: the harness bins and the tracer on the PATH.
      environment.systemPackages = [ pkgs.mura.perceptionIntake pkgs.strace ];
    })
  ];

  testScript = ''
    import re

    machine.start()
    machine.wait_for_unit("multi-user.target")

    def start(cmd, log):
        # detach every fd: the driver waits for the command's stdout to close
        return machine.succeed(f"{cmd} </dev/null >/dev/null 2>{log} & echo $!").strip()

    def report(path, timeout=60):
        machine.wait_until_succeeds(f"test -e {path}", timeout=timeout)
        out = machine.succeed(f"cat {path}")
        return {k: v for k, v in (l.split("=", 1) for l in out.strip().splitlines())}

    def wait_gone(pid, timeout=60):
        machine.wait_until_fails(f"kill -0 {pid}", timeout=timeout)

    def run(name, consumer_args, producer_args, during=None, kill_producer_after=None):
        """consumer first (it listens), then the producer; returns both reports."""
        sock = f"/run/intake-{name}.sock"
        c_rep, p_rep = f"/tmp/{name}.c.rep", f"/tmp/{name}.p.rep"
        cpid = start(f"intake-test-consumer --socket {sock} --report {c_rep} {consumer_args}", f"/tmp/{name}.c.log")
        machine.wait_until_succeeds(f"test -S {sock}", timeout=20)
        ppid = start(f"intake-fake-producer --socket {sock} --report {p_rep} {producer_args}", f"/tmp/{name}.p.log")
        if during:
            during(cpid, ppid)
        if kill_producer_after is not None:
            machine.sleep(kill_producer_after)
            machine.succeed(f"kill -9 {ppid}")
        c = report(c_rep)
        p = report(p_rep) if kill_producer_after is None and machine.execute(f"test -e {p_rep}")[0] == 0 else {}
        wait_gone(cpid)
        print(f"[{name}] consumer: {c}")
        print(f"[{name}] producer: {p}")
        return c, p

    with subtest("intake: the kernel side is there — a DRM node with syncobj timelines, /dev/udmabuf"):
        probe = machine.succeed("intake-test-consumer --probe")
        assert "syncobj_timeline=1" in probe and "udmabuf=yes" in probe, probe
        print(probe)

    with subtest("intake: registration — 32 images over REGISTER + 6× REGISTER_MORE (16 fds a datagram), async ack, generations selected, stamps agree, GOODBYE retires everything"):
        c, p = run("basic", "", "--generations 100 --rate-hz 60")
        assert p["published"] == "100" and p["dropped"] == "0" and p["images"] == "32" and p["slots"] == "4", p
        assert c["epochs_seen"] == "1" and c["selections"] == "100" and c["stamp_mismatch"] == "0" and c["calibration_mismatch"] == "0", c
        assert c["overwritten_while_in_use"] == "0" and c["generation_notifications"] == "100" and c["goodbyes"] == "1", c
        # 32 dmabufs + our use page stay open for the epoch's life (+ the accepted socket); the
        # timeline fds are imported and closed
        assert int(c["fds_registered"]) - int(c["fds_baseline"]) == 32 + 1 + 1, c
        assert c["epochs_retired"] == "1" and int(c["fds_end"]) <= int(c["fds_baseline"]) + 1, c
        assert c["nonzero_timeout_waits"] == "0", c

    with subtest("intake §8.1: kill the producer mid-generation — composes without the layer from the next pass, no torn read, images unmapped only after the GPU uses complete, no fd leak"):
        c, _ = run("death", "--gpu-ms 50 --hold-release-ms 500", "--rate-hz 60", kill_producer_after=1)
        assert c["producer_gone"] == "1" and int(c["selections"]) > 20 and c["stamp_mismatch"] == "0", c
        assert int(c["layer_absent"]) > 0, c                              # the passes after death
        assert int(c["retire_delay_ms"]) >= 500, c                        # held until the fake GPU finished
        assert c["fds_at_death"] == c["fds_registered"], c                # nothing retired early
        assert int(c["fds_after_retire"]) <= int(c["fds_baseline"]) + 1, c   # then all of it
        assert c["epochs_retired"] == "1", c

    with subtest("intake §8.2a: stall the consumer 1 s — the producer keeps publishing into the slots the consumer never took (no drop, no queue), memory stays bounded to the pool, and on resume the consumer reads the latest generation"):
        rss = {}
        def stall(cpid, ppid):
            machine.sleep(1)
            rss["before"] = int(machine.succeed(f"awk '/VmRSS/ {{print $2}}' /proc/{ppid}/status").strip())
            machine.succeed(f"kill -STOP {cpid}")
            machine.sleep(1)
            rss["during"] = int(machine.succeed(f"awk '/VmRSS/ {{print $2}}' /proc/{ppid}/status").strip())
            machine.succeed(f"kill -CONT {cpid}")
        c, p = run("stall", "", "--generations 180 --rate-hz 60", during=stall)
        # a conforming consumer holds at most max_in_flight uses, so the pool (2 + max_in_flight)
        # always has a slot to write: the stall costs nothing but the skipped generations
        assert p["dropped"] == "0" and p["published"] == "180", p
        assert int(p["reclaimed"]) >= 170, p                             # the skipped ones were reclaimed
        assert rss["during"] - rss["before"] < 512, rss                  # bounded: the pool, not a queue (kB)
        assert 40 <= int(c["generations_skipped"]) <= 90, c              # ≈ one second of 60 Hz, jumped over
        assert c["stamp_mismatch"] == "0" and c["overwritten_while_in_use"] == "0", c
        assert int(c["max_pending_uses"]) <= 2, c

    with subtest("intake §8.2b: the overrun path — a consumer holding more uses than it declared exhausts the pool; the producer drops with OVERRUN counts, never blocks, never overwrites a held slot, and resumes when releases come"):
        c, p = run("overrun", "--exceed-in-flight 6 --gpu-ms 300", "--generations 200 --rate-hz 60")
        assert int(p["dropped"]) > 50 and p["overrun_reports"] == p["dropped"] and c["overrun_dropped_total"] == p["dropped"], (c, p)
        assert int(p["published"]) + int(p["dropped"]) == 200 and int(p["published"]) > 20, p
        assert c["overwritten_while_in_use"] == "0" and c["stamp_mismatch"] == "0", c
        assert int(c["max_pending_uses"]) >= 3, c                        # it really did exceed its declaration

    with subtest("intake §8.3: recalibration — no composed pass pairs pixels and pose across calibration_ver values within a group"):
        c, p = run("recal", "", "--generations 120 --rate-hz 60 --recalibrate-at 60")
        assert c["calibration_changes_seen"] == "1" and c["calibration_mismatch"] == "0" and c["stamp_mismatch"] == "0", c
        assert c["selections"] == "120", c

    with subtest("intake §8.4: structural never-block — acquire held unsignalled, producer writing continuously, the pass window traced: zero blocking syscalls, fallback to the current generation observed by count"):
        sock = "/run/intake-hold.sock"
        cpid = start(f"strace -f -tt -s 64 -o /tmp/hold.trace intake-test-consumer --socket {sock} --report /tmp/hold.c.rep --passes 300 --spin --pace-hz 90", "/tmp/hold.c.log")
        machine.wait_until_succeeds(f"test -S {sock}", timeout=20)
        ppid = start(f"intake-fake-producer --socket {sock} --rate-hz 200 --hold-acquire-from 200", "/tmp/hold.p.log")  # held from 1 s in; the traced consumer has a current by then
        c = report("/tmp/hold.c.rep", timeout=120)
        wait_gone(cpid)
        machine.execute(f"kill {ppid}")  # it may already have left: the consumer is gone
        print(f"[hold] consumer: {c}")
        assert c["passes"] == "300" and int(c["selections"]) >= 1 and int(c["selections"]) < 120, c
        # every pass is one of: a selection, a fallback, absent, or "latest is already current"
        assert int(c["fallback_reused_current"]) + int(c["selections"]) + int(c["layer_absent"]) <= 300, c
        assert int(c["fallback_reused_current"]) > 100, c                 # the count, not elapsed time
        assert c["nonzero_timeout_waits"] == "0", c
        trace = machine.succeed("cat /tmp/hold.trace")
        lines = trace.splitlines()
        begin = next(i for i, l in enumerate(lines) if "PASSES_BEGIN" in l)
        end_ = next(i for i, l in enumerate(lines) if "PASSES_END" in l)
        window = lines[begin + 1:end_]
        assert len(window) > 300, len(window)
        deny = re.compile(r"\b(nanosleep|clock_nanosleep|futex|poll|ppoll|epoll_wait|epoll_pwait2?|select|pselect6|wait4|waitid|flock|fsync|fdatasync|msync|sched_yield|read|readv|pread64|recvfrom|accept4?|connect)\(|SYNCOBJ_(TIMELINE_)?WAIT|0x64, 0xc3\b|0x64, 0xca\b|recvmsg\((?!.*MSG_DONTWAIT)")
        hits = [l for l in window if deny.search(l)]
        assert not hits, hits[:10]
        seen = sorted({m.group(1) for l in window for m in [re.search(r"^\d+ +[\d:.]+ (\w+)\(", l)] if m})
        print(f"[hold] syscalls in the pass window: {seen}")
        assert "ioctl" in seen and "recvmsg" in seen, seen               # the query and the non-blocking drain

    with subtest("intake §8.5: out-of-order release across two in-flight generations — reclamation follows per-image release points, no slot is rewritten while a use holds one image of it"):
        c, p = run("swap", "--release-order swap --gpu-ms 20", "--generations 100 --rate-hz 60")
        assert int(c["out_of_order_releases"]) > 5 and c["overwritten_while_in_use"] == "0" and c["stamp_mismatch"] == "0", c
        assert int(p["reclaimed"]) > 20 and c["max_pending_uses"] == "2", (c, p)

    with subtest("intake §6: producer restart with a higher epoch supersedes the old identity; the old epoch is retired under the death rule"):
        c, p = run("epoch", "", "--generations 100 --rate-hz 60 --restart-epoch-after 50")
        assert p["epochs"] == "2" and p["published"] == "150", p
        assert c["epochs_seen"] == "2" and c["epochs_retired"] == "2" and c["stamp_mismatch"] == "0", c
        assert int(c["fds_registered"]) - int(c["fds_baseline"]) == 2 * (32 + 1) + 1, c   # both tables + both use pages open during the overlap
        assert int(c["fds_end"]) <= int(c["fds_baseline"]) + 1, c

    with subtest("intake §7: an unknown type at our version is ignored; a future version is a registration failure"):
        c, _ = run("unk", "", "--generations 20 --rate-hz 60 --send-unknown-type")
        assert c["unknown_ignored"] == "1" and c["selections"] == "20", c
        c, p = run("vbump", "--passes 60", "--generations 20 --rate-hz 60 --version-bump")
        assert c["registration_failures"] == "1" and c["epochs_seen"] == "0" and c["selections"] == "0", c
        assert p["exit_code"] == "3", p

    with subtest("intake: the hand_top layer kind runs the same protocol (6 images a set)"):
        c, p = run("hand", "", "--generations 50 --rate-hz 60 --layer hand_top")
        assert p["layer"] == "hand_top" and p["images"] == "24" and c["selections"] == "50" and c["stamp_mismatch"] == "0", (c, p)
  '';
}
