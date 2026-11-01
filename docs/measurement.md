# Measurement

`cadmpeg-measure` measures a decoded model in model space. This document
states the output contract: the value object, units, methods, exactness,
categories, the pinned reason and caveat vocabularies, regions, the pass
allowance and the reader duties. Algorithm details are in the crate's
rustdoc.

Measurement is an oracle on decoded output. A decoder or encoder never
selects a layout, carrier, sense, unit or interpretation by measuring
geometry. Only the `cadmpeg` command line package depends on
`cadmpeg-measure` outside `[dev-dependencies]`.

## Value object

Every quantity is one tagged object. A quantity that does not apply to an
entity is absent.

A measured value has these members, in this order:

| Member | Meaning |
| --- | --- |
| `status` | `"measured"`. |
| `value` | A finite number, `{x, y, z}` for a point or a direction, or `{xx, yy, zz, xy, yz, zx}` for a symmetric tensor. A range or a box has `min` and `max` in its place. |
| shape members | `angular_sweep`: `full` and `intervals`. `bbox`: `kind` (`tight` or `conservative`) and `boundary`, a lower bound of the tight box: the box of the boundary pieces of a face, the box of the lower bounds of the members of a union (see Boxes). |
| `unit` | `mm`, `mm2`, `mm3`, `mm5`, `kg`, `kg.mm2`, `rad` or `1`. The unit of the value and of both error terms. |
| `method` | How the value was obtained (below). |
| `numeric_error` | A non-negative a-posteriori estimate of the computation error. It is zero for `source` and `closed_form`. It is not a certified bound. |
| `model_uncertainty` | The non-negative first-order change of the value when the boundary moves by the larger of the declared tolerance, the measured boundary deviations and the bridged gaps, plus the placement term. The section Volume, centroid and closure states the terms for a body's volume, centroid and moments. |
| `provenance` | The source fields `{entity, field, exactness}` the value is read or computed from. Absent when empty. |
| `reading` | The reading the value is measured under, when the value has several readings. |
| `candidates` | The other readings: `{reading, value, unit, method, numeric_error, model_uncertainty}`. |

An unavailable value has `status` (`"unavailable"`), `reason`, `category`,
`entity` (optional), `message` and `candidates` (optional). `message` is
human text and not a contract.

No value is NaN or infinite. A computation that leaves the finite range makes
the value unavailable with `evaluation.non-finite`.

## Units

Lengths are in millimetres, areas in square millimetres, volumes in cubic
millimetres, second moments of volume in millimetres to the fifth power,
masses in kilograms, moments of inertia in kilogram square millimetres and
angles in radians. Directions and ratios have unit `1`.

## Methods

| Method | Meaning |
| --- | --- |
| `source` | A stored scalar, or a stored point or direction under the identity placement, exactly as stored. |
| `closed_form` | Exact arithmetic on stored values. |
| `boundary_quadrature` | Adaptive Gauss–Kronrod quadrature along boundary pieces. |
| `boundary_extremum` | Extremum search along boundary pieces only. |
| `critical_points` | Boundary extremes and the carrier's interior critical points in the region. |
| `control_hull` | A NURBS Bézier control-net bound. |
| `divergence_quadrature` | Volume, centroid, closure and mass properties from face fluxes. |
| `sum` | The sum of member values; errors add. |

## Exactness

`provenance[].exactness` is `byte_exact`, `derived`, `inferred` or `unknown`
as the decode sidecar records the field, or `not_recorded` for a CADIR
document without a sidecar. A field without a sidecar entry is
`byte_exact`.

## Categories

Every unavailable value and every caveat has one category. The set is
closed.

| Category | Meaning |
| --- | --- |
| `input` | The document's topology or geometry is missing, inconsistent, or contradicts itself for this quantity. |
| `undefined` | The quantity does not exist for the entity as the document states it, or the document carries no data for the fact. |
| `unsupported` | This build has no method for the quantity on this entity, or cannot confirm the reading it reports. |
| `budget` | A local work, sampling or quadrature cap, or the pass allowance, stopped the computation. |
| `placement` | The entity's composed placement prevents the quantity. |

A row's `status` is `measured` when every applicable quantity is measured,
`partial` when some are unavailable, and `unavailable` when none is
measured. Values of category `undefined` do not count against the status.

## Unavailable reasons

Each code is `family.detail`. A code is never renamed, removed, or given
another meaning or category. New codes can be added.
`aggregate.member-unavailable` takes the category of the member value it
cites.

