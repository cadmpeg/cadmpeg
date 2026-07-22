// SPDX-License-Identifier: Apache-2.0
//! IR-writing attachment of the native object model.
//!
//! This module is the sole IR-mutation surface inside `native/`: it walks the
//! extracted [`NativeModel`], emits source annotations in the legacy note order,
//! serializes each record family into an `nx` namespace arena, and attaches the
//! semantic islands (tessellations, source attributes, feature operations). The
//! IR-free domain modules, `model.rs`, and `catalogue.rs` never write IR.

use std::collections::BTreeMap;

use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    ConfigurationBodies, ConfigurationId, DesignConfiguration, Feature, FeatureId,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{AttributeId, BodyId, UnknownId};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

#[allow(clippy::wildcard_imports)]
use crate::decode::*;

use super::catalogue::{CATALOGUE, NOTE_GROUP_A_END, NOTE_GROUP_B_END};

pub(crate) fn attach(
    ir: &mut CadIr,
    model: &crate::native::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
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
    let annotation_stream = annotations.stream("nx:container");
    for (tessellation, source_offset) in display_jt_tessellations {
        annotations
            .note(&tessellation.id, annotation_stream, source_offset)
            .tag("DISPLAY_JT_TESSELLATION");
        annotations.exactness(&tessellation.id, Exactness::Derived);
        ir.model.tessellations.push(tessellation);
    }
    for row in &CATALOGUE[..NOTE_GROUP_A_END] {
        if let Some(note) = row.note {
            note(model, row, annotations);
        }
    }
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
        &model.parasolid.parasolid_topology_attribute_list_references,
        &model.parasolid.parasolid_topology_attribute_class_uses,
        &model.parasolid.parasolid_attribute_definitions,
        &model.parasolid.parasolid_entity_51_string_uses,
        &model.parasolid.parasolid_entity_54_string_records,
        annotations,
    );
    attach_parasolid_topology_numeric_attributes(
        ir,
        &ParasolidNumericAttributeSources {
            topology_references: &model.parasolid.parasolid_topology_attribute_list_references,
            class_uses: &model.parasolid.parasolid_topology_attribute_class_uses,
            definitions: &model.parasolid.parasolid_attribute_definitions,
            numeric_uses: &model.parasolid.parasolid_entity_51_numeric_uses,
            integers: &model.parasolid.parasolid_entity_52_integer_records,
            doubles: &model.parasolid.parasolid_entity_53_double_records,
        },
        annotations,
    );
    for row in &CATALOGUE[NOTE_GROUP_A_END..NOTE_GROUP_B_END] {
        if let Some(note) = row.note {
            note(model, row, annotations);
        }
    }
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
            annotations.derived(&id.0, "active");
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
                bodies,
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
    ir.model
        .features
        .sort_by(|first, second| first.id.cmp(&second.id));
    let namespace = ir.native.namespace_mut("nx");
    namespace.version = namespace.version.max(155);
    for row in CATALOGUE {
        (row.emit)(model, row, namespace)?;
    }
    Ok(())
}

