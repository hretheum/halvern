#!/usr/bin/env python3
"""Score a bake-off run. All six metrics, computed locally.

    ./score.py --raw ../results/raw/<run-id> --corpus ../corpus            # score
    ./score.py --raw ... --corpus ... --adjudications adj.json             # re-score with judge answers
    ./score.py --raw ... --corpus ... --report ../results/REPORT.md        # write the table

This deliberately does NOT call any model. Rows it cannot settle are written to
`uncertain.json` for adjudication by P3, and fed back in with --adjudications.
Two reasons: the scorer stays runnable with no network and no key, and the
privacy rule — real meetings are never sent to a judge — is enforced by the tool
rather than remembered by the operator. Records whose transcript_id begins with
`control-` are excluded from the uncertain file entirely.
"""

import argparse
import json
import re
import sys
import unicodedata
from collections import defaultdict
from pathlib import Path

# ---------------------------------------------------------------- helpers

def norm(s):
    s = unicodedata.normalize("NFKD", str(s)).casefold()
    s = "".join(c for c in s if not unicodedata.combining(c))
    return re.sub(r"\s+", " ", s).strip()


def content_words(s, min_len=4):
    return {w for w in re.findall(r"\w+", norm(s)) if len(w) >= min_len}


# ---------------------------------------------------------------- M1 structure

STRUCT = [
    (re.compile(r"^(#{1,6})\s"), lambda m: f"H{len(m.group(1))}"),
    (re.compile(r"^\s*[-*+]\s"), lambda m: "LI"),
    (re.compile(r"^\s*\d+\.\s"), lambda m: "OL"),
    (re.compile(r"^\s*\|"), None),          # table row, shape computed below
    (re.compile(r"^\s*```"), lambda m: "FENCE"),
]


def skeleton(md):
    """Structural tokens only, prose removed.

    A table row becomes `TR<n>` where n is its cell count, so a translation that
    drops a pipe changes the token and shows up as a diff. Emphasis is counted
    per line rather than positioned, because translations legitimately move
    bold text within a sentence.
    """
    out = []
    for line in (md or "").splitlines():
        if not line.strip():
            continue
        matched = False
        for pat, tok in STRUCT:
            m = pat.match(line)
            if not m:
                continue
            if tok is None:
                cells = [c for c in line.strip().strip("|").split("|")]
                sep = all(re.fullmatch(r"\s*:?-{2,}:?\s*", c) for c in cells)
                out.append("TSEP" if sep else f"TR{len(cells)}")
            else:
                out.append(tok(m))
            matched = True
            break
        if not matched:
            out.append("P")
        b = line.count("**")
        if b:
            out.append(f"B{b}")
    return out


def levenshtein(a, b):
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def m1_structure(english, final):
    """Structure preserved through the translation pass.

    Only meaningful when a translation actually happened. A model that ignores
    the instruction and returns the English document unchanged would score a
    perfect 1.000 here — it preserved the skeleton by not touching it — so
    `translated_at_all` gates this in the aggregate.

    Note the partial case, which is what the first real run actually produced:
    gemma3:4b translated the German prose correctly but left the title and the
    `**Summary**` section labels in English, while qwen3.5:4b translated those
    too. That is not caught here, because the skeleton is identical either way.
    It shows up in M2 instead, and perversely in Gemma's favour — the template's
    section titles are English, so a model that leaves them untranslated matches
    them better. Worth reading M2 and M6 together for any non-English row.
    """
    se, sf = skeleton(english), skeleton(final)
    if not se:
        return None
    return max(0.0, 1.0 - levenshtein(se, sf) / len(se))


def translated_at_all(english, final):
    """Did the translation pass change the prose at all?

    Compares the non-structural text. Identical prose means the pass was a
    no-op, whatever M6's language identifier makes of the result.
    """
    strip = lambda md: re.sub(r"[|`#*_\-\s]+", " ", md or "").strip()
    return strip(english) != strip(final)


# ---------------------------------------------------------------- M2 template

def section_label(line):
    """The section marker on this line, if it is one.

    The application's own prompt does not produce `# Heading` — it produces
    `**Summary**` on its own line. A first version of this recognised only
    hash-headings and scored a perfectly well-formed document at zero, which
    would have made M2 useless as the control condition it exists to be. Both
    forms count, because both are what the pipeline actually emits.
    """
    l = line.strip()
    m = re.match(r"^#{1,6}\s+(.*)$", l)
    if m:
        return norm(m.group(1))
    m = re.match(r"^\*\*(.+?)\*\*:?$", l)
    if m:
        return norm(m.group(1))
    return None


