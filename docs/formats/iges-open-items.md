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

### PH-03. The boundary between entity parameters and trailing pointer groups

**Question.** Where does an entity's own parameter list stop and its trailing pointer groups start?

**Known.** `parameter.rs:153-161` scans every token position and accepts the earliest candidate for which the count and pointer groups are structurally valid. `native.rs:1362-1371` applies the same earliest-candidate rule when only an association group is available. The selected boundary controls entity parameters and Type 406 ownership.

**Need.** We need per-entity parameter arity, or a source rule that makes the boundary unique. If a candidate group contains an unresolved member, the group must remain visible as a finding instead of disappearing through candidate selection.

**Note.** Closure audit 2026-08-10: reopened. Commit `fdfda1cff` preserved malformed trailing groups but did not establish the boundary. The current `iges.md` wording promotes the earliest structurally closed suffix to a format rule without independent evidence. If an entity contains an earlier token suffix that also satisfies the pointer shape, the decoder can assign the wrong parameters and property owner.

## 2. Global metadata

## 3. Directory fields, the reference graph, and the native arenas

### DR-04. One malformed subfigure definition promotes its instances to assembly roots

**Question.** How is a top-level product instance identified when a subfigure definition is malformed?

**Known.** `native.rs:3438-3450` drops a complete Type 308 or Type 320 definition when one member fails integer parsing. `native.rs:3515-3519` then infers roots from instances not named by a surviving definition. The current code suppresses root inference when it records a malformed definition, but this is a project recovery policy.

**Need.** We need a source rule for per-member recovery or for blocking root inference after a malformed definition. The decoder must not fabricate occurrences or silently discard valid independent roots.

**Note.** Closure audit 2026-08-10: reopened. Commit `4080057b9` added suppression and a synthetic malformed fixture, but no producer or format evidence establishes that one malformed member invalidates every root inference.

### DR-09. The Directory status field accepts blank or eight digits and nothing between

**Question.** Must the Directory status number be zero-padded to eight digits?

**Known.** `directory.rs:92-129` accepts an all-blank field or exactly eight digits. It rejects right-justified digits with leading blanks, unlike the other Directory numeric fields. The current documentation states that leading blanks are equivalent to zero.

**Need.** We need the required rendering and blank-field rule from the format source. A wrong choice rejects a complete file when one status field uses conventional right alignment.

**Note.** Closure audit 2026-08-10: reopened. Commit `4edd96bee` added the right-justified behavior and tests, but the external IGES material checked describes status subfields as fixed two-digit fields and does not settle this acceptance policy. The current code and documentation also need their leading-blank semantics reconciled.

### DR-10. Two fabricated defaults in the Type 406 Form 30 native record

**Question.** What are the defaults of the Type 406 Form 30 character-set and witness-line-angle fields, and in which unit is the angle native?

**Known.** `native.rs:2728-2737` supplies character set `1` and `FRAC_PI_2` for omitted fields. Other omitted Form 30 fields remain absent. The format documentation records no defaults or native angle unit.

**Need.** We need the two defaults and the native unit of the angle field. An injected value must remain distinguishable from an explicit value.

**Note.** Closure audit 2026-08-10: reopened. Commit `7472b1242` preserves declared and effective values, but the synthetic fixture and same-commit documentation do not prove the defaults or angle unit.

## 4. Geometry carriers and tolerances

### GE-01. The Type 124 transformation tolerance

**Question.** What tolerance applies when a Type 124 transformation is compared with the canonical transform?

**Known.** The reader uses intervals derived from Global real precision in `transform.rs`, and commit `40b1687ea` replaced a fixed tolerance with that calculation. The fixture perturbs a transform near the same project-selected boundary.

**Need.** We need an IGES rule or independent producer evidence for transform equality and the accepted precision. Internal interval arithmetic proves only the implementation's chosen criterion.

**Note.** Closure audit 2026-08-10: reopened. The code, fixture, and documentation were authored together; this is the promotion-to-spec pattern described by the QA plan.

### GE-02. Unit-vector acceptance

**Question.** What deviation from unit length is valid for a direction vector?

**Known.** `geometry.rs` derives an interval from Global real precision and accepts a vector when the squared length interval contains one. The threshold is exercised by project-generated data.

**Need.** We need a source or independent producer corpus that establishes the accepted numeric deviation and whether a decoder should normalize or reject it.

**Note.** Closure audit 2026-08-10: reopened. Commit `8d4e832c4` replaced the old threshold with a more principled calculation, but did not supply external evidence for the format tolerance.

