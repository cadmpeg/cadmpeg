<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# cadmpeg IR (`.cadir.json`) specification

`CadIr` is the versioned JSON product representation shared by codecs, validation, diffing, and encoders. This specification defines the current required IR version `"10"`. The `cadmpeg-ir` Rust types define field-level JSON types, and `cadir_json_schema()` derives the matching JSON Schema.

## Document layering

A product document has three semantic layers:

```text
CadIr
├── ir_version, scope, source?, units, tolerances
├── model
│   ├── topology and geometry carriers
│   ├── procedural constructions and neutral features
│   └── tessellation, appearance, and attributes
└── native
```

`model` is format-neutral. `native` is a map keyed by format ID. Each value is a map of arena name to arena. Each arena is an ID-sorted array of records with a required string `id` and codec-owned fields. The reserved `unknowns` arena stores opaque format-specific records; these do not count as typed content for patch eligibility. Decode-time source locations, exactness, and retained source records belong to the `SourceFidelity` sidecar. Namespace retention is settled in [native-arena-disposition.md](native-arena-disposition.md): retain every listed arena.

`scope` is required and holds one required boolean per arena class: `{"model": true, "design": false}`. Unknown scope keys are rejected. A false class has no populated neutral arena; true means its arenas are meaningful, including complete-and-empty arenas. Model and design are independent. The design arenas are features, input/result topologies, configurations, parameters, the six sketch arenas, and spreadsheets; all other neutral arenas are model. Serialization, parsing, decode admission, and `CadIr::set_scope` enforce this invariant. The setter refuses to drop a populated class atomically. `CadIr::empty()` declares full scope because an authored document is complete by definition. Scope participates in the content digest. The [document contract](document-contract.md) defines writer and sidecar semantics.

`source` is absent when the document has no source-container metadata. A present source block contains `format`, `attributes`, and `dialects`. `dialects` is always serialized. It is `null` for an unclassified source and otherwise contains the complete source `DialectLayers`, including the primary and every nested or carried layer. The primary dialect namespace equals `source.format`.

The neutral model arenas, in serialization order, are `bodies`, `regions`, `shells`, `faces`, `loops`, `coedges`, `edges`, `vertices`, `points`, `surfaces`, `curves`, `subds`, `pcurves`, `procedural_surfaces`, `procedural_curves`, `assets`, `features`, `feature_input_topologies`, `feature_result_topologies`, `configurations`, `parameters`, `sketches`, `sketch_entities`, `sketch_constraints`, `spatial_sketches`, `spatial_sketch_entities`, `spatial_sketch_constraints`, `spreadsheets`, `product_definitions`, `occurrences`, `assembly_joints`, `drawings`, `semantic_annotations`, `presentation_documents`, `view_presentations`, `tessellations`, `appearances`, `appearance_bindings`, `attributes`, `pmi`, `presentation_layers`, `physical_materials`, `mass_properties`, `rigid_bodies`, `colliders`, `articulations`, `physics_scenes`, and `solver_settings`. References are string IDs. `ProductDefinition.sketches` lists owned planar sketch IDs. Each ID names a member of `model.sketches` and a sketch belongs to at most one product definition. An absent or empty list owns no sketches. A sketch with no product definition belongs to the design root. `subds` contains subdivision-surface control cages and is a free carrier arena.

Pcurve geometry is a parameter-space line, angular circle, angular ellipse, parabola, hyperbola, first-order harmonic, first-order hyperbolic, polar harmonic, polar NURBS, NURBS, trimmed, or signed-offset curve. Circle and ellipse carriers store independent `x_axis` and `y_axis` parameter directions; a clockwise parameterization has a negated `y_axis`. General harmonic carriers evaluate `center + cosine*cos(t) + sine*sin(t)`; general hyperbolic carriers evaluate `center + cosine*cosh(t) + sine*sinh(t)`. A polar harmonic maps first-order radial-plane and axial harmonic coefficients to `(atan2(y, x), v)` without changing the spatial curve parameter. A polar NURBS evaluates radial-plane and axial control channels with one degree, knot vector, weight vector, and parameter, then maps the radial result through `atan2`. A signed offset adds its distance along the regular basis curve's left unit normal. Point evaluation requires a finite nonzero exact basis tangent; a nested signed offset has no point evaluation.

Maps serialize with lexicographically sorted keys. Arena entries are strictly sorted by ID. Canonical serialization uses that sorted order.

## Identity and order

Entity IDs have the grammar:

```text
<format>:<scope>:<kind>#<key>
```

`format` identifies the producing codec or `synthetic`. `scope` identifies the containing source object or stream. `kind` names the entity class. `key` is the source persistent key when one exists and otherwise a positional ordinal.

Each namespace component and the key are nonempty. An ID has exactly three colon-separated namespace components before one `#`, and contains no whitespace. These syntax rules also apply to native record IDs.

IDs are globally unique across neutral and native arenas. A codec produces identical IDs for identical input bytes when run at the same codec version. When the source supplies persistent identity, IDs stay stable across unrelated arena insertion. Each ID-bearing arena is sorted lexicographically by ID. Features also carry an `ordinal` for construction order. Array order remains ID order.

## Units, tolerances, and terms

All stored lengths, coordinates, distances, radii, linear tolerances, and length-bearing parameters are millimeters. `units.length` is `"millimeter"`. All angles and angular tolerances are radians. Dimensionless quantities remain unscaled.

`tolerances.linear` is the document-wide maximum linear deviation in millimeters. Consecutive planar or spatial sketch-profile entities are connected when their oriented endpoints differ by no more than this tolerance. `tolerances.angular` is the document-wide maximum angular deviation in radians. A face, edge, or vertex `tolerance` overrides `tolerances.linear` for that entity. The override has the same maximum-deviation meaning and must be finite and positive.

| IR term          | Meaning                                                        |
| ---------------- | -------------------------------------------------------------- |
| entity           | One ID-bearing neutral, native, or opaque record               |
| arena            | A flat, ID-sorted collection of one entity class               |
| topology         | Incidence and orientation independent of geometric coordinates |
| carrier          | Geometric support referenced by topology                       |
| sense            | Orientation relative to the referenced carrier                 |
| exactness        | Fidelity class of an entity or serialized field                |
| native namespace | Source-specific data outside the neutral model                 |
| unknown record   | Format-specific product identity and related entity links      |

