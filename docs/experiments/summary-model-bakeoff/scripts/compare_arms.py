#!/usr/bin/env python3
"""Put the two arms side by side, per tier, so the comparison can be read.

    ./compare_arms.py results/raw/scored-S.json results/raw/scored-M.json

score.py writes one flat table per run. The question the experiment asks is
which model wins inside a tier, in a group, and whether the answer survives
controlling the sampler — three axes that a flat table hides. This prints the
pairing instead: Qwen against its Gemma, shipped against matched, with the
margin that decides it.
"""

import json
import sys
from collections import defaultdict
from pathlib import Path

TIERS = [("A", "qwen3.5:4b", "gemma3:4b"), ("B", "qwen3.5:2b", "gemma3:1b")]
GROUPS = ["English", "WesternEuropean", "Slavic", "Cjk", "Other"]


def load(path):
    rows = json.loads(Path(path).read_text(encoding="utf-8"))
    agg = defaultdict(lambda: defaultdict(list))
    for r in rows:
        for m in ("M1", "M2_en", "M3", "M4", "M6", "M7", "M8"):
            if r.get(m) is not None:
                agg[(r["model"], r["group"])][m].append(r[m])
    return {k: {m: sum(v) / len(v) for m, v in d.items()} for k, d in agg.items()}


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    S, M = load(sys.argv[1]), load(sys.argv[2])

    for tier, qwen, gemma in TIERS:
        print(f"\n=== Tier {tier}: {qwen} vs {gemma} ===")
        print(f"{'group':<18}{'metric':<8}{'shipped':>18}{'matched':>18}")
        for g in GROUPS:
            for metric, label in (("M3", "recall"), ("M4", "fabric"), ("M7", "hygiene")):
                q_s = S.get((qwen, g), {}).get(metric)
                g_s = S.get((gemma, g), {}).get(metric)
                q_m = M.get((qwen, g), {}).get(metric)
                g_m = M.get((gemma, g), {}).get(metric)
                if q_s is None and q_m is None:
                    continue
                fmt = lambda a, b: (
                    f"{a:.2f} / {b:.2f}" if a is not None and b is not None else "     -     "
                )
                # Winner marked on recall and hygiene (higher better) and on
                # fabrication (lower better) — the arrow is the point of the
                # table, so it is computed rather than eyeballed.
                def mark(a, b):
                    if a is None or b is None:
                        return " "
                    if abs(a - b) < 1e-9:
                        return "="
                    better = a > b if metric != "M4" else a < b
                    return "Q" if better else "G"

                print(f"{g if metric=='M3' else '':<18}{label:<8}"
                      f"{fmt(q_s, g_s):>15} {mark(q_s, g_s)}"
                      f"{fmt(q_m, g_m):>15} {mark(q_m, g_m)}")
            print()

    print("Q = the Qwen wins that cell, G = the Gemma, = a tie.")
    print("Pairs read qwen / gemma. Recall and hygiene higher is better;")
    print("fabrication lower is better.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
