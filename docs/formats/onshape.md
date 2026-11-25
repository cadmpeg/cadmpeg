# Onshape offline bundle

## 1. Envelope and schema

The `.onshape-bundle` extension identifies a ZIP archive with a local file
header at byte zero. ZIP framing follows [ZIP](zip.md). The archive uses
one disk and a complete central directory. Members use stored or DEFLATE
compression. JSON members carry UTF-8 text. Each member name is unique. A name uses
forward slashes between nonempty components. Components exclude `.`, `..`,
colons and backslashes. Every member names a file.

The root member `manifest.json` is a JSON object. Its `schema` string is
`dutchman.onshape-bundle/1`. The integer after `/` selects the bundle schema.
A schema selects the manifest and member semantics together.

The optional `consistent` field is Boolean. `true` identifies a pinned
snapshot; `false` identifies mixed snapshots. Omission selects the version 1
legacy consistency convention. A consistent bundle has one document
microversion across all responses that declare a microversion.

## 2. Manifest and members

`document.microversion` is a nonblank string. `elements` is an array of
element records. Each record has a `type` and a nonblank `id`.
`PARTSTUDIO` identifies a Part Studio. `ASSEMBLY` identifies an assembly.
A single-Part-Studio bundle selects its sole element. For assembly selection,
`root_element` accepts an element-ID string or an object with a string `id`.
Omission and `null` select the first `ASSEMBLY` element in source order.
An element ID declared more than once selects its first declaration.
The ID consists of ASCII letters, digits, underscores and hyphens. A Part
Studio ID forms the member prefix `partstudios/<id>`. An assembly ID forms
`assemblies/<id>`. `external` entries store `documentId`, `elementId`, and
`microversionId` for references into other documents.

| Member | Value |
| --- | --- |
| `manifest.json` | Bundle identity and pinned document snapshot |
| `assemblies/<id>/definition.json` | Assembly definition response |
| `assemblies/<id>/features.json` | Assembly feature response |
| `partstudios/<id>/features.json` | Ordered Part Studio feature response |
| `partstudios/<id>/bodydetails/index.json` | Rollback ordinals with body responses |
| `partstudios/<id>/bodydetails/<ordinal>.json` | Input topology at that rollback |
| `partstudios/<id>/export.step` | Final geometry in STEP Part 21 UTF-8 text |
| `partstudios/<id>/configuration.json` | Configuration response |
| `partstudios/<id>/variables.json` | Variable response |

The STEP member can be absent for a nonempty feature list composed of
sketches, variable assignments and suppressed features. Other feature lists
require the STEP member. Configuration and variable response members are
optional. Additional members carry opaque data.

The manifest can carry document identity, names, fetch time, API version,
fetcher identity, request counts, member path labels and configuration
selection text. The canonical member paths above bind the Part Studio.

## 3. Features and consistency

The feature response has a `features` array in timeline order and can have
a `defaultFeatures` array. Each feature has a unique nonblank `featureId`.
`featureType` selects the operation. `name` carries its display label.
`parameters` is an array with unique `parameterId` keys; `btType` names each
REST record class. `suppressed` and `parentSuppressed` are Boolean fields
that mark inactive features. `rollbackIndex` is the number of features before the
rollback bar, from zero to the length of `features`. The bar stands before
the feature at that position, or after the last feature at the length.

A declared `sourceMicroversion` equals `document.microversion`. A
microversion declaration is either the string or an object with string
`theId`. The same rule applies to `microversionId` in rollback responses
and to both declaration fields in configuration and variable responses.
Every response declaration uses the pinned snapshot.

The rollback index is an array, or an object with that array in `indices`.
An entry is a nonnegative integer or a decimal filename `<ordinal>.json`.
Ordinal values are unique across both forms. Each ordinal identifies the topology before the
feature at that position. Ordinal `n + 1` identifies the state after feature
`n`. An indexed response contains a `bodies` array. Body, face, edge and
vertex IDs bind selections within that state. Body properties can carry a
part name. A created part binds a final body through a unique name claim
between adjacent rollback states.