## Topology

The B-rep hierarchy and carrier links are:

```text
body → region → shell → face → loop → coedge → edge → vertex → point
                           │        │         │
                           │        │         └── curve?
                           │        └── pcurve?
                           └── surface
```

`Body.kind` is `solid`, `sheet`, `wire`, or `general`. A body optionally records a display name, color, and `visible`. `visible` states whether the source document displays the body. Exporters omit bodies with `visible: false` from display-oriented formats. A body owns regions. A region is a connected component of a body and owns shells. A shell owns at least one of faces, wire edges, or free vertices. A face is an oriented bounded portion of one surface and owns loops. A loop's boundary role is `outer`, `inner`, or `unspecified`; a face has at most one explicit outer loop and may have only inner loops when the surface parameter domain supplies the exterior. An edge loop lists coedges in traversal order and may contain ordered pole-vertex uses anchored after a coedge. A vertex loop contains one unanchored vertex use and no coedges. A coedge and a pole-vertex use each own an ordered list of parameter-space curve uses. The uses of a coedge are in its traversal order, with its sense applied, and form one chain from its start to its end; the uses of a pole-vertex use follow the loop traversal. A use's optional parameter range states its interval in the traversal direction, so a decreasing range traverses the pcurve backward. Each pcurve use may record whether the source declared it isoparametric. An edge joins two vertices and optionally references a curve and canonical parameter range. A vertex references a point carrier. Point remains a separate carrier because it has independent identity and provenance.

| cadmpeg IR | ACIS/ASM | Parasolid        | STEP AP242                                                            |
| ---------- | -------- | ---------------- | --------------------------------------------------------------------- |
| body       | body     | body             | manifold_solid_brep / shell_based_surface_model / geometric_curve_set |
| region     | lump     | region           | no direct entity                                                      |
| shell      | shell    | shell            | closed_shell / open_shell                                             |
| face       | face     | face             | advanced_face                                                         |
| loop       | loop     | loop             | edge_loop / vertex_loop                                               |
| coedge     | coedge   | fin              | oriented_edge                                                         |
| edge       | edge     | edge             | edge_curve                                                            |
| vertex     | vertex   | vertex           | vertex_point                                                          |
| point      | apoint   | point            | cartesian_point                                                       |
| surface    | surface  | surface          | surface                                                               |
| curve      | curve    | curve            | curve                                                                 |
| pcurve     | pcurve   | curve-on-surface | pcurve                                                                |

### Loop and radial rings

A loop is either a nonempty `coedges` ring of distinct coedge identities or one unanchored vertex use. For every edge loop, `coedges` contains exactly one simple cycle. Each coedge's `next` and `previous` links are reciprocal and remain within that loop. Pole-vertex uses in an edge loop identify their preceding member with `after`; multiple uses after one coedge retain vector order. A vertex loop contains no coedges and exactly one vertex use whose `after` is absent.

An edge loop's traversal, with each coedge's sense applied, keeps the face on its left when viewed from the side its oriented normal points to. The oriented normal is the surface normal, reversed when the face's `sense` is `reversed`. An outer loop therefore runs counterclockwise about the oriented normal and an inner loop clockwise; on a closed solid the oriented normals point out of the material. This is the STEP `face_bound` convention after `orientation` and `same_sense` are applied, so the STEP reader and writer carry loops unchanged.

All coedges that use an edge form one closed radial ring through `radial_next`. Every member references the same edge:

- one member is a laminar boundary and points to itself;
- two members are manifold adjacency;
- three or more members are legal non-manifold adjacency.

A two-member ring may use opposite or equal senses. Equal senses produce a validation warning.

### Wires and free vertices

A wire edge appears in exactly one shell's `wire_edges` and in no coedge. A free vertex appears in exactly one shell's `free_vertices` and bounds no edge. A `wire` body contains no faces. `solid` and `sheet` bodies use face topology; `general` bodies may mix dimensionalities.

## Geometry and canonical parameterization

Surface carriers are plane, cylinder, cone, sphere, torus, NURBS, procedural, or unknown. Curve carriers are line, circle, ellipse, parabola, hyperbola, degenerate, NURBS, procedural, or unknown. Pcurves are analytic, first-order harmonic, first-order hyperbolic, polar, NURBS, trimmed, or signed-offset curves in a surface's `(u, v)` space. A subdivision surface is a Catmull–Clark control cage with vertices, edges, directed face edge uses, endpoint sharpness, edge tags, vertex tags, and sector coefficients.

Point, surface, curve, subdivision-surface, and tessellation carriers may carry a `SourceObjectAssociation`. The association records the source format and native object identifier, effective name, color, visibility, layer, and outermost-to-innermost instance path. These fields preserve source-object identity and display metadata. The optional `geometry_role` states `independent` or `support` from source ownership. A support retains its mathematics if its owner cannot be projected; it must not become standalone geometry simply because no projected topology uses it. Absence leaves ownership unknown, and references from existing topology still establish support ownership.

Analytic surfaces carry the frame needed to interpret parameters: plane `u_axis`; cylinder, cone, sphere, and torus axis and `ref_direction`. For optional frame fields, absence means that the source supplied no stable frame. When a decoder constructs a frame, it chooses the normalized projection of the global axis with the smallest absolute dot product with the carrier axis and marks the field `derived`.

| Carrier                | Canonical parameters                                                                                 |
| ---------------------- | ---------------------------------------------------------------------------------------------------- |
| line                   | `t` is signed arc length in millimeters; `P(t) = origin + t direction`                               |
| circle                 | `t` is radians from a deterministic in-plane reference; one revolution is `[0, 2π]`                  |
| ellipse                | `t` is radians from `major_direction`; `0` is the positive major axis                                |
| parabola               | STEP conic parameter about `major_direction`; geometry uses vertex and focal distance                |
| hyperbola              | STEP conic parameter about `major_direction`; geometry uses semi-transverse and semi-conjugate radii |
| plane                  | `u` and `v` are millimeters along `u_axis` and `normal × u_axis`                                     |
| cylinder               | `u` is azimuth in radians from `ref_direction`; `v` is axial distance in millimeters                 |
| cone                   | `u` is azimuth in radians; `v` is signed axial distance in millimeters from `origin`                 |
| sphere                 | `u` is azimuth in radians; `v` is latitude in `[-π/2, π/2]`                                          |
| torus                  | `u` is major azimuth and `v` is minor azimuth, both in `[0, 2π]`                                     |
| NURBS curve or surface | parameters are the stored knot-domain coordinates                                                    |

