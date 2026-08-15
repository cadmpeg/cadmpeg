// SPDX-License-Identifier: Apache-2.0
//! IR-writing attachment of the native object model.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, ChamferSpec,
    ConfigurationBodies, ConfigurationFeatureState, ConfigurationId, CurveProjectionDirection,
    CurveProjectionDirectionState, DesignConfiguration, DesignParameter, EdgeSelection,
    ExtrudeExtent, ExtrudeSide, FaceSelection, Feature, FeatureDefinition, FeatureId,
    FeatureResultTopology, FeatureSourceContent, FeatureTreeNodeRole, HoleForm, HoleKind,
    HolePlacement, Length, ParameterId, ParameterValue, PathRef, PatternKind, ProfileRef,
    RadiusForm, RadiusSpec, RibConstruction, RibDraft, SketchSpace, SweepMode, Termination,
    ThickenSide, TrimRegion,
};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    AppearanceId, AttributeId, BodyId, CurveId, EdgeId, FaceId, FeatureResultTopologyId, LoopId,
    SurfaceId, UnknownId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchPlacement,
};
use cadmpeg_ir::topology::{BodyKind, Coedge, Color, Face, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::EntryContent;
use crate::decode::Scan;
use crate::native::history::{active_feature_closure, BodyWriterHistory};
use crate::native::segments::BooleanOffsetStoreResolution;
use crate::native::vector::{cross_vector, dot_vector, unit_vector};

use super::catalogue::NATIVE_CATALOGUE;
use super::display_jt::{display_jt_tessellations, DisplayJtTessellationInputs};
use cadmpeg_ir::native::catalogue::Phase;

pub(crate) fn attach_container_layer(
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) {
    attach_container_payloads(ir, scan, annotations, unknowns);
    attach_indexed_om_unknowns(scan, annotations, unknowns);
}

fn attach_container_payloads(
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) {
    let annotation_stream = annotations.stream("nx:container");
    for (ordinal, entry) in scan.container.entries.iter().enumerate() {
        let content = entry.content();
        if !content.retains_opaque_payload() {
            continue;
        }
        let Some((offset, byte_len)) = entry.file_span else {
            continue;
        };
        let (Ok(start), Ok(byte_len_usize)) = (usize::try_from(offset), usize::try_from(byte_len))
        else {
            continue;
        };
        let Some(end) = start.checked_add(byte_len_usize) else {
            continue;
        };
        let Some(bytes) = scan.container.data.get(start..end) else {
            continue;
        };
        let id = UnknownId(format!("nx:container-entry:opaque#{ordinal}"));
        annotations
            .note(&id, annotation_stream, offset)
            .tag(content.label());
        annotations.exactness(&id, Exactness::ByteExact);
        unknowns.push(UnknownRecord {
            id,
            offset,
            byte_len,
            sha256: sha256_hex(bytes),
            data: Some(bytes.to_vec()),
            links: Vec::new(),
        });
    }
    attach_jpeg_preview_assets(ir, scan, annotations, unknowns);
}

fn attach_indexed_om_unknowns(
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) {
    let annotation_stream = annotations.stream("nx:container");
    let object_sections = scan.container.indexed_om_sections();
    for (section_index, (entry, section)) in object_sections.iter().enumerate() {
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (record_index, record) in section
            .control
            .iter()
            .chain(section.records.iter())
            .enumerate()
        {
            let kind = if record.object_id.is_some() {
                "record"
            } else {
                "block"
            };
            let id = UnknownId(format!(
                "nx:om-section-{section_index}:{kind}#{record_index}"
            ));
            let offset = entry_offset + record.offset as u64;
            annotations
                .note(&id, annotation_stream, offset)
                .tag(if record.object_id.is_some() {
                    "OM_ENTITY_RECORD"
                } else {
                    "OM_DATA_BLOCK"
                });
            annotations.exactness(&id, Exactness::ByteExact);
            unknowns.push(UnknownRecord {
                id,
                offset,
                byte_len: record.bytes.len() as u64,
                sha256: sha256_hex(record.bytes),
                data: Some(record.bytes.to_vec()),
                links: Vec::new(),
            });
        }
    }
}

pub(crate) fn attach(
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    attach_container_payloads(ir, scan, annotations, unknowns);
    let has_object_sections = !scan.container.indexed_om_sections().is_empty();
    let annotation_stream = annotations.stream("nx:container");
    if model.is_empty() && !has_object_sections {
        return Ok(());
    }
    attach_rm_face_colors(ir, model, scan, annotations);
    attach_rm_appearances(ir, model, scan, annotations);
    let display_jt_tessellations = display_jt_tessellations(&DisplayJtTessellationInputs {
        meshes: &model.display_jt.display_jt_polygon_meshes,
        coordinates: &model.display_jt.display_jt_vertex_coordinates,
        normals: &model.display_jt.display_jt_vertex_normals,
        colors: &model.display_jt.display_jt_vertex_colors,
        texture_coordinates: &model.display_jt.display_jt_vertex_texture_coordinates,
        vertex_flags: &model.display_jt.display_jt_vertex_flags,
        vertex_headers: &model.display_jt.display_jt_vertex_records_headers,
        coordinate_headers: &model.display_jt.display_jt_coordinate_array_headers,
        shape_elements: &model.display_jt.display_jt_shape_lod_elements,
        bindings: &model.display_jt.display_jt_shape_lod_bindings,
        shape_nodes: &model.display_jt.display_jt_tri_strip_shape_nodes,
        base_nodes: &model.display_jt.display_jt_base_node_data,
        group_nodes: &model.display_jt.display_jt_group_node_data,
        instance_nodes: &model.display_jt.display_jt_instance_nodes,
        transforms: &model.display_jt.display_jt_geometric_transform_attributes,
        materials: &model.display_jt.display_jt_material_attributes,
        compressed_elements: &model.display_jt.display_jt_compressed_elements,
    })
    .unwrap_or_default();
    for (tessellation, source_offset) in display_jt_tessellations {
        annotations
            .note(&tessellation.id, annotation_stream, source_offset)
            .tag("DISPLAY_JT_TESSELLATION");
        annotations.exactness(&tessellation.id, Exactness::Derived);
        ir.model.tessellations.push(tessellation);
    }
    NATIVE_CATALOGUE.note_phase(Phase::GroupA, model, annotations);
    attach_material_texture_assets(ir, model, scan, annotations);
    for attribute in &model.om.part_attributes {
        annotations
            .note(&attribute.id, annotation_stream, attribute.source_offset)
            .tag("Attribute");
        annotations.exactness(&attribute.id, Exactness::ByteExact);
        let id = AttributeId(format!("{}:neutral", attribute.id));
        annotations
            .note(&id.0, annotation_stream, attribute.source_offset)
            .tag("Attribute");
        annotations.derived(&id.0, "target");
        annotations.derived(&id.0, "name");
        annotations.derived(&id.0, "values");
        ir.model.attributes.push(SourceAttribute {
            id,
            target: AttributeTarget::Document,
            name: attribute.title.clone(),
            values: vec![AttributeValue::String(attribute.value.clone())],
        });
    }
    attach_parasolid_topology_string_attributes(
        ir,
        &ParasolidStringAttributeSources {
            topology_references: &model.parasolid.parasolid_topology_attribute_list_references,
            class_uses: &model.parasolid.parasolid_topology_attribute_class_uses,
            definitions: &model.parasolid.parasolid_attribute_definitions,
            field_uses: &model.parasolid.parasolid_attribute_field_uses,
            field_names: &model.parasolid.parasolid_attribute_field_names,
            string_uses: &model.parasolid.parasolid_entity_51_string_uses,
            strings: &model.parasolid.parasolid_entity_54_string_records,
        },
        annotations,
    );
    attach_parasolid_topology_numeric_attributes(
        ir,
        &ParasolidNumericAttributeSources {
            topology_references: &model.parasolid.parasolid_topology_attribute_list_references,
            class_uses: &model.parasolid.parasolid_topology_attribute_class_uses,
            definitions: &model.parasolid.parasolid_attribute_definitions,
            field_uses: &model.parasolid.parasolid_attribute_field_uses,
            field_names: &model.parasolid.parasolid_attribute_field_names,
            numeric_uses: &model.parasolid.parasolid_entity_51_numeric_uses,
            integers: &model.parasolid.parasolid_entity_52_integer_records,
            doubles: &model.parasolid.parasolid_entity_53_double_records,
        },
        annotations,
    );
    attach_parasolid_topology_structured_attributes(
        ir,
        &ParasolidStructuredAttributeSources {
            topology_references: &model.parasolid.parasolid_topology_attribute_list_references,
            class_uses: &model.parasolid.parasolid_topology_attribute_class_uses,
            definitions: &model.parasolid.parasolid_attribute_definitions,
            field_uses: &model.parasolid.parasolid_attribute_field_uses,
            field_names: &model.parasolid.parasolid_attribute_field_names,
            structured_uses: &model.parasolid.parasolid_entity_51_structured_uses,
            vectors: &model.parasolid.parasolid_entity_vector_records,
            axes: &model.parasolid.parasolid_entity_57_axis_records,
            tags: &model.parasolid.parasolid_entity_58_tag_records,
            unicode: &model.parasolid.parasolid_entity_62_unicode_records,
        },
        annotations,
    );
    NATIVE_CATALOGUE.note_phase(Phase::GroupB, model, annotations);
    attach_indexed_om_unknowns(scan, annotations, unknowns);
    if !model.om.configurations.is_empty() {
        for (ordinal, configuration) in model.om.configurations.iter().enumerate() {
            let id = ConfigurationId(format!("nx:arrangements:configuration#{ordinal}"));
            let active_attribute_use = model
                .om
                .configuration_attribute_uses
                .iter()
                .find(|relation| relation.configuration == configuration.id);
            let bodies = if active_attribute_use.is_some() {
                ConfigurationBodies::Resolved(
                    ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
                )
            } else {
                ConfigurationBodies::Unresolved
            };
            annotations
                .note(&id.0, annotation_stream, configuration.source_offset)
                .tag("Arrangement");
            annotations.derived(&id.0, "ordinal");
            if active_attribute_use.is_some() {
                annotations.derived(&id.0, "active");
            }
            annotations.derived(&id.0, "source_index");
            annotations.derived(&id.0, "name");
            annotations.derived(&id.0, "native_ref");
            if bodies.resolved().is_some_and(|bodies| !bodies.is_empty()) {
                annotations.derived(&id.0, "bodies");
            }
            ir.model.configurations.push(DesignConfiguration {
                id,
                ordinal: ordinal as u32,
                active: active_attribute_use.is_some().into(),
                source_index: Some(ordinal as u32),
                name: configuration.name.clone().into(),
                material: None,
                properties: active_attribute_use
                    .map(|relation| {
                        BTreeMap::from([("active_attribute_use".to_string(), relation.id.clone())])
                    })
                    .unwrap_or_default(),
                parameter_overrides: BTreeMap::new(),
                suppressed_features: Vec::new(),
                bodies,
                parameter_values: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                native_ref: Some(configuration.id.clone()),
            });
        }
    }
    attach_expression_parameters(
        ir,
        &model.om.expressions,
        &model.om.expression_declarations,
        &model.features.feature_parameter_uses,
        annotations,
    );
    attach_active_configuration_parameter_values(ir, annotations);
    attach_feature_operations(
        ir,
        &model.features,
        &model.om.data_blocks,
        &model.om.expressions,
        &model.segments.segment_body_bindings,
        annotations,
    );
    attach_block_dimension_parameter_consumers(
        ir,
        &model.features.feature_block_dimensions,
        annotations,
    );
    attach_current_feature_states(ir, annotations);
    attach_active_configuration_feature_states(ir, annotations);
    ir.model
        .features
        .sort_by(|first, second| first.id.cmp(&second.id));
    let namespace = ir.native.namespace_mut("nx");
    namespace.version = namespace.version.max(189);
    NATIVE_CATALOGUE.emit_all(model, namespace)?;
    Ok(())
}

fn attach_rm_face_colors(
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) {
    let face_indices = ir
        .model
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| (face.id.0.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let face_ids = face_indices.keys().cloned().collect::<BTreeSet<_>>();
    let bindings = resolve_rm_face_colors(
        &face_ids,
        &model.om.rm_display_color_assignments,
        &model.om.part_color_definitions,
        &model.parasolid.parasolid_deltas_records,
        &super::substrate::paired_delta_streams(scan),
    );
    for (face_id, color) in bindings {
        let Some(index) = face_indices.get(&face_id).copied() else {
            continue;
        };
        let face = &mut ir.model.faces[index];
        if face.color.is_none() || face.color == Some(color) {
            face.color = Some(color);
            annotations.derived(&face.id, "color");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RmSourceColorBinding {
    source_id: String,
    color_definition: String,
    source_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RmFaceColorBinding {
    face_id: String,
    color_definition: String,
    source_offset: u64,
}

fn attach_rm_appearances(
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) {
    let source_bindings = resolve_rm_source_color_bindings(&model.om.rm_display_color_assignments);
    let face_ids = ir
        .model
        .faces
        .iter()
        .map(|face| face.id.0.clone())
        .collect::<BTreeSet<_>>();
    let face_bindings = resolve_rm_face_color_bindings(
        &face_ids,
        &model.om.rm_display_color_assignments,
        &model.om.part_color_definitions,
        &model.parasolid.parasolid_deltas_records,
        &super::substrate::paired_delta_streams(scan),
    );
    if source_bindings.is_empty() && face_bindings.is_empty() {
        return;
    }
    let definitions = model
        .om
        .part_color_definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect::<BTreeMap<_, _>>();
    let annotation_stream = annotations.stream("nx:container");
    let mut appearances = BTreeMap::<String, AppearanceId>::new();
    for binding in source_bindings {
        let Some(definition) = definitions.get(binding.color_definition.as_str()) else {
            continue;
        };
        let appearance_id = ensure_rm_color_appearance(
            ir,
            annotations,
            &mut appearances,
            definition,
            annotation_stream,
        );
        let binding_id = format!(
            "nx:appearance-binding:rmfastload-color#{}",
            native_entity_key(&binding.source_id)
        );
        annotations
            .note(&binding_id, annotation_stream, binding.source_offset)
            .tag("RMFASTLOAD_COLOR_ASSIGNMENT");
        annotations.derived(&binding_id, "target");
        annotations.derived(&binding_id, "appearance");
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: binding_id,
            target: AppearanceTarget::Source {
                source_id: binding.source_id.clone(),
            },
            appearance: appearance_id,
            source_entity_id: Some(binding.source_id),
            object_type: Some("RMFastLoad object ID".into()),
            channels: BTreeMap::new(),
        });
    }
    for binding in face_bindings {
        let Some(definition) = definitions.get(binding.color_definition.as_str()) else {
            continue;
        };
        let Some(existing_color) = ir
            .model
            .faces
            .iter()
            .find(|face| face.id.0 == binding.face_id)
            .map(|face| face.color)
        else {
            continue;
        };
        let color = Color {
            r: definition.rgb[0],
            g: definition.rgb[1],
            b: definition.rgb[2],
            a: 1.0,
        };
        if existing_color.is_some_and(|existing| existing != color) {
            continue;
        }
        let appearance_id = ensure_rm_color_appearance(
            ir,
            annotations,
            &mut appearances,
            definition,
            annotation_stream,
        );
        let binding_id = format!(
            "nx:appearance-binding:rmfastload-face-color#{}",
            native_entity_key(&binding.face_id)
        );
        annotations
            .note(&binding_id, annotation_stream, binding.source_offset)
            .tag("RMFASTLOAD_FACE_COLOR_ASSIGNMENT");
        annotations.derived(&binding_id, "target");
        annotations.derived(&binding_id, "appearance");
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: binding_id,
            target: AppearanceTarget::Face(FaceId(binding.face_id.clone())),
            appearance: appearance_id,
            source_entity_id: Some(binding.face_id),
            object_type: Some("Parasolid FACE".into()),
            channels: BTreeMap::new(),
        });
    }
}

fn ensure_rm_color_appearance(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    appearances: &mut BTreeMap<String, AppearanceId>,
    definition: &crate::native::om::PartColorDefinition,
    annotation_stream: cadmpeg_ir::annotations::StreamHandle,
) -> AppearanceId {
    appearances
        .entry(definition.id.clone())
        .or_insert_with(|| {
            let id = AppearanceId(format!(
                "nx:appearance:rmfastload-color#{}",
                native_entity_key(&definition.id)
            ));
            annotations
                .note(&id.0, annotation_stream, definition.source_offset)
                .tag("RMFASTLOAD_COLOR_APPEARANCE");
            annotations.derived(&id.0, "name");
            annotations.derived(&id.0, "schema");
            annotations.derived(&id.0, "base_color");
            ir.model.appearances.push(Appearance {
                id: id.clone(),
                name: Some(definition.name.clone()),
                asset_guid: None,
                library_id: None,
                visual_guid: None,
                physical_token: None,
                schema: Some("UGS::COLOR_table".into()),
                category: None,
                base_color: Some(Color {
                    r: definition.rgb[0],
                    g: definition.rgb[1],
                    b: definition.rgb[2],
                    a: 1.0,
                }),
                properties: BTreeMap::new(),
                textures: Vec::new(),
            });
            id
        })
        .clone()
}

fn native_entity_key(id: &str) -> String {
    id.replace([':', '#'], "-")
}

fn resolve_rm_source_color_bindings(
    assignments: &[super::om::RmDisplayColorAssignment],
) -> Vec<RmSourceColorBinding> {
    let mut definitions_by_source = BTreeMap::<String, BTreeSet<String>>::new();
    let mut first_offset_by_source = BTreeMap::<String, u64>::new();
    for assignment in assignments {
        let Some(source_id) = assignment.target_object_id.as_ref() else {
            continue;
        };
        definitions_by_source
            .entry(source_id.clone())
            .or_default()
            .insert(assignment.color_definition.clone());
        first_offset_by_source
            .entry(source_id.clone())
            .and_modify(|offset| *offset = (*offset).min(assignment.source_offset))
            .or_insert(assignment.source_offset);
    }
    definitions_by_source
        .into_iter()
        .filter_map(|(source_id, color_definitions)| {
            let mut definitions = color_definitions.into_iter();
            let color_definition = definitions.next()?;
            if definitions.next().is_some() {
                return None;
            }
            let source_offset = first_offset_by_source
                .get(&source_id)
                .copied()
                .expect("every source has one assignment");
            Some(RmSourceColorBinding {
                source_id,
                color_definition,
                source_offset,
            })
        })
        .collect()
}

fn resolve_rm_face_colors(
    face_ids: &BTreeSet<String>,
    assignments: &[super::om::RmDisplayColorAssignment],
    definitions: &[super::om::PartColorDefinition],
    records: &[super::parasolid::ParasolidDeltasRecord],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
) -> Vec<(String, Color)> {
    let definitions_by_id = definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect::<BTreeMap<_, _>>();
    resolve_rm_face_color_bindings(face_ids, assignments, definitions, records, delta_pairs)
        .into_iter()
        .filter_map(|binding| {
            let definition = definitions_by_id.get(binding.color_definition.as_str())?;
            Some((
                binding.face_id,
                Color {
                    r: definition.rgb[0],
                    g: definition.rgb[1],
                    b: definition.rgb[2],
                    a: 1.0,
                },
            ))
        })
        .collect()
}

fn resolve_rm_face_color_bindings(
    face_ids: &BTreeSet<String>,
    assignments: &[super::om::RmDisplayColorAssignment],
    definitions: &[super::om::PartColorDefinition],
    records: &[super::parasolid::ParasolidDeltasRecord],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
) -> Vec<RmFaceColorBinding> {
    let mut partitions_by_delta = BTreeMap::<u32, BTreeSet<u32>>::new();
    for (partition, deltas) in delta_pairs {
        for delta in deltas {
            partitions_by_delta
                .entry(*delta as u32)
                .or_default()
                .insert(*partition as u32);
        }
    }

    let mut colors_by_object = BTreeMap::<u32, BTreeSet<String>>::new();
    for assignment in assignments {
        let crate::native::om::RmDisplayColorAssignmentEncoding::Linked { object_index, .. } =
            &assignment.encoding
        else {
            continue;
        };
        colors_by_object
            .entry(*object_index)
            .or_default()
            .insert(assignment.color_definition.clone());
    }
    let mut source_offsets_by_object = BTreeMap::<u32, u64>::new();
    for assignment in assignments {
        let crate::native::om::RmDisplayColorAssignmentEncoding::Linked { object_index, .. } =
            &assignment.encoding
        else {
            continue;
        };
        source_offsets_by_object
            .entry(*object_index)
            .and_modify(|offset| *offset = (*offset).min(assignment.source_offset))
            .or_insert(assignment.source_offset);
    }
    let definitions_by_id = definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect::<BTreeMap<_, _>>();
    let mut face_records_by_node = BTreeMap::<u32, Vec<_>>::new();
    for record in records.iter().filter(|record| record.family == "FACE") {
        let Some(node_id) = record.node_id else {
            continue;
        };
        face_records_by_node
            .entry(node_id)
            .or_default()
            .push(record);
    }

    let mut bindings = Vec::new();
    for (object_index, definition_ids) in colors_by_object {
        if definition_ids.len() != 1 {
            continue;
        }
        let definition_id = definition_ids.first().expect("one definition id");
        let Some(_definition) = definitions_by_id.get(definition_id.as_str()) else {
            continue;
        };
        let candidates = face_records_by_node
            .get(&object_index)
            .into_iter()
            .flatten()
            .filter_map(|record| {
                let partitions = partitions_by_delta.get(&record.stream_ordinal)?;
                if partitions.len() != 1 {
                    return None;
                }
                let partition = partitions.first().expect("one partition");
                let face_id = format!("nx:s{partition}:face#{}", record.xmt);
                face_ids.contains(&face_id).then_some(face_id)
            })
            .collect::<BTreeSet<_>>();
        if candidates.len() != 1 {
            continue;
        }
        let face_id = candidates.first().expect("one face id");
        let source_offset = source_offsets_by_object
            .get(&object_index)
            .copied()
            .expect("every linked color object has an assignment");
        bindings.push(RmFaceColorBinding {
            face_id: face_id.clone(),
            color_definition: definition_id.clone(),
            source_offset,
        });
    }
    bindings.sort_by(|left, right| left.face_id.cmp(&right.face_id));
    bindings
}

/// Transfer each independently validated JPEG preview with its exact bounded
/// container bytes. Invalid entries remain absent from the neutral asset arena.
fn attach_jpeg_preview_assets(
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) {
    let stream = annotations.stream("nx:container");
    for (ordinal, entry) in scan
        .container
        .entries
        .iter()
        .filter(|entry| entry.content() == EntryContent::PreviewImage)
        .enumerate()
    {
        let Some((source_offset, source_byte_len)) = entry.file_span else {
            continue;
        };
        let (Ok(start), Ok(byte_len)) = (
            usize::try_from(source_offset),
            usize::try_from(source_byte_len),
        ) else {
            continue;
        };
        let Some(bytes) = start
            .checked_add(byte_len)
            .and_then(|end| scan.container.data.get(start..end))
        else {
            continue;
        };
        let native_ref = format!("nx:container:jpeg-preview#{ordinal}");
        if crate::decode::jpeg_dimensions(bytes).is_none() {
            annotations
                .note(&native_ref, stream, source_offset)
                .tag("JPEG_PREVIEW_INVALID");
            annotations.exactness(&native_ref, Exactness::ByteExact);
            unknowns.push(UnknownRecord {
                id: UnknownId(native_ref),
                offset: source_offset,
                byte_len: source_byte_len,
                sha256: sha256_hex(bytes),
                data: Some(bytes.to_vec()),
                links: Vec::new(),
            });
            continue;
        }
        let id = AssetId(format!("{native_ref}:asset"));
        annotations
            .note(&id.0, stream, source_offset)
            .tag("JPEG_PREVIEW_ASSET");
        annotations.exactness(&id.0, Exactness::ByteExact);
        annotations.derived(&id.0, "id");
        annotations.derived(&id.0, "name");
        annotations.derived(&id.0, "media_type");
        annotations.derived(&id.0, "native_ref");
        ir.model.assets.push(Asset {
            id,
            name: Some(if ordinal == 0 {
                "preview.jpg".to_string()
            } else {
                format!("preview-{ordinal}.jpg")
            }),
            media_type: Some("image/jpeg".to_string()),
            content: AssetContent::Embedded {
                data: bytes.to_vec(),
            },
            native_ref: Some(native_ref),
        });
    }
}

/// Transfer the complete validated TIFF set atomically.
fn attach_material_texture_assets(
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) {
    let assets = model
        .om
        .material_texture_assets
        .iter()
        .map(|texture| {
            let start = usize::try_from(texture.source_offset).ok()?;
            let byte_len = usize::try_from(texture.byte_len).ok()?;
            let bytes = scan
                .container
                .data
                .get(start..start.checked_add(byte_len)?)?;
            (sha256_hex(bytes) == texture.sha256).then_some(Asset {
                id: AssetId(format!("{}:asset", texture.id)),
                name: Some(texture.name.clone()),
                media_type: Some("image/tiff".to_string()),
                content: AssetContent::Embedded {
                    data: bytes.to_vec(),
                },
                native_ref: Some(texture.id.clone()),
            })
        })
        .collect::<Option<Vec<_>>>();
    let Some(assets) = assets else {
        return;
    };
    let stream = annotations.stream("nx:container");
    for (texture, asset) in model.om.material_texture_assets.iter().zip(&assets) {
        annotations
            .note(&asset.id.0, stream, texture.source_offset)
            .tag("MATERIAL_TEXTURE_ASSET");
        annotations.exactness(&asset.id.0, Exactness::ByteExact);
        annotations.derived(&asset.id.0, "id");
        annotations.derived(&asset.id.0, "media_type");
        annotations.derived(&asset.id.0, "native_ref");
    }
    ir.model.assets.extend(assets);
}

fn attach_active_configuration_parameter_values(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    let Some(configuration_index) = unique_active_configuration_index(&ir.model.configurations)
    else {
        return;
    };
    let configuration = &ir.model.configurations[configuration_index];
    if configuration.bodies.is_unresolved()
        || !configuration.parameter_values.is_empty()
        || ir.model.parameters.is_empty()
    {
        return;
    }
    let parameters_by_id = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter))
        .collect::<BTreeMap<_, _>>();
    if parameters_by_id.len() != ir.model.parameters.len()
        || ir.model.parameters.iter().any(|parameter| {
            parameter.value.is_none()
                || parameter.dependencies.iter().any(|dependency| {
                    parameters_by_id.get(dependency).is_none_or(|dependency| {
                        dependency.owner != parameter.owner
                            || dependency.ordinal >= parameter.ordinal
                    })
                })
        })
    {
        return;
    }
    let values = ir
        .model
        .parameters
        .iter()
        .map(|parameter| {
            (
                parameter.id.clone(),
                parameter
                    .value
                    .clone()
                    .expect("validated parameter has an evaluated value"),
            )
        })
        .collect();
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.parameter_values = values;
    annotations.derived(&configuration.id.0, "parameter_values");
}

