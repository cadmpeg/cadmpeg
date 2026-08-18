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

Type 402 Form 9 puts required `NP=1` at index 1, positive `NC` at index 2, the parent pointer at index 3, and `NC` child pointers at indexes 4 through `3 + NC` under §4.86, so its trailing groups begin at token `4 + NC`. The owner tests cover one and two children, a target-valid generic alternative, and malformed parent or child counts. The valid witness has one Type 116 parent, one Type 116 child, their back pointers to the source, and a trailing Type 212 association; the ambiguity witness reports two target-valid boundaries before registration and selects token 6 after registration.

Type 230 Form 0 puts `BNDP`, `PATRN`, `XT`, `YT`, `ZT`, `DIST`, `ANGLE`, and `N` at indexes 1 through 8 under §4.68. The `N` island pointers occupy indexes 9 through `8 + N`; Form 0 permits `N=0`, so trailing groups begin at token `9 + N`. The owner tests cover zero, one, and two islands, a target-valid generic alternative, and negative, overflowing, missing, or truncated island counts. The valid zero- and one-island witnesses resolve their Type 230 boundary and association groups; malformed witnesses retain the entity loss and suppress suffix recovery.

Type 320 Form 0 puts `NA` at index 3, followed by `NA` child pointers, `TF`, `PRD`, and `DPTR`; `NC` is at index `7 + NA`, followed by `NC` nullable Connect Point pointers through index `7 + NA + NC` under §4.78. Its last primary index is `7 + NA + NC`, so trailing groups begin at token `8 + NA + NC`. `NA=0` and `NC=0` are valid when their fixed fields and counted lists fit. Missing, negative, or list-truncated counts suppress generic recovery. The owner tests cover independent member and connect-point counts, a target-valid generic alternative, and malformed count/list data.

Type 184 Forms 0 and 1 put the positive item count `N` at index 1 under §4.48. The `N` item pointers occupy indexes 2 through `1 + N`, followed by `N` Transformation Matrix pointers at indexes `2 + N` through `1 + 2*N`; the first trailing group begins at token `2 + 2*N`. A zero transformation pointer is the identity matrix. Missing, nonpositive, or list-truncated `N` suppresses generic recovery. The owner tests cover both forms, multiple items, a target-valid generic alternative, and malformed count/list data.

Type 412 Form 0 puts `LC` at index 11, `DDF` at index 12, and `LC` DO-DON'T position numbers at indexes 13 through `12 + LC` under §4.136. Its first trailing group begins at token `13 + LC`; `LC=0` is valid. Missing, negative, wrong-typed, or list-truncated `LC` suppresses generic recovery. The owner tests cover zero through two positions, a target-valid generic alternative, and malformed count/list data.

Type 414 Form 0 puts `LC` at index 9, `DDF` at index 10, and `LC` DO-DON'T position numbers at indexes 11 through `10 + LC` under §4.137. Its first trailing group begins at token `11 + LC`; `LC=0` is valid. Missing, negative, wrong-typed, or list-truncated `LC` suppresses generic recovery. The owner tests cover zero through two positions, a target-valid generic alternative, and malformed count/list data.

Type 126 Forms 0 through 5 put `K` and `M` at indexes 1 and 2 and define `A = 1 + K + M` under §4.23. The last primary index is `16 + A + 4*K`, so groups begin at token `18 + 5*K + M`. `parameter.rs::entity_primary_end` applies this formula for nonnegative `K` and `M` with `K >= M`, including `M = 0`; missing, negative, or `K < M` values suppress generic recovery. The owner tests cover Forms 0, 3, and 5, the degree-zero boundary, invalid `K` and `M` cases, and a controlled Type 126 record whose token-7 suffix is also target-valid. The rebuilt witness selects token 24, resolves its Type 212 association pointer, and transfers the spline without a boundary loss.

Type 112 Form 0 puts the segment count `N` at index 4 under §4.14. The primary layout contains `N + 1` breakpoints, `12*N` polynomial coefficients, and a 12-value terminal block, so the last primary index is `17 + 13*N` and groups begin at token `18 + 13*N`. `parameter.rs::entity_primary_end` applies this formula for positive `N`; missing, nonpositive, or overflowing values suppress generic recovery. The owner tests cover `N = 1` and `N = 2`, malformed counts, a target-valid generic alternative, and a decode witness that resolves the Type 112 trailing association.

