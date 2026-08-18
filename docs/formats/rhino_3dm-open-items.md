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

**Need.** A later user-data class writer and reader, or an independent witness,
for each version that is to be typed, including its fields and boundaries. The
same change must state the field-specific loss or neutral mapping required by
the section-20.6 admission rule. The remaining CADIR audit must cover the other
class-owned userdata carriers and their object, table, settings, and embedded
geometry owners.

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
