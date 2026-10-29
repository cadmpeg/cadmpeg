# Creo View `.pvz` package: Open Items

This document lists the parts of the Creo View `.pvz`, `.pvs`, and `.ol` formats that we do not know. The specification `creo_view_pvz.md` gives the parts that we know.

Each item has these parts:

- **Question** — what we must find.
- **Known** — what the specification gives now.
- **Need** — why we must find the answer.
- **Conflict** — a disagreement between two documents, or between a document and the decoder. An item with this part needs a decision.
- **Note** — a defect in the item or in the specification.

When an item is resolved, delete it in the same change that writes the answer into the specification. Do not keep a Resolved part.

Each item has an identifier. Use the identifier in commit messages and in code comments.

This document uses ASD-STE100 Simplified Technical English. Record names, field names, and tag values are technical names.

## 1. Package and PVS structure

### PVZ-01. Other package members

**Question.** What are the grammars of the `.pvi`, `.xml`, `.xlf`, `.etb`, and `.ast` members, and how do they refer to definitions and occurrences?

**Known.** `creo_view_pvz.md` §1.2 names the member roles. `.pvi`, `.xml`, and `.xlf` members are UTF-8 XML. Element `0x3b` (§3.12) names each `.pvi` member. An `.ast` member begins with a `u32` length and the ASCII text `!/EMK V7.0`.

**Need.** We must know these grammars to decode illustrations, sequences, and annotations.

### PVZ-02. PVS signature digits

**Question.** What does each signature digit group `0301` to `0305` change in the PVS model?

**Known.** `creo_view_pvz.md` §3.1 states that the five signatures share the container, element, and atom grammar.

**Need.** We must know whether a signature adds element families or fields that §3 does not define.

### PVZ-03. Other element descriptors

**Question.** Which descriptor values other than the seven in `creo_view_pvz.md` §3.4 occur, and what length width do they select?

**Known.** §3.4 defines the bit fields. The string pool uses a two-byte length, so a string pool larger than 65535 payload bytes needs another descriptor.

**Need.** We must know the descriptor for a four-byte length to read large structure and string-pool streams.

### PVZ-04. Bytes after the definition name

**Question.** What do the bytes after the name atom of a typed definition (types `0x01`, `0x02`, `0x06`, `0x1e`) mean? Does a typed definition of type `0x1f` exist, with an optional `0x21` byte before four bytes from the set `07 07 03 03`, `07 07 04 04`, `0d 0d 08 08`?

**Known.** `creo_view_pvz.md` §3.7 gives the byte counts: one byte for types `0x01` and `0x02`, two bytes for type `0x06`, four bytes for type `0x1e`. Each type-`0x1e` value is one of `07 07 03 03`, `07 07 04 04`, and `06 06 05 05`. Each type-`0x06` value is `07 07` or `0d 0d`. §3.7 does not define type `0x1f`.

**Need.** We must know whether the bytes carry a model kind, a version, or a units choice.

### PVZ-05. Placement flags and occurrence extension

**Question.** What do the high four bits of the placement kind, the high byte of a two-byte occurrence type, and the extension bytes of a descriptor-`0x22` occurrence mean?

**Known.** `creo_view_pvz.md` §3.9. A high nibble of 6 occurs with an identity placement. The two-byte type `0x0208` occurs. Its extension holds a byte, then pairs of a string atom and one byte, for example the atoms `A472` and `A482`.

**Need.** We must know whether the flags change the placement or the visibility of the occurrence.

### PVZ-06. Occurrence attribute element `0x30`

**Question.** What does the one-byte payload of a family-`0x30` child of an occurrence mean?

**Known.** `creo_view_pvz.md` §3.9. Values 0 to 4 occur.

**Need.** We must know whether the value controls visibility, a display state, or a selection group.

### PVZ-07. Appearance palette elements

**Question.** What do the ten-value and six-value `0x2f` elements of the structure stream hold, and which definitions use them?

