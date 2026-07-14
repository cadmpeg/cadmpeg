# FreeCAD `.FCStd`

## Support envelope

The primary envelope is a ZIP archive containing `Document.xml` with document `SchemaVersion=4`
and `FileVersion=1`. The application graph may contain core App, Part, PartDesign, Sketcher,
Spreadsheet, Assembly, TechDraw, and GUI persistence records. Exact shapes may use text or binary
B-rep side entries. GUI state, thumbnails, persistent element maps, and string-hasher tables are
independently optional.

Schema versions 2 and 3, pre-schema-4 object layouts, and earlier property encodings are separate
legacy envelopes. A decoder must identify their governing version before refusing a layout it does
not support.

Recovery directories, unpacked project trees, backups, and unrelated ZIP archives are not FCStd
documents.

## Container identity

An FCStd document is identified by ZIP framing plus a root `Document.xml` entry whose XML document
element and version attributes identify the persistence document. A ZIP signature alone is not an
FCStd identity marker.

Entry names are unique, relative paths. Absolute paths, parent traversal, encrypted entries, and
names whose normalized form aliases another entry are invalid. Logical entry size, total expanded
size, entry count, nesting depth, and expansion ratio are bounded before allocation or
decompression.

`Document.xml` is the authoritative application object and property graph. `GuiDocument.xml` is a
presentation graph. Other entries acquire meaning only from typed references in either graph;
unreferenced entries remain named archive records.

## Version dispatch

`ProgramVersion` is metadata. Parsing dispatch is selected by container layout, document schema and
file version, object type, property type, value tag, and side-entry form. Unsupported combinations
are reported using those structural attributes.

## Identity and retention

Every document object has a stable identity composed from the document identity and its persisted
object identity. Every property identity includes its owner and persisted property name. Source
order is significant for declarations, properties, links, and side-entry requests.

Unknown object and property types retain their type name, owner, persisted name, status and dynamic
metadata, recoverable links, raw XML span, referenced entry bytes, and source order. Unknown
application records remain named records rather than being merged into one document-wide payload.

Serialized Python and extension payloads are inert bytes. Reading, inspecting, validating,
diffing, and exporting never executes or imports them.

## Measurement semantics

Native scalar text and native quantity values are retained exactly. Neutral model-space lengths
are millimetres. Angles retain whether the native value is radians or degrees. Parameter domains,
placements, orientation, tolerance values, and display-unit settings are distinct fields; display
units do not rescale model geometry.

## Byte accounting

The physical archive and every decompressed logical entry each have an independent byte ledger.
Ledger spans are ordered, non-overlapping, and cover the complete stream.

Physical ZIP spans classify local headers, names, extra fields, compressed payloads, data
descriptors, central-directory records, end records, archive comments, and legal padding.
Compressed bytes and the corresponding logical entry bytes belong to different ledgers.

Logical XML spans classify declarations, delimiters, comments, whitespace, and escaping as
structural bytes. Typed values own their exact lexical spans. A retained record owns one named
opaque span with its declared length and digest. No byte may be both typed and opaque.

## Exact shapes

Part shape properties reference text or binary B-rep entries. Shape records retain native table
indices, locations, geometry carriers, topology, tolerances, flags, parameter ranges, and pcurves.
Transient table indices do not constitute persistent element identity. Persistent element names
exist only when an element-map record supplies them.

A shape value optionally carries an element-map version and a zero-based document string-table
index. A newly encoded string table consists of a legacy marker followed by a second XML element,
either containing the table stream or naming a side entry. Side-entry streams begin with
`StringTableStart v1` and a decimal record count. Each record begins with a hexadecimal string id,
a hexadecimal flag word, and zero or more dotted hexadecimal string-id references. A leading
minus on an id encodes a positive delta from the preceding id. Dotted references are deltas from
the corresponding preceding references; references beyond that preceding vector are encoded as a
subtraction from the current id. Non-postfixed payloads use a decimal newline count followed by a
colon and exact text. Postfixed payload fields are whitespace-delimited according to their flag
bits. XML and stream counts must agree.

A newly encoded element map likewise uses a compatibility marker followed by a second XML element,
inline or side-entry. A side entry begins with `BeginElementMap v1`. The stream then carries a map
id, an ordered postfix dictionary, a positive map-node count, and contiguous one-based map nodes.
Each node contains ordered indexed-name groups. A group contains child-map descriptors followed by
one persistent-name chain per transient indexed element. Chains terminate with `0`; each name
encodes a literal or dictionary-derived base, a postfix-dictionary index, and persistent string-id
references. The final node owns the shape. Group order and name position establish `Face1`,
`Edge1`, `Vertex1`, and the corresponding other topology-kind indices. These transient positions
are connected to persistent names and to every placed neutral occurrence; they are never exposed
as persistent identity by themselves. Counts, indices, dictionary references, string references,
property ownership, and neutral topology links are validated without synthesizing missing names.

The native location chain is applied exactly once at the owning topology level. Display
tessellation is presentation data and does not replace an available exact shape.

## Design-history transfer