fn attach_current_feature_states(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let current_bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let Some(active_features) = active_feature_closure(ir, &current_bodies) else {
        return;
    };
    let feature_indices = ir
        .model
        .features
        .iter()
        .enumerate()
        .map(|(index, feature)| (feature.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for id in active_features {
        let feature = feature_indices
            .get(&id)
            .and_then(|index| ir.model.features.get_mut(*index))
            .expect("active feature closure has validated every feature identity");
        feature.suppressed = Some(false);
        annotations.derived(&feature.id, "suppressed");
    }
}

fn attach_active_configuration_feature_states(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let Some(configuration_index) = unique_active_configuration_index(&ir.model.configurations)
    else {
        return;
    };
    let Some(configuration_bodies) = ir.model.configurations[configuration_index]
        .bodies
        .resolved()
        .map(<[BodyId]>::to_vec)
    else {
        return;
    };
    if !ir.model.configurations[configuration_index]
        .feature_states
        .is_empty()
    {
        return;
    }
    let Some(active_features) = active_feature_closure(ir, &configuration_bodies) else {
        return;
    };
    let feature_indices = ir
        .model
        .features
        .iter()
        .enumerate()
        .map(|(index, feature)| (feature.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let states = active_features
        .iter()
        .map(|id| {
            let feature = feature_indices
                .get(id)
                .and_then(|index| ir.model.features.get(*index))
                .expect("active feature has a validated index");
            (
                id.clone(),
                ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: feature.dependencies.clone(),
                    outputs: feature.outputs.clone(),
                    definition: feature.definition.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for id in &active_features {
        let feature = feature_indices
            .get(id)
            .and_then(|index| ir.model.features.get_mut(*index))
            .expect("active feature closure has validated every feature identity");
        if feature.suppressed != Some(false) {
            feature.suppressed = Some(false);
            annotations.derived(&feature.id, "suppressed");
        }
    }
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.feature_states = states;
    annotations.derived(&configuration.id.0, "feature_states");
}

fn unique_active_configuration_index(configurations: &[DesignConfiguration]) -> Option<usize> {
    let active = configurations
        .iter()
        .enumerate()
        .filter_map(|(index, configuration)| configuration.active.is_active().then_some(index))
        .collect::<Vec<_>>();
    let [index] = active.as_slice() else {
        return None;
    };
    Some(*index)
}

/// Materialize the exact body set present when retained feature replay begins.
fn attach_initial_segment_bodies(
    ir: &mut CadIr,
    body_bindings: &[crate::native::segments::SegmentBodyBinding],
    annotations: &mut AnnotationBuilder,
    stream: cadmpeg_ir::annotations::StreamHandle,
) -> Option<FeatureId> {
    let bindings_by_body = ir
        .model
        .bodies
        .iter()
        .filter_map(|body| {
            let bindings = body_bindings
                .iter()
                .filter(|binding| {
                    body.id
                        .0
                        .starts_with(&format!("nx:s{}:", binding.stream_ordinal))
                })
                .map(|binding| binding.id.clone())
                .collect::<Vec<_>>();
            (!bindings.is_empty()).then_some((body.id.clone(), bindings))
        })
        .collect::<BTreeMap<_, _>>();
    if bindings_by_body.is_empty() {
        return None;
    }

    let id = FeatureId("nx:feature-history:feature#initial-bodies".to_string());
    let outputs = bindings_by_body.keys().cloned().collect::<Vec<_>>();
    let source_properties = bindings_by_body
        .values()
        .flatten()
        .enumerate()
        .map(|(ordinal, binding)| (format!("segment_body_binding.{ordinal}"), binding.clone()))
        .collect();
    annotations
        .note(&id, stream, 0)
        .tag("FEATURE_HISTORY_INPUT");
    annotations.derived(&id, "definition");
    ir.model.features.push(Feature {
        id: id.clone(),
        ordinal: ir.model.features.len() as u64,
        name: Some("Retained history input".to_string()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties,
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: outputs.clone(),
        definition: FeatureDefinition::BaseFeature {
            bodies: BodySelection::Resolved {
                bodies: outputs,
                native: "nx:segment-body-bindings".to_string(),
            },
        },
        native_ref: None,
    });
    Some(id)
}

fn attach_feature_operations(
    ir: &mut CadIr,
    features: &crate::native::model::FeatureRecords,
    data_blocks: &[crate::native::om::DataBlock],
    expressions: &[crate::native::om::Expression],
    body_bindings: &[crate::native::segments::SegmentBodyBinding],
    annotations: &mut AnnotationBuilder,
) {
    let labels = features.feature_operation_labels.as_slice();
    let booleans = features.feature_boolean_operations.as_slice();
    let body_references = features.feature_body_references.as_slice();
    let body_segment_uses = features.feature_body_segment_uses.as_slice();
    let body_data_block_uses = features.feature_body_data_block_uses.as_slice();
    let body_reference_occurrences = features.feature_body_reference_occurrences.as_slice();
    let input_blocks = features.feature_input_blocks.as_slice();
    let input_block_identity_groups = features.feature_input_block_identity_groups.as_slice();
    let input_column_row_uses = features.feature_input_column_row_uses.as_slice();
    let input_column_targets = features.feature_input_column_targets.as_slice();
    let datum_csys_constructions = features.feature_datum_csys_constructions.as_slice();
    let datum_csys_column_row_uses = features.feature_datum_csys_column_row_uses.as_slice();
    let datum_csys_payloads = features.feature_datum_csys_payloads.as_slice();
    let datum_csys_payload_scalar_pairs =
        features.feature_datum_csys_payload_scalar_pairs.as_slice();
    let datum_csys_payload_fixed_pairs = features.feature_datum_csys_payload_fixed_pairs.as_slice();
    let datum_csys_payload_scalars = features.feature_datum_csys_payload_scalars.as_slice();
    let datum_csys_descriptors = features.feature_datum_csys_descriptors.as_slice();
    let datum_csys_block_uses = features.feature_datum_csys_block_uses.as_slice();
    let datum_plane_headers = features.feature_datum_plane_headers.as_slice();
    let datum_plane_block_uses = features.feature_datum_plane_block_uses.as_slice();
    let datum_plane_payloads = features.feature_datum_plane_payloads.as_slice();
    let datum_plane_payload_scalar_pairs =
        features.feature_datum_plane_payload_scalar_pairs.as_slice();
    let datum_plane_descriptors = features.feature_datum_plane_descriptors.as_slice();
    let datum_plane_csys_identity_uses = features.feature_datum_plane_csys_identity_uses.as_slice();
    let sketch_datum_csys_dependencies = features.feature_sketch_datum_csys_dependencies.as_slice();
    let sketch_references = features.feature_sketch_references.as_slice();
    let projected_curve_references = features.feature_projected_curve_references.as_slice();
    let projected_curve_construction_payloads = features
        .feature_projected_curve_construction_payloads
        .as_slice();
    let projected_curve_construction_strings = features
        .feature_projected_curve_construction_strings
        .as_slice();
    let fset_reference_graphs = features.feature_fset_reference_graphs.as_slice();
    let fset_construction_payloads = features.feature_fset_construction_payloads.as_slice();
    let delete_reference_fields = features.feature_delete_reference_fields.as_slice();
    let delete_construction_payloads = features.feature_delete_construction_payloads.as_slice();
    let pattern_references = features.feature_pattern_references.as_slice();
    let pattern_construction_payloads = features.feature_pattern_construction_payloads.as_slice();
    let pattern_construction_strings = features.feature_pattern_construction_strings.as_slice();
    let pattern_construction_fixed_lanes =
        features.feature_pattern_construction_fixed_lanes.as_slice();
    let pattern_transform_lanes = features.feature_pattern_transform_lanes.as_slice();
    let multi_instance_output_lanes = features.feature_multi_instance_output_lanes.as_slice();
    let identical_instance_output_lanes =
        features.feature_identical_instance_output_lanes.as_slice();
    let point_construction_headers = features.feature_point_construction_headers.as_slice();
    let point_construction_scalar_lanes =
        features.feature_point_construction_scalar_lanes.as_slice();
    let draft_construction_references = features.feature_draft_construction_references.as_slice();
    let draft_construction_index_lanes = features.feature_draft_construction_index_lanes.as_slice();
    let draft_construction_payloads = features.feature_draft_construction_payloads.as_slice();
    let draft_construction_graph_payloads = features
        .feature_draft_construction_graph_payloads
        .as_slice();
    let draft_construction_fixed_lanes = features.feature_draft_construction_fixed_lanes.as_slice();
    let draft_construction_binary32_lanes = features
        .feature_draft_construction_binary32_lanes
        .as_slice();
    let draft_construction_graph_strings =
        features.feature_draft_construction_graph_strings.as_slice();
    let draft_construction_identity_frames = features
        .feature_draft_construction_identity_frames
        .as_slice();
    let draft_construction_terminal_lanes = features
        .feature_draft_construction_terminal_lanes
        .as_slice();
    let surface_construction_references =
        features.feature_surface_construction_references.as_slice();
    let surface_construction_payloads = features.feature_surface_construction_payloads.as_slice();
    let surface_construction_scalar_pairs = features
        .feature_surface_construction_scalar_pairs
        .as_slice();
    let surface_construction_strings = features.feature_surface_construction_strings.as_slice();
    let surface_construction_branches = features.feature_surface_construction_branches.as_slice();
    let sketch_named_point_block_uses = features.feature_sketch_named_point_block_uses.as_slice();
    let sketch_preceding_named_point_uses = features
        .feature_sketch_preceding_named_point_uses
        .as_slice();
    let sketch_point_uses = features.feature_sketch_point_uses.as_slice();
    let sketch_point_groups = features.feature_sketch_point_groups.as_slice();
    let extrude_profile_references = features.feature_extrude_profile_references.as_slice();
    let extrude_construction_profiles = features.feature_extrude_construction_profiles.as_slice();
    let operation_body_operands = features.feature_operation_body_operands.as_slice();
    let sketch_construction_inputs = features.feature_sketch_construction_inputs.as_slice();
    let sketch_records = features.feature_sketch_records.as_slice();
    let sketch_construction_payloads = features.feature_sketch_construction_payloads.as_slice();
    let sketch_coordinate_pairs = features.feature_sketch_payload_coordinate_pairs.as_slice();
    let sketch_fixed_pairs = features.feature_sketch_payload_fixed_pairs.as_slice();
    let sketch_mixed_pairs = features.feature_sketch_payload_mixed_pairs.as_slice();
    let sketch_payload_scalars = features.feature_sketch_payload_scalars.as_slice();
    let sketch_fixed_points = features.feature_sketch_fixed_points.as_slice();
    let sketch_points = features.feature_sketch_points.as_slice();
    let block_constructions = features.feature_block_constructions.as_slice();
    let block_construction_payloads = features.feature_block_construction_payloads.as_slice();
    let block_dimensions = features.feature_block_dimensions.as_slice();
    let block_payload_points = features.feature_block_payload_points.as_slice();
    let block_payload_point_groups = features.feature_block_payload_point_groups.as_slice();
    let extrude_32_constructions = features.feature_extrude_32_constructions.as_slice();
    let extrude_payload_headers = features.feature_extrude_payload_headers.as_slice();
    let operation_terminal_discriminators = features
        .feature_operation_terminal_discriminators
        .as_slice();
    let extrude_payload_32_branches = features.feature_extrude_payload_32_branches.as_slice();
    let operation_body_scalar_triples = features.feature_operation_body_scalar_triples.as_slice();
    let operation_body_members = features.feature_operation_body_members.as_slice();
    let operation_body_11_continuations =
        features.feature_operation_body_11_continuations.as_slice();
    let operation_body_reference_lanes = features.feature_operation_body_reference_lanes.as_slice();
    let parameter_bindings = features.feature_parameter_bindings.as_slice();
    let parameter_uses = features.feature_parameter_uses.as_slice();
    let operation_records = features.feature_operation_records.as_slice();
    let operation_common_frames = features.feature_operation_common_frames.as_slice();
    let operation_terminal_frames = features.feature_operation_terminal_frames.as_slice();
    let payload_strings = features.feature_payload_strings.as_slice();
    let simple_hole_templates = features.feature_simple_hole_templates.as_slice();
    let simple_hole_repeated_scalar_lanes = features
        .feature_simple_hole_repeated_scalar_lanes
        .as_slice();
    let simple_hole_repeated_scalar_lane_block_references = features
        .feature_simple_hole_repeated_scalar_lane_block_references
        .as_slice();
    let simple_hole_construction_groups =
        features.feature_simple_hole_construction_groups.as_slice();
    let hole_package_construction_group_lanes = features
        .feature_hole_package_construction_group_lanes
        .as_slice();
    let hole_package_construction_group_uses = features
        .feature_hole_package_construction_group_uses
        .as_slice();
    let stream = annotations.stream("nx:container");
    let initial_body_id = attach_initial_segment_bodies(ir, body_bindings, annotations, stream);
    let base_ordinal = ir.model.features.len() as u64;
    let booleans = booleans
        .iter()
        .map(|operation| (operation.operation_label.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let body_references_by_id = body_references
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let mut body_segment_uses_by_reference =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodySegmentUse>>::new();
    for use_ in body_segment_uses {
        body_segment_uses_by_reference
            .entry(use_.feature_body_reference.as_str())
            .or_default()
            .push(use_);
    }
    let mut body_data_block_uses_by_reference =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodyDataBlockUse>>::new();
    for use_ in body_data_block_uses {
        body_data_block_uses_by_reference
            .entry(use_.feature_body_reference.as_str())
            .or_default()
            .push(use_);
    }
    let body_writer_references_by_operation =
        crate::native::features::unique_feature_body_references(body_references);
    let mut offset_store_bodies_by_operation = BTreeMap::<&str, Vec<(u32, String)>>::new();
    for body_use in body_data_block_uses {
        let Some(reference) = body_references_by_id.get(body_use.feature_body_reference.as_str())
        else {
            continue;
        };
        offset_store_bodies_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push((reference.body_object_index, body_use.data_block.clone()));
    }
    let body_references = native_primary_body_references(
        body_references,
        body_data_block_uses,
        body_segment_uses,
        input_blocks,
        data_blocks,
    );
    let mut body_reference_occurrences_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodyReferenceOccurrence>>::new();
    for reference in body_reference_occurrences {
        body_reference_occurrences_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut body_writer_history = BodyWriterHistory::default();
    if let Some(feature) = initial_body_id
        .as_ref()
        .and_then(|id| ir.model.features.iter().find(|feature| feature.id == *id))
    {
        body_writer_history.record_writer(None, None, &feature.outputs, &feature.id);
    }
    let body_alias_roots =
        crate::native::segments::body_alias_roots(body_bindings).unwrap_or_default();
    let canonical_body =
        |identity: u32| body_alias_roots.get(&identity).copied().unwrap_or(identity);
    let mut input_blocks_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureInputBlock>>::new();
    for input in input_blocks {
        input_blocks_by_operation
            .entry(input.operation_label.as_str())
            .or_default()
            .push(input);
    }
    let input_column_row_uses_by_operation =
        records_by_operation(input_column_row_uses, |use_| &use_.operation_label);
    let input_column_targets_by_operation =
        records_by_operation(input_column_targets, |target| &target.operation_label);
    let input_block_identity_group_by_input = input_block_identity_groups
        .iter()
        .flat_map(|group| {
            group
                .input_blocks
                .iter()
                .map(move |input| (input.as_str(), group.id.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let datum_csys_constructions_by_operation = datum_csys_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let datum_csys_payloads_by_operation =
        records_by_operation(datum_csys_payloads, |payload| &payload.operation_label);
    let datum_csys_payload_scalar_pairs_by_operation =
        records_by_operation(datum_csys_payload_scalar_pairs, |pair| {
            &pair.operation_label
        });
    let datum_csys_payload_fixed_pairs_by_operation =
        records_by_operation(datum_csys_payload_fixed_pairs, |pair| &pair.operation_label);
    let datum_csys_payload_scalars_by_operation =
        records_by_operation(datum_csys_payload_scalars, |scalar| &scalar.operation_label);
    let datum_csys_descriptors_by_operation =
        records_by_operation(datum_csys_descriptors, |descriptor| {
            &descriptor.operation_label
        });
    let datum_csys_column_row_uses_by_operation =
        records_by_operation(datum_csys_column_row_uses, |use_| &use_.operation_label);
    let mut datum_csys_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumCsysBlockUse>>::new();
    for block_use in datum_csys_block_uses {
        datum_csys_uses_by_input_operation
            .entry(block_use.input_operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let datum_plane_headers_by_operation = datum_plane_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let datum_plane_payloads_by_operation = datum_plane_payloads
        .iter()
        .map(|payload| (payload.operation_label.as_str(), payload))
        .collect::<BTreeMap<_, _>>();
    let datum_plane_payload_scalar_pairs_by_operation =
        records_by_operation(datum_plane_payload_scalar_pairs, |pair| {
            &pair.operation_label
        });
    let datum_plane_descriptors_by_operation =
        records_by_operation(datum_plane_descriptors, |descriptor| {
            &descriptor.operation_label
        });
    let mut datum_plane_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneBlockUse>>::new();
    for block_use in datum_plane_block_uses {
        datum_plane_uses_by_input_operation
            .entry(block_use.input_operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let chronological_labels =
        crate::native::features::feature_operation_chronological_labels(labels);
    let operation_positions = chronological_labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let sketch_datum_csys_dependencies = sketch_datum_csys_dependencies
        .iter()
        .map(|dependency| (dependency.datum_csys_operation_label.as_str(), dependency))
        .collect::<BTreeMap<_, _>>();
    let mut datum_identity_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneCsysIdentityUse>>::new();
    for identity_use in datum_plane_csys_identity_uses {
        datum_identity_uses_by_operation
            .entry(identity_use.datum_plane_operation_label.as_str())
            .or_default()
            .push(identity_use);
        datum_identity_uses_by_operation
            .entry(identity_use.datum_csys_operation_label.as_str())
            .or_default()
            .push(identity_use);
    }
    let mut sketch_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchReference>>::new();
    for reference in sketch_references {
        sketch_references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let projected_curve_references_by_operation =
        records_by_operation(projected_curve_references, |reference| {
            &reference.operation_label
        });
    let projected_curve_construction_payloads_by_operation =
        records_by_operation(projected_curve_construction_payloads, |payload| {
            &payload.operation_label
        });
    let projected_curve_construction_strings_by_operation =
        records_by_operation(projected_curve_construction_strings, |value| {
            &value.operation_label
        });
    let fset_reference_graphs_by_operation =
        records_by_operation(fset_reference_graphs, |graph| &graph.operation_label);
    let fset_construction_payloads_by_operation =
        records_by_operation(fset_construction_payloads, |payload| {
            &payload.operation_label
        });
    let delete_reference_fields_by_operation =
        records_by_operation(delete_reference_fields, |field| &field.operation_label);
    let delete_construction_payloads_by_operation =
        records_by_operation(delete_construction_payloads, |payload| {
            &payload.operation_label
        });
    let pattern_references_by_operation =
        records_by_operation(pattern_references, |reference| &reference.operation_label);
    let pattern_construction_payloads_by_operation =
        records_by_operation(pattern_construction_payloads, |payload| {
            &payload.operation_label
        });
    let pattern_construction_strings_by_operation =
        records_by_operation(pattern_construction_strings, |value| &value.operation_label);
    let pattern_construction_fixed_lanes_by_operation =
        records_by_operation(pattern_construction_fixed_lanes, |lane| {
            &lane.operation_label
        });
    let pattern_transform_lanes_by_operation =
        records_by_operation(pattern_transform_lanes, |lane| &lane.operation_label);
    let multi_instance_output_lanes_by_operation =
        records_by_operation(multi_instance_output_lanes, |lane| &lane.operation_label);
    let identical_instance_output_lanes_by_operation =
        records_by_operation(identical_instance_output_lanes, |lane| {
            &lane.operation_label
        });
    let point_construction_headers_by_operation = point_construction_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let point_construction_scalar_lanes_by_operation = point_construction_scalar_lanes
        .iter()
        .map(|lane| (lane.operation_label.as_str(), lane))
        .collect::<BTreeMap<_, _>>();
    let draft_construction_references_by_operation =
        records_by_operation(draft_construction_references, |reference| {
            &reference.operation_label
        });
    let draft_construction_index_lanes_by_operation =
        records_by_operation(draft_construction_index_lanes, |lane| &lane.operation_label);
    let draft_construction_payloads_by_operation =
        records_by_operation(draft_construction_payloads, |payload| {
            &payload.operation_label
        });
    let draft_construction_graph_payloads_by_operation =
        records_by_operation(draft_construction_graph_payloads, |payload| {
            &payload.operation_label
        });
    let draft_construction_fixed_lanes_by_operation =
        records_by_operation(draft_construction_fixed_lanes, |lane| &lane.operation_label);
    let draft_construction_binary32_lanes_by_operation =
        records_by_operation(draft_construction_binary32_lanes, |lane| {
            &lane.operation_label
        });
    let draft_construction_graph_strings_by_operation =
        records_by_operation(draft_construction_graph_strings, |value| {
            &value.operation_label
        });
    let draft_construction_identity_frames_by_operation =
        records_by_operation(draft_construction_identity_frames, |frame| {
            &frame.operation_label
        });
    let draft_construction_terminal_lanes_by_operation =
        records_by_operation(draft_construction_terminal_lanes, |lane| {
            &lane.operation_label
        });
    let surface_construction_references_by_operation =
        records_by_operation(surface_construction_references, |reference| {
            &reference.operation_label
        });
    let surface_construction_payloads_by_operation =
        records_by_operation(surface_construction_payloads, |payload| {
            &payload.operation_label
        });
    let surface_construction_scalar_pairs_by_operation =
        records_by_operation(surface_construction_scalar_pairs, |pair| {
            &pair.operation_label
        });
    let surface_construction_strings_by_operation =
        records_by_operation(surface_construction_strings, |value| &value.operation_label);
    let surface_construction_branches_by_operation =
        records_by_operation(surface_construction_branches, |branch| {
            &branch.operation_label
        });
    let mut sketch_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchNamedPointBlockUse>>::new();
    for block_use in sketch_named_point_block_uses {
        sketch_named_point_uses_by_operation
            .entry(block_use.operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let mut sketch_preceding_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPrecedingNamedPointUse>>::new();
    for point_use in sketch_preceding_named_point_uses {
        sketch_preceding_named_point_uses_by_operation
            .entry(point_use.operation_label.as_str())
            .or_default()
            .push(point_use);
    }
    let mut sketch_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPointUse>>::new();
    for point_use in sketch_point_uses {
        sketch_point_uses_by_operation
            .entry(point_use.operation_label.as_str())
            .or_default()
            .push(point_use);
    }
    let mut sketch_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPointGroup>>::new();
    for group in sketch_point_groups {
        sketch_point_groups_by_operation
            .entry(group.operation_label.as_str())
            .or_default()
            .push(group);
    }
    let mut extrude_profile_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureExtrudeProfileReference>>::new();
    for reference in extrude_profile_references {
        extrude_profile_references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let extrude_construction_profiles_by_operation = extrude_construction_profiles
        .iter()
        .map(|profile| (profile.operation_label.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let mut operation_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands {
        operation_body_operands_by_operation
            .entry(operand.operation_label.as_str())
            .or_default()
            .push(operand);
    }
    let mut segment_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands
        .iter()
        .filter(|operand| !operand.segment_body_bindings.is_empty())
    {
        segment_body_operands_by_operation
            .entry(operand.operation_label.as_str())
            .or_default()
            .push(operand);
    }
    let sketch_construction_inputs_by_operation = sketch_construction_inputs
        .iter()
        .map(|inputs| (inputs.operation_label.as_str(), inputs))
        .collect::<BTreeMap<_, _>>();
    let sketch_records_by_operation =
        records_by_operation(sketch_records, |record| &record.operation_label);
    let sketch_construction_payloads_by_operation =
        records_by_operation(sketch_construction_payloads, |payload| {
            &payload.operation_label
        });
    let mut sketch_coordinate_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadCoordinatePair>>::new();
    for pair in sketch_coordinate_pairs {
        sketch_coordinate_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_fixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadFixedPair>>::new();
    for pair in sketch_fixed_pairs {
        sketch_fixed_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_mixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadMixedPair>>::new();
    for pair in sketch_mixed_pairs {
        sketch_mixed_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_fixed_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchFixedPoint>>::new();
    for point in sketch_fixed_points {
        sketch_fixed_points_by_operation
            .entry(point.operation_label.as_str())
            .or_default()
            .push(point);
    }
    let block_constructions_by_operation = block_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let block_construction_payloads_by_operation =
        records_by_operation(block_construction_payloads, |payload| {
            &payload.operation_label
        });
    let block_dimensions_by_operation = block_dimensions
        .iter()
        .map(|dimensions| (dimensions.operation_label.as_str(), dimensions))
        .collect::<BTreeMap<_, _>>();
    let mut block_payload_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBlockPayloadPoint>>::new();
    for point in block_payload_points {
        block_payload_points_by_operation
            .entry(point.operation_label.as_str())
            .or_default()
            .push(point);
    }
    let mut block_payload_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBlockPayloadPointGroup>>::new();
    for group in block_payload_point_groups {
        block_payload_point_groups_by_operation
            .entry(group.operation_label.as_str())
            .or_default()
            .push(group);
    }
    let extrude_32_constructions_by_operation = extrude_32_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let extrude_payload_headers_by_operation = extrude_payload_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let operation_terminal_discriminators_by_operation = operation_terminal_discriminators
        .iter()
        .map(|lane| (lane.operation_label.as_str(), lane))
        .collect::<BTreeMap<_, _>>();
    let extrude_payload_32_branches_by_operation =
        records_by_operation(extrude_payload_32_branches, |branch| {
            &branch.operation_label
        });
    let mut operation_body_scalar_triples_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyScalarTriple>>::new();
    for triple in operation_body_scalar_triples {
        operation_body_scalar_triples_by_operation
            .entry(triple.operation_label.as_str())
            .or_default()
            .push(triple);
    }
    for triples in operation_body_scalar_triples_by_operation.values_mut() {
        triples.sort_by_key(|triple| triple.body_reference_ordinal);
    }
    let mut operation_body_members_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyMember>>::new();
    for member in operation_body_members {
        operation_body_members_by_operation
            .entry(member.operation_label.as_str())
            .or_default()
            .push(member);
    }
    let mut operation_body_11_continuations_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBody11Continuation>>::new();
    for continuation in operation_body_11_continuations {
        operation_body_11_continuations_by_operation
            .entry(continuation.operation_label.as_str())
            .or_default()
            .push(continuation);
    }
    let mut operation_body_reference_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyReferenceLane>>::new();
    for lane in operation_body_reference_lanes {
        operation_body_reference_lanes_by_operation
            .entry(lane.operation_label.as_str())
            .or_default()
            .push(lane);
    }
    let mut bodies_by_object_index = BTreeMap::<u32, Vec<BodyId>>::new();
    for binding in body_bindings {
        let prefix = format!("nx:s{}:", binding.stream_ordinal);
        let mut stream_bodies = Vec::new();
        for body in ir
            .model
            .bodies
            .iter()
            .filter(|body| body.id.0.starts_with(&prefix))
        {
            if !stream_bodies.contains(&body.id) {
                stream_bodies.push(body.id.clone());
            }
        }
        for identity in [binding.body_object_index, binding.body_alias_object_index] {
            let bodies = bodies_by_object_index.entry(identity).or_default();
            for body in &stream_bodies {
                if !bodies.contains(body) {
                    bodies.push(body.clone());
                }
            }
        }
    }
    let explicit_simple_hole_outputs = simple_hole_templates
        .iter()
        .filter_map(|template| {
            let object_index = body_references.get(template.operation_label.as_str())?;
            Some((
                template.operation_label.clone(),
                feature_body_outputs(*object_index, body_bindings, &bodies_by_object_index),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let simple_hole_operations = simple_hole_operations(
        simple_hole_templates,
        simple_hole_construction_groups,
        &operation_positions,
    )
    .unwrap_or_default();
    let mut hole_outputs = explicit_simple_hole_outputs;
    let mut simple_hole_diameters = BTreeMap::new();
    if let Some(projection) = hole_body_projection(ir, &simple_hole_operations, &hole_outputs) {
        hole_outputs.extend(projection.outputs);
        simple_hole_diameters.extend(projection.diameters);
    }
    let counterbore_operations =
        counterbore_operations(simple_hole_templates, &operation_positions).unwrap_or_default();
    let mut counterbore_dimensions = BTreeMap::new();
    if let Some(projection) =
        counterbore_body_projection(ir, &counterbore_operations, &hole_outputs)
    {
        hole_outputs.extend(projection.outputs);
        simple_hole_diameters.extend(projection.diameters);
        counterbore_dimensions.extend(projection.counterbores);
    }
    let blind_hole_operations =
        blind_hole_operations(simple_hole_templates, &operation_positions).unwrap_or_default();
    let mut blind_hole_depths = BTreeMap::new();
    if let Some(projection) = blind_hole_body_projection(ir, &blind_hole_operations, &hole_outputs)
    {
        hole_outputs.extend(projection.outputs);
        simple_hole_diameters.extend(projection.diameters);
        blind_hole_depths = projection.blind_depths;
    }
    let simple_hole_placements =
        hole_axis_placements_for_operations(ir, &simple_hole_operations, &hole_outputs);
    let counterbore_hole_placements =
        counterbore_axis_placements_for_operations(ir, &counterbore_operations, &hole_outputs);
    let blind_hole_placements =
        blind_hole_axis_placements_for_operations(ir, &blind_hole_operations, &hole_outputs);
    let simple_hole_chamfers = simple_hole_chamfers(ir, simple_hole_templates, &hole_outputs);
    let hole_packages = hole_package_projection(
        ir,
        simple_hole_templates,
        simple_hole_construction_groups,
        hole_package_construction_group_uses,
        &hole_outputs,
        &simple_hole_diameters,
        &simple_hole_chamfers,
    );
    let feature_ids_by_operation = labels
        .iter()
        .filter(|label| {
            projects_neutral_feature(&label.value)
                && !hole_packages.internal_operations.contains(&label.id)
        })
        .map(|label| {
            let key = label
                .id
                .strip_prefix("nx:feature-history:operation-label#")
                .unwrap_or(label.id.as_str());
            (
                label.id.as_str(),
                FeatureId(format!("nx:feature-history:feature#{key}")),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut parameter_bindings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterBinding>>::new();
    for binding in parameter_bindings {
        parameter_bindings_by_operation
            .entry(binding.operation_label.as_str())
            .or_default()
            .push(binding);
    }
    let mut parameter_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        parameter_uses_by_operation
            .entry(parameter_use.operation_label.as_str())
            .or_default()
            .push(parameter_use);
    }
    let operation_labels_by_record = operation_records
        .iter()
        .map(|record| (record.id.as_str(), record.operation_label.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut payload_strings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeaturePayloadString>>::new();
    for value in payload_strings {
        let Some(operation) = operation_labels_by_record.get(value.operation_record.as_str())
        else {
            continue;
        };
        payload_strings_by_operation
            .entry(operation)
            .or_default()
            .push(value);
    }
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect::<BTreeMap<_, _>>();
    let annotation_base_order =
        u32::try_from(ir.model.semantic_annotations.len()).unwrap_or(u32::MAX);
    for (annotation_ordinal, label) in labels
        .iter()
        .filter(|label| label.value == "TEXT")
        .enumerate()
    {
        let payload_strings = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice)
            .iter()
            .map(|value| value.value.as_str())
            .collect::<Vec<_>>();
        let Some(annotation) = text_semantic_annotation(
            &label.id,
            annotation_base_order
                .saturating_add(u32::try_from(annotation_ordinal).unwrap_or(u32::MAX)),
            &payload_strings,
        ) else {
            continue;
        };
        annotations
            .note(&annotation.id.0, stream, label.source_offset)
            .tag("TEXT_SEMANTIC_ANNOTATION");
        annotations.exactness(&annotation.id.0, Exactness::Derived);
        ir.model.semantic_annotations.push(annotation);
    }
    for (ordinal, label) in chronological_labels.into_iter().enumerate() {
        if !projects_neutral_feature(&label.value)
            || hole_packages.internal_operations.contains(&label.id)
        {
            continue;
        }
        let id = feature_ids_by_operation
            .get(label.id.as_str())
            .expect("every operation label owns one neutral feature identity")
            .clone();
        let boolean_offset_store_resolution = booleans.get(label.id.as_str()).map(|operation| {
            crate::native::segments::boolean_offset_store_resolution(operation, data_blocks)
        });
        let boolean_definition = booleans
            .get(label.id.as_str())
            .zip(boolean_offset_store_resolution.as_ref())
            .map(|(operation, resolution)| {
                boolean_feature_definition(
                    operation,
                    &body_alias_roots,
                    resolution,
                    &bodies_by_object_index,
                )
            });
        let mut dependencies = Vec::new();
        if let (
            Some(operation),
            Some(resolution),
            Some(FeatureDefinition::Combine { target, tools, .. }),
        ) = (
            booleans.get(label.id.as_str()),
            boolean_offset_store_resolution.as_ref(),
            boolean_definition.as_ref(),
        ) {
            if !matches!(resolution, BooleanOffsetStoreResolution::Unresolved) {
                let offset_store_body_blocks = match resolution {
                    BooleanOffsetStoreResolution::Complete(blocks) => Some(blocks),
                    BooleanOffsetStoreResolution::None
                    | BooleanOffsetStoreResolution::Unresolved => None,
                };
                if let Some(writer) = boolean_participant_writer(
                    target,
                    operation.target_object_index,
                    offset_store_body_blocks,
                    &body_alias_roots,
                    &body_writer_history,
                ) {
                    if !dependencies.contains(writer) {
                        dependencies.push(writer.clone());
                    }
                }
                for body in &operation.tool_object_indices {
                    if let Some(writer) = boolean_participant_writer(
                        tools,
                        *body,
                        offset_store_body_blocks,
                        &body_alias_roots,
                        &body_writer_history,
                    ) {
                        if !dependencies.contains(writer) {
                            dependencies.push(writer.clone());
                        }
                    }
                }
            }
        }
        for operand in segment_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            if let Some(writer) =
                body_writer_history.native_writer(canonical_body(operand.operand_object_index))
            {
                if !dependencies.contains(writer) {
                    dependencies.push(writer.clone());
                }
            }
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let Some(data_block) = operand.operand_data_block.as_deref() else {
                continue;
            };
            let Some(writer) = body_writer_history.offset_store_writer(data_block) else {
                continue;
            };
            if !dependencies.contains(writer) {
                dependencies.push(writer.clone());
            }
        }
        for block_use in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let Some(dependency) = preceding_operation_dependency(
                block_use.construction_operation_label.as_str(),
                ordinal,
                &operation_positions,
                &feature_ids_by_operation,
            ) else {
                continue;
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        for block_use in datum_csys_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let Some(dependency) = preceding_operation_dependency(
                block_use.construction_operation_label.as_str(),
                ordinal,
                &operation_positions,
                &feature_ids_by_operation,
            ) else {
                continue;
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        for identity_use in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let other = if identity_use.datum_plane_operation_label == label.id {
                identity_use.datum_csys_operation_label.as_str()
            } else {
                identity_use.datum_plane_operation_label.as_str()
            };
            let Some(dependency) = preceding_operation_dependency(
                other,
                ordinal,
                &operation_positions,
                &feature_ids_by_operation,
            ) else {
                continue;
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            if let Some(feature) = feature_ids_by_operation
                .get(dependency.sketch_operation_label.as_str())
                .cloned()
            {
                if !dependencies.contains(&feature) {
                    dependencies.push(feature);
                }
            }
        }
        let mut source_properties = BTreeMap::new();
        source_properties.extend(operation_source_properties(
            &label.id,
            operation_records,
            operation_common_frames,
            operation_terminal_frames,
        ));
        for (use_ordinal, block_use) in datum_csys_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_csys_block_use.{use_ordinal}"),
                block_use.id.clone(),
            );
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            source_properties.insert(
                "sketch_point_dependency_use".to_string(),
                dependency.sketch_point_use.clone(),
            );
            match &dependency.block_relation {
                crate::native::features::FeatureSketchDatumCsysBlockRelation::Shared {
                    data_block,
                } => {
                    source_properties.insert(
                        "sketch_point_dependency_shared_block".to_string(),
                        data_block.clone(),
                    );
                }
                crate::native::features::FeatureSketchDatumCsysBlockRelation::Consecutive {
                    point_data_block,
                    construction_data_block,
                } => {
                    source_properties.insert(
                        "sketch_point_dependency_point_block".to_string(),
                        point_data_block.clone(),
                    );
                    source_properties.insert(
                        "sketch_point_dependency_construction_block".to_string(),
                        construction_data_block.clone(),
                    );
                }
            }
            source_properties.insert(
                "sketch_datum_csys_dependency".to_string(),
                dependency.id.clone(),
            );
            for (alias_ordinal, alias) in dependency.scalar_aliases.iter().enumerate() {
                source_properties.insert(
                    format!("sketch_point_dependency_scalar.{alias_ordinal}"),
                    alias.datum_csys_scalar.clone(),
                );
                source_properties.insert(
                    format!("sketch_point_dependency_coordinate.{alias_ordinal}"),
                    alias.sketch_coordinate_ordinal.to_string(),
                );
            }
        }
        let deletes_body = label.value == "DELETE";
        let mut outputs = if deletes_body {
            Vec::new()
        } else {
            body_references
                .get(label.id.as_str())
                .map_or_else(Vec::new, |body| {
                    feature_body_outputs(*body, body_bindings, &bodies_by_object_index)
                })
        };
        if outputs.is_empty() {
            outputs = hole_outputs
                .get(label.id.as_str())
                .or_else(|| hole_packages.outputs.get(label.id.as_str()))
                .cloned()
                .unwrap_or_default();
        }
        let native_primary_body = body_references
            .get(label.id.as_str())
            .copied()
            .map(canonical_body);
        let offset_store_primary_body = offset_store_bodies_by_operation
            .get(label.id.as_str())
            .and_then(|uses| match uses.as_slice() {
                [(_, data_block)] => Some(data_block.as_str()),
                _ => None,
            });
        if let Some(body) = body_references.get(label.id.as_str()) {
            source_properties.insert("primary_body_object_index".to_string(), body.to_string());
        }
        if let Some(reference) = body_writer_references_by_operation.get(label.id.as_str()) {
            source_properties.insert("primary_body_reference".to_string(), reference.id.clone());
            if let Some(uses) = body_segment_uses_by_reference.get(reference.id.as_str()) {
                if let [use_] = uses.as_slice() {
                    source_properties
                        .insert("primary_body_segment_use".to_string(), use_.id.clone());
                    source_properties.insert(
                        "primary_body_segment_binding".to_string(),
                        use_.segment_body_binding.clone(),
                    );
                }
            }
            if let Some(uses) = body_data_block_uses_by_reference.get(reference.id.as_str()) {
                if let [use_] = uses.as_slice() {
                    source_properties
                        .insert("primary_body_data_block_use".to_string(), use_.id.clone());
                    source_properties.insert(
                        "primary_body_data_block".to_string(),
                        use_.data_block.clone(),
                    );
                }
            }
        }
        for reference in body_reference_occurrences_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("body_reference.{}", reference.ordinal),
                reference.body_object_index.to_string(),
            );
            source_properties.insert(
                format!("body_reference_occurrence.{}", reference.ordinal),
                reference.id.clone(),
            );
        }
        if let Some(inputs) = sketch_construction_inputs_by_operation.get(label.id.as_str()) {
            source_properties.insert("sketch_construction_inputs".to_string(), inputs.id.clone());
        }
        for (ordinal, record) in sketch_records_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_record.{ordinal}"), record.id.clone());
        }
        for (ordinal, payload) in sketch_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("sketch_construction_payload.{ordinal}"),
                payload.id.clone(),
            );
        }
        for pair in sketch_coordinate_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_coordinate_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for pair in sketch_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_fixed_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for pair in sketch_mixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_mixed_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for (ordinal, point) in sketch_fixed_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_fixed_point.{ordinal}"), point.id.clone());
        }
        if let Some(construction) = block_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert("block_construction".to_string(), construction.id.clone());
        }
        for (ordinal, payload) in block_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("block_construction_payload.{ordinal}"),
                payload.id.clone(),
            );
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            source_properties.insert("block_dimensions".to_string(), dimensions.id.clone());
            for (dimension_ordinal, (declaration, expression)) in dimensions
                .declarations
                .iter()
                .zip(&dimensions.expressions)
                .enumerate()
            {
                source_properties.insert(
                    format!("block_dimension_declaration.{dimension_ordinal}"),
                    declaration.clone(),
                );
                source_properties.insert(
                    format!("block_dimension_expression.{dimension_ordinal}"),
                    expression.clone(),
                );
            }
        }
        for (ordinal, point) in block_payload_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("block_payload_point.{ordinal}"), point.id.clone());
        }
        for (ordinal, group) in block_payload_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("block_payload_point_group.{ordinal}"),
                group.id.clone(),
            );
        }
        if let Some(construction) = extrude_32_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "extrude_32_construction".to_string(),
                construction.id.clone(),
            );
        }
        if let Some(header) = extrude_payload_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("extrude_payload_header".to_string(), header.id.clone());
        }
        if let Some(lane) = operation_terminal_discriminators_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "operation_terminal_discriminator".to_string(),
                lane.id.clone(),
            );
        }
        for (ordinal, branch) in extrude_payload_32_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("extrude_payload_32_branch.{ordinal}"),
                branch.id.clone(),
            );
        }
        for triple in operation_body_scalar_triples_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_scalar_triple.{}",
                    triple.body_reference_ordinal
                ),
                triple.id.clone(),
            );
        }
        for member in operation_body_members_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_member.{}.{}",
                    member.body_reference_ordinal, member.ordinal
                ),
                member.id.clone(),
            );
        }
        for continuation in operation_body_11_continuations_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_11_continuation.{}",
                    continuation.body_reference_ordinal
                ),
                continuation.id.clone(),
            );
        }
        for lane in operation_body_reference_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "operation_body_reference_lane.{}",
                    lane.body_reference_ordinal
                ),
                lane.id.clone(),
            );
        }
        if let Some(construction) = datum_csys_constructions_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "datum_csys_construction".to_string(),
                construction.id.clone(),
            );
        }
        for (ordinal, use_) in datum_csys_column_row_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_csys_column_row_use.{ordinal}"),
                use_.id.clone(),
            );
        }
        for (ordinal, payload) in datum_csys_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("datum_csys_payload.{ordinal}"), payload.id.clone());
        }
        for (ordinal, pair) in datum_csys_payload_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_csys_payload_scalar_pair.{ordinal}"),
                pair.id.clone(),
            );
        }
        for (ordinal, pair) in datum_csys_payload_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_csys_payload_fixed_pair.{ordinal}"),
                pair.id.clone(),
            );
        }
        for (ordinal, scalar) in datum_csys_payload_scalars_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_csys_payload_scalar.{ordinal}"),
                scalar.id.clone(),
            );
        }
        for (ordinal, descriptor) in datum_csys_descriptors_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_csys_descriptor.{ordinal}"),
                descriptor.id.clone(),
            );
        }
        if let Some(header) = datum_plane_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("datum_plane_header".to_string(), header.id.clone());
        }
        if let Some(payload) = datum_plane_payloads_by_operation.get(label.id.as_str()) {
            source_properties.insert("datum_plane_payload".to_string(), payload.id.clone());
        }
        for (ordinal, pair) in datum_plane_payload_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_plane_payload_scalar_pair.{ordinal}"),
                pair.id.clone(),
            );
        }
        for (ordinal, descriptor) in datum_plane_descriptors_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_plane_descriptor.{ordinal}"),
                descriptor.id.clone(),
            );
        }
        for (ordinal, identity_use) in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_identity_use.{ordinal}"),
                identity_use.id.clone(),
            );
        }
        for (use_ordinal, block_use) in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("datum_plane_block_use.{use_ordinal}"),
                block_use.id.clone(),
            );
        }
        source_properties.extend(simple_hole_native_properties(
            &label.id,
            simple_hole_templates,
            simple_hole_repeated_scalar_lanes,
            simple_hole_repeated_scalar_lane_block_references,
            simple_hole_construction_groups,
        ));
        for lane in hole_package_construction_group_lanes
            .iter()
            .filter(|lane| lane.operation_label == label.id)
        {
            source_properties.insert(
                "hole_package_construction_group_lane".to_string(),
                lane.id.clone(),
            );
        }
        for group_use in hole_package_construction_group_uses {
            let group = simple_hole_construction_groups
                .iter()
                .find(|group| group.id == group_use.simple_hole_construction_group);
            if group_use.operation_label == label.id {
                source_properties.insert(
                    "hole_package_construction_group_use".to_string(),
                    group_use.id.clone(),
                );
                source_properties.insert(
                    "simple_hole_construction_group".to_string(),
                    group_use.simple_hole_construction_group.clone(),
                );
            } else if group.is_some_and(|group| {
                group
                    .operation_labels
                    .iter()
                    .any(|operation| operation == &label.id)
            }) {
                source_properties.insert(
                    "hole_package_construction_group_use".to_string(),
                    group_use.id.clone(),
                );
                source_properties.insert(
                    "hole_package_operation".to_string(),
                    group_use.operation_label.clone(),
                );
            }
        }
        for (slot, value) in label.object_indices.iter().enumerate() {
            source_properties.insert(
                format!("object_index.{slot}"),
                value.map_or_else(|| "null".to_string(), |value| value.to_string()),
            );
        }
        for input in input_blocks_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("input_block_record.{}", input.input_slot),
                input.id.clone(),
            );
            source_properties.insert(
                format!("input_block.{}", input.input_slot),
                input.data_block.clone(),
            );
            if let Some(group) = input_block_identity_group_by_input.get(input.id.as_str()) {
                source_properties.insert(
                    format!("input_block_identity_group.{}", input.input_slot),
                    (*group).to_string(),
                );
            }
        }
        for (ordinal, use_) in input_column_row_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("input_column_row_use.{ordinal}"), use_.id.clone());
        }
        for (ordinal, target) in input_column_targets_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("input_column_target.{ordinal}"), target.id.clone());
        }
        for reference in sketch_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("sketch_reference_record.{}", reference.ordinal),
                reference.id.clone(),
            );
            source_properties.insert(
                format!("sketch_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for reference in projected_curve_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("projected_curve_reference_record.{}", reference.ordinal),
                reference.id.clone(),
            );
            source_properties.insert(
                format!("projected_curve_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for payload in projected_curve_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "projected_curve_construction_payload".to_string(),
                payload.id.clone(),
            );
        }
        for value in projected_curve_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("projected_curve_construction_string.{}", value.ordinal),
                value.id.clone(),
            );
        }
        for graph in fset_reference_graphs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("fset_reference_graph".to_string(), graph.id.clone());
        }
        for payload in fset_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let group = match payload.group {
                crate::native::features::FeatureFsetReferenceGroup::First => "first",
                crate::native::features::FeatureFsetReferenceGroup::Second => "second",
            };
            source_properties.insert(
                format!("fset_construction_payload.{group}"),
                payload.id.clone(),
            );
        }
        for field in delete_reference_fields_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("delete_reference_field".to_string(), field.id.clone());
        }
        for payload in delete_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "delete_construction_payload".to_string(),
                payload.id.clone(),
            );
        }
        for reference in pattern_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("pattern_reference_record.{}", reference.ordinal),
                reference.id.clone(),
            );
            source_properties.insert(
                format!("pattern_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for payload in pattern_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "pattern_construction_payload".to_string(),
                payload.id.clone(),
            );
        }
        for value in pattern_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("pattern_construction_string.{}", value.ordinal),
                value.id.clone(),
            );
        }
        for lane in pattern_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("pattern_construction_fixed_lane.{}", lane.ordinal),
                lane.id.clone(),
            );
        }
        for lane in pattern_transform_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("pattern_transform_lane".to_string(), lane.id.clone());
        }
        for lane in multi_instance_output_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("multi_instance_output_lane".to_string(), lane.id.clone());
        }
        for lane in identical_instance_output_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "identical_instance_output_lane".to_string(),
                lane.id.clone(),
            );
        }
        if let Some(header) = point_construction_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("point_construction_header".to_string(), header.id.clone());
            source_properties.insert(
                "point_construction_reference".to_string(),
                header
                    .data_block
                    .clone()
                    .unwrap_or_else(|| header.object_index.to_string()),
            );
            source_properties.insert(
                "point_construction_mode".to_string(),
                format!("{:02x}", header.mode),
            );
        }
        if let Some(lane) = point_construction_scalar_lanes_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "point_construction_scalar_lane".to_string(),
                lane.id.clone(),
            );
        }
        for reference in draft_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_reference_record.{}", reference.ordinal),
                reference.id.clone(),
            );
            source_properties.insert(
                format!("draft_construction_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for lane in draft_construction_index_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("draft_construction_index_lane".to_string(), lane.id.clone());
        }
        for payload in draft_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert("draft_construction_payload".to_string(), payload.id.clone());
        }
        for payload in draft_construction_graph_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "draft_construction_graph_payload".to_string(),
                payload.id.clone(),
            );
        }
        for lane in draft_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_fixed_lane.{}", lane.ordinal),
                lane.id.clone(),
            );
        }
        for lane in draft_construction_binary32_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_binary32_lane.{}", lane.ordinal),
                lane.id.clone(),
            );
        }
        for value in draft_construction_graph_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_graph_string.{}", value.ordinal),
                value.id.clone(),
            );
        }
        for frame in draft_construction_identity_frames_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("draft_construction_identity_frame.{}", frame.ordinal),
                frame.id.clone(),
            );
        }
        for lane in draft_construction_terminal_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "draft_construction_terminal_lane".to_string(),
                lane.id.clone(),
            );
        }
        for reference in surface_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "surface_construction_reference_record.{}",
                    reference.ordinal
                ),
                reference.id.clone(),
            );
            source_properties.insert(
                format!("surface_construction_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        for payload in surface_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                "surface_construction_payload".to_string(),
                payload.id.clone(),
            );
        }
        for pair in surface_construction_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("surface_construction_scalar_pair.{}", pair.ordinal),
                pair.id.clone(),
            );
        }
        for value in surface_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("surface_construction_string.{}", value.ordinal),
                value.id.clone(),
            );
        }
        for branch in surface_construction_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            for member in &branch.members {
                source_properties.insert(
                    format!(
                        "surface_construction_branch.{}.member.{}",
                        branch.ordinal, member.ordinal
                    ),
                    member
                        .data_block
                        .clone()
                        .unwrap_or_else(|| member.object_index.to_string()),
                );
            }
            source_properties.insert(
                format!("surface_construction_branch.{}.terminal", branch.ordinal),
                branch
                    .terminal
                    .data_block
                    .clone()
                    .unwrap_or_else(|| branch.terminal.object_index.to_string()),
            );
        }
        for (ordinal, block_use) in sketch_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("sketch_named_point_block_use.{ordinal}"),
                block_use.id.clone(),
            );
        }
        for (ordinal, point_use) in sketch_preceding_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(
                format!("sketch_preceding_named_point_use.{ordinal}"),
                point_use.id.clone(),
            );
        }
        for (ordinal, point_use) in sketch_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_point_use.{ordinal}"), point_use.id.clone());
        }
        for (ordinal, group) in sketch_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("sketch_point_group.{ordinal}"), group.id.clone());
        }
        for reference in extrude_profile_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!("extrude_profile_reference_record.{}", reference.ordinal),
                reference.id.clone(),
            );
            source_properties.insert(
                format!("extrude_profile_reference.{}", reference.ordinal),
                reference
                    .data_block
                    .clone()
                    .unwrap_or_else(|| reference.object_index.to_string()),
            );
        }
        if let Some(profile) = extrude_construction_profiles_by_operation.get(label.id.as_str()) {
            source_properties.insert(
                "extrude_construction_profile".to_string(),
                profile.id.clone(),
            );
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                operand.source_property_key(),
                operand
                    .operand_data_block
                    .clone()
                    .unwrap_or_else(|| operand.operand_object_index.to_string()),
            );
            source_properties.insert(
                format!(
                    "operation_body_operand_record.{}.{}",
                    operand.body_reference_ordinal, operand.ordinal
                ),
                operand.id.clone(),
            );
            for (binding_ordinal, binding) in operand.segment_body_bindings.iter().enumerate() {
                source_properties.insert(
                    format!(
                        "operation_body_operand_segment_binding.{}.{}.{}",
                        operand.body_reference_ordinal, operand.ordinal, binding_ordinal
                    ),
                    binding.clone(),
                );
            }
        }
        for binding in parameter_bindings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            source_properties.insert(
                format!(
                    "input_parameter_declaration.{}.{}",
                    binding.input_slot, binding.reference_ordinal
                ),
                binding.expression_declaration.clone(),
            );
            if let Some(expression) = &binding.expression {
                source_properties.insert(
                    format!(
                        "input_parameter_expression.{}.{}",
                        binding.input_slot, binding.reference_ordinal
                    ),
                    expression.clone(),
                );
            }
        }
        for (ordinal, parameter_use) in parameter_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("parameter_use.{ordinal}"), parameter_use.id.clone());
        }
        let operation_payload_string_records = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let operation_payload_strings = operation_payload_string_records
            .iter()
            .map(|value| value.value.as_str())
            .collect::<Vec<_>>();
        let block_dimension_values = block_dimensions_by_operation
            .get(label.id.as_str())
            .map(|dimensions| dimensions.values);
        let block_projection = (label.value == "BLOCK")
            .then(|| block_placement(ir, block_dimension_values?, &outputs))
            .flatten();
        let block_outputs_are_proven = !outputs.is_empty() || block_projection.is_some();
        if outputs.is_empty() {
            if let Some((body, _)) = &block_projection {
                outputs.push(body.clone());
            }
        }
        let sphere_projection = (label.value == "SPHERE")
            .then(|| sphere_body_projection(ir, &outputs))
            .flatten();
        let inferred_sphere_outputs = outputs
            .is_empty()
            .then(|| {
                sphere_projection
                    .as_ref()
                    .map(|(body, _, _)| vec![body.clone()])
            })
            .flatten();
        let sphere_outputs = inferred_sphere_outputs
            .as_deref()
            .unwrap_or(outputs.as_slice());
        let body_reference_count = body_reference_occurrences_by_operation
            .get(label.id.as_str())
            .map_or(0, Vec::len);
        let block_op = new_body_boolean_op(&NewBodyEvidence {
            has_complete_projection: block_projection.is_some(),
            has_complete_primitive_construction: block_constructions_by_operation
                .get(label.id.as_str())
                .is_some_and(|construction| {
                    block_construction_payloads_by_operation
                        .get(label.id.as_str())
                        .is_some_and(|payloads| {
                            payloads.len() == 1 && payloads[0].construction == construction.id
                        })
                }),
            outputs: &outputs,
            outputs_are_proven: block_outputs_are_proven,
            body_reference_count,
            provisional_feature: initial_body_id.as_ref(),
            native_primary_body,
            offset_store_primary_body,
            history: &body_writer_history,
        });
        let sphere_op = sphere_projection
            .as_ref()
            .map_or(BooleanOp::Unresolved, |_| {
                new_body_boolean_op(&NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: true,
                    outputs: sphere_outputs,
                    outputs_are_proven: true,
                    body_reference_count,
                    provisional_feature: initial_body_id.as_ref(),
                    native_primary_body,
                    offset_store_primary_body,
                    history: &body_writer_history,
                })
            });
        if sphere_op == BooleanOp::NewBody {
            if let Some(inferred_outputs) = inferred_sphere_outputs {
                outputs.extend(inferred_outputs);
            }
        }
        if block_op == BooleanOp::NewBody || sphere_op == BooleanOp::NewBody {
            if let Some(initial_feature) = initial_body_id.as_ref().and_then(|id| {
                ir.model
                    .features
                    .iter_mut()
                    .find(|feature| feature.id == *id)
            }) {
                initial_feature
                    .outputs
                    .retain(|body| !outputs.contains(body));
                if let FeatureDefinition::BaseFeature {
                    bodies: BodySelection::Resolved { bodies, .. },
                } = &mut initial_feature.definition
                {
                    bodies.retain(|body| !outputs.contains(body));
                }
                body_writer_history.retract_outputs(&initial_feature.id, &outputs);
            }
        }
        body_writer_history.extend_primary_dependencies(
            initial_body_id.as_ref(),
            native_primary_body,
            offset_store_primary_body,
            &outputs,
            &mut dependencies,
        );
        let block_placement = block_projection.map(|(_, placement)| placement);
        let sphere_definition = sphere_projection.as_ref().and_then(|(_, center, radius)| {
            (sphere_op == BooleanOp::NewBody).then_some(FeatureDefinition::Sphere {
                center: *center,
                radius: *radius,
                op: sphere_op,
            })
        });
        let sew_projection = (label.value == "SEW")
            .then(|| {
                sew_body_feature_definition(
                    body_references.get(label.id.as_str()).copied(),
                    offset_store_bodies_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    operation_body_operands_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    &body_alias_roots,
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let trim_body_projection = (label.value == "TRIM BODY")
            .then(|| {
                body_references
                    .get(label.id.as_str())
                    .and_then(|primary| {
                        trim_body_feature_definition(
                            *primary,
                            operation_body_operands_by_operation
                                .get(label.id.as_str())
                                .map_or([].as_slice(), Vec::as_slice),
                            &body_alias_roots,
                            &bodies_by_object_index,
                        )
                    })
                    .or_else(|| {
                        offset_store_trim_body_feature_definition(
                            offset_store_bodies_by_operation
                                .get(label.id.as_str())
                                .map_or([].as_slice(), Vec::as_slice),
                            operation_body_operands_by_operation
                                .get(label.id.as_str())
                                .map_or([].as_slice(), Vec::as_slice),
                        )
                    })
            })
            .flatten();
        let offset_projection = (label.value == "OFFSET")
            .then(|| offset_surface_feature_definition(ir, &outputs))
            .flatten();
        if let Some((_, supports)) = &offset_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                source_properties.insert(
                    format!("offset_support_surface.{support_ordinal}"),
                    support.0.clone(),
                );
            }
        }
        let thicken_projection = (label.value == "THICKEN_SHEET")
            .then(|| thicken_feature_definition(ir, &outputs))
            .flatten();
        if let Some((_, supports)) = &thicken_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                source_properties.insert(
                    format!("thicken_support_surface.{support_ordinal}"),
                    support.0.clone(),
                );
            }
        }
        let blend_family = match label.value.as_str() {
            "BLEND" => Some(NxBlendFamily::Edge),
            "FACE_BLEND" => Some(NxBlendFamily::Face),
            _ => None,
        };
        let blend_projection =
            blend_family.and_then(|family| blend_feature_definition(ir, &outputs, family));
        if let Some((_, surfaces)) = &blend_projection {
            for (surface_ordinal, surface) in surfaces.iter().enumerate() {
                source_properties.insert(
                    format!("blend_result_surface.{surface_ordinal}"),
                    surface.0.clone(),
                );
            }
        }
        let extrude_projection = (label.value == "EXTRUDE").then(|| {
            let output_kinds = outputs
                .iter()
                .map(|output| {
                    ir.model
                        .bodies
                        .iter()
                        .find(|body| body.id == *output)
                        .map(|body| body.kind)
                })
                .collect::<Option<Vec<_>>>()
                .unwrap_or_default();
            let op = extrude_boolean_op(
                &body_writer_history,
                native_primary_body,
                offset_store_primary_body,
                &output_kinds,
            );
            extrude_feature_definition(
                extrude_construction_profiles_by_operation
                    .get(label.id.as_str())
                    .map(|profile| profile.id.as_str()),
                extrude_32_constructions_by_operation
                    .get(label.id.as_str())
                    .map(|construction| construction.id.as_str()),
                op,
                &output_kinds,
            )
        });
        let delete_projection = deletes_body
            .then(|| {
                delete_body_feature_definition(
                    body_references.get(label.id.as_str()).copied(),
                    &body_alias_roots,
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let operation_parameter_uses = parameter_uses_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let native_parameters = native_feature_parameters(operation_parameter_uses, expressions);
        let sketch = (label.value == "SKETCH")
            .then(|| {
                attach_sketch_points(
                    ir,
                    label,
                    &SketchPointSources {
                        point_uses: sketch_point_uses_by_operation
                            .get(label.id.as_str())
                            .map_or([].as_slice(), Vec::as_slice),
                        point_groups: sketch_point_groups,
                        points: sketch_points,
                        payload_scalars: sketch_payload_scalars,
                    },
                    annotations,
                    stream,
                )
            })
            .flatten();
        let definition = boolean_definition.unwrap_or_else(|| {
            trim_body_projection
                .or(delete_projection)
                .or(sew_projection)
                .or(extrude_projection)
                .or_else(|| blend_projection.map(|(definition, _)| definition))
                .or_else(|| thicken_projection.map(|(definition, _)| definition))
                .or_else(|| offset_projection.map(|(definition, _)| definition))
                .or(sphere_definition)
                .unwrap_or_else(|| {
                    if let Some(sketch) = sketch {
                        return FeatureDefinition::Sketch {
                            space: SketchSpace::Planar,
                            sketch: Some(sketch),
                        };
                    }
                    let mut definition = non_modeling_history_definition(
                        &label.value,
                        &label.object_indices,
                        &outputs,
                        body_reference_occurrences_by_operation
                            .get(label.id.as_str())
                            .map_or(0, Vec::len),
                        operation_body_operands_by_operation
                            .get(label.id.as_str())
                            .map_or(0, Vec::len),
                        operation_payload_string_records.len(),
                        &source_properties,
                    )
                    .unwrap_or_else(|| {
                        non_boolean_feature_definition_with_parameters(
                            &label.value,
                            &operation_payload_strings,
                            block_dimension_values,
                            block_placement,
                            HoleProjection {
                                placements: simple_hole_placements
                                    .get(label.id.as_str())
                                    .cloned()
                                    .into_iter()
                                    .chain(
                                        counterbore_hole_placements.get(label.id.as_str()).cloned(),
                                    )
                                    .chain(blind_hole_placements.get(label.id.as_str()).cloned())
                                    .chain(
                                        hole_packages
                                            .placements
                                            .get(label.id.as_str())
                                            .cloned()
                                            .unwrap_or_default(),
                                    )
                                    .collect(),
                                diameter: simple_hole_diameters
                                    .get(label.id.as_str())
                                    .or_else(|| hole_packages.diameters.get(label.id.as_str()))
                                    .copied(),
                                extent: blind_hole_depths
                                    .get(label.id.as_str())
                                    .copied()
                                    .map(|length| Termination::Blind { length }),
                                counterbore: counterbore_dimensions.get(label.id.as_str()).copied(),
                                chamfer: simple_hole_chamfers
                                    .get(label.id.as_str())
                                    .or_else(|| hole_packages.chamfers.get(label.id.as_str()))
                                    .copied(),
                                grouped_simple_through: hole_packages
                                    .outputs
                                    .contains_key(label.id.as_str()),
                            },
                            native_parameters,
                        )
                    });
                    if let FeatureDefinition::Block { op, .. } = &mut definition {
                        *op = block_op;
                    }
                    definition
                })
        });
        annotations
            .note(&id, stream, label.source_offset)
            .tag("FEATURE_OPERATION");
        annotations.exactness(&id, Exactness::Derived);
        let source_content = feature_source_content(operation_payload_string_records);
        let mut referenced_parameters = operation_parameter_uses
            .iter()
            .filter_map(|parameter_use| expression_parameter_id(&parameter_use.expression))
            .collect::<Vec<_>>();
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            referenced_parameters.extend(
                dimensions
                    .expressions
                    .iter()
                    .filter_map(|expression| expression_parameter_id(expression)),
            );
        }
        for owner in parameter_owner_dependencies(&parameter_owners, &referenced_parameters) {
            if !dependencies.contains(&owner) {
                dependencies.push(owner);
            }
        }
        if !source_content.is_empty() {
            annotations.derived(&id, "source_content");
        }
        let native_output = (!deletes_body).then_some(native_primary_body).flatten();
        let offset_store_output = (!deletes_body)
            .then_some(offset_store_primary_body)
            .flatten();
        body_writer_history.record_writer(native_output, offset_store_output, &outputs, &id);
        if let Some(operation) = (!deletes_body)
            .then(|| booleans.get(label.id.as_str()))
            .flatten()
        {
            // A Boolean target writes its selected body image even when the
            // operation has no separate primary-body field.
            if !matches!(
                boolean_offset_store_resolution.as_ref(),
                Some(BooleanOffsetStoreResolution::Unresolved)
            ) {
                let (native_target, offset_store_target) = boolean_target_writer(
                    &definition,
                    canonical_body(operation.target_object_index),
                );
                body_writer_history.record_writer(native_target, offset_store_target, &[], &id);
            }
        }
        ir.model.features.push(Feature {
            id: id.clone(),
            ordinal: base_ordinal + ordinal as u64,
            name: Some(label.value.clone()),
            suppressed: None,
            parent: None,
            dependencies,
            source_properties,
            source_tag: Some(label.value.clone()),
            source_text: None,
            source_content,
            outputs,
            definition,
            native_ref: Some(label.id.clone()),
        });
        if !deletes_body {
            let result_body = native_result_body_identity(
                body_writer_references_by_operation
                    .get(label.id.as_str())
                    .copied(),
                booleans.get(label.id.as_str()).copied(),
            );
            if let Some((local_id, native_ref)) = result_body {
                let key = label
                    .id
                    .strip_prefix("nx:feature-history:operation-label#")
                    .unwrap_or(label.id.as_str());
                ir.model
                    .feature_result_topologies
                    .push(FeatureResultTopology {
                        id: FeatureResultTopologyId(format!(
                            "nx:feature-history:result-topology#{key}"
                        )),
                        output_of: id,
                        bodies: vec![local_id],
                        faces: Vec::new(),
                        edges: Vec::new(),
                        vertices: Vec::new(),
                        native_ref: Some(native_ref),
                    });
            }
        }
    }
    if let Some(initial_body_id) = initial_body_id {
        let has_outputs = ir
            .model
            .features
            .iter()
            .find(|feature| feature.id == initial_body_id)
            .is_some_and(|feature| !feature.outputs.is_empty());
        if has_outputs {
            annotations.derived(&initial_body_id, "outputs");
        }
    }
}