**Known.** `creo_view_pvz.md` §3.12. Type `0x15` holds ten `f32`, type `0x05` holds six `f32`. The first nine values are in the colour range, and the tenth value of type `0x15` is in the shininess range of §5.1.

**Need.** We must know the palette semantics to apply structure-level appearance overrides.

### PVZ-08. Illustration and group elements

**Question.** What do elements `0x3c`, `0x44`, and `0x45`, and the byte after the name of an illustration root, mean?

**Known.** `creo_view_pvz.md` §3.12 gives their framing. Element `0x3c` with type `0x06` holds a count byte and string atoms that also occur in descriptor-`0x22` occurrence extensions (PVZ-05).

**Need.** We must know these elements to connect illustrations to the occurrence tree.

### PVZ-09. Typed property bytes

**Question.** What do the three bytes after the two atoms of a descriptor-`0x12`, type-`0x05` property mean?

**Known.** `creo_view_pvz.md` §3.13.

**Need.** We must know whether they carry a value type or a unit.

### PVZ-10. XML PVS grammar

**Question.** What is the complete element and attribute grammar of the XML PVS form?

**Known.** `creo_view_pvz.md` §3.14 gives the elements that map to the binary model.

**Need.** We must know the complete grammar, including the elements for the binary auxiliary elements of §3.12, to decode an XML PVS without loss.

## 2. OL container and attribute stream

### PVZ-11. OL preamble block

**Question.** What do the `word`, `byte_0`, `generation`, and `byte_2` fields of the OL preamble block mean, and what decides the presence of the block?

**Known.** `creo_view_pvz.md` §4.3 gives the fields. `generation` is `0x09` only in `DIV-OL09BL` members. `word` is 2 in members that carry only the attribute stream, and 3 in members with more streams.

**Need.** We must know whether the block selects a grammar difference between `DIV-OL08BL` and `DIV-OL09BL`.

### PVZ-12. Appearance reference tail

**Question.** What do the 17 `tail` bytes of an appearance reference mean?

**Known.** `creo_view_pvz.md` §5.2. The tail is a `u32` (`0xffffffff` or 0), one byte (0 or 1), and twelve `0xff` bytes.

**Need.** We must know whether the tail refers to textures or decals (PVZ-17).

### PVZ-13. Other attribute records

**Question.** What are the grammars of the attribute-stream records with tags 3, 4, `0x19`, `0x1a`, `0x1b`, `0x2b`, `0xc8`, `0xcd`, `0x01000008`, `0x0100002c`, and the others in `creo_view_pvz.md` §5.4?

**Known.** Tag 3 holds a list of named identifier strings, for example `!pvFId`, `!pvId`, and `Layer`. Tag `0x0100002c` holds a feature identifier string and a feature name string. Tag `0x01000008` has a 34-byte payload.

**Need.** We must know these records to keep feature names, layers, and per-face attributes.

### PVZ-14. Patch header fields

**Question.** What do `face_ref`, `flag`, and `refs` in the tessellation patch header mean?

**Known.** `creo_view_pvz.md` §6.2. In a member with an exact stream, `face_ref` is less than the face count. The distance of the patch vertices from the surface of face `face_ref` is in the range 0 to 1 m: within 10⁻⁵ m for some patches, and 10⁻⁴ m to 1 m for others. `refs` holds one or more values; it can be the single value `face_ref`.

**Need.** We must know the link between tessellated patches and exact faces to give exact faces their appearance.

### PVZ-15. Other tessellation records

**Question.** What do the tessellation records with tags `0x0100000b`, `0x0100000c`, `0x0a`, `0x01000017`, `0x01000018`, `0x26`, `0x01000021`, and `0x01000027` mean, and which records open a scope that a terminator closes?

