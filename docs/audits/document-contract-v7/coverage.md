# Document contract coverage artifacts

This is a **historical, pre-addendum snapshot**. It records the exact executable and assertions exercised before the D/P/W/B/E corrections. It does not establish conformance of the current wire, native dispositions, canonical dimensions, recompute state, or revised E1–E5 tests. Fresh validation of those changes is still required.

All 26 positive Rhino golden fixtures and all 12 public FreeCAD fixtures synthesized and re-decoded successfully in the recorded run. The two malformed Rhino `reject_*` fixtures are negative inputs and were excluded. Each adjacent source sidecar was removed and retained separately as history before conversion, so these writes had no source fidelity. Targets were `rhino:archive-80` and `fcstd:schema-4`. The collection explicitly used `--allow-empty` to admit valid empty output from fixtures without exportable geometry; it used no `--allow-errors` override.

The executable SHA-256 was `b3393f5438709cb3c2cd265e139edfd5b137ff030dfe7ea6d0fc26c8ccd9a1f5` before and after collection. The corresponding [E1/E2/E3 verification](tests.md) passed all ten tests. This table reports actual chosen-plan dispositions and losses; a successful file write alone does not prove lossless content.

## Actual neutral dispositions

Each named group below is an exact grouping of identical harvested report maps, not a format capability assumption. Every fixture row selects its observed group. Written is a complete conservation claim; Reported permits the explicitly reported reductions; Dropped/Omitted do not claim conservation.

| Report group | Disposition | Arenas |
|---|---|---|
| Rhino | Written | None |
| Rhino | Reported | `appearance_bindings`, `appearances`, `bodies`, `coedges`, `curves`, `edges`, `faces`, `loops`, `pcurves`, `points`, `procedural_curves`, `regions`, `shells`, `surfaces`, `tessellations`, `vertices` |
| Rhino | Dropped | `attributes`, `configurations`, `features`, `parameters`, `procedural_surfaces`, `sketch_constraints`, `sketch_entities`, `sketches`, `subds` |
| Rhino | Omitted | `assembly_joints`, `assets`, `drawings`, `feature_input_topologies`, `feature_result_topologies`, `occurrences`, `pmi`, `presentation_documents`, `presentation_layers`, `product_definitions`, `semantic_annotations`, `spatial_sketch_constraints`, `spatial_sketch_entities`, `spatial_sketches`, `spreadsheets`, `view_presentations` |
| FreeCAD | Written | `appearance_bindings`, `appearances`, `assembly_joints`, `bodies`, `coedges`, `curves`, `edges`, `faces`, `features`, `loops`, `occurrences`, `parameters`, `pcurves`, `points`, `presentation_documents`, `procedural_curves`, `procedural_surfaces`, `product_definitions`, `regions`, `shells`, `sketch_constraints`, `sketch_entities`, `sketches`, `spreadsheets`, `surfaces`, `vertices`, `view_presentations` |
| FreeCAD | Reported | None |
| FreeCAD | Dropped | `assets`, `attributes`, `configurations`, `drawings`, `feature_input_topologies`, `feature_result_topologies`, `pmi`, `presentation_layers`, `semantic_annotations`, `spatial_sketch_constraints`, `spatial_sketch_entities`, `spatial_sketches`, `subds`, `tessellations` |
| FreeCAD | Omitted | None |
| FreeCAD GUI reduction | Written | `assembly_joints`, `bodies`, `coedges`, `curves`, `edges`, `faces`, `features`, `loops`, `occurrences`, `parameters`, `pcurves`, `points`, `procedural_curves`, `procedural_surfaces`, `product_definitions`, `regions`, `shells`, `sketch_constraints`, `sketch_entities`, `sketches`, `spreadsheets`, `surfaces`, `vertices` |
| FreeCAD GUI reduction | Reported | `appearance_bindings`, `appearances`, `presentation_documents`, `view_presentations` |
| FreeCAD GUI reduction | Dropped | `assets`, `attributes`, `configurations`, `drawings`, `feature_input_topologies`, `feature_result_topologies`, `pmi`, `presentation_layers`, `semantic_annotations`, `spatial_sketch_constraints`, `spatial_sketch_entities`, `spatial_sketches`, `subds`, `tessellations` |
| FreeCAD GUI reduction | Omitted | None |

The captured Rhino plans declare no neutral arena Written: their model geometry and appearances are Reported. Consequently the generic Written-arena equality assertion selects an empty set for those plans. The substantive E2 checks separately pin edited names/colors, translated coordinates, and geometry/topology counts. FreeCAD makes nonempty Written claims, and the E1 exact global identity-bijection comparisons exercise those claims. The GUI fixture reports appearance/presentation reductions instead of making false Written claims.

## Actual native dispositions

All captured native coverage maps use Declared mode. The following groups are exact observed maps.