| Code | Category | When |
| --- | --- | --- |
| `carrier.unknown` | input | The carrier is stored as an unknown shape. |
| `carrier.unresolved` | input | The referenced carrier ID does not resolve. |
| `carrier.procedural-unevaluated` | unsupported | A procedural carrier has no evaluator result and no usable cache for the value. |
| `carrier.polygonal` | unsupported | A polygonal carrier; bounded measures are not computed. |
| `carrier.composite-curve` | unsupported | A composite curve has no parameterization. |
| `carrier.elliptic-cone` | unsupported | Area or flux on a cone with an elliptic cross-section. |
| `placement.non-similarity` | placement | The composed placement is not a similarity; radii and angles are not invariant. |
| `placement.non-finite` | placement | Transform composition left the finite range. |
| `boundary.unbounded-face` | undefined | The face's region is unbounded. |
| `boundary.ring-chain-failed` | input | The ring's coedges do not form one closed traversal by oriented vertex identity, or an anchored pole use is not at the end of its coedge. |
| `boundary.ring-chain-ambiguous` | input | The ring is not continuous as listed and its coedges form more than one closed traversal. |
| `boundary.coedge-unresolved` | input | A coedge has no pcurve, no edge curve and no use-curve, and is not degenerate. |
| `boundary.edge-range-unresolved` | input | The 3D interval of an edge cannot be established. |
| `boundary.pcurve-interval-ambiguous` | input | Two candidate pcurve intervals qualify with different images and no 3D witness decides. |
| `boundary.pcurve-direction-ambiguous` | input | A closed pcurve-only coedge has no direction where its direction matters. |
| `boundary.pcurve-chain-broken` | input | Consecutive pcurve uses of one coedge do not join. |
| `boundary.pcurve-curve-mismatch` | input | The surface image of a coedge's pcurve does not follow its 3D curve once, in order, within the allowance. |
| `boundary.inversion-failed` | input | A boundary point does not invert onto the surface within the allowance. |
| `boundary.inversion-unsupported` | unsupported | A 3D-only coedge on a carrier with no inverse. |
| `boundary.not-closed` | input | Consecutive pieces do not join within the larger of the allowance and the sum of their edges' model tolerances. |
| `boundary.period-mismatch` | input | A loop's closing shift is not a whole number of periods. |
| `boundary.loop-degenerate` | input | A ring loop with zero net shift encloses no area relative to its own size. |
| `boundary.nesting-invalid` | input | The loops form no valid region for the domain. |
| `boundary.region-ambiguous` | input | The document admits two regions. |
| `region.domain-unsupported` | unsupported | The carrier's domain has no region rule in this build. |
| `region.orientation-conflict` | input | A witness contradicts the declared orientation. |
| `evaluation.no-value` | input | An evaluator has no value at a needed parameter. |
| `evaluation.non-finite` | input | An evaluation or a result left the finite range. |
| `evaluation.work-exhausted` | budget | The item's work slice, a sampling cap, or the pass allowance ran out. |
| `quadrature.not-converged` | budget | The quadrature error estimate exceeds the acceptance limit. |
| `volume.shell-open` | undefined | A shell is not closed, or one of its faces reaches a natural domain edge that no loop covers. |
| `volume.orientation-conflict` | input | A face's sense disagrees with its loop winding or its witness, or a shell's volume sign contradicts its role. |
| `volume.orientation-unverified` | unsupported | A declared-orientation face of the body is unconfirmed. |
| `volume.closure-flux` | input | The flux of the unit field exceeds the closure tolerance. |
| `aggregate.member-unavailable` | from the member | A sum or union has an unavailable member; `entity` names it. |
| `sweep.unsupported-carrier` | unsupported | Elliptic-cone sweeps and sweeps of a torus that is not a ring about its axis. |
| `bbox.interior-unbounded` | unsupported | No interior bound for the carrier. |
| `bbox.nonpositive-weights` | unsupported | The NURBS control hull needs positive weights. |
| `length.derivative-unavailable` | unsupported | A procedural curve without a solved cache has no derivative evaluator. |
| `placement.carrier-shared` | placement | A surface row's surface is used by faces of bodies with different placements. |
| `mass.principal-axes-degenerate` | undefined | A principal moment agrees with another within their separation, so its principal axis is not determined. |
| `placement.occurrence-scale` | placement | The placement of an occurrence of the `--occurrence` chain or of its subtree is not rigid: its linear part has a scale other than `(1, 1, 1)`. |
| `placement.occurrence-unplaced` | placement | An occurrence of the `--occurrence` chain or of its subtree states no placement. |
| `placement.prototype-unresolved` | placement | The prototype of the `--occurrence` occurrence or of an occurrence of its subtree is external or unresolved, or its definition does not resolve. |
| `placement.occurrence-chain-broken` | input | The `--occurrence` parent chain does not reach a root occurrence: a parent is missing or the chain repeats. |
| `placement.occurrence-body-repeated` | placement | Two occurrences of the `--occurrence` subtree place the same body; a pass has one row per body. |
| `placement.occurrence-empty` | undefined | Neither the `--occurrence` prototype nor a prototype of its subtree lists a body. |
| `boundary.period-unresolved` | input | A loop's parameter values are so large that a shift by one period does not change them. |

## Caveats

A caveat marks a measured result that carries a fact a reader should see.
Caveat codes follow the reason discipline.

| Code | Category | Level | When |
| --- | --- | --- | --- |
| `boundary.ring-order` | input | face | A ring's listed coedge order is not its traversal order; `entity` is the loop. |
| `boundary.pcurve-order` | input | face | A coedge's pcurve uses form their chain from its end to its start; `entity` is the coedge. |
| `orientation.unconfirmed` | unsupported | face | The face's region and normal rest on its declared orientation and no witness confirms it. |
| `measure.provenance-not-recorded` | undefined | document | Neutral CADIR without a decode sidecar; every exactness is `not_recorded`. |
| `measure.geometry-not-transferred` | input | document | The decode report states that no geometry was transferred. |

## Comparison causes

A comparison cause states why an ID-free comparison is inconclusive or why
one of its rows is not compared. Cause codes follow the reason discipline.

| Code | When |
| --- | --- |
| `compare.occurrence-placement` | A document's occurrences place bodies non-trivially. |
| `compare.unavailable` | A quantity is unavailable for a `budget`, `unsupported` or `placement` reason on at least one side and has no comparable candidates. |
| `compare.ambiguous-class` | Bodies whose compatibility is not transitive. |
| `compare.unmeasured-body` | A body with no comparable quantity. |
| `compare.nothing-compared` | No numeric or signature row is compared. |
| `compare.no-lower-bound` | Neither box of a `containment` row has a lower bound. |
| `compare.signature-unplaced` | A signature pair has no vertex on either side, so no compared row depends on the bodies' positions. |