Construction objects retain source order and native identity independently of their cached shape.
Planar sketch geometry is transferred in persisted entity order. Non-construction line segments
are connected into deterministic oriented profile chains; circles and arcs retain canonical
millimetre/radian values, while unsupported geometry families remain explicit native sketch
entities. A persisted placement supplies the sketch origin, normal, and in-plane axis by applying
its normalized quaternion to the canonical sketch basis. Attachment support and mapping mode remain
linked source state when their complete support-frame composition is not resolved.

Sketch constraints retain their append-only native family code and ordered geometry-position
operands. Coincident, horizontal, vertical, parallel, tangent, perpendicular, equal, block,
distance, horizontal/vertical distance, angle, radius, and diameter relations transfer to neutral
constraints when every operand resolves. Dimensional relations create canonical parameters linked
to the source constraint property and retain whether the value is driving. Negative external
indices, unsupported midpoint interpretations, unresolved operands, and future family codes remain
explicit native relations rather than being guessed.

An expression binding is retained independently from its target property's cached scalar. The
neutral parameter carries the exact decoded expression, evaluated canonical value, scalar-property
identity, expression-engine identity, and dependencies on other decoded parameters when qualified
or same-owner identifiers resolve unambiguously. Unresolved symbols remain expression text and do
not create fabricated parameter identities.

Spreadsheet sheets are equation-tree nodes. Every persisted used cell becomes an ordered design
parameter whose identity includes its sheet and address. Address and alias remain separate;
content, display unit, alignment, style, colors, and spans are retained independently. Plain numeric
content supplies a dimensionless evaluated value, while formula content remains an expression.
Same-sheet aliases and qualified `Sheet.alias` references connect spreadsheet and feature
parameters without evaluating arbitrary formulas in the decoder. Cell counts are bounded and must
match their declared framing.

## Product structure

Native namespace version 4 adds a `product_nodes` arena for groups, parts, link groups, and placed
link occurrences. Containers retain ordered member object identities. An occurrence retains its
own identity separately from its local or external prototype, its local affine placement and owning
property, link-transform policy, and array element count. Cross-document link properties use their
document token and target name without attempting to open the document. Missing local targets and
container/prototype cycles are validation errors; external targets remain intentionally unresolved
records rather than being silently dropped or recursively loaded.

Native namespace version 5 extends occurrences with ordered link-array element placements and
scale vectors. Each side entry begins with a little-endian element count followed by either all
single-precision or all double-precision components; exact entry length selects the precision.
Placement records carry position plus quaternion, while scale records carry three components.
Zero quaternions, non-finite values, malformed lengths, and non-empty list counts that disagree with
`ElementCount` are invalid.

Native namespace version 6 adds ordered assembly-joint records. Grounded constraints retain their
object and grounding frame. Other joints retain the persisted enumeration family, two connector
targets with each target's ordered subelement path, and both connector-local frames. Angular,
linear, limit-enable, detach, and suppression values remain independently named parameters. Nested
`Sub` elements belong to their enclosing cross-link and are not separate object references. Joint
Python proxy payloads remain inert native properties; decoding never imports their module.

## Drawing graph

Native namespace version 7 adds a `drawings` arena for every TechDraw page, template, view,
dimension, and annotation subtype. Pages retain ordered view membership and template identity.
Views and dimensions retain ordered local or external source objects with their subelement paths.
Position, scale, projection, direction, rotation, caption, format, measurement, and lock fields keep
their exact value XML by property name. Template and drawing side entries remain linked assets.
Validation rejects missing local page, template, view, source, or side-entry targets while leaving
unknown TechDraw subclasses available through their complete native object/property records.

Pad, pocket, and linear-extrusion records resolve linked neutral sketches when their profile link
targets an earlier decoded sketch. Their literal and evaluated length values remain linked to the
owning native property, and the operation records distinguish additive, subtractive, and
independent-body semantics. Object dependency links establish construction dependencies, and a
feature's cached shape property links its neutral operation to every transferred result body from
that payload. PartDesign body containers are structural history nodes: their group links establish
feature-tree parentage, while the tip link remains separate active-result state. Suppressed, active,
frozen, invalid, touched, mapping, support, tip, and visibility properties remain individually
named state rather than being collapsed into one enabled flag. This is a typed tracer subset;
complete support-frame composition and the remaining Part and PartDesign operation families are
still required by the L4 gate.

Revolution and groove operations retain their linked profile, explicit base point and axis,
one-angle or two-angle extent, and additive or subtractive effect. Fillet operations retain a
constant radius, and chamfers distinguish equal-distance, two-distance, and distance-angle laws.
These operation dimensions participate in the same literal/evaluated/expression parameter graph.
When a dress-up subelement selector has not resolved through persistent topology identity, its
native `Base` property remains the edge selection; the decoder does not infer an edge from a
transient label.

Decode loss reporting is attributable at the native record boundary. Each design operation or
sketch geometry family that remains only in the native lane produces its own blocking note carrying
the object or property identity and `Document.xml` provenance. Successfully neutralized geometry
does not inherit a format-wide placeholder loss.

## Presentation and application records

