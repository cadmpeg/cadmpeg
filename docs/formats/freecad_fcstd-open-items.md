# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

## 2. GUI properties

### GP-01. Other GUI property grammars

**Question.** What value grammar remains for each GUI property runtime type not yet covered by the
specification?

**Known.** View-provider properties use the shared application property persistence path. The
current registry in `gui.rs:30-39` classifies GUI links, custom types, and every non-Unknown
`PropertyFamily` as registered. `validate_gui_property` at `gui.rs:824-860` has exact branches for
some types, but no value grammar branch for the remaining link and application-family types.

**Need.** Establish the producer grammar for every classified GUI runtime type. Validate its direct
value roots, cardinality, attributes, and side-entry references before neutral transfer or native
admission. Provider-defined types without a registered grammar must remain opaque.

**Conflict.** A GUI `App::PropertyLink` or `Mesh::PropertyMeshKernel` property can enter the
registered path and return `Ok(())` without a link or geometry-root check. Its arbitrary descendant
values are then retained as if the registry had established their grammar. A nested or duplicate
carrier can therefore pass the typed gate while the neutral presentation path silently withholds
or misreads the value.

**Note.** Reopened. The GUI registry closure established runtime-name coverage and selected source
classes, but its broad registration predicate is not backed by executable grammar checks for all
members.

### GP-02. Other GUI property semantics

**Question.** What presentation semantics remain for each GUI property runtime type after the
settled core and visual-layer subset?

**Known.** The current presentation transfer projects only exact named presentation carriers.
Other registered or provider-defined GUI properties retain their owner, runtime type, ordered
values, and XML in the native arena.

**Need.** Establish the presentation meaning of each remaining runtime type from its defining
source and consumers, or record a per-type native-only decision with the reason that no neutral
field is valid.

**Conflict.** The GUI registry closure makes native retention the fallback for every non-selected
type, but does not establish the semantics of those types. A valid presentation property can be
withheld solely because its runtime type is outside the exact projection list, with no source-backed
decision distinguishing intentional native-only data from an unimplemented neutral mapping.

**Note.** Reopened. Native retention is safe, but it does not answer the remaining semantic
question.

### GP-09. Camera settings carrier and Position cardinality

**Question.** Which serialized camera carrier supplies camera position and orientation, and what
cardinality rule applies to any XML `Position` values?

**Known.** FreeCAD `Gui::Document::SaveDocFile` writes one self-closing `Camera` element whose
`settings` attribute contains the serialized Inventor camera state. The decoder retains that
attribute and all descendants in `gui.rs:662-704`, while `camera_state_value` at `gui.rs:495-541`
searches descendant `Position` elements and an `orientation` attribute.

**Need.** Establish the `settings` grammar and its `SoCamera` field cardinality, then parse or
explicitly retain the authoritative carrier. Any XML compatibility form must have an attributable
selection rule and duplicate handling.

**Conflict.** A real FreeCAD camera's position and orientation are inside the `settings` string, not
in descendant `Position` elements. The one-`Camera` gate therefore accepts the record while the
neutral camera omits persisted position and orientation; the duplicate-`Position` refusal only
guards synthetic XML that the producer does not write.

**Note.** Reopened. The camera cardinality fix does not bind to the producer's authoritative value
carrier.

## 4. Exact-topology transfer

### XT-01. Edge endpoint child selection

**Question.** What child-use cardinality and orientation grammar does a producer-valid degenerate
edge use, and which malformed endpoint forms are invalid?

**Known.** Exact-shape records retain ordered and oriented topology children. Normal and closed
edges use one `Forward` and one `Reversed` endpoint use, and a degenerate edge can reuse one vertex
identity in those orientations. `topology_transfer.rs:1677-1707` rejects duplicate orientations
and requires both endpoint directions.

**Need.** Establish the valid degenerate form and the malformed duplicate, missing, and extra
endpoint forms from the producer or OCCT writer. Apply the rule to every valid form and retain an
attributable refusal for invalid forms.

