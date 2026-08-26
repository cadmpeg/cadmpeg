# Rhino 3DM Open Items

Settled format rules remain in
[`rhino_3dm.md`](rhino_3dm.md). OpenNURBS transfer evidence remains in
[`rhino_3dm-opennurbs-comparison.md`](rhino_3dm-opennurbs-comparison.md).

## Remaining items

### FV-01. Future object-class payloads

**Question.** What field grammar does each later built-in object-class major
define, and which of its fields can be admitted as typed data?

**Known.** The typed readers consume the specified class prefixes and bounded
minor suffixes. The current producer paths define the object-class families
specified in sections 12 through 18; they do not define a later object-class
major. Section 20.6 settles the CADIR half: a registered class UUID and a
retained source record do not admit a later major, and the complete containing
record remains opaque until its grammar and neutral mapping are specified.

**Need.** A producer implementation or independent versioned witness for each
later object-class major that is to enter typed decoding, with its field
grammar and boundaries. The same change must state the field-specific neutral
mapping required by the section-20.6 admission rule.

**Note.** The absence of a current later producer does not settle a future
layout. Opaque retention and refusal are settled CADIR decisions, not typed
compatibility evidence.

### FV-02. Future table-record payloads

**Question.** Which later table-record versions retain typed decoding, and what
fields do changed versions add or change?

**Known.** Current producer-defined additions include the direct V2/V3
material major-1 minor-1 grammar, dimension-style minor-10 and minor-11
fields, the archive-90 hatch-pattern tail, and layer extension item 37. These
individual branches do not define later table-record layouts in general.
The current table-record families use the bounded branches specified throughout
this document, including sections 8.3, 18, and 20.2 through 20.4; no current
writer defines a later table-record major or changed layout outside those
branches. Unknown table records remain bounded source records without typed
fields from their typecode alone. Section 20.6 settles the CADIR half: a later
table-record major or changed layout is not typed from its record typecode until
its grammar and neutral mapping are specified.

**Need.** Producer source or an independent witness for each later table-record
major or changed layout that is to be admitted as typed data, including field
order, boundaries, defaults, and normalization. The same change must state the
field-specific neutral mapping required by the section-20.6 admission rule.

**Note.** The dimension-style, hatch, and layer closures each settled only a
subset of this item. The later archive-90 and layer reopenings show that an
individual current field is not complete table-record coverage. Opaque
retention and refusal are settled CADIR decisions, not evidence that a future
layout has been characterized.

### FV-03. Future user-data payloads

**Question.** Which later user-data versions have a typed payload grammar, and
which fields remain admissible?