### GE-03. Type 112 segment continuity

**Question.** What continuity tolerance applies between adjacent Type 112 segments?

**Known.** `splines.rs` uses Global real-precision intervals for adjacent endpoint comparisons. The fixtures exercise the selected interval, not an independently specified producer boundary.

**Need.** We need the continuity rule and its numeric tolerance from the format or independent producer output.

**Note.** Closure audit 2026-08-10: reopened. Commit `2481439ee` established an internal interval calculation, not conformance evidence.

### GE-05. Type 102 carrier concatenation uses a private tolerance and degrades silently

**Question.** What tolerance and failure behavior apply when Type 102 component curves do not join?

**Known.** `composite.rs` compares adjacent endpoints with a project-selected tolerance and the native path records a loss when it cannot concatenate. The current documentation describes this policy as a format rule.

**Need.** We need the source join rule and evidence for whether a non-joining Type 102 must be rejected, preserved as separate components, or projected with a loss.

**Note.** Closure audit 2026-08-10: reopened. Commit `611362a96` improved diagnostics and tests but did not establish the threshold or the required semantic result.

### GE-07. The curve parameter-domain convention

**Question.** Which parameter domain does each supported curve entity provide?

**Known.** `offsets.rs` maps Type 100, 110, and 130 into domains used by topology and `native.rs` applies fallback domains for native records. The current domains and fallback policy are documented without a complete source mapping.

**Need.** We need the parameter-domain rule for every supported curve form, including open, closed, and unbounded cases, and evidence for any fallback.

**Note.** Closure audit 2026-08-10: reopened. Commit `2731411c2` centralized domains and added self-authored coverage; it did not verify the mapping against the format or independent producer files.

### GE-08. Type 106 duplicate points and closure

**Question.** Which duplicate-point and closure patterns are valid in a Type 106 entity?

**Known.** `geometry.rs` removes or accepts duplicate points and tests closure using the Global minimum resolution. The current form interpretation and tolerance are encoded in the decoder and fixtures.

**Need.** We need the Type 106 form rules for duplicate points and closed paths, including the tolerance and whether source order must be retained.

**Note.** Closure audit 2026-08-10: reopened. Commit `42b06aa54` made the path policy internally consistent, but consistency with generated fixtures is not verification.

### GE-09. Type 104 endpoints are not tested against the conic

**Question.** Must Type 104 endpoint coordinates agree with the conic parameters, and what tolerance applies?

**Known.** `geometry.rs` uses the endpoint values and accepts agreement within the Global minimum resolution. The current tests use matching or project-bracketed values.

**Need.** We need the source authority between the analytic coefficients and endpoint fields, and the tolerance for disagreement.

**Note.** Closure audit 2026-08-10: reopened. Commit `2ac641864` added endpoint validation, but no independent evidence establishes the authority or tolerance.

### GE-10. Angular equality constants

**Question.** What angular tolerance applies when the codec compares directions and spans?

**Known.** `curve_conversion.rs:24-27` defines one `ANGULAR_TOLERANCE` as `TAU * 1e-12`. The current tests pin the same project constant.

**Conflict.** `writer.rs:4710-4717` accepts a conic sweep through `TAU + 1.0e-10`, while the reader uses `TAU * 1e-12` for angular equality. The writer can therefore emit a sweep that passes its gate but fails the reader's angular gate.

**Need.** We need producer or specification evidence for angular equality, or a project-policy classification that does not present this value as an IGES rule.

**Note.** Closure audit 2026-08-10: reopened. Commit `6173d018b` changed a magic number into a named constant, but naming a threshold is not evidence for it.

### GE-12. Type 126 property flags against the values

**Question.** Which Type 126 representation flags are authoritative when they disagree with the values?

**Known.** `splines.rs` and `geometry.rs` use Type 126 flags to select domains and representation behavior, with range clamping and loss paths for unsupported combinations. The current policy is covered by project fixtures.

**Need.** We need the precedence of flags, values, and derived ranges for every Type 126 form, plus the required behavior for inconsistent records.

**Note.** Closure audit 2026-08-10: reopened. Commit `fa5bddc17` improved numeric consistency checks, but it did not establish which fields are authoritative in a conformant file.

## 5. Surfaces and topology

### TP-01. The Global minimum resolution serves five unrelated roles

**Question.** Which topology and geometry decisions may use Global minimum resolution?

