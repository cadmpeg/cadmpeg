# STEP Part 21 clear-text format

Part 21 is a clear-text exchange grammar. [`docs/layouts/step.md`](../layouts/step.md)
records the binary literal's fixed nibble rule. The source table is
[`docs/layouts/step.toml`](../layouts/step.toml).

## 1. Envelope

A Part 21 exchange structure uses `FILE_SCHEMA` to identify AP203, AP214, or
AP242 and its edition. AP203, AP214, and AP242 documents carry exchanged
product shape and product structure. Product occurrence relationships carry
identity and placement.

Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use
separate encodings.

## 2. Byte repertoire and exchange framing

A clear-text exchange structure uses this outer grammar:

```text
exchange = "ISO-10303-21;" header anchor? reference? data+ signature?
           "END-ISO-10303-21;"
header   = "HEADER;" header_entity* "ENDSEC;"
anchor   = "ANCHOR;" anchor_entry* "ENDSEC;"
reference= "REFERENCE;" reference_entry* "ENDSEC;"
data     = "DATA" data_parameters? ";" entity_instance* "ENDSEC;"
signature= "SIGNATURE;" signature_content "ENDSEC;"
anchor_entry    = resource "=" parameter ";"
reference_entry = reference_name "=" resource ";"
reference_name  = resource | instance_name
```

Outside string escape sequences, editions 1 and 2 interpret character bytes as
ISO-8859-1. Edition 3 also accepts UTF-8. Every UTF-8 sequence uses the
shortest form, encodes one Unicode scalar value, and excludes surrogate code
points.

Whitespace consists of space, horizontal tab, carriage return, and line feed.
The `/*` delimiter starts a comment, and `*/` ends it. Comment delimiters form
non-nesting pairs.
The lexer applies whitespace and comments at token boundaries. String and
binary literals consume their contents as literal data.

Byte accounting assigns each consumed byte to structural syntax, whitespace,
comments, a typed record, or an opaque record. An unclassified byte raises a
parse error.

## 3. Tokens

```text
instance_name = "#" digit+
standard_name = letter (letter | digit | "_" | "-")*
user_name     = "!" standard_name
resource      = "<" resource_character* ">"
integer       = sign? digit+
real          = sign? ((digit+ "." digit*) | ("." digit+)) exponent?
exponent      = ("E" | "e" | "D" | "d") sign? digit+
enumeration   = "." standard_name "."
string        = "'" string_item* "'"
binary        = '"' indicator hex_digit* '"'
indicator     = "0" | "1" | "2" | "3"
omitted       = "$"
derived       = "*"
sign          = "+" | "-"
```

Keywords and entity names use ASCII letters, digits, underscore, and hyphen.
User-defined names begin with `!` where the grammar admits them. Keywords
ignore ASCII case. Canonical spelling uses uppercase.

`1.`, `0.E+000`, and Fortran `D` exponents are real values. A binary literal
starts with one indicator nibble and continues with hexadecimal payload digits.
The indicator gives the number of unused low-order bits in the final payload
digit. Its value is `0..=3`, and each unused bit is zero. Payload digits pack
most-significant nibble first. The decoded bit length is four times the payload
digit count minus the indicator. The empty bit sequence is written `"0"`.

Comma, equals sign, parentheses, and semicolon are individual punctuation
tokens. A resource token contains a UTF-8 byte sequence between `<` and `>`.
The sequence excludes `>`. Line breaks have the same separator role as other
whitespace.

## 4. Strings

Two consecutive apostrophes encode one apostrophe. Two consecutive reverse
solidus bytes encode one reverse solidus. Direct bytes in `0x20..=0x7e`, with
apostrophe and reverse solidus handled by the preceding rules, encode
themselves.