**Known.** The generic user-data headers and the audited class-owned payloads
have bounded children and source-defined minor gates. Unknown classes can be
retained, but the generic header does not define their payload fields. The
current OpenNURBS writer emits generic header version 2.2; its class-owned
payload writers remain the audited carriers and do not define a later
user-data major or an untyped class-specific payload grammar beyond them.
Section 20.6 settles the CADIR half: an unknown class or later major remains a
complete opaque userdata-bearing record, and the generic header never supplies
typed fields for the class-owned payload. The light-table attributes owner is
also proven to retain the complete record when the registered
`ON_UserStringList` payload has an unsupported anonymous major; independent
light fields remain typed and the user-string fields are omitted.
The material-table owner is proven to retain the complete record when the
registered `ON_PhysicallyBasedMaterialUserData` payload has an unsupported
anonymous major; independent material fields remain typed and the
physically-based value is omitted.
The V5 dimension-style owner is proven to retain the complete record when the
registered `ON_DimStyleExtra` payload has an unsupported anonymous major;
independent packed dimension-style fields remain typed and the extra value is
omitted.
The material-table owner is also proven to retain the complete record when the
registered `ON_RdkUserData` payload uses the callback-owned version-2 UTF-8
form; the userdata supplies no typed fields, independent material fields remain
typed, and the owner emits a presentation loss.
The object owner is proven to retain the complete object record when the
registered `ON_UserStringList` payload has an unsupported anonymous major;
independently admitted object geometry and attributes remain typed, the
user-string fields are omitted, and the owner emits an object decode loss.
The top-level mesh owner is proven to retain the complete object record when
the registered `ON_SubDMeshProxyUserData` payload has an unsupported anonymous
major; the parent tessellation remains typed, no SubD entity is admitted, and
the owner emits a decode warning for the failed proxy.
The Brep owner is proven to retain the complete object record when the
registered `ON_V5_BrepRegionTopologyUserData` payload has an unsupported
anonymous major; independent Brep geometry remains typed, the optional region
carrier is discarded, and the owner emits the repair warning.
The Brep nested-mesh owner is proven to retain the complete object record when
the registered `ON_V4V5_MeshNgonUserData` payload has an unsupported anonymous
major; the nested mesh tessellation remains typed, no n-gon grouping is
admitted, and the owner emits the mesh userdata warning.
The V5 extrusion display-mesh cache owner is proven to retain the complete
extrusion object record when a cached mesh carries the same registered
`ON_V4V5_MeshNgonUserData` payload with an unsupported anonymous major; the
analytic extrusion and cached tessellation remain typed, and the owner emits
the nested mesh userdata warning.
The texture-mapping owner is proven to retain the complete table record when a
custom mapping primitive carries a registered `MappingCRCCache` payload with
an unsupported anonymous major; the mapping fields and primitive class remain
typed, no `mapping_crc` field is admitted, and the owner emits a presentation
loss.
The top-level mesh owner is also proven for the registered
`CTtMappingMeshInfoUserData` and `CTtRenderMeshInfoUserData` carriers: when
either bounded payload has an unsupported anonymous major, the parent
tessellation remains typed, correspondence state is omitted, the complete
mesh object record is retained, and the owner emits a decode warning naming
the carrier.
The render-settings owner is proven to retain a complete framed
`TCODE_SETTINGS_RENDER_USERDATA` record when its registered class-owned
anonymous payload has an unsupported major; the typed render-settings record
remains, and no class-owned payload fields enter native data.
The viewport owner is proven to retain the complete containing named-view list
record when a framed `TCODE_VIEW_VIEWPORT_USERDATA` stream has a registered
class-owned anonymous payload with an unsupported major; the typed view and
viewport remain, and no viewport-userdata fields enter native data.
The layer owner is proven to retain the complete `TCODE_LAYER_RECORD` when a
framed `ON__LayerExtensions` payload has an unsupported outer anonymous major
or cannot be parsed; the typed layer remains, its per-viewport array is empty,
and no class-owned userdata fields enter native data.
The hatch owner is proven for the registered `ON_GradientColorData` carrier:
its current major-1 grammar supplies the native gradient parameter, while an
unsupported or malformed gradient payload leaves the hatch class data and loop
curves typed, omits that parameter, emits the hatch-userdata diagnostic, and
retains the complete `TCODE_OBJECT_RECORD`.
The object-attributes owner is proven for the registered
`ON_PerObjectMeshParameters` carrier: its current outer major-1 grammar
supplies the owning presentation's `custom_render_mesh` value, while an
unsupported outer major or malformed nested child leaves the point class data
and object attributes typed, omits that value, emits the bounded userdata
diagnostic, and retains the complete `TCODE_OBJECT_RECORD`.
The same object owner is proven across the five registered mesh-modifier XML
carriers: current XML userdata version 2 reaches the matching native modifier,
while version 3 leaves the point and attributes typed, omits the modifier,
emits the carrier-specific bounded diagnostic, and retains the complete object
record.

The mesh owner is proven for `ON_V5_MeshDoubleVertices`: a current major-1
payload supplies the synchronized f64 vertex array, while an unsupported major,
malformed child, or count mismatch leaves the float mesh typed, omits only the
redundant array, emits the bounded repair diagnostic, and retains the complete
object record.

