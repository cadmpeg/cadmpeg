# IGES open items

IGES L9 is not achieved. The current score is L8 tested. The bounded semantic
writer and its independent-application checks are extras above L8; they do not
close the L9 gate while decode can time out, return invalid `CadIr`, or omit
semantic records from transfer.

## P0 — Make decode terminating and resource-bounded

Fixed ASCII decode has exceeded the 30-second per-file guard on multiple
inputs. This is pathological and unacceptable for a production codec. The
decoder must not spend unbounded time in parameter assembly, reference graph
construction, topology projection, or geometric carrier recovery.

Required closure:

- instrument each decode stage and record the dominant cost for a reduced
  reproducer;
- bound every file-declared count, recursive traversal, graph walk, and
  geometry-recovery search with the service resource policy;
- return a deterministic structured resource error when a bound is exceeded;
- add synthesized regression fixtures for each pathological stage; and
- run the bounded full-file gate in CI so a timeout cannot be reported as a
  successful decode.

The item is closed only when every file in the declared envelope reaches a
terminal success or a bounded, classified error within the agreed limit.

## P0 — Decode success must imply valid `CadIr`

The decoder can return success for documents that `cadmpeg validate` rejects.
Observed failures include edge parameter ranges outside their canonical curve
domains and edge curve endpoints that do not meet their vertex positions.

Required closure:

- canonicalize or reject carrier domains before committing edges;
- validate edge endpoints, pcurves, topology ownership, and transforms before
  returning decode success;
- commit no partial topology after a failed validation; and
- add synthesized fixtures for each failure class and run decode followed by
  `cadmpeg validate` in the regression gate.

The item is closed only when a successful semantic decode is a valid `CadIr`,
not merely a parseable command result.

## P0 — Account for every omitted semantic record

Successful decodes still produce `record_not_typed` and
`material_not_transferred` losses for trimming, display, and other entity
branches. The read profile must not call these branches complete while the
decoder either drops their semantics or cannot prove their preservation.

Required closure:

- assign every unsupported or omitted semantic construct a stable loss code,
  severity, source identity, and retained native record;
- distinguish deliberate native preservation from geometric projection loss;
- make `--strict` reject all losses that can change model, topology, product,
  or document meaning; and
- update the read profile only after loss coverage and validation pass.

## P0 — Re-establish the L9 gate

L9 remains open until bounded decode, valid-IR output, complete loss accounting,
semantic writing, target-version selection, and independent application
acceptance pass together. The bounded writer tests are not evidence that the
full declared read/write envelope passes this gate.

Required closure:

- run decode, validate, convert, and generated-file re-decode as one evaluated
  gate;
- require independent native-application acceptance for every writable
  profile, including edited and source-less documents; and
- keep the support table and codec README at L8 until this gate passes.

## P1 — Exercise the writer under fuzzing and continuous stress

The current IGES fuzz target exercises container detection, inspection, and
decode. It does not exercise semantic planning, target-version emission, or
writer rejection paths.

Required closure:

- add writer fuzz coverage for valid and malformed `CadIr` values;
- cover replay, source-less synthesis, target versions, topology, loss
  rejection, and unsupported native arenas;
- record a reproducible fuzz campaign and retain minimized regressions; and
- run the timeout and validation gates continuously rather than as an
  environment-only check.

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

## 3. Directory fields, the reference graph, and the native arenas

## 4. Geometry carriers and tolerances

### GE-01. The Type 124 transformation tolerance

**Question.** What round-off does a conformant Type 124 linear part have?

**Known.** IGES requires the Form 0 and Form 1 linear part to be orthonormal and states no numeric slack. `geometry.rs:16` gives one absolute constant for orthonormality, column orthogonality, the determinant, and circular preservation:

```rust
const TRANSFORM_TOLERANCE: f64 = 1.0e-10;
```

The constant demands about ten correct significant digits in each element. The Global section declares the file's own precision (`iges.md` "Global section") and the test ignores it. A failure gives `Err` from `resolve_transform`, so each entity that refers to the matrix is dropped (`geometry.rs:343-346`).

**Note.** Commit `f20d17e65` set this constant and authored its witness fixture in the same commit. The fixture perturbs one element by 5e-11, which is inside the threshold that the fixture is cited to justify. It cannot show that the threshold fits producer output.

**Need.** A writer that prints direction cosines to seven significant digits gives a column norm error near 1e-8 and loses every placed entity in the file. We need the round-off that producers give, against the file's declared precision.

### GE-02. Unit-vector acceptance

**Question.** What norm error does a conformant IGES unit vector have?

**Known.** Four sites refuse an entity when a declared unit vector misses unit length by more than 1e-10: `csg.rs:29-35` (every supplied primitive axis, and the Type 162/164 sweep axis at `csg.rs:267`), `surfaces.rs:1064` (the Type 140 offset indicator), and `offsets.rs:110-123` (the Type 130 plane normal). None derives the threshold from the declared precision.

**Note.** `offsets.rs:110-123` normalizes the vector and then refuses on the raw norm, so the test adds no numerical guarantee. `surfaces.rs:1064` refuses on a magnitude that is then used for its sign only (`surfaces.rs:230`).

**Need.** A normal printed as `0.5773503,0.5773503,0.5773503` has a norm error of 6e-8 and loses its entity. We need the norm budget for a declared unit vector.

### GE-03. Type 112 segment continuity

**Question.** What join residual does a conformant Type 112 parametric spline have?

