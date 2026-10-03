// SPDX-License-Identifier: Apache-2.0
//! IR-writing attachment of the native object model.

use crate::loss::NxLossCode;
use cadmpeg_core::decode::id_from_index;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    AppearanceBindingId, AppearanceId, AttributeId, BodyId, FeatureResultTopologyId, UnknownId,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::semantic_annotations::{SemanticAnnotation, SemanticAnnotationKind};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement,
};
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{
    features::{
        BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, ConfigurationFeatureState,
        ConfigurationId, DesignConfiguration, DesignParameter, DistinctMembers, ExtrudeExtent,
        ExtrudeSide, Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation,
        FeatureResultTopology, FeatureSourceContent, FeatureTreeNodeRole, LinearTermination,
        ParameterId, ParameterValue, PlanarProfileRef, ProfileRef, TreeChildren,
    },
    scalar::{Angle, FiniteReal, Length},
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

pub(super) mod body_selection;
pub(super) mod expressions;
pub(super) mod feature_projection;

use body_selection::{
    atomic_disjoint_body_selections, boolean_participant_writer, boolean_target_output,
    boolean_target_writer, feature_body_selection, feature_body_selection_with_offset_blocks,
    feature_body_set_selection, local_body_selection, FeatureBodySelection,
};

use feature_projection::{
    blend_feature_definition, blind_hole_axis_placements_for_operations,
    blind_hole_body_projection, blind_hole_operations, block_placement,
    body_writing_unresolved_feature_definition, brep_feature_definition,
    counterbore_axis_placements_for_operations, counterbore_body_projection,
    counterbore_operations, extend_hole_projection_map, feature_source_content,
    hole_axis_placements_for_operations, hole_body_projection, hole_package_projection,
    native_feature_parameters, new_body_boolean_op, non_boolean_feature_definition_with_parameters,
    non_modeling_history_definition, offset_surface_feature_definition, primary_hole_outputs,
    selection_indices_native, simple_hole_chamfers, simple_hole_native_properties,
    simple_hole_operations, sphere_body_projection, thicken_feature_definition, HolePackageSources,
    HoleProjection, NewBodyEvidence, NxBlendFamily,
};

const MIN_LINEAR_TOLERANCE: f64 = 1.0e-9;
const MIN_ANGULAR_TOLERANCE: f64 = 1.0e-12;

fn push_native_unknown(
    ctx: &DecodeContext<'_>,
    unknowns: &mut Vec<UnknownRecord>,
    record: UnknownRecord,
) -> Result<(), CodecError> {
    ctx.charge_entities(1, "NX native unknown record")?;
    ctx.reserve_vec(unknowns, 1, "NX native unknown records")?;
    unknowns.push(record);
    Ok(())
}

/// Refuses a document tolerance finer than the hole recognisers resolve.
///
/// The recognisers below compare evaluated B-rep geometry against the document
/// linear and angular tolerances and then state the hole they find as a native
/// NX feature. Evaluated geometry carries no meaning below these bounds, so a
/// document that states a finer tolerance names a precision the attach route
/// does not have. The route refuses such a document by name here, once, and
/// every recogniser below reads the stated tolerance as stated.
fn admit_hole_recognition_tolerances(ir: &CadIr) -> Result<(), CodecError> {
    let linear = ir.tolerances.linear.get();
    if linear < MIN_LINEAR_TOLERANCE {
        return Err(CodecError::InvalidInput(format!(
            "document linear tolerance {linear} is finer than the NX hole recognition bound {MIN_LINEAR_TOLERANCE}"
        )));
    }
    let angular = ir.tolerances.angular.get();
    if angular < MIN_ANGULAR_TOLERANCE {
        return Err(CodecError::InvalidInput(format!(
            "document angular tolerance {angular} is finer than the NX hole recognition bound {MIN_ANGULAR_TOLERANCE}"
        )));
    }
    Ok(())
}

use crate::container::EntryContent;
use crate::decode::ids::{extended_id, native_entity_key, IdScope};
use crate::decode::Scan;
use crate::native::history::{
    active_feature_closure_for_decode, BodyWriterHistory, NATIVE_PRIMARY_BODY_CLOSURE_WITNESS,
    NATIVE_PRIMARY_BODY_OBJECT_INDEX,
};
use crate::native::om::display_color::{
    RmDisplayColorAssignment, RmDisplayColorAssignmentEncoding,
};
use crate::native::segments::BooleanOffsetStoreResolution;

use super::catalogue::NATIVE_CATALOGUE;
use super::display_jt::{display_jt_tessellations, DisplayJtTessellationInputs};
use super::toggle::has_complete_saved_toggle_stream;
use super::TypedNative;
use cadmpeg_ir::native::catalogue::NotePhase;

pub(super) fn attach_container_layer(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    typed_native: TypedNative,
) -> Result<(), CodecError> {
    attach_container_payloads(ctx, ir, scan, annotations, unknowns, typed_native)?;
    attach_indexed_om_unknowns(ctx, scan, annotations, unknowns)?;
    Ok(())
}

fn attach_container_payloads(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    typed_native: TypedNative,
) -> Result<(), CodecError> {
    let annotation_stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    for (ordinal, entry) in scan.container.entries.iter().enumerate() {
        let content = entry.content();
        if !content.retains_opaque_payload()
            || (typed_native == TypedNative::Available
                && content == EntryContent::SaveToggleInfo
                && has_complete_saved_toggle_stream(&scan.container))
        {
            continue;
        }
        let Some((offset, byte_len)) = entry.file_span() else {
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
        let id: UnknownId = IdScope::native(cadmpeg_ir::identity_component!("container-entry"))
            .id(&cadmpeg_ir::identity_component!("opaque"), ordinal);
        annotations.note(ctx, &id, &annotation_stream, offset, Some(content.label()))?;
        annotations.exactness(ctx, &id, Exactness::ByteExact)?;
        push_native_unknown(
            ctx,
            unknowns,
            UnknownRecord::retained(
                id,
                offset,
                ctx.copy_retained(bytes, "retain NX opaque container payload")?,
                Vec::new(),
            ),
        )?;
    }
    attach_jpeg_preview_assets(ctx, ir, scan, annotations, unknowns)?;
    Ok(())
}

fn attach_indexed_om_unknowns(
    ctx: &DecodeContext<'_>,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), CodecError> {
    let annotation_stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    let object_sections = scan.container.indexed_om_sections(ctx)?;
    for (section_index, (entry, section)) in object_sections.iter().enumerate() {
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        match &section.store {
            crate::om::IndexedStore::Fixed { records } => {
                for (record_index, record) in records.iter().enumerate() {
                    let id: UnknownId = IdScope::native(
                        cadmpeg_ir::identity_component!("om-section-").then(section_index),
                    )
                    .id(&cadmpeg_ir::identity_component!("record"), record_index);
                    let offset = entry_offset + cadmpeg_core::decode::u64_from_index(record.offset);
                    annotations.note(
                        ctx,
                        &id,
                        &annotation_stream,
                        offset,
                        Some("OM_ENTITY_RECORD"),
                    )?;
                    annotations.exactness(ctx, &id, Exactness::ByteExact)?;
                    push_native_unknown(
                        ctx,
                        unknowns,
                        UnknownRecord::retained(
                            id,
                            offset,
                            ctx.copy_retained(
                                record.bytes,
                                "retain NX indexed object-model record",
                            )?,
                            Vec::new(),
                        ),
                    )?;
                }
            }
            crate::om::IndexedStore::OffsetOnly {
                control, records, ..
            } => {
                for (record_index, record) in
                    std::iter::once(control).chain(records.iter()).enumerate()
                {
                    let id: UnknownId = IdScope::native(
                        cadmpeg_ir::identity_component!("om-section-").then(section_index),
                    )
                    .id(&cadmpeg_ir::identity_component!("block"), record_index);
                    let offset = entry_offset + cadmpeg_core::decode::u64_from_index(record.offset);
                    annotations.note(
                        ctx,
                        &id,
                        &annotation_stream,
                        offset,
                        Some("OM_DATA_BLOCK"),
                    )?;
                    annotations.exactness(ctx, &id, Exactness::ByteExact)?;
                    push_native_unknown(
                        ctx,
                        unknowns,
                        UnknownRecord::retained(
                            id,
                            offset,
                            ctx.copy_retained(
                                record.bytes,
                                "retain NX indexed object-model record",
                            )?,
                            Vec::new(),
                        ),
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn attach(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    attach_container_payloads(ctx, ir, scan, annotations, unknowns, TypedNative::Available)?;
    let has_object_sections = !scan.container.indexed_om_sections(ctx)?.is_empty();
    let annotation_stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    if model.is_empty() && !has_object_sections {
        return Ok(());
    }
    attach_rm_face_colors(ctx, ir, model, scan, annotations)?;
    attach_rm_appearances(ctx, ir, model, scan, annotations)?;
    let display_jt_tessellations = display_jt_tessellations(
        ctx,
        &DisplayJtTessellationInputs {
            meshes: &model.display_jt.polygon_meshes,
            coordinates: &model.display_jt.vertex_coordinates,
            normals: &model.display_jt.vertex_normals,
            colors: &model.display_jt.vertex_colors,
            texture_coordinates: &model.display_jt.vertex_texture_coordinates,
            vertex_flags: &model.display_jt.vertex_flags,
            vertex_headers: &model.display_jt.vertex_records_headers,
            coordinate_headers: &model.display_jt.coordinate_array_headers,
            shape_elements: model.display_jt.graph.shape_lod_elements(),
            bindings: &model.display_jt.shape_lod_bindings,
            shape_nodes: &model.display_jt.tri_strip_shape_nodes,
            base_nodes: &model.display_jt.base_node_data,
            group_nodes: &model.display_jt.group_node_data,
            instance_nodes: &model.display_jt.instance_nodes,
            transforms: &model.display_jt.geometric_transform_attributes,
            materials: &model.display_jt.material_attributes,
            compressed_elements: model.display_jt.graph.compressed_elements(),
        },
    )?;
    for (tessellation, source_offset) in display_jt_tessellations {
        ctx.reserve_vec(
            &mut ir.model.tessellations,
            1,
            "NX attached display tessellations",
        )?;
        annotations.note(
            ctx,
            tessellation.id.as_str(),
            &annotation_stream,
            source_offset,
            Some("DISPLAY_JT_TESSELLATION"),
        )?;
        annotations.exactness(ctx, tessellation.id.as_str(), Exactness::Derived)?;
        ir.model.tessellations.push(tessellation);
    }
    NATIVE_CATALOGUE.note_phase(ctx, NotePhase::GroupA, model, annotations)?;
    attach_material_texture_assets(
        ctx,
        ir,
        &model.om.material_texture_assets,
        scan,
        annotations,
    )?;
    attach_part_attributes(
        ctx,
        ir,
        model.om.part_attributes.iter().map(|attribute| {
            (
                attribute.id.as_str(),
                attribute.title.as_str(),
                attribute.value.as_str(),
                attribute.source_offset,
            )
        }),
        annotations,
        &annotation_stream,
    )?;
    let topology_attribute_index = ParasolidTopologyAttributeIndex::new(
        ctx,
        ir,
        &model.parasolid.topology_attribute_list_references,
        &model.parasolid.topology_attribute_class_uses,
        &model.parasolid.attribute_definitions,
        &model.parasolid.attribute_field_uses,
        &model.parasolid.attribute_field_names,
    )?;
    attach_parasolid_topology_string_attributes(
        ctx,
        ir,
        &ParasolidStringAttributeSources {
            string_uses: &model.parasolid.entity_51_string_uses,
            strings: &model.parasolid.entity_54_string_records,
        },
        &topology_attribute_index,
        annotations,
    )?;
    attach_parasolid_topology_numeric_attributes(
        ctx,
        ir,
        &ParasolidNumericAttributeSources {
            numeric_uses: &model.parasolid.entity_51_numeric_uses,
            integers: &model.parasolid.entity_52_integer_records,
            doubles: &model.parasolid.entity_53_double_records,
        },
        &topology_attribute_index,
        annotations,
    )?;
    attach_parasolid_topology_structured_attributes(
        ctx,
        ir,
        &ParasolidStructuredAttributeSources {
            structured_uses: &model.parasolid.entity_51_structured_uses,
            vectors: &model.parasolid.entity_vector_records,
            axes: &model.parasolid.entity_57_axis_records,
            tags: &model.parasolid.entity_58_tag_records,
            unicode: &model.parasolid.entity_62_unicode_records,
        },
        &topology_attribute_index,
        annotations,
    )?;
    NATIVE_CATALOGUE.note_phase(ctx, NotePhase::GroupB, model, annotations)?;
    attach_indexed_om_unknowns(ctx, scan, annotations, unknowns)?;
    let configuration_lookup_work = model
        .om
        .configurations
        .len()
        .checked_mul(model.om.configuration_attribute_uses.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX configuration relation lookup",
                0,
                cadmpeg_core::decode::u64_from_index(model.om.configurations.len()),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(configuration_lookup_work),
        "NX configuration relation lookup",
    )?;
    attach_configurations(
        ctx,
        ir,
        model.om.configurations.iter().map(|configuration| {
            (
                configuration.id.as_str(),
                configuration.name.as_str(),
                configuration.source_offset,
                model
                    .om
                    .configuration_attribute_uses
                    .iter()
                    .find(|relation| relation.configuration == configuration.id)
                    .map(|relation| relation.id.as_str()),
            )
        }),
        annotations,
        &annotation_stream,
    )?;
    expressions::attach_expression_parameters(
        ctx,
        ir,
        &model.om.expressions,
        &model.om.expression_declarations,
        &model.features.feature_parameter_uses,
        annotations,
    )?;
    attach_active_configuration_parameter_values(ctx, ir, annotations)?;
    attach_feature_operations(ctx, ir, model, annotations, losses)?;
    expressions::attach_block_dimension_parameter_consumers(
        ctx,
        ir,
        &model.features.feature_block_dimensions,
        annotations,
    )?;
    attach_current_feature_states(ctx, ir, annotations)?;
    attach_active_configuration_feature_states(ctx, ir, annotations)?;
    ctx.stable_sort_by(
        &mut ir.model.features,
        |first, second| first.id.cmp(&second.id),
        |feature| feature.id.as_str().len(),
        "sort NX features",
    )?;
    let namespace = ir.native.namespace_mut("nx");
    NATIVE_CATALOGUE
        .emit_all(ctx, model, namespace)
        .map_err(CodecError::from)?;
    Ok(())
}

fn attach_part_attributes<'a>(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    attributes: impl IntoIterator<Item = (&'a str, &'a str, &'a str, u64)>,
    annotations: &mut AnnotationBuilder,
    annotation_stream: &StreamHandle,
) -> Result<(), CodecError> {
    for (attribute_id, attribute_title, attribute_value, source_offset) in attributes {
        let id_bytes = attribute_id
            .len()
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(":neutral".len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX part attribute identity",
                    0,
                    cadmpeg_core::decode::u64_from_index(attribute_id.len()),
                )
            })?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(id_bytes),
            "NX part attribute identity",
        )?;
        annotations.note(
            ctx,
            attribute_id,
            annotation_stream,
            source_offset,
            Some("Attribute"),
        )?;
        annotations.exactness(ctx, attribute_id, Exactness::ByteExact)?;
        let id: AttributeId = extended_id(attribute_id, &cadmpeg_ir::identity_key!("neutral"))
            .ok_or_else(|| {
                CodecError::malformed(format_args!("NX part attribute id is not an identity"))
            })?;
        annotations.note(
            ctx,
            id.as_str(),
            annotation_stream,
            source_offset,
            Some("Attribute"),
        )?;
        annotations
            .derived(ctx, id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, id.as_str(), "name")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, id.as_str(), "values")
            .map_err(cadmpeg_core::CodecError::from)?;
        ctx.charge_collection_items(1, "NX attached part attributes")?;
        ctx.charge_collection_items(1, "NX part attribute values")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                attribute_title
                    .len()
                    .checked_add(attribute_value.len())
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "NX part attribute value",
                            0,
                            cadmpeg_core::decode::u64_from_index(attribute_value.len()),
                        )
                    })?,
            ),
            "NX part attribute values",
        )?;
        let mut values = Vec::new();
        ctx.reserve_capacity(&mut values, 1, "NX part attribute values")?;
        values.push(AttributeValue::String(attribute_value.to_string()));
        ctx.reserve_capacity(&mut ir.model.attributes, 1, "NX attached part attributes")?;
        ir.model.attributes.push(SourceAttribute {
            id,
            target: AttributeTarget::Document,
            name: attribute_title.to_string(),
            values,
        });
    }
    Ok(())
}

fn attach_configurations<'a>(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    configurations: impl IntoIterator<Item = (&'a str, &'a str, u64, Option<&'a str>)>,
    annotations: &mut AnnotationBuilder,
    annotation_stream: &StreamHandle,
) -> Result<(), CodecError> {
    for (ordinal, (configuration_id, configuration_name, source_offset, active_attribute_use)) in
        configurations.into_iter().enumerate()
    {
        let id: ConfigurationId = IdScope::native(cadmpeg_ir::identity_component!("arrangements"))
            .id(&cadmpeg_ir::identity_component!("configuration"), ordinal);
        let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
            ctx.refuse_codec_limit(
                "NX configuration ordinal",
                0,
                cadmpeg_core::decode::u64_from_index(ordinal),
            )
        })?;
        let bodies = if active_attribute_use.is_some() {
            let mut selected = Vec::new();
            ctx.reserve_vec(
                &mut selected,
                ir.model.bodies.len(),
                "NX active configuration bodies",
            )?;
            for body in &ir.model.bodies {
                selected.push(
                    body.id
                        .try_clone_for_decode(ctx, "NX decoded IR value copy")?,
                );
            }
            Some(
                cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
            )
        } else {
            None
        };
        annotations.note(
            ctx,
            id.as_str(),
            annotation_stream,
            source_offset,
            Some("Arrangement"),
        )?;
        annotations
            .derived(ctx, id.as_str(), "ordinal")
            .map_err(cadmpeg_core::CodecError::from)?;
        if active_attribute_use.is_some() {
            annotations
                .derived(ctx, id.as_str(), "active")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        annotations
            .derived(ctx, id.as_str(), "source_index")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, id.as_str(), "name")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, id.as_str(), "native_ref")
            .map_err(cadmpeg_core::CodecError::from)?;
        if bodies
            .as_deref()
            .is_some_and(|bodies: &[_]| !bodies.is_empty())
        {
            annotations
                .derived(ctx, id.as_str(), "bodies")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        ctx.charge_collection_items(1, "NX attached configurations")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<DesignConfiguration>()),
            "NX attached configurations",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(configuration_name.len()),
            "NX configuration name",
        )?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(configuration_id.len()),
            "NX configuration native reference",
        )?;
        let mut properties = BTreeMap::new();
        if let Some(relation) = active_attribute_use {
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<(String, String)>()
                        .checked_add(relation.len())
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "NX active configuration property",
                                0,
                                cadmpeg_core::decode::u64_from_index(relation.len()),
                            )
                        })?,
                ),
                "NX active configuration property",
            )?;
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_literal!("active_attribute_use"),
                relation.to_string(),
                "NX active configuration property",
            )?;
        }
        ctx.reserve_capacity(
            &mut ir.model.configurations,
            1,
            "NX attached configurations",
        )?;
        ir.model.configurations.push(DesignConfiguration {
            id,
            ordinal: ordinal_u32,
            active: active_attribute_use.is_some(),
            source_index: Some(ordinal_u32),
            name: configuration_name.to_string().into(),
            material: None,
            properties,
            parameter_overrides: BTreeMap::new(),
            bodies,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(configuration_id.to_string()),
        });
    }
    Ok(())
}

fn attach_rm_face_colors(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let (face_ids, _face_ids_reservation) =
        collect_rm_face_ids(ctx, ir.model.faces.iter().map(|face| face.id.as_str()))?;
    let (bindings, _bindings_storage) =
        ctx.with_scoped_storage("NX RM color projection storage", || {
            resolve_rm_face_colors(
                ctx,
                &face_ids,
                &model.om.rm_display_color_assignments,
                &model.om.part_color_definitions,
                &model.parasolid.deltas_records,
                &super::substrate::paired_delta_streams(ctx, scan)?,
            )
        })?;
    for (face_id, color) in bindings {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.faces.len()),
            "NX RM face color target lookup",
        )?;
        let Some(index) = ir
            .model
            .faces
            .iter()
            .rposition(|face| face.id.as_str() == face_id)
        else {
            continue;
        };
        let face = &mut ir.model.faces[index];
        if face.color.is_none() || face.color == Some(color) {
            face.color = Some(color);
            annotations
                .derived(ctx, &face.id, "color")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    Ok(())
}

fn collect_rm_face_ids<'a, 'b>(
    ctx: &'a DecodeContext<'_>,
    faces: impl IntoIterator<Item = &'b str>,
) -> Result<
    (
        BTreeSet<String>,
        cadmpeg_core::decode::ScopedReservation<'a>,
    ),
    CodecError,
> {
    let mut ids = BTreeSet::new();
    let mut reservation = ctx.reserve_scoped(0, "NX RM face identity lookup")?;
    for id in faces {
        ctx.charge_work(1, "NX RM face identity lookup")?;
        if ids.contains(id) {
            continue;
        }

        let id = reservation
            .with_storage(|| ctx.copy_retained_text(id, "NX RM face identity lookup"))?;
        reservation
            .with_storage(|| ctx.insert_btree_set(&mut ids, id, "NX RM face identity lookup"))?;
    }
    Ok((ids, reservation))
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
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let (source_bindings, _source_bindings_storage) = ctx
        .with_scoped_storage("NX RM color projection storage", || {
            resolve_rm_source_color_bindings(ctx, &model.om.rm_display_color_assignments)
        })?;
    let (face_ids, _face_ids_reservation) =
        collect_rm_face_ids(ctx, ir.model.faces.iter().map(|face| face.id.as_str()))?;
    let (face_bindings, _face_bindings_storage) =
        ctx.with_scoped_storage("NX RM color projection storage", || {
            resolve_rm_face_color_bindings(
                ctx,
                &face_ids,
                &model.om.rm_display_color_assignments,
                &model.om.part_color_definitions,
                &model.parasolid.deltas_records,
                &super::substrate::paired_delta_streams(ctx, scan)?,
            )
        })?;
    if source_bindings.is_empty() && face_bindings.is_empty() {
        return Ok(());
    }
    let annotation_stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    let mut appearances = BTreeMap::<String, AppearanceId>::new();
    let mut appearances_reservation = ctx.reserve_scoped(0, "NX RM appearance identity lookup")?;
    for binding in source_bindings {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(model.om.part_color_definitions.len()),
            "NX RM appearance definition lookup",
        )?;
        let Some(definition) = model
            .om
            .part_color_definitions
            .iter()
            .rev()
            .find(|definition| definition.id == binding.color_definition)
        else {
            continue;
        };
        let appearance_id = ensure_rm_color_appearance(
            ctx,
            ir,
            annotations,
            &mut appearances,
            &mut appearances_reservation,
            definition,
            &annotation_stream,
        )?;
        let binding_id_reservation = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(
                binding.source_id.len().checked_add(128).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX RM source binding identity",
                        0,
                        cadmpeg_core::decode::u64_from_index(binding.source_id.len()),
                    )
                })?,
            ),
            "NX RM source binding identity",
        )?;
        let binding_id: AppearanceBindingId =
            IdScope::native(cadmpeg_ir::identity_component!("appearance-binding")).id(
                &cadmpeg_ir::identity_component!("rmfastload-color"),
                native_entity_key(&binding.source_id).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "NX RMFASTLOAD_COLOR_ASSIGNMENT source id is not identity key text"
                    ))
                })?,
            );
        drop(binding_id_reservation);
        annotations.note(
            ctx,
            binding_id.as_str(),
            &annotation_stream,
            binding.source_offset,
            Some("RMFASTLOAD_COLOR_ASSIGNMENT"),
        )?;
        annotations
            .derived(ctx, binding_id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, binding_id.as_str(), "appearance")
            .map_err(cadmpeg_core::CodecError::from)?;
        let binding_bytes = binding_id
            .as_str()
            .len()
            .checked_add(binding.source_id.len())
            .and_then(|bytes| bytes.checked_add(appearance_id.as_str().len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX RM source appearance binding",
                    0,
                    cadmpeg_core::decode::u64_from_index(binding.source_id.len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX RM source appearance bindings")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(binding_bytes),
            "NX RM source appearance binding",
        )?;
        ctx.reserve_capacity(
            &mut ir.model.appearance_bindings,
            1,
            "NX RM source appearance bindings",
        )?;
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: binding_id,
            target: AppearanceTarget::Source {
                source_id: binding.source_id.clone(),
            },
            appearance: appearance_id,
            source_entity_id: Some(binding.source_id),
            object_type: Some("RMFastLoad object ID".into()),
            visible: None,
            channels: BTreeMap::new(),
        });
    }
    for binding in face_bindings {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(model.om.part_color_definitions.len()),
            "NX RM appearance definition lookup",
        )?;
        let Some(definition) = model
            .om
            .part_color_definitions
            .iter()
            .rev()
            .find(|definition| definition.id == binding.color_definition)
        else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.faces.len()),
            "NX RM appearance face lookup",
        )?;
        let Some(face) = ir
            .model
            .faces
            .iter()
            .find(|face| face.id.as_str() == binding.face_id)
        else {
            continue;
        };
        let existing_color = face.color;
        let color = Color::new(
            definition.components[0].0.value(),
            definition.components[1].0.value(),
            definition.components[2].0.value(),
            1.0,
        )
        .ok_or_else(|| CodecError::Malformed("RM color components must be in [0, 1]".into()))?;
        if existing_color.is_some_and(|existing| existing != color) {
            continue;
        }
        let face_id = face
            .id
            .try_clone_for_decode(ctx, "NX decoded IR value copy")?;
        let appearance_id = ensure_rm_color_appearance(
            ctx,
            ir,
            annotations,
            &mut appearances,
            &mut appearances_reservation,
            definition,
            &annotation_stream,
        )?;
        let binding_id_reservation = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(
                binding.face_id.len().checked_add(128).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX RM face binding identity",
                        0,
                        cadmpeg_core::decode::u64_from_index(binding.face_id.len()),
                    )
                })?,
            ),
            "NX RM face binding identity",
        )?;
        let binding_id: AppearanceBindingId =
            IdScope::native(cadmpeg_ir::identity_component!("appearance-binding")).id(
                &cadmpeg_ir::identity_component!("rmfastload-face-color"),
                native_entity_key(&binding.face_id).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "NX RMFASTLOAD_FACE_COLOR_ASSIGNMENT face id is not identity key text"
                    ))
                })?,
            );
        drop(binding_id_reservation);
        annotations.note(
            ctx,
            binding_id.as_str(),
            &annotation_stream,
            binding.source_offset,
            Some("RMFASTLOAD_FACE_COLOR_ASSIGNMENT"),
        )?;
        annotations
            .derived(ctx, binding_id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, binding_id.as_str(), "appearance")
            .map_err(cadmpeg_core::CodecError::from)?;
        let binding_bytes = binding_id
            .as_str()
            .len()
            .checked_add(face_id.as_str().len())
            .and_then(|bytes| bytes.checked_add(appearance_id.as_str().len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX RM face appearance binding",
                    0,
                    cadmpeg_core::decode::u64_from_index(face_id.as_str().len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX RM face appearance bindings")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(binding_bytes),
            "NX RM face appearance binding",
        )?;
        ctx.reserve_capacity(
            &mut ir.model.appearance_bindings,
            1,
            "NX RM face appearance bindings",
        )?;
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: binding_id,
            target: AppearanceTarget::Face(face_id),
            appearance: appearance_id,
            source_entity_id: Some(binding.face_id),
            object_type: Some("Parasolid FACE".into()),
            visible: None,
            channels: BTreeMap::new(),
        });
    }
    Ok(())
}

