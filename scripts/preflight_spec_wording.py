# SPDX-License-Identifier: Apache-2.0
"""Specification text states the settled model, not decoder behaviour or provenance.

CLAUDE.md: specs contain byte semantics and invariants only, state the settled
format model as fact without corpus, sample, experiment, or provenance
language, and keep implementation behaviour out. The specifications are the
format documents `docs/formats/*.md` and the CADIR specification the README
names: `docs/cad-ir.md` and the wire documents `docs/cadir-v<N>.md`.

In the lines a branch adds to a format specification, except the open-items
documents, this check warns on decoder behaviour ("the decoder keeps the first
binding", "a reader selects the typed spelling") and on provenance, so a person
rereads them. The CADIR specification states what decoders and writers do, so
its added lines warn on provenance only. Progress wording (not yet, do not yet,
for now, currently, TODO) warns in added lines of every specification and every
format open-items document; it cannot judge the sentence itself.
"""

from __future__ import annotations

import re

PROGRESS = re.compile(r"\b(?:not yet|do not yet|for now|currently|TODO)\b", re.IGNORECASE)
FORMAT_DOC = re.compile(r"^docs/formats/[^/]+\.md$")
FORMAT_SPEC = re.compile(r"^docs/formats/(?!.*-open-items\.md$)[^/]+\.md$")
CADIR_SPEC = re.compile(r"^docs/(?:cad-ir|cadir-v\d+)\.md$")

NAME = "spec-wording"
ORDER = 80

PROVENANCE = r"(?-i:[Ww]e|[Oo]ur|[Uu]s)|corpus|sampled files|observed|experiments?|so far|in practice"
PHRASES = re.compile(
    r"\b(?:the decoder|decoders?|the reader|a reader|readers|the writer|writers?|encoders?|"
    r"cadmpeg|is chosen|chooses|picks?|falls? back|fallback|projection|loss note|"
    r"samples?|fixtures?|the code|implementation|(?:do|does) not read|unread|"
    r"reads? (?:it|them) as|earliest|first wins|wins|" + PROVENANCE + r")\b",
    re.IGNORECASE,
)
CADIR_PHRASES = re.compile(r"\b(?:" + PROVENANCE + r")\b", re.IGNORECASE)


def flagged(lines: list[tuple[int, str]], phrases: re.Pattern[str]) -> list[tuple[int, str, str]]:
    found = []
    for number, text in lines:
        if text.lstrip().startswith(("|", "```", "<!--")):
            continue
        prose = re.sub(r"`[^`]*`", "", text)
        matches = list(phrases.finditer(prose))
        if matches:
            words = sorted({match.group(0).lower() for match in matches})
            start = max(0, matches[0].start() - 60)
            excerpt = prose[start:matches[0].end() + 80].strip()
            prefix = "..." if start else ""
            suffix = "..." if matches[0].end() + 80 < len(prose) else ""
            found.append((number, ", ".join(words), prefix + excerpt + suffix))
    return found


def spec_wording(context):
    details = []
    specs = 0
    for path in context.changed_paths():
        if FORMAT_SPEC.match(path):
            phrases = PHRASES
        elif CADIR_SPEC.match(path):
            phrases = CADIR_PHRASES
        else:
            continue
        specs += 1
        for number, words, excerpt in flagged(context.added_lines(path), phrases):
            details.append(f"{path}:{number}: [{words}] {excerpt}")
    if not specs:
        return context.passed("no specification text changed")
    if not details:
        return context.passed(f"{specs} specification(s): no behaviour or provenance wording")
    return context.warned(
        f"{len(details)} added spec line(s) may describe the decoder or provenance; reread them",
        *details[:25],
        "rule: state the byte condition; move decoder choices to code comments and unknowns "
        "to the open-items document",
    )


def run(context):
    results = []
    details = []
    for path in context.changed_paths():
        if FORMAT_DOC.match(path) or CADIR_SPEC.match(path):
            for number, text in context.added_lines(path):
                if PROGRESS.search(text):
                    details.append(f"{path}:{number}: {text.strip()}")
    if details:
        results.append(context.warned("added specification text uses progress wording", *details))
    wording = spec_wording(context)
    if wording.status != "PASS" or not results:
        results.append(wording)
    return results
