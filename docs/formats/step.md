# STEP Part 21 clear-text format

## 1. Envelope

The STEP codec reads ISO 10303-21 clear-text exchange structures whose
`FILE_SCHEMA` identifies AP203, AP214, or AP242. AP242 editions 1 through 3
and AP203 editions 1 and 2 and AP214 use this exchange structure. Part 28 XML,
Part 26 binary, AP242 BO-Model XML sidecars, and ZIP containers are distinct
encodings.

Part 21 AP203, AP214, and AP242 documents describe exchanged product shape and
product structure. Product occurrence relationships carry identity and
placement. The clear-text exchange structure contains no originating
application's ordered feature-replay history or assembly constraint-solver
state unless an application protocol entity explicitly carries that data.

## 2. Byte repertoire and exchange framing

An uncompressed exchange structure has this outer grammar:

```text
exchange = "ISO-10303-21;" header anchor? reference? data+ signature?
           "END-ISO-10303-21;"
header   = "HEADER;" header_entity* "ENDSEC;"
anchor   = "ANCHOR;" anchor_entry* "ENDSEC;"
reference= "REFERENCE;" reference_entry* "ENDSEC;"
data     = "DATA" data_parameters? ";" entity_instance* "ENDSEC;"
signature= "SIGNATURE;" signature_content "ENDSEC;"
```

Keywords and entity names use ASCII letters, digits, underscore, hyphen, and
`!` where the grammar below permits a user-defined keyword. Keywords are
case-insensitive. The canonical spelling is uppercase. Outside encoded string
characters, bytes are interpreted as ISO-8859-1 in editions 1 and 2. Edition 3
also permits UTF-8. A UTF-8 byte sequence must be shortest-form, encode a
Unicode scalar value, and contain no surrogate code point.

Whitespace bytes are space, horizontal tab, carriage return, and line feed.
`/*` begins a comment and the next `*/` ends it. Comments do not nest. A
comment or whitespace may occur between tokens. Neither is recognized inside
a string or binary literal.

Every byte belongs to exactly one of: exchange framing, whitespace/comment,
a parsed token in a typed record, or a parsed token in a named opaque record.
Malformed trailing or inter-token bytes are not opaque records.

## 3. Tokens

```text
instance_name = "#" digit+
standard_name = letter (letter | digit | "_")*
user_name     = "!" standard_name
integer       = sign? digit+
real          = sign? ((digit+ "." digit*) | ("." digit+)) exponent?
exponent      = ("E" | "e" | "D" | "d") sign? digit+
enumeration   = "." standard_name "."
string        = "'" string_item* "'"
binary        = '"' hex_digit* '"'
omitted       = "$"
derived       = "*"
sign          = "+" | "-"
```

`1.`, `0.E+000`, and Fortran `D` exponents are real values. A binary literal
starts with one indicator nibble followed by hexadecimal payload digits. The
indicator states the number of unused trailing bits in the final payload digit
and is in `0..=3`; those low-order bits are zero. Payload digits pack
most-significant nibble first. The decoded bit length is four times the payload
digit count minus the indicator. An empty payload uses indicator zero.
Comma, equals sign, parentheses, and semicolon are individual punctuation
tokens. A lexer never assigns line-based meaning to a token.

## 4. Strings

Two consecutive apostrophes encode one apostrophe. Two consecutive reverse
solidus bytes encode one reverse solidus. Direct bytes `0x20..=0x7e` other than
apostrophe and reverse solidus encode themselves.