fn ensure_rm_color_appearance(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    appearances: &mut BTreeMap<String, AppearanceId>,
    appearances_reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    definition: &crate::native::om::PartColorDefinition,
    annotation_stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<AppearanceId, CodecError> {
    let color = Color::new(
        definition.components[0].0.value(),
        definition.components[1].0.value(),
        definition.components[2].0.value(),
        1.0,
    )
    .ok_or_else(|| CodecError::Malformed("RM color components must be in [0, 1]".into()))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(appearances.len()),
        "NX RM appearance reuse lookup",
    )?;
    if let Some(id) = appearances.get(&definition.id) {
        return id.try_clone_for_decode(ctx, "NX decoded IR value copy");
    }
    let identity_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(definition.id.len().checked_add(128).ok_or_else(
            || {
                ctx.refuse_codec_limit(
                    "NX RM color appearance identity",
                    0,
                    cadmpeg_core::decode::u64_from_index(definition.id.len()),
                )
            },
        )?),
        "NX RM color appearance identity",
    )?;
    let id: AppearanceId = IdScope::native(cadmpeg_ir::identity_component!("appearance")).id(
        &cadmpeg_ir::identity_component!("rmfastload-color"),
        native_entity_key(&definition.id).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "NX RMFASTLOAD_COLOR_APPEARANCE definition id is not identity key text"
            ))
        })?,
    );
    drop(identity_reservation);
    annotations.note(
        ctx,
        id.as_str(),
        annotation_stream,
        definition.source_offset,
        Some("RMFASTLOAD_COLOR_APPEARANCE"),
    )?;
    annotations
        .derived(ctx, id.as_str(), "name")
        .map_err(cadmpeg_core::CodecError::from)?;
    annotations
        .derived(ctx, id.as_str(), "schema")
        .map_err(cadmpeg_core::CodecError::from)?;
    annotations
        .derived(ctx, id.as_str(), "base_color")
        .map_err(cadmpeg_core::CodecError::from)?;
    let appearance_bytes = definition.name.len();
    ctx.charge_collection_items(1, "NX RM color appearances")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(appearance_bytes),
        "NX RM color appearance",
    )?;
    ctx.reserve_capacity(&mut ir.model.appearances, 1, "NX RM color appearances")?;
    ir.model.appearances.push(Appearance {
        id: id.try_clone_for_decode(ctx, "NX decoded IR value copy")?,
        name: Some(definition.name.clone()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some("UGS::COLOR_table".into()),
        category: None,
        base_color: Some(color),
        properties: BTreeMap::new(),
        textures: Vec::new(),
    });

    let lookup_id = appearances_reservation
        .with_storage(|| id.try_clone_for_decode(ctx, "NX RM appearance identity lookup"))?;
    appearances_reservation.with_storage(|| {
        ctx.admit_btree_entry(
            appearances,
            &definition.id,
            "NX RM appearance identity lookup",
        )
    })?;
    let lookup_key = appearances_reservation.with_storage(|| {
        ctx.copy_retained_text(&definition.id, "NX RM appearance identity lookup")
    })?;
    appearances.insert(lookup_key, lookup_id);
    Ok(id)
}

/// One agreed palette definition and its earliest source occurrence.
enum RmColorChoice<'a> {
    Unique {
        definition: &'a str,
        source_offset: u64,
    },
    Conflicting,
}

impl<'a> RmColorChoice<'a> {
    fn new(assignment: &'a RmDisplayColorAssignment) -> Self {
        Self::Unique {
            definition: &assignment.color_definition,
            source_offset: assignment.frame.offset(),
        }
    }

    fn observe(&mut self, assignment: &'a RmDisplayColorAssignment) {
        if let Self::Unique {
            definition,
            source_offset,
        } = self
        {
            if *definition == assignment.color_definition {
                *source_offset = (*source_offset).min(assignment.frame.offset());
            } else {
                *self = Self::Conflicting;
            }
        }
    }
}

fn resolve_rm_source_color_bindings(
    ctx: &DecodeContext<'_>,
    assignments: &[RmDisplayColorAssignment],
) -> Result<Vec<RmSourceColorBinding>, CodecError> {
    let mut choices = BTreeMap::<&str, RmColorChoice<'_>>::new();
    let mut choices_reservation = ctx.reserve_scoped(0, "NX RM source color choices")?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(assignments.len()),
        "NX RM source color assignments",
    )?;
    for assignment in assignments {
        let Some(source_id) = assignment.target_object_id.as_deref() else {
            continue;
        };
        if !choices.contains_key(source_id) {
            choices_reservation.with_storage(|| {
                ctx.admit_btree_entry(&choices, &source_id, "NX RM source color choices")
            })?;
        }
        choices
            .entry(source_id)
            .and_modify(|choice| choice.observe(assignment))
            .or_insert_with(|| RmColorChoice::new(assignment));
    }
    let mut bindings = Vec::new();
    for (source_id, choice) in choices {
        let RmColorChoice::Unique {
            definition,
            source_offset,
        } = choice
        else {
            continue;
        };
        let bytes = source_id
            .len()
            .checked_add(definition.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX RM source color binding",
                    0,
                    cadmpeg_core::decode::u64_from_index(source_id.len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX RM source color bindings")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX RM source color bindings",
        )?;
        ctx.reserve_capacity(&mut bindings, 1, "NX RM source color bindings")?;
        bindings.push(RmSourceColorBinding {
            source_id: source_id.to_owned(),
            color_definition: definition.to_owned(),
            source_offset,
        });
    }
    Ok(bindings)
}

fn resolve_rm_face_colors(
    ctx: &DecodeContext<'_>,
    face_ids: &BTreeSet<String>,
    assignments: &[RmDisplayColorAssignment],
    definitions: &[super::om::PartColorDefinition],
    records: &[super::parasolid::ParasolidDeltasRecord],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
) -> Result<Vec<(String, Color)>, CodecError> {
    let (bindings, _bindings_storage) =
        ctx.with_scoped_storage("NX RM color projection storage", || {
            resolve_rm_face_color_bindings(
                ctx,
                face_ids,
                assignments,
                definitions,
                records,
                delta_pairs,
            )
        })?;
    let mut colors = Vec::new();
    for binding in bindings {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(definitions.len()),
            "NX RM face color definition lookup",
        )?;
        let Some(definition) = definitions
            .iter()
            .rev()
            .find(|definition| definition.id.as_str() == binding.color_definition.as_str())
        else {
            continue;
        };
        let color = Color::new(
            definition.components[0].0.value(),
            definition.components[1].0.value(),
            definition.components[2].0.value(),
            1.0,
        )
        .ok_or_else(|| CodecError::Malformed("RM color components must be in [0, 1]".into()))?;
        ctx.reserve_vec(&mut colors, 1, "NX resolved RM face colors")?;
        let face_id = ctx.copy_retained_text(&binding.face_id, "NX resolved RM face identity")?;
        colors.push((face_id, color));
    }
    Ok(colors)
}

fn resolve_rm_face_color_bindings(
    ctx: &DecodeContext<'_>,
    face_ids: &BTreeSet<String>,
    assignments: &[RmDisplayColorAssignment],
    definitions: &[super::om::PartColorDefinition],
    records: &[super::parasolid::ParasolidDeltasRecord],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
) -> Result<Vec<RmFaceColorBinding>, CodecError> {
    let mut bindings = Vec::new();
    for (position, assignment) in assignments.iter().enumerate() {
        let RmDisplayColorAssignmentEncoding::Linked(row) = assignment.frame.encoding() else {
            continue;
        };
        let object_index = row.first_index().atom.value();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(position),
            "NX RM face color assignment identity",
        )?;
        if assignments[..position].iter().any(|earlier| {
            matches!(
                earlier.frame.encoding(),
                RmDisplayColorAssignmentEncoding::Linked(previous)
                    if previous.first_index().atom.value() == object_index
            )
        }) {
            continue;
        }
        let mut choice = RmColorChoice::new(assignment);
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(assignments.len() - position - 1),
            "NX RM face color assignments",
        )?;
        for later in assignments.iter().skip(position + 1) {
            if matches!(
                later.frame.encoding(),
                RmDisplayColorAssignmentEncoding::Linked(next)
                    if next.first_index().atom.value() == object_index
            ) {
                choice.observe(later);
            }
        }
        let RmColorChoice::Unique {
            definition,
            source_offset,
        } = choice
        else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(definitions.len()),
            "NX RM face color definitions",
        )?;
        if !definitions
            .iter()
            .any(|candidate| candidate.id.as_str() == definition)
        {
            continue;
        }

        let mut candidate = None::<String>;
        let mut ambiguous = false;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(records.len()),
            "NX RM face color records",
        )?;
        for record in records {
            let crate::deltas::record_family::RecordFamily::Face { node_id, .. } = &record.family
            else {
                continue;
            };
            if *node_id != object_index {
                continue;
            }
            let mut partition = None;
            let mut multiple_partitions = false;
            for (raw_partition, deltas) in delta_pairs {
                let work = deltas
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit("NX RM face color delta links", 0, 1))?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(work),
                    "NX RM face color delta links",
                )?;
                let Ok(partition_id) = u32::try_from(*raw_partition) else {
                    continue;
                };
                if !deltas
                    .iter()
                    .any(|delta| u32::try_from(*delta).ok() == Some(record.stream_ordinal))
                {
                    continue;
                }
                match partition {
                    None => partition = Some(partition_id),
                    Some(existing) if existing == partition_id => {}
                    Some(_) => {
                        multiple_partitions = true;
                        break;
                    }
                }
            }
            if multiple_partitions {
                continue;
            }
            let Some(partition) = partition else {
                continue;
            };
            let face_id = format!("nx:s{partition}:face#{}", record.xmt);
            if !face_ids.contains(&face_id) {
                continue;
            }
            match candidate.as_ref() {
                None => candidate = Some(face_id),
                Some(existing) if existing == &face_id => {}
                Some(_) => {
                    ambiguous = true;
                    break;
                }
            }
        }
        if ambiguous {
            continue;
        }
        let Some(face_id) = candidate else {
            continue;
        };
        let bytes = face_id.len().checked_add(definition.len()).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX RM face color binding",
                0,
                cadmpeg_core::decode::u64_from_index(face_id.len()),
            )
        })?;
        ctx.charge_collection_items(1, "NX RM face color bindings")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX RM face color bindings",
        )?;
        ctx.reserve_capacity(&mut bindings, 1, "NX RM face color bindings")?;
        bindings.push(RmFaceColorBinding {
            face_id,
            color_definition: definition.to_owned(),
            source_offset,
        });
    }
    ctx.stable_sort_by(
        &mut bindings,
        |left, right| left.face_id.cmp(&right.face_id),
        |binding| binding.face_id.len(),
        "sort NX RM face color bindings",
    )?;
    Ok(bindings)
}

/// Transfer each independently validated JPEG preview with its exact bounded
/// container bytes. Invalid entries remain absent from the neutral asset arena.
fn attach_jpeg_preview_assets(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), CodecError> {
    let stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    for (ordinal, entry) in scan
        .container
        .entries
        .iter()
        .filter(|entry| entry.content() == EntryContent::PreviewImage)
        .enumerate()
    {
        let Some((source_offset, source_byte_len)) = entry.file_span() else {
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
        let native_ref: UnknownId =
            IdScope::container().id(&cadmpeg_ir::identity_component!("jpeg-preview"), ordinal);
        if crate::decode::jpeg::jpeg_dimensions(bytes).is_none() {
            annotations.note(
                ctx,
                native_ref.as_str(),
                &stream,
                source_offset,
                Some("JPEG_PREVIEW_INVALID"),
            )?;
            annotations.exactness(ctx, native_ref.as_str(), Exactness::ByteExact)?;
            push_native_unknown(
                ctx,
                unknowns,
                UnknownRecord::retained(
                    native_ref,
                    source_offset,
                    ctx.copy_retained(bytes, "retain NX invalid JPEG preview")?,
                    Vec::new(),
                ),
            )?;
            continue;
        }
        let id: AssetId = extended_id(native_ref.as_str(), &cadmpeg_ir::identity_key!("asset"))
            .ok_or_else(|| {
                CodecError::malformed(format_args!("NX JPEG preview id is not an identity"))
            })?;
        annotations.note(
            ctx,
            id.as_str(),
            &stream,
            source_offset,
            Some("JPEG_PREVIEW_ASSET"),
        )?;
        annotations.exactness(ctx, id.as_str(), Exactness::ByteExact)?;
        annotations
            .derived(ctx, id.as_str(), "id")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, id.as_str(), "name")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, id.as_str(), "media_type")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, id.as_str(), "native_ref")
            .map_err(cadmpeg_core::CodecError::from)?;
        ctx.reserve_vec(&mut ir.model.assets, 1, "NX JPEG preview assets")?;
        ir.model.assets.push(
            Asset::try_new(
                id,
                Some(if ordinal == 0 {
                    "preview.jpg".to_string()
                } else {
                    format!("preview-{ordinal}.jpg")
                }),
                Some("image/jpeg".to_string()),
                AssetContent::Embedded {
                    data: cadmpeg_ir::assets::AssetData::new(
                        ctx.copy_retained(bytes, "retain NX JPEG preview asset")?,
                    )
                    .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
                },
                Some(native_ref.into_string()),
            )
            .map_err(CodecError::Malformed)?,
        );
    }
    Ok(())
}

/// Transfer the complete validated TIFF set atomically.
fn attach_material_texture_assets(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    textures: &[crate::native::om::material_texture::MaterialTextureAsset],
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let mut sources = Vec::new();
    let mut source_reservation = ctx.reserve_scoped(0, "NX material texture source list")?;
    for texture in textures {
        let Some(start) = usize::try_from(texture.source_offset).ok() else {
            return Ok(());
        };
        let Some(byte_len) = usize::try_from(texture.byte_len()).ok() else {
            return Ok(());
        };
        let Some(end) = start.checked_add(byte_len) else {
            return Ok(());
        };
        let Some(bytes) = scan.container.data.get(start..end) else {
            return Ok(());
        };
        let (digest, _digest_storage) =
            ctx.with_scoped_storage("NX material texture hash", || {
                cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                    ctx,
                    bytes,
                    "NX material texture hash",
                )
            })?;
        if digest != texture.sha256 {
            return Ok(());
        }
        ctx.reserve_scoped_vec(
            &mut source_reservation,
            &mut sources,
            1,
            "NX material texture source list",
        )?;
        sources.push((texture, bytes));
    }

    let mut assets = Vec::new();
    for (texture, bytes) in sources {
        let id_bytes = texture
            .id
            .len()
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(":asset".len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX material asset identity",
                    0,
                    cadmpeg_core::decode::u64_from_index(texture.id.len()),
                )
            })?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(id_bytes),
            "NX material asset identity",
        )?;
        let text_bytes = texture
            .name()
            .len()
            .checked_add(texture.id.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX material asset text",
                    0,
                    cadmpeg_core::decode::u64_from_index(texture.id.len()),
                )
            })?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(text_bytes),
            "NX material asset text",
        )?;
        ctx.reserve_vec(&mut assets, 1, "NX material asset records")?;
        assets.push(
            Asset::try_new(
                extended_id::<AssetId>(texture.id.as_str(), &cadmpeg_ir::identity_key!("asset"))
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "NX material texture id is not an identity"
                        ))
                    })?,
                Some(texture.name().to_owned()),
                Some("image/tiff".to_string()),
                AssetContent::Embedded {
                    data: cadmpeg_ir::assets::AssetData::new(
                        ctx.copy_retained(bytes, "retain NX TIFF material asset")?,
                    )
                    .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
                },
                Some(texture.id.clone()),
            )
            .map_err(CodecError::Malformed)?,
        );
    }
    let stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    drop(source_reservation);
    for (texture, asset) in textures.iter().zip(&assets) {
        annotations.note(
            ctx,
            asset.id.as_str(),
            &stream,
            texture.source_offset,
            Some("MATERIAL_TEXTURE_ASSET"),
        )?;
        annotations.exactness(ctx, asset.id.as_str(), Exactness::ByteExact)?;
        annotations
            .derived(ctx, asset.id.as_str(), "id")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, asset.id.as_str(), "media_type")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(ctx, asset.id.as_str(), "native_ref")
            .map_err(cadmpeg_core::CodecError::from)?;
    }

    ctx.reserve_vec(
        &mut ir.model.assets,
        assets.len(),
        "NX attached material assets",
    )?;
    ir.model.assets.extend(assets);
    Ok(())
}

fn attach_active_configuration_parameter_values(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(configuration_index) = unique_active_configuration_index(&ir.model.configurations)
    else {
        return Ok(());
    };
    let configuration = &ir.model.configurations[configuration_index];
    if configuration.bodies.is_none()
        || !configuration.parameter_values.is_empty()
        || ir.model.parameters.is_empty()
    {
        return Ok(());
    }
    for (index, parameter) in ir.model.parameters.iter().enumerate() {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(index),
            "NX configuration parameter identity uniqueness",
        )?;
        if ir.model.parameters[..index]
            .iter()
            .any(|previous| previous.id == parameter.id)
        {
            return Ok(());
        }
    }
    for parameter in &ir.model.parameters {
        for dependency in &parameter.dependencies {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(ir.model.parameters.len()),
                "NX configuration parameter dependencies",
            )?;
            let Some(preceding) = ir
                .model
                .parameters
                .iter()
                .find(|candidate| candidate.id == *dependency)
            else {
                return Ok(());
            };
            if preceding.owner != parameter.owner || preceding.ordinal >= parameter.ordinal {
                return Ok(());
            }
        }
    }
    // A parameter with no evaluated value leaves the configuration untouched,
    // which is what the dependency guard above does for its own case.
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(ir.model.parameters.len()),
        "NX configuration parameter values",
    )?;
    if ir
        .model
        .parameters
        .iter()
        .any(|parameter| parameter.value.is_none())
    {
        return Ok(());
    }
    let mut values = BTreeMap::new();
    for parameter in &ir.model.parameters {
        let Some(value) = parameter.value.as_ref() else {
            return Ok(());
        };
        if !values.contains_key(&parameter.id) {
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                    ParameterId,
                    ParameterValue,
                )>()),
                "NX active configuration parameter values",
            )?;
        }
        let id = parameter
            .id
            .try_clone_for_decode(ctx, "NX active configuration parameter identity")?;
        let value = value.try_clone_for_decode(ctx, "NX active configuration parameter value")?;
        ctx.insert_btree_map(
            &mut values,
            id,
            value,
            "NX active configuration parameter values",
        )?;
    }
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.parameter_values = values;
    annotations
        .derived(ctx, configuration.id.as_str(), "parameter_values")
        .map_err(cadmpeg_core::CodecError::from)?;
    Ok(())
}

fn attach_current_feature_states(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut current_bodies = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX current body identities")?;
    for body in &ir.model.bodies {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut current_bodies,
            1,
            "NX current body identities",
        )?;
        current_bodies.push(reservation.with_storage(|| {
            body.id
                .try_clone_for_decode(ctx, "NX current body identity")
        })?);
    }
    let Ok(active_features) = active_feature_closure_for_decode(ctx, ir, &current_bodies)? else {
        return Ok(());
    };
    for index in active_features.into_values() {
        let feature = &mut ir.model.features[index];
        feature.suppressed = Some(false);
        annotations
            .derived(ctx, &feature.id, "suppressed")
            .map_err(cadmpeg_core::CodecError::from)?;
    }
    Ok(())
}

fn attach_active_configuration_feature_states(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(configuration_index) = unique_active_configuration_index(&ir.model.configurations)
    else {
        return Ok(());
    };
    let Some(configuration_bodies) = ir.model.configurations[configuration_index]
        .bodies
        .as_deref()
    else {
        return Ok(());
    };
    if !ir.model.configurations[configuration_index]
        .feature_states
        .is_empty()
    {
        return Ok(());
    }
    let Ok(active_features) = active_feature_closure_for_decode(ctx, ir, configuration_bodies)?
    else {
        return Ok(());
    };
    let mut states = BTreeMap::new();
    for (id, &index) in &active_features {
        let feature = &ir.model.features[index];
        let outputs = ctx.try_collect_retained_with(
            feature.evaluation.outputs().iter(),
            "NX configuration feature outputs",
            |output| output.try_clone_for_decode(ctx, "NX configuration feature output"),
        )?;
        let dependencies = ctx.try_collect_retained_with(
            feature.dependencies.iter(),
            "NX configuration feature dependencies",
            |dependency| {
                dependency.try_clone_for_decode(ctx, "NX configuration feature dependency")
            },
        )?;
        let definition = feature
            .evaluation
            .definition()
            .try_clone_for_decode(ctx, "NX feature definition copy")?;
        if !states.contains_key(id) {
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                    FeatureId,
                    ConfigurationFeatureState,
                )>()),
                "NX configuration feature state",
            )?;
        }
        let state_id = id.try_clone_for_decode(ctx, "NX configuration feature state identity")?;
        ctx.insert_btree_map(
            &mut states,
            state_id,
            ConfigurationFeatureState {
                evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                    outputs: cadmpeg_ir::features::DistinctMembers::try_from(outputs, ctx)
                        .map_err(cadmpeg_core::CodecError::from)?,
                },
                dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
                definition,
            },
            "NX configuration feature states",
        )?;
    }
    for index in active_features.into_values() {
        let feature = &mut ir.model.features[index];
        if feature.suppressed != Some(false) {
            feature.suppressed = Some(false);
            annotations
                .derived(ctx, &feature.id, "suppressed")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.feature_states = states;
    annotations
        .derived(ctx, configuration.id.as_str(), "feature_states")
        .map_err(cadmpeg_core::CodecError::from)?;
    Ok(())
}

fn unique_active_configuration_index(configurations: &[DesignConfiguration]) -> Option<usize> {
    let mut active = configurations
        .iter()
        .enumerate()
        .filter_map(|(index, configuration)| configuration.active.then_some(index));
    let index = active.next()?;
    active.next().is_none().then_some(index)
}

/// Materialize the exact body set present when retained feature replay begins.
fn attach_initial_segment_bodies(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    body_bindings: &[crate::native::segments::SegmentBodyBinding],
    annotations: &mut AnnotationBuilder,
    stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<Option<FeatureId>, CodecError> {
    let body_count = ir.model.bodies.len();
    let scan_work = body_count.checked_mul(body_bindings.len()).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX retained-history body binding scan",
            0,
            cadmpeg_core::decode::u64_from_index(body_count),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan_work),
        "NX retained-history body binding scan",
    )?;
    let has_binding = ir.model.bodies.iter().any(|body| {
        body_bindings.iter().any(|binding| {
            body.id
                .as_str()
                .starts_with(&format!("nx:s{}:", binding.stream_ordinal))
        })
    });
    if !has_binding {
        return Ok(None);
    }

    let (mut sorted_bodies, _sorting) =
        ctx.temporary_vec(body_count, "NX retained-history body order")?;
    sorted_bodies.extend(&ir.model.bodies);
    ctx.stable_sort_by(
        &mut sorted_bodies,
        |first, second| first.id.cmp(&second.id),
        |body| body.id.as_str().len(),
        "sort NX retained-history bodies",
    )?;

    let id: FeatureId = IdScope::native(cadmpeg_ir::identity_component!("feature-history")).id(
        &cadmpeg_ir::identity_component!("feature"),
        cadmpeg_ir::identity_key!("initial-bodies"),
    );
    let mut selection_bodies = Vec::new();
    let mut feature_outputs = Vec::new();
    let mut source_properties = BTreeMap::new();
    let mut binding_ordinal = 0usize;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan_work),
        "NX retained-history body binding scan",
    )?;
    for (position, body) in sorted_bodies.iter().enumerate() {
        if sorted_bodies
            .get(position + 1)
            .is_some_and(|next| next.id == body.id)
        {
            continue;
        }
        let mut matched = false;
        for binding in body_bindings {
            if !body
                .id
                .as_str()
                .starts_with(&format!("nx:s{}:", binding.stream_ordinal))
            {
                continue;
            }
            matched = true;
            let mut digits = 1usize;
            let mut value = binding_ordinal;
            while value >= 10 {
                value /= 10;
                digits += 1;
            }
            let key_bytes = "segment_body_binding."
                .len()
                .checked_add(digits)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX retained-history binding property",
                        0,
                        cadmpeg_core::decode::u64_from_index(binding_ordinal),
                    )
                })?;
            let bytes = std::mem::size_of::<(String, String)>()
                .checked_add(key_bytes)
                .and_then(|bytes| bytes.checked_add(binding.id.len()))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX retained-history binding property",
                        0,
                        cadmpeg_core::decode::u64_from_index(binding.id.len()),
                    )
                })?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(bytes),
                "NX retained-history binding properties",
            )?;
            ctx.insert_btree_map(
                &mut source_properties,
                cadmpeg_core::nonblank_literal!("segment_body_binding.{binding_ordinal}"),
                binding.id.clone(),
                "NX retained-history binding properties",
            )?;
            binding_ordinal = binding_ordinal.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("NX retained-history binding ordinal", 0, 1)
            })?;
        }
        if matched {
            ctx.charge_collection_items(2, "NX retained-history output bodies")?;
            ctx.reserve_capacity(
                &mut selection_bodies,
                1,
                "NX retained-history output bodies",
            )?;
            ctx.reserve_capacity(&mut feature_outputs, 1, "NX retained-history output bodies")?;
            selection_bodies.push(
                body.id
                    .try_clone_for_decode(ctx, "NX decoded IR value copy")?,
            );
            feature_outputs.push(
                body.id
                    .try_clone_for_decode(ctx, "NX decoded IR value copy")?,
            );
        }
    }

    annotations.note(ctx, &id, stream, 0, Some("FEATURE_HISTORY_INPUT"))?;
    annotations
        .derived(ctx, &id, "definition")
        .map_err(cadmpeg_core::CodecError::from)?;
    let uniqueness_work = feature_outputs
        .len()
        .checked_mul(feature_outputs.len())
        .and_then(|work| work.checked_mul(2))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX retained-history output uniqueness",
                0,
                cadmpeg_core::decode::u64_from_index(feature_outputs.len()),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(uniqueness_work),
        "NX retained-history output uniqueness",
    )?;
    ctx.reserve_vec(
        &mut ir.model.features,
        1,
        "NX retained-history input features",
    )?;
    ir.model.features.push(Feature {
        id: id.try_clone_for_decode(ctx, "NX decoded IR value copy")?,
        ordinal: cadmpeg_core::decode::u64_from_index(ir.model.features.len()),
        name: Some("Retained history input".to_string()),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties,
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: cadmpeg_ir::features::DistinctMembers::try_from(selection_bodies, ctx)
                        .map_err(cadmpeg_core::CodecError::from)?,
                    native: "nx:segment-body-bindings".to_string(),
                },
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(feature_outputs, ctx)
                .map_err(cadmpeg_core::CodecError::from)?,
        ),
        native_ref: None,
    });
    Ok(Some(id))
}

