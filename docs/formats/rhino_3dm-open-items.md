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
not establish typed compatibility with a new major layout.

**Need.** A producer implementation or independent witness for each later
object-class major/version that is to enter typed decoding, with its field
grammar and neutral mapping.

**Note.** Narrowed 2026-08-16. Bounded later-minor handling is settled for the
audited classes; later object-class versions with changed layout remain open.

### FV-02. Future table-record payloads

**Question.** Which later table-record versions retain typed decoding, and what
fields do those versions add or change?

**Known.** The audited material, texture, group, light, linetype, hatch, font,
dimension-style, view, and settings readers consume known bounded prefixes and
skip source-defined suffixes. Tagged streams, explicit terminators, and
writer-band ceilings remain grammar controls. An unknown table record has no
typed fields from its typecode alone.
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

**Need.** The later userdata class writer and reader, or an independent witness,
for each version that is to be typed, including its fields and loss mapping.

**Note.** Narrowed 2026-08-16. Generic header and boundary semantics, the
built-in hatch gradient userdata, the V5 dimension-style extra, and
`ON_UserStringList` are settled; other future class-specific payload semantics
remain open.

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
extension code after the component-attributes and segment prefix. `ON_Font::Read`
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
2 through V5 and minor 4 from V6, gating scaling fields at minors 1 through 4.
`ON_3dmConstructionPlaneGridDefaults` has a packed major-1 body with no minor
field gates. `ON_BinaryArchive::BeginRead3dmChunk` requires a nonnegative
modern minor. The Rust document-settings reader now consumes both complete
known prefixes and leaves only the respective containing boundary suffix.
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
failed child or malformed named-construction-plane list.
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
and structured objects. It does not preserve an independent byte-level,
class-by-class differential witness for every affected class.

**Need.** For each affected class, an independent witness file and a byte-level
differential report that names the field, accepted or rejected outcome, and
typed or opaque transfer result.

**Note.** Narrowed 2026-08-16. Aggregate floors and decoder tests do not answer
the per-class field question.

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
