<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# CADIR and OpenUSD: arena mapping

This document gives, for each CADIR arena and document field, the OpenUSD form that holds it. Each form is an official schema, the preliminary `UsdSolid` schema (`docs/formats/usd.md` §17), a `Cad*` schema of this codec, or none. The goal is a mapping in which a CADIR document written as OpenUSD and read again gives the same document.

This document uses ASD-STE100 Simplified Technical English. Schema, attribute, and arena names are technical names.

## 1. Status of each arena

| Arena or field | OpenUSD form now | Gap to 1:1 | Proposed form |
| --- | --- | --- | --- |
| `units`, `tolerances` | `metersPerUnit`; `brep:intersectTol3d` per B-rep | the document tolerances and the angular tolerance | `customLayerData` keys `cad:tolerance:linear`, `cad:tolerance:angular` |
| `source`, `decode_depth` | none | source format, dialects, and decode depth | `customLayerData` dictionary `cad:source` |
| `bodies`, `regions`, `shells` | `BrepArray` B-reps, regions, and shells | a face that two solid regions share; body kind, name, color, and visibility on a B-rep in a multi-B-rep prim | IR change for shared faces; `CadBrepIdentityAPI` per-B-rep name and kind |
| `faces`, `loops`, `coedges`, `edges`, `vertices`, `points` | `BrepArray` faces, loops, edgeuses, edges, vertices | entity IDs, face and edge names and colors, per-entity tolerances, wire edges, free vertices | `CadBrepIdentityAPI`, `CadBrepToleranceAPI`; read and write `wireEdge:*` and `BrepPointAPI:shellPoint` |
| `surfaces`, `curves` | rational NURBS in `BrepSurfaceNurbAPI` and `BrepCurve3dNurbAPI` | analytic carrier types and their parameterizations; a carrier that two faces or edges share | `CadBrepAnalyticAPI`: analytic catalogs, each NURBS carrier's analytic source, and the parameter map |
| `pcurves` | NURBS trim curves in `BrepCurveUvNurbAPI` | pcurve kinds (line, circle, ellipse, harmonic, and others) and their parameter ranges | `CadBrepAnalyticAPI` pcurve catalog, in analytic surface parameters |
| `procedural_surfaces`, `procedural_curves` | none | extrusion, revolution, sum, sweep, offset, subset, ruled, blend; intersection, projection, offset, subset curves | `CadBrepProceduralAPI`: a construction for each carrier, beside its NURBS solved form |
| `subds` | none | subdivision cages | `Mesh` with `subdivisionScheme = "catmullClark"`, creases, and corners (official) |
| `tessellations` | `Mesh` with normals, texture coordinates, and `GeomSubset` triangle groups | channels and texture assignments other than coordinates | primvars by channel name (official) |
| `appearances`, `appearance_bindings` | materials; bindings to bodies, tessellations, triangle groups, `BrepArray` faces and B-reps | bindings to edges, vertices, free curves, free points, and native carriers | `GeomSubset` of `BrepArray` with `elementType = "edge"` and `"vertex"` |
| `assets` | package members and asset paths | none for images; other asset kinds keep their path | none |
| `product_definitions`, `occurrences` | `Xform` prims, references, instancing, `kind` | part number, label, description, BOM properties, source name | `assetInfo` (official) for identifier and version; `CadProductAPI` for part number and BOM properties |
| `assembly_joints` | `UsdPhysics` joints, drives, PhysX gear and rack-and-pinion joints; `CadConnector` and `CadMate` on `BrepArray` faces (`usd.md` §18.3) | joint families with no `UsdPhysics` or `CadMate` type; joint values and limits on a mate; driving parameter | `CadMate` types for the other CADIR joint kinds, and `cadMate:value` |
| `physical_materials`, `mass_properties`, `rigid_bodies`, `colliders`, `articulations`, `physics_scenes`, `solver_settings` | `UsdPhysics` and PhysX schemas | none | none |
| `sketches`, `sketch_entities`, `sketch_constraints` | `CadSketch` types (`usd.md` §18.2) | entity and constraint kinds outside the `CadSketch` types | added `CadSketch` element and constraint types |
| `spatial_sketches`, `spatial_sketch_entities`, `spatial_sketch_constraints` | none | spatial sketches | `CadSketch` types with a 3D placement and 3D points |
| `parameters` | none | design parameters, units, expressions, owners | `CadParameter` prims, with `cad:parameter:expression` |
| `configurations` | none | configurations and their feature evaluations | variant sets (official composition) over parameter values and feature suppression |
| `features`, `feature_input_topologies`, `feature_result_topologies` | none | the neutral feature history and its topology references | `CadFeature` types: one prim per feature, its definition, inputs, outputs, and ordinal |
| `spreadsheets` | none | spreadsheet cells | `CadSpreadsheet` prim, or an asset |
| `pmi`, `semantic_annotations` | none | product manufacturing information on topology | `CadPmi` types that target `BrepArray` faces and edges through `CadBrepIdentityAPI` identities |
| `attributes` | none | user attributes on entities | `customData` dictionary `cad:attributes` on the prim of the entity, or keyed by entity identity on a `BrepArray` |
| `presentation_layers` | none | layers and their members and visibility | `UsdCollectionAPI` instances (official) |
| `drawings`, `presentation_documents`, `view_presentations` | none | drawing sheets and views | out of scope; keep as assets |
| `native` | none | format-specific namespaces | out of scope; a CADIR document beside the layer |