Final STEP topology describes final geometry. Historical selections use
the rollback response for their consuming feature. A deterministic ID must
resolve uniquely in that response. Tangent chains follow the response's
vertex connectivity and edge tangents. A next-face termination requires a
common first intersection across the complete selected profile.

## 4. Sketches, quantities and selections

Sketch entity coordinates and body-detail coordinates use metres. Angles
use radians. Quantity expressions carry their own unit suffixes. A line
uses an origin, direction and source parameter interval. Circular and
elliptical curves use their centre, axes and angular interval. Spline
records carry their degree, controls and knot data. Omitted coordinate
fields have value zero. Omitted constraint and optional entity arrays have
value empty. Present fields keep their declared JSON types.

Sketch plane queries can name built-in planes, created geometry or a
historical face. A built-in plane has a conventional frame. A selected
region carries connected, closed, directed boundary loops. A hole lies
inside its outer loop. Boundary interiors are disjoint. Directed sides
select the material side of each boundary.
Region selections can restrict the parameter interval of an exact curve.
Loop closure follows stored curve endpoints and parameter intervals. Analytic
intersection and containment of closed boundaries establish hole nesting.
Directed carrier turns establish loop sense.

A `BTMIndividualSketchRegionQuery-140` whose `queryString` is
`query = qSketchRegion(id + "<featureId>", <filterInnerLoops>);` selects
the regions of that sketch named by the query string; its
`deterministicIds` do not restrict that selection. With `filterInnerLoops`
equal to `false` the selection is every region of the sketch. With `true`
it excludes each region fully contained in another region. A selected region retains the loops directly inside it as hole
boundaries.

A sketch's optional `plane` object stores its attachment frame and is the
frame of that sketch whenever it is present. `origin` uses
metres; `normal` and `x` store its normal and x axis. The axes are orthogonal.
A referenced planar face and the stored attachment frame are coplanar. The
stored origin retains tangential translation. The stored x axis retains
in-plane rotation. The normal carries the attachment sense.
A sketch without a `plane` object on a created datum plane takes its frame
from its `sketchMatrix` row, which is coplanar with that datum plane.
The top-level `sketches` array in `sketches.json` contains sketch rows.
Each row identifies its sketch through `featureId`.
Its `sketchMatrix` stores a row-major homogeneous 4 by 4 transform.
The first three columns are the sketch x axis, y axis and normal.
The fourth column stores the origin in metres; the last row is `[0, 0, 0, 1]`.
The rotation is right-handed and orthonormal.
A sketch placed on a planar face stores its attachment frame in the same
`sketchMatrix` when the feature has no `plane` object. The matrix origin lies
on the face surface plane and the matrix normal is collinear with the surface
normal; the matrix stores the sign of the normal. The origin retains
tangential translation, the x axis retains in-plane rotation and the normal
carries the attachment sense.

A `cPlane` feature with `cplaneType` equal to `OFFSET` selects its support
through `entities`. A single default plane or created datum plane defines
the resolved support. `offset` is a length quantity. With
`oppositeDirection`, `flipNormal` and `flipAlignment` equal to `false`,
for support origin `o`, unit normal `n`
and offset `d`, the constructed plane contains `o + d * n` and retains
normal `n`.
The support frame and signed distance define the datum offset plane.
A sketch-plane creation query with operation suffix `.planeOp` selects the
created datum plane. The sketch's stored attachment frame remains coplanar
with that datum plane.