**Known.** `creo_view_pvz.md` §6.5 gives the payload lengths of the first five tags. The payload length of tags `0x26`, `0x01000021`, and `0x01000027` is variable, and their content is not known. §6.1 gives the line groups of a surface tessellation stream.

**Need.** We must know the scope structure to validate a tessellation stream.

### PVZ-16. Edge tessellation fields

**Question.** What do the `0x0100000f` values and the `word` and `value` fields of the edge header mean?

**Known.** `creo_view_pvz.md` §7. The `0x0100000f` values can be the sequence `0, 1, … n−1`, with `n` equal to the exact edge count. `value` can be 1.0.

**Need.** We must know them to validate edge tessellation against the exact edges.

### PVZ-17. Textures and decals

**Question.** Where does an OL or PVS member store texture images, texture bindings, and decals?

**Known.** `creo_view_pvz.md` §6.3 defines the texture coordinate of a vertex. Packages with textured parts contain `.png` members. A tag-6 attribute record (§5.4) names a texture, for example `plain.png` or `undefined`, and stands between a tag-5 record and the tag-7 record it pairs with (§5.3).

**Need.** We must know the binding to decode textured appearance.

## 3. Exact geometry

### PVZ-18. Other surface flag bits

**Question.** What do `flags` bits other than `0x80`, `0x08`, and `0x10`, and `marker` bits other than bit 0, mean in a surface record?

**Known.** `creo_view_pvz.md` §8.3 and §8.9. Bits `0x08` and `0x10` of a B-spline surface record mark the periodic directions. The other `flags` bits, and the `marker` bits other than bit 0, have the value 0.

**Need.** We must know them to read a surface record that sets them.

### PVZ-19. Curve and pcurve flag bits

**Question.** What do bits `0x02` and `0x04` of the edge-curve and pcurve `flags` mean?

**Known.** `creo_view_pvz.md` §8.15 and §8.16. Bit `0x04` occurs on pcurves of all types. Bit `0x02` occurs on composite curves, B-spline curves, and conics without a range pair.

**Need.** We must know whether a bit changes the parameterisation or the sense.

### PVZ-20. Coedge orientation byte

**Question.** What does the `orientation` byte of a coedge record mean?

**Known.** `creo_view_pvz.md` §8.12, §8.17, and §8.18. The byte does not equal the curve sense derived from the endpoints. In a loop of three or more coedges on a face that is not a B-spline face, the byte is 1 when the edge curve runs along the stored coedge order (§8.18) and 0 when it runs against it. On a B-spline face the byte is 0 or 1 in both cases.

**Need.** We must know whether it gives the loop direction or the face side.

### PVZ-21. Other loop markers

**Question.** What do negative loop markers other than −2 mean? Does marker −1 mark a planar two-edge loop with zero area, which bounds no face region?

**Known.** `creo_view_pvz.md` §8.11 defines −2 and values of 0 or more.

**Need.** We must know them to build face regions from every loop.

### PVZ-22. Exact-stream tag `0x0a`

**Question.** What does the 12-byte tag-`0x0a` record in the face section of an exact stream mean?

**Known.** The payload is three `u32` values `0, 1, n`, and `n` increases through the stream. The record stands between a singular-use record and its terminator (`creo_view_pvz.md` §8.2).

**Need.** We must know whether it attaches a face to a tessellation group or to an attribute.

### PVZ-23. Composite curve parameterisation

**Question.** How does a type-2 edge curve or pcurve parameterise the curves it lists, and which coedges use it?

**Known.** `creo_view_pvz.md` §8.15 and §8.16. The listed indexes are less than the table count and refer to records stored before the composite. Coedges of B-spline faces reference composite edge curves and composite pcurves.

**Need.** We must know the parameterisation to evaluate a composite curve.

### PVZ-24. B-spline face endpoint residuals

**Question.** Why do some coedges on B-spline faces have endpoint residuals larger than the agreement band of PVZ-42?

