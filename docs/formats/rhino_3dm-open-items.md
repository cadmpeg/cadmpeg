# Rhino 3DM Open Items

The remaining questions are narrowed to producer-defined payload fields or
class-specific transfer evidence. Settled format rules remain in
[`rhino_3dm.md`](rhino_3dm.md). OpenNURBS transfer evidence remains in
[`rhino_3dm-opennurbs-comparison.md`](rhino_3dm-opennurbs-comparison.md).

## Remaining items

### PP-01. Plug-in class payload grammar

**Question.** For each third-party plug-in class UUID admitted by this codec,
which class-owned `Write`/`Read` implementation defines its class-data fields
and typed mapping?

**Known.** Section 6.4 defines the class wrapper: class UUID, bounded
class-data, optional class-userdata, and class end. The class-data grammar is
owned by the class reader. OpenNURBS looks up the UUID and invokes that reader;
its application-loading hook delegates unavailable plug-in classes to the
plug-in. There is no common third-party class-data grammar.

**Need.** Read the producer implementation for each admitted third-party UUID,
or supply an independent witness whose bytes identify the class fields and
typed mapping.

**Note.** Narrowed 2026-08-16. The common wrapper and the source-defined
ownership rule are settled. The per-class producer contract remains open.

### PP-02. Plug-in class field semantics

**Question.** For each supported third-party class, which fields carry
transferable object state and what neutral mappings do they have?

**Known.** A class reader owns the field grammar after the class UUID. Public
OpenNURBS examples demonstrate that this is class-specific: `MyUserData` writes
an integer, line, and string, while `CExampleWriteUserData` writes a string.
Those examples do not establish the fields of the plug-ins admitted by this
codec.

**Need.** The actual supported class's writer and reader, or an independent
byte-level witness with field-to-neutral mappings, for every admitted class.

**Note.** Narrowed 2026-08-16. Per-class field semantics remain open; opaque
retention is not a field mapping.

### PP-03. Plug-in user-data dictionary grammar

**Question.** What semantics and typed mappings do non-standard or plug-in
dictionary UUIDs use?

**Known.** Section 20.6 now defines the standard `ON_ArchivableDictionary`
grammar, UUID `21EE7933-1E2D-4047-869E-6BDBF986EA11`, bounded entries, array
counts, nested dictionaries, and stable entry types 0 through 47. Unknown
entry types are skipped at their entry boundary. A dictionary with another ID
is not assigned this grammar.

**Need.** The producer dictionary implementation or an independent witness for
each non-standard dictionary UUID, including its value semantics and neutral
mapping.

**Note.** Narrowed 2026-08-16. The standard dictionary grammar is settled; the
plug-in dictionary set and its semantics remain open.

### PP-04. Plug-in direct user-record grammar

**Question.** What inner grammar and typed mapping does each plug-in direct
user-table record use?

**Known.** Section 20.6 defines the outer framing: a plug-in UUID chunk, an
optional major-1 record-header chunk containing the goo flag and producer
archive/runtime versions, one bounded `TCODE_USER_RECORD`, and the table end
marker. The UUID chunk is CRC-bearing; its CRC covers the direct UUID bytes and
excludes the complete record-header child. The record-header child has its own
CRC, while `TCODE_USER_RECORD` is long and non-CRC. The record body is
arbitrary plug-in-owned bytes; the common framing does not define its fields.

**Need.** The plug-in writer and reader, or an independent witness, for each
direct record type that is admitted as typed data.

**Note.** Narrowed 2026-08-16. Boundary and ownership are settled. The inner
plug-in grammar remains open.

### FV-01. Future object-class payloads

**Question.** Which later versions of each built-in object-class payload retain
typed compatibility, and which fields remain admissible?

**Known.** The audited built-in class readers consume their known major-1
prefixes, apply their minor field gates, and skip bounded suffixes. Major
version changes remain family-specific; a class UUID and a preserved record do
not establish typed compatibility with a new major layout. `ON_NurbsCage` is a
settled family: its writer emits anonymous version 1.0, its reader accepts
major 1 with any nonnegative minor after the fixed cage prefix, and its suffix
ends at the cage chunk boundary. A non-1 cage major is rejected. In morph
variant 3, that cage boundary remains distinct from the enclosing morph
control boundary.

**Need.** A producer implementation or independent witness for each later
object-class major/version that is to enter typed decoding, with its field
grammar and neutral mapping.

**Note.** Narrowed 2026-08-16. Bounded later-minor handling is settled for the
audited classes, including the nested NURBS cage; later object-class versions
with changed layout remain open.

### FV-02. Future table-record payloads

**Question.** Which later table-record versions retain typed decoding, and what
fields do those versions add or change?

**Known.** The audited material, texture, group, light, linetype, hatch, font,
dimension-style, view, and settings readers consume known bounded prefixes and
skip source-defined suffixes. Tagged streams, explicit terminators, and
writer-band ceilings remain grammar controls. An unknown table record has no
typed fields from its typecode alone.
`ON_Group::Internal_WriteV5` and `Internal_ReadV5` define the group class
wrapper's packed 1.1 prefix: archive index, UTF-16 name, and UUID. The group
record writer and reader in `ON_BinaryArchive::Write3dmGroup` and
`Read3dmGroup` contain that wrapper directly in the CRC-bearing group record.
The Rust decoder consumes that prefix and resolves object-attribute group
indexes to unique group records.
`ON_Linetype::Write` and `Read` define the linetype class wrapper's legacy
anonymous 1.1 and modern anonymous 2.3 payloads. The modern payload contains
the five-status model-attributes child, segment array, and minor-gated item
stream for cap, join, width, width units, taper, and always-model-distance.
`ON_BinaryArchive::WriteLinetypeSegment` defines the line/space wire tags.
`ON_HatchPattern::WriteV5`/`ReadV5` define the below-version-60 packed 1.2
prefix and packed 1.1 hatch-line elements. The version-60 writer uses an
anonymous major-1 pattern body with model-component attributes, a nested
anonymous line-list body, and anonymous major-1 hatch-line elements.
`ON_BinaryArchive::Write3dmHatchPattern`/`Read3dmHatchPattern` put one class
wrapper directly in each CRC-bearing hatch-pattern record. The Rust decoder
consumes both source branches and scales only line lengths.
The shared model-component child is also source-defined: the legacy anonymous
version-1.0 form uses the five-bit UUID/parent/index/name/status presence mask,
ignores other mask bits, and uses a two-`u32` locked/hidden status mask; the
modern `0x40008002` version-1.0 form uses five status bytes for model serial,
UUID, type, index, and name, where only 0, 1, and 2 have writer meanings and
the reader treats other values as no field. Both children are bounded by their
own chunk ends. This is established by
`ON_ModelComponent::ReadModelComponentAttributes` and
`ON_BinaryArchive::ReadModelComponentAttributes` in
`/home/pcurve/side2/opennurbs/opennurbs_model_component.cpp` and by the public
linetype status and legacy-mask witnesses recorded in the notebook.

**Need.** Producer source or an independent witness for any later table-record
major or changed layout that is to be admitted as typed data.

**Note.** Narrowed 2026-08-16. The audited bounded suffix subset is settled;
later table-record layouts outside it remain open.

### FV-03. Future user-data payloads

**Question.** Which later user-data versions have a typed payload grammar, and
which fields remain admissible?

**Known.** Section 7.2 defines major-1 and major-2 headers, their bounded
payload child, minor-gated header fields, and suffix skipping. The payload
grammar is owned by the userdata class. Unknown classes can be preserved but
do not gain typed fields from the header.

