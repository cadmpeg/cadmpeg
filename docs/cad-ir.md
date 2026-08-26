<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# cadmpeg IR (`.cadir.json`) specification

`CadIr` is the versioned JSON product representation shared by codecs, validation, diffing, and encoders. This specification defines the current required IR version `"5"`. The `cadmpeg-ir` Rust types define field-level JSON types, and `cadir_json_schema()` derives the matching JSON Schema.

## Document layering

A product document has three semantic layers:

```text
CadIr
├── ir_version, source?, units, tolerances
├── model
│   ├── topology and geometry carriers
│   ├── procedural constructions and neutral features
│   └── tessellation, appearance, and attributes
└── native
```

`model` is format-neutral. `native` is a map keyed by format ID. Each value contains an integer `version` and an `arenas` map. Each arena is an ID-sorted array of records with a required string `id` and codec-owned fields. The reserved `unknowns` arena stores format-specific product records. Decode-time source locations, exactness, and retained source records belong to the independently versioned `SourceFidelity` sidecar. Namespace retention is settled in [native-arena-disposition.md](native-arena-disposition.md): retain every listed arena.

The neutral model arenas, in serialization order, are `bodies`, `regions`, `shells`, `faces`, `loops`, `coedges`, `edges`, `vertices`, `points`, `surfaces`, `curves`, `subds`, `pcurves`, `procedural_surfaces`, `procedural_curves`, `assets`, `features`, `feature_input_topologies`, `configurations`, `parameters`, `sketches`, `sketch_entities`, `sketch_constraints`, `spatial_sketches`, `spatial_sketch_entities`, `spatial_sketch_constraints`, `spreadsheets`, `product_definitions`, `occurrences`, `assembly_joints`, `drawings`, `semantic_annotations`, `presentation_documents`, `view_presentations`, `tessellations`, `appearances`, `appearance_bindings`, `attributes`, `pmi`, and `presentation_layers`. References are string IDs. `subds` contains subdivision-surface control cages and is a free carrier arena.

Pcurve geometry is a parameter-space line, angular circle, angular ellipse, parabola, hyperbola, first-order harmonic, first-order hyperbolic, polar harmonic, polar NURBS, NURBS, trimmed, or signed-offset curve. Circle and ellipse carriers store independent `x_axis` and `y_axis` parameter directions; a clockwise parameterization has a negated `y_axis`. General harmonic carriers evaluate `center + cosine*cos(t) + sine*sin(t)`; general hyperbolic carriers evaluate `center + cosine*cosh(t) + sine*sinh(t)`. A polar harmonic maps first-order radial-plane and axial harmonic coefficients to `(atan2(y, x), v)` without changing the spatial curve parameter. A polar NURBS evaluates radial-plane and axial control channels with one degree, knot vector, weight vector, and parameter, then maps the radial result through `atan2`. A signed offset adds its distance along the regular basis curve's left unit normal. Point evaluation requires a finite nonzero exact basis tangent; a nested signed offset has no point evaluation.

Maps serialize with lexicographically sorted keys. Arena entries are strictly sorted by ID. Canonical serialization uses that sorted order.

## Identity and order

Entity IDs have the grammar:

```text
<format>:<scope>:<kind>#<key>
```

`format` identifies the producing codec or `synthetic`. `scope` identifies the containing source object or stream. `kind` names the entity class. `key` is the source persistent key when one exists and otherwise a positional ordinal.

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
| native namespace | Versioned source-specific data outside the neutral model       |
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

`Body.kind` is `solid`, `sheet`, `wire`, or `general`. A body optionally records a display name, color, and `visible`. `visible` states whether the source document displays the body. Exporters omit bodies with `visible: false` from display-oriented formats. A body owns regions. A region is a connected component of a body and owns shells. A shell owns at least one of faces, wire edges, or free vertices. A face is an oriented bounded portion of one surface and owns loops. A loop's boundary role is `outer`, `inner`, or `unspecified`; a face has at most one explicit outer loop and may have only inner loops when the surface parameter domain supplies the exterior. An edge loop lists coedges in traversal order and may contain ordered pole-vertex uses anchored after a coedge. A vertex loop contains one unanchored vertex use and no coedges. A coedge and a pole-vertex use each own an ordered list of parameter-space curve uses. Each pcurve use may record whether the source declared it isoparametric. An edge joins two vertices and optionally references a curve and canonical parameter range. A vertex references a point carrier. Point remains a separate carrier because it has independent identity and provenance.

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

