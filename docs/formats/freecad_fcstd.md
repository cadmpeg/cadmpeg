# FreeCAD `.FCStd`

Record offsets, field widths, and endianness are also maintained as a machine-checked table in [`docs/layouts/freecad.md`](../layouts/freecad.md), generated from `docs/layouts/freecad.toml`. That table is the canonical source for the numbers; the prose below carries the semantics. `cargo test -p cadmpeg --test layout_tables` proves the two agree.

## 1. Support envelope

The primary write envelope is a ZIP archive containing `Document.xml` with document
`SchemaVersion=4` and `FileVersion=1`. The decode envelope accepts document schemas 2, 3, and 4.
The application graph may contain core App, Part, PartDesign, Sketcher,
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

Schema 2 has one `Features` declaration section and one `FeatureData` value section. Schema 3 and
schema 4 have one `Objects` declaration section and one `ObjectData` value section. The section and
record names are part of the schema grammar and are not interchangeable. The root attributes use
the canonical spellings `SchemaVersion`, `FileVersion`, and `ProgramVersion`; lowercase aliases and
duplicate section elements are invalid.

Recovery directories, unpacked project trees, backups, and unrelated ZIP archives are not FCStd
documents.

## 2. Container identity

An FCStd document is identified by ZIP framing plus a root `Document.xml` entry whose XML document
element and version attributes identify the persistence document. A ZIP signature alone is not an
FCStd identity marker.

Entry names are unique, relative paths. Absolute paths, parent traversal, encrypted entries, and
names whose normalized form aliases another entry are invalid. Logical entry size, total expanded
size, entry count, nesting depth, and expansion ratio are bounded before allocation or
decompression.

`Document.xml` is the authoritative application object and property graph. `GuiDocument.xml` is a
presentation graph. Both XML roots use the canonical `SchemaVersion` spelling; the lowercase
`schemaVersion` alias is invalid. Other entries acquire meaning only from typed references in
either graph; unreferenced entries remain named archive records.

Each side-entry reference names one ZIP member. The member's complete uncompressed byte sequence
is one payload, and the ZIP member boundary is the only generic side-entry framing. A property
runtime type may define an internal grammar for that payload. A runtime type outside the registered
property grammar does not define a generic internal framing or field model; its complete payload
remains one named opaque record.

`GuiDocument.xml` has at most one `ViewProviderData` container. `ViewProvider` names are unique.
Each provider has one direct `Properties` container, and direct property names are unique within
that container. Registered GUI property types define the cardinality of their value elements;
unregistered properties retain their ordered values without semantic dispatch.

In schema 2, `Features.Count` equals the number of `Feature` declarations. Each declaration has a
unique `name` and a `type`. `FeatureData.Count` equals the number of `Feature` value records. Each
value record has a unique `name`. Declaration and value-record name sets are equal. Declaration
order is object order.

In schemas 3 and 4, `Objects.Count` equals the number of `Object` declarations. Each declaration
has a unique `name` and a `type`. `ObjectData.Count` equals the number of `Object` value records.
Each value record has a unique `name`. Declaration and value-record name sets are equal.
Declaration order is object order.

The presence of the `Objects` section's `Dependencies` attribute enables dependency records. An
enabled section contains exactly `Objects.Count` `ObjectDeps` elements before the object
declarations, in the same order and with the same names. Each `ObjectDeps.Count` equals its number
of `Dep` children. `ObjectDeps` names are unique. An optional `AllowPartial` is a positive integer
and remains attached to its object. A section without `Dependencies` contains no `ObjectDeps`
elements.

## 3. Version dispatch

`SchemaVersion` alone selects the object envelope. `ProgramVersion` is metadata. An absent
`FileVersion` has value zero. `FileVersion` does not select the object or property-container
envelope. It selects versioned side-entry details such as string tables and complex geometry.
Property runtime type and value tag select a property-value grammar.

Document properties and object properties use the same `Properties` container in schemas 2, 3,
and 4. The document root has at most one direct `Properties` container; duplicate root
containers are invalid. `Properties.Count` equals the number of `Property` records. An optional
`TransientCount` equals the number of `_Property` records. Each record has a `name` and `type`.
Property names are unique across both record kinds within one container. A property family is
selected by an exact registered runtime type. An unregistered runtime type does not select a family
from a substring of its name.
A `Property` contains its runtime-type-specific value XML. A `_Property` has no persisted value.
Status and dynamic-property metadata are optional record attributes. Property container dispatch
does not depend on `SchemaVersion`, `FileVersion`, or `ProgramVersion`.

Link properties use a closed runtime-type and value-tag grammar. All runtime types in this
grammar have the `App::` prefix. `PropertyLink` and its
`Child`, `Global`, and `Hidden` variants contain one `Link` with a `value` object name.
`PropertyLinkList` and its three variants contain one `LinkList`; `count` equals the number of
`Link` children, and each child has a `value` object name. `PropertyLinkSub` and its three variants
contain one `LinkSub` with a `value` object name and a `count` of `Sub` children. Each `Sub` has a
`value` subelement name. `PropertyLinkSubList` and its three variants contain one `LinkSubList`;
`count` equals the number of `Link` children, and each child has an `obj` object name and a `sub`
subelement name.

`PropertyXLink`, `PropertyXLinkSub`, and `PropertyXLinkSubHidden` contain one `XLink`. Its `name`
is the object name. An optional `file` is the external document path; an absent or empty `file`
selects the current document. Zero subelements use no `sub` or `count` attribute. One subelement
uses the `sub` attribute. Multiple subelements use `count` and the same number of `Sub` children,
each with a `value` attribute. `PropertyXLinkSubList` and `PropertyXLinkList` contain one
`XLinkSubList`; its `count` equals the number of child `XLink` values. A `shadowed` attribute on a
subelement carrier supplies the restored subelement value; the primary `sub` or `value` remains
the compatibility name. Other carrier names and simultaneous `sub` and `count` carriers are
invalid. A property type outside this registry retains its XML but does not infer link targets from
nested tag names.

