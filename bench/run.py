"""Benchmark fresh CLI processes against external Markdown corpora on Unix.

Terminal cases use a 100x40 pseudo-terminal (14x40 for terminal-narrow), with
10x20-pixel cells, drained without screen painting. Times include startup,
parsing, layout, encoding, and PTY I/O,
but exclude a real emulator's image decoding, GPU work, and scrollback storage.
Defaults: one warm-up, three measured processes per case, warm filesystem caches.
"""

from pathlib import Path
import argparse
import errno
import fcntl
import fnmatch
import hashlib
import json
import os
import platform
import pty
import selectors
import signal
import statistics
import struct
import subprocess
import sys
import termios
import time

RENDERERS = ["mdviewer/terminal-tex", "glow/terminal",
             "mdviewer/terminal-math", "mdviewer/plain-pipe", "mdviewer/terminal-narrow",
             "mdviewer/terminal-images", "mdviewer/terminal-mermaid"]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--work-dir", type=Path, required=True)
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--samples", type=int, default=3)
parser.add_argument("--warmups", type=int, default=1)
parser.add_argument("--timeout", type=float, default=45)
parser.add_argument("--corpus", action="append", help="Filename glob; repeat to select multiple inputs")
parser.add_argument("--renderer", action="append", choices=RENDERERS)
parser.add_argument("--hardware", default="unspecified", help="Generic CPU/RAM description; no hostname")
parser.add_argument("--profile", default="unspecified", help="Build command or profile used for the binary")
parser.add_argument("--binary-revision", help="Source revision actually used to build the binary")
parser.add_argument("--no-highlight", action="store_true", help="Measure mdviewer with syntax highlighting disabled")
parser.add_argument("--image-mode", choices=["auto", "off", "legacy"], default="auto",
                    help="Use legacy for binaries predating --images; off measures the disabled path")
parser.add_argument("--mermaid-mode", choices=["auto", "off", "legacy"], default="auto",
                    help="Use legacy for binaries predating --mermaid")
parser.add_argument("--process-tree", action="store_true",
                    help="Sample descendant RSS with ps; measure timing separately without this overhead")
args = parser.parse_args()
PROJECT = Path(__file__).resolve().parent.parent
ROOT = args.work_dir.expanduser().resolve()
BINARY = args.binary.expanduser().resolve()
if ROOT == PROJECT or PROJECT in ROOT.parents:
    parser.error("--work-dir must be outside the repository")
if args.samples < 1 or args.warmups < 0 or args.timeout <= 0:
    parser.error("samples and timeout must be positive; warmups cannot be negative")
if not BINARY.is_file() or not os.access(BINARY, os.X_OK):
    parser.error("--binary must name an executable")
CORPORA = json.loads((ROOT / "corpora.json").read_text(encoding="utf-8"))
MARKER = b"\x1b_Ga=T,"
ENV = dict(os.environ, TERM="xterm-ghostty", TERM_PROGRAM="ghostty", COLORTERM="truecolor")
for key in ("NO_COLOR", "TMUX", "STY", "ZELLIJ", "GLOW_STYLE", "GLOW_WIDTH"):
    ENV.pop(key, None)


def command(renderer, name):
    file = str(ROOT / "corpora" / name)
    if renderer == "glow/terminal":
        return ["glow", "--config", str(ROOT / "glow.yml"), "-s", "dark", "-w", "84", file]
    flags = {
        "mdviewer/terminal-tex": ["--math", "tex"],
        "mdviewer/terminal-narrow": ["--math", "tex"],
        "mdviewer/terminal-math": ["--math", "kitty"],
        "mdviewer/terminal-images": ["--math", "tex"],
        "mdviewer/terminal-mermaid": ["--math", "tex"],
        "mdviewer/plain-pipe": ["--output", "text"],
    }[renderer]
    image_flags = [] if args.image_mode == "legacy" else ["--images",
        "kitty" if args.image_mode == "auto" and renderer == "mdviewer/terminal-images" else args.image_mode]
    mermaid_flags = [] if args.mermaid_mode == "legacy" else ["--mermaid",
        "kitty" if args.mermaid_mode == "auto" and renderer == "mdviewer/terminal-mermaid" else args.mermaid_mode]
    return [str(BINARY), *flags, *image_flags, *mermaid_flags, *(["--no-highlight"] if args.no_highlight else []), file]


def process_tree(pid):
    # Only numeric process relationships and RSS, never command lines or env.
    data = subprocess.check_output(["ps", "-axo", "pid=,ppid=,pgid=,rss="], text=True)
    rows = [tuple(map(int, line.split())) for line in data.splitlines() if line.strip()]
    members = {pid}
    for _ in range(16):
        found = {child for child, parent, group, _ in rows if parent in members or group == pid}
        if found <= members:
            break
        members |= found
    live = [rss for child, _, _, rss in rows if child in members]
    return sum(live) / 1024, len(live)