The built-in `ON_GradientColorData` class userdata is now settled for archive
version 60 and later. Its class and item UUID is
`0C1AD613-4EFA-4F47-A147-4D79D77FCB0C`; the source writer and reader in
`opennurbs_hatch.cpp` define the bounded anonymous gradient-data and color-stop
chunks. The Rhino decoder maps its type, scaled endpoints, repeat, and ordered
RGBA stops to the hatch feature's native `gradient` parameter while retaining
the source object record because neutral hatch fill geometry is not produced.
The V5 `ON_DimStyleExtra` class userdata is also settled. V5 and V50
dimension-style records use `ON_V5x_DimStyle` class UUID
`81BD83D5-7120-41C4-9A57-C449336FF12C`; its packed 1.5 class-data prefix and
the nested anonymous 1.3 `ON_DimStyleExtra` payload are defined in section
20.3. The extra class and item UUID is
`513FDE53-7284-4065-8601-06CEA8B28D6F`, and the decoder retains its source
fields under the native dimension-style record.
The built-in `ON_UserStringList` class userdata is also settled. Its class and
item UUID is `CE28DE29-F4C5-4FAA-A50A-C3A6849B6329`, its application UUID is
`17B3ECDA-17BA-4E45-9E67-A2B8D9BE520D`, and section 7.2.1 defines the bounded
list and entry chunks. The decoder keeps geometry and object-attributes
user-string lists as separate ordered native arrays and applies the source
`$temp_object$` cleanup only to the attributes list.
The built-in `ON__LayerExtensions` class userdata is also settled. Its class
and item UUID is `3E4904E6-E930-4FBC-AA42-EBD407AEFE3B`, its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`, and section 8.3.1 defines its bounded
per-viewport entry grammar, fixed settings mask, compatibility visibility byte,
root-layer persistent-visibility rule, and source sort order. The decoder
stores the normalized entries under the owning layer.
The built-in `ON_AngularDimension2Extra` class userdata is also settled. Its
class and item UUID is `A68B151F-C778-4A6E-BCB4-23DDD1835677`, its application
UUID is `C8CDA597-D957-4625-A4B3-A0B510FC30D4`, and section 20.3 defines its
anonymous version-1.0 payload with two model-length extension-line offsets.
The decoder applies the values to V5 angular dimensions after unit scaling and
uses the source `-1.0` absent sentinel.
The built-in `ON_OBSOLETE_V5_TextExtra` class userdata is also settled. Its
class and item UUID is `D90490A5-DB86-49F8-BDA1-9080B1F4E976`, its application
UUID is `C8CDA597-D957-4625-A4B3-A0B510FC30D4`, and section 7.2.2 defines its
anonymous version-1.0 payload with the parent text UUID, mask flag, color
source, RGBA color bytes, and dimensionless border offset factor. The decoder
retains those fields under the owning V5 text annotation.
The built-in `ON_V5_MeshDoubleVertices` class userdata is also settled. Its
class and item UUID is `17F24E75-21BE-4A7B-9F3D-7F85225247E3`, its application
UUID is `C8CDA597-D957-4625-A4B3-A0B510FC30D4`, and section 7.2.3 defines its
anonymous version-1.0 payload with the two vertex counts, two CRC fields, and
serialized f64 vertex array. The decoder adopts the array only when its actual
count matches the mesh and its f64 values cast exactly to the stored f32
vertices; otherwise it retains the float mesh and reports the redundant-field
repair.
The same class wrapper and transfer rule apply to render and analysis mesh
cache objects nested in Brep minor-1 side arrays. They also apply to nested
`ON_Mesh` wrappers in the minor-3 extrusion mesh cache and to mesh wrappers
embedded in history geometry values. Minor-2 extrusion writers have a second
settled carrier: class/item UUID `A8130A3E-E4F3-4CB0-BB8A-F10A473912D0` with
the OpenNURBS 5 application UUID. Its bounded payload contains render,
analysis, and null object wrappers; the first two accept null or `ON_Mesh`, the
third is discarded, and a nested mesh carries the same section 7.2.3 rule.
The built-in `ON_V4V5_MeshNgonUserData` class is also settled. Its class and
item UUID is `31F55AA3-71FB-49F5-A975-757584D937FF`, its application UUID is
`17B3ECDA-17BA-4E45-9E67-A2B8D9BE520D`, and section 7.2.4 defines its minor-1
anonymous record list, validation counts, and old zero-count index checks.
The decoder uses its admitted record count for the existing neutral grouping
loss; the mesh face triangles remain the transferred geometry. The class is a
V4/V5 compatibility carrier. An explicitly attached item can persist in a
later all-userdata archive; when an inline n-gon count is present, that newer
count takes precedence for the neutral grouping loss.
The built-in `ON_V5_BrepRegionTopologyUserData` class is also settled. Its
class and item UUID is `7FE23D63-E536-43F1-98E2-C807A2625AFF`, its application
UUID is `17B3ECDA-17BA-4E45-9E67-A2B8D9BE520D`, and section 7.2.5 defines its
anonymous region-topology payload, V5 raw array elements, and later
polymorphic array elements. `ON_Brep::Write` attaches it automatically only
for archive version 50 when the Brep has faces and exactly twice as many face
sides; `DeleteAfterRead` installs it only when no inline region topology is
loaded. The decoder reuses the Brep region carriers and validation, so no new
neutral field is introduced.
The built-in `ON_SubDMeshProxyUserData` class is also settled. Its class and
item UUID is `2868B9CD-28AE-4EA7-8073-BD390B3E97C8`, its application UUID is
`7B0B585D-7A31-45D0-925E-BDD7DDF3E4E3`, and section 7.2.6 defines its positive
minor-1 anonymous payload, embedded SubD boundary, raw mesh-array SHA-1
records, identity-transform gate, archive-version write split, and parent-mesh
transfer check. A valid top-level proxy is promoted to the neutral SubD; a
failed admission retains the parent mesh under the CADIR decision in section
7.2.6.
The built-in `ON_OBSOLETE_IDefAlternativePathUserData` class is also settled.
Its class and item UUID is `F42D9671-21EB-4692-9B9A-BC3507FF28F5`, its
application UUID is `C8CDA597-D957-4625-A4B3-A0B510FC30D4`, and section 7.2.7
defines its anonymous path-and-relative-flag payload, trim rule, linked-type
gate, and empty-slot precedence. The decoder stores both legacy full and
relative paths and applies the carrier to the existing external-reference
record without creating another identity.
The built-in `ON_OBSOLETE_CCustomMeshUserData` class is also settled. Its
class and item UUID is `69F27695-3011-4FBA-82C1-E529F25B5FD9`; its direct
outer-anonymous payload contains the legacy integer, in-use Boolean, and
`ON_MeshParameters` body. The object-attributes reader applies the source
custom-render-mesh setter side effects and the decoder retains the converted
settings under the owning native object presentation.
The built-in `ON_PerObjectMeshParameters` class is also settled. Its class and
item UUID is `B5628CA9-82C4-4CAE-9883-487B3E4AB28B`, its application UUID is
`C8CDA597-D957-4625-A4B3-A0B510FC30D4`, and section 7.2.10 defines its
class-owned anonymous version-1.0 wrapper, nested anonymous mesh child, and
packed `ON_MeshParameters` body. The decoder applies the source custom and
curvature side effects under the owning object presentation; a malformed
payload leaves the object attributes admitted and records the bounded drop.
The built-in `ON_AnnotationTextFormula` class is also settled as a
runtime-only carrier. Its class and item UUID is
`699FCC42-62D4-488C-9109-F1B7A37CE926`; it has no archive writer or reader and
inherits `Archive() == false`, so it has no class-userdata payload. The formula
is the direct minor-2 UTF-16 field of the legacy annotation record in section
18.3.
The built-in `ON_DisplacementUserData` class is also settled. Its class UUID
is `B8C04604-B4EF-43B7-8C26-1AFB8F1C54EB`, item UUID is
`8224A7C4-5590-4AC4-A32C-DE85DC2FFDAE`, and application UUID is
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. Its object-attributes payload uses
`ON_XMLUserData` version 1 or 2 framing, the `xml`/
`new-displacement-object-data` XML roots, the source getter defaults, the
pre-V6 formula insertion, and ordered `sub` overrides. Section 7.2.12 and the
decoder retain the serialized sub-object count without using it as the child
count.

The built-in `ON_EdgeSofteningUserData` class is also settled. Its class UUID
is `CB5EB395-BF1B-4112-8F2F-F728FCE8169C`, item UUID is
`8CBE6160-5CBD-4B4D-8CD2-7CE0A7C8C2D8`, and application UUID is
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. Its object-attributes payload uses
`ON_XMLUserData` version 1 or 2 framing, the `xml`/
`edge-softening-object-data` XML roots, and the source getter defaults for
the six edge-softening parameters. Section 7.2.13 and the decoder retain the
typed `faceted` value from the XML `unweld` parameter.

The built-in `ON_ThickeningUserData` class is also settled. Its class UUID is
`AA03D9C3-4CCF-4431-A06E-25F38CF3913F`, item UUID is
`6AA7CCC3-2721-410F-AA56-E8AB4F3ECE67`, and application UUID is
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. Its object-attributes payload uses
`ON_XMLUserData` version 1 or 2 framing, the `xml`/`thickening-object-data`
XML roots, and the source getter defaults for the five thickening parameters.
Section 7.2.14 and the decoder retain the typed distance and Boolean values.

The built-in `ON_CurvePipingUserData` class is also settled. Its class UUID is
`2D5AFEA9-F458-4079-992F-C2D405D9383B`, item UUID is
`2B1A758E-7CB1-45AB-A5BF-DFCD6D3D136D`, and application UUID is
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. Its object-attributes payload uses
`ON_XMLUserData` version 1 or 2 framing, the `xml`/`curve-piping-object-data`
XML roots, and the source getter defaults. Section 7.2.15 and the decoder
retain the inverse `weld`/`faceted` value and canonical cap type.

The built-in `ON_ShutLiningUserData` class is also settled. Its class UUID is
`429DCD06-5643-4254-BDE8-C0557F8FD083`, item UUID is
`07506EBE-1D69-4345-9F0D-2B9AA1906EEF`, and application UUID is
`F293DE5C-D1FF-467A-9BD1-CAC8EC4B2E6B`. Its object-attributes payload uses
`ON_XMLUserData` version 1 or 2 framing and the `xml`/`shut-lining-object-data`
XML roots. The four scalar fields use typed XML parameters with false getter
defaults. Direct `curve` children are ordered; their UUID, radius, profile,
enabled, pull, and is-bump fields use the default-property text grammar and
getter defaults of nil UUID, 1.0, 0, false, false, and false. The source
writer emits one empty curve for each managed curve, followed by serialized
copies of all managed curves; the decoder retains all direct curve entries in
order.
Section 7.2.16 and the decoder retain the scalar fields and ordered curve
records.
The built-in `ON_PhysicallyBasedMaterialUserData` class is also settled. Its
class and item UUID is `5694E1AC-40E6-44F4-9CA9-3B6D0E8C4440`, its
application UUID is `7B0B585D-7A31-45D0-925E-BDD7DDF3E4E3`, and section
7.2.17 defines its nested anonymous version-1.1/version-1.2 payload, three
float RGBA colors, BRDF integer, fifteen scalar doubles, version-2 alpha,
default values including the unset base-color encoding, and bounded suffix
handling. The decoder stores the typed
fields under the owning material record and retains the material when a
recognized PBR payload is malformed.

The built-in `ON_RdkUserData` carrier is also partly settled as a generic XML
userdata carrier. Its class UUID is `AFA82772-1525-43DD-A63C-C84AC5806911`,
item UUID is `B63ED079-CF67-416C-800D-22023AE1BE21`, and application UUID is
`16592D58-4A2F-401D-BF5E-3B87741C1B1B`. Section 7.2.18 defines its version-2
UTF-8 payload, legacy version-1 UTF-16 branch, archive predicate, material
legacy-transfer behavior, and callback-owned XML boundary. The codec makes
the CADIR decision to retain this XML only through the complete containing
object record; it does not assign a neutral field grammar to RDK callbacks.

Section 7.2.18 now also settles the V4/V5 material compatibility carrier
written by `ON_RdkMaterialInstanceIdObsoleteUserData`. The source path
`ON_Material::Internal_WriteV5`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_material.cpp` gates the carrier to
archive version 50 and below and gates the inline class-data UUID to minor 5;
the carrier writer in the same file defines version 2, a 0..1024 byte count,
and raw unterminated XML. `ON_RdkUserData::DeleteAfterRead` in
`/home/pcurve/side2/opennurbs/opennurbs_xml.cpp` proves the universal
render-engine plug-in replacement, `material/@instance-id` transfer, and
deletion. The witness
`/home/pcurve/side2/tmp/agent-rhino-l9-20260816/rdk_material_witness.cpp`
produces V4/V5 direct XML and V6 inline UUID bytes; `cadmpeg inspect` locates
both forms, and OpenNURBS readback returns the same UUID with universal
plug-in identity for V4/V5. The codec now transfers the direct compatibility
form and leaves NUL-terminated callback XML opaque by CADIR decision. Other
future userdata class payloads remain open.

