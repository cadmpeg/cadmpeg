# FreeCAD `.FCStd`

## Support envelope

The primary envelope is a ZIP archive containing `Document.xml` with document `SchemaVersion=4`
and `FileVersion=1`. The application graph may contain core App, Part, PartDesign, Sketcher,
Spreadsheet, Assembly, TechDraw, and GUI persistence records. Exact shapes may use text or binary
B-rep side entries. GUI state, thumbnails, persistent element maps, and string-hasher tables are
independently optional.

The write envelope targets exactly schema 4/file 1. A retained document write regenerates the ZIP
container deterministically, writes `Document.xml` first, preserves every unedited XML record and
every named side entry, and serializes checked leaf-value edits with XML escaping. An edit to a
nested value without a typed serializer is refused rather than flattening or discarding children.
Schema/file targets outside the declared band and retained-document transcoding across bands are
explicitly refused.

Source-less construction declares application objects, runtime types, ordered dependencies,
recursive typed property values, and named side entries. It materializes the same native graph
used by decoded documents before encoding. This permits general extension-object retention and
parametric core objects without requiring a source archive; unsupported semantics must be supplied
as named records or rejected, never silently approximated.

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

Text shape sets accept the complete declared header band V1 through V3. Binary shape sets accept
V1 through V4. The version controls checked-flag restoration, cached curve-on-surface UV endpoints,
point-representation framing, and triangulation normals. Headers outside those closed ranges are
rejected before table parsing. Successfully parsed payloads emit a machine-derived census of every
recursive 2D curve, 3D curve, surface, polygon, triangulation, and topology family. Native
validation recomputes that census from the retained shape tables and rejects any mismatch.

Polygon carriers are also transferred as bounded neutral geometry. An edge without an exact 3D
curve uses its stored 3D polygon or polygon-on-triangulation nodes as a polyline, retaining explicit
parameters when present and scaling the chordal deflection with the carrier location. A face
without an analytic or spline surface uses its linked triangulation as a polygonal surface. The
same transformed vertices and zero-based triangle indices remain available as occurrence-owned
tessellation; no analytic carrier is inferred from sampled data.

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
are connected into deterministic oriented profile chains. Points, lines, circles, ellipses,
hyperbolas, parabolas, their bounded arc forms, and rational or non-rational B-splines retain
canonical millimetre/radian values and parameter bounds. Both start/end-angle and legacy
first/last-parameter bound names identify the same conic interval. A persisted placement supplies
the sketch origin, normal, and in-plane axis by applying
its normalized quaternion to the canonical sketch basis. Attachment support and mapping mode remain
linked source state when their complete support-frame composition is not resolved.

Sketch constraints retain their append-only native family code and ordered geometry-position
operands. Coincident, horizontal, vertical, parallel, tangent, perpendicular, equal, block,
distance, horizontal/vertical distance, angle, radius, and diameter relations transfer to neutral
constraints when every operand resolves. Point-on-object, symmetry, internal alignment, optical
refraction, B-spline weight, geometry group, and text relations retain their typed operands and
family-specific data. Dimensional relations create canonical parameters linked to the source
constraint property and retain whether the value is driving. Negative external indices,
unresolved operands, and future family codes remain explicit native relations rather than being
guessed.

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
match their declared framing. A neutral sheet record binds those cell identities to the owning
feature and retains ordered non-default column widths, row heights, and inclusive merged ranges.
Dimension counts must match their records; names, addresses, ownership, merged anchors, duplicate
cells, and overlapping merged ranges are validated.

## Product structure

The native `product_nodes` arena retains groups, parts, link groups, and placed link objects exactly
as application records. CADIR components separate reusable definitions from occurrences. Ordered
container membership resolves to component or occurrence ids, and each link-array element becomes
its own occurrence with a stable array index, scale, local transform, and transform resolved through
its containing components exactly once. Local prototypes resolve to component ids; cross-document
links keep the document token and target object without attempting to open the document. Missing
local targets, duplicate occurrence parents, invalid array counts, non-finite transforms, and
container cycles are validation errors; external targets remain intentionally unresolved.

The exact source attribute distinguishes an external file path from a document identity. Neutral
references keep that path or identity separately from the target object and mark resolution as
`unresolved`; decoding never guesses that an external file was loaded. A structurally present but
empty reference is a distinct `missing_reference` state.