**Known.** `splines.rs:28-30` gives a relative 1e-10 test and `splines.rs:273-289` makes a failure fatal to the entity. A Type 112 segment is a power-basis cubic evaluated as `a + b·w + c·w² + d·w³`, so coefficient round-off is multiplied by `w³`, where `w` is the breakpoint width. The comparison scale is the coordinate magnitude and not the breakpoint width.

**Note.** The terminal-derivative test at `splines.rs:316-325` uses the same helper and only warns. The codec accepts this residual class in one place and refuses the entity for it in another.

**Need.** Files whose only free-form curve type is Type 112 lose their complete curve content. We need the continuity budget as a function of the breakpoint width and the declared precision.

### GE-04. Type 130 unused parameter fields

**Question.** May a producer leave the unused Type 130 fields blank?

**Known.** `offsets.rs:186-198` requires each unused field to be an explicit integer `0` or real `0.0`. `parameter.rs:54-60` gives `None` for an omitted field, so a blank unused field fails. The loss text is "uniform offset has a nonzero unused field", which states the opposite of the condition that occurred. `integer()` also gives `None` for a real token, so an unused pointer field written as `0.` fails.

**Need.** The existence of `TokenValue::Omitted` is the codec's own evidence that a producer may leave a field blank. We need the rule for the unused fields of Type 130, and a message that names the condition.

### GE-05. Type 102 carrier concatenation uses a private tolerance and degrades silently

**Question.** Which tolerance joins Type 102 composite children, and what does a failed join mean?

**Known.** A tolerance-aware entry point exists, and `trimming.rs:111` gives it `global.minimum_resolution_mm()`. `composite.rs:475-478` gives `None`, so the join uses the relative 1e-10 constant in `close()` (`composite.rs:500-513`). The codec's own test `tolerance_allows_a_bounded_carrier_join_within_resolution` (`composite.rs:958-1023`) shows the same composite fails with `None` and passes with `Some(0.001)`.

**Conflict.** A failed join is not a loss. Five sites (`composite.rs:692`, `:709`, `:722`, `:737`, `:752`) fall back to `project_native_composite`, add the edge, insert the entity in `decoded`, and continue. The fallback edge carries `param_range: None` and `Discontinuous` joins, so the report shows complete coverage while the IR states that a closed producer profile is a set of disconnected segments. A genuinely non-concatenable composite and a valid one that misses a 1e-10 join give the same output.

**Need.** We need the join tolerance for Type 102, and a loss for every degraded carrier.

### GE-07. The curve parameter-domain convention

**Question.** Which parameter domain does each IGES curve type give to its neutral edge?

**Known.** The codec holds three conventions for one entity type. A standalone Type 110 edge gets `[0.0, length]` (`geometry.rs:605`). The same Type 110 as a composite child is given `[0.0, 1.0]` (`composite.rs:420-435`) after the code reads and discards `edge.param_range`. `offsets.rs:156-168` compensates for the `[0, length]` convention for lines only and asserts an identity mapping for everything else. An arc edge carries `[0, angle]` in radians (`geometry.rs:446`) and a Type 106 path carries `[0, n-1]` in point indices (`copious.rs:232`).

**Note.** `within_source_domain` (`offsets.rs:171-176`) accepts an interval because its numbers lie inside the domain, and not because a field declares the convention. A Type 130 of a full circle whose SPARM and EPARM are 0.0 and 1.0 in a normalized native parameter gives an edge that covers one radian, with no loss.

**Need.** We need the native parameter domain of each admitted curve type, and one recorded mapping to the IR domain.

### GE-08. Type 106 duplicate points and closure

**Question.** May a Type 106 linear path repeat a point, and which tolerance closes a Form 63 path?

**Known.** `copious.rs:221-231` refuses the complete entity when any two consecutive points are inside the relative 1e-10 test. `copious.rs:39-52` tests Form 63 closure with the same constant, where Type 100 uses the declared minimum resolution for the same class of comparison (`geometry.rs:379`).

**Need.** A digitized profile of 40000 points that repeats one sample loses the complete profile. A Form 63 boundary whose ends differ by 1e-7 mm inside a declared resolution of 1e-5 mm is refused as open. We need the rule for repeated tuples and the closure tolerance.

### GE-09. Type 104 endpoints are not tested against the conic

**Question.** Must a Type 104 start and terminate point lie on its conic?

**Known.** `conics.rs:227-233` gets the parameter of an endpoint through `atan2` of the two normalized in-plane projections, so the radial component is discarded and a point at twice the true radius gives the same parameter. `add_bounded_curve` stores the raw file coordinates as the edge vertices while the curve is the ideal conic, with `tolerance: None`.

**Note.** Type 100 does test this agreement (`geometry.rs:369-389`). The codec treats endpoint consistency as decode-relevant for arcs and not for conics.

**Need.** The output is an edge whose start vertex is off its own curve, with no loss and no recorded tolerance. We need the endpoint agreement rule for Type 104.

### GE-10. Angular equality constants

**Question.** Which angular difference makes a start and terminate point coincident?

**Known.** A coincident start and terminate means a complete turn, which is an IGES rule. The width of the test is not. `geometry.rs:394-399` and `conics.rs:235-239` use `1.0e-14`; `conics.rs:243-246` uses `ANGULAR_TOLERANCE`, which is 6.3e-12, inside the same function. `surfaces.rs:756` uses `1.0e-12` for the revolution periodicity flag while `surfaces.rs:109-115` clamps a sweep with `ANGULAR_TOLERANCE`, so a sweep between the two constants gives a closed revolution flagged non-periodic. The test at `surfaces.rs:1168-1174` constructs a sweep in exactly that band.