Type 114 Form 0 puts the u-segment count `M` and v-segment count `N` at indexes 3 and 4 under §4.15. The primary layout contains `M + 1` u breakpoints, `N + 1` v breakpoints, and `(M + 1)*(N + 1)` blocks of 48 values, including the placeholder row and column. The last primary index is `6 + M + N + 48*(M + 1)*(N + 1)` and groups begin at token `7 + M + N + 48*(M + 1)*(N + 1)`. `parameter.rs::entity_primary_end` applies this formula for positive dimensions; missing, nonpositive, or overflowing dimensions suppress generic recovery. The owner tests cover 1-by-1 and 2-by-1 grids, malformed dimensions, a target-valid generic alternative, and a decode witness that resolves the Type 114 trailing association.

Type 128 Forms 0 through 9 put `K1`, `K2`, `M1`, and `M2` at indexes 1 through 4 under §4.24. With `A = 1 + K1 + M1`, `B = 1 + K2 + M2`, and `C = (K1 + 1)*(K2 + 1)`, the primary layout contains `A + 1` and `B + 1` knots, `C` weights, `3*C` control-point values, and four range values. The last primary index is `15 + A + B + 4*C` and groups begin at token `16 + A + B + 4*C`. `parameter.rs::entity_primary_end` applies this formula for nonnegative indices with `K1 >= M1` and `K2 >= M2`; missing, incompatible, or overflowing values suppress generic recovery. The owner tests cover two index combinations, malformed indices, a target-valid generic alternative, and a decode witness that resolves the Type 128 trailing association.

Type 144 Form 0 puts the support-surface pointer, outer-boundary flag, inner-boundary count `N2`, and outer-boundary pointer at indexes 1 through 4 under §4.34. The `N2` inner-boundary pointers occupy indexes 5 through `4 + N2`, so the last primary index is `4 + N2` and groups begin at token `5 + N2`. `parameter.rs::entity_primary_end` applies this formula for nonnegative `N2`, including zero; missing, negative, or overflowing counts suppress generic recovery. The owner tests cover zero and one inner boundary, a target-valid generic alternative, malformed counts, and a valid Type 144 decode witness with a trailing association.

Type 143 Form 0 puts the representation flag, support-surface pointer, and boundary count `N` at indexes 1 through 3 under §4.33. The `N` Boundary Entity pointers occupy indexes 4 through `3 + N`, so the last primary index is `3 + N` and groups begin at token `4 + N`. `parameter.rs::entity_primary_end` applies this formula for nonnegative `N`, including zero; missing, negative, truncated, or overflowing counts suppress generic recovery. The owner tests cover zero through two boundaries, a target-valid generic alternative, and malformed counts; the valid Type 143 witness resolves its trailing association.

Type 141 Form 0 puts the positive model-curve count `N` at index 4 under §4.31. Item `i` starts at index `5 + 3*(i - 1) + K(1) + ... + K(i - 1)` and stores `CRVPT(i)`, `SENSE(i)`, `K(i)`, and `K(i)` parameter-curve pointers. The last primary index is `4 + 3*N + K(1) + ... + K(N)`, so groups begin at token `5 + 3*N + sum(K(i))`. `parameter.rs::entity_primary_end` validates positive `N`, nonnegative nested `K(i)`, and checked arithmetic before selecting this boundary; missing, nonpositive, negative, or overflowing counts suppress generic recovery. The owner tests cover zero and nonzero `K(i)`, multiple items, a target-valid generic alternative, and malformed outer and nested counts. The valid Type 141 witness resolves its trailing association.

The Type 320 boundary witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type320-boundary.igs` (SHA-256 `2be87a3bde120a92c086a6ef290d367603a5d30904de2d9940518473857c93e4`, source offset `0x41d`); it uses `NA=1`, `NC=1`, and resolves the trailing Type 212 association at token 10. The ambiguity witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type320-ambiguous.igs` (SHA-256 `4b22e7b0fb9bb13a0edf1e14647ae0d95626ea3f26a8db93168ee64348a0e503`, source offset `0x37b`); before registration it has two target-valid boundaries at tokens 9 and 10, and after registration token 10 resolves the Type 212 association. The negative-count and truncated-list witnesses are `f3b5e85b277ec8d5d897f95175434dbf6d33be1cefe37bb6e3e0b8b029155889` and `377d7ca123718cfac810da45bc48238a7e1ba2d47901741f8f34ca6e4a94be45`; after registration they do not infer a suffix.

The Type 320 Form 0 layout is settled; PH-03 remains open for the supported variable-width layouts not listed as settled above.