/// Select the exact native writer identity for one intermediate body result.
/// A primary-body writer is canonical when both native forms are present. The
/// Boolean target is independently sufficient when the primary form is absent.
fn native_result_body_identity(
    primary: Option<&crate::native::features::FeatureBodyReference>,
    boolean: Option<&crate::native::features::FeatureBooleanOperation>,
) -> Option<(String, String)> {
    primary
        .map(|writer| (writer.id.clone(), writer.id.clone()))
        .or_else(|| {
            boolean.map(|operation| (format!("{}:target", operation.id), operation.id.clone()))
        })
}

/// Return primary body fields that are proven to use the segment-object
/// namespace. An offset-store field may enter that namespace only when the
/// feature extractor has also retained one unique segment alias use for the
/// same field. Missing or ambiguous relations remain offset-store-local. An
/// operation with zero or multiple body fields has no primary-body writer.
fn native_primary_body_references<'a>(
    references: &'a [crate::native::features::FeatureBodyReference],
    data_block_uses: &[crate::native::features::FeatureBodyDataBlockUse],
    segment_uses: &[crate::native::features::FeatureBodySegmentUse],
    inputs: &[crate::native::features::FeatureInputBlock],
    data_blocks: &[crate::native::om::DataBlock],
) -> BTreeMap<&'a str, u32> {
    let unique_references = crate::native::features::unique_feature_body_references(references);
    let offset_store_references = data_block_uses
        .iter()
        .map(|use_| use_.feature_body_reference.as_str())
        .collect::<BTreeSet<_>>();
    let offset_store_operations =
        crate::native::features::feature_input_store_operations(inputs, data_blocks);
    let bridged_segment_references = segment_uses
        .iter()
        .map(|use_| use_.feature_body_reference.as_str())
        .collect::<BTreeSet<_>>();
    unique_references
        .into_iter()
        .filter(|(_, reference)| {
            bridged_segment_references.contains(reference.id.as_str())
                || (!offset_store_references.contains(reference.id.as_str())
                    && !offset_store_operations.contains(reference.operation_label.as_str()))
        })
        .map(|(operation, reference)| (operation, reference.body_object_index))
        .collect()
}

