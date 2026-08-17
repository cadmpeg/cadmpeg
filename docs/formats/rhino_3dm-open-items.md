# Rhino 3DM Open Items

Settled format rules remain in
[`rhino_3dm.md`](rhino_3dm.md). OpenNURBS transfer evidence remains in
[`rhino_3dm-opennurbs-comparison.md`](rhino_3dm-opennurbs-comparison.md).

## Remaining items

### PR-05. Current nested view-child checksum coverage

**Question.** Which direct byte ranges does the CRC on the current container
children `TCODE_VIEW_ATTRIBUTES` and `TCODE_VIEW_VIEWPORT_USERDATA` cover, and
how does the codec report a mismatch without changing the enclosing view
boundary?

**Known.** The direct-body long children `TCODE_VIEW_VIEWPORT`,
`TCODE_VIEW_CPLANE`, `TCODE_VIEW_TARGET`, `TCODE_VIEW_POSITION`,
`TCODE_VIEW_NAME`, `TCODE_VIEW_TRACEIMAGE`, `TCODE_VIEW_WALLPAPER`, and
`TCODE_VIEW_WALLPAPER_V3` are CRC-bearing leaves whose CRC covers their body.
The owner now verifies those leaves and preserves the typed view on mismatch;
a public V5 mutation reports `container.integrity-failure` at the child offset.
`TCODE_VIEW_ATTRIBUTES` and `TCODE_VIEW_VIEWPORT_USERDATA` remain CRC-bearing
containers with nested child streams, and their outer child checksums are not
yet verified.

**Need.** Trace the producer and reader CRC lifecycle for the anonymous
page-settings and clipping-plane children inside `TCODE_VIEW_ATTRIBUTES`, and
for the class-userdata children through the fake class-end marker inside
`TCODE_VIEW_VIEWPORT_USERDATA`. State each direct range, align the view owner
with the recoverable checksum policy, and add source-shaped owner tests. Keep
this child coverage separate from the already settled outer
`TCODE_VIEW_RECORD` checksum.

**Note.** The direct-leaf subset is settled in the specification and codec.
This remaining item does not change view field transfer or the CADIR identity
of a view.

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
typed fields for the class-owned payload.

**Need.** A later user-data class writer and reader, or an independent witness,
for each version that is to be typed, including its fields and boundaries. The
same change must state the field-specific loss or neutral mapping required by
the section-20.6 admission rule.

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