## 4. Identity and retention

Every document object has a stable identity composed from the document identity and its persisted
object identity. Every property identity includes its owner and persisted property name. Source
order is significant for declarations, properties, links, and side-entry requests.

Unknown object and property types retain their type name, owner, persisted name, status and dynamic
metadata, links decoded by a registered grammar, raw XML span, referenced entry bytes, and source
order. Unknown application records remain named records rather than being merged into one
document-wide payload.

Serialized Python and extension payloads are inert bytes. Reading, inspecting, validating,
diffing, and exporting never executes or imports them.

## 5. Measurement semantics

Native scalar text and native quantity values are retained exactly. Neutral model-space lengths
are millimetres. Angles retain whether the native value is radians or degrees. Parameter domains,
placements, orientation, tolerance values, and display-unit settings are distinct fields; display
units do not rescale model geometry.

## 6. Byte accounting

The physical archive and every decompressed logical entry each have an independent byte ledger.
Ledger spans are ordered, non-overlapping, and cover the complete stream.

Physical ZIP spans classify local headers, names, extra fields, compressed payloads, data
descriptors, central-directory records, end records, archive comments, and legal padding.
Compressed bytes and the corresponding logical entry bytes belong to different ledgers.

Logical XML spans classify declarations, delimiters, comments, whitespace, and escaping as
structural bytes. Typed values own their exact lexical spans. A retained record owns one named
opaque span with its declared length and digest. No byte may be both typed and opaque.

## 7. Exact shapes

The exact-shape runtime type is `Part::PropertyPartShape`. Other runtime types do not select
exact-shape or persistent element-map parsing. Part shape properties reference text or binary
B-rep entries. Shape records retain native table
indices, locations, geometry carriers, topology, tolerances, flags, parameter ranges, and pcurves.
An exact-shape property has at most one `Part` carrier and at most one `ElementMap2` carrier. Each
carrier belongs to its enclosing property. Duplicate carriers are malformed. A missing carrier
retains the exact property without selecting a different property or side entry.
An OCCT parabola edge parameter `u` maps to the STEP parabola parameter `t = u / (2f)`, where `f`
is the focal distance. A two-dimensional parabola pcurve retains the OCCT parameter `u`.
For a bounded circle or ellipse edge, the neutral start parameter is wrapped into `[0, 2π)` and
the serialized sweep is preserved. A full-period edge retains its serialized phase.
Transient table indices do not constitute persistent element identity. Persistent element names
exist only when an element-map record supplies them.

Text shape sets accept the complete declared header band V1 through V3. Binary shape sets accept
V1 through V4. The version controls checked-flag restoration, cached curve-on-surface UV endpoints,
point-representation framing, and triangulation normals. Headers outside those closed ranges are
rejected before table parsing. Successfully parsed payloads emit a machine-derived census of every
recursive 2D curve, 3D curve, surface, polygon, triangulation, and topology family. Native
validation recomputes that census from the retained shape tables and rejects any mismatch.

Every text shape set has exactly one supported topology header and exactly one marker for each
`Locations`, `Curve2ds`, `Curves`, `Polygon3D`, `PolygonOnTriangulations`, `Surfaces`,
`Triangulations`, and `TShapes` table. The markers occur in that order. Duplicate or missing
markers are invalid.

Polygon carriers are also transferred as bounded neutral geometry. An edge without an exact 3D
curve uses its stored 3D polygon or polygon-on-triangulation nodes as a polyline, retaining explicit
parameters when present and scaling the chordal deflection with the carrier location. A face
without an analytic or spline surface uses its linked triangulation as a polygonal surface. The
same transformed vertices and zero-based triangle indices remain available as occurrence-owned
tessellation; no analytic carrier is inferred from sampled data.

A shape value optionally carries an element-map version and a zero-based document string-table
index. A newly encoded string table consists of a legacy marker whose immediately following XML
element is `StringHasher2`, either containing the table stream or naming a side entry. Side-entry
streams begin with
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
The optional XML count is retained as native metadata and does not frame the map stream. The
stream's map, child, and name counts delimit its records.
Each node contains ordered indexed-name groups. A group contains child-map descriptors followed by
one persistent-name chain per transient indexed element. Chains terminate with `0`; each name
encodes a literal or dictionary-derived base, a postfix-dictionary index, and persistent string-id
references. The final node owns the shape. Group order and name position establish `Face1`,
`Edge1`, `Vertex1`, and the corresponding other topology-kind indices. Name position zero is
reserved; the transient element with one-based index N uses name position N. Each placed root
repeats the same one-based position sequence. For each topology kind, positions follow one
depth-first traversal of serialized child order across roots in serialized root order. Traversal
stops below a child of the requested kind, and the indexed map keeps the first occurrence of each
shape plus composed location while ignoring orientation. The decoder carries that source position
with each transferred neutral occurrence. It does not derive the position from neutral arena
order. Repeated roots at the same placement attach their distinct neutral occurrences to the same
source position; a distinct placement receives the next position. A source element that has no
neutral occurrence leaves its position empty and does not shift later bindings. These transient
positions are connected to persistent names and to every placed neutral occurrence;
they are never exposed as persistent identity by themselves. Counts, indices, dictionary
references, string references, property ownership, and neutral topology links are validated
without synthesizing missing names.

The native location chain is applied exactly once at the owning topology level. Display
tessellation is presentation data and does not replace an available exact shape. Each root shape
use is a distinct neutral occurrence. Repeated root uses of the same shape at the same placement
retain their serialized root order as an occurrence discriminator; they do not share body,
region, shell, face, loop, coedge, edge, vertex, or point identity.

