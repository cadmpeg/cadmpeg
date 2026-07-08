<!-- SPDX-License-Identifier: Apache-2.0 -->

# cadmpeg IR (`.cadir.json`): v0 specification

The cadmpeg intermediate representation is the data model that codecs produce and exporters consume. It stores topology, geometry carriers, source offsets, exactness, uninterpreted records, and decode losses. The `cadmpeg-ir` crate defines the Rust types. This document describes v0 (`ir_version = "0"`) as implemented, with a complete worked example in the appendix.

The IR does not silently normalize or drop data. Entities record source provenance and exactness. Decoders preserve recognized but uninterpreted records verbatim. The v0 schema reserves unsupported model areas instead of approximating them with speculative fields.

## Document shape

A document is a `CadIr`: a version tag, optional source metadata, a units block, kernel tolerances, and a set of **flat arenas** (one `Vec` per entity kind). The B-rep is an _id-referenced graph_: entities carry typed string ids and refer to one another by id. Serialization is canonical (struct field order is fixed and maps are sorted), so two equal documents serialize byte-for-byte identically.

Each body has `kind: solid | sheet`. Solid is the serialization default for v0 documents written before this field existed.

```
CadIr
├── ir_version         "0"
├── source?            { format, attributes: sorted map }
├── units              { length: millimeter | centimeter | meter | inch }
├── tolerances         { resabs, resnor }
├── bodies    []       ┐
├── lumps     []       │
├── shells    []       │  topology arenas
├── faces     []       │  (body → lump → shell → face →
├── loops     []       │   loop → coedge → edge → vertex)
├── coedges   []       │
├── edges     []       │
├── vertices  []       │
├── points    []       ┘
├── surfaces  []       ┐
├── curves    []       │  geometry-carrier arenas
├── pcurves   []       ┘
├── procedural_surfaces [] native construction + solved-cache fit contract
├── procedural_curves [] native construction subtype + cache fit contract
├── sketch_curve_links [] source sketch curve → solved coedge provenance
├── persistent_design_links [] Fusion design ids → solved B-rep entities
├── construction_recipes [] Design BulkStream regeneration records
├── persistent_references [] persistent point/curve recipe identifiers
├── lost_edge_references [] source-marked broken parametric selections
├── design_objects []    MetaStream Body/Component/Sketch/Dimension objects
├── design_entity_headers [] byte-state-bearing Design entity containers
├── design_record_headers [] indexed dynamic-class Design records
├── sketch_relations []    bidirectional sketch-owned reference relations
├── sketch_points []       persistent source sketch coordinates
├── sketch_curve_identities [] persistent ids bound to sketch-curve records
├── design_body_members [] native BodiesRoot membership
├── act_entities []       ACT entity table + change-version channels
├── act_guids []          ordered ACT stream-wide GUID pool
├── act_root_components [] ACT document-root registry links
├── tessellations []   source display/facet meshes
├── feature_histories [] ordered parametric operations
├── feature_input_lanes [] native feature-input payloads + typed sketch markers
├── asm_histories []   ASM delta states, bulletin changes, and raw revisions
├── appearances []     material/visual assets
├── appearance_bindings [] body/face assignments + ACT channels
├── attributes []      linked source-native attributes
└── unknowns  []       passthrough for uninterpreted records
```

The IR's canonical length unit is the **millimeter**. A decoder converts source units at decode time (for Fusion `.f3d`, model-space lengths are centimeters and are scaled ×10) and records the resulting unit here. `tolerances` preserves the kernel's absolute-distance (`resabs`) and normal (`resnor`) tolerances as metadata.

## Provenance and exactness

Every entity embeds an `EntityMeta`:

- **`provenance`**: `{ format, stream, offset, tag? }`. `format` is the source container (`"f3d"`, or `"synthetic"` for hand-built IR); `stream` is the named sub-stream (for `.f3d`, a decompressed ZIP entry such as `…/Breps.BlobParts/Body1.smbh`); `offset` is the byte offset of the record in that stream; `tag` is the source record/class name when attributable.
- **`exactness`**: one of:
  - `byte_exact`: read verbatim from the source (only documented unit conversion applied);
  - `derived`: computed deterministically from byte-exact inputs;
  - `inferred`: filled from context/convention rather than an explicit field;
  - `unknown`: origin or trustworthiness could not be established.