fn attach_sketch_points(
    ir: &mut CadIr,
    label: &crate::native::features::FeatureOperationLabel,
    sources: &SketchPointSources<'_>,
    annotations: &mut AnnotationBuilder,
    stream: cadmpeg_ir::annotations::StreamHandle,
) -> Option<SketchId> {
    let operation_groups = sources
        .point_groups
        .iter()
        .filter(|group| group.operation_label == label.id)
        .collect::<Vec<_>>();
    if operation_groups.is_empty() {
        return None;
    }
    let mut groups_by_id =
        BTreeMap::<&str, &crate::native::features::FeatureSketchPointGroup>::new();
    for group in &operation_groups {
        if groups_by_id.insert(group.id.as_str(), group).is_some() {
            return None;
        }
    }
    let mut point_uses_by_group =
        BTreeMap::<&str, &crate::native::features::FeatureSketchPointUse>::new();
    for point_use in sources.point_uses {
        if point_use.operation_label != label.id
            || point_uses_by_group
                .insert(point_use.sketch_point_group.as_str(), point_use)
                .is_some()
        {
            return None;
        }
    }
    if point_uses_by_group
        .keys()
        .any(|group| !groups_by_id.contains_key(group))
    {
        return None;
    }
    let mut points_by_id = BTreeMap::<&str, &crate::native::features::FeatureSketchPoint>::new();
    for point in sources.points {
        if points_by_id.insert(point.id.as_str(), point).is_some() {
            return None;
        }
    }
    let mut scalars_by_id =
        BTreeMap::<&str, &crate::native::features::FeatureSketchPayloadScalar>::new();
    for scalar in sources.payload_scalars {
        if scalars_by_id.insert(scalar.id.as_str(), scalar).is_some() {
            return None;
        }
    }
    let operation_key = label
        .id
        .strip_prefix("nx:feature-history:operation-label#")
        .unwrap_or(label.id.as_str());
    let sketch_id = SketchId(format!("nx:feature-history:sketch#{operation_key}"));
    let mut entities = Vec::new();
    let mut represented_groups = BTreeSet::new();
    for group in operation_groups {
        if !represented_groups.insert(group.id.as_str())
            || group
                .coordinates
                .iter()
                .any(|coordinate| !coordinate.is_finite())
        {
            return None;
        }
        let point_use = point_uses_by_group.get(group.id.as_str()).copied();
        let source_offsets = if let Some(point_use) = point_use {
            point_use.source_offsets.clone()
        } else {
            group
                .points
                .iter()
                .map(|point_id| {
                    let point = points_by_id.get(point_id.as_str()).copied()?;
                    if point.operation_label != label.id
                        || point.name != group.name
                        || point
                            .coordinates
                            .iter()
                            .zip(group.coordinates)
                            .any(|(first, second)| first.to_bits() != second.to_bits())
                    {
                        return None;
                    }
                    let scalar_fields = point
                        .scalar_fields
                        .iter()
                        .map(|scalar_id| scalars_by_id.get(scalar_id.as_str()).copied())
                        .collect::<Option<Vec<_>>>()?;
                    if scalar_fields.len() != 2
                        || scalar_fields.iter().zip(group.coordinates).any(
                            |(scalar, coordinate)| {
                                scalar.operation_label != label.id
                                    || scalar.value.to_bits() != coordinate.to_bits()
                            },
                        )
                    {
                        return None;
                    }
                    Some(
                        scalar_fields
                            .into_iter()
                            .map(|scalar| scalar.source_offset)
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect()
        };
        let source_offset = source_offsets.iter().copied().min()?;
        let native_ref =
            point_use.map_or_else(|| group.id.clone(), |point_use| point_use.id.clone());
        let entity_key = point_use
            .map_or(group.id.as_str(), |point_use| point_use.id.as_str())
            .strip_prefix("nx:feature-history:sketch-point-use#")
            .or_else(|| {
                group
                    .id
                    .strip_prefix("nx:feature-history:sketch-point-group#")
            })
            .unwrap_or(group.id.as_str());
        entities.push((
            source_offset,
            SketchEntity {
                id: SketchEntityId(format!(
                    "nx:feature-history:sketch-entity#point-{entity_key}"
                )),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: Some(native_ref),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(group.coordinates[0], group.coordinates[1]),
                },
            },
        ));
    }
    entities.sort_by(|(first_offset, first), (second_offset, second)| {
        first_offset
            .cmp(second_offset)
            .then_with(|| first.id.0.cmp(&second.id.0))
    });
    if entities.is_empty() {
        return None;
    }
    for (source_offset, entity) in &entities {
        annotations
            .note(&entity.id.0, stream, *source_offset)
            .tag("SKETCH_POINT");
        annotations.exactness(&entity.id.0, Exactness::Derived);
    }
    annotations
        .note(&sketch_id.0, stream, label.source_offset)
        .tag("SKETCH");
    annotations.exactness(&sketch_id.0, Exactness::Derived);
    ir.model
        .sketch_entities
        .extend(entities.into_iter().map(|(_, entity)| entity));
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some(label.value.clone()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved,
        profiles: Vec::new(),
        native_ref: Some(label.id.clone()),
    });
    Some(sketch_id)
}

struct SketchPointSources<'a> {
    point_uses: &'a [&'a crate::native::features::FeatureSketchPointUse],
    point_groups: &'a [crate::native::features::FeatureSketchPointGroup],
    points: &'a [crate::native::features::FeatureSketchPoint],
    payload_scalars: &'a [crate::native::features::FeatureSketchPayloadScalar],
}

fn records_by_operation<'a, T>(
    records: &'a [T],
    operation_label: impl Fn(&'a T) -> &'a str,
) -> BTreeMap<&'a str, Vec<&'a T>> {
    let mut grouped = BTreeMap::new();
    for record in records {
        grouped
            .entry(operation_label(record))
            .or_insert_with(Vec::new)
            .push(record);
    }
    grouped
}

fn operation_source_properties(
    operation_label: &str,
    records: &[crate::native::features::FeatureOperationRecord],
    common_frames: &[crate::native::features::FeatureOperationCommonFrame],
    terminal_frames: &[crate::native::features::FeatureOperationTerminalFrame],
) -> BTreeMap<String, String> {
    let mut matching_records = records
        .iter()
        .filter(|record| record.operation_label == operation_label);
    let Some(record) = matching_records.next() else {
        return BTreeMap::new();
    };
    if matching_records.next().is_some() {
        return BTreeMap::new();
    }

    let mut properties = BTreeMap::from([("operation_record".to_string(), record.id.clone())]);
    let matching_common_frames = common_frames
        .iter()
        .filter(|frame| frame.operation_record == record.id)
        .collect::<Vec<_>>();
    if matching_common_frames
        .iter()
        .enumerate()
        .all(|(ordinal, frame)| frame.ordinal == ordinal as u32)
    {
        for frame in matching_common_frames {
            properties.insert(
                format!("operation_common_frame.{}", frame.ordinal),
                frame.id.clone(),
            );
        }
    }
    let mut matching_frames = terminal_frames
        .iter()
        .filter(|frame| frame.operation_record == record.id);
    let Some(frame) = matching_frames.next() else {
        return properties;
    };
    if matching_frames.next().is_some() {
        return properties;
    }
    properties.insert("operation_terminal_frame".to_string(), frame.id.clone());
    properties
}

struct ParasolidStringAttributeSources<'a> {
    topology_references: &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
    field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
    field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    string_uses: &'a [crate::native::parasolid::ParasolidEntity51StringUse],
    strings: &'a [crate::native::parasolid::ParasolidEntity54StringRecord],
}

fn attach_parasolid_topology_string_attributes(
    ir: &mut CadIr,
    sources: &ParasolidStringAttributeSources<'_>,
    annotations: &mut AnnotationBuilder,
) {
    let class_names =
        parasolid_topology_attribute_class_names(sources.class_uses, sources.definitions);
    let strings_by_id = sources
        .strings
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut uses_by_entity =
        BTreeMap::<&str, Vec<&crate::native::parasolid::ParasolidEntity51StringUse>>::new();
    for string_use in sources.string_uses {
        uses_by_entity
            .entry(string_use.entity_51_record.as_str())
            .or_default()
            .push(string_use);
    }
    for uses in uses_by_entity.values_mut() {
        uses.sort_by_key(|string_use| string_use.reference_ordinal);
    }
    for context in parasolid_topology_attribute_contexts(ir, sources.topology_references) {
        let reference = context.reference;
        let entity = context.entity;
        for string_use in uses_by_entity.get(entity).into_iter().flatten() {
            let Some(string) = strings_by_id.get(string_use.string_record.as_str()) else {
                continue;
            };
            let id = AttributeId(format!(
                "nx:s{}:topology-string-attribute#{}-{}-{}",
                reference.stream_ordinal,
                reference.topology_type,
                reference.topology_xmt,
                string_use.reference_ordinal
            ));
            let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
            annotations
                .note(&id.0, source_stream, string.inflated_offset)
                .tag("ENTITY_54_STRING_ATTRIBUTE");
            annotations.derived(&id.0, "target");
            annotations.derived(&id.0, "name");
            let generic_name = format!(
                "parasolid_type_84_reference_{}",
                string_use.reference_ordinal
            );
            let name = parasolid_topology_attribute_field_name(
                reference,
                string_use.id.as_str(),
                sources.class_uses,
                sources.definitions,
                sources.field_uses,
                sources.field_names,
            );
            ir.model.attributes.push(SourceAttribute {
                id,
                target: context.target.clone(),
                name: name
                    .or_else(|| {
                        class_names
                            .get(reference.id.as_str())
                            .map(|class_name| format!("{class_name}.{generic_name}"))
                    })
                    .unwrap_or(generic_name),
                values: vec![AttributeValue::String(string.value.clone())],
            });
        }
    }
    ir.model
        .attributes
        .sort_by(|first, second| first.id.0.cmp(&second.id.0));
}

struct ParasolidNumericAttributeSources<'a> {
    pub(crate) topology_references:
        &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
    pub(crate) class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    pub(crate) definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
    pub(crate) field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
    pub(crate) field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    pub(crate) numeric_uses: &'a [crate::native::parasolid::ParasolidEntity51NumericUse],
    pub(crate) integers: &'a [crate::native::parasolid::ParasolidEntity52IntegerRecord],
    pub(crate) doubles: &'a [crate::native::parasolid::ParasolidEntity53DoubleRecord],
}

fn parasolid_topology_attribute_field_name(
    topology_reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
    value_use: &str,
    class_uses: &[crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &[crate::native::parasolid::ParasolidAttributeDefinition],
    field_uses: &[crate::native::parasolid::ParasolidAttributeFieldUse],
    field_names: &[crate::native::parasolid::ParasolidAttributeFieldNames],
) -> Option<String> {
    let mut classes = class_uses
        .iter()
        .filter(|class_use| class_use.topology_attribute_reference == topology_reference.id);
    let class_use = classes.next()?;
    if classes.next().is_some() {
        return None;
    }
    let mut matching_fields = field_uses.iter().filter(|field_use| {
        field_use.value_use == value_use
            && field_use.entity_51_record == class_use.entity_51_record
            && field_use.attribute_class_use == class_use.attribute_class_use
            && field_use.attribute_definition == class_use.attribute_definition
    });
    let field_use = matching_fields.next()?;
    if matching_fields.next().is_some() {
        return None;
    }
    let mut matching_definitions = definitions
        .iter()
        .filter(|definition| definition.id == class_use.attribute_definition);
    let definition = matching_definitions.next()?;
    if matching_definitions.next().is_some() {
        return None;
    }
    let declared_name = field_names
        .iter()
        .filter(|names| names.attribute_definition == definition.id)
        .collect::<Vec<_>>();
    let field_name = match (definition.name.as_str(), field_use.field_ordinal) {
        ("SDL/TYSA_DENSITY", 0) => "density".to_string(),
        ("SDL/TYSA_DENSITY", 1) => "units".to_string(),
        _ if matches!(declared_name.as_slice(), [_]) => declared_name[0]
            .names
            .get(field_use.field_ordinal as usize)?
            .clone(),
        _ => format!(
            "field_{}.parasolid_type_{}",
            field_use.field_ordinal, field_use.field_code
        ),
    };
    Some(format!("{}.{}", definition.name, field_name))
}

fn parasolid_topology_attribute_class_names<'a>(
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
) -> BTreeMap<&'a str, &'a str> {
    let mut definitions_by_id = BTreeMap::<&str, Vec<&str>>::new();
    for definition in definitions {
        definitions_by_id
            .entry(definition.id.as_str())
            .or_default()
            .push(definition.name.as_str());
    }
    let mut classes_by_reference = BTreeMap::<&str, Vec<&str>>::new();
    for class_use in class_uses {
        let Some([class_name]) = definitions_by_id
            .get(class_use.attribute_definition.as_str())
            .map(Vec::as_slice)
        else {
            continue;
        };
        classes_by_reference
            .entry(class_use.topology_attribute_reference.as_str())
            .or_default()
            .push(class_name);
    }
    classes_by_reference
        .into_iter()
        .filter_map(|(reference, names)| {
            let [name] = names.as_slice() else {
                return None;
            };
            Some((reference, *name))
        })
        .collect()
}

fn parasolid_topology_kind(topology_type: u8) -> Option<&'static str> {
    match topology_type {
        13 => Some("shell"),
        14 => Some("face"),
        15 => Some("loop"),
        16 => Some("edge"),
        17 => Some("fin"),
        18 => Some("vertex"),
        _ => None,
    }
}

fn parasolid_topology_attribute_targets(ir: &CadIr) -> BTreeMap<String, AttributeTarget> {
    ir.model
        .shells
        .iter()
        .map(|shell| (shell.id.0.clone(), AttributeTarget::Shell(shell.id.clone())))
        .chain(
            ir.model
                .faces
                .iter()
                .map(|face| (face.id.0.clone(), AttributeTarget::Face(face.id.clone()))),
        )
        .chain(
            ir.model
                .loops
                .iter()
                .map(|loop_| (loop_.id.0.clone(), AttributeTarget::Loop(loop_.id.clone()))),
        )
        .chain(
            ir.model
                .edges
                .iter()
                .map(|edge| (edge.id.0.clone(), AttributeTarget::Edge(edge.id.clone()))),
        )
        .chain(ir.model.coedges.iter().map(|coedge| {
            (
                coedge.id.0.clone(),
                AttributeTarget::Coedge(coedge.id.clone()),
            )
        }))
        .chain(ir.model.vertices.iter().map(|vertex| {
            (
                vertex.id.0.clone(),
                AttributeTarget::Vertex(vertex.id.clone()),
            )
        }))
        .collect()
}

struct ParasolidTopologyAttributeContext<'a> {
    reference: &'a crate::native::parasolid::ParasolidTopologyAttributeListReference,
    entity: &'a str,
    target: AttributeTarget,
}

fn parasolid_topology_attribute_contexts<'a>(
    ir: &CadIr,
    topology_references: &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
) -> Vec<ParasolidTopologyAttributeContext<'a>> {
    let mut references_by_target = BTreeMap::<String, Vec<_>>::new();
    for reference in topology_references {
        let Some(kind) = parasolid_topology_kind(reference.topology_type) else {
            continue;
        };
        references_by_target
            .entry(format!(
                "nx:s{}:{kind}#{}",
                reference.stream_ordinal, reference.topology_xmt
            ))
            .or_default()
            .push(reference);
    }
    let emitted_targets = parasolid_topology_attribute_targets(ir);
    references_by_target
        .into_iter()
        .filter_map(|(target_key, references)| {
            let [reference] = references.as_slice() else {
                return None;
            };
            Some(ParasolidTopologyAttributeContext {
                reference,
                entity: reference.attribute_list_record.as_deref()?,
                target: emitted_targets.get(target_key.as_str())?.clone(),
            })
        })
        .collect()
}