`Curve.parameter_range` optionally retains a finite source interval independently of topology edges. Its first endpoint is strictly below its second. The interval records the bounded carrier extent; it does not restrict use of the underlying mathematical curve outside that interval.

`Edge.param_range` uses the canonical parameterization of its curve when a 3D carrier exists. An edge runs from its start vertex to its end vertex in the direction of increasing curve parameter: the first endpoint of its range evaluates to its start vertex and the second to its end vertex. An edge with no range on an open bounded curve, which has a `Curve.parameter_range` or is a clamped NURBS curve, runs in the same direction over that domain or a part of it, so its start vertex is not at the end of the domain while its end vertex is at the start. A carrier-less degenerate or tolerant edge has no canonical 3D domain; its optional range retains finite native endpoint doubles without imposing carrier-domain ordering. Full circles are anchored to `[0, 2π]`. A closed edge, whose start and end are one vertex, with no range traverses its carrier once: one revolution `[0, 2π]` of a circle or an ellipse, or the effective knot domain of a clamped or periodic NURBS curve; a placed curve takes the interval of its basis. The edge is closed over that interval only when both ends of the interval evaluate to its vertex. Periodic ranges may cross a seam by using an end value greater than the start value in the unwrapped domain. Pcurve coordinates use the corresponding surface conventions. ASM inline metadata stores `uv_offset`, a finite two-component translation in the support native chart. General metadata can also store this optional translation. Pcurve geometry includes the translation. `parameter_range` in General metadata retains its directed trim-interval meaning.

Decoders convert kernel conventions at decode:

- NX/Parasolid linear parameters expressed in meters are multiplied by 1000. Unit conversion preserves `byte_exact` status.
- CATIA cylindrical arc-length coordinates use `u = rθ` and are divided by radius. The converted field is `derived`.
- CATIA conical angular coordinates already use the canonical azimuth and are unchanged.
- Fusion ellipse phases are normalized to the major-direction origin and marked `derived`.
- Kernel full-circle intervals are re-anchored to `[0, 2π]` and marked `derived`.

NURBS surfaces store degrees, full knot vectors, pole counts, u-major control points, optional per-pole weights, periodicity flags, and whether the carrier's oriented normal is opposite `Pu × Pv`. NURBS curves store degree, full knot vector, ordered control points, optional weights, and periodicity. Knot values are finite and nondecreasing. Control-point coordinates are finite. Each knot vector has its pole count plus its degree plus one values. A present weight vector has one weight per pole. NURBS weights are finite and nonzero; negative weights are permitted. This holds for model-space, pcurve and polar NURBS carriers alike.

A periodic NURBS pcurve and a periodic polar NURBS pcurve evaluate points and derivatives modulo
the positive width of their effective knot domain. Placement and offset carriers retain that
period. A coedge pcurve use preserves its unwrapped endpoint values, including a seam crossing,
and spans at most one period in either direction with a relative span allowance of `1e-12`.
An explicit nondegenerate trim restricts the use to its stated interval. A zero-width knot domain
supplies no period and does not admit a periodic use.

An intersection-curve support side has an optional pcurve binding. A binding contains its pcurve geometry and an optional explicit mapping range. The range endpoints are finite and distinct. Endpoint order defines mapping direction and may decrease. Supporting-surface presence is independent of pcurve presence.

## Procedural carriers

Procedural entities retain construction semantics either as a surface or curve carrier or beside a solved carrier. `SurfaceGeometry::Procedural.construction` and `CurveGeometry::Procedural.construction` identify the construction that exactly defines the carrier; the referenced construction identifies that carrier in return. This bidirectional relation is required. A procedural construction with an analytic or NURBS carrier retains both the construction and its solved representation. Model-aware evaluation resolves nested offset carriers recursively and rejects reference cycles; the support normal is the normalized cross product of exact analytic, rational NURBS, or affine-transformed parameter tangents. A regular parallel offset retains that oriented unit normal for a dependent offset. Other procedural families require a solved carrier or a family evaluator. `cache_fit_tolerance`, when present, is the maximum millimeter deviation between the procedural definition and solved carrier. A NURBS surface cache can retain this fit in its optional `cache_fit_tolerance` field. A pcurve's `fit_tolerance` likewise bounds the model-space deviation after mapping the pcurve through its coedge's face surface.

Procedural surface definitions are:

- `extrusion`: directrix and sweep direction;
- `revolution`: directrix, axis, radian `angular_interval`, optional source-carried
  `angular_parameter_interval` mapped affinely to that angular interval, optional
  source-carried directrix `parameter_interval`, and `transposed`;
- `sum`: ordered curves `first` and `second` with `basepoint`; the surface is `basepoint + first(u) + second(v)`;
- `sweep`: profile and spine;
- `offset`: support surface, signed distance, and optional source-carried U/V sense enums;
- `subset`: support surface and ordered U/V parameter intervals;
- `ruled`: two directrices;
- `blend`: two optional oriented supports, optional spine, radius law, and circular, conic, or polynomial cross-section;
- `unknown`: optional opaque-record reference.

A blend radius law is constant, linear between endpoint radii, or an explicit NURBS law. An unresolved support occupies its fixed side as `null`; omission of the semantic source is reported as decode loss.

Procedural curve definitions are intersection, projection, offset, blend spine, or unknown. Intersection keeps two fixed optional support slots. Projection identifies source curve, support surface, and optional projection direction. Offset identifies its source curve, signed distance, optional support surface, and an optional fixed plane-normal direction when the source defines a free-space planar offset. Subset identifies its source curve, an ordered interval `[first, last]` on the source parameter `t`, and a sense. A subset curve runs on its local parameter `s`, which is `0` at the first trim: `s = t - first` for a true sense and `s = last - t` for a false sense, over `[0, last - first]`. The subset's solved cache, `Curve.parameter_range`, the `Edge.param_range` of each edge on the subset, and each coedge use-curve interval on it are on `s`. A codec whose trimmed-curve record keeps the source parameter maps between `t` and `s` at decode and encode.