Components retain their own local and hierarchy-resolved placements as well as explicit parentage.
Neutral validation recomposes every component and occurrence world matrix from its direct parent
and local matrix and rejects any mismatch, including finite but stale or double-applied transforms.
For nested links, `prototype_transform` records the linked placement chain selected by
`LinkTransform`; the evaluated occurrence is container × local × prototype, each exactly once.
Prototype cycles are invalid in both the native and neutral product graphs.
Component identity keeps the stable source object name separately from its user-visible label,
description, part number, and additional named BOM fields. Generated BOM spreadsheets remain
spreadsheet objects; they are not treated as the authoritative identity of their source component.

Link semantics remain distinct from placement. Prototype subelement paths, tree-child claiming,
base and per-element scale, explicit element objects, and per-element visibility are retained on
neutral occurrences. Copy-on-change is typed as disabled, enabled, owned, tracking, or an explicit
future native policy, with its source, ownership group, and touched state resolved independently.
All array-valued fields must either be absent or match `ElementCount`.

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

CADIR assembly joints resolve local connector objects to component ids while retaining exact
object and persistent subelement paths. Fixed, revolute, slider, cylindrical, ball, distance,
parallel, perpendicular, angle, rack-pinion, screw, gears, belt, and grounded families are typed;
an unfamiliar future family remains explicitly native. Connector frames, connector attachment
offset frames, suppression and detach flags, linear offsets, and enabled limit intervals are
independent fields.
Persisted degree values convert to radians for neutral angles and angular limits. Validation checks
operand/frame cardinality, component references, finite values, and ordered intervals.

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
ordered feature-tree membership and reciprocal parentage, while the tip link identifies one owned
member as the active result. Suppressed, active, frozen, invalid, touched, mapping, support, and
visibility properties remain individually named state rather than being collapsed into one enabled
flag. Validation rejects duplicate members, inconsistent parentage, missing members, and an active
tip outside the body's ordered membership.

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

Format-neutral document and view presentation arenas represent GUI state. A GUI archive produces
one document presentation record; a headless archive produces none. The neutral document record
contains the schema version, active view, finite camera position and nonzero orientation quaternion,
ordered document state, and resolved display-asset references. Each view-provider record contains
its resolved application object, source order, tree expansion and visibility state, display and
selection modes, nonnegative line and point sizes, and exact-name fallback properties. References,
orders, and numeric invariants are validated independently of the FCStd native namespace.

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

Mesh-kernel properties reference a binary side entry. The current typed record begins with the
32-bit magic `a0b0c0d0`, the 32-bit version `00010000`, and a 256-byte information field. Both
integer byte orders are accepted when the magic and version agree. Two 32-bit counts precede
ordered float32 XYZ points and facets. Each facet contains three zero-based point indices followed
by three stored neighbour indices. Six float32 bounding-box limits close the record. Counts are
bounded, point indices must resolve, coordinates and bounds must be finite, and trailing or
truncated bytes are invalid. Neighbour indices and the complete entry bytes remain native even
when only the indexed triangle mesh is projected neutrally.

Point-kernel properties reference a side entry containing a little-endian 32-bit point count
followed by ordered float32 XYZ triples. The property's `Points` element carries the sixteen finite
row-major transform scalars. Neutral points are transformed once into model space and retain the
owning application object and property identity. Missing transforms mean identity; malformed
transforms, non-finite coordinates, excessive counts, truncation, and trailing bytes are rejected.

Native namespace version 8 adds an ordered `applications` census covering every declared object
exactly once. Each record retains the exact runtime type, its application-domain prefix, ordered
owned properties, ordered dependencies, and referenced side entries. A record is marked as carrying
an inert payload when it owns a Python-object property. Decoding never imports, instantiates, or
executes serialized application code. Validation derives the census again from the authoritative
object/property graph and rejects missing, duplicate, reordered, or cross-owned records.

Native namespace version 18 makes application preservation independently auditable. Every
application record now retains its object-data order, exact `Document.xml` span and bytes, length,
and SHA-256. Every owned property has a nested preservation record containing owner and property
identity, runtime type, typed persistence family, order, links, exact span and bytes, length,
SHA-256, inert-code classification, and complete referenced payload records. Each payload retains
its global entry identity, exact name, complete logical bytes, length, and SHA-256. Validation
reconstructs the complete preservation graph from authoritative object, property, and entry arenas
and rejects any byte, digest, ownership, ordering, link, or payload mismatch.

Native namespace version 9 separates semantic annotation records from their drawing presentation.
Annotation, dimension, balloon, leader, and symbol objects retain ordered visible text, all model
and subelement references grouped by source property, exact parameter records, and referenced
assets. Drawing records independently retain every link-valued relationship, including projection
and section parents rather than only page membership and model sources. Validation requires exact
annotation-object coverage and resolves both annotation and drawing relationships.