fn attach_parasolid_topology_numeric_attributes(
    ir: &mut CadIr,
    sources: &ParasolidNumericAttributeSources<'_>,
    annotations: &mut AnnotationBuilder,
) {
    let class_names =
        parasolid_topology_attribute_class_names(sources.class_uses, sources.definitions);
    let integers_by_id = sources
        .integers
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let doubles_by_id = sources
        .doubles
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut uses_by_entity =
        BTreeMap::<&str, Vec<&crate::native::parasolid::ParasolidEntity51NumericUse>>::new();
    for numeric_use in sources.numeric_uses {
        uses_by_entity
            .entry(numeric_use.entity_51_record.as_str())
            .or_default()
            .push(numeric_use);
    }
    for uses in uses_by_entity.values_mut() {
        uses.sort_by_key(|numeric_use| numeric_use.reference_ordinal);
    }
    for context in parasolid_topology_attribute_contexts(ir, sources.topology_references) {
        let reference = context.reference;
        let entity = context.entity;
        for numeric_use in uses_by_entity.get(entity).into_iter().flatten() {
            let (values, source_offset, tag, lane) = match numeric_use.kind {
                crate::native::parasolid::ParasolidEntity51NumericKind::UnsignedIntegers => {
                    let Some(record) = integers_by_id.get(numeric_use.value_record.as_str()) else {
                        continue;
                    };
                    (
                        record
                            .values
                            .iter()
                            .map(|value| AttributeValue::Integer(i64::from(*value)))
                            .collect(),
                        record.inflated_offset,
                        "ENTITY_52_INTEGER_ATTRIBUTE",
                        "integer",
                    )
                }
                crate::native::parasolid::ParasolidEntity51NumericKind::Doubles => {
                    let Some(record) = doubles_by_id.get(numeric_use.value_record.as_str()) else {
                        continue;
                    };
                    (
                        record
                            .values
                            .iter()
                            .copied()
                            .map(AttributeValue::Float)
                            .collect(),
                        record.inflated_offset,
                        "ENTITY_53_DOUBLE_ATTRIBUTE",
                        "double",
                    )
                }
            };
            let id = AttributeId(format!(
                "nx:s{}:topology-numeric-attribute#{}-{}-{}",
                reference.stream_ordinal,
                reference.topology_type,
                reference.topology_xmt,
                numeric_use.reference_ordinal
            ));
            let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
            annotations
                .note(&id.0, source_stream, source_offset)
                .tag(tag);
            annotations.derived(&id.0, "target");
            annotations.derived(&id.0, "name");
            let generic_name = format!(
                "parasolid_type_{lane}_reference_{}",
                numeric_use.reference_ordinal
            );
            let name = parasolid_topology_attribute_field_name(
                reference,
                numeric_use.id.as_str(),
                sources.class_uses,
                sources.definitions,
                sources.field_uses,
                sources.field_names,
            );
            ir.model.attributes.push(SourceAttribute {
                id,
                target: context.target.clone(),
                name: name
                    .or_else(|| {
                        class_names
                            .get(reference.id.as_str())
                            .map(|class_name| format!("{class_name}.{generic_name}"))
                    })
                    .unwrap_or(generic_name),
                values,
            });
        }
    }
    ir.model
        .attributes
        .sort_by(|first, second| first.id.0.cmp(&second.id.0));
}

struct ParasolidStructuredAttributeSources<'a> {
    topology_references: &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
    field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
    field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    structured_uses: &'a [crate::native::parasolid::ParasolidEntity51StructuredUse],
    vectors: &'a [crate::native::parasolid::ParasolidEntityVectorRecord],
    axes: &'a [crate::native::parasolid::ParasolidEntity57AxisRecord],
    tags: &'a [crate::native::parasolid::ParasolidEntity58TagRecord],
    unicode: &'a [crate::native::parasolid::ParasolidEntity62UnicodeRecord],
}

fn attach_parasolid_topology_structured_attributes(
    ir: &mut CadIr,
    sources: &ParasolidStructuredAttributeSources<'_>,
    annotations: &mut AnnotationBuilder,
) {
    let class_names =
        parasolid_topology_attribute_class_names(sources.class_uses, sources.definitions);
    let vectors_by_id = sources
        .vectors
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let axes_by_id = sources
        .axes
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let tags_by_id = sources
        .tags
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let unicode_by_id = sources
        .unicode
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let mut uses_by_entity =
        BTreeMap::<&str, Vec<&crate::native::parasolid::ParasolidEntity51StructuredUse>>::new();
    for structured_use in sources.structured_uses {
        uses_by_entity
            .entry(structured_use.entity_51_record.as_str())
            .or_default()
            .push(structured_use);
    }
    for uses in uses_by_entity.values_mut() {
        uses.sort_by_key(|structured_use| structured_use.reference_ordinal);
    }
    for context in parasolid_topology_attribute_contexts(ir, sources.topology_references) {
        let reference = context.reference;
        let entity = context.entity;
        for structured_use in uses_by_entity.get(entity).into_iter().flatten() {
            use crate::native::parasolid::ParasolidAttributeFieldValueKind as Kind;
            use crate::native::parasolid::ParasolidVectorValueKind;
            let (values, source_offset, tag, family) = match structured_use.kind {
                Kind::Points | Kind::Vectors | Kind::Directions => {
                    let Some(record) = vectors_by_id.get(structured_use.value_record.as_str())
                    else {
                        continue;
                    };
                    let family = match (structured_use.kind, record.kind) {
                        (Kind::Points, ParasolidVectorValueKind::Points) => "85_point",
                        (Kind::Vectors, ParasolidVectorValueKind::Vectors) => "86_vector",
                        (Kind::Directions, ParasolidVectorValueKind::Directions) => "89_direction",
                        _ => continue,
                    };
                    (
                        record
                            .values
                            .iter()
                            .map(|value| AttributeValue::Vector(value.to_vec()))
                            .collect(),
                        record.inflated_offset,
                        "PARASOLID_VECTOR_ATTRIBUTE",
                        family,
                    )
                }
                Kind::Axes => {
                    let Some(record) = axes_by_id.get(structured_use.value_record.as_str()) else {
                        continue;
                    };
                    (
                        record
                            .values
                            .iter()
                            .map(|axis| {
                                AttributeValue::Vector(
                                    axis.iter()
                                        .flat_map(|vector| vector.iter().copied())
                                        .collect(),
                                )
                            })
                            .collect(),
                        record.inflated_offset,
                        "ENTITY_57_AXIS_ATTRIBUTE",
                        "87_axis",
                    )
                }
                Kind::Tags => {
                    let Some(record) = tags_by_id.get(structured_use.value_record.as_str()) else {
                        continue;
                    };
                    (
                        record
                            .values
                            .iter()
                            .map(|value| AttributeValue::Integer(i64::from(*value)))
                            .collect(),
                        record.inflated_offset,
                        "ENTITY_58_TAG_ATTRIBUTE",
                        "88_tag",
                    )
                }
                Kind::Unicode => {
                    let Some(record) = unicode_by_id.get(structured_use.value_record.as_str())
                    else {
                        continue;
                    };
                    (
                        vec![AttributeValue::String(record.value.clone())],
                        record.inflated_offset,
                        "ENTITY_62_UNICODE_ATTRIBUTE",
                        "98_unicode",
                    )
                }
                Kind::UnsignedIntegers | Kind::Doubles | Kind::String => continue,
            };
            let id = AttributeId(format!(
                "nx:s{}:topology-structured-attribute#{}-{}-{}",
                reference.stream_ordinal,
                reference.topology_type,
                reference.topology_xmt,
                structured_use.reference_ordinal
            ));
            let source_stream = annotations.stream(format!("nx:s{}", reference.stream_ordinal));
            annotations
                .note(&id.0, source_stream, source_offset)
                .tag(tag);
            annotations.derived(&id.0, "target");
            annotations.derived(&id.0, "name");
            let generic_name = format!(
                "parasolid_type_{family}_reference_{}",
                structured_use.reference_ordinal
            );
            let name = parasolid_topology_attribute_field_name(
                reference,
                structured_use.id.as_str(),
                sources.class_uses,
                sources.definitions,
                sources.field_uses,
                sources.field_names,
            );
            ir.model.attributes.push(SourceAttribute {
                id,
                target: context.target.clone(),
                name: name
                    .or_else(|| {
                        class_names
                            .get(reference.id.as_str())
                            .map(|class_name| format!("{class_name}.{generic_name}"))
                    })
                    .unwrap_or(generic_name),
                values,
            });
        }
    }
    ir.model
        .attributes
        .sort_by(|first, second| first.id.0.cmp(&second.id.0));
}

fn preceding_operation_dependency(
    operation: &str,
    consumer_position: usize,
    operation_positions: &BTreeMap<&str, usize>,
    feature_ids: &BTreeMap<&str, FeatureId>,
) -> Option<FeatureId> {
    let position = operation_positions.get(operation)?;
    if *position >= consumer_position {
        return None;
    }
    feature_ids.get(operation).cloned()
}

fn projects_neutral_feature(label: &str) -> bool {
    !matches!(label, "Container" | "TEXT")
}

fn text_semantic_annotation(
    native_ref: &str,
    order: u32,
    payload_strings: &[&str],
) -> Option<SemanticAnnotation> {
    let [text, font_family] = payload_strings else {
        return None;
    };
    Some(SemanticAnnotation {
        id: SemanticAnnotationId(format!("{native_ref}:semantic-text")),
        object: native_ref.to_string(),
        kind: SemanticAnnotationKind::Text,
        runtime_type: "TEXT".to_string(),
        order,
        text: vec![(*text).to_string()],
        references: BTreeMap::new(),
        value: None,
        format: None,
        position: None,
        parameters: BTreeMap::from([("font_family".to_string(), (*font_family).to_string())]),
        assets: Vec::new(),
        native_ref: native_ref.to_string(),
    })
}

pub(crate) fn parameter_owner_dependencies(
    parameter_owners: &BTreeMap<ParameterId, Option<FeatureId>>,
    parameter_references: &[ParameterId],
) -> Vec<FeatureId> {
    let mut dependencies = Vec::new();
    for parameter_id in parameter_references {
        let Some(owner) = parameter_owners.get(parameter_id).and_then(Option::as_ref) else {
            continue;
        };
        if !dependencies.contains(owner) {
            dependencies.push(owner.clone());
        }
    }
    dependencies
}

fn extrude_feature_definition(
    construction_profile: Option<&str>,
    structured_construction: Option<&str>,
    op: BooleanOp,
    output_kinds: &[cadmpeg_ir::topology::BodyKind],
) -> FeatureDefinition {
    let constructions = [construction_profile, structured_construction]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let profile = match constructions.as_slice() {
        [construction] => ProfileRef::Native((*construction).to_string()),
        _ => ProfileRef::Unresolved("EXTRUDE".to_string()),
    };
    let solid = match output_kinds {
        [cadmpeg_ir::topology::BodyKind::Solid, rest @ ..]
            if rest
                .iter()
                .all(|kind| *kind == cadmpeg_ir::topology::BodyKind::Solid) =>
        {
            Some(true)
        }
        [cadmpeg_ir::topology::BodyKind::Sheet, rest @ ..]
            if rest
                .iter()
                .all(|kind| *kind == cadmpeg_ir::topology::BodyKind::Sheet) =>
        {
            Some(false)
        }
        _ => None,
    };
    FeatureDefinition::Extrude {
        profile,
        direction: cadmpeg_ir::features::ExtrudeDirection::Unresolved,
        start: cadmpeg_ir::features::ExtrudeStart::Unresolved,
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Unresolved,
                draft: None,
                offset: None,
            },
        },
        op,
        direction_source: None,
        solid,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }
}

fn extrude_boolean_op(
    history: &BodyWriterHistory,
    native_primary_body: Option<u32>,
    offset_store_primary_body: Option<&str>,
    output_kinds: &[cadmpeg_ir::topology::BodyKind],
) -> BooleanOp {
    let has_previous_writer =
        if native_primary_body.is_some() || offset_store_primary_body.is_some() {
            history.has_preceding_writer(None, native_primary_body, offset_store_primary_body, &[])
        } else {
            true
        };
    if !has_previous_writer
        && matches!(
            output_kinds,
            [cadmpeg_ir::topology::BodyKind::Solid | cadmpeg_ir::topology::BodyKind::Sheet]
        )
    {
        BooleanOp::NewBody
    } else {
        BooleanOp::Unresolved
    }
}

fn body_faces<'a>(ir: &'a CadIr, body_id: &BodyId) -> Option<Vec<&'a Face>> {
    let body = ir.model.bodies.iter().find(|body| body.id == *body_id)?;
    let mut faces = Vec::new();
    for region_id in &body.regions {
        let region = ir
            .model
            .regions
            .iter()
            .find(|region| region.id == *region_id && region.body == body.id)?;
        for shell_id in &region.shells {
            let shell = ir
                .model
                .shells
                .iter()
                .find(|shell| shell.id == *shell_id && shell.region == region.id)?;
            for face_id in &shell.faces {
                let face = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id && face.shell == shell.id)?;
                faces.push(face);
            }
        }
    }
    Some(faces)
}

fn connected_solid_body_faces<'a>(ir: &'a CadIr, body_id: &BodyId) -> Option<Vec<&'a Face>> {
    let body = ir.model.bodies.iter().find(|body| body.id == *body_id)?;
    if body.kind != cadmpeg_ir::topology::BodyKind::Solid {
        return None;
    }
    let [region_id] = body.regions.as_slice() else {
        return None;
    };
    let region = ir
        .model
        .regions
        .iter()
        .find(|region| region.id == *region_id && region.body == body.id)?;
    let [shell_id] = region.shells.as_slice() else {
        return None;
    };
    let shell = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == *shell_id && shell.region == region.id)?;
    shell
        .faces
        .iter()
        .map(|face_id| {
            ir.model
                .faces
                .iter()
                .find(|face| face.id == *face_id && face.shell == shell.id)
        })
        .collect()
}

fn body_surface_ids(ir: &CadIr, body_id: &BodyId) -> Option<BTreeSet<SurfaceId>> {
    Some(
        body_faces(ir, body_id)?
            .into_iter()
            .map(|face| face.surface.clone())
            .collect(),
    )
}

/// Neutral operand family named by an NX rolling-ball blend operation.
#[derive(Clone, Copy)]
enum NxBlendFamily {
    /// Edge-selected `BLEND` operation.
    Edge,
    /// Face-selected `FACE_BLEND` operation.
    Face,
}

/// Project complete owned rolling-ball carriers into their named blend family.
fn blend_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
    family: NxBlendFamily,
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let [body] = outputs else {
        return None;
    };
    let body_surfaces = body_surface_ids(ir, body)?;
    let mut surfaces = Vec::new();
    let mut laws = Vec::new();
    let mut support_pairs = Vec::new();
    for procedural in &ir.model.procedural_surfaces {
        if !body_surfaces.contains(&procedural.surface) {
            continue;
        }
        let ProceduralSurfaceDefinition::Blend {
            supports,
            radius,
            cross_section,
            ..
        } = &procedural.definition
        else {
            continue;
        };
        if *cross_section != BlendCrossSection::Circular {
            return None;
        }
        surfaces.push(procedural.surface.clone());
        laws.push(radius);
        support_pairs.push(supports);
    }
    if laws.is_empty() {
        return None;
    }
    surfaces.sort();
    let constant_radii = laws
        .iter()
        .map(|law| match law {
            BlendRadiusLaw::Constant { signed_radius }
                if signed_radius.is_finite() && *signed_radius != 0.0 =>
            {
                Some(signed_radius.abs())
            }
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    let radius = constant_radii
        .as_ref()
        .filter(|radii| {
            radii
                .iter()
                .all(|radius| radius.to_bits() == radii[0].to_bits())
        })
        .map_or_else(
            || RadiusSpec::Unresolved {
                form: if constant_radii.is_some() {
                    Some(RadiusForm::Constant)
                } else if laws.iter().all(|law| {
                    matches!(
                        law,
                        BlendRadiusLaw::Linear { .. } | BlendRadiusLaw::Law { .. }
                    )
                }) {
                    Some(RadiusForm::Variable)
                } else {
                    None
                },
            },
            |radii| RadiusSpec::Constant {
                radius: Length(radii[0]),
            },
        );
    let face_blend = matches!(family, NxBlendFamily::Face)
        .then(|| {
            support_pairs
                .iter()
                .map(|supports| {
                    let [Some(first), Some(second)] = supports else {
                        return None;
                    };
                    (first.surface != second.surface)
                        .then_some([first.surface.clone(), second.surface.clone()])
                })
                .collect::<Option<Vec<_>>>()
                .and_then(blend_support_bipartition)
                .and_then(|(first, second)| {
                    let (first_faces, _) = support_face_projection(
                        ir,
                        &first,
                        format!("{}:blend-first-support-surfaces", body.0),
                    );
                    let (second_faces, _) = support_face_projection(
                        ir,
                        &second,
                        format!("{}:blend-second-support-surfaces", body.0),
                    );
                    match (&first_faces, &second_faces) {
                        (FaceSelection::Resolved { .. }, FaceSelection::Resolved { .. }) => {
                            Some(FeatureDefinition::FaceBlend {
                                first_faces,
                                second_faces,
                                radius: radius.clone(),
                            })
                        }
                        _ => None,
                    }
                })
        })
        .flatten();
    let unresolved = match family {
        NxBlendFamily::Edge => FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius,
                tangency_weight: None,
            }],
        },
        NxBlendFamily::Face => FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Unresolved,
            second_faces: FaceSelection::Unresolved,
            radius,
        },
    };
    Some((face_blend.unwrap_or(unresolved), surfaces))
}

/// Split an unordered rolling-ball support graph into two deterministic face
/// sets. Face blending is symmetric, so each connected component starts with
/// its lowest surface identity on the first side. The support graph must be
/// complete bipartite: odd cycles and missing cross-pairs cannot be represented
/// by one neutral face-blend operation.
fn blend_support_bipartition(
    pairs: Vec<[SurfaceId; 2]>,
) -> Option<(Vec<SurfaceId>, Vec<SurfaceId>)> {
    let mut adjacent = BTreeMap::<SurfaceId, BTreeSet<SurfaceId>>::new();
    for [first, second] in pairs {
        if first == second {
            return None;
        }
        adjacent
            .entry(first.clone())
            .or_default()
            .insert(second.clone());
        adjacent.entry(second).or_default().insert(first);
    }
    let mut sides = BTreeMap::<SurfaceId, bool>::new();
    for seed in adjacent.keys() {
        if sides.contains_key(seed) {
            continue;
        }
        sides.insert(seed.clone(), false);
        let mut pending = vec![seed.clone()];
        while let Some(surface) = pending.pop() {
            let side = sides[&surface];
            for neighbor in &adjacent[&surface] {
                match sides.get(neighbor) {
                    Some(neighbor_side) if *neighbor_side == side => return None,
                    Some(_) => {}
                    None => {
                        sides.insert(neighbor.clone(), !side);
                        pending.push(neighbor.clone());
                    }
                }
            }
        }
    }
    let (first, second): (Vec<_>, Vec<_>) = sides
        .into_iter()
        .partition(|(_, second_side)| !*second_side);
    let first = first
        .into_iter()
        .map(|(surface, _)| surface)
        .collect::<Vec<_>>();
    let second = second
        .into_iter()
        .map(|(surface, _)| surface)
        .collect::<Vec<_>>();
    if first.iter().any(|surface| {
        second
            .iter()
            .any(|other| !adjacent[surface].contains(other))
    }) {
        return None;
    }
    Some((first, second))
}

fn offset_surface_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let (body, distance, supports) = owned_offset_surface_data(ir, outputs)?;
    let native = format!("{}:offset-support-surfaces", body.0);
    let (faces, senses) = support_face_projection(ir, &supports, native);
    let distance = senses
        .as_deref()
        .and_then(uniform_face_sense)
        .map(|sense| match sense {
            Sense::Forward => distance,
            Sense::Reversed => -distance,
        });
    Some((
        FeatureDefinition::OffsetSurface {
            faces,
            distance: distance.map(Length),
        },
        supports,
    ))
}

fn owned_offset_surface_data<'a>(
    ir: &CadIr,
    outputs: &'a [BodyId],
) -> Option<(&'a BodyId, f64, Vec<SurfaceId>)> {
    let (body, carriers) = owned_offset_carriers(ir, outputs)?;
    let distance = carriers[0].1;
    if carriers
        .iter()
        .any(|(_, candidate)| candidate.to_bits() != distance.to_bits())
    {
        return None;
    }
    let supports = carriers
        .into_iter()
        .map(|(support, _)| support)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Some((body, distance, supports))
}

fn owned_offset_carriers<'a>(
    ir: &CadIr,
    outputs: &'a [BodyId],
) -> Option<(&'a BodyId, Vec<(SurfaceId, f64)>)> {
    let [body] = outputs else {
        return None;
    };
    let body_surfaces = body_surface_ids(ir, body)?;
    let mut carriers = Vec::new();
    for procedural in &ir.model.procedural_surfaces {
        if !body_surfaces.contains(&procedural.surface) {
            continue;
        }
        let ProceduralSurfaceDefinition::Offset {
            support,
            distance: candidate,
            ..
        } = &procedural.definition
        else {
            continue;
        };
        carriers.push((support.clone(), *candidate));
    }
    (!carriers.is_empty()).then_some((body, carriers))
}

fn thicken_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let (body, thickness, supports, direction) = owned_thicken_surface_data(ir, outputs)?;
    let native = format!("{}:thicken-support-surfaces", body.0);
    let (faces, senses) = support_face_projection(ir, &supports, native);
    let side = match direction {
        ThickenDirection::Both => Some(ThickenSide::Both),
        ThickenDirection::Signed(distance) => senses
            .as_deref()
            .and_then(uniform_face_sense)
            .map(|sense| thicken_side(distance, sense)),
    };
    Some((
        FeatureDefinition::Thicken {
            faces,
            thickness: Some(Length(thickness)),
            side,
        },
        supports,
    ))
}

enum ThickenDirection {
    Signed(f64),
    Both,
}

fn owned_thicken_surface_data<'a>(
    ir: &CadIr,
    outputs: &'a [BodyId],
) -> Option<(&'a BodyId, f64, Vec<SurfaceId>, ThickenDirection)> {
    let (body, carriers) = owned_offset_carriers(ir, outputs)?;
    if ir
        .model
        .bodies
        .iter()
        .find(|candidate| candidate.id == *body)?
        .kind
        != BodyKind::Solid
    {
        return None;
    }
    let distance = carriers[0].1;
    if carriers
        .iter()
        .all(|(_, candidate)| candidate.to_bits() == distance.to_bits())
    {
        if distance.is_finite() && distance != 0.0 {
            let supports = carriers
                .into_iter()
                .map(|(support, _)| support)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            return Some((
                body,
                distance.abs(),
                supports,
                ThickenDirection::Signed(distance),
            ));
        }
        return None;
    }

    let mut magnitude = None::<f64>;
    let mut positive = BTreeSet::new();
    let mut negative = BTreeSet::new();
    for (support, distance) in carriers {
        if !distance.is_finite() || distance == 0.0 {
            return None;
        }
        let candidate = distance.abs();
        if magnitude.is_some_and(|magnitude| magnitude.to_bits() != candidate.to_bits()) {
            return None;
        }
        magnitude = Some(candidate);
        if distance.is_sign_positive() {
            positive.insert(support);
        } else {
            negative.insert(support);
        }
    }
    if positive.is_empty() || positive != negative {
        return None;
    }
    let thickness = magnitude? * 2.0;
    if !thickness.is_finite() {
        return None;
    }
    Some((
        body,
        thickness,
        positive.into_iter().collect(),
        ThickenDirection::Both,
    ))
}

fn support_face_projection(
    ir: &CadIr,
    supports: &[SurfaceId],
    native: String,
) -> (FaceSelection, Option<Vec<Sense>>) {
    let faces = supports
        .iter()
        .map(|support| {
            let matches = ir
                .model
                .faces
                .iter()
                .filter(|face| face.surface == *support)
                .collect::<Vec<_>>();
            let [face] = matches.as_slice() else {
                return None;
            };
            Some((face.id.clone(), face.sense))
        })
        .collect::<Option<Vec<_>>>();
    match faces {
        Some(faces)
            if faces
                .iter()
                .map(|(face, _)| face)
                .collect::<BTreeSet<_>>()
                .len()
                == faces.len() =>
        {
            let (faces, senses): (Vec<_>, Vec<_>) = faces.into_iter().unzip();
            (FaceSelection::Resolved { faces, native }, Some(senses))
        }
        _ => (FaceSelection::Native(native), None),
    }
}

fn thicken_side(distance: f64, sense: Sense) -> ThickenSide {
    match (distance.is_sign_positive(), sense) {
        (true, Sense::Forward) | (false, Sense::Reversed) => ThickenSide::Forward,
        (true, Sense::Reversed) | (false, Sense::Forward) => ThickenSide::Reverse,
    }
}

fn uniform_face_sense(senses: &[Sense]) -> Option<Sense> {
    let (first, rest) = senses.split_first()?;
    rest.iter().all(|sense| sense == first).then_some(*first)
}

pub(crate) fn feature_source_content(
    payload_strings: &[&crate::native::features::FeaturePayloadString],
) -> Vec<FeatureSourceContent> {
    let mut content = payload_strings
        .iter()
        .map(|value| {
            (
                value.source_offset,
                FeatureSourceContent::Text(value.value.clone()),
            )
        })
        .collect::<Vec<_>>();
    content.sort_by_key(|(offset, _)| *offset);
    content.into_iter().map(|(_, content)| content).collect()
}

fn simple_hole_native_properties(
    operation_label: &str,
    templates: &[crate::native::features::FeatureSimpleHoleTemplate],
    repeated_lanes: &[crate::native::features::FeatureSimpleHoleRepeatedScalarLane],
    block_references: &[crate::native::features::FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
    construction_groups: &[crate::native::features::FeatureSimpleHoleConstructionGroup],
) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    if let Some(template) = templates
        .iter()
        .find(|template| template.operation_label == operation_label)
    {
        properties.insert("simple_hole_template".to_string(), template.id.clone());
    }
    if let Some(pair) = repeated_lanes
        .iter()
        .find(|pair| pair.operation_label == operation_label)
    {
        properties.insert(
            "simple_hole_repeated_scalar_lane".to_string(),
            pair.id.clone(),
        );
    }
    if let Some(references) = block_references
        .iter()
        .find(|references| references.operation_label == operation_label)
    {
        properties.insert(
            "simple_hole_repeated_scalar_lane_block_references".to_string(),
            references.id.clone(),
        );
    }
    if let Some(group) = construction_groups.iter().find(|group| {
        group
            .operation_labels
            .iter()
            .any(|label| label == operation_label)
    }) {
        properties.insert(
            "simple_hole_construction_group".to_string(),
            group.id.clone(),
        );
    }
    properties
}