## Source-fidelity annotations

`SourceFidelity.unrepresented` is a sorted array of arena classes whose source records have no typed neutral or native representation. It is not digested. Nonempty sets forbid patch and replay; synthesis consumes no retained bytes. Scope and unrepresented are independent: typed native content can survive without a neutral projection.

`SourceFidelity.decode_digest` is the decode-time document digest baseline for byte reuse. It is absent when the decode declines to record one.

`SourceFidelity.annotations.streams` interns source stream names. `SourceFidelity.annotations.provenance` maps a product or retained-record ID to a stream index, byte offset, and optional source tag. Stream indices must resolve.

`SourceFidelity.annotations.exactness` maps an entity ID to entity exactness plus field overrides keyed by serialized field path. Exactness values are:

- `byte_exact`: directly represented source data, including declared unit conversion;
- `derived`: deterministic computation from byte-exact inputs;
- `inferred`: selected from context or convention;
- `unknown`: source fidelity remains unresolved.

Absence from sidecar exactness means `byte_exact` for a decoded source-backed value. A field override takes precedence over entity exactness. Codecs record every decoded entity and field whose exactness is `derived`, `inferred`, or `unknown`. Source-less product documents omit the source-fidelity sidecar. Annotation keys resolve to globally identified product entities or retained source records. Unknown field paths are warnings so additive product fields remain readable.

## Neutral feature model

Each feature and design parameter has an optional `product_definition` reference to its owning product definition. The reference must name an existing definition. The owning frame is that product definition's frame. An absent reference leaves product ownership unresolved. The parameter `owner` reference names its consuming feature and is independent of product ownership. Every output body of a feature with a product owner must occur in that definition's `bodies` list. Features without outputs can have product owners. Product ownership does not imply a body output or a dependency edge.

Each feature has an ID, source-history `ordinal`, optional name, suppression state, optional parent, output bodies, a neutral definition, and optional `native_ref`.

Neutral definitions include directly stored geometry, solid and surface construction, direct editing, body composition, sketches, datums, holes, and patterns. A stored-geometry feature identifies retained exact bodies as outputs. A body extraction identifies the source body selection independently of its copied outputs. One `unresolved` definition covers every operation kind that is established without its construction operands; its `family` field names the kind, such as `datum_plane`, `datum_coordinate_system`, `loft`, or `freeform_surface`. An edge fillet consumes an edge selection; a face blend keeps its two support-face selections distinct. Both carry a constant, variable, or unresolved radius law. `sew_bodies` joins an ordered body selection and carries an optional nonnegative gap tolerance. It remains distinct from `knit_surface`, whose operands are selected faces. A historical body set is ordered when each resolved body corresponds to the native member at the same position. A historical unordered body set records the resolved body membership and the ordered native members separately when only collective membership is established. A surface trim retains independently resolved face, path, and inside-or-outside region semantics. A surface extension independently retains its face selection, positive distance, and natural, linear, or perpendicular continuation law. `trim_bodies` keeps target and tool body selections distinct and records a forward, reverse, or unresolved retained side. `native` holds a feature with no neutral definition and carries its source kind, parameter map, and non-parameter property map. Length wrappers are millimeters and angle wrappers are radians.

Datum planes retain their operation family when placement is unresolved and carry a model-space frame when resolved. A solid primitive is defined in its feature frame: a cylinder or cone has its base centre at the frame origin and its axis along the frame z axis. Its optional `placement` is the rigid local-to-model transform of that frame; an absent placement leaves the frame unresolved. Extents are one-sided, two-sided, or symmetric around the profile plane. Each side carries a one-sided termination law: unresolved, blind, through-all, through-next, to-first, to-last, to-face, to-vertex, to-plane, offset-from-face, to-shape, or angular. A symmetric side is mirrored across the profile plane; its blind length or angular travel states the total travel split evenly around the plane. An extrusion side additionally carries an optional draft angle, measured from the profile plane outward along that side's travel, and an optional signed offset from its terminating geometry; an absent draft leaves that side's walls parallel. Revolution sides carry termination laws only. The fixed angles of a revolution total at most one turn. A total up to 1e-12 rad above 2π counts as one turn. Holes travel on one side only and state a bare termination law. Boolean operations are join, cut, intersect, or new-body. Profiles reference unresolved, native, sketch, or solved-face identity. Paths reference unresolved, native, sketch, edge, or curve identity. Projected curves retain unresolved directionality independently of an absent explicit direction vector. Draft faces, neutral plane, pull direction, angle, and side state resolve independently. Filled-surface boundaries, supports, continuity, and merge state also resolve independently. Continuity is a nonempty ordered list of boundary conditions. Each condition is `contact`, `tangent`, or `curvature`; tangent and curvature carry an optional positive dimensionless weight. Contact carries no weight. An absent weight means the source weight is unresolved. Conditions serialize as objects with `kind` and, when resolved, `weight`. Uniformity of the complete condition includes its weight. Boundary surfaces retain their operation family when their directional curve networks are unresolved. Surface-knit operands, entity merging, solid conversion, and tolerance resolve independently. Edge fillets use constant or sampled variable radii. Full-round fillets keep a center-face selection and two side-face selections; each side is explicit, automatic, or unresolved. Chamfers use distance, two distances, or distance-angle and retain reference-side reversal only when resolved. Hole entry and optional exit shapes are simple, chamfered, counterbored, or countersunk. Patterns are linear, circular, or mirrored.

Linear pattern counts define grid positions, including the seed at `(0, 0)`. An absent second direction has one position. `omitted_instances` is an optional array of zero-based `[first, second]` grid coordinates. Each coordinate is in bounds and distinct, and `(0, 0)` is excluded. An absent or empty array omits no position. The retained instance count is the product of the direction counts minus the number of omitted coordinates. Length conversion preserves these coordinates.

A revolution turns about `axis`, the line through `axis.origin` along `axis.direction`. A positive angle turns right-handed about that direction: counter-clockwise when viewed from the tip of the direction back toward its origin. A profile point on the +x side of an axis along +z therefore moves toward +y. The profile is at angle zero. `one_sided` travel runs from zero to its positive angle. `two_sided` travel runs from zero to `first` in the positive sense and from zero to `second` in the opposite sense, so `first` is the side that a positive turn reaches. `symmetric` travel runs from minus half of its angle to plus half of its angle.