## Rings

A ring lists its coedges in traversal order. When the listed order is not
continuous by oriented vertex identity, the measurement takes the traversal
from topology: the unique closed traversal that uses every coedge once. The
loop entry's `listed_order` is `traversal`, `reversed` or `other`, and a
value other than `traversal` adds the face caveat `boundary.ring-order`. Area
and flux are sums of per-piece integrals, so the listed order changes no
value once the traversal is known.

## Boundaries

Each coedge takes its parameter-plane image from the first of these that
applies:

1. A pole segment, when the coedge is degenerate and its vertex lies at a
   singular point of the carrier.
2. Its pcurve uses.
3. Its 3D curve interval, inverted onto the carrier.

A single pcurve use takes the first candidate interval whose mapped ends lie
within the allowance of the coedge's ends: the use's stored range, the
pcurve's stored range, its trim range, the edge interval and its negation,
the pcurve's domain, and then an interval recovered by Newton steps. A
closed coedge takes its direction from the tangent of its 3D curve at the
traversal start: the interval read forward leaves the vertex along the image
tangent at its start, read backward it leaves against the image tangent at
its end, and the reading whose departure has the larger component along the
3D tangent applies. With no 3D tangent, or with equal components, the use's
stored range gives the direction. Several uses form a chain from the coedge's
start to its end, each on its stored range. A chain that joins only when its
uses and their ranges are read in reverse is measured in that reading and
adds the face caveat `boundary.pcurve-order` on the coedge: CADIR lists the
uses in traversal order. The listing changes no value once the chain is
known.

When the coedge also has a 3D curve, the pcurve image at the Gauss–Kronrod
nodes of every breakpoint interval must lie on that curve within the larger
of the allowance and the pcurve's fit tolerance, at curve parameters that
advance monotonically over the curve's interval. Otherwise the face's bounded
quantities are `boundary.pcurve-curve-mismatch`. `coedges[].uv_source`
names the source of each coedge. An edge with no 3D curve takes its length
and box from the pcurve uses of its first coedge, mapped through that
coedge's face surface.

An inverted 3D curve interval that passes within the allowance of a cone
apex or a sphere pole inside the interval is not split there: the face's
bounded quantities are `boundary.inversion-failed`. The resolver detects the
pass at a sample, or between two samples whose parameter image turns by a
quarter period or more, by a golden-section search of the distance to the
singular points.

## Regions

`region` states the rule that decides a face's area:

| Region | Meaning |
| --- | --- |
| `nesting` | Loop roles from geometry and classification. The region does not depend on the face's orientation. |
| `declared_orientation` | The loops bound two regions; the declared orientation picks one. |
| `whole_surface` | The face covers its whole carrier. |
| `domain_exterior` | The carrier's domain is the outer boundary and every loop is a hole. |

On an unbounded plane the outer loop is the one loop that winds around a
point of every other loop. The other loops are holes and do not contain
each other. Winding numbers use each loop's parameter-plane polyline,
sampled until the deviation of each chord midpoint is at most `1e-3` of the
loop's parameter-plane box diagonal.

On a cylinder or a cone, a loop with zero net shift in `u` is contractible,
and a loop that goes around the axis once is not. With `m` loops that go
around the axis:

| `m` | Region |
| --- | --- |
| 2 | The band between the two loops. Contractible loops are holes in the band. |
| 1 | On a cone, the part between the loop and the apex. The apex is a member of the region. |
| 0 | The planar rule, with the winding numbers taken over lifts of each loop by whole turns. |

Other configurations are `boundary.nesting-invalid`. A ring whose coedges are
all degenerate at the cone apex bounds no area and is a vertex loop at the apex.
It is valid only with `m` = 1, and the edge has length 0. A loop that meets the cone apex has a synthesized
segment along the apex. When the loop has one
such segment and the segment can close the loop without a full turn, the
segment takes that length and the loop is contractible.

On a NURBS carrier's finite parameter domain:

| Loops | Region |
| --- | --- |
| No ring loop | `whole_surface`: the face covers the whole domain. |
| A curve-bounded construction with an implicit outer boundary | `domain_exterior`: the domain is the outer boundary and every loop is a hole. On an unbounded support the face is `boundary.unbounded-face`. |
| `FaceLoops::Classified` | `nesting` by the planar rule in the parameter plane. |
| `FaceLoops::Unspecified` | `declared_orientation`: the region lies on the left of every loop, after each loop is reversed when the declared normal is reversed. The region reaches the domain's natural edges when a point just left of a loop has a total winding number of zero. |

Loops on a periodic NURBS domain are `region.domain-unsupported`.

On a sphere or a ring torus both regions that the loops bound are bounded,
so the loops do not choose a side:

| Loops | Region |
| --- | --- |
| No ring loop, or ring loops of seam and synthesized pieces only | `whole_surface`: the face covers the whole carrier, `4πR²` or `4π²R\|r\|`, with the method `closed_form`. |
| Other ring loops | `declared_orientation`: the region lies on the left of every loop, after each loop is reversed when the declared normal is reversed. |

A loop that meets a sphere pole has a synthesized segment along the pole.
The segment keeps the region on its left: it runs in `−u` at the pole
`v = π/2` and in `+u` at the pole `v = −π/2`, after the loop reversal for a
reversed declared normal, by an amount in `[0, 2π)`. When the amount is within the
angular allowance of 0 or `2π`, the segment takes the nearest lift.

On a torus the net shifts of the loops sum to zero, else the face is
`boundary.nesting-invalid`. Horn and spindle tori are
`region.domain-unsupported`.