An edge endpoint accessor visits the edge's direct child uses in serialized order. Exactly one
`Forward` vertex supplies the start vertex and exactly one `Reversed` vertex supplies the end
vertex. `Internal` and `External` children do not supply endpoints. Closed and degenerate edges
still require both oriented uses; the uses can reference the same vertex. Duplicate endpoint
orientations and an edge without both endpoint orientations are invalid.

Edge geometry access follows serialized representation order. At most one 3D-curve representation
can supply the exact neutral carrier and parameter range. Only when no 3D curve exists can one
stored polygon representation supply an approximate carrier; multiple fallback polygons are
invalid. For a face use, at most one pcurve representation whose surface and composed location
equal the face surface supplies the pcurve. A closed-surface representation supplies its second
pcurve when the edge use is reversed. Later nonmatching representations remain in the native edge
record. Duplicate matching representations are invalid. The neutral analytic-surface frame uses
the cross product of its axis and reference direction. If the persisted plane frame has the
opposite V direction, the pcurve V parameter is negated. If a persisted cylinder, cone, sphere, or
torus frame has the opposite circumferential direction, the pcurve U parameter is negated. A cone
pcurve V parameter is also multiplied by the cosine of the cone half-angle to convert persisted
slant distance to neutral axial distance. A surface of revolution uses U as its rotation angle and
V as its directrix parameter. A trimmed surface converts its persisted support-coordinate bounds
and pcurves to zero-based local parameters while preserving each bound's direction.

The serialized edge child uses provide endpoint incidence but no radial-neighbor relation. One
coedge is self-radial. Two coedges reference each other. Three or more coedges remain self-radial;
their shared edge identity carries unordered non-manifold incidence without asserting a
serialization-dependent radial cycle.

## 8. Design-history transfer

Construction objects retain source order and native identity independently of their cached shape.
Planar sketch geometry is transferred in persisted entity order. The one-based position of each
`Geometry` element in `GeometryList` is its numeric persisted entity position. Non-construction
line, circular-arc, and elliptical-arc entities are connected into deterministic oriented profile
chains. Each chain starts with the earliest unused non-construction entity by that position and
grows from both endpoints. Points, lines, circles, ellipses, hyperbolas, parabolas, their bounded
arc forms, and rational or
non-rational B-splines retain
canonical millimetre/radian values and parameter bounds. Both start/end-angle and legacy
first/last-parameter bound names identify the same conic interval. A persisted placement supplies
the sketch origin, normal, and in-plane axis by applying
its normalized quaternion to the canonical sketch basis. Attachment support and mapping mode remain
linked source state when their complete support-frame composition is not resolved.
Sketch geometry dispatch uses exact runtime names. `Part::GeomLine` and `Part::GeomLineSegment`
select lines; `Part::GeomCircle` selects circles; `Part::GeomArcOfCircle` selects bounded arcs;
`Part::GeomEllipse` and `Part::GeomArcOfEllipse` select ellipses; `Part::GeomHyperbola` and
`Part::GeomArcOfHyperbola` select hyperbolas; `Part::GeomParabola` and
`Part::GeomArcOfParabola` select parabolas; `Part::GeomPoint` selects points; and
`Part::GeomBSplineCurve` selects NURBS. A `Geometry` record with an unknown runtime name or with
zero or multiple eligible carrier children remains a native sketch entity. Metadata children
`Construction`, `GeoExtensions`, and `UID` do not count as geometry carriers.
An `ExternalGeometry` link creates an ordered construction entity. When `ExternalGeo` supplies its
cached carrier, that carrier defines the solved sketch geometry. Without a cached carrier, the
neutral entity retains the target document, object, and subelements as an unresolved external
reference. Constraints can address its entity, endpoint, and center loci without inventing solved
coordinates.

An active coincident-loci constraint between two non-construction endpoints is authoritative. If an
endpoint has one or more such relations, only those relations are profile candidates. Otherwise,
two bounded endpoints connect when their solved coordinates differ by at most 64 binary64 machine
epsilons at the coordinate scale. The coordinate scale is the maximum absolute endpoint
coordinate or one; the bound covers endpoint evaluation roundoff and is not a persisted FreeCAD
tolerance. If one endpoint connects to more than one endpoint under the selected relation, all
incident entities remain separate single-entity profiles. The decoder does not select one branch
by record order.

Sketch constraints retain their append-only native family code and ordered geometry-position
operands. Coincident, horizontal, vertical, parallel, tangent, perpendicular, equal, block,
distance, horizontal/vertical distance, angle, radius, and diameter relations transfer to neutral
constraints when every operand resolves. Point-on-object, symmetry, internal alignment, optical
refraction, B-spline weight, geometry group, and text relations retain their typed operands and
family-specific data. Dimensional relations create canonical parameters linked to the source
constraint property and retain whether the value is driving. A dimension parameter uses its
one-based constraint index as its neutral name. Its persisted label remains source metadata and is
an expression alias only when that label is unique in the sketch. A one-entity angle measures that
entity from the horizontal sketch axis. In a two-operand angle, negative geometry indices identify
the horizontal or vertical sketch axis independently of their position field. A one-locus
horizontal or vertical distance measures the
locus from the sketch root point. The persisted endpoint-one selector of an isolated point resolves
to the point entity, not to a nonexistent curve endpoint. Negative indices resolve through the
ordered external-reference entities. Invalid indices, unresolved operands, and future family codes
remain explicit native relations rather than being guessed.
`ElementIds` and `ElementPositions`, when present, are complete comma- or whitespace-separated
integer lists with equal lengths. Legacy `First`/`Second`/`Third` fields require their matching
`*Pos` field, and each supplied value must be an integer. Malformed or incomplete operand fields
are invalid rather than partially decoded.

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
match their declared framing. Parameter ordinals use stable dependency order and persisted cell
order as the tie-break rule. A neutral sheet record binds those cell identities to the owning
feature and retains ordered non-default column widths, row heights, and inclusive merged ranges.
Only positive row and column spans define a neutral merged range. Nonpositive span attributes remain
native cell metadata and do not create a neutral range.
Dimension counts must match their records; names, addresses, ownership, merged anchors, duplicate
cells, and overlapping merged ranges are validated.