The instance-definition owner is proven for the registered
`ON_OBSOLETE_IDefAlternativePathUserData` carrier: its current major-1 payload
trims the UTF-16 path and fills only the selected empty linked-definition slot;
an unsupported major or malformed payload leaves the linked definition typed,
omits the alternate path, emits the bounded diagnostic, and retains the
complete definition record in source fidelity.
The V5 text owner is proven for the registered `ON_OBSOLETE_V5_TextExtra`
carrier: its current anonymous major-1 child supplies the nil-parent, mask,
color, and border fields; an unsupported major or malformed child leaves the
legacy text annotation typed, omits `v5_text_extra`, emits the annotation
userdata loss, and retains the complete object record.
The obsolete custom-mesh owner is proven for the registered
`ON_OBSOLETE_CCustomMeshUserData` carrier: its direct legacy fields transfer
to the object presentation's `custom_render_mesh`; a bounded mesh-parameter
failure leaves point geometry and attributes typed, omits that presentation
field, emits the bounded userdata diagnostic, and retains the complete object
record.
The V5 dimension owner is proven for the registered
`ON_OBSOLETE_V5_DimExtra` carrier: its anonymous 1.2 child supplies forced
arrow position, detail distance scale, and measured-detail UUID to a legacy
linear dimension; an unsupported child major or truncated child retains the
complete object record, admits no dimension, and emits the bounded dimension
diagnostic. The class and item UUID must both match before the carrier is
admitted.
The separate V5 angular owner is proven for the registered
`ON_AngularDimension2Extra` carrier: its anonymous 1.0 child supplies the two
extension-line origin offsets in order; an unsupported child major or
truncated child retains the complete object record, admits no angular
dimension, and emits the bounded dimension diagnostic. The class and item UUID
must both match before the offsets are applied.
The V5 hatch owner is proven for the registered
`ON_OBSOLETE_V5_HatchExtra` carrier: its anonymous 1.0 child supplies the
serialized base point for a packed V5 hatch; an unsupported child major or
truncated coordinate retains the complete object record, keeps the hatch and
loop curve typed, leaves the base point at `[0,0]`, and emits the bounded hatch
userdata diagnostic. The class and item UUID must both match before the base
point is applied.
The obsolete layer-settings owner is proven for both
`ON_OBSOLETE_IDefLayerSettingsUserData` and
`ON_OBSOLETE_LayerSettingsUserData`: their shared reader consumes one
anonymous child without reading fields and deletes the userdata after reading.
A well-framed child leaves the typed layer unchanged, creates no
per-viewport-settings field, and emits no layer-userdata loss, including when
the child major is later than the current writer major. If the generic wrapper
is framed but the obsolete child is absent or malformed, the userdata item is
discarded and the typed layer remains unchanged.
The generic unregistered class boundary is also proven: a point object with an
unregistered class, item, and application UUID in generic userdata 2.2 retains
typed point geometry and its complete object record without typed userdata
fields; a later generic userdata header has the same result.

The current built-in class-userdata inventory is complete. The registered
object, table, geometry, presentation, view, layer, and render-settings
carriers are covered by the owner rules above. `ON_AnnotationTextFormula` is
not an archived carrier: its source class forbids `Write` and `Read`, and the
formula is the direct minor-2 UTF-16 field in the legacy annotation record.
No current OpenNURBS writer defines another class-owned userdata major or
unlisted carrier.

**Need.** A later user-data class writer and reader, or an independent witness,
for each version that is to be typed, including its fields and boundaries. The
same change must state the field-specific loss or neutral mapping required by
the section-20.6 admission rule. The current-producer audit is complete; only a
future class-owned grammar can answer the remaining format question.

**Note.** No current later producer is evidence that future class-specific
payload semantics are settled. Opaque retention and refusal are settled CADIR
decisions, not evidence that a future payload has been characterized.

### FV-06. Later major payload grammar

**Question.** What wire fields and boundaries does a later major version of a
built-in table, object, geometry, presentation, or user-data payload define?

**Known.** Section 20.6 retains an unknown major record and withholds typed
fields until a grammar and neutral admission rule exist. The current producer
inventory defines no additional major family. The CADIR half is settled in
section 20.6: the codec never applies a known-major prefix to an undefined
major and retains the complete containing record.

**Need.** Producer source or an independent witness for each later major that
is to be admitted, naming its fields, boundaries, and validation. The same
change must add the field-specific neutral mapping before typed admission.

**Note.** This item retains only the format half of the question. Opaque
retention and refusal are settled CADIR decisions, not evidence that a later
major has been characterized.

### FV-07. Later minor payload suffixes

**Question.** Which fields and boundaries do future minor versions append after
the known prefix of each built-in payload?

**Known.** Source-defined later-minor fields are consumed at their version
gates, and bytes after a known prefix remain bounded source bytes. A generic
future-minor policy does not assign names or meanings to a suffix that no
audited producer writes. Section 20.6 settles the CADIR half: an undefined
minor suffix receives no typed field or neutral value and remains in its
containing bounded payload.

**Need.** A future producer writer and reader, or an independent witness, for
each suffix that is to be typed, with its field order, boundary, and
validation. The same change must state the field-specific neutral mapping
before typed admission.

