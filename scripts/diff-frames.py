#!/usr/bin/env python3
"""Frame differential between two PPM sets (SPEC-011 VAL-03).

Stdlib only; run with `python3 -I`.

PPM sets use gba-run naming: `<prefix>.<frame>.ppm` (P6, 240x160, maxval 255).
For each selected frame N it compares ours[N] with ref[N] (exact pixel
equality) and, when --window K > 0, searches ref[N+d] for d in [-K, K] (only
the reference frames that exist) and reports the best offset. Pixels are
compared after both sides were expanded from BGR555 with (v<<3)|(v>>2), so
identical guest colors compare equal.

With --ours-hashes/--ref-hashes (the `frame N hash H` lines that gba-run and
mgba-oracle print with --hashes, FNV-1a over BGR555) it also analyses every
frame: how many are hash-equal at the same index and, for each frame where our
image changes, the offset d (|d| <= --hash-window) of the nearest reference
frame with the same hash. A positive d means the reference shows our image d
frames later (the reference is behind); "unmatched" frames never appear in the
reference within the window.

Output: a text table on stdout and, with --json, a numbers-only JSON report.
With --mask-dir, writes a PPM per frame with differing pixels highlighted
(for local inspection only; never commit them, they derive from the ROM).
"""

import argparse
import json
import os
import sys

WIDTH, HEIGHT = 240, 160


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    # P6\n<w> <h>\n255\n — tolerate generic whitespace, no comments expected.
    fields = []
    pos = 0
    while len(fields) < 4:
        while data[pos : pos + 1].isspace():
            pos += 1
        start = pos
        while not data[pos : pos + 1].isspace():
            pos += 1
        fields.append(data[start:pos])
    pos += 1
    if fields[0] != b"P6" or int(fields[3]) != 255:
        raise ValueError(f"{path}: not a P6/255 PPM")
    w, h = int(fields[1]), int(fields[2])
    pixels = data[pos : pos + w * h * 3]
    if (w, h) != (WIDTH, HEIGHT) or len(pixels) != w * h * 3:
        raise ValueError(f"{path}: unexpected size {w}x{h}")
    return pixels


def frame_path(prefix, n):
    return f"{prefix}.{n}.ppm"


class Cache:
    def __init__(self, prefix):
        self.prefix = prefix
        self.data = {}

    def get(self, n):
        if n not in self.data:
            p = frame_path(self.prefix, n)
            self.data[n] = read_ppm(p) if os.path.exists(p) else None
        return self.data[n]


def diff_stats(a, b):
    """Return (differing pixels, bbox or None, max channel delta)."""
    total = 0
    max_delta = 0
    x0 = y0 = 10**9
    x1 = y1 = -1
    for y in range(HEIGHT):
        row = y * WIDTH * 3
        ra = a[row : row + WIDTH * 3]
        rb = b[row : row + WIDTH * 3]
        if ra == rb:
            continue
        for x in range(WIDTH):
            i = x * 3
            pa = ra[i : i + 3]
            pb = rb[i : i + 3]
            if pa != pb:
                total += 1
                d = max(abs(pa[0] - pb[0]), abs(pa[1] - pb[1]), abs(pa[2] - pb[2]))
                max_delta = max(max_delta, d)
                x0, y0, x1, y1 = min(x0, x), min(y0, y), max(x1, x), max(y1, y)
    bbox = None if total == 0 else [x0, y0, x1, y1]
    return total, bbox, max_delta


def count_diff(a, b):
    """Fast differing-pixel count (used for the offset search)."""
    if a == b:
        return 0
    total = 0
    for y in range(HEIGHT):
        row = y * WIDTH * 3
        ra = a[row : row + WIDTH * 3]
        rb = b[row : row + WIDTH * 3]
        if ra == rb:
            continue
        for x in range(0, WIDTH * 3, 3):
            if ra[x : x + 3] != rb[x : x + 3]:
                total += 1
    return total


def write_mask(path, a, b):
    out = bytearray(b"P6\n%d %d\n255\n" % (WIDTH, HEIGHT))
    for i in range(0, WIDTH * HEIGHT * 3, 3):
        if a[i : i + 3] == b[i : i + 3]:
            g = (a[i] + a[i + 1] + a[i + 2]) // 12  # dimmed grey of ours
            out += bytes((g, g, g))
        else:
            out += b"\xff\x00\xff"
    with open(path, "wb") as f:
        f.write(out)


def load_hashes(path):
    out = {}
    with open(path) as f:
        for line in f:
            parts = line.split()
            if len(parts) == 4 and parts[0] == "frame" and parts[2] == "hash":
                out[int(parts[1])] = parts[3]
    return out