**Note.** `curve_conversion.rs:26-38` documents its constant at the definition and defends it against the discontinuity it guards. The other constants have no such record.

**Need.** We need one angular equality rule.

### GE-12. Type 126 property flags against the values

**Question.** What does a PROP3 polynomial flag that disagrees with the weights mean, and what slack does the declared parameter range have?

**Known.** `geometry.rs:704-710` tests weight equality with exact `f64` equality, so weights printed as `1.` and `0.9999999999` under PROP3 = 1 lose the curve. `geometry.rs:730-739` compares the declared range with the knot domain with no slack, so a V(0) printed to fewer digits than T(M) loses the curve instead of clamping.

**Need.** We need the authority order between the property flags and the values, and the slack for the declared range.

### GE-13. The unreachable Type 104 parabola branch

**Question.** May a Type 104 parabola open along the XT axis?

**Known.** `conics.rs:160-166` refuses the entity unless coefficients B and D are both zero. `conics.rs:343-373` holds a classification arm that requires `!zero(coeff_d)`, which the earlier gate has made impossible, so about thirty lines cannot execute. A conic written as `C·y² + D·x = 0` is refused with a message about standard position, and the code written to recover it never runs.

**Need.** We need the standard-position requirement for Type 104. The unreachable arm is then removed or its gate is corrected.

### GE-14. Type 106 interpretation flag against the form

**Question.** Must the Type 106 interpretation flag agree with the entity form?

**Known.** `copious.rs:26-33` and `:99-105` refuse the entity when the flag does not match the form-derived expectation. The interpretation flag alone gives the tuple arity and is sufficient to read the record. The form is a usage classification.

**Need.** We need to know whether IGES requires the pairing before a disagreement is fatal.

## 5. Surfaces and topology

### TP-01. The Global minimum resolution serves five unrelated roles

**Question.** Which of the trimming tolerances does the Global minimum resolution govern?

**Known.** IGES gives Global field 19 as the smallest distance the sender considers meaningful, which is a granularity floor. `trimming.rs` uses one value for five purposes: pcurve and model-curve endpoint agreement (`:601`), loop ring closure (`:642`), the vertex merge radius (`:660-675`), the stored `Edge` and `Face` tolerance (`:682`, `:751`), and the NURBS fit tolerance given to the carrier converter and recorded on every pcurve (`:584`, `:698`).

**Note.** Only the endpoint agreement use is anchored in `iges.md` "Topology". Ring closure, vertex merge, and fit tolerance have no anchor. A granularity floor used as a maximum permitted error is the strictest available reading.

**Need.** A Type 141 boundary of 24 curves exported at seven significant digits near 1000 mm has adjacent endpoint gaps near 1e-4 mm. With a declared resolution of 1e-5 mm the ring-closure test fails and the complete trimmed surface is dropped. Commercial importers give this a separate sewing tolerance. We need the tolerance channel for each of the five roles.

### TP-02. Type 141 and Type 142 `PREF` is validated and discarded

**Question.** What does the preferred-representation flag change?

**Known.** `trimming.rs:208-214` range-checks `CRTN` and `PREF` and stores neither. `trimming.rs:257-268` sets `require_carrier_agreement: pcurve.is_some()`, so the only input is whether a pcurve pointer exists. `PREF = 1` and `PREF = 2` behave identically. Type 141's `PREF` is likewise validated and dropped (`:287-290`).

**Note.** `PREF` exists because the two representations are not expected to be interchangeable. A file exported with `PREF = 2` and a coarse parameter-space approximation is a normal export choice; the codec refuses every boundary segment and drops the trimmed surface instead of using the representation the sender declared authoritative.

**Need.** We need the agreement rule as a function of `PREF`, or evidence that IGES requires agreement independent of it.

### TP-03. Declared surface parameter subranges are discarded with no loss

**Question.** What does a declared parameter subrange on a surface entity mean?

**Known.** `surfaces.rs:492` discards the Type 118 rail intervals with `let _ = (first_interval, second_interval, developable_flag);`. `surfaces.rs:979-996` validates the Type 128 `U(0), U(1), V(0), V(1)` and then never uses them. The Type 122 directrix interval survives only in the procedural record (`:598`) while the surface at `:576-591` spans the full basis, and the Type 120 generatrix interval is dropped at `:749`. The emitted `NurbsSurface` carries knot vectors only and the IR type has no domain field.

**Note.** The Type 126 projector explicitly admits a proper subrange (`geometry.rs:725-733`), so the codec is not consistent about whether a declared subrange is data.

**Note.** `surfaces.rs:983` holds an undocumented producer-compatibility path that accepts a permuted range order (`alternate_ranges`). Because the values are then discarded, the guard can only refuse: a Type 128 whose `U(0)` sits one ulp below `u_knots[u_degree]` loses the complete surface.

**Need.** A Type 122 whose directrix declares `V(0)=0.2, V(1)=0.8` inside a `[0,1]` domain gives a sheet 66 percent longer than the file declares, silently. We need the retention rule for a declared subrange, and a loss when it is dropped.

### TP-04. The Type 140 offset sign uses a per-kind representative normal

**Question.** At which location is the Type 140 offset indicator compared with the surface normal?

**Known.** `surfaces.rs:154-167` uses the surface `ref_direction` as the representative normal for a cylinder, sphere, torus, and cone, and `surfaces.rs:230-234` selects `radius + distance` or `radius - distance` from the sign of the dot product. For a Form 0 analytic surface, or a Form 1 whose reference-direction pointer does not resolve, that frame was invented by the codec (`analytic_surfaces.rs:75`, `derive_reference_direction`).