`validate` and `export` use this metadata to report loss without asserting unsupported fidelity.

## Topology

The graph follows the ACIS/ASM hierarchy `body → lump → shell → face → loop → coedge → edge → vertex`, with geometry attached by reference:

| Link               | Field                              |
| ------------------ | ---------------------------------- |
| face → surface     | `Face.surface : SurfaceId`         |
| edge → 3D curve    | `Edge.curve : Option<CurveId>`     |
| coedge → UV pcurve | `Coedge.pcurve : Option<PcurveId>` |
| vertex → point     | `Vertex.point : PointId`           |

A **loop** is an ordered ring of coedges; each `Coedge` also stores `next`, `previous`, an `edge`, a `partner` (the coedge on the adjacent face sharing that edge), and a `sense` (`forward`/`reversed`) relative to the edge's curve. On a manifold edge the two coedges have opposite sense. Faces and bodies carry optional `name` and `color`; bodies carry an optional 4×4 affine `transform`.

## Geometry carriers

Surfaces (`SurfaceGeometry`, tagged by `kind`): `plane`, `cylinder`, `cone`, `sphere`, `torus`, `nurbs`, and `unknown`. An `unknown` surface means the decoder recovered the face topology but did not interpret the underlying shape into a typed carrier. The face keeps its loops and trims. The surface can link (`record`) to preserved raw bytes in the unknowns arena so a re-encode can recover them. Such a surface should carry `Exactness::Unknown`.

Curves (`CurveGeometry`): `line`, `circle`, `ellipse`, and `nurbs`. Pcurves (`PcurveGeometry`): a parameter-space `line` or `nurbs`.

NURBS payloads carry degrees, full (clamped) knot vectors, a flat control-point list (u-major for surfaces), optional per-pole weights (absent ⇒ non-rational), and periodicity flags. `procedural_surfaces` retains source-native constructions independently of their solved NURBS carriers. It currently distinguishes translational extrusions from rolling-ball blends, preserves signed radius laws, records complete versus partial support resolution, and carries the solved-cache fit tolerance. `procedural_curves` retains each native construction subtype and its 3D-cache fit tolerance.

All analytic directions (plane normals, axes) are unit vectors in well-formed IR; lengths and points are in the document's length unit.

## Uninterpreted records

When a decoder recognizes a record but cannot map it to a typed entity, it emits an `UnknownRecord` (`{ id, offset, byte_len, sha256, data?, links, meta }`). `data` retains bytes required by writers. Validation checks its length and SHA-256. `links` identifies related IR entities.

## Feature history

`feature_histories` stores ordered parametric operations, configurations, named parameters, suppression state, source ids, and provenance. Codecs populate only fields established by source records.

`asm_histories` stores each ASM history-stream header and its linked `delta_state` nodes. A state retains its previous/next/partner/owner references, BulletinBoard insert/delete/update pairs, and state-local SAB revisions. Recognized revisions are individually framed; an unframed state payload remains one byte-exact opaque revision. The raw revision bytes preserve the native replay and write substrate while typed entity-revision decoding advances.

`act_entities` stores ACTTable membership and entity-bound change-version channels keyed by the ACT record index and Fusion entity id. Channel class tags remain source values because their registry is per-file. `act_guids` is the independent ordered GUID pool; its entries are not assigned positionally to ACTTable entities.

## Native encoding

`Encoder` writes IR to a native format. The SLDPRT encoder writes an unchanged retained source image byte-for-byte. It regenerates supported source-less or modified IR as native blocks plus a tail section directory. Rigid body transforms are baked into model-space carriers. Auxiliary-only edits retain the original partition, deltas, opaque carriers, matching cache cells, and directory state bytes. Unsupported typed fields fail explicitly.

## Reserved model areas

`reserved::Assembly` remains reserved for assembly instancing, component trees, and joints or mates.

## Validation