fn attach_feature_operations(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let features = &model.features;
    let parasolid_group_members = model.parasolid.group_members.as_slice();
    let data_blocks = model.om.data_blocks.as_slice();
    let expressions = model.om.expressions.as_slice();
    let body_bindings = model.segments.body_bindings.as_slice();
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
    let pattern_counted_reference_lanes =
        features.feature_pattern_counted_reference_lanes.as_slice();
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
    let sketch_payload_scalar_lanes = features.feature_sketch_payload_scalar_lanes.as_slice();
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
    let operation_body_writes = features.feature_operation_body_writes.as_slice();
    let operation_body_image_segment_uses = features
        .feature_operation_body_image_segment_uses
        .as_slice();
    let operation_body_identity_segment_uses = features
        .feature_operation_body_identity_segment_uses
        .as_slice();
    let operation_body_partition_uses = features.feature_operation_body_partition_uses.as_slice();
    let body_write_group_partition_uses =
        features.feature_body_write_group_partition_uses.as_slice();
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
    let admitted_body_references = native_primary_body_references(
        ctx,
        body_references,
        body_data_block_uses,
        body_segment_uses,
        input_blocks,
        data_blocks,
    )?;
    let stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    let initial_body_id =
        attach_initial_segment_bodies(ctx, ir, body_bindings, annotations, &stream)?;
    let base_ordinal = cadmpeg_core::decode::u64_from_index(ir.model.features.len());
    let (booleans, _booleans_reservation) = ctx.collect_scoped_btree_map(
        booleans
            .iter()
            .map(|operation| (operation.operation_label.as_str(), operation)),
        "NX last-record index",
    )?;
    let (body_references_by_id, _body_references_by_id_reservation) = ctx
        .collect_scoped_btree_map(
            body_references
                .iter()
                .map(|reference| (reference.id.as_str(), reference)),
            "NX last-record index",
        )?;
    let mut group_reservation = ctx.reserve_scoped(0, "NX feature operation group indexes")?;
    let mut body_segment_uses_by_reference =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodySegmentUse>>::new();
    for use_ in body_segment_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut body_segment_uses_by_reference,
            use_.feature_body_reference.as_str(),
            || use_,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut body_data_block_uses_by_reference =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodyDataBlockUse>>::new();
    for use_ in body_data_block_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut body_data_block_uses_by_reference,
            use_.feature_body_reference.as_str(),
            || use_,
            0,
            "NX feature operation group index",
        )?;
    }
    let (body_writer_references_by_operation, _body_writer_references_by_operation_storage) = ctx
        .with_scoped_storage(
        "NX local body_writer_references_by_operation storage",
        || crate::native::features::unique_feature_body_references(ctx, body_references),
    )?;
    let mut offset_store_bodies_by_operation = BTreeMap::<&str, Vec<(u32, String)>>::new();
    for body_use in body_data_block_uses {
        let Some(reference) = body_references_by_id.get(body_use.feature_body_reference.as_str())
        else {
            continue;
        };
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut offset_store_bodies_by_operation,
            reference.operation_label.as_str(),
            || (reference.body.value(), body_use.data_block.clone()),
            body_use.data_block.len(),
            "NX feature operation group index",
        )?;
    }
    let body_references = admitted_body_references;
    let mut body_reference_occurrences_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodyReference>>::new();
    for reference in body_reference_occurrences {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut body_reference_occurrences_by_operation,
            reference.operation_label.as_str(),
            || reference,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut body_writer_history = BodyWriterHistory::default();
    if let Some(feature) = initial_body_id
        .as_ref()
        .and_then(|id| ir.model.features.iter().find(|feature| feature.id == *id))
    {
        body_writer_history.record_writer(
            ctx,
            None,
            None,
            feature.evaluation.outputs(),
            &feature.id,
        )?;
    }
    let (body_alias_roots, _body_alias_roots_storage) = ctx
        .with_scoped_storage("NX local body_alias_roots storage", || {
            crate::native::segments::body_alias_roots(ctx, body_bindings)
        })?;
    let canonical_body =
        |identity: u32| body_alias_roots.get(&identity).copied().unwrap_or(identity);
    let mut input_blocks_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureInputBlock>>::new();
    for input in input_blocks {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut input_blocks_by_operation,
            input.operation_label.as_str(),
            || input,
            0,
            "NX feature operation group index",
        )?;
    }
    let (input_column_row_uses_by_operation, _input_column_row_uses_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            input_column_row_uses
                .iter()
                .map(|use_| (use_.operation_label.as_str(), use_)),
            "NX operation record index",
        )?;
    let (input_column_targets_by_operation, _input_column_targets_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
        input_column_targets
            .iter()
            .map(|target| (target.operation_label.as_str(), target)),
        "NX operation record index",
    )?;
    let (input_block_identity_group_by_input, _input_block_identity_group_by_input_reservation) =
        ctx.collect_scoped_btree_map(
            input_block_identity_groups.iter().flat_map(|group| {
                group
                    .members
                    .iter()
                    .map(move |member| (member.input_block.as_str(), group.id.as_str()))
            }),
            "NX last-record index",
        )?;
    let (datum_csys_constructions_by_operation, _datum_csys_constructions_by_operation_reservation) =
        ctx.collect_scoped_btree_map(
            datum_csys_constructions
                .iter()
                .map(|construction| (construction.operation_label.as_str(), construction)),
            "NX last-record index",
        )?;
    let (datum_csys_payloads_by_operation, _datum_csys_payloads_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            datum_csys_payloads
                .iter()
                .map(|payload| (payload.operation_label.as_str(), payload)),
            "NX operation record index",
        )?;
    let (
        datum_csys_payload_scalar_pairs_by_operation,
        _datum_csys_payload_scalar_pairs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_csys_payload_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
        "NX operation record index",
    )?;
    let (
        datum_csys_payload_fixed_pairs_by_operation,
        _datum_csys_payload_fixed_pairs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_csys_payload_fixed_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
        "NX operation record index",
    )?;
    let (
        datum_csys_payload_scalars_by_operation,
        _datum_csys_payload_scalars_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_csys_payload_scalars
            .iter()
            .map(|scalar| (scalar.operation_label.as_str(), scalar)),
        "NX operation record index",
    )?;
    let (datum_csys_descriptors_by_operation, _datum_csys_descriptors_by_operation_reservation) =
        ctx.collect_scoped_btree_groups(
            datum_csys_descriptors
                .iter()
                .map(|descriptor| (descriptor.operation_label.as_str(), descriptor)),
            "NX operation record index",
        )?;
    let (
        datum_csys_column_row_uses_by_operation,
        _datum_csys_column_row_uses_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_csys_column_row_uses
            .iter()
            .map(|use_| (use_.operation_label.as_str(), use_)),
        "NX operation record index",
    )?;
    let mut datum_csys_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumCsysBlockUse>>::new();
    for block_use in datum_csys_block_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut datum_csys_uses_by_input_operation,
            block_use.input_operation_label.as_str(),
            || block_use,
            0,
            "NX feature operation group index",
        )?;
    }
    let (datum_plane_headers_by_operation, _datum_plane_headers_by_operation_reservation) = ctx
        .collect_scoped_btree_map(
            datum_plane_headers
                .iter()
                .map(|header| (header.operation_label.as_str(), header)),
            "NX last-record index",
        )?;
    let (datum_plane_payloads_by_operation, _datum_plane_payloads_by_operation_reservation) = ctx
        .collect_scoped_btree_map(
        datum_plane_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX last-record index",
    )?;
    let (
        datum_plane_payload_scalar_pairs_by_operation,
        _datum_plane_payload_scalar_pairs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_plane_payload_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
        "NX operation record index",
    )?;
    let (datum_plane_descriptors_by_operation, _datum_plane_descriptors_by_operation_reservation) =
        ctx.collect_scoped_btree_groups(
            datum_plane_descriptors
                .iter()
                .map(|descriptor| (descriptor.operation_label.as_str(), descriptor)),
            "NX operation record index",
        )?;
    let mut datum_plane_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneBlockUse>>::new();
    for block_use in datum_plane_block_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut datum_plane_uses_by_input_operation,
            block_use.input_operation_label.as_str(),
            || block_use,
            0,
            "NX feature operation group index",
        )?;
    }
    let (chronological_labels, _chronological_labels_storage) = ctx
        .with_scoped_storage("NX local chronological_labels storage", || {
            crate::native::features::feature_operation_chronological_labels(ctx, labels)
        })?;
    let (operation_positions, _operation_positions_reservation) = ctx.collect_scoped_btree_map(
        chronological_labels
            .iter()
            .enumerate()
            .map(|(position, label)| (label.id.as_str(), position)),
        "NX last-record index",
    )?;
    let (sketch_datum_csys_dependencies, _sketch_datum_csys_dependencies_reservation) = ctx
        .collect_scoped_btree_map(
            sketch_datum_csys_dependencies
                .iter()
                .map(|dependency| (dependency.datum_csys_operation_label.as_str(), dependency)),
            "NX last-record index",
        )?;
    let mut datum_identity_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneCsysIdentityUse>>::new();
    for identity_use in datum_plane_csys_identity_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut datum_identity_uses_by_operation,
            identity_use.datum_plane_operation_label.as_str(),
            || identity_use,
            0,
            "NX feature operation group index",
        )?;
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut datum_identity_uses_by_operation,
            identity_use.datum_csys_operation_label.as_str(),
            || identity_use,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut sketch_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchReference>>::new();
    for reference in sketch_references {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_references_by_operation,
            reference.operation_label.as_str(),
            || reference,
            0,
            "NX feature operation group index",
        )?;
    }
    let (
        projected_curve_references_by_operation,
        _projected_curve_references_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        projected_curve_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "NX operation record index",
    )?;
    let (
        projected_curve_construction_payloads_by_operation,
        _projected_curve_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        projected_curve_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let (
        projected_curve_construction_strings_by_operation,
        _projected_curve_construction_strings_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        projected_curve_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
        "NX operation record index",
    )?;
    let (fset_reference_graphs_by_operation, _fset_reference_graphs_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            fset_reference_graphs
                .iter()
                .map(|graph| (graph.operation_label.as_str(), graph)),
            "NX operation record index",
        )?;
    let (
        fset_construction_payloads_by_operation,
        _fset_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        fset_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let (delete_reference_fields_by_operation, _delete_reference_fields_by_operation_reservation) =
        ctx.collect_scoped_btree_groups(
            delete_reference_fields
                .iter()
                .map(|field| (field.operation_label.as_str(), field)),
            "NX operation record index",
        )?;
    let (
        delete_construction_payloads_by_operation,
        _delete_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        delete_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let (pattern_references_by_operation, _pattern_references_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            pattern_references
                .iter()
                .map(|reference| (reference.operation_label.as_str(), reference)),
            "NX operation record index",
        )?;
    let (
        pattern_counted_reference_lanes_by_operation,
        _pattern_counted_reference_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        pattern_counted_reference_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX operation record index",
    )?;
    let (
        pattern_construction_payloads_by_operation,
        _pattern_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        pattern_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let (
        pattern_construction_strings_by_operation,
        _pattern_construction_strings_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        pattern_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
        "NX operation record index",
    )?;
    let (
        pattern_construction_fixed_lanes_by_operation,
        _pattern_construction_fixed_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        pattern_construction_fixed_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX operation record index",
    )?;
    let (pattern_transform_lanes_by_operation, _pattern_transform_lanes_by_operation_reservation) =
        ctx.collect_scoped_btree_groups(
            pattern_transform_lanes
                .iter()
                .map(|lane| (lane.operation_label.as_str(), lane)),
            "NX operation record index",
        )?;
    let (
        multi_instance_output_lanes_by_operation,
        _multi_instance_output_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        multi_instance_output_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX operation record index",
    )?;
    let (
        identical_instance_output_lanes_by_operation,
        _identical_instance_output_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        identical_instance_output_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX operation record index",
    )?;
    let (
        point_construction_headers_by_operation,
        _point_construction_headers_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        point_construction_headers
            .iter()
            .map(|header| (header.operation_label.as_str(), header)),
        "NX last-record index",
    )?;
    let (
        point_construction_scalar_lanes_by_operation,
        _point_construction_scalar_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        point_construction_scalar_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX last-record index",
    )?;
    let (
        draft_construction_references_by_operation,
        _draft_construction_references_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "NX operation record index",
    )?;
    let (
        draft_construction_index_lanes_by_operation,
        _draft_construction_index_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_index_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX operation record index",
    )?;
    let (
        draft_construction_payloads_by_operation,
        _draft_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let (
        draft_construction_graph_payloads_by_operation,
        _draft_construction_graph_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_graph_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let (
        draft_construction_fixed_lanes_by_operation,
        _draft_construction_fixed_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_fixed_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX operation record index",
    )?;
    let (
        draft_construction_binary32_lanes_by_operation,
        _draft_construction_binary32_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_binary32_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX operation record index",
    )?;
    let (
        draft_construction_graph_strings_by_operation,
        _draft_construction_graph_strings_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_graph_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
        "NX operation record index",
    )?;
    let (
        draft_construction_identity_frames_by_operation,
        _draft_construction_identity_frames_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_identity_frames
            .iter()
            .map(|frame| (frame.operation_label.as_str(), frame)),
        "NX operation record index",
    )?;
    let (
        draft_construction_terminal_lanes_by_operation,
        _draft_construction_terminal_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_terminal_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX operation record index",
    )?;
    let (
        surface_construction_references_by_operation,
        _surface_construction_references_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "NX operation record index",
    )?;
    let (
        surface_construction_payloads_by_operation,
        _surface_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let (
        surface_construction_scalar_pairs_by_operation,
        _surface_construction_scalar_pairs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
        "NX operation record index",
    )?;
    let (
        surface_construction_strings_by_operation,
        _surface_construction_strings_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
        "NX operation record index",
    )?;
    let (
        surface_construction_branches_by_operation,
        _surface_construction_branches_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_branches
            .iter()
            .map(|branch| (branch.operation_label.as_str(), branch)),
        "NX operation record index",
    )?;
    let mut sketch_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchNamedPointBlockUse>>::new();
    for block_use in sketch_named_point_block_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_named_point_uses_by_operation,
            block_use.operation_label.as_str(),
            || block_use,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut sketch_preceding_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPrecedingNamedPointUse>>::new();
    for point_use in sketch_preceding_named_point_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_preceding_named_point_uses_by_operation,
            point_use.operation_label.as_str(),
            || point_use,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut sketch_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPointUse>>::new();
    for point_use in sketch_point_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_point_uses_by_operation,
            point_use.operation_label.as_str(),
            || point_use,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut sketch_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPointGroup>>::new();
    for group in sketch_point_groups {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_point_groups_by_operation,
            group.operation_label.as_str(),
            || group,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut extrude_profile_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureExtrudeProfileReference>>::new();
    for reference in extrude_profile_references {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut extrude_profile_references_by_operation,
            reference.operation_label.as_str(),
            || reference,
            0,
            "NX feature operation group index",
        )?;
    }
    let (
        extrude_construction_profiles_by_operation,
        _extrude_construction_profiles_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        extrude_construction_profiles
            .iter()
            .map(|profile| (profile.operation_label.as_str(), profile)),
        "NX last-record index",
    )?;
    let mut operation_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_operands_by_operation,
            operand.operation_label.as_str(),
            || operand,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut segment_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands
        .iter()
        .filter(|operand| !operand.segment_body_bindings.is_empty())
    {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut segment_body_operands_by_operation,
            operand.operation_label.as_str(),
            || operand,
            0,
            "NX feature operation group index",
        )?;
    }
    let (
        sketch_construction_inputs_by_operation,
        _sketch_construction_inputs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        sketch_construction_inputs
            .iter()
            .map(|inputs| (inputs.operation_label.as_str(), inputs)),
        "NX last-record index",
    )?;
    let (sketch_records_by_operation, _sketch_records_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            sketch_records
                .iter()
                .map(|record| (record.operation_label.as_str(), record)),
            "NX operation record index",
        )?;
    let (
        sketch_construction_payloads_by_operation,
        _sketch_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        sketch_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let mut sketch_coordinate_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeaturePayloadScalarPair>>::new();
    for pair in sketch_coordinate_pairs {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_coordinate_pairs_by_operation,
            pair.operation_label.as_str(),
            || pair,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut sketch_fixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadFixedPair>>::new();
    for pair in sketch_fixed_pairs {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_fixed_pairs_by_operation,
            pair.operation_label.as_str(),
            || pair,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut sketch_mixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadMixedPair>>::new();
    for pair in sketch_mixed_pairs {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_mixed_pairs_by_operation,
            pair.operation_label.as_str(),
            || pair,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut sketch_payload_scalar_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadScalarLane>>::new();
    for lane in sketch_payload_scalar_lanes {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_payload_scalar_lanes_by_operation,
            lane.operation_label.as_str(),
            || lane,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut sketch_fixed_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchFixedPoint>>::new();
    for point in sketch_fixed_points {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_fixed_points_by_operation,
            point.operation_label.as_str(),
            || point,
            0,
            "NX feature operation group index",
        )?;
    }
    let (block_constructions_by_operation, _block_constructions_by_operation_reservation) = ctx
        .collect_scoped_btree_map(
            block_constructions
                .iter()
                .map(|construction| (construction.operation_label.as_str(), construction)),
            "NX last-record index",
        )?;
    let (
        block_construction_payloads_by_operation,
        _block_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        block_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX operation record index",
    )?;
    let (block_dimensions_by_operation, _block_dimensions_by_operation_reservation) = ctx
        .collect_scoped_btree_map(
            block_dimensions
                .iter()
                .map(|dimensions| (dimensions.operation_label.as_str(), dimensions)),
            "NX last-record index",
        )?;
    let mut block_payload_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBlockPayloadPoint>>::new();
    for point in block_payload_points {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut block_payload_points_by_operation,
            point.operation_label.as_str(),
            || point,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut block_payload_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBlockPayloadPointGroup>>::new();
    for group in block_payload_point_groups {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut block_payload_point_groups_by_operation,
            group.operation_label.as_str(),
            || group,
            0,
            "NX feature operation group index",
        )?;
    }
    let (extrude_32_constructions_by_operation, _extrude_32_constructions_by_operation_reservation) =
        ctx.collect_scoped_btree_map(
            extrude_32_constructions
                .iter()
                .map(|construction| (construction.operation_label.as_str(), construction)),
            "NX last-record index",
        )?;
    let (extrude_payload_headers_by_operation, _extrude_payload_headers_by_operation_reservation) =
        ctx.collect_scoped_btree_map(
            extrude_payload_headers
                .iter()
                .map(|header| (header.operation_label.as_str(), header)),
            "NX last-record index",
        )?;
    let (
        operation_terminal_discriminators_by_operation,
        _operation_terminal_discriminators_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        operation_terminal_discriminators
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX last-record index",
    )?;
    let (
        extrude_payload_32_branches_by_operation,
        _extrude_payload_32_branches_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        extrude_payload_32_branches
            .iter()
            .map(|branch| (branch.operation_label.as_str(), branch)),
        "NX operation record index",
    )?;
    let mut operation_body_scalar_triples_by_operation = BTreeMap::<
        &str,
        Vec<&crate::native::features::body_scalar_triple::FeatureOperationBodyScalarTriple>,
    >::new();
    for triple in operation_body_scalar_triples {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_scalar_triples_by_operation,
            triple.operation_label.as_str(),
            || triple,
            0,
            "NX feature operation group index",
        )?;
    }
    for triples in operation_body_scalar_triples_by_operation.values_mut() {
        ctx.stable_sort_by(
            triples,
            |left, right| {
                left.body_reference_ordinal
                    .cmp(&right.body_reference_ordinal)
            },
            |_| 0,
            "sort NX operation body scalar triples",
        )?;
    }
    let mut operation_body_members_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyMember>>::new();
    for member in operation_body_members {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_members_by_operation,
            member.operation_label.as_str(),
            || member,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut operation_body_11_continuations_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBody11Continuation>>::new();
    for continuation in operation_body_11_continuations {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_11_continuations_by_operation,
            continuation.operation_label.as_str(),
            || continuation,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut operation_body_reference_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyReferenceLane>>::new();
    for lane in operation_body_reference_lanes {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_reference_lanes_by_operation,
            lane.operation_label.as_str(),
            || lane,
            0,
            "NX feature operation group index",
        )?;
    }
    let SegmentBindingBodyIndexes {
        by_object: bodies_by_object_index,
        by_binding: bodies_by_segment_binding,
        _reservation: _body_index_reservation,
    } = segment_binding_body_indexes(ctx, ir, body_bindings)?;
    let (mut body_image_outputs_by_write, mut body_output_reservation) =
        operation_body_image_outputs_by_write(
            ctx,
            operation_body_image_segment_uses,
            &bodies_by_segment_binding,
        )?;
    let mut conflicting_body_output_writes = BTreeSet::new();
    let (identity_candidates, _identity_candidate_reservation) =
        operation_body_identity_outputs_by_write(
            ctx,
            operation_body_identity_segment_uses,
            &bodies_by_segment_binding,
        )?;
    merge_operation_body_outputs(
        ctx,
        &mut body_output_reservation,
        &mut body_image_outputs_by_write,
        &mut conflicting_body_output_writes,
        &identity_candidates,
    )?;
    let (partition_candidates, _partition_candidate_reservation) =
        operation_body_group_partition_outputs_by_write(
            ctx,
            operation_body_writes,
            body_write_group_partition_uses,
            &ir.model.bodies,
        )?;
    merge_operation_body_outputs(
        ctx,
        &mut body_output_reservation,
        &mut body_image_outputs_by_write,
        &mut conflicting_body_output_writes,
        &partition_candidates,
    )?;
    let explicit_hole_outputs = primary_hole_outputs(
        ctx,
        simple_hole_templates,
        &body_references,
        body_bindings,
        &bodies_by_object_index,
    )?;
    let simple_hole_operations = simple_hole_operations(
        ctx,
        simple_hole_templates,
        simple_hole_construction_groups,
        &operation_positions,
    )?
    .unwrap_or_default();
    let mut hole_outputs = explicit_hole_outputs;
    let mut simple_hole_diameters = BTreeMap::new();
    admit_hole_recognition_tolerances(ir)?;
    if let Some(projection) = hole_body_projection(ctx, ir, &simple_hole_operations, &hole_outputs)?
    {
        extend_hole_projection_map(
            ctx,
            &mut hole_outputs,
            projection.outputs,
            "NX simple hole output merge",
        )?;
        extend_hole_projection_map(
            ctx,
            &mut simple_hole_diameters,
            projection.diameters,
            "NX simple hole diameter merge",
        )?;
    }
    let counterbore_operations =
        counterbore_operations(ctx, simple_hole_templates, &operation_positions)?
            .unwrap_or_default();
    let mut counterbore_dimensions = BTreeMap::new();
    if let Some(projection) =
        counterbore_body_projection(ctx, ir, &counterbore_operations, &hole_outputs)?
    {
        extend_hole_projection_map(
            ctx,
            &mut hole_outputs,
            projection.outputs,
            "NX counterbore output merge",
        )?;
        extend_hole_projection_map(
            ctx,
            &mut simple_hole_diameters,
            projection.diameters,
            "NX counterbore diameter merge",
        )?;
        extend_hole_projection_map(
            ctx,
            &mut counterbore_dimensions,
            projection.counterbores,
            "NX counterbore dimension merge",
        )?;
    }
    let blind_hole_operations =
        blind_hole_operations(ctx, simple_hole_templates, &operation_positions)?
            .unwrap_or_default();
    let mut blind_hole_depths = BTreeMap::new();
    if let Some(projection) =
        blind_hole_body_projection(ctx, ir, &blind_hole_operations, &hole_outputs)?
    {
        extend_hole_projection_map(
            ctx,
            &mut hole_outputs,
            projection.outputs,
            "NX blind hole output merge",
        )?;
        extend_hole_projection_map(
            ctx,
            &mut simple_hole_diameters,
            projection.diameters,
            "NX blind hole diameter merge",
        )?;
        blind_hole_depths = projection.blind_depths;
    }
    let simple_hole_placements =
        hole_axis_placements_for_operations(ctx, ir, &simple_hole_operations, &hole_outputs)?;
    let counterbore_hole_placements = counterbore_axis_placements_for_operations(
        ctx,
        ir,
        &counterbore_operations,
        &hole_outputs,
    )?;
    let blind_hole_placements =
        blind_hole_axis_placements_for_operations(ctx, ir, &blind_hole_operations, &hole_outputs)?;
    let simple_hole_chamfers = simple_hole_chamfers(ctx, ir, simple_hole_templates, &hole_outputs)?;
    let hole_packages = hole_package_projection(
        ctx,
        ir,
        simple_hole_templates,
        simple_hole_construction_groups,
        hole_package_construction_group_uses,
        HolePackageSources {
            outputs: &hole_outputs,
            diameters: &simple_hole_diameters,
            chamfers: &simple_hole_chamfers,
        },
    )?;
    let mut feature_ids_by_operation = BTreeMap::new();
    let mut feature_id_reservation = ctx.reserve_scoped(0, "NX operation feature identities")?;
    for label in labels {
        const PREFIX: &str = "nx:feature-history:feature#";
        ctx.charge_work(1, "NX operation feature identity scan")?;
        if !projects_neutral_feature(&label.value)
            || hole_packages.internal_operations.contains(&label.id)
        {
            continue;
        }
        let key = label
            .id
            .strip_prefix("nx:feature-history:operation-label#")
            .unwrap_or(label.id.as_str());
        if key.is_empty() || key.contains('#') || key.chars().any(char::is_whitespace) {
            continue;
        }
        let id_len = PREFIX.len().checked_add(key.len()).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX operation feature identity",
                0,
                cadmpeg_core::decode::u64_from_index(key.len()),
            )
        })?;
        let mut id_text = String::new();
        ctx.charge_work(1, "NX operation feature identity index")?;

        feature_id_reservation.with_storage(|| {
            ctx.try_reserve_retained_text(&mut id_text, id_len, "NX operation feature identity")
        })?;
        id_text.push_str(PREFIX);
        id_text.push_str(key);
        let Ok(id) = FeatureId::mint(id_text) else {
            continue;
        };
        feature_id_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut feature_ids_by_operation,
                label.id.as_str(),
                id,
                "NX operation feature identity index",
            )
        })?;
    }
    let mut parameter_bindings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterBinding>>::new();
    for binding in parameter_bindings {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut parameter_bindings_by_operation,
            binding.operation_label.as_str(),
            || binding,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut parameter_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut parameter_uses_by_operation,
            parameter_use.operation_label.as_str(),
            || parameter_use,
            0,
            "NX feature operation group index",
        )?;
    }
    let (operation_labels_by_record, _operation_labels_by_record_reservation) = ctx
        .collect_scoped_btree_map(
            operation_records
                .iter()
                .map(|record| (record.id.as_str(), record.operation_label.as_str())),
            "NX last-record index",
        )?;
    let mut body_writes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyWrite>>::new();
    for (write, operation_label) in operation_body_writes
        .iter()
        .filter_map(|write| write.operation_label.as_deref().map(|label| (write, label)))
    {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut body_writes_by_operation,
            operation_label,
            || write,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut body_identity_writer_storage = ctx.reserve_scoped(0, "NX body identity writer")?;
    let mut body_identity_writers = BTreeMap::<u8, FeatureId>::new();
    let mut payload_strings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeaturePayloadString>>::new();
    for value in payload_strings {
        let Some(operation) = operation_labels_by_record.get(value.operation_record.as_str())
        else {
            continue;
        };
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut payload_strings_by_operation,
            *operation,
            || value,
            0,
            "NX feature operation group index",
        )?;
    }
    let mut parameter_owners = BTreeMap::new();
    let mut parameter_owner_reservation = ctx.reserve_scoped(0, "NX parameter owner index")?;
    for parameter in &ir.model.parameters {
        ctx.charge_work(1, "NX parameter owner index")?;

        let key = parameter_owner_reservation.with_storage(|| {
            parameter
                .id
                .try_clone_for_decode(ctx, "NX parameter owner index")
        })?;
        let owner = parameter_owner_reservation.with_storage(|| {
            parameter
                .owner
                .as_ref()
                .map(|owner| owner.try_clone_for_decode(ctx, "NX parameter owner index"))
                .transpose()
        })?;
        parameter_owner_reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut parameter_owners,
                key,
                owner,
                "NX parameter owner index",
            )
        })?;
    }
    let annotation_base_order = id_from_index(ir.model.semantic_annotations.len());
    for (annotation_ordinal, label) in labels
        .iter()
        .filter(|label| label.value == "TEXT")
        .enumerate()
    {
        let payload_strings = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let order = annotation_base_order.and_then(|base| {
            id_from_index(annotation_ordinal).and_then(|ordinal| base.checked_add(ordinal))
        });
        let Some(order) = order else {
            const PREFIX: &str = "NX TEXT label ";
            const SUFFIX: &str = " lies past the stated semantic-annotation order width, so it states no order and is not projected.";
            let message_len = PREFIX
                .len()
                .checked_add(label.id.len())
                .and_then(|bytes| bytes.checked_add(SUFFIX.len()))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX TEXT annotation order loss",
                        0,
                        cadmpeg_core::decode::u64_from_index(label.id.len()),
                    )
                })?;
            ctx.reserve_vec(losses, 1, "NX TEXT annotation losses")?;
            let mut message = String::new();
            ctx.try_reserve_retained_text(
                &mut message,
                message_len,
                "allocate NX TEXT annotation order loss",
            )?;
            message.push_str(PREFIX);
            message.push_str(&label.id);
            message.push_str(SUFFIX);
            losses.push(NxLossCode::SemanticAnnotationOrderUnstatable.note(message));
            continue;
        };
        let [text, font_family] = payload_strings else {
            continue;
        };
        let Some(annotation) = text_semantic_annotation(
            ctx,
            &label.id,
            order,
            &[text.value.as_str(), font_family.value.as_str()],
        )?
        else {
            continue;
        };
        annotations.note(
            ctx,
            annotation.id.as_str(),
            &stream,
            label.source_offset,
            Some("TEXT_SEMANTIC_ANNOTATION"),
        )?;
        annotations.exactness(ctx, annotation.id.as_str(), Exactness::Derived)?;
        ctx.reserve_capacity(
            &mut ir.model.semantic_annotations,
            1,
            "allocate NX semantic annotations",
        )?;
        ir.model.semantic_annotations.push(annotation);
    }
    for (ordinal, label) in chronological_labels.into_iter().enumerate() {
        if !projects_neutral_feature(&label.value)
            || hole_packages.internal_operations.contains(&label.id)
        {
            continue;
        }
        let Some(source_id) = feature_ids_by_operation.get(label.id.as_str()) else {
            continue;
        };
        let mut feature_copy_storage = ctx.reserve_scoped(0, "NX current feature identity")?;
        let id = feature_copy_storage
            .with_storage(|| source_id.try_clone_for_decode(ctx, "NX current feature identity"))?;
        let boolean_offset_store_resolution = booleans
            .get(label.id.as_str())
            .map(|operation| {
                crate::native::segments::boolean_offset_store_resolution(
                    ctx,
                    operation,
                    data_blocks,
                )
            })
            .transpose()?;
        let boolean_definition = booleans
            .get(label.id.as_str())
            .zip(boolean_offset_store_resolution.as_ref())
            .map(|(operation, resolution)| {
                boolean_feature_definition(
                    ctx,
                    operation,
                    &body_alias_roots,
                    resolution,
                    &bodies_by_object_index,
                )
            })
            .transpose()?;
        let mut dependencies = Vec::new();
        let retained_operation_body_writes = body_writes_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let operation_body_writes = if body_writes_match_boolean_target(
            retained_operation_body_writes,
            booleans.get(label.id.as_str()).copied(),
        ) {
            retained_operation_body_writes
        } else {
            &[]
        };
        for write in operation_body_writes {
            if let Some(writer) = body_identity_writers.get(&write.frame.body_identity()) {
                push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
            }
        }
        if let (
            Some(operation),
            Some(resolution),
            Some(FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })),
        ) = (
            booleans.get(label.id.as_str()),
            boolean_offset_store_resolution.as_ref(),
            boolean_definition.as_ref(),
        ) {
            let target = operands.target();
            let tools = operands.tools();
            if !matches!(resolution, BooleanOffsetStoreResolution::Unresolved) {
                let offset_store_body_blocks = match resolution {
                    BooleanOffsetStoreResolution::Complete(blocks) => Some(blocks),
                    BooleanOffsetStoreResolution::None
                    | BooleanOffsetStoreResolution::Unresolved => None,
                };
                if let Some(writer) = boolean_participant_writer(
                    target,
                    operation.target.token.value(),
                    offset_store_body_blocks,
                    &body_alias_roots,
                    &body_writer_history,
                ) {
                    push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
                }
                for body in &operation.tools {
                    if let Some(writer) = boolean_participant_writer(
                        tools,
                        body.token.value(),
                        offset_store_body_blocks,
                        &body_alias_roots,
                        &body_writer_history,
                    ) {
                        push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
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
                body_writer_history.native_writer(canonical_body(operand.operand.atom.value()))
            {
                push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
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
            push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
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
            push_unique_feature_dependency(ctx, &mut dependencies, dependency)?;
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
            push_unique_feature_dependency(ctx, &mut dependencies, dependency)?;
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
            push_unique_feature_dependency(ctx, &mut dependencies, dependency)?;
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            if let Some(feature) =
                feature_ids_by_operation.get(dependency.sketch_operation_label.as_str())
            {
                push_unique_feature_dependency(ctx, &mut dependencies, feature)?;
            }
        }
        let mut source_properties = BTreeMap::new();
        for write in retained_operation_body_writes {
            let ordinal = write.ordinal;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("body_write.{ordinal}"),
                format_args!("{}", write.id),
            )?;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("body_write.{ordinal}.body_identity"),
                format_args!("{}", write.frame.body_identity()),
            )?;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("body_write.{ordinal}.group_node"),
                format_args!("{}", write.frame.group_node().value()),
            )?;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("body_write.{ordinal}.endpoint_tag"),
                format_args!("{}", write.frame.endpoint_tag().code()),
            )?;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("body_write.{ordinal}.body_image_object_index"),
                format_args!("{}", write.frame.body_image().value()),
            )?;
            if let Some(use_) = operation_body_image_segment_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.body_image_segment_use"),
                    format_args!("{}", use_.id),
                )?;
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.segment_body_binding"),
                    format_args!("{}", use_.segment_body_binding),
                )?;
            }
            if let Some(use_) = operation_body_identity_segment_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.body_identity_segment_use"),
                    format_args!("{}", use_.id),
                )?;
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.body_identity_segment_binding"),
                    format_args!("{}", use_.segment_body_binding),
                )?;
            }
            if let Some(use_) = operation_body_partition_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.partition_use"),
                    format_args!("{}", use_.id),
                )?;
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.partition_stream_ordinal"),
                    format_args!("{}", use_.partition_stream_ordinal),
                )?;
                for (group_ordinal, group) in use_.parasolid_group_records.iter().enumerate() {
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("body_write.{ordinal}.parasolid_group.{group_ordinal}"),
                        format_args!("{group}"),
                    )?;
                }
                for (member_ordinal, member) in use_.parasolid_group_members.iter().enumerate() {
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!(
                            "body_write.{ordinal}.parasolid_group_member.{member_ordinal}"
                        ),
                        format_args!("{member}"),
                    )?;
                }
            }
            if let Some(use_) = body_write_group_partition_uses
                .iter()
                .find(|use_| use_.body_write == write.id)
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.group_partition_use"),
                    format_args!("{}", use_.id),
                )?;
            }
        }
        operation_source_properties(
            ctx,
            &mut source_properties,
            &label.id,
            operation_records,
            operation_common_frames,
            operation_terminal_frames,
        )?;
        if let Some(stable_identity) = &label.stable_identity {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("operation_stable_identity"),
                format_args!("{stable_identity}"),
            )?;
        }
        for (use_ordinal, block_use) in datum_csys_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_block_use.{use_ordinal}"),
                format_args!("{}", block_use.id),
            )?;
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_point_dependency_use"),
                format_args!("{}", dependency.sketch_point_use),
            )?;
            match &dependency.block_relation {
                crate::native::features::FeatureSketchDatumCsysBlockRelation::Shared {
                    data_block,
                } => {
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("sketch_point_dependency_shared_block"),
                        format_args!("{data_block}"),
                    )?;
                }
                crate::native::features::FeatureSketchDatumCsysBlockRelation::Consecutive {
                    point_data_block,
                    construction_data_block,
                } => {
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("sketch_point_dependency_point_block"),
                        format_args!("{point_data_block}"),
                    )?;
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("sketch_point_dependency_construction_block"),
                        format_args!("{construction_data_block}"),
                    )?;
                }
            }
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_datum_csys_dependency"),
                format_args!("{}", dependency.id),
            )?;
            for (alias_ordinal, alias) in dependency.scalar_aliases.iter().enumerate() {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_point_dependency_scalar.{alias_ordinal}"),
                    format_args!("{}", alias.datum_csys_scalar),
                )?;
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_point_dependency_coordinate.{alias_ordinal}"),
                    format_args!("{}", alias.sketch_coordinate_ordinal),
                )?;
            }
        }
        let deletes_body = label.value == "DELETE";
        let mut outputs = if deletes_body {
            Vec::new()
        } else {
            match body_references.get(label.id.as_str()) {
                Some(body) => {
                    feature_body_outputs(ctx, *body, body_bindings, &bodies_by_object_index)?
                }
                None => Vec::new(),
            }
        };
        if !deletes_body && outputs.is_empty() && !operation_body_writes.is_empty() {
            outputs = complete_operation_body_image_outputs(
                ctx,
                operation_body_writes,
                &body_image_outputs_by_write,
            )?;
        }
        if outputs.is_empty() {
            if let Some(bodies) = hole_outputs
                .get(label.id.as_str())
                .or_else(|| hole_packages.outputs.get(label.id.as_str()))
            {
                outputs = ctx.collection_vec(bodies.len(), "NX feature output bodies")?;
                for body in bodies {
                    outputs.push(body.try_clone_for_decode(ctx, "NX feature output body")?);
                }
            }
        }
        if outputs.is_empty() {
            if let Some(body) = boolean_target_output(boolean_definition.as_ref()) {
                outputs = ctx.collection_vec(1, "NX feature output bodies")?;
                outputs.push(body.try_clone_for_decode(ctx, "NX feature output body")?);
            }
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
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("{NATIVE_PRIMARY_BODY_OBJECT_INDEX}"),
                format_args!("{body}"),
            )?;
        }
        if let Some(reference) = body_writer_references_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("primary_body_reference"),
                format_args!("{}", reference.id),
            )?;
            if let Some(uses) = body_segment_uses_by_reference.get(reference.id.as_str()) {
                if let [use_] = uses.as_slice() {
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("primary_body_segment_use"),
                        format_args!("{}", use_.id),
                    )?;
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("primary_body_segment_binding"),
                        format_args!("{}", use_.segment_body_binding),
                    )?;
                }
            }
            if let Some(uses) = body_data_block_uses_by_reference.get(reference.id.as_str()) {
                if let [use_] = uses.as_slice() {
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("primary_body_data_block_use"),
                        format_args!("{}", use_.id),
                    )?;
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("primary_body_data_block"),
                        format_args!("{}", use_.data_block),
                    )?;
                }
            }
        }
        for (reference, ordinal) in body_reference_occurrences_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .filter_map(|reference| reference.ordinal.map(|ordinal| (reference, ordinal)))
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("body_reference.{ordinal}"),
                format_args!("{}", reference.body.value()),
            )?;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("body_reference_occurrence.{ordinal}"),
                format_args!("{}", reference.id),
            )?;
        }
        if let Some(inputs) = sketch_construction_inputs_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_construction_inputs"),
                format_args!("{}", inputs.id),
            )?;
        }
        for (ordinal, record) in sketch_records_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_record.{ordinal}"),
                format_args!("{}", record.id),
            )?;
        }
        for (ordinal, payload) in sketch_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_construction_payload.{ordinal}"),
                format_args!("{}", payload.id),
            )?;
        }
        for pair in sketch_coordinate_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_coordinate_pair.{}", pair.ordinal),
                format_args!("{}", pair.id),
            )?;
        }
        for pair in sketch_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_fixed_pair.{}", pair.ordinal),
                format_args!("{}", pair.id),
            )?;
        }
        for pair in sketch_mixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_mixed_pair.{}", pair.ordinal),
                format_args!("{}", pair.id),
            )?;
        }
        for lane in sketch_payload_scalar_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_scalar_lane.{}", lane.ordinal),
                format_args!("{}", lane.id),
            )?;
        }
        for (ordinal, point) in sketch_fixed_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_fixed_point.{ordinal}"),
                format_args!("{}", point.id),
            )?;
        }
        if let Some(construction) = block_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("block_construction"),
                format_args!("{}", construction.id),
            )?;
        }
        for (ordinal, payload) in block_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("block_construction_payload.{ordinal}"),
                format_args!("{}", payload.id),
            )?;
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("block_dimensions"),
                format_args!("{}", dimensions.id),
            )?;
            for (dimension_ordinal, dimension) in dimensions.dimensions.iter().enumerate() {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("block_dimension_declaration.{dimension_ordinal}"),
                    format_args!("{}", dimension.declaration),
                )?;
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("block_dimension_expression.{dimension_ordinal}"),
                    format_args!("{}", dimension.expression),
                )?;
            }
        }
        for (ordinal, point) in block_payload_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("block_payload_point.{ordinal}"),
                format_args!("{}", point.id),
            )?;
        }
        for (ordinal, group) in block_payload_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("block_payload_point_group.{ordinal}"),
                format_args!("{}", group.id),
            )?;
        }
        if let Some(construction) = extrude_32_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("extrude_32_construction"),
                format_args!("{}", construction.id),
            )?;
        }
        if let Some(header) = extrude_payload_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("extrude_payload_header"),
                format_args!("{}", header.id),
            )?;
        }
        if let Some(lane) = operation_terminal_discriminators_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("operation_terminal_discriminator"),
                format_args!("{}", lane.id),
            )?;
        }
        for (ordinal, branch) in extrude_payload_32_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("extrude_payload_32_branch.{ordinal}"),
                format_args!("{}", branch.id),
            )?;
        }
        for triple in operation_body_scalar_triples_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!(
                    "operation_body_scalar_triple.{}",
                    triple.body_reference_ordinal
                ),
                format_args!("{}", triple.id),
            )?;
        }
        for member in operation_body_members_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!(
                    "operation_body_member.{}.{}",
                    member.body_reference_ordinal, member.ordinal
                ),
                format_args!("{}", member.id),
            )?;
        }
        for continuation in operation_body_11_continuations_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!(
                    "operation_body_11_continuation.{}",
                    continuation.body_reference_ordinal
                ),
                format_args!("{}", continuation.id),
            )?;
        }
        for lane in operation_body_reference_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!(
                    "operation_body_reference_lane.{}",
                    lane.body_reference_ordinal
                ),
                format_args!("{}", lane.id),
            )?;
        }
        if let Some(construction) = datum_csys_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_construction"),
                format_args!("{}", construction.id),
            )?;
        }
        for (ordinal, use_) in datum_csys_column_row_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_column_row_use.{ordinal}"),
                format_args!("{}", use_.id),
            )?;
        }
        for (ordinal, payload) in datum_csys_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_payload.{ordinal}"),
                format_args!("{}", payload.id),
            )?;
        }
        for (ordinal, pair) in datum_csys_payload_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_payload_scalar_pair.{ordinal}"),
                format_args!("{}", pair.id),
            )?;
        }
        for (ordinal, pair) in datum_csys_payload_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_payload_fixed_pair.{ordinal}"),
                format_args!("{}", pair.id),
            )?;
        }
        for (ordinal, scalar) in datum_csys_payload_scalars_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_payload_scalar.{ordinal}"),
                format_args!("{}", scalar.id),
            )?;
        }
        for (ordinal, descriptor) in datum_csys_descriptors_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_descriptor.{ordinal}"),
                format_args!("{}", descriptor.id),
            )?;
        }
        if let Some(header) = datum_plane_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_plane_header"),
                format_args!("{}", header.id),
            )?;
        }
        if let Some(payload) = datum_plane_payloads_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_plane_payload"),
                format_args!("{}", payload.id),
            )?;
        }
        for (ordinal, pair) in datum_plane_payload_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_plane_payload_scalar_pair.{ordinal}"),
                format_args!("{}", pair.id),
            )?;
        }
        for (ordinal, descriptor) in datum_plane_descriptors_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_plane_descriptor.{ordinal}"),
                format_args!("{}", descriptor.id),
            )?;
        }
        for (ordinal, identity_use) in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_identity_use.{ordinal}"),
                format_args!("{}", identity_use.id),
            )?;
        }
        for (use_ordinal, block_use) in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_plane_block_use.{use_ordinal}"),
                format_args!("{}", block_use.id),
            )?;
        }
        simple_hole_native_properties(
            ctx,
            &mut source_properties,
            &label.id,
            simple_hole_templates,
            simple_hole_repeated_scalar_lanes,
            simple_hole_repeated_scalar_lane_block_references,
            simple_hole_construction_groups,
        )?;
        for lane in hole_package_construction_group_lanes
            .iter()
            .filter(|lane| lane.operation_label == label.id)
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("hole_package_construction_group_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        for group_use in hole_package_construction_group_uses {
            let group = simple_hole_construction_groups
                .iter()
                .find(|group| group.id == group_use.simple_hole_construction_group);
            if group_use.operation_label == label.id {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("hole_package_construction_group_use"),
                    format_args!("{}", group_use.id),
                )?;
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("simple_hole_construction_group"),
                    format_args!("{}", group_use.simple_hole_construction_group),
                )?;
            } else if group.is_some_and(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.operation_label == label.id)
            }) {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("hole_package_construction_group_use"),
                    format_args!("{}", group_use.id),
                )?;
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("hole_package_operation"),
                    format_args!("{}", group_use.operation_label),
                )?;
            }
        }
        for (slot, value) in label.objects.0.iter().enumerate() {
            if let Some(value) = value {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("object_index.{slot}"),
                    format_args!("{}", value.value()),
                )?;
            } else {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("object_index.{slot}"),
                    format_args!("null"),
                )?;
            }
        }
        for input in input_blocks_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("input_block_record.{}", input.input_slot),
                format_args!("{}", input.id),
            )?;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("input_block.{}", input.input_slot),
                format_args!("{}", input.data_block),
            )?;
            if let Some(group) = input_block_identity_group_by_input.get(input.id.as_str()) {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("input_block_identity_group.{}", input.input_slot),
                    format_args!("{}", (*group)),
                )?;
            }
        }
        for (ordinal, use_) in input_column_row_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("input_column_row_use.{ordinal}"),
                format_args!("{}", use_.id),
            )?;
        }
        for (ordinal, target) in input_column_targets_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("input_column_target.{ordinal}"),
                format_args!("{}", target.id),
            )?;
        }
        for reference in sketch_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_reference_record.{}", reference.position.ordinal()),
                format_args!("{}", reference.id),
            )?;
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!("sketch_reference.{}", reference.position.ordinal()),
                reference.data_block.as_deref(),
                reference.token.value(),
            )?;
        }
        for reference in projected_curve_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("projected_curve_reference_record.{}", reference.ordinal),
                format_args!("{}", reference.id),
            )?;
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!("projected_curve_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            )?;
        }
        for payload in projected_curve_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("projected_curve_construction_payload"),
                format_args!("{}", payload.id),
            )?;
        }
        for value in projected_curve_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("projected_curve_construction_string.{}", value.ordinal),
                format_args!("{}", value.id),
            )?;
        }
        for graph in fset_reference_graphs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("fset_reference_graph"),
                format_args!("{}", graph.id),
            )?;
        }
        for payload in fset_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            let crate::native::features::FeatureConstructionOwner::Fset { group, .. } =
                &payload.owner
            else {
                continue;
            };
            let group = match group {
                crate::native::features::fset::FeatureFsetReferenceGroup::First => "first",
                crate::native::features::fset::FeatureFsetReferenceGroup::Second => "second",
            };
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("fset_construction_payload.{group}"),
                format_args!("{}", payload.id),
            )?;
        }
        for field in delete_reference_fields_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("delete_reference_field"),
                format_args!("{}", field.id),
            )?;
        }
        for payload in delete_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("delete_construction_payload"),
                format_args!("{}", payload.id),
            )?;
        }
        for reference in pattern_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("pattern_reference_record.{}", reference.ordinal),
                format_args!("{}", reference.id),
            )?;
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!("pattern_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            )?;
        }
        for payload in pattern_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("pattern_construction_payload"),
                format_args!("{}", payload.id),
            )?;
        }
        for lane in pattern_counted_reference_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("pattern_counted_reference_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        for value in pattern_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("pattern_construction_string.{}", value.ordinal),
                format_args!("{}", value.id),
            )?;
        }
        for lane in pattern_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("pattern_construction_fixed_lane.{}", lane.ordinal),
                format_args!("{}", lane.id),
            )?;
        }
        for lane in pattern_transform_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("pattern_transform_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        for lane in multi_instance_output_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("multi_instance_output_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        for lane in identical_instance_output_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("identical_instance_output_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        if let Some(header) = point_construction_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("point_construction_header"),
                format_args!("{}", header.id),
            )?;
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!("point_construction_reference"),
                header.data_block.as_deref(),
                header.token.value(),
            )?;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("point_construction_mode"),
                format_args!("{:02x}", u8::from(header.mode)),
            )?;
        }
        if let Some(lane) = point_construction_scalar_lanes_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("point_construction_scalar_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        for reference in draft_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_reference_record.{}", reference.ordinal),
                format_args!("{}", reference.id),
            )?;
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            )?;
        }
        for lane in draft_construction_index_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_index_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        for payload in draft_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_payload"),
                format_args!("{}", payload.id),
            )?;
        }
        for payload in draft_construction_graph_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_graph_payload"),
                format_args!("{}", payload.id),
            )?;
        }
        for lane in draft_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_fixed_lane.{}", lane.ordinal),
                format_args!("{}", lane.id),
            )?;
        }
        for lane in draft_construction_binary32_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_binary32_lane.{}", lane.ordinal),
                format_args!("{}", lane.id),
            )?;
        }
        for value in draft_construction_graph_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_graph_string.{}", value.ordinal),
                format_args!("{}", value.id),
            )?;
        }
        for frame in draft_construction_identity_frames_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_identity_frame.{}", frame.ordinal),
                format_args!("{}", frame.id),
            )?;
        }
        for lane in draft_construction_terminal_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("draft_construction_terminal_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        for reference in surface_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!(
                    "surface_construction_reference_record.{}",
                    reference.ordinal
                ),
                format_args!("{}", reference.id),
            )?;
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!("surface_construction_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            )?;
        }
        for payload in surface_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("surface_construction_payload"),
                format_args!("{}", payload.id),
            )?;
        }
        for pair in surface_construction_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("surface_construction_scalar_pair.{}", pair.ordinal),
                format_args!("{}", pair.id),
            )?;
        }
        for value in surface_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("surface_construction_string.{}", value.ordinal),
                format_args!("{}", value.id),
            )?;
        }
        for branch in surface_construction_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            for (ordinal, (token, data_block)) in
                branch.references.members().as_slice().iter().enumerate()
            {
                insert_source_property_reference(
                    ctx,
                    &mut source_properties,
                    format_args!(
                        "surface_construction_branch.{}.member.{}",
                        branch.ordinal(),
                        ordinal
                    ),
                    data_block.as_deref(),
                    token.value(),
                )?;
            }
            let (token, data_block) = branch.references.terminal();
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!("surface_construction_branch.{}.terminal", branch.ordinal()),
                data_block.as_deref(),
                token.value(),
            )?;
        }
        for (ordinal, block_use) in sketch_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_named_point_block_use.{ordinal}"),
                format_args!("{}", block_use.id),
            )?;
        }
        for (ordinal, point_use) in sketch_preceding_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_preceding_named_point_use.{ordinal}"),
                format_args!("{}", point_use.id),
            )?;
        }
        for (ordinal, point_use) in sketch_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_point_use.{ordinal}"),
                format_args!("{}", point_use.id),
            )?;
        }
        for (ordinal, group) in sketch_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_point_group.{ordinal}"),
                format_args!("{}", group.id),
            )?;
        }
        for reference in extrude_profile_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("extrude_profile_reference_record.{}", reference.ordinal),
                format_args!("{}", reference.id),
            )?;
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!("extrude_profile_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            )?;
        }
        if let Some(profile) = extrude_construction_profiles_by_operation.get(label.id.as_str()) {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("extrude_construction_profile"),
                format_args!("{}", profile.id),
            )?;
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property_reference(
                ctx,
                &mut source_properties,
                format_args!(
                    "operation_body_operand.{}.{}",
                    operand.body_reference_ordinal, operand.ordinal
                ),
                operand.operand_data_block.as_deref(),
                operand.operand.atom.value(),
            )?;
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!(
                    "operation_body_operand_record.{}.{}",
                    operand.body_reference_ordinal, operand.ordinal
                ),
                format_args!("{}", operand.id),
            )?;
            for (binding_ordinal, binding) in operand.segment_body_bindings.iter().enumerate() {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!(
                        "operation_body_operand_segment_binding.{}.{}.{}",
                        operand.body_reference_ordinal, operand.ordinal, binding_ordinal
                    ),
                    format_args!("{binding}"),
                )?;
            }
        }
        for binding in parameter_bindings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!(
                    "input_parameter_declaration.{}.{}",
                    binding.input_slot, binding.reference_ordinal
                ),
                format_args!("{}", binding.expression_declaration),
            )?;
            if let Some(expression) = &binding.expression {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!(
                        "input_parameter_expression.{}.{}",
                        binding.input_slot, binding.reference_ordinal
                    ),
                    format_args!("{expression}"),
                )?;
            }
        }
        for (ordinal, parameter_use) in parameter_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("parameter_use.{ordinal}"),
                format_args!("{}", parameter_use.id),
            )?;
        }
        let operation_payload_string_records = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);

        let (mut operation_payload_strings, _payload_reservation) = ctx.temporary_vec(
            operation_payload_string_records.len(),
            "NX operation payload string references",
        )?;
        operation_payload_strings.extend(
            operation_payload_string_records
                .iter()
                .map(|value| value.value.as_str()),
        );
        let block_dimension_values =
            block_dimensions_by_operation
                .get(label.id.as_str())
                .map(|dimensions| {
                    dimensions
                        .dimensions
                        .each_ref()
                        .map(|dimension| dimension.value.get())
                });
        let block_projection = if label.value == "BLOCK" {
            match block_dimension_values {
                Some(dimensions) => block_placement(ctx, ir, dimensions, &outputs)?,
                None => None,
            }
        } else {
            None
        };
        if outputs.is_empty() {
            if let Some((body, _)) = &block_projection {
                ctx.reserve_vec(&mut outputs, 1, "NX block output bodies")?;
                outputs.push(body.try_clone_for_decode(ctx, "NX decoded IR value copy")?);
            }
        }
        let sphere_projection = if label.value == "SPHERE" {
            sphere_body_projection(ctx, ir, &outputs)?
        } else {
            None
        };
        let sphere_outputs = if outputs.is_empty() {
            sphere_projection
                .as_ref()
                .map_or([].as_slice(), |(body, _, _)| std::slice::from_ref(body))
        } else {
            outputs.as_slice()
        };
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
                            matches!(payloads.as_slice(), [payload]
                                if matches!(&payload.owner,
                                    crate::native::features::FeatureConstructionOwner::Block {
                                        construction: owner,
                                    } if owner == &construction.id))
                        })
                }),
            outputs: &outputs,
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
                    body_reference_count,
                    provisional_feature: initial_body_id.as_ref(),
                    native_primary_body,
                    offset_store_primary_body,
                    history: &body_writer_history,
                })
            });
        if sphere_op == BooleanOp::NewBody && outputs.is_empty() {
            if let Some((body, _, _)) = &sphere_projection {
                ctx.reserve_vec(&mut outputs, 1, "NX sphere output bodies")?;
                outputs.push(body.try_clone_for_decode(ctx, "NX decoded IR value copy")?);
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
                    .evaluation
                    .edit(|definition, initial_outputs| {
                        initial_outputs.retain(|body| !outputs.contains(body));
                        if let FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                            bodies: BodySelection::Resolved { bodies, .. },
                        }) = definition
                        {
                            bodies.retain(|body| !outputs.contains(body));
                        }
                    });
                body_writer_history.retract_outputs(&initial_feature.id, &outputs);
            }
        }
        body_writer_history.extend_primary_dependencies(
            ctx,
            initial_body_id.as_ref(),
            native_primary_body,
            offset_store_primary_body,
            &outputs,
            &mut dependencies,
        )?;
        let block_placement = block_projection.map(|(_, placement)| placement);
        let sphere_definition = sphere_projection.as_ref().and_then(|(_, center, radius)| {
            (sphere_op == BooleanOp::NewBody).then_some(FeatureDefinition::Operation(
                FeatureOperation::Sphere {
                    center: *center,
                    radius: *radius,
                    op: sphere_op,
                },
            ))
        });
        let sew_projection = if label.value == "SEW" {
            sew_body_feature_definition(
                ctx,
                body_references.get(label.id.as_str()).copied(),
                offset_store_bodies_by_operation
                    .get(label.id.as_str())
                    .map_or([].as_slice(), Vec::as_slice),
                operation_body_operands_by_operation
                    .get(label.id.as_str())
                    .map_or([].as_slice(), Vec::as_slice),
                &body_alias_roots,
                &bodies_by_object_index,
            )?
        } else {
            None
        };
        let trim_body_projection = if label.value == "TRIM BODY" {
            if let Some(primary) = body_references.get(label.id.as_str()) {
                Some(trim_body_feature_definition(
                    ctx,
                    *primary,
                    operation_body_operands_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    &body_alias_roots,
                    &bodies_by_object_index,
                )?)
            } else {
                offset_store_trim_body_feature_definition(
                    ctx,
                    offset_store_bodies_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    operation_body_operands_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                )?
            }
        } else {
            None
        };
        let offset_projection = if label.value == "OFFSET" {
            offset_surface_feature_definition(ctx, ir, &outputs)?
        } else {
            None
        };
        if let Some((_, supports)) = &offset_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("offset_support_surface.{support_ordinal}"),
                    format_args!("{}", support.as_str()),
                )?;
            }
        }
        let thicken_projection = if label.value == "THICKEN_SHEET" {
            thicken_feature_definition(ctx, ir, &outputs)?
        } else {
            None
        };
        if let Some((_, supports)) = &thicken_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("thicken_support_surface.{support_ordinal}"),
                    format_args!("{}", support.as_str()),
                )?;
            }
        }
        let blend_family = match label.value.as_str() {
            "BLEND" => Some(NxBlendFamily::Edge),
            "FACE_BLEND" => Some(NxBlendFamily::Face),
            _ => None,
        };
        let blend_projection = if let Some(family) = blend_family {
            blend_feature_definition(ctx, ir, &outputs, family)?
        } else {
            None
        };
        if let Some((_, surfaces)) = &blend_projection {
            for (surface_ordinal, surface) in surfaces.iter().enumerate() {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("blend_result_surface.{surface_ordinal}"),
                    format_args!("{}", surface.as_str()),
                )?;
            }
        }
        let extrude_projection = if label.value == "EXTRUDE" {
            let mut output_kinds = Vec::new();
            let mut output_kind_reservation =
                ctx.reserve_scoped(0, "NX extrude output body kinds")?;
            for output in &outputs {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()),
                    "NX extrude output body lookup",
                )?;
                let Some(body) = ir.model.bodies.iter().find(|body| body.id == *output) else {
                    output_kinds.clear();
                    break;
                };
                ctx.reserve_scoped_vec(
                    &mut output_kind_reservation,
                    &mut output_kinds,
                    1,
                    "NX extrude output body kinds",
                )?;
                output_kinds.push(body.kind);
            }
            let op = extrude_boolean_op(
                &body_writer_history,
                native_primary_body,
                offset_store_primary_body,
                &output_kinds,
            );
            let construction_profile = extrude_construction_profiles_by_operation
                .get(label.id.as_str())
                .map(|profile| profile.id.as_str());
            let structured_construction = extrude_32_constructions_by_operation
                .get(label.id.as_str())
                .map(|construction| construction.id.as_str());
            if let (Some(profile), None) | (None, Some(profile)) =
                (construction_profile, structured_construction)
            {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(profile.len()),
                    "NX extrude construction profile",
                )?;
            }
            Some(extrude_feature_definition(
                construction_profile,
                structured_construction,
                op,
                &output_kinds,
            ))
        } else {
            None
        };
        let delete_projection = if deletes_body {
            let field = body_references
                .get(label.id.as_str())
                .copied()
                .map(DeleteBodyField::Native)
                .or_else(|| {
                    offset_store_bodies_by_operation
                        .get(label.id.as_str())
                        .and_then(|uses| match uses.as_slice() {
                            [(object_index, data_block)] => Some(DeleteBodyField::OffsetStore {
                                object_index: *object_index,
                                data_block,
                            }),
                            _ => None,
                        })
                });
            field
                .map(|field| {
                    delete_body_feature_definition(
                        ctx,
                        field,
                        &body_alias_roots,
                        &bodies_by_object_index,
                    )
                })
                .transpose()?
        } else {
            None
        };
        let extract_body_projection = if label.value == "EXTRACT_BODY" {
            Some(extract_body_feature_definition(
                ctx,
                body_references.get(label.id.as_str()).copied(),
                offset_store_bodies_by_operation
                    .get(label.id.as_str())
                    .map_or([].as_slice(), Vec::as_slice),
                &body_alias_roots,
                &bodies_by_object_index,
            )?)
        } else {
            None
        };
        let operation_parameter_uses = parameter_uses_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let native_parameters =
            native_feature_parameters(ctx, operation_parameter_uses, expressions)?;
        let sketch = if label.value == "SKETCH" {
            attach_sketch_graph(
                ctx,
                ir,
                label,
                &SketchSources {
                    point_uses: sketch_point_uses_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    point_groups: sketch_point_groups,
                    points: sketch_points,
                    payload_scalars: sketch_payload_scalars,
                    fixed_points: sketch_fixed_points_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    coordinate_pairs: sketch_coordinate_pairs_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                },
                annotations,
                &stream,
            )?
        } else {
            None
        };
        let primary_definition = boolean_definition
            .or(trim_body_projection)
            .or(delete_projection)
            .or(extract_body_projection)
            .or(sew_projection)
            .or(extrude_projection)
            .or_else(|| blend_projection.map(|(definition, _)| definition))
            .or_else(|| thicken_projection.map(|(definition, _)| definition))
            .or_else(|| offset_projection.map(|(definition, _)| definition))
            .or(sphere_definition);
        let brep_projection = if primary_definition.is_none() && label.value == "BREP" {
            brep_feature_definition(ctx, &outputs)?
        } else {
            None
        };
        let definition = if let Some(definition) = primary_definition.or(brep_projection) {
            definition
        } else if let Some(sketch) = sketch {
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            })
        } else {
            let history_definition = non_modeling_history_definition(
                &label.value,
                &label.objects.values(),
                &outputs,
                body_reference_occurrences_by_operation
                    .get(label.id.as_str())
                    .map_or(0, Vec::len),
                operation_body_operands_by_operation
                    .get(label.id.as_str())
                    .map_or(0, Vec::len),
                operation_payload_string_records.len(),
                &source_properties,
            );
            let fallback_definition = match history_definition {
                Some(definition) => Some(definition),
                None => body_writing_unresolved_feature_definition(
                    ctx,
                    &label.value,
                    &source_properties,
                )?,
            };
            let mut definition = if let Some(definition) = fallback_definition {
                definition
            } else {
                let mut placements = Vec::new();
                for source in [
                    simple_hole_placements
                        .get(label.id.as_str())
                        .map_or([].as_slice(), std::slice::from_ref),
                    counterbore_hole_placements
                        .get(label.id.as_str())
                        .map_or([].as_slice(), std::slice::from_ref),
                    blind_hole_placements
                        .get(label.id.as_str())
                        .map_or([].as_slice(), std::slice::from_ref),
                    hole_packages
                        .placements
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                ] {
                    for placement in source {
                        ctx.reserve_vec(&mut placements, 1, "NX feature hole placements")?;
                        placements
                            .push(placement.try_clone_for_decode(ctx, "NX decoded IR value copy")?);
                    }
                }
                non_boolean_feature_definition_with_parameters(
                    ctx,
                    &label.value,
                    &operation_payload_strings,
                    block_dimension_values,
                    block_placement,
                    HoleProjection {
                        placements,
                        diameter: simple_hole_diameters
                            .get(label.id.as_str())
                            .or_else(|| hole_packages.diameters.get(label.id.as_str()))
                            .copied(),
                        extent: blind_hole_depths
                            .get(label.id.as_str())
                            .copied()
                            .map(|length| LinearTermination::Blind { length }),
                        counterbore: counterbore_dimensions.get(label.id.as_str()).copied(),
                        chamfer: simple_hole_chamfers
                            .get(label.id.as_str())
                            .or_else(|| hole_packages.chamfers.get(label.id.as_str()))
                            .copied(),
                        grouped_simple_through: hole_packages
                            .outputs
                            .contains_key(label.id.as_str()),
                    },
                    cadmpeg_core::text::named_entries_for_decode(
                        ctx,
                        &label.id,
                        native_parameters,
                    )?,
                )?
            };
            if let FeatureDefinition::Operation(FeatureOperation::Block { op, .. }) =
                &mut definition
            {
                *op = block_op;
            }
            definition
        };
        annotations.note(
            ctx,
            &id,
            &stream,
            label.source_offset,
            Some("FEATURE_OPERATION"),
        )?;
        annotations.exactness(ctx, &id, Exactness::Derived)?;
        let source_content = feature_source_content(ctx, operation_payload_string_records)?;
        let mut referenced_parameters = Vec::new();
        let mut parameter_reservation = ctx.reserve_scoped(0, "NX referenced parameters")?;
        for parameter_use in operation_parameter_uses {
            push_referenced_parameter(
                ctx,
                &mut parameter_reservation,
                &mut referenced_parameters,
                &parameter_use.expression,
            )?;
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            for dimension in &dimensions.dimensions {
                push_referenced_parameter(
                    ctx,
                    &mut parameter_reservation,
                    &mut referenced_parameters,
                    &dimension.expression,
                )?;
            }
        }
        for owner in parameter_owner_dependencies(ctx, &parameter_owners, &referenced_parameters)? {
            push_unique_feature_dependency(ctx, &mut dependencies, &owner)?;
        }
        if !source_content.is_empty() {
            annotations
                .derived(ctx, &id, "source_content")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        let native_output = (!deletes_body).then_some(native_primary_body).flatten();
        let offset_store_output = (!deletes_body)
            .then_some(offset_store_primary_body)
            .flatten();
        body_writer_history.record_writer(
            ctx,
            native_output,
            offset_store_output,
            &outputs,
            &id,
        )?;
        for write in operation_body_writes {
            let writer = body_identity_writer_storage
                .with_storage(|| id.try_clone_for_decode(ctx, "NX body identity writer"))?;
            body_identity_writer_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut body_identity_writers,
                    write.frame.body_identity(),
                    writer,
                    "NX body identity writers",
                )
            })?;
        }
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
                    canonical_body(operation.target.token.value()),
                );
                body_writer_history.record_writer(
                    ctx,
                    native_target,
                    offset_store_target,
                    &[],
                    &id,
                )?;
            }
        }
        let dependency_check_work = dependencies
            .len()
            .checked_mul(dependencies.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX feature dependency validation",
                    0,
                    cadmpeg_core::decode::u64_from_index(dependencies.len()),
                )
            })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(dependency_check_work),
            "NX feature dependency validation",
        )?;
        let feature_text_bytes = label
            .value
            .len()
            .checked_add(label.value.len())
            .and_then(|bytes| bytes.checked_add(label.id.len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX feature record",
                    0,
                    cadmpeg_core::decode::u64_from_index(label.id.len()),
                )
            })?;
        ctx.charge_collection_items(1, "NX feature records")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(feature_text_bytes),
            "NX feature record",
        )?;
        ctx.reserve_capacity(&mut ir.model.features, 1, "allocate NX feature records")?;
        ir.model.features.push(Feature {
            id: id.try_clone_for_decode(ctx, "NX decoded IR value copy")?,
            ordinal: base_ordinal + cadmpeg_core::decode::u64_from_index(ordinal),
            name: Some(label.value.clone()),
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies, ctx)
                .map_err(cadmpeg_core::CodecError::from)?,
            source_properties: cadmpeg_core::text::named_entries_for_decode(
                ctx,
                &label.id,
                source_properties,
            )?,
            source_tag: Some(label.value.clone()),
            source_text: None,
            source_content,

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                definition,
                DistinctMembers::try_from(outputs, ctx).map_err(CodecError::from)?,
            ),
            native_ref: Some(label.id.clone()),
        });
        if !deletes_body && !operation_body_writes.is_empty() {
            let key = label
                .id
                .strip_prefix("nx:feature-history:operation-label#")
                .unwrap_or(label.id.as_str());
            for write in operation_body_writes {
                const BODY_PREFIX: &str = "nx:feature-history:body-identity#";
                let result_members = operation_body_write_result_group_members(
                    ctx,
                    write.id.as_str(),
                    operation_body_partition_uses,
                    body_write_group_partition_uses,
                    parasolid_group_members,
                )?;
                let body_len = BODY_PREFIX.len() + 10;
                ctx.charge_collection_items(1, "NX feature result bodies")?;

                let mut body_text = String::new();
                ctx.try_reserve_retained_text(&mut body_text, body_len, "NX feature result body")?;
                std::fmt::Write::write_fmt(
                    &mut body_text,
                    format_args!("{BODY_PREFIX}{:010}", write.frame.body_identity()),
                )
                .map_err(|_| {
                    CodecError::malformed("NX feature result body identity formatting failed")
                })?;
                let body = cadmpeg_core::text::NonBlankString::new(body_text).ok_or_else(|| {
                    CodecError::malformed("NX feature result body identity is blank")
                })?;
                let mut bodies = Vec::new();
                ctx.reserve_capacity(&mut bodies, 1, "allocate NX feature result bodies")?;
                bodies.push(body);
                let mut native_ref =
                    ctx.retained_string(write.id.len(), "NX result topology native reference")?;
                native_ref.push_str(&write.id);
                append_feature_result_topology(
                    ctx,
                    ir,
                    result_topology_id(ctx, key, Some(write.ordinal))?,
                    &id,
                    bodies,
                    result_members,
                    native_ref,
                )?;
            }
        } else if !deletes_body {
            let result_body = native_result_body_identity(
                ctx,
                body_writer_references_by_operation
                    .get(label.id.as_str())
                    .copied(),
                booleans.get(label.id.as_str()).copied(),
            )?;
            if let Some((local_id, native_ref)) = result_body {
                let key = label
                    .id
                    .strip_prefix("nx:feature-history:operation-label#")
                    .unwrap_or(label.id.as_str());
                let mut bodies = ctx.collection_vec(1, "NX feature result bodies")?;
                bodies.push(local_id);
                append_feature_result_topology(
                    ctx,
                    ir,
                    result_topology_id(ctx, key, None)?,
                    &id,
                    bodies,
                    FeatureResultGroupMembers::default(),
                    native_ref,
                )?;
            }
        }
    }
    // Only namespace-admitted primary-body relations can open the state-only
    // witness. Unique fields rejected by native_primary_body_references remain
    // native evidence without becoming current-body state roots.
    if !body_references.is_empty() {
        if let Some(initial_body_id) = initial_body_id.as_ref() {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(ir.model.features.len()),
                "NX primary body closure feature lookup",
            )?;
            if let Some(initial_feature) = ir
                .model
                .features
                .iter_mut()
                .find(|feature| feature.id == *initial_body_id)
            {
                const WITNESS_VALUE: &str = "primary-body-relations";

                let mut key = String::new();
                ctx.try_reserve_retained_text(
                    &mut key,
                    NATIVE_PRIMARY_BODY_CLOSURE_WITNESS.len(),
                    "allocate NX primary body closure witness key",
                )?;
                key.push_str(NATIVE_PRIMARY_BODY_CLOSURE_WITNESS);
                let mut value = String::new();
                ctx.try_reserve_retained_text(
                    &mut value,
                    WITNESS_VALUE.len(),
                    "allocate NX primary body closure witness value",
                )?;
                value.push_str(WITNESS_VALUE);
                ctx.insert_btree_map(
                    &mut initial_feature.source_properties,
                    cadmpeg_core::text::NonBlankString::new(key).ok_or_else(|| {
                        CodecError::malformed("NX primary body closure witness key is blank")
                    })?,
                    value,
                    "NX primary body closure witness",
                )?;
                annotations
                    .derived(ctx, initial_body_id, NATIVE_PRIMARY_BODY_CLOSURE_WITNESS)
                    .map_err(cadmpeg_core::CodecError::from)?;
            }
        }
    }
    if let Some(initial_body_id) = initial_body_id {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.features.len()),
            "NX initial body output lookup",
        )?;
        let has_outputs = ir
            .model
            .features
            .iter()
            .find(|feature| feature.id == initial_body_id)
            .is_some_and(|feature| !feature.evaluation.outputs().is_empty());
        if has_outputs {
            annotations
                .derived(ctx, &initial_body_id, "outputs")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    Ok(())
}

