# Rhino 3DM Open Items

Settled format rules remain in
[`rhino_3dm.md`](rhino_3dm.md). OpenNURBS transfer evidence remains in
[`rhino_3dm-opennurbs-comparison.md`](rhino_3dm-opennurbs-comparison.md).

## Remaining items

### FV-01. Future object-class payloads

**Question.** Which later versions of each built-in object-class payload retain
typed compatibility, and which fields remain admissible?

**Known.** The typed readers consume the specified class prefixes and bounded
minor suffixes. A later major has no typed meaning from its class UUID or
retained source record. The current producer paths define the object-class
families specified in sections 12 through 18; they do not define a later
object-class major.

**Need.** A producer implementation or independent versioned witness for each
later object-class major that is to enter typed decoding, with its field
grammar, boundaries, and neutral mapping.

**Note.** The absence of a current later producer does not settle a future
layout. Opaque retention is not typed compatibility.

### FV-02. Future table-record payloads

**Question.** Which later table-record versions retain typed decoding, and what
fields do changed versions add or change?

**Known.** Current producer-defined additions include dimension-style minor-10
and minor-11 fields, the archive-90 hatch-pattern tail, and layer extension
item 37. These individual branches do not define later table-record layouts in
general. Unknown table records remain bounded source records without typed
fields from their typecode alone.

**Need.** Producer source or an independent witness for each later table-record
major or changed layout that is to be admitted as typed data, including field
order, boundaries, defaults, normalization, and neutral mapping.

**Note.** The dimension-style, hatch, and layer closures each settled only a
subset of this item. The later archive-90 and layer reopenings show that an
individual current field is not complete table-record coverage.

### FV-03. Future user-data payloads

**Question.** Which later user-data versions have a typed payload grammar, and
which fields remain admissible?

**Known.** The generic user-data headers and the audited class-owned payloads
have bounded children and source-defined minor gates. Unknown classes can be
retained, but the generic header does not define their payload fields. The
current producer paths do not define a later user-data major or an untyped
class-specific payload grammar beyond the audited carriers.

**Need.** A later user-data class writer and reader, or an independent witness,
for each version that is to be typed, including its fields, boundaries, and
loss mapping.

**Note.** No current later producer is evidence that future class-specific
payload semantics are settled.

### FV-06. Later major payload admission

**Question.** Which later major versions of built-in table, object, geometry,
presentation, or user-data payloads may enter typed decoding?

**Known.** Section 20.6 retains an unknown major record and withholds typed
fields until a grammar and neutral admission rule exist. That refusal policy
prevents unsafe decoding, but it does not define the fields, boundaries, or
admission rule for any later major. The current producer inventory defines no
additional major family.

**Need.** Producer source or an independent witness for each later major that
is to be admitted, naming its fields, boundaries, validation, and neutral
mapping.

**Note.** Opaque retention and refusal are safety decisions, not evidence that
a later major has been characterized.

### FV-07. Later minor payload suffixes

**Question.** Which fields and boundaries do future minor versions append after
the known prefix of each built-in payload?

**Known.** Source-defined later-minor fields are consumed at their version
gates, and bytes after a known prefix remain bounded source bytes. A generic
future-minor policy does not assign names or meanings to a suffix that no
audited producer writes.

**Need.** A future producer writer and reader, or an independent witness, for
each suffix that is to be typed, with its field order, boundary, validation,
and neutral admission rule.

**Note.** The current producer inventory and bounded retention policy do not
settle future suffix field semantics.

### RS-01. Later-minor bounded suffixes

**Question.** Which remaining versioned readers outside the settled sections
accept unread fields appended before their bounded end?