**Note.** For a full cylinder any fixed indicator agrees with the true normal on one half of the surface only.

**Need.** A Type 140 offsetting a Type 192 Form 0 cylinder of radius 10 mm outward by 2 mm can decode as radius 8 mm, with no loss. We need the location the format designates for the comparison.

### TP-05. A Form 1 reference direction that fails to resolve is treated as absent

**Question.** What does a Form 1 analytic surface whose declared reference direction does not resolve mean?

**Known.** `analytic_surfaces.rs:192-203`, and the same shape at `:155-166`, `:236-247`, `:280-291`, `:324-335`, collapse every failure of the declared pointer — omitted field, even sequence, dangling target, wrong type, non-numeric Type 123 parameters, unresolvable transform — into `None`, which `reference_direction` (`:72-77`) then treats as "the direction was absent" and replaces with a derived frame. The surface is marked `decoded` and no loss is recorded.

**Conflict.** `iges.md` "Geometry" sanctions a derived frame for a missing reference direction. A Form 1 record that declares one and fails to deliver it is a different condition.

**Note.** The downstream pcurve agreement tests compose the pcurve with the surface, so a wrong seam cannot reach a trimmed face. The result is a loss reported against the Type 144 rather than the surface, which sends the investigation to the wrong entity.

**Need.** We need the two conditions separated, and the loss attributed to the entity that holds the defect.

### TP-06. Type 180 Form 1 requires a direct Type 186 operand

**Question.** Does "contains a Manifold Solid B-rep operand" apply to direct operands or to the complete subtree?

**Known.** `csg.rs:370-389` tests the direct operands only and compares with strict equality in both directions, so `(entry.form == 1) != has_brep` refuses the tree. `iges.md` "Primitive solids" repeats the ambiguity of the standard text ("Form 1 contains at least one such operand").

**Note.** A tree that unions two Type 430 Form 1 instances, each referencing a Type 186 (`iges.md` "Product structure"), holds B-rep content with no direct Type 186 operand. The complete tree is refused.

**Need.** We need the scope of the Form 1 rule.

### TP-07. Type 144 with a zero outer-boundary flag requires a literal zero pointer

**Question.** May the Type 144 outer pointer field be omitted when the outer-boundary flag is zero?

**Known.** `trimming.rs:432-438` refuses the entity when `record.integer(4) != Some(0)`. An omitted field gives `None`, which is not `Some(0)`, so `144,PTS,0,1,,PT1;` is refused with the message "trimmed-surface parameter-domain outer boundary has a nonzero pointer", which names a pointer that is not present.

**Need.** We need the rule for an omitted `PTO`, and a message that names the condition that occurred.

### TP-08. Independent Type 141 and Type 142 entities are reported as losses

**Question.** Must every boundary entity be consumed by a trimmed surface?

**Known.** `trimming.rs:786-796` emits a loss for every Type 141 and Type 142 that no projected trimmed surface consumed. A Type 142 Curve on a Parametric Surface is a legal independent entity. The same loop also emits a second loss for boundaries whose owning Type 143 or Type 144 already recorded one.

**Need.** The loss count includes entities that decoded correctly. We need the rule for an unconsumed boundary entity.

## 6. Product structure, annotation, and presentation

### PS-01. Parameter defaults are honored at selected token indices only

**Question.** Which parameter fields admit an omitted value that takes the field default?

**Known.** IGES gives a uniform rule: an empty field takes its default, and defaulted trailing parameters may be omitted. The codec applies it at about fifteen hand-picked indices through `integer_or` and `number_or` (`structure.rs:209-223`, `drawing.rs:100-103`, `:139-144`, `presentation.rs:149`, `:224`, `:247`, `annotation.rs:584-596`) and requires an explicit token everywhere else.

**Conflict.** Type 212 and Type 312 carry the identical field sequence `WT, HT, FC, SL, A, M, VH, X, Y, Z` (compare `annotation.rs:47-62` with `presentation.rs:240-255`). Type 312 defaults the font code to 1 and the slant to `π/2`; Type 212 requires both to be present. Both behaviors cannot be right, and `iges.md` "Views and drawings" record neither.

**Note.** A view written `410,1,1.,,,,,,;` — six explicitly defaulted null clipping planes, which is how a producer writes "no clipping" — is refused at `drawing.rs:167`. A note written with an empty font code costs the complete owning dimension, because `annotation.rs:244` requires the child to be valid.

**Need.** We need the default column for each admitted entity, and one uniform treatment of an omitted field.

### PS-02. The same text-box metric has two different bounds

**Question.** May a character-box width or height be zero?

**Known.** `annotation.rs:47-51` requires the Type 212 box width and height to be finite and nonnegative. `presentation.rs:241-245` requires the Type 312 values to be strictly positive. `annotation.rs:90-104` requires the Type 213 values to be strictly positive. `iges.md` "Views and drawings" states "nonnegative box width and height" and `iges.md` "Views and drawings" states "positive character-box width and height" for the same physical quantity.

**Need.** One of the two bounds is a guess. We need the bound for each entity.

### PS-03. The perspective-view up-vector test uses an absolute epsilon

**Question.** What makes a view-up vector have a nonzero component in the view plane?