#[derive(Clone, Default, PartialEq, Eq)]
struct FeatureResultGroupMembers {
    faces: Vec<cadmpeg_core::text::NonBlankString>,
    edges: Vec<cadmpeg_core::text::NonBlankString>,
    vertices: Vec<cadmpeg_core::text::NonBlankString>,
}

fn result_topology_id(
    ctx: &DecodeContext<'_>,
    key: &str,
    ordinal: Option<u32>,
) -> Result<FeatureResultTopologyId, CodecError> {
    const PREFIX: &str = "nx:feature-history:result-topology#";
    let suffix_len = if ordinal.is_some() { 11 } else { 0 };
    let key_len = key.len().checked_add(suffix_len).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX result topology key",
            0,
            cadmpeg_core::decode::u64_from_index(key.len()),
        )
    })?;
    let id_len = PREFIX.len().checked_add(key_len).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX result topology identity",
            0,
            cadmpeg_core::decode::u64_from_index(key_len),
        )
    })?;
    let mut key_reservation = ctx.reserve_scoped(0, "NX result topology key")?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(key_len),
        "NX result topology key formatting",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(id_len),
        "NX result topology identity",
    )?;
    let mut owned_key = String::new();
    key_reservation.with_storage(|| {
        ctx.try_reserve_retained_text(&mut owned_key, key_len, "allocate NX result topology key")
    })?;
    owned_key.push_str(key);
    if let Some(ordinal) = ordinal {
        std::fmt::Write::write_fmt(&mut owned_key, format_args!("-{ordinal:010}"))
            .map_err(|_| CodecError::malformed("NX result topology key formatting failed"))?;
    }
    IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
        .try_id::<FeatureResultTopologyId>(
            &cadmpeg_ir::identity_component!("result-topology"),
            owned_key,
        )
        .ok_or_else(|| CodecError::malformed("NX operation label key is not identity key text"))
}