**Note.** The current producer inventory and bounded retention policy do not
settle future suffix field semantics. Opaque retention and refusal are settled
CADIR decisions, not evidence that a future suffix has been characterized.

### QA-01. Named construction-plane child CRC admission

**Question.** How does the named-construction-plane owner validate the CRC of
each counted `TCODE_VIEW_CPLANE` child before admitting its typed fields?

**Known.** Section 4.1 defines a CRC-bearing leaf as covering its direct body,
and section 20.4 states that the named-construction-plane list CRC excludes
each complete child while each `TCODE_VIEW_CPLANE` child has its own CRC.
`views.rs:1235-1248` frames and parses each child but does not call
`direct_view_child_checksum_warning`. `container.rs:211-219` validates only
the parent list's direct ranges. OpenNURBS reads each child through
`EndRead3dmChunk` after `BeginRead3dmBigChunk` at
`opennurbs_3dm_settings.cpp:5663-5672`.

**Need.** The owner must validate each child CRC and apply the documented
recoverable integrity-warning or retention policy before transferring the
construction plane.

**Note.** If a child body changes while its child CRC is stale and the parent
list CRC is recomputed over its unchanged direct fields, the current parser
admits the changed `parse_cplane` result into `construction_planes` without an
integrity finding.

### QA-02. Nested anonymous CRC admission

**Question.** Which owner validates each CRC-bearing nested anonymous chunk
before transferring its fields?

**Known.** Section 4.1 gives nested CRC-bearing chunks independent boundaries.
The generic userdata reader checks its header and wrapper ranges but excludes
the anonymous payload child at `objects.rs:550-557`. The user-string owner at
`objects.rs:581-620` parses the list and each entry without validating either
anonymous chunk. The layer-extension owner at `settings.rs:752-884` likewise
parses the outer and per-viewport anonymous chunks without validation. The
shared settings helper at `settings.rs:1193-1210` admits anonymous plugin,
earth-anchor, IO-settings, SubD-display, and settings-attributes children
without validating the returned chunk; the shared presentation helper at
`presentation.rs:1013-1027` does the same for material, texture, linetype,
dimension-style, and other class-data children. Source readers close these
`TCODE_ANONYMOUS_CHUNK` records with `EndRead3dmChunk`, including
`ON_UserStringList` at `opennurbs_userdata.cpp:761-820`,
`ON__LayerExtensions` at `opennurbs_layer.cpp:1299-1363`, `1368-1446`, and
`1593-1668`, plugin references at `opennurbs_pluginlist.cpp:50-80` and
`117-168`, and settings children at
`opennurbs_3dm_settings.cpp:4178-4261` and `5110-5135`.
The same omission is present in the clipping-plane surface and detail-view
anonymous readers at `surfaces.rs:156-192` and `detail.rs:25-70`, and in the
SubD proxy reader at `subd.rs:250-296`; their source writers and readers also
use the anonymous chunk API and close the chunk.
The rendering-attributes reader records mapping-channel ranges for the parent
CRC at `settings.rs:1761-1790` but never validates each channel chunk; the
source `ON_MappingChannel` reader closes the same anonymous chunk at
`opennurbs_material.cpp:7536-7567`.

**Need.** Each registered nested-chunk reader must validate the CRC-bearing
anonymous chunks it admits and route a mismatch through that owner's warning,
drop, or opaque-retention rule.

**Note.** A stale user-string entry CRC can still produce a typed user-string
record, a stale layer-extension entry CRC can still produce a typed
per-viewport setting, and a stale plugin-reference, material-texture,
clipping-plane, detail-view, or SubD-proxy CRC can still produce its typed
child. The generic userdata and parent record checks do not replace the nested
checks because their CRC ranges exclude complete children.

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

### QA-05. Archive OpenNURBS version gates that owners do not apply

**Question.** Which record owners must read the archive OpenNURBS version
before they frame or default their fields, and what does each gate change?