**Conflict.** The rejection in `edge_endpoint_uses` is a decoder policy. The closure evidence
establishes one valid degenerate witness but does not establish that every duplicate, missing, or
extra orientation is producer-invalid. A legal edge form outside the two-orientation subset is
therefore refused without a source-backed loss or compatibility rule.

**Note.** Reopened. The valid case is narrower than the original cardinality question.

### XT-04. P-curve composed-location equality

**Question.** What equality rule selects a p-curve representation when its surface matches but
its composed carrier location differs from the face location?

**Known.** The specification requires the first p-curve whose surface and composed location equal
the face surface. `topology_transfer.rs:1217-1240` selects the first matching representation, and
`transforms_equal` at `topology_transfer.rs:1644-1650` treats matrix components within `1.0e-12`
as equal.

**Need.** Establish the producer or kernel equality rule and apply it to duplicate p-curve
representations. A tolerance must be source-backed and specified; otherwise the comparison must
be exact.

**Conflict.** Two p-curve representations whose composed locations differ by less than
`1.0e-12` are treated as equal, so serialized order selects the neutral p-curve even though the
specification's equality rule is exact. Swapping those representations changes the neutral result
without a refusal or loss.

**Note.** New hostile-sweep finding.

### XT-05. Neutral topology transform equality

**Question.** What exact location equality and identity rule selects neutral topology and located
geometry identities when equal shapes occur at nearby composed locations?

**Known.** The specification gives a distinct indexed position to a shape at a different composed
location. `topology_transfer.rs:146-168` uses tolerant `OccurrenceKey` values for neutral vertices
and edges; `topology_transfer.rs:1135-1181` uses the same tolerance for located curves and
surfaces. `transform_digest` at `topology_transfer.rs:1612-1627` rounds matrix components to
`1.0e-11`, and `transforms_equal` at `topology_transfer.rs:1644-1650` treats components within
`1.0e-12` as equal. The source-index map uses an exact transform digest, so it does not repair
the neutral identity collapse.

**Need.** Establish the producer or kernel equality rule for composed locations and apply it to
all neutral identity and identity-elision decisions. A decoder tolerance must be source-backed and
specified; otherwise distinct locations must remain distinct.

**Conflict.** Two uses of one shape at translations of `2.0e-12` and `4.0e-12` are different
locations but round to the same transform digest and reuse one neutral edge, vertex, curve, or
surface identity. A location within `1.0e-12` of identity is also elided as identity. The neutral
topology can therefore collapse or omit source-distinct locations without a refusal or loss,
despite the specification's separate-location rule.

**Note.** New hostile-sweep finding.

## 5. Design projection

### DP-02. Sketch profile seed order

**Question.** Which neutral seed rule applies when the producer does not persist a profile-chain
seed?

**Known.** FreeCAD persists ordered `GeometryList` and `ConstraintList` values, but no profile
chain or seed entity. The current projection at `design.rs:2527-2555` starts each disconnected
profile at the lowest unassigned non-construction entity ordinal.

**Need.** Establish the neutral seed rule and retain the persisted entity ordinal in the decision,
or define an explicit decoder-owned policy with an attributable result for an unsupported or
ambiguous seed.

**Conflict.** The closure evidence in `d61600a25` establishes source order and the absence of a
seed carrier, but it does not establish that the lowest ordinal is the neutral seed. The current
`BTreeSet::pop_first()` choice is a decoder policy, so exchanging serialized geometry order can
change profile order without a producer-defined tie-break or loss.

**Note.** Reopened by closure audit. Producer field absence does not settle neutral projection
ownership.

### DP-03. Sketch profile junction ambiguity and tolerance

**Question.** What neutral endpoint-equivalence and junction policy applies when the producer
persists coordinates with optional constraint operands but no junction tolerance or tie-break?

**Known.** FreeCAD persists ordered geometry and constraint operands but no generic endpoint
junction tolerance or junction-selection field. `design.rs:2735-2745` uses a decoder constant of
64 scaled machine epsilons for coordinate matching, after explicit relations are considered.

**Need.** Establish endpoint equivalence and the admissible profile topology. An ambiguous junction
must use constraint identity, an explicit source-order rule, or an attributable refusal.