fn append_feature_result_topology(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    result_id: FeatureResultTopologyId,
    output_of: &FeatureId,
    bodies: Vec<cadmpeg_core::text::NonBlankString>,
    members: FeatureResultGroupMembers,
    native_ref: String,
) -> Result<(), CodecError> {
    let members = cadmpeg_ir::features::FeatureResultMembers::new(
        bodies,
        members.faces,
        members.edges,
        members.vertices,
        ctx,
        "NX result topology member validation",
    )?
    .map_err(|error| CodecError::Malformed(error.to_string()))?;
    ctx.reserve_vec_limit(
        &mut ir.model.feature_result_topologies,
        1,
        "allocate NX result topology records",
    )?;
    let result = FeatureResultTopology::new(
        result_id,
        output_of.try_clone_for_decode(ctx, "NX decoded IR value copy")?,
        members,
        Some(native_ref),
    );
    ir.model.feature_result_topologies.push(result);
    Ok(())
}

fn operation_body_write_result_group_members(
    ctx: &DecodeContext<'_>,
    body_write: &str,
    image_partition_uses: &[crate::native::features::FeatureOperationBodyPartitionUse],
    group_partition_uses: &[crate::native::features::FeatureBodyWriteGroupPartitionUse],
    members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Result<FeatureResultGroupMembers, CodecError> {
    let mut matching_image_uses = image_partition_uses
        .iter()
        .filter(|use_| use_.operation_body_write == body_write);
    let image_use = matching_image_uses.next();
    if matching_image_uses.next().is_some() {
        return Ok(FeatureResultGroupMembers::default());
    }
    let image_members = image_use
        .map(|use_| {
            feature_result_group_members(
                ctx,
                use_.partition_stream_ordinal,
                &use_.parasolid_group_members,
                members,
            )
        })
        .transpose()?;
    let mut matching_group_uses = group_partition_uses
        .iter()
        .filter(|use_| use_.body_write == body_write);
    let group_use = matching_group_uses.next();
    if matching_group_uses.next().is_some() {
        return Ok(FeatureResultGroupMembers::default());
    }
    let group_members = group_use
        .map(|use_| {
            feature_result_group_members(
                ctx,
                use_.partition_stream_ordinal,
                &use_.parasolid_group_members,
                members,
            )
        })
        .transpose()?;
    Ok(match (image_members, group_members) {
        (Some(image), Some(group)) if image == group => image,
        (Some(_), Some(_)) => FeatureResultGroupMembers::default(),
        (Some(image), None) => image,
        (None, Some(group)) => group,
        (None, None) => FeatureResultGroupMembers::default(),
    })
}

fn feature_result_group_members(
    ctx: &DecodeContext<'_>,
    partition_stream_ordinal: u32,
    member_ids: &[String],
    members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Result<FeatureResultGroupMembers, CodecError> {
    use crate::native::parasolid::group_member::{GroupMemberTarget, GroupNodeFamily};
    let mut result = FeatureResultGroupMembers::default();
    for member_id in member_ids {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(members.len()),
            "NX feature result group member lookup",
        )?;
        let mut matches = members.iter().filter(|member| member.id == *member_id);
        let Some(member) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        if member.partition_stream_ordinal != partition_stream_ordinal {
            continue;
        }
        let GroupMemberTarget::Node {
            family,
            current_xmt: Some(xmt),
            ..
        } = member.target
        else {
            continue;
        };
        let (kind, output) = match family {
            GroupNodeFamily::Face => ("face", &mut result.faces),
            GroupNodeFamily::Edge => ("edge", &mut result.edges),
            GroupNodeFamily::Vertex => ("vertex", &mut result.vertices),
            _ => continue,
        };
        let identity_len = 5 + 10 + 1 + kind.len() + 1 + 10;
        ctx.charge_collection_items(1, "NX feature result group members")?;

        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(identity_len),
            "NX feature result group member identity formatting",
        )?;
        ctx.reserve_capacity(output, 1, "allocate NX feature result group members")?;
        let mut text = String::new();
        ctx.try_reserve_retained_text(
            &mut text,
            identity_len,
            "NX feature result group member identity",
        )?;
        std::fmt::Write::write_fmt(
            &mut text,
            format_args!("nx:s{partition_stream_ordinal}:{kind}#{xmt}"),
        )
        .map_err(|_| {
            CodecError::InvalidInput("NX result member identity formatting failed".to_string())
        })?;
        let identity = cadmpeg_core::text::NonBlankString::new(text)
            .ok_or_else(|| CodecError::malformed("NX result member identity is blank"))?;
        output.push(identity);
    }
    Ok(result)
}

/// Select the exact native writer identity for one intermediate body result.
/// A primary-body writer is canonical when both native forms are present. The
/// Boolean target is independently sufficient when the primary form is absent.
fn native_result_body_identity(
    ctx: &DecodeContext<'_>,
    primary: Option<&crate::native::features::FeatureBodyReference>,
    boolean: Option<&crate::native::features::FeatureBooleanOperation>,
) -> Result<Option<(cadmpeg_core::text::NonBlankString, String)>, CodecError> {
    let (native, suffix) = if let Some(primary) = primary {
        (primary.id.as_str(), "")
    } else if let Some(boolean) = boolean {
        (boolean.id.as_str(), ":target")
    } else {
        return Ok(None);
    };
    let local_len = native.len().checked_add(suffix.len()).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX result body local identity",
            0,
            cadmpeg_core::decode::u64_from_index(native.len()),
        )
    })?;

    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(local_len),
        "NX result body identity formatting",
    )?;

    let mut local = String::new();
    ctx.try_reserve_retained_text(&mut local, local_len, "NX result body identity")?;
    local.push_str(native);
    local.push_str(suffix);
    let Some(local) = cadmpeg_core::text::NonBlankString::new(local) else {
        return Ok(None);
    };
    let mut native_ref = String::new();
    ctx.try_reserve_retained_text(&mut native_ref, native.len(), "NX result body identity")?;
    native_ref.push_str(native);
    Ok(Some((local, native_ref)))
}

/// Return primary body fields that are proven to use the segment-object
/// namespace. An offset-store field may enter that namespace only when the
/// feature extractor has also retained one unique segment alias use for the
/// same field. Missing or ambiguous relations remain offset-store-local. An
/// operation with zero or multiple body fields has no primary-body selection.
fn native_primary_body_references<'a>(
    ctx: &DecodeContext<'_>,
    references: &'a [crate::native::features::FeatureBodyReference],
    data_block_uses: &[crate::native::features::FeatureBodyDataBlockUse],
    segment_uses: &[crate::native::features::FeatureBodySegmentUse],
    inputs: &[crate::native::features::FeatureInputBlock],
    data_blocks: &[crate::native::om::DataBlock],
) -> Result<BTreeMap<&'a str, u32>, CodecError> {
    let (unique_references, _unique_references_storage) = ctx
        .with_scoped_storage("NX local unique_references storage", || {
            crate::native::features::unique_feature_body_references(ctx, references)
        })?;
    let mut offset_reservation = ctx.reserve_scoped(0, "NX primary offset-store references")?;
    let mut offset_store_references = BTreeSet::new();
    for use_ in data_block_uses {
        offset_reservation.with_storage(|| {
            ctx.insert_btree_set(
                &mut offset_store_references,
                use_.feature_body_reference.as_str(),
                "NX primary offset-store references",
            )
        })?;
    }
    let (offset_store_operations, _offset_store_operations_storage) = ctx
        .with_scoped_storage("NX local offset_store_operations storage", || {
            crate::native::features::feature_input_store_operations(ctx, inputs, data_blocks)
        })?;
    let mut bridge_reservation = ctx.reserve_scoped(0, "NX primary bridged references")?;
    let mut bridged_segment_references = BTreeSet::new();
    for use_ in segment_uses {
        bridge_reservation.with_storage(|| {
            ctx.insert_btree_set(
                &mut bridged_segment_references,
                use_.feature_body_reference.as_str(),
                "NX primary bridged references",
            )
        })?;
    }
    let mut output = BTreeMap::new();
    for (operation, reference) in unique_references {
        if !bridged_segment_references.contains(reference.id.as_str())
            && (offset_store_references.contains(reference.id.as_str())
                || offset_store_operations.contains(reference.operation_label.as_str()))
        {
            continue;
        }
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, u32)>() * 4),
            "NX primary body references",
        )?;
        ctx.insert_btree_map(
            &mut output,
            operation,
            reference.body.value(),
            "NX primary body references",
        )?;
    }
    Ok(output)
}