The built-in `MappingCRCCache` userdata is also settled as a derived cache on
custom texture-mapping primitives. Its class and item UUID is
`5A4971F3-AA73-493C-A385-2F7EB4288989`, its application is `ON_opennurbs_id`,
and section 7.2.19 defines its version-1 `i32` version/checksum payload, the
primitive checksum inputs, and its placement inside the nested primitive class
wrapper. The codec now reports the primitive class UUID, skips this cache, and
does not invent a neutral `mapping_crc` field; this is the CADIR decision for
recomputable source state.

The built-in `CTtMappingMeshInfoUserData` and
`CTtRenderMeshInfoUserData` carriers are also settled as derived mesh
correspondence caches. Their class and item UUIDs are
`1706ADC5-52BF-4BE2-8402-4501EB2AE675` and
`4960A046-8201-4F0F-8F22-FCB6F91C765D`; both use `ON_opennurbs_id` and section
7.2.20 defines their version-1 geometry-fingerprint payloads, source-face
fields, defaults, and closest-point-mapping use. The codec consumes their
bounded wrappers, retains the containing source record, and does not invent
native mapping-mesh or render-mesh cache fields. This is the CADIR decision for
recomputable mesh correspondence state.
The built-in `ON_OBSOLETE_V5_DimExtra` carrier is settled in section 18.1. Its
V5 application UUID, class-owned anonymous version-1 payload, V5 conversion
path, minor-gated distance-scale and detail fields, and CADIR mapping are
specified for legacy linear, radial, and ordinate dimensions. The built-in
`ON_OBSOLETE_V5_HatchExtra` carrier is settled in section 18.2. Its V5
application UUID, class-owned anonymous version-1 payload, archive-50 write
gate, inline-basepoint split, read-side basepoint application, and consumed
userdata rule are specified.

**Need.** The later userdata class writer and reader, or an independent witness,
for each version that is to be typed, including its fields and loss mapping.

**Note.** Narrowed 2026-08-16. Generic header and boundary semantics, the
built-in hatch gradient userdata, the V5 dimension-style extra, and
`ON_UserStringList`, `ON__LayerExtensions`, `ON_AngularDimension2Extra`,
`ON_OBSOLETE_V5_TextExtra`, `ON_OBSOLETE_V5_DimExtra`,
`ON_OBSOLETE_V5_HatchExtra`, `ON_V5_MeshDoubleVertices`,
`ON_V4V5_MeshNgonUserData`, `ON_V5_BrepRegionTopologyUserData`, and
`ON_SubDMeshProxyUserData`, and `ON_OBSOLETE_IDefAlternativePathUserData` are
settled, including the Brep, extrusion, history, top-level SubD proxy, and
linked-definition carriers where applicable. The obsolete
`ON_OBSOLETE_IDefLayerSettingsUserData` and `ON_OBSOLETE_LayerSettingsUserData`
classes are also settled as no-op V5 compatibility records under section
7.2.8. `ON_OBSOLETE_CCustomMeshUserData` is settled as the typed
object-attributes compatibility carrier under section 7.2.9, and
`ON_PerObjectMeshParameters` is settled as the typed modern object-attributes
carrier under section 7.2.10. `ON_AnnotationTextFormula` is settled as a
runtime-only helper with no serialized userdata under section 7.2.11; other
future class-specific payload semantics, beyond the settled mesh-modifier,
physically based material, RDK, mapping-CRC, and texture-mesh correspondence
carriers, remain open.

