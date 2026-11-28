# SPDX-License-Identifier: Apache-2.0
"""Warn about provenance and progress wording in specifications.

Format specifications and CADIR documents state the format model as fact.
Added specification lines are checked for provenance terms. Progress terms are
also checked in format specifications and open-items documents.
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
PHRASES = re.compile(r"\b(?:" + PROVENANCE + r")\b", re.IGNORECASE)


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
        if not (FORMAT_SPEC.match(path) or CADIR_SPEC.match(path)):
            continue
        specs += 1
        for number, words, excerpt in flagged(context.added_lines(path), PHRASES):
            details.append(f"{path}:{number}: [{words}] {excerpt}")
    if not specs:
        return context.passed("no specification text changed")
    if not details:
        return context.passed(f"{specs} specification(s): no provenance wording")
    return context.warned(
        f"{len(details)} added spec line(s) may describe provenance; reread them",
        *details[:25],
        "rule: state the byte condition; move unknowns "
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