fn attach_sketch_graph(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    label: &crate::native::features::FeatureOperationLabel,
    sources: &SketchSources<'_>,
    annotations: &mut AnnotationBuilder,
    stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<Option<SketchId>, CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "NX sketch projection")?;
    let mut operation_groups = Vec::new();
    for group in sources
        .point_groups
        .iter()
        .filter(|group| group.operation_label == label.id)
    {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut operation_groups,
            1,
            "NX sketch operation groups",
        )?;
        operation_groups.push(group);
    }
    let operation_key = label
        .id
        .strip_prefix("nx:feature-history:operation-label#")
        .unwrap_or(label.id.as_str());
    let sketch_id_bytes = operation_key
        .len()
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(40))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX sketch identity",
                0,
                cadmpeg_core::decode::u64_from_index(operation_key.len()),
            )
        })?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(sketch_id_bytes),
        "NX sketch identity",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(operation_key.len()),
        "NX sketch identity",
    )?;
    let mut owned_operation_key = String::new();
    ctx.try_reserve_retained_text(
        &mut owned_operation_key,
        operation_key.len(),
        "allocate NX sketch identity",
    )?;
    owned_operation_key.push_str(operation_key);
    let Some(sketch_id): Option<SketchId> =
        IdScope::native(cadmpeg_ir::identity_component!("feature-history")).try_id(
            &cadmpeg_ir::identity_component!("sketch"),
            owned_operation_key,
        )
    else {
        return Ok(None);
    };
    let mut operation_fixed_points = Vec::new();
    for &point in sources
        .fixed_points
        .iter()
        .filter(|point| point.operation_label == label.id)
    {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut operation_fixed_points,
            1,
            "NX sketch fixed-point inputs",
        )?;
        operation_fixed_points.push(point);
    }
    if operation_groups.is_empty() {
        let mut coordinate_pairs = Vec::new();
        for &pair in sources
            .coordinate_pairs
            .iter()
            .filter(|pair| pair.operation_label == label.id)
        {
            ctx.reserve_scoped_vec(
                &mut reservation,
                &mut coordinate_pairs,
                1,
                "NX sketch coordinate pair inputs",
            )?;
            coordinate_pairs.push(pair);
        }
        if coordinate_pairs.is_empty() && operation_fixed_points.is_empty() {
            return Ok(None);
        }
        let mut entities = Vec::new();
        let mut pair_ids = BTreeSet::new();
        let mut pair_entity_keys = BTreeSet::new();
        let mut pair_ordinals = BTreeSet::new();
        for pair in coordinate_pairs {
            if !ctx.insert_scoped_btree_set(
                &mut reservation,
                &mut pair_ids,
                pair.id.as_str(),
                "NX sketch key uniqueness",
                "NX sketch key index",
            )? || !ctx.insert_scoped_btree_set(
                &mut reservation,
                &mut pair_ordinals,
                (pair.payload.id(), pair.ordinal),
                "NX sketch key uniqueness",
                "NX sketch key index",
            )? {
                return Ok(None);
            }
            let pair_key = pair
                .id
                .rsplit_once('#')
                .map_or(pair.id.as_str(), |(_, key)| key);
            if pair_key.is_empty()
                || pair_key.chars().any(char::is_whitespace)
                || !ctx.insert_scoped_btree_set(
                    &mut reservation,
                    &mut pair_entity_keys,
                    pair_key,
                    "NX sketch key uniqueness",
                    "NX sketch key index",
                )?
            {
                return Ok(None);
            }
            let Some(entity_id) = sketch_entity_identity(ctx, "coordinate-pair-", pair_key)? else {
                return Ok(None);
            };
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index("nx-coordinate-pair".len()),
                "NX coordinate-pair sketch entity",
            )?;
            let Some(native_kind) = cadmpeg_core::text::NonBlankString::new("nx-coordinate-pair")
            else {
                return Ok(None);
            };
            let native_ref = ctx.copy_retained_text(&pair.id, "allocate NX sketch text")?;
            push_sketch_entity(
                ctx,
                &mut reservation,
                &mut entities,
                pair.source_offset,
                SketchEntity::new(
                    entity_id,
                    sketch_id.try_clone_for_decode(ctx, "NX decoded IR value copy")?,
                    SketchGeometry::native(native_kind),
                )
                .with_native_ref(Some(native_ref)),
            )?;
        }
        if !append_fixed_sketch_entities(
            ctx,
            &mut reservation,
            &mut entities,
            label,
            &sketch_id,
            &operation_fixed_points,
        )? {
            return Ok(None);
        }
        ctx.stable_sort_by(
            &mut entities,
            |(first_offset, first), (second_offset, second)| {
                first_offset
                    .cmp(second_offset)
                    .then_with(|| first.id().cmp(second.id()))
            },
            |(_, entity)| entity.id().as_str().len(),
            "NX sketch entity order",
        )?;
        for (source_offset, entity) in &entities {
            let tag = match entity.geometry.definition() {
                SketchGeometryDefinition::Native { native_kind }
                    if native_kind == "nx-coordinate-pair" =>
                {
                    "SKETCH_NATIVE_COORDINATE_PAIR"
                }
                SketchGeometryDefinition::Native { native_kind }
                    if native_kind == "nx-fixed-point" =>
                {
                    "SKETCH_NATIVE_FIXED_POINT"
                }
                _ => "SKETCH_NATIVE",
            };
            annotations.note(ctx, entity.id().as_str(), stream, *source_offset, Some(tag))?;
            annotations.exactness(ctx, entity.id().as_str(), Exactness::ByteExact)?;
        }
        annotations.note(
            ctx,
            sketch_id.as_str(),
            stream,
            label.source_offset,
            Some("SKETCH"),
        )?;
        annotations.exactness(ctx, sketch_id.as_str(), Exactness::Derived)?;
        emit_sketch(ctx, ir, label, &sketch_id, entities)?;
        return Ok(Some(sketch_id));
    }
    let mut groups_by_id =
        BTreeMap::<&str, &crate::native::features::FeatureSketchPointGroup>::new();
    for group in &operation_groups {
        if !ctx.insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut groups_by_id,
            group.id.as_str(),
            group,
            "NX sketch record uniqueness",
            "NX sketch record index",
        )? {
            return Ok(None);
        }
    }
    let mut point_uses_by_group =
        BTreeMap::<&str, &crate::native::features::FeatureSketchPointUse>::new();
    for point_use in sources.point_uses {
        if point_use.operation_label != label.id
            || !ctx.insert_scoped_btree_map_if_vacant(
                &mut reservation,
                &mut point_uses_by_group,
                point_use.sketch_point_group.as_str(),
                point_use,
                "NX sketch record uniqueness",
                "NX sketch record index",
            )?
        {
            return Ok(None);
        }
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(
            groups_by_id
                .len()
                .checked_mul(point_uses_by_group.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "NX sketch point-use group check",
                        0,
                        cadmpeg_core::decode::u64_from_index(groups_by_id.len()),
                    )
                })?,
        ),
        "NX sketch point-use group check",
    )?;
    if point_uses_by_group
        .keys()
        .any(|group| !groups_by_id.contains_key(group))
    {
        return Ok(None);
    }
    let mut points_by_id = BTreeMap::<&str, &crate::native::features::FeatureSketchPoint>::new();
    for point in sources.points {
        if !ctx.insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut points_by_id,
            point.id.as_str(),
            point,
            "NX sketch record uniqueness",
            "NX sketch record index",
        )? {
            return Ok(None);
        }
    }
    let mut scalars_by_id = BTreeMap::<&str, &crate::native::features::FeaturePayloadScalar>::new();
    for scalar in sources.payload_scalars {
        if !ctx.insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut scalars_by_id,
            scalar.id.as_str(),
            scalar,
            "NX sketch record uniqueness",
            "NX sketch record index",
        )? {
            return Ok(None);
        }
    }
    let mut entities = Vec::new();
    for group in operation_groups {
        let point_use = point_uses_by_group.get(group.id.as_str()).copied();
        let Some(source_offset) = sketch_group_source_offset(
            ctx,
            label,
            group,
            point_use,
            &points_by_id,
            &scalars_by_id,
        )?
        else {
            return Ok(None);
        };
        let native_ref_source =
            point_use.map_or(group.id.as_str(), |point_use| point_use.id.as_str());
        let entity_key = point_use
            .map_or(group.id.as_str(), |point_use| point_use.id.as_str())
            .strip_prefix("nx:feature-history:sketch-point-use#")
            .or_else(|| {
                group
                    .id
                    .strip_prefix("nx:feature-history:sketch-point-group#")
            })
            .unwrap_or(group.id.as_str());
        let Some(entity_id) = sketch_entity_identity(ctx, "point-", entity_key)? else {
            return Ok(None);
        };
        let native_ref = ctx.copy_retained_text(native_ref_source, "allocate NX sketch text")?;
        let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(group.coordinates[0], group.coordinates[1]),
        }) else {
            return Ok(None);
        };
        push_sketch_entity(
            ctx,
            &mut reservation,
            &mut entities,
            source_offset,
            SketchEntity::new(
                entity_id,
                sketch_id.try_clone_for_decode(ctx, "NX decoded IR value copy")?,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
        )?;
    }
    if !append_fixed_sketch_entities(
        ctx,
        &mut reservation,
        &mut entities,
        label,
        &sketch_id,
        &operation_fixed_points,
    )? {
        return Ok(None);
    }
    ctx.stable_sort_by(
        &mut entities,
        |(first_offset, first), (second_offset, second)| {
            first_offset
                .cmp(second_offset)
                .then_with(|| first.id().cmp(second.id()))
        },
        |(_, entity)| entity.id().as_str().len(),
        "NX sketch entity order",
    )?;
    if entities.is_empty() {
        return Ok(None);
    }
    for (source_offset, entity) in &entities {
        match entity.geometry.definition() {
            SketchGeometryDefinition::Point { .. } => {
                annotations.note(
                    ctx,
                    entity.id().as_str(),
                    stream,
                    *source_offset,
                    Some("SKETCH_POINT"),
                )?;
                annotations.exactness(ctx, entity.id().as_str(), Exactness::Derived)?;
            }
            SketchGeometryDefinition::Native { native_kind } => {
                let tag = if native_kind == "nx-fixed-point" {
                    "SKETCH_NATIVE_FIXED_POINT"
                } else {
                    "SKETCH_NATIVE"
                };
                annotations.note(ctx, entity.id().as_str(), stream, *source_offset, Some(tag))?;
                annotations.exactness(ctx, entity.id().as_str(), Exactness::ByteExact)?;
            }
            _ => return Ok(None),
        }
    }
    annotations.note(
        ctx,
        sketch_id.as_str(),
        stream,
        label.source_offset,
        Some("SKETCH"),
    )?;
    annotations.exactness(ctx, sketch_id.as_str(), Exactness::Derived)?;
    emit_sketch(ctx, ir, label, &sketch_id, entities)?;
    Ok(Some(sketch_id))
}

struct SketchSources<'a> {
    point_uses: &'a [&'a crate::native::features::FeatureSketchPointUse],
    point_groups: &'a [crate::native::features::FeatureSketchPointGroup],
    points: &'a [crate::native::features::FeatureSketchPoint],
    payload_scalars: &'a [crate::native::features::FeaturePayloadScalar],
    fixed_points: &'a [&'a crate::native::features::FeatureSketchFixedPoint],
    coordinate_pairs: &'a [&'a crate::native::features::FeaturePayloadScalarPair],
}

fn sketch_group_source_offset(
    ctx: &DecodeContext<'_>,
    label: &crate::native::features::FeatureOperationLabel,
    group: &crate::native::features::FeatureSketchPointGroup,
    point_use: Option<&crate::native::features::FeatureSketchPointUse>,
    points_by_id: &BTreeMap<&str, &crate::native::features::FeatureSketchPoint>,
    scalars_by_id: &BTreeMap<&str, &crate::native::features::FeaturePayloadScalar>,
) -> Result<Option<u64>, CodecError> {
    if let Some(point_use) = point_use {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(point_use.references.len()),
            "NX sketch point-use offsets",
        )?;
        return Ok(point_use
            .references
            .iter()
            .map(|reference| reference.source_offset)
            .min());
    }
    let mut minimum = None::<u64>;
    for point_id in &group.points {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(points_by_id.len()),
            "NX sketch point lookup",
        )?;
        let Some(point) = points_by_id.get(point_id.as_str()).copied() else {
            return Ok(None);
        };
        if point.operation_label != label.id
            || point.name != group.name
            || point
                .coordinates
                .iter()
                .zip(group.coordinates)
                .any(|(first, second)| first.to_bits() != second.to_bits())
        {
            return Ok(None);
        }
        let [first, second] = point.scalar_fields.as_slice() else {
            return Ok(None);
        };
        for (scalar_id, coordinate) in [first, second].into_iter().zip(group.coordinates) {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(scalars_by_id.len()),
                "NX sketch scalar lookup",
            )?;
            let Some(scalar) = scalars_by_id.get(scalar_id.as_str()).copied() else {
                return Ok(None);
            };
            if scalar.operation_label != label.id
                || scalar.scalar.value().get().to_bits() != coordinate.to_bits()
            {
                return Ok(None);
            }
            minimum = Some(minimum.map_or(scalar.source_offset, |current| {
                current.min(scalar.source_offset)
            }));
        }
    }
    Ok(minimum)
}

fn sketch_entity_identity(
    ctx: &DecodeContext<'_>,
    prefix: &str,
    key: &str,
) -> Result<Option<SketchEntityId>, CodecError> {
    let key_len = prefix.len().checked_add(key.len()).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX sketch entity identity",
            0,
            cadmpeg_core::decode::u64_from_index(key.len()),
        )
    })?;
    let charged_len = key_len
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(48))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX sketch entity identity",
                0,
                cadmpeg_core::decode::u64_from_index(key_len),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(key_len),
        "NX sketch entity identity",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(charged_len),
        "NX sketch entity identity",
    )?;
    let mut text = String::new();
    ctx.try_reserve_retained_text(&mut text, key_len, "allocate NX sketch entity identity")?;
    text.push_str(prefix);
    text.push_str(key);
    Ok(
        IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
            .try_id(&cadmpeg_ir::identity_component!("sketch-entity"), text),
    )
}

fn push_sketch_entity(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    entities: &mut Vec<(u64, SketchEntity)>,
    source_offset: u64,
    entity: SketchEntity,
) -> Result<(), CodecError> {
    ctx.reserve_scoped_vec(reservation, entities, 1, "NX sketch staged entities")?;
    entities.push((source_offset, entity));
    Ok(())
}

fn emit_sketch(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    label: &crate::native::features::FeatureOperationLabel,
    sketch_id: &SketchId,
    entities: Vec<(u64, SketchEntity)>,
) -> Result<(), CodecError> {
    ctx.reserve_vec(
        &mut ir.model.sketch_entities,
        entities.len(),
        "NX sketch output entities",
    )?;
    ctx.reserve_vec(&mut ir.model.sketches, 1, "NX sketch output")?;
    let name = ctx.copy_retained_text(&label.value, "allocate NX sketch text")?;
    let native_ref = ctx.copy_retained_text(&label.id, "allocate NX sketch text")?;
    ir.model
        .sketch_entities
        .extend(entities.into_iter().map(|(_, entity)| entity));
    ir.model.sketches.push(Sketch {
        id: sketch_id.try_clone_for_decode(ctx, "NX decoded IR value copy")?,
        name: Some(name),
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(native_ref),
    });
    Ok(())
}

fn native_fixed_point_entities(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    label: &crate::native::features::FeatureOperationLabel,
    sketch_id: &SketchId,
    points: &[&crate::native::features::FeatureSketchFixedPoint],
) -> Result<Option<Vec<(u64, SketchEntity)>>, CodecError> {
    let mut point_ids = BTreeSet::new();
    let mut entity_keys = BTreeSet::new();
    let mut entities = Vec::new();
    for point in points {
        if point.operation_label != label.id
            || !ctx.insert_scoped_btree_set(
                reservation,
                &mut point_ids,
                point.id.as_str(),
                "NX sketch key uniqueness",
                "NX sketch key index",
            )?
        {
            return Ok(None);
        }
        let point_key = point
            .id
            .rsplit_once('#')
            .map_or(point.id.as_str(), |(_, key)| key);
        if point_key.is_empty()
            || point_key.chars().any(char::is_whitespace)
            || !ctx.insert_scoped_btree_set(
                reservation,
                &mut entity_keys,
                point_key,
                "NX sketch key uniqueness",
                "NX sketch key index",
            )?
        {
            return Ok(None);
        }
        let Some(entity_id) = sketch_entity_identity(ctx, "fixed-point-", point_key)? else {
            return Ok(None);
        };
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index("nx-fixed-point".len()),
            "NX fixed-point sketch entity",
        )?;
        let Some(native_kind) = cadmpeg_core::text::NonBlankString::new("nx-fixed-point") else {
            return Ok(None);
        };
        let native_ref = ctx.copy_retained_text(&point.id, "allocate NX sketch text")?;
        push_sketch_entity(
            ctx,
            reservation,
            &mut entities,
            point.source_offset,
            SketchEntity::new(
                entity_id,
                sketch_id.try_clone_for_decode(ctx, "NX decoded IR value copy")?,
                SketchGeometry::native(native_kind),
            )
            .with_native_ref(Some(native_ref)),
        )?;
    }
    Ok(Some(entities))
}

fn append_fixed_sketch_entities(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    entities: &mut Vec<(u64, SketchEntity)>,
    label: &crate::native::features::FeatureOperationLabel,
    sketch_id: &SketchId,
    fixed_points: &[&crate::native::features::FeatureSketchFixedPoint],
) -> Result<bool, CodecError> {
    let Some(fixed_entities) =
        native_fixed_point_entities(ctx, reservation, label, sketch_id, fixed_points)?
    else {
        return Ok(false);
    };

    ctx.reserve_scoped_vec(
        reservation,
        entities,
        fixed_entities.len(),
        "NX sketch merged fixed points",
    )?;
    entities.extend(fixed_entities);
    Ok(true)
}

struct SegmentBindingBodyIndexes<'a, 'ctx> {
    by_object: BTreeMap<u32, Vec<BodyId>>,
    by_binding: BTreeMap<&'a str, Vec<BodyId>>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn segment_binding_body_indexes<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    bindings: &'a [crate::native::segments::SegmentBodyBinding],
) -> Result<SegmentBindingBodyIndexes<'a, 'ctx>, CodecError> {
    let mut by_object = BTreeMap::<u32, Vec<BodyId>>::new();
    let mut by_binding = BTreeMap::<&str, Vec<BodyId>>::new();
    let mut reservation = ctx.reserve_scoped(0, "NX segment binding body indexes")?;
    for binding in bindings {
        let (prefix, prefix_len) = stream_prefix(binding.stream_ordinal, false)?;
        let mut stream_bodies = Vec::new();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()),
            "NX segment body prefix scan",
        )?;
        for body in ir.model.bodies.iter().filter(|body| {
            body.id
                .as_str()
                .as_bytes()
                .starts_with(&prefix[..prefix_len])
        }) {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(stream_bodies.len()),
                "NX segment body uniqueness",
            )?;
            if stream_bodies.contains(&body.id) {
                continue;
            }
            ctx.charge_collection_items(1, "NX segment body identity")?;
            reservation.with_storage(|| {
                ctx.reserve_capacity(&mut stream_bodies, 1, "NX segment body identity")
            })?;
            stream_bodies.push(reservation.with_storage(|| {
                body.id
                    .try_clone_for_decode(ctx, "NX segment body identity")
            })?);
        }
        for identity in [binding.body_object_index, binding.body_alias_object_index] {
            for body in &stream_bodies {
                let existing = by_object.get(&identity).map_or(0, Vec::len);
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(existing),
                    "NX segment body alias uniqueness",
                )?;
                if by_object
                    .get(&identity)
                    .is_some_and(|bodies| bodies.contains(body))
                {
                    continue;
                }
                let body = reservation.with_storage(|| {
                    body.try_clone_for_decode(ctx, "NX feature operation group body")
                })?;
                ctx.push_scoped_btree_group(
                    &mut reservation,
                    &mut by_object,
                    identity,
                    || body,
                    0,
                    "NX feature operation group index",
                )?;
            }
        }
        ctx.charge_work(1, "NX segment binding identity index")?;
        if !by_binding.contains_key(binding.id.as_str()) {
            reservation.with_storage(|| {
                ctx.admit_btree_entry(
                    &by_binding,
                    &binding.id.as_str(),
                    "NX segment binding identity index",
                )
            })?;
        }
        by_binding.insert(binding.id.as_str(), stream_bodies);
    }
    Ok(SegmentBindingBodyIndexes {
        by_object,
        by_binding,
        _reservation: reservation,
    })
}

fn stream_prefix(ordinal: u32, body_marker: bool) -> Result<([u8; 20], usize), CodecError> {
    let mut decimal = [0u8; 10];
    let mut digit_count = 0;
    let mut ordinal = ordinal;
    loop {
        decimal[digit_count] = b'0'
            + u8::try_from(ordinal % 10)
                .map_err(|_| CodecError::malformed("NX stream ordinal decimal digit exceeds u8"))?;
        digit_count += 1;
        ordinal /= 10;
        if ordinal == 0 {
            break;
        }
    }
    let mut prefix = [0u8; 20];
    prefix[..4].copy_from_slice(b"nx:s");
    for (index, digit) in decimal[..digit_count].iter().rev().enumerate() {
        prefix[4 + index] = *digit;
    }
    let suffix = if body_marker {
        b":body#".as_slice()
    } else {
        b":".as_slice()
    };
    let suffix_start = 4 + digit_count;
    let prefix_len = suffix_start + suffix.len();
    prefix[suffix_start..prefix_len].copy_from_slice(suffix);
    Ok((prefix, prefix_len))
}

fn operation_source_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    operation_label: &str,
    records: &[crate::native::features::operation_record::FeatureOperationRecord],
    common_frames: &[crate::native::features::FeatureOperationCommonFrame],
    terminal_frames: &[crate::native::features::FeatureOperationTerminalFrame],
) -> Result<(), CodecError> {
    let mut matching_records = records
        .iter()
        .filter(|record| record.operation_label == operation_label);
    let Some(record) = matching_records.next() else {
        return Ok(());
    };
    if matching_records.next().is_some() {
        return Ok(());
    }

    insert_operation_source_property(ctx, properties, "operation_record", &record.id)?;
    let contiguous_common_frames = common_frames
        .iter()
        .filter(|frame| frame.operation_record == record.id)
        .enumerate()
        .all(|(ordinal, frame)| {
            u64::from(frame.ordinal) == cadmpeg_core::decode::u64_from_index(ordinal)
        });
    if contiguous_common_frames {
        for frame in common_frames
            .iter()
            .filter(|frame| frame.operation_record == record.id)
        {
            const PREFIX: &str = "operation_common_frame.";
            let key_len = PREFIX
                .len()
                .checked_add(10)
                .ok_or_else(|| ctx.refuse_codec_limit("NX operation source property key", 0, 10))?;

            let mut key = String::new();
            ctx.try_reserve_retained_text(&mut key, key_len, "NX operation source property")?;
            std::fmt::Write::write_fmt(&mut key, format_args!("{PREFIX}{}", frame.ordinal))
                .map_err(|_| {
                    CodecError::InvalidInput(
                        "NX operation source property key formatting failed".to_string(),
                    )
                })?;
            let mut value = String::new();
            ctx.try_reserve_retained_text(
                &mut value,
                frame.id.len(),
                "NX operation source property",
            )?;
            value.push_str(&frame.id);
            ctx.insert_btree_map(properties, key, value, "NX operation source properties")?;
        }
    }
    let mut matching_frames = terminal_frames
        .iter()
        .filter(|frame| frame.operation_record == record.id);
    let Some(frame) = matching_frames.next() else {
        return Ok(());
    };
    if matching_frames.next().is_some() {
        return Ok(());
    }
    insert_operation_source_property(ctx, properties, "operation_terminal_frame", &frame.id)?;
    Ok(())
}

fn insert_operation_source_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    key: &str,
    value: &str,
) -> Result<(), CodecError> {
    let mut owned_key = String::new();
    ctx.try_reserve_retained_text(&mut owned_key, key.len(), "NX operation source property")?;
    owned_key.push_str(key);
    let mut owned_value = String::new();
    ctx.try_reserve_retained_text(
        &mut owned_value,
        value.len(),
        "NX operation source property",
    )?;
    owned_value.push_str(value);
    ctx.insert_btree_map(
        properties,
        owned_key,
        owned_value,
        "NX operation source properties",
    )?;
    Ok(())
}

fn insert_source_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    struct Length(usize);

    impl std::fmt::Write for Length {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.0 = self.0.checked_add(text.len()).ok_or(std::fmt::Error)?;
            Ok(())
        }
    }

    let mut key_length = Length(0);
    std::fmt::write(&mut key_length, key)
        .map_err(|_| ctx.refuse_codec_limit("NX source property key length", 0, 1))?;
    let mut value_length = Length(0);
    std::fmt::write(&mut value_length, value)
        .map_err(|_| ctx.refuse_codec_limit("NX source property value length", 0, 1))?;
    let text_bytes = key_length.0.checked_add(value_length.0).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX source property text",
            0,
            cadmpeg_core::decode::u64_from_index(value_length.0),
        )
    })?;

    let work = text_bytes.checked_mul(2).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX source property formatting",
            0,
            cadmpeg_core::decode::u64_from_index(text_bytes),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "NX source property formatting",
    )?;

    let mut owned_key = String::new();
    ctx.try_reserve_retained_text(&mut owned_key, key_length.0, "NX source property")?;
    std::fmt::write(&mut owned_key, key).map_err(|_| {
        CodecError::InvalidInput("NX source property key formatting failed".to_string())
    })?;
    let mut owned_value = String::new();
    ctx.try_reserve_retained_text(&mut owned_value, value_length.0, "NX source property")?;
    std::fmt::write(&mut owned_value, value).map_err(|_| {
        CodecError::InvalidInput("NX source property value formatting failed".to_string())
    })?;
    ctx.insert_btree_map(properties, owned_key, owned_value, "NX source properties")?;
    Ok(())
}

fn insert_source_property_reference(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    key: std::fmt::Arguments<'_>,
    data_block: Option<&str>,
    fallback: impl std::fmt::Display,
) -> Result<(), CodecError> {
    match data_block {
        Some(data_block) => {
            insert_source_property(ctx, properties, key, format_args!("{data_block}"))
        }
        None => insert_source_property(ctx, properties, key, format_args!("{fallback}")),
    }
}

struct ParasolidStringAttributeSources<'a> {
    string_uses: &'a [crate::native::parasolid::ParasolidEntity51StringUse],
    strings: &'a [crate::native::parasolid::ParasolidEntity54StringRecord],
}

fn attach_parasolid_topology_string_attributes(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &ParasolidStringAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_, '_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let (strings_by_id, _strings_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .strings
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid attribute record index",
    )?;
    let (mut uses_by_entity, _uses_by_entity_reservation) = ctx.collect_scoped_btree_groups(
        sources
            .string_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
        "NX Parasolid attribute use groups",
    )?;
    for uses in uses_by_entity.values_mut() {
        ctx.stable_sort_by(
            uses,
            |left, right| left.position.cmp(&right.position),
            |_| 0,
            "NX Parasolid string attribute ordering",
        )?;
    }
    for context in &attribute_index.contexts {
        let reference = context.reference;
        let entity = context.entity;
        for string_use in uses_by_entity.get(entity).into_iter().flatten() {
            let Some(string) = strings_by_id.get(string_use.string_record.as_str()) else {
                continue;
            };
            let id = topology_attribute_id(
                ctx,
                reference,
                &cadmpeg_ir::identity_component!("topology-string-attribute"),
                string_use.position.reference_ordinal(),
                context.id_suffix.as_ref(),
            )?;
            let source_stream = StreamHandle::new(
                ctx,
                cadmpeg_ir::stream_name!("nx:s").with_suffix(
                    ctx,
                    reference.stream_ordinal,
                    "compose annotation stream name",
                )?,
                "allocate annotation stream handle",
            )?;
            annotations.note(
                ctx,
                id.as_str(),
                &source_stream,
                string.inflated_offset,
                Some("ENTITY_54_STRING_ATTRIBUTE"),
            )?;
            annotations
                .derived(ctx, id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(ctx, id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::from)?;
            let field_name = attribute_index.attribute_names.field_name(
                ctx,
                reference,
                string_use.id.as_str(),
            )?;
            let name = topology_attribute_name(
                ctx,
                field_name,
                attribute_index
                    .class_names
                    .get(reference.id.as_str())
                    .and_then(Option::as_ref)
                    .copied(),
                "84",
                string_use.position.reference_ordinal(),
            )?;
            let values = single_string_attribute_values(ctx, string.value.as_str())?;
            push_topology_attribute(ctx, ir, context, id, name, values)?;
        }
    }
    ctx.stable_sort_by(
        &mut ir.model.attributes,
        |first, second| first.id.as_str().cmp(second.id.as_str()),
        |attribute| attribute.id.as_str().len(),
        "sort NX Parasolid string attributes",
    )?;
    Ok(())
}

struct ParasolidNumericAttributeSources<'a> {
    numeric_uses: &'a [crate::native::parasolid::ParasolidEntity51NumericUse],
    integers: &'a [crate::native::parasolid::ParasolidEntity52IntegerRecord],
    doubles: &'a [crate::native::parasolid::ParasolidEntity53DoubleRecord],
}

struct ParasolidAttributeNameIndex<'a> {
    classes_by_entity: BTreeMap<
        (&'a str, &'a str),
        Option<&'a crate::native::parasolid::ParasolidTopologyAttributeClassUse>,
    >,
    fields_by_value_use:
        BTreeMap<&'a str, Option<&'a crate::native::parasolid::ParasolidAttributeFieldUse>>,
    definitions_by_id:
        BTreeMap<&'a str, Option<&'a crate::native::parasolid::ParasolidAttributeDefinition>>,
    field_names_by_definition:
        BTreeMap<&'a str, Option<&'a crate::native::parasolid::ParasolidAttributeFieldNames>>,
}