### RS-01. Later-minor bounded suffixes

**Question.** Which remaining versioned readers outside sections 7.1, 13.3,
13.4, 18.3, and 20.2-20.3 accept unread fields appended before their bounded
end?

**Known.** The global rule at section 4.2 and the source-backed changes cover
point clouds, simple curves, curve-on-surface, NURBS curves and surfaces,
procedural surfaces, extrusions, cages, annotations, dimensions, userdata,
views, document settings, presentation resources, and object-class suffixes.
`ON_3dmObjectAttributes::Internal_ReadV5` accepts a future minor, consumes one
unknown item ID, and leaves its unlength-prefixed value to the containing
attributes chunk boundary. `ON_Layer::Read` applies the same boundary rule to a
nonzero extension ID outside its defined set after the fixed layer prefix and
known extension payloads. `ON_Linetype::Read` applies it to an unknown major-2
extension code after the component-attributes and segment prefix. Its known
item gates are minor 1 for cap/join, minor 2 for width/units/taper, and minor 3
for always-model-distance; its writer emits modern minor 3. The legacy major-1
reader has the archive index, name, segment array, and minor-1 UUID prefix.
`ON_Font::Read`
accepts modern anonymous major-1 fonts, applies the minor 0 through 6 field
gates, and ends at the font chunk boundary; its modern writer emits minor 6
and its Windows LOGFONT name is a `TCODE_UTF8_STRING_CHUNK` with a format byte.
The legacy V5 font branch remains a packed 1.2 record with its own minor gates.
`ON_DimStyle::Write` emits anonymous version 1.9 after the model-component
attributes and the fixed minor-0 prefix. `ON_DimStyle::Read` accepts major 1,
applies the minor 1 through 9 gates for the scale-value, font, text-mask, and
control fields, and ends at the anonymous chunk boundary for later minors.
The Rust dimension-style reader mirrors that prefix, each gate, and the later-
minor boundary.
`ON_BinaryArchive::Internal_Write3dmDimStyle` selects `ON_V5x_DimStyle` for
archives below version 60. `ON_V5x_DimStyle::Write_v5` and
`Internal_Read_v5` use the packed 1.5 prefix defined in section 20.3; minor
gates 1 through 5 are bounded by the class-data chunk. `ON_DimStyleExtra::Write`
and `Read` use the nested anonymous 1.3 payload and its minor 1 through 3
gates. The Rust V5 reader mirrors both grammars and preserves a bounded suffix
after each known prefix.
`ON_RenderingAttributes::Write/Read` is the layer form: the writer emits
anonymous version 1.0 with a material-reference array, and the major-1 reader
consumes that array without a minor gate. `ON_ObjectRenderingAttributes` is the
object form: its writer emits version 1.3 with material and mapping-reference
arrays, then shadow flags at minor 2 and advanced texture preview at minor 3;
its reader requires major 1 and minor at least 1, consumes those gates, and
ends at the object chunk boundary for later minors. `ON_MappingRef` and
`ON_MappingChannel` provide the nested mapping-array grammar; the latter adds
its 16-double transform at minor 1. The Rust object and layer readers now
select the correct outer class and consume the complete known object prefix.
Remaining strict rules are writer-band ceilings, direct readers with other
version families, tagged item streams with explicit terminators, or versioned
readers whose exact producer field gates have not yet been characterized
individually. `ON_TextStyle::Read` consumes the model-component, description,
font, UUID, and name prefix for modern anonymous version 1.1 and then closes
the bounded chunk; the Rust wrapper test proves that a future outer minor does
not swallow the post-font identity fields. `ON_3dmSettings::Write_v2` places
render settings in `TCODE_SETTINGS_RENDER`. `ON_3dmRenderSettings` has a
source-backed direct legacy body with version range 100–199 and gates at 101,
102, and 103, and a modern anonymous major-1 body with gates at minors 1, 2,
and 3. `ON_3dmAnnotationSettings` has packed major-1 bodies with writer minor
2 for archive versions below 60 and minor 4 for archive version 60 and later.
Its fixed base prefix writes dimension units as a `u32`; the writer hard-codes
the dimension-scale field to `1.0`. Minor 1 gates world-view text scale and the
V5 annotation flag, minor 2 gates world-view hatch scale and the hatch flag,
minor 3 gates model/layout scaling, and minor 4 gates the dimension-layer
Boolean and UUID. V4/V5/V6 witness files with non-default base fields and all
gated fields confirmed the writer bands and the source read defaults; the Rust
owner test also covers a non-nil dimension-layer UUID and a future minor
suffix.
`ON_3dmConstructionPlaneGridDefaults::Write/Read` in
`opennurbs_3dm_settings.cpp` writes and reads packed version 1.0 followed by
two document-length doubles, two signed counts, and three nonzero-true signed
visibility values. Its in-class defaults are spacing 1.0, snap spacing 1.0,
line count 70, thick-line frequency 5, and all visibility values true. The
writer/reader witness `grid_defaults_witness.cpp` produced the same 41-byte
body in V4, V5, and V6, with packed version `0x10`, values 2.5/0.75, counts
42/3, and flags 0/1/0. The V6 inch witness transferred those two stored
document lengths to 63.5/19.05 millimeters in the Rust arena. The owner test
covers a future minor suffix and the length transfer. The Rust document-
settings reader now consumes the complete known prefix and leaves only the
respective containing boundary suffix.
`ON_3dmConstructionPlane::Write/Read` similarly defines the
`TCODE_VIEW_CPLANE` child as packed 1.1, a 16-double plane, two document-length
spacing values, two counts, a UTF-16 name, and a minor-1 depth-buffer flag. The
construction-plane witness `construction_plane_witness.cpp` produced this
body in V4, V5, and V6 with a non-axis-aligned frame, and the inch V6
differential transferred its origin, equation offset, and spacings to
millimeters while preserving the axes. The owner test covers the spatial
scaling, name, depth flag, and bounded suffix.
`ON_3dmUnitsAndTolerances::Read` accepts direct versions 100–199, gates display
fields at 101 and custom-unit fields at 102, and relies on the outer settings
chunk for future suffix bytes; the Rust units reader now matches that band.
`ON_3dmSettings::Write_v2` emits `TCODE_SETTINGS_ATTRIBUTES` packed version
1.7. Its reader gates the page-units wrapper, active-view UUID, model basepoint
and earth anchor, texture-save flag, IO settings, custom render mesh, and six
current-component UUIDs at minors 1 through 7. The nested earth-anchor, IO,
SubD-display, and custom-mesh readers have source-defined major-1 field
prefixes; the custom-mesh body is direct and has no boundary before the
version-1.7 UUIDs. The Rust settings reader now decodes this complete
source-defined prefix and skips only the outer settings-attributes suffix.
The settings-attributes outer CRC covers direct body bytes and excludes the
complete page-units, earth-anchor, IO-settings, and SubD-display anonymous
children; direct custom-mesh bytes, the six UUIDs, and a direct suffix remain
covered.
`ON_3dmSettings::Write_v2/Read_v2` also place the direct packed-1.5
`ON_MeshParameters` body in `TCODE_SETTINGS_RENDERMESH` and
`TCODE_SETTINGS_ANALYSISMESH`. Those enclosing records provide the suffix
boundary, so the Rust reader admits later packed minors after the known
minor-5 SubD child and skips their remaining bytes at the top-level record.
The outer CRC for each record covers the direct mesh bytes, excludes the
complete anonymous SubD-display child, and includes any direct suffix after
that child.
`ON_3dmSettings::Write_v2/Read_v2` place modern render settings in one
anonymous child of `TCODE_SETTINGS_RENDER`; legacy V5 render settings are
direct. The modern outer CRC excludes that child and includes a direct suffix;
the legacy outer CRC covers its direct body.
`ON_3dmSettings::Write_v2` places `TCODE_SETTINGS_PLUGINLIST` first for
archive versions at least 4 when the list is nonempty. Its outer reader uses
packed major 1 and a count, then calls `ON_PlugInRef::Read` for each anonymous
child. `ON_PlugInRef::Write/Read` defines anonymous version 1.2: base plugin
identity and executable fields, eight developer strings at minor 1, and three
platform/SDK integers at minor 2. Both the list and each reference have
bounded ends, so later minors can be skipped independently. The Rust settings
reader now retains the complete known reference fields and both boundaries.
`ON_3dmSettings::Write_v2/Read_v2` places and consumes
`TCODE_SETTINGS_RENDER_USERDATA` only after a successful render-settings body
in archive version 60 or later. Its outer reader consumes repeated standard
class-userdata chunks through a short class-end marker, skips unknown nonzero
children, and leaves a later outer suffix at the settings-record boundary.
The Rust document-data reader now recognizes this wrapper and reuses the
source-defined major-1/major-2 userdata header parser; the anonymous payload
remains class-owned.
The remaining direct selectors are now characterized. The current-layer,
current-wire-density, current-font, and current-dimstyle records are short
chunks. Current material and current color are CRC-bearing long chunks with
eight-byte known prefixes: two signed `i32` values for material index/source,
and four color bytes followed by one signed `i32` color-source value. The
producer readers consume those prefixes and let `EndRead3dmChunk()` skip a
later direct suffix. The Rust settings reader now accepts that suffix and
does not add a material-index range gate absent from the producer reader.
The legacy `ON_3dmSettings::Read_v1` path also defines a flat settings stream:
unit/tolerance data plus legacy named-construction-plane, named-view, and
viewport records, each with its child grammar and end marker. The Rust V1
decoder now consumes the unit record, skips the structural top-level end
marker, and retains the three legacy presentation records as opaque bytes
with a typed presentation loss, matching its CADIR admission decision.
The historical `0x2000803e` settings record is a CRC-bearing long record with
24 obsolete bytes when its declared length is 28; its reader has no field
grammar, and the current writer does not emit it. The container now admits its
bounded framing and retains it as an unsupported setting record.
`ON_3dmSettings::Write_v2/Read_v2` also define the three counted presentation
lists. Named construction planes use long `TCODE_VIEW_CPLANE` children; named
views and active views use long `TCODE_VIEW_RECORD` children. Each outer list
CRC covers the count and direct suffix bytes, excluding complete child chunks.
`ON_3dmView::Write/Read` defines the ordered child writers, the archive gates
for viewport userdata, V3 wallpaper, and view attributes, and the short zero
`TCODE_ENDOFTABLE` terminator. The reader skips unknown children before that
marker and accepts a bounded suffix after it. The Rust view parser now requires
the source child type, stops at the marker, and reports a typed loss for a
failed child or malformed named-construction-plane list. The view-attributes
reader invokes `ON_3dmPageSettings::Read` at outer minor 2. Its anonymous child
has version 1.0 on write, a page number, width and height, four millimeter
margins, and a UTF-16 printer name; the major-1 reader accepts every
nonnegative minor and skips later bytes at the child boundary before continuing
with the outer fields. `ON_3dmViewPosition::Write/Read` uses the direct packed
1.0/1.1 body, with the four normalized window bounds, maximized flag, and the
archive-5 floating-viewport byte. Its reader repairs the bounds, leaves source
defaults for unknown majors, and lets the enclosing long child skip later bytes.
The Rust view parser now decodes this value into the native view record.
`ON_3dmViewTraceImage::Write/Read` emits packed 1.3 below archive 60 and 1.4
at archive 60, adding the section 20.1 file-reference child at minor 4.
`ON_3dmWallpaperImage::Write/Read` emits packed 1.1 below archive 60 and 1.2
at archive 60, adding the same child at minor 2; the separate legacy wallpaper
child carries only the UTF-16 path. The Rust image readers now mirror these
writer bands and minor gates.
`ON_BinaryArchive::Write3dmLight` and `Read3dmLight` use an `ON_Light` class
wrapper followed by optional light-record attributes, optional attribute
userdata, and a short light-record-end marker. The light class-data writer
emits packed version 1.2; its major-1 prefix contains the enabled flag, style,
intensity, watts, three colors, direction, location, degree-valued spot angle,
spot exponent, attenuation, shadow intensity, archive index, UUID, name,
length, width, and hotspot. The length and width fields are minor 1 gates and
the hotspot is a minor 2 gate. The Rust light-table decoder bounds the class
wrapper separately from those record children.
`ON_BinaryArchive::Write3dmGroup` and `Read3dmGroup` use a group record whose
only record child is the `ON_Group` class wrapper. `ON_Group::Internal_WriteV5`
emits packed version 1.1 with the archive index, UTF-16 name, and UUID; its
reader accepts major 1 and leaves later bytes at the class-data boundary. The
Rust group-table decoder now consumes the same bounded prefix. This group
writer/reader slice is settled; the RS-01 residue is limited to the
uncharacterized direct-reader, writer-band, and tagged-stream families.
`ON_HatchPattern::Write` selects packed version 1.2 below archive 60 and an
anonymous version-1.0 body at archive 60 and later. The V5 body contains the
component index, fill type, name, description, optional packed 1.1 hatch-line
array, and UUID. The modern body contains the filtered model-component
attributes, fill type, description, and a bounded anonymous line-list whose
elements are anonymous major-1 line records. The shared hatch reader admits
the V5 compatibility branch for the old archive-60 writer band. The Rust
hatch-pattern parser consumes both branches and leaves later bytes at their
respective boundaries.
The property readers are also source-backed: `ON_3dmRevisionHistory` uses a
major-1 prefix, `ON_3dmNotes` uses major 1 with `locked` at minor 1, and
`ON_3dmApplication` reads its three strings without a major/minor gate; all
three stop at their containing property-record boundaries.
`ON_InstanceDefinition::Internal_ReadV5` in
`/home/pcurve/side2/opennurbs/opennurbs_instance.cpp` requires packed major 1,
gates the V5 prefix through minor 1.7, and returns after the optional 1.7
file-reference record, leaving the obsolete linked-layer-settings Boolean and
later bytes at the class-data boundary. `Internal_ReadV6` requires anonymous
major 1 for the outer and linked-type chunks and closes each bounded child
after its fixed prefix. `ON_UnitSystem::Read`, `ON_FileReference::Read`,
`ON_ContentHash::Read`, `ON_SHA1_Hash::Read`, and
`ON_ReferencedComponentSettings::Read` use the same major-1 bounded-child
rule; file references add the embedded-file UUID at minor 1. Referenced
component settings has an outer major-1 presence wrapper and an optional
major-1 implementation child containing the two layer arrays and optional
parent layer. The Rust instance parser now matches both boundaries. Owner
tests cover future outer minors, the abandoned V5 tail, a future
file-reference suffix, and source-shaped referenced-component settings. The
remaining RS-01 residue is the uncharacterized direct-reader, writer-band,
and tagged-stream families.
`ON_MorphControl::Write` emits anonymous major 2 minor 1. Its reader accepts
major 1 and major 2 with nonnegative minors, consumes the known major-specific
prefix, and closes the outer anonymous boundary; other major versions are
rejected. The Rust morph parser now mirrors this gate and its owner tests cover
a major-2 future-minor suffix and a rejected major 3. The producer-shaped
version witness is recorded in the notebook.

