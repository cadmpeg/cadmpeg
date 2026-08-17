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

### PH-03 — Generic trailing pointer boundary selection

**Question.** When a Parameter Data record has more than one structurally valid
trailing pointer-group boundary, what rule identifies the primary-parameter end?

**Known.** `parameter.rs` enumerates structural candidates in token order and
returns the first target-valid candidate. `native.rs` uses that boundary as the
primary-parameter end. `ParameterBoundaryAmbiguous` is emitted only for a
required back-pointer route when no candidate is selected. The test
`earliest_valid_trailing_pointer_group_boundary_wins` requires the earliest
valid candidate.

**Need.** A variable-length primary list must not be cut at a pointer-shaped
suffix merely because that suffix validates against the Directory. The decoder
must have an entity-specific discriminator, or retain the alternatives and
report ambiguity before assigning parameter and pointer ownership.

**Conflict.** IGES 5.3 §2.2.4.5.2 places association and property groups after
the primary parameters. It does not state that the earliest target-valid
structural candidate is the universal boundary rule.

**Note.** Commit `11321f89cdfb` closed this item by promoting the earliest-valid
candidate to the specification. A variable-length entity can contain a valid
pointer-shaped suffix before the actual trailing groups. The ordinary path then
cuts the primary fields, assigns them to association or property groups, and
emits no ambiguity loss. This is a partial closure.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

### DR-01 — Display-attribute pointer resolution

**Question.** When a Directory display field is a negative pointer, when may
the native display arena expose a typed definition identity?

**Known.** `native.rs` creates a `NativeDisplayAttributes` record for every
Directory entry and constructs `line_font_definition`, `level_definition`, and
`color_definition` identities directly from the signed fields. `graph.rs`
separately resolves the exact target types and emits `graph.pointer-unresolved`
for a missing or wrong target. The direct native projection does not use that
resolution result.

**Need.** A typed native identity must resolve to an existing definition of the
required type and form, or the field must remain raw and carry an attributed
loss. A consumer must not receive a dead typed link.

**Conflict.** The native arena can expose `iges:presentation:*#D<n>` for a
missing or wrong target while the reference graph says that the pointer is
unresolved. Native raw retention does not validate the typed identity.

**Note.** A Directory entry with `line_font = -99` and no Type 304 D99
produces a non-null typed native path with no corresponding line-font record.
The same failure exists for level and color. The broad closure in
`6f8556118971` did not cover this direct projection path. This is a partial
closure.

## 4. Geometry carriers and tolerances

## 5. Surfaces and topology

### TP-09 — Repeated BRep curve occurrence selection

**Question.** When several neutral edge occurrences use one curve carrier,
which occurrence may supply the Type 508 source-edge range?

**Known.** `entities/brep.rs` filters edge occurrences by curve identity and
endpoint agreement, then uses `.find(...)` to select the first match.
Type 508 projection copies that occurrence's parameter range into the emitted
edge. `iges.md` states that a candidate is selected only when exactly one
validated occurrence exists and that edge-list order is not a selection rule.

**Need.** Count all validated occurrences. Select one only when the match is
unique; otherwise preserve the ambiguity and refuse or classify the affected
projection without choosing by storage order.

**Conflict.** The implementation uses the first matching edge, while the
settled Type 508 rule requires exactly one validated occurrence.

**Note.** Commit `5acb47b1` and the later helper change in `091b7d121a` fixed
wrong-range filtering but did not add a uniqueness check. The existing test
covers one wrong range followed by one matching range, not two matching ranges.
A periodic or self-intersecting carrier can make both candidates pass, so edge
storage order changes the emitted topology. This is a partial closure.

## 6. Product structure, annotation, and presentation

## 7. Write path

### WR-01 — Type 143 representation across unclassified loops

**Question.** How does the writer select and validate the single Type 143
representation flag when an unclassified bounded face has several loops?

**Known.** `writer.rs` derives the Type 143 representation from the first loop's
first coedge and emits a Type 141 for every loop. Its validation checks p-curve
count consistency within each loop, but resets the comparison for the next
loop. The Type 143 reader validates every boundary against the one record-level
representation flag.

**Need.** All emitted Type 141 loops must agree with the Type 143 flag, or the
writer must reject the face or apply a documented normalization rule.

**Conflict.** The writer accepts a first model-only loop followed by a loop with
p-curves, emits representation `0`, and produces a second boundary that the
reader rejects as inconsistent. Reversing loop order changes the result.

**Note.** Commit `1b024e5af` fixed the outer/inner role marker for unclassified
BRep loops. It did not close the cross-loop Type 143 representation invariant.
This is a partial closure.

### WR-08 — Preservation of finite real values

**Question.** How must the writer spell finite binary64 values so that nonzero
values are not changed by serialization?

**Known.** `writer.rs::stabilize_real` maps every finite value with absolute
magnitude at most `cadmpeg_ir::compare::FLOAT_TOLERANCE` (`1e-12`) to zero and
pre-rounds other finite values through a twelve-decimal exponential string
before final formatting. The writer tests require near-zero collapse. `iges.md`
requires every nonzero finite real to be written with seventeen significant
decimal digits and to round-trip without flushing small nonzero values to zero.

**Need.** Serialization must preserve finite values, or the specification must
state and justify a separate writer quantization rule with its loss or refusal
behavior. A comparison tolerance must not silently become a write tolerance.

**Conflict.** A finite value such as `5e-13` is written as `0`, and other values
are pre-rounded before the seventeen-digit spelling. This disagrees with the
settled writer contract.

**Note.** The current behavior is present in the refactor identified by
`66419386cf82`. The earlier closure of WR-08 did not account for this
quantization path. Geometry coordinates, coefficients, and weights can change
without a loss or refusal. This is a spec-code disagreement.

### WR-12 — Missing ISOP metadata in Type 508 output

**Question.** What does the semantic BRep writer emit when
`PcurveUse.isoparametric` is absent?

**Known.** The IR defines `isoparametric: Option<bool>` and uses `None` when a
source does not declare the property. Type 508 requires an explicit `ISOP`
value. `writer.rs` emits `i32::from(pcurve_use.isoparametric.unwrap_or(false))`.
`validate_brep_pcurve_uses` checks p-curve identity, range, metadata, and
orientation, but not a missing ISOP value. Type 141 and Type 144 decoding can
produce `None`.

**Need.** The writer must derive or validate a false value, preserve the
unknown state through an allowed representation, or refuse/classify the
record. It must not convert absent metadata to an explicit assertion without a
documented rule.

**Conflict.** The IR distinguishes “not declared” from `false`, but the writer
silently emits `ISOP = 0` for both states.

**Note.** A neutral BRep with an absent ISOP declaration can therefore change
meaning on export. The current validation and Type 508 specification do not
define this substitution. This is a new hostile-sweep item.