impl<'a> ParasolidAttributeNameIndex<'a> {
    fn new(
        ctx: &DecodeContext<'_>,
        reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
        definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
        field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
        field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    ) -> Result<Self, CodecError> {
        let mut classes_by_entity = BTreeMap::new();
        for class_use in class_uses {
            insert_sole(
                ctx,
                reservation,
                &mut classes_by_entity,
                (
                    class_use.topology_attribute_reference.as_str(),
                    class_use.entity_51_record.as_str(),
                ),
                class_use,
            )?;
        }

        let mut fields_by_value_use = BTreeMap::new();
        for field_use in field_uses {
            insert_sole(
                ctx,
                reservation,
                &mut fields_by_value_use,
                field_use.value_use.as_str(),
                field_use,
            )?;
        }

        let mut definitions_by_id = BTreeMap::new();
        for definition in definitions {
            insert_sole(
                ctx,
                reservation,
                &mut definitions_by_id,
                definition.id.as_str(),
                definition,
            )?;
        }

        let mut field_names_by_definition = BTreeMap::new();
        for names in field_names {
            insert_sole(
                ctx,
                reservation,
                &mut field_names_by_definition,
                names.attribute_definition.as_str(),
                names,
            )?;
        }

        Ok(Self {
            classes_by_entity,
            fields_by_value_use,
            definitions_by_id,
            field_names_by_definition,
        })
    }

    fn field_name(
        &self,
        ctx: &DecodeContext<'_>,
        topology_reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
        value_use: &str,
    ) -> Result<Option<String>, CodecError> {
        let lookup_work = self
            .fields_by_value_use
            .len()
            .checked_add(self.classes_by_entity.len())
            .and_then(|count| count.checked_add(self.definitions_by_id.len()))
            .and_then(|count| count.checked_add(self.field_names_by_definition.len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX Parasolid attribute field name lookup",
                    0,
                    cadmpeg_core::decode::u64_from_index(self.fields_by_value_use.len()),
                )
            })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(lookup_work),
            "NX Parasolid attribute field name lookup",
        )?;
        let Some(field_use) = self
            .fields_by_value_use
            .get(value_use)
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        let Some(class_use) = self
            .classes_by_entity
            .get(&(
                topology_reference.id.as_str(),
                field_use.entity_51_record.as_str(),
            ))
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        if field_use.attribute_class_use != class_use.attribute_class_use
            || field_use.attribute_definition != class_use.attribute_definition
        {
            return Ok(None);
        }
        let Some(definition) = self
            .definitions_by_id
            .get(class_use.attribute_definition.as_str())
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        let mut field_reservation = ctx.reserve_scoped(0, "NX Parasolid field name component")?;
        let field_name = match (definition.name.as_str(), field_use.position.field_ordinal()) {
            ("SDL/TYSA_DENSITY", 0) => std::borrow::Cow::Borrowed("density"),
            ("SDL/TYSA_DENSITY", 1) => std::borrow::Cow::Borrowed("units"),
            _ if self
                .field_names_by_definition
                .get(definition.id.as_str())
                .and_then(Option::as_ref)
                .is_some() =>
            {
                let Some(name) = self
                    .field_names_by_definition
                    .get(definition.id.as_str())
                    .and_then(Option::as_ref)
                    .and_then(|names| {
                        names.fields.get(cadmpeg_core::decode::index_from_u32(
                            field_use.position.field_ordinal(),
                        ))
                    })
                    .map(|field| field.name.as_str())
                else {
                    return Ok(None);
                };
                std::borrow::Cow::Borrowed(name)
            }
            _ => {
                const BOUND: usize = 64;

                let mut name = String::new();
                field_reservation.with_storage(|| {
                    ctx.try_reserve_retained_text(
                        &mut name,
                        BOUND,
                        "allocate NX Parasolid field name component",
                    )
                })?;
                std::fmt::Write::write_fmt(
                    &mut name,
                    format_args!(
                        "field_{}.parasolid_type_{}",
                        field_use.position.field_ordinal(),
                        field_use.value_kind.field_code().code()
                    ),
                )
                .map_err(|_| CodecError::malformed("NX Parasolid field name formatting failed"))?;
                std::borrow::Cow::Owned(name)
            }
        };
        let name_len = definition
            .name
            .as_str()
            .len()
            .checked_add(1)
            .and_then(|bytes| bytes.checked_add(field_name.len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX Parasolid attribute field name",
                    0,
                    cadmpeg_core::decode::u64_from_index(field_name.len()),
                )
            })?;
        let mut name = ctx.retained_string(name_len, "NX Parasolid attribute field name")?;
        name.push_str(definition.name.as_str());
        name.push('.');
        name.push_str(&field_name);
        Ok(Some(name))
    }
}

fn topology_attribute_name(
    ctx: &DecodeContext<'_>,
    field_name: Option<String>,
    class_name: Option<&str>,
    family: &str,
    reference_ordinal: u32,
) -> Result<String, CodecError> {
    if let Some(name) = field_name {
        return Ok(name);
    }
    let mut remaining = reference_ordinal;
    let mut digits = 1_usize;
    while remaining >= 10 {
        remaining /= 10;
        digits += 1;
    }
    let class_prefix_len = class_name
        .map_or(Some(0), |name| name.len().checked_add(1))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid attribute class prefix",
                0,
                class_name.map_or(0, |name| cadmpeg_core::decode::u64_from_index(name.len())),
            )
        })?;
    let name_len = "parasolid_type_"
        .len()
        .checked_add(family.len())
        .and_then(|bytes| bytes.checked_add("_reference_".len()))
        .and_then(|bytes| bytes.checked_add(digits))
        .and_then(|bytes| bytes.checked_add(class_prefix_len))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid attribute fallback name",
                0,
                cadmpeg_core::decode::u64_from_index(family.len()),
            )
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(name_len),
        "NX Parasolid attribute fallback name",
    )?;
    let mut name = ctx.retained_string(name_len, "NX Parasolid attribute fallback name")?;
    if let Some(class_name) = class_name {
        name.push_str(class_name);
        name.push('.');
    }
    std::fmt::Write::write_fmt(
        &mut name,
        format_args!("parasolid_type_{family}_reference_{reference_ordinal}"),
    )
    .map_err(|_| CodecError::malformed("NX Parasolid attribute fallback name formatting failed"))?;
    Ok(name)
}

/// Records `value` as the sole value for `key`, or `None` once the key repeats.
fn insert_sole<'a, K: Ord, V>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    values: &mut BTreeMap<K, Option<&'a V>>,
    key: K,
    value: &'a V,
) -> Result<(), CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(values.len()),
        "NX Parasolid attribute name lookup",
    )?;
    reservation.with_storage(|| {
        ctx.admit_btree_entry(values, &key, "NX Parasolid attribute name index")
    })?;
    match values.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
        Entry::Occupied(mut entry) => {
            entry.insert(None);
        }
    }
    Ok(())
}

fn parasolid_topology_attribute_class_names<'a>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
) -> Result<BTreeMap<&'a str, Option<&'a str>>, CodecError> {
    let mut classes_by_reference = BTreeMap::<&str, Option<&str>>::new();
    for class_use in class_uses {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(definitions.len()),
            "NX Parasolid class name lookup",
        )?;
        for definition in definitions
            .iter()
            .filter(|definition| definition.id == class_use.attribute_definition)
        {
            let key = class_use.topology_attribute_reference.as_str();
            let name = definition.name.as_str();
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(classes_by_reference.len()),
                "NX Parasolid class name index",
            )?;
            reservation.with_storage(|| {
                ctx.admit_btree_entry(&classes_by_reference, &key, "NX Parasolid class names")
            })?;
            match classes_by_reference.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(Some(name));
                }
                Entry::Occupied(mut entry) => {
                    if entry.get().is_some_and(|existing| existing != name) {
                        entry.insert(None);
                    }
                }
            }
        }
    }
    Ok(classes_by_reference)
}

fn parasolid_topology_attribute_targets(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ir: &CadIr,
) -> Result<BTreeMap<String, AttributeTarget>, CodecError> {
    let mut targets = BTreeMap::new();
    for shell in &ir.model.shells {
        insert_parasolid_topology_target(
            ctx,
            reservation,
            &mut targets,
            shell.id.as_str(),
            || {
                shell
                    .id
                    .try_clone_for_decode(ctx, "NX Parasolid topology target identity")
                    .map(AttributeTarget::Shell)
            },
        )?;
    }
    for face in &ir.model.faces {
        insert_parasolid_topology_target(ctx, reservation, &mut targets, face.id.as_str(), || {
            face.id
                .try_clone_for_decode(ctx, "NX Parasolid topology target identity")
                .map(AttributeTarget::Face)
        })?;
    }
    for loop_ in &ir.model.loops {
        insert_parasolid_topology_target(
            ctx,
            reservation,
            &mut targets,
            loop_.id.as_str(),
            || {
                loop_
                    .id
                    .try_clone_for_decode(ctx, "NX Parasolid topology target identity")
                    .map(AttributeTarget::Loop)
            },
        )?;
    }
    for edge in &ir.model.edges {
        insert_parasolid_topology_target(ctx, reservation, &mut targets, edge.id.as_str(), || {
            edge.id
                .try_clone_for_decode(ctx, "NX Parasolid topology target identity")
                .map(AttributeTarget::Edge)
        })?;
    }
    for coedge in &ir.model.coedges {
        insert_parasolid_topology_target(
            ctx,
            reservation,
            &mut targets,
            coedge.id.as_str(),
            || {
                coedge
                    .id
                    .try_clone_for_decode(ctx, "NX Parasolid topology target identity")
                    .map(AttributeTarget::Coedge)
            },
        )?;
    }
    for vertex in &ir.model.vertices {
        insert_parasolid_topology_target(
            ctx,
            reservation,
            &mut targets,
            vertex.id.as_str(),
            || {
                vertex
                    .id
                    .try_clone_for_decode(ctx, "NX Parasolid topology target identity")
                    .map(AttributeTarget::Vertex)
            },
        )?;
    }
    Ok(targets)
}

fn insert_parasolid_topology_target(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    targets: &mut BTreeMap<String, AttributeTarget>,
    id: &str,
    target: impl FnOnce() -> Result<AttributeTarget, CodecError>,
) -> Result<(), CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(targets.len()),
        "NX Parasolid topology target lookup",
    )?;

    let mut key = String::new();
    reservation.with_storage(|| {
        ctx.try_reserve_retained_text(
            &mut key,
            id.len(),
            "allocate NX Parasolid topology target key",
        )
    })?;
    key.push_str(id);
    let target = reservation.with_storage(target)?;
    reservation.with_storage(|| {
        ctx.insert_btree_map(targets, key, target, "NX Parasolid topology targets")
    })?;
    Ok(())
}

struct ParasolidTopologyAttributeContext<'a> {
    reference: &'a crate::native::parasolid::ParasolidTopologyAttributeListReference,
    entity: &'a str,
    id_suffix: Option<cadmpeg_ir::ids::IdentityKey>,
    target: AttributeTarget,
}

struct ParasolidTopologyAttributeIndex<'a, 'ctx> {
    class_names: BTreeMap<&'a str, Option<&'a str>>,
    attribute_names: ParasolidAttributeNameIndex<'a>,
    contexts: Vec<ParasolidTopologyAttributeContext<'a>>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> ParasolidTopologyAttributeIndex<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        ir: &CadIr,
        topology_references:
            &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
        class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
        definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
        field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
        field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    ) -> Result<Self, CodecError> {
        let mut reservation = ctx.reserve_scoped(0, "NX Parasolid attribute indexes")?;
        let attribute_names = ParasolidAttributeNameIndex::new(
            ctx,
            &mut reservation,
            class_uses,
            definitions,
            field_uses,
            field_names,
        )?;
        let class_names = parasolid_topology_attribute_class_names(
            ctx,
            &mut reservation,
            class_uses,
            definitions,
        )?;
        Ok(Self {
            class_names,
            attribute_names,
            contexts: parasolid_topology_attribute_contexts(
                ctx,
                &mut reservation,
                ir,
                topology_references,
                class_uses,
            )?,
            _reservation: reservation,
        })
    }
}

fn parasolid_topology_attribute_contexts<'a>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ir: &CadIr,
    topology_references: &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
) -> Result<Vec<ParasolidTopologyAttributeContext<'a>>, CodecError> {
    let mut entities_by_reference = BTreeMap::<&str, BTreeSet<&str>>::new();
    for class_use in class_uses {
        let key = class_use.topology_attribute_reference.as_str();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entities_by_reference.len()),
            "NX Parasolid attribute entity lookup",
        )?;
        reservation.with_storage(|| {
            ctx.admit_btree_entry(
                &entities_by_reference,
                &key,
                "NX Parasolid attribute entity groups",
            )
        })?;
        let entities = entities_by_reference.entry(key).or_default();
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entities.len()),
            "NX Parasolid attribute entity uniqueness",
        )?;
        reservation.with_storage(|| {
            ctx.insert_btree_set(
                entities,
                class_use.entity_51_record.as_str(),
                "NX Parasolid attribute group entity",
            )
        })?;
    }
    let mut references_by_target = BTreeMap::<String, Vec<_>>::new();
    for reference in topology_references {
        let kind = reference.topology_type.as_str();
        let key_capacity = kind.len().checked_add(26).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid topology reference key",
                0,
                cadmpeg_core::decode::u64_from_index(kind.len()),
            )
        })?;

        let mut key = String::new();
        reservation.with_storage(|| {
            ctx.try_reserve_retained_text(
                &mut key,
                key_capacity,
                "allocate NX Parasolid topology reference key",
            )
        })?;
        std::fmt::Write::write_fmt(
            &mut key,
            format_args!(
                "nx:s{}:{kind}#{}",
                reference.stream_ordinal, reference.topology_xmt
            ),
        )
        .map_err(|_| {
            CodecError::malformed("NX Parasolid topology reference key formatting failed")
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(references_by_target.len()),
            "NX Parasolid topology reference lookup",
        )?;
        reservation.with_storage(|| {
            ctx.admit_btree_entry(
                &references_by_target,
                &key,
                "NX Parasolid topology reference groups",
            )
        })?;
        ctx.charge_collection_items(1, "NX Parasolid topology reference")?;
        let references = references_by_target.entry(key).or_default();
        reservation.with_storage(|| {
            ctx.reserve_capacity(references, 1, "NX Parasolid topology reference")
        })?;
        references.push(reference);
    }
    let emitted_targets = parasolid_topology_attribute_targets(ctx, reservation, ir)?;
    let mut contexts = Vec::new();
    for (target_key, references) in references_by_target {
        let Some(&reference) = references.first().filter(|_| references.len() == 1) else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(emitted_targets.len()),
            "NX Parasolid emitted target lookup",
        )?;
        let Some(target) = emitted_targets.get(target_key.as_str()) else {
            continue;
        };
        let mut entities = BTreeSet::new();
        if let Some(entity) = reference.attribute_list_record.as_deref() {
            reservation.with_storage(|| {
                ctx.insert_btree_set(&mut entities, entity, "NX Parasolid reference entity")
            })?;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entities_by_reference.len()),
            "NX Parasolid class entity lookup",
        )?;
        if let Some(class_entities) = entities_by_reference.get(reference.id.as_str()) {
            for entity in class_entities {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(entities.len()),
                    "NX Parasolid reference entity uniqueness",
                )?;
                reservation.with_storage(|| {
                    ctx.insert_btree_set(&mut entities, entity, "NX Parasolid reference entity")
                })?;
            }
        }
        let multiple_entities = entities.len() > 1;
        for entity in entities {
            let id_suffix = if multiple_entities {
                entity_suffix_key(ctx, reservation, entity)?
            } else {
                None
            };
            ctx.charge_collection_items(1, "NX Parasolid attribute contexts")?;
            reservation.with_storage(|| {
                ctx.reserve_capacity(&mut contexts, 1, "NX Parasolid attribute contexts")
            })?;
            contexts.push(ParasolidTopologyAttributeContext {
                reference,
                entity,
                id_suffix,
                target: reservation.with_storage(|| {
                    target.try_clone_for_decode(ctx, "NX Parasolid attribute context")
                })?,
            });
        }
    }
    Ok(contexts)
}

fn topology_attribute_id(
    ctx: &DecodeContext<'_>,
    reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
    family: &cadmpeg_ir::ids::IdentityComponent,
    reference_ordinal: u32,
    entity_suffix: Option<&cadmpeg_ir::ids::IdentityKey>,
) -> Result<AttributeId, CodecError> {
    let suffix_len = entity_suffix.map_or(0, |suffix| suffix.as_str().len());
    let id_len = family
        .as_str()
        .len()
        .checked_add(suffix_len)
        .and_then(|bytes| bytes.checked_add(64))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid attribute identity",
                0,
                cadmpeg_core::decode::u64_from_index(suffix_len),
            )
        })?;
    let bytes = id_len.checked_mul(4).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX Parasolid attribute identity",
            0,
            cadmpeg_core::decode::u64_from_index(id_len),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(id_len),
        "NX Parasolid attribute identity",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(bytes),
        "NX Parasolid attribute identity",
    )?;
    let mut key = cadmpeg_ir::ids::IdentityKey::from(reference.topology_type.code())
        .dash(reference.topology_xmt)
        .dash(reference_ordinal);
    if let Some(suffix) = entity_suffix {
        key = key.dash(suffix);
    }
    Ok(IdScope::stream(reference.stream_ordinal).id(family, key))
}

fn single_string_attribute_values(
    ctx: &DecodeContext<'_>,
    text: &str,
) -> Result<Vec<AttributeValue>, CodecError> {
    ctx.charge_collection_items(1, "NX Parasolid string attribute values")?;

    let mut owned = String::new();
    ctx.try_reserve_retained_text(
        &mut owned,
        text.len(),
        "NX Parasolid string attribute value",
    )?;
    owned.push_str(text);
    let mut values = Vec::new();
    ctx.reserve_capacity(&mut values, 1, "NX Parasolid string attribute values")?;
    values.push(AttributeValue::String(owned));
    Ok(values)
}

fn mapped_attribute_values<T>(
    ctx: &DecodeContext<'_>,
    input: &[T],
    map: impl Fn(&T) -> AttributeValue,
) -> Result<Vec<AttributeValue>, CodecError> {
    let mut values = ctx.collection_vec(input.len(), "NX Parasolid numeric attribute values")?;
    values.extend(input.iter().map(map));
    Ok(values)
}

fn mapped_vector_attribute_values<T, const N: usize>(
    ctx: &DecodeContext<'_>,
    input: &[T],
    map: impl Fn(&T) -> [FiniteReal; N],
) -> Result<Vec<AttributeValue>, CodecError> {
    let items = input
        .len()
        .checked_mul(N.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid vector value items",
                0,
                cadmpeg_core::decode::u64_from_index(N),
            )
        })?)
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX Parasolid vector value items",
                0,
                cadmpeg_core::decode::u64_from_index(input.len()),
            )
        })?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(items),
        "NX Parasolid vector value items",
    )?;
    let mut values = Vec::new();
    ctx.reserve_capacity(&mut values, input.len(), "NX Parasolid vector values")?;
    for item in input {
        let mut components = Vec::new();
        ctx.reserve_capacity(&mut components, N, "NX Parasolid vector components")?;
        components.extend(map(item));
        values.push(AttributeValue::Vector(components));
    }
    Ok(values)
}

fn push_topology_attribute(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    context: &ParasolidTopologyAttributeContext<'_>,
    id: AttributeId,
    name: String,
    values: Vec<AttributeValue>,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "NX Parasolid attribute output")?;
    ctx.reserve_capacity(&mut ir.model.attributes, 1, "NX Parasolid attribute output")?;
    ir.model.attributes.push(SourceAttribute {
        id,
        target: context
            .target
            .try_clone_for_decode(ctx, "NX Parasolid attribute output")?,
        name,
        values,
    });
    Ok(())
}

/// The key half of an entity reference, which is what an id suffix names.
///
/// The reference reaches this as stored record text, so it is admitted here;
/// text that is not key text names no suffix.
fn entity_suffix_key(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    entity: &str,
) -> Result<Option<cadmpeg_ir::ids::IdentityKey>, CodecError> {
    let suffix = entity.rsplit_once('#').map_or(entity, |(_, key)| key);
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(suffix.len()),
        "NX Parasolid entity suffix key",
    )?;
    reservation.grow(cadmpeg_core::decode::u64_from_index(suffix.len()))?;
    let Ok(key) = cadmpeg_ir::ids::IdentityKey::try_new(suffix) else {
        return Ok(None);
    };
    Ok(Some(key))
}

fn attach_parasolid_topology_numeric_attributes(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &ParasolidNumericAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_, '_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let (integers_by_id, _integers_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .integers
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid attribute record index",
    )?;
    let (doubles_by_id, _doubles_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .doubles
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid attribute record index",
    )?;
    let (mut uses_by_entity, _uses_by_entity_reservation) = ctx.collect_scoped_btree_groups(
        sources
            .numeric_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
        "NX Parasolid attribute use groups",
    )?;
    for uses in uses_by_entity.values_mut() {
        ctx.stable_sort_by(
            uses,
            |left, right| left.position.cmp(&right.position),
            |_| 0,
            "NX Parasolid numeric attribute ordering",
        )?;
    }
    for context in &attribute_index.contexts {
        let reference = context.reference;
        let entity = context.entity;
        for numeric_use in uses_by_entity.get(entity).into_iter().flatten() {
            let (values, source_offset, tag, lane) = match numeric_use.kind {
                crate::native::parasolid::ParasolidEntity51NumericKind::UnsignedIntegers => {
                    let Some(record) = integers_by_id.get(numeric_use.value_record.as_str()) else {
                        continue;
                    };
                    (
                        mapped_attribute_values(ctx, record.values.as_slice(), |value| {
                            AttributeValue::Integer(i64::from(*value))
                        })?,
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
                        mapped_attribute_values(ctx, record.values.as_slice(), |value| {
                            AttributeValue::Float(*value)
                        })?,
                        record.inflated_offset,
                        "ENTITY_53_DOUBLE_ATTRIBUTE",
                        "double",
                    )
                }
            };
            let id = topology_attribute_id(
                ctx,
                reference,
                &cadmpeg_ir::identity_component!("topology-numeric-attribute"),
                numeric_use.position.reference_ordinal(),
                context.id_suffix.as_ref(),
            )?;
            let source_stream = StreamHandle::new(
                ctx,
                cadmpeg_ir::stream_name!("nx:s").with_suffix(
                    ctx,
                    reference.stream_ordinal,
                    "compose annotation stream name",
                )?,
                "allocate annotation stream handle",
            )?;
            annotations.note(ctx, id.as_str(), &source_stream, source_offset, Some(tag))?;
            annotations
                .derived(ctx, id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(ctx, id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::from)?;
            let field_name = attribute_index.attribute_names.field_name(
                ctx,
                reference,
                numeric_use.id.as_str(),
            )?;
            let name = topology_attribute_name(
                ctx,
                field_name,
                attribute_index
                    .class_names
                    .get(reference.id.as_str())
                    .and_then(Option::as_ref)
                    .copied(),
                lane,
                numeric_use.position.reference_ordinal(),
            )?;
            push_topology_attribute(ctx, ir, context, id, name, values)?;
        }
    }
    ctx.stable_sort_by(
        &mut ir.model.attributes,
        |first, second| first.id.as_str().cmp(second.id.as_str()),
        |attribute| attribute.id.as_str().len(),
        "sort NX Parasolid numeric attributes",
    )?;
    Ok(())
}

struct ParasolidStructuredAttributeSources<'a> {
    structured_uses: &'a [crate::native::parasolid::ParasolidEntity51StructuredUse],
    vectors: &'a [crate::native::parasolid::ParasolidEntityVectorRecord],
    axes: &'a [crate::native::parasolid::ParasolidEntity57AxisRecord],
    tags: &'a [crate::native::parasolid::ParasolidEntity58TagRecord],
    unicode: &'a [crate::native::parasolid::ParasolidEntity62UnicodeRecord],
}

fn attach_parasolid_topology_structured_attributes(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &ParasolidStructuredAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_, '_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let (vectors_by_id, _vectors_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .vectors
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid attribute record index",
    )?;
    let (axes_by_id, _axes_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .axes
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid attribute record index",
    )?;
    let (tags_by_id, _tags_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .tags
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid attribute record index",
    )?;
    let (unicode_by_id, _unicode_by_id_reservation) = ctx.collect_scoped_btree_map(
        sources
            .unicode
            .iter()
            .map(|record| (record.id.as_str(), record)),
        "NX Parasolid attribute record index",
    )?;
    let (mut uses_by_entity, _uses_by_entity_reservation) = ctx.collect_scoped_btree_groups(
        sources
            .structured_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
        "NX Parasolid attribute use groups",
    )?;
    for uses in uses_by_entity.values_mut() {
        ctx.stable_sort_by(
            uses,
            |left, right| left.position.cmp(&right.position),
            |_| 0,
            "NX Parasolid structured attribute ordering",
        )?;
    }
    for context in &attribute_index.contexts {
        let reference = context.reference;
        let entity = context.entity;
        for structured_use in uses_by_entity.get(entity).into_iter().flatten() {
            use crate::native::parasolid::structured_value_kind::StructuredValueKind as Kind;
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
                        mapped_vector_attribute_values(ctx, record.values.as_slice(), |value| {
                            value.finite_components()
                        })?,
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
                        mapped_vector_attribute_values(ctx, record.values.as_slice(), |axis| {
                            let first = axis[0].finite_components();
                            let second = axis[1].finite_components();
                            [
                                first[0], first[1], first[2], second[0], second[1], second[2],
                            ]
                        })?,
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
                        mapped_attribute_values(ctx, record.values.as_slice(), |value| {
                            AttributeValue::Integer(i64::from(*value))
                        })?,
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
                        single_string_attribute_values(ctx, record.value.as_str())?,
                        record.inflated_offset,
                        "ENTITY_62_UNICODE_ATTRIBUTE",
                        "98_unicode",
                    )
                }
            };
            let id = topology_attribute_id(
                ctx,
                reference,
                &cadmpeg_ir::identity_component!("topology-structured-attribute"),
                structured_use.position.reference_ordinal(),
                context.id_suffix.as_ref(),
            )?;
            let source_stream = StreamHandle::new(
                ctx,
                cadmpeg_ir::stream_name!("nx:s").with_suffix(
                    ctx,
                    reference.stream_ordinal,
                    "compose annotation stream name",
                )?,
                "allocate annotation stream handle",
            )?;
            annotations.note(ctx, id.as_str(), &source_stream, source_offset, Some(tag))?;
            annotations
                .derived(ctx, id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(ctx, id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::from)?;
            let field_name = attribute_index.attribute_names.field_name(
                ctx,
                reference,
                structured_use.id.as_str(),
            )?;
            let name = topology_attribute_name(
                ctx,
                field_name,
                attribute_index
                    .class_names
                    .get(reference.id.as_str())
                    .and_then(Option::as_ref)
                    .copied(),
                family,
                structured_use.position.reference_ordinal(),
            )?;
            push_topology_attribute(ctx, ir, context, id, name, values)?;
        }
    }
    ctx.stable_sort_by(
        &mut ir.model.attributes,
        |first, second| first.id.as_str().cmp(second.id.as_str()),
        |attribute| attribute.id.as_str().len(),
        "sort NX Parasolid structured attributes",
    )?;
    Ok(())
}

fn push_referenced_parameter(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    referenced: &mut Vec<ParameterId>,
    expression: &str,
) -> Result<(), CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(expression.len()),
        "NX referenced parameter identity",
    )?;
    let bytes = expression.len();
    ctx.charge_collection_items(1, "NX referenced parameters")?;
    reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
    let Some(id) = expressions::expression_parameter_id(expression) else {
        return Ok(());
    };
    reservation.with_storage(|| ctx.reserve_capacity(referenced, 1, "NX referenced parameters"))?;
    referenced.push(id);
    Ok(())
}

fn push_unique_feature_dependency(
    ctx: &DecodeContext<'_>,
    dependencies: &mut Vec<FeatureId>,
    candidate: &FeatureId,
) -> Result<(), CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(dependencies.len()),
        "NX feature dependency uniqueness",
    )?;
    if dependencies.contains(candidate) {
        return Ok(());
    }
    ctx.charge_collection_items(1, "NX feature dependencies")?;
    ctx.reserve_capacity(dependencies, 1, "NX feature dependency")?;
    dependencies.push(candidate.try_clone_for_decode(ctx, "NX feature dependency")?);
    Ok(())
}