**Known.** `trimming.rs:59-74` derives a coordinate quantum from the largest coordinate and single-precision significance, then takes the maximum with Global minimum resolution. The result is used for ring closure, vertex merging, and stored edge and face tolerances.

**Need.** We need the source meaning of minimum resolution and separate rules for coordinate precision, curve-fit tolerance, topology sewing, and native tolerance fields.

**Note.** Closure audit 2026-08-10: reopened. Commit `6bb0de35f` supplied a formula and synthetic boundary tests, but no external evidence supports using one value for these five roles.

### TP-03. Declared surface parameter subranges are discarded with no loss

**Question.** Must a Type 141/142 surface-boundary use retain its declared surface parameter subrange?

**Known.** `trimming.rs` projects model curves and uses the neutral edge range; the declared surface parameter bounds are not retained in the native-to-IR path. The current documentation calls this intentional.

**Need.** We need the source semantics of the parameter subrange and a decision whether projection may discard it, must preserve it, or must report a loss.

**Note.** Closure audit 2026-08-10: reopened. Commit `8ddb25d46` added bounds handling and tests, but it did not establish whether the bounds are authoritative for the boundary use.

### TP-04. The Type 140 offset sign uses a per-kind representative normal

**Question.** Which normal determines the sign of a Type 140 offset indicator?

**Known.** `surfaces.rs:206-225` uses the support surface's bounds midpoint when finite and otherwise `(0, 0)` as the representative parameter, then evaluates a normal. The current documentation states this rule.

**Need.** We need the source rule for the offset sign and a representative point that is valid for bounded, unbounded, and varying-normal surfaces.

**Note.** Closure audit 2026-08-10: reopened. Commit `23554c501` changed implementation, documentation, and fixtures together. No independent evidence establishes midpoint selection or the `(0, 0)` fallback.

### TP-06. Type 180 Form 1 requires a direct Type 186 operand

**Question.** Does a Type 180 Form 1 Boolean tree accept a Type 186 solid directly, or through a complete operand subtree?

**Known.** `brep.rs` recursively checks Type 180 and Type 430 references and accepts a Form 1 operand when the complete referenced subtree contains a Type 186. The current fixtures use project-generated nested trees.

**Need.** We need the operand rule for Boolean subtrees and the treatment of nested or malformed operands from the format source or independent producer files.

**Note.** Closure audit 2026-08-10: reopened. Commit `34861ac75` made the recursive interpretation internally consistent, but the source rule remains unverified. The current documentation promotes the recursive choice to a settled format fact.

### TP-09. A model-curve pointer has no resolved edge-ownership rule

**Question.** How does a Type 141/142 model-curve pointer select a neutral edge when multiple edges use the same curve carrier?

**Known.** `trimming.rs` retains all candidates and selects a unique Type 141/142 edge whose parameter-curve endpoints agree; a model-preferred boundary with multiple candidates is rejected as ambiguous. `brep.rs` selects a candidate whose declared range evaluates to the Type 186 vertex-list endpoints. `offsets.rs` uses the same curve-endpoint check for the source parameter range, and `csg.rs` rejects conflicting closed/open results. `composite.rs` retains every edge candidate and rejects conflicting parameter ranges or resolved endpoints. `Edge` permits repeated `CurveId` values with distinct vertices and parameter ranges. Type 141/142 carry curve entity pointers, not edge occurrence identities.

**Need.** We need an ownership invariant or a source rule that makes the curve-to-edge relation unique, or a resolution path that verifies each candidate's range and endpoints. A wrong choice transfers the wrong range or endpoints to a boundary, B-rep, offset, composite, or sweep.

**Note.** Candidate checks prevent silent transfer of a wrong range or endpoint, but they do not establish the source ownership rule. The codec now refuses conflicting composite candidates instead of selecting one by storage order.

### TP-10. Tolerance vertex merging depends on the first representative

**Question.** How are boundary endpoints clustered when one point is within tolerance of more than one existing vertex?

**Known.** `entities/trimming.rs:89-117` returns the first vertex whose position is within tolerance and otherwise creates a vertex at the current endpoint. The function is called for every boundary segment endpoint at `entities/trimming.rs:757-771`. It does not choose the nearest representative, compute a transitive cluster, or record a merge ambiguity.

**Need.** We need a deterministic clustering rule and a loss or refusal policy for non-transitive tolerance neighborhoods. Topology must not depend on the order of boundary segments without an explicit rule.

**Note.** Hostile sweep 2026-08-15: if A and B are farther apart than tolerance while C is within tolerance of both, source order determines whether C joins A or B. This can change vertex identity and sewing topology.

