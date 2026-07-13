# IGES 5.3 Fixed ASCII format specification

> **License:** This document is released under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). Attribute to the cadmpeg project.

## Physical representation

The Fixed ASCII representation is an ordered sequence of physical lines. A canonical line contains an 80-byte card followed by a line ending. Card bytes 1 through 72 are section data. Byte 73 is the section marker. Bytes 74 through 80 are the right-aligned decimal sequence field. Parameter Data cards instead use bytes 65 through 72 for the right-aligned Directory Entry back-pointer, byte 73 for the `P` marker, and bytes 74 through 80 for the Parameter Data sequence.

Byte positions are one-based in this specification. Source spans use zero-based half-open byte ranges. Parsing records the card payload and line ending as disjoint source spans before interpreting fields. A short card, bytes beyond column 80, a noncanonical line ending, and bytes after the Terminate card remain separate physical records with their original spans.

The canonical section order is Start (`S`), Global (`G`), Directory Entry (`D`), Parameter Data (`P`), and Terminate (`T`). Section sequences are positive decimal integers starting at one and increasing by one within each section. A Directory Entry occupies two consecutive `D` cards. Its first card has an odd sequence and its second card has the following even sequence. Directory pointers refer to the odd Directory Entry sequence. Zero is a null pointer where the owning field permits null.

The Terminate data area contains four eight-byte fields: `S` plus the seven-digit Start count, `G` plus the seven-digit Global count, `D` plus the seven-digit Directory Entry card count, and `P` plus the seven-digit Parameter Data card count. The remaining data area is blank.

## Global section

The Global data stream is the concatenation of bytes 1 through 72 from its cards. Its first value defines the parameter delimiter and its second value defines the record delimiter. Each is a one-character Hollerith string. Omitted first and second values select comma and semicolon respectively.

A Hollerith value is an unsigned decimal byte count, the byte `H` or `h`, and exactly that many following bytes. Delimiters inside the counted payload are data. The count and payload may cross card boundaries. Integer values are signed decimal integers. Real values accept a decimal point and an exponent introduced by `E`, `e`, `D`, or `d`. An empty field between parameter delimiters is omitted. The record delimiter terminates the Global record.

Global fields declare the sender and receiver identifiers, native file name, generator, significant digits, single and double precision limits, model scale, units flag and unit name, maximum line-weight gradation and width, creation and modification timestamps, minimum resolution, maximum coordinate, author, organization, specification version, drafting standard, and application protocol. Length-valued fields are converted from the declared units and model scale only when projected to neutral IR. Native values remain unchanged.

## Directory Entry section

Each Directory Entry contains twenty fixed eight-byte fields across two cards. Blank numeric fields have their field-defined default. Nonblank numeric fields are right-aligned signed decimal integers.

The first card fields are entity type, Parameter Data start sequence, structure, line font pattern, level, view, transformation matrix, label-display associativity, and the eight-digit status number. The second card fields are the repeated entity type, line weight, color, Parameter Data card count, form number, two reserved fields, entity label, and entity subscript. The repeated entity type must equal the first-card value. Reserved bytes are retained whether blank or nonblank.

The status number consists of four two-digit decimal fields: blank status, subordinate-entity switch, entity-use flag, and hierarchy. A negative structure, line-font, level, view, transformation, label-display, or color value denotes a Directory Entry pointer where that field permits an entity reference. Pointer parity and target type are validated after all entries are indexed.

## Parameter Data section

Bytes 1 through 64 of Parameter Data cards form parameter fragments. Bytes 65 through 72 identify the owning odd Directory Entry sequence. Fragments are grouped by that back-pointer and ordered by Parameter Data sequence. The Directory Entry Parameter Data start sequence and card count define the expected contiguous range. A record delimiter terminates the entity's primary parameters.

Tokens retain their exact source spans and lexical bytes. Token classes are integer, real, Hollerith string, Directory Entry pointer, omitted value, parameter delimiter, record delimiter, and retained uninterpreted value. Entity accessors impose field-specific token types and arity. Pointer interpretation is field-specific; a numeric token is not globally coerced to a pointer.

After the primary record delimiter, an entity may carry an ordered associativity pointer group and an ordered property pointer group. Each group begins with a count followed by that many pointers. The complete trailing groups remain part of the owning entity and retain token spans.

## Entity graph

The entity identity is its odd Directory Entry sequence. Graph construction indexes all identities before resolving references. Each edge records the source entity, source field or parameter index, raw pointer, expected target class, and resolution state. Resolution states distinguish resolved, null, dangling, even-sequence, wrong-type, and cyclic references. Cycles are findings unless the owning relationship explicitly permits them.

An entity retains its type, form, Directory Entry fields, status fields, ordered parameter tokens, trailing association and property groups, source spans, and reference edges. Unsupported type/form pairs remain named native records and prevent any support claim whose closed envelope admits the pair.

## Units and transformations

Model-space lengths equal native values multiplied by the Global model scale and converted from the declared unit to millimetres. Dimensionless values, parameter coordinates, weights, and unit direction vectors are not length-scaled. Angles convert to radians when projected to neutral IR. A transformation matrix is a 3-by-3 linear part plus translation. Translation is length-valued. Entity transforms compose from the entity definition toward model space exactly once. Definition, subfigure-instance, and occurrence transforms remain separate native relationships.

## Topology

Manifold solid B-rep entities preserve source vertex, edge, loop, face, shell, and solid identity. Edge uses reference shared edge identity; loop orientation and face same-sense fields determine coedge and face orientation. Void shells remain distinct from the exterior shell.

Boundary, curve-on-surface, bounded-surface, and trimmed-surface entities carry face-local boundaries. They produce sheet regions whose loops, coedges, edges, and vertices are owned by the source face. No cross-face edge sharing is inferred without a shared source topology entity. A topology candidate is attached only after the complete neutral ownership and reference graph validates.

## Byte accounting

Every source byte belongs to one nonempty half-open ledger span. Typed spans cover values with decoded semantics. Structural spans cover framing, delimiters, padding, and sequence fields. Opaque spans name the native record that retains their bytes or their length and digest. Canonical ledger order is ascending start offset. Adjacent spans may be coalesced only when class, owner, and meaning are identical. Coverage starts at zero, ends at source length, and has neither gaps nor overlaps.