**Known.** `iges.md` "Views and drawings" gives the rule qualitatively. `drawing.rs:182-187` implements it as `up_norm - dot * dot / normal_norm > 1.0e-20`, an absolute threshold on a squared magnitude. IGES does not require view direction vectors to be unit length, so the tested quantity scales with the square of the input magnitudes.

**Note.** A view whose vectors are scaled to 1e-11 is exactly perpendicular and is refused. A degenerate view whose vectors are scaled to 1e6 passes with a relative perpendicularity of 1e-32. No fixture exercises a non-unit view basis.

**Need.** We need a scale-invariant test, or the magnitude range that producers give.

### PS-04. Enumerated value tables exist only in the source

**Question.** What are the admitted Type 316 unit values, Type 230 pattern codes, and Type 322 attribute-list classifications?

**Known.** `structure.rs:188-207`, `annotation.rs:573-582`, `structure.rs:461-478`, and `structure.rs:1118-1120` hold closed tables and ranges. `iges.md` "Views and drawings" gives "unit values use the standard type-specific codes" and `iges.md` "Product structure" gives "attribute-list classification". The tables are not in this repository outside the source, so a refusal rests on grounds no reader of `iges.md` can reconstruct.

**Note.** The `LENGTH` row admits `KN` and omits `KM`, `MM`, and `CM`. `KN` is not a length unit. `KM` is the most probable intended value, so this row is likely a transcription defect. The single fixture (`tests.rs:4554`) uses `1HM` only, so no unit code outside the happy path is exercised.

**Need.** We need the standard tables recorded, and the `LENGTH` row checked against them.

### PS-05. Type 420 accepts a wrong-typed type flag and Type 320 does not

**Question.** Does the Type 420 type flag have a default?

**Known.** `structure.rs:1959` gives `record.integer(8).is_none_or(|value| matches!(value, 0..=2))`. `integer()` gives `None` for a missing token, an omitted token, a real, and a Hollerith alike, so all four are accepted. The identical field on the Type 320 definition uses `is_some_and` (`structure.rs:1878-1880`) and is refused. `iges.md` "Product structure" give the flag for both with no default and no optionality.

**Need.** `is_none_or` over `integer()` is not the spelling of "may be defaulted" used anywhere else in the file. We need the default, and one spelling.

### PS-06. Type 402 Form 5 requires a non-null leader pointer

**Question.** May a label placement have no leader?

**Known.** `structure.rs:573-594` requires `existing_pointer` for the leader field, which refuses `0`. Every other nullable pointer in the file admits `Some(0)` explicitly (`structure.rs:667-675`, `annotation.rs:250-256`, `:309-315`, `:423-433`). `iges.md` "Views and drawings" records the tuple roles and no nullability policy.

**Note.** One leaderless placement refuses the complete associativity, so every label placement in the sheet is lost.

**Need.** We need the nullability of the Form 5 leader field.

### PS-07. Type 406 Form 33 requires a file-global unique identity

**Question.** Must a sheet identifier be unique across the file?

**Known.** `structure.rs:1033-1045` refuses a Form 33 property whose (number, name) pair occurs more than one time in the file. `iges.md` "Product structure" records "at most one sheet identifier per drawing", which is a per-owner rule and is separately enforced at `structure.rs:1046-1068`.

**Note.** The rule refuses both members of a duplicate pair, so two drawings that legitimately carry the same sheet number lose their sheet identity. The comparison uses raw Hollerith bytes, so `1HA` and `2HA ` are distinct.

**Need.** We need the uniqueness rule, if one exists.

### PS-08. Type 406 Form 6 requires an ordered layer pair

**Question.** Must the drilled-hole layer numbers be in ascending order?

**Known.** `structure.rs:320-324` requires `upper >= lower`. `iges.md` "Product structure" records for this form only that the plating and hierarchy fields are Boolean integers. A hole written top-first is a loss whose message does not name the field.

**Need.** We need the field definitions for the Form 6 layer pair.

## 7. Write path

### WR-01. An unclassified loop is written as an inner loop

**Question.** How does the writer encode a loop whose source does not classify it as outer or inner?

**Known.** `writer.rs:1328-1337` computes `has_outer` from the first loop's `boundary_role` and writes `i32::from(has_outer)` as the Type 510 outer-loop flag. `LoopBoundaryRole::Unspecified` is the default variant and is documented as "The source does not classify this loop as outer or inner" (`cadmpeg-ir/src/topology.rs:149-151`). The writer maps that third state onto `OF = 0`, whose meaning `iges.md` "Topology" states as fact: every loop is inner and the support surface's parameter domain supplies the exterior boundary.

**Note.** Every non-STEP decoder in the workspace emits `Unspecified` (sldprt, nx, catia, rhino, freecad, f3d, asm). A six-face solid from any of them writes six faces each declared to have no outer boundary, over Type 190 planes whose domain is unbounded. The receiving system reads unbounded faces. `validate_brep_topology` has no `boundary_role` test.

**Note.** The sheet path already handles this: commit `175d17b42` routes all-`Unspecified` loops to a Type 143 bounded surface (`writer.rs:1755-1757`). The B-rep path did not get the same treatment.

**Need.** The IGES reader only ever produces `Outer` or `Inner`, so no round-trip test can see this. We need the encoding for an unclassified loop, and a refusal if none exists.

### WR-02. The declared Global minimum resolution is tighter than the writer's own acceptance bound

**Question.** What minimum resolution must a generated file declare?