The escape `\S\c` adds 128 to the seven-bit code of `c`. Selectors `\PA\`
through `\PI\` choose the ISO 8859 part used by later `\S\` escapes. A
selector contains its letter directly between the two reverse solidus bytes.

The escape `\X\hh` encodes one byte with two hexadecimal digits. The form
`\X2\hhhh...\X0\` encodes four-hex-digit UTF-16 code units. A valid surrogate
pair combines into one scalar value. An isolated surrogate is invalid. The
form `\X4\hhhhhhhh...\X0\` encodes eight-hex-digit Unicode scalar values.
Hexadecimal digits ignore case. Direct ASCII, `\X2\`, and `\X4\` forms denote
the same scalar values where their repertoires overlap.

## 5. Values and records

A parameter is an instance reference, integer, real, enumeration, string,
binary literal, omitted value, derived value, list, or typed parameter. A list
is a parenthesized comma-separated sequence. A typed parameter is a name
followed by one parenthesized parameter. Empty lists are valid.

A simple entity instance is:

```text
#id = ENTITY_NAME(parameter, ...);
```

A complex entity instance is:

```text
#id = (LEAF_A(...) LEAF_B(...) ...);
```

Complex-instance partial records appear in ascending entity-name order. Each
partial record supplies the attributes introduced by its leaf in external
mapping. `*` marks an inherited attribute supplied by a sibling leaf. The
merged instance retains every leaf name and parameter sequence. Schema
accessors resolve inherited attributes against that representation.

Instance names share one namespace across all DATA sections. Forward and
backward references resolve after all DATA sections are read. A reference to an
absent local instance is a structural reference error. An instance name
declared by a REFERENCE entry is an external reference and is not required in
the local DATA graph. An unknown standard or user-defined entity name produces
a named opaque record that retains its complete token span, byte span, and
links to other named opaque records.

## 6. Header

The header contains `FILE_DESCRIPTION`, `FILE_NAME`, and `FILE_SCHEMA` in that
order. `FILE_DESCRIPTION` supplies description strings and implementation
level. `FILE_NAME` supplies name, timestamp, authors, organizations,
preprocessor version, originating system, and authorization. `FILE_SCHEMA`
supplies one or more schema identifiers. Schema identifiers select the
application protocol and edition. ASCII case differences compare equal.

## 7. Edition 3 sections

ANCHOR entries bind a resource name to an in-file parameter value. Anchor names
are unique. Resource values that name anchors resolve recursively before schema
decoding. A cycle is a structural error.

REFERENCE entries bind a local resource name to a resource URI. They also bind
an external instance name to a resource URI. Resource names and URIs are
delimited by `<` and `>`; an external instance name uses the `#id` form. A URI
target outside the exchange structure is an external dependency. External
instance names do not enter the local DATA instance graph.
SIGNATURE content begins after `SIGNATURE;`, ends at the next `ENDSEC;`, and
retains its complete byte range.

DATA section parameters identify the governing schema and section population.
All DATA sections share the instance-name namespace.

## 8. Entity-layer invariants

All STEP aggregate indices are one-based. Entity references preserve identity,
and CADIR keeps one carrier for each referenced entity. `$` denotes an omitted
optional value. `*` denotes a derived attribute. An empty aggregate uses an
empty list. Select and typed-parameter wrappers remain available to schema
accessors.

Length values convert to millimetres. Plane-angle values remain radians. SI
prefixes apply before conversion-based-unit factors. Conversion-based units
form an acyclic chain that ends in a dimensional base unit. Representation
uncertainty is a linear tolerance measured in the representation's length
unit.

A conical surface accepts zero reference radius at its placement origin. Its
finite half-angle converts from the representation's plane-angle unit to
radians. A NURBS `closed` or `periodic` field uses the STEP LOGICAL domain.
TRUE and FALSE encode the property state. UNKNOWN is a valid LOGICAL value.
A `POLYLINE` with `n` points is the degree-one NURBS with those points as
control points and a clamped piecewise-linear knot vector.

`TRIMMED_CURVE` stores trim selects as parameter values, Cartesian points, or
both. Cartesian selects on lines, circles, and ellipses resolve through the
basis curve's parameterization. Composite-curve segments retain order,
same-sense, transition continuity, and carrier identity.

Bounded-surface boundaries use `BOUNDARY_CURVE` or a degenerate pcurve. A
`BOUNDARY_CURVE` is a closed composite curve on its bounded surface. Its
segments resolve to bounded surface curves, bounded pcurves, or nested
composite curves on that surface. A plain three-dimensional composite curve
has a general curve role.

Orientation composes at each topology relation through face-bound orientation,
oriented-edge orientation, edge-curve `same_sense`, face `same_sense`, and
oriented-shell orientation. Reversing a relation reverses the occurrence
direction while the shared entity keeps its identity. A shell-based wireframe
creates an occurrence-specific edge whose endpoints follow the composed curve
direction. The edge occurrence carries the wireframe use. A committed body
graph has complete ownership and valid referenced indices. Recoverable
non-manifold incidence remains attached and is reported.

Connected face sets join through common edges or common vertices. Edge-based
and shell-based wireframe models preserve their connected edge and vertex
ownership. Each independent connected edge set or wire shell receives
owner-scoped neutral edge and vertex identities. Faceted B-reps materialize
polygon-loop straight edges and vertices as topology carriers. Oriented faces,
subfaces, seam edges, subedges, connected-edge subsets, and connected-face sets
resolve inherited attributes before topology is committed. A connected-edge or
connected-face subset resolves its own member list. Its parent reference must
resolve to the matching parent set type for the subset record to be typed;
parent lineage remains in the source records.