**Need.** Producer writer/reader evidence for each remaining reader, or an
independent witness that distinguishes an appendable suffix from a changed
layout. Remove only rejection not required by that evidence.

**Note.** Narrowed 2026-08-16. The bounded-reader subset is substantially
settled; annotation settings, grid defaults, units/tolerances, plugin list,
settings attributes, render-mesh, analysis-mesh, render-settings, render
settings userdata, the current-selector records, and the V1 settings
presentation stream, and revision-history, notes, and application property
records are now source-backed through their known prefixes,
child types, stream markers, version gates, settings-attributes, mesh, and
modern render-settings outer-CRC ranges. The
historical unused settings
record and the three counted view-list wrappers are also source-backed through
the same evidence.
The residual is the explicit
writer-band/tagged/direct-reader audit for readers not yet characterized.

### TE-01. Class-specific transfer differential evidence

**Question.** Which remaining Rhino-authored object classes differ from the
committed transfer fixtures, and which byte-level fields cause each difference?

**Known.** The public source and example-file comparison establishes aggregate
source floors and class admission, while the synthesized tier covers a point
and structured objects. The `ON_Light` class is now independently covered by
V4, V5, and V6 OpenNURBS witnesses. Its class wrapper, light-record child
boundary, packed 1.2 payload, document-unit scaling, degree-valued spot angle,
and explicit-hotspot versus exponent-sentinel representations are settled;
the Rust native record preserves those raw fields. The `ON_Material` class is
also independently covered by V4, V5, and V6 witnesses. Its V4/V5 direct
legacy RDK material-instance carrier, universal render-engine plug-in
precedence, and V6 inline UUID field are settled; the Rust material record
transfers the compatibility UUID and retains the class-data UUID otherwise.
The `ON_Group` class is independently covered by V4, V5, and V6 witnesses. Its
class UUID, class-data boundary, packed 1.1 archive-index/name/UUID prefix, and
object-attribute group-index links are settled; the Rust group record preserves
the fields and emits both witnessed member links.
The `ON_HatchPattern` class is independently covered by V4, V5, and V6
witnesses. Its class wrapper, V4/V5 packed 1.2 identity/description/fill/line
fields, V6 anonymous component/line-list branch, line units, and nested line
boundary are settled; the Rust hatch-pattern record preserves the fields in all
three archive versions.
The `ON_Linetype` class is independently covered by V4, V5, and V6 witnesses.
Its class UUID, table-record wrapper, V4/V5 anonymous 1.1 identity and segment
payload, V6 anonymous 2.3 model-attributes and item stream, segment type tags,
and minor-gated defaults are settled. The false/true model-distance differential
also establishes the CADIR conversion boundary: legacy and print-distance
segments remain print millimeters, while true model-distance segments use the
document millimeters-per-unit scale; width and taper values retain their unit
selector.
The `ON_TextStyle`/`ON_DimStyle` table transition is independently covered by
V4, V50, and V6 model witnesses. Below archive version 60, the writer emits
one compatibility font-table record per dimension style, and the dimension
style stores its legacy text-style reference; the Rust decoder transfers that
record as a typed `text_styles` entry. At archive version 60, the font table is
empty, the dimension-style text-style index is unset, and the modern
font-characteristics child is retained as bounded source metadata under the
owning dimension style. The source-defined font child grammar is the same
anonymous `ON_Font` grammar used by modern text styles.
The `ON_Layer` class is independently covered by V4, V50, and V6 model
witnesses. Its packed 1.15 base prefix, V4 child-name rewrite, archive and
component references, IGES level, colors, visibility and locking, UUID and
parent fields, rendering child, display-material UUID, item-34 new-detail
visibility flag, item-35 direct section-style child, and class-owned
per-viewport userdata are settled. The Rust decoder transfers the base values,
IGES level, item 34, and normalized per-viewport entries; it bounds and
validates item 35 while retaining the complete layer record for source
fidelity.