Membership of a parameter point in such a region uses the region
indicator. The indicator is 1 at a reference point just left of a loop
segment. A path from the reference point runs along `v` to the point's `v`,
then along `u` to the point. Each crossing of a loop segment adds 1 when the
path enters the left side of the segment and subtracts 1 when it leaves it.
The reference point is offset from the midpoint of one of the longest
segments by `1e-6` of the loop's parameter-plane box diagonal. Up to 8
segments are tried; a reference whose indicator beyond the loops is not 0
or 1 is rejected. When every reference is rejected, the face's bounded
quantities are `evaluation.work-exhausted`.

Area and flux on a sphere or torus use Green's theorem on the strip
`u ∈ [0, 2π)` by `v ∈ [base, base + span]`. The parts of the upper edge in
the region add to the loops' integrals; each `u` interval `I` of the edge
adds `W·|I|`, where `W` is the area of one unit of `u`. On a sphere
`base = −π/2`, and the upper edge is the north pole. The edge is in the
region when the region indicator beyond every boundary point toward the
north pole is 1. A torus is cut at a level `base` midway in the widest gap
between the loops' `v` values. Each piece is split where it crosses the cut,
and its `v` is taken in `[base, base + 2π)`. An interval of the cut circle
starts at each crossing in `−v` and ends at the next crossing in `+v`.
Crossings of seam pieces do not count, and crossings that do not alternate
make the face `boundary.nesting-invalid`. Without crossings, the indicator
at one point of the cut decides the whole circle.

The parameter values of a loop are not reduced to the carrier's domain, so
one polyline segment can cover many periods. A membership path, a crossing
count on a cylinder or a cone, and the cut of a torus visit each lift of a
segment by whole periods. A segment that covers more than `MAX_SPAN_LIFTS`
periods ([decode-resource-caps.md](decode-resource-caps.md)) makes the
face's bounded quantities `evaluation.work-exhausted`. A segment whose
parameter values do not change by a shift of one period makes them
`boundary.period-unresolved`. A non-finite parameter value makes them
`evaluation.non-finite`.

### Readings

A `declared_orientation` face has two readings: the `declared` region and
its `complement` in the domain. When one reading has zero area within its
error, the other is the face's region whatever the orientation: the witness
`empty_reading` confirms the declared reading, or puts the face in conflict
with the complement's values. Otherwise the area, box, latitude range and
sweep are measured under `reading: declared` with the `complement` value as
a candidate, and the face has the caveat `orientation.unconfirmed`. When
both readings give one value within error, the value has no reading and no
candidate. Two sweeps are one value only when their angles, `full` flags and
intervals agree.

A sum or union whose member value is measured under `reading: declared`
rests on that reading: a body's `area` and `bbox` over its faces, and the
document's `area` and `bbox` over its bodies. It carries
`reading: declared` and a `complement` candidate, which takes every such
member under its `complement` candidate; without a complement candidate
for one such member, it carries no candidate. The body's caveat count
states the unconfirmed faces.

### Witnesses

A face is established when its loops and its sense agree, when its region
is `whole_surface`, or when a witness confirms its declared reading.
Witnesses run over each shell that holds an unconfirmed face, so a face row
reads the whole shell.

- Tangent edge. A non-seam edge used by exactly two coedges with opposite
  senses, one in the unconfirmed face and one in an established face, is a
  witness edge when the two oriented normals are parallel within
  `1e-2` rad at its two vertices and its midpoint. A singular point is
  skipped; the midpoint is required. The edge votes `agree` when the
  normals point the same way at every point, and `disagree` when they point
  opposite ways at every point. A face with votes that all agree is
  confirmed. A face with a disagreeing vote is in conflict. A tangent edge
  is taken as a smooth continuation of the two faces; at a cusp edge the
  vote is wrong.
- Shell sign. In a closed shell whose faces are all established except one
  unconfirmed face, the shell volume is positive for an outer shell and
  negative for a void. When exactly one reading of that face gives the
  required sign beyond the volume error, that reading is the face's.

Confirmed values have no reading and no candidate. A face in conflict by a
witness has `consistent: false`; its values that depend on the reading are
`region.orientation-conflict` with the `declared` and `complement` values
as candidates.

### Procedural carriers

A curve-bounded construction takes the canonical kind and chart of its
support. An exact construction is measured on its solved cache. Another
construction is measured on its solved cache when the cache states its fit
tolerance, which enters the model uncertainty; its kind stays
`procedural`. Every other procedural carrier is
`carrier.procedural-unevaluated`.

## Extents

`axial_range` is the range of `(P − axis_point) · axis_direction` over a
cylinder or cone face, in millimetres. `axial_extent` is its length.
`angular_sweep` is the measure of the face's projection onto the circle of
directions about the axis: the union of the `u`-ranges of the boundary
pieces. A loop that goes around the axis makes the sweep full. `intervals`
gives the covered `u` intervals from `ref_direction`, each start in
`[0, 2π)`. Gaps smaller than the angular allowance
`max(tolerances.angular, allowance / largest radius)` are closed.
`radius_range` is the cross-section radius range of a cone face; its minimum
is 0 when the apex is a member of the region. On an elliptic cone,
`angular_sweep` is `sweep.unsupported-carrier` and `radius_range` is
`carrier.elliptic-cone`.

On a sphere `latitude_range` is the range of the latitude over the face, in
radians from the equator toward the carrier's `axis_direction`, and
`angular_sweep` is about that axis, with its intervals from the carrier's
`ref_direction`. A sphere with a negative stored radius has the chart point
`c + R(cos v(cos u·e1 + sin u·e2) + sin v·a)` with `R < 0`: its latitude is
`−v` and its longitude `u + π` in the stored frame. Both start
from the boundary pieces' ranges; pole pieces carry no direction and do not
enter the sweep. A latitude circle or meridian that misses the boundary lies
wholly in the region or wholly out of it, so one membership test decides
each gap between the boundary's ranges. A loop that goes around the axis
makes the sweep full.