`validate(&CadIr, losses) -> ValidationReport` runs only in-IR arithmetic, no geometry kernel, and returns per-kind entity counts plus a list of `Finding`s. The checks:

| Check                   | What it verifies                                                                 |
| ----------------------- | -------------------------------------------------------------------------------- |
| `referential_integrity` | every referenced id resolves in its arena                                        |
| `loop_closure`          | each loop's `next` chain is a simple cycle over exactly the loop's coedges       |
| `coedge_pairing`        | partners point back at each other, share an edge, and (warn) have opposite sense |
| `units`                 | length unit present/canonical; `resabs` positive                                 |
| `bounds`                | non-degenerate directions, positive radii, and consistent NURBS pole/knot counts |

Validation does not run geometric-kernel checks, such as whether a pcurve lies on its surface or whether faces bound a closed solid.

`ValidationReport::is_ok()` is true when there are no `error`/`blocking` findings. The report also carries any `losses` propagated from the decode that produced the document.

## The codec contract

A format plugin implements `Codec`:

- `detect(&[u8]) -> Confidence`: `no`/`low`/`medium`/`high` from a byte prefix.
- `inspect(reader) -> ContainerSummary`: enumerate container streams/segments (name, role, compression, sizes, and codec-extracted `attributes`) without decoding geometry.
- `decode(reader, &DecodeOptions) -> DecodeResult { ir, report }`: decode into the IR. `DecodeOptions::container_only` stops after the container layer. The `DecodeReport` states whether geometry was transferred and lists `LossNote`s by category and severity.

A codec that cannot transfer geometry must add a loss note to its report. A codec must not fabricate geometry to satisfy the contract.

## Fusion `.f3d` decode coverage

The `cadmpeg-codec-f3d` codec decodes the container and the active ASM/SAB B-rep stream into byte-backed IR topology and geometry. It establishes these container facts from the `.f3d` format spec:

- A `.f3d` is a ZIP archive. Entries are classified into families: BREP streams (`*.smbh` authoritative, `*.smb` construction snapshot), `.protein` material ZIPs, design/ACT/browser `BulkStream.dat`, per-segment `MetaStream.dat`, `Manifest.dat`, previews, images, `.paramesh`, and placeholders.
- BREP streams begin with an ASM `BinaryFile8<`/`BinaryFile4<` header. For `BinaryFile8` the codec reads the big-endian version words, the tagged product strings (`product_family` = `Autodesk Neutron`, `product_version`, `save_date`) and the tagged f64s `scale` (a corpus-constant kernel default, **not** a coordinate multiplier), `resabs`, and `resnor`.
- The `delta_state` marker partitions the active solved model (before) from construction history (after); the codec locates the first occurrence.
- The active stream decodes into bodies, shells, faces, loops, coedges, edges, vertices, analytic surface/curve carriers, and inline cached NURBS carriers.

Faces whose geometry depends on an unresolved subtype-reference cache keep their topology and trims, but use an `unknown` surface linked to preserved record bytes. The decode report lists those faces as geometry losses instead of silently dropping them.

## JSON Schema

`cadmpeg_ir::cadir_json_schema()` produces the JSON Schema for `CadIr` via `schemars`, for publishing the contract and validating documents out-of-band.

## Appendix: full worked example (unit cube)

The document below is the serialized output for a hand-built 10 mm axis-aligned cube: 8 vertices, 12 edges, 6 planar faces, 24 coedges (each edge shared by two opposite-sense coedges), 6 planes, 12 line curves. It validates with zero errors and zero warnings. Regenerate it verbatim with:

```text
cargo run -p cadmpeg-ir --example emit_cube
```

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
  "bodies": [
    {
      "id": "body0",
      "lumps": ["lump0"],
      "name": "unit cube",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "lumps": [
    {
      "id": "lump0",
      "body": "body0",
      "shells": ["shell0"],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "shells": [
    {
      "id": "shell0",
      "lump": "lump0",
      "faces": ["f_bottom", "f_top", "f_front", "f_right", "f_back", "f_left"],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "faces": [
    {
      "id": "f_bottom",
      "shell": "shell0",
      "surface": "srf_bottom",
      "sense": "forward",
      "loops": ["lp_bottom"],
      "name": "bottom face",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "f_top",
      "shell": "shell0",
      "surface": "srf_top",
      "sense": "forward",
      "loops": ["lp_top"],
      "name": "top face",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "f_front",
      "shell": "shell0",
      "surface": "srf_front",
      "sense": "forward",
      "loops": ["lp_front"],
      "name": "front face",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "f_right",
      "shell": "shell0",
      "surface": "srf_right",
      "sense": "forward",
      "loops": ["lp_right"],
      "name": "right face",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "f_back",
      "shell": "shell0",
      "surface": "srf_back",
      "sense": "forward",
      "loops": ["lp_back"],
      "name": "back face",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "f_left",
      "shell": "shell0",
      "surface": "srf_left",
      "sense": "forward",
      "loops": ["lp_left"],
      "name": "left face",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "loops": [
    {
      "id": "lp_bottom",
      "face": "f_bottom",
      "coedges": ["ce_bottom_0", "ce_bottom_1", "ce_bottom_2", "ce_bottom_3"],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "lp_top",
      "face": "f_top",
      "coedges": ["ce_top_0", "ce_top_1", "ce_top_2", "ce_top_3"],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "lp_front",
      "face": "f_front",
      "coedges": ["ce_front_0", "ce_front_1", "ce_front_2", "ce_front_3"],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "lp_right",
      "face": "f_right",
      "coedges": ["ce_right_0", "ce_right_1", "ce_right_2", "ce_right_3"],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "lp_back",
      "face": "f_back",
      "coedges": ["ce_back_0", "ce_back_1", "ce_back_2", "ce_back_3"],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "lp_left",
      "face": "f_left",
      "coedges": ["ce_left_0", "ce_left_1", "ce_left_2", "ce_left_3"],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "coedges": [
    {
      "id": "ce_bottom_0",
      "owner_loop": "lp_bottom",
      "edge": "e0",
      "next": "ce_bottom_1",
      "previous": "ce_bottom_3",
      "partner": "ce_front_0",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_bottom_1",
      "owner_loop": "lp_bottom",
      "edge": "e1",
      "next": "ce_bottom_2",
      "previous": "ce_bottom_0",
      "partner": "ce_right_0",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_bottom_2",
      "owner_loop": "lp_bottom",
      "edge": "e2",
      "next": "ce_bottom_3",
      "previous": "ce_bottom_1",
      "partner": "ce_back_0",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_bottom_3",
      "owner_loop": "lp_bottom",
      "edge": "e3",
      "next": "ce_bottom_0",
      "previous": "ce_bottom_2",
      "partner": "ce_left_0",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_top_0",
      "owner_loop": "lp_top",
      "edge": "e7",
      "next": "ce_top_1",
      "previous": "ce_top_3",
      "partner": "ce_left_2",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_top_1",
      "owner_loop": "lp_top",
      "edge": "e6",
      "next": "ce_top_2",
      "previous": "ce_top_0",
      "partner": "ce_back_2",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_top_2",
      "owner_loop": "lp_top",
      "edge": "e5",
      "next": "ce_top_3",
      "previous": "ce_top_1",
      "partner": "ce_right_2",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_top_3",
      "owner_loop": "lp_top",
      "edge": "e4",
      "next": "ce_top_0",
      "previous": "ce_top_2",
      "partner": "ce_front_2",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_front_0",
      "owner_loop": "lp_front",
      "edge": "e0",
      "next": "ce_front_1",
      "previous": "ce_front_3",
      "partner": "ce_bottom_0",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_front_1",
      "owner_loop": "lp_front",
      "edge": "e8",
      "next": "ce_front_2",
      "previous": "ce_front_0",
      "partner": "ce_left_3",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_front_2",
      "owner_loop": "lp_front",
      "edge": "e4",
      "next": "ce_front_3",
      "previous": "ce_front_1",
      "partner": "ce_top_3",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_front_3",
      "owner_loop": "lp_front",
      "edge": "e9",
      "next": "ce_front_0",
      "previous": "ce_front_2",
      "partner": "ce_right_1",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_right_0",
      "owner_loop": "lp_right",
      "edge": "e1",
      "next": "ce_right_1",
      "previous": "ce_right_3",
      "partner": "ce_bottom_1",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_right_1",
      "owner_loop": "lp_right",
      "edge": "e9",
      "next": "ce_right_2",
      "previous": "ce_right_0",
      "partner": "ce_front_3",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_right_2",
      "owner_loop": "lp_right",
      "edge": "e5",
      "next": "ce_right_3",
      "previous": "ce_right_1",
      "partner": "ce_top_2",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_right_3",
      "owner_loop": "lp_right",
      "edge": "e10",
      "next": "ce_right_0",
      "previous": "ce_right_2",
      "partner": "ce_back_1",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_back_0",
      "owner_loop": "lp_back",
      "edge": "e2",
      "next": "ce_back_1",
      "previous": "ce_back_3",
      "partner": "ce_bottom_2",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_back_1",
      "owner_loop": "lp_back",
      "edge": "e10",
      "next": "ce_back_2",
      "previous": "ce_back_0",
      "partner": "ce_right_3",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_back_2",
      "owner_loop": "lp_back",
      "edge": "e6",
      "next": "ce_back_3",
      "previous": "ce_back_1",
      "partner": "ce_top_1",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_back_3",
      "owner_loop": "lp_back",
      "edge": "e11",
      "next": "ce_back_0",
      "previous": "ce_back_2",
      "partner": "ce_left_1",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_left_0",
      "owner_loop": "lp_left",
      "edge": "e3",
      "next": "ce_left_1",
      "previous": "ce_left_3",
      "partner": "ce_bottom_3",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_left_1",
      "owner_loop": "lp_left",
      "edge": "e11",
      "next": "ce_left_2",
      "previous": "ce_left_0",
      "partner": "ce_back_3",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_left_2",
      "owner_loop": "lp_left",
      "edge": "e7",
      "next": "ce_left_3",
      "previous": "ce_left_1",
      "partner": "ce_top_0",
      "sense": "forward",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "ce_left_3",
      "owner_loop": "lp_left",
      "edge": "e8",
      "next": "ce_left_0",
      "previous": "ce_left_2",
      "partner": "ce_front_1",
      "sense": "reversed",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "edges": [
    {
      "id": "e0",
      "curve": "crv_e0",
      "start": "v0",
      "end": "v1",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e1",
      "curve": "crv_e1",
      "start": "v1",
      "end": "v2",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e2",
      "curve": "crv_e2",
      "start": "v2",
      "end": "v3",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e3",
      "curve": "crv_e3",
      "start": "v3",
      "end": "v0",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e4",
      "curve": "crv_e4",
      "start": "v4",
      "end": "v5",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e5",
      "curve": "crv_e5",
      "start": "v5",
      "end": "v6",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e6",
      "curve": "crv_e6",
      "start": "v6",
      "end": "v7",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e7",
      "curve": "crv_e7",
      "start": "v7",
      "end": "v4",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e8",
      "curve": "crv_e8",
      "start": "v0",
      "end": "v4",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e9",
      "curve": "crv_e9",
      "start": "v1",
      "end": "v5",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e10",
      "curve": "crv_e10",
      "start": "v2",
      "end": "v6",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "e11",
      "curve": "crv_e11",
      "start": "v3",
      "end": "v7",
      "param_range": [0.0, 10.0],
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "vertices": [
    {
      "id": "v0",
      "point": "p0",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "v1",
      "point": "p1",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "v2",
      "point": "p2",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "v3",
      "point": "p3",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "v4",
      "point": "p4",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "v5",
      "point": "p5",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "v6",
      "point": "p6",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "v7",
      "point": "p7",
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "points": [
    {
      "id": "p0",
      "position": {
        "x": 0.0,
        "y": 0.0,
        "z": 0.0
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "p1",
      "position": {
        "x": 10.0,
        "y": 0.0,
        "z": 0.0
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "p2",
      "position": {
        "x": 10.0,
        "y": 10.0,
        "z": 0.0
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "p3",
      "position": {
        "x": 0.0,
        "y": 10.0,
        "z": 0.0
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "p4",
      "position": {
        "x": 0.0,
        "y": 0.0,
        "z": 10.0
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "p5",
      "position": {
        "x": 10.0,
        "y": 0.0,
        "z": 10.0
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "p6",
      "position": {
        "x": 10.0,
        "y": 10.0,
        "z": 10.0
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "p7",
      "position": {
        "x": 0.0,
        "y": 10.0,
        "z": 10.0
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "surfaces": [
    {
      "id": "srf_bottom",
      "geometry": {
        "kind": "plane",
        "origin": {
          "x": 0.0,
          "y": 0.0,
          "z": 0.0
        },
        "normal": {
          "x": 0.0,
          "y": 0.0,
          "z": -1.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "srf_top",
      "geometry": {
        "kind": "plane",
        "origin": {
          "x": 0.0,
          "y": 0.0,
          "z": 10.0
        },
        "normal": {
          "x": 0.0,
          "y": 0.0,
          "z": 1.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "srf_front",
      "geometry": {
        "kind": "plane",
        "origin": {
          "x": 0.0,
          "y": 0.0,
          "z": 0.0
        },
        "normal": {
          "x": 0.0,
          "y": -1.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "srf_right",
      "geometry": {
        "kind": "plane",
        "origin": {
          "x": 10.0,
          "y": 0.0,
          "z": 0.0
        },
        "normal": {
          "x": 1.0,
          "y": 0.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "srf_back",
      "geometry": {
        "kind": "plane",
        "origin": {
          "x": 0.0,
          "y": 10.0,
          "z": 0.0
        },
        "normal": {
          "x": 0.0,
          "y": 1.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "srf_left",
      "geometry": {
        "kind": "plane",
        "origin": {
          "x": 0.0,
          "y": 0.0,
          "z": 0.0
        },
        "normal": {
          "x": -1.0,
          "y": 0.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "curves": [
    {
      "id": "crv_e0",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 0.0,
          "y": 0.0,
          "z": 0.0
        },
        "direction": {
          "x": 1.0,
          "y": 0.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e1",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 10.0,
          "y": 0.0,
          "z": 0.0
        },
        "direction": {
          "x": 0.0,
          "y": 1.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e2",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 10.0,
          "y": 10.0,
          "z": 0.0
        },
        "direction": {
          "x": -1.0,
          "y": 0.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e3",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 0.0,
          "y": 10.0,
          "z": 0.0
        },
        "direction": {
          "x": 0.0,
          "y": -1.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e4",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 0.0,
          "y": 0.0,
          "z": 10.0
        },
        "direction": {
          "x": 1.0,
          "y": 0.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e5",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 10.0,
          "y": 0.0,
          "z": 10.0
        },
        "direction": {
          "x": 0.0,
          "y": 1.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e6",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 10.0,
          "y": 10.0,
          "z": 10.0
        },
        "direction": {
          "x": -1.0,
          "y": 0.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e7",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 0.0,
          "y": 10.0,
          "z": 10.0
        },
        "direction": {
          "x": 0.0,
          "y": -1.0,
          "z": 0.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e8",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 0.0,
          "y": 0.0,
          "z": 0.0
        },
        "direction": {
          "x": 0.0,
          "y": 0.0,
          "z": 1.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e9",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 10.0,
          "y": 0.0,
          "z": 0.0
        },
        "direction": {
          "x": 0.0,
          "y": 0.0,
          "z": 1.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e10",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 10.0,
          "y": 10.0,
          "z": 0.0
        },
        "direction": {
          "x": 0.0,
          "y": 0.0,
          "z": 1.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    },
    {
      "id": "crv_e11",
      "geometry": {
        "kind": "line",
        "origin": {
          "x": 0.0,
          "y": 10.0,
          "z": 0.0
        },
        "direction": {
          "x": 0.0,
          "y": 0.0,
          "z": 1.0
        }
      },
      "meta": {
        "provenance": {
          "format": "synthetic",
          "stream": "",
          "offset": 0
        },
        "exactness": "inferred"
      }
    }
  ],
  "pcurves": [],
  "appearances": [],
  "appearance_bindings": [],
  "attributes": [],
  "unknowns": []
}
```