The rendering-attributes transfer slice is independently covered by the
authored V4, V50, and V6 point witnesses. `ON_RenderingAttributes::Write/Read`
defines the layer material-reference array. `ON_ObjectRenderingAttributes::Write/Read`
defines the object material and mapping-reference arrays, the shadow flags,
and the advanced-preview byte. `ON_MappingRef::Write/Read` and
`ON_MappingChannel::Write/Read` define the plug-in UUID, channel ID, mapping
UUID, and row-major 16-double object transform. The mapping-reference CRC
covers its direct fields and excludes each nested channel; the outer
rendering-attributes and object-attributes CRCs likewise exclude complete
nested children. The Rust presentation record transfers material references,
mapping references and channels, and the three non-default object flags using
the CADIR fields defined in section 8.4.
The `ON_TextDot` class is independently covered by V4, V50, V6, and inch-unit
V6 witnesses. Its class UUID, direct packed 1.0/1.1 payload, archive-60
secondary-text gate, display bits, and document-unit center conversion are
settled; the Rust annotation decoder transfers the complete known prefix to
`native.rhino.text_dots` and keeps the record linked to its object without
creating neutral geometry.
The `ON_Hatch` class is independently covered by V4, V50, V6, and inch-unit
V50/V6 witnesses. Its packed 1.1/1.2 payload, plane and loop boundaries,
pattern fields, V5 basepoint userdata carrier, V6 inline basepoint, and
document-unit scaling are settled. The Rust decoder emits the hatch feature
and linked loop curves, retains the native pattern index and source object,
and reports the specified non-neutral-fill boundary.
The `ON_DetailView` class is independently covered by V4, V50, V6, and
inch-unit V6 witnesses. Its anonymous 1.1 outer payload, bounded view-state
and raw two-dimensional boundary children, page-layout millimeter boundary
coordinates, dimensionless page-per-model ratio, and archive-independent
boundary transfer are settled. The Rust decoder emits the detail feature and
linked boundary curve, retains the bounded view payload as length and SHA-256
properties, and reports the specified view-state retention boundary.
The `ON_NurbsCage` class is independently covered by V4, V50, V6, and
inch-unit V6 witnesses. Its anonymous 1.0 dimension, rational flag, orders,
counts, U/V/W knot vectors, and ordered control net are settled. The Rust
decoder transfers the complete nonrational cage state to native feature
parameters and properties, converting only spatial control coordinates and
retaining the source object for the native lattice boundary.
The `ON_MorphControl` class is independently covered by V4, V50, V6, and
inch-unit V6 witnesses. Its modern anonymous 2.1 variant-3 payload, bounded
start-transform and end-cage children, sorted captive UUID list, spherical
localizer, option fields, and class boundary are settled. The Rust decoder
transfers the complete witnessed cage variant, resolves the captive-object
link, converts spatial values and transform translations, and retains the
source object because it does not apply the deformation.
The `ON_PolyEdgeCurve` class is independently covered by V4, V50, V6, and
inch-unit V6 witnesses. Its inherited polycurve payload, persistent segment
class UUID, object UUIDs, component type/index pairs, edge and trim domains,
proxy-reversal byte, segment domains, and referenced-curve domains are
settled. The Rust decoder preserves the construction values, resolves unique
segment UUIDs to source-record links, and retains unresolved or ambiguous
references as explicit reference losses.

**Need.** For each remaining affected class, an independent witness file and a
byte-level differential report that names the field, accepted or rejected
outcome, and typed or opaque transfer result.

