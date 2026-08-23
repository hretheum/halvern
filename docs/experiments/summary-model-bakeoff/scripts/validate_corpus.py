#!/usr/bin/env python3
"""Gate every corpus file before a run costs anyone an overnight.

The corpus is what makes this experiment cheap to score: because we wrote the
transcripts, faithfulness becomes arithmetic instead of reading. That only holds
if the ground truth is actually true of the transcript. A planted fact nobody
says, or a decoy whose cancellation cue is missing, produces numbers that look
fine and mean nothing — and you find out after the run, not before.

So this checks the two things that matter and are easy to get wrong: that the
declared structure is present, and that every planted item is genuinely
traceable in the text.

    ./validate_corpus.py corpus/            # everything
    ./validate_corpus.py corpus/pl/pl-short-01.json
"""

import json
import re
import sys
import unicodedata
from pathlib import Path

REQUIRED_TOP = {
    "id", "language", "group", "length_class", "template_id",
    "participants", "transcript", "ground_truth",
}
GROUPS = {"English", "WesternEuropean", "Slavic", "Cjk", "Other"}
FACT_KINDS = {"decision": 3, "action": 4, "figure": 3, "date": 2}
# Bands are generous at the edges; the point is to keep `short` under the 4000
# token single-pass threshold and `long` comfortably over it.
#
# The short floor started at 1600 and was lowered after writing real material.
# A four-to-five minute exchange that settles three things comes out around
# 1000 tokens, and the only way to reach 1600 was padding — which buys a number
# at the cost of the realism the corpus exists to have. The floor is now what a
# genuinely short meeting weighs.
TOKEN_BANDS = {"short": (900, 3800), "long": (5500, 9500)}

# The app's own estimate, from processor.rs `rough_token_count`, is a flat 0.35
# tokens per character for every language. That is roughly right for Latin
# scripts and roughly three times wrong for CJK, where one character is about
# one token.
#
# This validator deliberately does NOT copy the app's figure, because copying it
# would hide the problem: a Japanese transcript would be declared "short" here
# for the same wrong reason the app declares it short, and the corpus would
# never exercise the map/reduce branch in the language most likely to overflow
# a context window. The discrepancy is recorded as a product finding rather than
# reproduced.
TOKENS_PER_CHAR_LATIN = 0.35
TOKENS_PER_CHAR_CJK = 1.0
CJK_RANGE = re.compile(r"[぀-ヿ㐀-䶿一-鿿가-힯]")


def estimate_tokens(text):
    cjk = len(CJK_RANGE.findall(text))
    other = len(text) - cjk
    return int(cjk * TOKENS_PER_CHAR_CJK + other * TOKENS_PER_CHAR_LATIN)


def norm(s):
    """Casefold and strip accents, so a fact planted as 'Wiśniewski' still
    matches 'wisniewski' in a check. Deliberately lossy — this is a presence
    test, not a comparison."""
    s = unicodedata.normalize("NFKD", str(s)).casefold()
    return "".join(c for c in s if not unicodedata.combining(c))


def content_words(s, min_len=4):
    """Words long enough to be worth tracing. Short tokens ('the', 'na', 'は')
    match everywhere and would make traceability vacuous."""
    return [w for w in re.findall(r"\w+", norm(s)) if len(w) >= min_len]