## 9. Product structure

The native `product_nodes` arena retains groups, parts, link groups, and placed link objects exactly
as application records. Product dispatch uses the exact runtime registry: `Assembly::AssemblyObject`,
`Assembly::AssemblyLink`, and `App::Part` are parts, `App::DocumentObjectGroup` is a group,
`App::LinkGroup` is a link group, and `App::Link` and `App::LinkElement` are occurrences. Other runtime types remain native and do
not enter the product arena. CADIR components separate reusable definitions from occurrences.
Ordered container membership resolves to component or occurrence ids, and each link-array element
becomes its own occurrence with a stable array index, scale, local transform, and transform
resolved through its containing components exactly once. Local prototypes resolve to component ids;
cross-document links keep the document token and target object without attempting to open the
document. Missing local targets, invalid array counts, non-finite transforms, and container cycles
are validation errors; external targets remain intentionally unresolved. The native graph retains
every direct container membership, while neutral admission requires each member to have at most one
distinct parent container. Overlapping parent containers are refused instead of selecting by source
order. Product record identities are unique by object identity; duplicate records are invalid.

The exact source attribute distinguishes an external file path from a document identity. Neutral
references keep that path or identity separately from the target object and mark resolution as
`unresolved`; decoding never guesses that an external file was loaded. A structurally present but
empty reference is a distinct `missing_reference` state.

Components retain their own local and hierarchy-resolved placements as well as explicit parentage.
Neutral validation recomposes every component and occurrence world matrix from its direct parent
and local matrix and rejects any mismatch, including finite but stale or double-applied transforms.
An occurrence has at most one `LinkedObject` target. Its scalar target is not selected from a link
list by source order. When both `LinkPlacement` and `Placement` are present, `LinkTransform=true`
selects `LinkPlacement` and `LinkTransform=false` selects `Placement`; when only one carrier is
present, that carrier supplies the local placement. Both carriers without a valid `LinkTransform`
policy are ambiguous and are refused. Each named `LinkedObject`, `LinkPlacement`, `Placement`, and
`LinkTransform` carrier occurs at most once in a product record. Each placement carrier has at most
one `App::PropertyPlacement` property with at most one `PropertyPlacement` value. A value carries finite
`Px`, `Py`, and `Pz` plus either finite
quaternion components `Q0` through `Q3` or finite axis-angle components `Ox`, `Oy`, `Oz`, and `A`;
when both representations are present, the quaternion components are authoritative.
Missing components, non-finite components, duplicate values, invalid rotation norms, and zero
quaternions are malformed.
For nested links, `prototype_transform` records the linked placement chain selected by
`LinkTransform`; the evaluated occurrence is container × local × prototype, each exactly once.
The chain resolves each local product target through its unique product record. A local target with
no product record contributes its persisted `LinkPlacement` or `Placement` matrix directly.
Prototype cycles are invalid in both the native and neutral product graphs.
Component identity keeps the stable source object name separately from its user-visible label,
description, part number, and additional named BOM fields. Generated BOM spreadsheets remain
spreadsheet objects; they are not treated as the authoritative identity of their source component.

Link semantics remain distinct from placement. Prototype subelement paths, tree-child claiming,
base and per-element scale, explicit element objects, and per-element visibility are retained on
neutral occurrences. `LinkCopyOnChange` is valid for neutral transfer only when its exact runtime
type is `App::PropertyEnumeration`; a same-named property of another runtime type remains native
and does not alter occurrence semantics. Copy-on-change is typed as disabled, enabled, owned,
tracking, or an explicit future native policy, with its source, ownership group, and touched state
resolved independently.
All array-valued fields must either be absent or match `ElementCount`.
A present zero `ElementCount` requires every array-valued field to be empty. The link retains its
single scalar occurrence. An absent `ElementCount` permits one scalar link occurrence or infers a
nonzero count from the populated array-valued fields.

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
Python proxy payloads remain inert native properties; decoding never imports their module. A joint
has exactly one kind carrier: `ObjectToGround` or `JointType`. Both carriers are invalid. The
canonical `ObjectToGround` runtime type is `App::PropertyLinkGlobal`. Legacy `App::PropertyLink`
and `App::PropertyLinkSub` carriers are accepted only with one object target and no nonempty
subelement; all other runtime types are invalid. `JointType` is exactly
`App::PropertyEnumeration` and has exactly one selected `Integer`; its zero-based index selects the matching
ordered `Enum` value when present, and an out-of-range index remains the numeric native family.
Each named scalar joint parameter has at most one root value; duplicate values are invalid.
Each connector-frame and connector-offset carrier is an `App::PropertyPlacement` property with at
most one `PropertyPlacement` value and the same finite position and quaternion or axis-angle
component rules as product placements.

CADIR assembly joints resolve local connector objects to component ids while retaining exact
object and persistent subelement paths. Fixed, revolute, slider, cylindrical, ball, distance,
parallel, perpendicular, angle, rack-pinion, screw, gears, belt, and grounded families are typed;
an unfamiliar future family remains explicitly native. Connector frames, connector attachment
offset frames, suppression and detach flags, linear offsets, and enabled limit intervals are
independent fields.
Persisted degree values convert to radians for neutral angles and angular limits. Validation checks
operand/frame cardinality, component references, finite values, and ordered intervals.