**Conflict.** The `64 × f64::EPSILON` boundary and the separate-seed ambiguity policy are not
producer rules. They are pinned by decoder tests and a witness that exercises the chosen boundary,
not by a source tolerance or a complete topology contract. A near-coincident endpoint can therefore
change profile connectivity solely when it crosses a decoder-owned threshold.

**Note.** Reopened. The closure established producer field absence, not the neutral numeric policy.

### DP-05. Dependency-cycle ordinal fallback

**Question.** What neutral projection applies when feature dependencies, parents, or expressions
form a cycle?

**Known.** FreeCAD persists directed dependency cycles in the native graph. The current ordinal
assignment at `design.rs:679-694` selects remaining cycle-affected objects in source order, and
the dependency filter at `design.rs:450-456` keeps only targets with earlier neutral ordinals.

**Need.** Define a stable neutral cycle projection that preserves the admissible relation set and
its source provenance, or retain the affected relation and operation as native with an explicit
blocking result. Do not silently discard cycle edges by a decoder-owned source-order rule.

**Conflict.** The closure evidence in `f6fd9df86` establishes that FreeCAD can persist reciprocal
links and that recompute requires a DAG, but it does not establish source-order ordinal assignment
or earlier-target edge discard as the neutral result. Reordering the persisted objects can therefore
change the neutral dependency subset while the native cycle remains the same.

**Note.** Reopened by closure audit. Native cycle retention and a blocking loss do not establish
the neutral relation projection.

### DP-07. Legacy point carrier provenance

**Question.** Does any FreeCAD producer version write a declared `Part::GeomPoint` with a `Point`
carrier instead of the current `GeomPoint` carrier?

**Known.** The current `PropertyGeometryList::Save` writes the geometry runtime name and
`GeomPoint::Save` writes a `GeomPoint` carrier. The decoder accepts `Point` as a compatibility
carrier for `Part::GeomPoint`.

**Need.** Establish a producer source path or independent witness for the historical `Point` tag,
including producer version and value grammar, or remove the compatibility admission.

**Conflict.** The closure proves the current runtime-name/carrier mapping and one early source
revision, but that subset does not answer the question about any supported historical producer.
The compatibility path remains an unproven alias.

**Note.** Reopened. Current-version evidence is a subset closure of a historical-version question.

### DP-09. Spreadsheet carrier and value-container selection

**Question.** Which property and XML value container supply spreadsheet cells and row or column
dimensions when more than one candidate matches the spreadsheet selectors?

**Known.** FreeCAD writes one direct `Cells`, `ColumnInfo`, or `RowInfo` root for the exact
spreadsheet property runtime type. `design.rs:771-798` and `design.rs:906-930` select unique
properties but search for the value roots through all descendants.

**Need.** Enforce the producer's direct-root framing and exact property/value cardinality for cells,
column widths, and row heights. Reject nested lookalikes before projecting spreadsheet records.

**Conflict.** `<Wrapper><Cells Count="...">...</Cells></Wrapper>` and an analogous wrapper around
`ColumnInfo` or `RowInfo` are accepted and projected. A nested lookalike can therefore supply cells
or dimensions despite the producer's direct-root rule; existing tests cover duplicate roots but not
nested framing.

**Note.** Reopened. The uniqueness fix does not establish or enforce root ownership.

### DP-10. Design value-root framing

**Question.** Which direct value root and cardinality supply named design scalar, vector, and list
properties when nested candidates are present?

**Known.** FreeCAD standard scalar writers emit one direct `Float`, `Integer`, or `Bool` root, and
the sketch geometry and constraint writers emit direct `GeometryList` and `ConstraintList` roots.
Persistence retains every descendant value. `design.rs:3898-3903` and `design.rs:5132-5157` select
the first descendant value with a parseable attribute, while `design.rs:2224-2256` finds the first
descendant list container and later loops over all descendant records.

**Need.** Establish each design property's exact runtime type, direct root tag, cardinality, and
record ownership. Reject or retain nested and duplicate value carriers before neutral feature,
parameter, or sketch transfer.