An extrusion starts on its profile plane, on a plane at a signed offset from the profile plane, on a selected face, or on a plane that has no face identity. A to-plane termination and a from-plane start each carry the model-space frame of that plane: an origin in millimeters, a normal, and a `u_axis` in the plane. The normal and `u_axis` are orthogonal and need not have unit length. Each also names the native record that stores the plane. This extrusion starts on the plane z = 40 mm and ends on the plane z = 100 mm:

```json
{
  "definition": "extrude",
  "profile": { "kind": "native", "value": "native:profile" },
  "direction": { "kind": "profile_normal" },
  "start": {
    "kind": "from_plane",
    "plane": {
      "origin": { "x": 0.0, "y": 0.0, "z": 40.0 },
      "normal": { "x": 0.0, "y": 0.0, "z": 1.0 },
      "u_axis": { "x": 1.0, "y": 0.0, "z": 0.0 }
    },
    "native": "native:start-plane"
  },
  "extent": {
    "kind": "one_sided",
    "side": {
      "termination": {
        "kind": "to_plane",
        "plane": {
          "origin": { "x": 0.0, "y": 0.0, "z": 100.0 },
          "normal": { "x": 0.0, "y": 0.0, "z": 1.0 },
          "u_axis": { "x": 1.0, "y": 0.0, "z": 0.0 }
        },
        "native": "native:end-plane"
      }
    }
  },
  "op": "join"
}
```

Ordered resolved and historical body sets store nonempty member rows. Each row pairs one body identity with one nonblank native member string. Body identities and native member strings are unique within the set. Historical unordered sets retain separate body and native-member collections with equal nonzero counts and unique members; no positional correspondence is implied.

A configuration feature evaluation is `suppressed` or `active`. A suppressed evaluation has no outputs. An active evaluation owns its output list, which may be empty. Dependencies and the feature definition remain present independently of the evaluation state.

A sheet-metal base flange places its thickness along the profile plane's positive normal (`forward`), along its negative normal (`reverse`), or split equally across both sides of the plane (`symmetric`).

`SheetMetalAttachedPlate` builds a flat plate from its profile, thickness, and side. Its attachment selects the existing sheet edge. It has no Boolean operation or bend dimensions. `SheetMetalBend` names that plate and selects the attachment edge in its own input topology. It builds the joining bend region with its inside radius, relief, and transition. Replay the plate first, then its bend. The pair transfers together; an unresolved member keeps both members native. Each historical attachment selection identifies its own feature's input topology row.

A `sheet_metal_corner` operation names one preceding `sheet_metal_bend` feature through `bend`. The bend occurs in the corner's dependencies. Its explicit `effect: "unchanged"` states that the corner preserves the input body geometry and topology. It creates no faces, edges, or vertices. Source controls and dimensions remain in source properties and native records when they have no neutral construction meaning. An unchanged corner requires no generated result topology.

Loft guidance is a guide-path list or one centerline path. An empty guide-path list means unguided. Guide paths and a centerline cannot coexist.

A sketch native operand has a nonempty native kind and an optional field. A present field has a nonempty name and an optional role. A role exists only within its field.

`native_ref` identifies the full-fidelity native record corresponding to a neutral projection. The neutral definition keeps its own meaning.

`source_content` retains the ordered mixed content of a feature. Parameter items
reference entries in the document parameter arena. A referenced parameter may
be owned by another feature, including a global equations node, and may occur
more than once when the source serializes repeated consumption slots.

## Assembly joints

An assembly joint grounds one connector, or connects two connectors through a paired family. Each connector holds an operand and a frame. The frame is in the operand container's own coordinate space: the document root, the prototype of a local occurrence, or the external document. It is not composed with the occurrence placement; the connector's model-space frame is the occurrence placement applied to the frame. An operand container is the document root, a local occurrence, or an external document. An occurrence container optionally holds `occurrence_path`: the native keys of the nested occurrences, outermost first, that lead from the prototype of the occurrence to the document that owns the object. An absent path means that the prototype of the occurrence owns the object. The path identifies the placement of a part that is placed more than once; placement transforms stay on the occurrences.

`plane_coincidence` makes the two connector planes coincident. The z axis of each connector frame is the outward normal of its plane, and the frame origin lies on the plane. `sense` is `opposed` (`z₁ = −z₀`) or `aligned` (`z₁ = z₀`). The optional `offset` is the signed separation `(origin₁ − origin₀) · z₀`. `axis_coincidence` makes the two connector axes coincident. The z axis of each connector frame is the axis direction, in either sense, and the frame origin lies on the axis. `insert` makes the two circle axes coincident. The z axis of each connector frame is the normal of its circle, and the frame origin is the circle centre; both origins lie on the line that the z axis of the first connector frame spans. `sense` is `opposed` or `aligned` as for a plane coincidence, and the optional `offset` is the signed separation `(origin₀ − origin₁) · z₀`. `tangent` makes a cylindrical face of the first connector touch the plane of the second connector. The z axis of the first connector frame is the cylinder axis and its origin lies on that axis; the z axis of the second connector frame is the outward plane normal and its origin lies on the plane. `contact` is `outside` (the cylinder lies on the side that the plane normal points to) or `inside` (the opposite side). The cylinder radius is the radius of the referenced face. `point_on_axis` puts the origin of the first connector frame at the optional `distance` from the axis that the z axis of the second connector frame spans.

The optional `drives` list the motors and springs of the joint. Each drive acts on one rotation about, or one translation along, the x, y, or z axis of the first connector frame. Its output is `stiffness · (target_position − position) + damping · (target_velocity − velocity)`, limited to `max_force`, and it is applied as a force or as an acceleration. Rotations are radians and translations millimetres; stiffness is newton millimetres per radian or newtons per millimetre, damping is newton millimetre seconds per radian or newton seconds per millimetre, and the force limit is newton millimetres or newtons. An absent limit is unlimited.

