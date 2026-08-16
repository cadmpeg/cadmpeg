# FreeCAD .FCStd: Open Items

This document records unresolved FreeCAD .FCStd format questions. The specification records
settled byte semantics and invariants.

Each item has an identifier and these fields:

- Question
- Known
- Need
- Conflict
- Note

## 1. Application-specific side entries

### BR-02. Exact-shape side-entry admission

**Question.** Which exact XML value owns each B-rep side entry for a
`Part::PropertyPartShape` property?

**Known.** FreeCAD `PropertyTopoShape::Save` emits a direct `Part file="..."` carrier.
Persistence collects every descendant `file` or `File` attribute into `PropertyRecord.side_entries`.
`container.rs:147-160` classifies every `.brp` or `.brep` archive member as `brep`, and
`brep.rs:525-551` parses every side entry with that role or with a matching descendant `Part`
file attribute.

**Need.** Bind each exact-shape payload to the registered direct `Part` carrier and its owning
property, and derive payload admission from the property/value grammar rather than the archive
extension.

**Conflict.** A shape property containing `<Part file="shape.brp"/><Extra file="other.brp"/>`
causes both entries to enter exact-shape parsing. The second entry can add a payload or fail on
arbitrary bytes even though no direct `Part` carrier names it. The specification states that a
file extension or payload signature does not select an application grammar.

**Note.** New hostile-sweep finding.

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

## 3. Persistent topology identity

### PT-03. Element-map carrier and owner selection

**Question.** Which `Part`, `ElementMap2`, and property carrier belong to one persistent element
map when a shape XML contains more than one candidate?

**Known.** FreeCAD's exact-shape writer emits direct `Part` and `ElementMap2` carriers under the
owning property and associates their file references with that property. The decoder retains the
property XML and map bytes but uses `element_map.rs:86-141` and `unique_descendant` at
`element_map.rs:213-227` to find carriers anywhere below the property.

**Need.** Establish direct-root framing, exact property ownership, and the discriminator that binds
the B-rep side entry to its shape carrier. Reject or retain nested and duplicate candidates without
a source-order choice.

**Conflict.** `<Wrapper><Part file="shape.brp"/></Wrapper>` or a similarly nested `ElementMap2`
passes the descendant lookup even though the producer writes the direct child. `brep.rs:542-550`
also selects the first descendant `Part` file attribute when associating a side entry. A nested
lookalike can therefore become the selected carrier or side-entry owner without an explicit
framing result.

**Note.** Reopened. The closure established source cardinality and duplicate rejection but did not
enforce the producer's direct-root framing or nested-lookalike boundary.

### PT-04. Source topology index provenance

**Question.** What OCCT identity and traversal rules determine whether repeated placed roots or
equal shape-plus-location occurrences receive one shared or multiple indexed-map positions?

**Known.** FreeCAD assigns non-root positions through `TopExp::MapShapes` and uses direct
`TopoDS_Iterator` order for root-shape positions. The specification requires pre-order traversal,
including nested same-kind topology. `topology_transfer.rs:1540-1584` assigns positions by a
decoder-owned walk keyed by shape and composed transform.

**Need.** Match the producer traversal and identity rules for every topology kind, including nested
same-kind compounds, or carry an independently established source position through transfer.

**Conflict.** At `topology_transfer.rs:1560-1568`, a matching shape is indexed and then `continue`
skips its children. For a `Compound` containing a nested `Compound`, the outer compound receives a
position but the nested compound is never visited; later element-map names then bind to missing or
shifted neutral occurrences. The existing depth-first test covers Compound-to-Solid-to-... nesting,
not same-kind nesting.

**Note.** Reopened. The counter-scope correction did not establish or implement the full producer
traversal.

### PT-05. StringHasher owner and root framing

**Question.** Which direct shape-owner `StringHasher` marker and `StringHasher2` successor belong
to one persistent string table?

