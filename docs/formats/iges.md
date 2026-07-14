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

Model-space lengths equal native values divided by the Global model-space scale and converted from the declared unit to millimetres. Dimensionless values, parameter coordinates, weights, and unit direction vectors are not length-scaled. Angles convert to radians when projected to neutral IR. A transformation matrix is a 3-by-3 linear part plus translation. Translation is length-valued. Entity transforms compose from the entity definition toward model space exactly once. Definition, subfigure-instance, and occurrence transforms remain separate native relationships.

## Primitive solids

Primitive solid entities use Form 0. Their native dimensional values remain in declared model units, their origin defaults to `(0,0,0)`, and their axis vectors are dimensionless. An omitted X axis defaults to `(1,0,0)` and an omitted Z or revolution axis defaults to `(0,0,1)`. Every supplied or defaulted axis is unit length. Entities carrying both X and Z axes require them to be orthogonal; local Y is `Z × X`. The Directory transformation remains a separate placement link.

Type 150 stores three positive block lengths, origin, X axis, and Z axis. Type 152 adds a nonnegative top X length strictly smaller than its positive base X length; its Y and Z lengths are positive. Type 154 stores positive cylinder height and radius, its first-face center, and axis. Type 156 stores positive frustum height, a positive large radius, and a smaller radius in `[0, large_radius)`; the small radius defaults to zero. Type 158 stores a positive sphere radius and center. Type 160 stores torus major and minor radii with `major > minor > 0`, center, and axis. Type 168 stores ellipsoid radii with `X ≥ Y ≥ Z > 0`, center, X axis, and Z axis.

The `native.iges` `primitive_solids` arena retains the typed primitive kind, named native dimensions, omitted-versus-present origin and axis components, source entity, and transformation link. Invalid dimensions, axes, units, or transformation chains prevent semantic decoding while the generic native entity remains intact.

Type 162 defines a solid of revolution from a profile-curve pointer, a revolution fraction in `(0,1]` defaulting to `1`, an axis origin defaulting to `(0,0,0)`, and a unit axis defaulting to `(0,0,1)`. Form 0 requires an open profile whose endpoints close to the axis by projection. Form 1 carries a profile whose area is closed by the profile itself or by joining its endpoints. Type 164 defines a linear extrusion from a closed profile, a positive length, and a unit direction defaulting to `(0,0,1)`. Both retain their Directory transformation as the resulting solid's placement.

Type 180 Forms 0 and 1 store a postorder regularized Boolean expression. The declared term count is greater than two. Negative terms are negated odd Directory pointers to solid operands; positive terms `1`, `2`, and `3` mean union, intersection, and difference. Each operation consumes two stack values and produces one. The stack never underflows and contains exactly one value after the final term. Operand links are acyclic. Form 0 excludes Manifold Solid B-rep operands; Form 1 contains at least one such operand. The typed `boolean_trees` arena retains ordered operands and operations without converting the expression to an unordered relationship graph.

Type 182 Form 0 selects one connected component of a disjoint Boolean result. It has entity-use flag `03`, a pointer to a semantically decoded Type 180 tree, and a finite model-space point in or on the selected component. The `selected_components` arena retains the tree identity, native selection coordinates, source identity, and optional Directory transformation.

The `native.iges` `procedural_solids` arena retains sweep kind, form, profile identity, native sweep amount, omitted-versus-present axis fields, and transformation link. Semantic decoding requires a decoded profile carrier and closure consistent with the owning form.

## Product structure

Type 184 Forms 0 and 1 define an ordered solid assembly and have entity-use flag `02`. A positive member count is followed by that many solid-item pointers and a parallel list of the same number of Transformation Matrix pointers. A zero member transformation means identity. Each nonzero member transformation is applied to that member before the assembly's Directory transformation is applied to the complete collection. Assembly references are acyclic. Form 0 members are primitives, solid instances, Boolean trees, or other assemblies. Form 1 contains at least one Manifold Solid B-rep member and otherwise admits the same member classes. The `solid_assemblies` arena preserves definition identity, member order, member-to-transformation pairing, form, and collection placement.