Sheet and wire representations commit each independently resolvable shell or
connected set. A failed member produces a decode loss. Solid roots, including
every shell in `BREP_WITH_VOIDS`, commit atomically. A mandatory member failure
rejects the solid root. One STEP face shared by several shell occurrences maps
to one owner-scoped CADIR face per occurrence. Boundary edges and vertices
remain shared when their owner scope is unambiguous.

A face boundary uses an `EDGE_LOOP` coedge ring, a `POLY_LOOP` point ring, or a
`VERTEX_LOOP` vertex at a surface singularity. A vertex loop emits a
vertex-only boundary. A base `FACE` with a valid non-collinear `POLY_LOOP`
derives an implicit plane from the loop points. A base `EDGE` emits a curve-less
CADIR edge when both endpoint vertices have point carriers. A base `VERTEX` whose
point carrier is absent makes its containing member mandatory and
unrepresentable. Sheet and wire members containing that vertex are omitted;
the solid-root transaction rejects it. A geometric set with surface members
forms a sheet carrier. Curve-only and point-only sets remain standalone
geometry.

A face has at most one `FACE_OUTER_BOUND`. Other face bounds are not outer
bounds. When malformed input declares more than one outer bound, the decoder
retains every loop, keeps the first outer role in source order, marks the
remaining conflicting roles unspecified, and reports a topology loss.

`AXIS2_PLACEMENT_2D` defines the origin and positive-u axis of a parameter-space
conic. Its positive-v axis is the counterclockwise perpendicular. A `PCURVE`
definitional representation transfers one exact 2D line, circle, ellipse,
parabola, hyperbola, polyline, NURBS, trimmed curve, or offset curve. An
ambiguous or unsupported 2D representation stays opaque and remains detached
from the coedge.

A topology-referenced curve or surface whose geometry fails transfer retains
its STEP identity as an unknown carrier linked to its opaque record. The body
topology keeps the relation. An optional pcurve that fails transfer leaves the
coedge usable and produces a loss. An unowned pcurve and its unshared 2D
dependency closure stay named opaque records. A shared dependency remains
typed when another retained carrier owns it. Failure of a mandatory topology
relation rejects the complete solid root. Records owned only by that root stay
opaque, and product bindings omit the body. Sheet and wire members use the
independent-member salvage rule. A `SURFACE_CURVE` with a star or non-reference
basis keeps its edge occurrence without a CADIR curve and produces a loss; the
decoder does not fabricate a curve carrier. A pcurve on a plane scales both
parameter axes and all length-valued 2D geometry by the document length unit.
A surface-curve carrier and its pcurve represent the same point set but may
use different parameterizations; its edge vertices determine the occurrence
interval when no explicit trim is present. The carrier's own NURBS domain is
not an edge trim.

The writer records each omitted shell, face, loop, and edge relation as a
topology-transfer loss. Omitted outer shells, void shells, and outer bounds
are errors. Other omitted topology relations are warnings. The strict
unsupported policy rejects output when any topology-transfer loss exists.

Product shape binds through `PRODUCT_DEFINITION_SHAPE` and
`SHAPE_DEFINITION_REPRESENTATION`. Every body-producing representation,
including `GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION`, uses the same
source-root-to-body map. Occurrence transforms compose once from the
product-definition relationship into model space. Mapped representations and
context-dependent relationships that identify one placement apply that
placement once. Reused source topology roots reuse their committed body
identity.

A shape representation contains at least one representation item. The two
items of an `ITEM_DEFINED_TRANSFORMATION` belong to the two representations
connected by its representation relationship. An occurrence placement belongs
to its defining relationship and representation context.

Exact and tessellated representations of one product remain linked when their
source item has one exact body owner. A missing or ambiguous owner detaches the
tessellation, retains its source item association, and records a
`ReferenceGraphNotClosed` loss. Tessellated indices are one-based. PNINDEX maps
local points to shared coordinates. Triangle, strip, and fan indices address
local points. A normal aggregate of length one applies to every local point;
other normal aggregates align with the local point table.

Styles resolve from a styled item through presentation assignments to color.
An overriding style takes precedence for its occurrence. A style on a
geometric set applies to each member. Empty and NULL style assignments leave
appearance unchanged. A native presentation carrier without a neutral geometry
arena retains its carrier identity as the style target. Semantic PMI retains its
shape-aspect target. Presentation PMI retains annotation identity and
placement. `PLUS_MINUS_TOLERANCE` carries numeric lower and upper deviations,
or the form variance, zone variance, grade, and source fields of
`LIMITS_AND_FITS`.
