# STEP Part 21 clear-text format

Part 21 is a clear-text exchange grammar. [`docs/layouts/step.md`](../layouts/step.md)
records the binary literal's fixed nibble rule. The source table is
[`docs/layouts/step.toml`](../layouts/step.toml). STEP `inspect` runs the
semantic decode path; the concluded disposition is
[step-inspect.md](step-inspect.md).

## 1. Envelope

A Part 21 exchange structure uses `FILE_SCHEMA` to identify AP203, AP214, or
AP242 and its edition. AP203, AP214, and AP242 documents carry exchanged
product shape and product structure. Product occurrence relationships carry
identity and placement.

Part 28 XML, Part 26 binary, AP242 BO-Model XML, and ZIP containers use
separate encodings.
A Part 21 ZIP container uses PKZIP 2.04g stored or Deflate entries. Its root
member is named exactly `ISO-10303.p21` and is at the archive root. Every
other member is a subsidiary, including directories, nested archives, and
ancillary data. Archive member paths are relative, use `/`, and cannot contain
`.` or `..` components that escape the archive. A container without that root
member, with an unsupported compression method, with a duplicate member name,
or with an encrypted entry is invalid. The root member is the Part 21 exchange
structure.

## 2. Byte repertoire and exchange framing

A clear-text exchange structure uses this outer grammar:

```text
exchange = "ISO-10303-21;" header anchor? reference? data*
           "END-ISO-10303-21;" signature*
header   = "HEADER;" header_entity* "ENDSEC;"
anchor   = "ANCHOR;" anchor_entry* "ENDSEC;"
reference= "REFERENCE;" reference_entry* "ENDSEC;"
data     = "DATA" data_parameters? ";" entity_instance* "ENDSEC;"
data_parameters = "(" string "," "(" string ")" ")"
signature= "SIGNATURE;" base64 "ENDSEC;"
anchor_entry    = anchor_name "=" anchor_item anchor_tag* ";"
anchor_item     = omitted | integer | real | enumeration | string | binary
                  | rhs_occurrence_name | resource | anchor_item_list
anchor_item_list = "(" anchor_item* ")"
rhs_occurrence_name = entity_instance_name | value_instance_name
                      | constant_entity_name | constant_value_name
anchor_tag      = "{" tag_name ":" anchor_item "}"
reference_entry = (entity_instance_name | value_instance_name) "=" resource ";"
anchor_name     = "<" uri_fragment_identifier ">"
```

Outside string escape sequences, implementation levels with a major value
below `4` interpret character bytes as ISO-8859-1. Edition 3 uses
implementation levels `4;1`, `4;2`, and `4;3` and interprets direct character
bytes as UTF-8. Class 1 (`4;1`) forbids ANCHOR, REFERENCE, SCHEMA_POPULATION,
and SIGNATURE sections. Class 2 (`4;2`) permits those sections but forbids
value instances and EXPRESS constants. Class 3 (`4;3`) permits all edition-3
occurrence forms. Historical levels `1`, `2`, `2;1`, and `2;2` require one
unparameterized DATA section and no FILE_POPULATION, SECTION_LANGUAGE, or
SECTION_CONTEXT header entity. Levels `3;1` and `3;2` require at least one
DATA section and forbid ANCHOR, REFERENCE, SCHEMA_POPULATION, and SIGNATURE
sections, value instances, EXPRESS constants, and resource values. Every
UTF-8 sequence uses the shortest form, encodes one Unicode scalar value, and
excludes surrogate code points.

Space, the explicit `\N\` and `\F\` print-control directives, and comments
separate tokens. The `/*` delimiter starts a comment, and `*/` ends it.
Comment delimiters form non-nesting pairs. ASCII control octets are ignored
when processing the exchange structure, including when they occur inside a
token. A leading byte-order mark is not Part 21 whitespace and is invalid. The print-control directives are ignored in effective string and
binary contents and are forbidden in resources, ANCHOR sections, and
REFERENCE sections. String and binary literals retain the other source bytes
needed for escape decoding.

Byte accounting assigns each consumed byte to structural syntax, whitespace,
comments, a typed record, or an opaque record. An unclassified byte raises a
parse error.

## 3. Tokens

```text
entity_instance_name = "#" digit+
value_instance_name  = "@" digit+
constant_entity_name = "#" upper (upper | digit)*
constant_value_name  = "@" upper (upper | digit)*
standard_name = letter (letter | digit | "_" | "-")*
user_name     = "!" standard_name
resource      = "<" resource_character* ">"
integer       = sign? digit+
real          = sign? ((digit+ "." digit* exponent?)
                       | ("." digit+ exponent?)
                       | (digit+ exponent ".")
                       | (digit+ exponent))
