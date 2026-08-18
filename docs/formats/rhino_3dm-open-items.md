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
