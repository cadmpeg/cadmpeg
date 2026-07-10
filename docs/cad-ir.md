<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# cadmpeg IR (`.cadir.json`): v0 specification

`CadIr` is the versioned intermediate representation shared by codecs, validation, diffing, and encoders. It contains topology, geometry, construction semantics, presentation data, source provenance, exactness, opaque records, and independently versioned native namespaces. The `cadmpeg-ir` Rust types are authoritative for field-level types; `cadir_json_schema()` derives the corresponding JSON Schema.

## Document shape

A document contains these root fields in canonical serialization order:

```text
CadIr
├── ir_version
├── source?
├── units
├── tolerances
├── annotations
├── native
└── arenas...
```

- `ir_version` is `"0"`.
- `source`, when present, contains a format identifier and a sorted string map of source metadata.
- `units.length` is `millimeter`, `centimeter`, `meter`, or `inch`. Millimeter is canonical.
- `tolerances.resabs` is an absolute distance tolerance in the document length unit. `tolerances.resnor` is dimensionless.
- `annotations` contains sparse document-wide provenance and exactness tables.
- `native` contains independently versioned, source-format-specific namespaces outside the format-neutral model.

Canonical JSON preserves the `CadIr` struct field order and each arena's insertion order. Maps use sorted keys. Equal documents therefore serialize identically.

## Arenas

Every arena is a flat JSON array. References use typed string IDs rather than array positions. `CadIr::arena_names()` returns the canonical arena order.

| Group                        | Arenas                                                                                                                                                    |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Topology                     | `bodies`, `lumps`, `shells`, `faces`, `loops`, `coedges`, `edges`, `vertices`, `points`                                                                   |
| Geometry                     | `surfaces`, `curves`, `pcurves`, `surface_parameterizations`, `procedural_surfaces`, `procedural_curves`                                                  |
| Construction                 | `features`, `sketch_curve_links`, `persistent_design_links`, `construction_recipes`, `persistent_references`, `lost_edge_references`                      |
| Design records               | `design_objects`, `design_entity_headers`, `design_record_headers`, `sketch_relations`, `sketch_points`, `sketch_curve_identities`, `design_body_members` |
| Change and history records   | `act_entities`, `act_guids`, `act_root_components`, `feature_histories`, `feature_input_lanes`, `asm_histories`                                           |
| Presentation and source data | `tessellations`, `appearances`, `appearance_bindings`, `attributes`, `unknowns`                                                                           |

The groups above are thematic. Canonical serialization order is the field order of the minimal document in the appendix; it differs from the grouping in places (`tessellations` serializes between `act_root_components` and `feature_histories`).

All listed arenas are serialized by `CadIr`. Arenas added after the initial v0 fields deserialize to empty vectors when omitted.

## Topology

The principal B-rep hierarchy is:

```text
body → lump → shell → face → loop → coedge → edge → vertex → point
                         │        │         │
                         │        │         └── curve?
                         │        └── pcurve?
                         └── surface
```

- A `Body` has `kind: solid | sheet | wire | general`. `solid` is the serde default and is the value assigned when `kind` is omitted.
- A `Shell` owns faces and may directly own `wire_edges` or `free_vertices`.
- A `Face` references one surface, carries orientation relative to that surface, and owns boundary loops.
- A `Loop` lists its coedges in ring order.
- A `Coedge` references its owner loop, edge, next and previous coedges, optional manifold partner, optional non-manifold radial successor, orientation, and optional pcurve.
- An `Edge` references optional 3D curve geometry, start and end vertices, an optional parameter range, and optional tolerance.
- A `Vertex` references a point and may carry a tolerance.

Bodies may carry an affine placement, name, and color. Faces may carry a name, color, and tolerance.

## Geometry

`SurfaceGeometry` is tagged by `kind`:

- `plane`
- `cylinder`
- `cone`
- `sphere`
- `torus`
- `nurbs`
- `unknown`

An `unknown` surface preserves a valid topology-to-surface reference when the shape is not represented by a typed carrier. It may reference an `UnknownRecord` containing the opaque source record.

`CurveGeometry` supports `line`, `circle`, `ellipse`, `parabola`, `hyperbola`, and `nurbs`. `PcurveGeometry` supports parameter-space `line` and `nurbs`.

NURBS surfaces store u and v degrees, full knot vectors, u and v pole counts, u-major control points, optional weights, and periodicity flags. NURBS curves store degree, full knot vector, ordered control points, optional weights, and periodicity. Length-bearing coordinates use the document length unit.

`surface_parameterizations` stores a surface's parameter origin and positive-u and positive-v world-space references. `procedural_surfaces` and `procedural_curves` retain construction semantics beside solved surface or curve carriers and may carry a cache-fit tolerance.

## Construction and history

`features` contains format-neutral construction features with stable IDs, construction order, suppression state, optional parent and output-body references, neutral definitions, and optional references into `native`.

The sketch, persistent-design, construction-recipe, and persistent-reference arenas preserve typed relationships between construction records and solved topology. The design-record arenas preserve object identity, record identity, sketch relations, sketch geometry, and body membership. Feature and ASM history arenas preserve ordered operations and change-state graphs. ACT arenas preserve entity, GUID, root-component, and change-channel relationships.

These arenas describe IR semantics. Their source encodings are outside this specification.

## Presentation and opaque data