fn block_placement(
    ir: &CadIr,
    dimensions: [f64; 3],
    outputs: &[BodyId],
) -> Option<(BodyId, Transform)> {
    struct PlaneBand {
        normal: Vector3,
        offsets: Vec<f64>,
    }

    #[derive(Clone, Copy)]
    struct PlaneExtent {
        normal: Vector3,
        minimum: f64,
        maximum: f64,
    }

    fn canonical_normal(mut normal: Vector3, angular_tolerance: f64) -> Option<Vector3> {
        normal = unit_vector(normal)?;
        let leading = [normal.x, normal.y, normal.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)?;
        if leading < 0.0 {
            normal = Vector3::new(-normal.x, -normal.y, -normal.z);
        }
        Some(normal)
    }

    let linear_tolerance = ir.tolerances.linear;
    let angular_tolerance = ir.tolerances.angular;
    if dimensions
        .iter()
        .any(|dimension| !dimension.is_finite() || *dimension <= linear_tolerance)
    {
        return None;
    }
    let body = match outputs {
        [body] => body,
        [] => {
            let candidates = ir
                .model
                .bodies
                .iter()
                .filter(|body| connected_solid_body_faces(ir, &body.id).is_some())
                .map(|body| &body.id)
                .collect::<Vec<_>>();
            let [body] = candidates.as_slice() else {
                return None;
            };
            *body
        }
        _ => return None,
    };
    let faces = connected_solid_body_faces(ir, body)?;
    let surface_geometry = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let mut bands = Vec::<PlaneBand>::new();
    for face in faces {
        let geometry = surface_geometry.get(&face.surface).copied()?;
        let SurfaceGeometry::Plane { origin, normal, .. } = geometry else {
            continue;
        };
        let normal = canonical_normal(*normal, angular_tolerance)?;
        let offset = normal.dot(Vector3::new(origin.x, origin.y, origin.z));
        let existing = bands
            .iter_mut()
            .find(|band| (1.0 - dot_vector(band.normal, normal)).abs() <= angular_tolerance);
        if let Some(band) = existing {
            band.offsets.push(offset);
        } else {
            bands.push(PlaneBand {
                normal,
                offsets: vec![offset],
            });
        }
    }
    if bands.len() != 3
        || (0..3).any(|first| {
            (first + 1..3).any(|second| {
                dot_vector(bands[first].normal, bands[second].normal).abs() > angular_tolerance
            })
        })
    {
        return None;
    }
    let mut bands = bands
        .into_iter()
        .map(|mut band| {
            band.offsets.sort_by(f64::total_cmp);
            let mut clusters = Vec::<[f64; 2]>::new();
            for offset in band.offsets {
                if !offset.is_finite() {
                    return None;
                }
                match clusters.last_mut() {
                    Some(cluster) if offset - cluster[0] <= linear_tolerance => {
                        cluster[1] = offset;
                    }
                    _ => clusters.push([offset, offset]),
                }
            }
            let [minimum, maximum] = clusters.as_slice() else {
                return None;
            };
            (maximum[1] - minimum[0] > linear_tolerance).then_some(PlaneExtent {
                normal: band.normal,
                minimum: minimum[0],
                maximum: maximum[1],
            })
        })
        .collect::<Option<Vec<_>>>()?;
    bands.sort_by(|left, right| {
        right
            .normal
            .x
            .total_cmp(&left.normal.x)
            .then_with(|| right.normal.y.total_cmp(&left.normal.y))
            .then_with(|| right.normal.z.total_cmp(&left.normal.z))
    });
    let permutations = [
        [0usize, 1usize, 2usize],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let matches = permutations
        .into_iter()
        .filter(|permutation| {
            (0..3).all(|axis| {
                let band = bands[permutation[axis]];
                ((band.maximum - band.minimum) - dimensions[axis]).abs() <= linear_tolerance
            })
        })
        .collect::<Vec<_>>();
    let [permutation] = matches.as_slice() else {
        return None;
    };
    let mut ordered = permutation.map(|index| bands[index]);
    if dot_vector(
        cross_vector(ordered[0].normal, ordered[1].normal),
        ordered[2].normal,
    ) < 0.0
    {
        let third = &mut ordered[2];
        third.normal = Vector3::new(-third.normal.x, -third.normal.y, -third.normal.z);
        (third.minimum, third.maximum) = (-third.maximum, -third.minimum);
    }
    let origin = Point3::new(
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.x)
            .sum(),
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.y)
            .sum(),
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.z)
            .sum(),
    );
    let [x_axis, y_axis, z_axis] = ordered.map(|band| band.normal);
    Some((
        body.clone(),
        Transform {
            rows: [
                [x_axis.x, y_axis.x, z_axis.x, origin.x],
                [x_axis.y, y_axis.y, z_axis.y, origin.y],
                [x_axis.z, y_axis.z, z_axis.z, origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        },
    ))
}

/// Return the complete primitive witness for an NX `SPHERE` operation.
///
/// A spherical surface inside a larger result is not enough: a Boolean or a
/// later feature can leave the same carrier in the output. The primitive
/// projection therefore accepts only one connected solid body with exactly
/// one face whose surface is a finite positive-radius sphere. With no native
/// output relation, the candidate must also be unique across the model.
fn sphere_body_projection(ir: &CadIr, outputs: &[BodyId]) -> Option<(BodyId, Point3, Length)> {
    let body = match outputs {
        [body] => body.clone(),
        [] => {
            let candidates = ir
                .model
                .bodies
                .iter()
                .filter_map(|body| {
                    let faces = connected_solid_body_faces(ir, &body.id)?;
                    let [face] = faces.as_slice() else {
                        return None;
                    };
                    let surface = ir.model.surfaces.iter().find(|surface| {
                        surface.id == face.surface
                            && matches!(&surface.geometry, SurfaceGeometry::Sphere { .. })
                    })?;
                    Some((body.id.clone(), surface.id.clone()))
                })
                .collect::<Vec<_>>();
            let [(body, _)] = candidates.as_slice() else {
                return None;
            };
            body.clone()
        }
        _ => return None,
    };
    let faces = connected_solid_body_faces(ir, &body)?;
    let [face] = faces.as_slice() else {
        return None;
    };
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)?;
    let SurfaceGeometry::Sphere { center, radius, .. } = &surface.geometry else {
        return None;
    };
    ((*radius).is_finite()
        && *radius > ir.tolerances.linear
        && [center.x, center.y, center.z]
            .into_iter()
            .all(f64::is_finite))
    .then_some((body, *center, Length(*radius)))
}

struct NewBodyEvidence<'a> {
    has_complete_projection: bool,
    has_complete_primitive_construction: bool,
    outputs: &'a [BodyId],
    outputs_are_proven: bool,
    body_reference_count: usize,
    provisional_feature: Option<&'a FeatureId>,
    native_primary_body: Option<u32>,
    offset_store_primary_body: Option<&'a str>,
    history: &'a BodyWriterHistory,
}

fn new_body_boolean_op(evidence: &NewBodyEvidence<'_>) -> BooleanOp {
    // A unique offset-store body field proves the operation's local writer
    // namespace, but the fallback body selected for placement is not that
    // writer. Likewise, multiple body fields have no primary role until the
    // operation-specific relation identifies one. Do not let a placement
    // fallback turn either case into a neutral body Boolean.
    if evidence.body_reference_count > 1
        && evidence.native_primary_body.is_none()
        && evidence.offset_store_primary_body.is_none()
        && !evidence.has_complete_primitive_construction
    {
        return BooleanOp::Unresolved;
    }
    let writer_outputs = if evidence.outputs_are_proven {
        evidence.outputs
    } else {
        &[]
    };
    if evidence.has_complete_projection
        && matches!(evidence.outputs, [_])
        && !evidence.history.has_preceding_writer(
            evidence.provisional_feature,
            evidence.native_primary_body,
            evidence.offset_store_primary_body,
            writer_outputs,
        )
    {
        BooleanOp::NewBody
    } else {
        BooleanOp::Unresolved
    }
}

#[cfg(test)]
fn non_boolean_feature_definition(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole_diameter: Option<Length>,
) -> FeatureDefinition {
    non_boolean_feature_definition_with_parameters(
        kind,
        payload_strings,
        block_dimensions,
        block_placement,
        HoleProjection {
            diameter: hole_diameter,
            ..HoleProjection::default()
        },
        BTreeMap::new(),
    )
}

/// Project one operation as a history node only when its bounded record has
/// no modeling relation or value lane.
fn non_modeling_history_definition(
    kind: &str,
    object_indices: &[Option<u32>; 4],
    outputs: &[BodyId],
    body_reference_count: usize,
    body_operand_count: usize,
    payload_string_count: usize,
    source_properties: &BTreeMap<String, String>,
) -> Option<FeatureDefinition> {
    let operation_identity_only = source_properties.keys().all(|key| {
        matches!(
            key.as_str(),
            "operation_record" | "operation_terminal_frame"
        ) || (key
            .strip_prefix("object_index.")
            .is_some_and(|slot| matches!(slot, "0" | "1" | "2" | "3")))
    });
    (kind == "EXTRACT_STRING"
        && object_indices.iter().all(Option::is_none)
        && outputs.is_empty()
        && body_reference_count == 0
        && body_operand_count == 0
        && payload_string_count == 0
        && source_properties.contains_key("operation_record")
        && source_properties.contains_key("operation_terminal_frame")
        && operation_identity_only)
        .then_some(FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        })
}

/// Permutation-invariant hole properties derived from one complete body partition.
#[derive(Default)]
struct HoleProjection {
    pub(crate) placements: Vec<HolePlacement>,
    pub(crate) diameter: Option<Length>,
    pub(crate) extent: Option<Termination>,
    pub(crate) counterbore: Option<CounterboreDimensions>,
    pub(crate) chamfer: Option<HoleKind>,
    pub(crate) grouped_simple_through: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CounterboreDimensions {
    diameter: Length,
    depth: Length,
}

fn non_boolean_feature_definition_with_parameters(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole: HoleProjection,
    native_parameters: BTreeMap<String, String>,
) -> FeatureDefinition {
    let hole_template = unique_simple_hole_template(payload_strings);
    if let ("BLOCK", Some(dimensions)) = (kind, block_dimensions) {
        return FeatureDefinition::Block {
            dimensions: Some(dimensions.map(Length)),
            placement: block_placement,
            op: BooleanOp::Unresolved,
        };
    }
    if let Some(op) = match kind {
        "UNITE" => Some(BooleanOp::Join),
        "SUBTRACT" => Some(BooleanOp::Cut),
        "INTERSECT" => Some(BooleanOp::Intersect),
        _ => None,
    } {
        return FeatureDefinition::Combine {
            target: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            op,
            keep_tools: false,
        };
    }
    match kind {
        "DATUM_PLANE" | "EXTRACT_DATUM_PLANE" => FeatureDefinition::DatumPlaneUnresolved,
        "POINT" => FeatureDefinition::DatumPointUnresolved,
        "DATUM_CSYS" => FeatureDefinition::DatumCoordinateSystemUnresolved,
        "BLOCK" => FeatureDefinition::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        },
        "SKETCH" => FeatureDefinition::Sketch {
            space: SketchSpace::Unresolved,
            sketch: None,
        },
        "EXTRACT_BODY" => FeatureDefinition::ExtractBody {
            source: BodySelection::Unresolved,
        },
        "MASTER SNAPSHOT BODY" => FeatureDefinition::BaseFeature {
            bodies: BodySelection::Unresolved,
        },
        "SKIN" | "THRU_CURVE" => FeatureDefinition::LoftUnresolved,
        "Studio Surface" => FeatureDefinition::FreeformSurfaceUnresolved,
        "SWP104" => FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(None),
            sections: Vec::new(),
            path: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            mode: SweepMode::Unresolved,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
        "DRAFT" => FeatureDefinition::DraftUnresolved,
        "CPROJ" | "CPROJ_CMB" => FeatureDefinition::ProjectedCurve {
            source: PathRef::Unresolved("nx:unresolved".into()),
            target_faces: FaceSelection::Unresolved,
            direction: CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved),
            bidirectional: None,
        },
        "TRIMMED_SH" => FeatureDefinition::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: PathRef::Unresolved("nx:unresolved".into()),
            keep: TrimRegion::Unresolved,
        },
        "EXTEND_SHEET" => FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        },
        "SIMPLE HOLE" | "CBORE_HOLE" => {
            let measured_chamfer = hole.chamfer;
            let (template_kind, template_exit_kind, template_extent) = hole_template.map_or(
                (
                    if kind == "CBORE_HOLE" {
                        HoleKind::Unresolved {
                            form: None,
                            counterbore_diameter: None,
                            counterbore_depth: None,
                            countersink_diameter: None,
                            countersink_angle: None,
                        }
                    } else {
                        HoleKind::Simple
                    },
                    None,
                    None,
                ),
                |(_, form, extent, start_treatment, end_treatment)| {
                    let kind = match start_treatment {
                        crate::native::features::SimpleHoleEndTreatment::Chamfer => {
                            HoleKind::Unresolved {
                                form: Some(HoleForm::Chamfer),
                                counterbore_diameter: None,
                                counterbore_depth: None,
                                countersink_diameter: None,
                                countersink_angle: None,
                            }
                        }
                        crate::native::features::SimpleHoleEndTreatment::None => match form {
                            crate::native::features::SimpleHoleForm::Simple => HoleKind::Simple,
                            crate::native::features::SimpleHoleForm::Counterbored => {
                                HoleKind::Unresolved {
                                    form: Some(HoleForm::Counterbore),
                                    counterbore_diameter: None,
                                    counterbore_depth: None,
                                    countersink_diameter: None,
                                    countersink_angle: None,
                                }
                            }
                        },
                    };
                    let exit_kind = match end_treatment {
                        crate::native::features::SimpleHoleEndTreatment::Chamfer => {
                            Some(HoleKind::Unresolved {
                                form: Some(HoleForm::Chamfer),
                                counterbore_diameter: None,
                                counterbore_depth: None,
                                countersink_diameter: None,
                                countersink_angle: None,
                            })
                        }
                        crate::native::features::SimpleHoleEndTreatment::None => None,
                    };
                    let extent = match extent {
                        crate::native::features::SimpleHoleExtent::Through => {
                            Some(cadmpeg_ir::features::Termination::ThroughAll)
                        }
                        crate::native::features::SimpleHoleExtent::Blind => None,
                    };
                    (kind, exit_kind, extent)
                },
            );
            let template_kind = match (
                hole.counterbore,
                matches!(
                    &template_kind,
                    HoleKind::Unresolved {
                        form: Some(HoleForm::Counterbore),
                        ..
                    }
                ),
            ) {
                (Some(dimensions), true) => HoleKind::Counterbore {
                    diameter: dimensions.diameter,
                    depth: dimensions.depth,
                },
                _ => template_kind,
            };
            FeatureDefinition::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                position: None,
                direction: None,
                placements: hole.placements,
                kind: match (measured_chamfer, hole_template) {
                    (
                        Some(chamfer),
                        Some((
                            _,
                            crate::native::features::SimpleHoleForm::Simple,
                            crate::native::features::SimpleHoleExtent::Through,
                            crate::native::features::SimpleHoleEndTreatment::Chamfer,
                            crate::native::features::SimpleHoleEndTreatment::Chamfer,
                        )),
                    ) => chamfer,
                    _ => template_kind,
                },
                exit_kind: match (measured_chamfer, hole_template) {
                    (
                        Some(chamfer),
                        Some((
                            _,
                            crate::native::features::SimpleHoleForm::Simple,
                            crate::native::features::SimpleHoleExtent::Through,
                            crate::native::features::SimpleHoleEndTreatment::Chamfer,
                            crate::native::features::SimpleHoleEndTreatment::Chamfer,
                        )),
                    ) => Some(chamfer),
                    _ => template_exit_kind,
                },
                diameter: hole.diameter,
                extent: hole.extent.or(template_extent),
                bottom: None,
                taper_angle: None,
                specification: None,
                allow_multi_profile_faces: None,
            }
        }
        "HOLE PACKAGE" => FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            position: None,
            direction: None,
            placements: hole.placements,
            kind: if hole.grouped_simple_through {
                hole.chamfer.unwrap_or(HoleKind::Simple)
            } else {
                HoleKind::Unresolved {
                    form: None,
                    counterbore_diameter: None,
                    counterbore_depth: None,
                    countersink_diameter: None,
                    countersink_angle: None,
                }
            },
            exit_kind: hole
                .grouped_simple_through
                .then_some(hole.chamfer)
                .flatten(),
            diameter: hole.diameter,
            extent: hole
                .grouped_simple_through
                .then_some(cadmpeg_ir::features::Termination::ThroughAll),
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
        "RIB" => FeatureDefinition::Rib {
            construction: RibConstruction {
                profile: None,
                direction: None,
                thickness: None,
                side: None,
                draft: RibDraft::Unresolved,
            },
            op: BooleanOp::Unresolved,
        },
        "CHAMFER" => FeatureDefinition::Chamfer {
            groups: vec![cadmpeg_ir::features::ChamferGroup {
                edges: EdgeSelection::Unresolved,
                spec: ChamferSpec::Unresolved { form: None },
            }],
            flip_direction: false,
        },
        "BLEND" => FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius: RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            }],
        },
        "FACE_BLEND" => FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Unresolved,
            second_faces: FaceSelection::Unresolved,
            radius: RadiusSpec::Unresolved { form: None },
        },
        "SEW" => FeatureDefinition::SewBodies {
            bodies: BodySelection::Unresolved,
            gap_tolerance: None,
        },
        "TRIM BODY" => FeatureDefinition::TrimBodies {
            targets: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            keep: BodyTrimSide::Unresolved,
        },
        "EXTRUDE" => extrude_feature_definition(None, None, BooleanOp::Unresolved, &[]),
        "OFFSET" => FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
        },
        "THICKEN_SHEET" => FeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        },
        "Pattern Feature"
        | "Pattern Geometry"
        | "Geometry Instance"
        | "Multi Instance Output"
        | "IDENTICAL INSTANCE OUTPUT"
        | "Instance Feature" => FeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved { form: None },
        },
        "ASSOCIATIVE_INTERSECTION" | "Intersection Curve" => FeatureDefinition::SectionShape {
            first: BodySelection::Unresolved,
            second: BodySelection::Unresolved,
            approximate: None,
        },
        _ => FeatureDefinition::Native {
            kind: kind.to_string(),
            parameters: native_parameters,
            properties: BTreeMap::new(),
        },
    }
}

fn native_feature_parameters(
    uses: &[&crate::native::features::FeatureParameterUse],
    expressions: &[crate::native::om::Expression],
) -> BTreeMap<String, String> {
    let by_id = expressions
        .iter()
        .map(|expression| (expression.id.as_str(), expression))
        .collect::<BTreeMap<_, _>>();
    let mut parameters = BTreeMap::new();
    for parameter_use in uses {
        let Some(expression) = by_id.get(parameter_use.expression.as_str()) else {
            return BTreeMap::new();
        };
        if parameters
            .insert(expression.name.clone(), expression.expression.clone())
            .is_some()
        {
            return BTreeMap::new();
        }
    }
    parameters
}

fn simple_hole_operations(
    templates: &[crate::native::features::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::FeatureSimpleHoleConstructionGroup],
    operation_positions: &BTreeMap<&str, usize>,
) -> Option<Vec<String>> {
    let template_counts = templates
        .iter()
        .fold(BTreeMap::new(), |mut counts, template| {
            *counts
                .entry(template.operation_label.as_str())
                .or_insert(0usize) += 1;
            counts
        });
    let mut ordered_templates = templates
        .iter()
        .filter(|template| {
            template.form == crate::native::features::SimpleHoleForm::Simple
                && template.extent == crate::native::features::SimpleHoleExtent::Through
        })
        .collect::<Vec<_>>();
    let template_operations = ordered_templates
        .iter()
        .map(|template| template.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    if template_operations.is_empty()
        || ordered_templates
            .iter()
            .any(|template| template_counts.get(template.operation_label.as_str()) != Some(&1))
    {
        return None;
    }
    if ordered_templates
        .iter()
        .any(|template| !operation_positions.contains_key(template.operation_label.as_str()))
    {
        return None;
    }
    ordered_templates.sort_by(|first, second| {
        operation_positions
            .get(first.operation_label.as_str())
            .cmp(&operation_positions.get(second.operation_label.as_str()))
            .then_with(|| first.operation_label.cmp(&second.operation_label))
    });
    let matching_groups = groups
        .iter()
        .filter(|group| {
            let group_operations = group
                .operation_labels
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            group_operations == template_operations
        })
        .collect::<Vec<_>>();
    if matching_groups.len() > 1 {
        return None;
    }
    Some(match matching_groups.as_slice() {
        [] => ordered_templates
            .iter()
            .map(|template| template.operation_label.clone())
            .collect::<Vec<_>>(),
        [group] => {
            let group_operations = group
                .operation_labels
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            if group_operations.len() != group.operation_labels.len()
                || template_operations != group_operations
                || group
                    .operation_labels
                    .iter()
                    .any(|operation| !operation_positions.contains_key(operation.as_str()))
                || group.operation_labels.windows(2).any(|pair| {
                    operation_positions[pair[0].as_str()] >= operation_positions[pair[1].as_str()]
                })
            {
                return None;
            }
            group.operation_labels.clone()
        }
        _ => unreachable!(),
    })
}

/// Return simple blind-hole operations in feature-history order. A blind
/// operation with competing typed templates is not assignable to one body
/// witness and remains native-only.
fn blind_hole_operations(
    templates: &[crate::native::features::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
) -> Option<Vec<String>> {
    let template_counts = templates
        .iter()
        .fold(BTreeMap::new(), |mut counts, template| {
            *counts
                .entry(template.operation_label.as_str())
                .or_insert(0usize) += 1;
            counts
        });
    let mut operations = templates
        .iter()
        .filter(|template| {
            template.form == crate::native::features::SimpleHoleForm::Simple
                && template.extent == crate::native::features::SimpleHoleExtent::Blind
        })
        .filter(|template| template_counts.get(template.operation_label.as_str()) == Some(&1))
        .map(|template| template.operation_label.clone())
        .collect::<Vec<_>>();
    if operations.is_empty()
        || operations.iter().collect::<BTreeSet<_>>().len() != operations.len()
        || operations
            .iter()
            .any(|operation| !operation_positions.contains_key(operation.as_str()))
    {
        return None;
    }
    operations.sort_by(|first, second| {
        operation_positions
            .get(first.as_str())
            .cmp(&operation_positions.get(second.as_str()))
            .then_with(|| first.cmp(second))
    });
    Some(operations)
}

/// Return counterbored through-hole operations in feature-history order.
/// Counterbore construction groups are not inferred from the scalar lanes:
/// each operation must have its own unambiguous body and topology witness.
fn counterbore_operations(
    templates: &[crate::native::features::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
) -> Option<Vec<String>> {
    let template_counts = templates
        .iter()
        .fold(BTreeMap::new(), |mut counts, template| {
            *counts
                .entry(template.operation_label.as_str())
                .or_insert(0usize) += 1;
            counts
        });
    let mut operations = templates
        .iter()
        .filter(|template| {
            template.form == crate::native::features::SimpleHoleForm::Counterbored
                && template.extent == crate::native::features::SimpleHoleExtent::Through
                && template.start_treatment == crate::native::features::SimpleHoleEndTreatment::None
                && template.end_treatment == crate::native::features::SimpleHoleEndTreatment::None
        })
        .filter(|template| template_counts.get(template.operation_label.as_str()) == Some(&1))
        .map(|template| template.operation_label.clone())
        .collect::<Vec<_>>();
    if operations.is_empty()
        || operations.iter().collect::<BTreeSet<_>>().len() != operations.len()
        || operations
            .iter()
            .any(|operation| !operation_positions.contains_key(operation.as_str()))
    {
        return None;
    }
    operations.sort_by(|first, second| {
        operation_positions
            .get(first.as_str())
            .cmp(&operation_positions.get(second.as_str()))
            .then_with(|| first.cmp(second))
    });
    Some(operations)
}

#[derive(Default)]
struct HolePackageProjection {
    internal_operations: BTreeSet<String>,
    outputs: BTreeMap<String, Vec<BodyId>>,
    diameters: BTreeMap<String, Length>,
    chamfers: BTreeMap<String, HoleKind>,
    placements: BTreeMap<String, Vec<HolePlacement>>,
}

fn hole_package_projection(
    ir: &CadIr,
    templates: &[crate::native::features::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::FeatureSimpleHoleConstructionGroup],
    uses: &[crate::native::features::FeatureHolePackageConstructionGroupUse],
    outputs: &BTreeMap<String, Vec<BodyId>>,
    diameters: &BTreeMap<String, Length>,
    chamfers: &BTreeMap<String, HoleKind>,
) -> HolePackageProjection {
    let mut projection = HolePackageProjection::default();
    let package_counts = uses
        .iter()
        .fold(BTreeMap::<&str, usize>::new(), |mut counts, use_| {
            *counts.entry(use_.operation_label.as_str()).or_default() += 1;
            counts
        });
    let group_counts = uses
        .iter()
        .fold(BTreeMap::<&str, usize>::new(), |mut counts, use_| {
            *counts
                .entry(use_.simple_hole_construction_group.as_str())
                .or_default() += 1;
            counts
        });
    for use_ in uses {
        if package_counts.get(use_.operation_label.as_str()) != Some(&1)
            || group_counts.get(use_.simple_hole_construction_group.as_str()) != Some(&1)
        {
            continue;
        }
        let Some(group) = groups
            .iter()
            .find(|group| group.id == use_.simple_hole_construction_group)
        else {
            continue;
        };
        if group.operation_labels.is_empty()
            || group.operation_labels.iter().collect::<BTreeSet<_>>().len()
                != group.operation_labels.len()
            || group
                .operation_labels
                .iter()
                .any(|operation| projection.internal_operations.contains(operation))
        {
            continue;
        }
        let child_templates = group
            .operation_labels
            .iter()
            .map(|operation| {
                templates
                    .iter()
                    .filter(|template| template.operation_label == *operation)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if child_templates.iter().any(|matches| {
            !matches!(matches.as_slice(), [template]
                if template.form == crate::native::features::SimpleHoleForm::Simple
                    && template.extent == crate::native::features::SimpleHoleExtent::Through)
        }) {
            continue;
        }
        let child_outputs = group
            .operation_labels
            .iter()
            .filter_map(|operation| outputs.get(operation))
            .collect::<Vec<_>>();
        let Some([body]) = child_outputs.first().map(|bodies| bodies.as_slice()) else {
            continue;
        };
        if child_outputs.len() != group.operation_labels.len()
            || child_outputs
                .iter()
                .any(|candidate| candidate.as_slice() != [body.clone()])
        {
            continue;
        }
        let Some(diameter) = group
            .operation_labels
            .first()
            .and_then(|operation| diameters.get(operation))
            .copied()
        else {
            continue;
        };
        if group
            .operation_labels
            .iter()
            .any(|operation| diameters.get(operation).copied() != Some(diameter))
        {
            continue;
        }
        let requests_chamfer = child_templates.iter().all(|matches| {
            let template = matches[0];
            template.start_treatment == crate::native::features::SimpleHoleEndTreatment::Chamfer
                && template.end_treatment
                    == crate::native::features::SimpleHoleEndTreatment::Chamfer
        });
        if !requests_chamfer {
            continue;
        }
        let Some(chamfer) = group
            .operation_labels
            .first()
            .and_then(|operation| chamfers.get(operation))
            .copied()
        else {
            continue;
        };
        if group
            .operation_labels
            .iter()
            .any(|operation| chamfers.get(operation).copied() != Some(chamfer))
        {
            continue;
        }
        projection
            .internal_operations
            .extend(group.operation_labels.iter().cloned());
        projection
            .outputs
            .insert(use_.operation_label.clone(), vec![body.clone()]);
        projection
            .diameters
            .insert(use_.operation_label.clone(), diameter);
        projection
            .chamfers
            .insert(use_.operation_label.clone(), chamfer);
        let placements = hole_axis_placements_for_body(ir, body);
        if placements.len() == group.operation_labels.len() {
            projection
                .placements
                .insert(use_.operation_label.clone(), placements);
        }
    }
    projection
}

struct HoleBodyProjection {
    outputs: BTreeMap<String, Vec<BodyId>>,
    diameters: BTreeMap<String, Length>,
    blind_depths: BTreeMap<String, Length>,
    counterbores: BTreeMap<String, CounterboreDimensions>,
}

fn hole_body_projection(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Option<HoleBodyProjection> {
    if operations.is_empty() || operations.iter().collect::<BTreeSet<_>>().len() != operations.len()
    {
        return None;
    }
    let operations_by_body = hole_operations_by_body(ir, operations, outputs)?;

    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let body_faces = connected_solid_body_faces(ir, &body)?;
        let bores = through_bore_cylinders(ir, &body_faces)?;
        let radii = bores
            .into_iter()
            .map(|(_, _, radius)| radius)
            .collect::<Vec<_>>();
        let radius = radii.first().copied()?;
        if radii.len() != operations.len()
            || radii
                .iter()
                .any(|candidate| candidate.to_bits() != radius.to_bits())
        {
            return None;
        }
        for operation in operations {
            projected_outputs.insert(operation.clone(), vec![body.clone()]);
            diameters.insert(operation, Length(radius * 2.0));
        }
    }
    Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths: BTreeMap::new(),
        counterbores: BTreeMap::new(),
    })
}

fn counterbore_body_projection(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Option<HoleBodyProjection> {
    if operations.is_empty() || operations.iter().collect::<BTreeSet<_>>().len() != operations.len()
    {
        return None;
    }
    let operations_by_body = hole_operations_by_body(ir, operations, outputs)?;
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut counterbores = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            // A counterbore pair has no serialized operation-to-pair relation
            // once multiple operations share one result body. Do not assign
            // geometry to history order.
            return None;
        };
        let body_faces = connected_solid_body_faces(ir, &body)?;
        let witnesses = counterbore_cylinders(ir, &body_faces)?;
        let [witness] = witnesses.as_slice() else {
            return None;
        };
        projected_outputs.insert(operation.clone(), vec![body.clone()]);
        diameters.insert(operation.clone(), Length(witness.bore_radius * 2.0));
        counterbores.insert(
            operation.clone(),
            CounterboreDimensions {
                diameter: Length(witness.counterbore_radius * 2.0),
                depth: Length(witness.depth),
            },
        );
    }
    Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths: BTreeMap::new(),
        counterbores,
    })
}