**Note.** Narrowed 2026-08-16. Aggregate floors and decoder tests do not answer
the per-class field question. The light-class remainder is closed by the
OpenNURBS writer/reader trace in `opennurbs_light.cpp` and
`opennurbs_archive.cpp`, the authored V4/V5/V6 light witnesses, the
`cadmpeg inspect` class-data and raw-field reads, and the owner tests
`light_table_class_data_stops_before_record_children`,
`light_scales_spatial_values_but_not_direction_or_angles`, and
`light_preserves_unset_hotspot_for_exponent_interface`. The material slice is
closed by `ON_Material::Internal_WriteV5`/`Read` and
`ON_RdkUserData::DeleteAfterRead`, the authored V4/V5/V6 material witness,
the `cadmpeg inspect` direct-XML and inline-UUID reads, and the owner tests
`legacy_rdk_material_userdata_transfers_uuid_from_unterminated_xml`,
`legacy_rdk_material_userdata_ignores_terminated_callback_xml`, and
`legacy_rdk_material_userdata_rejects_malformed_xml`. The group slice is
closed by `ON_Group::Internal_WriteV5`/`Internal_ReadV5` and
`ON_BinaryArchive::Write3dmGroup`/`Read3dmGroup`, the authored V4/V5/V6 group
witness, `cadmpeg inspect` reads of the group table and class-data wrapper, and
the three-version `cadmpeg query item` result showing the preserved index,
name, UUID, and two object links. The hatch-pattern slice is closed by
`ON_HatchPattern::Write`/`Read`, `WriteV5`/`ReadV5`, `ON_HatchLine::Write`/`Read`,
and `ON_BinaryArchive::Write3dmHatchPattern`/`Read3dmHatchPattern`, the
authored V4/V5/V6 hatch witness, `cadmpeg inspect` reads of the legacy and
anonymous payload branches, the three-version `cadmpeg query item` result,
and the owner test `modern_hatch_pattern_reads_nested_line_chunks`.
The linetype slice is closed by `ON_Linetype::Write`/`Read`,
`ON_BinaryArchive::WriteLinetypeSegment`/`ReadLinetypeSegment`,
`ON_BinaryArchive::Write3dmLinetype`/`Read3dmLinetype`, and
`ON_BinaryArchive::WriteModelComponentAttributes`/
`ReadModelComponentAttributes`, the authored V4/V5/V6 linetype witnesses and
inch-unit differential witnesses, the `cadmpeg inspect` table/class-data and
segment-tag reads, the three-version `cadmpeg query item` results, and the
owner tests `legacy_linetype_preserves_print_lengths_and_wire_segment_tags` and
`modern_linetype_scales_only_model_distance_segments`.
The font/dimension-style slice is closed by
`ON_BinaryArchive::EndWrite3dmDimStyleTable`/`BeginRead3dmDimStyleTable`,
`ON_DimStyle::Write`/`Read`, `ON_TextStyle::Write`/`Read`, and
`ON_Font::Write`/`WriteV5`, the rebuilt OpenNURBS
`dimension_style_model_witness` outputs
`dimension-style-current-font-v4.3dm`, `dimension-style-current-font-v50.3dm`,
and `dimension-style-current-font-v60.3dm`, `cadmpeg inspect` reads showing
font-record headers at V4/V50 and none at V6 plus the V6 anonymous 1.6 font
child, the three-version `cadmpeg query item` results, and the owner test
`dimension_style_future_minor_preserves_known_prefix_and_suffix`.

The layer slice is closed by `ON_Layer::Write`/`Read` and
`ON_BinaryArchive::Write3dmLayer`/`Read3dmLayer` in
`/home/pcurve/side2/opennurbs/opennurbs_layer.cpp` and
`opennurbs_archive.cpp`, `ON__LayerPerViewSettings::Write`/`Read`,
`ON__LayerExtensions::Write`/`Read`, and `ON_SectionStyle::Write`/`Read`.
The authored witness
`/home/pcurve/side2/tmp/agent-rhino-l9-20260816/layer_model_witness.cpp`
adds two layers, referenced materials and linetypes, a child-layer parent,
non-default base fields, item-34 and item-35 data, and a complete per-viewport
override, then writes V4, V50, and V6 files. The public `example_read` harness
reads all three; V4 renames the child to `child-layer (e875)` while V50/V6
retain `child-layer`. `cadmpeg inspect` finds the `ON_Layer` class UUIDs at
V6 offsets `3201` and `3458`, the layer-extension class/item UUIDs at `3786`
and `3802`, the OpenNURBS application UUID at `3950`, and the V6 child layer
payload shows the item-34 Boolean, the item-35 anonymous section-style child,
and the bounded userdata child. The three-version `cadmpeg query item` output
transfers IGES levels `42`/`43`, material and linetype indices `0`/`1`, the
parent UUID, the V4 name rewrite, `visible_in_new_details=false`, and the
per-viewport mask `63` with colors, weight `2.25`, visible value `2`, and
persistent-visibility value `2`. The owner test
`parses_layer_class_wrapper_and_rendering_chunk` now gates the base fields and
item-34 Boolean; `layer_extensions_read_effective_fields_sort_entries_and_apply_root_rule`
gates the normalized userdata values.

The witness initially omitted the referenced material and linetype components;
OpenNURBS therefore resolved their layer references to `-1`. Adding the
components and using their assigned manifest indices produced the final
reference-bearing witness. That harness correction is not format evidence.

The rendering-attributes slice is closed by
`ON_RenderingAttributes::Write`/`Read`,
`ON_ObjectRenderingAttributes::Write`/`Read`, `ON_MappingRef::Write`/`Read`,
and `ON_MappingChannel::Write`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_material.cpp`, the authored witness
`/home/pcurve/side2/tmp/agent-rhino-l9-20260816/rendering_attributes_witness.cpp`
and its V4/V50/V6 files, `cadmpeg inspect` reads of the mapping and channel
headers and 16 transform doubles, the rebuilt `example_read` outputs for all
three archives, the three-version `cadmpeg query item` output showing the
material reference, mapping channel, translation transform, false shadow
flags, and true advanced-preview flag, and the owner tests
`object_rendering_attributes_consume_mapping_reference_and_channel`,
`rendering_attributes_parse_object_mapping_and_future_suffix`, and
`rendering_attributes_transfer_mapping_channels_and_flags`.

The text-dot transfer slice is independently covered by the authored V4, V50,
V6, and inch-unit V6 witnesses. `ON_TextDot::Write/Read` defines the packed
1.0/1.1 class-data prefix, the center, height, strings, display-bit meanings,
and the archive-60 secondary-text gate. The Rust annotation decoder transfers
the complete known prefix to `native.rhino.text_dots`; center coordinates use
the document length scale, while height, strings, font face, and display flags
remain unchanged. The V4/V50/V6 `cadmpeg inspect`, `example_read`, and
`cadmpeg query item` results establish the minor transition and all four bits;
the inch differential establishes the length conversion. The owner tests
`text_dot_preserves_text_style_flags_and_scaled_location` and
`text_dot_v10_omits_secondary_text_and_skips_suffix` gate the known prefix and
bounded suffix behavior.

The hatch-object slice is closed by `ON_Hatch::Write`/`Read` and
`ON_HatchLoop::Write`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_hatch.cpp:1178-1223,1496-1577`, the
authored
`/home/pcurve/side2/tmp/agent-rhino-l9-20260816/hatch_object_witness.cpp`, and
its V4/V50/V6 plus inch-unit V50/V6 outputs. `cadmpeg inspect` reads the
class-data packed bytes `0x11`, `0x11`, and `0x12`, the plane origin, pattern
fields, loop count 2, and the V50 userdata class/item UUID at the recorded
offsets. OpenNURBS `example_read` reports the V4 missing basepoint carrier,
the V50 userdata-restored basepoint, and the V6 inline basepoint. The initial
inch differential exposed the missing V6 inline basepoint scale; the Rust fix
now transfers both V50 and V6 basepoints as `[38.1,-57.15]` millimeters while
scaling plane and loop geometry and preserving pattern scale/rotation. The
three-version `cadmpeg query item` results show the hatch feature and outer /
inner loop links; the report records the specified native-fill retention.
The owner test `decodes_version_two_loop_geometry_and_pattern_state` now
asserts the scaled inline basepoint.

The detail-view slice is closed by `ON_DetailView::Write`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_detail.cpp:62-162`, the source field
comment in `/home/pcurve/side2/opennurbs/opennurbs_detail.h:68-75`, and
`ON_3dmView::Write`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_3dm_settings.cpp:3507-3955`, the
authored
`/home/pcurve/side2/tmp/agent-rhino-l9-20260816/detail_view_witness.cpp`, and
its V4/V50/V6 plus inch-unit V6 outputs. `cadmpeg inspect` finds the detail
class UUID at V4 `0x8da`, V50 `0x9d8`, V6 `0xa13`, and inch V6 `0xa21`; the V4
class-data bytes show the outer anonymous body at `0x8f6`, version `1.1` at
`0x8fe`, view child at `0x906`, boundary child at `0xd7b`, raw NURBS version
`0x10` at `0xd8b`, dimension `2` at `0xd8c`, and ratio `0.125` at `0xe58`.
The initial decoder run rejected the producer's 2D boundary as an invalid
3D NURBS header, falsifying the old dimension assumption; the inch
differential also rules out model-unit scaling because the source readback
and final CADIR boundary points remain `[10,20]`, `[110,20]`, `[110,70]`,
`[10,70]`, `[10,20]` in both unit systems. The source readback reports ratio
`0.125`, nested view type, viewport UUID, camera, target, and five boundary
CVs for all witnesses. Final `cadmpeg query item` results transfer the same
ratio and boundary link; V60 and inch V6 retain equal view payload length
`1558` and equal SHA-256. The owner test
`decodes_boundary_and_bounds_native_view_state` now uses a dimension-2 raw
curve and asserts unscaled page coordinates.