## 10. Drawing graph

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
independent-body semantics. A profile uses one scalar `PropertyLink` or `PropertyLinkSub` carrier
from the `Profile`, `Sketch`, `Base`, or `Source` compatibility names. A link-list carrier,
multiple targets, or more than one populated compatibility name is not resolved by source order;
a linkless profile remains the native profile property. Object dependency links establish
construction dependencies, and a feature's cached shape property links its neutral operation to
every transferred result body from that payload. PartDesign body containers are structural
history nodes: the current `Group` or legacy `Model` is one `App::PropertyLinkList` membership
carrier, and both aliases are malformed. Its scalar `App::PropertyLink` `Tip` has at most one
local target; a link-list runtime type, multiple targets, or an unresolved non-null target retains
the body natively instead of selecting a source-order value. A valid tip identifies one owned
member as the active result. Suppressed, active,
frozen, invalid, touched, mapping, support, and visibility properties remain individually named
state rather than being collapsed into one enabled flag. Validation rejects duplicate members,
inconsistent parentage, missing members, and an active tip outside the body's ordered membership.

Revolution and groove operations retain their linked profile, explicit base point and axis,
one-angle or two-angle extent, and additive or subtractive effect. Fillet operations retain a
constant radius, and chamfers distinguish equal-distance, two-distance, and distance-angle laws.
Dress-up dispatch recognizes exactly `Part::Fillet`, `PartDesign::Fillet`, `Part::Chamfer`, and
`PartDesign::Chamfer`. Other runtime names remain native operations.
These operation dimensions participate in the same literal/evaluated/expression parameter graph.
When a dress-up subelement selector has not resolved through persistent topology identity, its
native `Base` property remains the edge selection; the decoder does not infer an edge from a
transient label.

Decode loss reporting is attributable at the native record boundary. Each design operation or
sketch geometry family that remains only in the native lane produces its own blocking note carrying
the object or property identity and `Document.xml` provenance. Successfully neutralized geometry
does not inherit a format-wide placeholder loss.

## 11. Presentation and application records

Format-neutral document and view presentation arenas represent GUI state. A GUI archive produces
one document presentation record; a headless archive produces none. The GUI root accepts the
canonical `SchemaVersion` attribute only; the lowercase alias is invalid. The neutral document record
contains the schema version, one camera, ordered document state, and resolved display-asset
references. GUI schema 1 has exactly one direct `Camera` element. Its `settings` attribute is the
serialized camera state. GUI schema 1 does not serialize an active view; an `active` root attribute
or an `ActiveView` element remains source state and does not set the neutral active view. A decoded
camera position and orientation are optional derived fields and must be finite and nonzero when
present. Each view-provider record contains
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

Core GUI properties use the application property grammar. `App::PropertyBool` contains `Bool`.
Enumeration, integer, integer-constraint, and percent properties contain `Integer`. Angle,
distance, float, float-constraint, and length properties contain `Float`. File, font,
persistent-object, and string properties contain `String`. Color, color-list, material,
material-list, vector, bool-list, and Python-object properties contain `PropertyColor`,
`ColorList`, `PropertyMaterial`, `MaterialList`, `PropertyVector`, `BoolList`, and `Python`,
respectively. Each registered property contains exactly one value root. Scalar values use the
`value` attribute. Vectors use `valueX`, `valueY`, and `valueZ`. Color-list and material-list
values use one `file` attribute. Material values use four packed-color attributes plus finite `shininess` and
`transparency` scalars. Boolean lists contain only `0` and `1`. Numeric values are finite, and
registered tags and attributes are mandatory. The GUI property registry is the same exact runtime
type registry used for application properties; GUI persistence does not introduce a second value
grammar. A registered property family remains typed even when no neutral presentation field uses
it. An unregistered GUI runtime type retains its exact ordered XML values without semantic
dispatch, and its XML span is `named_opaque` in the logical ledger.

A color-list side entry contains a little-endian `u32` count followed by that many little-endian
packed `u32` colors. A material-list value has a format version from zero through three. Versions
zero and one begin with a signed 32-bit count. A negative value is a provisional-version marker
followed by the unsigned 32-bit count. Version two begins with the unsigned count. Each material
then contains four packed `u32` colors followed by float32 shininess and transparency. Version
three uses the version-two header and material records, followed by three length-prefixed byte
strings for each material in image, image-path, UUID order. Each string has a little-endian `u32`
byte count and UTF-8 bytes. Counts are bounded by the remaining payload. Truncated records,
non-finite scalars, invalid UTF-8, and trailing bytes are invalid. Producing versions before 1.1
store the low color byte with the inverse alpha convention; readers invert that byte before
presentation transfer.

Neutral presentation dispatch requires both the exact property name and runtime type.
`Visibility` is `App::PropertyBool`; `DisplayMode` and `SelectionStyle` are
`App::PropertyEnumeration`; `Transparency` is `App::PropertyPercent`; shape, line, and point colors
are `App::PropertyColor`; shape material is `App::PropertyMaterial`; face, line, and point color
arrays are `App::PropertyColorList`; `ShapeAppearance` is `App::PropertyMaterialList`; and line
width and point size are `App::PropertyFloatConstraint`. A same-named property of another runtime
type remains native and does not populate the neutral field.