**Known.** The scan decodes the archive OpenNURBS version as the writer
version. Four owners apply it: `objects.rs:28`, `settings.rs:2177`,
`brep.rs:1570`, and `mesh.rs:346`. Three owners do not apply a source gate that
uses the same value. `presentation.rs:2661-2668` reads three arrow-block UUIDs
after the arrow types for every dimension-style minor, and `rhino_3dm.md` §20.3
"3 × UUID arrow block IDs" states the same field. The source reader stops
before those UUIDs when the minor is 0, the archive is 60, and the archive
OpenNURBS version is at most 2348833437, at
`opennurbs_dimensionstyle.cpp:2880-2913`. `settings.rs:1352-1354` admits the
earth latitude, longitude, and elevation as decoded; the source reader replaces
all three with the unset values when the minor is below 2, the three values are
zero, and the archive OpenNURBS version is at most 2348834428, at
`opennurbs_3dm_settings.cpp:4194-4206`. `dimensions.rs:616-620` and
`annotations.rs:244-247` select the direct packed legacy annotation form from
the archive version alone, and `rhino_3dm.md` §18.1 "For archive versions 2
through 4, the linear, radial, and angular class data" states the same rule;
the source reader also requires an archive OpenNURBS version of at least
200710180, at `opennurbs_internal_V2_annotation.cpp:1062`.

**Need.** Each owner must state whether its source gate applies to the archive
bands the codec decodes, and the specification must carry the same rule. An
owner that does not apply a gate needs the source citation that supports that
result.

**Note.** Without the dimension-style gate, an archive-60 record that has no
arrow-block UUIDs exhausts its bounded body, becomes an opaque record with a
presentation loss, and each dimension that names the style loses it. Without
the earth-anchor gate, an unset anchor enters native data as latitude 0,
longitude 0, and elevation 0, which is a valid position. The legacy annotation
gate has no effect now because `container.rs:1190-1194` refuses raw archive
value 5.

### QA-06. Material class-data form discriminator

**Question.** Which value selects the packed-`2.0` wrapper form or the direct
anonymous form of a material class-data payload, and for which archive
versions does each form occur?

**Known.** `rhino_3dm.md` §20.2 "For archive versions 4, 5, and 50, the
class-data payload begins with packed" gives the archive version as the
discriminator and names archives 4, 5, and 50. The OpenNURBS writer agrees with
that band, because it emits the packed form when the archive version is above 3
and below 60 (`opennurbs_material.cpp:130-133,321-337`). The OpenNURBS reader
is wider: it also reads the packed form for archives 60 and later when the
archive OpenNURBS version is below 2348833910
(`opennurbs_material.cpp:222-232`). `presentation.rs:1874` does not use the
archive version. It selects the anonymous form when the first class-data byte
is zero and the packed form for any other first byte, and rejects a first byte
other than `0x20` in the packed branch.

**Need.** The specification and the owner must state the same discriminator. If
the first class-data byte is the intended discriminator, the specification must
state that rule and the byte values that select each form, together with the
archive bands in which each form occurs.

**Note.** A material record in an archive-60, archive-70, or archive-80 file
whose archive OpenNURBS version is below 2348833910 has the packed form. The
specification does not admit that record, while the owner accepts it through
the first-byte test. The obsolete transparent-color substitution is bound to
the same branch, so the two rules must select the same records.

### QA-07. Declared archive OpenNURBS version in written archives

**Question.** Which archive OpenNURBS version value must a written 3DM archive
declare, and how is that value obtained?

**Known.** `rhino_3dm.md` §6.3 "| writer version" gives `0xa0000026` as the
typecode of the writer-version record, and `settings.rs:26`,
`container.rs:67`, and `writer.rs:33` all hold that same value as a typecode.
`writer.rs:34` holds `0xa000_0026` again as the record's value, and
`writer.rs:236-239` writes it as the declared archive OpenNURBS version. A
reader therefore obtains 2684354598. Decoded under the source version-number
rule at `opennurbs_version_number.cpp:81-113`, that value is major 16, dated
9 January 2000. No OpenNURBS release has that number. `settings.rs:2391-2393`
stores the value as the writer version, and the owners at `objects.rs:28`,
`settings.rs:2177`, `brep.rs:1570`, `mesh.rs:346`, `chunks.rs:31`, and
`presentation.rs:1924-1929` compare it against source thresholds.

**Need.** The writer must declare a version it can support, and the
specification must state which value a written archive carries and why. The
same change must confirm the record layouts that the value selects.

**Note.** The written records are read back with the current-generation rules
only because the declared value is above every threshold the owners compare
against. Two of those selections change the record layout, not a repair: the
object-attributes reader selects the item-coded form only at or above
200712190 (`opennurbs_3dm_attributes.cpp:925-929`), and the mesh reader
expects the mapping tag only at or above 200606010
(`opennurbs_mesh.cpp:2692`). A corrected value must stay above both.

