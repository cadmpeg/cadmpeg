# Rhino 3DM Open Items

The remaining questions are narrowed to producer-defined payload fields or
class-specific transfer evidence. Settled format rules remain in
[`rhino_3dm.md`](rhino_3dm.md). OpenNURBS transfer evidence remains in
[`rhino_3dm-opennurbs-comparison.md`](rhino_3dm-opennurbs-comparison.md).

## Remaining items

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