For shape-bearing objects, the view provider's shape color, transparency, visibility, and material
scalars describe the application object's exact-shape property named `Shape`. They produce an
object appearance and explicit bindings only for bodies transferred from that property. Other
exact-shape properties on the same object do not inherit this view-provider state. Packed colors
decode as red, green, blue, and reserved low byte; the independent transparency percentage
determines opacity. The effective body display fields mirror this object-level assignment.
`ShapeAppearance` is the current shape material carrier. One material replaces the legacy shape
color assignment, binds to every body transferred from `Shape`, and supplies diffuse color,
transparency, four packed material colors, shininess, and UUID. Multiple materials bind in order to
the persistent Face element-map group only when their count equals the group's indexed face count.
If persistent face identity is absent or the counts differ, the material list remains native and
the legacy object color remains the neutral fallback.
Topology-color and multi-material association requires one unambiguous application property named
`Shape`, at most one exact-shape payload for that property, and at most one element-map record for
that payload. The final `ElementMap2` node owns the shape. A requested `Face`, `Edge`, or `Vertex`
group occurs at most once in that node. Duplicate association candidates are malformed. Missing
association candidates retain the native side entry or material list without a source-order choice.
Per-face `DiffuseColor`, per-edge `LineColorArray`, and per-vertex `PointColorArray` lists are
higher-precedence presentation layers. They are not inferred from the corresponding object color.
Each list contains a little-endian count followed by packed-color records. A count of one applies
its color to every
member of the corresponding Face, Edge, or Vertex element-map group. Otherwise, the count must
equal the number of names in that ordered group. The group comes only from the element map owned by
the `Shape` property. Each persistent element name supplies the neutral topology occurrences that
receive the override. The resulting bindings explicitly record
face-over-object, edge-array-over-line, or vertex-array-over-point precedence. Missing identity or
a count mismatch leaves the side entry retained without guessing transient topology labels.

Application data without a neutral representation retains its owning object and property,
declared application type, links, source order, XML bytes, referenced side-entry bytes, byte spans,
lengths, and digests.

A side entry has field framing and value semantics only when an exact registered runtime property
type and value tag select that grammar. A file name, extension, value-tag spelling, byte prefix, or
payload signature does not select an application grammar. Without the registered property
discriminator, the complete side entry is one named opaque payload owned by its archive entry and
referenced by the declaring property. This registry boundary is the complete generic codec
contract: the decoder does not infer fields, record boundaries, or neutral values from an
unregistered payload. This rule permits extension properties to remain byte-exact without
confusing coincidental payload bytes with a core mesh, point, shape, list, or asset grammar.
`EntryRecord.referenced_by` retains each distinct referring property, GUI property, or GUI state in
serialized traversal order; the logical span remains single-owner and is never duplicated for
shared references.

`Mesh::PropertyMeshKernel` contains one `Mesh` value. The value has zero or one non-empty `file`
attribute. A non-empty attribute identifies the property's only binary side entry. A `Mesh` value
without a side entry contains inline XML mesh data and remains in the native property record. The
current typed binary record begins with the
32-bit magic `a0b0c0d0`, the 32-bit version `00010000`, and a 256-byte information field. Both
integer byte orders are accepted when the magic and version agree. Two 32-bit counts precede
ordered float32 XYZ points and facets. Each facet contains three zero-based point indices followed
by three stored neighbour indices. Six float32 bounding-box limits close the record. Counts are
bounded, point indices must resolve, coordinates and bounds must be finite, and trailing or
truncated bytes are invalid. Neighbour indices and the complete entry bytes remain native even
when only the indexed triangle mesh is projected neutrally.

`Points::PropertyPointKernel` contains one `Points` value. Its zero or one non-empty `file` attribute
identifies the property's only side entry. The entry contains a little-endian 32-bit point count
followed by ordered float32 XYZ triples. The `Points` value carries the sixteen finite row-major
transform scalars. Neutral points are transformed once into model space and retain the
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

The format-neutral drawing arena contains pages, templates, model views, projection groups,
sections, details, dimensions, annotations, balloons, symbols, leaders, images, and registered
extension drawing records. Records retain runtime classification and source order. Local drawing
relationships resolve to neutral drawing identities, model sources resolve to their local object
identities, and external document/object pairs remain explicit without being treated as local
references. A runtime type outside the registry remains in the native object and property records
and does not enter the drawing arena.

Core drawing dispatch uses exact runtime names. `TechDraw::DrawPage` is a page;
`DrawSVGTemplate`, `DrawDXFTemplate`, and `DrawParametricTemplate` are templates;
`DrawViewPart`, `DrawViewSpreadsheet`, `DrawViewClip`, `DrawViewMulti`, `DrawBrokenView`,
`DrawViewArch`, and `DrawViewDraft` are model views; `DrawProjGroup` and `DrawProjGroupItem` are
projections; `DrawViewSection` and `DrawComplexSection` are sections; `DrawViewDetail` is a detail;
`DrawViewDimension`, `DrawViewDimExtent`, and `LandmarkDimension` are dimensions;
`DrawViewAnnotation`, `DrawViewAnnotationPython`, `DrawRichAnno`, and `DrawRichAnnoPython` are
annotations; `DrawViewBalloon` is a balloon; `DrawLeaderLine` and `DrawLeaderLinePython` are
leaders; `DrawViewSymbol`, `DrawViewSymbolPython`, `DrawWeldSymbol`, and `DrawWeldSymbolPython`
are symbols; and `DrawViewImage` is an image. `DrawView`, `DrawViewCollection`, `DrawHatch`,
`DrawGeomHatch`, `DrawTile`, and `DrawTileWeld` are registered extension records with kind `other`.
Python variants of the registered classes use the same kind. Drawing property names and registered
value grammars provide carrier cardinality; projection does not select a carrier by source-order
precedence. A page `Template` property is one `App::PropertyLink` with at most one target. A page
`Views` property is one `App::PropertyLinkList` whose targets retain serialized order. Other runtime
types do not supply page template or page-view carriers.

For a page, the persisted `Template` link is represented as an optional local template identity only
when its nonempty target resolves to a registered template drawing. Its typed relationship retains an
explicit null, external target, or non-drawing target. The persisted `Views` link list is represented
by an ordered typed relationship, including null, external, and non-drawing targets.