def hash_analysis(ours_path, ref_path, window):
    ours = load_hashes(ours_path)
    ref = load_hashes(ref_path)
    where = {}
    for n, h in ref.items():
        where.setdefault(h, []).append(n)
    equal = sum(1 for n, h in ours.items() if ref.get(n) == h)
    hist = {}
    unmatched = []
    for n in sorted(ours):
        if ours.get(n - 1) == ours[n]:
            continue  # only frames where our image changes carry timing information
        cands = [k - n for k in where.get(ours[n], []) if abs(k - n) <= window]
        if not cands:
            unmatched.append(n)
            continue
        d = min(cands, key=lambda v: (abs(v), v))
        hist[d] = hist.get(d, 0) + 1
    first_diff = next((n for n in sorted(ours) if ref.get(n) != ours[n]), None)
    return {
        "frames": len(ours),
        "hash_equal_same_frame": equal,
        "first_differing_frame": first_diff,
        "change_frames": sum(hist.values()) + len(unmatched),
        "hash_window": window,
        "offset_histogram": {str(k): hist[k] for k in sorted(hist)},
        "unmatched_change_frames": unmatched,
    }


def parse_frames(args):
    if args.frames:
        return [int(x) for x in args.frames.split(",") if x]
    return list(range(args.every, args.last + 1, args.every))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--ours", required=True, help="PPM prefix of gba-run (without .<N>.ppm)")
    ap.add_argument("--ref", required=True, help="PPM prefix of the oracle")
    ap.add_argument("--frames", help="comma-separated frame list")
    ap.add_argument("--every", type=int, default=100)
    ap.add_argument("--last", type=int, default=3000)
    ap.add_argument("--window", type=int, default=0, help="search ref[N-K..N+K] for best match")
    ap.add_argument("--json", help="write numbers-only JSON report here")
    ap.add_argument("--mask-dir", help="write diff masks (local only)")
    ap.add_argument("--ours-hashes", help="stderr of gba-run --hashes")
    ap.add_argument("--ref-hashes", help="stderr of mgba-oracle --hashes")
    ap.add_argument("--hash-window", type=int, default=30)
    args = ap.parse_args()

    ours = Cache(args.ours)
    ref = Cache(args.ref)
    rows = []
    print(f"{'frame':>6} {'diff_px':>8} {'diff_%':>7} {'maxΔ':>5} {'bbox(x0,y0,x1,y1)':>20}"
          f" {'best_d':>6} {'best_px':>8} {'best_%':>7}")
    for n in parse_frames(args):
        a = ours.get(n)
        b = ref.get(n)
        if a is None or b is None:
            print(f"{n:>6} missing {'ours' if a is None else 'ref'}")
            continue
        total, bbox, max_delta = diff_stats(a, b)
        best_d, best = 0, total
        if args.window and total:
            for d in sorted(range(-args.window, args.window + 1), key=abs):
                c = ref.get(n + d)
                if c is None or d == 0:
                    continue
                t = count_diff(a, c)
                if t < best:
                    best_d, best = d, t
                if best == 0:
                    break
        npx = WIDTH * HEIGHT
        row = {
            "frame": n,
            "diff_pixels": total,
            "diff_pct": round(100.0 * total / npx, 3),
            "max_channel_delta": max_delta,
            "bbox": bbox,
            "best_offset": best_d,
            "best_diff_pixels": best,
            "best_diff_pct": round(100.0 * best / npx, 3),
        }
        rows.append(row)
        bb = "-" if bbox is None else ",".join(str(v) for v in bbox)
        print(f"{n:>6} {total:>8} {row['diff_pct']:>7.2f} {max_delta:>5} {bb:>20}"
              f" {best_d:>+6} {best:>8} {row['best_diff_pct']:>7.2f}")
        if args.mask_dir and total:
            os.makedirs(args.mask_dir, exist_ok=True)
            write_mask(os.path.join(args.mask_dir, f"mask.{n}.ppm"), a, b)

    exact = sum(1 for r in rows if r["diff_pixels"] == 0)
    exact_shifted = sum(1 for r in rows if r["best_diff_pixels"] == 0)
    summary = {
        "frames_compared": len(rows),
        "identical_same_frame": exact,
        "identical_within_window": exact_shifted,
        "window": args.window,
    }
    print(json.dumps(summary))
    report = {"summary": summary, "frames": rows}
    if args.ours_hashes and args.ref_hashes:
        h = hash_analysis(args.ours_hashes, args.ref_hashes, args.hash_window)
        report["hashes"] = h
        print(f"hash-equal frames: {h['hash_equal_same_frame']}/{h['frames']}, "
              f"first differing: {h['first_differing_frame']}")
        print(f"offsets over {h['change_frames']} change frames (+d = reference behind): "
              f"{h['offset_histogram']}")
        print(f"unmatched within ±{h['hash_window']}: {len(h['unmatched_change_frames'])} "
              f"{h['unmatched_change_frames'][:40]}")
    if args.json:
        with open(args.json, "w") as f:
            json.dump(report, f, indent=1)
            f.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