fn attach_feature_operations(
    ir: &mut CadIr,
    features: &crate::native::FeatureRecords,
    expressions: &[crate::native::Expression],
    body_bindings: &[crate::native::SegmentBodyBinding],
    annotations: &mut AnnotationBuilder,
) {
    let labels = features.feature_operation_labels.as_slice();
    let booleans = features.feature_boolean_operations.as_slice();
    let body_references = features.feature_body_references.as_slice();
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
    let pattern_references = features.feature_pattern_references.as_slice();
    let pattern_construction_payloads = features.feature_pattern_construction_payloads.as_slice();
    let pattern_construction_strings = features.feature_pattern_construction_strings.as_slice();
    let pattern_construction_fixed_lanes =
        features.feature_pattern_construction_fixed_lanes.as_slice();
    let pattern_transform_lanes = features.feature_pattern_transform_lanes.as_slice();
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
    let sketch_fixed_points = features.feature_sketch_fixed_points.as_slice();
    let block_constructions = features.feature_block_constructions.as_slice();
    let block_construction_payloads = features.feature_block_construction_payloads.as_slice();
    let block_dimensions = features.feature_block_dimensions.as_slice();
    let block_payload_points = features.feature_block_payload_points.as_slice();
    let block_payload_point_groups = features.feature_block_payload_point_groups.as_slice();
    let extrude_32_constructions = features.feature_extrude_32_constructions.as_slice();
    let extrude_payload_headers = features.feature_extrude_payload_headers.as_slice();
    let extrude_payload_footers = features.feature_extrude_payload_footers.as_slice();
    let extrude_payload_32_branches = features.feature_extrude_payload_32_branches.as_slice();
    let operation_body_scalar_triples = features.feature_operation_body_scalar_triples.as_slice();
    let operation_body_members = features.feature_operation_body_members.as_slice();
    let operation_body_11_continuations =
        features.feature_operation_body_11_continuations.as_slice();
    let operation_body_reference_lanes = features.feature_operation_body_reference_lanes.as_slice();
    let parameter_bindings = features.feature_parameter_bindings.as_slice();
    let parameter_uses = features.feature_parameter_uses.as_slice();
    let operation_records = features.feature_operation_records.as_slice();
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
    let stream = annotations.stream("nx:container");
    let base_ordinal = ir.model.features.len() as u64;
    let booleans = booleans
        .iter()
        .map(|operation| (operation.operation_label.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let body_references = body_references
        .iter()
        .map(|reference| {
            (
                reference.operation_label.as_str(),
                reference.body_object_index,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut body_reference_occurrences_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBodyReferenceOccurrence>>::new();
    for reference in body_reference_occurrences {
        body_reference_occurrences_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut last_body_writer = BTreeMap::<u32, FeatureId>::new();
    let body_alias_roots = crate::native::body_alias_roots(body_bindings).unwrap_or_default();
    let canonical_body =
        |identity: u32| body_alias_roots.get(&identity).copied().unwrap_or(identity);
    let mut input_blocks_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureInputBlock>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeatureDatumCsysBlockUse>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeatureDatumPlaneBlockUse>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeatureDatumPlaneCsysIdentityUse>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchReference>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchNamedPointBlockUse>>::new();
    for block_use in sketch_named_point_block_uses {
        sketch_named_point_uses_by_operation
            .entry(block_use.operation_label.as_str())
            .or_default()
            .push(block_use);
    }
    let mut sketch_preceding_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPrecedingNamedPointUse>>::new();
    for point_use in sketch_preceding_named_point_uses {
        sketch_preceding_named_point_uses_by_operation
            .entry(point_use.operation_label.as_str())
            .or_default()
            .push(point_use);
    }
    let mut sketch_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPointUse>>::new();
    for point_use in sketch_point_uses {
        sketch_point_uses_by_operation
            .entry(point_use.operation_label.as_str())
            .or_default()
            .push(point_use);
    }
    let mut sketch_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPointGroup>>::new();
    for group in sketch_point_groups {
        sketch_point_groups_by_operation
            .entry(group.operation_label.as_str())
            .or_default()
            .push(group);
    }
    let mut extrude_profile_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureExtrudeProfileReference>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands {
        operation_body_operands_by_operation
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
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPayloadCoordinatePair>>::new();
    for pair in sketch_coordinate_pairs {
        sketch_coordinate_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_fixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchPayloadFixedPair>>::new();
    for pair in sketch_fixed_pairs {
        sketch_fixed_pairs_by_operation
            .entry(pair.operation_label.as_str())
            .or_default()
            .push(pair);
    }
    let mut sketch_fixed_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureSketchFixedPoint>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeatureBlockPayloadPoint>>::new();
    for point in block_payload_points {
        block_payload_points_by_operation
            .entry(point.operation_label.as_str())
            .or_default()
            .push(point);
    }
    let mut block_payload_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureBlockPayloadPointGroup>>::new();
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
    let extrude_payload_footers_by_operation = extrude_payload_footers
        .iter()
        .map(|footer| (footer.operation_label.as_str(), footer))
        .collect::<BTreeMap<_, _>>();
    let extrude_payload_32_branches_by_operation =
        records_by_operation(extrude_payload_32_branches, |branch| {
            &branch.operation_label
        });
    let mut operation_body_scalar_triples_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyScalarTriple>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyMember>>::new();
    for member in operation_body_members {
        operation_body_members_by_operation
            .entry(member.operation_label.as_str())
            .or_default()
            .push(member);
    }
    let mut operation_body_11_continuations_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBody11Continuation>>::new();
    for continuation in operation_body_11_continuations {
        operation_body_11_continuations_by_operation
            .entry(continuation.operation_label.as_str())
            .or_default()
            .push(continuation);
    }
    let mut operation_body_reference_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureOperationBodyReferenceLane>>::new();
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
    let hole_outputs = simple_hole_templates
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
    let simple_hole_diameters = simple_hole_diameters(
        ir,
        simple_hole_templates,
        simple_hole_construction_groups,
        &hole_outputs,
    );
    let simple_hole_directions =
        hole_directions_for_operations(ir, &simple_hole_operations, &hole_outputs);
    let simple_hole_positions =
        hole_positions_for_operations(ir, &simple_hole_operations, &hole_outputs);
    let hole_package_operations = labels
        .iter()
        .filter(|label| label.value == "HOLE PACKAGE")
        .map(|label| label.id.clone())
        .collect::<Vec<_>>();
    let hole_package_outputs = hole_package_operations
        .iter()
        .filter_map(|operation| {
            let object_index = body_references.get(operation.as_str())?;
            Some((
                operation.clone(),
                feature_body_outputs(*object_index, &bodies_by_object_index),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let hole_package_diameters =
        hole_diameters_for_operations(ir, &hole_package_operations, &hole_package_outputs);
    let hole_package_directions =
        hole_directions_for_operations(ir, &hole_package_operations, &hole_package_outputs);
    let hole_package_positions =
        hole_positions_for_operations(ir, &hole_package_operations, &hole_package_outputs);
    let simple_hole_chamfers = simple_hole_chamfers(ir, simple_hole_templates, &hole_outputs);
    let mut parameter_bindings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureParameterBinding>>::new();
    for binding in parameter_bindings {
        parameter_bindings_by_operation
            .entry(binding.operation_label.as_str())
            .or_default()
            .push(binding);
    }
    let mut parameter_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::FeatureParameterUse>>::new();
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
        BTreeMap::<&str, Vec<&crate::native::FeaturePayloadString>>::new();
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
        if let Some(body) = body_references.get(label.id.as_str()) {
            if let Some(writer) = last_body_writer.get(&canonical_body(*body)) {
                dependencies.push(writer.clone());
            }
        }
        if let Some(operation) = booleans.get(label.id.as_str()) {
            for body in &operation.tool_object_indices {
                if let Some(writer) = last_body_writer.get(&canonical_body(*body)) {
                    if !dependencies.contains(writer) {
                        dependencies.push(writer.clone());
                    }
                }
            }
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            if let Some(writer) =
                last_body_writer.get(&canonical_body(operand.operand_object_index))
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
                crate::native::FeatureSketchDatumCsysBlockRelation::Shared { data_block } => {
                    source_properties.insert(
                        "sketch_point_dependency_shared_block".to_string(),
                        data_block.clone(),
                    );
                }
                crate::native::FeatureSketchDatumCsysBlockRelation::Consecutive {
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
        }
        let deletes_body = label.value == "DELETE";
        let outputs = if deletes_body {
            Vec::new()
        } else {
            body_references
                .get(label.id.as_str())
                .map_or_else(Vec::new, |body| {
                    feature_body_outputs(*body, &bodies_by_object_index)
                })
        };
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
        if let Some(footer) = extrude_payload_footers_by_operation.get(label.id.as_str()) {
            source_properties.insert("extrude_payload_footer".to_string(), footer.id.clone());
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
                operand.operand_object_index.to_string(),
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
        let block_placement = (label.value == "BLOCK")
            .then(|| block_placement(ir, block_dimension_values?, &outputs))
            .flatten();
        let sew_projection = (label.value == "SEW")
            .then(|| {
                sew_body_feature_definition(
                    *body_references.get(label.id.as_str())?,
                    operation_body_operands_by_operation
                        .get(label.id.as_str())?
                        .as_slice(),
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let trim_body_projection = (label.value == "TRIM BODY")
            .then(|| {
                trim_body_feature_definition(
                    *body_references.get(label.id.as_str())?,
                    operation_body_operands_by_operation
                        .get(label.id.as_str())?
                        .as_slice(),
                    &bodies_by_object_index,
                )
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
        let extrude_projection = (label.value == "EXTRUDE")
            .then(|| {
                let body = body_references.get(label.id.as_str())?;
                let output_kinds = outputs
                    .iter()
                    .filter_map(|output| {
                        ir.model
                            .bodies
                            .iter()
                            .find(|body| body.id == *output)
                            .map(|body| body.kind)
                    })
                    .collect::<Vec<_>>();
                let op = extrude_boolean_op(
                    last_body_writer.contains_key(&canonical_body(*body)),
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
                )
            })
            .flatten();
        let delete_projection = deletes_body
            .then(|| {
                delete_body_feature_definition(
                    body_references.get(label.id.as_str()).copied(),
                    &bodies_by_object_index,
                )
            })
            .flatten();
        let operation_parameter_uses = parameter_uses_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let native_parameters = native_feature_parameters(operation_parameter_uses, expressions);
        let definition = booleans.get(label.id.as_str()).map_or_else(
            || {
                trim_body_projection
                    .or(delete_projection)
                    .or(sew_projection)
                    .or(extrude_projection)
                    .or_else(|| blend_projection.map(|(definition, _)| definition))
                    .or_else(|| thicken_projection.map(|(definition, _)| definition))
                    .or_else(|| offset_projection.map(|(definition, _)| definition))
                    .unwrap_or_else(|| {
                        non_boolean_feature_definition_with_parameters(
                            &label.value,
                            &operation_payload_strings,
                            block_dimension_values,
                            block_placement,
                            HoleProjection {
                                position: simple_hole_positions
                                    .get(label.id.as_str())
                                    .or_else(|| hole_package_positions.get(label.id.as_str()))
                                    .copied(),
                                diameter: simple_hole_diameters
                                    .get(label.id.as_str())
                                    .or_else(|| hole_package_diameters.get(label.id.as_str()))
                                    .copied(),
                                direction: simple_hole_directions
                                    .get(label.id.as_str())
                                    .or_else(|| hole_package_directions.get(label.id.as_str()))
                                    .copied(),
                                chamfer: simple_hole_chamfers.get(label.id.as_str()).copied(),
                            },
                            native_parameters,
                        )
                    })
            },
            |operation| boolean_feature_definition(operation, &bodies_by_object_index),
        );
        annotations
            .note(&id, stream, label.source_offset)
            .tag("FEATURE_OPERATION");
        annotations.exactness(&id, Exactness::Derived);
        let mut source_content =
            feature_source_content(operation_payload_string_records, operation_parameter_uses);
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            append_feature_expression_content(&mut source_content, &dimensions.expressions);
        }
        for owner in parameter_owner_dependencies(&parameter_owners, &source_content) {
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
            if let Some(body) = body_references.get(label.id.as_str()) {
                last_body_writer.insert(canonical_body(*body), id);
            }
        }
    }
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