A gear joint's optional `ratio` is the rotation of the second connector per rotation of the first, and a rack-and-pinion joint's `ratio` is the pinion rotation in radians per millimetre of rack translation. Each ratio is a magnitude: the connector frames give its sign. The optional `coupled_joints` of a gear or rack-and-pinion joint are empty, when the coupling acts on the connectors directly, or name two other joints of the `assembly_joints` arena: for a gear joint the joints of the first and the second gear, and for a rack-and-pinion joint the joint of the pinion and the joint of the rack, in that order. Validation checks the count and the references, not the kinds of the coupled joints.

The optional `driving_parameter` names the design parameter whose value drives the offset, distance, or angle of the joint. It resolves in the `parameters` arena. The optional `frame_sense` of a paired joint's `operands` states how the z axes of the two connector frames mate when the joint is solved: `aligned` is `z₁ = z₀` and `opposed` is `z₁ = −z₀`. A grounded joint has one connector and no `frame_sense`. When the kind states its own `sense`, as a plane coincidence or an insert does, `frame_sense` equals it or is absent.

## Physical properties

Physical records describe a target: a body in its own coordinates, an occurrence in the coordinates of its prototype, or a definition in its own coordinates for every placement of it. Masses are kilograms, densities kilograms per cubic metre, positions millimetres, and moments of inertia kilogram square millimetres.

A physical material states an optional density, static and kinetic friction coefficients, and a restitution coefficient. Mass properties state an optional mass, density, center of mass, principal moments of inertia with an optional rotation to the principal axes, and material. A stated mass takes precedence over a density, and a density over the density of the material. Mass properties, a rigid body, and a collider name a target: a body in its own coordinates, an occurrence in the coordinates of its prototype, or every placement of a definition in the definition's coordinates. A rigid body marks its target as one body that moves in a simulation; it states whether it is kinematic and whether it is enabled, an optional initial linear velocity in millimetres per second and angular velocity in radians per second in the coordinates that the target's placement maps into, and whether it starts at rest until a contact or a force wakes it. An articulation names the root of a tree of rigid bodies that the joints connect and that the simulation solves as one system: a body in the tree, or an occurrence or a definition that holds the tree. A collider marks the geometry of a target for contact, with a contact shape (the geometry itself, a convex hull, a convex decomposition, a bounding box, a bounding sphere, a simplified mesh, or an application-defined approximation), an enabled state, and an optional material.

A physics scene states the gravity that acts on its rigid bodies: an optional unit direction in model coordinates and an optional magnitude in millimetres per square second. An absent direction acts against the up axis, and an absent magnitude is standard gravity, 9806.65 millimetres per square second.

Solver settings hold the properties that one solver schema states for one record, for a solver such as `physx` and a schema with its instance name, such as `PhysxLimitAPI:angular`. The target is a physics scene, an occurrence, a definition, a body, a collider, an assembly joint, a physical material, or an articulation. A property value is a bool, an integer, a real or a list of reals with a dimension, an unbounded real, a token or a list of tokens, or a list of targets. A dimension states the exponents of length, mass, and angle: a value with exponents `(l, m, a)` is in millimetres to the power `l`, kilograms to the power `m`, and radians to the power `a`. Each target, in the settings and in their values, names a record of the model.

## Native namespaces

A native namespace is a map of arena name to arena.

Fusion native data includes ACT, Design, persistent-reference, sketch-link, construction-recipe, and ASM-history records. SOLIDWORKS native data includes feature histories and feature-input lanes. Inventor native data includes RSe segment inventories, OLE property sets, Protein package assets, external-reference records, presentation joins, and design-parameter, sketch, and feature arenas. Bare SAT streams retain ASM-native topology and unknown SAB records under `native.sat`.

NX native data retains the ordered UG_PART segment index with validated compressed-stream, body-image alias, and role-classified OM-section links. Parasolid attribute-class declarations keep exact field descriptors, topology attribute-list ownership, and counted integer, binary64, and string value records. OM retention covers internally pointed record-area headers and byte identities; object-ID-bounded records; section-scoped class and member declarations with bounded registry suffixes and structured class-layout fingerprints; offset-only store control and column blocks with atomic store-local class-selection lanes; ordered references to uniquely resolved object records and parameter declarations; product-terminated control indices; and complete counted same-store block-index lanes.

Feature-operation records are exactly bounded and ordered. They retain separately identified post-label payloads, ordered self-framed payload strings, typed simple-hole templates, variable-cardinality duplicated scalar lanes, shared construction identities, redundantly witnessed simple-hole planar placements and same-store construction-block references, resolved datum-coordinate-system construction lanes, exact operation-input block reuse, ordered body-reference field occurrences, sketch payload-to-data-block references, extrusion profile-reference lists, shifted-IEEE extrusion scalar headers, typed post-body scalar triples, labels with four object-index slots, unambiguous primary-body writers, and ordered Boolean target/tool bindings. Indexed-store product and version headers remain with those records.

Sketch retention reconstructs exact payloads across ordered column boundaries, including framed sketch scalar and name fields, name-delimited scalar groups, and complete named two-dimensional sketch points. Expressions keep exact-name links to object-ID-bounded parameter declarations, parsed parameter indices, qualifiers, declaration-local constant literals, formulas, dependencies, and values. Ordered operation-input-to-parameter bindings, framed entity strings, ordered entity-reference occurrences with resolved same-section targets, grouped persistent handles, indexed external-reference handle sets, end-anchored external child-part strings, arrangements, and typed part attributes travel with the same namespace.

NX datum-coordinate-system payloads retain complete framed scalar fields with exact source offsets. NX assembly placement records retain complete frame bytes, row-major matrices in their serialized translation unit, labels, UUIDs, and container-image offsets. Native validation checks all derived fields against the frame bytes. NX JPEG previews with valid bounded marker structure and embedded TIFF material textures transfer to exact neutral document assets. NX native data retains TIFF metadata and exact QAF stored-path-to-logical-material-path catalog relations. Those relations identify texture assets and logical names and leave body and face appearance assignment to the neutral appearance model. Topology-owned Parasolid type-81 attribute instances retain exact class relations to same-stream type-79 definitions selected by their serialized discriminators. Class-specific field-value roles remain native-only. Byte layout for these records lives in [`formats/siemens_nx.md`](formats/siemens_nx.md).