The format-neutral drawing arena contains pages, templates, model
views, projection groups, sections, details, dimensions, annotations, balloons, symbols, leaders,
images, and extension drawing objects retain their runtime classification and source order. Local
drawing relationships resolve to neutral drawing identities, model sources resolve to their local
object identities, and external document/object pairs remain explicit without being treated as
local references. View position, positive scale, nonzero projection direction, rotation, exact
fallback parameters, and resolved template or image assets are independently validated.

The format-neutral semantic-annotation arena contains dimensions, notes,
geometric tolerances, datums, balloons, leaders, symbols, and extension annotations retain source
order, visible text, exact runtime classification, role-grouped model or drawing references,
subelement selectors, explicit numeric measurements, formatting expressions, positions, fallback
parameters, and resolved assets. Local drawing targets resolve to neutral drawing identities;
external document/object pairs remain explicit. Referential and finite-numeric validation is
independent of drawing presentation and of provenance annotations.

Persisted empty drawing and annotation links are explicit. A target whose
native link record is present but names no document object has `is_null: true`; it is distinct
from an absent target and from an unresolved nonempty reference. Local referential validation
therefore accepts only explicitly null empty targets and continues to reject every nonempty
missing object identity.

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

Native namespace version 19 adds a deterministic `byte_coverage` report. It records physical
archive length and span count, logical entry length and span count, byte totals by the closed
`structural`, `typed`, and `named_opaque` classes, and the sorted entries containing opaque bytes.
Its `exact` flag is true only when the physical archive and every nonempty logical entry partition
from zero through the declared length with positive, contiguous, nonoverlapping spans. Validation
re-derives the report, rejects missing or unknown logical entries, validates every typed or opaque
span owner, requires structural spans to be ownerless, and rechecks retained entry lengths and
SHA-256 digests. Zero-length entries are represented by an empty partition and still counted.

Native namespace version 20 gives a zero-byte exact-shape side entry the typed `empty` payload
form. This is FreeCAD's persisted representation of a null or suppressed `PropertyPartShape`, not
a malformed text B-rep. Only side entries classified as B-rep payloads are parsed as shapes;
element-map, placement-list, scale-list, and other side entries owned by the same property remain
in their own typed or named-opaque carrier.

Native namespace version 11 adds attachment records. Support links retain ordered object and
subelement identity separately from the map mode. The persisted resolved `Placement` and local
`AttachmentOffset` remain distinct matrices. Neutral geometry uses the resolved placement when it
is present and otherwise the offset; the decoder never multiplies both speculatively. Validation
checks support identity, finite matrices, and this effective-frame rule.

Native namespace version 12 adds one carrier-census record per exact-shape payload. Census records
identify text versus binary framing, the declared topology version, recursive carrier-family
counts, all eight topology families, and polygon and triangulation counts.

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
list, three-operand symmetry retains both loci plus its axis entity, and point-on-object retains the
point locus separately from its supporting entity. Refraction retains its two curve loci,
interface entity, and dimensionless index ratio. Spline weights remain dimensionless parameters,
and internal-alignment helpers retain their conic or spline family plus control-point or knot index.
Group relations retain their ordered handle and member loci. Text relations additionally decode
their JSON metadata into text, font, and height-versus-width control while retaining the original
metadata string. When a relation addresses the implicit horizontal axis, vertical axis, or root
point, that negative source operand resolves to an exact construction-only reference line or point;
no finite axis segment is synthesized. External-geometry ids begin after those two implicit axes.
They resolve to the corresponding cached external carrier while retaining the ordered object and
subelement link as its source reference. Dimension parameters keep their driving flag
and native identity. Every relation independently retains its name, metadata, solver-active,
visible, virtual-space and driving flags, orientation bits, and finite label placement. Angular
values use radians, geometric distances use model lengths, and spline-weight values are
dimensionless. Any constraint family left in the native variant emits its own attributable
blocking design-loss record.

Standalone Part and additive or subtractive PartDesign box, cylinder, cone, sphere, and torus
objects transfer as neutral analytic-solid primitives. Lengths are canonical model lengths and
persisted degree-valued angular bounds become radians. A standalone primitive creates a new body;
additive and subtractive families explicitly join or cut. Required dimensions must be finite,
linear sizes must be positive except that one cone end radius may be zero, and latitude bounds must
be ordered. Incomplete or invalid primitive definitions remain attributable native operations.