A `sweep` feature stores solid cross-section selections in `profiles` and
trajectory selections in `path`. `bodyType: "SOLID"` selects a solid result.
`operationType: "NEW"` creates a body; `ADD` joins the swept volume to the
merge scope. `profileControl` has values `NONE`, `KEEP_ORIENTATION`,
`LOCK_FACES`, and `LOCK_DIRECTION`. `lockFaces` and `lockDirectionQuery`
store query lists. `hasTwist` and `hasScale` enable twist and scale controls.
`turns`, `angle`, and `pitch` store twist quantities; `scaleFactor` stores
the scale quantity. `trimEnds` stores a Boolean end-trim control.
`pathOppositeDirection` reverses path travel; `extendToFullPath` extends
travel to the full path.
A sketch-edge path query has `queryType: "SKETCH_ENTITY"`,
`historyType: "CREATION"`, `entityType: "EDGE"`, one `operationId` with the
suffix `.wireOp`, and a `sketchEntityId`. The operation prefix identifies
the source sketch. A `UNION` query stores its ordered operands in
`subqueries`; nested unions preserve operand order.

Thin extrusions select curves through `wallShape` in the `MODEL` domain.
Explicit `midplane`, `flipWall` and `oppositeDirection` fields establish wall
placement. `midplane` selects a symmetric `thickness`. Otherwise,
`thickness1` lies on the directed curve's right and `thickness2` on its left;
`flipWall` exchanges the sides. A line with an increasing parameter interval
is directed along its stored direction. The direction of a line with
`endParam < startParam` is open (ONSHAPE-007). A closed circular or elliptical carrier's
`clockwise` field establishes its sense. The `MODEL` domain binds the
feature to its Part Studio model.

A single planar target with a closed convex line boundary admits an exact
`UP_TO_NEXT` intersection for a closed convex line profile. Each positive
ray-plane intersection lies within all target boundary half-planes.
Convexity extends coverage across the complete profile. The target plane,
line carriers, endpoints and coedge directions are stored in the rollback.

A present `booleanScope` or `scope` parameter has a `queries` array. A nonempty
scope list identifies target bodies at the consuming rollback. An empty
list retains the implicit scope. `endStyle` selects the bore termination.
`isTappedThrough` selects thread termination: `true` uses the bore depth;
`false` uses the finite `tappedDepth` quantity.

Explicit feature and operation provenance refers to the producer of the
selected geometry revision. Rollback transitions establish that revision's
producer interval. The final consumed selection includes edges reached by
tangent propagation. Compact fields and outer fields describe the same
operand; outer deterministic IDs remain part of that operand.

A query can carry explicit deterministic IDs or a literal
`query=qCompressed(1.0,<JSON string>,id);`. `%` starts the compact value
stream. `&<hex length>$<base64>` carries a zlib-compressed compact stream;
the length counts expanded UTF-8 bytes. The stream uses lowercase hexadecimal integer
fields and uppercase tags. A signed field can start with `-`.

| Tag | Payload | Meaning |
| --- | --- | --- |
| `S` | `<first>[.<second>]$<bytes>` | One string or a dotted pair of fragments |
| `D` | Signed hexadecimal integer | Integer value |
| `T`, `F` | Empty | Boolean value |
| `A` | Count followed by values | Ordered array |
| `M` | Count followed by key/value pairs | Map with unique string keys |
| `B` | Encoded type-name string, then value | Typed value and interned type name |
| `C` | Type-name dictionary index, then value | Typed value with a reused name |
| `R` | Value dictionary index | Reused completed value |

For a string fragment, a nonnegative size counts literal UTF-8 bytes after
`$`. A negative size references the string dictionary at the size's
absolute value. Two sizes select two fragments, joined with `.`. Each
literal fragment enters the string dictionary. A dotted result also enters
that dictionary. Type names share the string dictionary. Admitted type
names are `Query`, `QueryType`, `EntityType` and `Id`.

Each completed scalar, array or map enters the value dictionary after its
children. A reference reuses that stored value and charges its expanded
size. Typed wrappers preserve the enclosed value. The complete stream
contains one value. Query text can omit the `query=` prefix and can end
with semicolons.

Quantity expressions support bounded arithmetic, parentheses, unit suffixes
and references to earlier variable assignments. Variable dependencies
name the bindings in scope at the consuming feature. Expressions represent
literal computation. A numeric quantity with a supported explicit `units`
string uses that unit. A feature dependency identifies a consumed earlier
feature in source operand order. Feature ordinals give timeline order.