A loop is either a nonempty `coedges` ring or one unanchored vertex use. For every edge loop, `coedges` contains exactly one simple cycle. Each coedge's `next` and `previous` links are reciprocal and remain within that loop. Pole-vertex uses in an edge loop identify their preceding member with `after`; multiple uses after one coedge retain vector order. A vertex loop contains no coedges and exactly one vertex use whose `after` is absent.

All coedges that use an edge form one closed radial ring through `radial_next`. Every member references the same edge:

- one member is a laminar boundary and points to itself;
- two members are manifold adjacency;
- three or more members are legal non-manifold adjacency.

A two-member ring may use opposite or equal senses. Equal senses produce a validation warning.

### Wires and free vertices

A wire edge appears in exactly one shell's `wire_edges` and in no coedge. A free vertex appears in exactly one shell's `free_vertices` and bounds no edge. A `wire` body contains no faces. `solid` and `sheet` bodies use face topology; `general` bodies may mix dimensionalities.

## Geometry and canonical parameterization

Surface carriers are plane, cylinder, cone, sphere, torus, NURBS, procedural, or unknown. Curve carriers are line, circle, ellipse, parabola, hyperbola, degenerate, NURBS, procedural, or unknown. Pcurves are analytic, first-order harmonic, first-order hyperbolic, polar, NURBS, trimmed, or signed-offset curves in a surface's `(u, v)` space. A subdivision surface is a Catmull–Clark control cage with vertices, edges, directed face edge uses, endpoint sharpness, edge tags, vertex tags, and sector coefficients.

Free surface, curve, subdivision-surface, and tessellation carriers may carry a `SourceObjectAssociation`. The association records the source format and native object identifier, effective name, color, visibility, layer, and outermost-to-innermost instance path. These fields preserve source-object identity and display metadata when no topology entity owns the carrier.

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

`Edge.param_range` uses the canonical parameterization of its curve when a 3D carrier exists. A carrier-less degenerate or tolerant edge has no canonical 3D domain; its optional range retains finite native endpoint doubles without imposing carrier-domain ordering. Full circles are anchored to `[0, 2π]`. Periodic ranges may cross a seam by using an end value greater than the start value in the unwrapped domain. Pcurve coordinates use the corresponding surface conventions.

Decoders convert kernel conventions at decode:

- NX/Parasolid linear parameters expressed in meters are multiplied by 1000. Unit conversion preserves `byte_exact` status.
- CATIA cylindrical arc-length coordinates use `u = rθ` and are divided by radius. The converted field is `derived`.
- CATIA conical angular coordinates already use the canonical azimuth and are unchanged.
- Fusion ellipse phases are normalized to the major-direction origin and marked `derived`.
- Kernel full-circle intervals are re-anchored to `[0, 2π]` and marked `derived`.

NURBS surfaces store degrees, full knot vectors, pole counts, u-major control points, optional per-pole weights, and periodicity flags. NURBS curves store degree, full knot vector, ordered control points, optional weights, and periodicity.

## Procedural carriers

Procedural entities retain construction semantics either as a surface or curve carrier or beside a solved carrier. `SurfaceGeometry::Procedural.construction` and `CurveGeometry::Procedural.construction` identify the construction that exactly defines the carrier; the referenced construction identifies that carrier in return. This bidirectional relation is required. A procedural construction with an analytic or NURBS carrier retains both the construction and its solved representation. Model-aware evaluation resolves nested offset carriers recursively and rejects reference cycles; the support normal is the normalized cross product of exact analytic, rational NURBS, or affine-transformed parameter tangents. A regular parallel offset retains that oriented unit normal for a dependent offset. Other procedural families require a solved carrier or a family evaluator. `cache_fit_tolerance`, when present, is the maximum millimeter deviation between the procedural definition and solved carrier. A pcurve's `fit_tolerance` likewise bounds the model-space deviation after mapping the pcurve through its coedge's face surface.

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