fn blind_hole_body_projection(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Option<HoleBodyProjection> {
    if operations.is_empty() || operations.iter().collect::<BTreeSet<_>>().len() != operations.len()
    {
        return None;
    }
    let operations_by_body = hole_operations_by_body(ir, operations, outputs)?;
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut blind_depths = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return None;
        };
        let body_faces = connected_solid_body_faces(ir, &body)?;
        let witnesses = blind_bore_cylinders(ir, &body_faces)?;
        let [witness] = witnesses.as_slice() else {
            return None;
        };
        projected_outputs.insert(operation.clone(), vec![body.clone()]);
        diameters.insert(operation.clone(), Length(witness.bore_radius * 2.0));
        blind_depths.insert(operation.clone(), Length(witness.depth));
    }
    Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths,
        counterbores: BTreeMap::new(),
    })
}

/// Derive one complete unoriented placement when one operation owns exactly
/// one through bore. The closest point to the model origin is invariant under
/// axial shifts of the serialized cylinder origin. Canonical axis sign makes
/// serialization deterministic but carries no drilling-direction semantics.
fn hole_axis_placements_for_operations(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, HolePlacement> {
    if operations.is_empty() || operations.iter().collect::<BTreeSet<_>>().len() != operations.len()
    {
        return BTreeMap::new();
    }
    let Some(operations_by_body) = hole_operations_by_body(ir, operations, outputs) else {
        return BTreeMap::new();
    };

    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            continue;
        };
        let mut body_placements = hole_axis_placements_for_body(ir, &body);
        if body_placements.len() != 1 {
            continue;
        }
        placements.insert(operation.clone(), body_placements.remove(0));
    }
    placements
}

fn counterbore_axis_placements_for_operations(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, HolePlacement> {
    if operations.is_empty() || operations.iter().collect::<BTreeSet<_>>().len() != operations.len()
    {
        return BTreeMap::new();
    }
    let Some(operations_by_body) = hole_operations_by_body(ir, operations, outputs) else {
        return BTreeMap::new();
    };
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return BTreeMap::new();
        };
        let Some(body_faces) = connected_solid_body_faces(ir, &body) else {
            return BTreeMap::new();
        };
        let Some(witnesses) = counterbore_cylinders(ir, &body_faces) else {
            return BTreeMap::new();
        };
        let [witness] = witnesses.as_slice() else {
            return BTreeMap::new();
        };
        placements.insert(
            operation.clone(),
            HolePlacement::Axis {
                origin: witness.line_origin,
                axis: witness.axis,
            },
        );
    }
    placements
}

fn blind_hole_axis_placements_for_operations(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, HolePlacement> {
    if operations.is_empty() || operations.iter().collect::<BTreeSet<_>>().len() != operations.len()
    {
        return BTreeMap::new();
    }
    let Some(operations_by_body) = hole_operations_by_body(ir, operations, outputs) else {
        return BTreeMap::new();
    };
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return BTreeMap::new();
        };
        let Some(body_faces) = connected_solid_body_faces(ir, &body) else {
            return BTreeMap::new();
        };
        let Some(witnesses) = blind_bore_cylinders(ir, &body_faces) else {
            return BTreeMap::new();
        };
        let [witness] = witnesses.as_slice() else {
            return BTreeMap::new();
        };
        placements.insert(
            operation.clone(),
            HolePlacement::Directed {
                position: witness.position,
                direction: witness.direction,
            },
        );
    }
    placements
}

fn hole_axis_placements_for_body(ir: &CadIr, body: &BodyId) -> Vec<HolePlacement> {
    let Some(body_faces) = connected_solid_body_faces(ir, body) else {
        return Vec::new();
    };
    let Some(bores) = through_bore_cylinders(ir, &body_faces) else {
        return Vec::new();
    };
    let angular_tolerance = ir.tolerances.angular.max(1e-12);
    let mut placements = Vec::new();
    for (origin, axis, _) in bores {
        let Some(mut axis) = unit_vector(axis) else {
            return Vec::new();
        };
        let Some(leading) = [axis.x, axis.y, axis.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)
        else {
            return Vec::new();
        };
        if leading < 0.0 {
            axis = Vector3::new(-axis.x, -axis.y, -axis.z);
        }
        let axial_offset = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
        let origin = Point3::new(
            origin.x - axial_offset * axis.x,
            origin.y - axial_offset * axis.y,
            origin.z - axial_offset * axis.z,
        );
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
            return Vec::new();
        }
        placements.push(HolePlacement::Axis { origin, axis });
    }
    placements.sort_by_key(hole_placement_key);
    placements
}

fn hole_placement_key(placement: &HolePlacement) -> [u64; 6] {
    let HolePlacement::Axis { origin, axis } = placement else {
        return [0; 6];
    };
    [
        origin.x.to_bits(),
        origin.y.to_bits(),
        origin.z.to_bits(),
        axis.x.to_bits(),
        axis.y.to_bits(),
        axis.z.to_bits(),
    ]
}

#[derive(Clone, Debug)]
struct CylindricalFaceWitness {
    line_origin: Point3,
    axis: Vector3,
    radius: f64,
    stations: [f64; 2],
    loop_ids: [LoopId; 2],
}

#[derive(Clone, Copy)]
struct CounterboreCylinderWitness {
    line_origin: Point3,
    axis: Vector3,
    bore_radius: f64,
    counterbore_radius: f64,
    depth: f64,
}

#[derive(Clone, Copy)]
struct BlindBoreCylinderWitness {
    position: Point3,
    direction: Vector3,
    bore_radius: f64,
    depth: f64,
}

fn canonical_axis(axis: Vector3, angular_tolerance: f64) -> Option<Vector3> {
    let mut axis = unit_vector(axis)?;
    let leading = [axis.x, axis.y, axis.z]
        .into_iter()
        .find(|component| component.abs() > angular_tolerance)?;
    if leading < 0.0 {
        axis = Vector3::new(-axis.x, -axis.y, -axis.z);
    }
    Some(axis)
}

fn circular_loop_geometry(
    loop_id: &LoopId,
    coedges_by_loop: &BTreeMap<&LoopId, Vec<&Coedge>>,
    edges: &BTreeMap<&EdgeId, Option<&CurveId>>,
    curves: &BTreeMap<&CurveId, &CurveGeometry>,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> Option<(Point3, Vector3, f64)> {
    let coedges = coedges_by_loop.get(loop_id)?;
    if coedges.is_empty() {
        return None;
    }
    let mut witness: Option<(Point3, Vector3, f64)> = None;
    for coedge in coedges {
        let curve_id = edges.get(&coedge.edge).copied().flatten()?;
        let CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        } = curves.get(curve_id)?
        else {
            return None;
        };
        let axis = canonical_axis(*axis, angular_tolerance)?;
        if ![center.x, center.y, center.z, *radius]
            .into_iter()
            .all(f64::is_finite)
            || *radius <= 0.0
        {
            return None;
        }
        if let Some((previous_center, previous_axis, previous_radius)) = witness {
            if (radius - previous_radius).abs() > linear_tolerance
                || (1.0 - dot_vector(axis, previous_axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - previous_center.x,
                    center.y - previous_center.y,
                    center.z - previous_center.z,
                )
                .norm()
                    > linear_tolerance
            {
                return None;
            }
        }
        witness = Some((*center, axis, *radius));
    }
    witness
}

fn loop_edge_ids(
    loop_id: &LoopId,
    coedges_by_loop: &BTreeMap<&LoopId, Vec<&Coedge>>,
) -> Option<BTreeSet<EdgeId>> {
    let coedges = coedges_by_loop.get(loop_id)?;
    (!coedges.is_empty()).then(|| {
        coedges
            .iter()
            .map(|coedge| coedge.edge.clone())
            .collect::<BTreeSet<_>>()
    })
}

fn cylindrical_face_witnesses(
    ir: &CadIr,
    body_faces: &[&Face],
) -> Option<Vec<CylindricalFaceWitness>> {
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (&edge.id, edge.curve.as_ref()))
        .collect::<BTreeMap<_, _>>();
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let mut coedges_by_loop = BTreeMap::<&LoopId, Vec<&Coedge>>::new();
    for coedge in &ir.model.coedges {
        coedges_by_loop
            .entry(&coedge.owner_loop)
            .or_default()
            .push(coedge);
    }
    let linear_tolerance = ir.tolerances.linear.max(1e-9);
    let angular_tolerance = ir.tolerances.angular.max(1e-12);
    let mut witnesses = Vec::new();
    for face in body_faces
        .iter()
        .copied()
        .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
    {
        let Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        }) = surfaces.get(&face.surface)
        else {
            continue;
        };
        if ![origin.x, origin.y, origin.z, *radius]
            .into_iter()
            .all(f64::is_finite)
            || *radius <= 0.0
        {
            return None;
        }
        let axis = canonical_axis(*axis, angular_tolerance)?;
        let axial_offset = dot_vector(Vector3::new(origin.x, origin.y, origin.z), axis);
        let line_origin = Point3::new(
            origin.x - axial_offset * axis.x,
            origin.y - axial_offset * axis.y,
            origin.z - axial_offset * axis.z,
        );
        let [first_loop, second_loop] = face.loops.as_slice() else {
            return None;
        };
        let mut stations = Vec::with_capacity(2);
        for loop_id in &face.loops {
            let (center, circle_axis, circle_radius) = circular_loop_geometry(
                loop_id,
                &coedges_by_loop,
                &edges,
                &curves,
                linear_tolerance,
                angular_tolerance,
            )?;
            if (circle_radius - *radius).abs() > linear_tolerance
                || (1.0 - dot_vector(axis, circle_axis).abs()) > angular_tolerance
                || cross_vector(
                    Vector3::new(
                        center.x - origin.x,
                        center.y - origin.y,
                        center.z - origin.z,
                    ),
                    axis,
                )
                .norm()
                    > linear_tolerance
            {
                return None;
            }
            let station = dot_vector(Vector3::new(center.x, center.y, center.z), axis);
            if !station.is_finite() {
                return None;
            }
            stations.push(station);
        }
        let [first, second] = stations.as_slice() else {
            return None;
        };
        if (first - second).abs() <= linear_tolerance {
            return None;
        }
        witnesses.push(CylindricalFaceWitness {
            line_origin,
            axis,
            radius: *radius,
            stations: [*first, *second],
            loop_ids: [first_loop.clone(), second_loop.clone()],
        });
    }
    Some(witnesses)
}

fn plane_annulus_witness(
    ir: &CadIr,
    body_faces: &[&Face],
    small: &CylindricalFaceWitness,
    small_station_ordinal: usize,
    large: &CylindricalFaceWitness,
    large_station_ordinal: usize,
) -> bool {
    let line_origin = small.line_origin;
    let axis = small.axis;
    let station = small.stations[small_station_ordinal];
    let inner_radius = small.radius;
    let outer_radius = large.radius;
    let inner_loop = &small.loop_ids[small_station_ordinal];
    let outer_loop = &large.loop_ids[large_station_ordinal];
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (&edge.id, edge.curve.as_ref()))
        .collect::<BTreeMap<_, _>>();
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let mut coedges_by_loop = BTreeMap::<&LoopId, Vec<&Coedge>>::new();
    for coedge in &ir.model.coedges {
        coedges_by_loop
            .entry(&coedge.owner_loop)
            .or_default()
            .push(coedge);
    }
    let linear_tolerance = ir.tolerances.linear.max(1e-9);
    let angular_tolerance = ir.tolerances.angular.max(1e-12);
    let mut matches = 0;
    for face in body_faces {
        if face.loops.len() != 2 {
            continue;
        }
        let Some(SurfaceGeometry::Plane { origin, normal, .. }) = surfaces.get(&face.surface)
        else {
            continue;
        };
        let Some(normal) = canonical_axis(*normal, angular_tolerance) else {
            continue;
        };
        if (1.0 - dot_vector(normal, axis).abs()) > angular_tolerance
            || (dot_vector(
                Vector3::new(
                    origin.x - line_origin.x,
                    origin.y - line_origin.y,
                    origin.z - line_origin.z,
                ),
                axis,
            ) - station)
                .abs()
                > linear_tolerance
        {
            continue;
        }
        let mut boundaries = Vec::with_capacity(2);
        let mut valid = true;
        for loop_id in &face.loops {
            let Some((center, circle_axis, radius)) = circular_loop_geometry(
                loop_id,
                &coedges_by_loop,
                &edges,
                &curves,
                linear_tolerance,
                angular_tolerance,
            ) else {
                valid = false;
                break;
            };
            if (1.0 - dot_vector(circle_axis, normal).abs()) > angular_tolerance
                || (dot_vector(
                    Vector3::new(
                        center.x - origin.x,
                        center.y - origin.y,
                        center.z - origin.z,
                    ),
                    normal,
                ))
                .abs()
                    > linear_tolerance
                || cross_vector(
                    Vector3::new(
                        center.x - line_origin.x,
                        center.y - line_origin.y,
                        center.z - line_origin.z,
                    ),
                    axis,
                )
                .norm()
                    > linear_tolerance
                || (dot_vector(Vector3::new(center.x, center.y, center.z), axis) - station).abs()
                    > linear_tolerance
            {
                valid = false;
                break;
            }
            boundaries.push((radius, loop_id.clone()));
        }
        if !valid {
            continue;
        }
        boundaries.sort_by(|(first, _), (second, _)| first.total_cmp(second));
        let [(inner, inner_boundary), (outer, outer_boundary)] = boundaries.as_slice() else {
            continue;
        };
        if (inner - inner_radius).abs() <= linear_tolerance
            && (outer - outer_radius).abs() <= linear_tolerance
            && loop_edge_ids(inner_boundary, &coedges_by_loop)
                == loop_edge_ids(inner_loop, &coedges_by_loop)
            && loop_edge_ids(outer_boundary, &coedges_by_loop)
                == loop_edge_ids(outer_loop, &coedges_by_loop)
        {
            matches += 1;
        }
    }
    matches == 1
}

fn counterbore_cylinders(
    ir: &CadIr,
    body_faces: &[&Face],
) -> Option<Vec<CounterboreCylinderWitness>> {
    let cylinders = cylindrical_face_witnesses(ir, body_faces)?;
    if cylinders.is_empty() || cylinders.len() % 2 != 0 {
        return None;
    }
    let linear_tolerance = ir.tolerances.linear.max(1e-9);
    let angular_tolerance = ir.tolerances.angular.max(1e-12);
    let mut candidates = vec![Vec::<(usize, CounterboreCylinderWitness)>::new(); cylinders.len()];
    for (first_index, first) in cylinders.iter().enumerate() {
        for (second_index, second) in cylinders.iter().enumerate().skip(first_index + 1) {
            let (small, large) = if first.radius < second.radius {
                (first, second)
            } else {
                (second, first)
            };
            if large.radius - small.radius <= linear_tolerance
                || (1.0 - dot_vector(small.axis, large.axis).abs()) > angular_tolerance
                || cross_vector(
                    Vector3::new(
                        large.line_origin.x - small.line_origin.x,
                        large.line_origin.y - small.line_origin.y,
                        large.line_origin.z - small.line_origin.z,
                    ),
                    small.axis,
                )
                .norm()
                    > linear_tolerance
            {
                continue;
            }
            let mut common = Vec::new();
            for (small_ordinal, small_station) in small.stations.iter().enumerate() {
                for (large_ordinal, large_station) in large.stations.iter().enumerate() {
                    if (small_station - large_station).abs() <= linear_tolerance {
                        common.push((small_ordinal, large_ordinal, *small_station));
                    }
                }
            }
            let [(small_shared, large_shared, shared_station)] = common.as_slice() else {
                continue;
            };
            let small_other = small.stations[1 - small_shared];
            let large_other = large.stations[1 - large_shared];
            let depth = (large_other - shared_station).abs();
            if depth <= linear_tolerance
                || (small_other - shared_station).abs() <= linear_tolerance
                || !plane_annulus_witness(
                    ir,
                    body_faces,
                    small,
                    *small_shared,
                    large,
                    *large_shared,
                )
            {
                continue;
            }
            let witness = CounterboreCylinderWitness {
                line_origin: small.line_origin,
                axis: small.axis,
                bore_radius: small.radius,
                counterbore_radius: large.radius,
                depth,
            };
            candidates[first_index].push((second_index, witness));
            candidates[second_index].push((first_index, witness));
        }
    }
    if candidates.iter().any(|candidates| candidates.len() != 1) {
        return None;
    }
    let mut witnesses = Vec::with_capacity(cylinders.len() / 2);
    let mut used = vec![false; cylinders.len()];
    for first_index in 0..cylinders.len() {
        if used[first_index] {
            continue;
        }
        let (second_index, witness) = candidates[first_index][0];
        if used[second_index]
            || candidates[second_index][0].0 != first_index
            || first_index == second_index
        {
            return None;
        }
        used[first_index] = true;
        used[second_index] = true;
        witnesses.push(witness);
    }
    Some(witnesses)
}

/// Identify one blind bore from its unique planar termination. The cylinder
/// boundary and cap loop must share the exact edge identities; a radius or
/// station match alone is not a topology relation.
fn blind_bore_cylinders(ir: &CadIr, body_faces: &[&Face]) -> Option<Vec<BlindBoreCylinderWitness>> {
    let cylinders = cylindrical_face_witnesses(ir, body_faces)?;
    let [cylinder] = cylinders.as_slice() else {
        return None;
    };
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (&edge.id, edge.curve.as_ref()))
        .collect::<BTreeMap<_, _>>();
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let mut coedges_by_loop = BTreeMap::<&LoopId, Vec<&Coedge>>::new();
    for coedge in &ir.model.coedges {
        coedges_by_loop
            .entry(&coedge.owner_loop)
            .or_default()
            .push(coedge);
    }
    let linear_tolerance = ir.tolerances.linear.max(1e-9);
    let angular_tolerance = ir.tolerances.angular.max(1e-12);
    let mut cap_stations = Vec::new();
    for (station_ordinal, station) in cylinder.stations.iter().enumerate() {
        let cylinder_loop = &cylinder.loop_ids[station_ordinal];
        let cylinder_edges = loop_edge_ids(cylinder_loop, &coedges_by_loop)?;
        for face in body_faces {
            let [cap_loop] = face.loops.as_slice() else {
                continue;
            };
            if loop_edge_ids(cap_loop, &coedges_by_loop) != Some(cylinder_edges.clone()) {
                continue;
            }
            let Some(SurfaceGeometry::Plane { origin, normal, .. }) = surfaces.get(&face.surface)
            else {
                continue;
            };
            let Some(normal) = canonical_axis(*normal, angular_tolerance) else {
                continue;
            };
            let Some((center, circle_axis, circle_radius)) = circular_loop_geometry(
                cap_loop,
                &coedges_by_loop,
                &edges,
                &curves,
                linear_tolerance,
                angular_tolerance,
            ) else {
                continue;
            };
            if (circle_radius - cylinder.radius).abs() > linear_tolerance
                || (1.0 - dot_vector(circle_axis, normal).abs()) > angular_tolerance
                || (1.0 - dot_vector(normal, cylinder.axis).abs()) > angular_tolerance
                || cross_vector(
                    Vector3::new(
                        center.x - cylinder.line_origin.x,
                        center.y - cylinder.line_origin.y,
                        center.z - cylinder.line_origin.z,
                    ),
                    cylinder.axis,
                )
                .norm()
                    > linear_tolerance
                || (dot_vector(Vector3::new(center.x, center.y, center.z), cylinder.axis)
                    - *station)
                    .abs()
                    > linear_tolerance
                || (dot_vector(Vector3::new(origin.x, origin.y, origin.z), cylinder.axis)
                    - *station)
                    .abs()
                    > linear_tolerance
            {
                continue;
            }
            cap_stations.push((station_ordinal, *station));
        }
    }
    let [(cap_ordinal, cap_station)] = cap_stations.as_slice() else {
        return None;
    };
    let entry_ordinal = 1 - *cap_ordinal;
    let entry_station = cylinder.stations[entry_ordinal];
    let depth = (*cap_station - entry_station).abs();
    if !depth.is_finite() || depth <= linear_tolerance {
        return None;
    }
    let position = Point3::new(
        cylinder.line_origin.x + entry_station * cylinder.axis.x,
        cylinder.line_origin.y + entry_station * cylinder.axis.y,
        cylinder.line_origin.z + entry_station * cylinder.axis.z,
    );
    let direction = if *cap_station > entry_station {
        cylinder.axis
    } else {
        Vector3::new(-cylinder.axis.x, -cylinder.axis.y, -cylinder.axis.z)
    };
    Some(vec![BlindBoreCylinderWitness {
        position,
        direction,
        bore_radius: cylinder.radius,
        depth,
    }])
}