def m2_compliance(md, template):
    md = md or ""
    lines = md.splitlines()
    labels = {section_label(l) for l in lines if section_label(l)}
    score, sections = 0.0, template["sections"]

    present = [s for s in sections if norm(s["title"]) in labels]
    score += 0.4 * (len(present) / len(sections))

    def body_of(title):
        want, out, collecting = norm(title), [], False
        for l in lines:
            lab = section_label(l)
            if lab is not None:
                collecting = lab == want
                continue
            if collecting:
                out.append(l)
        return out

    paras = [s for s in sections if s.get("format") == "paragraph"]
    lists = [s for s in sections if s.get("format") == "list"]

    if paras:
        ok = sum(1 for s in paras
                 if any(l.strip() and not re.match(r"^\s*([-*+]|\d+\.|\|)", l) for l in body_of(s["title"])))
        score += 0.2 * (ok / len(paras))
    else:
        score += 0.2

    if lists:
        ok = sum(1 for s in lists
                 if any(re.match(r"^\s*([-*+]|\d+\.|\|)", l) for l in body_of(s["title"])))
        score += 0.2 * (ok / len(lists))
    else:
        score += 0.2

    tabled = [s for s in sections if "item_format" in s and "|" in s["item_format"]]
    if tabled:
        ok = 0
        for s in tabled:
            want = len([c for c in s["item_format"].splitlines()[0].strip().strip("|").split("|")])
            rows = [l for l in body_of(s["title"]) if l.strip().startswith("|")]
            data = [r for r in rows
                    if not all(re.fullmatch(r"\s*:?-{2,}:?\s*", c)
                               for c in r.strip().strip("|").split("|"))]
            if data and any(len(r.strip().strip("|").split("|")) == want for r in data):
                ok += 1
        score += 0.2 * (ok / len(tabled))
    else:
        score += 0.2

    return round(score, 4)


# ---------------------------------------------------------------- M3 recall

def fact_status(fact, english_md):
    """HIT / MISS / UNCERTAIN against the English intermediate."""
    hay = norm(english_md or "")
    for phrase in [fact["canonical"]] + list(fact.get("paraphrases") or []):
        p = norm(phrase)
        if p and p in hay:
            return "HIT", phrase
        words = content_words(phrase)
        if words and len(words & content_words(hay, 4)) / len(words) >= 0.75:
            return "HIT", phrase
    anchors = content_words(fact["canonical"], 5)
    if anchors and (anchors & content_words(hay, 5)):
        return "UNCERTAIN", None
    return "MISS", None


def owner_ok(fact, english_md):
    owner = fact.get("owner")
    if not owner:
        return True
    parts = [p for p in norm(owner).split() if len(p) >= 3]
    return any(p in norm(english_md or "") for p in parts)


# ---------------------------------------------------------------- M4 fabrication

DECISION_SECTIONS = ("key decisions", "action items")


def sections_of_interest(md):
    out, collecting = [], False
    for l in (md or "").splitlines():
        lab = section_label(l)
        if lab is not None:
            collecting = lab in DECISION_SECTIONS
            continue
        if collecting and l.strip():
            out.append(l)
    return "\n".join(out)


def decoy_status(decoy, md):
    zone = norm(sections_of_interest(md))
    words = content_words(decoy["claim"], 5)
    if not words:
        return "CLEAN", None
    overlap = len(words & content_words(zone, 5)) / len(words)
    if overlap >= 0.6:
        return "UNCERTAIN", decoy["claim"]
    return "CLEAN", None


# ---------------------------------------------------------------- M5 reversal

def m5_reversal(rev, english_md):
    """1 correct, 0 wrong, None means a judge has to look.

    The naive rule — final present and initial absent — fails on the phrasing a
    good summary actually uses. "The release date moved from 5 October to 12
    October" mentions both dates and is exactly right: it reports the final
    decision and marks the change. A first version of this scored that as a
    failure, which would have penalised the best possible answer.

    So both-present is not a verdict, it is a question.
    """
    hay = norm(english_md or "")

    def present(s):
        w = content_words(s, 4)
        return bool(w) and len(w & content_words(hay, 4)) / len(w) >= 0.6

    has_final, has_initial = present(rev["final"]), present(rev["initial"])
    if has_final and not has_initial:
        return 1
    if not has_final:
        return 0
    return None  # both present — did it mark the change, or report both as live?


