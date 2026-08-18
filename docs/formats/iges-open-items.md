# IGES open items

This document lists the parts of the IGES format that we do not know. The specification `iges.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Each item has an identifier. Use the identifier in commit messages and in code comments.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

Status requirements for resource-bounded decode, valid semantic output,
complete transfer accounting, semantic writing, target selection, independent
application acceptance, and writer stress are settled in the IGES
specification and support profile. They are not open format items.

# Unrecorded format rules

The items below record decode and write rules that the codec applies and that
neither IGES nor `iges.md` states. They come from a directed sweep of the codec
on 2026-08-08. Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now, with the code that depends on it.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and
  the code. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

Use the identifier in commit messages and in code comments. This document uses
ASD-STE100 Simplified Technical English. Record names, field names, and token
values are technical names. They keep their source spelling.

Many items have one shape: the codec refuses an entity because a value misses a
threshold that the codec selected, or because a field is blank where the codec
requires an explicit value. A refusal is not a safe default. It removes geometry
from a conformant file.

## 1. Physical framing and lexical rules

### PH-03. Entity-specific boundary for trailing pointer groups

**Question.** Which remaining supported entity-specific layouts must supply `NV`, the last primary Parameter Data index, before generic recovery is allowed?

**Known.** IGES 5.3 §2.2.4.5.2 places the two trailing pointer groups after all specified or defaulted entity parameters and defines `NV` as the last parameter number. The entity tables define the primary indexes. Type 102 Form 0 §4.4 puts `N` at index 1 and `N` constituent pointers at indexes 2 through `N + 1`; the selector uses token index `N + 2` and suppresses generic recovery when the count is malformed. Type 110 Forms 0–2 §4.13 list six real values at indexes 1 through 6; `parameter.rs::entity_primary_end` now selects token index 7 for those forms. Type 116 Form 0 §4.16 lists X, Y, Z, and PTR at indexes 1 through 4; §§2.2.1 and 2.2.3 make the preceding optional display-pointer slot explicit or defaulted when later groups are present, so the selector uses token index 5. Type 123 Form 0 §4.20 lists X, Y, and Z at indexes 1 through 3; the same selector uses token index 4. Synthetic Type 102/Type 402 Form 7, Type 110/Type 402 Form 7, Type 116/Type 402 Form 7, and Type 123/Type 402 Form 7 witnesses each contain a valid relationship group; the Type 116 pair differs only by explicit zero versus an empty pointer field. The rebuilt decoder assigns the table-selected Type 402 association links and emits no boundary-ambiguous loss for all four layouts. `native.rs:1497-1502` uses the selected result for `primary_end`; `native.rs:1525-1541` reports ambiguity only for generic layouts.

Type 106 Forms 1–3, 11–13, 20–21, 31–38, 40, and 63 use the form-required IP and N fields from §§4.6–4.11: IP 1 groups begin at token `4 + 2*N`, IP 2 at `3 + 3*N`, and IP 3 at `3 + 6*N`. An absent, invalid, or form-disagreeing IP suppresses generic recovery. The focused parameter tests construct Form 11, 12, and 13 records with Type 402 Form 7 association groups at the computed boundaries; rebuilt `inspect`, `dump`, and `check` runs report the expected tuple widths, two resolved references, zero findings, and zero losses for each form.

The Form 63 table repeats the generic IP descriptions, but its simple-closed-area application rule requires IP 1; the owner test `type106_form63_rejects_nonplanar_interpretation_for_boundary_recovery` preserves that restriction. The NIST simple-closed-area application protocol provides the independent rule.

Type 402 Forms 1, 7, 14, and 15 put `N` at Parameter index 1 and `N` member pointers at indexes 2 through `N + 1` under §§4.81, 4.85, 4.89, and 4.90, so their trailing groups begin at token `N + 2`. The owner tests construct all four forms with a second target-valid generic suffix; the selected boundary is token 4, and the decode witness resolves the trailing association pointer while preserving the form-specific ordered and back-pointer flags. A malformed member count suppresses generic recovery.

Type 126 Forms 0 through 5 put `K` and `M` at indexes 1 and 2 and define `A = 1 + K + M` under §4.23. The last primary index is `16 + A + 4*K`, so groups begin at token `18 + 5*K + M`. `parameter.rs::entity_primary_end` applies this formula for nonnegative `K` and `M` with `K >= M`, including `M = 0`; missing, negative, or `K < M` values suppress generic recovery. The owner tests cover Forms 0, 3, and 5, the degree-zero boundary, invalid `K` and `M` cases, and a controlled Type 126 record whose token-7 suffix is also target-valid. The rebuilt witness selects token 24, resolves its Type 212 association pointer, and transfers the spline without a boundary loss.

Type 112 Form 0 puts the segment count `N` at index 4 under §4.14. The primary layout contains `N + 1` breakpoints, `12*N` polynomial coefficients, and a 12-value terminal block, so the last primary index is `17 + 13*N` and groups begin at token `18 + 13*N`. `parameter.rs::entity_primary_end` applies this formula for positive `N`; missing, nonpositive, or overflowing values suppress generic recovery. The owner tests cover `N = 1` and `N = 2`, malformed counts, a target-valid generic alternative, and a decode witness that resolves the Type 112 trailing association.

Type 114 Form 0 puts the u-segment count `M` and v-segment count `N` at indexes 3 and 4 under §4.15. The primary layout contains `M + 1` u breakpoints, `N + 1` v breakpoints, and `(M + 1)*(N + 1)` blocks of 48 values, including the placeholder row and column. The last primary index is `6 + M + N + 48*(M + 1)*(N + 1)` and groups begin at token `7 + M + N + 48*(M + 1)*(N + 1)`. `parameter.rs::entity_primary_end` applies this formula for positive dimensions; missing, nonpositive, or overflowing dimensions suppress generic recovery. The owner tests cover 1-by-1 and 2-by-1 grids, malformed dimensions, a target-valid generic alternative, and a decode witness that resolves the Type 114 trailing association.

Type 128 Forms 0 through 9 put `K1`, `K2`, `M1`, and `M2` at indexes 1 through 4 under §4.24. With `A = 1 + K1 + M1`, `B = 1 + K2 + M2`, and `C = (K1 + 1)*(K2 + 1)`, the primary layout contains `A + 1` and `B + 1` knots, `C` weights, `3*C` control-point values, and four range values. The last primary index is `15 + A + B + 4*C` and groups begin at token `16 + A + B + 4*C`. `parameter.rs::entity_primary_end` applies this formula for nonnegative indices with `K1 >= M1` and `K2 >= M2`; missing, incompatible, or overflowing values suppress generic recovery. The owner tests cover two index combinations, malformed indices, a target-valid generic alternative, and a decode witness that resolves the Type 128 trailing association.

Type 144 Form 0 puts the support-surface pointer, outer-boundary flag, inner-boundary count `N2`, and outer-boundary pointer at indexes 1 through 4 under §4.34. The `N2` inner-boundary pointers occupy indexes 5 through `4 + N2`, so the last primary index is `4 + N2` and groups begin at token `5 + N2`. `parameter.rs::entity_primary_end` applies this formula for nonnegative `N2`, including zero; missing, negative, or overflowing counts suppress generic recovery. The owner tests cover zero and one inner boundary, a target-valid generic alternative, malformed counts, and a valid Type 144 decode witness with a trailing association.

Type 143 Form 0 puts the representation flag, support-surface pointer, and boundary count `N` at indexes 1 through 3 under §4.33. The `N` Boundary Entity pointers occupy indexes 4 through `3 + N`, so the last primary index is `3 + N` and groups begin at token `4 + N`. `parameter.rs::entity_primary_end` applies this formula for nonnegative `N`, including zero; missing, negative, truncated, or overflowing counts suppress generic recovery. The owner tests cover zero through two boundaries, a target-valid generic alternative, and malformed counts; the valid Type 143 witness resolves its trailing association.

Type 141 Form 0 puts the positive model-curve count `N` at index 4 under §4.31. Item `i` starts at index `5 + 3*(i - 1) + K(1) + ... + K(i - 1)` and stores `CRVPT(i)`, `SENSE(i)`, `K(i)`, and `K(i)` parameter-curve pointers. The last primary index is `4 + 3*N + K(1) + ... + K(N)`, so groups begin at token `5 + 3*N + sum(K(i))`. `parameter.rs::entity_primary_end` validates positive `N`, nonnegative nested `K(i)`, and checked arithmetic before selecting this boundary; missing, nonpositive, negative, or overflowing counts suppress generic recovery. The owner tests cover zero and nonzero `K(i)`, multiple items, a target-valid generic alternative, and malformed outer and nested counts. The valid Type 141 witness resolves its trailing association.

**Need.** We need to trace and register the primary layout for every remaining supported variable-width entity form. Without it, a valid pointer group can become primary data, or a valid relationship can remain unassigned, when the candidate scan finds more than one target-valid suffix.

**Conflict.** The Parameter Data, counted-parameter, and Entity graph sections in `iges.md` now state that a proven entity-table boundary takes precedence and that unique-candidate recovery is only a CADIR fallback. The decoder implements this precedence for Type 102 Form 0, Type 106 supported forms, Type 110 Forms 0–2, Type 112 Form 0, Type 114 Form 0, Type 128 Forms 0 through 9, Type 141 Form 0, Type 143 Form 0, Type 144 Form 0, Type 116 Form 0, Type 123 Form 0, Type 126 Forms 0 through 5, and Type 402 Forms 1, 7, 14, and 15; the remaining supported layouts still use the generic fallback until their table rules are proven.

**Note.** The earlier Type 116 ambiguity fixture was malformed for the Type 116 table: its candidate bytes did not follow the required three-coordinate plus display-pointer prefix. The explicit-zero and empty-field Type 116/Type 402 Form 7 pair supplies the differential evidence for the fixed boundary. The Type 102/Type 402 Form 7 witness supplies a count-driven boundary with two primary child pointers. The Type 106 Form 11, 12, and 13 witnesses exercise the pair, triple, and sextuple formulas and resolve their trailing Type 402 association links. The Type 402 Form 1, 7, 14, and 15 witnesses exercise the `N + 2` boundary against a valid generic alternative and verify the ordered and back-pointer policies. The Type 126 witness exercises the `18 + 5*K + M` boundary against a target-valid token-7 alternative. The Type 112 witness exercises the `18 + 13*N` boundary against three target-valid generic alternatives. The Type 114 witness exercises the `7 + M + N + 48*(M + 1)*(N + 1)` boundary against two target-valid generic alternatives. The Type 128 witness exercises the `16 + A + B + 4*C` boundary at token 38 against two target-valid generic alternatives; the rebuilt decoder resolves its D5→D1 association. The Type 144 witness exercises the `5 + N2` boundary with one inner boundary and a trailing Type 212 association; the rebuilt decoder resolves D15→D17 and `check` reports zero errors and zero warnings. The Type 143 witness exercises the `4 + N` boundary with one Boundary Entity and a trailing Type 212 association; the rebuilt decoder resolves D11→D13 and `check` reports zero errors and zero warnings. The Type 141 witness exercises the nested `5 + 3*N + sum(K(i))` boundary with `N=1`, `K(1)=2`, and a trailing Type 212 association; the rebuilt decoder resolves D9→D13 and `check` reports zero errors and zero warnings. The Type 141 ambiguity witness reports two target-valid boundaries before registration and selects token 10 after registration. The Type 102, Type 106, Type 110, Type 112, Type 114, Type 128, Type 141, Type 143, Type 144, Type 116, Type 123, Type 126, and Type 402 layouts are settled; do not delete this item until the remaining supported layouts are covered.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