Type 308 Form 0 defines a reusable subfigure and has entity-use flag `02`. It stores a nonnegative nesting depth, nonempty Hollerith name, nonnegative member count, and that many ordered entity pointers. A depth-zero definition contains no Type 408 members. Every contained Type 408 instance references a definition whose depth is strictly less than the containing definition's depth. Type 308 carries no independent transformation.

Type 408 Form 0 instantiates one Type 308 definition. It stores the definition pointer, a model-space translation defaulting componentwise to zero, and a positive scale defaulting to one. Its Directory transformation supplies rotation or other permitted affine placement and is applied in addition to the instance translation and scale. The `subfigure_definitions` and `subfigure_instances` arenas preserve definition identity separately from occurrence identity, ordered members, native placement components, and nesting links.

Type 320 Form 0 defines a reusable network subfigure and has entity-use flag `02`. It stores a nonnegative nesting depth, nonempty name, ordered child entities, a type flag identifying unspecified, logical, or physical content, a primary reference designator, an optional Type 312 display-template pointer, and an ordered list of nullable Type 132 connect-point pointers. Its nesting depth includes both Type 308 and Type 320 definitions, and every contained Type 408 or Type 420 instance targets a definition of strictly smaller depth. Type 320 carries no independent transformation.

Type 420 Form 0 instantiates one Type 320 definition. It stores translation coordinates defaulting to zero; positive definition-space x, y, and z scale factors, where x defaults to one and omitted y or z defaults to x; a type flag; a primary reference designator; an optional Type 312 display-template pointer; and an ordered nullable connect-point list. The instance and definition connect-point counts are equal. Definition-space scaling precedes the instance translation and the Directory transformation. The `network_definitions` and `network_instances` arenas retain native values, ordered identities, nullable connection positions, and placement links.

## Topology

Manifold solid B-rep entities preserve source vertex, edge, loop, face, shell, and solid identity. Edge uses reference shared edge identity; loop orientation and face same-sense fields determine coedge and face orientation. Void shells remain distinct from the exterior shell.

Every use of one Edge List item in a shell belongs to one cyclic radial ring. A closed Form 1 shell requires exactly two opposite-sense uses per edge. An open Form 2 shell preserves one, two, or more uses without imposing the closed-manifold cardinality rule; rings with more than two uses represent explicit non-manifold sharing.

A Form 1 Edge List Entity (Type 504) stores a positive ordered list of model-space curve pointers and one-based start and end indices into Form 1 Vertex List entities. Evaluating the curve at its stored parameter interval endpoints must agree with the referenced vertex points within the Global minimum resolution. An inconsistent edge prevents attachment of every topology candidate that consumes it.

A Form 1 Loop Entity (Type 508) stores a positive ordered use count. Each use selects an Edge List or Vertex List item, stores an orientation logical, and stores an arbitrary ordered sequence of `(ISOP, CURV)` parameter-curve pairs. Edge uses become coedges. Vertex uses become pole uses positioned after the preceding edge use in cyclic loop order, or the sole unanchored use of a vertex-only loop. Every parameter curve remains ordered and retains its isoparametric logical. The parameter-curve collection forms one connected parameter-space image. Composing its first endpoint, adjacent joins, and final endpoint with the face surface must agree with the oriented edge vertices within the Global minimum resolution. A pole-use collection has the same endpoint contract with the pole vertex. Disagreement prevents attachment of the containing topology candidate.

A Form 1 Face Entity (Type 510) stores a support-surface pointer, a positive loop count, an outer-loop logical, and the ordered loop pointers. When the logical is true, the first loop is `outer` and every following loop is `inner`. When it is false, every loop is `inner` and the support surface's parameter domain supplies the exterior boundary. A face has at most one explicit outer loop.

Boundary, curve-on-surface, bounded-surface, and trimmed-surface entities carry face-local boundaries. They produce sheet regions whose loops, coedges, edges, and vertices are owned by the source face. No cross-face edge sharing is inferred without a shared source topology entity. A topology candidate is attached only after the complete neutral ownership and reference graph validates.