Native records retain typed references into the neutral model. Format-neutral consumers treat foreign native records as opaque. An exporter preserves a supported namespace or reports its omission as loss. Native IDs participate in global uniqueness. A consumer that does not know a namespace still processes the neutral model and treats that namespace as opaque.

## Presentation, attributes, and source fidelity

Tessellations are display meshes independent of exact B-rep geometry. Appearances describe visual or physical assets. Appearance bindings assign appearances to topology entities, tessellations, triangle groups, or native source carriers. A triangle-group target names a tessellation and the `source_id` of one of its triangle groups, and takes precedence over a binding of the whole tessellation for the triangles of the group. A binding's optional `visible` field is `None` when the source provides no binding-level visibility value and is `false` when that binding is explicitly hidden; binding visibility does not change visibility on a shared geometry carrier. Drawings preserve page, view, and annotation entities. A drawing's optional `visible` field is `None` when the source provides no drawing-level visibility value and is `false` when that drawing entity is explicitly hidden; drawing visibility does not change visibility on its relationships or contents. PMI annotations preserve semantic and graphical annotation entities. A PMI annotation's optional `visible` field is `None` when the source provides no annotation-level visibility value and is `false` when that annotation occurrence is explicitly hidden; annotation visibility does not change visibility on a shared geometry or tessellation carrier. Presentation layers group model or presentation items. A layer's optional `visible` field is `None` when the source provides no layer-level visibility value and is `false` when the layer is explicitly hidden; layer visibility does not change visibility on its assigned items. Attributes attach source-native values to supported targets.

A drawing's `sheet` holds positive width and height in millimetres and ordered,
uniquely named title-block fields. A page can obtain its sheet through its
`template` identity. `curves` is an ordered collection in the owning view frame:
X right, Y up, millimetres before view scale and rotation. Each curve has required
`visible` or `hidden` visibility and `projected_model_edge`, `cosmetic`, or
`center_line` source. Its optional `displayed` flag is independent presentation
visibility. Geometry is a finite line, a positive-radius circle, or an arc with
three distinct points on its circle within relative radial error 1e-9.

`template_geometry` belongs only to templates and holds ordered curves and
baseline-positioned texts in physical page millimetres, X right and Y up. It
reuses `DrawingCurveGeometry` in this page frame. Each text has a resolved string,
finite baseline position, and positive em height in millimetres. `x_direction`
belongs only to view, projection, section, or detail drawings and gives their
finite nonzero horizontal model direction.

A semantic dimension has one `dimension_kind`, optional `value`, format,
`text_override`, signed nonzero `upper_tolerance` and `lower_tolerance`, and text
position. Linear values and tolerances are millimetres; angular values and
tolerances are degrees. Its `reference_anchors` is empty or contains two ordered
finite page-space reference points in millimetres. A nonempty pair requires a
typed dimension owner. Anchors do not define dimension lines, arrows, or text
orientation. Annotation `text_style` holds an optional positive physical em
height and optional stored font family without substitution. No second dimension
kind, unit selector, or sheet-size field exists.

`Tessellation.triangles` preserves source winding. Each triangle index addresses
`vertices`. `feature_edges` is the
source-classified undirected feature-edge set. The list is lexicographically
sorted. Each pair is strictly ascending, unique, and indexes `vertices`; an
ordinary triangulation edge is absent unless the source classifies it as a
feature. `normals` is empty or parallel to `vertices`. `corner_normals` is empty
or parallel to the flattened triangle corner sequence. Corner normals preserve
normal seams without duplicating vertices and take precedence when an exporter
supports only one normal domain.

`triangle_groups` is empty or is an ordered partition of all triangle ordinals.
Each group is nonempty, its ordinals are strictly increasing, and a nonempty
`source_id` is unique within the tessellation. `texture_assignments` associates
one source texture resource and asset with each nonempty, strictly increasing
triangle subset. Nonempty texture-resource identities are unique. Anonymous
assignments use an asset at most once. Distinct source resources can use the
same asset. Assignment subsets do not overlap, and an omitted triangle has no
direct texture assignment.

`texture_coordinates` holds named sets of `(u, v)` values. A `vertex` set has
one value for each vertex, and a `corner` set one value for each triangle
corner, in the flattened triangle corner sequence. Set names are unique within
the tessellation. `TextureMap2d.coordinates` names the set that a texture
mapping reads; when it is absent, the mapping reads the source's default set.

A tessellation channel stores `count` fixed-width values in `data`. A vertex
channel uses implicit vertex-order addressing and has no `indices`. A corner
channel has one selector for each triangle corner, and a triangle channel has
one selector for each triangle. Every selector is less than `count`; the
selected value is the channel element at `selector`.

An unknown product record has an ID and related entity IDs. Source offset, byte length, digest, and retained source bytes belong to the matching `SourceFidelity.retained_records` entry. Source-only records may omit a product record. Retained sidecar bytes use standard RFC 4648 base64 with padding and no line breaks. Native byte strings that are product values and tessellation byte channels remain product data.

### Source attribute values

`SourceAttribute.values` is an ordered array of nullable typed values. A present
value has adjacent `kind` and `value` fields. A slot without a decoded value is
JSON `null`; the source producer uses that state only for a given-up value
part, at its original position. The other positions retain their order.
`Some(value)` has the same wire form as `value`. An empty array contains no
value slots. It is distinct from a missing slot and from an absent attribute.

The scalar kinds are `integer` (`i64`), `float` (finite binary64), `string`,
`boolean`, `reference` (one local object identity), and `vector` (finite
binary64 components). `map` holds an ordered array of `{key, value}` text pairs;
repeated keys remain separate entries. `expression` holds `path`, a typed
`Expression`, and nullable `comment`. Parameter tokens refer to parameters;
text tokens, paths, comments, and map text are not identity references.

`sub_reference` holds nullable `object` and an ordered, nonempty `subelements`
array. Its present object is a local native object identity. `external_reference`
holds `document`, nullable `object`, and ordered `subelements`; its document
and object are external authored names. A null object means a readable empty
object name. An empty string does not represent an absent object.
`empty_reference` is a unit kind without a `value` field. It is a readable
local link with no object and no subelement. It is distinct from a null slot.