**Conflict.** A named scalar property with a nested parseable `Float` before its direct `Float`
root is projected from the nested value instead of being rejected. A nested `GeometryList` or
`ConstraintList` with a valid local count is also accepted. Nesting or reordering parseable values
can therefore change a neutral design value without a loss.

**Note.** New hostile-sweep finding.

### DP-11. Post-processing control fallback

**Question.** What admission rule applies when a design operation carries a malformed `Refine` or
`FuzzyTolerance` control?

**Known.** `design.rs:699-745` returns the underlying operation when
`post_process_controls` cannot resolve both controls. The helper uses generic descendant scalar
and boolean extraction, and returns `None` for malformed or non-finite fuzzy tolerance values.
The specification says post-processing controls are retained compositionally around the neutral
operation.

**Need.** Distinguish an absent control from a malformed present control. Enforce each control's
runtime type, direct value root, cardinality, and finite value before wrapping the operation, or
retain an attributable native operation with a loss.

**Conflict.** A malformed or non-finite `FuzzyTolerance`, or a wrong-carrier `Refine`, silently
drops the post-processing wrapper while the underlying operation remains neutral. Changing the
nested control or its spelling therefore changes neutral state without a refusal or loss.

**Note.** New hostile-sweep finding.

### DP-12. Sketch placement zero-axis fallback

**Question.** Is a zero-length axis-angle axis a valid sketch placement when its angle is zero?

**Known.** The specification rejects an invalid axis-angle rotation as a sketch frame. In
`design.rs:1511-1573`, `placement_frame` converts an `A` carrier with a zero axis and zero angle
to the identity quaternion; `validate_sketch_placement` at `design.rs:1575-1595` accepts the
result because the frame is present.

**Need.** Reject a zero-length axis for every axis-angle sketch placement, including zero angle,
or retain the affected sketch or datum operation as native with an attributable loss.

**Conflict.** A `Placement` or `AttachmentOffset` with finite position, `A=0`, and
`Ox=Oy=Oz=0` becomes an identity sketch frame instead of remaining invalid. The neutral frame
therefore changes without a refusal or loss.

**Note.** New hostile-sweep finding.

### DP-13. ExternalGeo cached-carrier prefix admission

**Question.** Which leading `ExternalGeo` records are reserved, and how must the cached-carrier
list correspond to `ExternalGeometry` links?

**Known.** `design.rs:1214-1227` validates the declared count against direct `Geometry` children,
then scans all descendant `Geometry` records and unconditionally skips two before pairing the
remaining records with `ExternalGeometry` links. FreeCAD's sketch representation reserves two
leading external-geometry slots, while the specification requires any supplied cached carrier to
define the solved external entity.

**Need.** Validate the reserved prefix, direct list framing, cache cardinality, and link/cache
ordinal correspondence before emitting external sketch entities. Reject malformed short or
misframed lists instead of treating an ignored cache as absent.

**Conflict.** A `GeometryList count="1"` containing one valid cached `Circle` passes the count
check, but `.skip(2)` drops it; a corresponding link is then emitted as an unresolved external
reference. A supplied solved carrier is ignored without a refusal or loss.

**Note.** New hostile-sweep finding.

### DP-14. Sketch constraint family-code default

**Question.** How does constraint transfer distinguish an absent or malformed family code from an
explicit disabled constraint?

**Known.** The specification retains the persisted constraint family code and leaves invalid or
future families native. `design.rs:1680-1686` parses `Constrain@Type` with
`int_attr(...).unwrap_or(0)`, and `neutral_constraint` at `design.rs:2057-2071` interprets code `0`
as `Disabled`.

**Need.** Require a present integer family code, preserve explicit code `0` as its own value, and
retain missing or malformed codes as attributable native relations.

**Conflict.** `<Constrain First="0" FirstPos="0"/>` or `Type="bad"` defaults to code `0` and
projects a neutral `Disabled` constraint. A malformed relation therefore changes neutral state
without a refusal or loss.

**Note.** New hostile-sweep finding.

### DP-15. Design operation selector fallback

**Question.** How are absent and malformed design operation mode or flag properties distinguished
before selecting a neutral operation family?