exponent      = ("E" | "e" | "D" | "d") sign? digit+
enumeration   = "." standard_name "."
string        = "'" string_item* "'"
binary        = '"' indicator hex_digit* '"'
indicator     = "0" | "1" | "2" | "3"
omitted       = "$"
derived       = "*"
sign          = "+" | "-"
tag_name      = (letter | "_") (letter | digit | "_")*
```

Keywords and entity names use ASCII letters, digits, underscore, and hyphen.
User-defined names begin with `!` where the grammar admits them. Keywords
ignore ASCII case. Canonical spelling uses uppercase. Anchor tag names preserve
source case and use letters, digits, and underscore; a tag name cannot begin
with a digit.

Numeric `#` and `@` occurrences require at least one nonzero digit. Leading
zeroes are accepted and removed from the stored integer. Entity and value
occurrence integers share one namespace: an integer used by one prefix cannot
be used by the other prefix in the same exchange. Named occurrences begin with
an ASCII letter or underscore, use only ASCII letters, digits, and underscore,
and are canonicalized to uppercase. A numeric `#` occurrence is a DATA entity
reference. A numeric `@` occurrence is a value reference declared by a
`REFERENCE` entry. Named occurrences are EXPRESS entity or value constants. An
anchor name is a nonempty URI fragment identifier with at least one non-digit
character. A reference left-hand side is a numeric entity or value occurrence
name.

`1.`, `0.E+000`, exponent-form values without a decimal point, exponent-form
values with a trailing decimal point such as `6E-16.`, and Fortran `D`
exponents are real values. A binary literal starts with one indicator nibble
and continues with hexadecimal payload digits.
The indicator gives the number of unused low-order bits in the final payload
digit. Its value is `0..=3`, and each unused bit is zero. Payload digits pack
most-significant nibble first. The decoded bit length is four times the payload
digit count minus the indicator. The empty bit sequence is written `"0"`.