Part cut, fuse, common, multi-fuse, and multi-common objects transfer as neutral Boolean combine
operations. Two-input forms retain distinct `Base` and `Tool` property identities. Multi-input
forms define link zero as the target and the remaining ordered `Shapes` links as tools without
claiming that application-object links are already neutral body ids. Feature dependencies are the
stable union of declared object dependencies and earlier link-property operands in source order.

Part and PartDesign lofts retain ordered section profiles and closed state. Part sweeps and
PartDesign additive or subtractive pipes retain the profile plus the complete native spine/path
property, including its ordered subelement selectors. Standalone sweeps distinguish surface from
solid results through their persisted solid flag; PartDesign pipes are solid and explicitly join
or cut. Cached result shapes remain outputs and do not replace these construction operands.

Lofts additionally retain whether adjacent sections use ruled spans and whether a standalone Part
loft produces a solid or sheet result. When carried, the interpolation degree limit and section
compatibility policy remain explicit. PartDesign lofts are solid and explicitly join or cut;
standalone lofts create a new result body without fabricating a Boolean relationship.

Sweeps retain the primary and additional ordered sections, primary path and tangent-edge
extension, corrected-Frenet, fixed, Frenet, auxiliary-path, or fixed-binormal orientation,
transformed, sharp, or rounded corner transition, and constant, multisection, linear, S-shaped, or
smooth-interpolation section transformation. Auxiliary orientation additionally retains its path,
tangent-edge extension, and curvilinear correspondence flag. Standalone sweep linearization and
solid-versus-sheet result remain explicit. Invalid enumeration values, a zero binormal, or a
missing auxiliary path leave the operation attributable and native.

Part scale operations retain their source-shape selection and model-origin scale center. Uniform
mode carries one factor; anisotropic mode carries independent x, y, and z factors. Finite nonzero
negative factors remain valid reflections. Missing sources, zero factors, and non-finite factors
remain attributable native operations.

Part and PartDesign thickness operations retain removed-face selection, absolute wall thickness,
offset side, skin, pipe, or both-sides mode, arc, tangent, or intersection corner continuation,
intersection resolution, and self-intersection policy. A signed Part thickness selects its offset
side; the PartDesign reversal flag selects the same neutral meaning. Whole-shape Part offsets
retain their source, signed distance, the same mode and join laws, intersection policies, boundary
fill, and the distinction between three-dimensional and planar offset construction. Planar
both-sides mode and incomplete or zero-distance operations remain attributable and native.

Part compound operations retain the complete ordered source list as one non-Boolean topology
construction; the alternate compound persistence class has the same construction semantics.
Refine operations retain the single source whose redundant splitter boundaries are removed.
Reverse operations retain the single source whose complete topological orientation is inverted.
Missing, empty, or multiply valued single-source links remain attributable native operations.

Part ruled surfaces retain two independently selected curve or wire boundaries and automatic,
forward, or second-boundary-reversed traversal. Part section operations retain their two shape
operands and whether section edges use approximation. Each operand must resolve to exactly one
persisted link; invalid orientation values and incomplete operands remain attributable native
operations.

Standalone Part mirror operations retain their single source shape and the resolved model-space
plane as an origin and unit normal. When present, the native plane, face, or circle selection that
supplied that resolved plane remains attached for attribution and dependency recovery. A missing
source or zero-length normal leaves the operation attributable and native.

Parametric Part helices retain radius, pitch, height-derived revolution count, handedness, conical
angle, optional curve-subdivision length, and legacy-versus-corrected construction style. Planar
Part spirals use the same neutral curve family with zero axial pitch and retain radius growth per
revolution, total rotations, and subdivision length. Invalid dimensions or enumeration values
leave the operation attributable and native.

Part projection-on-surface operations retain the complete ordered source-subelement property, one
support face, normalized projection direction, all-shapes, faces-only, or edges-only result mode,
solid extrusion height, and signed surface offset. Empty sources, ambiguous support selections,
invalid modes, and zero directions leave the operation attributable and native.

PartDesign operations that carry topology post-processing retain it compositionally around the
underlying neutral operation. Redundant-boundary refinement remains independently enabled or
disabled, and fuzzy tolerance distinguishes modeling-kernel default, automatic determination, and
an explicit positive tolerance. Wrapping an attributable native operation does not suppress its
design-domain loss report.