Procedural curve definitions are intersection, projection, offset, blend spine, or unknown. Intersection keeps two fixed optional support slots. Projection identifies source curve, support surface, and optional projection direction. Offset identifies its source curve, signed distance, optional support surface, and an optional fixed plane-normal direction when the source defines a free-space planar offset.

## Source-fidelity annotations

`SourceFidelity.annotations.streams` interns source stream names. `SourceFidelity.annotations.provenance` maps a product or retained-record ID to a stream index, byte offset, and optional source tag. Stream indices must resolve.

`SourceFidelity.annotations.exactness` maps an entity ID to entity exactness plus field overrides keyed by serialized field path. Exactness values are:

- `byte_exact`: directly represented source data, including declared unit conversion;
- `derived`: deterministic computation from byte-exact inputs;
- `inferred`: selected from context or convention;
- `unknown`: source fidelity remains unresolved.

Absence from sidecar exactness means `byte_exact` for a decoded source-backed value. A field override takes precedence over entity exactness. Codecs record every decoded entity and field whose exactness is `derived`, `inferred`, or `unknown`. Source-less product documents omit the source-fidelity sidecar. Annotation keys resolve to globally identified product entities or retained source records. Unknown field paths are warnings so additive product fields remain readable.

## Neutral feature model

Each feature has an ID, source-history `ordinal`, optional name, suppression state, optional parent, output bodies, a neutral definition, and optional `native_ref`.

Neutral definitions include directly stored geometry, solid and surface construction, direct editing, body composition, sketches, datums, holes, and patterns. A stored-geometry feature identifies retained exact bodies as outputs. A body extraction identifies the source body selection independently of its copied outputs. Constructed datum planes, coordinate systems, lofts, and freeform surfaces have distinct unresolved-family variants when their operation kind is established but their construction operands are not. An edge fillet consumes an edge selection; a face blend keeps its two support-face selections distinct. Both carry a constant, variable, or unresolved radius law. `sew_bodies` joins an ordered body selection and carries an optional nonnegative gap tolerance. It remains distinct from `knit_surface`, whose operands are selected faces. A historical body set is ordered when each resolved body corresponds to the native member at the same position. A historical unordered body set records the resolved body membership and the ordered native members separately when only collective membership is established. A surface trim retains independently resolved face, path, and inside-or-outside region semantics. A surface extension independently retains its face selection, positive distance, and natural, linear, or perpendicular continuation law. `trim_bodies` keeps target and tool body selections distinct and records a forward, reverse, or unresolved retained side. `native` holds a feature with no neutral definition and carries its source kind, parameter map, and non-parameter property map. Length wrappers are millimeters and angle wrappers are radians.

Datum planes retain their operation family when placement is unresolved and carry a model-space frame when resolved. Extents are one-sided, two-sided, or symmetric around the profile plane. Each side carries a one-sided termination law: unresolved, blind, through-all, through-next, to-first, to-last, to-face, to-vertex, offset-from-face, to-shape, or angular. A symmetric side is mirrored across the profile plane; its blind length or angular travel states the total travel split evenly around the plane. An extrusion side additionally carries an optional draft angle, measured from the profile plane outward along that side's travel, and an optional signed offset from its terminating geometry; an absent draft leaves that side's walls parallel. Revolution sides carry termination laws only. Holes travel on one side only and state a bare termination law. Boolean operations are join, cut, intersect, or new-body. Profiles reference unresolved, native, sketch, or solved-face identity. Paths reference unresolved, native, sketch, edge, or curve identity. Projected curves retain unresolved directionality independently of an absent explicit direction vector. Draft faces, neutral plane, pull direction, angle, and side state resolve independently. Filled-surface boundaries, supports, continuity, and merge state also resolve independently. Boundary surfaces retain their operation family when their directional curve networks are unresolved. Surface-knit operands, entity merging, solid conversion, and tolerance resolve independently. Edge fillets use constant or sampled variable radii. Full-round fillets keep a center-face selection and two side-face selections; each side is explicit, automatic, or unresolved. Chamfers use distance, two distances, or distance-angle and retain reference-side reversal only when resolved. Hole entry and optional exit shapes are simple, chamfered, counterbored, or countersunk. Patterns are linear, circular, or mirrored.

