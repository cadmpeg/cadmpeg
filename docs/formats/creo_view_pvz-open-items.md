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

**Question.** What do the bytes after the name atom of a typed definition (types `0x01`, `0x02`, `0x06`, `0x1e`) mean?

**Known.** `creo_view_pvz.md` §3.7 gives the byte counts: one byte for types `0x01` and `0x02`, two bytes for type `0x06`, four bytes for type `0x1e`. Values include `07 07 03 03`, `07 07 04 04`, `06 06 05 05` (type `0x1e`) and `07 07`, `0d 0d` (type `0x06`).

**Need.** We must know whether the bytes carry a model kind, a version, or a units choice.

**Note.** The openpvz-rs notes also describe a type `0x1f` with an optional `0x21` byte before four bytes from the set `07 07 03 03`, `07 07 04 04`, `0d 0d 08 08`. No such definition is in the available files.

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

**Known.** `creo_view_pvz.md` §6.2. In members with an exact stream, `face_ref` is less than the face count. For most patches, the vertices lie on the surface of face `face_ref` within 10⁻⁵ m. `refs` is often the single value `face_ref`.

**Need.** We must know the link between tessellated patches and exact faces to give exact faces their appearance.

**Conflict.** In six members, some patches lie off the surface of face `face_ref` by 10⁻⁴ m to 1 m. The specification therefore does not state the link as an invariant.

### PVZ-15. Other tessellation records

**Question.** What do the tessellation records with tags `0x0100000b`, `0x0100000c`, `0x0a`, `0x01000017`, `0x01000018`, `0x26`, `0x01000021`, and `0x01000027` mean, and which records open a scope that a terminator closes?

**Known.** `creo_view_pvz.md` §6.5 gives the payload lengths. §6.1 gives the line groups of a surface tessellation stream.

**Need.** We must know the scope structure to validate a tessellation stream.

### PVZ-16. Edge tessellation fields

**Question.** What do the `0x0100000f` values and the `word` and `value` fields of the edge header mean?

**Known.** `creo_view_pvz.md` §7. The `0x0100000f` values are often `0, 1, … n−1` with `n` equal to the exact edge count. `value` is often 1.0.

**Need.** We must know them to validate edge tessellation against the exact edges.

### PVZ-17. Textures and decals

**Question.** Where does an OL or PVS member store texture images, texture bindings, and decals?

**Known.** `creo_view_pvz.md` §6.3 defines the texture coordinate of a vertex. Packages with textured parts contain `.png` members. A tag-6 attribute record (§5.4) names a texture, for example `plain.png` or `undefined`, and stands between a tag-5 record and the tag-7 record it pairs with (§5.3).

**Need.** We must know the binding to decode textured appearance.

## 3. Exact geometry

### PVZ-18. Other surface flag bits

**Question.** What do `flags` bits other than `0x80`, and `marker` bits other than bit 0, mean in a surface record?

**Known.** `creo_view_pvz.md` §8.3. B-spline surfaces with `flags` `0x08`, `0x10`, and `0x90` occur; their range pairs follow the bit-7 and bit-0 rule.

**Need.** We must know whether these bits mark periodic or closed directions.

### PVZ-19. Curve and pcurve flag bits

**Question.** What do bits `0x02` and `0x04` of the edge-curve and pcurve `flags` mean?

**Known.** `creo_view_pvz.md` §8.15 and §8.16. Bit `0x04` occurs on pcurves of all types. Bit `0x02` occurs on composite curves, B-spline curves, and conics without a range pair.

**Need.** We must know whether a bit changes the parameterisation or the sense.

### PVZ-20. Coedge orientation byte

**Question.** What does the `orientation` byte of a coedge record mean?

**Known.** `creo_view_pvz.md` §8.12 and §8.17. The byte does not equal the curve sense derived from the endpoints.

**Need.** We must know whether it gives the loop direction or the face side.

### PVZ-21. Other loop markers

**Question.** What do negative loop markers other than −2 mean?

**Known.** `creo_view_pvz.md` §8.11 defines −2 and values of 0 or more.

**Need.** We must know them to build face regions from every loop.

**Note.** The openpvz-rs notes report one loop with marker −1: a planar two-edge loop with zero area, which bounds no face region. No such loop is in the available files.

### PVZ-22. Exact-stream tag `0x0a`

**Question.** What does the 12-byte tag-`0x0a` record in the face section of an exact stream mean?

**Known.** The payload is three `u32` values `0, 1, n`, and `n` increases through the stream.

**Need.** We must know whether it attaches a face to a tessellation group or to an attribute.

### PVZ-23. Composite curve parameterisation

**Question.** How does a type-2 edge curve or pcurve parameterise the curves it lists, and which coedges use it?

**Known.** `creo_view_pvz.md` §8.15 and §8.16. The listed indexes are less than the table count and refer to records stored before the composite.

**Need.** We must know the parameterisation to evaluate a composite curve.

### PVZ-24. B-spline face endpoint residuals

**Question.** Why do some coedges on B-spline faces have endpoint residuals larger than the tolerance of `creo_view_pvz.md` §8.21?

**Known.** About 4 % of native-pcurve coedges on B-spline faces have residuals from 10⁻³ m to 10⁻¹ m. In each such face the residuals of all coedges are of similar size, which indicates an offset or a transform of the whole face rather than a local error. The surface flags and range pairs do not separate these faces from the others.

**Need.** We must know the missing transform to evaluate these faces.

**Conflict.** `creo_view_pvz.md` §8.17 and §8.21 state the endpoint agreement as an invariant. The openpvz-rs notes report that all coedges of two sample sets pass at 2×10⁻⁵ m.

### PVZ-25. Cone-face residuals

**Question.** Why do some coedges on cone faces fail the endpoint agreement of `creo_view_pvz.md` §8.17?

**Known.** The openpvz-rs notes report coedges with residuals up to about 1.4×10⁻⁴ m, mostly on cone faces, in a few parts. In the available files, cone coedges agree within 2×10⁻⁵ m.

**Need.** We must know the cause to decide whether the cone parameterisation of §8.6 is complete.

### PVZ-26. Length unit

**Question.** Does a PVS or OL member declare its length unit?

**Known.** `creo_view_pvz.md` §8.21 states metres. Two placements carry a uniform scale (§3.10) of 100 and 0.03937. One member has geometry extents of more than 100 units.

**Need.** We must know whether a member can store another unit, so that an importer does not misscale it.

### PVZ-27. PVS bounds and tessellation bounds

**Question.** When does the bounding box of a geometry element differ from the vertex bounds of the tessellation stream?

**Known.** `creo_view_pvz.md` §3.8. For most definitions the two boxes agree within 10⁻⁶ m. For a few they differ by more than 10⁻⁴ m.

**Need.** We must know which geometry the bounds cover before a decoder uses them as a check.

### PVZ-28. Other versions and record types

**Question.** Which stream identities, exact versions, surface types, curve types, pcurve types, and PVS codec values occur outside the sets that `creo_view_pvz.md` defines?

**Known.** §3.2, §4.5, §8.1, §8.3, §8.15, and §8.16 list the defined values. Pcurve type 1 (circle) is not in the defined set.

**Need.** A decoder must reject or preserve an undefined value without loss.