# ---------------------------------------------------------------- M6 language

CJK = re.compile(r"[぀-ヿ㐀-鿿]")
STOPWORDS = {
    "en": {"the", "and", "that", "with", "were", "have", "this", "will"},
    "de": {"und", "der", "die", "das", "nicht", "wird", "werden", "eine"},
    "es": {"que", "para", "los", "las", "con", "por", "una", "del"},
    "pl": {"nie", "sie", "jest", "oraz", "przez", "tego", "ktore", "dla"},
    "tr": {"ile", "icin", "olarak", "bir", "daha", "sonra", "olan", "gore"},
}


def m6_language(md, target):
    """Script check for Japanese, stopword frequency for the Latin languages.

    Deliberately crude and dependency-free. It is enough to separate the five
    languages in this corpus, and M6 is expected to be near-perfect anyway —
    output language is set by an explicit translation pass, so a failure here is
    a pipeline bug rather than a close call. If it ever becomes a close call,
    replace this with a real identifier rather than trusting it.
    """
    text = re.sub(r"[|`#*_\-]", " ", md or "")
    if target == "ja":
        return 1 if CJK.search(text) else 0
    if CJK.search(text):
        return 0
    # Stopwords alone were not discriminative enough: the first run scored an
    # English document as German, because a handful of short function words is
    # weak evidence and German names in the text tipped it. Diacritic
    # signatures are far stronger — English has none of these, and each
    # language's set is close to unique.
    raw = text
    signature = {
        "de": len(re.findall(r"[äöüßÄÖÜ]", raw)),
        "pl": len(re.findall(r"[ąćęłńóśźżĄĆĘŁŃÓŚŹŻ]", raw)),
        "tr": len(re.findall(r"[ğışİĞŞ]", raw)),
        "es": len(re.findall(r"[ñ¿¡áéíóúÑ]", raw)),
        "en": 0,
    }
    words = re.findall(r"\w+", norm(text))
    if not words:
        return 0

    stop = {lang: sum(1 for w in words if w in sw) for lang, sw in STOPWORDS.items()}
    # Weight the signature heavily; one umlaut is worth more than one "der".
    score = {lang: stop.get(lang, 0) + 3 * signature.get(lang, 0) for lang in STOPWORDS}

    best = max(score, key=score.get)
    runner_up = max((v for k, v in score.items() if k != best), default=0)

    if score[best] == 0:
        # No evidence at all — short or heavily tabular documents hit this.
        # Not the same as evidence of the wrong language.
        return None
    if score[best] < runner_up * 1.5:
        # Too close to call. Mixed-language output lands here, which is itself
        # worth seeing rather than rounding to a verdict.
        return None
    return 1 if best == target else 0


# ---------------------------------------------------------------- M8 intermediate

def m8_intermediate_is_english(eng):
    """The pipeline forces an English intermediate. Is it actually English?

    Added after the matched-sampling run, where qwen3.5:2b produced its entire
    "English" intermediate in Chinese for the Japanese transcript — despite the
    prompt saying `non-English prose is invalid`. Recall scored 0.000 because
    the matcher had no English to match, which looked like a comprehension
    collapse and was really a language failure one stage earlier.

    M6 could not see it: M6 reads the FINAL document, which was Japanese and
    therefore correct. Nothing checked the stage in between, and everything
    downstream — the structured pass and the translation — was built on it.
    """
    text = re.sub(r"[|`#*_\-]", " ", eng or "")
    cjk = len(re.findall(r"[぀-ヿ㐀-鿿]", text))
    letters = len(re.findall(r"[A-Za-z]", text))
    if cjk == 0 and letters == 0:
        return None
    # An English document naming Japanese participants carries a little CJK;
    # one that is mostly CJK is not English by any reading.
    return 0 if cjk > letters * 0.5 else 1


# ---------------------------------------------------------------- M7 hygiene

SCAFFOLD = [
    (re.compile(r"</?document>"), "prompt tag <document> in the output"),
    (re.compile(r"\A\s*```"), "whole document wrapped in a code fence"),
    (re.compile(r"<Add Title here>"), "unfilled title placeholder"),
    (re.compile(r"<transcript_chunk>|</transcript_chunk>"), "chunk tag in the output"),
    (re.compile(r"\A\s*(Here is|Sure,|Certainly)", re.I), "conversational preamble"),
]