def kill_group(process):
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def run(renderer, name):
    tty = renderer != "mdviewer/plain-pipe"
    master = slave = process = None
    selector = selectors.DefaultSelector()
    try:
        if tty:
            master, slave = pty.openpty()
            columns = 14 if renderer == "mdviewer/terminal-narrow" else 100
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, columns, columns * 10, 800))
        start = time.perf_counter()
        process = subprocess.Popen(
            command(renderer, name), stdin=subprocess.DEVNULL,
            stdout=slave if tty else subprocess.PIPE, stderr=subprocess.PIPE,
            env=ENV, start_new_session=True,
        )
        if tty:
            os.close(slave)
            slave = None
        output_fd = master if tty else process.stdout.fileno()
        for fd, kind in [(output_fd, "output"), (process.stderr.fileno(), "error")]:
            os.set_blocking(fd, False)
            selector.register(fd, selectors.EVENT_READ, kind)
        size = images = errors = 0
        first_output = None
        tail = b""
        diagnostics = bytearray()
        timed_out = False
        usage = None
        tree_peak = processes_peak = 0
        next_tree_sample = 0
        while selector.get_map() or process.returncode is None:
            elapsed = time.perf_counter() - start
            if args.process_tree and elapsed >= next_tree_sample and process.returncode is None:
                rss, count = process_tree(process.pid)
                tree_peak, processes_peak = max(tree_peak, rss), max(processes_peak, count)
                next_tree_sample = time.perf_counter() - start + 0.02
            if elapsed > args.timeout and not timed_out:
                kill_group(process)
                timed_out = True
            if timed_out and elapsed > args.timeout + 1:
                break
            for key, _ in selector.select(0.01):
                try:
                    data = os.read(key.fd, 262144)
                except OSError as error:
                    if error.errno == errno.EIO:
                        data = b""
                    elif error.errno == errno.EAGAIN:
                        continue
                    else:
                        raise
                if not data:
                    selector.unregister(key.fd)
                    continue
                if key.data == "output":
                    if first_output is None:
                        first_output = time.perf_counter() - start
                    size += len(data)
                    combined = tail + data
                    images += combined.count(MARKER)
                    tail = combined[-(len(MARKER) - 1):]
                else:
                    errors += len(data)
                    diagnostics.extend(data[:max(0, 65536 - len(diagnostics))])
            if process.returncode is None:
                pid, status, candidate = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    usage = candidate
        if process.returncode is None:
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
        # macOS reports bytes; other supported Unix hosts report KiB.
        rss_unit = 1024**2 if sys.platform == "darwin" else 1024
        return {
            "wall_seconds": time.perf_counter() - start,
            "first_output_seconds": first_output,
            "cpu_seconds": usage.ru_utime + usage.ru_stime,
            "peak_rss_mib": usage.ru_maxrss / rss_unit,
            "sampled_tree_peak_rss_mib": max(tree_peak, usage.ru_maxrss / rss_unit) if args.process_tree else None,
            "sampled_peak_processes": processes_peak if args.process_tree else None,
            "output_bytes": size,
            "images": images,
            "stderr_bytes": errors,
            "diagnostics": diagnostics.decode(errors="replace"),
            "exit_code": process.returncode,
            "timed_out": timed_out,
        }
    finally:
        if process is not None:
            kill_group(process)
            if process.returncode is None:
                _, status, _ = os.wait4(process.pid, 0)
                process.returncode = os.waitstatus_to_exitcode(status)
            for pipe in (process.stdout, process.stderr):
                if pipe is not None:
                    pipe.close()
        selector.close()
        for fd in (master, slave):
            if fd is not None:
                os.close(fd)


def read_command(command, cwd=None):
    try:
        return subprocess.check_output(command, cwd=cwd, stderr=subprocess.DEVNULL,
                                       text=True, timeout=5).strip()
    except (OSError, subprocess.SubprocessError):
        return None


def checkout(path):
    revision = read_command(["git", "rev-parse", "HEAD"], path)
    status = read_command(["git", "status", "--porcelain"], path)
    return {"revision": revision, "dirty": None if status is None else bool(status)}


cases = []
for name in ["commonmark.md", "mixed-256k.md", "mixed-1m.md", "mixed-5m.md", "mixed-10m.md", "long-lines-1m.md"]:
    cases.extend((renderer, name) for renderer in RENDERERS[:2])
for name in ["math-100.md", "math-1000.md", "math-5000.md", "math-repeated-1000.md"]:
    cases.append(("mdviewer/terminal-math", name))
for name in ["mixed-1m.md", "mixed-10m.md", "math-5000.md"]:
    cases.append(("mdviewer/plain-pipe", name))