`native_ref` identifies the full-fidelity native record corresponding to a neutral projection. The neutral definition keeps its own meaning.

`source_content` retains the ordered mixed content of a feature. Parameter items
reference entries in the document parameter arena. A referenced parameter may
be owned by another feature, including a global equations node, and may occur
more than once when the source serializes repeated consumption slots.

## Native namespaces

A native namespace version declares which arena set and which record shapes a
stored document holds, and it rises when either changes.

When present, native namespace versions are:

| Namespace         | Version |
| ----------------- | ------- |
| `native.f3d`      | 13      |
| `native.sldprt`   | 13      |
| `native.nx`       | 189     |
| `native.inventor` | 25      |
| `native.fcstd`    | 22      |
| `native.catia`    | 276     |
| `native.creo`     | 1       |
| `native.rhino`    | 2       |
| `native.iges`     | 5       |
| `native.sat`      | 1       |

Fusion native data includes ACT, Design, persistent-reference, sketch-link, construction-recipe, and ASM-history records. SOLIDWORKS native data includes feature histories and feature-input lanes. Inventor native data includes RSe segment inventories, OLE property sets, Protein package assets, external-reference records, presentation joins, and design-parameter, sketch, and feature arenas. Bare SAT streams retain ASM-native topology and unknown SAB records under `native.sat`; its version 1 is the IR's default for a namespace that declares no shape revision of its own, not a version the codec stamps.

NX native data retains the ordered UG_PART segment index with validated compressed-stream, body-image alias, and role-classified OM-section links. Parasolid attribute-class declarations keep exact field descriptors, topology attribute-list ownership, and counted integer, binary64, and string value records. OM retention covers internally pointed record-area headers and byte identities; object-ID-bounded records; section-scoped class and member declarations with bounded registry suffixes and structured class-layout fingerprints; offset-only store control and column blocks with atomic store-local class-selection lanes; ordered references to uniquely resolved object records and parameter declarations; product-terminated control indices; and complete counted same-store block-index lanes.

Feature-operation records are exactly bounded and ordered. They retain separately identified post-label payloads, ordered self-framed payload strings, typed simple-hole templates, variable-cardinality duplicated scalar lanes, shared construction identities, redundantly witnessed simple-hole planar placements and same-store construction-block references, resolved datum-coordinate-system construction lanes, exact operation-input block reuse, ordered body-reference field occurrences, sketch payload-to-data-block references, extrusion profile-reference lists, shifted-IEEE extrusion scalar headers, typed post-body scalar triples, labels with four object-index slots, unambiguous primary-body writers, and ordered Boolean target/tool bindings. Indexed-store product and version headers remain with those records.

Sketch retention reconstructs exact payloads across ordered column boundaries, including framed sketch scalar and name fields, name-delimited scalar groups, and complete named two-dimensional sketch points. Expressions keep exact-name links to object-ID-bounded parameter declarations, parsed parameter indices, qualifiers, declaration-local constant literals, formulas, dependencies, and values. Ordered operation-input-to-parameter bindings, framed entity strings, ordered entity-reference occurrences with resolved same-section targets, grouped persistent handles, indexed external-reference handle sets, end-anchored external child-part strings, arrangements, and typed part attributes travel with the same namespace.

NX datum-coordinate-system payloads retain complete framed scalar fields with exact source offsets. NX JPEG previews with valid bounded marker structure and embedded TIFF material textures transfer to exact neutral document assets. NX native data retains TIFF metadata and exact QAF stored-path-to-logical-material-path catalog relations. Those relations identify texture assets and logical names and leave body and face appearance assignment to the neutral appearance model. Topology-owned Parasolid type-81 attribute instances retain exact class relations to same-stream type-79 definitions selected by their serialized discriminators. Class-specific field-value roles remain native-only. Byte layout for these records lives in [`formats/siemens_nx.md`](formats/siemens_nx.md).

Native records retain typed references into the neutral model. Format-neutral consumers treat foreign native records as opaque. An exporter preserves a supported namespace or reports its omission as loss. Native IDs participate in global uniqueness. Namespace versions change independently of `ir_version`. A consumer that omits a namespace version still processes the neutral model and treats that namespace as opaque.