On a ring torus (`R > |r|`), `angular_sweep` is about the axis from
`ref_direction`, with `u` the angle: the union of the boundary pieces'
`u`-ranges, with each gap between them in the region when its middle
meridian is. A loop that goes around the axis, or a face without loops,
makes the sweep full. Gaps smaller than `max(tolerances.angular, allowance
/ (R + |r|))` are closed. The model uncertainty is `τ_b / (R − |r|)`. On
any other torus `angular_sweep` is `sweep.unsupported-carrier`.

## Boxes

A face box is the box of the boundary pieces joined with the carrier's
interior critical points that lie in the region. A plane or a cylinder has no
interior critical point, and its box has the method `boundary_extremum`. On a
cone the apex is the only one, and the box has the method `critical_points`.
On a sphere the candidates are the six points at the radius along the
model axes; on a ring torus, the points where a model coordinate is
stationary, four per axis. A candidate at a sphere pole is a member when the
region indicator beyond the boundary toward that pole is 1. The box of the
complement reading joins the candidates that are not members.
On a NURBS carrier the box is the box of the placed control net,
`conservative` with the method `control_hull`; a weight that is not
positive makes it `bbox.nonpositive-weights`. `bbox.boundary` is the box of
the boundary pieces alone.

A body box is the union of its face boxes, or of its edge boxes for a body
with no faces. A document box is the union of its body boxes. A union is
`tight` only when every member box is `tight`. A `conservative` union has
`bbox.boundary`, a lower bound of its tight box: the union of the `tight`
member boxes and of the `boundary` boxes of the `conservative` members. It
is absent when no member has a lower bound.

A body with vertices also reports `vertex_box`, the box of its placed vertex
points: `tight`, with the method `closed_form` and a model uncertainty of the
larger of the document's linear tolerance and the vertex tolerances.

`u_range` and `v_range` of a NURBS face are the parameter ranges of its
boundary pieces, or the domain when the region reaches the domain's natural
edges.

## Orientation

`orientation.declared_normal` is the oriented normal the face's sense
states. `orientation.loop_normal` is the oriented normal the loop winding
gives against the computed loop roles. Loops keep the face on their left
viewed from the oriented normal. `consistent` is `true` when the two
readings agree, `false` when they disagree or the loops disagree with each
other (`mixed`), and `null` when no reading decides.

A body's `orientation` lists its faces by class: `inconsistent_faces`
(sense against loop winding), `mixed_faces`, `unconfirmed_faces` (a
`declared_orientation` region with no witness) and `conflict_faces` (a
`declared_orientation` region a witness contradicts). Its `consistent` is
`false` when one of `inconsistent_faces`, `mixed_faces` or
`conflict_faces` is not empty, `null` when a face's orientation is not
established, and `true` otherwise. The document's `orientation` lists the
same classes over the body rows.

## Surface rows

`measure --surface ID` reports the carrier of a surface: `surface`,
`placement` (see Placement space), `carrier` as a face row states it,
`bodies` and `faces`, the bodies and faces that use the surface, and
`status`. A surface row has no area, length, box or extent. A face uses a
surface when it names it as its surface; a face that reaches the surface
only through a carrier chain, such as the basis of an offset or the
support of a curve-bounded construction, does not use it. The row is in
model space under the placement of the bodies of its faces. A surface
that no face uses is measured as stored, and its row has the placement
`{"space": "stored"}`. When those bodies have different placements, the
row has no parameters and its `unresolved` member is the unavailable value
`placement.carrier-shared`; `kind` and `chain` are then read from the
surface alone. A placement or carrier chain that does not resolve gives
`unresolved` with its own reason in the same way. A carrier with no chart,
such as `carrier.procedural-unevaluated`, has no parameters, and its
reason is the `unresolved` value.

## Placement space

Every body, face, edge and surface row has a `placement` member:
`{"space": "model"}`, or `{"space": "occurrence", "occurrence": ID}` under
`--occurrence ID`, or `{"space": "stored"}` for a surface row that no face
uses (see Surface rows). In model space each body is placed by its own
`Body.transform`. In the instance space of an occurrence, each body of the
occurrence's prototype is placed by its own transform first and then by the
occurrence chain: `E(root) ∘ … ∘ E(ID) ∘ Body.transform`, where `E` is the
occurrence's effective transform (its transform composed with its linked
prototype contribution) and each occurrence applies inside its parent. The
subtree of the occurrence is in the same space: a child `C` of an
occurrence `O` places the bodies of its prototype by `P(O) ∘ E(C)`, where
`P(O)` is the composed placement of `O`. The scope is the bodies of the
prototypes of the occurrence and of its subtree; a `--body`, `--face`,
`--edge` or `--surface` selector outside it is refused.