`json` holds an `AttributeJson` tree. Every tree node also uses adjacent `kind`
and `value` fields: `null` is a unit kind, `boolean` holds a boolean, `integer`
holds `i64`, `unsigned` holds an integer from 9223372036854775808 through
18446744073709551615, `float` holds finite binary64, `string` holds text,
`array` holds ordered nodes, and `object` holds a map from text keys to nodes.
Object keys are serialized in sorted order. JSON keys and strings are literal
content, including text that resembles a CADIR identity. A JSON null is
`{"kind":"json","value":{"kind":"null"}}`; it is not an absent value slot.
Unknown node kinds and fields are invalid.

## Validation

Validation uses reference lookup and in-IR arithmetic. It checks:

- non-empty globally unique IDs and strict arena ordering;
- document and per-entity tolerance bounds;
- all neutral and native references;
- loop closure, radial-ring closure, and same-edge radial membership;
- wire-edge and free-vertex ownership;
- reachability of surface, curve, pcurve, and point carriers;
- structural validity of subdivision surfaces and their source associations;
- directed, closed subdivision face rings with continuous endpoints;
- annotation entity, stream, and field-path integrity;
- canonical periodic parameter domains;
- edge curve endpoints at the edge's vertices, in the edge's direction;
- finite coordinates, unit directions, positive radii, and NURBS shape invariants;
- tessellation channel and index bounds, canonical feature-edge pairs, and
  vertex- and corner-normal cardinalities;
- native record counts, IDs, links, and payload spans;
- opaque payload length and SHA-256;
- retained-record byte length and SHA-256 digest.

Structural failures are errors. Same-sense two-member radial rings, unknown annotation field paths, and tolerances outside sane canonical ranges are warnings where the representation remains unambiguous. `ValidationReport::is_ok()` is true when no error or blocking finding exists. Decode and export loss notes are reported separately and leave this predicate unchanged.

## Version policy and JSON Schema

Readers provide no migration.

Readers accept exactly `ir_version: "10"`. The `model.subds` arena is required, including when empty. Source annotations and retained records are excluded from the neutral product model. Recursive affine-transformed curve and surface carriers preserve exact source parameterization under occurrence placement. Maintainers select the IR version for each release. After a release, removing or renaming a product field, or changing its type, units, parameterization, or invariant, requires a new IR version. New product fields carry identity, units, ordering, reference, and validation contracts.

`Sweep.section` is a required sum type and `Sweep.sections` is a same-typed list. A sweep section is unresolved, references a `ProfileRef`, or owns generated section geometry. A generated circular region stores its outer radius and optional inward wall thickness.

JSON Schema is generated per IR version by `cadmpeg_ir::cadir_json_schema()`, which requires the crate's `schema` feature.

## Version history

Version 6 was written inconsistently by releases 0.6.0–0.6.1 and 0.6.2. Versions 7, 8 and 9 record `decode_depth` instead of `scope`. Version 10 replaces that depth field with content scope and changes decode transfer to carry scope. Readers refuse every earlier wire version.

## Worked cube

[`emit_cube.rs`](../crates/cadmpeg-ir/examples/emit_cube.rs) emits a 10 mm solid cube with one region, one shell, six planar faces, twelve line edges, eight vertices, and twenty-four coedges. Every edge has a two-member radial ring.

The generated document begins with this complete hierarchy and representative radial pair:

```json
{
  "ir_version": "10",
  "scope": { "model": true, "design": true },
  "units": { "length": "millimeter" },
  "tolerances": { "linear": 1e-6, "angular": 1e-10 },
  "model": {
    "bodies": [
      {
        "id": "body0",
        "kind": "solid",
        "regions": ["region0"],
        "name": "unit cube"
      }
    ],
    "regions": [{ "id": "region0", "body": "body0", "shells": ["shell0"] }],
    "shells": [
      {
        "id": "shell0",
        "region": "region0",
        "faces": ["f_bottom", "f_top", "f_front", "f_right", "f_back", "f_left"]
      }
    ],
    "faces": [
      {
        "id": "f_bottom",
        "shell": "shell0",
        "surface": "srf_bottom",
        "sense": "forward",
        "loops": ["lp_bottom"],
        "name": "bottom face"
      }
    ],
    "loops": [
      {
        "id": "lp_bottom",
        "face": "f_bottom",
        "coedges": ["ce_bottom_0", "ce_bottom_1", "ce_bottom_2", "ce_bottom_3"]
      }
    ],
    "coedges": [
      {
        "id": "ce_bottom_0",
        "owner_loop": "lp_bottom",
        "edge": "e0",
        "next": "ce_bottom_1",
        "previous": "ce_bottom_3",
        "radial_next": "ce_front_0",
        "sense": "reversed"
      },
      {
        "id": "ce_front_0",
        "owner_loop": "lp_front",
        "edge": "e0",
        "next": "ce_front_1",
        "previous": "ce_front_3",
        "radial_next": "ce_bottom_0",
        "sense": "forward"
      }
    ],
    "edges": [
      {
        "id": "e0",
        "curve": "crv_e0",
        "start": "v0",
        "end": "v1",
        "param_range": [0.0, 10.0]
      }
    ],
    "vertices": [
      { "id": "v0", "point": "p0" },
      { "id": "v1", "point": "p1" }
    ],
    "points": [
      { "id": "p0", "position": { "x": 0.0, "y": 0.0, "z": 0.0 } },
      { "id": "p1", "position": { "x": 10.0, "y": 0.0, "z": 0.0 } }
    ],
    "surfaces": [
      {
        "id": "srf_bottom",
        "geometry": {
          "kind": "plane",
          "origin": { "x": 0.0, "y": 0.0, "z": 0.0 },
          "normal": { "x": 0.0, "y": 0.0, "z": -1.0 },
          "u_axis": { "x": 1.0, "y": 0.0, "z": 0.0 }
        }
      }
    ],
    "curves": [
      {
        "id": "crv_e0",
        "geometry": {
          "kind": "line",
          "origin": { "x": 0.0, "y": 0.0, "z": 0.0 },
          "direction": { "x": 1.0, "y": 0.0, "z": 0.0 }
        }
      }
    ]
  },
  "native": {}
}
```

The extract omits repeated faces, loops, coedges, edges, vertices, points, surfaces, and curves. Regenerate the complete canonical artifact with:

```sh
cargo run -p cadmpeg-ir --example emit_cube > cube.cadir.json
```