`\S\c` adds 128 to the seven-bit code of `c`. `\P A\` through `\P I\` select
the ISO 8859 part used by subsequent `\S\` escapes. The selector contains no
space; the displayed separation only distinguishes the selector letter from
surrounding prose.

`\X\hh` encodes one byte using two hexadecimal digits. `\X2\hhhh...\X0\`
encodes a sequence of four-hex-digit UTF-16 code units. Valid surrogate pairs
combine into one scalar value; isolated surrogates are invalid. `\X4\hhhhhhhh
...\X0\` encodes eight-hex-digit Unicode scalar values. Hexadecimal digits are
case-insensitive. Direct ASCII, `\X2\`, and `\X4\` forms denote the same scalar
values where their permitted repertoires overlap.

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

The partial records in a complex instance are ordered alphabetically by entity
name. Each partial record supplies the explicit attributes introduced by that
leaf in external mapping. `*` marks an inherited attribute supplied by a
sibling leaf. The merged instance retains every leaf name and its parameter
sequence; schema accessors resolve inherited attributes without discarding the
external representation.

Instance names are unique across all DATA sections. References may point
forward or backward and resolve after all DATA sections have parsed. A missing
instance is a structural reference error. Unknown entity names remain named
opaque records with their complete token and byte spans and resolved outgoing
references.

## 6. Header

`FILE_DESCRIPTION`, `FILE_NAME`, and `FILE_SCHEMA` occur in that order.
`FILE_DESCRIPTION` supplies description strings and implementation level.
`FILE_NAME` supplies name, timestamp, authors, organizations, preprocessor
version, originating system, and authorization. `FILE_SCHEMA` supplies one or
more schema identifiers. Schema identifiers select the AP and edition; aliases
that differ only by ASCII case compare equal.

## 7. Edition 3 sections

ANCHOR entries bind an anchor name to an in-file value. An anchor name is
unique and resolves before schema decoding. REFERENCE entries bind a local
resource name to a URI. URI targets outside the exchange structure are
reported as external dependencies and are not fetched implicitly. SIGNATURE
content is structurally bounded by its section terminator and retained with
identity when its signature method is not modeled.

DATA section parameters name the governing schema and section population.
Multiple DATA sections share the same instance-name namespace.

## 8. Entity-layer invariants

All STEP aggregate indices are one-based. Entity references preserve identity;
the reader does not duplicate a referenced carrier to satisfy ownership.
Optional `$` differs from derived `*` and from an empty aggregate. Select and
typed-parameter wrappers remain available to schema accessors.

Length values convert to millimeters. Plane angles remain radians. SI prefixes
apply before conversion-based-unit factors. Conversion-based units resolve as
an acyclic chain ending in a dimensional base unit. Representation uncertainty
is a linear tolerance in the representation's length unit.

A conical surface permits zero reference radius at its placement origin. Its
half-angle is converted from the representation's plane-angle unit to radians.
An unknown NURBS closed or periodic LOGICAL value makes no periodicity
assertion. A POLYLINE with `n` points is the degree-one NURBS having those
points as control points and a clamped piecewise-linear knot vector.

TRIMMED_CURVE trim selects contain parameter values, Cartesian points, or
both. Cartesian selects on lines, circles, and ellipses resolve through the
basis curve's parameterization. Composite-curve segments retain order,
same-sense, transition continuity, and their referenced trimmed or composite
carrier identities.

A `CURVE_BOUNDED_SURFACE` boundary is a `BOUNDARY_CURVE` or degenerate pcurve.
Every `BOUNDARY_CURVE` is a closed composite curve on the bounded surface; its
segments reference bounded surface curves, bounded pcurves, or nested composite
curves on that surface. A plain three-dimensional composite curve does not
satisfy the boundary type or surface-association invariant.

Topology orientation composes at each relation: face bound orientation,
oriented-edge orientation, edge-curve `same_sense`, face `same_sense`, and
oriented-shell orientation. Reversing a relation reverses use, not the shared
underlying entity. A committed body graph contains complete ownership and
valid referenced indices; recoverable non-manifold incidence is retained and
reported without fabricating manifold ownership.

A face boundary is either an EDGE_LOOP coedge ring or a VERTEX_LOOP naming one
vertex at a surface singularity. No zero-length edge is introduced for a
vertex loop. A geometric set containing surfaces forms a sheet carrier;
decoded curve-only or point-only sets remain standalone geometry and do not
create a body.

Product shape binds through `PRODUCT_DEFINITION_SHAPE` and
`SHAPE_DEFINITION_REPRESENTATION`. Occurrence transforms compose once from the
product-definition relationship into model space. Mapped representations and
context-dependent relationships that identify the same placement do not cause
double application.

A shape representation contains at least one representation item. The two
items of an `ITEM_DEFINED_TRANSFORMATION` belong respectively to the two
representations connected by its representation relationship; an occurrence
placement is not an unowned auxiliary item.

Exact and tessellated representations of the same product remain linked.
Tessellated indices are one-based. PNINDEX is the local-point to shared-coordinate
table; triangle, strip, and fan indices address local points. A normal aggregate
of length one applies to every local point; otherwise normals align with the
local point table.

Styles resolve from a styled item through presentation assignments to color,
with overriding styles taking precedence for their occurrence. A style on a
geometric set applies to each member. Empty and NULL style assignments add no
appearance. Styles on native presentation carriers without a neutral geometry
arena retain the carrier identity as their target. Semantic PMI retains its
shape-aspect target; presentation PMI retains annotation identity and
placement. PLUS_MINUS_TOLERANCE carries either numeric lower/upper deviations
or the form variance, zone variance, grade, and source fields of
LIMITS_AND_FITS.