## 6. Product structure, annotation, and presentation

## 7. Write path

### WR-01. An unclassified loop is written as an inner loop

**Question.** How does the writer encode a loop whose source does not classify it as outer or inner?

**Known.** `writer.rs:2321-2364` orders loop roles and returns the first loop that is not `Inner`. The Type 510 path uses `face_outer_loop` at `writer.rs:1375-1382`; the Type 144 path also promotes the first unclassified loop. `LoopBoundaryRole::Unspecified` is a real IR state.

**Need.** We need the encoding for an unclassified loop, or a refusal when no valid outer-loop representation exists. The choice must not depend on list order.

**Note.** Closure audit 2026-08-10: reopened. Commit `ed211eb05` documented the first-unclassified policy and added generated fixtures, but it supplied no source or producer evidence. Two unclassified loops with different containment or orientation can produce the wrong outer boundary.

### WR-02. The declared Global minimum resolution is tighter than the writer's own acceptance bound

**Question.** What minimum resolution must a generated file declare?

**Known.** `writer.rs:3232-3245` derives a generated value from model tolerances and a floor, while `writer.rs:3037-3040` accepts some pcurve gaps against a larger effective floor. The current documentation presents the generated resolution as the settled writer policy.

**Need.** We need the declared resolution derived from the tolerances the writer accepts, and a round-trip test that proves the reader and writer use compatible bounds.

**Note.** Closure audit 2026-08-10: reopened. Commit `17c19bdcb` changed the generated-resolution policy and fixtures without independent producer evidence. The code needs an evidence-backed relation between accepted gaps and the declared value.

### WR-03. The Type 186 outer shell is the first shell by position

**Question.** Which shell of a region is the exterior shell?

**Known.** `cadmpeg-ir/src/topology.rs:97-110` documents ordered shells and `Region::exterior_shell()` returns `self.shells.first()`. `writer.rs:1442-1457` uses that accessor for Type 186. No validation proves containment, orientation, or producer ordering.

**Need.** We need the exterior shell identified from geometry, an explicit source role, or a validated IR invariant. List position alone must not invert a solid.

**Note.** Closure audit 2026-08-10: reopened. Commit `46a71f68c` introduced the IR wording, accessor, writer use, and tests together. This is promotion to an IR invariant, not evidence that all producers supply the order.

### WR-04. Global fields are a fixed string

**Question.** Which Global values must a generated file compute from the model?

**Known.** `writer.rs:4822-4835` writes fixed sender, file, author, organization, timestamp, resolution, and coordinate metadata. The current documentation records this as writer policy.

**Need.** We need the fields that must be computed, the fields that may use project defaults, and the required treatment of timestamps and source identity.

**Note.** Closure audit 2026-08-10: reopened. Commit `2b3306edc` made the fixed metadata explicit, but no interoperability evidence or settled project policy justifies the values.

### WR-05. The target version changes one digit only

**Question.** What does `IgesWriteOptions::version` constrain?

**Known.** `writer.rs:295-310` rejects one unsupported Type 514 form for older targets, while `version.global_flag()` changes the Global version field. Other emitted entity families are not checked against the selected target version.

**Need.** We need the entity and form set of each target version, and a refusal when the model requires an entity the target does not define.

**Note.** Closure audit 2026-08-10: reopened. Commit `1a6b988e7` added a target entity check, but its coverage and version matrix are not independently established.

### WR-06. The analytic surface family is fixed with no fallback

**Question.** Which IGES surface entity should a generated file use for each analytic surface?

**Known.** `writer.rs` maps planes, cylinders, cones, spheres, and tori to Types 190, 192, 194, 196, and 198, and rejects unsupported native forms. The writer does not record why this family is preferred over Type 108/120/128 alternatives.

**Need.** We need the encoding choice and its interoperability evidence, plus a loss or refusal when the selected family is not supported by the target profile.

**Note.** Closure audit 2026-08-10: reopened. Commit `f4a07d64b` made the analytic-family choice explicit but did not establish portability or a target-profile rule.

### WR-07. Orthonormality gates refuse foreign frames instead of repairing them

**Question.** What frame perturbation must the writer accept?

**Known.** `writer.rs` uses `FRAME_REPAIR_DOT_LIMIT = 1e-6` in `orthonormal_pair`; the current writer repairs only within that project-selected bound and rejects larger residuals.

**Need.** We need a source or producer-derived bound for representational frame noise, and a rule for repair versus refusal.