Comma, equals sign, parentheses, braces, colon, and semicolon are individual
punctuation tokens. A resource token contains a UTF-8 byte sequence between
`<` and `>`. The sequence excludes `>` and print-control directives.

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
The print-control directives `\N\` and `\F\` do not contribute to effective
string contents. A string occupies at most 32,769 source octets, including
its opening and closing apostrophes.

## 5. Values and records

A parameter is an entity reference, value reference, named entity constant,
named value constant, integer, real, enumeration, string, binary literal,
resource, omitted value, derived value, list, or typed parameter. A list is a
parenthesized comma-separated sequence. A typed parameter is a name followed
by one parenthesized parameter. A user-defined type name does not assign
value semantics; the wrapped parameter remains a typed opaque value. Empty
lists are valid. Numeric value references
and named constants are values, not local DATA entity identifiers.

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

Entity instance names share one namespace across all DATA sections. Forward
and backward references resolve after all DATA sections are read. A reference
to an absent local instance is a structural reference error. An entity or
value occurrence declared by a REFERENCE entry is external and is not required
in the local DATA graph. A value occurrence cannot resolve to a DATA entity
instance. An unknown standard or user-defined entity name produces a named
opaque record that retains its complete token span, byte span, and links to
other named opaque records.

## 6. Header

The header contains `FILE_DESCRIPTION`, `FILE_NAME`, and `FILE_SCHEMA` in that
order. `FILE_DESCRIPTION` supplies description strings and implementation
level. The major value in `implementation_level` selects the direct string
repertoire: `4` selects UTF-8 and earlier levels select ISO-8859-1.
`FILE_NAME` supplies name, timestamp, authors, organizations,
preprocessor version, originating system, and authorization. `FILE_SCHEMA`
supplies one or more unique string identifiers. The first identifier governs
the application protocol and edition; later identifiers do not override it.
`FILE_DESCRIPTION` strings and every `FILE_NAME` string attribute have an
effective length of at most 256 characters. A non-empty `FILE_NAME` timestamp
uses the complete extended calendar-date and time-of-day form
`YYYY-MM-DDTHH:MM:SS`, with an optional fractional second and an optional `Z`
or signed `HH:MM` time-zone offset. Each `FILE_SCHEMA` identifier has an
effective length of at most 1024 characters. Its schema name is a non-empty
ASCII identifier containing uppercase letters, digits, and underscores after
case normalization. Its optional object identifier is a non-empty sequence of
signed decimal components enclosed in braces. A component may have an optional
leading sign and contains at least one decimal digit. Leading and trailing
whitespace around the identifier is ignored.
Each parameterized DATA section names one schema from this list. The schema
name compares with the identifier's schema-name portion when the identifier
has an object identifier.
An identifier is a schema name with an optional brace-delimited object
identifier containing space-separated signed decimal components. The supported
identifiers are:

| Identifier                                                                                                   | Protocol and edition |
| ------------------------------------------------------------------------------------------------------------ | -------------------- |
| `CONFIG_CONTROL_DESIGN`                                                                                      | AP203 edition 1      |
| `AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF { 1 0 10303 403 2 1 2 }` | AP203 edition 2      |
| `AUTOMOTIVE_DESIGN` or `AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }`                                         | AP214                |
| `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }`                                    | AP242 edition 1      |
| `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }`                                    | AP242 edition 2      |
| `AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }`                                    | AP242 edition 3      |

An AP242 identifier with another object identifier has an unspecified edition.
ASCII case differences compare equal.

After `FILE_SCHEMA`, the header may contain at most one `SCHEMA_POPULATION`,
zero or more `FILE_POPULATION` entities, and `SECTION_LANGUAGE` and
`SECTION_CONTEXT` entities with unique section selectors. A
`SCHEMA_POPULATION` contains one or more triples of address string, optional
timestamp string, and optional non-empty Base64 digest string. A timestamp in
this triple uses the same complete timestamp form as `FILE_NAME`. A
`FILE_POPULATION`
contains a governing schema name, a determination-method string, and either
`$` or a nonempty set of DATA section names. `SECTION_LANGUAGE` contains an
optional DATA section name and a three-letter language code. `SECTION_CONTEXT`
contains an optional DATA section name and a nonempty list of context strings.
Built-in header entities precede user-defined `!` header entities. Named
section selectors identify DATA sections. Implementation level `2;1` forbids
FILE_POPULATION, SECTION_LANGUAGE, and SECTION_CONTEXT; level `3;1` forbids
SCHEMA_POPULATION.

## 7. Edition 3 sections

ANCHOR entries bind a resource name to an in-file parameter value and may carry
ordered `{tag:value}` metadata tags. Anchor and tag values retain their source
values after resource references resolve. Anchor names are unique. Resource
values that name anchors resolve recursively before schema decoding and before
omitted inherited `name` attributes are repaired. A cycle is a structural
error. Resource references in tag values use the same recursive resolution
rules as anchor values.

REFERENCE entries bind an external entity or value occurrence name to a resource URI. Resource names and URIs are delimited by `<` and `>`; external names use `#id` or `@id`.
Entity and value occurrence integers are unique across both prefixes, and
neither may collide with a local DATA entity instance. A URI without a
fragment resolves to `$`. A fragment-only URI whose fragment is not a UUID
resolves to the same-named local ANCHOR; a missing local anchor resolves to
`$`. A fragment-only UUID requires a resource locator or registry. A URI with
a resource path is resolved against that resource; its fragment must identify
an ANCHOR that supplies an entity for a `#id` occurrence or a value for an
`@id` occurrence. If an ANCHOR forwards another URI, resolution repeats, and a
failed or cyclic resolution produces `$`. A resource path or UUID that cannot
be obtained remains an external dependency until the caller supplies resource
access. External occurrence names do not create local DATA entity identities.
Each SIGNATURE section follows the exchange terminator. Its content is a
detached CMS `SignedData` object as defined by RFC 5652, encoded as RFC 4648
Base64. Digest and signature algorithm identifiers are inside that object, not
in a Part 21 field. The Base64 content begins after `SIGNATURE;` and ends at
its next `ENDSEC;`. The signature
authenticates the Part 21 alphabet bytes from `ISO-10303-21;` through the byte
before that section's `SIGNATURE;` token. A later section therefore also
authenticates every earlier signature section. The reader retains both the
complete source span and the decoded CMS payload. Signature verification still
requires a CMS verifier and caller-supplied trust policy.

DATA sections are optional in edition 3. One unnamed DATA section requires one
FILE_SCHEMA identifier. If a DATA section has parameters, they contain a
decoded unique section name and one governing schema name listed in
FILE_SCHEMA. Multiple DATA sections require parameters on every section. All
DATA sections share the entity-instance namespace.

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
unit. Each representation's `GLOBAL_UNIT_ASSIGNED_CONTEXT` supplies the
length and plane-angle scales for that representation and its reachable
representation-item closure. A carrier shared by representations must have
one equal scale in every such context; conflicting contexts leave the carrier
on the document fallback scale and produce a geometry loss. The document
fallback scale is the unique scale shared by all applicable global unit
contexts. If no context supplies a dimension, all records for that dimension
must resolve to one scale. Equivalent contexts define that shared scale.
Conflicting contexts or unresolved dimension units do not define a fallback;
unscoped values remain in source numeric units and produce a document-unit
unresolved error. `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` selects an uncertainty
measure whose unit resolves to a length unit. When several length measures are
present, the one named `distance_accuracy_value` is selected; otherwise a
unique length measure is required. An ambiguous set does not define a linear
tolerance. Geometric-consistency checks use the selected document tolerance as
their baseline. Entity and solved-carrier tolerances can widen that baseline.

