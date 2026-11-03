"""Measure pager readiness, navigation, idle watch CPU, and peak process RSS.

Uses a drained 100x40 controlling PTY with 10x20-pixel cells. Excludes a real
terminal's image decoding/GPU costs. Input files and JSON output stay external.
"""
import argparse
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import statistics
import struct
import subprocess
import sys
import termios
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--input", type=Path, required=True)
parser.add_argument("--samples", type=int, default=3)
parser.add_argument("--warmups", type=int, default=1)
parser.add_argument("--watch-seconds", type=float, default=0)
parser.add_argument("--reloads", type=int, default=0, help="manual reloads after the first page")
parser.add_argument("--resize-cycles", type=int, default=0, help="reflow from 100 to 60 columns and back")
parser.add_argument("--timeout", type=float, default=60)
args = parser.parse_args()
if args.samples < 1 or args.warmups < 0 or not 0 <= args.watch_seconds < args.timeout:
    parser.error("positive samples and timeout required; idle duration must be smaller than timeout")
if args.reloads < 0 or args.resize_cycles < 0:
    parser.error("reload and resize counts must not be negative")
binary, source = args.binary.resolve(), args.input.resolve()
project = Path(__file__).resolve().parent.parent
if project in source.parents:
    parser.error("keep benchmark inputs outside the repository")
footer = re.compile(rb"\x1b\[40;1H\x1b\[0m(?:\x1b\[7m)?([^\x1b]*)\x1b\[0m")
env = dict(os.environ, TERM="xterm-ghostty", TERM_PROGRAM="ghostty")
for key in ("NO_COLOR", "TMUX", "STY", "ZELLIJ", "COLORFGBG"):
    env.pop(key, None)


def cpu_seconds(pid):
    text = subprocess.check_output(["ps", "-p", str(pid), "-o", "time="], text=True).strip()
    return sum(float(part) * 60 ** index for index, part in enumerate(reversed(text.split(":"))))


def descendants(pid):
    rows = [tuple(map(int, line.split())) for line in
            subprocess.check_output(["ps", "-axo", "pid=,ppid="], text=True).splitlines() if line.strip()]
    members = {pid}
    for _ in range(16):
        children = {child for child, parent in rows if parent in members}
        if children <= members:
            break
        members |= children
    return len(members) - 1


def run():
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 100, 1000, 800))

    def terminal():
        os.setsid()
        fcntl.ioctl(1, termios.TIOCSCTTY, 0)

    start = time.perf_counter()
    process = subprocess.Popen([str(binary), "--watch" if args.watch_seconds else "--pager", str(source)],
                               stdin=subprocess.DEVNULL, stdout=slave, stderr=subprocess.PIPE,
                               env=env, preexec_fn=terminal)
    os.close(slave)
    tail, diagnostics = bytearray(), bytearray()
    total = frames = 0
    first = ready = usage = None
    last_status = b""
    try:
        def pump():
            nonlocal tail, first, ready, total, frames, usage, last_status
            if time.perf_counter() - start > args.timeout:
                raise TimeoutError("pager timed out")
            for fd in select.select([master, process.stderr.fileno()], [], [], 0.01)[0]:
                try:
                    data = os.read(fd, 262144)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    data = b""
                if fd != master:
                    diagnostics.extend(data)
                    continue
                if data and first is None:
                    first = time.perf_counter() - start
                total += len(data)
                tail.extend(data)
                last = 0
                for match in footer.finditer(tail):
                    frames += 1
                    last_status = match[1]
                    if ready is None and re.match(rb"\d+-\d+/\d+", match[1]) and b" rendering" not in match[1]:
                        ready = time.perf_counter() - start
                    last = match.end()
                tail = tail[max(last, len(tail) - 65536):]
            if usage is None:
                pid, status, sample = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    usage = sample
                    if process.returncode:
                        raise RuntimeError(diagnostics.decode(errors="replace"))

        while ready is None:
            pump()
            if usage is not None:
                raise RuntimeError("pager exited before rendering")
        scroll = []
        for key in (b"j", b"G", b"g", b" ", b"b"):
            previous = frames
            sent = time.perf_counter()
            os.write(master, key)
            while frames == previous:
                pump()
            scroll.append(time.perf_counter() - sent)
        reloads, resizes, resident = [], [], []

        def refreshed(previous):
            while frames <= previous or b" rendering" in last_status:
                pump()
                if usage is not None:
                    raise RuntimeError("pager exited during refresh")
            completed = time.perf_counter()
            rss = subprocess.check_output(["ps", "-p", str(process.pid), "-o", "rss="], text=True)
            resident.append(int(rss.strip()) / 1024)
            return completed

        for _ in range(args.reloads):
            previous, sent = frames, time.perf_counter()
            os.write(master, b"r")
            reloads.append((refreshed(previous) - sent) * 1000)
        for _ in range(args.resize_cycles):
            for columns in (60, 100):
                previous, sent = frames, time.perf_counter()
                fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 40, columns, columns * 10, 800))
                resizes.append((refreshed(previous) - sent) * 1000)
        idle_cpu = idle_output = idle_helpers = None
        if args.watch_seconds:
            before_cpu, before_bytes = cpu_seconds(process.pid), total
            until = time.perf_counter() + args.watch_seconds
            while time.perf_counter() < until:
                pump()
            idle_cpu, idle_output = cpu_seconds(process.pid) - before_cpu, total - before_bytes
            idle_helpers = descendants(process.pid)
        sent = time.perf_counter()
        os.write(master, b"q")
        while usage is None:
            pump()
        return dict(first_output_ms=first * 1000, ready_ms=ready * 1000,
                    navigation_ms=[value * 1000 for value in scroll], quit_ms=(time.perf_counter() - sent) * 1000,
                    peak_rss_mib=usage.ru_maxrss / (1024 * 1024 if sys.platform == "darwin" else 1024),
                    user_cpu_s=usage.ru_utime, system_cpu_s=usage.ru_stime,
                    output_bytes=total, idle_cpu_s=idle_cpu, idle_output_bytes=idle_output,
                    idle_helper_processes=idle_helpers, reload_ms=reloads, resize_ms=resizes,
                    refresh_resident_mib=resident)
    finally:
        os.close(master)
        if process.returncode is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
        process.stderr.close()


for _ in range(args.warmups):
    run()
samples = [run() for _ in range(args.samples)]
print(json.dumps(dict(input=source.name, input_bytes=source.stat().st_size,
                      input_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                      binary_bytes=binary.stat().st_size, watch_seconds=args.watch_seconds,
                      reloads=args.reloads, resize_cycles=args.resize_cycles,
                      ready_median_ms=statistics.median(s["ready_ms"] for s in samples),
                      peak_rss_median_mib=statistics.median(s["peak_rss_mib"] for s in samples),
                      samples=samples), indent=2))