The format-neutral semantic-annotation arena maps an exact core runtime-type registry. Text records
are `App::Annotation`, `App::AnnotationLabel`, `TechDraw::DrawViewAnnotation`,
`TechDraw::DrawViewAnnotationPython`, `TechDraw::DrawRichAnno`, and
`TechDraw::DrawRichAnnoPython`. Dimension records are `TechDraw::DrawViewDimension`,
`TechDraw::DrawViewDimExtent`, and `TechDraw::LandmarkDimension`. Balloon records are
`TechDraw::DrawViewBalloon`. Leader records are `TechDraw::DrawLeaderLine` and
`TechDraw::DrawLeaderLinePython`. Symbol records are `TechDraw::DrawViewSymbol`,
`TechDraw::DrawViewSymbolPython`, `TechDraw::DrawWeldSymbol`, and
`TechDraw::DrawWeldSymbolPython`. Other runtime types remain application objects and do not enter
the semantic-annotation arena. The core registry has no semantic datum or geometric-tolerance
runtime type.

`TechDraw::DrawViewSpreadsheet` is a model view, not a semantic annotation. `DrawViewArch` and
`DrawViewDraft` are model views with no semantic annotation kind. `DrawViewCollection` is a drawing
extension record with no semantic annotation kind. Datums and geometric-tolerance application
objects likewise remain native application records.

`App::Annotation` uses `App::PropertyStringList` `LabelText` and `App::PropertyVector` `Position`.
`App::AnnotationLabel` uses `App::PropertyStringList` `LabelText` and `App::PropertyVector`
`TextPosition`. TechDraw text, dimension, balloon, leader, and symbol records use the inherited `X`
and `Y` pair as their optional position; one coordinate without the other is invalid. TechDraw
annotation text uses `Text`, rich annotation text uses `AnnoText`, balloon text uses `Text`, weld
symbol text uses `TailText`, and dimension display text and format use `FormatSpec`. A persisted
TechDraw dimension has no scalar measurement property; its measurement is computed from its
references, so decode does not select `Value`, `Measurement`, `Distance`, or `Angle` by name.
Each registered scalar, vector, position, or format carrier has at most one property with that name
and exactly one root value when present. Duplicate named carriers or duplicate root values are
invalid. Text-list carriers retain all ordered text values; this cardinality rule applies only to
scalar, vector, and format carriers. An `App::PropertyEnumeration` has one selected `Integer`
carrier; its optional `CustomEnumList` is metadata and does not count as another selected value.
`X` and `Y` use `App::PropertyDistance`; historical
`App::PropertyLength` and `App::PropertyFloat` forms are accepted. `Scale` uses
`App::PropertyFloatConstraint` with the historical `App::PropertyFloat` form, and `Rotation` uses
`App::PropertyAngle` with the historical `App::PropertyFloat` form. `Direction` and `XDirection`
use `App::PropertyVector`; captions and format strings use `App::PropertyString`; scale, measure,
dimension, and projection modes use `App::PropertyEnumeration`; and lock or perspective flags use
`App::PropertyBool`. `XSource` uses `App::PropertyXLinkList`; `Sources` uses
`App::PropertyLinkList`; and `References2D`, `References3D`, and `Source3d` use
`App::PropertyLinkSubList`. `Source` uses the registered scalar or list link-family carrier for
its runtime class, including explicit legacy link-family forms. A scalar `Source` carrier has at
most one target. Wrong runtime types and conflicting carriers are invalid. Position and rotation
values are finite, scale is positive, and a direction is finite and nonzero before neutral
transfer.
Records retain source order, exact runtime classification, role-grouped references, subelement
selectors, fallback parameters, and resolved assets. Local drawing targets resolve to neutral
drawing identities; external document/object pairs remain explicit.

Persisted empty drawing and annotation links are explicit. A target whose
native link record is present but names no document object has `is_null: true`; it is distinct
from an absent target and from an unresolved nonempty reference. Local referential validation
therefore accepts only explicitly null empty targets and continues to reject every nonempty
missing object identity.

Native namespace version 10 adds a `gui_documents` arena. A GUI archive has exactly one document
record; a headless archive has none. The record retains the GUI schema and root attributes plus
every document-level element outside `ViewProviderData` in source order. These named state records
cover the camera, unrecognized active-view data, clipping or section state, and future GUI state
without treating it as an application-object property. Each retains its exact XML span, ordered
descendant values, and display-asset references.

Logical byte accounting consumes the records emitted by each bounded parser. Exact-shape,
side-entry string-table, and side-entry element-map payloads are wholly typed after successful
framing. `Document.xml` properties and `GuiDocument.xml` state/property spans are typed while the
intervening XML syntax is structural. Each side-entry span is owned by its archive entry record.
The entry's ordered `referenced_by` relation independently retains every application property, GUI
property, or GUI state that references the bytes. Uninterpreted embedded assets remain named
opaque. These claims are sorted and rejected on overlap before the ledger is emitted; validation
then requires every logical entry to close without gaps.

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

Native namespace version 22 separates side-entry byte ownership from semantic references. A
logical side-entry span has its `EntryRecord` as its single owner. `EntryRecord.referenced_by` is
the ordered many-reference relation and can contain more than one property or GUI record.

Native namespace version 11 adds attachment records. Support links retain ordered object and
subelement identity separately from the map mode. The persisted resolved `Placement` and local
`AttachmentOffset` remain distinct matrices. Neutral geometry composes them as
`Placement × AttachmentOffset` when both are present, and uses the sole present matrix otherwise.
Validation checks support identity, finite matrices, and this effective-frame rule. Each named attachment
carrier occurs at most once. `MapMode` has at most one text value, and each placement carrier is an
`App::PropertyPlacement` property with at most one `PropertyPlacement` value containing finite
position and quaternion or axis-angle components.
Duplicate carriers or values are malformed.

Native namespace version 12 adds one carrier-census record per exact-shape payload. Census records
identify text versus binary framing, the declared topology version, recursive carrier-family
counts, all eight topology families, and polygon and triangulation counts.

