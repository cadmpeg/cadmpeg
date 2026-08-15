# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

## 1. Design projection

### DP-01. Forward declared dependencies

**Question.** Can a declared ObjectDeps target appear later than its dependent object in source
order?

**Known.** Declared dependencies and earlier link-property operands form the feature dependency
graph. A declared dependency can target a later declaration. The source-order restriction
applies to link operands, not declared dependencies.

**Conflict.** design.rs:420-449 filters every dependency by the target feature ordinal being
earlier than the consumer. A declared forward dependency remains native but disappears from the
neutral feature graph.

**Need.** Preserve every resolved declared feature dependency. Apply the earlier-source rule only
to link-property operands.

**Note.** The current specification and FreeCAD Document.cpp dependency writer allow the
forward-declaration case. The implementation still drops it.

### DP-02. Sketch profile seed order

**Question.** Which non-construction entity starts each oriented sketch profile chain?

**Known.** Sketch entities retain persisted source order and native identity. Profile chains must
be deterministic and attributable.

**Conflict.** design.rs:2371-2457 seeds each profile from the first remaining entity in a
decoder-owned ordered set. FreeCAD PropertyGeometryList.cpp proves persisted geometry-list order
but does not prove this disconnected-chain seed rule.

**Need.** Establish the producer-defined seed rule for each disconnected chain and retain the
persisted entity ordinal in the decision.

**Note.** The closure fixed an earlier decimal-string ordering defect. It did not establish the
source rule for disconnected profiles, so the first unused entity remains an unverified
semantic choice.

### DP-03. Sketch profile junction ambiguity and tolerance

**Question.** What endpoint tolerance connects two sketch entities, and what happens when more
than one unused entity meets the current endpoint?

**Known.** Constraints and persisted geometry can produce coincident endpoints. A neutral
profile chain asserts one ordered continuation and orientation at every junction.

**Conflict.** design.rs:2460-2485 supplements constraints with coordinate matching.
endpoints_match_by_roundoff at 2579-2589 uses 64 times machine epsilon scaled by the coordinate
magnitude. No FreeCAD tolerance or admissible profile topology supports this numeric boundary.

**Need.** Establish endpoint equivalence and the admissible profile topology. An ambiguous
junction must use constraint identity, an explicit source-order rule, or an attributable
refusal.

**Note.** The closure added ambiguity handling and a scale formula. The boundary remains a
decoder policy without a producer or kernel witness.

### DP-04. Design runtime and sketch-carrier dispatch

**Question.** Which exact runtime type and child value select a design feature or sketch geometry
family?

**Known.** Native records retain exact runtime types and ordered XML children. Known FreeCAD
families have family-specific value fields.

**Conflict.** design.rs:5066-5078 and 5185-5190 use contains for several design families.
Application-owned types containing a built-in token can therefore enter a built-in neutral
family. The specification requires exact dispatch and excludes subclass or substring
misclassification.

**Need.** Establish the complete exact design runtime registry and carrier grammar. Unknown or
conflicting carriers must remain native or be rejected.

**Note.** The closure removed one family of first-carrier ambiguity but did not remove the
substring dispatch paths.

### DP-05. Dependency-cycle ordinal fallback

**Question.** What neutral projection applies when feature dependencies, parents, or expressions
form a cycle?

**Known.** The native graph retains cycles. The neutral graph must use a stable maximal subset whose
targets precede their consumers.

**Conflict.** design.rs:650-675 falls back to the minimum source-order un-emitted object when no
object is ready. design.rs:420-449 then removes dependencies whose target ordinal is not earlier.

**Need.** Define a cycle projection that is stable and preserves the maximal admissible subset, or
refuse with an explicit loss. Do not source-order a cycle and silently discard its edges.

**Note.** A two-object A-to-B and B-to-A cycle emits one object by source order and drops the
back-edge from the neutral graph. Reordering the declarations changes the neutral relation while
decode still succeeds.

## 2. Product structure

## 3. Semantic annotations

### SA-01. Runtime-type to annotation-kind mapping

**Question.** Which exact application runtime types represent dimensions, geometric tolerances,
datums, balloons, leaders, symbols, and text annotations?