| Report group | Disposition | Native arenas |
|---|---|---|
| Rhino | Written | `layers`, `object_presentation` |
| Rhino | Omitted | `annotation_settings`, `annotations`, `applications`, `construction_planes`, `dimension_styles`, `document_notes`, `document_settings`, `embedded_images`, `external_references`, `grid_defaults`, `groups`, `hatch_patterns`, `history_records`, `lights`, `linetypes`, `materials`, `previews`, `product_definitions`, `product_occurrences`, `render_settings`, `revisions`, `setting_records`, `text_dots`, `text_styles`, `texture_mappings`, `unknowns`, `views`, `windows_bitmaps` |
| Rhino generated defaults | Written | `document_settings`, `layers`, `object_presentation` |
| Rhino generated defaults | Omitted | `annotation_settings`, `annotations`, `applications`, `construction_planes`, `dimension_styles`, `document_notes`, `embedded_images`, `external_references`, `grid_defaults`, `groups`, `hatch_patterns`, `history_records`, `lights`, `linetypes`, `materials`, `previews`, `product_definitions`, `product_occurrences`, `render_settings`, `revisions`, `setting_records`, `text_dots`, `text_styles`, `texture_mappings`, `unknowns`, `views`, `windows_bitmaps` |
| FreeCAD | Reported | `applications`, `attachments`, `element_maps`, `extensions`, `gui_documents`, `gui_properties`, `gui_view_providers`, `objects`, `properties`, `string_tables` |

Rhino can regenerate default-equivalent `document_settings` for the generated-point fixture; other captured source settings are omitted and reported. Shared `export/native.records-omitted` losses identify the writer namespace and omitted native arenas. FreeCAD native Reported entries do not assert complete preservation of every native record.

## Per-fixture chosen paths and export losses

Loss entries include namespace, local code, severity, and repeated-note count. `None` means the actual export report contained no loss notes. Counts and full loss messages remain in the collected report artifacts; they are not inferred from source decoder history.

| Fixture | Path | Neutral report | Native report | Export losses |
|---|---|---|---|---|
| `arc.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `brep.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `brep_singular_seam.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/target.source-dialect-displaced` (warning); `rhino/writer.body-omitted` (error) |
| `circle.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `curves_mixed.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `extrusion.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning); `rhino/writer.procedural-reduced` (warning) |
| `extrusion_hole.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning); `rhino/writer.procedural-reduced` (warning) |
| `generated_point.3dm` | Synthesized | Rhino | Rhino generated defaults | None |
| `instance_reference_expanded.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/target.source-dialect-displaced` (warning) |
| `instance_reference_retained.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/target.source-dialect-displaced` (warning) |
| `line.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `mesh_v1_0.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `mesh_v3_5.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `mesh_v3_8.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `metadata_only_v3.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/target.source-dialect-displaced` (warning) |
| `metadata_only_v4.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/target.source-dialect-displaced` (warning) |
| `nurbs_curve.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `point.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `point_cloud.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `polycurve.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `polyline.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning) |
| `rev_surface.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/target.source-dialect-displaced` (warning); `rhino/writer.procedural-reduced` (warning) |
| `subd_v50.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/target.source-dialect-displaced` (warning); `rhino/writer.subds-omitted` (warning) |
| `subd_v60.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/target.source-dialect-displaced` (warning); `rhino/writer.subds-omitted` (warning) |
| `subd_v70.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/target.source-dialect-displaced` (warning); `rhino/writer.subds-omitted` (warning) |
| `subd_v80.3dm` | Synthesized | Rhino | Rhino | `export/native.records-omitted` (info); `rhino/export.no-exportable-solids` (warning); `rhino/writer.subds-omitted` (warning) |
| `application_payloads.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/geometry.not-transferred` (error); `fcstd/metadata.native-property-not-transferred` (info) ×6 |
| `binary_exact_shape.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info) |
| `core_design_product.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/writer.brep-not-regenerated` (warning) |
| `core_operations.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/metadata.native-property-not-transferred` (info) ×4; `fcstd/writer.brep-not-regenerated` (warning) |
| `design_history.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/metadata.native-property-not-transferred` (info) ×2; `fcstd/writer.brep-not-regenerated` (warning) |
| `external_component.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info) |
| `geometry_topology.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/metadata.native-property-not-transferred` (info) |
| `gui_appearance.FCStd` | Synthesized | FreeCAD GUI reduction | FreeCAD | `export/native.records-omitted` (info); `fcstd/appearance.not-transferred` (warning) ×25; `fcstd/metadata.native-property-not-transferred` (info) ×4; `fcstd/presentation.not-transferred` (warning) ×4 |
| `product_assembly.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/metadata.native-property-not-transferred` (info) ×11 |
| `sketch_conics.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/writer.brep-not-regenerated` (warning) |
| `sketch_constraints.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/writer.brep-not-regenerated` (warning) |
| `techdraw_annotations.FCStd` | Synthesized | FreeCAD | FreeCAD | `export/native.records-omitted` (info); `fcstd/arena.not-transferred` (warning) ×2; `fcstd/metadata.native-property-not-transferred` (info); `fcstd/writer.brep-not-regenerated` (warning) |

## Verification boundary

This collection proves successful source-less synthesis and re-decode with consistent delivered scope for all 38 inputs. It records reductions, including Error-severity omitted geometry, rather than treating output success as losslessness. It does not assert exact Written-content equality for every fixture independently; that property is tested by the dedicated document-contract cases. Native FreeCAD application acceptance and private-corpus results are separate checks.