The NURBS-cage slice is closed by `ON_NurbsCage::Write`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_nurbsvolume.cpp:29-228`, the authored
`/home/pcurve/side2/tmp/agent-rhino-l9-20260816/cage_object_witness.cpp`, and
its V4/V50/V6 plus inch-unit V6 outputs. `cadmpeg inspect` reads the class
UUID at V4 `0x8d6`, V50 `0x9d4`, and V6 `0xa0f`; the V4 class-data child has
anonymous body `0x8f2`, version `1.0` at `0x8fa`, dimension `3` at `0x902`,
orders and counts `2,2,2`, and U/V/W knot pairs `[0,1]`. OpenNURBS readback
reports the same nonrational 2x2x2 cage and first/last control points
`[10,20,30]` and `[30,50,70]` in all four witnesses. Final
`cadmpeg query item` results transfer the same parameters, knots, and control
points in V4/V50/V6; the inch result converts them to the corresponding
millimeter values while retaining knots. The owner `cage` tests continue to
gate rational weights and future-minor suffix handling.

The morph-control slice is closed by `ON_MorphControl::Read`/`Write` in
`/home/pcurve/side2/opennurbs/opennurbs_nurbsvolume.cpp:2540-2765`,
`ON_Localizer::Read`/`Write` in
`/home/pcurve/side2/opennurbs/opennurbs_morph.cpp:81-180`, and
`ON_UuidList::Read`/`Write` in
`/home/pcurve/side2/opennurbs/opennurbs_array.cpp:793-871`, the authored
`/home/pcurve/side2/tmp/agent-rhino-l9-20260816/morph_object_witness.cpp`,
and its V4/V50/V6 plus inch-unit V6 outputs. `cadmpeg inspect` finds the
morph class UUID at V4 `0x9c1`, V50 `0xa71`, V6 `0xaaa`, and inch V6 `0xab4`;
the V4 class-data child starts at `0x9d5`, the outer anonymous payload is
version `2.1` at `0x9e5`, the variant is `3` at `0x9ed`, the start-transform
child begins at `0x9f1`, the end-control child at `0xa85`, the nested cage at
`0xa95`, the captive UUID list at `0xbbd`, and the localizer list at `0xbe5`.
The source readback reports the same transform, captive UUID, spherical
localizer `(type 1, point [7,8,9], interval [5,2])`, tolerance `0.75`, and
option flags in all four files. Final V4/V50/V6 query results transfer the
same cage, transform, localizer, captive-object link, and options; inch V6
converts end control points, localizer point and interval, transform
translation, and tolerance by `25.4` while retaining transform linear terms,
knots, and vectors. The existing `morph` owner tests gate the bounded cage,
transform, localizer, future-minor, and major-rejection paths. This change
closes this slice; the docs gate and pre-commit workspace gate remain to be
run.

The curve-on-surface slice is closed by `ON_CurveOnSurface::Write`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_curveonsurface.cpp:167-232`, the
authored `/home/pcurve/side2/tmp/agent-rhino-l9-20260816/curve_on_surface_witness.cpp`,
and its V4/V50/V6 plus inch-unit V6 outputs. `cadmpeg inspect` finds the
`ON_CurveOnSurface` class UUID at V4 `0x8d6`, V50 `0x9d4`, V6 `0xa0d`, and
inch V6 `0xa17`; the nested line-carrier UUID occurs twice and the plane
surface UUID once in each class payload. The producer writer emits the
parameter curve, presence integer, optional model curve, and support surface
in that order. The source reader's optional-C3 branch unconditionally clears
its success flag, so the OpenNURBS model reader drops this otherwise valid
object; that implementation defect is not promoted to a format rule. The
Rust decoder transfers all three carriers: V4/V50/V6 keep parameter controls
`[0.75,1.75]` and `[2.25,4.25]`, while inch V6 keeps those values and converts
the model curve to `[381,635,889]`/`[1143,1397,1651]` and the support-plane
origin to `[127,152.4,177.8]`. The owner test
`decodes_parameter_model_and_support_carriers` now asserts the parameter
curve remains unscaled while the model curve and support plane scale.

The polyedge-reference slice is closed by `ON_PolyCurve::Write`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_polycurve.cpp:410-499` and
`ON_PolyEdgeSegment::Init`/`Create`/`Write`/`Read` in
`/home/pcurve/side2/opennurbs/opennurbs_polyedgecurve.cpp:25-39,103-139,780-847`, the
authored `/home/pcurve/side2/tmp/agent-rhino-l9-20260816/polyedge_witness.cpp`,
and its V4/V50/V6 plus inch-unit V6 outputs. `cadmpeg inspect` finds the
`ON_PolyEdgeCurve` class UUID at V4 `0x0b7c`, V50 `0x0be8`, V6 `0x0c21`, and
inch V6 `0x0c2b`; the first persistent segment UUID occurs at V4 `0x0c2d`,
V50 `0x0cad`, V6 `0x0ce6`, and inch V6 `0x0cf0`; the second occurs at V4
`0x0cd2`, V50 `0x0d66`, V6 `0x0d9f`, and inch V6 `0x0da9`. The
valid producer witness uses two segments and records parameter values
`[10,20,35]`, component pairs `[31,17]` and `[2,23]`, edge domains
`[-1.23432101234321e+308,-1.23432101234321e+308]`/`[6,8]`, trim domains
`[-1.23432101234321e+308,-1.23432101234321e+308]`/`[7,9]`, segment domains
`[-20,-10]`/`[-10,5]`, referenced domains `[1,9]`/`[3,18]`, and one reversed
segment. `ON_PolyEdgeSegment::Create` leaves the first source-curve
segment's edge and trim intervals at the finite `ON_UNSET_VALUE`
empty-interval sentinel. OpenNURBS model readback exposes the two source curves and the
persisted segment fields but cannot restore the runtime proxy carriers; its
reader also loses the reversal flag because it calls `Reverse` before
`SetDomain`. The Rust decoder retains the reversal and every persisted field,
links both UUIDs to the source curve records, and emits the native-retention
loss. Inch V6 scales only the two source curve geometries; the polyedge
parameter array, component pairs, reversal, and all domains remain unchanged.
The owner test `decodes_persistent_polyedge_segment_construction` now also
asserts the persisted segment UUID, and
`accepts_empty_edge_and_trim_domains_for_a_source_curve_segment` gates the
finite empty-interval sentinel.

### FV-06. Later major payload admission

**Question.** Which later major versions of built-in table, object, geometry,
presentation, or userdata payloads may enter typed decoding?

**Known.** The specification now distinguishes source-defined major families,
bounded later-minor suffixes, writer-band ceilings, and opaque unknown records.
A later major still needs its own field grammar; complete byte retention does
not establish typed admission.

**Need.** Producer source or an independent witness for each later major that is
to be admitted, naming its fields, boundaries, and neutral mapping.

**Note.** Narrowed 2026-08-16. The admission distinction is settled; later
major field grammars remain open.

### FV-07. Later minor payload suffixes

**Question.** Which fields and boundaries do future minor versions append after
the known prefix of each built-in payload?

**Known.** Source-defined later-minor readers consume known prefixes and skip
bounded suffixes. The specification does not assign names or meanings to
future bytes that no audited producer writes.

**Need.** A future producer writer/reader or an independent witness for each
future suffix, with its field order, boundary, and typed admission rule.

**Note.** Narrowed 2026-08-16. Suffix preservation is settled; future suffix
field semantics remain open.