A Form 0 Boundary Entity (Type 141) stores `TYPE`, `PREF`, the support-surface pointer, and a positive model-curve count. Each ordered model-curve item stores its curve pointer, sense (`1` forward or `2` reversed), pcurve count, and that many parameter-curve pointers. `TYPE=0` requires every pcurve count to be zero. `TYPE=1` requires every pcurve count to be positive. The parameter curves of one item remain ordered and together form that coedge's parameter-space image.

A Form 0 Curve on a Parametric Surface Entity (Type 142) stores its creation method, support-surface pointer, parameter-space curve pointer, model-space curve pointer, and preferred representation. The parameter-space curve has entity-use flag `05`. Its composition with the support surface has the same oriented model-space endpoints as the model-space curve. Projection requires both endpoint distances to be no greater than the Global minimum resolution; disagreement prevents attachment of the containing topology candidate.

A Form 0 Trimmed Surface Entity (Type 144) stores a support-surface pointer, an outer-boundary flag, an inner-boundary count, an outer Curve on a Parametric Surface pointer or zero, and the ordered inner Curve on a Parametric Surface pointers. When the outer-boundary flag is zero, the outer pointer is zero and the rectangular parameter domain of the support surface supplies the outer boundary. The entity then produces no explicit outer loop; each listed loop is `inner`. When the flag is one, the outer pointer produces the single `outer` loop and each listed loop is `inner`.

## Appearance

Directory color zero supplies no direct color. Positive values `1` through `8` select black, red, green, blue, yellow, magenta, cyan, and white. A negative value is the negated odd Directory sequence of a Form 0 Color Definition Entity (Type 314). Type 314 stores red, green, and blue intensities as finite percentages from `0` through `100` and an optional Hollerith name. Neutral RGBA components equal the percentages divided by `100`, with alpha `1`.

Directory line-font values `1` through `5` select standard patterns. A negative value is the negated odd Directory sequence of a Line Font Definition Entity (Type 304). Form 1 stores an orientation flag, a Form 0 Subfigure Definition pointer, a positive display spacing, and a positive template scale. Form 2 stores a positive segment count, that many positive segment lengths, and exactly `ceil(count / 4)` hexadecimal digits. Pattern bits are right-justified; the unit bit describes the last segment, and unused high bits are zero. Type 304 has entity-use flag `02` and carries a standard fallback line-font value `1` through `5` in its Directory entry.

A nonnegative Directory level value selects that single exchange-file level. A negative value is the negated odd Directory sequence of a Definition Levels Property Entity (Type 406, Form 1). The property stores a positive count followed by that many distinct nonnegative level numbers. A malformed definition or link is retained but does not become a decoded display-level set.

A positive Directory line-weight number selects that numbered increment in the uniform Global line-weight series and cannot exceed the Global maximum gradation count. Its model-space width is `number * maximum_width / maximum_gradations`; neutral/native display width converts that result to millimetres. Zero leaves the display width unspecified.

A Manifold Solid B-rep Object or orphan Shell color binds to its body. A Face color binds to that face and overrides the body color for that face. Trimmed and bounded surface colors bind to their generated sheet body and face. The blank-status field determines body visibility. Curve and surface source-object associations retain their direct Directory colors.

The `native.iges` namespace version is `2`. Its `colors` arena stores typed Type 314 percentages, names, and fallback color numbers. Its `line_fonts` arena stores typed template and visible-blank definitions. Its `definition_levels` arena stores ordered multiple-level sets. Its `display_attributes` arena stores one record per Directory entity with visibility, line-font number or definition link, level number or definition link, view, line weight, and color number or definition link. These values remain distinct from effective neutral appearance bindings.

## Byte accounting

Every source byte belongs to one nonempty half-open ledger span. Typed spans cover values with decoded semantics. Structural spans cover framing, delimiters, padding, and sequence fields. Opaque spans name the native record that retains their bytes or their length and digest. Canonical ledger order is ascending start offset. Adjacent spans may be coalesced only when class, owner, and meaning are identical. Coverage starts at zero, ends at source length, and has neither gaps nor overlaps.
