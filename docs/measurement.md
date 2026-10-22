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

## Rings

A ring lists its coedges in traversal order. When the listed order is not
continuous by oriented vertex identity, the measurement takes the traversal
from topology: the unique closed traversal that uses every coedge once. The
loop entry's `listed_order` is `traversal`, `reversed` or `other`, and a
value other than `traversal` adds the face caveat `boundary.ring-order`. Area
and flux are sums of per-piece integrals, so the listed order changes no
value once the traversal is known.

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

## Orientation

`orientation.declared_normal` is the oriented normal the face's sense
states. `orientation.loop_normal` is the oriented normal the loop winding
gives against the computed loop roles. Loops keep the face on their left
viewed from the oriented normal. `consistent` is `true` when the two
readings agree, `false` when they disagree or the loops disagree with each
other (`mixed`), and `null` when no reading decides.

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