def check(path):
    problems = []

    def bad(msg):
        problems.append(msg)

    try:
        doc = json.loads(path.read_text(encoding="utf-8"))
    except Exception as e:
        return [f"unreadable JSON: {e}"]

    missing = REQUIRED_TOP - doc.keys()
    if missing:
        bad(f"missing top-level fields: {sorted(missing)}")
        return problems

    if doc["group"] not in GROUPS:
        bad(f"group {doc['group']!r} is not one of {sorted(GROUPS)}")
    if doc["length_class"] not in TOKEN_BANDS:
        bad(f"length_class {doc['length_class']!r} unknown")

    # --- participants
    names = doc["participants"]
    if len(names) != 4:
        bad(f"expected 4 participants, found {len(names)}")
    for a in names:
        for b in names:
            if a != b and norm(a) in norm(b):
                bad(f"participant {a!r} is a substring of {b!r} — scoring cannot tell them apart")

    # --- transcript
    turns = doc["transcript"]
    if not turns:
        bad("transcript is empty")
        return problems

    last_t = -1.0
    for i, turn in enumerate(turns):
        if not {"t", "speaker", "text"} <= turn.keys():
            bad(f"turn {i} missing t/speaker/text")
            continue
        if turn["t"] < last_t:
            bad(f"turn {i}: timestamp {turn['t']} goes backwards from {last_t}")
        last_t = turn["t"]
        if turn["speaker"] not in names:
            bad(f"turn {i}: speaker {turn['speaker']!r} is not a listed participant")

    body = " ".join(t.get("text", "") for t in turns)
    body_norm = norm(body)
    tokens = estimate_tokens(body)
    lo, hi = TOKEN_BANDS.get(doc["length_class"], (0, 10**9))
    if not lo <= tokens <= hi:
        bad(f"~{tokens} tokens is outside the {doc['length_class']} band {lo}-{hi}")

    gt = doc["ground_truth"]

    # --- facts
    facts = gt.get("facts", [])
    if len(facts) != 12:
        bad(f"expected 12 facts, found {len(facts)}")

    kinds = {}
    seen_ids = set()
    for f in facts:
        kinds[f.get("kind")] = kinds.get(f.get("kind"), 0) + 1
        if f.get("id") in seen_ids:
            bad(f"duplicate fact id {f.get('id')!r}")
        seen_ids.add(f.get("id"))

        if not f.get("canonical", "").strip():
            bad(f"{f.get('id')}: empty canonical")
        paras = f.get("paraphrases") or []
        if len(paras) < 2:
            bad(f"{f.get('id')}: needs at least 2 paraphrases, has {len(paras)}")
        if len(set(map(norm, paras))) != len(paras):
            bad(f"{f.get('id')}: duplicate paraphrases")

        # Traceability. `canonical` is English while the transcript may not be,
        # so a literal search would fail on every non-English file. Owners and
        # due dates are the parts a wrong plant would corrupt, and they are
        # language-independent enough to trace.
        if f.get("kind") == "action":
            owner = f.get("owner")
            if not owner:
                bad(f"{f.get('id')}: action with no owner")
            elif owner not in names:
                bad(f"{f.get('id')}: owner {owner!r} is not one of the participants")
            else:
                # Match on name parts, not the full string. People are addressed
                # by first name in speech — "Marek, przygotujesz skrypt?" — so
                # requiring "Marek Wiśniewski" verbatim fails every realistic
                # transcript. Being a listed participant is the structural
                # check; this is the "somebody actually says their name" check.
                parts = [p for p in re.split(r"\s+", norm(owner)) if len(p) >= 3]
                if parts and not any(p in body_norm for p in parts):
                    bad(f"{f.get('id')}: owner {owner!r} is never named aloud in the transcript")
            if not f.get("due"):
                bad(f"{f.get('id')}: action with no due date")

        # Figures are deliberately NOT checked digit-by-digit. A spoken
        # transcript says "osiemset euro" and "sześćdziesiąt siedem procent",
        # not "800" and "67%" — an earlier version of this check failed every
        # correctly-written Polish file for exactly that reason. Whether the
        # figure is really in the transcript is a judgement the generator makes
        # and the reviewer confirms; a regex cannot do it across six languages.

    for kind, want in FACT_KINDS.items():
        got = kinds.get(kind, 0)
        if got != want:
            bad(f"expected {want} facts of kind {kind}, found {got}")

    # --- decoys
    decoys = gt.get("decoys", [])
    if len(decoys) != 4:
        bad(f"expected 4 decoys, found {len(decoys)}")
    for d in decoys:
        cue = d.get("cancellation_cue", "")
        if not cue.strip():
            bad(f"{d.get('id')}: empty cancellation_cue")
        elif norm(cue) not in body_norm:
            bad(f"{d.get('id')}: cancellation cue {cue!r} does not appear verbatim in the transcript")

    # --- distractors
    dis = gt.get("distractors", {})
    if len(dis.get("numbers", [])) < 2:
        bad("need at least 2 number distractors")
    if len(dis.get("names", [])) < 2:
        bad("need at least 2 name distractors")
    for n in dis.get("names", []):
        if norm(n) not in body_norm:
            bad(f"distractor name {n!r} is not in the transcript — a distractor must be mentioned")
        if n in names:
            bad(f"distractor name {n!r} is also a participant")

    # --- reversal
    rev = gt.get("reversal") or {}
    if not rev.get("initial") or not rev.get("final"):
        bad("reversal needs both initial and final")
    if rev.get("correct_answer") != "final":
        bad("reversal.correct_answer must be 'final'")
    if rev.get("initial") and rev.get("final") and norm(rev["initial"]) == norm(rev["final"]):
        bad("reversal initial and final are the same")

    return problems


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2

    root = Path(argv[1])
    files = sorted(root.rglob("*.json")) if root.is_dir() else [root]
    files = [f for f in files if "_control" not in f.parts]

    if not files:
        print(f"no corpus files under {root}")
        return 1

    failed = 0
    for f in files:
        problems = check(f)
        if problems:
            failed += 1
            print(f"\n\033[1m{f}\033[0m")
            for p in problems:
                print(f"    {p}")

    print()
    if failed:
        print(f"{failed} of {len(files)} file(s) failed. Fix them before running —")
        print("numbers from an unvalidated corpus cannot be defended.")
        return 1
    print(f"{len(files)} corpus file(s) valid")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
