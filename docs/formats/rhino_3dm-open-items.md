# Rhino 3DM Open Items

Settled format rules remain in
[`rhino_3dm.md`](rhino_3dm.md). OpenNURBS transfer evidence remains in
[`rhino_3dm-opennurbs-comparison.md`](rhino_3dm-opennurbs-comparison.md).

## Remaining items

### FV-02. Archive-90 unregistered tables

**Question.** What records do the archive-90 tables `0x10000027`,
`0x10000028`, and `0x10000029` contain, and which of their fields can be
admitted as typed data?

**Known.** Archive version 90 writes the three tables between the
hatch-pattern table and the instance-definition table. Section 7 settles their
framing: each is an unregistered table that the reader skips as one bounded
unit, and its records remain opaque source records. The OpenNURBS reference
source defines no typecode, record typecode, or class for these tables. The
written tables observed carry only the end-of-table marker.

**Need.** The producer source or an independent witness for each table, with
its record typecode, record grammar, and boundaries, and the field-specific
neutral mapping required by the section-20.6 admission rule.

### QA-02. Nested anonymous CRC admission

**Question.** Which owner validates each CRC-bearing nested anonymous chunk
before transferring its fields?

**Known.** Section 4.1 gives nested CRC-bearing chunks independent boundaries
and makes a mismatch an integrity failure whose fields stay admitted. The
container checks the leaf children of the settings-attributes record (page
units, earth anchor, IO settings, SubD display), the plugin references of the
plugin list, the SubD-display child of the mesh settings, and the deflate
child of the compressed preview, and each named construction-plane child. The
generic userdata reader checks its header and wrapper ranges but excludes the
anonymous payload child at `objects.rs:550-557`. The user-string owner at
`objects.rs:581-620` parses the list and each entry without validating either
anonymous chunk. The layer-extension owner at `settings.rs:752-884` likewise
parses the outer and per-viewport anonymous chunks without validation. The
shared presentation helper at `presentation.rs:1013-1027` admits material,
texture, linetype, dimension-style, and other class-data children without
validating the returned chunk. Source readers close these
`TCODE_ANONYMOUS_CHUNK` records with `EndRead3dmChunk`, including
`ON_UserStringList` at `opennurbs_userdata.cpp:761-820` and
`ON__LayerExtensions` at `opennurbs_layer.cpp:1299-1363`, `1368-1446`, and
`1593-1668`. The same omission is present in the clipping-plane surface and
detail-view anonymous readers at `surfaces.rs:156-192` and `detail.rs:25-70`,
and in the SubD proxy reader at `subd.rs:250-296`; their source writers and
readers also use the anonymous chunk API and close the chunk. The
rendering-attributes reader records mapping-channel ranges for the parent CRC
at `settings.rs:1761-1790` but never validates each channel chunk; the source
`ON_MappingChannel` reader closes the same anonymous chunk at
`opennurbs_material.cpp:7536-7567`.

**Need.** Each remaining nested-chunk reader must validate the CRC-bearing
anonymous chunks it admits and report a mismatch as an integrity failure.

**Note.** A stale user-string entry CRC can still produce a typed user-string
record without a diagnostic, and the same holds for a layer-extension entry,
a material texture, a clipping plane, a detail view, and a SubD proxy. The
generic userdata and parent record checks do not replace the nested checks
because their CRC ranges exclude complete children.

### QA-04. Instance-definition layout selection below archive 50

**Question.** Which packed or anonymous instance-definition layout does an
archive-2, archive-3, or archive-4 record use, and which owner selects it?

**Known.** `rhino_3dm.md` §18 "Instance-definition records are in the
instance-definition table." states that archive versions through 50 use packed
major version 1, minor 6. `rhino_3dm.md` §19.6 "The instance-definition table
record contains the class payload." states the same packed form for archive 50,
and the anonymous V6 form for archives 60 and later. `instances.rs:982-988`
selects the packed layout only when the archive is 50, or when the archive is
60 and the first class-data byte is not zero. An archive-2, archive-3, or
archive-4 record therefore goes to the anonymous reader at `instances.rs:705`.
OpenNURBS selects the packed reader for every archive version through 50 at
`opennurbs_instance.cpp:2304-2331`, and its writer selects the packed writer
under the same rule at `opennurbs_instance.cpp:2294-2302`.