The report then has an `occurrence` member: `occurrence`, `prototype` (the
local product definition), `chain` (root first), `transform` (the composed
chain as three rows of a 3 × 4 affine map), `bodies` (the bodies of the
scope) and `descendants`, breadth first, each with `occurrence`, `parent`,
`prototype`, `transform` (its composed placement from the root) and
`bodies` (the bodies its prototype lists). When the space cannot place its
bodies, the report has no row, every document quantity that applies to the
selection is unavailable with the same reason, `transform` is absent, and
`occurrence.unavailable` states why: `placement.occurrence-chain-broken`
(a parent is missing or the chain repeats), `placement.occurrence-scale`
(an occurrence of the chain or of the subtree has a scale other than
`(1, 1, 1)`; `entity` names it), `placement.prototype-unresolved` (an
external or unresolved prototype, or a definition that does not resolve),
`placement.non-finite`, `placement.occurrence-body-repeated` (two
occurrences of the subtree place one body; `entity` names it) or
`placement.occurrence-empty` (the scope has no body). The first failure in
the order chain, occurrence, subtree breadth first is the reason.
`document.unavailable` counts it. Provenance lists the stored fields of
the carrier chain and of the body transform; the transforms of the chain
and of the subtree are in the `occurrence` member. `diff --geometry`
measures in model space.

## Shells

A shell is closed when every edge its coedges use is used by exactly two of
them with opposite senses, and no face of the shell reaches a natural edge
of its parameter domain. Degenerate edges do not count. The two coedges of
a seam in one face count as two uses. A face reaches a natural edge when
its region is `whole_surface` or `domain_exterior`, or a
`declared_orientation` region whose indicator constant is 1. An open shell
lists its first 16 open edges, the open edge count, and the faces that
reach natural edges (`natural_edge_faces`).

## Volume, centroid and closure

Each face region gives five flux integrals over the face, with
`N = S_u × S_v` and `d = P − P_ref`: `∫ d·N / 3` (volume), `∫ d_i² N_i / 2`
(first moments), `∫ N` (closure vector), `∫ d_i³ N_i / 3` and
`∫ d_i² d_j N_i / 2` (second moments, see Mass properties) and
`∫ d_i |N|` (area moments). By
Green's theorem each is `∮ F du` over the region's loops with the inner
integral `F(u, v) = −∫_{v₀}^{v} f(u, s) ds` from the region's base `v₀`,
plus the whole domain when the region reaches the domain's natural edges.
`P_ref` is the centre of the body's edge and vertex box; for a body with
neither, the centre of its faces' whole carriers (the critical points of a
sphere or torus, the control net of a NURBS carrier); else the origin.

A body sums its faces' fluxes with the sign `σ` of each face's oriented
normal, times the sign of the determinant of the body placement:

- volume `V = Σ σ Φ_V`;
- centroid `C = P_ref + M / V`;
- closure vector `Z = Σ σ Φ_N`, in mm².

The body reports `|Z|` under two readings in `orientation.closure_flux`:
`face_sense` (every face's sense) and `loop_winding` (every face's loop
winding where it is determinable). The volume and centroid use the
`face_sense` reading. They are measured when, in this order:

1. every shell is closed (else `volume.shell-open`, category `undefined`);
2. every face has a flux (else `aggregate.member-unavailable` with the
   member's category);
3. no face disagrees with its readings (else `volume.orientation-conflict`)
   and every face's orientation is established (else
   `volume.orientation-unverified`);
4. `|Z|` is at most the closure tolerance, the flux error plus the sum over
   faces of boundary length × `τ_b` (else `volume.closure-flux`);
5. the outer shell's volume is positive and each void shell's volume is
   negative (else `volume.orientation-conflict`).

Otherwise the volume and the centroid are unavailable with the first
failing reason. They carry as candidates the `face_sense` and
`loop_winding` readings, and the `complement` reading for
`volume.orientation-unverified`; a reading is a candidate only when every
face has a flux under it. A body with closed shells reports each shell's
signed volume.

`numeric_error` of `V` is the volume flux error. A face's surface
tolerance `s` is the larger of the document's linear tolerance, the face's
tolerance and its carrier's fit tolerance. An edge's trim tolerance `t` on
a face is the larger of the edge's model tolerance (the largest of the
linear tolerance, the edge's tolerance and its vertices' tolerances) and
its largest measured deviation on the face: the gaps at its vertices and
junctions, and its pieces' distance from the surface. A face's area `A` is
its largest area over its readings. A face's lever `d` is the largest
distance from `C` to a corner of its boxes over its readings, or the body
box diagonal when it has no box. An edge's lever `ρ` is the largest
distance from `C` to a corner of its box. Each face adds `A s`, `A s d` and
`A s d²` to the volume, first-moment and second-moment sums. Each of its
edge uses of length `L` adds `⅓ L t ρ`, `¼ L t ρ²` and `⅕ L t ρ³`. A face
whose boundary is unavailable, or with an edge that has no length or box,
adds `A τ_b`, `A τ_b d` and `A τ_b d²` instead. `model_uncertainty` of `V` is the volume sum plus the
placement term. `model_uncertainty` of `C` is the first-moment sum plus the
placement term of `V` times the largest face lever, over `|V|`. The body
box diagonal is the largest diagonal of the body box and its candidates.
When the body box is unavailable, it is the diagonal of the union of every
face's boxes over its readings. When a needed face area, lever or diagonal
is unavailable, the value is unavailable (`aggregate.member-unavailable`).

A body with faces and no closed shell reports `area_centroid`, the
area-weighted centroid `P_ref + Σ Φ_A / A`, where `Φ_A` is the area moment
`∫ d |N|` of a face region, which does not depend on the orientation, and
`A` the sum of the face areas. Each face contributes the region its
readings establish: the complement region of a face in conflict by the
empty-reading test, else the declared region. When a witness contradicts a
face, the area centroid is unavailable (`region.orientation-conflict`) with
the `face_sense` candidate (every declared region) and the `loop_winding`
candidate (the complement region of every face in conflict). When an
unconfirmed face's region changes the value, the area centroid carries
`reading: declared` and a `complement` candidate with every unconfirmed
face in its complement region. Its `numeric_error` is the area moment
error over `A` plus the largest area moment times the area error over
`A²`. Its `model_uncertainty` is the sum over faces of boundary length ×
`τ_b`, plus the placement term of `A`, times the body box diagonal over
`A`.
`document.closed_volume` is the sum of the body volumes; it is unavailable
when one is.

