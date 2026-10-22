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
| `value` | A finite number, or `{x, y, z}` for a point or a direction. A range or a box has `min` and `max` in its place. |
| shape members | `angular_sweep`: `full` and `intervals`. `bbox`: `kind` (`tight` or `conservative`) and `boundary`, the box of the boundary pieces. |
| `unit` | `mm`, `mm2`, `mm3`, `rad` or `1`. The unit of the value and of both error terms. |
| `method` | How the value was obtained (below). |
| `numeric_error` | A non-negative a-posteriori estimate of the computation error. It is zero for `source` and `closed_form`. It is not a certified bound. |
| `model_uncertainty` | The non-negative first-order change of the value when the boundary moves by the larger of the declared tolerance, the measured boundary deviations and the bridged gaps, plus the placement term. |
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
millimetres and angles in radians. Directions and ratios have unit `1`.

## Methods

| Method | Meaning |
| --- | --- |
| `source` | A stored scalar, or a stored point or direction under the identity placement, exactly as stored. |
| `closed_form` | Exact arithmetic on stored values. |
| `boundary_quadrature` | Adaptive Gauss–Kronrod quadrature along boundary pieces. |
| `boundary_extremum` | Extremum search along boundary pieces only. |
| `critical_points` | Boundary extremes and the carrier's interior critical points in the region. |
| `control_hull` | A NURBS Bézier control-net bound. |
| `divergence_quadrature` | Volume, centroid and closure from face fluxes. |
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
| `boundary.not-closed` | input | Consecutive pieces do not join within the allowance. |
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
| `sweep.unsupported-carrier` | unsupported | Torus and elliptic-cone sweeps. |
| `bbox.interior-unbounded` | unsupported | No interior bound for the carrier. |
| `bbox.nonpositive-weights` | unsupported | The NURBS control hull needs positive weights. |
| `length.derivative-unavailable` | unsupported | A procedural curve without a solved cache has no derivative evaluator. |

## Caveats

A caveat marks a measured result that carries a fact a reader should see.
Caveat codes follow the reason discipline.

| Code | Category | Level | When |
| --- | --- | --- | --- |
| `boundary.ring-order` | input | face | A ring's listed coedge order is not its traversal order; `entity` is the loop. |
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
closed coedge takes its direction from the tangent of its 3D curve, else
from the use's stored range. Several uses form a chain from the coedge's
start to its end.

When the coedge also has a 3D curve, the pcurve image at the Gauss–Kronrod
nodes of every breakpoint interval must lie on that curve within the larger
of the allowance and the pcurve's fit tolerance, at curve parameters that
advance monotonically over the curve's interval. Otherwise the face's bounded
quantities are `boundary.pcurve-curve-mismatch`. `coedges[].uv_source`
names the source of each coedge. An edge with no 3D curve takes its length
and box from the pcurve uses of its first coedge, mapped through that
coedge's face surface.

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

Other configurations are `boundary.nesting-invalid`. A loop that meets the
cone apex has a synthesized segment along the apex. When the loop has one
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

### Readings

A `declared_orientation` face has two readings: the `declared` region and
its `complement` in the domain. When one reading has zero area within its
error, the other is the face's region whatever the orientation: the witness
`empty_reading` confirms the declared reading, or puts the face in conflict
with the complement's values. Otherwise the area, box, latitude range and
sweep are measured under `reading: declared` with the `complement` value as
a candidate, and the face has the caveat `orientation.unconfirmed`. When
both readings give one value within error, the value has no reading and no
candidate. A body with one unconfirmed face reports its area under
`reading: declared` with the area under that face's complement as a
candidate; with several, the body area stays under the declared readings and
the body's caveat count states them.

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

On a sphere `latitude_range` is the range of `v` over the face, in radians
from the equator, and `angular_sweep` is about the sphere's axis. Both start
from the boundary pieces' ranges; pole pieces carry no direction and do not
enter the sweep. A latitude circle or meridian that misses the boundary lies
wholly in the region or wholly out of it, so one membership test decides
each gap between the boundary's ranges. A loop that goes around the axis
makes the sweep full. On a torus `angular_sweep` is
`sweep.unsupported-carrier`.

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

Each face region gives four flux integrals over the face, with
`N = S_u × S_v` and `d = P − P_ref`: `∫ d·N / 3` (volume), `∫ d_i² N_i / 2`
(first moments), `∫ N` (closure vector) and `∫ d_i |N|` (area moments). By
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

`numeric_error` of `V` is the volume flux error. `model_uncertainty` of `V`
is body area × the largest `τ_b` of its faces plus the placement term. Of
`C`, it is that value × the body box diagonal / `|V|`.

A body with faces and no closed shell reports `area_centroid`, the
area-weighted centroid `P_ref + Σ σ Φ_A / A`, where `A` is the body area.
`document.closed_volume` is the sum of the body volumes; it is unavailable
when one is.

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
  compare their corners `equal`; otherwise each side's boundary box must lie
  in the other side's box widened by `t` (`containment`, with the largest
  violation as the difference). `closed` compares `exact`;
  `orientation.consistent` compares `exact` when it is not `null` on both
  sides.
- Signature. Two bodies with no measured numeric quantity, whose
  unavailable values all have the category `input` or `undefined`, compare
  their kind, face, edge and vertex counts and their sets of reason codes.
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

## Pass allowance

One measurement pass owns a work allowance of
`max(MEASURE_PASS_WORK_FLOOR, k × (faces + edges of the document))` units,
with `k` set by `--limits`. Each face and each edge draws a capped slice from
it. A slice that runs out makes the item's remaining quantities
`evaluation.work-exhausted`; the measurement continues. `document.work`
states the allowance, the units used, and whether the pass ran out.

## Reader duties

A reader ignores unknown members, handles an unknown `reason` or caveat
`code` through its `category`, and never parses `message`.