cases.append(("mdviewer/terminal-tex", "math-5000.md"))
for name in ["prose-1m.md", "prose-10m.md", "code-languages.md", "code-multiline.md",
             "code-over-limit.md", "code-captures.md"]:
    if name in CORPORA:
        cases.append(("mdviewer/terminal-tex", name))
for name in ["tables-1m.md", "tables-10m.md", "tables-wide.md", "tables-over-limit.md", "tables-math-100.md"]:
    if name in CORPORA:
        cases.append(("mdviewer/terminal-tex", name))
        if name in ("tables-1m.md", "tables-10m.md"):
            cases.append(("mdviewer/plain-pipe", name))
        if name == "tables-1m.md":
            cases.append(("mdviewer/terminal-narrow", name))
        if name == "tables-math-100.md":
            cases.append(("mdviewer/terminal-math", name))
for name in CORPORA:
    if name.startswith("images-"):
        cases.append(("mdviewer/terminal-images", name))
        cases.append(("mdviewer/plain-pipe", name))
    if name.startswith("mermaid-"):
        cases.append(("mdviewer/terminal-mermaid", name))
        cases.append(("mdviewer/plain-pipe", name))
cases = [(renderer, name) for renderer, name in cases
         if (not args.renderer or renderer in args.renderer)
         and (not args.corpus or any(fnmatch.fnmatchcase(name, pattern) for pattern in args.corpus))]
if not cases:
    parser.error("no matching benchmark cases")
for name in {name for _, name in cases}:
    data = (ROOT / "corpora" / name).read_bytes()
    if len(data) != CORPORA[name]["bytes"] or hashlib.sha256(data).hexdigest() != CORPORA[name]["sha256"]:
        parser.error(f"corpus checksum mismatch: {name}")
    for asset, checksum in CORPORA[name].get("assets", {}).items():
        if hashlib.sha256((ROOT / "corpora" / asset).read_bytes()).hexdigest() != checksum:
            parser.error(f"asset checksum mismatch: {asset}")
del data

results = {
    "metadata": {
        "harness_version": 1,
        "mdviewer_commit": args.binary_revision,
        "source_checkout": checkout(PROJECT),
        "katex_native_checkout": checkout(PROJECT.parent / "katex-native"),
        "binary_sha256": hashlib.sha256(BINARY.read_bytes()).hexdigest(),
        "binary_bytes": BINARY.stat().st_size,
        "syntax_highlighting": "disabled" if args.no_highlight else "default",
        "image_rendering": args.image_mode,
        "mermaid_rendering": args.mermaid_mode,
        "process_tree_sampling": args.process_tree,
        "glow_version": read_command(["glow", "--version"]),
        "profile": args.profile,
        "hardware": args.hardware,
        "os": (f"macOS {platform.mac_ver()[0]}" if sys.platform == "darwin"
               else f"{platform.system()} {platform.release()}"),
        "rustc": read_command(["rustc", "--version"]),
        "terminal_columns": 100, "terminal_rows": 40, "cell_pixels": [10, 20],
        "narrow_terminal_columns": 14,
        "content_width": 80, "horizontal_margin": 2,
        "timeout_seconds": args.timeout,
        "warmups": args.warmups, "samples": args.samples,
        "method": __doc__,
    },
    "corpora": CORPORA,
    "cases": [],
}
for renderer, name in cases:
    print(f"START {renderer} {name}", flush=True)
    expected_images = CORPORA[name].get("equations", 0) if renderer == "mdviewer/terminal-math" else 0
    if args.image_mode == "auto" and renderer.startswith("mdviewer/terminal"):
        expected_images += CORPORA[name].get("images", 0)
    if args.mermaid_mode == "auto" and renderer.startswith("mdviewer/terminal"):
        expected_images += CORPORA[name].get("diagrams", 0)

    def valid_sample(sample):
        return (not sample["timed_out"] and sample["exit_code"] == 0
                and sample["stderr_bytes"] == 0 and sample["images"] == expected_images)

    warmups, measured = [], []
    valid = True
    for target, count in [(warmups, args.warmups), (measured, args.samples)]:
        for _ in range(count if valid else 0):
            sample = run(renderer, name)
            target.append(sample)
            if not valid_sample(sample):
                valid = False
                break
    metrics = {
        key: statistics.median(sample[key] for sample in measured)
        for key in ["wall_seconds", "first_output_seconds", "cpu_seconds", "peak_rss_mib", "output_bytes", "images",
                    "sampled_tree_peak_rss_mib", "sampled_peak_processes"]
        if measured and all(sample[key] is not None for sample in measured)
    }
    results["cases"].append({"renderer": renderer, "corpus": name, "warmups": warmups,
                             "runs": measured, "median": metrics, "valid": valid})
    (ROOT / "results.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    print(f"RESULT {renderer} {name}: {json.dumps(metrics)}, valid={valid}", flush=True)
print("DONE", flush=True)
sys.exit(0 if all(case["valid"] for case in results["cases"]) else 1)