**Known.** Section 4.2 distinguishes direct-payload, anonymous-child, and
tagged-stream boundaries. The audited readers use those boundaries where their
producer rules are known. The direct-reader, writer-band, and tagged-stream
families named by the removed item still lack a reader-by-reader source
inventory that proves which later fields are appendable and which change the
layout. The tagged linetype, section-style, and layer readers are now settled:
OpenNURBS writes non-default item codes in increasing order, consumes the same
ordered cascade with each family’s minor gates, and leaves an out-of-order,
gate-inadmissible, or future item value at the containing bounded boundary.
The Rust parsers follow those cascades.
The revision-history, notes, and application property readers are also settled
as direct prefixes: their writers emit the listed packed versions and fields,
their readers consume those prefixes, and the containing property record is the
only suffix boundary.
The units/tolerances, annotation-settings, grid-defaults, and render-settings
readers are likewise settled: the first three use direct settings-record
prefixes, while render settings use the source-selected legacy direct body or
the modern anonymous child; each remaining suffix is bounded by its enclosing
settings record or anonymous child.
The font/text-style compatibility pair is settled: the V5 packed branch and
the modern anonymous branch have separate known prefixes and boundaries, and a
modern font suffix cannot consume the enclosing text-style identity fields.
The group and light class-data readers are also settled: group 1.1 and light
1.2 are packed major-1 prefixes, with group UUID, light length/width, and light
hotspot admitted only at their documented minor gates; later bytes remain at
the `OPENNURBS_CLASS_DATA` boundary. The material, texture, texture-array,
texture-mapping, material-reference, mapping-reference, and mapping-channel
readers now have the same child-by-child inventory. Their anonymous and class
children close independently, source writer bands select the known texture
minors, and later bytes remain at the child that contains them. A material
reference consumes and drops its obsolete mapping-channel array before its
minor-gated back-face fields.
The hatch-pattern and hatch-line readers now have the same source-selected
inventory: packed pattern 1.2 and line 1.1 below archive 60, anonymous pattern
and line children from archive 60, and the pattern unit-system and
model-distance fields at archive 90. The line, line-list, and pattern child
boundaries are independent; the direct `ON_Hatch` record separately admits its
minor-2 basepoint from archive 60 and leaves loop children bounded.
The viewport direct payload is also settled: its writer emits packed 1.5 in
every archive, its reader gates the UUID, locks, target, camera-frame validity,
and view scale at minors 1 through 5, and later bytes remain at the
`TCODE_VIEW_VIEWPORT` child boundary. CADIR withholds typed fields for another
major while retaining the enclosing view child.
The view-attributes child is settled as packed version 1.9: its direct fields
are gated at minors 1 through 9, its page-settings and clipping-plane children
close independently, and later bytes remain at the
`TCODE_VIEW_ATTRIBUTES` boundary.
Trace-image and V3 wallpaper children are also settled: their writers select
1.3/1.4 and 1.1/1.2 at archive 60, their file-reference children are gated at
minors 4 and 2, and their remaining bytes stay at the respective view-child
boundaries.

**Need.** Producer writer/reader evidence for the remaining direct-reader and
writer-band families. Record the field gate, containing boundary, and admission
rule.

**Note.** Partly settled 2026-08-17 with a different evidence kind: the
OpenNURBS `ON_Linetype::Write`/`Read`, `ON_SectionStyle::Write`/`Read`, and
`ON_Layer::Write`/`Read` implementations establish the ordered tagged-stream
boundaries; an authored out-of-order section-style payload witness and owner
tests exercise the boundary. The property subset is established by
`ON_3dmRevisionHistory::Write`/`Read`, `ON_3dmNotes::Write`/`Read`, and
`ON_3dmApplication::Write`/`Read`; the owner test exercises direct suffixes.
The settings subset is established by `ON_3dmUnitsAndTolerances::Write`/`Read`,
the V1 units helper, `ON_3dmAnnotationSettings::Write`/`Read`,
`ON_3dmConstructionPlaneGridDefaults::Write`/`Read`, and
`ON_3dmRenderSettings::UseV5ReadWrite`, `Write`/`Read`, and
`WriteV5`/`ReadV5`; the document-settings owner tests exercise every known
gate and bounded suffix.
The font/text-style subset is established by `ON_TextStyle::Write`/`Read` and
`ON_Font::Write`/`Read` with `WriteV5`/`ReadV5`; the owner tests exercise the
modern nested boundary, future suffixes, and the legacy packed fields.
The group/light subset is established by `ON_Group::Internal_WriteV5`/
`Internal_ReadV5` and `ON_Light::Write`/`Read`; the group and light owner tests
exercise their class-data bounds and minor-gated fields. The material/mapping
subset is established by `ON_Material::Write`/`Read`, `ON_Texture::Write`/`Read`,
`ON_TextureMapping::Internal_WriteV5`/`Internal_ReadV5`,
`ON_MaterialRef::Write`/`Read`, `ON_MappingRef::Write`/`Read`,
`ON_MappingChannel::Write`/`Read`, and
`ON_ObjectRenderingAttributes::Write`/`Read` in the OpenNURBS 9.x source;
the owner tests exercise non-empty obsolete material-reference arrays and
future suffixes at texture, texture-array, mapping, mapping-reference, and
mapping-channel boundaries.
The hatch subset is established by `ON_HatchPattern::Write`/`Read`,
`WriteV8`/`ReadV8`, and `WriteV5`/`ReadV5`, `ON_HatchLine::Write`/`Read` and
`WriteV5`/`ReadV5`, and `ON_Hatch::Write`/`Read`; the presentation and hatch
owner tests exercise line, line-list, pattern, loop, and archive-90 suffix
boundaries.
The viewport subset is established by `ON_Viewport::Write`/`Read` in
`opennurbs_viewport.cpp`; the view owner test exercises bytes after the minor-5
view-scale prefix.
The view-attributes subset is established by `ON_3dmView::Write`/`Read` and
`ON_3dmPageSettings::Write`/`Read` in `opennurbs_3dm_settings.cpp`; the view
owner tests exercise page, clipping, and outer attributes suffixes.
The image subset is established by `ON_3dmViewTraceImage::Write`/`Read` and
`ON_3dmWallpaperImage::Write`/`Read`; the existing view image owner witness
exercises their direct suffixes and section 20.1 covers their shared file
reference.
The earlier aggregate closure did not provide this reader-level trace. The
remaining direct-reader and writer-band inventory remains open.