A conical surface accepts zero reference radius at its placement origin. Its
finite half-angle converts from the representation's plane-angle unit to
radians. A NURBS `closed` or `periodic` field uses the STEP LOGICAL domain.
TRUE and FALSE encode the property state. UNKNOWN is a valid LOGICAL value.
A `POLYLINE` with `n` points is the degree-one NURBS with those points as
control points and a clamped piecewise-linear knot vector. `UNIFORM_CURVE`,
`QUASI_UNIFORM_CURVE`, `BEZIER_CURVE`, and the corresponding surface entities
use default knot vectors with `control_count + degree + 1` entries in each
parameter direction. Uniform values are consecutive integers starting at
`-degree`, with multiplicity one. Quasi-uniform values start at zero, repeat
each endpoint `degree + 1` times, and repeat each interior value once. Bezier
values have endpoint multiplicity `degree + 1`, interior multiplicity `degree`,
and a control count that forms an integral number of degree-sized spans.
Complex `B_SPLINE_CURVE` and `B_SPLINE_SURFACE` records use the same defaults
when one of these subtype leaves is present. Rational weight aggregates have
the same shape as their control-point aggregates.

`CARTESIAN_TRANSFORMATION_OPERATOR_3D` stores a required local origin and
optional axis1, axis2, axis3, and scale attributes. Its transformation matrix
columns are normalized and orthogonal: axis3 defaults to +Z, axis1 is projected
onto the plane normal to axis3, and axis2 determines the sense of the projected
second axis. The default axis1 is +X, except for an axis3 within `1e-12` of
parallel to X, where it is +Y; the default axis2 is +Y. The 2D operator derives a perpendicular
second axis from axis1 and uses axis2 only to select its sense. Omitted scale is 1.

`AXIS2_PLACEMENT_3D` uses the same first-projected-axis rule when its optional
reference direction is omitted or parallel to its axis: +X is projected onto
the plane normal to the axis, except for an axis within `1e-12` of parallel to
X, where +Y is projected.

The IR stores the longer `ELLIPSE` semi-axis as `major_direction` and
`major_radius`. If the first STEP semi-axis is shorter than the second, the
canonical parameter is `v = u - π/2`; numeric `TRIMMED_CURVE` selectors are
mapped by that phase after their angular unit conversion. Cartesian selectors
invert the canonical carrier and therefore need no phase adjustment. The
phase is inherited by curve replicas, trims, and spatial offsets.

`TRIMMED_CURVE` stores trim selects as parameter values, Cartesian points, or
both. Cartesian selects on lines, circles, and ellipses resolve through the
basis curve's parameterization. Its local parameter domain is the directed
trim interval measured from the first select. On a cyclic basis, a forward
trim increases the second select by one period when it is below the first;
a reversed trim increases the first select by one period when it is below the
second. The stored sense maps local parameters in the increasing or decreasing
parent direction. A
`CURVE_REPLICA` retains the complete parent relation, including a trim, and
inherits the parent's parameter range and parameterization; its transformation
changes model-space location and dimensions only. Deferred curve dependencies
resolve by graph fixpoint, including forward and nested replicas. Composite-
curve segments retain order, same-sense, transition continuity, and carrier
identity. A curve construction that references a `SURFACE_CURVE` uses its
3D `curve_3d` carrier for geometry and parameterization.

The endpoint vertices of an `EDGE_CURVE` trim its curve carrier. A
non-periodic carrier has an increasing parameter interval from the start
vertex witness to the end vertex witness. Its domain endpoints select the
first and last branches when the same model-space point occurs at more than
one parameter. A periodic carrier normalizes the start parameter into its
fundamental domain and stores the positive directed sweep to the end witness.
The sweep is not greater than one period. An edge has no parameter interval
when either endpoint cannot be inverted on the carrier or the witnesses do not
define such an interval.

Bounded-surface boundaries use `BOUNDARY_CURVE` or a degenerate pcurve. A
`BOUNDARY_CURVE` is a closed composite curve on its bounded surface. Its
segments resolve to bounded surface curves, bounded pcurves, or nested
composite curves on that surface. A plain three-dimensional composite curve
has a general curve role.

`RECTANGULAR_TRIMMED_SURFACE` retains its basis surface, both parameter
endpoint pairs, and both parameter-direction senses as a surface subset. Its
local U and V domains are `0..abs(u2-u1)` and `0..abs(v2-v1)`. A local parameter
maps to `u1 + s` or `u1 - s`, and to `v1 + t` or `v1 - t`, according to the
stored senses. On a cyclic basis axis, apply the same directed-branch period
adjustment to the second endpoint before computing the absolute span. A
`SURFACE_REPLICA` retains the complete parent relation,
including a rectangular or curve-bounded surface; its transformation changes
model-space location and dimensions while preserving the parent parameter
domain. Deferred surface dependencies resolve by graph fixpoint, including
forward and nested replicas. Its native entity is emitted again when those
values are available.

