<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# cadmpeg IR (`.cadir.json`) specification

`CadIr` is the versioned JSON product representation shared by codecs, validation, diffing, and encoders. This specification defines the current required IR version `"55"`. The `cadmpeg-ir` Rust types define field-level JSON types, and `cadir_json_schema()` derives the matching JSON Schema.

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

`model` is format-neutral. `native` is a map keyed by format ID. Each value contains an integer `version` and an `arenas` map. Each arena is an ID-sorted array of records with a required string `id` and codec-owned fields. The reserved `unknowns` arena stores format-specific product records. Decode-time source locations, exactness, retained source records, and byte ownership belong to the independently versioned `SourceFidelity` sidecar and are not serialized in `CadIr`.

The neutral model arenas, in serialization order, are `bodies`, `regions`, `shells`, `faces`, `loops`, `coedges`, `edges`, `vertices`, `points`, `surfaces`, `curves`, `subds`, `pcurves`, `procedural_surfaces`, `procedural_curves`, `features`, `tessellations`, `appearances`, `appearance_bindings`, and `attributes`. Every arena is a required flat JSON array. References are string IDs, never array indices. `subds` contains subdivision-surface control cages and is a free carrier arena; it is not owned by B-rep topology.

Pcurve geometry is a parameter-space line, angular circle, angular ellipse, polar harmonic, polar NURBS, or NURBS curve. Circle and ellipse carriers store independent `x_axis` and `y_axis` parameter directions; a clockwise parameterization has a negated `y_axis`. A polar harmonic maps first-order radial-plane and axial harmonic coefficients to `(atan2(y, x), v)` without changing the spatial curve parameter. A polar NURBS evaluates radial-plane and axial control channels with one degree, knot vector, weight vector, and parameter, then maps the radial result through `atan2`.

Maps serialize with lexicographically sorted keys. Arena entries are strictly sorted by ID. Canonical serialization therefore does not use discovery order as semantic state.

## Identity and order

Entity IDs have the grammar:

```text
<format>:<scope>:<kind>#<key>
```

`format` identifies the producing codec or `synthetic`. `scope` identifies the containing source object or stream. `kind` names the entity class. `key` is the source persistent key when one exists and otherwise a positional ordinal.

IDs are globally unique across neutral and native arenas. A codec produces identical IDs for identical input bytes when run at the same codec version. Renumbering caused only by unrelated arena insertion is invalid when the source supplies persistent identity. Each ID-bearing arena is sorted lexicographically by ID. Features also carry an `ordinal`; ordinal is construction order, while array order remains ID order.

## Units, tolerances, and terms

All stored lengths, coordinates, distances, radii, linear tolerances, and length-bearing parameters are millimeters. `units.length` is `"millimeter"`. All angles and angular tolerances are radians. Dimensionless quantities remain unscaled.

`tolerances.linear` is the document-wide maximum linear deviation in millimeters. `tolerances.angular` is the document-wide maximum angular deviation in radians. A face, edge, or vertex `tolerance` overrides `tolerances.linear` for that entity. The override has the same maximum-deviation meaning and must be finite and positive.

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

`Body.kind` is `solid`, `sheet`, `wire`, or `general`. A body optionally records a display name, color, and `visible` — whether the source document displays it; exporters omit bodies with `visible: false` from display-oriented formats. A body owns regions. A region is a connected component of a body and owns shells. A shell owns at least one of faces, wire edges, or free vertices. A face is an oriented bounded portion of one surface and owns loops. A loop's boundary role is `outer`, `inner`, or `unspecified`; a face has at most one explicit outer loop and may have only inner loops when the surface parameter domain supplies the exterior. An edge loop lists coedges in traversal order and may contain ordered pole-vertex uses anchored after a coedge. A vertex loop contains one unanchored vertex use and no coedges. A coedge and a pole-vertex use each own an ordered list of parameter-space curve uses. Each pcurve use may record whether the source declared it isoparametric. An edge joins two vertices and optionally references a curve and canonical parameter range. A vertex references a point carrier. Point remains a separate carrier because it has independent identity and provenance.

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

The two members of a two-member ring normally have opposite senses. Equal senses are structurally representable but produce a validation warning.

### Wires and free vertices

A wire edge appears in exactly one shell's `wire_edges` and in no coedge. A free vertex appears in exactly one shell's `free_vertices` and bounds no edge. A `wire` body contains no faces. `solid` and `sheet` bodies use face topology; `general` bodies may mix dimensionalities.

## Geometry and canonical parameterization

Surface carriers are plane, cylinder, cone, sphere, torus, NURBS, or unknown. Curve carriers are line, circle, ellipse, parabola, hyperbola, NURBS, or unknown. Pcurves are line or NURBS curves in a surface's `(u, v)` space. A subdivision surface is a Catmull–Clark control cage with vertices, edges, directed face edge uses, endpoint sharpness, edge tags, vertex tags, and sector coefficients.

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

`Edge.param_range` uses the canonical parameterization of its curve. Full circles are anchored to `[0, 2π]`. Periodic ranges may cross a seam by using an end value greater than the start value in the unwrapped domain. Pcurve coordinates use the corresponding surface conventions.

Decoders convert kernel conventions at decode:

- NX/Parasolid linear parameters expressed in meters are multiplied by 1000. Unit conversion preserves `byte_exact` status.
- CATIA cylindrical arc-length coordinates use `u = rθ` and are divided by radius. The converted field is `derived`.
- CATIA conical angular coordinates already use the canonical azimuth and are unchanged.
- Fusion ellipse phases are normalized to the major-direction origin and marked `derived`.
- Kernel full-circle intervals are re-anchored to `[0, 2π]` and marked `derived`.