GUI records retain view-provider identity separately from application-object identity. Visibility,
display modes, materials, colors, line and point styles, cameras, view state, tree state, clipping,
thumbnail references, and display assets remain presentation records linked to their owners.
Native namespace version 3 adds ordered `gui_view_providers` and `gui_properties` arenas. A provider
retains its name, optional application-object link, expansion state, order, and exact XML. Each GUI
property retains its owner, runtime type, status, ordered value elements, referenced side entries,
exact XML, and byte range. GUI-only providers remain valid named records rather than being attached
to an unrelated application object.

For shape-bearing objects, the view provider's shape color, transparency, visibility, and material
scalars produce an object appearance and explicit body bindings. Packed colors decode as red,
green, blue, and reserved low byte; the independent transparency percentage determines opacity.
The effective body display fields mirror this object-level assignment. Per-face diffuse-color lists
are a higher-precedence presentation layer and are not inferred from the object color. Their
little-endian count and packed-color records bind only when the count equals the owning element
map's ordered Face group. Each persistent face name supplies the neutral face occurrences receiving
the override, and the resulting bindings explicitly record face-over-object precedence. Missing
identity or a count mismatch leaves the side entry retained without guessing transient face labels.

Application data without a neutral representation retains its owning object and property,
declared application type, links, source order, XML bytes, referenced side-entry bytes, byte spans,
lengths, and digests.

Native namespace version 8 adds an ordered `applications` census covering every declared object
exactly once. Each record retains the exact runtime type, its application-domain prefix, ordered
owned properties, ordered dependencies, and referenced side entries. A record is marked as carrying
an inert payload when it owns a Python-object property. Decoding never imports, instantiates, or
executes serialized application code. Validation derives the census again from the authoritative
object/property graph and rejects missing, duplicate, reordered, or cross-owned records.

Native namespace version 9 separates semantic annotation records from their drawing presentation.
Annotation, dimension, balloon, leader, and symbol objects retain ordered visible text, all model
and subelement references grouped by source property, exact parameter records, and referenced
assets. Drawing records independently retain every link-valued relationship, including projection
and section parents rather than only page membership and model sources. Validation requires exact
annotation-object coverage and resolves both annotation and drawing relationships.

Native namespace version 10 adds a `gui_documents` arena. A GUI archive has exactly one document
record; a headless archive has none. The record retains the GUI schema and root attributes plus
every document-level element outside `ViewProviderData` in source order. These named state records
cover cameras, active views, clipping or section state, and future GUI state without treating it as
an application-object property. Each retains its exact XML span, ordered descendant values, and
display-asset references.

Logical byte accounting consumes the records emitted by each bounded parser. Exact-shape,
side-entry string-table, and side-entry element-map payloads are wholly typed after successful
framing. `Document.xml` properties and `GuiDocument.xml` state/property spans are typed while the
intervening XML syntax is structural. Uninterpreted embedded assets remain named opaque and retain
their owning record. These claims are sorted and rejected on overlap before the ledger is emitted;
validation then requires every logical entry to close without gaps.

Native namespace version 11 adds attachment records. Support links retain ordered object and
subelement identity separately from the map mode. The persisted resolved `Placement` and local
`AttachmentOffset` remain distinct matrices. Neutral geometry uses the resolved placement when it
is present and otherwise the offset; the decoder never multiplies both speculatively. Validation
checks support identity, finite matrices, and this effective-frame rule.

Sketch point, line, circle, circular-arc, ellipse, and elliptical-arc carriers transfer only when
all family-required numeric fields are present and finite. Ellipse orientation may be carried as a
major-axis angle or a two-component major-axis direction. Bounded ellipses additionally require
both parameter bounds. Missing radii, coordinates, orientation, or bounds leave the carrier as a
named native geometry record; the decoder does not synthesize zero coordinates or full-curve
bounds.

Sketch B-splines retain degree, periodic state, ordered poles, rational weights, and distinct knot
values with their positive multiplicities. The neutral NURBS knot vector expands each value by its
multiplicity. Declared pole and knot counts must match; values and weights must be finite; weights
must be positive; planar pole z-coordinates must be zero; and degree must be smaller than the pole
count. A non-periodic full knot vector must contain `pole_count + degree + 1` entries. Invalid or
resource-exceeding records remain named native carriers.

Constraint transfer distinguishes whole entities from endpoint and center loci. Two-locus distance
constraints remain locus-to-locus measurements rather than being reduced to a duplicate entity
list, and three-operand symmetry retains both loci plus its axis entity. Dimension parameters keep
their driving flag and native identity; angular values use radians, geometric distances use model
lengths, and spline-weight values are dimensionless. Any constraint family left in the native
variant emits its own attributable blocking design-loss record.

Standalone Part and additive or subtractive PartDesign box, cylinder, cone, sphere, and torus
objects transfer as neutral analytic-solid primitives. Lengths are canonical model lengths and
persisted degree-valued angular bounds become radians. A standalone primitive creates a new body;
additive and subtractive families explicitly join or cut. Required dimensions must be finite,
linear sizes must be positive except that one cone end radius may be zero, and latitude bounds must
be ordered. Incomplete or invalid primitive definitions remain attributable native operations.
