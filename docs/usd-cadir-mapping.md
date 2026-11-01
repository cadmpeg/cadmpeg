<!-- SPDX-License-Identifier: CC-BY-4.0 -->

# CADIR and OpenUSD: arena mapping

This document gives, for each CADIR arena and document field, the OpenUSD form that holds it. Each form is an official schema, the preliminary `UsdSolid` schema (`docs/formats/usd.md` §17), a `Cad*` schema of this codec, or none. The goal is a mapping in which a CADIR document written as OpenUSD and read again gives the same document.

This document uses ASD-STE100 Simplified Technical English. Schema, attribute, and arena names are technical names.

## 1. Status of each arena

| Arena or field | OpenUSD form now | Gap to 1:1 | Proposed form |
| --- | --- | --- | --- |
| `units`, `tolerances` | `metersPerUnit`; `brep:intersectTol3d` per B-rep; `customLayerData` dictionary `cad` with the document tolerances (`usd.md` §18) | none | none |
| `source`, `decode_depth` | none | source format, dialects, and decode depth | `customLayerData` dictionary `cad:source` |
| `bodies`, `regions`, `shells` | `BrepArray` B-reps, regions, and shells | a face that two solid regions share; body kind, color, and visibility on a B-rep in a multi-B-rep prim | IR change for shared faces; per-B-rep kind, color, and visibility arrays |
| `faces`, `loops`, `coedges`, `edges`, `vertices`, `points` | `BrepArray` faces, loops, edgeuses, edges, vertices; `CadBrepIdentityAPI` identities and names; `CadBrepToleranceAPI` tolerances | face colors, wire edges, free vertices | face colors as `GeomSubset` display colors; read and write `wireEdge:*` and `BrepPointAPI:shellPoint` |
| `surfaces`, `curves` | rational NURBS in `BrepSurfaceNurbAPI` and `BrepCurve3dNurbAPI`; exact carriers in `CadBrepAnalyticAPI` (`usd.md` §18.5) | a carrier that two faces or edges share | shared catalog indices in `CadBrepAnalyticAPI` |
| `pcurves` | NURBS trim curves in `BrepCurveUvNurbAPI`; exact pcurves in `CadBrepAnalyticAPI` | pcurve kinds other than line, circle, ellipse, and NURBS | added `CadBrepAnalyticAPI` pcurve types |
| `procedural_surfaces`, `procedural_curves` | extrusion and revolution surfaces in `CadBrepAnalyticAPI` | sum, sweep, offset, subset, ruled, blend; intersection, projection, offset, subset curves | `CadBrepProceduralAPI`: a construction for each carrier, beside its NURBS solved form; the writer needs a NURBS form of each kind |
| `subds` | none | subdivision cages | `Mesh` with `subdivisionScheme = "catmullClark"`, creases, and corners (official) |
| `tessellations` | `Mesh` with normals, texture coordinates, and `GeomSubset` triangle groups | per-vertex colors (a scan mesh) and channels other than coordinates, such as intensity and confidence | vertex-interpolated `primvars:displayColor` and primvars by channel name (official) |
| point clouds (no CADIR arena) | `UsdGeomPoints` prims are not read; free CADIR `points` are written as one `UsdGeomPoints` prim | a CADIR arena of packed points with per-point channels (normal, color, intensity, classification) and a sensor frame | `UsdGeomPoints` with `normals`, `widths`, `ids`, per-point `displayColor`, and primvars by channel name (official); capture metadata (sensor pose, capture time, registration) as `assetInfo` or a `CadScanAPI` |
| `appearances`, `appearance_bindings` | materials; bindings to bodies, tessellations, triangle groups, `BrepArray` faces and B-reps | bindings to edges, vertices, free curves, free points, and native carriers | `GeomSubset` of `BrepArray` with `elementType = "edge"` and `"vertex"` |
| `assets` | package members and asset paths | none for images; other asset kinds keep their path | none |
| `product_definitions`, `occurrences` | `Xform` prims, references, instancing, `kind`; `cad:id` and `cad:definition:*` identities and product data; `documentation` (`usd.md` §18) | asset identifier and version | `assetInfo` (official) |
| `assembly_joints` | `UsdPhysics` joints, drives, PhysX gear and rack-and-pinion joints; `CadConnector` and `CadMate` on `BrepArray` faces, one mate type for each CADIR joint family, with their values, limits, couplings, and driving parameter (`usd.md` §18.3) | mates on faces of bodies that are not `BrepArray` prims | connectors on mesh and intrinsic bodies |
| `physical_materials`, `mass_properties`, `rigid_bodies`, `colliders`, `articulations`, `physics_scenes`, `solver_settings` | `UsdPhysics` and PhysX schemas | none | none |
| `sketches`, `sketch_entities`, `sketch_constraints` | `CadSketch` types (`usd.md` §18.2) | entity and constraint kinds outside the `CadSketch` types | added `CadSketch` element and constraint types |
| `spatial_sketches`, `spatial_sketch_entities`, `spatial_sketch_constraints` | none | spatial sketches | `CadSketch` types with a 3D placement and 3D points |
| `parameters` | `CadParameter` prims (`usd.md` §18.6), with `rel cad:parameter:owner` to a `CadFeature` prim | PMI semantics, which name a native record | a native-record form for USD, or PMI semantics with no native link |
| `configurations` | `CadConfiguration` prims (`usd.md` §18.8): typed identity, order, name, material, and properties; overrides, values, bodies, and feature states as CADIR wire JSON | a variant set that selects the parameter values and feature suppression of each configuration for viewers | typed configuration properties; a `configuration` variant set |
| `features`, `feature_input_topologies`, `feature_result_topologies` | `CadFeature` prims (`usd.md` §18.7): typed identity, order, suppression, dependencies, outputs, and properties; typed subtypes for 17 families (tree node and sketch, with feature references as relationships, extrude, revolve, fillet, chamfer, pattern, hole, shell, draft, mirror, combine, primitive, and datum plane, axis, point, and frame) with their scalar leaves; the rest of each definition and the topologies as CADIR wire JSON | selections (edges, faces, profiles) and the families without a subtype | `CadFeature` subtypes for further families, and selections as typed identity arrays |
| `spreadsheets` | none | spreadsheet cells | `CadSpreadsheet` prim, or an asset |
| `pmi`, `semantic_annotations` | `CadPmi` prims (`usd.md` §18.9) with targets and definition as CADIR wire JSON | semantic annotations and the PMI semantics of a design parameter: each names native records (`object`, `native_ref`, assets) that the IR requires to resolve, and a stage holds no native records | an IR change that makes those native links optional, or a native-record form in USD |
| `attributes` | one `CadAttributes` prim (`usd.md` §18.11) | attributes on records the stage does not write | none |
| `presentation_layers` | `CadLayer` prims with `CollectionAPI:members` (`usd.md` §18.10) | items on records the stage does not write | none |
| `drawings`, `presentation_documents`, `view_presentations` | none | drawing sheets and views | out of scope; keep as assets |
| `native` | none | format-specific namespaces | out of scope; a CADIR document beside the layer |