**Need.** The owner must state the archive band that selects each layout, and
the specification must state the same band. If archives 2 through 4 stay
outside typed decoding, the specification must state that boundary in place of
the packed rule.

**Note.** With the current rule, each archive-2, archive-3, and archive-4
instance definition fails its framing check at the anonymous reader, becomes an
opaque record with the retained-definition diagnostic, and supplies no
definition for the instance references that name it.

### QA-05. Legacy annotation archive OpenNURBS version gate

**Question.** Which form does the legacy annotation reader select for an
archive-50-or-later record whose archive OpenNURBS version is below
200710180?

**Known.** The source reader reads the anonymous-chunk form only when the
archive version is at least 5 and the archive OpenNURBS version is at least
200710180, and the direct packed form otherwise, at
`opennurbs_internal_V2_annotation.cpp:1062`. `dimensions.rs` `decode_legacy`
and `annotations.rs` select the direct packed form from the archive version
alone: archives 2 through 4 use it and every later decoded archive uses the
anonymous form. Archive value 5 is refused by the container.

**Need.** The owners must apply the archive OpenNURBS version term, or the
specification must state the CADIR rule for an archive-50-or-later header that
declares an earlier OpenNURBS version, with a witness for that boundary.

### QA-10. Marked CADIR decisions have no decision records

**Question.** Where does each marked CADIR decision record its Question,
Silence, Rule, Ground, Cost, and Reopens fields?

**Known.** `rhino_3dm.md` marks CADIR decisions at sections 7.2.4, 7.2.5,
7.2.6, 7.2.18, 8.3, 13.2, 13.3, 13.4, 15.4, 18.2, 20.1, 20.6, and other
typed-transfer clauses. The working tree contains no Rhino decision-record
document and no record with the required Question, Silence, Rule, Ground,
Cost, and Reopens fields. For example, section 7.2.4 "CADIR decision: neutral
tessellation carries" assigns legacy mesh n-gon grouping to a loss, while
section 20.6 "CADIR decision: a major version" and the following marked
paragraphs define later-major, later-minor, and archive-version admission.
Those clauses state rules and some costs, but do not record the format silence,
grounds, or reopening conditions that make them auditable as project-owned
decisions.

**Need.** A decision record for each distinct marked rule, or a cited format
rule that removes the project-owned decision, with Question, Silence, Rule,
Ground, Cost, and Reopens present in the current tree. Grouped records must
identify every specification clause they own and every named loss or refusal
that charges their cost.

**Note.** A future audit cannot falsify a silence claim or distinguish a
deliberate transfer boundary from an unsupported promoted rule when the
silence, ground, and reopening condition exist only implicitly in prose and
code.

### QA-12. Shipped-guess paths lack discriminating witnesses

**Question.** Which tests fail when the instance-definition layout selection of
QA-04 or the legacy annotation gate of QA-05 chooses the wrong branch?

**Known.** No test contains an archive-2 through archive-4 packed instance
definition or an archive-50-or-later legacy annotation whose archive
OpenNURBS version is below 200710180. The dimension-style arrow-block gate,
the material form selection, the duplicate gradient arbitration, and the
declared writer version have source-derived boundary tests.

**Need.** One independent or source-derived witness per remaining branch
boundary, plus a test that asserts the specification-owned result. Each test
must fail under the current disputed rule before that rule is changed.

### PC-01. Polycurve endpoints moved to an invented midpoint

**Question.** What neutral geometry can transfer from an `ON_PolyCurve` whose
adjacent child endpoints do not agree?

**Known.** `rhino_3dm.md` §12.6 states that the source validity rule rejects a
gap between adjacent children. The same section defines a CADIR midpoint
repair. `crates/cadmpeg-codec-rhino/src/curves.rs:912-931` computes the midpoint
of every unequal adjacent endpoint pair, replaces the last control point of the
preceding segment and the first control point of the next segment with that
midpoint, and reports the distance moved. The joined neutral NURBS therefore
contains neither source endpoint at that join.

**Conflict.** The source object is invalid because it does not define one
continuous join. The decoder creates a continuous curve by changing both
source geometries and admits the result as the neutral carrier.

**Need.** The answer keeps the ordered child carriers and their endpoint gap
without claiming one continuous curve, or defines an explicit repaired
geometry whose invented join and displacement remain queryable on that curve.
The ordinary neutral carrier must not present the midpoint as source geometry.

