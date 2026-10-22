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

**Question.** Which test fails when the legacy annotation gate of QA-05
chooses the wrong branch?

**Known.** No test contains an archive-50-or-later legacy annotation whose
archive OpenNURBS version is below 200710180. The packed instance-definition
selection, the dimension-style arrow-block gate, the material form selection,
the duplicate gradient arbitration, and the declared writer version have
source-derived boundary tests.

**Need.** One independent or source-derived witness per remaining branch
boundary, plus a test that asserts the specification-owned result. Each test
must fail under the current disputed rule before that rule is changed.


### OF-01. Built-in object families remain opaque

**Question.** Which current built-in Rhino object classes must have a typed
geometry or annotation transfer?

**Known.** `decode.rs` dispatches classes through the current family
predicates and groups every object that remains in the retained state under
`object.family-not-transferred`. Current built-in records that reach that
state include `TL_Brep` alias records that are members of an unexpanded
instance definition. In `v2_Camera.3dm` the 54 old-polycurve
(`EF638317-154B-11d4-8000-0010830122F0`) records decode, but each curve
candidate reaches `object.framing-undecodable`.

**Need.** Add typed transfers for the listed annotation and instance classes,
route the `TL_Brep` alias to the Brep decoder, and scope candidate admission
so that findings on committed entities outside the candidate do not reject it.

### AN-01. Annotation RTF text has no plain-text fragment

**Question.** Which plain visible text does an `ON_Text` or `ON_Leader` RTF
rich-text string carry?

**Known.** `ON_TextContent::Read` stores one rich-text string. A string that
does not begin with `{\rtf` is plain text. An RTF string is parsed by
`ON_RtfParser` (`opennurbs_textiterator.cpp`) into text runs; the plain text is
the concatenation of those runs. The decoder keeps the complete string as the
annotation format and emits `annotation.rich-text-unflattened` with no plain
text fragment.

**Need.** The `ON_RtfParser` group, destination, control-word, `\uN`, and
`\'hh` rules that produce the run text, so that the plain text comes from the
same parse.

### CT-01. Hatch fill and detail views have no neutral carrier

**Question.** Which neutral carrier holds hatch fill appearance and a detail
view's model-to-page view?

**Known.** The hatch feature holds typed pattern record, scale, rotation,
basepoint, gradient, and loop roles, and the detail feature holds the typed
`ON_3dmView` and page-per-model ratio, both as native feature state. The
neutral model has no filled-region or page-viewport entity, so
`hatch.fill-not-transferred` and `detail.view-not-transferred` remain.

**Need.** A neutral filled-region carrier with pattern and gradient fields, and
a neutral page viewport with projection, clipping, display, and scale.

### IN-01. Instance references without a neutral occurrence

**Question.** What neutral occurrence survives for an `ON_InstanceRef` whose
transform is not affine, or that is a member of a definition no occurrence
places?

**Known.** `ON_InstanceRef` stores a 4×4 `ON_Xform`. The neutral occurrence
placement is an affine 3×4 transform. A reference with a nonzero projective
row has no neutral placement, so it produces no occurrence and no expanded
geometry; the native product-occurrence record keeps the definition UUID. A
reference inside a definition becomes a child occurrence of each placed
occurrence of that definition. The neutral product model has no
definition-to-definition usage without a placed parent, so a reference inside
an unplaced definition has no occurrence and remains a retained object.

**Need.** A neutral placement that carries a projective row, and a neutral
definition-level usage of one definition by another, or a typed record that
keeps both relationships with the unrepresentable part marked.

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

### UN-01. Unit-dependent records without a physical unit binding

**Question.** Which CADIR length scale applies to a document without a
physical unit binding, for every record family?

**Known.** Object geometry and the annotation, grid, and render settings
records use the model coordinates at scale 1.0 and the document carries one
`geometry.document-length-unit-undeclared` loss. Named views, construction
planes, viewport lists, lights, linetypes, hatch patterns, dimension styles,
and annotation text still return their complete source records under
`presentation.record-dropped` or `annotation.record-dropped` when the unit
system is `none` or `unset`. A V2 or later archive without a units record is
classified as `unset`.

**Conflict.** `ON_3dmUnitsAndTolerances::Read` and the default
`ON_3dmSettings` select millimeters when no units record is present. The
codec reports such a document as having no usable unit system.

**Need.** Apply the model-unit scale and the single document loss to the
remaining presentation and annotation records, and bind a missing units
record to the default millimetre system.

### AR-01. Non-canonical arc NURBS form misses its arc parameterization

**Question.** Which neutral carrier reproduces `ON_ArcCurve` evaluation at an
arbitrary parameter when the arc is not a full circle whose angle and domain
start on whole turns?

**Known.** `ON_ArcCurve::Evaluate` maps its domain linearly onto the arc
angle. `curves.rs` `read_arc` keeps a neutral circle for a full circle whose
angle interval and domain each span `2π` and start on whole turns, and Brep
edges on that carrier wrap their proxy interval into the first turn. Every
other arc becomes a rational quadratic NURBS whose knots span the native
domain. The rational quadratic form is not linear in angle, so a point at an
interior parameter differs from the arc point at the same parameter. A Brep
edge whose proxy domain is a proper sub-interval of such an arc then misses
its vertex.

**Need.** A carrier or parameter map that reproduces `ON_ArcCurve`
evaluation for partial arcs and for full circles with a phase offset, applied
to object curves, Brep C3 curves, and Brep C2 curves.