fn preceding_operation_dependency<'a>(
    operation: &str,
    consumer_position: usize,
    operation_positions: &BTreeMap<&str, usize>,
    feature_ids: &'a BTreeMap<&str, FeatureId>,
) -> Option<&'a FeatureId> {
    let position = operation_positions.get(operation)?;
    if *position >= consumer_position {
        return None;
    }
    feature_ids.get(operation)
}

fn projects_neutral_feature(label: &str) -> bool {
    !matches!(label, "Container" | "TEXT")
}

fn text_semantic_annotation(
    ctx: &DecodeContext<'_>,
    native_ref: &str,
    order: u32,
    payload_strings: &[&str],
) -> Result<Option<SemanticAnnotation>, CodecError> {
    const FONT_KEY: &str = "font_family";
    const ID_SUFFIX: &str = ":semantic-text";
    let [text, font_family] = payload_strings else {
        return Ok(None);
    };
    let id_len = native_ref
        .len()
        .checked_add(ID_SUFFIX.len())
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX TEXT annotation identity",
                0,
                cadmpeg_core::decode::u64_from_index(native_ref.len()),
            )
        })?;

    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(native_ref.len()),
        "NX TEXT annotation identity",
    )?;
    ctx.charge_collection_items(2, "NX TEXT annotation record, text and parameter")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(id_len),
        "NX TEXT annotation",
    )?;
    let _identity_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(id_len),
        "NX TEXT annotation identity assembly",
    )?;
    let Some(id) = extended_id(native_ref, &cadmpeg_ir::identity_key!("semantic-text")) else {
        return Ok(None);
    };
    let copy = |source: &str| -> Result<String, CodecError> {
        let mut owned = String::new();
        ctx.try_reserve_retained_text(
            &mut owned,
            source.len(),
            "allocate NX TEXT annotation text",
        )?;
        owned.push_str(source);
        Ok(owned)
    };
    let mut text_values = Vec::new();
    ctx.reserve_capacity(&mut text_values, 1, "allocate NX TEXT annotation text list")?;
    text_values.push(copy(text)?);
    let key = cadmpeg_core::text::NonBlankString::new(copy(FONT_KEY)?)
        .ok_or_else(|| CodecError::malformed("NX TEXT annotation font key is blank"))?;
    let mut parameters = BTreeMap::new();
    ctx.insert_btree_map(
        &mut parameters,
        key,
        copy(font_family)?,
        "NX TEXT annotation record, text and parameter",
    )?;
    Ok(Some(SemanticAnnotation {
        id,
        object: copy(native_ref)?,
        kind: SemanticAnnotationKind::Text,
        runtime_type: copy("TEXT")?,
        order,
        text: text_values,
        references: BTreeMap::new(),
        value: None,
        format: None,
        position: None,
        parameters,
        assets: Vec::new(),
        native_ref: copy(native_ref)?,
    }))
}

pub(super) fn parameter_owner_dependencies(
    ctx: &DecodeContext<'_>,
    parameter_owners: &BTreeMap<ParameterId, Option<FeatureId>>,
    parameter_references: &[ParameterId],
) -> Result<Vec<FeatureId>, CodecError> {
    let mut dependencies = Vec::new();
    for parameter_id in parameter_references {
        let work = parameter_owners
            .len()
            .checked_add(dependencies.len())
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "NX parameter owner dependency scan",
                    0,
                    cadmpeg_core::decode::u64_from_index(parameter_owners.len()),
                )
            })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(work),
            "NX parameter owner dependency scan",
        )?;
        let Some(owner) = parameter_owners.get(parameter_id).and_then(Option::as_ref) else {
            continue;
        };
        if !dependencies.contains(owner) {
            ctx.charge_collection_items(1, "NX parameter owner dependencies")?;
            ctx.reserve_capacity(&mut dependencies, 1, "NX parameter owner dependency")?;
            dependencies.push(owner.try_clone_for_decode(ctx, "NX parameter owner dependency")?);
        }
    }
    Ok(dependencies)
}

fn extrude_feature_definition(
    construction_profile: Option<&str>,
    structured_construction: Option<&str>,
    op: BooleanOp,
    output_kinds: &[cadmpeg_ir::topology::BodyKind],
) -> FeatureDefinition {
    let profile = match (construction_profile, structured_construction) {
        (Some(construction), None) | (None, Some(construction)) => {
            ProfileRef::Planar(PlanarProfileRef::Native(construction.to_string()))
        }
        _ => ProfileRef::Planar(PlanarProfileRef::Unresolved("EXTRUDE".to_string())),
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
    FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile,
        direction: cadmpeg_ir::features::ExtrudeDirection::Unresolved {},
        start: cadmpeg_ir::features::ExtrudeStart::Unresolved {},
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Unresolved {},
                draft: None,
            },
        },
        op,
        solid,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    })
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

pub(super) fn boolean_feature_definition(
    ctx: &DecodeContext<'_>,
    operation: &crate::native::features::FeatureBooleanOperation,
    body_alias_roots: &BTreeMap<u32, u32>,
    offset_store_resolution: &BooleanOffsetStoreResolution,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<FeatureDefinition, CodecError> {
    let empty_offset_store_body_blocks = BTreeMap::new();
    let native_target = ctx.format_retained(
        format_args!("nx:om-object-index#{}", operation.target.token.value()),
        "NX feature projection text",
    )?;
    let native_tools =
        selection_indices_native(ctx, operation.tools.iter().map(|token| token.token.value()))?;
    let offset_store_body_blocks = match offset_store_resolution {
        BooleanOffsetStoreResolution::Unresolved => None,
        BooleanOffsetStoreResolution::None => Some(&empty_offset_store_body_blocks),
        BooleanOffsetStoreResolution::Complete(blocks) => Some(blocks),
    };
    let (target, tools) = match offset_store_body_blocks {
        None => (
            BodySelection::Native(native_target),
            BodySelection::Native(native_tools),
        ),
        Some(offset_store_body_blocks) => {
            let mut tool_indices = Vec::new();
            let mut reservation = ctx.reserve_scoped(0, "NX Boolean tool indices")?;
            for tool in &operation.tools {
                ctx.reserve_scoped_vec(
                    &mut reservation,
                    &mut tool_indices,
                    1,
                    "NX Boolean tool indices",
                )?;
                tool_indices.push(tool.token.value());
            }
            atomic_disjoint_body_selections(
                ctx,
                feature_body_selection_with_offset_blocks(
                    ctx,
                    &[operation.target.token.value()],
                    body_alias_roots,
                    offset_store_body_blocks,
                    bodies_by_object_index,
                    native_target,
                )?,
                feature_body_selection_with_offset_blocks(
                    ctx,
                    &tool_indices,
                    body_alias_roots,
                    offset_store_body_blocks,
                    bodies_by_object_index,
                    native_tools,
                )?,
            )?
        }
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Combine {
        operands: cadmpeg_ir::features::CombineOperands::new(target, tools, ctx)?
            .map_err(cadmpeg_core::CodecError::malformed)?,

        op: match operation.kind {
            crate::native::features::FeatureBooleanKind::Unite => {
                cadmpeg_ir::features::BooleanKind::Join
            }
            crate::native::features::FeatureBooleanKind::Subtract => {
                cadmpeg_ir::features::BooleanKind::Cut
            }
            crate::native::features::FeatureBooleanKind::Intersect => {
                cadmpeg_ir::features::BooleanKind::Intersect
            }
        },
        keep_tools: false,
    }))
}

#[derive(Clone, Copy)]
enum DeleteBodyField<'a> {
    Native(u32),
    OffsetStore {
        object_index: u32,
        data_block: &'a str,
    },
}

/// Project `DELETE` as body deletion only when its bounded operation record
/// carries a primary-body field. Other `DELETE` payloads target a different
/// object family and remain native until that family is decoded.
fn delete_body_feature_definition(
    ctx: &DecodeContext<'_>,
    field: DeleteBodyField<'_>,
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<FeatureDefinition, CodecError> {
    let bodies = match field {
        DeleteBodyField::Native(body) => match feature_body_selection(
            ctx,
            &[body],
            body_alias_roots,
            bodies_by_object_index,
            ctx.format_retained(
                format_args!("nx:om-object-index#{body}"),
                "NX feature projection text",
            )?,
        )? {
            FeatureBodySelection::Native(native) => {
                let mut reservation = ctx.reserve_scoped(0, "NX DELETE local body")?;
                let mut bodies = Vec::new();
                ctx.reserve_scoped_vec(&mut reservation, &mut bodies, 1, "NX DELETE local body")?;
                bodies.push(ctx.format_scoped_text(
                    &mut reservation,
                    format_args!("nx:om-body-object#{body}"),
                    "NX body selection text",
                )?);
                local_body_selection(ctx, &bodies, native)?
            }
            selection => selection.into_selection(ctx)?,
        },
        DeleteBodyField::OffsetStore {
            object_index,
            data_block,
        } => {
            let mut reservation = ctx.reserve_scoped(0, "NX DELETE offset body")?;
            let mut bodies = Vec::new();
            ctx.reserve_scoped_vec(&mut reservation, &mut bodies, 1, "NX DELETE offset body")?;
            bodies.push(ctx.format_scoped_text(
                &mut reservation,
                format_args!("{data_block}"),
                "NX body selection text",
            )?);
            local_body_selection(
                ctx,
                &bodies,
                ctx.format_retained(
                    format_args!("nx:om-object-index#{object_index}"),
                    "NX feature projection text",
                )?,
            )?
        }
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        // A typed DELETE primary-body field names one exact feature input. It
        // needs no cross-selection alias proof when it has no segment binding.
        bodies,
        mode: BodyRetentionMode::DeleteSelected,
    }))
}

/// Project the exact source body of an `EXTRACT_BODY` operation.
fn extract_body_feature_definition(
    ctx: &DecodeContext<'_>,
    body_object_index: Option<u32>,
    offset_store_bodies: &[(u32, String)],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<FeatureDefinition, CodecError> {
    let source = if let Some(body) = body_object_index {
        feature_body_selection(
            ctx,
            &[body],
            body_alias_roots,
            bodies_by_object_index,
            ctx.format_retained(
                format_args!("nx:om-object-index#{body}"),
                "NX feature projection text",
            )?,
        )?
        .into_selection(ctx)?
    } else if let [(object_index, data_block)] = offset_store_bodies {
        let mut reservation = ctx.reserve_scoped(0, "NX EXTRACT local body")?;
        let mut bodies = Vec::new();
        ctx.reserve_scoped_vec(&mut reservation, &mut bodies, 1, "NX EXTRACT local body")?;
        bodies.push(ctx.format_scoped_text(
            &mut reservation,
            format_args!("{data_block}"),
            "NX body selection text",
        )?);
        local_body_selection(
            ctx,
            &bodies,
            ctx.format_retained(
                format_args!("nx:om-object-index#{object_index}"),
                "NX feature projection text",
            )?,
        )?
    } else {
        BodySelection::Unresolved
    };
    Ok(FeatureDefinition::Operation(
        FeatureOperation::ExtractBody { source },
    ))
}

/// Project exact feature-local input-store identities for a trim target and
/// every complete, distinct tool operand. Retained-side semantics stay unresolved.
fn offset_store_trim_body_feature_definition(
    ctx: &DecodeContext<'_>,
    offset_store_bodies: &[(u32, String)],
    operands: &[&crate::native::features::FeatureOperationBodyOperand],
) -> Result<Option<FeatureDefinition>, CodecError> {
    let [(object_index, data_block)] = offset_store_bodies else {
        return Ok(None);
    };
    let primary_store = data_block.rsplit_once(":block#").map(|(store, _)| store);
    let mut tool_data_blocks = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX trim offset tool blocks")?;
    let mut complete = true;
    for operand in operands {
        let Some(block) = operand.operand_data_block.as_deref() else {
            complete = false;
            break;
        };
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut tool_data_blocks,
            1,
            "NX trim offset tool blocks",
        )?;
        tool_data_blocks.push(block);
    }
    let tools = if operands.is_empty() || primary_store.is_none() || !complete {
        BodySelection::Unresolved
    } else {
        let primary_store = primary_store
            .ok_or_else(|| ctx.refuse_codec_limit("NX trim offset primary store", 0, 1))?;
        let work = operands.len().checked_mul(operands.len()).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX trim offset uniqueness",
                0,
                cadmpeg_core::decode::u64_from_index(operands.len()),
            )
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(work),
            "NX trim offset uniqueness",
        )?;
        let distinct_operand_indices = operands.iter().enumerate().all(|(index, operand)| {
            !operands[..index]
                .iter()
                .any(|other| other.operand.atom.value() == operand.operand.atom.value())
        });
        let same_store = tool_data_blocks.iter().all(|block| {
            block
                .rsplit_once(":block#")
                .is_some_and(|(store, _)| store == primary_store)
        });
        let distinct_tool_blocks = tool_data_blocks
            .iter()
            .enumerate()
            .all(|(index, block)| !tool_data_blocks[..index].contains(block));
        let no_target_alias = tool_data_blocks.iter().all(|block| *block != data_block);
        if operands.iter().all(|operand| {
            operand.body_object_index == *object_index
                && operand.operand.atom.value() != *object_index
        }) && distinct_operand_indices
            && same_store
            && distinct_tool_blocks
            && no_target_alias
        {
            let mut bodies = Vec::new();
            for block in tool_data_blocks {
                ctx.reserve_scoped_vec(
                    &mut reservation,
                    &mut bodies,
                    1,
                    "NX trim local tool bodies",
                )?;
                bodies.push(ctx.format_scoped_text(
                    &mut reservation,
                    format_args!("{block}"),
                    "NX body selection text",
                )?);
            }
            let native = selection_indices_native(
                ctx,
                operands.iter().map(|operand| operand.operand.atom.value()),
            )?;
            local_body_selection(ctx, &bodies, native)?
        } else {
            BodySelection::Unresolved
        }
    };
    let mut target = Vec::new();
    ctx.reserve_scoped_vec(
        &mut reservation,
        &mut target,
        1,
        "NX trim local target body",
    )?;
    target.push(ctx.format_scoped_text(
        &mut reservation,
        format_args!("{data_block}"),
        "NX body selection text",
    )?);
    let target = local_body_selection(
        ctx,
        &target,
        ctx.format_retained(
            format_args!("nx:om-object-index#{object_index}"),
            "NX feature projection text",
        )?,
    )?;
    let Ok(operands) = cadmpeg_ir::features::TrimBodyOperands::new(target, tools, ctx)? else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::TrimBodies {
            operands,
            keep: BodyTrimSide::Unresolved,
        },
    )))
}

fn sew_body_feature_definition(
    ctx: &DecodeContext<'_>,
    primary_segment_body_object_index: Option<u32>,
    offset_store_bodies: &[(u32, String)],
    operands: &[&crate::native::features::FeatureOperationBodyOperand],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
    }
    let primary_offset_store_body = match offset_store_bodies {
        [(object_index, data_block)] => Some((*object_index, data_block.as_str())),
        _ => None,
    };
    let Some(primary_body_object_index) = primary_segment_body_object_index
        .or_else(|| primary_offset_store_body.map(|(object_index, _)| object_index))
    else {
        return Ok(None);
    };
    let indices = std::iter::once(primary_body_object_index)
        .chain(operands.iter().map(|operand| operand.operand.atom.value()));
    let native = selection_indices_native(ctx, indices.clone())?;
    let mut object_indices = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX sew body indices")?;
    for index in indices {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut object_indices,
            1,
            "NX sew body indices",
        )?;
        object_indices.push(index);
    }
    let bodies = if primary_segment_body_object_index.is_some() {
        if operands
            .iter()
            .all(|operand| !operand.segment_body_bindings.is_empty())
        {
            feature_body_set_selection(
                ctx,
                &object_indices,
                body_alias_roots,
                bodies_by_object_index,
                native,
            )?
        } else {
            BodySelection::Native(native)
        }
    } else if let Some((primary_object_index, primary_data_block)) = primary_offset_store_body {
        let primary_store = primary_data_block
            .rsplit_once(":block#")
            .map(|(store, _)| store);
        let mut blocks = Vec::new();
        let mut complete = true;
        for operand in operands {
            let Some(block) = operand.operand_data_block.as_deref() else {
                complete = false;
                break;
            };
            ctx.reserve_scoped_vec(&mut reservation, &mut blocks, 1, "NX sew offset blocks")?;
            blocks.push(block);
        }
        let work = blocks.len().checked_mul(blocks.len()).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "NX sew offset blocks",
                0,
                cadmpeg_core::decode::u64_from_index(blocks.len()),
            )
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(work),
            "NX sew offset uniqueness",
        )?;
        let valid = complete
            && operands
                .iter()
                .all(|operand| operand.body_object_index == primary_object_index)
            && blocks.iter().all(|block| {
                block
                    .rsplit_once(":block#")
                    .is_some_and(|(store, _)| Some(store) == primary_store)
            })
            && blocks
                .iter()
                .enumerate()
                .all(|(index, block)| !blocks[..index].contains(block))
            && !blocks.contains(&primary_data_block);
        if valid {
            let mut bodies = Vec::new();
            for block in std::iter::once(primary_data_block).chain(blocks) {
                ctx.reserve_scoped_vec(&mut reservation, &mut bodies, 1, "NX sew local bodies")?;
                bodies.push(ctx.format_scoped_text(
                    &mut reservation,
                    format_args!("{block}"),
                    "NX body selection text",
                )?);
            }
            local_body_selection(ctx, &bodies, native)?
        } else {
            BodySelection::Native(native)
        }
    } else {
        BodySelection::Native(native)
    };
    let Ok(bodies) = bodies.try_into() else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SewBodies {
            bodies,
            gap_tolerance: None,
        },
    )))
}

fn trim_body_feature_definition(
    ctx: &DecodeContext<'_>,
    target_object_index: u32,
    operands: &[&crate::native::features::FeatureOperationBodyOperand],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<FeatureDefinition, CodecError> {
    let native_target = ctx.format_retained(
        format_args!("nx:om-object-index#{target_object_index}"),
        "NX feature projection text",
    )?;
    if operands.is_empty() {
        return Ok(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                feature_body_selection(
                    ctx,
                    &[target_object_index],
                    body_alias_roots,
                    bodies_by_object_index,
                    native_target,
                )?
                .into_selection(ctx)?,
                BodySelection::Unresolved,
                ctx,
            )?
            .map_err(cadmpeg_core::CodecError::malformed)?,

            keep: BodyTrimSide::Unresolved,
        }));
    }
    let native_tools = selection_indices_native(
        ctx,
        operands.iter().map(|operand| operand.operand.atom.value()),
    )?;
    let mut tool_object_indices = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX trim tool indices")?;
    for operand in operands {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut tool_object_indices,
            1,
            "NX trim tool indices",
        )?;
        tool_object_indices.push(operand.operand.atom.value());
    }
    if operands.iter().any(|operand| {
        operand.operand_data_block.is_some() || operand.segment_body_bindings.is_empty()
    }) {
        return Ok(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Native(native_target),
                BodySelection::Native(native_tools),
                ctx,
            )?
            .map_err(cadmpeg_core::CodecError::malformed)?,

            keep: BodyTrimSide::Unresolved,
        }));
    }
    let (targets, tools) = atomic_disjoint_body_selections(
        ctx,
        feature_body_selection(
            ctx,
            &[target_object_index],
            body_alias_roots,
            bodies_by_object_index,
            native_target,
        )?,
        feature_body_selection(
            ctx,
            &tool_object_indices,
            body_alias_roots,
            bodies_by_object_index,
            native_tools,
        )?,
    )?;
    Ok(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
        operands: cadmpeg_ir::features::TrimBodyOperands::new(targets, tools, ctx)?
            .map_err(cadmpeg_core::CodecError::malformed)?,

        keep: BodyTrimSide::Unresolved,
    }))
}

fn feature_body_outputs(
    ctx: &DecodeContext<'_>,
    object_index: u32,
    segment_bindings: &[crate::native::segments::SegmentBodyBinding],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<Vec<BodyId>, CodecError> {
    if crate::native::segments::unique_segment_body_binding(object_index, segment_bindings)
        .is_none()
    {
        return Ok(Vec::new());
    }
    let Some([body]) = bodies_by_object_index.get(&object_index).map(Vec::as_slice) else {
        return Ok(Vec::new());
    };
    ctx.charge_collection_items(1, "NX feature body output")?;
    let mut outputs = Vec::new();
    ctx.reserve_capacity(&mut outputs, 1, "NX feature body output")?;
    outputs.push(body.try_clone_for_decode(ctx, "NX feature body output")?);
    Ok(outputs)
}

fn operation_body_image_outputs_by_write<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    uses: &'a [crate::native::features::FeatureOperationBodyImageSegmentUse],
    bodies_by_segment_binding: &BTreeMap<&str, Vec<BodyId>>,
) -> Result<
    (
        BTreeMap<&'a str, BodyId>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut unique_uses = BTreeMap::new();
    let mut reservation = ctx.reserve_scoped(0, "NX body image output indexes")?;
    for use_ in uses {
        ctx.charge_work(1, "NX body image unique-use index")?;
        let write_key = use_.operation_body_write.as_str();
        reservation.with_storage(|| {
            ctx.admit_btree_entry(&unique_uses, &write_key, "NX body image unique-use index")
        })?;
        match unique_uses.entry(write_key) {
            Entry::Vacant(entry) => {
                entry.insert(Some(use_));
            }
            Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
    }
    let mut outputs = BTreeMap::new();
    for (write, use_) in unique_uses {
        let Some(use_) = use_ else {
            continue;
        };
        let Some([body]) = bodies_by_segment_binding
            .get(use_.segment_body_binding.as_str())
            .map(Vec::as_slice)
        else {
            continue;
        };
        ctx.charge_work(1, "NX body image outputs")?;
        let entry = (
            write,
            reservation.with_storage(|| body.try_clone_for_decode(ctx, "NX body image outputs"))?,
        );
        reservation.with_storage(|| {
            ctx.insert_btree_map(&mut outputs, entry.0, entry.1, "NX body image outputs")
        })?;
    }
    Ok((outputs, reservation))
}

fn merge_operation_body_outputs<'a>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    outputs: &mut BTreeMap<&'a str, BodyId>,
    conflicts: &mut BTreeSet<&'a str>,
    candidates: &BTreeMap<&'a str, BodyId>,
) -> Result<(), CodecError> {
    for (&write, body) in candidates {
        ctx.charge_work(1, "NX body output candidate merge")?;
        if conflicts.contains(write) {
            continue;
        }
        reservation
            .with_storage(|| ctx.admit_btree_entry(outputs, &write, "NX merged body output"))?;
        match outputs.entry(write) {
            Entry::Vacant(entry) => {
                entry
                    .insert(reservation.with_storage(|| {
                        body.try_clone_for_decode(ctx, "NX merged body output")
                    })?);
            }
            Entry::Occupied(entry) if entry.get() == body => {}
            Entry::Occupied(entry) => {
                entry.remove();

                reservation.with_storage(|| {
                    ctx.insert_btree_set(conflicts, write, "NX conflicting body output")
                })?;
            }
        }
    }
    Ok(())
}

fn operation_body_identity_outputs_by_write<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    uses: &'a [crate::native::features::FeatureOperationBodyIdentitySegmentUse],
    bodies_by_segment_binding: &BTreeMap<&str, Vec<BodyId>>,
) -> Result<
    (
        BTreeMap<&'a str, BodyId>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut outputs = BTreeMap::new();
    let mut reservation = ctx.reserve_scoped(0, "NX body identity output index")?;
    for use_ in uses {
        ctx.charge_work(1, "NX body identity output lookup")?;
        let Some([body]) = bodies_by_segment_binding
            .get(use_.segment_body_binding.as_str())
            .map(Vec::as_slice)
        else {
            continue;
        };
        let write_key = use_.operation_body_write.as_str();
        reservation.with_storage(|| {
            ctx.admit_btree_entry(&outputs, &write_key, "NX body identity output index")
        })?;
        match outputs.entry(write_key) {
            Entry::Vacant(entry) => {
                entry.insert(reservation.with_storage(|| {
                    body.try_clone_for_decode(ctx, "NX body identity output index")
                })?);
            }
            Entry::Occupied(entry) if entry.get() == body => {}
            Entry::Occupied(entry) => {
                entry.remove();
            }
        }
    }
    Ok((outputs, reservation))
}

fn operation_body_group_partition_outputs_by_write<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    writes: &'a [crate::native::features::FeatureOperationBodyWrite],
    uses: &[crate::native::features::FeatureBodyWriteGroupPartitionUse],
    bodies: &[cadmpeg_ir::topology::Body],
) -> Result<
    (
        BTreeMap<&'a str, BodyId>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut partitions_by_identity = BTreeMap::<u8, Option<u32>>::new();
    let mut reservation = ctx.reserve_scoped(0, "NX body partition output indexes")?;
    for use_ in uses {
        ctx.charge_work(1, "NX body partition identity index")?;
        reservation.with_storage(|| {
            ctx.admit_btree_entry(
                &partitions_by_identity,
                &use_.body_identity,
                "NX body partition identity index",
            )
        })?;
        match partitions_by_identity.entry(use_.body_identity) {
            Entry::Vacant(entry) => {
                entry.insert(Some(use_.partition_stream_ordinal));
            }
            Entry::Occupied(mut entry)
                if entry
                    .get()
                    .is_some_and(|partition| partition != use_.partition_stream_ordinal) =>
            {
                entry.insert(None);
            }
            Entry::Occupied(_) => {}
        }
    }
    let mut unique_bodies = BTreeMap::new();
    for (identity, partition) in partitions_by_identity {
        let Some(partition) = partition else {
            continue;
        };
        let (prefix, prefix_len) = stream_prefix(partition, true)?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(bodies.len()),
            "NX body partition prefix scan",
        )?;
        let mut matches = bodies.iter().filter(|body| {
            body.id
                .as_str()
                .as_bytes()
                .starts_with(&prefix[..prefix_len])
        });
        let Some(body) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        ctx.charge_work(1, "NX unique partition body")?;
        let entry = (
            identity,
            reservation.with_storage(|| {
                body.id
                    .try_clone_for_decode(ctx, "NX unique partition body")
            })?,
        );
        reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut unique_bodies,
                entry.0,
                entry.1,
                "NX unique partition body",
            )
        })?;
    }
    let mut outputs = BTreeMap::new();
    for write in writes {
        ctx.charge_work(1, "NX partition body write lookup")?;
        if let Some(body) = unique_bodies.get(&write.frame.body_identity()) {
            ctx.charge_work(1, "NX partition body output")?;
            let entry = (
                write.id.as_str(),
                reservation
                    .with_storage(|| body.try_clone_for_decode(ctx, "NX partition body output"))?,
            );
            reservation.with_storage(|| {
                ctx.insert_btree_map(&mut outputs, entry.0, entry.1, "NX partition body output")
            })?;
        }
    }
    Ok((outputs, reservation))
}

fn complete_operation_body_image_outputs(
    ctx: &DecodeContext<'_>,
    writes: &[&crate::native::features::FeatureOperationBodyWrite],
    outputs_by_write: &BTreeMap<&str, BodyId>,
) -> Result<Vec<BodyId>, CodecError> {
    let mut outputs = Vec::new();
    for write in writes {
        ctx.charge_work(1, "NX complete body image output lookup")?;
        let Some(body) = outputs_by_write.get(write.id.as_str()) else {
            return Ok(Vec::new());
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(outputs.len()),
            "NX complete body image output uniqueness",
        )?;
        if outputs.contains(body) {
            return Ok(Vec::new());
        }
        ctx.charge_collection_items(1, "NX complete body image output")?;
        ctx.reserve_capacity(&mut outputs, 1, "NX complete body image output")?;
        outputs.push(body.try_clone_for_decode(ctx, "NX complete body image output")?);
    }
    Ok(outputs)
}

fn body_writes_match_boolean_target(
    writes: &[&crate::native::features::FeatureOperationBodyWrite],
    boolean: Option<&crate::native::features::FeatureBooleanOperation>,
) -> bool {
    let Some(boolean) = boolean else {
        return true;
    };
    if writes.is_empty() {
        return true;
    }
    let [write] = writes else {
        return false;
    };
    write.frame.body_image().value() == boolean.target.token.value()
        && !boolean
            .tools
            .iter()
            .any(|token| token.token.value() == write.frame.body_image().value())
}

#[cfg(test)]
mod tests;
