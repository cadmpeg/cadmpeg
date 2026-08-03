// SPDX-License-Identifier: Apache-2.0
//! IR-writing attachment of the native object model.
//!
//! This module is the sole IR-mutation surface inside `native/`: it walks the
//! extracted [`NativeModel`], emits source annotations in the legacy note order,
//! serializes each record family into an `nx` namespace arena, and attaches the
//! semantic islands (tessellations, source attributes, feature operations). The
//! IR-free domain modules, `model.rs`, and `catalogue.rs` never write IR.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, ChamferSpec,
    ConfigurationBodies, ConfigurationFeatureState, ConfigurationId, CurveProjectionDirection,
    CurveProjectionDirectionState, DesignConfiguration, DesignParameter, EdgeSelection,
    ExtrudeExtent, ExtrudeSide, FaceSelection, Feature, FeatureDefinition, FeatureId,
    FeatureSourceContent, FeatureTreeNodeRole, HoleForm, HoleKind, HolePlacement, Length,
    ParameterId, ParameterValue, PathRef, PatternKind, ProfileRef, RadiusForm, RadiusSpec,
    RibConstruction, RibDraft, SketchSpace, SweepMode, Termination, ThickenSide, TrimRegion,
};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{AttributeId, BodyId, LoopId, SurfaceId, UnknownId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchPlacement,
};
use cadmpeg_ir::topology::{BodyKind, Coedge, Face, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::EntryContent;
use crate::decode::Scan;
use crate::native::history::{active_feature_closure, BodyWriterHistory};
use crate::native::vector::{cross_vector, dot_vector, unit_vector};

use super::catalogue::NATIVE_CATALOGUE;
use super::display_jt::{display_jt_tessellations, DisplayJtTessellationInputs};
use cadmpeg_ir::native::catalogue::Phase;

pub(crate) fn attach(
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
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
    let object_sections = scan.container.indexed_om_sections();
    if model.is_empty() && object_sections.is_empty() {
        return Ok(());
    }
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
                active: active_attribute_use.is_some(),
                source_index: Some(ordinal as u32),
                name: configuration.name.clone(),
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
    namespace.version = namespace.version.max(181);
    NATIVE_CATALOGUE.emit_all(model, namespace)?;
    Ok(())
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

/// Transfer the complete validated TIFF set atomically. A partial transfer
/// would make native catalog links appear usable when one of their targets is
/// absent from the neutral asset arena.
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
        .filter_map(|(index, configuration)| configuration.active.then_some(index))
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
    annotations.derived(&id, "outputs");
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
    expressions: &[crate::native::om::Expression],
    body_bindings: &[crate::native::segments::SegmentBodyBinding],
    annotations: &mut AnnotationBuilder,
) {
    let labels = features.feature_operation_labels.as_slice();
    let booleans = features.feature_boolean_operations.as_slice();
    let body_references = features.feature_body_references.as_slice();
    let body_data_block_uses = features.feature_body_data_block_uses.as_slice();
    let body_reference_occurrences = features.feature_body_reference_occurrences.as_slice();
    let input_blocks = features.feature_input_blocks.as_slice();
    let input_block_identity_groups = features.feature_input_block_identity_groups.as_slice();
    let datum_csys_constructions = features.feature_datum_csys_constructions.as_slice();
    let datum_csys_payloads = features.feature_datum_csys_payloads.as_slice();
    let datum_csys_block_uses = features.feature_datum_csys_block_uses.as_slice();
    let datum_plane_headers = features.feature_datum_plane_headers.as_slice();
    let datum_plane_block_uses = features.feature_datum_plane_block_uses.as_slice();
    let datum_plane_payloads = features.feature_datum_plane_payloads.as_slice();
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
    let sketch_fixed_points = features.feature_sketch_fixed_points.as_slice();
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
    let offset_store_body_references = body_data_block_uses
        .iter()
        .map(|use_| use_.feature_body_reference.as_str())
        .collect::<BTreeSet<_>>();
    let body_references = body_references
        .iter()
        .filter(|reference| !offset_store_body_references.contains(reference.id.as_str()))
        .map(|reference| {
            (
                reference.operation_label.as_str(),
                reference.body_object_index,
            )
        })
        .collect::<BTreeMap<_, _>>();
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
        body_writer_history.record_writer(None, &feature.outputs, &feature.id);
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
    let mut datum_plane_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneBlockUse>>::new();
    for block_use in datum_plane_block_uses {
        datum_plane_uses_by_input_operation
            .entry(block_use.input_operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let operation_positions = labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let feature_ids_by_operation = labels
        .iter()
        .filter(|label| projects_neutral_feature(&label.value))
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
                feature_body_outputs(*object_index, &bodies_by_object_index),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let simple_hole_operations =
        simple_hole_operations(simple_hole_templates, simple_hole_construction_groups)
            .unwrap_or_default();
    let (hole_outputs, simple_hole_diameters) =
        match hole_body_projection(ir, &simple_hole_operations, &explicit_simple_hole_outputs) {
            Some(projection) => (projection.outputs, projection.diameters),
            None => (explicit_simple_hole_outputs, BTreeMap::new()),
        };
    let simple_hole_placements =
        hole_axis_placements_for_operations(ir, &simple_hole_operations, &hole_outputs);
    let hole_package_operations = labels
        .iter()
        .filter(|label| label.value == "HOLE PACKAGE")
        .map(|label| label.id.clone())
        .collect::<Vec<_>>();
    let explicit_hole_package_outputs = hole_package_operations
        .iter()
        .filter_map(|operation| {
            let object_index = body_references.get(operation.as_str())?;
            Some((
                operation.clone(),
                feature_body_outputs(*object_index, &bodies_by_object_index),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let (hole_package_outputs, hole_package_diameters) =
        match hole_body_projection(ir, &hole_package_operations, &explicit_hole_package_outputs) {
            Some(projection) => (projection.outputs, projection.diameters),
            None => (explicit_hole_package_outputs, BTreeMap::new()),
        };
    let hole_package_placements =
        hole_axis_placements_for_operations(ir, &hole_package_operations, &hole_package_outputs);
    let simple_hole_chamfers = simple_hole_chamfers(ir, simple_hole_templates, &hole_outputs);
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
    for (ordinal, label) in labels.iter().enumerate() {
        if !projects_neutral_feature(&label.value) {
            continue;
        }
        let id = feature_ids_by_operation
            .get(label.id.as_str())
            .expect("every operation label owns one neutral feature identity")
            .clone();
        let mut dependencies = Vec::new();
        if let Some(operation) = booleans.get(label.id.as_str()) {
            for body in &operation.tool_object_indices {
                if let Some(writer) = body_writer_history.native_writer(canonical_body(*body)) {
                    if !dependencies.contains(writer) {
                        dependencies.push(writer.clone());
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
                    feature_body_outputs(*body, &bodies_by_object_index)
                })
        };
        if outputs.is_empty() {
            outputs = hole_outputs
                .get(label.id.as_str())
                .or_else(|| hole_package_outputs.get(label.id.as_str()))
                .cloned()
                .unwrap_or_default();
        }
        let native_primary_body = body_references
            .get(label.id.as_str())
            .copied()
            .map(canonical_body);
        if let Some(body) = body_references.get(label.id.as_str()) {
            source_properties.insert("primary_body_object_index".to_string(), body.to_string());
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
        for (ordinal, payload) in datum_csys_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            source_properties.insert(format!("datum_csys_payload.{ordinal}"), payload.id.clone());
        }
        if let Some(header) = datum_plane_headers_by_operation.get(label.id.as_str()) {
            source_properties.insert("datum_plane_header".to_string(), header.id.clone());
        }
        if let Some(payload) = datum_plane_payloads_by_operation.get(label.id.as_str()) {
            source_properties.insert("datum_plane_payload".to_string(), payload.id.clone());
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
        for reference in sketch_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
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
        if outputs.is_empty() {
            if let Some((body, _)) = &block_projection {
                outputs.push(body.clone());
            }
        }
        let block_op = if block_projection.is_some()
            && matches!(outputs.as_slice(), [_])
            && !body_writer_history.has_primary_writer(native_primary_body, &outputs)
        {
            BooleanOp::NewBody
        } else {
            BooleanOp::Unresolved
        };
        body_writer_history.extend_primary_dependencies(
            native_primary_body,
            &outputs,
            &mut dependencies,
        );
        let block_placement = block_projection.map(|(_, placement)| placement);
        let sew_projection = (label.value == "SEW")
            .then(|| {
                sew_body_feature_definition(
                    *body_references.get(label.id.as_str())?,
                    segment_body_operands_by_operation
                        .get(label.id.as_str())?
                        .as_slice(),
                    &body_alias_roots,
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let trim_body_projection = (label.value == "TRIM BODY")
            .then(|| {
                trim_body_feature_definition(
                    *body_references.get(label.id.as_str())?,
                    segment_body_operands_by_operation
                        .get(label.id.as_str())?
                        .as_slice(),
                    &body_alias_roots,
                    &bodies_by_object_index,
                )
                .or_else(|| {
                    offset_store_trim_body_feature_definition(
                        offset_store_bodies_by_operation
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
                body_references.get(label.id.as_str()).is_none_or(|body| {
                    body_writer_history
                        .native_writer(canonical_body(*body))
                        .is_some()
                }),
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
        let extract_projection = (label.value == "EXTRACT_BODY")
            .then(|| {
                extract_body_feature_definition(
                    offset_store_bodies_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                )
            })
            .flatten();
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
                attach_solved_sketch_points(
                    ir,
                    label,
                    sketch_point_uses_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    sketch_point_groups,
                    annotations,
                    stream,
                )
            })
            .flatten();
        let definition = booleans.get(label.id.as_str()).map_or_else(
            || {
                trim_body_projection
                    .or(delete_projection)
                    .or(sew_projection)
                    .or(extrude_projection)
                    .or(extract_projection)
                    .or_else(|| blend_projection.map(|(definition, _)| definition))
                    .or_else(|| thicken_projection.map(|(definition, _)| definition))
                    .or_else(|| offset_projection.map(|(definition, _)| definition))
                    .unwrap_or_else(|| {
                        if let Some(sketch) = sketch {
                            return FeatureDefinition::Sketch {
                                space: SketchSpace::Planar,
                                sketch: Some(sketch),
                            };
                        }
                        let mut definition = non_boolean_feature_definition_with_parameters(
                            &label.value,
                            &operation_payload_strings,
                            block_dimension_values,
                            block_placement,
                            HoleProjection {
                                placement: simple_hole_placements
                                    .get(label.id.as_str())
                                    .or_else(|| hole_package_placements.get(label.id.as_str()))
                                    .cloned(),
                                diameter: simple_hole_diameters
                                    .get(label.id.as_str())
                                    .or_else(|| hole_package_diameters.get(label.id.as_str()))
                                    .copied(),
                                chamfer: simple_hole_chamfers.get(label.id.as_str()).copied(),
                            },
                            native_parameters,
                        );
                        if let FeatureDefinition::Block { op, .. } = &mut definition {
                            *op = block_op;
                        }
                        definition
                    })
            },
            |operation| {
                boolean_feature_definition(operation, &body_alias_roots, &bodies_by_object_index)
            },
        );
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
        if let Some(annotation) = text_semantic_annotation(
            &label.value,
            &id,
            &label.id,
            u32::try_from(ir.model.semantic_annotations.len()).unwrap_or(u32::MAX),
            &operation_payload_strings,
        ) {
            annotations
                .note(&annotation.id.0, stream, label.source_offset)
                .tag("TEXT_SEMANTIC_ANNOTATION");
            annotations.exactness(&annotation.id.0, Exactness::Derived);
            ir.model.semantic_annotations.push(annotation);
        }
        let native_output = (!deletes_body).then_some(native_primary_body).flatten();
        body_writer_history.record_writer(native_output, &outputs, &id);
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
    }
}

fn attach_solved_sketch_points(
    ir: &mut CadIr,
    label: &crate::native::features::FeatureOperationLabel,
    point_uses: &[&crate::native::features::FeatureSketchPointUse],
    point_groups: &[crate::native::features::FeatureSketchPointGroup],
    annotations: &mut AnnotationBuilder,
    stream: cadmpeg_ir::annotations::StreamHandle,
) -> Option<SketchId> {
    if point_uses.is_empty() {
        return None;
    }
    let groups = point_groups
        .iter()
        .map(|group| (group.id.as_str(), group))
        .collect::<BTreeMap<_, _>>();
    let operation_key = label
        .id
        .strip_prefix("nx:feature-history:operation-label#")
        .unwrap_or(label.id.as_str());
    let sketch_id = SketchId(format!("nx:feature-history:sketch#{operation_key}"));
    let mut entities = Vec::new();
    let mut represented_groups = BTreeSet::new();
    for point_use in point_uses {
        let group = groups.get(point_use.sketch_point_group.as_str())?;
        if group.operation_label != label.id
            || !represented_groups.insert(group.id.as_str())
            || group
                .coordinates
                .iter()
                .any(|coordinate| !coordinate.is_finite())
        {
            return None;
        }
        let entity_key = point_use
            .id
            .strip_prefix("nx:feature-history:sketch-point-use#")
            .unwrap_or(point_use.id.as_str());
        entities.push((
            point_use.source_offsets.iter().copied().min()?,
            SketchEntity {
                id: SketchEntityId(format!(
                    "nx:feature-history:sketch-entity#point-{entity_key}"
                )),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: Some(point_use.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(group.coordinates[0], group.coordinates[1]),
                },
            },
        ));
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
        placement: SketchPlacement::Unresolved,
        profiles: Vec::new(),
        native_ref: Some(label.id.clone()),
    });
    Some(sketch_id)
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

// ===== Feature-semantics and attachment helpers (moved from decode.rs) =====

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
    label != "Container"
}

fn text_semantic_annotation(
    operation_kind: &str,
    feature: &FeatureId,
    native_ref: &str,
    order: u32,
    payload_strings: &[&str],
) -> Option<SemanticAnnotation> {
    if operation_kind != "TEXT" {
        return None;
    }
    let [text, font_family] = payload_strings else {
        return None;
    };
    Some(SemanticAnnotation {
        id: SemanticAnnotationId(format!("{}:semantic-text", feature.0)),
        object: feature.0.clone(),
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
    has_previous_writer: bool,
    output_kinds: &[cadmpeg_ir::topology::BodyKind],
) -> BooleanOp {
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
    let face_blend = support_pairs
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
        });
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
        let offset = normal.x * origin.x + normal.y * origin.y + normal.z * origin.z;
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

/// Permutation-invariant hole properties derived from one complete body partition.
#[derive(Default)]
struct HoleProjection {
    pub(crate) placement: Option<HolePlacement>,
    pub(crate) diameter: Option<Length>,
    pub(crate) chamfer: Option<HoleKind>,
}

fn non_boolean_feature_definition_with_parameters(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole: HoleProjection,
    native_parameters: BTreeMap<String, String>,
) -> FeatureDefinition {
    let simple_hole_template = unique_simple_hole_template(payload_strings);
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
        "DATUM_PLANE" => FeatureDefinition::DatumPlaneUnresolved,
        "POINT" => FeatureDefinition::DatumPointUnresolved,
        "DATUM_CSYS" => FeatureDefinition::DatumCoordinateSystemUnresolved,
        "TEXT" if matches!(payload_strings, [_, _]) => FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::Annotations,
            children: Vec::new(),
            active_child: None,
        },
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
        "SIMPLE HOLE" => FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            position: None,
            direction: None,
            placements: hole.placement.into_iter().collect(),
            kind: hole.chamfer.unwrap_or_else(|| {
                if simple_hole_template.is_some() {
                    HoleKind::Unresolved {
                        form: Some(HoleForm::Chamfer),
                        counterbore_diameter: None,
                        counterbore_depth: None,
                        countersink_diameter: None,
                        countersink_angle: None,
                    }
                } else {
                    HoleKind::Simple
                }
            }),
            exit_kind: hole.chamfer.or_else(|| {
                simple_hole_template
                    .is_some()
                    .then_some(HoleKind::Unresolved {
                        form: Some(HoleForm::Chamfer),
                        counterbore_diameter: None,
                        counterbore_depth: None,
                        countersink_diameter: None,
                        countersink_angle: None,
                    })
            }),
            diameter: hole.diameter,
            extent: simple_hole_template
                .is_some()
                .then_some(cadmpeg_ir::features::Termination::ThroughAll),
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
        "HOLE PACKAGE" => FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            position: None,
            direction: None,
            placements: hole.placement.into_iter().collect(),
            kind: HoleKind::Unresolved {
                form: None,
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            },
            exit_kind: None,
            diameter: hole.diameter,
            extent: None,
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
) -> Option<Vec<String>> {
    let template_operations = templates
        .iter()
        .filter(|template| {
            template.form == crate::native::features::SimpleHoleForm::Simple
                && template.extent == crate::native::features::SimpleHoleExtent::Through
        })
        .map(|template| template.operation_label.as_str())
        .collect::<BTreeSet<_>>();
    if template_operations.len() != templates.len() || template_operations.is_empty() {
        return None;
    }
    Some(match groups {
        [] => templates
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
            {
                return None;
            }
            group.operation_labels.clone()
        }
        _ => return None,
    })
}

struct HoleBodyProjection {
    outputs: BTreeMap<String, Vec<BodyId>>,
    diameters: BTreeMap<String, Length>,
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

    let angular_tolerance = ir.tolerances.angular.max(1e-12);
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            continue;
        };
        let Some(body_faces) = connected_solid_body_faces(ir, &body) else {
            continue;
        };
        let Some(bores) = through_bore_cylinders(ir, &body_faces) else {
            continue;
        };
        let [(origin, axis, _)] = bores.as_slice() else {
            continue;
        };
        let Some(mut axis) = unit_vector(*axis) else {
            continue;
        };
        let Some(leading) = [axis.x, axis.y, axis.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)
        else {
            continue;
        };
        if leading < 0.0 {
            axis = Vector3::new(-axis.x, -axis.y, -axis.z);
        }
        let axial_offset = origin.x * axis.x + origin.y * axis.y + origin.z * axis.z;
        let origin = Point3::new(
            origin.x - axial_offset * axis.x,
            origin.y - axial_offset * axis.y,
            origin.z - axial_offset * axis.z,
        );
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
            continue;
        }
        placements.insert(operation.clone(), HolePlacement::Axis { origin, axis });
    }
    placements
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
    body_faces
        .iter()
        .copied()
        .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
        .filter_map(|face| match surfaces.get(&face.surface)? {
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                radius,
                ..
            } if radius.is_finite() && *radius > 0.0 => Some((face, *origin, *axis, *radius)),
            _ => None,
        })
        .map(|(face, origin, axis, radius)| {
            let mut loop_offsets = Vec::with_capacity(2);
            for loop_id in &face.loops {
                let coedges = coedges_by_loop.get(loop_id)?;
                if coedges.is_empty() {
                    return None;
                }
                let mut loop_offset = None::<f64>;
                for coedge in coedges {
                    let curve_id = edges.get(&coedge.edge).copied().flatten()?;
                    let CurveGeometry::Circle {
                        center,
                        axis: circle_axis,
                        radius: circle_radius,
                        ..
                    } = curves.get(curve_id)?
                    else {
                        return None;
                    };
                    if (circle_radius - radius).abs() > linear_tolerance
                        || (1.0 - dot_vector(axis, *circle_axis).abs()) > angular_tolerance
                    {
                        return None;
                    }
                    let delta = Vector3::new(
                        center.x - origin.x,
                        center.y - origin.y,
                        center.z - origin.z,
                    );
                    if cross_vector(delta, axis).norm() > linear_tolerance {
                        return None;
                    }
                    let offset = dot_vector(delta, axis);
                    if loop_offset.is_some_and(|value| (value - offset).abs() > linear_tolerance) {
                        return None;
                    }
                    loop_offset = Some(offset);
                }
                loop_offsets.push(loop_offset?);
            }
            let [first, second] = loop_offsets.as_slice() else {
                return None;
            };
            if (first - second).abs() <= linear_tolerance {
                return None;
            }
            Some((origin, axis, radius))
        })
        .collect()
}

/// Derive identical entry and exit chamfer treatments only when every simple
/// through-hole bore has exactly two coaxial conical faces and every cone is
/// bounded by the bore circle and one equal larger circle.
fn simple_hole_chamfers(
    ir: &CadIr,
    templates: &[crate::native::features::FeatureSimpleHoleTemplate],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, HoleKind> {
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
        .map(|template| template.operation_label.clone())
        .collect::<BTreeSet<_>>();
    if operations.len() != templates.len() || operations.is_empty() {
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
                    let dot = axis.x * bore_axis.x + axis.y * bore_axis.y + axis.z * bore_axis.z;
                    if (1.0 - dot.abs()) > angular_tolerance {
                        return None;
                    }
                    let delta = Vector3::new(
                        origin.x - bore_origin.x,
                        origin.y - bore_origin.y,
                        origin.z - bore_origin.z,
                    );
                    let cross = Vector3::new(
                        delta.y * bore_axis.z - delta.z * bore_axis.y,
                        delta.z * bore_axis.x - delta.x * bore_axis.z,
                        delta.x * bore_axis.y - delta.y * bore_axis.x,
                    );
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

/// Resolve a complete object-index selection only when every alias root owns one
/// decoded body image. Retain the complete feature-input-local identities when
/// current topology cannot represent a consumed historical body, and fall back
/// to the native expression when even the alias namespace is incomplete.
struct FeatureBodySelection {
    selection: BodySelection,
    alias_roots: Option<Vec<u32>>,
}

fn feature_body_selection(
    object_indices: &[u32],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
    native: String,
) -> FeatureBodySelection {
    let mut roots = Vec::new();
    for index in object_indices {
        let Some(root) = body_alias_roots.get(index) else {
            return FeatureBodySelection {
                selection: BodySelection::Native(native),
                alias_roots: None,
            };
        };
        if !roots.contains(root) {
            roots.push(*root);
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
        return FeatureBodySelection {
            selection: BodySelection::Resolved { bodies, native },
            alias_roots: Some(roots),
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
        alias_roots: Some(roots),
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
    let complete = left.alias_roots.as_ref().is_some_and(|left| {
        right
            .alias_roots
            .as_ref()
            .is_some_and(|right| !left.iter().any(|root| right.contains(root)))
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
        BodySelection::NativeSet(members) => BodySelection::NativeSet(members),
        BodySelection::Bodies(bodies) => BodySelection::Bodies(bodies),
        BodySelection::Generated { .. }
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::Unresolved => BodySelection::Unresolved,
    };
    (native(left), native(right))
}

pub(crate) fn boolean_feature_definition(
    operation: &crate::native::features::FeatureBooleanOperation,
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> FeatureDefinition {
    let (target, tools) = atomic_disjoint_body_selections(
        feature_body_selection(
            &[operation.target_object_index],
            body_alias_roots,
            bodies_by_object_index,
            format!("nx:om-object-index#{}", operation.target_object_index),
        ),
        feature_body_selection(
            &operation.tool_object_indices,
            body_alias_roots,
            bodies_by_object_index,
            format!(
                "nx:om-object-indices#{}",
                operation
                    .tool_object_indices
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
    );
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
    Some(FeatureDefinition::DeleteBody {
        // A typed DELETE primary-body field names one exact feature input. It
        // needs no cross-selection alias proof when it has no segment binding.
        bodies: match selection {
            BodySelection::Native(native) => BodySelection::Local {
                bodies: vec![format!("nx:om-body-object#{body}")],
                native,
            },
            selection => selection,
        },
        mode: BodyRetentionMode::DeleteSelected,
    })
}

/// Project one input-store body as the exact feature-local source of an extract.
/// Multiple or absent body uses do not establish a source identity.
fn extract_body_feature_definition(
    offset_store_bodies: &[(u32, String)],
) -> Option<FeatureDefinition> {
    let [(object_index, data_block)] = offset_store_bodies else {
        return None;
    };
    Some(FeatureDefinition::ExtractBody {
        source: BodySelection::Local {
            bodies: vec![data_block.clone()],
            native: format!("nx:om-object-index#{object_index}"),
        },
    })
}

/// Project one input-store body as the exact feature-local trim target. Tool
/// and retained-side semantics remain unresolved independently of target identity.
fn offset_store_trim_body_feature_definition(
    offset_store_bodies: &[(u32, String)],
) -> Option<FeatureDefinition> {
    let [(object_index, data_block)] = offset_store_bodies else {
        return None;
    };
    Some(FeatureDefinition::TrimBodies {
        targets: BodySelection::Local {
            bodies: vec![data_block.clone()],
            native: format!("nx:om-object-index#{object_index}"),
        },
        tools: BodySelection::Unresolved,
        keep: BodyTrimSide::Unresolved,
    })
}

fn sew_body_feature_definition(
    primary_body_object_index: u32,
    operands: &[&crate::native::features::FeatureOperationBodyOperand],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Option<FeatureDefinition> {
    (!operands.is_empty()).then(|| {
        let object_indices = std::iter::once(primary_body_object_index)
            .chain(operands.iter().map(|operand| operand.operand_object_index))
            .collect::<Vec<_>>();
        FeatureDefinition::SewBodies {
            bodies: feature_body_set_selection(
                &object_indices,
                body_alias_roots,
                bodies_by_object_index,
                format!(
                    "nx:om-object-indices#{}",
                    object_indices
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
            gap_tolerance: None,
        }
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
        let (targets, tools) = atomic_disjoint_body_selections(
            feature_body_selection(
                &[target_object_index],
                body_alias_roots,
                bodies_by_object_index,
                format!("nx:om-object-index#{target_object_index}"),
            ),
            feature_body_selection(
                &tool_object_indices,
                body_alias_roots,
                bodies_by_object_index,
                format!(
                    "nx:om-object-indices#{}",
                    tool_object_indices
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
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
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Vec<BodyId> {
    bodies_by_object_index
        .get(&object_index)
        .cloned()
        .unwrap_or_default()
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
mod tests {
    #![allow(unused_imports)]
    use std::io::{Cursor, Write};

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};

    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry,
        ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::report::LossCategory;
    use cadmpeg_ir::Exactness;

    use crate::container;
    use crate::parasolid::{self, StreamKind};
    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

    fn hole_diameters_for_operations(
        ir: &CadIr,
        operations: &[String],
        outputs: &BTreeMap<String, Vec<BodyId>>,
    ) -> BTreeMap<String, Length> {
        hole_body_projection(ir, operations, outputs)
            .map(|projection| projection.diameters)
            .unwrap_or_default()
    }

    fn simple_hole_diameters(
        ir: &CadIr,
        templates: &[crate::native::features::FeatureSimpleHoleTemplate],
        groups: &[crate::native::features::FeatureSimpleHoleConstructionGroup],
        outputs: &BTreeMap<String, Vec<BodyId>>,
    ) -> BTreeMap<String, Length> {
        let Some(operations) = simple_hole_operations(templates, groups) else {
            return BTreeMap::new();
        };
        hole_diameters_for_operations(ir, &operations, outputs)
    }

    #[test]
    fn active_configuration_retains_complete_evaluated_parameter_state() {
        let parameter =
            |id: &str, ordinal, value, dependencies: Vec<ParameterId>| DesignParameter {
                id: ParameterId(id.into()),
                owner: None,
                ordinal,
                name: id.into(),
                expression: id.into(),
                display: None,
                value,
                dependencies,
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            };
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.parameters = vec![
            parameter(
                "length",
                0,
                Some(ParameterValue::Length(Length(25.4))),
                Vec::new(),
            ),
            parameter(
                "angle",
                1,
                Some(ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2))),
                vec![ParameterId("length".into())],
            ),
        ];
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("active".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Model".into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });
        let mut annotations = AnnotationBuilder::new();

        super::attach_active_configuration_parameter_values(&mut ir, &mut annotations);

        assert_eq!(
            ir.model.configurations[0].parameter_values,
            BTreeMap::from([
                (
                    ParameterId("angle".into()),
                    ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2))
                ),
                (
                    ParameterId("length".into()),
                    ParameterValue::Length(Length(25.4))
                ),
            ])
        );
    }

    #[test]
    fn active_configuration_parameter_state_rejects_incomplete_sets_atomically() {
        let parameter = |id: &str, value, dependencies: Vec<ParameterId>| DesignParameter {
            id: ParameterId(id.into()),
            owner: None,
            ordinal: 0,
            name: id.into(),
            expression: id.into(),
            display: None,
            value,
            dependencies,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        };
        let configuration = || DesignConfiguration {
            id: ConfigurationId("active".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Model".into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        };
        let mut cases = [
            vec![parameter("p1", None, Vec::new())],
            vec![parameter(
                "p1",
                Some(ParameterValue::Real(1.0)),
                vec![ParameterId("missing".into())],
            )],
            vec![
                parameter("p1", Some(ParameterValue::Real(1.0)), Vec::new()),
                parameter("p1", Some(ParameterValue::Real(2.0)), Vec::new()),
            ],
            vec![
                parameter("p1", Some(ParameterValue::Real(1.0)), Vec::new()),
                parameter(
                    "p2",
                    Some(ParameterValue::Real(2.0)),
                    vec![ParameterId("p1".into())],
                ),
            ],
        ];
        let mut annotations = AnnotationBuilder::new();
        for parameters in &mut cases {
            let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
            ir.model.parameters = std::mem::take(parameters);
            ir.model.configurations.push(configuration());

            super::attach_active_configuration_parameter_values(&mut ir, &mut annotations);

            assert!(ir.model.configurations[0].parameter_values.is_empty());
        }
    }

    #[test]
    fn active_configuration_body_writers_close_false_suppression_through_dependencies() {
        let feature =
            |id: &str, dependencies: Vec<FeatureId>, outputs: Vec<BodyId>, suppressed| Feature {
                id: FeatureId(id.into()),
                ordinal: 0,
                name: None,
                suppressed,
                parent: None,
                dependencies,
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs,
                definition: FeatureDefinition::TreeNode {
                    role: FeatureTreeNodeRole::History,
                    children: Vec::new(),
                    active_child: None,
                },
                native_ref: None,
            };
        let configuration = |active, bodies| DesignConfiguration {
            id: ConfigurationId("configuration".into()),
            ordinal: 0,
            active,
            source_index: Some(0),
            name: "Model".into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            suppressed_features: Vec::new(),
            bodies,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        };
        let body = BodyId("body".into());
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.features = vec![
            feature("dependency", Vec::new(), Vec::new(), None),
            feature(
                "writer",
                vec![FeatureId("dependency".into())],
                vec![body.clone()],
                None,
            ),
            feature("unrelated", Vec::new(), Vec::new(), None),
        ];
        for (ordinal, feature) in ir.model.features.iter_mut().enumerate() {
            feature.ordinal = ordinal as u64;
        }
        ir.model.configurations = vec![configuration(
            true,
            ConfigurationBodies::Resolved(vec![body]),
        )];
        let mut annotations = AnnotationBuilder::new();

        super::attach_active_configuration_feature_states(&mut ir, &mut annotations);

        assert_eq!(ir.model.features[0].suppressed, Some(false));
        assert_eq!(ir.model.features[1].suppressed, Some(false));
        assert_eq!(ir.model.features[2].suppressed, None);
        let states = &ir.model.configurations[0].feature_states;
        assert_eq!(
            states.keys().cloned().collect::<Vec<_>>(),
            [FeatureId("dependency".into()), FeatureId("writer".into())]
        );
        assert_eq!(
            states[&FeatureId("writer".into())].dependencies,
            [FeatureId("dependency".into())]
        );
        assert_eq!(
            states[&FeatureId("writer".into())].outputs,
            [BodyId("body".into())]
        );
    }

    #[test]
    fn current_body_writers_close_false_suppression_without_a_configuration() {
        let body = BodyId("body".into());
        let feature = |id: &str, ordinal, dependencies, outputs| Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: None,
            parent: None,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        };
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut body_record = cadmpeg_ir::examples::unit_cube().model.bodies.remove(0);
        body_record.id = body.clone();
        ir.model.bodies.push(body_record);
        ir.model.features = vec![
            feature("dependency", 1, Vec::new(), Vec::new()),
            feature(
                "writer",
                2,
                vec![FeatureId("dependency".into())],
                vec![body],
            ),
            feature("unrelated", 3, Vec::new(), Vec::new()),
        ];
        let mut annotations = AnnotationBuilder::new();

        super::attach_current_feature_states(&mut ir, &mut annotations);

        assert_eq!(ir.model.features[0].suppressed, Some(false));
        assert_eq!(ir.model.features[1].suppressed, Some(false));
        assert_eq!(ir.model.features[2].suppressed, None);

        ir.model.features[0].ordinal = 2;
        assert!(super::active_feature_closure(&ir, &[BodyId("body".into())]).is_none());
        ir.model.features[0].ordinal = 1;
        ir.model.features[2].id = FeatureId("writer".into());
        assert!(super::active_feature_closure(&ir, &[BodyId("body".into())]).is_none());
        ir.model.features[2].id = FeatureId("unrelated".into());
        ir.model.features[1].suppressed = Some(true);
        assert!(super::active_feature_closure(&ir, &[BodyId("body".into())]).is_none());
    }

    #[test]
    fn active_configuration_feature_states_reject_incomplete_or_ambiguous_graphs_atomically() {
        let producer = |dependency: &str| Feature {
            id: FeatureId("writer".into()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: vec![FeatureId(dependency.into())],
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![BodyId("body".into())],
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        };
        let configuration = |id: &str, active, bodies| DesignConfiguration {
            id: ConfigurationId(id.into()),
            ordinal: 0,
            active,
            source_index: Some(0),
            name: id.into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            suppressed_features: Vec::new(),
            bodies,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        };
        let mut missing_dependency = CadIr::empty(cadmpeg_ir::units::Units::default());
        missing_dependency.model.features = vec![producer("missing")];
        missing_dependency.model.configurations = vec![configuration(
            "active",
            true,
            ConfigurationBodies::Resolved(vec![BodyId("body".into())]),
        )];
        let mut annotations = AnnotationBuilder::new();
        super::attach_active_configuration_feature_states(
            &mut missing_dependency,
            &mut annotations,
        );
        assert_eq!(missing_dependency.model.features[0].suppressed, None);
        assert!(missing_dependency.model.configurations[0]
            .feature_states
            .is_empty());

        let mut unresolved_bodies = CadIr::empty(cadmpeg_ir::units::Units::default());
        unresolved_bodies.model.features = vec![producer("writer")];
        unresolved_bodies.model.features[0].dependencies.clear();
        unresolved_bodies.model.configurations = vec![configuration(
            "active",
            true,
            ConfigurationBodies::Unresolved,
        )];
        super::attach_active_configuration_feature_states(&mut unresolved_bodies, &mut annotations);
        assert_eq!(unresolved_bodies.model.features[0].suppressed, None);
        assert!(unresolved_bodies.model.configurations[0]
            .feature_states
            .is_empty());

        let mut contradicted = CadIr::empty(cadmpeg_ir::units::Units::default());
        contradicted.model.features = vec![producer("writer")];
        contradicted.model.features[0].dependencies.clear();
        contradicted.model.features[0].suppressed = Some(true);
        contradicted.model.configurations = vec![configuration(
            "active",
            true,
            ConfigurationBodies::Resolved(vec![BodyId("body".into())]),
        )];
        super::attach_active_configuration_feature_states(&mut contradicted, &mut annotations);
        assert_eq!(contradicted.model.features[0].suppressed, Some(true));
        assert!(contradicted.model.configurations[0]
            .feature_states
            .is_empty());

        let mut ambiguous = CadIr::empty(cadmpeg_ir::units::Units::default());
        ambiguous.model.features = vec![producer("writer")];
        ambiguous.model.features[0].dependencies.clear();
        ambiguous.model.configurations = vec![
            configuration(
                "first",
                true,
                ConfigurationBodies::Resolved(vec![BodyId("body".into())]),
            ),
            configuration(
                "second",
                true,
                ConfigurationBodies::Resolved(vec![BodyId("body".into())]),
            ),
        ];
        super::attach_active_configuration_feature_states(&mut ambiguous, &mut annotations);
        assert_eq!(ambiguous.model.features[0].suppressed, None);
        assert!(ambiguous
            .model
            .configurations
            .iter()
            .all(|configuration| configuration.feature_states.is_empty()));
    }

    #[test]
    fn operation_source_properties_require_unique_owned_structures() {
        let record = crate::native::features::FeatureOperationRecord {
            id: "record".into(),
            operation_label: "operation".into(),
            ordinal: 3,
            byte_len: 20,
            sha256: "record-hash".into(),
            payload_byte_len: 10,
            payload_sha256: "payload-hash".into(),
            payload_source_offset: 110,
            source_offset: 100,
        };
        let common = crate::native::features::FeatureOperationCommonFrame {
            id: "common".into(),
            operation_record: record.id.clone(),
            ordinal: 0,
            indices: [0, 351, 171],
            raw_indices: [vec![0], vec![0x81, 0x5f], vec![0x80, 0xab]],
            marker: [1, 3, 2],
            state: [1, 2, 1, 1, 1, 0, 0, 0],
            local_ordinal: 41,
            raw_local_ordinal: vec![0x29],
            object_index: Some(65),
            raw_object_index: vec![0x41],
            byte_len: 20,
            source_offset: 101,
            index_source_offsets: [101, 102, 104],
            state_source_offset: 109,
            local_ordinal_source_offset: 117,
            object_index_source_offset: 119,
        };
        let frame = crate::native::features::FeatureOperationTerminalFrame {
            id: "frame".into(),
            operation_record: record.id.clone(),
            immediate_common_frame: Some(common.id.clone()),
            local_ordinal: 41,
            raw_local_ordinal: vec![0x29],
            object_index: Some(65),
            raw_object_index: vec![0x41],
            source_offset: 117,
            object_index_source_offset: 119,
        };
        assert_eq!(
            super::operation_source_properties(
                &record.operation_label,
                std::slice::from_ref(&record),
                std::slice::from_ref(&common),
                std::slice::from_ref(&frame),
            ),
            BTreeMap::from([
                ("operation_common_frame.0".into(), "common".into()),
                ("operation_record".into(), "record".into()),
                ("operation_terminal_frame".into(), "frame".into()),
            ])
        );
        assert!(super::operation_source_properties("missing", &[], &[], &[]).is_empty());
        assert_eq!(
            super::operation_source_properties(
                &record.operation_label,
                std::slice::from_ref(&record),
                &[],
                &[],
            ),
            BTreeMap::from([("operation_record".into(), "record".into())])
        );
        let mut noncontiguous_common = common.clone();
        noncontiguous_common.ordinal = 1;
        assert_eq!(
            super::operation_source_properties(
                &record.operation_label,
                std::slice::from_ref(&record),
                std::slice::from_ref(&noncontiguous_common),
                std::slice::from_ref(&frame),
            ),
            BTreeMap::from([
                ("operation_record".into(), "record".into()),
                ("operation_terminal_frame".into(), "frame".into()),
            ])
        );
        assert!(super::operation_source_properties(
            &record.operation_label,
            &[record.clone(), record.clone()],
            std::slice::from_ref(&common),
            std::slice::from_ref(&frame),
        )
        .is_empty());
        assert_eq!(
            super::operation_source_properties(
                &record.operation_label,
                std::slice::from_ref(&record),
                &[],
                &[frame.clone(), frame],
            ),
            BTreeMap::from([("operation_record".into(), "record".into())])
        );
    }

    #[test]
    fn solved_sketch_points_require_unique_exact_ownership_atomically() {
        let label = crate::native::features::FeatureOperationLabel {
            id: "nx:feature-history:operation-label#section-7".to_string(),
            section_link: "section".to_string(),
            ordinal: 7,
            value: "SKETCH".to_string(),
            object_indices: [None; 4],
            raw_object_indices: Default::default(),
            source_offset: 40,
        };
        let group = crate::native::features::FeatureSketchPointGroup {
            id: "point-group".to_string(),
            operation_label: label.id.clone(),
            name: "Point1".to_string(),
            points: vec!["payload-point".to_string()],
            coordinates: [12.5, -3.0],
        };
        let point_use = crate::native::features::FeatureSketchPointUse {
            id: "nx:feature-history:sketch-point-use#section-7-0".to_string(),
            operation_label: label.id.clone(),
            sketch_references: vec!["reference".to_string()],
            block_uses: vec!["block-use".to_string()],
            sketch_point_group: group.id.clone(),
            named_point: "named-point".to_string(),
            source_offsets: vec![52],
        };
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = AnnotationBuilder::new();
        let stream = annotations.stream("nx:container");
        let sketch = super::attach_solved_sketch_points(
            &mut ir,
            &label,
            &[&point_use],
            std::slice::from_ref(&group),
            &mut annotations,
            stream,
        )
        .expect("one exact point use projects a sketch");
        assert_eq!(ir.model.sketches[0].id, sketch);
        assert!(matches!(
            ir.model.sketch_entities[0].geometry,
            SketchGeometry::Point {
                position: Point2 { u: 12.5, v: -3.0 }
            }
        ));

        let mut rejected_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut rejected_annotations = AnnotationBuilder::new();
        let rejected_stream = rejected_annotations.stream("nx:container");
        assert!(super::attach_solved_sketch_points(
            &mut rejected_ir,
            &label,
            &[&point_use, &point_use],
            &[group],
            &mut rejected_annotations,
            rejected_stream,
        )
        .is_none());
        assert!(rejected_ir.model.sketches.is_empty());
        assert!(rejected_ir.model.sketch_entities.is_empty());
    }

    #[test]
    fn nx_native_feature_parameters_require_unique_resolved_names() {
        let expression = |id: &str, name: &str, text: &str| crate::native::om::Expression {
            id: id.to_string(),
            object_id: None,
            record: None,
            declaration: None,
            name: name.to_string(),
            parameter_index: None,
            qualifier: None,
            unit: crate::native::om::ExpressionUnit::Millimeter,
            expression: text.to_string(),
            value: None,
            source_entry: "entry".to_string(),
            source_table: "table".to_string(),
            source_offset: 0,
        };
        let parameter_use =
            |id: &str, expression: &str| crate::native::features::FeatureParameterUse {
                id: id.to_string(),
                operation_label: "operation".to_string(),
                expression: expression.to_string(),
                bindings: vec![format!("binding-{id}")],
                source_offsets: vec![0],
            };
        let expressions = vec![
            expression("expression-a", "p1_length", "p2_length * 2"),
            expression("expression-b", "p2_length", "12.5"),
        ];
        let uses = [
            parameter_use("use-a", "expression-a"),
            parameter_use("use-b", "expression-b"),
        ];
        let use_refs = uses.iter().collect::<Vec<_>>();
        let parameters = super::native_feature_parameters(&use_refs, &expressions);
        assert_eq!(
            parameters,
            std::collections::BTreeMap::from([
                ("p1_length".to_string(), "p2_length * 2".to_string()),
                ("p2_length".to_string(), "12.5".to_string()),
            ])
        );
        assert_eq!(
            super::non_boolean_feature_definition_with_parameters(
                "UNKNOWN OPERATION",
                &[],
                None,
                None,
                super::HoleProjection::default(),
                parameters,
            ),
            cadmpeg_ir::features::FeatureDefinition::Native {
                kind: "UNKNOWN OPERATION".to_string(),
                parameters: std::collections::BTreeMap::from([
                    ("p1_length".to_string(), "p2_length * 2".to_string()),
                    ("p2_length".to_string(), "12.5".to_string()),
                ]),
                properties: std::collections::BTreeMap::new(),
            }
        );
        assert!(matches!(
            super::non_boolean_feature_definition_with_parameters(
                "DELETE",
                &[],
                None,
                None,
                super::HoleProjection::default(),
                std::collections::BTreeMap::default(),
            ),
            cadmpeg_ir::features::FeatureDefinition::Native { kind, .. } if kind == "DELETE"
        ));
        assert!(matches!(
            super::non_boolean_feature_definition_with_parameters(
                "THRU_CURVE",
                &[],
                None,
                None,
                super::HoleProjection::default(),
                std::collections::BTreeMap::new(),
            ),
            cadmpeg_ir::features::FeatureDefinition::LoftUnresolved
        ));
        assert!(matches!(
            super::non_boolean_feature_definition_with_parameters(
                "SWP104",
                &[],
                None,
                None,
                super::HoleProjection::default(),
                std::collections::BTreeMap::new(),
            ),
            cadmpeg_ir::features::FeatureDefinition::Sweep {
                section: cadmpeg_ir::features::SweepSection::Unresolved(_),
                path: None,
                mode: cadmpeg_ir::features::SweepMode::Unresolved,
                ..
            }
        ));
        let duplicate_expressions = vec![
            expression("expression-a", "p1_length", "1"),
            expression("expression-b", "p1_length", "2"),
        ];
        assert!(super::native_feature_parameters(&use_refs, &duplicate_expressions).is_empty());
        let unresolved = [parameter_use("use-c", "missing")];
        assert!(super::native_feature_parameters(
            &unresolved.iter().collect::<Vec<_>>(),
            &expressions,
        )
        .is_empty());
    }

    #[test]
    fn nx_intersection_labels_project_without_fabricating_construction_fields() {
        for operation in ["ASSOCIATIVE_INTERSECTION", "Intersection Curve"] {
            assert!(matches!(
                super::non_boolean_feature_definition_with_parameters(
                    operation,
                    &[],
                    None,
                    None,
                    super::HoleProjection::default(),
                    std::collections::BTreeMap::default(),
                ),
                cadmpeg_ir::features::FeatureDefinition::SectionShape {
                    first: cadmpeg_ir::features::BodySelection::Unresolved,
                    second: cadmpeg_ir::features::BodySelection::Unresolved,
                    approximate: None,
                }
            ));
        }
    }

    #[test]
    fn nx_multi_instance_output_projects_as_an_unresolved_pattern() {
        assert!(matches!(
            super::non_boolean_feature_definition_with_parameters(
                "Multi Instance Output",
                &[],
                None,
                None,
                super::HoleProjection::default(),
                std::collections::BTreeMap::default(),
            ),
            cadmpeg_ir::features::FeatureDefinition::Pattern {
                seeds,
                pattern: cadmpeg_ir::features::PatternKind::Unresolved { form: None },
            } if seeds.is_empty()
        ));
    }

    #[test]
    fn topology_inferred_hole_axis_is_not_an_authored_direction() {
        use cadmpeg_ir::features::{FeatureDefinition, HolePlacement};
        use cadmpeg_ir::math::{Point3, Vector3};

        for kind in ["SIMPLE HOLE", "HOLE PACKAGE"] {
            assert!(matches!(
                super::non_boolean_feature_definition_with_parameters(
                    kind,
                    &[],
                    None,
                    None,
                    super::HoleProjection {
                        placement: Some(HolePlacement::Axis {
                            origin: Point3::new(1.0, 2.0, 3.0),
                            axis: Vector3::new(0.0, 0.0, 1.0),
                        }),
                        ..super::HoleProjection::default()
                    },
                    std::collections::BTreeMap::new(),
                ),
                FeatureDefinition::Hole {
                    position: None,
                    direction: None,
                    placements,
                    ..
                } if placements == [HolePlacement::Axis {
                    origin: Point3::new(1.0, 2.0, 3.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                }]
            ));
        }
    }

    #[test]
    fn complete_extrude_profile_projects_without_guessing_scalar_roles() {
        use cadmpeg_ir::features::{
            BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, ProfileRef, Termination,
        };

        assert_eq!(
            super::extrude_feature_definition(
                Some("nx:profile#1"),
                None,
                BooleanOp::NewBody,
                &[cadmpeg_ir::topology::BodyKind::Solid],
            ),
            FeatureDefinition::Extrude {
                profile: ProfileRef::Native("nx:profile#1".to_string()),
                direction: cadmpeg_ir::features::ExtrudeDirection::Unresolved,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Unresolved,
                        draft: None,
                        offset: None,
                    },
                },
                op: BooleanOp::NewBody,
                start: cadmpeg_ir::features::ExtrudeStart::Unresolved,
                direction_source: None,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }
        );
        assert!(matches!(
            super::extrude_feature_definition(
                None,
                None,
                BooleanOp::Unresolved,
                &[cadmpeg_ir::topology::BodyKind::Sheet],
            ),
            FeatureDefinition::Extrude {
                profile: ProfileRef::Unresolved(_),
                solid: Some(false),
                ..
            }
        ));
        assert!(matches!(
            super::extrude_feature_definition(
                Some("nx:profile#1"),
                Some("nx:profile#2"),
                BooleanOp::Unresolved,
                &[
                    cadmpeg_ir::topology::BodyKind::Solid,
                    cadmpeg_ir::topology::BodyKind::Sheet,
                ],
            ),
            FeatureDefinition::Extrude {
                profile: ProfileRef::Unresolved(_),
                solid: None,
                ..
            }
        ));
    }

    #[test]
    fn extrusion_is_new_body_only_for_one_first_written_surface_or_solid_output() {
        use cadmpeg_ir::features::BooleanOp;
        use cadmpeg_ir::topology::BodyKind;

        assert_eq!(
            super::extrude_boolean_op(false, &[BodyKind::Solid]),
            BooleanOp::NewBody
        );
        assert_eq!(
            super::extrude_boolean_op(true, &[BodyKind::Solid]),
            BooleanOp::Unresolved
        );
        assert_eq!(
            super::extrude_boolean_op(false, &[BodyKind::Sheet]),
            BooleanOp::NewBody
        );
        assert_eq!(
            super::extrude_boolean_op(false, &[BodyKind::Wire]),
            BooleanOp::Unresolved
        );
        assert_eq!(
            super::extrude_boolean_op(false, &[BodyKind::General]),
            BooleanOp::Unresolved
        );
        assert_eq!(
            super::extrude_boolean_op(false, &[BodyKind::Solid, BodyKind::Solid]),
            BooleanOp::Unresolved
        );
        assert_eq!(super::extrude_boolean_op(false, &[]), BooleanOp::Unresolved);
    }

    #[test]
    fn nx_block_dimension_parameters_name_the_block_as_consumer() {
        let expression = |key: u32| crate::native::om::Expression {
            id: format!("nx:test:expression#{key}"),
            object_id: Some(key),
            record: None,
            declaration: None,
            name: format!("p{key}"),
            parameter_index: Some(key),
            qualifier: None,
            unit: crate::native::om::ExpressionUnit::Millimeter,
            expression: key.to_string(),
            value: Some(f64::from(key)),
            source_entry: "part".into(),
            source_table: "table".into(),
            source_offset: u64::from(key),
        };
        let expressions = [expression(20), expression(21), expression(22)];
        let dimensions = crate::native::features::FeatureBlockDimensions {
            id: "dimensions".into(),
            operation_label: "nx:feature-history:operation-label#1-4".into(),
            construction: "construction".into(),
            anchor_bindings: vec!["binding".into()],
            declarations: ["d20".into(), "d21".into(), "d22".into()],
            expressions: [
                expressions[0].id.clone(),
                expressions[1].id.clone(),
                expressions[2].id.clone(),
            ],
            values: [20.0, 21.0, 22.0],
        };
        let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        super::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);
        let parameter_owners = ir
            .model
            .parameters
            .iter()
            .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
            .collect();
        let parameter_references = dimensions
            .expressions
            .iter()
            .filter_map(|expression| super::expression_parameter_id(expression))
            .collect::<Vec<_>>();
        assert_eq!(
            super::parameter_owner_dependencies(&parameter_owners, &parameter_references),
            [ir.model.features[0].id.clone()]
        );
        assert_eq!(
            ir.model.features[0].source_content,
            ir.model
                .parameters
                .iter()
                .map(|parameter| {
                    cadmpeg_ir::features::FeatureSourceContent::Parameter(parameter.id.clone())
                })
                .collect::<Vec<_>>()
        );
        super::attach_block_dimension_parameter_consumers(&mut ir, &[dimensions], &mut annotations);
        assert_eq!(ir.model.parameters.len(), 3);
        for (ordinal, parameter) in ir.model.parameters.iter().enumerate() {
            assert_eq!(
                parameter.properties[&format!("block_dimension.{ordinal}")],
                "dimensions"
            );
            assert_eq!(
                parameter.properties["consumer.0"],
                "nx:feature-history:feature#1-4"
            );
        }
    }

    #[test]
    fn feature_body_selection_retains_complete_input_local_identities_atomically() {
        use cadmpeg_ir::features::BodySelection;
        use cadmpeg_ir::ids::BodyId;
        use std::collections::BTreeMap;

        let first = BodyId("nx:s2:body#3".to_string());
        let roots = BTreeMap::from([(94, 94), (122, 122)]);
        assert_eq!(
            super::feature_body_selection(
                &[94, 122],
                &roots,
                &BTreeMap::new(),
                "nx:om-object-indices#94,122".to_string(),
            )
            .selection,
            BodySelection::Local {
                bodies: vec![
                    "nx:om-body-object#94".to_string(),
                    "nx:om-body-object#122".to_string(),
                ],
                native: "nx:om-object-indices#94,122".to_string(),
            }
        );
        assert!(matches!(
            super::feature_body_selection(
                &[94, 123],
                &roots,
                &BTreeMap::new(),
                "nx:om-object-indices#94,123".to_string(),
            )
            .selection,
            BodySelection::Native(_)
        ));
        let aliases = BTreeMap::from([(94, 94), (150, 94)]);
        assert_eq!(
            super::feature_body_selection(
                &[94, 150],
                &aliases,
                &BTreeMap::new(),
                "nx:om-object-indices#94,150".to_string(),
            )
            .selection,
            BodySelection::Local {
                bodies: vec!["nx:om-body-object#94".to_string()],
                native: "nx:om-object-indices#94,150".to_string(),
            }
        );
        let bindings = BTreeMap::from([(94, vec![first.clone()])]);
        assert_eq!(
            super::feature_body_selection(
                &[94],
                &roots,
                &bindings,
                "nx:om-object-index#94".to_string(),
            )
            .selection,
            BodySelection::Resolved {
                bodies: vec![first.clone()],
                native: "nx:om-object-index#94".to_string(),
            }
        );
        assert_eq!(super::feature_body_outputs(94, &bindings), vec![first]);
        assert!(super::feature_body_outputs(123, &bindings).is_empty());
    }

    #[test]
    fn segment_bound_bodies_form_the_exact_retained_history_input() {
        use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
        use cadmpeg_ir::ids::{BodyId, RegionId};
        use cadmpeg_ir::topology::{Body, BodyKind};

        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let bound = BodyId("nx:s2:body#3".to_string());
        ir.model.bodies.extend([
            Body {
                id: bound.clone(),
                kind: BodyKind::Solid,
                regions: vec![RegionId("region-2".to_string())],
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
            Body {
                id: BodyId("nx:s3:body#4".to_string()),
                kind: BodyKind::Solid,
                regions: vec![RegionId("region-3".to_string())],
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        ]);
        let binding = crate::native::segments::SegmentBodyBinding {
            id: "nx:segment-body-bindings:binding#0".to_string(),
            stream_link: "nx:segment-stream-links:link#0".to_string(),
            stream_ordinal: 2,
            stream_kind: "partition".to_string(),
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 100,
        };
        let mut annotations = AnnotationBuilder::new();
        let stream = annotations.stream("nx:container");

        let id =
            super::attach_initial_segment_bodies(&mut ir, &[binding], &mut annotations, stream)
                .expect("one emitted body has an exact segment binding");

        assert_eq!(
            id,
            FeatureId("nx:feature-history:feature#initial-bodies".into())
        );
        assert_eq!(ir.model.features[0].outputs, std::slice::from_ref(&bound));
        assert_eq!(
            ir.model.features[0].definition,
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: vec![bound.clone()],
                    native: "nx:segment-body-bindings".to_string(),
                },
            }
        );
        assert_eq!(
            crate::evaluation::evaluate_saved_body_census(&ir),
            crate::evaluation::BodyCensusEvaluation::Mismatch {
                rederived: vec![bound],
                saved: ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
            }
        );
    }

    #[test]
    fn nx_boolean_retains_disjoint_current_and_input_local_bodies() {
        use cadmpeg_ir::features::{
            BodySelection, BooleanOp, Feature, FeatureDefinition, FeatureId,
        };
        use cadmpeg_ir::ids::BodyId;
        use std::collections::BTreeMap;

        let operation = crate::native::features::FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#0".to_string(),
            kind: crate::native::features::FeatureBooleanKind::Subtract,
            target_object_index: 94,
            raw_target_object_index: vec![94],
            target_source_offset: 0,
            tool_object_indices: vec![122],
            raw_tool_object_indices: vec![vec![122]],
            tool_source_offsets: vec![1],
            source_offset: 0,
        };
        let body = BodyId("nx:s18:body#3".to_string());
        let definition = super::boolean_feature_definition(
            &operation,
            &BTreeMap::from([(94, 94), (122, 122)]),
            &BTreeMap::from([(94, vec![body.clone()])]),
        );

        assert_eq!(
            definition,
            FeatureDefinition::Combine {
                target: BodySelection::Resolved {
                    bodies: vec![body.clone()],
                    native: "nx:om-object-index#94".to_string(),
                },
                tools: BodySelection::Local {
                    bodies: vec!["nx:om-body-object#122".to_string()],
                    native: "nx:om-object-indices#122".to_string(),
                },
                op: BooleanOp::Cut,
                keep_tools: false,
            }
        );
        let feature = Feature {
            id: FeatureId("feature".to_string()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![body],
            definition,
            native_ref: None,
        };
        assert!(!crate::decode::combine_definition_is_incomplete(&feature));
    }

    #[test]
    fn nx_sew_projects_ordered_body_operands_without_inventing_tolerance() {
        use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
        use cadmpeg_ir::ids::BodyId;
        use std::collections::BTreeMap;

        let operand =
            |ordinal, object_index| crate::native::features::FeatureOperationBodyOperand {
                id: format!("operand#{ordinal}"),
                operation_label: "operation#0".to_string(),
                body_object_index: 10,
                body_reference_ordinal: 0,
                ordinal,
                operand_object_index: object_index,
                raw_operand_object_index: vec![object_index as u8],
                operand_data_block: None,
                segment_body_bindings: vec![format!("binding#{ordinal}")],
                source_offset: u64::from(ordinal),
            };
        let operands = [operand(0, 20), operand(1, 30)];
        let references = operands.iter().collect::<Vec<_>>();
        let roots = BTreeMap::from([(10, 10), (20, 20), (30, 30)]);

        assert_eq!(
            super::sew_body_feature_definition(10, &references, &roots, &BTreeMap::new()),
            Some(FeatureDefinition::SewBodies {
                bodies: BodySelection::Local {
                    bodies: vec![
                        "nx:om-body-object#10".to_string(),
                        "nx:om-body-object#20".to_string(),
                        "nx:om-body-object#30".to_string(),
                    ],
                    native: "nx:om-object-indices#10,20,30".to_string(),
                },
                gap_tolerance: None,
            })
        );
        assert!(matches!(
            super::sew_body_feature_definition(
                736,
                &references,
                &roots,
                &BTreeMap::new(),
            ),
            Some(FeatureDefinition::SewBodies {
                bodies: BodySelection::Local { bodies, .. },
                ..
            }) if bodies == [
                "nx:om-body-object#736",
                "nx:om-body-object#20",
                "nx:om-body-object#30",
            ]
        ));
        let resolved = BTreeMap::from([
            (10, vec![BodyId("target".to_string())]),
            (20, vec![BodyId("first-tool".to_string())]),
            (30, vec![BodyId("second-tool".to_string())]),
        ]);
        assert_eq!(
            super::sew_body_feature_definition(10, &references, &roots, &resolved),
            Some(FeatureDefinition::SewBodies {
                bodies: BodySelection::Resolved {
                    bodies: vec![
                        BodyId("target".to_string()),
                        BodyId("first-tool".to_string()),
                        BodyId("second-tool".to_string()),
                    ],
                    native: "nx:om-object-indices#10,20,30".to_string(),
                },
                gap_tolerance: None,
            })
        );
        assert_eq!(
            super::sew_body_feature_definition(10, &[], &roots, &BTreeMap::new()),
            None
        );

        let alias_roots = BTreeMap::from([(10, 10), (20, 20), (30, 20)]);
        assert_eq!(
            super::sew_body_feature_definition(10, &references, &alias_roots, &BTreeMap::new()),
            Some(FeatureDefinition::SewBodies {
                bodies: BodySelection::Local {
                    bodies: vec![
                        "nx:om-body-object#10".to_string(),
                        "nx:om-body-object#20".to_string(),
                    ],
                    native: "nx:om-object-indices#10,20,30".to_string(),
                },
                gap_tolerance: None,
            })
        );
    }

    #[test]
    fn nx_delete_body_requires_a_primary_body_field() {
        use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};
        use std::collections::BTreeMap;

        let roots = BTreeMap::from([(20, 20)]);
        assert_eq!(
            super::delete_body_feature_definition(Some(20), &roots, &BTreeMap::new()),
            Some(FeatureDefinition::DeleteBody {
                bodies: BodySelection::Local {
                    bodies: vec!["nx:om-body-object#20".to_string()],
                    native: "nx:om-object-index#20".to_string(),
                },
                mode: BodyRetentionMode::DeleteSelected,
            })
        );
        assert_eq!(
            super::delete_body_feature_definition(Some(72), &roots, &BTreeMap::new()),
            Some(FeatureDefinition::DeleteBody {
                bodies: BodySelection::Local {
                    bodies: vec!["nx:om-body-object#72".to_string()],
                    native: "nx:om-object-index#72".to_string(),
                },
                mode: BodyRetentionMode::DeleteSelected,
            })
        );
        assert_eq!(
            super::delete_body_feature_definition(None, &roots, &BTreeMap::new()),
            None
        );
    }

    #[test]
    fn nx_extract_body_requires_one_exact_input_store_body() {
        use cadmpeg_ir::features::{BodySelection, FeatureDefinition};

        let body = (736, "nx:om-data-blocks-2:block#736".to_string());
        assert_eq!(
            super::extract_body_feature_definition(std::slice::from_ref(&body)),
            Some(FeatureDefinition::ExtractBody {
                source: BodySelection::Local {
                    bodies: vec!["nx:om-data-blocks-2:block#736".to_string()],
                    native: "nx:om-object-index#736".to_string(),
                },
            })
        );
        assert!(super::extract_body_feature_definition(&[]).is_none());
        assert!(super::extract_body_feature_definition(&[body.clone(), body]).is_none());
    }

    #[test]
    fn nx_trim_body_retains_one_exact_input_store_target() {
        use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition};

        let body = (114, "nx:om-data-blocks-2:block#114".to_string());
        assert_eq!(
            super::offset_store_trim_body_feature_definition(std::slice::from_ref(&body)),
            Some(FeatureDefinition::TrimBodies {
                targets: BodySelection::Local {
                    bodies: vec!["nx:om-data-blocks-2:block#114".to_string()],
                    native: "nx:om-object-index#114".to_string(),
                },
                tools: BodySelection::Unresolved,
                keep: BodyTrimSide::Unresolved,
            })
        );
        assert!(super::offset_store_trim_body_feature_definition(&[]).is_none());
        assert!(super::offset_store_trim_body_feature_definition(&[body.clone(), body]).is_none());
    }

    #[test]
    fn nx_trim_body_projects_distinct_target_and_ordered_tools() {
        use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition};
        use cadmpeg_ir::ids::BodyId;
        use std::collections::BTreeMap;

        let operands = [crate::native::features::FeatureOperationBodyOperand {
            id: "operand#0".to_string(),
            operation_label: "operation#0".to_string(),
            body_object_index: 10,
            body_reference_ordinal: 0,
            ordinal: 0,
            operand_object_index: 20,
            raw_operand_object_index: vec![20],
            operand_data_block: None,
            segment_body_bindings: vec!["binding#0".to_string()],
            source_offset: 0,
        }];
        let references = operands.iter().collect::<Vec<_>>();
        let roots = BTreeMap::from([(10, 10), (20, 20)]);

        assert_eq!(
            super::trim_body_feature_definition(10, &references, &roots, &BTreeMap::new()),
            Some(FeatureDefinition::TrimBodies {
                targets: BodySelection::Local {
                    bodies: vec!["nx:om-body-object#10".to_string()],
                    native: "nx:om-object-index#10".to_string(),
                },
                tools: BodySelection::Local {
                    bodies: vec!["nx:om-body-object#20".to_string()],
                    native: "nx:om-object-indices#20".to_string(),
                },
                keep: BodyTrimSide::Unresolved,
            })
        );
        let resolved = BTreeMap::from([
            (10, vec![BodyId("target".to_string())]),
            (20, vec![BodyId("tool".to_string())]),
        ]);
        assert_eq!(
            super::trim_body_feature_definition(10, &references, &roots, &resolved),
            Some(FeatureDefinition::TrimBodies {
                targets: BodySelection::Resolved {
                    bodies: vec![BodyId("target".to_string())],
                    native: "nx:om-object-index#10".to_string(),
                },
                tools: BodySelection::Resolved {
                    bodies: vec![BodyId("tool".to_string())],
                    native: "nx:om-object-indices#20".to_string(),
                },
                keep: BodyTrimSide::Unresolved,
            })
        );
        assert_eq!(
            super::trim_body_feature_definition(10, &[], &roots, &BTreeMap::new()),
            None
        );

        let same_body = BTreeMap::from([(10, 10), (20, 10)]);
        assert!(matches!(
            super::trim_body_feature_definition(
                10,
                &references,
                &same_body,
                &BTreeMap::new(),
            ),
            Some(FeatureDefinition::TrimBodies {
                targets: BodySelection::Native(target),
                tools: BodySelection::Native(tools),
                ..
            }) if target == "nx:om-object-index#10" && tools == "nx:om-object-indices#20"
        ));
    }

    #[test]
    fn nx_named_operation_families_preserve_unresolved_semantics() {
        assert!(matches!(
            super::non_boolean_feature_definition("SKETCH", &[], None, None, None),
            cadmpeg_ir::features::FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Unresolved,
                sketch: None,
            }
        ));
        assert!(matches!(
            super::non_boolean_feature_definition(
                "SIMPLE HOLE",
                &["Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer"],
                None,
                None,
                None,
            ),
            cadmpeg_ir::features::FeatureDefinition::Hole {
                face: None,
                position: None,
                direction: None,
                kind: cadmpeg_ir::features::HoleKind::Unresolved {
                    form: Some(cadmpeg_ir::features::HoleForm::Chamfer),
                    counterbore_diameter: None,
                    counterbore_depth: None,
                    countersink_diameter: None,
                    countersink_angle: None,
                },
                exit_kind: Some(cadmpeg_ir::features::HoleKind::Unresolved {
                    form: Some(cadmpeg_ir::features::HoleForm::Chamfer),
                    counterbore_diameter: None,
                    counterbore_depth: None,
                    countersink_diameter: None,
                    countersink_angle: None,
                }),
                diameter: None,
                extent: Some(cadmpeg_ir::features::Termination::ThroughAll),
                ..
            }
        ));
        assert!(matches!(
            super::non_boolean_feature_definition("SIMPLE HOLE", &["unrelated"], None, None, None,),
            cadmpeg_ir::features::FeatureDefinition::Hole { extent: None, .. }
        ));
        for competing in [
            "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer",
            "Hole_Unknown",
        ] {
            assert!(matches!(
                super::non_boolean_feature_definition(
                    "SIMPLE HOLE",
                    &[
                        "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer",
                        competing,
                    ],
                    None,
                    None,
                    None,
                ),
                cadmpeg_ir::features::FeatureDefinition::Hole {
                    kind: cadmpeg_ir::features::HoleKind::Simple,
                    exit_kind: None,
                    extent: None,
                    ..
                }
            ));
        }
        assert!(matches!(
            super::non_boolean_feature_definition("DATUM_PLANE", &[], None, None, None),
            cadmpeg_ir::features::FeatureDefinition::DatumPlaneUnresolved
        ));
        assert!(matches!(
            super::non_boolean_feature_definition("DATUM_CSYS", &[], None, None, None),
            cadmpeg_ir::features::FeatureDefinition::DatumCoordinateSystemUnresolved
        ));
        assert!(matches!(
            super::non_boolean_feature_definition("MASTER SNAPSHOT BODY", &[], None, None, None,),
            cadmpeg_ir::features::FeatureDefinition::BaseFeature {
                bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            }
        ));
        assert!(matches!(
            super::non_boolean_feature_definition(
                "TEXT",
                &["annotation", "Arial"],
                None,
                None,
                None,
            ),
            cadmpeg_ir::features::FeatureDefinition::TreeNode {
                role: cadmpeg_ir::features::FeatureTreeNodeRole::Annotations,
                ref children,
                active_child: None,
            } if children.is_empty()
        ));
        assert!(matches!(
            super::non_boolean_feature_definition("TEXT", &["annotation"], None, None, None),
            cadmpeg_ir::features::FeatureDefinition::Native { .. }
        ));
        assert!(matches!(
            super::non_boolean_feature_definition("TEXT", &["", ""], None, None, None),
            cadmpeg_ir::features::FeatureDefinition::TreeNode {
                role: cadmpeg_ir::features::FeatureTreeNodeRole::Annotations,
                ..
            }
        ));
        assert!(matches!(
            super::non_boolean_feature_definition(
                "BLOCK",
                &[],
                Some([10.0, 20.0, 30.0]),
                None,
                None,
            ),
            cadmpeg_ir::features::FeatureDefinition::Block {
                dimensions: Some([
                    cadmpeg_ir::features::Length(10.0),
                    cadmpeg_ir::features::Length(20.0),
                    cadmpeg_ir::features::Length(30.0),
                ]),
                placement: None,
                op: BooleanOp::Unresolved,
            }
        ));
        assert_eq!(
            super::non_boolean_feature_definition("BLOCK", &[], None, None, None),
            cadmpeg_ir::features::FeatureDefinition::Block {
                dimensions: None,
                placement: None,
                op: BooleanOp::Unresolved,
            }
        );
    }

    #[test]
    fn nx_text_payload_projects_semantic_text_and_font_family() {
        let feature = cadmpeg_ir::features::FeatureId("feature#text".to_string());
        let annotation = super::text_semantic_annotation(
            "TEXT",
            &feature,
            "nx:text#1",
            7,
            &["plate label", "Arial"],
        )
        .expect("valid text annotation");
        assert_eq!(annotation.object, feature.0);
        assert_eq!(
            annotation.kind,
            cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text
        );
        assert_eq!(annotation.text, ["plate label"]);
        assert_eq!(annotation.parameters["font_family"], "Arial");
        assert_eq!(annotation.native_ref, "nx:text#1");
        assert_eq!(annotation.order, 7);

        let empty =
            super::text_semantic_annotation("TEXT", &feature, "nx:text#empty", 8, &["", ""])
                .expect("empty text fields remain a valid annotation");
        assert_eq!(empty.text, [""]);
        assert_eq!(empty.parameters["font_family"], "");

        assert!(
            super::text_semantic_annotation("BLOCK", &feature, "nx:block#1", 0, &["10", "20"],)
                .is_none()
        );
        assert!(super::text_semantic_annotation(
            "TEXT",
            &feature,
            "nx:text#2",
            0,
            &["ambiguous", "Arial", "extra"],
        )
        .is_none());
    }

    #[test]
    fn nx_mainstream_operation_labels_project_typed_unresolved_definitions() {
        use cadmpeg_ir::features::{
            BodySelection, BodyTrimSide, BooleanOp, ChamferSpec, EdgeSelection, FaceSelection,
            FeatureDefinition, HoleKind, PatternKind, RadiusSpec, RibDraft,
        };

        for (kind, op) in [
            ("UNITE", BooleanOp::Join),
            ("SUBTRACT", BooleanOp::Cut),
            ("INTERSECT", BooleanOp::Intersect),
        ] {
            assert_eq!(
                super::non_boolean_feature_definition(kind, &[], None, None, None),
                FeatureDefinition::Combine {
                    target: BodySelection::Unresolved,
                    tools: BodySelection::Unresolved,
                    op,
                    keep_tools: false,
                }
            );
        }

        assert_eq!(
            super::non_boolean_feature_definition("EXTRACT_BODY", &[], None, None, None),
            FeatureDefinition::ExtractBody {
                source: BodySelection::Unresolved,
            }
        );
        assert_eq!(
            super::non_boolean_feature_definition("SKIN", &[], None, None, None),
            FeatureDefinition::LoftUnresolved
        );
        assert_eq!(
            super::non_boolean_feature_definition("Studio Surface", &[], None, None, None),
            FeatureDefinition::FreeformSurfaceUnresolved
        );
        assert_eq!(
            super::non_boolean_feature_definition("POINT", &[], None, None, None),
            FeatureDefinition::DatumPointUnresolved
        );
        assert_eq!(
            super::non_boolean_feature_definition("DRAFT", &[], None, None, None),
            FeatureDefinition::DraftUnresolved
        );

        assert!(matches!(
            super::non_boolean_feature_definition("HOLE PACKAGE", &[], None, None, None),
            FeatureDefinition::Hole {
                kind: HoleKind::Unresolved { form: None, .. },
                ..
            }
        ));
        assert!(matches!(
            super::non_boolean_feature_definition(
                "HOLE PACKAGE",
                &[],
                None,
                None,
                Some(cadmpeg_ir::features::Length(8.0)),
            ),
            FeatureDefinition::Hole {
                diameter: Some(cadmpeg_ir::features::Length(8.0)),
                kind: HoleKind::Unresolved { form: None, .. },
                ..
            }
        ));
        assert!(matches!(
            super::non_boolean_feature_definition("RIB", &[], None, None, None),
            FeatureDefinition::Rib {
                construction: cadmpeg_ir::features::RibConstruction {
                    draft: RibDraft::Unresolved,
                    ..
                },
                op: BooleanOp::Unresolved,
            }
        ));
        assert_eq!(
            super::non_boolean_feature_definition("BLEND", &[], None, None, None),
            FeatureDefinition::Fillet {
                groups: vec![cadmpeg_ir::features::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                }],
            }
        );
        assert_eq!(
            super::non_boolean_feature_definition("FACE_BLEND", &[], None, None, None),
            FeatureDefinition::FaceBlend {
                first_faces: FaceSelection::Unresolved,
                second_faces: FaceSelection::Unresolved,
                radius: RadiusSpec::Unresolved { form: None },
            }
        );
        for kind in ["CPROJ", "CPROJ_CMB"] {
            assert_eq!(
                super::non_boolean_feature_definition(kind, &[], None, None, None),
                FeatureDefinition::ProjectedCurve {
                    source: cadmpeg_ir::features::PathRef::Unresolved("nx:unresolved".into()),
                    target_faces: FaceSelection::Unresolved,
                    direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                        cadmpeg_ir::features::CurveProjectionDirectionState::Unresolved,
                    ),
                    bidirectional: None,
                }
            );
        }
        assert_eq!(
            super::non_boolean_feature_definition("TRIMMED_SH", &[], None, None, None),
            FeatureDefinition::TrimSurface {
                faces: FaceSelection::Unresolved,
                tool: cadmpeg_ir::features::PathRef::Unresolved("nx:unresolved".into()),
                keep: cadmpeg_ir::features::TrimRegion::Unresolved,
            }
        );
        assert_eq!(
            super::non_boolean_feature_definition("EXTEND_SHEET", &[], None, None, None),
            FeatureDefinition::ExtendSurface {
                faces: FaceSelection::Unresolved,
                distance: None,
                method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
            }
        );
        assert!(matches!(
            super::non_boolean_feature_definition("CHAMFER", &[], None, None, None),
            FeatureDefinition::Chamfer {
                groups,
                flip_direction: false,
            } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
                edges: EdgeSelection::Unresolved,
                spec: ChamferSpec::Unresolved { form: None },
            }])
        ));
        assert_eq!(
            super::non_boolean_feature_definition("SEW", &[], None, None, None),
            FeatureDefinition::SewBodies {
                bodies: BodySelection::Unresolved,
                gap_tolerance: None,
            }
        );
        assert_eq!(
            super::non_boolean_feature_definition("TRIM BODY", &[], None, None, None),
            FeatureDefinition::TrimBodies {
                targets: BodySelection::Unresolved,
                tools: BodySelection::Unresolved,
                keep: BodyTrimSide::Unresolved,
            }
        );
        assert_eq!(
            super::non_boolean_feature_definition("EXTRUDE", &[], None, None, None),
            FeatureDefinition::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Unresolved("EXTRUDE".into()),
                direction: cadmpeg_ir::features::ExtrudeDirection::Unresolved,
                extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                    side: cadmpeg_ir::features::ExtrudeSide {
                        termination: cadmpeg_ir::features::Termination::Unresolved,
                        draft: None,
                        offset: None,
                    },
                },
                op: BooleanOp::Unresolved,
                start: cadmpeg_ir::features::ExtrudeStart::Unresolved,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }
        );
        assert_eq!(
            super::non_boolean_feature_definition("OFFSET", &[], None, None, None),
            FeatureDefinition::OffsetSurface {
                faces: FaceSelection::Unresolved,
                distance: None,
            }
        );
        assert!(matches!(
            super::non_boolean_feature_definition("THICKEN_SHEET", &[], None, None, None),
            FeatureDefinition::Thicken {
                faces: FaceSelection::Unresolved,
                thickness: None,
                side: None,
            }
        ));
        for kind in [
            "Pattern Feature",
            "Pattern Geometry",
            "Geometry Instance",
            "IDENTICAL INSTANCE OUTPUT",
            "Instance Feature",
        ] {
            assert!(matches!(
                super::non_boolean_feature_definition(kind, &[], None, None, None),
                FeatureDefinition::Pattern {
                    seeds,
                    pattern: PatternKind::Unresolved { form: None },
                } if seeds.is_empty()
            ));
        }
    }

    #[test]
    fn nx_container_record_is_not_a_modeling_feature() {
        assert!(!super::projects_neutral_feature("Container"));
        assert!(super::projects_neutral_feature("EXTRUDE"));
    }

    #[test]
    fn nx_block_placement_requires_native_dimensions_and_unique_axes() {
        let mut ir = cadmpeg_ir::examples::unit_cube();
        let dimensions = [10.0, 20.0, 30.0];
        for axis in 0..3 {
            let mut surfaces = ir
                .model
                .surfaces
                .iter_mut()
                .filter_map(|surface| {
                    let SurfaceGeometry::Plane { origin, normal, .. } = &mut surface.geometry
                    else {
                        return None;
                    };
                    let components = [normal.x.abs(), normal.y.abs(), normal.z.abs()];
                    (components[axis] > 0.5).then_some(origin)
                })
                .collect::<Vec<_>>();
            assert_eq!(surfaces.len(), 2);
            surfaces.sort_by(|first, second| {
                [first.x, first.y, first.z][axis].total_cmp(&[second.x, second.y, second.z][axis])
            });
            match axis {
                0 => {
                    surfaces[0].x = 0.0;
                    surfaces[1].x = dimensions[axis];
                }
                1 => {
                    surfaces[0].y = 0.0;
                    surfaces[1].y = dimensions[axis];
                }
                2 => {
                    surfaces[0].z = 0.0;
                    surfaces[1].z = dimensions[axis];
                }
                _ => unreachable!(),
            }
        }
        let output = ir.model.bodies[0].id.clone();
        let placement = |ir: &CadIr, dimensions, outputs: &[BodyId]| {
            super::block_placement(ir, dimensions, outputs).map(|(_, transform)| transform)
        };

        assert_eq!(
            placement(&ir, dimensions, std::slice::from_ref(&output)),
            Some(cadmpeg_ir::transform::Transform::identity())
        );
        assert_eq!(
            super::block_placement(&ir, dimensions, &[]),
            Some((output.clone(), cadmpeg_ir::transform::Transform::identity()))
        );
        assert_eq!(
            placement(&ir, dimensions, &[]),
            Some(cadmpeg_ir::transform::Transform::identity())
        );
        assert_eq!(
            placement(&ir, dimensions, &[output.clone(), output.clone()],),
            None
        );
        assert_eq!(
            placement(&ir, [10.0, 10.0, 30.0], std::slice::from_ref(&output),),
            None
        );

        let mut repeated = ir.clone();
        let high_y = repeated
            .model
            .surfaces
            .iter_mut()
            .find_map(|surface| {
                let SurfaceGeometry::Plane { origin, normal, .. } = &mut surface.geometry else {
                    return None;
                };
                (normal.y.abs() > 0.5 && origin.y > 0.0).then_some(origin)
            })
            .expect("positive y plane");
        high_y.y = 10.0;
        assert_eq!(
            placement(&repeated, [10.0, 10.0, 30.0], std::slice::from_ref(&output),),
            None
        );

        let mut stepped = ir.clone();
        let mut intermediate_surface = stepped
            .model
            .surfaces
            .iter()
            .find(|surface| {
                matches!(
                    &surface.geometry,
                    SurfaceGeometry::Plane { normal, .. } if normal.x.abs() > 0.5
                )
            })
            .expect("x-normal plane")
            .clone();
        intermediate_surface.id = cadmpeg_ir::ids::SurfaceId("intermediate-plane".into());
        let SurfaceGeometry::Plane { origin, .. } = &mut intermediate_surface.geometry else {
            unreachable!()
        };
        origin.x = 5.0;
        stepped.model.surfaces.push(intermediate_surface);
        let mut intermediate_face = stepped.model.faces.first().expect("cube face").clone();
        intermediate_face.id = cadmpeg_ir::ids::FaceId("intermediate-face".into());
        intermediate_face.surface = cadmpeg_ir::ids::SurfaceId("intermediate-plane".into());
        intermediate_face.loops.clear();
        stepped.model.shells[0]
            .faces
            .push(intermediate_face.id.clone());
        stepped.model.faces.push(intermediate_face);
        assert_eq!(
            placement(&stepped, dimensions, std::slice::from_ref(&output)),
            None
        );

        let mut nonplanar = ir.clone();
        nonplanar.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
            center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        };
        assert_eq!(
            placement(&nonplanar, dimensions, std::slice::from_ref(&output)),
            None
        );

        let mut missing_surface = ir.clone();
        let removed = missing_surface.model.surfaces.pop().expect("cube surface");
        assert!(missing_surface
            .model
            .faces
            .iter()
            .any(|face| face.surface == removed.id));
        assert_eq!(placement(&missing_surface, dimensions, &[]), None);

        let mut curved_feature = ir.clone();
        let mut curved_surface = curved_feature.model.surfaces[0].clone();
        curved_surface.id = cadmpeg_ir::ids::SurfaceId("later-curved-surface".into());
        curved_surface.geometry = SurfaceGeometry::Sphere {
            center: cadmpeg_ir::math::Point3::new(5.0, 10.0, 15.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        };
        curved_feature.model.surfaces.push(curved_surface);
        let mut curved_face = curved_feature.model.faces[0].clone();
        curved_face.id = cadmpeg_ir::ids::FaceId("later-curved-face".into());
        curved_face.surface = cadmpeg_ir::ids::SurfaceId("later-curved-surface".into());
        curved_face.loops.clear();
        curved_feature.model.shells[0]
            .faces
            .push(curved_face.id.clone());
        curved_feature.model.faces.push(curved_face);
        assert_eq!(
            placement(&curved_feature, dimensions, &[]),
            Some(cadmpeg_ir::transform::Transform::identity())
        );

        let mut sheet = ir.clone();
        sheet.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet;
        assert_eq!(
            placement(&sheet, dimensions, std::slice::from_ref(&output)),
            None
        );

        let mut disconnected = ir.clone();
        let mut second_region = disconnected.model.regions[0].clone();
        second_region.id = cadmpeg_ir::ids::RegionId("second-region".into());
        second_region.shells.clear();
        disconnected.model.bodies[0]
            .regions
            .push(second_region.id.clone());
        disconnected.model.regions.push(second_region);
        assert_eq!(
            placement(&disconnected, dimensions, std::slice::from_ref(&output),),
            None
        );
    }

    #[test]
    fn nx_simple_hole_feature_owns_its_exact_native_constructions() {
        use crate::native::features::{
            FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleRepeatedScalarLane,
            FeatureSimpleHoleRepeatedScalarLaneBlockReferences, FeatureSimpleHoleTemplate,
            SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
        };
        let operation = "nx:feature-history:operation-label#1-4";
        let template = FeatureSimpleHoleTemplate {
            id: "template".to_string(),
            operation_label: operation.to_string(),
            payload_string: "string".to_string(),
            family: SimpleHoleFamily::GeneralHole,
            form: SimpleHoleForm::Simple,
            extent: SimpleHoleExtent::Through,
            start_treatment: SimpleHoleEndTreatment::Chamfer,
            end_treatment: SimpleHoleEndTreatment::Chamfer,
        };
        let lane = FeatureSimpleHoleRepeatedScalarLane {
            id: "lane".to_string(),
            operation_label: operation.to_string(),
            values: vec![508.0, 38.1],
            raw_values: vec![[0x30; 8], [0x31; 8]],
            first_witness_offsets: vec![10, 18],
            second_witness_offsets: vec![30, 38],
        };
        let blocks = FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
            id: "blocks".to_string(),
            operation_label: operation.to_string(),
            first_data_blocks: ["block#231".to_string(), "block#232".to_string()],
            second_data_blocks: ["block#233".to_string(), "block#234".to_string()],
            first_reference_offsets: [20, 22],
            second_reference_offsets: [40, 42],
        };
        let group = FeatureSimpleHoleConstructionGroup {
            id: "group".into(),
            first_data_blocks: blocks.first_data_blocks.clone(),
            second_data_blocks: blocks.second_data_blocks.clone(),
            operation_labels: vec![operation.into(), "other-operation".into()],
            scalar_lanes: vec!["lane".into(), "other-lane".into()],
            block_references: vec!["blocks".into(), "other-blocks".into()],
        };
        let properties = super::simple_hole_native_properties(
            operation,
            &[template],
            &[lane],
            &[blocks],
            &[group],
        );
        assert_eq!(properties["simple_hole_template"], "template");
        assert_eq!(properties["simple_hole_repeated_scalar_lane"], "lane");
        assert_eq!(
            properties["simple_hole_repeated_scalar_lane_block_references"],
            "blocks"
        );
        assert_eq!(properties["simple_hole_construction_group"], "group");
        assert!(super::simple_hole_native_properties(
            "nx:feature-history:operation-label#1-5",
            &[],
            &[],
            &[],
            &[],
        )
        .is_empty());
    }

    #[test]
    fn nx_hole_geometry_projection_requires_complete_through_bore_partitions() {
        use crate::native::features::{
            FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleTemplate, SimpleHoleEndTreatment,
            SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
        };
        use cadmpeg_ir::document::{CadIr, Model, IR_VERSION};
        use cadmpeg_ir::features::HolePlacement;
        use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
        use cadmpeg_ir::ids::{
            BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId,
            VertexId,
        };
        use cadmpeg_ir::math::{Point3, Vector3};
        use cadmpeg_ir::native::Native;
        use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Region, Sense, Shell};
        use cadmpeg_ir::units::{Tolerances, Units};
        use cadmpeg_ir::SourceObjectAssociation;

        let operations = ["hole-a".to_string(), "hole-b".to_string()];
        let templates = operations
            .iter()
            .map(|operation| FeatureSimpleHoleTemplate {
                id: format!("template-{operation}"),
                operation_label: operation.clone(),
                payload_string: format!("string-{operation}"),
                family: SimpleHoleFamily::GeneralHole,
                form: SimpleHoleForm::Simple,
                extent: SimpleHoleExtent::Through,
                start_treatment: SimpleHoleEndTreatment::Chamfer,
                end_treatment: SimpleHoleEndTreatment::Chamfer,
            })
            .collect::<Vec<_>>();
        let group = FeatureSimpleHoleConstructionGroup {
            id: "group".into(),
            first_data_blocks: ["a".into(), "b".into()],
            second_data_blocks: ["c".into(), "d".into()],
            operation_labels: operations.to_vec(),
            scalar_lanes: vec!["lane-a".into(), "lane-b".into()],
            block_references: vec!["refs-a".into(), "refs-b".into()],
        };
        let mut model = Model::default();
        for ordinal in 0..2 {
            let surface = SurfaceId(format!("surface-{ordinal}"));
            model.surfaces.push(Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Cylinder {
                    origin: Point3::new(ordinal as f64, 0.0, 0.0),
                    axis: Vector3::new(0.0, 1.0, 0.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 2.55,
                },
                source_object: None::<SourceObjectAssociation>,
            });
            model.faces.push(Face {
                id: FaceId(format!("face-{ordinal}")),
                shell: ShellId("shell".into()),
                surface,
                sense: Sense::Reversed,
                loops: vec![
                    LoopId(format!("loop-{ordinal}-0")),
                    LoopId(format!("loop-{ordinal}-1")),
                ],
                name: None,
                color: None,
                tolerance: None,
            });
            for boundary in 0..2 {
                let loop_id = LoopId(format!("loop-{ordinal}-{boundary}"));
                let curve = CurveId(format!("bore-curve-{ordinal}-{boundary}"));
                let edge = EdgeId(format!("bore-edge-{ordinal}-{boundary}"));
                let coedge = CoedgeId(format!("bore-coedge-{ordinal}-{boundary}"));
                model.curves.push(Curve {
                    id: curve.clone(),
                    geometry: CurveGeometry::Circle {
                        center: Point3::new(ordinal as f64, boundary as f64, 0.0),
                        axis: Vector3::new(0.0, 1.0, 0.0),
                        ref_direction: Vector3::new(1.0, 0.0, 0.0),
                        radius: 2.55,
                    },
                    source_object: None,
                });
                model.edges.push(Edge {
                    id: edge.clone(),
                    curve: Some(curve),
                    start: VertexId("vertex".into()),
                    end: VertexId("vertex".into()),
                    param_range: None,
                    tolerance: None,
                });
                model.coedges.push(Coedge {
                    id: coedge.clone(),
                    owner_loop: loop_id,
                    edge,
                    next: coedge.clone(),
                    previous: coedge.clone(),
                    radial_next: coedge,
                    sense: Sense::Forward,
                    pcurves: Vec::new(),
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
        }
        let body = BodyId("body".into());
        model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Solid,
            regions: vec![RegionId("region".into())],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        model.regions.push(Region {
            id: RegionId("region".into()),
            body: body.clone(),
            shells: vec![ShellId("shell".into())],
        });
        model.shells.push(Shell {
            id: ShellId("shell".into()),
            region: RegionId("region".into()),
            faces: vec![FaceId("face-0".into()), FaceId("face-1".into())],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        let ir = CadIr {
            ir_version: IR_VERSION.into(),
            source: None,
            units: Units::default(),
            tolerances: Tolerances::default(),
            model,
            native: Native::default(),
        };
        let outputs = std::collections::BTreeMap::from([
            ("hole-a".to_string(), vec![body.clone()]),
            ("hole-b".to_string(), vec![body]),
        ]);
        let inferred =
            super::hole_body_projection(&ir, &operations, &std::collections::BTreeMap::new())
                .expect("complete bore bijection");
        assert_eq!(inferred.outputs, outputs);
        assert_eq!(
            simple_hole_diameters(&ir, &templates, std::slice::from_ref(&group), &outputs,),
            std::collections::BTreeMap::from([
                ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
                ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
            ])
        );
        assert_eq!(
            simple_hole_diameters(&ir, &templates, &[], &outputs),
            std::collections::BTreeMap::from([
                ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
                ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
            ])
        );
        assert_eq!(
            hole_diameters_for_operations(&ir, &operations, &outputs),
            std::collections::BTreeMap::from([
                ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
                ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
            ])
        );
        assert!(super::hole_axis_placements_for_operations(&ir, &operations, &outputs).is_empty());
        assert!(super::hole_axis_placements_for_operations(
            &ir,
            &operations,
            &std::collections::BTreeMap::new(),
        )
        .is_empty());
        let mut single_hole = ir.clone();
        single_hole.model.shells[0].faces = vec![FaceId("face-1".into())];
        let single_operation = [operations[1].clone()];
        let single_output = std::collections::BTreeMap::from([(
            operations[1].clone(),
            outputs[&operations[1]].clone(),
        )]);
        assert_eq!(
            super::hole_axis_placements_for_operations(
                &single_hole,
                &single_operation,
                &single_output,
            ),
            std::collections::BTreeMap::from([(
                operations[1].clone(),
                HolePlacement::Axis {
                    origin: Point3::new(1.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 1.0, 0.0),
                },
            )])
        );
        let SurfaceGeometry::Cylinder { origin, .. } = &mut single_hole.model.surfaces[1].geometry
        else {
            unreachable!()
        };
        origin.y = 91.0;
        assert_eq!(
            super::hole_axis_placements_for_operations(
                &single_hole,
                &single_operation,
                &single_output,
            ),
            std::collections::BTreeMap::from([(
                operations[1].clone(),
                HolePlacement::Axis {
                    origin: Point3::new(1.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 1.0, 0.0),
                },
            )])
        );
        let mut opposite_axis = single_hole.clone();
        let SurfaceGeometry::Cylinder { axis, .. } = &mut opposite_axis.model.surfaces[1].geometry
        else {
            unreachable!()
        };
        *axis = Vector3::new(0.0, -1.0, 0.0);
        for curve in opposite_axis
            .model
            .curves
            .iter_mut()
            .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
        {
            let CurveGeometry::Circle { axis, .. } = &mut curve.geometry else {
                unreachable!()
            };
            *axis = Vector3::new(0.0, -1.0, 0.0);
        }
        assert_eq!(
            super::hole_axis_placements_for_operations(
                &opposite_axis,
                &single_operation,
                &single_output,
            ),
            std::collections::BTreeMap::from([(
                operations[1].clone(),
                HolePlacement::Axis {
                    origin: Point3::new(1.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 1.0, 0.0),
                },
            )])
        );
        let mut different_radii = ir.clone();
        let SurfaceGeometry::Cylinder { radius, .. } =
            &mut different_radii.model.surfaces[1].geometry
        else {
            unreachable!()
        };
        *radius = 3.1;
        for curve in different_radii
            .model
            .curves
            .iter_mut()
            .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
        {
            let CurveGeometry::Circle { radius, .. } = &mut curve.geometry else {
                unreachable!()
            };
            *radius = 3.1;
        }
        assert!(hole_diameters_for_operations(&different_radii, &operations, &outputs,).is_empty());
        assert!(super::hole_body_projection(
            &different_radii,
            &operations,
            &std::collections::BTreeMap::new(),
        )
        .is_none());
        assert_eq!(
            simple_hole_diameters(
                &ir,
                &templates,
                std::slice::from_ref(&group),
                &std::collections::BTreeMap::new(),
            ),
            std::collections::BTreeMap::from([
                ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
                ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
            ])
        );
        assert!(hole_diameters_for_operations(
            &ir,
            &[operations[0].clone(), operations[0].clone()],
            &outputs,
        )
        .is_empty());
        let mut invalid_boundary = ir.clone();
        let CurveGeometry::Circle { radius, .. } = &mut invalid_boundary.model.curves[0].geometry
        else {
            unreachable!()
        };
        *radius += 0.1;
        assert!(
            hole_diameters_for_operations(&invalid_boundary, &operations, &outputs,).is_empty()
        );
        let mut coincident_boundaries = ir.clone();
        let CurveGeometry::Circle { center, .. } =
            &mut coincident_boundaries.model.curves[1].geometry
        else {
            unreachable!()
        };
        center.y = 0.0;
        assert!(
            hole_diameters_for_operations(&coincident_boundaries, &operations, &outputs,)
                .is_empty()
        );
        let mut nonparallel = single_hole.clone();
        let SurfaceGeometry::Cylinder { axis, .. } = &mut nonparallel.model.surfaces[1].geometry
        else {
            unreachable!()
        };
        *axis = Vector3::new(0.0, 0.0, 1.0);
        assert!(super::hole_axis_placements_for_operations(
            &nonparallel,
            &single_operation,
            &single_output,
        )
        .is_empty());
        let mut sheet = ir.clone();
        sheet.model.bodies[0].kind = BodyKind::Sheet;
        assert!(hole_diameters_for_operations(&sheet, &operations, &outputs).is_empty());
        let mut disconnected = ir.clone();
        disconnected.model.bodies[0]
            .regions
            .push(RegionId("second-region".into()));
        assert!(hole_diameters_for_operations(&disconnected, &operations, &outputs).is_empty());
        let mut shared_carrier = ir.clone();
        shared_carrier.model.faces.push(Face {
            id: FaceId("unowned-shared-cylinder-face".into()),
            shell: ShellId("unowned-shell".into()),
            surface: SurfaceId("surface-0".into()),
            sense: Sense::Reversed,
            loops: vec![
                LoopId("unowned-loop-a".into()),
                LoopId("unowned-loop-b".into()),
            ],
            name: None,
            color: None,
            tolerance: None,
        });
        assert_eq!(
            simple_hole_diameters(
                &shared_carrier,
                &templates,
                std::slice::from_ref(&group),
                &outputs,
            ),
            simple_hole_diameters(&ir, &templates, std::slice::from_ref(&group), &outputs,)
        );

        let mut distinct = ir.clone();
        distinct.model.shells[0].faces.pop();
        distinct.model.bodies.push(Body {
            id: BodyId("second-body".into()),
            kind: BodyKind::Solid,
            regions: vec![RegionId("second-region".into())],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        distinct.model.regions.push(Region {
            id: RegionId("second-region".into()),
            body: BodyId("second-body".into()),
            shells: vec![ShellId("second-shell".into())],
        });
        distinct.model.shells.push(Shell {
            id: ShellId("second-shell".into()),
            region: RegionId("second-region".into()),
            faces: vec![FaceId("face-1".into())],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        distinct.model.faces[1].shell = ShellId("second-shell".into());
        let SurfaceGeometry::Cylinder { radius, .. } = &mut distinct.model.surfaces[1].geometry
        else {
            unreachable!()
        };
        *radius = 3.0;
        for curve in distinct
            .model
            .curves
            .iter_mut()
            .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
        {
            let CurveGeometry::Circle { radius, .. } = &mut curve.geometry else {
                unreachable!()
            };
            *radius = 3.0;
        }
        let distinct_outputs = std::collections::BTreeMap::from([
            ("hole-a".to_string(), vec![BodyId("body".into())]),
            ("hole-b".to_string(), vec![BodyId("second-body".into())]),
        ]);
        assert_eq!(
            simple_hole_diameters(
                &distinct,
                &templates,
                std::slice::from_ref(&group),
                &distinct_outputs,
            ),
            std::collections::BTreeMap::from([
                ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
                ("hole-b".into(), cadmpeg_ir::features::Length(6.0)),
            ])
        );
        assert_eq!(
            hole_diameters_for_operations(&distinct, &operations, &distinct_outputs,),
            std::collections::BTreeMap::from([
                ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
                ("hole-b".into(), cadmpeg_ir::features::Length(6.0)),
            ])
        );
        assert!(hole_diameters_for_operations(
            &distinct,
            &operations,
            &std::collections::BTreeMap::new(),
        )
        .is_empty());
        assert!(hole_diameters_for_operations(
            &ir,
            &operations,
            &std::collections::BTreeMap::from([(
                "hole-a".to_string(),
                vec![BodyId("body".into())],
            )]),
        )
        .is_empty());

        let mut chamfered = ir.clone();
        for bore in 0..2 {
            for end in 0..2 {
                let surface = SurfaceId(format!("cone-{bore}-{end}"));
                let face = FaceId(format!("cone-face-{bore}-{end}"));
                let loops = [
                    LoopId(format!("cone-loop-{bore}-{end}-inner")),
                    LoopId(format!("cone-loop-{bore}-{end}-outer")),
                ];
                chamfered.model.surfaces.push(Surface {
                    id: surface.clone(),
                    geometry: SurfaceGeometry::Cone {
                        origin: Point3::new(bore as f64, end as f64, 0.0),
                        axis: Vector3::new(0.0, if end == 0 { 1.0 } else { -1.0 }, 0.0),
                        ref_direction: Vector3::new(1.0, 0.0, 0.0),
                        radius: 0.0,
                        ratio: 1.0,
                        half_angle: std::f64::consts::FRAC_PI_4,
                    },
                    source_object: None,
                });
                chamfered.model.shells[0].faces.push(face.clone());
                chamfered.model.faces.push(Face {
                    id: face,
                    shell: ShellId("shell".into()),
                    surface,
                    sense: Sense::Reversed,
                    loops: loops.to_vec(),
                    name: None,
                    color: None,
                    tolerance: None,
                });
                for (boundary, (loop_id, radius)) in loops.into_iter().zip([2.55, 3.55]).enumerate()
                {
                    let curve = CurveId(format!("cone-curve-{bore}-{end}-{boundary}"));
                    let edge = EdgeId(format!("cone-edge-{bore}-{end}-{boundary}"));
                    let coedge = CoedgeId(format!("cone-coedge-{bore}-{end}-{boundary}"));
                    chamfered.model.curves.push(Curve {
                        id: curve.clone(),
                        geometry: CurveGeometry::Circle {
                            center: Point3::new(bore as f64, end as f64, 0.0),
                            axis: Vector3::new(0.0, 1.0, 0.0),
                            ref_direction: Vector3::new(1.0, 0.0, 0.0),
                            radius,
                        },
                        source_object: None,
                    });
                    chamfered.model.edges.push(Edge {
                        id: edge.clone(),
                        curve: Some(curve),
                        start: VertexId("vertex".into()),
                        end: VertexId("vertex".into()),
                        param_range: None,
                        tolerance: None,
                    });
                    chamfered.model.coedges.push(Coedge {
                        id: coedge.clone(),
                        owner_loop: loop_id,
                        edge,
                        next: coedge.clone(),
                        previous: coedge.clone(),
                        radial_next: coedge,
                        sense: Sense::Forward,
                        pcurves: Vec::new(),
                        use_curve: None,
                        use_curve_parameter_range: None,
                    });
                }
            }
        }
        assert_eq!(
            super::simple_hole_chamfers(&chamfered, &templates, &outputs),
            std::collections::BTreeMap::from([
                (
                    "hole-a".into(),
                    cadmpeg_ir::features::HoleKind::Chamfer {
                        diameter: cadmpeg_ir::features::Length(7.1),
                        angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
                    },
                ),
                (
                    "hole-b".into(),
                    cadmpeg_ir::features::HoleKind::Chamfer {
                        diameter: cadmpeg_ir::features::Length(7.1),
                        angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
                    },
                ),
            ])
        );
        assert_eq!(
            super::simple_hole_chamfers(&chamfered, &templates, &std::collections::BTreeMap::new(),),
            super::simple_hole_chamfers(&chamfered, &templates, &outputs)
        );
        let mut sheet = chamfered.clone();
        sheet.model.bodies[0].kind = BodyKind::Sheet;
        assert!(super::simple_hole_chamfers(&sheet, &templates, &outputs).is_empty());
        let mut unrelated = chamfered.clone();
        unrelated.model.surfaces.push(Surface {
            id: SurfaceId("unrelated-cone".into()),
            geometry: SurfaceGeometry::Cone {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 0.0,
                ratio: 0.0,
                half_angle: 0.0,
            },
            source_object: None,
        });
        unrelated.model.faces.push(Face {
            id: FaceId("unrelated-cone-face".into()),
            shell: ShellId("unrelated-shell".into()),
            surface: SurfaceId("unrelated-cone".into()),
            sense: Sense::Reversed,
            loops: vec![LoopId("unrelated-a".into()), LoopId("unrelated-b".into())],
            name: None,
            color: None,
            tolerance: None,
        });
        assert_eq!(
            super::simple_hole_chamfers(&unrelated, &templates, &outputs),
            super::simple_hole_chamfers(&chamfered, &templates, &outputs)
        );
        let mut unequal_chamfers = chamfered;
        let CurveGeometry::Circle { radius, .. } = &mut unequal_chamfers
            .model
            .curves
            .last_mut()
            .expect("required invariant")
            .geometry
        else {
            unreachable!()
        };
        *radius += 0.1;
        assert!(super::simple_hole_chamfers(&unequal_chamfers, &templates, &outputs).is_empty());

        let mut mismatched = ir;
        let SurfaceGeometry::Cylinder { radius, .. } = &mut mismatched.model.surfaces[1].geometry
        else {
            unreachable!()
        };
        *radius = 3.0;
        assert!(simple_hole_diameters(&mismatched, &templates, &[group], &outputs,).is_empty());
    }

    #[test]
    fn nx_offset_feature_requires_one_output_image_and_one_exact_distance() {
        use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};
        use cadmpeg_ir::geometry::ProceduralSurface;
        use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

        let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
        let output = BodyId("nx:s4:body#3".into());
        let make_offset = |ordinal: u32, distance: f64| ProceduralSurface {
            id: ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
            surface: SurfaceId(format!("nx:s4:offset-surf#{ordinal}")),
            definition: ProceduralSurfaceDefinition::Offset {
                support: SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
                distance,
                u_sense: Some(1),
                v_sense: Some(1),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        };
        for ordinal in 0..2 {
            let procedural = make_offset(ordinal, 30.0);
            attach_test_body_surface(&mut ir, &output, procedural.surface.clone());
            ir.model.procedural_surfaces.push(procedural);
        }

        let (definition, supports) =
            super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
                .expect("unique offset distance");
        assert_eq!(supports.len(), 2);
        assert!(matches!(
            definition,
            FeatureDefinition::OffsetSurface {
                faces: FaceSelection::Native(_),
                distance: None,
            }
        ));

        let input = BodyId("nx:s4:body#input".into());
        for ordinal in 0..2 {
            attach_test_body_surface(
                &mut ir,
                &input,
                SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
            );
        }
        let (definition, _) =
            super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
                .expect("uniquely faced supports");
        assert!(matches!(
            definition,
            FeatureDefinition::OffsetSurface {
                faces: FaceSelection::Resolved { faces, .. },
                distance: Some(cadmpeg_ir::features::Length(30.0)),
            } if faces.len() == 2
        ));

        for face in ir.model.faces.iter_mut().filter(|face| {
            face.surface.0 == "nx:s4:nurbs-surf#0" || face.surface.0 == "nx:s4:nurbs-surf#1"
        }) {
            face.sense = cadmpeg_ir::topology::Sense::Reversed;
        }
        let (definition, _) =
            super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
                .expect("uniformly reversed support faces");
        assert!(matches!(
            definition,
            FeatureDefinition::OffsetSurface {
                distance: Some(cadmpeg_ir::features::Length(-30.0)),
                ..
            }
        ));

        ir.model
            .faces
            .iter_mut()
            .find(|face| face.surface == SurfaceId("nx:s4:nurbs-surf#0".into()))
            .expect("first support face")
            .sense = cadmpeg_ir::topology::Sense::Forward;
        let (definition, _) =
            super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
                .expect("mixed support-face orientations retain offset family");
        assert!(matches!(
            definition,
            FeatureDefinition::OffsetSurface {
                faces: FaceSelection::Resolved { .. },
                distance: None,
            }
        ));

        let mut ambiguous = ir.clone();
        attach_test_body_surface(
            &mut ambiguous,
            &BodyId("nx:s4:body#duplicate".into()),
            SurfaceId("nx:s4:nurbs-surf#0".into()),
        );
        let (definition, _) =
            super::offset_surface_feature_definition(&ambiguous, std::slice::from_ref(&output))
                .expect("offset semantics survive ambiguous face identity");
        assert!(matches!(
            definition,
            FeatureDefinition::OffsetSurface {
                faces: FaceSelection::Native(_),
                distance: None,
            }
        ));

        ir.model.procedural_surfaces.push(make_offset(99, -40.0));
        assert!(
            super::offset_surface_feature_definition(&ir, std::slice::from_ref(&output)).is_some()
        );
        ir.model.procedural_surfaces.pop();

        let conflicting = make_offset(2, -30.0);
        attach_test_body_surface(&mut ir, &output, conflicting.surface.clone());
        ir.model.procedural_surfaces.push(conflicting);
        assert!(super::offset_surface_feature_definition(&ir, &[output]).is_none());
    }

    #[test]
    fn nx_thicken_feature_uses_the_magnitude_of_one_owned_offset_distance() {
        use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};
        use cadmpeg_ir::geometry::ProceduralSurface;
        use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

        let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
        let output = BodyId("nx:s4:body#3".into());
        let make_offset = |ordinal: u32, distance: f64| ProceduralSurface {
            id: ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
            surface: SurfaceId(format!("nx:s4:offset-surf#{ordinal}")),
            definition: ProceduralSurfaceDefinition::Offset {
                support: SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
                distance,
                u_sense: Some(1),
                v_sense: Some(1),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        };
        for ordinal in 0..2 {
            let procedural = make_offset(ordinal, -12.5);
            attach_test_body_surface(&mut ir, &output, procedural.surface.clone());
            ir.model.procedural_surfaces.push(procedural);
        }

        let (definition, supports) =
            super::thicken_feature_definition(&ir, std::slice::from_ref(&output))
                .expect("unique nonzero offset distance");
        assert_eq!(supports.len(), 2);
        assert!(matches!(
            definition,
            FeatureDefinition::Thicken {
                faces: FaceSelection::Native(_),
                thickness: Some(Length(12.5)),
                side: None,
            }
        ));

        let mut sheet_output = ir.clone();
        sheet_output
            .model
            .bodies
            .iter_mut()
            .find(|body| body.id == output)
            .expect("output body")
            .kind = cadmpeg_ir::topology::BodyKind::Sheet;
        assert!(
            super::thicken_feature_definition(&sheet_output, std::slice::from_ref(&output))
                .is_none()
        );

        let input = BodyId("nx:s4:body#input".into());
        for ordinal in 0..2 {
            attach_test_body_surface(
                &mut ir,
                &input,
                SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
            );
        }
        let (definition, _) = super::thicken_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("uniquely faced supports");
        assert!(matches!(
            definition,
            FeatureDefinition::Thicken {
                faces: FaceSelection::Resolved { faces, .. },
                side: Some(ThickenSide::Reverse),
                ..
            } if faces.len() == 2
        ));

        ir.model
            .faces
            .iter_mut()
            .find(|face| face.surface == SurfaceId("nx:s4:nurbs-surf#1".into()))
            .expect("second support face")
            .sense = cadmpeg_ir::topology::Sense::Reversed;
        let (definition, _) = super::thicken_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("mixed support senses preserve thicken semantics");
        assert!(matches!(
            definition,
            FeatureDefinition::Thicken {
                faces: FaceSelection::Resolved { .. },
                side: None,
                ..
            }
        ));

        ir.model.procedural_surfaces.push(make_offset(99, 40.0));
        assert!(super::thicken_feature_definition(&ir, std::slice::from_ref(&output)).is_some());
        ir.model.procedural_surfaces.pop();

        let conflicting = make_offset(2, 12.5);
        attach_test_body_surface(&mut ir, &output, conflicting.surface.clone());
        ir.model.procedural_surfaces.push(conflicting);
        assert!(super::thicken_feature_definition(&ir, &[output]).is_none());

        let zero_output = BodyId("nx:s4:body#4".into());
        let zero = make_offset(3, 0.0);
        attach_test_body_surface(&mut ir, &zero_output, zero.surface.clone());
        ir.model.procedural_surfaces.push(zero);
        assert!(super::thicken_feature_definition(&ir, &[zero_output]).is_none());
    }

    #[test]
    fn nx_thicken_symmetric_offsets_require_identical_support_sets() {
        use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};
        use cadmpeg_ir::geometry::ProceduralSurface;
        use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

        let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
        let output = BodyId("nx:s4:body#symmetric".into());
        let input = BodyId("nx:s4:body#input".into());
        let support = SurfaceId("nx:s4:nurbs-surf#0".into());
        attach_test_body_surface(&mut ir, &input, support.clone());
        let make_offset = |ordinal: u32, support: SurfaceId, distance: f64| ProceduralSurface {
            id: ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
            surface: SurfaceId(format!("nx:s4:offset-surf#{ordinal}")),
            definition: ProceduralSurfaceDefinition::Offset {
                support,
                distance,
                u_sense: Some(1),
                v_sense: Some(1),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        };
        for (ordinal, distance) in [(0, -6.25), (1, 6.25)] {
            let procedural = make_offset(ordinal, support.clone(), distance);
            attach_test_body_surface(&mut ir, &output, procedural.surface.clone());
            ir.model.procedural_surfaces.push(procedural);
        }

        let (definition, supports) =
            super::thicken_feature_definition(&ir, std::slice::from_ref(&output))
                .expect("matched symmetric offsets");
        assert_eq!(supports, std::slice::from_ref(&support));
        assert!(matches!(
            definition,
            FeatureDefinition::Thicken {
                faces: FaceSelection::Resolved { faces, .. },
                thickness: Some(Length(12.5)),
                side: Some(ThickenSide::Both),
            } if faces.len() == 1
        ));

        let mut mismatched_support = ir.clone();
        let ProceduralSurfaceDefinition::Offset { support, .. } = &mut mismatched_support
            .model
            .procedural_surfaces
            .last_mut()
            .expect("positive offset")
            .definition
        else {
            unreachable!()
        };
        *support = SurfaceId("nx:s4:nurbs-surf#other".into());
        assert!(super::thicken_feature_definition(
            &mismatched_support,
            std::slice::from_ref(&output)
        )
        .is_none());

        let ProceduralSurfaceDefinition::Offset { distance, .. } = &mut ir
            .model
            .procedural_surfaces
            .last_mut()
            .expect("positive offset")
            .definition
        else {
            unreachable!()
        };
        *distance = 7.0;
        assert!(super::thicken_feature_definition(&ir, std::slice::from_ref(&output)).is_none());
    }

    #[test]
    fn nx_blend_feature_requires_one_output_image_and_circular_result_carriers() {
        use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, RadiusForm, RadiusSpec};
        use cadmpeg_ir::geometry::{
            BlendCrossSection, BlendRadiusLaw, BlendSupport, ProceduralSurface,
            ProceduralSurfaceDefinition,
        };
        use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

        let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
        let output = BodyId("nx:s4:body#3".into());
        let support_a = SurfaceId("support-a".into());
        let support_b = SurfaceId("support-b".into());
        let support_c = SurfaceId("support-c".into());
        assert_eq!(
            super::blend_support_bipartition(vec![
                [support_a.clone(), support_b.clone()],
                [support_b.clone(), support_c.clone()],
            ]),
            Some((
                vec![support_a.clone(), support_c.clone()],
                vec![support_b.clone()],
            ))
        );
        assert!(super::blend_support_bipartition(vec![
            [support_a.clone(), support_b.clone()],
            [support_b.clone(), support_c.clone()],
            [support_c, support_a],
        ])
        .is_none());
        assert!(super::blend_support_bipartition(vec![
            [SurfaceId("a".into()), SurfaceId("b".into())],
            [SurfaceId("c".into()), SurfaceId("d".into())],
        ])
        .is_none());
        let make_blend = |ordinal: u32, radius: BlendRadiusLaw| ProceduralSurface {
            id: ProceduralSurfaceId(format!("nx:s4:blend-construction#{ordinal}")),
            surface: SurfaceId(format!("nx:s4:blend-surf#{ordinal}")),
            definition: ProceduralSurfaceDefinition::Blend {
                supports: [None, None],
                spine: None,
                radius,
                cross_section: BlendCrossSection::Circular,
                native: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        };
        let first = make_blend(0, BlendRadiusLaw::Constant { signed_radius: 5.0 });
        attach_test_body_surface(&mut ir, &output, first.surface.clone());
        ir.model.procedural_surfaces.push(first);
        let second = make_blend(
            1,
            BlendRadiusLaw::Constant {
                signed_radius: -5.0,
            },
        );
        attach_test_body_surface(&mut ir, &output, second.surface.clone());
        ir.model.procedural_surfaces.push(second);

        let (definition, surfaces) = super::blend_feature_definition(
            &ir,
            std::slice::from_ref(&output),
            super::NxBlendFamily::Edge,
        )
        .expect("one circular constant-radius blend result");
        assert_eq!(surfaces.len(), 2);
        assert!(matches!(
            definition,
            FeatureDefinition::Fillet {
                groups
            } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Constant { radius: cadmpeg_ir::features::Length(5.0) },
                ..
            }])
        ));
        let (definition, _) = super::blend_feature_definition(
            &ir,
            std::slice::from_ref(&output),
            super::NxBlendFamily::Face,
        )
        .expect("face blend retains unresolved supports");
        assert!(matches!(
            definition,
            FeatureDefinition::FaceBlend {
                first_faces: FaceSelection::Unresolved,
                second_faces: FaceSelection::Unresolved,
                radius: RadiusSpec::Constant { .. },
            }
        ));

        let mut face_blend_ir = ir.clone();
        let first_support = SurfaceId("nx:s4:blend-support#a".into());
        let second_support = SurfaceId("nx:s4:blend-support#b".into());
        for procedural in &mut face_blend_ir.model.procedural_surfaces {
            let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut procedural.definition
            else {
                unreachable!()
            };
            *supports = [
                Some(BlendSupport {
                    surface: first_support.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: second_support.clone(),
                    reversed: true,
                }),
            ];
        }
        attach_test_body_surface(&mut face_blend_ir, &output, first_support);
        attach_test_body_surface(&mut face_blend_ir, &output, second_support);
        let (definition, _) = super::blend_feature_definition(
            &face_blend_ir,
            std::slice::from_ref(&output),
            super::NxBlendFamily::Edge,
        )
        .expect("complete blend supports");
        assert!(matches!(
            definition,
            FeatureDefinition::FaceBlend {
                first_faces: FaceSelection::Resolved { ref faces, .. },
                second_faces: FaceSelection::Resolved {
                    faces: ref second,
                    ..
                },
                radius: RadiusSpec::Constant { .. },
            } if faces.len() == 1 && second.len() == 1 && faces != second
        ));

        ir.model.procedural_surfaces.push(make_blend(
            99,
            BlendRadiusLaw::Constant {
                signed_radius: 17.0,
            },
        ));
        let (definition, _) = super::blend_feature_definition(
            &ir,
            std::slice::from_ref(&output),
            super::NxBlendFamily::Edge,
        )
        .expect("required invariant");
        assert!(matches!(
            definition,
            FeatureDefinition::Fillet {
                groups
            } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Constant { radius: cadmpeg_ir::features::Length(5.0) },
                ..
            }])
        ));
        ir.model.procedural_surfaces.pop();

        let conflicting = make_blend(2, BlendRadiusLaw::Constant { signed_radius: 7.0 });
        attach_test_body_surface(&mut ir, &output, conflicting.surface.clone());
        ir.model.procedural_surfaces.push(conflicting);
        let (definition, _) =
            super::blend_feature_definition(&ir, &[output], super::NxBlendFamily::Edge)
                .expect("required invariant");
        assert!(matches!(
            definition,
            FeatureDefinition::Fillet {
                groups
            } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
                radius: RadiusSpec::Unresolved { form: Some(RadiusForm::Constant) },
                ..
            }])
        ));
        assert!(super::blend_feature_definition(&ir, &[], super::NxBlendFamily::Edge,).is_none());

        let conic = ProceduralSurface {
            id: ProceduralSurfaceId("nx:s4:blend-construction#3".into()),
            surface: SurfaceId("nx:s4:blend-surf#3".into()),
            definition: ProceduralSurfaceDefinition::Blend {
                supports: [None, None],
                spine: None,
                radius: BlendRadiusLaw::Constant { signed_radius: 7.0 },
                cross_section: BlendCrossSection::Conic,
                native: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        };
        attach_test_body_surface(
            &mut ir,
            &BodyId("nx:s4:body#3".into()),
            conic.surface.clone(),
        );
        ir.model.procedural_surfaces.push(conic);
        assert!(super::blend_feature_definition(
            &ir,
            &[BodyId("nx:s4:body#3".into())],
            super::NxBlendFamily::Edge,
        )
        .is_none());
    }

    #[test]
    fn nx_construction_dependency_requires_a_preceding_projected_operation() {
        use std::collections::BTreeMap;

        use cadmpeg_ir::features::FeatureId;

        let positions = BTreeMap::from([("csys", 1), ("consumer", 2), ("later", 3)]);
        let features = BTreeMap::from([
            ("csys", FeatureId("nx:test:feature#csys".into())),
            ("consumer", FeatureId("nx:test:feature#consumer".into())),
        ]);

        assert_eq!(
            super::preceding_operation_dependency("csys", 2, &positions, &features),
            Some(FeatureId("nx:test:feature#csys".into()))
        );
        assert_eq!(
            super::preceding_operation_dependency("consumer", 2, &positions, &features),
            None
        );
        assert_eq!(
            super::preceding_operation_dependency("later", 2, &positions, &features),
            None
        );
        assert_eq!(
            super::preceding_operation_dependency("missing", 2, &positions, &features),
            None
        );
    }

    #[test]
    fn topology_numeric_attribute_values_transfer_in_native_lane_order() {
        use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
        use cadmpeg_ir::ids::{FaceId, LoopId, ShellId};
        use cadmpeg_ir::AnnotationBuilder;

        use crate::native::parasolid::{
            ParasolidAttributeDefinition, ParasolidEntity51NumericKind,
            ParasolidEntity51NumericUse, ParasolidEntity52IntegerRecord,
            ParasolidEntity53DoubleRecord, ParasolidTopologyAttributeClassUse,
            ParasolidTopologyAttributeListReference,
        };

        let mut ir = cadmpeg_ir::examples::unit_cube();
        ir.model.shells[0].id = ShellId("nx:s3:shell#58".into());
        ir.model.faces[0].id = FaceId("nx:s3:face#60".into());
        ir.model.loops[0].id = LoopId("nx:s3:loop#59".into());
        let references = [(13, 58), (14, 60), (15, 59)].map(|(topology_type, topology_xmt)| {
            ParasolidTopologyAttributeListReference {
                id: format!("topology-reference-{topology_type}"),
                stream_ordinal: 3,
                topology_type,
                topology_xmt,
                attribute_list_xmt: 50,
                attribute_list_record: Some("entity".into()),
                inflated_offset: 300,
            }
        });
        let integer = ParasolidEntity52IntegerRecord {
            id: "integers".into(),
            stream_ordinal: 3,
            xmt: 70,
            values: vec![4, u32::MAX],
            byte_len: 18,
            inflated_offset: 400,
        };
        let double = ParasolidEntity53DoubleRecord {
            id: "doubles".into(),
            stream_ordinal: 3,
            xmt: 71,
            values: vec![0.25, 7.5],
            byte_len: 26,
            inflated_offset: 500,
        };
        let uses = [
            ParasolidEntity51NumericUse {
                id: "double-use".into(),
                stream_ordinal: 3,
                entity_51_record: "entity".into(),
                reference_ordinal: 4,
                referenced_xmt: 71,
                kind: ParasolidEntity51NumericKind::Doubles,
                value_record: double.id.clone(),
                inflated_offset: 200,
            },
            ParasolidEntity51NumericUse {
                id: "integer-use".into(),
                stream_ordinal: 3,
                entity_51_record: "entity".into(),
                reference_ordinal: 3,
                referenced_xmt: 70,
                kind: ParasolidEntity51NumericKind::UnsignedIntegers,
                value_record: integer.id.clone(),
                inflated_offset: 200,
            },
        ];
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 3,
            xmt: 34,
            next_definition_xmt: 1,
            identifier_xmt: 35,
            identifier_inflated_offset: 90,
            name: "SDL/TYSA_DENSITY".into(),
            type_id: 8004,
            action_codes: [0; 8],
            field_names_xmt: 1,
            legal_owner_flags: [0; 16],
            field_count: 1,
            field_codes: vec![2],
            inflated_offset: 100,
        };
        let class_use = ParasolidTopologyAttributeClassUse {
            id: "class-use".into(),
            topology_attribute_reference: references[2].id.clone(),
            entity_51_record: "entity".into(),
            attribute_class_use: "attribute-class-use".into(),
            definition_xmt: definition.xmt,
            attribute_definition: definition.id.clone(),
        };
        let mut annotations = AnnotationBuilder::new();

        super::attach_parasolid_topology_numeric_attributes(
            &mut ir,
            &super::ParasolidNumericAttributeSources {
                topology_references: &references,
                class_uses: &[class_use],
                definitions: &[definition],
                field_uses: &[],
                field_names: &[],
                numeric_uses: &uses,
                integers: &[integer],
                doubles: &[double],
            },
            &mut annotations,
        );

        let attributes = ir
            .model
            .attributes
            .iter()
            .filter(|attribute| attribute.id.0.contains("topology-numeric-attribute"))
            .collect::<Vec<_>>();
        assert_eq!(attributes.len(), 6);
        assert_eq!(
            attributes[0].target,
            AttributeTarget::Shell(ShellId("nx:s3:shell#58".into()))
        );
        assert_eq!(attributes[0].name, "parasolid_type_integer_reference_3");
        assert_eq!(
            attributes[4].name,
            "SDL/TYSA_DENSITY.parasolid_type_integer_reference_3"
        );
        assert_eq!(
            attributes[0].values,
            [
                AttributeValue::Integer(4),
                AttributeValue::Integer(i64::from(u32::MAX))
            ]
        );
        for (attributes, target) in [
            (
                &attributes[0..2],
                AttributeTarget::Shell(ShellId("nx:s3:shell#58".into())),
            ),
            (
                &attributes[2..4],
                AttributeTarget::Face(FaceId("nx:s3:face#60".into())),
            ),
            (
                &attributes[4..6],
                AttributeTarget::Loop(LoopId("nx:s3:loop#59".into())),
            ),
        ] {
            assert!(attributes
                .iter()
                .all(|attribute| attribute.target == target));
            assert_eq!(
                attributes[1].values,
                [AttributeValue::Float(0.25), AttributeValue::Float(7.5)]
            );
        }
    }

    #[test]
    fn topology_attribute_field_names_use_unique_declared_assignments() {
        use crate::native::parasolid::{
            ParasolidAttributeDefinition, ParasolidAttributeFieldNames, ParasolidAttributeFieldUse,
            ParasolidAttributeFieldValueKind, ParasolidTopologyAttributeClassUse,
            ParasolidTopologyAttributeListReference,
        };

        let reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 3,
            topology_type: 14,
            topology_xmt: 60,
            attribute_list_xmt: 50,
            attribute_list_record: Some("entity".into()),
            inflated_offset: 300,
        };
        let definition = ParasolidAttributeDefinition {
            id: "definition".into(),
            stream_ordinal: 3,
            xmt: 34,
            next_definition_xmt: 1,
            identifier_xmt: 35,
            identifier_inflated_offset: 90,
            name: "SDL/TYSA_DENSITY".into(),
            type_id: 8004,
            action_codes: [0; 8],
            field_names_xmt: 1,
            legal_owner_flags: [0; 16],
            field_count: 2,
            field_codes: vec![2, 3],
            inflated_offset: 100,
        };
        let class_use = ParasolidTopologyAttributeClassUse {
            id: "topology-class-use".into(),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: "entity".into(),
            attribute_class_use: "attribute-class-use".into(),
            definition_xmt: definition.xmt,
            attribute_definition: definition.id.clone(),
        };
        let field_use = ParasolidAttributeFieldUse {
            id: "field-use".into(),
            stream_ordinal: 3,
            attribute_class_use: "attribute-class-use".into(),
            entity_51_record: "entity".into(),
            attribute_definition: definition.id.clone(),
            field_ordinal: 0,
            field_code: 2,
            reference_ordinal: 5,
            value_kind: ParasolidAttributeFieldValueKind::Doubles,
            value_use: "double-use".into(),
            value_record: "double-record".into(),
            inflated_offset: 200,
        };

        assert_eq!(
            super::parasolid_topology_attribute_field_name(
                &reference,
                "double-use",
                std::slice::from_ref(&class_use),
                std::slice::from_ref(&definition),
                std::slice::from_ref(&field_use),
                &[],
            )
            .as_deref(),
            Some("SDL/TYSA_DENSITY.density")
        );

        let units = ParasolidAttributeFieldUse {
            field_ordinal: 1,
            field_code: 3,
            reference_ordinal: 6,
            value_kind: ParasolidAttributeFieldValueKind::String,
            value_use: "string-use".into(),
            value_record: "string-record".into(),
            ..field_use.clone()
        };
        assert_eq!(
            super::parasolid_topology_attribute_field_name(
                &reference,
                "string-use",
                std::slice::from_ref(&class_use),
                std::slice::from_ref(&definition),
                &[units],
                &[],
            )
            .as_deref(),
            Some("SDL/TYSA_DENSITY.units")
        );

        let named_definition = ParasolidAttributeDefinition {
            name: "PVM/25_1".into(),
            field_names_xmt: 25,
            ..definition.clone()
        };
        let field_names = ParasolidAttributeFieldNames {
            id: "field-names-relation".into(),
            stream_ordinal: 3,
            attribute_definition: named_definition.id.clone(),
            field_names_record: "field-names-record".into(),
            value_records: vec!["name-1".into(), "name-2".into()],
            names: vec!["width".into(), "units".into()],
        };
        assert_eq!(
            super::parasolid_topology_attribute_field_name(
                &reference,
                "double-use",
                std::slice::from_ref(&class_use),
                std::slice::from_ref(&named_definition),
                std::slice::from_ref(&field_use),
                std::slice::from_ref(&field_names),
            )
            .as_deref(),
            Some("PVM/25_1.width")
        );

        let duplicate_class = ParasolidTopologyAttributeClassUse {
            id: "duplicate-class-use".into(),
            ..class_use.clone()
        };
        assert!(super::parasolid_topology_attribute_field_name(
            &reference,
            "double-use",
            &[class_use, duplicate_class],
            &[definition],
            &[field_use],
            &[],
        )
        .is_none());
    }

    #[test]
    fn topology_structured_attribute_values_preserve_serialized_lanes() {
        use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
        use cadmpeg_ir::ids::FaceId;
        use cadmpeg_ir::AnnotationBuilder;

        use crate::native::parasolid::{
            ParasolidAttributeFieldValueKind as Kind, ParasolidEntity51StructuredUse,
            ParasolidEntity57AxisRecord, ParasolidEntity58TagRecord,
            ParasolidEntity62UnicodeRecord, ParasolidEntityVectorRecord,
            ParasolidTopologyAttributeListReference, ParasolidVectorValueKind,
        };

        let mut ir = cadmpeg_ir::examples::unit_cube();
        ir.model.faces[0].id = FaceId("nx:s3:face#60".into());
        let reference = ParasolidTopologyAttributeListReference {
            id: "topology-reference".into(),
            stream_ordinal: 3,
            topology_type: 14,
            topology_xmt: 60,
            attribute_list_xmt: 50,
            attribute_list_record: Some("entity".into()),
            inflated_offset: 300,
        };
        let vectors = [
            (ParasolidVectorValueKind::Points, "point", [1.0, 2.0, 3.0]),
            (ParasolidVectorValueKind::Vectors, "vector", [4.0, 5.0, 6.0]),
            (
                ParasolidVectorValueKind::Directions,
                "direction",
                [7.0, 8.0, 9.0],
            ),
        ]
        .map(|(kind, id, value)| ParasolidEntityVectorRecord {
            id: id.into(),
            stream_ordinal: 3,
            kind,
            xmt: 70,
            values: vec![value],
            byte_len: 36,
            inflated_offset: 400,
        });
        let axis = ParasolidEntity57AxisRecord {
            id: "axis".into(),
            stream_ordinal: 3,
            xmt: 73,
            values: vec![[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
            byte_len: 60,
            inflated_offset: 430,
        };
        let tag = ParasolidEntity58TagRecord {
            id: "tag".into(),
            stream_ordinal: 3,
            xmt: 74,
            values: vec![u32::MAX],
            byte_len: 16,
            inflated_offset: 440,
        };
        let unicode = ParasolidEntity62UnicodeRecord {
            id: "unicode".into(),
            stream_ordinal: 3,
            xmt: 75,
            code_units: vec![0x03bc],
            value: "μ".into(),
            byte_len: 14,
            inflated_offset: 450,
        };
        let uses = [
            (Kind::Points, "point"),
            (Kind::Vectors, "vector"),
            (Kind::Directions, "direction"),
            (Kind::Axes, "axis"),
            (Kind::Tags, "tag"),
            (Kind::Unicode, "unicode"),
        ]
        .into_iter()
        .enumerate()
        .map(|(ordinal, (kind, record))| ParasolidEntity51StructuredUse {
            id: format!("use-{ordinal}"),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            reference_ordinal: u32::try_from(ordinal).expect("test ordinal fits u32") + 5,
            referenced_xmt: u32::try_from(ordinal).expect("test ordinal fits u32") + 70,
            kind,
            value_record: record.into(),
            inflated_offset: 200,
        })
        .collect::<Vec<_>>();
        let mut annotations = AnnotationBuilder::new();
        super::attach_parasolid_topology_structured_attributes(
            &mut ir,
            &super::ParasolidStructuredAttributeSources {
                topology_references: &[reference],
                class_uses: &[],
                definitions: &[],
                field_uses: &[],
                field_names: &[],
                structured_uses: &uses,
                vectors: &vectors,
                axes: &[axis],
                tags: &[tag],
                unicode: &[unicode],
            },
            &mut annotations,
        );

        let attributes = ir
            .model
            .attributes
            .iter()
            .filter(|attribute| attribute.id.0.contains("topology-structured-attribute"))
            .collect::<Vec<_>>();
        assert_eq!(attributes.len(), 6);
        assert!(attributes.iter().all(|attribute| {
            attribute.target == AttributeTarget::Face(FaceId("nx:s3:face#60".into()))
        }));
        let values = attributes
            .iter()
            .map(|attribute| (attribute.name.as_str(), attribute.values.as_slice()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            values["parasolid_type_85_point_reference_5"],
            [AttributeValue::Vector(vec![1.0, 2.0, 3.0])]
        );
        assert_eq!(
            values["parasolid_type_87_axis_reference_8"],
            [AttributeValue::Vector(vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0])]
        );
        assert_eq!(
            values["parasolid_type_88_tag_reference_9"],
            [AttributeValue::Integer(i64::from(u32::MAX))]
        );
        assert_eq!(
            values["parasolid_type_98_unicode_reference_10"],
            [AttributeValue::String("μ".into())]
        );
    }
}