def m7_hygiene(eng, fin):
    """Did the model's own scaffolding end up in the user's document?

    Added after the first run, because the six designed metrics all missed a
    defect a user would notice immediately: gemma3:1b returned a summary headed
    `# <Add Title here>` in every language tested, and on Japanese wrapped the
    whole thing in a code fence with the prompt's `<document>` tag still around
    it. `clean_llm_markdown_output` did not strip either.

    Binary and objective, so it needs no judge — and unlike the quality metrics
    it fails a model for something indefensible rather than merely worse.
    """
    found = []
    for text in (eng or "", fin or ""):
        for pat, label in SCAFFOLD:
            if pat.search(text) and label not in found:
                found.append(label)
    return (0 if found else 1), found


# ---------------------------------------------------------------- driver

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw", required=True, type=Path)
    ap.add_argument("--corpus", required=True, type=Path)
    ap.add_argument("--template", type=Path,
                    default=Path("../../../../frontend/src-tauri/templates/standard_meeting.json"))
    ap.add_argument("--adjudications", type=Path)
    ap.add_argument("--report", type=Path)
    args = ap.parse_args()

    template = json.loads(args.template.read_text(encoding="utf-8"))
    truth = {}
    for f in args.corpus.rglob("*.json"):
        d = json.loads(f.read_text(encoding="utf-8"))
        truth[d["id"]] = d

    adj = {}
    if args.adjudications and args.adjudications.exists():
        adj = json.loads(args.adjudications.read_text(encoding="utf-8"))

    records = [json.loads(f.read_text(encoding="utf-8")) for f in sorted(args.raw.glob("*.json"))]
    if not records:
        print(f"no records under {args.raw}", file=sys.stderr)
        return 1

    shas = {r.get("processor_sha") for r in records if r.get("processor_sha")}
    if len(shas) > 1:
        print(f"INVALID RUN: processor.rs changed mid-run ({sorted(shas)})", file=sys.stderr)
        return 2

    uncertain, rows = [], []
    for r in records:
        gt = truth.get(r["transcript_id"], {}).get("ground_truth")
        is_control = r["transcript_id"].startswith("control-")
        eng, fin = r.get("english_intermediate", ""), r.get("final_markdown", "")

        row = {
            "transcript_id": r["transcript_id"], "model": r["model"], "arm": r["arm"],
            "repeat": r.get("repeat", 1), "group": r.get("group"), "language": r.get("language"),
            "length_class": r.get("length_class"), "wall_ms": r.get("wall_ms"),
            "M1": m1_structure(eng, fin) if r.get("language") != "en" else None,
            "translated": translated_at_all(eng, fin) if r.get("language") != "en" else None,
            "M2": m2_compliance(fin, template),
            "M2_en": m2_compliance(eng, template),
            "M6": m6_language(fin, r.get("language")),
            "M8": m8_intermediate_is_english(eng),
            "M7": m7_hygiene(eng, fin)[0],
            "M7_issues": m7_hygiene(eng, fin)[1],
        }

        if gt and not is_control:
            hits = 0
            for fact in gt["facts"]:
                key = f"{r['transcript_id']}|{r['model']}|{r['arm']}|{r['repeat']}|{fact['id']}"
                status, _ = fact_status(fact, eng)
                if status == "HIT" and not owner_ok(fact, eng):
                    status = "MISS"
                if status == "UNCERTAIN":
                    if key in adj:
                        status = "HIT" if adj[key].get("present") else "MISS"
                    else:
                        uncertain.append({"key": key, "kind": "fact",
                                          "canonical": fact["canonical"],
                                          "paraphrases": fact.get("paraphrases", []),
                                          "summary": eng})
                if status == "HIT":
                    hits += 1
            row["M3"] = round(hits / len(gt["facts"]), 4)

            fabricated = pending = 0
            for d in gt["decoys"]:
                key = f"{r['transcript_id']}|{r['model']}|{r['arm']}|{r['repeat']}|{d['id']}"
                # Checked on the ENGLISH intermediate, for the same reason
                # recall is: the decoy `claim` is written in English, and word
                # overlap against a Polish or Japanese translation finds
                # nothing. A first version checked the final document and
                # scored a summary that plainly asserted a decoy as clean.
                #
                # The cost is that a fabrication introduced *by the translation
                # pass* is invisible here. That is a narrow gap — translation
                # invents structure far more often than it invents decisions,
                # and M1 catches that.
                status, _ = decoy_status(d, eng)
                if status == "UNCERTAIN":
                    if key in adj:
                        status = "FABRICATED" if adj[key].get("fabricated") else "CLEAN"
                    else:
                        uncertain.append({"key": key, "kind": "decoy",
                                          "claim": d["claim"],
                                          "cancellation_cue": d["cancellation_cue"],
                                          "summary": eng})
                if status == "FABRICATED":
                    fabricated += 1
                elif status == "UNCERTAIN":
                    pending += 1
            row["M4"] = round(fabricated / len(gt["decoys"]), 4)
            # Unadjudicated decoys count as clean, which biases fabrication
            # DOWNWARDS — the dangerous direction for the one metric that
            # decides trust. Carrying the count means the aggregate can refuse
            # to look finished when it is not.
            row["M4_pending"] = pending
            row["M5"] = m5_reversal(gt["reversal"], eng)

        rows.append(row)

    out = args.raw.parent / "scored.json"
    out.write_text(json.dumps(rows, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"scored {len(rows)} record(s) -> {out}")

    if uncertain:
        u = args.raw.parent / "uncertain.json"
        u.write_text(json.dumps(uncertain, indent=2, ensure_ascii=False), encoding="utf-8")
        pct = 100 * len(uncertain) / max(1, len(rows) * 16)
        print(f"{len(uncertain)} row(s) need adjudication ({pct:.1f}%) -> {u}")
        print("Run P3 / P3b from 03-prompts.md, then re-score with --adjudications.")
        if pct > 25:
            print("WARNING: over 25% uncertain suggests the paraphrase lists are too thin.")

    agg = defaultdict(lambda: defaultdict(list))
    untranslated = defaultdict(int)
    for r in rows:
        k = (r["model"], r["group"], r["arm"])
        for m in ("M1", "M2", "M2_en", "M3", "M4", "M5", "M6", "M7", "M8"):
            if r.get(m) is None:
                continue
            # Structure "preserved" by never translating is not preservation.
            if m == "M1" and r.get("translated") is False:
                untranslated[k] += 1
                continue
            agg[k][m].append(r[m])

    pending_total = sum(r.get("M4_pending", 0) for r in rows)
    print(f"\n{'model':<14}{'group':<18}{'arm':<5}{'M1':>7}{'M2en':>7}{'M3':>7}{'M4':>7}{'M5':>7}{'M6':>7}{'M7':>7}{'M8':>7}")
    lines = []
    for k in sorted(agg):
        v = agg[k]
        mean = lambda m: f"{sum(v[m])/len(v[m]):.3f}" if v.get(m) else "  -  "
        line = (f"{k[0]:<14}{k[1]:<18}{k[2]:<5}"
                f"{mean('M1'):>7}{mean('M2_en'):>7}{mean('M3'):>7}"
                f"{mean('M4'):>7}{mean('M5'):>7}{mean('M6'):>7}{mean('M7'):>7}{mean('M8'):>7}")
        print(line)
        lines.append(line)

    for k, n in sorted(untranslated.items()):
        print(f"\nNOT TRANSLATED: {k[0]} / {k[1]} / arm {k[2]} — {n} document(s) came back")
        print("  unchanged from the English intermediate. Excluded from M1: preserving")
        print("  the skeleton by not touching it is not preservation.")

    if pending_total:
        print(f"\nM4 IS NOT FINAL: {pending_total} decoy(s) await adjudication and are")
        print("counted as clean, so fabrication is understated. Adjudicate before")
        print("applying any decision rule that reads M4.")

    if args.report:
        args.report.write_text(
            "# Bake-off results\n\n"
            f"processor.rs: `{shas.pop() if shas else 'unknown'}`\n\n"
            "```\n"
            f"{'model':<14}{'group':<18}{'arm':<5}{'M1':>7}{'M2en':>7}{'M3':>7}{'M4':>7}{'M5':>7}{'M6':>7}{'M7':>7}\n"
            + "\n".join(lines) + "\n```\n\n"
            "Metric definitions and the decision rules are in "
            "[04-measurement.md](../04-measurement.md).\n",
            encoding="utf-8")
        print(f"\nreport -> {args.report}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