`tessellations` stores display meshes independently of exact B-rep geometry. `appearances` stores visual or physical appearance assets. `appearance_bindings` assigns an appearance to a body or face. `attributes` attaches source-native attributes to supported IR targets.

`UnknownRecord` contains:

- a stable ID;
- source offset and byte length;
- SHA-256 digest;
- optional retained bytes;
- links to related IR entities;
- provenance and exactness metadata.

When retained bytes are present, their length and SHA-256 digest must match the declared values.

## Provenance and exactness

Topology, geometry, presentation, history, and opaque entities that carry `EntityMeta` record:

- `provenance.format`: source format identifier;
- `provenance.stream`: source stream name;
- `provenance.offset`: byte offset within that stream;
- `provenance.tag?`: source record or class tag;
- `exactness`: `byte_exact`, `derived`, `inferred`, or `unknown`.

`byte_exact` permits only the representation's declared unit conversion. `derived` is deterministic from byte-exact inputs. `inferred` comes from context or convention. `unknown` has no established source fidelity.

The root `annotations` block provides sparse provenance and exactness for globally identified entities and fields. It interns stream names, maps entity IDs to stream index and offset, and stores field-level exactness overrides. Absence from `annotations.exactness` means byte-exact in that sparse table.

## Validation

`validate(&CadIr, losses) -> ValidationReport` uses in-IR arithmetic and reference lookups only. It returns counts for every registered arena, nested ASM history counts, unknown-surface counts, findings, and the supplied loss notes unchanged.

| Check                   | Validation performed                                                                                                                                                                                                                                                                                                                                                                            |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `units`                 | Warn when the length unit is not millimeter or `resabs` is non-finite or non-positive.                                                                                                                                                                                                                                                                                                          |
| `referential_integrity` | Resolve the body-to-lump-to-shell-to-face-to-loop topology chain, coedge ring and partner links, edge curves and vertices, vertex points, parameterization and procedural carriers, appearances, attributes, selected design/sketch links, and opaque-record references. Validate tessellation indices, feature-input payload offsets, design-record references, and sketch-relation ownership. |
| `loop_closure`          | Require each loop to contain coedges and require its `next` chain to form one simple cycle over exactly the listed coedges.                                                                                                                                                                                                                                                                     |
| `coedge_pairing`        | Require an existing partner to point back and reference the same edge. Warn when paired coedges have the same sense.                                                                                                                                                                                                                                                                            |
| `bounds`                | Reject degenerate required directions, invalid radii, inconsistent NURBS pole counts, decreasing knot vectors, non-finite tessellation and sketch coordinates, and invalid typed sketch-curve frames.                                                                                                                                                                                           |
| `counts`                | Validate tessellation channel lengths, declared design-reference counts, and retained unknown-record byte length and digest.                                                                                                                                                                                                                                                                    |

The validator emits warnings only for non-canonical units, invalid `resabs`, and same-sense coedge partners. The listed structural, reference, count, and geometry failures are errors. It does not currently emit blocking findings.

`ValidationReport::is_ok()` is true when its findings contain no `error` or `blocking` severity. Warning findings do not fail validation. Propagated loss notes do not affect `is_ok()`.

Validation does not evaluate geometry. It does not establish that pcurves lie on surfaces, edges lie on curves, loops bound valid face regions, or shells bound closed volumes.

Reference validation does not yet cover shell `wire_edges` or `free_vertices`, coedge `radial_next`, or neutral feature parent and output references.

## JSON Schema

`cadmpeg_ir::cadir_json_schema()` returns the `schemars` JSON Schema for Rust tooling. The repository does not yet publish a generated schema artifact; publishing one is part of [roadmap milestone 1](roadmap.md#milestone-1-public-proof-and-fidelity-measurement).

## Minimal canonical document

This is a complete minimal v0 document. It contains every currently registered arena and no entities:

```json
{
  "ir_version": "0",
  "units": {
    "length": "millimeter"
  },
  "tolerances": {
    "resabs": 1e-6,
    "resnor": 1e-10
  },
  "annotations": {},
  "native": {},
  "bodies": [],
  "lumps": [],
  "shells": [],
  "faces": [],
  "loops": [],
  "coedges": [],
  "edges": [],
  "vertices": [],
  "points": [],
  "surfaces": [],
  "curves": [],
  "pcurves": [],
  "surface_parameterizations": [],
  "procedural_surfaces": [],
  "procedural_curves": [],
  "features": [],
  "sketch_curve_links": [],
  "persistent_design_links": [],
  "construction_recipes": [],
  "persistent_references": [],
  "lost_edge_references": [],
  "design_objects": [],
  "design_entity_headers": [],
  "design_record_headers": [],
  "sketch_relations": [],
  "sketch_points": [],
  "sketch_curve_identities": [],
  "design_body_members": [],
  "act_entities": [],
  "act_guids": [],
  "act_root_components": [],
  "tessellations": [],
  "feature_histories": [],
  "feature_input_lanes": [],
  "asm_histories": [],
  "appearances": [],
  "appearance_bindings": [],
  "attributes": [],
  "unknowns": []
}
```

The populated unit-cube generator is [`crates/cadmpeg-ir/examples/emit_cube.rs`](../crates/cadmpeg-ir/examples/emit_cube.rs). Run:

```text
cargo run -p cadmpeg-ir --example emit_cube
```

Its output is the complete canonical cube artifact; it is not embedded here.