**Known.** FreeCAD `StringHasher::Save` emits `StringHasher` directly after the shape `Part` and
then emits its immediate `StringHasher2` successor. `element_map.rs:35-83` scans every descendant
`StringHasher`; `owning_property` at `element_map.rs:194-212` checks only the enclosing property
byte range. New-layout markers require an immediate `StringHasher2`, while legacy markers use the
marker itself as the data carrier.

**Need.** Enforce the producer's direct-root ownership and one-table association for both layouts.
Reject nested or duplicate markers instead of assigning a table by descendant traversal order.

**Conflict.** A nested `StringHasher` under an unrelated value in a shape property is accepted and
increments the table index. A legacy marker in that position is parsed as a valid table even
though the producer writes the marker as a direct shape sibling. The resulting names can bind to
the wrong topology map without a malformed-record refusal.

**Note.** New hostile-sweep finding.

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

## 5. Design projection

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

## 8. Product structure

### PR-04. Product placement zero-axis fallback

**Question.** How does product placement admission handle an axis-angle value with a zero-length
axis?

**Known.** `product.rs:979-1079` accepts one `App::PropertyPlacement` value and converts an
axis-angle carrier to a quaternion. When the axis norm is zero, `placement_matrix` substitutes the
Z axis. The specification marks an invalid axis-angle rotation as malformed.

**Need.** Reject a zero-length axis, including a zero axis with a nonzero angle, or establish a
source-defined identity rule and apply it consistently to product, attachment, and joint frames.

**Conflict.** `A=1, Ox=0, Oy=0, Oz=0` becomes a valid rotation about Z instead of a malformed
placement. The product occurrence transform changes without a refusal or loss; a zero-axis,
zero-angle value is also accepted despite the invalid-axis rule.

**Note.** New hostile-sweep finding.

### PR-05. Product metadata value-root framing

**Question.** Which direct value root and property carrier supply product labels, descriptions,
part numbers, and BOM metadata?

**Known.** Product identity transfer at `product.rs:356-366` reads named scalar properties through
`property_scalar` at `product.rs:802-812`, which selects the first retained descendant carrying a
`value` attribute. FreeCAD standard string properties write one direct `String` value. The
application property registry requires an exact runtime type, direct root, canonical attribute,
and cardinality.

**Need.** Establish the runtime type, direct root, attribute, and duplicate-property rule for
each product metadata carrier before projecting it into a definition or BOM field.

**Conflict.** A nested parseable value or a duplicate named property can win by retained-property
or value order and change a neutral label, description, part number, or BOM field. A wrong or
malformed carrier is instead omitted silently, so product identity changes without native
retention or a loss.

**Note.** New hostile-sweep finding.

## 9. Assembly joints

### JN-05. Assembly joint value-root framing

**Question.** Which direct value roots and child containers supply `JointType` and connector
properties when nested lookalikes occur?

**Known.** FreeCAD `PropertyEnumeration::Save` writes one direct `Integer` followed by an optional
`CustomEnumList` containing direct `Enum` values. `PropertyXLink::Save` writes one `XLink` target
whose `Sub` children belong to that target. `joint.rs:286-321` selects integers and enum values from
the retained descendant list; `joint.rs:403-436` checks only the first value tag and parsed link
cardinality.

**Need.** Enforce direct-root and child-container cardinality for `JointType`, `Reference1`, and
`Reference2`. Reject nested or extra carriers before selecting the neutral joint family or operand.

**Conflict.** An `Enum` under an extra nested wrapper inside `CustomEnumList` is included in the
ordinal sequence used by `enumeration_value`, so the selected joint family can differ from the
producer's direct enum list. A nested `XLink` under the accepted root `XLink` is retained but
ignored by `connector`, which still accepts one parsed target. The source order of an invalid
nested carrier changes or silently omits joint state.

**Note.** New hostile-sweep finding.

## 10. Attachment and assembly

## 11. Persistent graph admission