## Mass properties

A body with faces reports `second_moments`, the symmetric tensor
`∫ (x − C)(x − C)ᵀ dV` about the centroid in mm⁵, with the `value`
`{xx, yy, zz, xy, yz, zx}`. It comes from the same fluxes, reading and
preconditions as the volume: with the second moments `S = Σ σ Φ_S` and the
first moments `m` about `P_ref`, it is `S − m mᵀ / V`. When the volume is
unavailable, every mass property is unavailable with the same reason, and
it carries no candidate.

- `numeric_error` is the largest first-order propagation of the flux
  errors of `S`, `m` and `V` into one entry. The second moments do not
  drive the flux quadrature. They are integrated on the panels that the
  volume, first-moment, closure and area-moment components refine, and the
  flux error of each is its own Gauss–Kronrod estimate on those panels.
- `model_uncertainty` is the second-moment sum (see Volume, centroid and
  closure) plus the placement term. The move of the centroid changes the
  tensor only at second order.

`principal_moments` is a list of three values, the eigenvalues of
`second_moments` in mm⁵, smallest first, by cyclic Jacobi rotations. The
error terms of each are three times those of the tensor; the numeric error
adds the off-diagonal norm the last rotation leaves. `principal_axes` is a
list of three values in the order of the moments: the unit eigenvector of
each moment as a direction `{x, y, z}` with unit `1` and its component of
largest magnitude positive. The gap of a moment is its smallest distance to
the other two moments. The error terms of an axis are those of the moments
over its gap. When the gap is at most `1e-6` of the largest moment plus
twice the moments' summed error terms, the axis is unavailable with
`mass.principal-axes-degenerate` (category `undefined`). An axisymmetric
body thus has its axis of symmetry and two undetermined axes; a body with
three equal moments has none.

With `--density VALUE` (kilograms per cubic metre, finite and positive),
the body also reports `mass` in kg, `ρ V`, and `inertia`, the inertia tensor
about the centroid in kg·mm² (unit `kg.mm2`, `value` as for
`second_moments`): `ρ (tr(M) E − M)` for the
second moments `M`, so `I_xx = ρ (M_yy + M_zz)` and `I_xy = −ρ M_xy`. Their
error terms scale with `ρ`; those of `inertia` are twice those of
`second_moments`. Without a density, `mass` and `inertia` are absent.

## Comparison

An ID-free comparison measures every body of two documents in model space
and matches bodies by their measured geometry, not by their IDs.

- Placement. A document's placement is `trivial` when it has no
  occurrences, or when every occurrence has a local prototype, unit scale and
  an effective transform whose linear part is the identity within `1e-8` and
  whose translation is within `tolerances.linear` of zero, every product
  definition that lists bodies is the prototype of exactly one occurrence,
  and no body is listed by two definitions. Otherwise it is `occurrences`:
  the document row `occurrences` is not compared
  (`compare.occurrence-placement`), and the bodies are still compared.
- Tolerance. A value compares within
  `t = e_a + e_b + u_a + u_b + floor × max(|a|, |b|, L^d)`, with `e` the
  numeric error, `u` the model uncertainty, `L` the larger document box
  diagonal, `d` the quantity's dimension and `floor` the relative floor,
  `1e-9` by default.
- Rows of a body pair. `area`, `volume`, `centroid`, `area_centroid` and
  `length` are `equal` within `t` when measured on both sides; a vector
  splits into components. A value measured under a reading compares with
  the same reading on the other side when that side has it. A value
  measured on one side and unavailable on the other disagrees for an
  `input` or `undefined` category, and is not compared
  (`compare.unavailable`) for `budget`, `unsupported` or `placement`. Values
  unavailable on both sides compare by their candidates of the same reading
  (`volume.face_sense`, `volume.loop_winding`); without such candidates the
  reason codes compare `exact` when both categories are `input` or
  `undefined`, and the row is not compared otherwise. Two `tight` boxes
  compare their corners `equal`. Otherwise each side's lower bound must lie
  in the other side's box widened by `t` (`containment`, with the largest
  violation as the difference). The lower bound of a `tight` box is the box,
  and of a `conservative` box its `boundary`. When neither side has a lower
  bound, the row is not compared (`compare.no-lower-bound`). `closed`
  compares `exact`;
  `orientation.consistent` compares `exact` when it is not `null` on both
  sides.
- Signature. Two bodies with no measured numeric quantity, whose
  unavailable values all have the category `input` or `undefined`, compare
  their kind, face, edge and vertex counts, their `vertex_box` corners
  (`equal` within `t`) and their sets of reason codes. The vertex box is the
  only signature row that depends on position. When neither body has a
  vertex, it is not compared (`compare.signature-unplaced`).
- Classes. Two bodies are compatible when at least one numeric or
  signature row is compared and every compared row agrees, across and within
  the two documents. Compatibility splits the bodies into connected classes.
  A class whose bodies are all pairwise compatible pairs its bodies of the
  two documents by multiplicity in ID order and leaves the surplus
  unmatched; another class is ambiguous (`compare.ambiguous-class`). A body
  with no comparable quantity is unmeasured (`compare.unmeasured-body`).
- Document rows: `body_count` (`exact`), total `area` and `closed_volume`
  when measured on both sides, the union box, and `occurrences`.
- Verdict: `disagree` when a compared row disagrees or a body is
  unmatched; else `inconclusive` when a cause applies, including
  `compare.nothing-compared`; else `agree`. Surface-kind counts and `visible`
  are informational.