`SURFACE_OF_LINEAR_EXTRUSION` uses the directrix parameter as U and the stored
vector as the V-direction displacement. `SURFACE_OF_REVOLUTION` uses the axis
placement origin and direction as its rotation axis, the directrix parameter as
V, and the plane angle in radians as U. A pcurve on either surface uses that
same U/V parameterization. The pcurve population does not redefine the chart:
trimmed pcurves cannot establish a surface-wide scale or direction, and a
non-linear directrix keeps its native parameterization. Endpoint-derived
calibration of a bounded procedural pcurve is accepted only when every source
coordinate that the affine map collapses is constant across the pcurve's
declared parameter interval. Otherwise the pcurve remains opaque and the
decoder does not replace its native parameterization.

The neutral chart conversion for a swept surface preserves the parameterization
of each defining equation. A linear-extrusion U coordinate uses the directrix
scale. For a `LINE` directrix, that scale is the directrix `VECTOR` magnitude
times its length-unit conversion. `CIRCLE` and `ELLIPSE` directrices use the
plane-angle conversion. `PARABOLA`, `HYPERBOLA`, `POLYLINE`, and B-spline
directrices use dimensionless parameters. A curve replica, trim, or offset
inherits its defining curve scale. A linear-extrusion V coordinate is
dimensionless because the extrusion vector is already stored in document
length units; its scale is one. A revolution U coordinate uses plane-angle
conversion and its V coordinate uses the directrix scale. A composite
directrix whose parameterization has no single affine scale does not admit
typed pcurve conversion and remains opaque.

A pcurve has no separate angular-unit override. The reader does not choose a
degree or radian interpretation from endpoint fit; an angular coordinate that
fails the owning surface chart remains an unusable pcurve carrier.

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

Each distinct topology root is an ownership boundary. When one distinct root
exists, source edge and vertex identities remain shared within that root. When
multiple distinct roots exist, every root scopes its shell, edge, and vertex
identities by root instance; aliases with the same root key reuse the committed
body. A root with multiple shell owners also scopes carriers by shell. This
prevents independent roots from claiming one global CADIR identity and does
not depend on source record order.

Sheet and wire representations commit each independently resolvable shell or
connected set. A failed member produces a decode loss. Solid roots, including
every shell in `BREP_WITH_VOIDS`, commit atomically. A mandatory member failure
rejects the solid root. The outer shell of `BREP_WITH_VOIDS` must decode to one
connected IR shell; a split outer shell rejects the root because the IR stores
the outer role by position. One STEP face shared by several shell occurrences
maps to one owner-scoped CADIR face per occurrence. Boundary edges and
vertices remain shared when their owner scope is unambiguous.

A face boundary uses an `EDGE_LOOP` coedge ring, a `POLY_LOOP` point ring, or a
`VERTEX_LOOP` vertex at a surface singularity. A vertex loop emits a
vertex-only boundary. A base `FACE` without an explicit surface derives an
implicit plane from the first `FACE_OUTER_BOUND` in source order, or from the
first valid boundary when no outer bound is declared. The selected loop keeps
its ordered points and applies `FACE_BOUND.orientation`; its signed polygon
area defines the plane normal. The plane origin is the arithmetic centroid of
the selected ring. Its u-axis is the projection of the first global coordinate
axis whose projection has the greatest length; ties keep x, then y, then z.
Every point must be within `max(0.01, 1e-12 * ring_scale)` of that plane, where
`ring_scale` is the largest displacement from the centroid. A ring whose
signed area is at most `1e-12 * ring_scale^2`, or whose point residual exceeds
that bound, does not produce a plane. In a complex face-bound instance, the
partial with the boundary parameters supplies the inherited `FACE_BOUND`
attributes; an empty `FACE_OUTER_BOUND` partial supplies only the outer role.
An `ORIENTED_FACE` keeps the base plane carrier orientation and composes its
reversal through face sense and boundary traversal. A base `EDGE` emits a
curve-less CADIR edge when both endpoint vertices have point carriers. A base
`VERTEX` whose point carrier is absent makes its containing member mandatory and
unrepresentable. Sheet and wire members containing that vertex are omitted;
the solid-root transaction rejects it. CADIR has no tolerant-point or
partial-solid carrier and does not infer coordinates. A geometric set with
surface members forms a sheet carrier. Curve-only and point-only sets remain
standalone geometry.

A face has at most one `FACE_OUTER_BOUND`. Other face bounds are not outer
bounds. When malformed input declares more than one outer bound, the decoder
omits the containing topology shell without assigning an outer role or
deriving an implicit face carrier. It retains the source records as opaque and
reports the malformed face and rejected topology root.