## 2. Proposed `Cad*` schemas

### 2.1 B-rep extensions on `BrepArray`

These are applied API schemas on a `BrepArray` prim. A reader that does not know them reads the NURBS B-rep. A reader that knows them gives the exact CADIR entities.

- `CadBrepAnalyticAPI` (`usd.md` §18.5). Catalogs of analytic surfaces, analytic and NURBS curves, and analytic and NURBS pcurves, and the exact carrier of each face, edge, and edgeuse. The NURBS carrier of the `BrepArray` is the exact rational form of the analytic carrier.
- `CadBrepProceduralAPI`. A construction for each carrier that one names: kind, operand carriers, and scalar parameters, after CADIR `procedural_surfaces` and `procedural_curves`.
- `CadBrepIdentityAPI` (`usd.md` §18.1). `cad:brep:id`, `cad:face:id`, `cad:edge:id`, `cad:vertex:id` give the CADIR identity of each entity, so that features, mates, and PMI that reference topology resolve after a round trip. `cad:brep:name` and `cad:face:name` give the body and face names.
- `CadBrepToleranceAPI` (`usd.md` §18.4). Per-face, per-edge, and per-vertex tolerances; `NaN` is the B-rep tolerance.

### 2.2 Assembly

- `CadConnector`: a frame on a face of a `BrepArray` prim, named by prim and face position (`usd.md` §18.3).
- `CadMate` and its families: the CADIR joint kinds that `UsdPhysics` has no type for, and the connector semantics of those it has. A mate prim wraps or references its `UsdPhysics` joint.

### 2.3 Design

- `CadSketch` and its entity and constraint types (`usd.md` §18.2), extended for spatial sketches.
- `CadParameter` (`usd.md` §18.6): name, expression, typed value, display, dependencies, product definition, and properties.
- `CadFeature` (`usd.md` §18.7): one prim for each feature, with typed common properties and the operation as CADIR wire JSON; typed subtypes for the operation families follow.

## 3. Changes outside the USD codec

- CADIR: a point cloud arena that holds packed positions and per-point channels, as `tessellations` hold packed meshes. Free `points` records hold one identity per point, which a scan of millions of points does not admit.
- CADIR: a face that two solid regions share. A `BrepArray` represents it; a CADIR face belongs to one shell.
- `cadmpeg-measure`: the per-edge work allowance of `cadmpeg check` is below the cost of a rational NURBS circle edge, so a check of a `BrepArray` body skips its measurement checks.

## 4. Order of work

1. `CadBrepIdentityAPI`, so that the later schemas reference topology by identity.
2. `CadConnector` and `CadMate` on `BrepArray` faces, and `CadSketch`, from `pr/usd`.
3. `CadBrepAnalyticAPI` and `CadBrepToleranceAPI`.
4. `CadParameter`.
5. `CadFeature`, `CadConfiguration`, and `CadPmi`.
6. `CadBrepProceduralAPI`, after the NURBS forms of the procedural carriers.