- Work. Only body pairs that can be compatible are evaluated, and only the
  rows of paired bodies are kept. A signature body is evaluated against the
  signature bodies of the same kind, counts and reason codes. Two bodies
  whose boxes are measured under no reading and have a lower bound are
  evaluated when their boxes overlap along the axis of the widest spread of
  box centres, within the largest box tolerance of the comparison; a box row
  of two bodies that do not overlap disagrees. Every other body is evaluated
  against every body that is not a signature body. Each evaluation charges
  one unit of the pair allowance
  `max(2^20, k × (bodies of both documents))`, with `k` set by `--limits`.
  A comparison that needs more evaluations is refused as a resource limit at
  the `measure` stage. `comparison.work` states the allowance and the
  evaluations used.

## Numerical facts

Three steps rest on sampling or on an assumption, and are not proofs. Every
other step is exact up to the reported error terms.

- Winding numbers and region membership use boundary polylines. Each chord
  midpoint deviates from its piece by at most `1e-3` of the loop's
  parameter-plane box diagonal. A loop feature narrower than that deviation
  can change a winding number.
- The tangent witness reads a tangent-continuous edge as a smooth
  continuation of its two faces.
- Extremum search on pieces other than lines and conics samples the
  derivative at the Gauss–Kronrod nodes of each panel and refines each sign
  change by bisection. Two roots between adjacent samples can be missed.

## Before and after a change

To see what a code change does to measured geometry, write the model with
the old binary and compare it with the new one:

```sh
cadmpeg dump part.step -o before.cadir.json
cadmpeg diff before.cadir.json part.step --geometry
```

Run the first command with the old binary and the second with the new
binary. Both binaries must read one `IR_VERSION`. Across a version change,
run the first command with the new binary built from the old source
revision.

## Guardrail

Measurement is an oracle on decoded output. A decoder or encoder never
selects a layout, carrier, sense, unit, or interpretation by measuring
geometry. Only the `cadmpeg` CLI crate depends on `cadmpeg-measure` outside
`[dev-dependencies]`; the source-policy rule `measurement_dependency`
enforces this.

A writer test compares the measures of a document and of its written and
decoded copy with `cadmpeg_measure::test_support::round_trip`. The writer
crate takes that module through the `test-support` feature of its
`cadmpeg-measure` dev-dependency.

## Check findings

`cadmpeg check`, and the check that `convert` runs, add the defects that
measurement finds as `warning` findings, and one `info` finding when the
pass does not check every item. These findings do not change the exit
status or refuse a write. No decoder or export admission route reads these
checks ([admissibility-routes.md](admissibility-routes.md)).

| Check | Entity | Related | Defect |
| --- | --- | --- | --- |
| `ring_order` | the loop | the face | The ring's coedges, listed, do not run end to start by oriented vertex identity: they form one closed traversal in another order (`reversed` or `other`), several closed traversals, or none. |
| `pcurve_order` | the coedge | the face | The coedge's pcurve uses form their chain from its end to its start: the face caveat `boundary.pcurve-order`. |
| `orientation` | the face | none | The face's sense disagrees with its loop winding, or its loops disagree with each other: a face of `inconsistent_faces` or `mixed_faces`. |
| `orientation` | the body | the faces or the shell the conflict names | Every shell of the body is closed, and the volume is unavailable with `volume.orientation-conflict` or `volume.closure-flux`. |
| `measurement_incomplete` | none | none | Severity `info`. The findings allowance ran out before the pass checked every face, edge and closed body. The message states the number of faces and bodies not checked. |

The findings pass resolves the boundary, region and orientation readings of
every face and runs the witnesses. For a body whose shells use every edge
twice with opposite senses, it also integrates the face areas and the
volume and closure fluxes; its flux quadrature targets only those
components. On a plane `d·N` is `h |N|` with `h = (o − P_ref)·n̂`
constant, so the volume flux is `h A / 3` and the closure flux `n̂ A` for
the face area `A`, with the area's numeric error scaled into each. The
pass computes no carrier parameter, box, extent or edge box.

The pass has its own allowance,
`max(FINDINGS_PASS_WORK_FLOOR, k × (faces + edges of the document))` units,
with the floor and `k` set by the `--limits` profile of the command
([decode-resource-caps.md](decode-resource-caps.md)). It is much smaller
than the allowance of a measurement pass. Each face draws a slice of at
most `MAX_FINDINGS_FACE_WORK` units and each edge a slice of at most
`MAX_FINDINGS_EDGE_WORK`. A face or an edge whose slice runs out is not
checked, and neither is a body that owns it; the pass then adds the
`measurement_incomplete` finding. A session resource refusal refuses the
check at the `check` stage.

## Pass allowance

One measurement pass owns a work allowance of
`max(MEASURE_PASS_WORK_FLOOR, k × (faces + edges of the document))` units,
with `k` set by `--limits`. Each face and each edge draws a capped slice from
it. A slice that runs out makes the item's remaining quantities
`evaluation.work-exhausted`; the measurement continues. `document.work`
states the allowance, the units used, and whether the pass ran out.

Faces draw in ascending order of the curve poles of the coedges they invert
onto a NURBS surface, and then in identity order. A coedge with a pcurve use
and a face on an analytic carrier invert none. So a face whose boundary costs
the most cannot take the allowance of a face whose boundary costs less. An
inversion that its slice stops is `evaluation.work-exhausted`, not
`boundary.inversion-failed`.

## Reader duties

A reader ignores unknown members, handles an unknown `reason` or caveat
`code` through its `category`, and never parses `message`.