`AXIS2_PLACEMENT_2D` defines the origin and positive-u axis of a parameter-space
conic. Its positive-v axis is the counterclockwise perpendicular. A `PCURVE`
definitional representation transfers one exact 2D line, circle, ellipse,
parabola, hyperbola, polyline, NURBS, trimmed curve, offset curve, or curve
replica. A 2D `CURVE_REPLICA` retains its parent pcurve and parameterization;
its 2D affine operator maps the parent coordinates to the replica coordinates.
The definitional representation supplies exactly one 2D item. Active-record
cycles and graphs at depth 256 or greater remain opaque; the recursion guard
releases its active record on every return path. An unrecognized composite 2D
carrier remains opaque rather than becoming an approximate pcurve.
An unsupported 2D representation stays opaque and remains detached from the
coedge. A `SEAM_EDGE` uses its explicit pcurve reference only when that
reference belongs to the edge's `SEAM_CURVE` associated geometry and the face
surface. The reader does not replace an invalid reference with a guessed
branch. For a non-seam source curve with multiple pcurve candidates on its
owning surface, the decoder maps each candidate through that surface and
selects it when one candidate has a unique endpoint-continuous fit for the
coedge. If several candidates tie, the decoder compares their mapped loci
over the endpoint interval with adaptive subdivision. Candidates with
equivalent model-space loci are one semantic carrier and the candidate with
the lowest STEP identity is retained. A declared pcurve trim is the endpoint
witness when it maps to both edge vertices; a stale declared trim can be
replaced by an independently inverted edge interval. Distinct tied or
otherwise unresolved candidates remain detached and produce a topology loss.

The source pcurve carrier is immutable. A chart variant derived from one
coedge's endpoint fit is a use-scoped pcurve carrier. The coedge owns that
variant through its `PcurveUse`; selecting a variant for another coedge does not
change the source carrier or the first coedge's parameter range.

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
decoder does not fabricate a curve carrier. A plane pcurve uses the document
length scale for both parameter axes and all length-valued 2D geometry. A
cylinder or cone uses plane-angle scale for `u` and length scale for `v`. A
sphere or torus uses plane-angle scale for both axes. NURBS surface parameter
axes are dimensionless. A pcurve carrier that cannot preserve its native
parameterization under an anisotropic surface-unit map remains opaque.
A surface-curve carrier and its pcurve represent the same point set but may
use different parameterizations; its edge vertices determine the occurrence
interval when no explicit trim is present. The carrier's own NURBS domain is
not an edge trim.

The writer records each omitted shell, face, loop, and edge relation as a
topology-transfer loss. Omitted outer shells, void shells, and outer bounds
are errors. Other omitted topology relations are warnings. The strict
unsupported policy rejects output when any topology-transfer loss exists.

Each CADIR product definition represents one `PRODUCT_DEFINITION` view.
A product with one definition uses identity `step:product:product#<product>`.
When one `PRODUCT` has multiple definitions, each view receives a distinct
identity suffixed by its definition instance. Shape bodies and definition
descriptions bind to their own view and are not merged. Each definition that
is not a usage child receives one root occurrence. Every usage occurrence
references the specific child definition view.
Product shape binds through `PRODUCT_DEFINITION_SHAPE` and
`SHAPE_DEFINITION_REPRESENTATION`. Every body-producing representation,
including `ADVANCED_BREP_REPRESENTATION` and
`GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION`, uses the same
source-root-to-body map. An `ADVANCED_BREP_REPRESENTATION` is typed when its
items resolve directly or through a mapped representation to committed
topology roots. Occurrence
transforms compose once from the
product-definition relationship into model space. Mapped representations and
context-dependent relationships that identify one placement apply that
placement once. A mapped occurrence uses a
`PRODUCT_DEFINITION_SHAPE` whose definition is the
`NEXT_ASSEMBLY_USAGE_OCCURRENCE`; its
`SHAPE_DEFINITION_REPRESENTATION` contains the mapped item that identifies the
child representation and its target placement. A mapped item target may be an
`AXIS2_PLACEMENT_3D` or a `CARTESIAN_TRANSFORMATION_OPERATOR_3D`; the mapped
transform is the target transform composed with the inverse mapping-origin
transform. Reused source topology roots of the same root type and shell
orientation reuse their committed body identity. Distinct topology roots
retain their governing root type, even when they share shell carriers.
An inferred occurrence placement uses only a mapped item directly listed by a
representation of the parent definition. A mapped item listed by an unrelated
representation does not place the occurrence and produces an assembly-placement
loss.
Several standalone mapped items may share one body only when they resolve to
one transform. Conflicting standalone placements leave the body unplaced and
report an assembly-placement loss; occurrence-owned mappings remain
occurrence transforms.
Repeated child uses without an
occurrence-specific shape representation remain ambiguous and report the
unresolved placement. A mapping whose origin and target are both 2D placement
or 2D transformation records is presentation geometry and does not change a
body placement.

Product definitions and product-definition formations use the inherited base
attribute prefix. A direct subtype carries that prefix in its own parameter
list. A multiple-inheritance complex instance uses the parameters of its
`PRODUCT_DEFINITION` or `PRODUCT_DEFINITION_FORMATION` partial. Product
records use the parameters of their `PRODUCT` partial. A presentation layer
item that references a `PRODUCT` expands to every CADIR product-definition
view derived from that product, in source-definition order.