### QA-13. Instance-definition record CRC coverage

**Question.** Which direct byte ranges does a
`TCODE_INSTANCE_DEFINITION_RECORD` CRC cover?

**Known.** The record is a CRC-bearing table record whose body contains a
complete nested OpenNURBS class chunk. OpenNURBS writes that child between
`BeginWrite3dmChunk(TCODE_INSTANCE_DEFINITION_RECORD, 0)` and
`EndWrite3dmChunk()` at `opennurbs_archive.cpp:11778-11791`. Section 4.1
excludes complete nested chunks from a container CRC. The general table-record
branch in `container.rs:200-211` supplies an empty direct range for the other
class-owning table records, but it omits `TCODE_INSTANCE_DEFINITION_RECORD`.
The fallback at `container.rs:271` therefore hashes the complete nested class
chunk. A stored zero CRC is reported as an integrity failure even though the
record has no direct body bytes.

**Need.** Add the instance-definition record to the class-owning table-record
CRC rule and add a witness whose nested class chunk is nonempty and whose
record CRC is the CRC of its empty direct range.

### AR-01. Non-canonical arc NURBS form misses its arc parameterization

**Question.** Which neutral carrier reproduces `ON_ArcCurve` evaluation at an
arbitrary parameter when the arc is not a full circle on `[0, 2π]`?

**Known.** `ON_ArcCurve::Evaluate` maps its domain linearly onto the arc
angle. `curves.rs` `read_arc` keeps a neutral circle only for a full circle
whose angle and domain are both `[0, 2π]`, and otherwise emits a rational
quadratic NURBS whose knots span the native domain. The rational quadratic
form is not linear in angle, so a point at an interior parameter differs from
the arc point at the same parameter. A Brep edge whose proxy domain is a
proper sub-interval of such an arc then misses its vertex: in the
`flevodam_example.3dm` web file two edges on `[-2π, 0]` arcs miss by up to
1260.79 model units, and the Brep topology falls back with
`topology.brep-fallback`.

**Need.** A carrier or parameter map that reproduces `ON_ArcCurve`
evaluation for every domain and angle interval, applied to object curves,
Brep C3 curves, and Brep C2 curves.

### OF-01. Built-in object families remain opaque

**Question.** Which current built-in Rhino object classes must have a typed
geometry or annotation transfer?

**Known.** `decode.rs:789-843` dispatches classes through the current family
predicates, while `decode.rs:2404-2415` groups every object that remains in the
retained state under `object.family-not-transferred`. Current built-in records
that reach that state include
`ON_Text`, `ON_TextDot`, obsolete V2 text dots and annotation arrows, obsolete
V5 text and leaders, instance references, the legacy `TL_Brep` alias, and
ordinary `ON_Point`, `ON_LineCurve`, and `ON_ArcCurve` records. The V1 path
also retains flat geometry typecodes `0x80400025`, `0x02000014`, `0x02000013`,
`0x00400020`, `0x00200001`, `0x0200000f`, `0x00400010`, and `0x00800001`
without neutral geometry. Some nonempty documents consequently produce no
neutral geometry.

**Need.** Inventory each listed UUID and V1 typecode against its source class,
then distinguish an unsupported dispatch from a failed supported decode and
add the missing typed reader or repair the failing reader. Ordinary point,
line, arc, and Brep aliases must reach their owning geometry decoder and leave
the retained state for every valid versioned payload.

### AN-01. Annotation transfer loses text and style binding

**Question.** Which typed annotation state survives for text objects, text
dots, leaders, arrows, and dimensions?

**Known.** The text, text-dot, leader, and arrow classes listed in OF-01 remain
opaque. Separately, `dimensions.rs:1618-1628` admits a dimension while emitting
`dimension.style-unresolved` when its style UUID does not resolve to a decoded
dimension-style record. The neutral annotation then lacks the source style
binding.

**Need.** Add typed transfers for the built-in annotation families and close
dimension-style references against every versioned dimension-style table
grammar. If a style cannot resolve, retain the serialized reference explicitly
and do not present the annotation as fully styled neutral PMI.

### MS-01. Mesh face topology is reduced during decode

**Question.** How can Rhino quadrilateral and n-gon face identity survive in
neutral tessellation?