Sketch point, line, circle, circular-arc, ellipse, and elliptical-arc carriers transfer only when
all family-required numeric fields are present and finite. Ellipse orientation may be carried as a
major-axis angle or a two-component major-axis direction. Bounded ellipses additionally require
both parameter bounds. A circular arc's `AngleXU` rotates its reference X axis
counterclockwise in the sketch plane. Its global start and end angles are `StartAngle + AngleXU`
and `EndAngle + AngleXU`; an absent `AngleXU` is zero. Missing radii, coordinates, orientation, or
bounds leave the carrier as a named native geometry record; the decoder does not synthesize zero
coordinates or full-curve bounds.

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

Primitive dispatch recognizes exactly `Part::Box`, `Part::Cylinder`, `Part::Cone`, `Part::Sphere`,
`Part::Ellipsoid`, `Part::Torus`, `Part::Prism`, and `Part::Wedge`. For each of `Box`, `Cylinder`,
`Cone`, `Sphere`, `Ellipsoid`, `Torus`, `Prism`, and `Wedge`, it also recognizes exactly the
`PartDesign::<Family>`, `PartDesign::Additive<Family>`, and `PartDesign::Subtractive<Family>` names.
These objects transfer as neutral analytic-solid primitives. Lengths are canonical model lengths
and persisted degree-valued angular bounds become radians. A standalone primitive creates a new
body; additive and subtractive families explicitly join or cut. Required dimensions must be finite,
linear sizes must be positive except that one cone end radius may be zero, and latitude bounds must
be ordered. Incomplete or invalid primitive definitions remain attributable native operations.

Part cut, fuse, common, multi-fuse, and multi-common objects transfer as neutral Boolean combine
operations. Two-input forms retain distinct `Base` and `Tool` property identities. Multi-input
forms define link zero as the target and the remaining ordered `Shapes` links as tools without
claiming that application-object links are already neutral body ids. For non-container features,
feature dependencies are the stable union of all declared object dependencies and earlier
link-property operands in source order. PartDesign body dependency records describe structural
membership and do not duplicate the body's neutral child relations. Body membership comes from the
current `Group` link list or the legacy `Model` link list. A declared dependency can
target a later declaration. Declared dependency identities are resolved before neutral ordinal
assignment; their source declaration order does not filter them. Neutral feature ordinals use a
stable dependency order and use source order as the tie-break rule. Forward profile, base-feature,
and pattern-seed links also precede
their consumers. Body child lists are structural membership, not body inputs. If the native graph
contains a dependency, parent, or expression cycle, the native graph retains it.
When ordinal assignment has no ready object, every remaining cycle-affected history object receives
a native feature definition and one blocking `feature.cyclic-history` loss. Its neutral dependency
list retains only edges whose targets precede the consumer; the native graph remains authoritative.

Design dispatch uses exact runtime names. `PartDesign::Pad`, `PartDesign::Pocket`, and
`Part::Extrusion` are extrusions; `PartDesign::Revolution`, `PartDesign::Groove`, and
`Part::Revolution` are revolutions; `PartDesign::Body` is a body container; and
`Spreadsheet::Sheet` is a spreadsheet. Substrings and vendor-qualified variants do not select
these families.

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

PartDesign ShapeBinder and SubShapeBinder operations retain their ordered support links and
subelement selectors. A SubShapeBinder `Context` property is optional and, when present, is one
`App::PropertyXLink` carrier with at most one link target. A duplicate `Context` carrier, another
runtime type, multiple link targets, or a subelement selector is malformed for this operation. An
admissible carrier with an unresolved target leaves the binder attributable and native; the
decoder does not select a target by source order.

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
Part scale and whole-shape offset operations retain one source link. Two-input Part booleans use
one `Base` link and one `Tool` link. A PartDesign Boolean uses one `BaseFeature` link when that
optional carrier is populated. A compound with an empty source list, or any missing, empty, or
multiply valued single-source link, remains an attributable native operation. These operations do
not select a target by link-list order.

Sweep spine and auxiliary-spine carriers each contain one link when the corresponding path is
required. A selected extrusion or revolution face or shape termination contains one link. A
helical PartDesign operation requires its profile carrier; an absent profile remains native.

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
instead a derived-geometry operation whose input is the earlier linked feature. Its `BaseFeature`
carrier is one `App::PropertyLink` with exactly one target. Another runtime type or multiple targets
leave the feature attributable and native; duplicate named carriers are malformed. The decoder does
not select a target by source order. Application-owned feature subclasses remain in the complete
native object/property graph and are not misclassified as built-in modeling operations solely because
their type derives from a core feature class. Legacy spline, extended-feature, geometry-set, and
planar-feature containers likewise represent direct stored geometry when they carry no replay
construction. STEP, IGES, B-rep, and curve-network
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
directions, and non-positive intervals leave the operation attributable and native. Axis and plane
references use one scalar `PropertyLink` or `PropertyLinkSub` carrier, including their scalar
runtime variants, with one target and at most one subelement selector. Link-list carriers,
multiple targets, and multiple selectors are not resolved by source order; required references
leave the operation native and optional references remain unresolved.

Part extrusions retain their normalized direction, custom-vector, selected-edge, or profile-normal
direction source, independent forward and reverse lengths and tapers, symmetric construction, and
solid-versus-sheet result. Solid construction additionally retains the extensible face-maker class
and mode and whether inner wires taper with or against outer wires. An absent or zero pair of
explicit lengths uses the persisted direction-vector magnitude. PartDesign pads and pockets distinguish blind,
through-all, first-intersection, last-intersection, face-selected, and shape-selected termination
independently on both sides. Midplane construction mirrors either a length or a non-length
termination, while signed blind lengths preserve the persisted side orientation. Features retain
both taper angles and offsets, whether length follows the profile normal, and whether multiple
profile faces are allowed. Direction provenance distinguishes the profile normal, an explicit
custom vector, and a selected reference axis while also retaining the normalized resolved
direction. A non-sketch planar profile supplies its profile normal from its persisted placement
frame. Reversal inverts the direction. A pad joins and a pocket cuts. Missing required lengths
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