The Type 184 boundary witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type184-boundary.igs` (SHA-256 `bc97d78210b1e559884ebfbe0f04d662cf50e79d8ca6bad2b26e00a7b014fa20`, source offset `0x510`); it uses `N=2`, a zero and a nonzero transformation pointer, and resolves the trailing Type 212 association at token 6. The ambiguity witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type184-ambiguous.igs` (SHA-256 `be960b979804ca43f2823145e7a69124cde1a8ba60049bfbfdeef871a82840a9`, source offset `0x41d`); before registration it has target-valid boundaries at tokens 5 and 6, and after registration token 6 resolves D5→D7. The negative-count and zero-count witnesses have SHA-256 `8c61fac512497872538837ce01c2f0784d37622ffc75fe86e258a8787f4529ca` and `d270d35a3cca22805171985b11a7229a7b46a6b920e733c18dd0dd0369c140e4`; before registration they falsely link D3→D5, while registered decoding retains no association. The truncated transformation-list witness is `2df365a284d61d1a144370850f62914a7456f7108f5aa39c8a515a8fc07db758` at source offset `0x195` and retains no suffix.

The Type 412 boundary witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type412-boundary.igs` (SHA-256 `9a1f3b4ae76132046b37b497b92af49f818785e93853beb5576222f16a2a86cb`, source offset `0x32a`); it uses `LC=1`, selects token 14, resolves D3→D5, and checks with zero findings or losses after registration. The ambiguity witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type412-ambiguous.igs` (SHA-256 `c3e22a3c885e48bf27d55d13dc877c3d4995bacd1bb10e76d89ff659a65c7627`, source offset `0x288`); before registration tokens 13 and 14 are both target-valid boundaries, while after registration token 14 resolves D3→D1 and check has zero findings or losses. The negative-count and wrong-typed-count witnesses have SHA-256 `b677589a43549f4e16cf5b9c2bcb2c929643981c74c110cad84a6d840165d3dc` and `8e9375869b1b044edd3ab8945a53940d2a5fd1aa6ef56ee9b94cf8facbe1855e`, both at source offset `0x32a`; before registration they falsely resolve D3→D5, while registered decoding retains no association. The truncated-list witness is `f16d88a06f051f6781b7973b0878c633d3f6be201b8175bacbbc66ffe9866673` at source offset `0x288` and retains no suffix.

The Type 414 boundary witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type414-boundary.igs` (SHA-256 `7a328ea8969af2d9a34d9ba413bfc4351d33a7ebc85a46d8aa0ce6e6795a4402`, source offset `0x32a`); it uses `LC=1`, selects token 12, resolves D3→D5, and checks with zero findings or losses after registration. The ambiguity witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type414-ambiguous.igs` (SHA-256 `be725267134f29fe742d097e53c1378362f365e8726c8cdde9fb33c0d8cc6604`, source offset `0x288`); before registration tokens 11 and 12 are both target-valid boundaries, while after registration token 12 resolves D3→D1 and check has zero findings or losses. The negative-count and wrong-typed-count witnesses have SHA-256 `ada30eae2f1086340aef34ef0a31b527014d9662d0dba01e59b513526625af67` and `487c421df11e51cca231c7aacec0762102654dee0e7f06d02662fd590ecadfde`, both at source offset `0x32a`; before registration they falsely resolve D3→D5, while registered decoding retains no association. The truncated-list witness is `8e2f7e895842ac5ee411e76a87db0d87c7232e9b103355c2cd2706aad242cef8` at source offset `0x288` and retains no suffix.

The Type 184 Forms 0 and 1 layouts are settled; PH-03 remains open for the supported layouts not yet listed as settled.

**Need.** We need to trace and register the primary layout for every remaining supported variable-width entity form. Without it, a valid pointer group can become primary data, or a valid relationship can remain unassigned, when the candidate scan finds more than one target-valid suffix.

**Conflict.** The Parameter Data, counted-parameter, and Entity graph sections in `iges.md` now state that a proven entity-table boundary takes precedence and that unique-candidate recovery is only a CADIR fallback. The decoder implements this precedence for Type 102 Form 0, Type 106 supported forms, Type 110 Forms 0–2, Type 112 Form 0, Type 114 Form 0, Type 128 Forms 0 through 9, Type 141 Form 0, Type 143 Form 0, Type 144 Form 0, Type 116 Form 0, Type 123 Form 0, Type 126 Forms 0 through 5, Type 230 Form 0, Type 184 Forms 0 and 1, Type 320 Form 0, Type 412 Form 0, Type 414 Form 0, and Type 402 Forms 1, 7, 9, 14, and 15; the remaining supported layouts still use the generic fallback until their table rules are proven.

