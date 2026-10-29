# STEP Open Items

This document lists the parts of STEP exchange formats that we do not know. The specification `step.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and token values are technical names. They keep their source spelling.

## 1. External resources

### ER-01. AP242 Edition 4 without a dialect row or schema variant

**Question.** Is AP242 Edition 4 an intended CADIR exchange grammar, and which
`FILE_SCHEMA` identifier selects it?

**Known.** `step.md` §6 lists the supported schema identifiers through AP242
Edition 3 (`{ 1 0 10303 442 4 1 4 }`).
`crates/cadmpeg-codec-step/src/options.rs:61-71` defines `StepSchema` through
`Ap242Edition3` with no Edition 4 variant, and
`crates/cadmpeg-codec-step/src/dialect.rs:232-240` maps only the Edition 1
through 3 object identifiers to rows. An Edition 4 declaration
(`{ 1 0 10303 442 7 1 4 }`) therefore classifies as `Unknown` and is read
with the nearest-strategy grammar. `step.md` §1 names the Edition 4 object
identifier as a Part 28 derivation input. `docs/evaluations.toml` declares no
Edition 4 dialect.

**Need.** An Edition 4 source is decoded with a grammar no row declares for
it. The answer either adds the Edition 4 row, schema variant, and evaluation,
or records Edition 4 as out of scope, marking future sources at the
nearest-strategy fallback.

### ER-02. `step:ap242-e3` as the weakest supported row

**Question.** Does an AP242 Edition 3 source decode to the Edition 3 grammar
it declares?

**Known.** `crates/cadmpeg-registry/docs/dialect-support.toml` records
`step:ap242-e3` as read `detected`: no fixture in
`crates/cadmpeg-codec-step/tests/fixtures/` declares the Edition 3 schema, and
no evaluation in `docs/evaluations.toml` declares `step:ap242-e3`. The write
cell is `verified` only through `explicit_ap242_e3` arms that export other
rows' sources to an Edition 3 target. Four encode goldens freeze those arms;
no golden freezes a decode of an Edition 3 source.

**Need.** Every other verified read row has a declaring fixture and an
evaluation. The answer adds an Edition 3 fixture, its golden, and its
evaluation, or lowers the row until that evidence exists so the matrix marks
it apart from verified rows.

### ER-03. Encode goldens cover 4 of 19 fixtures

**Question.** Do the unwitnessed export paths transfer every carrier they
admit?

**Known.** `crates/cadmpeg-codec-step/tests/fixtures/` holds 19 exchange
fixtures. `tests/golden/encode/` freezes 4 of them (`ap203_sheet`,
`ap214_sheet`, `ap242_geometry`, `ap242_minimal`); the remaining 15, including
the assembly, mapped-assembly, occurrence, presentation, PMI, tessellation,
geometric-set, and vertex-loop populations, have no frozen export artifact.
`docs/golden-coverage-floors.toml` records floors without mapping frozen
goldens to exercised export branches.

**Need.** An export drop or carrier change on an uncovered fixture alters
emitted files without breaking a golden. The answer records which export
branches lack a frozen witness and adds the missing goldens, or records the
coverage boundary so a passing suite reads as a bounded proof.

## 2. Geometry and curves

### GC-16. Invalid placement with a fabricated reference direction

**Question.** What geometry, if any, can an invalid `AXIS2_PLACEMENT_3D`
transfer when its supplied `ref_direction` is parallel or anti-parallel to its
`axis`?

**Known.** ISO 10303-42 `build_axes` uses the default projected axis only when
`ref_direction` is absent. `AXIS2_PLACEMENT_3D` WR4 rejects a supplied
reference direction that is parallel to the axis. `step.md` §8 instead defines
a salvage rule: the decoder reports `geometry.placement-reference-inferred`,
discards the supplied reference direction, and constructs the default
orthogonal reference. `crates/cadmpeg-codec-step/src/reader/geometry.rs:777-814`
implements that rule and inserts the resulting placement into the ordinary
placement map. Geometry and transformations that reference it then consume the
fabricated frame as their carrier.

**Need.** The source does not define the rotation about the placement axis, and
the STEP default for an absent attribute does not repair an invalid supplied
attribute. The answer either keeps geometry that depends on this placement on
the opaque path, or defines an explicit inferred-carrier representation that a
consumer cannot mistake for the placement declared by the source.

### GC-17. Substitution for a missing `TRIMMED_CURVE` master selector

**Question.** Can a `TRIMMED_CURVE` transfer when the selector form named by
`master_representation` is absent?

**Known.** A trim select can contain parameter values, Cartesian points, or
both, and `master_representation` identifies the preferred representation.
`crates/cadmpeg-codec-step/src/reader/geometry.rs:4133-4178` uses the requested
form when it is present. When it is absent, the decoder evaluates the other
form and uses that result as the trim endpoint. It reports the substitution in
a document warning, but the resulting neutral curve carries no per-curve
indication that its declared master selector was unavailable.

**Need.** The substituted selector can select a parameter only through the
decoder's geometric inversion and tolerance policy. The answer states whether
the other selector is authoritative in this malformed case. If it is a salvage
result, its status must remain attached to the transferred curve and not only
to a document warning.

### GC-18. Edge vertex off its carrier without a specification rule

**Question.** What does a vertex position state for a vertex off its edge's
carrier?

**Known.** `crates/cadmpeg-codec-step/src/reader/geometry.rs:195-232` measures
each edge vertex against its circular carrier. When the miss exceeds the
document accuracy, the decoder derives the arc parameter range from the
carrier's closed form, transfers the vertex as authored, loosens the edge
tolerance to cover the miss, and reports
`geometry.edge-vertex-off-carrier`. Which value is authoritative, and whether
the loosened tolerance counts as an edge-tolerance source, stays open.

**Need.** The transferred vertex and the transferred range disagree past the
document accuracy, with the loss note as the record of that fact. A consumer that
holds the edge to the loosened tolerance cannot separate a source-stated
tolerance from a decoder-widened one. The answer either admits the
decoder-widened tolerance as an edge-tolerance source with a queryable mark,
or refuses the disagreeing edge.

## 3. Containers and other encodings

## 4. Signatures

## 5. Topology and pcurve decisions

### TP-14. Per-relation status of an admitted pcurve

**Question.** Which channel gives the verification status of one admitted
pcurve relation?

**Known.** `crates/cadmpeg-codec-step/src/reader/topology.rs:3109-3113`
records each admitted relation,
`crates/cadmpeg-codec-step/src/reader/topology.rs:3518` attaches the collected
relations to the committed body,
`crates/cadmpeg-codec-step/src/reader/topology/admissions.rs:27-36` formats the
one document warning that names the first 8
(`PCURVE_UNPROVED_NOTE_EXEMPLARS`,
`crates/cadmpeg-codec-step/src/reader/topology/admissions.rs:13`), and
`crates/cadmpeg-codec-step/src/reader/topology.rs:710` reports it. That warning
is the only record of admitted relations. The exactness map (`crates/cadmpeg-ir/src/annotations.rs:20-35`) is
the per-entity trust channel, and it uses a globally unique entity identity as
its key. A pcurve relation is a `PcurveUse` field of a `Coedge`
(`crates/cadmpeg-ir/src/topology.rs:644-661`); the map keys entities, so one
relation stays keyless. `Exactness`
(`crates/cadmpeg-ir/src/provenance.rs:614-623`) holds `ByteExact`, `Derived`,
`Inferred`, and `Unknown`. An admitted relation transfers the source value as-is with its global
invariant unproved; `Exactness` offers no transferred-unproved value. `ModelDraft::exactness` (`crates/cadmpeg-ir/src/draft.rs:403-411`)
writes an entity value only. The STEP reader leaves exactness at default:
`crates/cadmpeg-codec-step/src/reader/mod.rs:341` builds
`SourceFidelity::default()`.

**Need.** A consumer that repairs or reviews one coedge must know whether its
pcurve relation holds an unproved invariant. The document warning gives a count
and 8 examples, so relations past those 8 stay unqueryable. The answer adds a
key for one pcurve use and a status value that means transferred and unproved.
Both changes are in `cadmpeg-ir` and reach each codec that writes exactness.

### TP-15. Export split of a non-manifold edge into use-pair edges

**Question.** Which emitted `EDGE_CURVE` carries one coedge use when a closed
shell uses its edge more than twice?

**Known.** `step.md` §8 states that recoverable non-manifold incidence remains
attached and is reported; that rule covers the decode graph, not the emitted
STEP text. The writer instead splits the edge:
`crates/cadmpeg-codec-step/src/export.rs:2815-2864` assigns the coedges of an
edge used more than twice to forward and reversed use pairs
(`crates/cadmpeg-codec-step/src/export/non_manifold.rs:14-43`, in
`radial_next` ring order when the ring walk visits exactly them, else in the
given order) and writes each pair as its own `EDGE_CURVE`. This split, its
pairing order, and the identity of the emitted copies are unspecified;
rewritten edges carry no loss mark.

**Need.** A consumer that compares the emitted file against the source graph
sees new edge identities with the rewrite unrecorded. The answer either
specifies the pairing and the copy identity as the exchange form for
non-manifold edges, or records the split on each emitted copy so the exchange
form stays traceable to its source edge.

### TP-16. Wire-shell free vertices without an export carrier

**Question.** Where does a wire-shell free vertex go when no STEP edge can
carry it?

**Known.** `crates/cadmpeg-codec-step/src/export.rs:2367-2377` reports
`topology.wire-shell-free-vertices` for each free vertex of an emitted wire
shell and writes only the shell's wire edges. `step.md` §8 gives the decode
ownership of wire shells. The export rule for vertices outside every emitted
edge is unspecified.

**Need.** The emitted shell loses vertices that the neutral shell retains,
with the omission recorded in the loss note. The answer either defines a STEP
carrier for free vertices or states that the wire export is edge-only, so a
consumer knows the emitted shell is not the complete neutral shell.

## 6. Units and measures

### UM-06. Unresolved uncertainty measure with a transferred projection

**Question.** Which channel reports a linear uncertainty measure that does not
resolve, when the document projection transfers a value?

**Known.** `crates/cadmpeg-codec-step/src/reader/geometry.rs:3907-3965` counts
each `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` measure that does not resolve. A
measure does not resolve when its record is absent, when it holds no number,
when it holds no unit reference, when its unit is not a length unit and not a
plane-angle unit, or when its scaled value is not finite or is not more than
zero. `crates/cadmpeg-codec-step/src/reader/geometry.rs:3983-4018` adds these
counts across all contexts and gives `LinearUncertainty::Value`, `Empty`, or
`Ambiguous`. Only `Empty` and `Ambiguous` keep the count.
`crates/cadmpeg-codec-step/src/reader/geometry.rs:550-576` reports
`geometry.uncertainty-length-unresolved` for `Empty`, and names the count in the
`geometry.uncertainty-length-ambiguous` note for `Ambiguous`. The `Value` arm
writes `ir.tolerances.linear` and reports nothing. `step.md` §8 "CADIR decision:
`Tolerances.linear` is the document projection of the" gives the same rule: it
asks for a note only when no candidate resolves.

**Need.** A measure that does not resolve is a candidate that cannot compete.
One distinct resolved value therefore does not show that the contexts agree. A
document that declares one value in one context, and a measure that does not
resolve in a second context, transfers the first value and keeps no record of
the second measure. `step.md` §8 "Geometric-consistency checks use the selected
document tolerance as their" makes `Tolerances.linear` the baseline of the
consistency checks, so a caller that repairs or examines the document must know
that a declared measure was dropped. The answer gives the count of measures that
do not resolve to the transferred-value result, or gives the reason that a
dropped measure needs no record.

## 7. Annotation, presentation, and tessellation

### AP-14. Override precedence for a complex styled item

**Question.** How must the reader find the attributes of an
`OVER_RIDING_STYLED_ITEM` that is a complex instance?

**Known.** `step.md` §8 "`OVER_RIDING_STYLED_ITEM` is the explicit precedence
relation: its style takes" gives the override precedence. In a complex instance,
each simple record holds the attributes of its own entity type only.
`OVER_RIDING_STYLED_ITEM` gives one attribute of its own, so its partial holds
one parameter. `crates/cadmpeg-codec-step/src/reader/presentation.rs:982-996`
reads the target from position `len - 2` and the style set from position
`len - 3` of that partial. Both positions are absent when the partial holds one
parameter, and the `?` operator then gives `None` for the full function, so the
`STYLED_ITEM` branch at
`crates/cadmpeg-codec-step/src/reader/presentation.rs:997-1010` does not run.
`crates/cadmpeg-codec-step/src/reader/presentation.rs:231-244` collects only the
records that give parts, and then makes the set of overridden records from that
collection. A complex `OVER_RIDING_STYLED_ITEM` is therefore absent from both
sets: its style does not apply, its `over_ridden_style` is not marked, and the
overridden style applies. No loss records this. The test
`complex_styled_item_decodes_color_and_owns_its_curve` in
`crates/cadmpeg-codec-step/src/reader/presentation/tests.rs` writes the
`OVER_RIDING_STYLED_ITEM` partial with three parameters, so it holds the
inherited attributes and does not exercise the one-parameter form.

**Conflict.** The specification gives precedence to the overriding style. For a
complex instance in the form that Part 21 gives, the decoder applies the
overridden style instead, and reports nothing.

**Need.** A color that a file overrides is a visible result. The answer reads
the `STYLED_ITEM` partial for the inherited style set and target, and reads the
`OVER_RIDING_STYLED_ITEM` partial for `over_ridden_style`. Add a witness that
writes each partial with its own attributes only, and that shows the overriding
color in the output.

### AP-15. Transparency value outside its permitted range

**Question.** What must the reader do with a `SURFACE_STYLE_TRANSPARENT` value
that is finite and outside `0..=1`?

**Known.** `step.md` §8 "CADIR decision: if a malformed rendering record contains
multiple finite" makes more than one finite value a conflict that omits
transparency and records `presentation.surface-transparency-conflict`.
`crates/cadmpeg-codec-step/src/reader/presentation.rs:1526-1549` admits
values that `Fraction` accepts, so a value outside `0..=1` stays out of the
candidates.
`crates/cadmpeg-codec-step/src/reader/presentation.rs:1551-1559` counts what
remains. A record with two finite values, of which one is outside the range,
therefore keeps one candidate and applies it, and no conflict loss is reported. A
record with one finite value outside the range keeps no candidate, so the
rendering becomes opaque, and no loss is reported. The specification records
the ISO 10303-46 §6.4.65 range (`0.0..1.0`) for this value; the out-of-range
decoder rule is unspecified.

**Need.** A dropped transparency gives an appearance that a consumer cannot
separate from a file that declares no transparency. The two rules also disagree:
the specification counts finite values, and the decoder counts values inside the
range. Give the range rule and its loss, or count each finite value as the
specification gives. Add a witness with one out-of-range value, and a witness
with one value inside the range and one value outside it.

### AP-16. Projection of distinct positive and negative surface styles

**Question.** How must CADIR retain a STEP surface whose positive and negative
sides have different appearances?

**Known.** ISO 10303-46 defines `.POSITIVE.`, `.NEGATIVE.`, and `.BOTH.` as
surface-side applicability. It defines no precedence between valid styles on
opposite sides. `step.md` §8 instead defines a scalar projection that ranks
`.BOTH.` before `.POSITIVE.` before `.NEGATIVE.` because one neutral surface
color cannot express two sided appearances.
`crates/cadmpeg-codec-step/src/reader/presentation.rs:1562-1595` and
`crates/cadmpeg-codec-step/src/reader/presentation.rs:1314-1322` define and
apply that rank. A lower-ranked valid appearance remains in retained source data and
appearance bindings, but the ordinary scalar surface color presents the
higher-ranked branch as the surface appearance.

**Need.** The ranking is an IR projection, not STEP format semantics. A consumer
that reads only the scalar appearance cannot distinguish a genuinely
single-style surface from a two-sided surface whose other appearance lost the
projection. The answer gives sided appearance a neutral representation, or
makes the lossy scalar projection and both source sides explicitly queryable as
one semantic result.

### AP-17. Datum system with inconsistent precedence compartments

**Question.** Which datum references survive when a `DATUM_SYSTEM` holds
compartments the decoder cannot order?

**Known.** `crates/cadmpeg-codec-step/src/reader/pmi.rs:175-244` decodes each
`DATUM_SYSTEM` record. When its datum references do not convert to the
expected compartment structure, the decoder omits the whole system and reports
`pmi.datum-system-invalid`. `step.md` §8 defines datum, datum-feature, and
datum-target transfer. The inconsistent-compartment rule and a
partial-transfer form for valid references are unspecified.

**Need.** The omission discards valid datum references together with the
invalid structure, with the dropped system recorded in the loss note. The
answer either defines which compartments are authoritative so the valid
references transfer, or specifies whole-system omission as the exchange form
for this malformed case.

### AP-18. Layer item without a writable STEP carrier on export

**Question.** Which neutral layer member does the writer drop when no STEP
record can carry it, and where is that decision recorded?

**Known.** `crates/cadmpeg-codec-step/src/export.rs:1270-1297` maps each layer
item to writable STEP references. Items with no carrier increment an
`unsupported` count; when that count is more than zero the writer reports
`layer.item-without-carrier` with the layer name and the dropped count, and
emits the assignment with only the carried items. `step.md` §8 defines the
decode carrier (`PRESENTATION_LAYER_ASSIGNMENT`) and the hidden-layer export
rules. A layer item without a writable STEP form awaits a rule.

**Need.** The emitted assignment is a subset of the neutral layer, with the
gap recorded in the loss note. A consumer that reads the emitted file reads
a reduced layer as source-small. The answer either
defines a carrier for each neutral member or specifies subset export with a
per-item record as the exchange form.

## 8. Product structure and placement

### PS-16. Occurrence placement inferred without a STEP occurrence association

**Question.** Can a parent representation's `MAPPED_ITEM` place a
`NEXT_ASSEMBLY_USAGE_OCCURRENCE` when no STEP relationship associates that
mapped item with the occurrence?

**Known.** The occurrence-specific mapped-item assembly form uses an additional
shape representation linked to the occurrence through
`SHAPE_DEFINITION_REPRESENTATION` and `PRODUCT_DEFINITION_SHAPE`.
`step.md` §8 also defines an implicit parent-representation salvage path and
states that it is not a STEP occurrence association.
`crates/cadmpeg-codec-step/src/reader/product.rs:1101-1163` searches the parent
definition's representations for mapped items whose mapped representation
resolves to the child definition. It assigns the transform when the child
definition has one matching usage and the search finds one distinct transform.
The uniqueness checks make the result independent of aggregate and record
order, but they do not create a source relationship between the mapped item and
the occurrence.

**Need.** The neutral occurrence receives a placement that the source graph does
not assign to it. The answer either requires an occurrence association before
the placement transfers, or represents the inferred association explicitly so
that it cannot be mistaken for a STEP-declared occurrence placement.