**Known.** `design.rs:3898-3918` extracts the first parseable generic value attribute without a
runtime-type or direct-root gate. Callers default an absent or malformed selector at
`design.rs:2809`, `3309`, `3334`, `4061`, `4310`, and `4336` before choosing revolution,
extrusion, boolean, hole, or other operation modes. The specification requires invalid modes to
remain attributable native operations.

**Need.** Validate each named selector's runtime type, direct root, cardinality, and value before
applying a legacy absent-property default. A present malformed selector must leave the operation
native or produce an attributable refusal.

**Conflict.** A `PartDesign::Boolean` with a valid source group and a nonnumeric `Type` carrier
defaults to `Join`; a malformed `PartDesign::Revolution` mode similarly defaults to angular
termination. Replacing the malformed carrier with a valid explicit mode changes neutral semantics
without a refusal or loss.

**Note.** New hostile-sweep finding.

## 6. Semantic annotations

### SA-03. Annotation value-root framing and attribute selection

**Question.** Which direct value root and canonical attributes supply each registered annotation
text, scalar, vector, and format property?

**Known.** Registered annotation properties have exact runtime types. Standard property writers
emit one direct root: `String` uses `value`, `PropertyVector` uses `valueX`, `valueY`, and `valueZ`,
`Float` uses `value`, and `StringList` owns its ordered direct `String` children. The persistence
layer retains every descendant as a `ValueRecord`. `annotation.rs:50-55` collects text from every
retained descendant, while `annotation.rs:318-376` selects scalar, vector, and format values by
retained-value count without checking the direct root tag. `annotation.rs:448-473` also accepts
capitalized and generic text attributes that the registered property grammar does not define.

**Need.** Enforce the direct root tag, root cardinality, owned child grammar, and canonical
attributes for every registered annotation carrier before neutral transfer. Reject nested roots,
unexpected descendants, and simultaneous or unsupported attribute spellings.

**Conflict.** A registered scalar property with one nested parseable `Float` and no direct
`Float` root passes `unique_value` and supplies a neutral position. A format `String` with both
`value` and `Value` silently selects `value`, and an annotation text property can collect text from
an unrelated nested descendant. Invalid nesting or attribute spelling can therefore create or
change a neutral annotation without a loss.

**Note.** New hostile-sweep finding.

## 7. TechDraw projection

### DG-05. Drawing scalar attribute spelling

**Question.** Which attribute spelling supplies a registered TechDraw scalar value?

**Known.** The registered application-property grammar and FreeCAD scalar writers use the
lowercase `value` attribute. `drawing.rs:550-607` enforces the direct value-root tag and
cardinality, but `drawing.rs:629-634` accepts `Value` when `value` is absent and selects `value`
when both occur.

**Need.** Enforce the producer's canonical scalar attribute and reject unsupported or duplicate
spellings before transferring position, scale, or rotation.

**Conflict.** `<Float Value="2"/>` supplies a neutral drawing scalar although it is outside the
settled property grammar. `<Float value="1" Value="2"/>` silently selects `1`; deleting one
attribute changes the result from `1` to `2` instead of making the contradictory carrier invalid.

**Note.** New hostile-sweep finding.

### DG-06. Non-page template relationship admission

**Question.** Which drawing runtime types may populate the neutral page-template field?

**Known.** `drawing.rs:32-45` extracts typed `Views` and `Template` carriers only for page
objects, but `drawing.rs:66-70` retains every link-valued property as a relationship. Neutral
transfer at `drawing.rs:175-190` then reads any relationship named `Template` for every registered
drawing record. The specification limits page template carriers to pages and states that other
runtime types do not supply them.

**Need.** Gate neutral `Drawing.template` extraction on the page runtime kind. Retain a non-page
`Template` relationship as native relationship data without interpreting it as page membership.

**Conflict.** A registered non-page view with an `App::PropertyLink` named `Template` targeting a
registered template populates `Drawing.template`, although the source type cannot own a page
template carrier. An unrelated link property therefore changes a neutral page field without a
refusal or loss.

**Note.** New hostile-sweep finding.

## 9. Assembly joints

## 10. Attachment and assembly

## 11. Persistent graph admission