**Known.** Native annotation records retain the exact runtime type. The neutral arena requires
separate semantic kinds.

**Conflict.** annotation.rs:167-225 uses a positive exact registry and sends every other type to
native retention. The FreeCAD TechDraw source tree contains additional standard derived drawing
types, including DrawViewArch, DrawViewDraft, DrawViewSpreadsheet, and DrawViewCollection; the
current registry has no exhaustive producer evidence or explicit exclusion for related semantic
types.

**Need.** Establish the complete exact runtime registry and kind mapping. Unknown application
types must remain native until their semantic family is established.

**Note.** Positive fixture cases prove listed mappings only. Inheritance alone does not prove a
neutral kind, but the current positive list cannot be treated as complete without an exhaustive
producer registry or explicit exclusions.

### SA-02. Annotation scalar and position property selection

**Question.** Which property carries the semantic scalar and position for each annotation runtime
type?

**Known.** The native property graph retains every named value independently. A neutral semantic
annotation has one optional scalar and one optional position.

**Conflict.** annotation.rs:228-330 selects values by property name and fixed priority, without
checking the expected runtime type for the annotation family. A wrong-type numeric value can
populate the neutral scalar or position.

**Need.** Map scalar and position carriers by exact runtime type and reject contradictory or
wrong-type carriers. Do not use property-name priority as semantic dispatch.

**Note.** FreeCAD Annotation.cpp, DrawViewAnnotation.cpp, and DrawRichAnno.cpp provide expected
property definitions for their types. The current path does not enforce those definitions.

## 4. TechDraw projection

### DG-01. TechDraw runtime-type classification

**Question.** Which exact runtime types enter the TechDraw arena and which drawing kind does each
type represent?

**Known.** Drawing records retain exact runtime type and source order. Core drawing dispatch uses
exact runtime names.

**Conflict.** drawing.rs:23-25 admits every type beginning with TechDraw::, while classify at
147-172 omits standard derived types such as DrawBrokenView, DrawComplexSection, DrawParametricTemplate,
DrawViewMulti, DrawViewArch, and DrawViewDraft. These valid types are admitted as DrawingKind::Other
or receive an incomplete inherited classification.

**Need.** Establish the complete exact TechDraw runtime registry and inheritance/type mapping.
Unknown extension types must remain native without suppressing known standard types.

**Note.** TechDraw PROPERTY_SOURCE declarations establish the omitted derived types and their
base classes. The positive exact list is not exhaustive.

### DG-02. Drawing link, type, and parameter selection

**Question.** What cardinality and precedence rules select drawing templates, scalar/vector
carriers, link properties, and repeated parameters?

**Known.** Drawing records retain all links and source properties. DrawView defines exact base
property types.

**Conflict.** drawing.rs:175-193 and 246-287 select values by name and parse their XML value
without enforcing the expected runtime type. links selects the first same-name property.
Consequently a wrong-type numeric carrier can populate a drawing parameter and conflicting
same-name paths can disagree.

**Need.** Establish TechDraw property definitions, cardinalities, and precedence from FreeCAD
source. Gate every semantic carrier by its exact runtime type and reject contradictory duplicates.

**Note.** DrawView.cpp, DrawPage.cpp, and DrawViewPart.cpp provide the normal base property
definitions. The current generic name/value path does not enforce them.

### DG-03. Drawing numeric and geometric admission

**Question.** Which numeric and geometric invariants must drawing transfer enforce before it
creates neutral view fields?

**Known.** Drawing view position is finite, scale is positive, and projection direction is
finite and nonzero.

**Conflict.** drawing.rs:108-135 and 246-287 validate parseability but not finiteness, scale
sign, or nonzero direction. lib.rs:1238-1239 transfers drawing records without running final
validation. A negative Scale, NaN value, or zero Direction can therefore enter the neutral graph.

**Need.** Enforce numeric and vector invariants during admission or return an explicit loss before
neutral transfer.

**Note.** DrawView.cpp and DrawViewPart.cpp establish the normal property definitions and
direction behavior. The current path permits a hostile value that violates the specification.

## 5. Attachment and assembly