### QA-08. Legacy major-2 Brep vertices for an edge with no trim

**Question.** What vertex identity does a legacy major-2 Brep edge with no
trim receive, and what does the transfer record for it?

**Known.** `rhino_3dm.md` §15.0 "Major-2 has no serialized vertex table."
states that an edge with no trim uses its C3 endpoints as independent vertex
positions. `brep.rs:1056-1076` does not keep them independent: `legacy_vertex`
at `brep.rs:1350-1358` returns an existing vertex when its stored point is
exactly equal to the new point, so two such edges share a vertex and
`brep.rs:1078-1097` then averages their endpoints. The source reader creates
one edge for each C3 curve at `opennurbs_brep_io.cpp:1239-1244`, but builds
vertices only from loop rings at `opennurbs_brep.cpp:5994-6006`; an edge with
no trim keeps the `ON_BrepEdge` initial vertex indices of `-1`
(`opennurbs_brep.cpp:158-171`). The transfer emits no loss for the added
vertices.

**Need.** The owner must state the vertex identity rule for a trimless legacy
edge, the specification must state the same rule, and the added vertices need
a named loss or finding because the source admits none.

**Note.** Exact floating-point equality is the current identity test. Two
trimless edges whose endpoints differ by one unit in the last place receive
separate vertices, and a trimless edge whose two C3 endpoints are exactly
equal collapses to one vertex used twice. Each result changes the vertex
count, the averaged vertex position, and the vertex tolerance that §15.0
derives from that position.

### QA-09. Brep solid-cache reset threshold

**Question.** Which archive OpenNURBS version values make the serialized Brep
`m_is_solid` cache unusable, and does any source reader discard it?

**Known.** `rhino_3dm.md` §15.5 "For minor at least 2, the writer copies the
Brep" states that the source reader resets the value to 0 when the archive
OpenNURBS writer version is before 2 October 2002.
`decode.rs:4613-4615` puts that rule in code: it keeps the stored value only
when the writer version is at least 200210020. The source literal is not that
value. `opennurbs_brep_io.cpp:1134-1137` reads
`ArchiveOpenNURBSVersion() < 20021002`, which is eight digits, while every
archive value in the year-month-day form has nine digits and starts at
200012210 (`opennurbs_version_number.cpp:167-174`). The source condition is
therefore false for every such archive, and the source reader keeps the stored
value in each of them.

**Need.** The owner and the specification must state the reset rule that the
source applies, or state that the codec deliberately departs from the source
literal and name the evidence for the departure.

**Note.** A Brep in an archive whose writer version is between 200012210 and
200210019 has a minor at least 2 record whose `m_is_solid` the source keeps and
the codec discards. When that stored value is 1 or 2 and the topology does not
close, the neutral `BodyKind` becomes a sheet where the source is solid. The
codec also has no diagnostic for the discarded cache.

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

### QA-11. Gradient userdata duplicate arbitration

**Question.** Does the first serialized matching gradient userdata item own
the hatch gradient when that item is malformed, or may a later duplicate
supply the typed value?

**Known.** Section 7.2 "An object accepts one userdata item" states that
attachment rejects a duplicate item UUID, the first serialized item owns
object state, and attached built-in extension readers use the first serialized
matching item. The gradient rule in section 18.2 "If a registered gradient
userdata item" states that a malformed registered gradient omits the typed
parameter and retains the object record. `hatch.rs:289-300` instead parses
every matching class UUID and stores the first gradient that parses. The
integration tests cover one valid item and one malformed item separately; they
do not cover a malformed first item followed by a valid duplicate.

**Need.** An OpenNURBS attachment or read witness containing duplicate
`ON_GradientColorData` item UUIDs, including the result when the first payload
is malformed. The owner, specification, and a duplicate-order test must then
state the same arbitration and diagnostic rule.

**Conflict.** If a file contains a malformed matching gradient followed by a
valid duplicate, the specification withholds the gradient because the first
item owns the state, while the codec admits the later duplicate as typed
native data. Reversing the two records admits the same valid gradient, so the
current gate does not enforce serialized ownership.