**Note.** Closure audit 2026-08-10: reopened. Commit `dc2bd137a` added the repair threshold and synthetic boundary tests, but no external evidence establishes the bound.

### WR-10. Fixed protocol constants with no IR source

**Question.** What are the correct `PREF`, creation-method, and hierarchy values for generated records?

**Known.** `writer.rs` emits fixed values for Type 141/142 preferences, Type 142 creation method, and Type 504 hierarchy. The values are justified by the current neutral IR and writer behavior, not by a complete source mapping.

**Need.** We need the correct value for each field and independent evidence for the Type 504 hierarchy difference.

**Note.** Closure audit 2026-08-10: reopened. Commit `82c13da5a` recorded protocol constants as settled without external format or producer evidence.

## 8. Evidence

### EV-01. No file authored by another system has ever been decoded

**Question.** Does the decoder read IGES files that this project did not write?

**Known.** `corpus/manifest.toml` holds eleven files and every one is `fcstd`. No IGES file exists in the corpus. `iges-fixture-charter.md` states that fixtures are generated from builders that "serialize the rules in `iges.md`" and "do not ingest, rewrite, minimize, or transform external files".

**Conflict.** The decoder is tested only against bytes written by this project's own reading of the format. Agreement between a builder and a decoder that share one author proves that the two agree. It does not test the reading.

**Note.** Nearly every item in sections 1 through 6 above is a rule that a self-authored fixture satisfies by construction and that a producer file may not. The items were found by reading, not by testing, because no test could have found them.

**Need.** We need IGES files from at least two independent producers, under a license the repository admits, decoded and recorded. That measurement decides which of the tolerance items above are real and which are theoretical.

### EV-02. The independent-application gate cannot detect wrong geometry

**Question.** What does FreeCAD acceptance prove about a generated file?

**Known.** `scripts/prepare-iges-freecad-golden.py` materializes either the exact geometry manifest or every checked-in golden with a successful writer output. `scripts/prepare-iges-freecad-edited.py` creates one edited neutral point document for each IGES target version. `scripts/verify-iges-freecad.py` imports each file, refuses an import that gives no object unless the broad writer pass explicitly allows presentation-only emptiness, and refuses shapes that are null or invalid. The exact pass requires a complete manifest of topology counts, bounding-box coordinates, and scalar measures with explicit tolerances. `scripts/iges-freecad-expectations.json` covers finite points, curves, trimmed surfaces, and B-rep solids; unbounded analytic surfaces use topology counts only. The workflow runs both passes.

**Note.** `.github/workflows/iges-freecad.yml` runs the gate on pull requests, main pushes, scheduled runs, and manual dispatch, then uploads the report. The script still needs an external FreeCAD runtime, and no result artifact is committed to the repository.

**Need.** Require one successful workflow run for the complete writable geometry profile and retain both report artifacts as evidence. Expand the exact manifest when a geometry profile grows and keep the broad pass aligned with every successful writer output.

### EV-03. The fixture builders and the decoder share one author

**Question.** Which decoder rules do the fixtures actually test?

**Known.** `iges-fixture-charter.md` states that builders serialize the rules in `iges.md`. A builder therefore writes the byte pattern that the decoder expects. Where a decoder rule is a guess, the builder embodies the same guess, and the test passes for both.

**Note.** GE-01 is the demonstrated case. Commit `f20d17e65` set `TRANSFORM_TOLERANCE` and authored the fixture that justifies it in the same commit, perturbed to 5e-11 against a threshold of 1e-10.

**Need.** We need each tolerance and default in sections 2 through 5 traced to evidence outside this repository, or marked as a project convention in `iges.md` rather than as a format rule.

### EV-07. Tolerance coverage is partial and not independently bracketed

**Question.** Which tolerance and default decisions have independent accept/reject boundary evidence?

**Known.** The current suite has paired geometry cases for selected Type 141/142 boundaries, Type 104 endpoints, Type 100 arcs, Type 102 joins, Type 112 continuity, and Type 106 endpoints. It does not provide paired boundary cases for every tolerance or default currently stated in sections 2 through 5; the transform, angular, frame, flag, and several default policies remain unbracketed.

**Need.** We need an independent accept/reject pair for each material tolerance and default, or a project-policy classification that removes the claim of format authority.

**Note.** Closure audit 2026-08-15: reopened. The mass-closure tests improved coverage but closed this item only partially; selected geometry pairs do not establish coverage of every active gate.
