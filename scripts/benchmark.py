#!/usr/bin/env python3
"""Benchmark svgm against SVGO on the 100-logo corpus and print the numbers
used in README.md ("Benchmarks") and benchmark-results.md.

Usage:
  cargo build --release
  scripts/benchmark.py [--corpus ~/Desktop/svg-logos-benchmark-test] [--out DIR]
                       [--runs 5] [--per-file-runs 3] [--table]

Measures:
  - output bytes / compression for both tools (batch mode outputs),
  - per-file wins / ties,
  - batch time (one process for the folder): median of --runs,
  - per-file time (one process per file, as earlier benchmarks measured):
    median of --per-file-runs (SVGO per-file takes ~10 s per run).
With --table, also prints the per-file Markdown table for benchmark-results.md.
"""
import argparse
import os
import shutil
import statistics
import subprocess
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def timed(cmd):
    t = time.perf_counter()
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return time.perf_counter() - t


def sizes(d):
    return {f: os.path.getsize(os.path.join(d, f)) for f in os.listdir(d) if f.endswith(".svg")}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--corpus", default=os.path.expanduser("~/Desktop/svg-logos-benchmark-test"))
    ap.add_argument("--svgm", default=os.path.join(REPO, "target/release/svgm"))
    ap.add_argument("--out", default="/tmp/svgm-benchmark")
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--per-file-runs", type=int, default=3)
    ap.add_argument("--table", action="store_true")
    args = ap.parse_args()

    if os.path.exists(args.out):
        shutil.rmtree(args.out)
    inp = os.path.join(args.out, "in")
    shutil.copytree(args.corpus, inp)
    files = sorted(f for f in os.listdir(inp) if f.endswith(".svg"))
    out_m, out_o = os.path.join(args.out, "svgm"), os.path.join(args.out, "svgo")

    batch_m, batch_o = [], []
    for _ in range(args.runs):
        shutil.rmtree(out_m, ignore_errors=True)
        shutil.rmtree(out_o, ignore_errors=True)
        batch_m.append(timed([args.svgm, "-r", inp, "-o", out_m, "--quiet", "--no-config"]))
        batch_o.append(timed(["svgo", "-f", inp, "-o", out_o, "-q"]))

    scratch = os.path.join(args.out, "perfile")
    os.makedirs(scratch, exist_ok=True)
    per_m, per_o = [], []
    for _ in range(args.per_file_runs):
        t_m = t_o = 0.0
        for f in files:
            src = os.path.join(inp, f)
            t_m += timed([args.svgm, src, "-o", os.path.join(scratch, "m.svg"), "--quiet", "--no-config"])
            t_o += timed(["svgo", "-i", src, "-o", os.path.join(scratch, "o.svg"), "-q"])
        per_m.append(t_m)
        per_o.append(t_o)

    s_in, s_m, s_o = sizes(inp), sizes(out_m), sizes(out_o)
    total_in = sum(s_in.values())
    total_m = sum(s_m[f] for f in files)
    total_o = sum(s_o[f] for f in files)
    wins_m = sum(s_m[f] < s_o[f] for f in files)
    wins_o = sum(s_o[f] < s_m[f] for f in files)
    ties = len(files) - wins_m - wins_o

    def pct(out):
        return 100.0 * (1 - out / total_in)

    med = statistics.median
    print(f"files: {len(files)}, input {total_in:,} B ({total_in / 1024:.1f} KiB)")
    print(f"compression: svgm {pct(total_m):.1f}% ({total_m:,} B), svgo {pct(total_o):.1f}% ({total_o:,} B)")
    print(f"wins: svgm {wins_m}, svgo {wins_o}, tied {ties}")
    print("batch runs (ms): svgm " + ", ".join(f"{t * 1000:.1f}" for t in batch_m)
          + " | svgo " + ", ".join(f"{t * 1000:.1f}" for t in batch_o))
    print(f"batch median: svgm {med(batch_m) * 1000:.1f}ms, svgo {med(batch_o) * 1000:.1f}ms, "
          f"{med(batch_o) / med(batch_m):.1f}x")
    print(f"per-file median: svgm {med(per_m) * 1000:.0f}ms, svgo {med(per_o) * 1000:,.0f}ms, "
          f"{med(per_o) / med(per_m):.1f}x")

    if args.table:
        rows = []
        for f in files:
            cm = 100.0 * (1 - s_m[f] / s_in[f])
            co = 100.0 * (1 - s_o[f] / s_in[f])
            rows.append((cm - co, f[:-4], cm, co))
        rows.sort(key=lambda r: -r[0])
        print("\n| File | SVGM | SVGO | Gap | |\n| --- | --- | --- | --- | --- |")
        for gap, name, cm, co in rows:
            verdict = "ahead" if gap > 0.05 else ("behind" if gap < -0.05 else "tied")
            print(f"| {name} | {cm:.1f}% | {co:.1f}% | {gap:+.1f} | {verdict} |")


if __name__ == "__main__":
    main()