**Known.** `writer.rs:4738-4741` writes field 19 as the constant `0.001`. `writer.rs:2973-2996` accepts a pcurve chain within `self.tolerance.max(COINCIDENCE_TOLERANCE)`, where `COINCIDENCE_TOLERANCE` is 0.01 mm (`cadmpeg-ir/src/units.rs:42`) and `self.tolerance` is unbounded above (`writer.rs:3153-3166`).

**Conflict.** The file declares a resolution ten times tighter than the gap the writer permits. `iges.md` "Topology" states that disagreement beyond the declared resolution prevents attachment, so this codec's own reader discards the topology of a file this codec wrote. There is no writer-to-reader round-trip test in the crate, so nothing detects it.

**Need.** We need the declared resolution derived from the tolerances the writer actually accepted.

### WR-03. The Type 186 outer shell is the first shell by position

**Question.** Which shell of a region is the exterior shell?

**Known.** `writer.rs:1396-1404` writes `shell_indices[0]` as the Type 186 `SHELL` argument and every other index as a void. `Region.shells` is documented as "Boundary shells (typically one outer, plus voids)" (`cadmpeg-ir/src/topology.rs:97`), which is not an ordering invariant. `validate_brep_topology` (`writer.rs:469-498`) tests ownership and non-emptiness and never tests containment or orientation.

**Note.** A decoder that materializes shells in identity order can present a cavity first. The file then declares the cavity as the outer boundary and the skin as a void, which is an inside-out solid.

**Need.** We need the exterior shell identified from geometry or from a declared field, not from list position.

### WR-04. Global fields are a fixed string

**Question.** Which Global values must a generated file compute from the model?

**Known.** `writer.rs:4738-4741` writes one format string. The maximum coordinate is always `1000.0`, the timestamp is always `20260807.000000`, the line-weight series is always one gradation of 1.0 mm, and the sender, receiver, author, and organization are literals. The maximum coordinate is computable from coordinates the writer already holds.

**Note.** The reader preserves the source's declared unit as the `native_units` attribute (`reader.rs:30-31`) and the writer never reads it, so a file authored in inches re-exports declaring `2HMM` with no loss note. The units flag `2` and model scale `1.0` are correct, because the IR has one length unit.

**Need.** We need the Global fields a generated file must compute, and a decision on the frozen timestamp.

### WR-05. The target version changes one digit only

**Question.** What does `IgesWriteOptions::version` constrain?

**Known.** `version.global_flag()` at `writer.rs:4740` is the only use of `version` in the synthesis path. `surface_entities` emits Types 190 through 198 and `brep_entities` emits Types 186 and 502 through 514 for every target.

**Note.** A caller that selects 5.1 for a legacy receiver gets a file whose Global field 23 says 9 and whose Directory holds entities that the declared version may not define. Which of Types 186, 190 through 198, and 502 through 514 postdate 5.1 is not verified against the specification text.

**Need.** We need the entity set of each target version, and a refusal when the model needs an entity the target does not define.

### WR-06. The analytic surface family is fixed with no fallback

**Question.** Which IGES surface entity should a generated file use for each analytic surface?

**Known.** `writer.rs:3593-3763` maps a plane to Type 190 Form 1 and a cylinder, cone, sphere, and torus to Types 192, 194, 196, and 198. `reject_unsupported_native` (`writer.rs:3414-3423`) shows the reader accepts twelve native surface types. Twelve types in, five types out, with no recorded rationale and no loss note when a low-interoperability encoding is selected.

**Note.** A plane has at least three legal encodings (Type 108, Type 190, Type 128) and a cylinder at least three (Type 192, Type 120, Type 128). Types 190 through 198 are pointer-defined records that a receiver may not implement; a receiver that drops them also drops every Type 510 and Type 144 that refers to them, so the body disappears.

**Need.** We need the encoding choice recorded with its reason, and a loss note when the chosen family is not the most portable one.

### WR-07. Orthonormality gates refuse foreign frames instead of repairing them

**Question.** What frame perturbation must the writer accept?

**Known.** `writer.rs:4593-4601` refuses a placement whose axis dot products pass 1e-10, and the same threshold appears at `writer.rs:3548`, `:4097`, `:4131`, `:4176`, `:4220`. A failure gives `CodecError::Malformed` from `plan()`, so the complete export fails rather than one entity.

**Note.** A float32-backed source gives dot products near 3e-8. The writer computes `y_axis = axis.cross(reference)` one line later, so an orthonormal frame is available at no cost through one Gram-Schmidt step.

**Need.** Representation noise is not an unrepresentable value. We need the acceptance bound, and a decision between repair and refusal.

### WR-08. Real numbers are written in fixed notation

**Question.** What number format must a generated file use?

**Known.** `writer.rs:4905-4911` gives `format!("{value:.17}")`, which is 17 digits after the decimal point and never an exponent. Values below about 5e-18 flush to a literal decimal zero. A NURBS weight of 1e-20 passes the `weight <= 0.0` guard at `writer.rs:4310` and is written as zero, so the receiver divides by zero at that pole. Every value of magnitude 1 or more costs 19 or more bytes.

**Note.** The realistic exposure is narrow. Model coordinates in millimetres and normalized knot vectors stay inside the round-tripping range.

**Note.** `entity.parameters.chunks(64)` (`writer.rs:4818`) splits Parameter Data mid-token, so a long real can straddle a card boundary. `iges.md` "Global section" sanctions this for Global Hollerith values. Whether IGES permits it for Parameter Data reals is not verified, and many translators break records at a delimiter.

**Need.** We need the number format and the card-splitting rule for Parameter Data.

### WR-09. A zero-length span on a closed curve becomes a full revolution