**Known.** `decode.rs:3220-3260` always emits triangles with an empty
`triangle_groups` array. It reports every quad through
`mesh.quad-topology-triangulated` and every n-gon through
`mesh.ngon-grouping-dropped`. A mesh with many quads produces one loss per
mesh and no machine-readable relation from the emitted triangles back to the
serialized face. Section 14 currently documents this reduction as the chosen
transfer.

**Need.** Preserve source face grouping for both quads and n-gons, including
the selected quad diagonal, or add a neutral mesh-face carrier that keeps the
original face topology. The loss report alone cannot reconstruct which
triangles belonged to one source face.

### PR-01. Presentation and viewport records have no typed owner

**Question.** Which V1 presentation records and viewport userdata fields can
enter typed CADIR presentation?

**Known.** `legacy.rs:2413-2425` retains V1 presentation typecodes
`0x02000005` and `0x02000006` as opaque records. `views.rs:1008-1055` frames
viewport userdata and reports `viewport.userdata-dropped` when its content has
no typed CADIR owner. Current documents therefore lose view- and
presentation-specific behavior even when the records are bounded and readable.

**Need.** Identify the fields owned by these records and map each field to a
typed presentation or view carrier. Keep only genuinely application-owned
suffixes opaque, with a field-specific loss rather than dropping the complete
readable record semantically.

### SD-01. SubD cache, texture, symmetry, and packing metadata is untyped

**Question.** Which serialized SubD metadata must remain associated with a
typed neutral SubD surface?

**Known.** `decode.rs:2256-2276` commits the SubD surface but reduces cache,
texture, symmetry, or packing state to an `object.decode-diagnostic` warning.
The neutral SubD has no typed representation of those fields and the generic
diagnostic does not identify which individual metadata channels were omitted.

**Need.** Map each source-defined SubD metadata channel to a typed neutral or
native field and emit a field-specific loss for any channel that still has no
owner. Cache data that is only derived acceleration state must be distinguished
from symmetry and packing data that changes editing or parameterization.

### CT-01. Hatch fill and detail-view construction state is passthrough only

**Question.** Which hatch and detail-view semantics are represented by the
neutral model?

**Known.** `decode.rs:1260-1304` transfers hatch boundary curves but records
the fill as native passthrough. `decode.rs:1424-1473` transfers a detail
boundary and a native feature but records the detail view itself as not
transferred. The resulting neutral geometry cannot reproduce hatch fill
appearance or the detail's model-to-page view behavior.

**Need.** Add typed hatch pattern, scale, rotation, base point, gradient, and
loop-role state, and add a typed detail viewport with projection, clipping,
display, and page/model ratio. The boundary curves alone are not equivalent to
either source object.

### IN-01. Instance references are retained when expansion is incomplete

**Question.** What neutral occurrence survives when an instance transform,
definition, or definition member cannot be expanded?

**Known.** Instance diagnostics include non-affine transforms, missing
definitions, undecoded definition members, and transformed procedural
definitions that are omitted while a solved carrier is retained.
`decode.rs:1966-2200` then leaves the instance-reference class in
`object.family-not-transferred`. The document can lose occurrence structure
even when the definition and exact untransformed carriers remain available.

**Need.** Preserve a typed occurrence and its serialized transform and
definition reference independently of geometry expansion. Expansion failures
must affect only derived placed geometry, not erase the source assembly
relationship.

### PF-01. Rhino decode repeatedly validates the full accumulated model

**Question.** What validation work may one object commit perform as the Rhino
model grows?

**Known.** `decode.rs:595-666` calls `admit_with_annotations` for individual
candidate commits. `validate/admit.rs:81-90` implements admission by running
full neutral validation and filtering its findings afterward. The decoder
repeats this full-document walk for successive candidates, and `cadmpeg check`
runs full validation again after decode. On one 26,176,187-byte archive with
69 object records and 27 decoded objects, full check takes 8.36 seconds,
including 8.31 seconds of user CPU time, and reaches about 125 MiB resident
memory. Container-only check takes about 0.02 seconds; checking the emitted
49 MiB CADIR alone takes about 4.7 seconds and reaches about 186 MiB resident
memory. This performance is not acceptable for an interactive check.

**Need.** Make candidate admission proportional to the staged delta and its
affected references, or batch candidates and validate once before commit.
Add a scaling benchmark that varies accumulated entity count and object count,
with a regression ceiling for a document of this size and topology density.