NURBS surfaces store degrees, full knot vectors, pole counts, u-major control points, optional per-pole weights, and periodicity flags. NURBS curves store degree, full knot vector, ordered control points, optional weights, and periodicity.

## Procedural carriers

Procedural entities retain construction semantics beside a solved carrier. `cache_fit_tolerance`, when present, is the maximum millimeter deviation between the procedural definition and solved carrier.

Procedural surface definitions are:

- `extrusion`: directrix and sweep direction;
- `revolution`: directrix, axis, `angular_interval`, optional source-carried `parameter_interval`, and `transposed`;
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
- `unknown`: source fidelity is not established.

Absence from sidecar exactness means `byte_exact` for a decoded source-backed value. A field override takes precedence over entity exactness. Codecs record every decoded entity and field that is not byte-exact. Source-less product documents carry no source-fidelity sidecar. Annotation keys resolve to globally identified product entities or retained source records. Unknown field paths are warnings so additive product fields remain readable.

## Neutral feature model

Each feature has an ID, source-history `ordinal`, optional name, suppression state, optional parent, output bodies, a neutral definition, and optional `native_ref`.

Neutral definitions include directly stored geometry, extrude, revolve, fillet, chamfer, shell,
hole, and pattern. A stored-geometry feature has no fabricated replay construction; its feature
outputs identify any retained exact bodies. `native` is the sole escape hatch for a feature with
no neutral definition and carries its source kind, parameter map, and non-parameter property map.
Length wrappers are millimeters and angle wrappers are radians.

Extents are blind, symmetric, two-sided, through-all, to-face, or angular. Boolean operations are join, cut, intersect, or new-body. Profiles reference native profile identity or solved faces. Fillets use constant or sampled variable radii. Chamfers use distance, two distances, or distance-angle. Holes are simple, counterbored, or countersunk. Patterns are linear, circular, or mirrored.

`native_ref` identifies the full-fidelity native record corresponding to a neutral projection. It does not change the neutral definition's meaning.

## Native namespaces

`native.f3d` and `native.sldprt`, when present, each contain `version: 1`. Fusion native data includes ACT, Design, persistent-reference, sketch-link, construction-recipe, and ASM-history records. SOLIDWORKS native data includes feature histories and feature-input lanes.

Native records retain typed references into the neutral model but are otherwise opaque to format-neutral consumers. A consumer must not reinterpret, normalize, discard, or synthesize native records it does not own. An exporter either preserves a supported namespace unchanged or reports its omission as loss. Native IDs participate in global uniqueness. Namespace versions change independently of `ir_version`; a consumer that does not support a namespace version may still process the neutral model while treating that namespace as opaque.

## Presentation, attributes, and source fidelity

Tessellations are display meshes independent of exact B-rep geometry. Appearances describe visual or physical assets. Appearance bindings assign appearances to bodies or faces. Attributes attach source-native values to supported targets.

An unknown product record has an ID and related entity IDs. It contains no source offset, byte length, digest, or retained source bytes. Those fields belong to the matching `SourceFidelity.retained_records` entry. Source-only records need not have a product record. Retained sidecar bytes use standard RFC 4648 base64 with padding and no line breaks. Native byte strings that are product values and tessellation byte channels remain product data.

## Validation

Validation uses reference lookup and in-IR arithmetic. It does not invoke a geometry kernel. It checks:

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
- tessellation channel and index bounds;
- native record counts, IDs, links, and payload spans;
- opaque payload length and SHA-256.
- complete, nonoverlapping source-byte ownership and opaque retained-record links.

Structural failures are errors. Same-sense two-member radial rings, unknown annotation field paths, and tolerances outside sane canonical ranges are warnings where the representation remains unambiguous. `ValidationReport::is_ok()` is true when no error or blocking finding exists. Decode and export loss notes are reported separately and do not change this predicate.

Validation does not prove that an edge lies on its curve, a pcurve lies on its surface, loops bound valid face regions, or shells enclose a volume.

## Version policy and JSON Schema

Readers accept exactly `ir_version: "55"`. `CadIr::migrate_json` explicitly migrates version 54. The `model.subds` arena is required, including when empty. Source-byte accounting is excluded from the neutral product model. Recursive affine-transformed curve and surface carriers preserve exact source parameterization under occurrence placement. Removing or renaming a product field, changing its type, units, parameterization, or invariant requires a new IR version. Source-fidelity accounting versions independently as described in [byte-accounting.md](byte-accounting.md).

Native namespaces use their own integer versions. A native-only semantic change increments that namespace version without changing the neutral IR version. JSON Schema is generated per IR version by `cadmpeg_ir::cadir_json_schema()`.

## Reserved neutral domains

The following domains are reserved for dedicated neutral models:

- assembly occurrence graphs, instance transforms, and product structure;
- sketch entities, dimensions, constraints, profiles, and construction geometry;
- PMI, GD&T, datums, semantic dimensions, surface texture, and annotation presentation.

Native namespaces may preserve these domains. New neutral fields for them require explicit identity, units, ordering, reference, and validation contracts. Format-specific records must not be added to `model`.

## Worked cube

[`emit_cube.rs`](../crates/cadmpeg-ir/examples/emit_cube.rs) emits a 10 mm solid cube with one region, one shell, six planar faces, twelve line edges, eight vertices, and twenty-four coedges. Every edge has a two-member radial ring.

The generated document begins with this complete hierarchy and representative radial pair:

```json
{
  "ir_version": "55",
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