**Known.** The endpoint residuals of native-pcurve coedges on B-spline faces are in the range 0 to 10⁻¹ m. In a face with a residual above 2×10⁻⁵ m, the residuals of all coedges are of similar size. The periodic directions of §8.9, the surface flags, and the range pairs do not separate these faces from the others.

**Need.** We must know whether a transform of the whole face is missing, to evaluate these faces.

### PVZ-25. Cone-face residuals

**Question.** Can a coedge on a cone face have an endpoint residual above the agreement band of PVZ-42, up to 1.4×10⁻⁴ m?

**Known.** With the cone evaluation of `creo_view_pvz.md` §8.6, the endpoint residuals of coedges on cone faces are in the range 0 to 2×10⁻⁵ m.

**Need.** We must know the cause to decide whether the cone parameterisation of §8.6 is complete.

### PVZ-26. Length unit

**Question.** Does a PVS or OL member declare its length unit?

**Known.** `creo_view_pvz.md` §8.21 states metres. Uniform placement scales (§3.10) are in the range 0.03937 to 100; the values 100 and 0.03937 occur. The geometry extent of a member can be more than 100 units.

**Need.** We must know whether a member can store another unit, so that an importer does not misscale it.

### PVZ-27. PVS bounds and tessellation bounds

**Question.** When does the bounding box of a geometry element differ from the vertex bounds of the tessellation stream?

**Known.** `creo_view_pvz.md` §3.8. The difference between the two boxes is in the range 0 to more than 10⁻⁴ m.

**Need.** We must know which geometry the bounds cover before a decoder uses them as a check.

### PVZ-28. Other versions and record types

**Question.** Which stream identities, exact versions, surface types, curve types, pcurve types, and PVS codec values occur outside the sets that `creo_view_pvz.md` defines?

**Known.** §3.2, §4.5, §8.1, §8.3, §8.15, and §8.16 list the defined values. Pcurve type 1 (circle) is not in the defined set.

**Need.** A decoder must reject or preserve an undefined value without loss.

### PVZ-29. Singular use of an extended curve

**Question.** What does a singular-use record mean when its edge curve does not collapse to one point?

**Known.** `creo_view_pvz.md` §8.13. The extent of a referenced curve is in the range 0 to model-space length: a curve can stay within 2×10⁻⁵ m of one point, or be a circular arc of radius 9×10⁻³ m, a line, a B-spline, or a polyline of model-space length. The referenced edge is also used by a coedge of the same face, and that coedge can have a second use on another face.

**Need.** We must know the meaning to decide whether such an edge is degenerate in the neutral model.

### PVZ-40. Singular-use edge index with composite edges

**Question.** Which edge index space does the `edge` field of a singular-use record (`creo_view_pvz.md` §8.13) use in a stream that has composite edge records?

**Known.** `creo_view_pvz.md` §8.2. A coedge edge index is an ordinal among the edge records that no composite lists, and a composite member index is an ordinal among all edge records. Without composite edge records the two spaces are equal. No available stream has both a composite edge record and a singular-use record.

**Need.** We must know the space to find the edge of a singular use in a stream with composite edges.

**Conflict.** `brep/records.rs:369-374` reads every singular-use edge index in the coedge index space unconditionally. The Known cites no stream with both a composite edge record and a singular-use record, so this choice has no test on current evidence.

### PVZ-41. Face records without a loop

**Question.** What does a surface record mean when no loop record and no singular-use record follow it (`creo_view_pvz.md` §8.19)?

**Known.** Such faces are plane records with both range pairs (§8.3). They come after every face with a loop in the face section. A member has up to seven of them. They have no coedge, so they bound no face region and share no edge with another face.

**Need.** We must know whether they are datum planes, construction geometry, or trimmed faces that another record bounds, to decide whether a decoder can transfer them.

### PVZ-42. Geometric agreement band

