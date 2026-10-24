# SPDX-License-Identifier: Apache-2.0
"""Specification text states byte semantics, not decoder behaviour or provenance.

CLAUDE.md: specs contain byte semantics and invariants only, state the settled
format model as fact without corpus, sample, experiment, or provenance
language, and keep implementation behaviour out. A sentence such as "the
decoder keeps the first binding" or "a reader selects the typed spelling"
describes code, not bytes. This check warns on such phrases in the lines a
branch adds to `docs/formats/*.md`, except the open-items documents, so a
person rereads them. Progress wording (not yet, do not yet, for now, currently,
TODO) warns in added lines of every format Markdown document, including open
items; it cannot judge the sentence itself.
"""

from __future__ import annotations

import re

PROGRESS = re.compile(r"\b(?:not yet|do not yet|for now|currently|TODO)\b", re.IGNORECASE)
FORMAT_DOC = re.compile(r"^docs/formats/[^/]+\.md$")

NAME = "spec-wording"
ORDER = 80

SPEC = re.compile(r"^docs/formats/(?!.*-open-items\.md$)[^/]+\.md$")
PHRASES = re.compile(
    r"\b(?:the decoder|decoders?|the reader|a reader|readers|the writer|writers?|encoders?|"
    r"cadmpeg|we|our|us|is chosen|chooses|picks?|falls? back|fallback|projection|loss note|"
    r"corpus|samples?|sampled files|observed|fixtures?|experiments?|so far|"
    r"in practice|the code|implementation|(?:do|does) not read|unread|reads? (?:it|them) as|earliest|first wins|wins)\b",
    re.IGNORECASE,
)


def flagged(lines: list[tuple[int, str]]) -> list[tuple[int, str, str]]:
    found = []
    for number, text in lines:
        if text.lstrip().startswith(("|", "```", "<!--")):
            continue
        prose = re.sub(r"`[^`]*`", "", text)
        matches = list(PHRASES.finditer(prose))
        if matches:
            words = sorted({match.group(0).lower() for match in matches})
            start = max(0, matches[0].start() - 60)
            excerpt = prose[start:matches[0].end() + 80].strip()
            prefix = "..." if start else ""
            suffix = "..." if matches[0].end() + 80 < len(prose) else ""
            found.append((number, ", ".join(words), prefix + excerpt + suffix))
    return found


def spec_wording(context):
    paths = [path for path in context.changed_paths() if SPEC.match(path)]
    if not paths:
        return context.passed("no specification text changed")
    details = []
    for path in paths:
        for number, words, excerpt in flagged(context.added_lines(path)):
            details.append(f"{path}:{number}: [{words}] {excerpt}")
    if not details:
        return context.passed(f"{len(paths)} specification(s): no behaviour or provenance wording")
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
        if FORMAT_DOC.match(path):
            for number, text in context.added_lines(path):
                if PROGRESS.search(text):
                    details.append(f"{path}:{number}: {text.strip()}")
    if details:
        results.append(context.warned("added format documentation uses progress wording", *details))
    wording = spec_wording(context)
    if wording.status != "PASS" or not results:
        results.append(wording)
    return results