## Presentation, attributes, and source fidelity

Tessellations are display meshes independent of exact B-rep geometry. Appearances describe visual or physical assets. Appearance bindings assign appearances to topology entities or native source carriers. A binding's optional `visible` field is `None` when the source provides no binding-level visibility value and is `false` when that binding is explicitly hidden; binding visibility does not change visibility on a shared geometry carrier. Drawings preserve page, view, and annotation entities. A drawing's optional `visible` field is `None` when the source provides no drawing-level visibility value and is `false` when that drawing entity is explicitly hidden; drawing visibility does not change visibility on its relationships or contents. PMI annotations preserve semantic and graphical annotation entities. A PMI annotation's optional `visible` field is `None` when the source provides no annotation-level visibility value and is `false` when that annotation occurrence is explicitly hidden; annotation visibility does not change visibility on a shared geometry or tessellation carrier. Presentation layers group model or presentation items. A layer's optional `visible` field is `None` when the source provides no layer-level visibility value and is `false` when the layer is explicitly hidden; layer visibility does not change visibility on its assigned items. Attributes attach source-native values to supported targets.

`Tessellation.triangles` preserves source winding. `feature_edges` is the
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

A tessellation channel stores `count` fixed-width values in `data`. A vertex
channel uses implicit vertex-order addressing and has no `indices`. A corner
channel has one selector for each triangle corner, and a triangle channel has
one selector for each triangle. Every selector is less than `count`; the
selected value is the channel element at `selector`.

An unknown product record has an ID and related entity IDs. Source offset, byte length, digest, and retained source bytes belong to the matching `SourceFidelity.retained_records` entry. Source-only records may omit a product record. Retained sidecar bytes use standard RFC 4648 base64 with padding and no line breaks. Native byte strings that are product values and tessellation byte channels remain product data.

## Validation

Validation uses reference lookup and in-IR arithmetic. It checks:

- exact IR and native namespace versions;
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
- finite coordinates, unit directions, positive radii, and NURBS shape invariants;
- tessellation channel and index bounds, canonical feature-edge pairs, and
  vertex- and corner-normal cardinalities;
- native record counts, IDs, links, and payload spans;
- opaque payload length and SHA-256;
- retained-record byte length and SHA-256 digest.

Structural failures are errors. Same-sense two-member radial rings, unknown annotation field paths, and tolerances outside sane canonical ranges are warnings where the representation remains unambiguous. `ValidationReport::is_ok()` is true when no error or blocking finding exists. Decode and export loss notes are reported separately and leave this predicate unchanged.

## Version policy and JSON Schema

Readers accept exactly `ir_version: "5"`. The `model.subds` arena is required, including when empty. Source annotations and retained records are excluded from the neutral product model. Recursive affine-transformed curve and surface carriers preserve exact source parameterization under occurrence placement. Removing or renaming a product field, or changing its type, units, parameterization, or invariant, requires a new IR version. New product fields carry identity, units, ordering, reference, and validation contracts.

Version 5 replaces the optional `Sweep.profile` field and profile-only `Sweep.sections` list with the required `Sweep.section` sum type and a same-typed `Sweep.sections` list. A sweep section is unresolved, references a `ProfileRef`, or owns generated section geometry. A generated circular region stores its outer radius and optional inward wall thickness.

Native namespaces use their own integer versions. A native-only semantic change increments that namespace version without changing the neutral IR version. JSON Schema is generated per IR version by `cadmpeg_ir::cadir_json_schema()`, which requires the crate's `schema` feature.

## Worked cube

[`emit_cube.rs`](../crates/cadmpeg-ir/examples/emit_cube.rs) emits a 10 mm solid cube with one region, one shell, six planar faces, twelve line edges, eight vertices, and twenty-four coedges. Every edge has a two-member radial ring.

The generated document begins with this complete hierarchy and representative radial pair:

```json
{
  "ir_version": "5",
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
        "sense": "forward"
      },
      {
        "id": "ce_front_0",
        "owner_loop": "lp_front",
        "edge": "e0",
        "next": "ce_front_1",
        "previous": "ce_front_3",
        "radial_next": "ce_bottom_0",
        "sense": "reversed"
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