**Question.** What does a parameter range whose start equals its end mean on output?

**Known.** `writer.rs:4611-4623` gives `range[0] + TAU` when the curve is closed and the sweep is at most 1e-14. `edge_span` (`writer.rs:3957-4006`) refuses `range[0] > range[1]` only, so a degenerate zero-length edge reaches this path and is written as a complete circle whose two vertices are the same point. Re-reading that file trips the edge consistency rule at `iges.md` "Topology" and discards the topology candidate.

**Note.** In the ordinary full-circle case the terminate point is computed at `TAU`, where `sin(TAU)` is -2.449e-16, so the start and terminate coordinates are not byte-identical. This codec's reader was relaxed to tolerate that (`f20d17e65`). A strict receiver reads a nearly-closed arc.

**Need.** We need the encoding of a degenerate span, and byte-identical endpoints for a closed curve.

### WR-10. Fixed protocol constants with no IR source

**Question.** What are the correct `PREF`, creation-method, and hierarchy values for generated records?

**Known.** `writer.rs:2332` writes Type 141 `PREF` as `0`. `writer.rs:2515-2520` writes Type 142 as creation method `0` and `PREF` `3`, which declares the two representations equally authoritative although the writer generated the pcurve by composition and validated it only to 0.01 mm (WR-02). `writer.rs:1135` gives the Type 504 Edge List hierarchy `01` where every sibling topology entity uses `00` (`writer.rs:1092`, `:1311`, `:1351`, `:1386-1390`).

**Need.** We need the correct value for each, and a reason recorded for the Type 504 difference.

### WR-11. Type 123 is missing from the export census

**Question.** Which entity types does the export report count?

**Known.** `writer.rs:3168-3202` matches 25 entity types and omits Type 123, although `pointer_surface_support` (`writer.rs:3560-3569`) writes two Type 123 direction entities for every analytic surface. A solid with six analytic faces reports `unknown_entity: 12` for a fully supported export.

**Need.** `cadmpeg query counts` and any consumer that treats `unknown_entity > 0` as a coverage gap gets a false signal. We need Type 123 in the census.

## 8. Evidence

### EV-01. No file authored by another system has ever been decoded

**Question.** Does the decoder read IGES files that this project did not write?

**Known.** `corpus/manifest.toml` holds eleven files and every one is `fcstd`. No IGES file exists in the corpus. `iges-fixture-charter.md` states that fixtures are generated from builders that "serialize the rules in `iges.md`" and "do not ingest, rewrite, minimize, or transform external files".

**Conflict.** The decoder is tested only against bytes written by this project's own reading of the format. Agreement between a builder and a decoder that share one author proves that the two agree. It does not test the reading.

**Note.** Nearly every item in sections 1 through 6 above is a rule that a self-authored fixture satisfies by construction and that a producer file may not. The items were found by reading, not by testing, because no test could have found them.

**Need.** We need IGES files from at least two independent producers, under a license the repository admits, decoded and recorded. That measurement decides which of the tolerance items above are real and which are theoretical.

### EV-02. The independent-application gate cannot detect wrong geometry

**Question.** What does FreeCAD acceptance prove about a generated file?

**Known.** `scripts/verify-iges-freecad.py` imports each file and refuses an import that gives no object or whose shapes are null or invalid (`:37-50`). It counts solids and faces and asserts nothing about them. A file with the wrong units, a mirrored surface, an inverted solid (WR-03), or an unbounded face (WR-01) imports as a valid shape and passes.

**Note.** The script is wired into no CI job and no test, and it needs a manual environment. No result artifact is committed, so no run is on record.

**Note.** The script globs `*.igs` only (`:68`). The CLI accepts and writes both `.igs` and `.iges` (`crates/cadmpeg/src/main.rs:168`), so a directory of `.iges` output is silently outside the check.

**Need.** The P0 gate above requires independent native-application acceptance. We need the acceptance criterion to compare geometry with the intended model, the glob to cover both extensions, and each run recorded.

### EV-03. The fixture builders and the decoder share one author

**Question.** Which decoder rules do the fixtures actually test?

**Known.** `iges-fixture-charter.md` states that builders serialize the rules in `iges.md`. A builder therefore writes the byte pattern that the decoder expects. Where a decoder rule is a guess, the builder embodies the same guess, and the test passes for both.

**Note.** GE-01 is the demonstrated case. Commit `f20d17e65` set `TRANSFORM_TOLERANCE` and authored the fixture that justifies it in the same commit, perturbed to 5e-11 against a threshold of 1e-10.

**Need.** We need each tolerance and default in sections 2 through 5 traced to evidence outside this repository, or marked as a project convention in `iges.md` rather than as a format rule.

### TE-01a. Integration arena rows do not name the subject entity's arena

**Question.** Which arena must each integration fixture populate, and which entity in that fixture must populate it?

**Known.** TE-01 asked for a per-fixture expectation table. Commit `0352402f3` delivered one: `integration_tests.rs:47-62` names an arena per fixture and fails with the fixture name. The union assertions are gone. The remaining defects are narrower than TE-01 and are recorded here:

- Every row asserts `arena_count(...) > 0`. No row declares a count.
- Some rows name an arena that a different entity in the same fixture fills. `mixed_analytic_composite_curve` (`tests.rs:854-880`) holds a Type 100, a Type 110, and a Type 102, and maps to `ModelCurves` (`integration_tests.rs:120-124`). The matrix gives Type 102 the destination `model.procedural_curves`, and Types 100 and 110 fill `model.curves` on their own. The row passes when the Type 102 decoder emits nothing. The same shape applies to `procedural_and_boolean_solids` and to five drawing fixtures that all name `Native("annotations")` while each holds a Type 212 that satisfies the row alone.
- The table names the decoder's internal arena keys and is not cross-checked against the `destination` column of `corpus/iges-envelope-a.toml`. Envelope admission is cross-checked against that file (`tests.rs:190-242`); the arena table is not.