**Note.** The earlier Type 116 ambiguity fixture was malformed for the Type 116 table: its candidate bytes did not follow the required three-coordinate plus display-pointer prefix. The explicit-zero and empty-field Type 116/Type 402 Form 7 pair supplies the differential evidence for the fixed boundary. The Type 102/Type 402 Form 7 witness supplies a count-driven boundary with two primary child pointers. The Type 106 Form 11, 12, and 13 witnesses exercise the pair, triple, and sextuple formulas and resolve their trailing Type 402 association links. The Type 402 Form 1, 7, 14, and 15 witnesses exercise the `N + 2` boundary against a valid generic alternative and verify the ordered and back-pointer policies. The Type 402 Form 9 valid witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type402-form9-boundary.igs` (SHA-256 `c95b31b294d70d9e8153db0c0886709d0b92ef5674e9d2b5baa750ef7fcd9cd7`); it selects token 5 for `NC=1`, resolves the source's Type 212 association, and projects the single-parent relationship with zero check findings. The ambiguity witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type402-form9-ambiguous.igs` (SHA-256 `e1ab235f7c7bd81457d28159d7278991861ec74dc4f89f5bca9eff2ede01d8d4`); before registration it reports two target-valid boundaries at tokens 5 and 6, and after registration it selects token 6. The Type 230 zero-island witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type230-zero.igs` (SHA-256 `8cf05ab5b960dc092348f183aafd1b5eda16759185658180ef659ea44382e16a`, source offset `0x32a`); the one-island witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type230-one-island.igs` (SHA-256 `953abfdebcae0269aab43ed2626f4b80574cd6eca75708edc1e5358d51cabcbc`, source offset `0x41d`). Both check cleanly after registration. The ambiguity witness is `/home/pcurve/side2/tmp/iges-l9/ph03-type230-ambiguous.igs` (SHA-256 `d9225bc5ad8bcf1729a77162c515c88d8439e3c035869fe53d0b9e62ac50e134`); before registration it reports target-valid boundaries at tokens 9 and 10, and after registration token 10 wins. The negative-count witness is `6d984fc140bd0fd257b392386a5eea83458b55ed23155a67b23629548aa73eec`, and the wrong-island and truncated-list witnesses are `ca4a2f0d2f230fa3add3aed19031d940f99a823e5257ec9423c18f58fd0846fc` and `50c0d00a93dfb425ad67ee12ead7b05dce89fb9fa4080bebcde56f5e0cdbfdd9`; after registration they retain their Type 230 losses and no inferred suffix for malformed list/count data. The Type 126 witness exercises the `18 + 5*K + M` boundary against a target-valid token-7 alternative. The Type 112 witness exercises the `18 + 13*N` boundary against three target-valid generic alternatives. The Type 114 witness exercises the `7 + M + N + 48*(M + 1)*(N + 1)` boundary against two target-valid generic alternatives. The Type 128 witness exercises the `16 + A + B + 4*C` boundary at token 38 against two target-valid generic alternatives; the rebuilt decoder resolves its D5→D1 association. The Type 144 witness exercises the `5 + N2` boundary with one inner boundary and a trailing Type 212 association; the rebuilt decoder resolves D15→D17 and `check` reports zero errors and zero warnings. The Type 143 witness exercises the `4 + N` boundary with one Boundary Entity and a trailing Type 212 association; the rebuilt decoder resolves D11→D13 and `check` reports zero errors and zero warnings. The Type 141 witness exercises the nested `5 + 3*N + sum(K(i))` boundary with `N=1`, `K(1)=2`, and a trailing Type 212 association; the rebuilt decoder resolves D9→D13 and `check` reports zero errors and zero warnings. The Type 141 ambiguity witness reports two target-valid boundaries before registration and selects token 10 after registration. The Type 102, Type 106, Type 110, Type 112, Type 114, Type 126, Type 128, Type 141, Type 143, Type 144, Type 116, Type 123, Type 230, and Type 402 Forms 1, 7, 9, 14, and 15 layouts are settled; do not delete this item until the remaining supported layouts are covered.

The Type 320 Form 0 layout is also settled; PH-03 remains open for the supported layouts not yet listed as settled.

The Type 184 Forms 0 and 1 layouts are also settled; PH-03 remains open for the supported layouts not yet listed as settled.

The Type 412 Form 0 layout is also settled; PH-03 remains open for the supported layouts not yet listed as settled.

The Type 414 Form 0 layout is also settled; PH-03 remains open for the supported layouts not yet listed as settled.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

## 6. Product structure, annotation, and presentation

## 7. Write path