**Question.** Within what distance do the equalities of `creo_view_pvz.md` §8.13, §8.17, and §8.20 hold: a collapsed singular-use curve and its start point, the pcurve image ends and the edge curve ends of a coedge, and the two curves of a coincident edge pair?

**Known.** §8.21: the member stores no tolerance value. Outside the faces of PVZ-24, the endpoint residuals of coedges are in the range 0 to 2×10⁻⁵ m.

**Need.** A decoder decides coedge sense, loop closure, vertex joins, coincident edge pairs, and singular-use collapse with a distance. We must know whether the format implies that distance or whether it varies per model.

**Conflict.** An agreement band of 2×10⁻⁵ m for every coedge disagrees with the endpoint residuals of up to 10⁻¹ m on B-spline faces (PVZ-24), and with the cone-face residuals of up to 1.4×10⁻⁴ m that PVZ-25 asks about. The decoder applies the band to every face and admits a larger residual on a face with a face tolerance and the loss `pvz/exact.endpoint-mismatch`.

### PVZ-43. Silent acceptance of unknown surface, curve, and pcurve flag bits

**Question.** What does each surface `flags` bit other than `0x80`, `0x08`, and `0x10`, each `marker` bit other than bit 0, and each edge-curve and pcurve `flags` bit other than `0x08` mean, and must a decoder that sees one transfer, reject, or report the record?

**Known.** `brep/records.rs:514-517` masks only `SURFACE_U_RANGE` (`0x80`) and `SURFACE_V_RANGE` (bit 0), with the B-spline periodic bits `0x08` and `0x10` read separately. `read_curve` masks only `CURVE_RANGE` (`0x08`). Every remaining bit passes silently; the decoder records nothing.

**Need.** An unknown bit can change the parameterisation or the sense (PVZ-18, PVZ-19). Silent acceptance risks undetected wrong geometry. We must decide the transfer, reject, or report rule before reading a record that sets such a bit.

**Note.** This item touches PVZ-18 and PVZ-19. Keep those items until the bits are settled.

### PVZ-44. Negative loop markers other than −2 refuse the exact stream

**Question.** What does each negative loop marker other than −2 mean, and must a decoder that sees one transfer the loop or refuse the stream?

**Known.** `brep/records.rs:241-252` rejects any marker that does not name the first loop of its face as malformed, which includes marker −1. `decode.rs:257-264` maps that refusal to the loss `GeometryExactUndecoded`, so the member keeps its tessellation only.

**Need.** We must know whether marker −1 marks, for example, a planar two-edge loop with zero area that bounds no face region, before deciding between a targeted transfer and the current whole-stream refusal.

**Note.** This item touches PVZ-21. Keep that item until the markers are settled.

### PVZ-45. Non-similarity placements kept as general affine transforms

**Question.** Which stored placements violate the §3.10 mutually perpendicular, equal-length, right-handed invariant, and what transform must a decoder apply to them?

**Known.** `creo_view_pvz.md` §3.10 states the invariant as fact: the three basis vectors have one common length and form a right-handed set. `model.rs:311-316` keeps a violating basis as a general affine transform and reports the loss `StructurePlacementNotSimilarity`.

**Need.** We must know whether such bases occur in real files and what semantics they carry before accepting affine transfer. A general affine placement can shear geometry.

**Conflict.** The decoder transfers geometry under a general affine placement while the specification admits only similarity placements.

### PVZ-46. Out-of-range appearance values rejected without a palette join

**Question.** Which appearance values can lie outside their range, and which definitions use the ten-value and six-value palette entries?

**Known.** `model.rs:494-499` drops appearance values outside their range with the loss `AttributeAppearanceColorRejected`. PVZ-07 leaves the semantics of the structure-level palette entries and their definition use open.

**Need.** We must join palette entries to definitions before deciding between rejection and clamping, keeping per-member appearance rejection distinct from structure-level palette overrides.

**Note.** This item touches PVZ-07. Keep that item until the palette semantics are settled.