**Need.** We need each row to name the arena of the entity the fixture exists to exercise, and a test that compares the table with the matrix `destination` column.

### EV-04. Assertions that restate a generated identifier

**Question.** Which ordered collections have order evidence?

**Known.** `tests.rs:5189-5214` is named `decode_preserves_ordered_type_141_pcurve_collections` and asserts `coedge.pcurves[0].pcurve.0.ends_with(":0:0:0")` and `[1]...ends_with(":0:0:1")`. The identifier suffix is the array index, minted by the `enumerate()` that builds the array (`trimming.rs:684-691`). The assertion holds for every permutation of the source list, so a decoder that reverses, sorts, or de-duplicates the Type 141 pcurve pointers keeps the test green. The fixture supplies two distinguishable pcurve geometries and neither is inspected.

**Note.** The sibling test at `tests.rs:5312-5316` is sound, because it also asserts the isoparametric flag, which comes from the source tuple and not from an index.

**Need.** We need order evidence taken from the data, not from a generated identifier.

### EV-05. The power-basis conversion is exercised on linear data only

**Question.** Is the Type 112 and Type 114 power-basis to Bezier conversion correct?

**Known.** `power_to_bezier` (`splines.rs:36-44`) and the tensor-product `patch_bezier` (`splines.rs:46-65`) carry every cubic constant. The three fixtures that reach them (`tests.rs:1352-1357`, `:1392-1407`) declare spline type 3 and set every quadratic and cubic coefficient to zero. The curve fixture is `x = t, y = 0, z = 0` and the surface fixture is the identity plane, asserted at `(1.5) -> (1.5, 0, 0)` and `(0.25, 0.75) -> (0.25, 0.75, 0)`.

**Note.** A wrong constant in the `c` or `d` term, a dropped `d·w³` term, a global-versus-local breakpoint width, a y-or-z coefficient block read from the x offset, and every `u^i v^j` cross term of the tensor product all give the same result on this data. The two goldens freeze the output of the same degenerate inputs, so they cannot detect a wrong rule.

**Need.** We need a fixture whose cubic and cross terms are nonzero, with evaluation points computed from the power-basis definition.

### EV-06. Nine property forms are asserted through a copied form number

**Question.** Do the Type 406 Forms 28 through 36 typed fields decode correctly?

**Known.** `tests.rs:6285-6311` asserts for each form that some property record has `fields()["form"] == form`. `native.rs:2798` sets `form: entry.form`, a copy of the Directory field the builder wrote, so the assertion proves only that the match arm gave `Some`. The fixtures write about 35 typed values across nine variants (`native.rs:624-677`), and no test names any of them. None of the four fixtures has a golden.

**Note.** `native.rs:2789-2792` reads the Form 36 closure flags from token indices 2 and 3. Shifting both to 1 and 2 keeps every test in the repository green. Sibling tests at `tests.rs:6169-6247` assert field values correctly, so this test is the outlier.

**Need.** We need field-value assertions for each typed property variant.

### EV-07. Tolerance gates are bracketed at 100 times the threshold

**Question.** Which tolerance value does each gated test actually pin?

**Known.** Every fixture declares a Global minimum resolution of `0.001`. The accept and reject pair for the Type 100 radius gate uses a radius delta of 1.7e-9 to accept and 9.8e-2 to refuse (`tests.rs:8298`, `:8325`). The threshold sits between them with seven orders of magnitude of slack, so any tolerance in `[2e-9, 9.7e-2)` keeps both tests green, including removal of `minimum_resolution_mm()` from `geometry.rs:380`. The three carrier-disagreement reject tests use a 0.1 coordinate shift against the same 0.001 tolerance.

**Note.** `bounded_plane_with_resolution_gap_file` (`tests.rs:2464`) is the exception and shows the correct shape: a 0.0005 gap against a 0.001 tolerance. It has no reject-side twin.

**Need.** GE-01 through GE-10 record tolerances that no test pins. We need each gate bracketed on both sides.

### EV-08. The minimum resolution is never unit-converted in a test

**Question.** Does `minimum_resolution_mm` apply the length factor?

**Known.** `global.rs:241-245` multiplies the declared resolution by `length_factor_mm()`. Every fixture but one declares millimetres with model scale 1.0, so the factor is 1.0. The one fixture that declares centimetres and scale 0.5 (`tests.rs:8538`) holds one point and two transforms, so it reaches none of the ten call sites that consume the resolution.

**Note.** Removing `* factor` from `global.rs:244` keeps every test green. On a centimetre file the true tolerance is 20 times the value the decoder would then use.

**Need.** We need a resolution-gated fixture in a unit other than millimetres.

### EV-09. The envelope rejection sweep stops at form 100

**Question.** Does the envelope matrix test prove the rejection side?

**Known.** `tests.rs:222-241` sweeps entity types 0 through 600 against forms -1 through 100, and probes forms above 100 for Type 302 only. Every matrix-listed form is inside -1 through 100, so admission is complete and rejection is not. A `profile.rs` arm widened to admit an implementor-defined range on a type other than 302 passes.

**Need.** We need the rejection probe extended to every admitted type.