A shape representation contains at least one representation item. In a
complex instance, its name, item list, and context use the populated
`REPRESENTATION` partial; an empty inherited subtype partial does not replace
those attributes. The two items of an `ITEM_DEFINED_TRANSFORMATION` belong to
the two representations connected by its representation relationship. An
occurrence placement belongs to its defining relationship and representation
context. A
`SHAPE_REPRESENTATION_RELATIONSHIP` connects its two shape-representation
endpoints for body reachability and representation identity. A contextual
occurrence endpoint identifies a child or parent definition representation
when it is that representation or is connected to it by one or more
parameterized shape-representation relationships. These identity edges are
undirected. An empty inherited subtype partial has no endpoints and creates no
edge. In a complex instance, endpoint attributes come from the inherited
`REPRESENTATION_RELATIONSHIP` partial when the subtype partial has no
parameters. For an occurrence relationship, the transform maps the child
representation to the parent representation. If the relationship lists those
endpoints in reverse order, the item direction is inverted. If neither order,
or both orders, identifies the child and parent representations, the
occurrence placement is unresolved.

Exact and tessellated representations of one product remain linked when their
source item has one exact body owner. A tessellated solid, shell, or shape
representation may list a supported triangulated item directly or through a
tessellated geometric set. A product-linked shape representation supplies a
declaration. An exact body link or representation relationship supplies the
body owner to every supported leaf in the item graph. An isolated shape
representation is also admitted when its supported item identities are listed
by a product-linked shape representation. A shape representation without a
product link, shared product-linked items, or an exact representation link
remains a detached source association. A missing or ambiguous owner detaches
the tessellation, retains its source item association, and records a
`ReferenceGraphNotClosed` loss.
`TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS` uses the inherited
representation name, item set, and context for the same ownership rules. Its
accuracy-specific record remains source-native while the supported tessellated
items transfer.
`SHAPE_REPRESENTATION_WITH_PARAMETERS` uses the inherited representation name,
item set, and context. Its item set contains descriptive representation items,
directions, measure representation items, and placements. The reader applies
its context to reachable item units and uses its item set for inherited
representation membership, including validation properties. Unsupported item
semantics remain source-native.
Tessellated indices are one-based. PNINDEX maps local points to shared
coordinates. Triangle and fan indices address local points in listed order. A
triangle strip alternates the first two indices for each odd triangle so
adjacent triangles keep one surface orientation. A normal aggregate of length
one applies to every local point; other normal aggregates align with the local
point table.
`TESSELLATED_CURVE_SET` uses its `COORDINATES_LIST` and one-based `line_strips`
indices to transfer each strip as a separate polyline carrier. The reader does
not join strips or invent source parameters or a chordal bound.

`TESSELLATED_ANNOTATION_OCCURRENCE` carries a tessellated geometric set;
supported triangulated descendants transfer as detached tessellations. A
`REPOSITIONED_TESSELLATED_ITEM` applies its axis placement, including nested
repositioning, to a detached leaf. Unsupported annotation wrappers and
unsupported descendants remain native records. If one detached leaf is
reached through multiple distinct placement transforms, no transform is
selected, source coordinates remain, and `tessellation.placement-ambiguous`
is recorded.