**Note.** The obsolete V5 hatch extension is not a counterexample. It is
consumed after reading and section 18.2 "The obsolete hatch extension is
consumed after reading" explicitly gives that class a last-valid-record
side-effect rule.

### QA-12. Shipped-guess paths lack discriminating witnesses

**Question.** Which tests fail when the shipped selection and version-gate
rules named by QA-04 through QA-09 and QA-11 choose the wrong branch?

**Known.** The current tests exercise archive-2 framing, current instance
definitions, individual valid and malformed gradient carriers, current earth
anchors, and current material forms. No test contains an archive-2 through
archive-4 packed instance definition, an archive-60 dimension style below the
arrow-block writer threshold, a zero-valued old-writer earth anchor, an
archive-60-or-later old-writer packed material, a trimless legacy major-2 Brep
edge, a Brep solid cache on either side of the disputed threshold, or a
malformed-first duplicate gradient sequence. The source tree contains no test
literal for thresholds `2348833437`, `2348834428`, `2348833910`, or
`200210020`. Writer round-trip tests accept the writer-version value emitted by
the same implementation and therefore do not independently establish QA-07.

**Need.** One independent or source-derived witness per branch boundary named
above, plus a test that asserts the specification-owned result. The writer
version needs a value obtained independently of the codec constant. Each test
must fail under the current disputed rule before that rule is changed.

**Note.** The existing tests establish local framing and ordinary transfers;
they do not close the selection questions because none supplies the competing
candidate or boundary value that makes the arbitration observable.

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

### BR-01. Region topology replaced by incidence-derived shells

**Question.** What neutral region and shell structure can transfer when a
minor-3 Brep's serialized region topology does not assign exactly one bounded
region to each face?

**Known.** The Brep stores explicit face-side and region records.
`crates/cadmpeg-codec-rhino/src/decode.rs:4997-5060` uses those records only
when every face has one bounded side. Otherwise
`region_shell_groups_without_records` groups faces by edge-incidence component
and assigns generated numeric region labels. The decoder commits those inferred
shells and reports that incidence-derived shells were used. The source region
records remain in native data but no longer determine the neutral ownership
graph.

**Need.** Edge incidence can identify connected face components, but it does
not define the source's bounded-region membership. The answer withholds the
region and shell projection when the explicit relationship is unusable, or
represents the incidence grouping as inferred topology that cannot be mistaken
for the serialized Brep regions.

### WR-01. Unspecified loop role selected from neutral list order

**Question.** Which Rhino loop type can the writer emit for a neutral loop whose
`boundary_role` is `Unspecified`?

**Known.** Rhino Brep loop types distinguish outer, inner, slit,
curve-on-surface, and point-on-surface loops. `rhino_3dm.md` §15 states that an
unspecified neutral role makes the first loop of a face outer and all remaining
loops inner. `crates/cadmpeg-codec-rhino/src/writer.rs:1968-1985` implements
that rule by comparing each loop with `face.loops.first()`. It does not use a
declared neutral role or geometric containment to establish the distinction.

**Need.** Neutral loop order is traversal order, not an outer-boundary
declaration. The writer therefore creates Rhino topology semantics from list
position. The answer requires explicit representable boundary roles, or derives
the roles through a stated geometric containment result with an explicit
inference status. It must not silently classify the first loop as outer.

### LY-01. Duplicate layer identity rewritten during decode

**Question.** What neutral identity and archive index can a later layer retain
when its UUID or integer layer index duplicates an earlier layer record?

**Known.** `crates/cadmpeg-codec-rhino/src/settings.rs:2417-2428` keeps every
typed layer but assigns archive-identity ownership of a duplicate UUID to the
first serialized record. `crates/cadmpeg-codec-rhino/src/settings.rs:2495-2512`
keeps the first occurrence of a duplicate integer index and replaces each later
layer's index with the next unused value above the existing range. `LayerRecord`
has no separate original-index field, so the typed layer record retains the
generated value in place of its serialized index. A duplicate-resolution loss
reports the rewrite.

**Need.** A generated index was not declared by the archive and can be confused
with a valid source layer index. First-record UUID ownership also does not give
the later record a distinct source identity. The answer preserves each
serialized identity and represents the collision explicitly, or withholds the
ambiguous archive-reference binding without manufacturing a replacement index.

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

### QA-14. Failed candidate admission leaves native-record links behind

**Question.** What state can a failed Rhino candidate admission change?