## 5. Admission bounds

| Resource | Bound |
| --- | ---: |
| ZIP members | 4,096 |
| Central directory and ZIP64 end-record declared size | 16 MiB |
| One JSON member | 32 MiB |
| Sum of expanded member bytes | 256 MiB |
| Feature and topology array length; query node count | 10,000 |
| Query text and expanded query text | 1 MiB |
| Query cached values and strings | 4 MiB |
| Query and expression nesting | 64 |
| Expanded assembly instances; mate and parameter visits | 10,000 |
| Assembly path depth | 64 |
| Mate diagnostics per expanded assembly level | 16 |
| Mate diagnostics per bundle, plus aggregate and work-limit notes | 256 |
| Expression text | 65,536 bytes |
| Region recovery work | 1,000,000 |
| Region boundary output | 20,000 |
| Serialized region boundaries | 8 MiB |

Each bound applies before the corresponding unbounded operation. A compact
reference also charges its expanded value size and depth.

## Assembly definitions

An assembly definition stores `rootAssembly`, `subAssemblies`, and `parts`.
`rootAssembly.instances` lists direct instances. Each instance stores `id`
and `type`. Its reference fields are `documentId`, `elementId`,
`documentMicroversion`, and `configuration`. Optional `name` and `suppressed`
fields state the instance name and suppression state.
A `Part` instance also stores `partId`. An `Assembly` instance refers to a
subassembly definition by document, element, microversion, and configuration.
`subAssemblies` holds the corresponding definition records and their direct
`instances`. `parts` holds part identities and names.

`rootAssembly.occurrences` stores placements. `path` is the ordered array of
instance IDs from the root. `transform` holds 16 row-major coefficients. The
matrix maps prototype coordinates to root assembly coordinates. Translation
uses metres. The bottom row is `[0, 0, 0, 1]`. `fixed` grounds the occurrence.
Nested placement is the inverse parent transform multiplied by the child
transform. An unresolved ancestor placement leaves descendant local transforms
unstated. `suppressed` holds an instance and its descendants out of the assembly.
Occurrence `hidden` is the inverse of its visibility state.

An evaluated assembly feature stores `id`, `featureType`, `suppressed`, and
`featureData`. `featureType: "mate"` selects a mate. Its `featureData` stores
`mateType` and two `matedEntities`. Each mated
entity stores `matedOccurrence`, an instance path relative to its assembly,
and `matedCS`, a connector frame. A direct part path gives the frame in the
part coordinates. `matedCS` stores
three-component `origin`, `xAxis`, `yAxis`, and `zAxis` arrays. The origin uses
metres. The axes are the columns of the connector rotation matrix.

| `mateType` | Joint family |
| --- | --- |
| `FASTENED` | Fixed |
| `REVOLUTE` | Revolute |
| `SLIDER` | Slider |
| `CYLINDRICAL` | Cylindrical |
| `PLANAR` | Plane coincidence |
| `BALL` | Ball |

The solved connector world frame is the occurrence transform multiplied by
the connector frame. The sign of the dot product of the two world z axes
gives aligned or opposed sense. A zero dot product leaves the sense unstated.

### Part identity and final geometry

A part binds to final Part Studio geometry when its document is local, its
microversion matches the pinned document microversion or is omitted, and its
configuration is omitted, empty, or `default`. Part and body names establish
a binding when the name is unique in both the Part Studio's part list and
its final bodies. A Part Studio with one part and one final body establishes
a binding independently of the name. A microversion equal to the pinned
document microversion names the same reference as an omitted microversion,
and a `default` configuration names the same reference as an omitted or empty
configuration. Two instances of the same part place the same product
definition. An `Assembly` instance resolves against the `subAssemblies`
record with the same reference. External reference entries supply an omitted
`documentMicroversion` through their `microversionId` when the document and
element identify one microversion.