Styles resolve from a styled item through presentation assignments to color.
For `SURFACE_STYLE_USAGE`, `.BOTH.` takes precedence over `.POSITIVE.`, and
`.POSITIVE.` takes precedence over `.NEGATIVE.` when one neutral color must be
selected from a style set. An overriding style takes precedence for its
occurrence. A style on a geometric set applies to each member. Empty and NULL
style assignments leave appearance unchanged. Independent effective styles on
one face or body retain every appearance binding. The neutral scalar color is
set only when those styles produce one distinct color; conflicting colors
leave it unset and produce a metadata loss. A direct `STYLED_ITEM` or
`OVER_RIDING_STYLED_ITEM` still owns its curve, point, or surface target when
the assignment has no resolvable colour. A `PRESENTATION_STYLE_BY_CONTEXT`
branch transfers when its context representation or representation context
directly contains the styled item. A branch without that exact membership
remains named opaque source data and produces a
`presentation.context-dependent-style-unresolved` loss. An unscoped sibling
style may transfer independently. An `ANNOTATION_PLANE` owns each
referenced surface carrier. A native presentation carrier without a neutral
geometry arena retains its carrier identity as the style target. Semantic PMI
retains every supported STEP `SHAPE_ASPECT` subtype as a shape-aspect target,
including a simple leaf subtype and a shape-aspect partial in a complex datum
feature. A complex datum reads identification from its `DATUM` partial and
name, targets, and product shape from its inherited `SHAPE_ASPECT` partial.
A complex dimension uses its dimensional partial for its kind and all inherited
partials for its name, targets, and characteristic value.
Its characteristic representation collects every measure representation item.
A unique item named `nominal value` supplies the nominal. When that name is
absent, exactly one measure item supplies the nominal; multiple unnamed items
are ambiguous and do not supply one. Complex measure records referenced by a
characteristic representation remain typed measure carriers.
`GEOMETRIC_ITEM_SPECIFIC_USAGE` resolves a shape-aspect definition, including a
definition reached through a `SHAPE_ASPECT_RELATIONSHIP`, to its identified
topology item. A resolved face, edge, vertex, or body is added as a typed PMI
target while the source shape-aspect target remains. An unresolved usage remains
source-native with its identity and links.
`DATUM_TARGET` and `PLACED_DATUM_TARGET_FEATURE` transfer as typed datum-target
definitions with their target form and identification, while their source
shape-aspect identity remains a PMI target. Standard placed-target forms are
point, line, rectangle, circle, and circular curve; another source description
is retained as an `Other` form. `FEATURE_FOR_DATUM_TARGET_RELATIONSHIP` remains
source-native because the neutral PMI model has no relationship field.
Geometric validation properties read area, volume, and centroid values through
inherited `REPRESENTATION`, `MEASURE_REPRESENTATION_ITEM`, and
`MEASURE_WITH_UNIT` partials. Direct `AREA_UNIT` and `VOLUME_UNIT` subtypes and
their inherited `DERIVED_UNIT_ELEMENT` factors are typed. Every referenced
area, volume, or centroid item is evaluated; derived-unit factors scale area
and volume by their dimensions.
Geometric tolerances select their kind from the exact geometric-tolerance
leaf partial, not from an inherited or modifier partial. They read their name
and magnitude from the `GEOMETRIC_TOLERANCE` partial when the tolerance is
complex. The
`GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE` partial supplies the datum-system
link and does not add a shape-aspect target. The defined-unit and
defined-area-unit partials retain their unit sizes and area shape; modifier
aggregates retain their enumeration values. Presentation PMI
retains annotation identity, text, and placement across inherited annotation
partials. A direct text carrier or a graph with exactly one reachable text
carrier supplies the presentation text. A graph with multiple reachable text
carriers has no ordered composition in this model, so the text remains absent,
a metadata loss is emitted, and the carriers remain named opaque records with
their source links. Unmodeled tessellated annotation carriers remain named
opaque records with their source links. `PLUS_MINUS_TOLERANCE` carries
numeric lower and upper
deviations, or the form variance, zone variance, grade, and source fields of
`LIMITS_AND_FITS`.

An `APLL_POINT` or `APLL_POINT_WITH_SURFACE` referenced by an annotation
placeholder, annotation-to-annotation, annotation-to-model, or auxiliary
leader line transfers its three-dimensional coordinates to a neutral point
with the APLL source identity. The APLL and leader-line records remain named
opaque records because the neutral model has no fields for `symbol_applied`,
`associated_surface`, or ordered leader-line semantics.

Drawing structure is a linked object graph. `DRAWING_DEFINITION` identifies the
drawing, `DRAWING_REVISION` identifies one revision of it, and
`DRAWING_SHEET_REVISION` identifies a sheet revision with its ordered drawing
items, presentation context, and revision. `DRAWING_SHEET_REVISION_USAGE`
links a sheet revision to its drawing revision and carries the sheet sequence.
`PRESENTATION_VIEW` carries a named view, its ordered items, and presentation
context. `PRESENTATION_SIZE` links a sheet revision to its presentation size.
`DRAUGHTING_MODEL` carries a presentation model with its items and context;
in a complex instance, these attributes come from its inherited
`REPRESENTATION` partial.
`DRAUGHTING_MODEL_ITEM_ASSOCIATION` links model items to their semantic
definition. `DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER` carries the
same definition, draughting model, and callout links plus its annotation
placeholder occurrence. Complex association instances read these attributes
from their inherited `ITEM_IDENTIFIED_REPRESENTATION_USAGE` partial.
`DRAUGHTING_CALLOUT` carries an ordered callout-content set. Each drawing
relationship target transfers when its source record has exactly one neutral,
named opaque, or source-native identity. A terminal source-typed target with no
neutral identity owns
an identity-only `NativeRecord` in the STEP `drawing_targets` arena; its
source id and complete source type remain available for the relationship.
Representation-context relationships without a neutral target retain their raw
source reference with an explicit loss. An annotation plane transfers through its
plane carrier, and a mapped item transfers through the items of its mapped
representation, when that wrapper has no identity of its own and the reachable
carrier graph has exactly one neutral identity and is acyclic. If a source
record has multiple neutral identities, no target is selected and the raw source
parameter remains stored with `drawing.relationship-target-ambiguous`. A typed
wrapper with a cyclic carrier graph receives its own source-native
identity. Other unresolved wrappers retain their raw source parameter with
`drawing.relationship-untyped-target`. Target selection does not use identity
ordering.
Unsupported drawing graphics retain their source entity and references without
becoming geometric carriers.