Plain Part and PartDesign features are direct stored geometry rather than unknown parametric
operations. Their exact shape payload supplies the feature outputs when present; no replay
construction is fabricated when a stored feature is empty or frozen. A PartDesign base feature is
instead a derived-geometry operation whose input is the earlier linked feature. Application-owned
feature subclasses remain in the complete native object/property graph and are not misclassified
as built-in modeling operations solely because their type derives from a core feature class.
Legacy spline, extended-feature, geometry-set, and planar-feature containers likewise represent
direct stored geometry when they carry no replay construction. STEP, IGES, B-rep, and curve-network
import features instead retain their exact external path and source model format as replayable
import intent; an absent or empty source path leaves the feature attributable and native.

The native design census contains one record for every object admitted to the design projection.
Each record binds the persisted object type and neutral feature identity to its CADIR semantic
family, native-versus-neutral status, and post-processing composition. Native validation derives
the census again from the object and feature graphs; missing projections and stale classifications
are errors rather than coverage-report omissions.

Native document, object, property, payload, ledger, product, drawing, and application identities
use canonical CADIR ids. Persisted names form percent-escaped id keys while the exact unescaped
name remains in its typed record. Child records derive their key from the owning record instead of
embedding a second id delimiter. Neutral topology and carrier ids use the shape-payload key under
their own model arena kind, so persistent references remain globally valid and collision-free.

Part construction geometry transfers as neutral history rather than relying on cached result
shapes. This includes standalone vertices, line segments, circular and elliptic angular arcs,
open or closed ordered polylines, regular polygons, bounded rectangular planes, and faces built
from ordered source shapes with an extensible face-maker class. Invalid dimensions, coincident
line endpoints, undersized point lists, and empty face sources remain attributable and native.

Part and PartDesign revolutions retain the resolved axis together with the native edge, datum, or
sketch-axis selection that supplied it. Standalone Part revolutions additionally retain
solid-versus-sheet result and the face-maker class used for solids. PartDesign revolutions retain
the compatibility ordering used when fusing the new feature with the existing body. Every
profile-based PartDesign operation—extrusion, revolution, loft, pipe, helix, and hole—retains
whether a profile containing multiple faces is accepted as one construction input.

PartDesign linear and polar patterns retain both uniform and explicitly spaced instance
sequences. Explicit sequences are cumulative transforms beginning at the unchanged seed; per-gap
values override defaults, while multi-value spacing patterns repeat cyclically for unspecified
gaps. A second linear direction is an ordered Cartesian-product stage with its own direction,
reversal, mode, occurrence count, and spacing sequence. Invalid counts, list cardinalities,
directions, and non-positive intervals leave the operation attributable and native.

Part extrusions retain their normalized direction, custom-vector, selected-edge, or profile-normal
direction source, independent forward and reverse lengths and tapers, symmetric construction, and
solid-versus-sheet result. Solid construction additionally retains the extensible face-maker class
and mode and whether inner wires taper with or against outer wires. A zero pair of explicit lengths
uses the persisted direction-vector magnitude. PartDesign pads and pockets distinguish blind,
through-all, first-intersection, last-intersection, face-selected, and shape-selected termination
independently on both sides. Midplane construction mirrors either a length or a non-length
termination, while signed blind lengths preserve the persisted side orientation. Features retain
both taper angles and offsets, whether length follows the profile normal, and whether multiple
profile faces are allowed. Direction provenance distinguishes the profile normal, an explicit
custom vector, and a selected reference axis while also retaining the normalized resolved
direction; reversal inverts that direction. A pad joins and a pocket cuts. Missing required lengths
or selections and invalid directions remain attributable native operations instead of being
rewritten as zero-length or blind features.

Part and PartDesign revolutions normalize the persisted axis direction and retain its model-space
origin. Angular, symmetric-angular, two-angle, through-all or last-intersection,
first-intersection, and selected-face termination remain distinct. Reversal changes the oriented
axis rather than the magnitude of the angular extent. Standalone Part revolutions create a new
body, PartDesign revolutions join, and grooves cut. A missing profile, zero axis, invalid angle, or
incomplete selected termination remains an attributable native operation.

Part and PartDesign fillets distinguish an explicit edge selection from the persisted all-edges
mode and require a finite positive constant radius. Chamfers retain equal-distance,
two-distance, and distance-angle dimensions plus the persisted reference-side reversal. Their
linear dimensions must be finite and positive, and their angle must lie strictly between zero and
180 degrees. An absent selection or invalid dimensional law remains an attributable native
operation rather than an unresolved neutral dress-up feature.