## 2. Proposed `Cad*` schemas

### 2.1 B-rep extensions on `BrepArray`

These are applied API schemas on a `BrepArray` prim. A reader that does not know them reads the NURBS B-rep. A reader that knows them gives the exact CADIR entities.

- `CadBrepAnalyticAPI`. The catalogs `cad:surface:analytic:*`, `cad:curve3d:analytic:*`, and `cad:pcurve:analytic:*` hold frames and parameters of the analytic carriers (the record model of `pr/usd`, §16 of its `usd.md`). `cad:face:analyticSurface` and `cad:edge:analyticCurve` give, for each face and edge, the analytic carrier index or `-1`. The NURBS carrier is the exact rational form of the analytic carrier; its parameter is the arc parameter whose knots are angles, which maps in closed form to the analytic parameter. Two faces with one analytic index share one carrier.
- `CadBrepProceduralAPI`. A construction for each carrier that one names: kind, operand carriers, and scalar parameters, after CADIR `procedural_surfaces` and `procedural_curves`.
- `CadBrepIdentityAPI`. `cad:brep:id`, `cad:face:id`, `cad:edge:id`, `cad:vertex:id` (string arrays) give the CADIR identity of each entity, so that features, mates, and PMI that reference topology resolve after a round trip. Optional name and color arrays.
- `CadBrepToleranceAPI`. Per-face, per-edge, and per-vertex tolerances; `NaN` is the B-rep tolerance.

### 2.2 Assembly

- `CadConnector`: a frame on a face of a `BrepArray` prim, named by prim and face position (`usd.md` §18.3).
- `CadMate` and its families: the CADIR joint kinds that `UsdPhysics` has no type for, and the connector semantics of those it has. A mate prim wraps or references its `UsdPhysics` joint.

### 2.3 Design

- `CadSketch` and its entity and constraint types (`usd.md` §18.2), extended for spatial sketches.
- `CadParameter`: name, value with unit, expression, owner.
- `CadFeature`: one typed prim for each CADIR feature definition family, with relationships to its sketch, inputs, and outputs.

## 3. Changes outside the USD codec

- CADIR: a face that two solid regions share. A `BrepArray` represents it; a CADIR face belongs to one shell.
- `cadmpeg-measure`: the per-edge work allowance of `cadmpeg check` is below the cost of a rational NURBS circle edge, so a check of a `BrepArray` body skips its measurement checks.

## 4. Order of work

1. `CadBrepIdentityAPI`, so that the later schemas reference topology by identity.
2. `CadConnector` and `CadMate` on `BrepArray` faces, and `CadSketch`, from `pr/usd`.
3. `CadBrepAnalyticAPI` and `CadBrepToleranceAPI`.
4. `CadParameter`, configurations as variant sets, and `CadBrepProceduralAPI`.
5. `CadFeature` and `CadPmi`.