/// Resolve hole operations to their explicit output bodies, or to the one
/// connected solid when NX omits every operation-output relation.
fn hole_operations_by_body(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Option<BTreeMap<BodyId, Vec<String>>> {
    let explicit = operations
        .iter()
        .filter(|operation| {
            outputs
                .get(*operation)
                .is_some_and(|bodies| !bodies.is_empty())
        })
        .count();
    if explicit != 0 && explicit != operations.len() {
        return None;
    }
    if explicit == operations.len() {
        let mut operations_by_body = BTreeMap::<BodyId, Vec<String>>::new();
        for operation in operations {
            let [body] = outputs.get(operation)?.as_slice() else {
                return None;
            };
            operations_by_body
                .entry(body.clone())
                .or_default()
                .push(operation.clone());
        }
        return Some(operations_by_body);
    }

    let mut connected_solids = ir
        .model
        .bodies
        .iter()
        .filter(|body| connected_solid_body_faces(ir, &body.id).is_some());
    let body = connected_solids.next()?;
    if connected_solids.next().is_some() {
        return None;
    }
    Some(BTreeMap::from([(body.id.clone(), operations.to_vec())]))
}

fn through_bore_cylinders(ir: &CadIr, body_faces: &[&Face]) -> Option<Vec<(Point3, Vector3, f64)>> {
    Some(
        cylindrical_face_witnesses(ir, body_faces)?
            .into_iter()
            .map(|witness| (witness.line_origin, witness.axis, witness.radius))
            .collect(),
    )
}

/// Derive identical entry and exit chamfer treatments only when every simple
/// through-hole bore has exactly two coaxial conical faces and every cone is
/// bounded by the bore circle and one equal larger circle.
fn simple_hole_chamfers(
    ir: &CadIr,
    templates: &[crate::native::features::FeatureSimpleHoleTemplate],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, HoleKind> {
    let template_counts = templates
        .iter()
        .fold(BTreeMap::new(), |mut counts, template| {
            *counts
                .entry(template.operation_label.as_str())
                .or_insert(0usize) += 1;
            counts
        });
    let operations = templates
        .iter()
        .filter(|template| {
            template.form == crate::native::features::SimpleHoleForm::Simple
                && template.extent == crate::native::features::SimpleHoleExtent::Through
                && template.start_treatment
                    == crate::native::features::SimpleHoleEndTreatment::Chamfer
                && template.end_treatment
                    == crate::native::features::SimpleHoleEndTreatment::Chamfer
        })
        .filter(|template| template_counts.get(template.operation_label.as_str()) == Some(&1))
        .map(|template| template.operation_label.clone())
        .collect::<BTreeSet<_>>();
    if operations.is_empty() {
        return BTreeMap::new();
    }
    let operations = operations.into_iter().collect::<Vec<_>>();
    let Some(operations_by_body) = hole_operations_by_body(ir, &operations, outputs) else {
        return BTreeMap::new();
    };

    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (&edge.id, edge.curve.as_ref()))
        .collect::<BTreeMap<_, _>>();
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let mut coedges_by_loop = BTreeMap::<&LoopId, Vec<&Coedge>>::new();
    for coedge in &ir.model.coedges {
        coedges_by_loop
            .entry(&coedge.owner_loop)
            .or_default()
            .push(coedge);
    }

    let linear_tolerance = ir.tolerances.linear.max(1e-9);
    let angular_tolerance = ir.tolerances.angular.max(1e-12);
    let mut treatments = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let Some(body_faces) = connected_solid_body_faces(ir, &body) else {
            return BTreeMap::new();
        };
        let Some(bores) = through_bore_cylinders(ir, &body_faces) else {
            return BTreeMap::new();
        };
        let [(_, _, bore_radius), ..] = bores.as_slice() else {
            return BTreeMap::new();
        };
        if bores.len() != operations.len()
            || bores
                .iter()
                .any(|(_, _, radius)| radius.to_bits() != bore_radius.to_bits())
        {
            return BTreeMap::new();
        }
        let mut cone_counts = vec![0usize; bores.len()];
        let mut outer_radii = Vec::new();
        let mut included_angles = Vec::new();
        for face in body_faces
            .into_iter()
            .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
        {
            let Some(SurfaceGeometry::Cone {
                origin,
                axis,
                half_angle,
                ..
            }) = surfaces.get(&face.surface).copied()
            else {
                continue;
            };
            if !half_angle.is_finite()
                || *half_angle <= 0.0
                || *half_angle >= std::f64::consts::FRAC_PI_2
            {
                return BTreeMap::new();
            }
            let matching_bores = bores
                .iter()
                .enumerate()
                .filter_map(|(ordinal, (bore_origin, bore_axis, _))| {
                    let dot = axis.dot(*bore_axis);
                    if (1.0 - dot.abs()) > angular_tolerance {
                        return None;
                    }
                    let delta = Vector3::new(
                        origin.x - bore_origin.x,
                        origin.y - bore_origin.y,
                        origin.z - bore_origin.z,
                    );
                    let cross = delta.cross(*bore_axis);
                    (cross.norm() <= linear_tolerance).then_some(ordinal)
                })
                .collect::<Vec<_>>();
            let [bore_ordinal] = matching_bores.as_slice() else {
                return BTreeMap::new();
            };
            cone_counts[*bore_ordinal] += 1;

            let mut radii = face
                .loops
                .iter()
                .flat_map(|loop_id| coedges_by_loop.get(loop_id).into_iter().flatten())
                .filter_map(|coedge| edges.get(&coedge.edge).copied().flatten())
                .filter_map(|curve_id| match curves.get(curve_id)? {
                    CurveGeometry::Circle { radius, .. } if radius.is_finite() && *radius > 0.0 => {
                        Some(*radius)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            radii.sort_by(f64::total_cmp);
            let [inner, outer] = radii.as_slice() else {
                return BTreeMap::new();
            };
            if inner.to_bits() != bore_radius.to_bits() || outer <= inner {
                return BTreeMap::new();
            }
            outer_radii.push(*outer);
            included_angles.push(half_angle * 2.0);
        }
        if cone_counts.iter().any(|count| *count != 2)
            || outer_radii.len() != bores.len() * 2
            || included_angles.len() != outer_radii.len()
        {
            return BTreeMap::new();
        }
        outer_radii.sort_by(f64::total_cmp);
        included_angles.sort_by(f64::total_cmp);
        if outer_radii.last().expect("nonempty") - outer_radii[0] > linear_tolerance
            || included_angles.last().expect("nonempty") - included_angles[0] > angular_tolerance
        {
            return BTreeMap::new();
        }
        let treatment = HoleKind::Chamfer {
            diameter: Length(2.0 * outer_radii.iter().sum::<f64>() / outer_radii.len() as f64),
            angle: Angle(included_angles.iter().sum::<f64>() / included_angles.len() as f64),
        };
        treatments.extend(
            operations
                .into_iter()
                .map(|operation| (operation, treatment)),
        );
    }
    treatments
}

fn unique_simple_hole_template(
    payload_strings: &[&str],
) -> Option<(
    crate::native::features::SimpleHoleFamily,
    crate::native::features::SimpleHoleForm,
    crate::native::features::SimpleHoleExtent,
    crate::native::features::SimpleHoleEndTreatment,
    crate::native::features::SimpleHoleEndTreatment,
)> {
    let mut candidates = payload_strings
        .iter()
        .copied()
        .filter(|value| value.starts_with("Hole_"));
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    crate::native::features::parse_simple_hole_template(candidate)
}

/// Identity namespace used to prove that two Boolean selections are disjoint.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FeatureBodyIdentity {
    Segment(u32),
    OffsetStore(String),
}

fn offset_store_identity(data_block: &str) -> Option<&str> {
    data_block
        .strip_prefix("nx:om-data-blocks-")
        .and_then(|data_block| data_block.split_once(":block#"))
        .map(|(store, _)| store)
}

struct FeatureBodySelection {
    selection: BodySelection,
    identity_keys: Option<Vec<FeatureBodyIdentity>>,
}

/// Resolve a complete object-index selection only when every alias root owns one
/// decoded body image. Retain the complete feature-input-local identities when
/// current topology cannot represent a consumed historical body. An offset-store
/// selection uses the exact data-block identities from its feature-history
/// section, but never crosses into the segment-body identity namespace.
fn feature_body_selection(
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> FeatureBodySelection {
    feature_body_selection_with_offset_blocks(
        object_indices,
        body_alias_roots,
        &BTreeMap::new(),
        bodies_by_object_index,
        native,
    )
}

fn feature_body_selection_with_offset_blocks(
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    offset_store_body_blocks: &BTreeMap<u32, String>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> FeatureBodySelection {
    let mut roots = Vec::new();
    let mut offset_blocks = Vec::new();
    for index in object_indices {
        match (
            body_alias_roots.get(index),
            offset_store_body_blocks.get(index),
        ) {
            (Some(_), Some(_)) => {
                // The same integer in both stores has no operation-local
                // namespace proof; integer equality cannot choose a body.
                return FeatureBodySelection {
                    selection: BodySelection::Native(native),
                    identity_keys: None,
                };
            }
            (Some(root), None) => {
                if !roots.contains(root) {
                    roots.push(*root);
                }
            }
            (None, Some(data_block)) => {
                if !offset_blocks.contains(data_block) {
                    offset_blocks.push(data_block.clone());
                }
            }
            (None, None) => {
                return FeatureBodySelection {
                    selection: BodySelection::Native(native),
                    identity_keys: None,
                };
            }
        }
    }
    if !roots.is_empty() && !offset_blocks.is_empty() {
        return FeatureBodySelection {
            selection: BodySelection::Native(native),
            identity_keys: None,
        };
    }
    let offset_store = offset_blocks
        .first()
        .and_then(|block| offset_store_identity(block));
    if !offset_blocks.is_empty()
        && (offset_store.is_none()
            || offset_blocks
                .iter()
                .any(|block| offset_store_identity(block) != offset_store))
    {
        return FeatureBodySelection {
            selection: BodySelection::Native(native),
            identity_keys: None,
        };
    }
    if !offset_blocks.is_empty() {
        return FeatureBodySelection {
            selection: BodySelection::Local {
                bodies: offset_blocks.clone(),
                native,
            },
            identity_keys: Some(
                offset_blocks
                    .into_iter()
                    .map(FeatureBodyIdentity::OffsetStore)
                    .collect(),
            ),
        };
    }
    let resolved = roots
        .iter()
        .map(|root| {
            let [body] = bodies_by_object_index.get(root)?.as_slice() else {
                return None;
            };
            Some(body.clone())
        })
        .collect::<Option<Vec<_>>>();
    if let Some(bodies) =
        resolved.filter(|bodies| bodies.iter().collect::<BTreeSet<_>>().len() == bodies.len())
    {
        return FeatureBodySelection {
            selection: BodySelection::Resolved { bodies, native },
            identity_keys: Some(
                roots
                    .into_iter()
                    .map(FeatureBodyIdentity::Segment)
                    .collect(),
            ),
        };
    }
    FeatureBodySelection {
        selection: BodySelection::Local {
            bodies: roots
                .iter()
                .map(|root| format!("nx:om-body-object#{root}"))
                .collect(),
            native,
        },
        identity_keys: Some(
            roots
                .into_iter()
                .map(FeatureBodyIdentity::Segment)
                .collect(),
        ),
    }
}

/// Resolve one complete body set when possible. Otherwise retain every exact
/// input-local object identity. A body set needs no cross-role disjointness
/// proof, so an identity outside the segment alias table remains its own root.
fn feature_body_set_selection(
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> BodySelection {
    let mut roots = Vec::new();
    for index in object_indices {
        let root = body_alias_roots.get(index).copied().unwrap_or(*index);
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    let resolved = roots
        .iter()
        .map(|root| {
            let [body] = bodies_by_object_index.get(root)?.as_slice() else {
                return None;
            };
            Some(body.clone())
        })
        .collect::<Option<Vec<_>>>();
    if let Some(bodies) =
        resolved.filter(|bodies| bodies.iter().collect::<BTreeSet<_>>().len() == bodies.len())
    {
        return BodySelection::Resolved { bodies, native };
    }
    BodySelection::Local {
        bodies: roots
            .iter()
            .map(|root| format!("nx:om-body-object#{root}"))
            .collect(),
        native,
    }
}

fn atomic_disjoint_body_selections(
    left: FeatureBodySelection,
    right: FeatureBodySelection,
) -> (BodySelection, BodySelection) {
    let complete = left.identity_keys.as_ref().is_some_and(|left| {
        right.identity_keys.as_ref().is_some_and(|right| {
            let same_namespace =
                left.first()
                    .zip(right.first())
                    .is_none_or(|(left, right)| match (left, right) {
                        (FeatureBodyIdentity::Segment(_), FeatureBodyIdentity::Segment(_)) => true,
                        (
                            FeatureBodyIdentity::OffsetStore(left),
                            FeatureBodyIdentity::OffsetStore(right),
                        ) => offset_store_identity(left) == offset_store_identity(right),
                        _ => false,
                    });
            same_namespace && !left.iter().any(|key| right.contains(key))
        })
    });
    let left = left.selection;
    let right = right.selection;
    if complete {
        return (left, right);
    }
    let native = |selection: BodySelection| match selection {
        BodySelection::Resolved { native, .. }
        | BodySelection::Local { native, .. }
        | BodySelection::Native(native) => BodySelection::Native(native),
        BodySelection::ResolvedSet { native, .. } => BodySelection::NativeSet(native),
        BodySelection::NativeSet(members) => BodySelection::NativeSet(members),
        BodySelection::Bodies(bodies) => BodySelection::Bodies(bodies),
        BodySelection::Generated { .. }
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Unresolved => BodySelection::Unresolved,
    };
    (native(left), native(right))
}

/// Resolve one Boolean participant through the namespace selected by the
/// complete Boolean definition. Native integer identity is used only when the
/// definition did not establish one exact offset-store selection.
fn boolean_participant_writer<'a>(
    selection: &BodySelection,
    object_index: u32,
    offset_store_body_blocks: Option<&BTreeMap<u32, String>>,
    body_alias_roots: &BTreeMap<u32, u32>,
    history: &'a BodyWriterHistory,
) -> Option<&'a FeatureId> {
    let offset_store_selection = matches!(
        selection,
        BodySelection::Local { bodies, .. }
            if !bodies.is_empty()
                && bodies
                    .iter()
                    .all(|body| offset_store_identity(body).is_some())
    );
    if offset_store_selection {
        return offset_store_body_blocks
            .and_then(|blocks| blocks.get(&object_index))
            .and_then(|data_block| history.offset_store_writer(data_block));
    }
    history.native_writer(
        body_alias_roots
            .get(&object_index)
            .copied()
            .unwrap_or(object_index),
    )
}

/// Register a Boolean's target in the namespace established by its complete
/// target selection. An offset-store target must not create a native writer
/// for the same integer object index.
fn boolean_target_writer(
    definition: &FeatureDefinition,
    native_body: u32,
) -> (Option<u32>, Option<&str>) {
    if let FeatureDefinition::Combine {
        target: BodySelection::Local { bodies, .. },
        ..
    } = definition
    {
        if let [body] = bodies.as_slice() {
            if offset_store_identity(body).is_some() {
                return (None, Some(body.as_str()));
            }
        }
    }
    (Some(native_body), None)
}

pub(crate) fn boolean_feature_definition(
    operation: &crate::native::features::FeatureBooleanOperation,
    body_alias_roots: &BTreeMap<u32, u32>,
    offset_store_resolution: &BooleanOffsetStoreResolution,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> FeatureDefinition {
    let empty_offset_store_body_blocks = BTreeMap::new();
    let native_target = format!("nx:om-object-index#{}", operation.target_object_index);
    let native_tools = format!(
        "nx:om-object-indices#{}",
        operation
            .tool_object_indices
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let (target, tools) = match offset_store_resolution {
        BooleanOffsetStoreResolution::Unresolved => (
            BodySelection::Native(native_target),
            BodySelection::Native(native_tools),
        ),
        BooleanOffsetStoreResolution::None | BooleanOffsetStoreResolution::Complete(_) => {
            let offset_store_body_blocks = match offset_store_resolution {
                BooleanOffsetStoreResolution::Complete(blocks) => blocks,
                BooleanOffsetStoreResolution::None => &empty_offset_store_body_blocks,
                BooleanOffsetStoreResolution::Unresolved => unreachable!("matched above"),
            };
            atomic_disjoint_body_selections(
                feature_body_selection_with_offset_blocks(
                    &[operation.target_object_index],
                    body_alias_roots,
                    offset_store_body_blocks,
                    bodies_by_object_index,
                    native_target.clone(),
                ),
                feature_body_selection_with_offset_blocks(
                    &operation.tool_object_indices,
                    body_alias_roots,
                    offset_store_body_blocks,
                    bodies_by_object_index,
                    native_tools.clone(),
                ),
            )
        }
    };
    FeatureDefinition::Combine {
        target,
        tools,
        op: match operation.kind {
            crate::native::features::FeatureBooleanKind::Unite => BooleanOp::Join,
            crate::native::features::FeatureBooleanKind::Subtract => BooleanOp::Cut,
            crate::native::features::FeatureBooleanKind::Intersect => BooleanOp::Intersect,
        },
        keep_tools: false,
    }
}

/// Project `DELETE` as body deletion only when its bounded operation record
/// carries a primary-body field. Other `DELETE` payloads target a different
/// object family and remain native until that family is decoded.
fn delete_body_feature_definition(
    body_object_index: Option<u32>,
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Option<FeatureDefinition> {
    let body = body_object_index?;
    let selection = feature_body_selection(
        &[body],
        body_alias_roots,
        bodies_by_object_index,
        format!("nx:om-object-index#{body}"),
    )
    .selection;
    let bodies = match selection {
        BodySelection::Native(native) => BodySelection::Local {
            bodies: vec![format!("nx:om-body-object#{body}")],
            native,
        },
        selection => selection,
    };
    Some(FeatureDefinition::DeleteBody {
        // A typed DELETE primary-body field names one exact feature input. It
        // needs no cross-selection alias proof when it has no segment binding.
        bodies,
        mode: BodyRetentionMode::DeleteSelected,
    })
}

/// Project exact feature-local input-store identities for a trim target and
/// every complete, distinct tool operand. Retained-side semantics stay unresolved.
fn offset_store_trim_body_feature_definition(
    offset_store_bodies: &[(u32, String)],
    operands: &[&crate::native::features::FeatureOperationBodyOperand],
) -> Option<FeatureDefinition> {
    let [(object_index, data_block)] = offset_store_bodies else {
        return None;
    };
    let tools = if operands.is_empty()
        || operands.iter().any(|operand| {
            operand.body_object_index != *object_index
                || operand.operand_object_index == *object_index
                || operand.operand_data_block.is_none()
        }) {
        BodySelection::Unresolved
    } else {
        let mut operand_indices = BTreeSet::new();
        if operands
            .iter()
            .any(|operand| !operand_indices.insert(operand.operand_object_index))
        {
            BodySelection::Unresolved
        } else {
            let tool_data_blocks = operands
                .iter()
                .map(|operand| operand.operand_data_block.clone())
                .collect::<Option<Vec<_>>>()?;
            BodySelection::Local {
                bodies: tool_data_blocks,
                native: format!(
                    "nx:om-object-indices#{}",
                    operands
                        .iter()
                        .map(|operand| operand.operand_object_index.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            }
        }
    };
    Some(FeatureDefinition::TrimBodies {
        targets: BodySelection::Local {
            bodies: vec![data_block.clone()],
            native: format!("nx:om-object-index#{object_index}"),
        },
        tools,
        keep: BodyTrimSide::Unresolved,
    })
}

fn sew_body_feature_definition(
    primary_segment_body_object_index: Option<u32>,
    offset_store_bodies: &[(u32, String)],
    operands: &[&crate::native::features::FeatureOperationBodyOperand],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Option<FeatureDefinition> {
    if operands.is_empty() {
        return None;
    }
    let primary_offset_store_body = match offset_store_bodies {
        [(object_index, data_block)] => Some((*object_index, data_block.as_str())),
        _ => None,
    };
    let primary_body_object_index = primary_segment_body_object_index
        .or_else(|| primary_offset_store_body.map(|(object_index, _)| object_index))?;
    let object_indices = std::iter::once(primary_body_object_index)
        .chain(operands.iter().map(|operand| operand.operand_object_index))
        .collect::<Vec<_>>();
    let native = format!(
        "nx:om-object-indices#{}",
        object_indices
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let bodies = if primary_segment_body_object_index.is_some() {
        if operands
            .iter()
            .all(|operand| !operand.segment_body_bindings.is_empty())
        {
            feature_body_set_selection(
                &object_indices,
                body_alias_roots,
                bodies_by_object_index,
                native.clone(),
            )
        } else {
            BodySelection::Native(native.clone())
        }
    } else if let Some((primary_object_index, primary_data_block)) = primary_offset_store_body {
        let primary_store = primary_data_block
            .rsplit_once(":block#")
            .map(|(store, _)| store);
        let operand_data_blocks = operands
            .iter()
            .map(|operand| operand.operand_data_block.as_deref())
            .collect::<Option<Vec<_>>>();
        let offset_store_participants = operand_data_blocks.as_ref().filter(|blocks| {
            operands
                .iter()
                .all(|operand| operand.body_object_index == primary_object_index)
                && blocks.iter().all(|block| {
                    block
                        .rsplit_once(":block#")
                        .is_some_and(|(store, _)| Some(store) == primary_store)
                })
                && blocks.iter().collect::<BTreeSet<_>>().len() == blocks.len()
                && !blocks.contains(&primary_data_block)
        });
        if let Some(blocks) = offset_store_participants {
            BodySelection::Local {
                bodies: std::iter::once(primary_data_block.to_string())
                    .chain(blocks.iter().map(|block| (*block).to_string()))
                    .collect(),
                native: native.clone(),
            }
        } else {
            BodySelection::Native(native.clone())
        }
    } else {
        BodySelection::Native(native)
    };
    Some(FeatureDefinition::SewBodies {
        bodies,
        gap_tolerance: None,
    })
}

fn trim_body_feature_definition(
    target_object_index: u32,
    operands: &[&crate::native::features::FeatureOperationBodyOperand],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Option<FeatureDefinition> {
    let tool_object_indices = operands
        .iter()
        .map(|operand| operand.operand_object_index)
        .collect::<Vec<_>>();
    (!tool_object_indices.is_empty()).then(|| {
        let native_target = format!("nx:om-object-index#{target_object_index}");
        let native_tools = format!(
            "nx:om-object-indices#{}",
            tool_object_indices
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        if operands.iter().any(|operand| {
            operand.operand_data_block.is_some() || operand.segment_body_bindings.is_empty()
        }) {
            return FeatureDefinition::TrimBodies {
                targets: BodySelection::Native(native_target),
                tools: BodySelection::Native(native_tools),
                keep: BodyTrimSide::Unresolved,
            };
        }
        let (targets, tools) = atomic_disjoint_body_selections(
            feature_body_selection(
                &[target_object_index],
                body_alias_roots,
                bodies_by_object_index,
                native_target,
            ),
            feature_body_selection(
                &tool_object_indices,
                body_alias_roots,
                bodies_by_object_index,
                native_tools,
            ),
        );
        FeatureDefinition::TrimBodies {
            targets,
            tools,
            keep: BodyTrimSide::Unresolved,
        }
    })
}

fn feature_body_outputs(
    object_index: u32,
    segment_bindings: &[crate::native::segments::SegmentBodyBinding],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Vec<BodyId> {
    if crate::native::segments::unique_segment_body_binding(object_index, segment_bindings)
        .is_none()
    {
        return Vec::new();
    }
    let Some([body]) = bodies_by_object_index.get(&object_index).map(Vec::as_slice) else {
        return Vec::new();
    };
    vec![body.clone()]
}

pub(crate) fn attach_expression_parameters(
    ir: &mut CadIr,
    expressions: &[crate::native::om::Expression],
    declarations: &[crate::native::om::ExpressionDeclaration],
    parameter_uses: &[crate::native::features::FeatureParameterUse],
    annotations: &mut AnnotationBuilder,
) {
    let declarations = declarations
        .iter()
        .map(|declaration| (declaration.id.as_str(), declaration))
        .collect::<BTreeMap<_, _>>();
    let mut tables = BTreeMap::<String, Vec<&crate::native::om::Expression>>::new();
    for expression in expressions {
        let table = if expression.source_table.is_empty() {
            let Some((section, _)) = expression.id.split_once(":expression#") else {
                continue;
            };
            section
        } else {
            expression.source_table.as_str()
        };
        tables
            .entry(table.to_string())
            .or_default()
            .push(expression);
    }
    let stream = annotations.stream("nx:container");
    let mut uses_by_expression =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        uses_by_expression
            .entry(parameter_use.expression.as_str())
            .or_default()
            .push(parameter_use);
    }
    for uses in uses_by_expression.values_mut() {
        uses.sort_by(|first, second| {
            first
                .source_offsets
                .first()
                .cmp(&second.source_offsets.first())
                .then_with(|| first.id.cmp(&second.id))
        });
    }
    let mut tables = tables.into_iter().collect::<Vec<_>>();
    for (_, expressions) in &mut tables {
        expressions.sort_by(|first, second| {
            first
                .source_offset
                .cmp(&second.source_offset)
                .then_with(|| first.id.cmp(&second.id))
        });
    }
    tables.sort_by(|(first_table, first), (second_table, second)| {
        first
            .first()
            .map(|expression| expression.source_offset)
            .cmp(&second.first().map(|expression| expression.source_offset))
            .then_with(|| first_table.cmp(second_table))
    });
    let tables = tables
        .into_iter()
        .map(|(table, mut expressions)| {
            let dependency_ordered_expressions = order_expression_dependencies(&mut expressions);
            (table, expressions, dependency_ordered_expressions)
        })
        .collect::<Vec<_>>();
    let base_ordinal = ir.model.features.len() as u64;
    for (table_ordinal, (table, expressions, dependency_ordered_expressions)) in
        tables.into_iter().enumerate()
    {
        let feature_id = FeatureId(table.split_once(":expression-table#").map_or_else(
            || format!("{table}:feature#equations"),
            |(scope, key)| format!("{scope}:feature#equations-{key}"),
        ));
        let first_offset = expressions
            .iter()
            .map(|expression| expression.source_offset)
            .min()
            .unwrap_or(0);
        annotations
            .note(&feature_id, stream, first_offset)
            .tag("hostglobalvariables");
        annotations.exactness(&feature_id, Exactness::Derived);
        let source_content = expressions
            .iter()
            .filter_map(|expression| {
                expression_parameter_id(&expression.id).map(FeatureSourceContent::Parameter)
            })
            .collect::<Vec<_>>();
        if !source_content.is_empty() {
            annotations.derived(&feature_id, "source_content");
        }
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: base_ordinal + table_ordinal as u64,
            name: Some("NX expressions".to_string()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("hostglobalvariables".to_string()),
            source_text: None,
            source_content,
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        let mut parameter_ids =
            BTreeMap::<(&str, crate::native::om::ExpressionUnit), Vec<ParameterId>>::new();
        for expression in &expressions {
            parameter_ids
                .entry((expression.name.as_str(), expression.unit))
                .or_default()
                .push(
                    expression_parameter_id(&expression.id)
                        .expect("sectioned expressions have parameter identities"),
                );
        }
        for (ordinal, expression) in expressions.into_iter().enumerate() {
            let id = expression_parameter_id(&expression.id)
                .expect("sectioned expressions have parameter identities");
            annotations
                .note(&id.0, stream, expression.source_offset)
                .tag("Number");
            annotations.derived(&id.0, "owner");
            annotations.derived(&id.0, "ordinal");
            annotations.derived(&id.0, "value");
            annotations.derived(&id.0, "native_ref");
            let dependencies = if dependency_ordered_expressions.contains(&expression.id) {
                let mut seen_dependencies = BTreeSet::new();
                crate::native::om::expression_parameter_names(&expression.expression)
                    .into_iter()
                    .filter_map(|name| {
                        let candidates = parameter_ids.get(&(name, expression.unit))?;
                        (candidates.len() == 1).then(|| candidates[0].clone())
                    })
                    .filter(|dependency| seen_dependencies.insert(dependency.clone()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if !dependencies.is_empty() {
                annotations.derived(&id.0, "dependencies");
            }
            let value = expression.value.map(|value| match expression.unit {
                crate::native::om::ExpressionUnit::Millimeter => {
                    ParameterValue::Length(Length(value))
                }
                crate::native::om::ExpressionUnit::Degree => {
                    ParameterValue::Angle(Angle(value.to_radians()))
                }
            });
            let mut properties = BTreeMap::new();
            properties.insert(
                "unit".to_string(),
                match expression.unit {
                    crate::native::om::ExpressionUnit::Millimeter => "millimeter",
                    crate::native::om::ExpressionUnit::Degree => "degree",
                }
                .to_string(),
            );
            annotations.derived(&id.0, "properties");
            if let Some(declaration) = expression
                .declaration
                .as_deref()
                .and_then(|id| declarations.get(id))
            {
                properties.insert("declaration".to_string(), declaration.id.clone());
                properties.insert(
                    "declaration_object_id".to_string(),
                    declaration.object_id.to_string(),
                );
                annotations.derived(&id.0, "properties");
            }
            for (consumer_ordinal, parameter_use) in uses_by_expression
                .get(expression.id.as_str())
                .into_iter()
                .flatten()
                .enumerate()
            {
                properties.insert(
                    format!("consumer.{consumer_ordinal}"),
                    parameter_use
                        .operation_label
                        .replacen("operation-label", "feature", 1),
                );
                properties.insert(
                    format!("parameter_use.{consumer_ordinal}"),
                    parameter_use.id.clone(),
                );
                annotations.derived(&id.0, "properties");
            }
            ir.model.parameters.push(DesignParameter {
                id,
                owner: Some(feature_id.clone()),
                ordinal: ordinal as u32,
                name: expression.name.clone(),
                expression: expression.expression.clone(),
                display: None,
                value,
                dependencies,
                properties,
                pmi: None,
                native_ref: Some(expression.id.clone()),
            });
        }
    }
}

fn order_expression_dependencies(
    expressions: &mut Vec<&crate::native::om::Expression>,
) -> BTreeSet<String> {
    let mut indices_by_name =
        BTreeMap::<(&str, crate::native::om::ExpressionUnit), Vec<usize>>::new();
    for (index, expression) in expressions.iter().enumerate() {
        indices_by_name
            .entry((expression.name.as_str(), expression.unit))
            .or_default()
            .push(index);
    }
    let dependencies = expressions
        .iter()
        .map(|expression| {
            crate::native::om::expression_parameter_names(&expression.expression)
                .into_iter()
                .filter_map(|name| {
                    let [index] = indices_by_name.get(&(name, expression.unit))?.as_slice() else {
                        return None;
                    };
                    Some(*index)
                })
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let mut emitted = BTreeSet::new();
    let mut order = Vec::with_capacity(expressions.len());
    while let Some(index) = (0..expressions.len()).find(|index| {
        !emitted.contains(index)
            && dependencies[*index]
                .iter()
                .all(|dependency| emitted.contains(dependency))
    }) {
        emitted.insert(index);
        order.push(expressions[index]);
    }
    let dependency_ordered_expression_ids = order
        .iter()
        .map(|expression| expression.id.clone())
        .collect();
    order.extend(
        expressions
            .iter()
            .enumerate()
            .filter(|(index, _)| !emitted.contains(index))
            .map(|(_, expression)| *expression),
    );
    *expressions = order;
    dependency_ordered_expression_ids
}

fn attach_block_dimension_parameter_consumers(
    ir: &mut CadIr,
    dimensions: &[crate::native::features::FeatureBlockDimensions],
    annotations: &mut AnnotationBuilder,
) {
    let mut parameters = ir
        .model
        .parameters
        .iter_mut()
        .map(|parameter| (parameter.id.clone(), parameter))
        .collect::<BTreeMap<_, _>>();
    for dimension_set in dimensions {
        let consumer = dimension_set
            .operation_label
            .replacen("operation-label", "feature", 1);
        for (ordinal, expression) in dimension_set.expressions.iter().enumerate() {
            let Some(parameter_id) = expression_parameter_id(expression) else {
                continue;
            };
            let Some(parameter) = parameters.get_mut(&parameter_id) else {
                continue;
            };
            parameter.properties.insert(
                format!("block_dimension.{ordinal}"),
                dimension_set.id.clone(),
            );
            if !parameter
                .properties
                .values()
                .any(|value| value == &consumer)
            {
                let consumer_ordinal = (0..=parameter.properties.len())
                    .find(|candidate| {
                        !parameter
                            .properties
                            .contains_key(&format!("consumer.{candidate}"))
                    })
                    .expect("finite parameter properties have a free consumer ordinal");
                parameter
                    .properties
                    .insert(format!("consumer.{consumer_ordinal}"), consumer.clone());
            }
            annotations.derived(&parameter.id.0, "properties");
        }
    }
}

fn expression_parameter_id(expression_id: &str) -> Option<ParameterId> {
    let (section, key) = expression_id.split_once(":expression#")?;
    Some(ParameterId(format!("{section}:parameter#{key}")))
}

#[cfg(test)]
mod tests;