**Known.** `decode.rs:602-666` checkpoints appended arena lengths and
annotations, then replaces the complete Rhino native-unknown arena before it
runs `admit_with_annotations`. The error branch truncates model arenas and
rolls back annotations, but it does not restore the native-unknown arena. A
rejected history projection can therefore leave an object record linked to a
procedural surface that was rolled back. Final validation then reports a
`native_links` error for the unresolved target.

**Need.** Candidate admission must be atomic across the model, annotations,
and native unknown records. A failure witness must assert that both the staged
entity and every link to it are absent after rollback.

### QA-15. Error-severity decode losses do not fail `cadmpeg check`

**Question.** Can `cadmpeg check` return success when decoding reports an
integrity failure at error severity?

**Known.** `commands.rs:288-294` passes decode losses into the validation
report and bases the check verdict on `ValidationReport::is_ok`.
`report.rs:997-1016` counts only validation findings; it does not inspect the
report's losses. `loss.rs:193` assigns `container.integrity-failure` error
severity, but a document with that loss and no error finding receives a
successful check verdict.

**Need.** Define one verdict rule for error-severity losses and error-severity
findings. If an integrity loss is recoverable enough for a successful check,
its severity and strict consequence must say so instead of placing an ignored
error in a successful report.

### TP-01. Decoder output fails geometric-consistency validation

**Question.** Which topology and geometry can the decoder return as a
successful Rhino decode?

**Known.** The V1 path appends legacy Breps directly at
`legacy.rs:2107-2444` and never applies the Rhino candidate-admission gate.
The object path commits Brep drafts at `decode.rs:3320-3405`. Returned models
from both paths can fail final geometric consistency: edge curves miss their
declared vertex positions, and pcurves mapped through their face surfaces miss
the same vertices. The observed displacement reaches `190.716486` model
units. The same defect is present in archive-2 and current class-table Breps,
so it is not confined to V1 framing.

**Need.** Every Brep transfer path must establish edge/vertex and
pcurve/surface consistency before commit. A failing candidate must remain
opaque or retain only independently valid carriers; a successful decode must
pass the same geometric checks used by `cadmpeg check`.

### TP-02. Legacy Breps emit equal senses in two-member radial rings

**Question.** Which orientation relation must the two coedges of a shared V1
edge have in neutral topology?

**Known.** `legacy.rs:1382-1770` builds V1 edges, coedges, and radial rings.
Final validation at `validate/topology.rs:6374-6385` reports
`two-member radial ring has equal coedge senses` when both incident coedges
carry the same sense. The condition occurs across many decoded V1 Breps and
produces thousands of warnings, rather than an isolated degenerate edge.

**Need.** Establish whether the V1 trim-reversal mapping or the neutral
validator's sense invariant is wrong, then make the decoder and invariant
agree. Add a two-face shared-edge witness that checks face reversal, trim
reversal, coedge sense, and radial order together.

### BR-02. Invalid face surface slots discard complete Brep topology

**Question.** What can transfer when a Brep face references a surface slot
that the current reader does not admit?

**Known.** `brep.rs:496-503` rejects the complete raw topology when
`face.surface` does not identify a typed surface slot. `decode.rs:3406-3428`
then retains decoded child curves, surfaces, or mesh caches but discards the
body, regions, shells, faces, loops, edges, coedges, vertices, points, and
pcurves. The resulting `topology.brep-fallback` loss has occurred repeatedly
for otherwise readable Breps.

**Need.** Determine whether the rejected indexes use an unhandled serialized
slot form or genuinely reference absent data. Admit the correct slot grammar
when present. When the source reference is absent, retain the complete Brep
record and expose only carriers whose identity does not imply discarded
topology.

### BR-03. Valid Brep not-solid cache value is reported as invalid

**Question.** Which values are valid for the serialized Brep `m_is_solid`
cache?

**Known.** OpenNURBS defines `0` as unset, `1` as outward solid, `2` as inward
solid, and `3` as not solid at `opennurbs_brep.cpp:6977-6996`.
`brep.rs:647-657` accepts only `0..=2` and reports value `3` as an invalid
enumeration. Valid non-solid Breps therefore receive
`container.enumeration-value-degraded` warnings.

**Need.** Accept all four source-defined values and map value `3` to the
documented non-solid result. Add a witness for each cache value and for a value
outside the source-defined range.

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
