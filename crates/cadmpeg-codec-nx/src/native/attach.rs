// SPDX-License-Identifier: Apache-2.0
//! IR-writing attachment of the native object model.

use crate::loss::NxLossCode;
#[cfg(test)]
use cadmpeg_core::decode::cost::DecodeCost;
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
    scalar::{Angle, Length},
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

pub(super) mod body_selection;
pub(super) mod expressions;
pub(super) mod feature_projection;
mod topology_attributes;

use topology_attributes::{
    attach_parasolid_topology_numeric_attributes, attach_parasolid_topology_string_attributes,
    attach_parasolid_topology_structured_attributes, ParasolidNumericAttributeSources,
    ParasolidStringAttributeSources, ParasolidStructuredAttributeSources,
    ParasolidTopologyAttributeIndex,
};

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
    active_feature_closure, BodyWriterHistory, NATIVE_PRIMARY_BODY_CLOSURE_WITNESS,
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
    for (ordinal, entry) in ctx
        .admit_iter(
            &scan.container.entries,
            "NX opaque container entry traversal",
        )?
        .enumerate()
    {
        let content = entry.content();
        if !content.retains_opaque_payload()
            || (typed_native == TypedNative::Available
                && content == EntryContent::SaveToggleInfo
                && has_complete_saved_toggle_stream(ctx, &scan.container)?)
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
    let (object_sections, _object_sections_storage) = scan.container.indexed_om_sections(ctx)?;
    for (section_index, (entry, section)) in ctx
        .admit_iter(&object_sections, "NX indexed unknown section traversal")?
        .enumerate()
    {
        let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
        match &section.store {
            crate::om::IndexedStore::Fixed { records } => {
                for (record_index, record) in ctx
                    .admit_iter(records.as_ref(), "NX fixed unknown record traversal")?
                    .enumerate()
                {
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
                for (record_index, record) in std::iter::once(control)
                    .chain(ctx.admit_iter(records.as_ref(), "NX offset unknown record traversal")?)
                    .enumerate()
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
    let no_native_content = model.is_empty() && scan.container.indexed_om_sections(ctx)?.0.is_empty();
    let annotation_stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    if no_native_content {
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
    for (tessellation, source_offset) in ctx.admit_iter(
        display_jt_tessellations,
        "NX display tessellation attachment",
    )? {
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
        &model.om.part_attributes,
        |attribute| {
            (
                attribute.id.as_str(),
                attribute.title.as_str(),
                attribute.value.as_str(),
                attribute.source_offset,
            )
        },
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
    attach_configurations(
        ctx,
        ir,
        &model.om.configurations,
        |configuration| {
            Ok((
                configuration.id.as_str(),
                configuration.name.as_str(),
                configuration.source_offset,
                ctx.find_by(
                    &model.om.configuration_attribute_uses,
                    |relation| {
                        ctx.equal_bytes(
                            relation.configuration.as_bytes(),
                            configuration.id.as_bytes(),
                            "NX configuration attribute use owner equality",
                        )
                    },
                    "NX configuration relation lookup",
                )?
                .map(|relation| relation.id.as_str()),
            ))
        },
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
        |value| &value.id,
        Ord::cmp,
        "sort NX features",
    )?;
    let namespace = ir.native.namespace_mut("nx");
    NATIVE_CATALOGUE
        .emit_all(ctx, model, namespace)
        .map_err(CodecError::from)?;
    Ok(())
}

fn attach_part_attributes<'a, T: 'a>(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    attributes: &'a [T],
    fields: impl Fn(&'a T) -> (&'a str, &'a str, &'a str, u64),
    annotations: &mut AnnotationBuilder,
    annotation_stream: &StreamHandle,
) -> Result<(), CodecError> {
    for attribute in ctx.admit_iter(attributes, "NX part attribute traversal")? {
        let (attribute_id, attribute_title, attribute_value, source_offset) = fields(attribute);
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
        let mut values = Vec::new();
        ctx.reserve_capacity(&mut values, 1, "NX part attribute values")?;
        values.push(AttributeValue::String(ctx.format_retained(
            format_args!("{attribute_value}"),
            "NX part attribute values",
        )?));
        ctx.reserve_capacity(&mut ir.model.attributes, 1, "NX attached part attributes")?;
        ir.model.attributes.push(SourceAttribute {
            id,
            target: AttributeTarget::Document,
            name: ctx
                .format_retained(format_args!("{attribute_title}"), "NX part attribute name")?,
            values,
        });
    }
    Ok(())
}

fn attach_configurations<'a, T: 'a>(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    configurations: &'a [T],
    mut fields: impl FnMut(&'a T) -> Result<(&'a str, &'a str, u64, Option<&'a str>), CodecError>,
    annotations: &mut AnnotationBuilder,
    annotation_stream: &StreamHandle,
) -> Result<(), CodecError> {
    for (ordinal, configuration) in ctx
        .admit_iter(configurations, "NX configuration traversal")?
        .enumerate()
    {
        let (configuration_id, configuration_name, source_offset, active_attribute_use) =
            fields(configuration)?;
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
            for body in
                ctx.admit_iter(&ir.model.bodies, "NX active configuration body traversal")?
            {
                selected.push(
                    body.id
                        .try_clone_for_decode(ctx, "NX attach configurations body id copy")?,
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
        let mut properties = BTreeMap::new();
        if let Some(relation) = active_attribute_use {
            ctx.insert_btree_map(
                &mut properties,
                cadmpeg_core::nonblank_literal!("active_attribute_use"),
                ctx.format_retained(
                    format_args!("{relation}"),
                    "NX active configuration property",
                )?,
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
            name: Some(ctx.copy_retained_text(configuration_name, "NX configuration name")?),
            material: None,
            properties,
            parameter_overrides: BTreeMap::new(),
            bodies,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(ctx.format_retained(
                format_args!("{configuration_id}"),
                "NX configuration native reference",
            )?),
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
    for (face_id, color) in ctx.admit_iter(&bindings, "NX RM face color bindings")? {
        let Some(index) = ctx.rposition_by(
            &ir.model.faces,
            |face| {
                ctx.equal_bytes(
                    face.id.as_str().as_bytes(),
                    face_id.as_str().as_bytes(),
                    "NX attach rm face colors equality",
                )
            },
            "NX RM face color target lookup",
        )?
        else {
            continue;
        };
        let face = &mut ir.model.faces[index];
        if face.color.is_none() || face.color == Some(*color) {
            face.color = Some(*color);
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
    let mut faces = faces.into_iter();
    while let Some(id) = ctx.next_charged(&mut faces, "NX RM face identity lookup")? {
        if ctx.contains_btree_set(&ids, id, "NX RM face identity membership")? {
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
    for binding in ctx.admit_iter(source_bindings, "NX source appearance bindings")? {
        let Some(definition) = ctx
            .rposition_by(
                &model.om.part_color_definitions,
                |definition| {
                    ctx.equal_bytes(
                        definition.id.as_bytes(),
                        binding.color_definition.as_bytes(),
                        "NX source appearance color definition equality",
                    )
                },
                "NX RM appearance definition lookup",
            )?
            .and_then(|index| model.om.part_color_definitions.get(index))
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
            .checked_add(appearance_id.as_str().len())
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
                source_id: ctx
                    .copy_retained_text(&binding.source_id, "NX RM source appearance binding")?,
            },
            appearance: appearance_id,
            source_entity_id: Some(
                ctx.copy_retained_text(&binding.source_id, "NX RM source entity identity")?,
            ),
            object_type: Some(
                ctx.copy_retained_text("RMFastLoad object ID", "NX RM source object type")?,
            ),
            visible: None,
            channels: BTreeMap::new(),
        });
    }
    for binding in ctx.admit_iter(face_bindings, "NX face appearance bindings")? {
        let Some(definition) = ctx
            .rposition_by(
                &model.om.part_color_definitions,
                |definition| {
                    ctx.equal_bytes(
                        definition.id.as_bytes(),
                        binding.color_definition.as_bytes(),
                        "NX face appearance color definition equality",
                    )
                },
                "NX RM appearance definition lookup",
            )?
            .and_then(|index| model.om.part_color_definitions.get(index))
        else {
            continue;
        };
        let Some(face) = ctx.find_by(
            &ir.model.faces,
            |face| {
                ctx.equal_bytes(
                    face.id.as_str().as_bytes(),
                    binding.face_id.as_bytes(),
                    "NX face appearance face identity equality",
                )
            },
            "NX RM appearance face lookup",
        )?
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
            .try_clone_for_decode(ctx, "NX attach rm appearances face id copy")?;
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
            source_entity_id: Some(
                ctx.copy_retained_text(&binding.face_id, "NX RM face entity identity")?,
            ),
            object_type: Some(ctx.copy_retained_text("Parasolid FACE", "NX RM face object type")?),
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
    if let Some(id) =
        ctx.get_btree_map(appearances, &definition.id, "NX RM appearance reuse lookup")?
    {
        return id.try_clone_for_decode(ctx, "NX ensure rm color appearance id copy");
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
    ctx.charge_collection_items(1, "NX RM color appearances")?;
    ctx.reserve_capacity(&mut ir.model.appearances, 1, "NX RM color appearances")?;
    ir.model.appearances.push(Appearance {
        id: id.try_clone_for_decode(ctx, "NX ensure rm color appearance id copy")?,
        name: Some(ctx.copy_retained_text(&definition.name, "NX RM color appearance")?),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: Some(ctx.copy_retained_text("UGS::COLOR_table", "NX RM appearance schema")?),
        category: None,
        base_color: Some(color),
        properties: BTreeMap::new(),
        textures: Vec::new(),
    });

    let lookup_id = appearances_reservation
        .with_storage(|| id.try_clone_for_decode(ctx, "NX RM appearance identity lookup"))?;
    let lookup_key = appearances_reservation.with_storage(|| {
        ctx.copy_retained_text(&definition.id, "NX RM appearance identity lookup")
    })?;
    appearances_reservation.with_storage(|| {
        ctx.insert_btree_map(
            appearances,
            lookup_key,
            lookup_id,
            "NX RM appearance identity lookup",
        )
    })?;
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

    fn observe(
        &mut self,
        ctx: &DecodeContext<'_>,
        assignment: &'a RmDisplayColorAssignment,
    ) -> Result<(), CodecError> {
        if let Self::Unique {
            definition,
            source_offset,
        } = self
        {
            if ctx.equal_bytes(
                definition.as_bytes(),
                assignment.color_definition.as_bytes(),
                "NX observe equality",
            )? {
                *source_offset = (*source_offset).min(assignment.frame.offset());
            } else {
                *self = Self::Conflicting;
            }
        }
        Ok(())
    }
}

fn resolve_rm_source_color_bindings(
    ctx: &DecodeContext<'_>,
    assignments: &[RmDisplayColorAssignment],
) -> Result<Vec<RmSourceColorBinding>, CodecError> {
    let mut choices = BTreeMap::<&str, RmColorChoice<'_>>::new();
    let mut choices_reservation = ctx.reserve_scoped(0, "NX RM source color choices")?;
    for assignment in ctx.admit_iter(assignments, "NX RM source color assignments")? {
        let Some(source_id) = assignment.target_object_id.as_deref() else {
            continue;
        };
        match choices_reservation.with_storage(|| {
            ctx.entry_btree_map(
                &mut choices,
                source_id,
                "NX resolve rm source color bindings choices entry",
            )
        })? {
            Entry::Occupied(mut entry) => entry.get_mut().observe(ctx, assignment)?,
            Entry::Vacant(entry) => {
                entry.insert(RmColorChoice::new(assignment));
            }
        }
    }
    let mut bindings = Vec::new();
    for (&source_id, choice) in ctx.admit_iter(&choices, "NX RM source choice traversal")? {
        let RmColorChoice::Unique {
            definition,
            source_offset,
        } = *choice
        else {
            continue;
        };
        ctx.reserve_vec(&mut bindings, 1, "NX RM source color bindings")?;
        bindings.push(RmSourceColorBinding {
            source_id: ctx.copy_retained_text(source_id, "NX RM source binding identity")?,
            color_definition: ctx.copy_retained_text(definition, "NX RM source binding color")?,
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
    for binding in ctx.admit_iter(&bindings, "NX resolved face color traversal")? {
        let Some(definition) = ctx
            .rposition_by(
                definitions,
                |definition| {
                    ctx.equal_bytes(
                        definition.id.as_bytes(),
                        binding.color_definition.as_bytes(),
                        "NX resolve rm face colors equality",
                    )
                },
                "NX RM face color definition lookup",
            )?
            .and_then(|index| (definitions).get(index))
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
    let mut records_iter = assignments.iter().enumerate();
    while let Some((position, assignment)) =
        ctx.next_charged(&mut records_iter, "NX linked color assignment traversal")?
    {
        let RmDisplayColorAssignmentEncoding::Linked(row) = assignment.frame.encoding() else {
            continue;
        };
        let object_index = row.first_index().atom.value();
        if ctx.any_by(
            &assignments[..position],
            |earlier| {
                Ok({
                    matches!(
                        earlier.frame.encoding(),
                        RmDisplayColorAssignmentEncoding::Linked(previous)
                            if previous.first_index().atom.value() == object_index
                    )
                })
            },
            "NX RM face color assignment identity",
        )? {
            continue;
        }
        let mut choice = RmColorChoice::new(assignment);
        for later in ctx.admit_iter(&assignments[position + 1..], "NX RM face color assignments")? {
            if matches!(
                later.frame.encoding(),
                RmDisplayColorAssignmentEncoding::Linked(next)
                    if next.first_index().atom.value() == object_index
            ) {
                choice.observe(ctx, later)?;
            }
        }
        let RmColorChoice::Unique {
            definition,
            source_offset,
        } = choice
        else {
            continue;
        };
        if !ctx.any_by(
            definitions,
            |candidate| {
                ctx.equal_bytes(
                    candidate.id.as_bytes(),
                    definition.as_bytes(),
                    "NX resolve rm face color bindings equality",
                )
            },
            "NX RM face color definitions",
        )? {
            continue;
        }

        let mut candidate = None::<(u32, u32)>;
        let mut ambiguous = false;
        let mut records = records.iter();
        while let Some(record) = ctx.next_charged(&mut records, "NX RM face color records")? {
            let crate::deltas::record_family::RecordFamily::Face { node_id, .. } = &record.family
            else {
                continue;
            };
            if *node_id != object_index {
                continue;
            }
            let mut partition = None;
            let mut multiple_partitions = false;
            let mut records_iter = delta_pairs.iter();
            while let Some((raw_partition, deltas)) =
                ctx.next_charged(&mut records_iter, "NX RM delta pair traversal")?
            {
                let Ok(partition_id) = u32::try_from(*raw_partition) else {
                    continue;
                };
                if !ctx.any_by(
                    deltas,
                    |delta| Ok(u32::try_from(*delta).ok() == Some(record.stream_ordinal)),
                    "NX RM face color delta links",
                )? {
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
            let (face_id, _face_identity_storage) =
                ctx.with_scoped_storage("NX RM candidate face identity", || {
                    ctx.format_retained(
                        format_args!("nx:s{partition}:face#{}", record.xmt),
                        "NX RM candidate face formatting",
                    )
                })?;
            if !ctx.contains_btree_set(
                face_ids,
                &face_id,
                "NX RM face color identity membership",
            )? {
                continue;
            }
            match candidate.as_ref() {
                None => candidate = Some((partition, record.xmt)),
                Some(existing) if *existing == (partition, record.xmt) => {}
                Some(_) => {
                    ambiguous = true;
                    break;
                }
            }
        }
        if ambiguous {
            continue;
        }
        let Some((partition, xmt)) = candidate else {
            continue;
        };
        ctx.charge_collection_items(1, "NX RM face color bindings")?;
        ctx.reserve_capacity(&mut bindings, 1, "NX RM face color bindings")?;
        bindings.push(RmFaceColorBinding {
            face_id: ctx.format_retained(
                format_args!("nx:s{partition}:face#{xmt}"),
                "NX RM selected face identity",
            )?,
            color_definition: ctx.copy_retained_text(definition, "NX RM face color binding")?,
            source_offset,
        });
    }
    ctx.stable_sort_by(
        &mut bindings,
        |value| &value.face_id,
        Ord::cmp,
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
    for (ordinal, entry) in ctx
        .admit_iter(&scan.container.entries, "NX JPEG preview entry traversal")?
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
        if crate::decode::jpeg::jpeg_dimensions(ctx, bytes)?.is_none() {
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
        ir.model.assets.push(Asset::try_new(
            ctx,
            id,
            Some(if ordinal == 0 {
                ctx.copy_retained_text("preview.jpg", "NX JPEG preview name")?
            } else {
                ctx.format_retained(
                    format_args!("preview-{ordinal}.jpg"),
                    "NX JPEG preview name",
                )?
            }),
            Some(ctx.copy_retained_text("image/jpeg", "NX JPEG preview media type")?),
            AssetContent::Embedded {
                data: cadmpeg_ir::assets::AssetData::new(
                    ctx.copy_retained(bytes, "retain NX JPEG preview asset")?,
                )
                .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
            },
            Some(native_ref.into_string()),
        )?);
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
    let mut records_iter = textures.iter();
    while let Some(texture) =
        ctx.next_charged(&mut records_iter, "NX material texture traversal")?
    {
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
        if !ctx.equal_bytes(
            digest.as_str().as_bytes(),
            texture.sha256.as_str().as_bytes(),
            "NX attach material texture assets equality",
        )? {
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
    let mut asset_storage = ctx.reserve_scoped(0, "NX material asset staging slots")?;
    for (texture, bytes) in ctx
        .admit_iter(&sources, "NX material texture source traversal")?
        .copied()
    {
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
        ctx.reserve_scoped_vec(
            &mut asset_storage,
            &mut assets,
            1,
            "NX material asset records",
        )?;
        assets.push(Asset::try_new(
            ctx,
            extended_id::<AssetId>(texture.id.as_str(), &cadmpeg_ir::identity_key!("asset"))
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("NX material texture id is not an identity"))
                })?,
            Some(ctx.copy_retained_text(texture.name(), "NX material asset text")?),
            Some(ctx.copy_retained_text("image/tiff", "NX TIFF material media type")?),
            AssetContent::Embedded {
                data: cadmpeg_ir::assets::AssetData::new(
                    ctx.copy_retained(bytes, "retain NX TIFF material asset")?,
                )
                .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
            },
            Some(ctx.copy_retained_text(&texture.id, "NX material asset text")?),
        )?);
    }
    let stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    drop(sources);
    drop(source_reservation);
    for (texture, asset) in ctx
        .admit_iter(textures, "NX material texture annotations")?
        .zip(ctx.admit_iter(&assets, "NX material asset annotations")?)
    {
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
    ir.model
        .assets
        .extend(ctx.admit_iter(assets, "NX attached material asset moves")?);
    Ok(())
}

fn attach_active_configuration_parameter_values(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let Some(configuration_index) =
        unique_active_configuration_index(ctx, &ir.model.configurations)?
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
    let (parameters_by_id, _parameter_index_storage) = ctx.unique_index(
        ir.model
            .parameters
            .iter()
            .map(|parameter| (parameter.id.as_str(), parameter)),
        "NX configuration parameter identity index",
    )?;
    let mut candidates = ir.model.parameters.iter();
    while let Some(parameter) = ctx.next_charged(
        &mut candidates,
        "NX configuration parameter identity uniqueness",
    )? {
        if ctx
            .get_hash_map(
                &parameters_by_id,
                parameter.id.as_str(),
                "NX configuration unique parameter identity",
            )?
            .and_then(Option::as_ref)
            .is_none()
        {
            return Ok(());
        }
    }
    let mut records_iter = ir.model.parameters.iter();
    while let Some(parameter) =
        ctx.next_charged(&mut records_iter, "NX configuration parameter projection")?
    {
        let mut records_iter = parameter.dependencies.iter();
        while let Some(dependency) =
            ctx.next_charged(&mut records_iter, "NX configuration dependency traversal")?
        {
            let Some(preceding) = ctx
                .get_hash_map(
                    &parameters_by_id,
                    dependency.as_str(),
                    "NX configuration parameter dependencies",
                )?
                .and_then(Option::as_ref)
            else {
                return Ok(());
            };
            let same_owner = match (&preceding.owner, &parameter.owner) {
                (Some(first), Some(second)) => ctx.equal_bytes(
                    first.as_str().as_bytes(),
                    second.as_str().as_bytes(),
                    "NX configuration parameter owner identity",
                )?,
                (None, None) => true,
                _ => false,
            };
            if !same_owner || preceding.ordinal >= parameter.ordinal {
                return Ok(());
            }
        }
    }
    // A parameter with no evaluated value leaves the configuration untouched,
    // which is what the dependency guard above does for its own case.
    if ctx.any_by(
        &ir.model.parameters,
        |parameter| Ok(parameter.value.is_none()),
        "NX configuration parameter values",
    )? {
        return Ok(());
    }
    let mut values = BTreeMap::new();
    let mut records_iter = ir.model.parameters.iter();
    while let Some(parameter) =
        ctx.next_charged(&mut records_iter, "NX configuration parameter projection")?
    {
        let Some(value) = parameter.value.as_ref() else {
            return Ok(());
        };

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
    for body in ctx.admit_iter(&ir.model.bodies, "NX current body traversal")? {
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
    let (closure, _closure_storage) = ctx
        .with_scoped_storage("NX current feature closure", || {
            active_feature_closure(ctx, ir, &current_bodies)
        })?;
    let Ok(active_features) = closure else {
        return Ok(());
    };
    for (_, &index) in ctx.admit_iter(&active_features, "NX active feature traversal")? {
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
    let Some(configuration_index) =
        unique_active_configuration_index(ctx, &ir.model.configurations)?
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
    let (closure, _closure_storage) = ctx
        .with_scoped_storage("NX configuration feature closure", || {
            active_feature_closure(ctx, ir, configuration_bodies)
        })?;
    let Ok(active_features) = closure else {
        return Ok(());
    };
    let mut states = BTreeMap::new();
    for (id, &index) in
        ctx.admit_iter(&active_features, "NX configuration active feature states")?
    {
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
    for (_, &index) in ctx.admit_iter(&active_features, "NX active feature traversal")? {
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

fn unique_active_configuration_index(
    ctx: &DecodeContext<'_>,
    configurations: &[DesignConfiguration],
) -> Result<Option<usize>, CodecError> {
    let mut active = None;
    let mut configurations = configurations.iter().enumerate();
    while let Some((index, configuration)) =
        ctx.next_charged(&mut configurations, "NX active configuration uniqueness")?
    {
        if configuration.active && active.replace(index).is_some() {
            return Ok(None);
        }
    }
    Ok(active)
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
    let mut has_binding = false;
    let mut records_iter = ir.model.bodies.iter();
    while let Some(body) =
        ctx.next_charged(&mut records_iter, "NX retained-history body traversal")?
    {
        if ctx.any_by(
            body_bindings,
            |binding| {
                Ok({
                    let (prefix, length) = stream_prefix(binding.stream_ordinal, false)?;
                    body.id.as_str().as_bytes().starts_with(&prefix[..length])
                })
            },
            "NX retained-history binding traversal",
        )? {
            has_binding = true;
            break;
        }
    }
    if !has_binding {
        return Ok(None);
    }

    let (mut sorted_bodies, _sorting) =
        ctx.temporary_vec(body_count, "NX retained-history body order")?;
    sorted_bodies.extend(ctx.admit_iter(&ir.model.bodies, "NX retained-history body order input")?);
    ctx.stable_sort_by(
        &mut sorted_bodies,
        |value| &value.id,
        Ord::cmp,
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
    for (position, body) in ctx
        .admit_iter(&sorted_bodies, "NX retained-history sorted body traversal")?
        .enumerate()
    {
        if match sorted_bodies.get(position + 1) {
            Some(next) => ctx.equal_bytes(
                next.id.as_str().as_bytes(),
                body.id.as_str().as_bytes(),
                "NX attach initial segment bodies equality",
            )?,
            None => false,
        } {
            continue;
        }
        let mut matched = false;
        for binding in ctx.admit_iter(
            body_bindings,
            "NX retained-history selected binding traversal",
        )? {
            let (prefix, length) = stream_prefix(binding.stream_ordinal, false)?;
            if !body.id.as_str().as_bytes().starts_with(&prefix[..length]) {
                continue;
            }
            matched = true;
            ctx.insert_btree_map(
                &mut source_properties,
                cadmpeg_core::nonblank_literal!(ctx, "segment_body_binding.{binding_ordinal}")?,
                ctx.copy_retained_text(&binding.id, "NX retained-history binding properties")?,
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
                    .try_clone_for_decode(ctx, "NX attach initial segment bodies body id copy")?,
            );
            feature_outputs.push(
                body.id
                    .try_clone_for_decode(ctx, "NX attach initial segment bodies body id copy")?,
            );
        }
    }

    annotations.note(ctx, &id, stream, 0, Some("FEATURE_HISTORY_INPUT"))?;
    annotations
        .derived(ctx, &id, "definition")
        .map_err(cadmpeg_core::CodecError::from)?;
    ctx.reserve_vec(
        &mut ir.model.features,
        1,
        "NX retained-history input features",
    )?;
    ir.model.features.push(Feature {
        id: id.try_clone_for_decode(ctx, "NX attach initial segment bodies id copy")?,
        ordinal: cadmpeg_core::decode::u64_from_index(ir.model.features.len()),
        name: Some(
            ctx.copy_retained_text("Retained history input", "NX retained history feature name")?,
        ),
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
                    native: ctx.copy_retained_text(
                        "nx:segment-body-bindings",
                        "NX retained history native selection",
                    )?,
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
    let (admitted_body_references, _admitted_body_references_storage) =
        ctx.with_scoped_storage("NX primary body reference index", || {
            native_primary_body_references(
                ctx,
                body_references,
                body_data_block_uses,
                body_segment_uses,
                input_blocks,
                data_blocks,
            )
        })?;
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
        "NX attach feature operations booleans index",
    )?;
    let (body_references_by_id, _body_references_by_id_reservation) = ctx
        .collect_scoped_btree_map(
            body_references
                .iter()
                .map(|reference| (reference.id.as_str(), reference)),
            "NX attach feature operations body references index",
        )?;
    let mut group_reservation = ctx.reserve_scoped(0, "NX feature operation group indexes")?;
    let mut body_segment_uses_by_reference =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodySegmentUse>>::new();
    for use_ in ctx.admit_iter(
        body_segment_uses,
        "NX attach feature operations: body segment uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut body_segment_uses_by_reference,
            use_.feature_body_reference.as_str(),
            || use_,
            0,
            "NX attach feature operations body segment uses by reference group index",
        )?;
    }
    let mut body_data_block_uses_by_reference =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodyDataBlockUse>>::new();
    for use_ in ctx.admit_iter(
        body_data_block_uses,
        "NX attach feature operations: body data block uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut body_data_block_uses_by_reference,
            use_.feature_body_reference.as_str(),
            || use_,
            0,
            "NX attach feature operations body data block uses by reference group index",
        )?;
    }
    let (body_writer_references_by_operation, _body_writer_references_by_operation_storage) = ctx
        .with_scoped_storage(
        "NX local body_writer_references_by_operation storage",
        || crate::native::features::unique_feature_body_references(ctx, body_references),
    )?;
    let mut offset_store_bodies_by_operation = BTreeMap::<&str, Vec<(u32, String)>>::new();
    for body_use in ctx.admit_iter(
        body_data_block_uses,
        "NX attach feature operations: body data block uses traversal",
    )? {
        let Some(reference) = ctx.get_btree_map(
            &body_references_by_id,
            body_use.feature_body_reference.as_str(),
            "NX attach feature operations body references by id lookup",
        )?
        else {
            continue;
        };
        let data_block = group_reservation.with_storage(|| {
            ctx.copy_retained_text(
                &body_use.data_block,
                "NX attach feature operations body use data block index",
            )
        })?;
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut offset_store_bodies_by_operation,
            reference.operation_label.as_str(),
            || (reference.body.value(), data_block),
            0,
            "NX attach feature operations offset store bodies by operation group index",
        )?;
    }
    let body_references = admitted_body_references;
    let mut body_reference_occurrences_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodyReference>>::new();
    for reference in ctx.admit_iter(
        body_reference_occurrences,
        "NX attach feature operations: body reference occurrences traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut body_reference_occurrences_by_operation,
            reference.operation_label.as_str(),
            || reference,
            0,
            "NX attach feature operations body reference occurrences by operation group index",
        )?;
    }
    let (body_kinds_by_id, _body_kinds_storage) = if ctx.any_by(
        labels,
        |label| Ok(label.value == "EXTRUDE"),
        "NX extrude body kind index need",
    )? {
        ctx.with_scoped_storage("NX output body kind index", || {
            let mut kinds = BTreeMap::new();
            // Reverse input preserves the first body for a repeated identity.
            for body in ctx
                .admit_iter(&ir.model.bodies, "NX output body kind index traversal")?
                .rev()
            {
                let key =
                    ctx.copy_retained_text(body.id.as_str(), "NX output body kind index key")?;
                ctx.insert_btree_map(
                    &mut kinds,
                    key,
                    body.kind,
                    "NX attach feature operations kinds index",
                )?;
            }
            Ok::<_, CodecError>(kinds)
        })?
    } else {
        (
            BTreeMap::new(),
            ctx.reserve_scoped(0, "NX unused output body kind index")?,
        )
    };
    let initial_feature_index = match initial_body_id.as_ref() {
        Some(id) => ctx.position_by(
            &ir.model.features,
            |feature| {
                ctx.equal_bytes(
                    feature.id.as_str().as_bytes(),
                    id.as_str().as_bytes(),
                    "NX initial feature identity equality",
                )
            },
            "NX initial body feature search",
        )?,
        None => None,
    };
    let mut body_writer_history_storage = ctx.reserve_scoped(0, "NX body writer history")?;
    let mut body_writer_history = BodyWriterHistory::default();
    if let Some(index) = initial_feature_index {
        let feature = &ir.model.features[index];
        body_writer_history_storage.with_storage(|| {
            body_writer_history.record_writer(
                ctx,
                None,
                None,
                feature.evaluation.outputs(),
                &feature.id,
            )
        })?;
    }
    let (body_alias_roots, _body_alias_roots_storage) = ctx
        .with_scoped_storage("NX local body_alias_roots storage", || {
            crate::native::segments::body_alias_roots(ctx, body_bindings)
        })?;
    let canonical_body = |identity: u32| -> Result<u32, CodecError> {
        Ok(ctx
            .get_btree_map(
                &body_alias_roots,
                &identity,
                "NX canonical body alias lookup",
            )?
            .copied()
            .unwrap_or(identity))
    };
    let mut input_blocks_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureInputBlock>>::new();
    for input in ctx.admit_iter(
        input_blocks,
        "NX attach feature operations: input blocks traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut input_blocks_by_operation,
            input.operation_label.as_str(),
            || input,
            0,
            "NX attach feature operations input blocks by operation group index",
        )?;
    }
    let (input_column_row_uses_by_operation, _input_column_row_uses_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            input_column_row_uses
                .iter()
                .map(|use_| (use_.operation_label.as_str(), use_)),
            "NX attach feature operations input column row uses index",
        )?;
    let (input_column_targets_by_operation, _input_column_targets_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
        input_column_targets
            .iter()
            .map(|target| (target.operation_label.as_str(), target)),
        "NX attach feature operations input column targets index",
    )?;
    let mut input_block_identity_group_by_input = BTreeMap::new();
    let mut input_block_identity_group_by_input_reservation =
        ctx.reserve_scoped(0, "NX last-record index")?;
    input_block_identity_group_by_input_reservation.with_storage(|| {
        for group in ctx.admit_iter(
            input_block_identity_groups,
            "NX input block group traversal",
        )? {
            for member in ctx.admit_iter(&*group.members, "NX input block group members")? {
                ctx.insert_btree_map(
                    &mut input_block_identity_group_by_input,
                    member.input_block.as_str(),
                    group.id.as_str(),
                    "NX attach feature operations input block identity group by input index",
                )?;
            }
        }
        Ok::<(), CodecError>(())
    })?;
    let (datum_csys_constructions_by_operation, _datum_csys_constructions_by_operation_reservation) =
        ctx.collect_scoped_btree_map(
            datum_csys_constructions
                .iter()
                .map(|construction| (construction.operation_label.as_str(), construction)),
            "NX attach feature operations datum csys constructions index",
        )?;
    let (datum_csys_payloads_by_operation, _datum_csys_payloads_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            datum_csys_payloads
                .iter()
                .map(|payload| (payload.operation_label.as_str(), payload)),
            "NX attach feature operations datum csys payloads index",
        )?;
    let (
        datum_csys_payload_scalar_pairs_by_operation,
        _datum_csys_payload_scalar_pairs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_csys_payload_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
        "NX attach feature operations datum csys payload scalar pairs index",
    )?;
    let (
        datum_csys_payload_fixed_pairs_by_operation,
        _datum_csys_payload_fixed_pairs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_csys_payload_fixed_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
        "NX attach feature operations datum csys payload fixed pairs index",
    )?;
    let (
        datum_csys_payload_scalars_by_operation,
        _datum_csys_payload_scalars_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_csys_payload_scalars
            .iter()
            .map(|scalar| (scalar.operation_label.as_str(), scalar)),
        "NX attach feature operations datum csys payload scalars index",
    )?;
    let (datum_csys_descriptors_by_operation, _datum_csys_descriptors_by_operation_reservation) =
        ctx.collect_scoped_btree_groups(
            datum_csys_descriptors
                .iter()
                .map(|descriptor| (descriptor.operation_label.as_str(), descriptor)),
            "NX attach feature operations datum csys descriptors index",
        )?;
    let (
        datum_csys_column_row_uses_by_operation,
        _datum_csys_column_row_uses_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_csys_column_row_uses
            .iter()
            .map(|use_| (use_.operation_label.as_str(), use_)),
        "NX attach feature operations datum csys column row uses index",
    )?;
    let mut datum_csys_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumCsysBlockUse>>::new();
    for block_use in ctx.admit_iter(
        datum_csys_block_uses,
        "NX attach feature operations: datum csys block uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut datum_csys_uses_by_input_operation,
            block_use.input_operation_label.as_str(),
            || block_use,
            0,
            "NX attach feature operations datum csys uses by input operation group index",
        )?;
    }
    let (datum_plane_headers_by_operation, _datum_plane_headers_by_operation_reservation) = ctx
        .collect_scoped_btree_map(
            datum_plane_headers
                .iter()
                .map(|header| (header.operation_label.as_str(), header)),
            "NX attach feature operations datum plane headers index",
        )?;
    let (datum_plane_payloads_by_operation, _datum_plane_payloads_by_operation_reservation) = ctx
        .collect_scoped_btree_map(
        datum_plane_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations datum plane payloads index",
    )?;
    let (
        datum_plane_payload_scalar_pairs_by_operation,
        _datum_plane_payload_scalar_pairs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        datum_plane_payload_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
        "NX attach feature operations datum plane payload scalar pairs index",
    )?;
    let (datum_plane_descriptors_by_operation, _datum_plane_descriptors_by_operation_reservation) =
        ctx.collect_scoped_btree_groups(
            datum_plane_descriptors
                .iter()
                .map(|descriptor| (descriptor.operation_label.as_str(), descriptor)),
            "NX attach feature operations datum plane descriptors index",
        )?;
    let mut datum_plane_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneBlockUse>>::new();
    for block_use in ctx.admit_iter(
        datum_plane_block_uses,
        "NX attach feature operations: datum plane block uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut datum_plane_uses_by_input_operation,
            block_use.input_operation_label.as_str(),
            || block_use,
            0,
            "NX attach feature operations datum plane uses by input operation group index",
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
        "NX attach feature operations chronological labels index",
    )?;
    let (sketch_datum_csys_dependencies, _sketch_datum_csys_dependencies_reservation) = ctx
        .collect_scoped_btree_map(
            sketch_datum_csys_dependencies
                .iter()
                .map(|dependency| (dependency.datum_csys_operation_label.as_str(), dependency)),
            "NX attach feature operations sketch datum csys dependencies index",
        )?;
    let mut datum_identity_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneCsysIdentityUse>>::new();
    for identity_use in ctx.admit_iter(
        datum_plane_csys_identity_uses,
        "NX attach feature operations: datum plane csys identity uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut datum_identity_uses_by_operation,
            identity_use.datum_plane_operation_label.as_str(),
            || identity_use,
            0,
            "NX attach feature operations datum identity uses by operation group index",
        )?;
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut datum_identity_uses_by_operation,
            identity_use.datum_csys_operation_label.as_str(),
            || identity_use,
            0,
            "NX attach feature operations datum identity uses by operation group index",
        )?;
    }
    let mut sketch_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchReference>>::new();
    for reference in ctx.admit_iter(
        sketch_references,
        "NX attach feature operations: sketch references traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_references_by_operation,
            reference.operation_label.as_str(),
            || reference,
            0,
            "NX attach feature operations sketch references by operation group index",
        )?;
    }
    let (
        projected_curve_references_by_operation,
        _projected_curve_references_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        projected_curve_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "NX attach feature operations projected curve references index",
    )?;
    let (
        projected_curve_construction_payloads_by_operation,
        _projected_curve_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        projected_curve_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations projected curve construction payloads index",
    )?;
    let (
        projected_curve_construction_strings_by_operation,
        _projected_curve_construction_strings_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        projected_curve_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
        "NX attach feature operations projected curve construction strings index",
    )?;
    let (fset_reference_graphs_by_operation, _fset_reference_graphs_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            fset_reference_graphs
                .iter()
                .map(|graph| (graph.operation_label.as_str(), graph)),
            "NX attach feature operations fset reference graphs index",
        )?;
    let (
        fset_construction_payloads_by_operation,
        _fset_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        fset_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations fset construction payloads index",
    )?;
    let (delete_reference_fields_by_operation, _delete_reference_fields_by_operation_reservation) =
        ctx.collect_scoped_btree_groups(
            delete_reference_fields
                .iter()
                .map(|field| (field.operation_label.as_str(), field)),
            "NX attach feature operations delete reference fields index",
        )?;
    let (
        delete_construction_payloads_by_operation,
        _delete_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        delete_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations delete construction payloads index",
    )?;
    let (pattern_references_by_operation, _pattern_references_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            pattern_references
                .iter()
                .map(|reference| (reference.operation_label.as_str(), reference)),
            "NX attach feature operations pattern references index",
        )?;
    let (
        pattern_counted_reference_lanes_by_operation,
        _pattern_counted_reference_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        pattern_counted_reference_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations pattern counted reference lanes index",
    )?;
    let (
        pattern_construction_payloads_by_operation,
        _pattern_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        pattern_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations pattern construction payloads index",
    )?;
    let (
        pattern_construction_strings_by_operation,
        _pattern_construction_strings_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        pattern_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
        "NX attach feature operations pattern construction strings index",
    )?;
    let (
        pattern_construction_fixed_lanes_by_operation,
        _pattern_construction_fixed_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        pattern_construction_fixed_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations pattern construction fixed lanes index",
    )?;
    let (pattern_transform_lanes_by_operation, _pattern_transform_lanes_by_operation_reservation) =
        ctx.collect_scoped_btree_groups(
            pattern_transform_lanes
                .iter()
                .map(|lane| (lane.operation_label.as_str(), lane)),
            "NX attach feature operations pattern transform lanes index",
        )?;
    let (
        multi_instance_output_lanes_by_operation,
        _multi_instance_output_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        multi_instance_output_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations multi instance output lanes index",
    )?;
    let (
        identical_instance_output_lanes_by_operation,
        _identical_instance_output_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        identical_instance_output_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations identical instance output lanes index",
    )?;
    let (
        point_construction_headers_by_operation,
        _point_construction_headers_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        point_construction_headers
            .iter()
            .map(|header| (header.operation_label.as_str(), header)),
        "NX attach feature operations point construction headers index",
    )?;
    let (
        point_construction_scalar_lanes_by_operation,
        _point_construction_scalar_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        point_construction_scalar_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations point construction scalar lanes index",
    )?;
    let (
        draft_construction_references_by_operation,
        _draft_construction_references_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "NX attach feature operations draft construction references index",
    )?;
    let (
        draft_construction_index_lanes_by_operation,
        _draft_construction_index_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_index_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations draft construction index lanes index",
    )?;
    let (
        draft_construction_payloads_by_operation,
        _draft_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations draft construction payloads index",
    )?;
    let (
        draft_construction_graph_payloads_by_operation,
        _draft_construction_graph_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_graph_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations draft construction graph payloads index",
    )?;
    let (
        draft_construction_fixed_lanes_by_operation,
        _draft_construction_fixed_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_fixed_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations draft construction fixed lanes index",
    )?;
    let (
        draft_construction_binary32_lanes_by_operation,
        _draft_construction_binary32_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_binary32_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations draft construction binary32 lanes index",
    )?;
    let (
        draft_construction_graph_strings_by_operation,
        _draft_construction_graph_strings_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_graph_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
        "NX attach feature operations draft construction graph strings index",
    )?;
    let (
        draft_construction_identity_frames_by_operation,
        _draft_construction_identity_frames_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_identity_frames
            .iter()
            .map(|frame| (frame.operation_label.as_str(), frame)),
        "NX attach feature operations draft construction identity frames index",
    )?;
    let (
        draft_construction_terminal_lanes_by_operation,
        _draft_construction_terminal_lanes_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        draft_construction_terminal_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations draft construction terminal lanes index",
    )?;
    let (
        surface_construction_references_by_operation,
        _surface_construction_references_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
        "NX attach feature operations surface construction references index",
    )?;
    let (
        surface_construction_payloads_by_operation,
        _surface_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations surface construction payloads index",
    )?;
    let (
        surface_construction_scalar_pairs_by_operation,
        _surface_construction_scalar_pairs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
        "NX attach feature operations surface construction scalar pairs index",
    )?;
    let (
        surface_construction_strings_by_operation,
        _surface_construction_strings_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
        "NX attach feature operations surface construction strings index",
    )?;
    let (
        surface_construction_branches_by_operation,
        _surface_construction_branches_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        surface_construction_branches
            .iter()
            .map(|branch| (branch.operation_label.as_str(), branch)),
        "NX attach feature operations surface construction branches index",
    )?;
    let mut sketch_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchNamedPointBlockUse>>::new();
    for block_use in ctx.admit_iter(
        sketch_named_point_block_uses,
        "NX attach feature operations: sketch named point block uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_named_point_uses_by_operation,
            block_use.operation_label.as_str(),
            || block_use,
            0,
            "NX attach feature operations sketch named point uses by operation group index",
        )?;
    }
    let mut sketch_preceding_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPrecedingNamedPointUse>>::new();
    for point_use in ctx.admit_iter(
        sketch_preceding_named_point_uses,
        "NX attach feature operations: sketch preceding named point uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_preceding_named_point_uses_by_operation,
            point_use.operation_label.as_str(),
            || point_use,
            0,"NX attach feature operations sketch preceding named point uses by operation group index",
        )?;
    }
    let mut sketch_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPointUse>>::new();
    for point_use in ctx.admit_iter(
        sketch_point_uses,
        "NX attach feature operations: sketch point uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_point_uses_by_operation,
            point_use.operation_label.as_str(),
            || point_use,
            0,
            "NX attach feature operations sketch point uses by operation group index",
        )?;
    }
    let mut sketch_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPointGroup>>::new();
    for group in ctx.admit_iter(
        sketch_point_groups,
        "NX attach feature operations: sketch point groups traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_point_groups_by_operation,
            group.operation_label.as_str(),
            || group,
            0,
            "NX attach feature operations sketch point groups by operation group index",
        )?;
    }
    let mut extrude_profile_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureExtrudeProfileReference>>::new();
    for reference in ctx.admit_iter(
        extrude_profile_references,
        "NX attach feature operations: extrude profile references traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut extrude_profile_references_by_operation,
            reference.operation_label.as_str(),
            || reference,
            0,
            "NX attach feature operations extrude profile references by operation group index",
        )?;
    }
    let (
        extrude_construction_profiles_by_operation,
        _extrude_construction_profiles_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        extrude_construction_profiles
            .iter()
            .map(|profile| (profile.operation_label.as_str(), profile)),
        "NX attach feature operations extrude construction profiles index",
    )?;
    let mut operation_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyOperand>>::new();
    for operand in ctx.admit_iter(
        operation_body_operands,
        "NX attach feature operations: operation body operands traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_operands_by_operation,
            operand.operation_label.as_str(),
            || operand,
            0,
            "NX attach feature operations operation body operands by operation group index",
        )?;
    }
    let mut segment_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyOperand>>::new();
    for operand in ctx
        .admit_iter(operation_body_operands, "NX segment operand traversal")?
        .filter(|operand| !operand.segment_body_bindings.is_empty())
    {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut segment_body_operands_by_operation,
            operand.operation_label.as_str(),
            || operand,
            0,
            "NX attach feature operations segment body operands by operation group index",
        )?;
    }
    let (
        sketch_construction_inputs_by_operation,
        _sketch_construction_inputs_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        sketch_construction_inputs
            .iter()
            .map(|inputs| (inputs.operation_label.as_str(), inputs)),
        "NX attach feature operations sketch construction inputs index",
    )?;
    let (sketch_records_by_operation, _sketch_records_by_operation_reservation) = ctx
        .collect_scoped_btree_groups(
            sketch_records
                .iter()
                .map(|record| (record.operation_label.as_str(), record)),
            "NX attach feature operations sketch records index",
        )?;
    let (
        sketch_construction_payloads_by_operation,
        _sketch_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        sketch_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations sketch construction payloads index",
    )?;
    let mut sketch_coordinate_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeaturePayloadScalarPair>>::new();
    for pair in ctx.admit_iter(
        sketch_coordinate_pairs,
        "NX attach feature operations: sketch coordinate pairs traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_coordinate_pairs_by_operation,
            pair.operation_label.as_str(),
            || pair,
            0,
            "NX attach feature operations sketch coordinate pairs by operation group index",
        )?;
    }
    let mut sketch_fixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadFixedPair>>::new();
    for pair in ctx.admit_iter(
        sketch_fixed_pairs,
        "NX attach feature operations: sketch fixed pairs traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_fixed_pairs_by_operation,
            pair.operation_label.as_str(),
            || pair,
            0,
            "NX attach feature operations sketch fixed pairs by operation group index",
        )?;
    }
    let mut sketch_mixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadMixedPair>>::new();
    for pair in ctx.admit_iter(
        sketch_mixed_pairs,
        "NX attach feature operations: sketch mixed pairs traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_mixed_pairs_by_operation,
            pair.operation_label.as_str(),
            || pair,
            0,
            "NX attach feature operations sketch mixed pairs by operation group index",
        )?;
    }
    let mut sketch_payload_scalar_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadScalarLane>>::new();
    for lane in ctx.admit_iter(
        sketch_payload_scalar_lanes,
        "NX attach feature operations: sketch payload scalar lanes traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_payload_scalar_lanes_by_operation,
            lane.operation_label.as_str(),
            || lane,
            0,
            "NX attach feature operations sketch payload scalar lanes by operation group index",
        )?;
    }
    let mut sketch_fixed_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchFixedPoint>>::new();
    for point in ctx.admit_iter(
        sketch_fixed_points,
        "NX attach feature operations: sketch fixed points traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut sketch_fixed_points_by_operation,
            point.operation_label.as_str(),
            || point,
            0,
            "NX attach feature operations sketch fixed points by operation group index",
        )?;
    }
    let (block_constructions_by_operation, _block_constructions_by_operation_reservation) = ctx
        .collect_scoped_btree_map(
            block_constructions
                .iter()
                .map(|construction| (construction.operation_label.as_str(), construction)),
            "NX attach feature operations block constructions index",
        )?;
    let (
        block_construction_payloads_by_operation,
        _block_construction_payloads_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        block_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
        "NX attach feature operations block construction payloads index",
    )?;
    let (block_dimensions_by_operation, _block_dimensions_by_operation_reservation) = ctx
        .collect_scoped_btree_map(
            block_dimensions
                .iter()
                .map(|dimensions| (dimensions.operation_label.as_str(), dimensions)),
            "NX attach feature operations block dimensions index",
        )?;
    let mut block_payload_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBlockPayloadPoint>>::new();
    for point in ctx.admit_iter(
        block_payload_points,
        "NX attach feature operations: block payload points traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut block_payload_points_by_operation,
            point.operation_label.as_str(),
            || point,
            0,
            "NX attach feature operations block payload points by operation group index",
        )?;
    }
    let mut block_payload_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBlockPayloadPointGroup>>::new();
    for group in ctx.admit_iter(
        block_payload_point_groups,
        "NX attach feature operations: block payload point groups traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut block_payload_point_groups_by_operation,
            group.operation_label.as_str(),
            || group,
            0,
            "NX attach feature operations block payload point groups by operation group index",
        )?;
    }
    let (extrude_32_constructions_by_operation, _extrude_32_constructions_by_operation_reservation) =
        ctx.collect_scoped_btree_map(
            extrude_32_constructions
                .iter()
                .map(|construction| (construction.operation_label.as_str(), construction)),
            "NX attach feature operations extrude 32 constructions index",
        )?;
    let (extrude_payload_headers_by_operation, _extrude_payload_headers_by_operation_reservation) =
        ctx.collect_scoped_btree_map(
            extrude_payload_headers
                .iter()
                .map(|header| (header.operation_label.as_str(), header)),
            "NX attach feature operations extrude payload headers index",
        )?;
    let (
        operation_terminal_discriminators_by_operation,
        _operation_terminal_discriminators_by_operation_reservation,
    ) = ctx.collect_scoped_btree_map(
        operation_terminal_discriminators
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
        "NX attach feature operations operation terminal discriminators index",
    )?;
    let (
        extrude_payload_32_branches_by_operation,
        _extrude_payload_32_branches_by_operation_reservation,
    ) = ctx.collect_scoped_btree_groups(
        extrude_payload_32_branches
            .iter()
            .map(|branch| (branch.operation_label.as_str(), branch)),
        "NX attach feature operations extrude payload 32 branches index",
    )?;
    let mut operation_body_scalar_triples_by_operation = BTreeMap::<
        &str,
        Vec<&crate::native::features::body_scalar_triple::FeatureOperationBodyScalarTriple>,
    >::new();
    for triple in ctx.admit_iter(
        operation_body_scalar_triples,
        "NX attach feature operations: operation body scalar triples traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_scalar_triples_by_operation,
            triple.operation_label.as_str(),
            || triple,
            0,
            "NX attach feature operations operation body scalar triples by operation group index",
        )?;
    }
    for (_, triples) in ctx.admit_iter(
        &mut operation_body_scalar_triples_by_operation,
        "NX operation body scalar triple groups",
    )? {
        ctx.stable_sort_by(
            triples,
            |value| &value.body_reference_ordinal,
            Ord::cmp,
            "sort NX operation body scalar triples",
        )?;
    }
    let mut operation_body_members_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyMember>>::new();
    for member in ctx.admit_iter(
        operation_body_members,
        "NX attach feature operations: operation body members traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_members_by_operation,
            member.operation_label.as_str(),
            || member,
            0,
            "NX attach feature operations operation body members by operation group index",
        )?;
    }
    let mut operation_body_11_continuations_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBody11Continuation>>::new();
    for continuation in ctx.admit_iter(
        operation_body_11_continuations,
        "NX feature continuation traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_11_continuations_by_operation,
            continuation.operation_label.as_str(),
            || continuation,
            0,
            "NX attach feature operations operation body 11 continuations by operation group index",
        )?;
    }
    let mut operation_body_reference_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyReferenceLane>>::new();
    for lane in ctx.admit_iter(
        operation_body_reference_lanes,
        "NX attach feature operations: operation body reference lanes traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut operation_body_reference_lanes_by_operation,
            lane.operation_label.as_str(),
            || lane,
            0,
            "NX attach feature operations operation body reference lanes by operation group index",
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
    let (
        (
            hole_outputs,
            simple_hole_diameters,
            counterbore_dimensions,
            blind_hole_depths,
            simple_hole_placements,
            counterbore_hole_placements,
            blind_hole_placements,
            simple_hole_chamfers,
            hole_packages,
        ),
        _hole_projection_storage,
    ) = ctx.with_scoped_storage("NX hole projection indexes", || {
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
        if let Some(projection) =
            hole_body_projection(ctx, ir, &simple_hole_operations, &hole_outputs)?
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
        let blind_hole_placements = blind_hole_axis_placements_for_operations(
            ctx,
            ir,
            &blind_hole_operations,
            &hole_outputs,
        )?;
        let simple_hole_chamfers =
            simple_hole_chamfers(ctx, ir, simple_hole_templates, &hole_outputs)?;
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
        Ok::<_, CodecError>((
            hole_outputs,
            simple_hole_diameters,
            counterbore_dimensions,
            blind_hole_depths,
            simple_hole_placements,
            counterbore_hole_placements,
            blind_hole_placements,
            simple_hole_chamfers,
            hole_packages,
        ))
    })?;
    let mut feature_ids_by_operation = BTreeMap::new();
    let mut feature_id_reservation = ctx.reserve_scoped(0, "NX operation feature identities")?;
    for label in ctx.admit_iter(labels, "NX operation feature identity scan")? {
        const PREFIX: &str = "nx:feature-history:feature#";

        if !projects_neutral_feature(&label.value)
            || ctx.contains_btree_set(
                &hole_packages.internal_operations,
                &label.id,
                "NX internal hole operation membership",
            )?
        {
            continue;
        }
        let key = label
            .id
            .strip_prefix("nx:feature-history:operation-label#")
            .unwrap_or(label.id.as_str());
        if key.is_empty()
            || ctx.contains_text(key, "#", "NX operation feature key separator")?
            || ctx.any_by(
                key.chars(),
                |ch| Ok(ch.is_whitespace()),
                "NX feature identity whitespace scan",
            )?
        {
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

        feature_id_reservation.with_storage(|| {
            ctx.try_reserve_retained_text(&mut id_text, id_len, "NX operation feature identity")
        })?;
        ctx.append_retained(
            &mut id_text,
            PREFIX,
            "NX attach feature operations prefix append",
        )?;
        ctx.append_retained(&mut id_text, key, "NX attach feature operations key append")?;
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
    for binding in ctx.admit_iter(
        parameter_bindings,
        "NX attach feature operations: parameter bindings traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut parameter_bindings_by_operation,
            binding.operation_label.as_str(),
            || binding,
            0,
            "NX attach feature operations parameter bindings by operation group index",
        )?;
    }
    let mut parameter_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterUse>>::new();
    for parameter_use in ctx.admit_iter(
        parameter_uses,
        "NX attach feature operations: parameter uses traversal",
    )? {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut parameter_uses_by_operation,
            parameter_use.operation_label.as_str(),
            || parameter_use,
            0,
            "NX attach feature operations parameter uses by operation group index",
        )?;
    }
    let (operation_labels_by_record, _operation_labels_by_record_reservation) = ctx
        .collect_scoped_btree_map(
            operation_records
                .iter()
                .map(|record| (record.id.as_str(), record.operation_label.as_str())),
            "NX attach feature operations operation records index",
        )?;
    let mut body_writes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyWrite>>::new();
    for (write, operation_label) in ctx
        .admit_iter(operation_body_writes, "NX body writer traversal")?
        .filter_map(|write| write.operation_label.as_deref().map(|label| (write, label)))
    {
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut body_writes_by_operation,
            operation_label,
            || write,
            0,
            "NX attach feature operations body writes by operation group index",
        )?;
    }
    let mut body_identity_writer_storage = ctx.reserve_scoped(0, "NX body identity writer")?;
    let mut body_identity_writers = BTreeMap::<u8, FeatureId>::new();
    let mut payload_strings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeaturePayloadString>>::new();
    for value in ctx.admit_iter(
        payload_strings,
        "NX attach feature operations: payload strings traversal",
    )? {
        let Some(operation) = ctx.get_btree_map(
            &operation_labels_by_record,
            value.operation_record.as_str(),
            "NX attach feature operations operation labels by record lookup",
        )?
        else {
            continue;
        };
        ctx.push_scoped_btree_group(
            &mut group_reservation,
            &mut payload_strings_by_operation,
            *operation,
            || value,
            0,
            "NX attach feature operations payload strings by operation group index",
        )?;
    }
    let mut parameter_owners = BTreeMap::new();
    let mut parameter_owner_reservation = ctx.reserve_scoped(0, "NX parameter owner index")?;
    for parameter in ctx.admit_iter(&ir.model.parameters, "NX parameter owner index")? {
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
    for (annotation_ordinal, label) in ctx
        .admit_iter(labels, "NX text annotation label traversal")?
        .filter(|label| label.value == "TEXT")
        .enumerate()
    {
        let payload_strings = ctx
            .get_btree_map(
                &payload_strings_by_operation,
                label.id.as_str(),
                "NX attach feature operations payload strings by operation lookup",
            )?
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
            ctx.append_retained(
                &mut message,
                PREFIX,
                "NX attach feature operations prefix append",
            )?;
            ctx.append_retained(
                &mut message,
                &label.id,
                "NX attach feature operations label id append",
            )?;
            ctx.append_retained(
                &mut message,
                SUFFIX,
                "NX attach feature operations suffix append",
            )?;
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
    let mut records_iter = chronological_labels.iter().copied().enumerate();
    while let Some((ordinal, label)) =
        ctx.next_charged(&mut records_iter, "NX chronological operation traversal")?
    {
        if !projects_neutral_feature(&label.value)
            || ctx.contains_btree_set(
                &hole_packages.internal_operations,
                &label.id,
                "NX internal hole operation membership",
            )?
        {
            continue;
        }
        let Some(source_id) = ctx.get_btree_map(
            &feature_ids_by_operation,
            label.id.as_str(),
            "NX attach feature operations feature ids by operation lookup",
        )?
        else {
            continue;
        };
        let mut feature_copy_storage = ctx.reserve_scoped(0, "NX current feature identity")?;
        let id = feature_copy_storage
            .with_storage(|| source_id.try_clone_for_decode(ctx, "NX current feature identity"))?;
        let boolean_offset_store_resolution = ctx
            .get_btree_map(
                &booleans,
                label.id.as_str(),
                "NX attach feature operations booleans lookup",
            )?
            .map(|operation| {
                crate::native::segments::boolean_offset_store_resolution(
                    ctx,
                    operation,
                    data_blocks,
                )
            })
            .transpose()?;
        let boolean_definition = ctx
            .get_btree_map(
                &booleans,
                label.id.as_str(),
                "NX attach feature operations booleans lookup",
            )?
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
        let retained_operation_body_writes = ctx
            .get_btree_map(
                &body_writes_by_operation,
                label.id.as_str(),
                "NX attach feature operations body writes by operation lookup",
            )?
            .map_or([].as_slice(), Vec::as_slice);
        let operation_body_writes = if body_writes_match_boolean_target(
            ctx,
            retained_operation_body_writes,
            ctx.get_btree_map(
                &booleans,
                label.id.as_str(),
                "NX attach feature operations booleans lookup",
            )?
            .copied(),
        )? {
            retained_operation_body_writes
        } else {
            &[]
        };
        for write in ctx.admit_iter(
            operation_body_writes,
            "NX operation writer dependency traversal",
        )? {
            if let Some(writer) = ctx.get_btree_map(
                &body_identity_writers,
                &write.frame.body_identity(),
                "NX body identity writer lookup",
            )? {
                push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
            }
        }
        if let (
            Some(operation),
            Some(resolution),
            Some(FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })),
        ) = (
            ctx.get_btree_map(
                &booleans,
                label.id.as_str(),
                "NX attach feature operations booleans lookup",
            )?,
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
                    ctx,
                    target,
                    operation.target.token.value(),
                    offset_store_body_blocks,
                    &body_alias_roots,
                    &body_writer_history,
                )? {
                    push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
                }
                for body in
                    ctx.admit_iter(&operation.tools, "NX boolean tool dependency traversal")?
                {
                    if let Some(writer) = boolean_participant_writer(
                        ctx,
                        tools,
                        body.token.value(),
                        offset_store_body_blocks,
                        &body_alias_roots,
                        &body_writer_history,
                    )? {
                        push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
                    }
                }
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &segment_body_operands_by_operation,
            label.id.as_str(),
            "NX attach feature operations segment body operands by operation lookup",
        )? {
            for operand in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations segment body operands by operation traversal",
            )? {
                if let Some(writer) = body_writer_history
                    .native_writer(ctx, canonical_body(operand.operand.atom.value())?)?
                {
                    push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
                }
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &operation_body_operands_by_operation,
            label.id.as_str(),
            "NX attach feature operations operation body operands by operation lookup",
        )? {
            for operand in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations operation body operands by operation traversal",
            )? {
                let Some(data_block) = operand.operand_data_block.as_deref() else {
                    continue;
                };
                let Some(writer) = body_writer_history.offset_store_writer(ctx, data_block)? else {
                    continue;
                };
                push_unique_feature_dependency(ctx, &mut dependencies, writer)?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_plane_uses_by_input_operation,
            label.id.as_str(),
            "NX attach feature operations datum plane uses by input operation lookup",
        )? {
            for block_use in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations datum plane uses by input operation traversal",
            )? {
                let Some(dependency) = preceding_operation_dependency(
                    ctx,
                    block_use.construction_operation_label.as_str(),
                    ordinal,
                    &operation_positions,
                    &feature_ids_by_operation,
                )?
                else {
                    continue;
                };
                push_unique_feature_dependency(ctx, &mut dependencies, dependency)?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_csys_uses_by_input_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys uses by input operation lookup",
        )? {
            for block_use in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations datum csys uses by input operation traversal",
            )? {
                let Some(dependency) = preceding_operation_dependency(
                    ctx,
                    block_use.construction_operation_label.as_str(),
                    ordinal,
                    &operation_positions,
                    &feature_ids_by_operation,
                )?
                else {
                    continue;
                };
                push_unique_feature_dependency(ctx, &mut dependencies, dependency)?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_identity_uses_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum identity uses by operation lookup",
        )? {
            for identity_use in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations datum identity uses by operation traversal",
            )? {
                let other = if ctx.equal_bytes(
                    identity_use.datum_plane_operation_label.as_bytes(),
                    label.id.as_bytes(),
                    "NX datum plane operation label equality",
                )? {
                    identity_use.datum_csys_operation_label.as_str()
                } else {
                    identity_use.datum_plane_operation_label.as_str()
                };
                let Some(dependency) = preceding_operation_dependency(
                    ctx,
                    other,
                    ordinal,
                    &operation_positions,
                    &feature_ids_by_operation,
                )?
                else {
                    continue;
                };
                push_unique_feature_dependency(ctx, &mut dependencies, dependency)?;
            }
        }
        if let Some(dependency) = ctx.get_btree_map(
            &sketch_datum_csys_dependencies,
            label.id.as_str(),
            "NX attach feature operations sketch datum csys dependencies lookup",
        )? {
            if let Some(feature) = ctx.get_btree_map(
                &feature_ids_by_operation,
                dependency.sketch_operation_label.as_str(),
                "NX attach feature operations feature ids by operation lookup",
            )? {
                push_unique_feature_dependency(ctx, &mut dependencies, feature)?;
            }
        }
        let mut source_properties = BTreeMap::new();
        for write in ctx.admit_iter(
            retained_operation_body_writes,
            "NX retained operation writer traversal",
        )? {
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
            if let Some(use_) = ctx.find_by(
                operation_body_image_segment_uses,
                |use_| {
                    ctx.equal_bytes(
                        use_.operation_body_write.as_bytes(),
                        write.id.as_bytes(),
                        "NX body image use writer identity equality",
                    )
                },
                "NX body image writer use search",
            )? {
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
            if let Some(use_) = ctx.find_by(
                operation_body_identity_segment_uses,
                |use_| {
                    ctx.equal_bytes(
                        use_.operation_body_write.as_bytes(),
                        write.id.as_bytes(),
                        "NX body identity use writer identity equality",
                    )
                },
                "NX body identity writer use search",
            )? {
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
            if let Some(use_) = ctx.find_by(
                operation_body_partition_uses,
                |use_| {
                    ctx.equal_bytes(
                        use_.operation_body_write.as_bytes(),
                        write.id.as_bytes(),
                        "NX body partition use writer identity equality",
                    )
                },
                "NX body partition writer use search",
            )? {
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
                for (group_ordinal, group) in ctx
                    .admit_iter(
                        &use_.parasolid_group_records,
                        "NX partition group record traversal",
                    )?
                    .enumerate()
                {
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("body_write.{ordinal}.parasolid_group.{group_ordinal}"),
                        format_args!("{group}"),
                    )?;
                }
                for (member_ordinal, member) in ctx
                    .admit_iter(
                        &use_.parasolid_group_members,
                        "NX partition group member traversal",
                    )?
                    .enumerate()
                {
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
            if let Some(use_) = ctx.find_by(
                body_write_group_partition_uses,
                |use_| {
                    ctx.equal_bytes(
                        use_.body_write.as_bytes(),
                        write.id.as_bytes(),
                        "NX group partition use writer identity equality",
                    )
                },
                "NX group partition writer use search",
            )? {
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
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_csys_uses_by_input_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys uses by input operation lookup",
        )? {
            for (use_ordinal, block_use) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations datum csys uses by input operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_csys_block_use.{use_ordinal}"),
                    format_args!("{}", block_use.id),
                )?;
            }
        }
        if let Some(dependency) = ctx.get_btree_map(
            &sketch_datum_csys_dependencies,
            label.id.as_str(),
            "NX attach feature operations sketch datum csys dependencies lookup",
        )? {
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
            for (alias_ordinal, alias) in ctx
                .admit_iter(
                    &dependency.scalar_aliases,
                    "NX datum scalar alias traversal",
                )?
                .enumerate()
            {
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
            match ctx.get_btree_map(
                &body_references,
                label.id.as_str(),
                "NX attach feature operations body references lookup",
            )? {
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
            let hole_bodies = match ctx.get_btree_map(
                &hole_outputs,
                label.id.as_str(),
                "NX attach feature operations hole outputs lookup",
            )? {
                Some(bodies) => Some(bodies),
                None => ctx.get_btree_map(
                    &hole_packages.outputs,
                    label.id.as_str(),
                    "NX attach feature operations hole packages outputs lookup",
                )?,
            };
            if let Some(bodies) = hole_bodies {
                outputs = ctx.collection_vec(bodies.len(), "NX feature output bodies")?;
                for body in ctx.admit_iter(bodies, "NX hole output body identities")? {
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
        let native_primary_body = ctx
            .get_btree_map(
                &body_references,
                label.id.as_str(),
                "NX attach feature operations body references lookup",
            )?
            .copied()
            .map(canonical_body)
            .transpose()?;
        let offset_store_primary_body = ctx
            .get_btree_map(
                &offset_store_bodies_by_operation,
                label.id.as_str(),
                "NX attach feature operations offset store bodies by operation lookup",
            )?
            .and_then(|uses| match uses.as_slice() {
                [(_, data_block)] => Some(data_block.as_str()),
                _ => None,
            });
        if let Some(body) = ctx.get_btree_map(
            &body_references,
            label.id.as_str(),
            "NX attach feature operations body references lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("{NATIVE_PRIMARY_BODY_OBJECT_INDEX}"),
                format_args!("{body}"),
            )?;
        }
        if let Some(reference) = ctx.get_btree_map(
            &body_writer_references_by_operation,
            label.id.as_str(),
            "NX attach feature operations body writer references by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("primary_body_reference"),
                format_args!("{}", reference.id),
            )?;
            if let Some(uses) = ctx.get_btree_map(
                &body_segment_uses_by_reference,
                reference.id.as_str(),
                "NX attach feature operations body segment uses by reference lookup",
            )? {
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
            if let Some(uses) = ctx.get_btree_map(
                &body_data_block_uses_by_reference,
                reference.id.as_str(),
                "NX attach feature operations body data block uses by reference lookup",
            )? {
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
        if let Some(feature_property_records) = ctx.get_btree_map(
            &body_reference_occurrences_by_operation,
            label.id.as_str(),
            "NX attach feature operations body reference occurrences by operation lookup",
        )? {
            for (reference, ordinal) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations body reference occurrences by operation traversal",
                )?
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
        }
        if let Some(inputs) = ctx.get_btree_map(
            &sketch_construction_inputs_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch construction inputs by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("sketch_construction_inputs"),
                format_args!("{}", inputs.id),
            )?;
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_records_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch records by operation lookup",
        )? {
            for (ordinal, record) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations sketch records by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_record.{ordinal}"),
                    format_args!("{}", record.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_construction_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch construction payloads by operation lookup",
        )? {
            for (ordinal, payload) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations sketch construction payloads by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_construction_payload.{ordinal}"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_coordinate_pairs_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch coordinate pairs by operation lookup",
        )? {
            for pair in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations sketch coordinate pairs by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_coordinate_pair.{}", pair.ordinal),
                    format_args!("{}", pair.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_fixed_pairs_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch fixed pairs by operation lookup",
        )? {
            for pair in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations sketch fixed pairs by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_fixed_pair.{}", pair.ordinal),
                    format_args!("{}", pair.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_mixed_pairs_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch mixed pairs by operation lookup",
        )? {
            for pair in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations sketch mixed pairs by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_mixed_pair.{}", pair.ordinal),
                    format_args!("{}", pair.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_payload_scalar_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch payload scalar lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations sketch payload scalar lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_scalar_lane.{}", lane.ordinal),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_fixed_points_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch fixed points by operation lookup",
        )? {
            for (ordinal, point) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations sketch fixed points by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_fixed_point.{ordinal}"),
                    format_args!("{}", point.id),
                )?;
            }
        }
        if let Some(construction) = ctx.get_btree_map(
            &block_constructions_by_operation,
            label.id.as_str(),
            "NX attach feature operations block constructions by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("block_construction"),
                format_args!("{}", construction.id),
            )?;
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &block_construction_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations block construction payloads by operation lookup",
        )? {
            for (ordinal, payload) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations block construction payloads by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("block_construction_payload.{ordinal}"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(dimensions) = ctx.get_btree_map(
            &block_dimensions_by_operation,
            label.id.as_str(),
            "NX attach feature operations block dimensions by operation lookup",
        )? {
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
        if let Some(feature_property_records) = ctx.get_btree_map(
            &block_payload_points_by_operation,
            label.id.as_str(),
            "NX attach feature operations block payload points by operation lookup",
        )? {
            for (ordinal, point) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations block payload points by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("block_payload_point.{ordinal}"),
                    format_args!("{}", point.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &block_payload_point_groups_by_operation,
            label.id.as_str(),
            "NX attach feature operations block payload point groups by operation lookup",
        )? {
            for (ordinal, group) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations block payload point groups by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("block_payload_point_group.{ordinal}"),
                    format_args!("{}", group.id),
                )?;
            }
        }
        if let Some(construction) = ctx.get_btree_map(
            &extrude_32_constructions_by_operation,
            label.id.as_str(),
            "NX attach feature operations extrude 32 constructions by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("extrude_32_construction"),
                format_args!("{}", construction.id),
            )?;
        }
        if let Some(header) = ctx.get_btree_map(
            &extrude_payload_headers_by_operation,
            label.id.as_str(),
            "NX attach feature operations extrude payload headers by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("extrude_payload_header"),
                format_args!("{}", header.id),
            )?;
        }
        if let Some(lane) = ctx.get_btree_map(
            &operation_terminal_discriminators_by_operation,
            label.id.as_str(),
            "NX attach feature operations operation terminal discriminators by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("operation_terminal_discriminator"),
                format_args!("{}", lane.id),
            )?;
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &extrude_payload_32_branches_by_operation,
            label.id.as_str(),
            "NX attach feature operations extrude payload 32 branches by operation lookup",
        )? {
            for (ordinal, branch) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations extrude payload 32 branches by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("extrude_payload_32_branch.{ordinal}"),
                    format_args!("{}", branch.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &operation_body_scalar_triples_by_operation,
            label.id.as_str(),
            "NX attach feature operations operation body scalar triples by operation lookup",
        )? {
            for triple in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations operation body scalar triples by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &operation_body_members_by_operation,
            label.id.as_str(),
            "NX attach feature operations operation body members by operation lookup",
        )? {
            for member in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations operation body members by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &operation_body_11_continuations_by_operation,
            label.id.as_str(),
            "NX attach feature operations operation body 11 continuations by operation lookup",
        )? {
            for continuation in ctx.admit_iter(
                feature_property_records,"NX attach feature operations operation body 11 continuations by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &operation_body_reference_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations operation body reference lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,"NX attach feature operations operation body reference lanes by operation traversal",
            )? {
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
        }
        if let Some(construction) = ctx.get_btree_map(
            &datum_csys_constructions_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys constructions by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_csys_construction"),
                format_args!("{}", construction.id),
            )?;
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_csys_column_row_uses_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys column row uses by operation lookup",
        )? {
            for (ordinal, use_) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations datum csys column row uses by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_csys_column_row_use.{ordinal}"),
                    format_args!("{}", use_.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_csys_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys payloads by operation lookup",
        )? {
            for (ordinal, payload) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations datum csys payloads by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_csys_payload.{ordinal}"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_csys_payload_scalar_pairs_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys payload scalar pairs by operation lookup",
        )? {
            for (ordinal, pair) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations datum csys payload scalar pairs by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_csys_payload_scalar_pair.{ordinal}"),
                    format_args!("{}", pair.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_csys_payload_fixed_pairs_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys payload fixed pairs by operation lookup",
        )? {
            for (ordinal, pair) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations datum csys payload fixed pairs by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_csys_payload_fixed_pair.{ordinal}"),
                    format_args!("{}", pair.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_csys_payload_scalars_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys payload scalars by operation lookup",
        )? {
            for (ordinal, scalar) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations datum csys payload scalars by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_csys_payload_scalar.{ordinal}"),
                    format_args!("{}", scalar.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_csys_descriptors_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum csys descriptors by operation lookup",
        )? {
            for (ordinal, descriptor) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations datum csys descriptors by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_csys_descriptor.{ordinal}"),
                    format_args!("{}", descriptor.id),
                )?;
            }
        }
        if let Some(header) = ctx.get_btree_map(
            &datum_plane_headers_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum plane headers by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_plane_header"),
                format_args!("{}", header.id),
            )?;
        }
        if let Some(payload) = ctx.get_btree_map(
            &datum_plane_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum plane payloads by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("datum_plane_payload"),
                format_args!("{}", payload.id),
            )?;
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_plane_payload_scalar_pairs_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum plane payload scalar pairs by operation lookup",
        )? {
            for (ordinal, pair) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations datum plane payload scalar pairs by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_plane_payload_scalar_pair.{ordinal}"),
                    format_args!("{}", pair.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_plane_descriptors_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum plane descriptors by operation lookup",
        )? {
            for (ordinal, descriptor) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations datum plane descriptors by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_plane_descriptor.{ordinal}"),
                    format_args!("{}", descriptor.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_identity_uses_by_operation,
            label.id.as_str(),
            "NX attach feature operations datum identity uses by operation lookup",
        )? {
            for (ordinal, identity_use) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations datum identity uses by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_identity_use.{ordinal}"),
                    format_args!("{}", identity_use.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &datum_plane_uses_by_input_operation,
            label.id.as_str(),
            "NX attach feature operations datum plane uses by input operation lookup",
        )? {
            for (use_ordinal, block_use) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations datum plane uses by input operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("datum_plane_block_use.{use_ordinal}"),
                    format_args!("{}", block_use.id),
                )?;
            }
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
        for lane in ctx.admit_iter(
            hole_package_construction_group_lanes,
            "NX hole package construction lane traversal",
        )? {
            if !(ctx.equal_bytes(
                lane.operation_label.as_bytes(),
                label.id.as_bytes(),
                "NX hole construction lane operation equality",
            )?) {
                continue;
            }

            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("hole_package_construction_group_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        for group_use in ctx.admit_iter(
            hole_package_construction_group_uses,
            "NX hole package construction use traversal",
        )? {
            let group = ctx.find_by(
                simple_hole_construction_groups,
                |group| {
                    ctx.equal_bytes(
                        group.id.as_bytes(),
                        group_use.simple_hole_construction_group.as_bytes(),
                        "NX hole construction group identity equality",
                    )
                },
                "NX hole package construction group search",
            )?;
            if ctx.equal_bytes(
                group_use.operation_label.as_bytes(),
                label.id.as_bytes(),
                "NX hole construction use operation equality",
            )? {
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
            } else if group
                .map(|group| {
                    ctx.any_by(
                        &*group.members,
                        |member| {
                            ctx.equal_bytes(
                                member.operation_label.as_bytes(),
                                label.id.as_bytes(),
                                "NX hole construction member operation equality",
                            )
                        },
                        "NX hole package group operation membership",
                    )
                })
                .transpose()?
                .is_some_and(|matches| matches)
            {
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
        if let Some(feature_property_records) = ctx.get_btree_map(
            &input_blocks_by_operation,
            label.id.as_str(),
            "NX attach feature operations input blocks by operation lookup",
        )? {
            for input in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations input blocks by operation traversal",
            )? {
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
                if let Some(group) = ctx.get_btree_map(
                    &input_block_identity_group_by_input,
                    input.id.as_str(),
                    "NX attach feature operations input block identity group by input lookup",
                )? {
                    insert_source_property(
                        ctx,
                        &mut source_properties,
                        format_args!("input_block_identity_group.{}", input.input_slot),
                        format_args!("{}", (*group)),
                    )?;
                }
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &input_column_row_uses_by_operation,
            label.id.as_str(),
            "NX attach feature operations input column row uses by operation lookup",
        )? {
            for (ordinal, use_) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations input column row uses by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("input_column_row_use.{ordinal}"),
                    format_args!("{}", use_.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &input_column_targets_by_operation,
            label.id.as_str(),
            "NX attach feature operations input column targets by operation lookup",
        )? {
            for (ordinal, target) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations input column targets by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("input_column_target.{ordinal}"),
                    format_args!("{}", target.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_references_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch references by operation lookup",
        )? {
            for reference in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations sketch references by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &projected_curve_references_by_operation,
            label.id.as_str(),
            "NX attach feature operations projected curve references by operation lookup",
        )? {
            for reference in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations projected curve references by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &projected_curve_construction_payloads_by_operation,
            label.id.as_str(),"NX attach feature operations projected curve construction payloads by operation lookup",
        )? {
            for payload in ctx.admit_iter(
                feature_property_records,"NX attach feature operations projected curve construction payloads by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("projected_curve_construction_payload"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &projected_curve_construction_strings_by_operation,
            label.id.as_str(),
            "NX attach feature operations projected curve construction strings by operation lookup",
        )? {
            for value in ctx.admit_iter(
                feature_property_records,"NX attach feature operations projected curve construction strings by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("projected_curve_construction_string.{}", value.ordinal),
                    format_args!("{}", value.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &fset_reference_graphs_by_operation,
            label.id.as_str(),
            "NX attach feature operations fset reference graphs by operation lookup",
        )? {
            for graph in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations fset reference graphs by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("fset_reference_graph"),
                    format_args!("{}", graph.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &fset_construction_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations fset construction payloads by operation lookup",
        )? {
            for payload in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations fset construction payloads by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &delete_reference_fields_by_operation,
            label.id.as_str(),
            "NX attach feature operations delete reference fields by operation lookup",
        )? {
            for field in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations delete reference fields by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("delete_reference_field"),
                    format_args!("{}", field.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &delete_construction_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations delete construction payloads by operation lookup",
        )? {
            for payload in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations delete construction payloads by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("delete_construction_payload"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &pattern_references_by_operation,
            label.id.as_str(),
            "NX attach feature operations pattern references by operation lookup",
        )? {
            for reference in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations pattern references by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &pattern_construction_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations pattern construction payloads by operation lookup",
        )? {
            for payload in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations pattern construction payloads by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("pattern_construction_payload"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &pattern_counted_reference_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations pattern counted reference lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,"NX attach feature operations pattern counted reference lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("pattern_counted_reference_lane"),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &pattern_construction_strings_by_operation,
            label.id.as_str(),
            "NX attach feature operations pattern construction strings by operation lookup",
        )? {
            for value in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations pattern construction strings by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("pattern_construction_string.{}", value.ordinal),
                    format_args!("{}", value.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &pattern_construction_fixed_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations pattern construction fixed lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,"NX attach feature operations pattern construction fixed lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("pattern_construction_fixed_lane.{}", lane.ordinal),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &pattern_transform_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations pattern transform lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations pattern transform lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("pattern_transform_lane"),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &multi_instance_output_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations multi instance output lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations multi instance output lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("multi_instance_output_lane"),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &identical_instance_output_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations identical instance output lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,"NX attach feature operations identical instance output lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("identical_instance_output_lane"),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(header) = ctx.get_btree_map(
            &point_construction_headers_by_operation,
            label.id.as_str(),
            "NX attach feature operations point construction headers by operation lookup",
        )? {
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
        if let Some(lane) = ctx.get_btree_map(
            &point_construction_scalar_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations point construction scalar lanes by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("point_construction_scalar_lane"),
                format_args!("{}", lane.id),
            )?;
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_references_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction references by operation lookup",
        )? {
            for reference in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations draft construction references by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_index_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction index lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,"NX attach feature operations draft construction index lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("draft_construction_index_lane"),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction payloads by operation lookup",
        )? {
            for payload in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations draft construction payloads by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("draft_construction_payload"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_graph_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction graph payloads by operation lookup",
        )? {
            for payload in ctx.admit_iter(
                feature_property_records,"NX attach feature operations draft construction graph payloads by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("draft_construction_graph_payload"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_fixed_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction fixed lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,"NX attach feature operations draft construction fixed lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("draft_construction_fixed_lane.{}", lane.ordinal),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_binary32_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction binary32 lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,"NX attach feature operations draft construction binary32 lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("draft_construction_binary32_lane.{}", lane.ordinal),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_graph_strings_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction graph strings by operation lookup",
        )? {
            for value in ctx.admit_iter(
                feature_property_records,"NX attach feature operations draft construction graph strings by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("draft_construction_graph_string.{}", value.ordinal),
                    format_args!("{}", value.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_identity_frames_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction identity frames by operation lookup",
        )? {
            for frame in ctx.admit_iter(
                feature_property_records,"NX attach feature operations draft construction identity frames by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("draft_construction_identity_frame.{}", frame.ordinal),
                    format_args!("{}", frame.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &draft_construction_terminal_lanes_by_operation,
            label.id.as_str(),
            "NX attach feature operations draft construction terminal lanes by operation lookup",
        )? {
            for lane in ctx.admit_iter(
                feature_property_records,"NX attach feature operations draft construction terminal lanes by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("draft_construction_terminal_lane"),
                    format_args!("{}", lane.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &surface_construction_references_by_operation,
            label.id.as_str(),
            "NX attach feature operations surface construction references by operation lookup",
        )? {
            for reference in ctx.admit_iter(
                feature_property_records,"NX attach feature operations surface construction references by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &surface_construction_payloads_by_operation,
            label.id.as_str(),
            "NX attach feature operations surface construction payloads by operation lookup",
        )? {
            for payload in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations surface construction payloads by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("surface_construction_payload"),
                    format_args!("{}", payload.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &surface_construction_scalar_pairs_by_operation,
            label.id.as_str(),
            "NX attach feature operations surface construction scalar pairs by operation lookup",
        )? {
            for pair in ctx.admit_iter(
                feature_property_records,"NX attach feature operations surface construction scalar pairs by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("surface_construction_scalar_pair.{}", pair.ordinal),
                    format_args!("{}", pair.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &surface_construction_strings_by_operation,
            label.id.as_str(),
            "NX attach feature operations surface construction strings by operation lookup",
        )? {
            for value in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations surface construction strings by operation traversal",
            )? {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("surface_construction_string.{}", value.ordinal),
                    format_args!("{}", value.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &surface_construction_branches_by_operation,
            label.id.as_str(),
            "NX attach feature operations surface construction branches by operation lookup",
        )? {
            for branch in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations surface construction branches by operation traversal",
            )? {
                for (ordinal, (token, data_block)) in ctx
                    .admit_iter(
                        branch.references.members().as_slice(),
                        "NX surface branch member traversal",
                    )?
                    .enumerate()
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_named_point_uses_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch named point uses by operation lookup",
        )? {
            for (ordinal, block_use) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations sketch named point uses by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_named_point_block_use.{ordinal}"),
                    format_args!("{}", block_use.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_preceding_named_point_uses_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch preceding named point uses by operation lookup",
        )? {
            for (ordinal, point_use) in ctx
                .admit_iter(
                    feature_property_records,"NX attach feature operations sketch preceding named point uses by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_preceding_named_point_use.{ordinal}"),
                    format_args!("{}", point_use.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_point_uses_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch point uses by operation lookup",
        )? {
            for (ordinal, point_use) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations sketch point uses by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_point_use.{ordinal}"),
                    format_args!("{}", point_use.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &sketch_point_groups_by_operation,
            label.id.as_str(),
            "NX attach feature operations sketch point groups by operation lookup",
        )? {
            for (ordinal, group) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations sketch point groups by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("sketch_point_group.{ordinal}"),
                    format_args!("{}", group.id),
                )?;
            }
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &extrude_profile_references_by_operation,
            label.id.as_str(),
            "NX attach feature operations extrude profile references by operation lookup",
        )? {
            for reference in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations extrude profile references by operation traversal",
            )? {
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
        }
        if let Some(profile) = ctx.get_btree_map(
            &extrude_construction_profiles_by_operation,
            label.id.as_str(),
            "NX attach feature operations extrude construction profiles by operation lookup",
        )? {
            insert_source_property(
                ctx,
                &mut source_properties,
                format_args!("extrude_construction_profile"),
                format_args!("{}", profile.id),
            )?;
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &operation_body_operands_by_operation,
            label.id.as_str(),
            "NX attach feature operations operation body operands by operation lookup",
        )? {
            for operand in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations operation body operands by operation traversal",
            )? {
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
                for (binding_ordinal, binding) in ctx
                    .admit_iter(
                        &operand.segment_body_bindings,
                        "NX operand segment binding traversal",
                    )?
                    .enumerate()
                {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &parameter_bindings_by_operation,
            label.id.as_str(),
            "NX attach feature operations parameter bindings by operation lookup",
        )? {
            for binding in ctx.admit_iter(
                feature_property_records,
                "NX attach feature operations parameter bindings by operation traversal",
            )? {
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
        }
        if let Some(feature_property_records) = ctx.get_btree_map(
            &parameter_uses_by_operation,
            label.id.as_str(),
            "NX attach feature operations parameter uses by operation lookup",
        )? {
            for (ordinal, parameter_use) in ctx
                .admit_iter(
                    feature_property_records,
                    "NX attach feature operations parameter uses by operation traversal",
                )?
                .enumerate()
            {
                insert_source_property(
                    ctx,
                    &mut source_properties,
                    format_args!("parameter_use.{ordinal}"),
                    format_args!("{}", parameter_use.id),
                )?;
            }
        }
        let operation_payload_string_records = ctx
            .get_btree_map(
                &payload_strings_by_operation,
                label.id.as_str(),
                "NX attach feature operations payload strings by operation lookup",
            )?
            .map_or([].as_slice(), Vec::as_slice);

        let (mut operation_payload_strings, _payload_reservation) = ctx.temporary_vec(
            operation_payload_string_records.len(),
            "NX operation payload string references",
        )?;
        operation_payload_strings.extend(
            ctx.admit_iter(
                operation_payload_string_records,
                "NX operation payload string visits",
            )?
            .map(|value| value.value.as_str()),
        );
        let block_dimension_values = ctx
            .get_btree_map(
                &block_dimensions_by_operation,
                label.id.as_str(),
                "NX attach feature operations block dimensions by operation lookup",
            )?
            .map(|dimensions| {
                dimensions
                    .dimensions
                    .each_ref()
                    .map(|dimension| dimension.value.get())
            });
        let (block_projection, _block_projection_storage) =
            ctx.with_scoped_storage("NX block construction evidence", || {
                match (label.value.as_str(), block_dimension_values) {
                    ("BLOCK", Some(dimensions)) => block_placement(ctx, ir, dimensions, &outputs),
                    _ => Ok(None),
                }
            })?;
        if outputs.is_empty() {
            if let Some((body, _)) = &block_projection {
                ctx.reserve_vec(&mut outputs, 1, "NX block output bodies")?;
                outputs.push(body.try_clone_for_decode(ctx, "NX block output body identity")?);
            }
        }
        let (sphere_projection, _sphere_projection_storage) =
            ctx.with_scoped_storage("NX sphere construction evidence", || {
                if label.value == "SPHERE" {
                    sphere_body_projection(ctx, ir, &outputs)
                } else {
                    Ok(None)
                }
            })?;
        let sphere_outputs = if outputs.is_empty() {
            sphere_projection
                .as_ref()
                .map_or([].as_slice(), |(body, _, _)| std::slice::from_ref(body))
        } else {
            outputs.as_slice()
        };
        let body_reference_count = ctx
            .get_btree_map(
                &body_reference_occurrences_by_operation,
                label.id.as_str(),
                "NX attach feature operations body reference occurrences by operation lookup",
            )?
            .map_or(0, Vec::len);
        let block_op = new_body_boolean_op(
            ctx,
            &NewBodyEvidence {
                has_complete_projection: block_projection.is_some(),
                has_complete_primitive_construction: match ctx.get_btree_map(
                    &block_constructions_by_operation,
                    label.id.as_str(),"NX attach feature operations block constructions by operation lookup",
                )? {
                    Some(construction) => match ctx
                        .get_btree_map(
                            &block_construction_payloads_by_operation,
                            label.id.as_str(),"NX attach feature operations block construction payloads by operation lookup",
                        )?
                        .map(Vec::as_slice)
                    {
                        Some([payload]) => match &payload.owner {
                            crate::native::features::FeatureConstructionOwner::Block {
                                construction: owner,
                            } => ctx.equal_bytes(owner.as_str().as_bytes(), construction.id.as_bytes(), "NX block construction payload owner equality")?,
                            _ => false,
                        },
                        _ => false,
                    },
                    None => false,
                },
                outputs: &outputs,
                body_reference_count,
                provisional_feature: initial_body_id.as_ref(),
                native_primary_body,
                offset_store_primary_body,
                history: &body_writer_history,
            },
        )?;
        let sphere_op = if sphere_projection.is_some() {
            new_body_boolean_op(
                ctx,
                &NewBodyEvidence {
                    has_complete_projection: true,
                    has_complete_primitive_construction: true,
                    outputs: sphere_outputs,
                    body_reference_count,
                    provisional_feature: initial_body_id.as_ref(),
                    native_primary_body,
                    offset_store_primary_body,
                    history: &body_writer_history,
                },
            )?
        } else {
            BooleanOp::Unresolved
        };
        if sphere_op == BooleanOp::NewBody && outputs.is_empty() {
            if let Some((body, _, _)) = &sphere_projection {
                ctx.reserve_vec(&mut outputs, 1, "NX sphere output bodies")?;
                outputs.push(body.try_clone_for_decode(ctx, "NX sphere output body identity")?);
            }
        }
        if block_op == BooleanOp::NewBody || sphere_op == BooleanOp::NewBody {
            if let Some(initial_feature_index) = initial_feature_index {
                let initial_feature = &mut ir.model.features[initial_feature_index];
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
                body_writer_history.retract_outputs(ctx, &initial_feature.id, &outputs)?;
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
                ctx.get_btree_map(
                    &body_references,
                    label.id.as_str(),
                    "NX attach feature operations body references lookup",
                )?
                .copied(),
                ctx.get_btree_map(
                    &offset_store_bodies_by_operation,
                    label.id.as_str(),
                    "NX attach feature operations offset store bodies by operation lookup",
                )?
                .map_or([].as_slice(), Vec::as_slice),
                ctx.get_btree_map(
                    &operation_body_operands_by_operation,
                    label.id.as_str(),
                    "NX attach feature operations operation body operands by operation lookup",
                )?
                .map_or([].as_slice(), Vec::as_slice),
                &body_alias_roots,
                &bodies_by_object_index,
            )?
        } else {
            None
        };
        let trim_body_projection = if label.value == "TRIM BODY" {
            if let Some(primary) = ctx.get_btree_map(
                &body_references,
                label.id.as_str(),
                "NX attach feature operations body references lookup",
            )? {
                Some(trim_body_feature_definition(
                    ctx,
                    *primary,
                    ctx.get_btree_map(
                        &operation_body_operands_by_operation,
                        label.id.as_str(),
                        "NX attach feature operations operation body operands by operation lookup",
                    )?
                    .map_or([].as_slice(), Vec::as_slice),
                    &body_alias_roots,
                    &bodies_by_object_index,
                )?)
            } else {
                offset_store_trim_body_feature_definition(
                    ctx,
                    ctx.get_btree_map(
                        &offset_store_bodies_by_operation,
                        label.id.as_str(),
                        "NX attach feature operations offset store bodies by operation lookup",
                    )?
                    .map_or([].as_slice(), Vec::as_slice),
                    ctx.get_btree_map(
                        &operation_body_operands_by_operation,
                        label.id.as_str(),
                        "NX attach feature operations operation body operands by operation lookup",
                    )?
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
            for (support_ordinal, support) in ctx
                .admit_iter(&supports.values, "NX feature support source properties")?
                .enumerate()
            {
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
            for (support_ordinal, support) in ctx
                .admit_iter(&supports.values, "NX feature support source properties")?
                .enumerate()
            {
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
            for (surface_ordinal, surface) in ctx
                .admit_iter(&surfaces.values, "NX blend source surfaces")?
                .enumerate()
            {
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
            let mut records_iter = outputs.iter();
            while let Some(output) =
                ctx.next_charged(&mut records_iter, "NX extrude output traversal")?
            {
                let Some(kind) = ctx.get_btree_map(
                    &body_kinds_by_id,
                    output.as_str(),
                    "NX extrude output body lookup",
                )?
                else {
                    output_kinds.clear();
                    break;
                };
                ctx.reserve_scoped_vec(
                    &mut output_kind_reservation,
                    &mut output_kinds,
                    1,
                    "NX extrude output body kinds",
                )?;
                output_kinds.push(*kind);
            }
            let op = extrude_boolean_op(
                ctx,
                &body_writer_history,
                native_primary_body,
                offset_store_primary_body,
                &output_kinds,
            )?;
            let construction_profile = ctx
                .get_btree_map(
                    &extrude_construction_profiles_by_operation,
                    label.id.as_str(),"NX attach feature operations extrude construction profiles by operation lookup",
                )?
                .map(|profile| profile.id.as_str());
            let structured_construction = ctx
                .get_btree_map(
                    &extrude_32_constructions_by_operation,
                    label.id.as_str(),
                    "NX attach feature operations extrude 32 constructions by operation lookup",
                )?
                .map(|construction| construction.id.as_str());
            Some(extrude_feature_definition(
                ctx,
                construction_profile,
                structured_construction,
                op,
                &output_kinds,
            )?)
        } else {
            None
        };
        let delete_projection = if deletes_body {
            let field = match ctx.get_btree_map(
                &body_references,
                label.id.as_str(),
                "NX attach feature operations body references lookup",
            )? {
                Some(body) => Some(DeleteBodyField::Native(*body)),
                None => ctx
                    .get_btree_map(
                        &offset_store_bodies_by_operation,
                        label.id.as_str(),
                        "NX attach feature operations offset store bodies by operation lookup",
                    )?
                    .and_then(|uses| match uses.as_slice() {
                        [(object_index, data_block)] => Some(DeleteBodyField::OffsetStore {
                            object_index: *object_index,
                            data_block,
                        }),
                        _ => None,
                    }),
            };
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
                ctx.get_btree_map(
                    &body_references,
                    label.id.as_str(),
                    "NX attach feature operations body references lookup",
                )?
                .copied(),
                ctx.get_btree_map(
                    &offset_store_bodies_by_operation,
                    label.id.as_str(),
                    "NX attach feature operations offset store bodies by operation lookup",
                )?
                .map_or([].as_slice(), Vec::as_slice),
                &body_alias_roots,
                &bodies_by_object_index,
            )?)
        } else {
            None
        };
        let operation_parameter_uses = ctx
            .get_btree_map(
                &parameter_uses_by_operation,
                label.id.as_str(),
                "NX attach feature operations parameter uses by operation lookup",
            )?
            .map_or([].as_slice(), Vec::as_slice);
        let sketch = if label.value == "SKETCH" {
            attach_sketch_graph(
                ctx,
                ir,
                label,
                &SketchSources {
                    point_uses: ctx
                        .get_btree_map(
                            &sketch_point_uses_by_operation,
                            label.id.as_str(),"NX attach feature operations sketch point uses by operation lookup",
                        )?
                        .map_or([].as_slice(), Vec::as_slice),
                    point_groups: sketch_point_groups,
                    points: sketch_points,
                    payload_scalars: sketch_payload_scalars,
                    fixed_points: ctx
                        .get_btree_map(
                            &sketch_fixed_points_by_operation,
                            label.id.as_str(),"NX attach feature operations sketch fixed points by operation lookup",
                        )?
                        .map_or([].as_slice(), Vec::as_slice),
                    coordinate_pairs: ctx
                        .get_btree_map(
                            &sketch_coordinate_pairs_by_operation,
                            label.id.as_str(),"NX attach feature operations sketch coordinate pairs by operation lookup",
                        )?
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
                ctx,
                &label.value,
                &label.objects.values(),
                &outputs,
                [
                    ctx.get_btree_map(
                        &body_reference_occurrences_by_operation,
                        label.id.as_str(),
                        "NX attach feature operations body reference occurrences by operation lookup",
                    )?
                    .map_or(0, Vec::len),
                    ctx.get_btree_map(
                        &operation_body_operands_by_operation,
                        label.id.as_str(),
                        "NX attach feature operations operation body operands by operation lookup",
                    )?
                    .map_or(0, Vec::len),
                    operation_payload_string_records.len(),
                ],
                &source_properties,
            )?;
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
                let hole = if matches!(
                    label.value.as_str(),
                    "SIMPLE HOLE" | "CBORE_HOLE" | "CSUNK_HOLE" | "HOLE PACKAGE"
                ) {
                    let mut placements = Vec::new();
                    for source in [
                        ctx.get_btree_map(
                            &simple_hole_placements,
                            label.id.as_str(),
                            "NX attach feature operations simple hole placements lookup",
                        )?
                        .map_or([].as_slice(), std::slice::from_ref),
                        ctx.get_btree_map(
                            &counterbore_hole_placements,
                            label.id.as_str(),
                            "NX attach feature operations counterbore hole placements lookup",
                        )?
                        .map_or([].as_slice(), std::slice::from_ref),
                        ctx.get_btree_map(
                            &blind_hole_placements,
                            label.id.as_str(),
                            "NX attach feature operations blind hole placements lookup",
                        )?
                        .map_or([].as_slice(), std::slice::from_ref),
                        ctx.get_btree_map(
                            &hole_packages.placements,
                            label.id.as_str(),
                            "NX attach feature operations hole packages placements lookup",
                        )?
                        .map_or([].as_slice(), Vec::as_slice),
                    ] {
                        for placement in ctx.admit_iter(source, "NX hole placement traversal")? {
                            ctx.reserve_vec(&mut placements, 1, "NX feature hole placements")?;
                            placements.push(placement.try_clone_for_decode(
                                ctx,
                                "NX attach feature operations placement copy",
                            )?);
                        }
                    }
                    HoleProjection {
                        placements,
                        diameter: match ctx.get_btree_map(
                            &simple_hole_diameters,
                            label.id.as_str(),
                            "NX attach feature operations simple hole diameters lookup",
                        )? {
                            Some(value) => Some(*value),
                            None => ctx
                                .get_btree_map(
                                    &hole_packages.diameters,
                                    label.id.as_str(),
                                    "NX attach feature operations hole packages diameters lookup",
                                )?
                                .copied(),
                        },
                        extent: ctx
                            .get_btree_map(
                                &blind_hole_depths,
                                label.id.as_str(),
                                "NX attach feature operations blind hole depths lookup",
                            )?
                            .copied()
                            .map(|length| LinearTermination::Blind { length }),
                        counterbore: ctx
                            .get_btree_map(
                                &counterbore_dimensions,
                                label.id.as_str(),
                                "NX attach feature operations counterbore dimensions lookup",
                            )?
                            .copied(),
                        chamfer: match ctx.get_btree_map(
                            &simple_hole_chamfers,
                            label.id.as_str(),
                            "NX attach feature operations simple hole chamfers lookup",
                        )? {
                            Some(value) => Some(*value),
                            None => ctx
                                .get_btree_map(
                                    &hole_packages.chamfers,
                                    label.id.as_str(),
                                    "NX attach feature operations hole packages chamfers lookup",
                                )?
                                .copied(),
                        },
                        grouped_simple_through: ctx.contains_key_btree_map(
                            &hole_packages.outputs,
                            label.id.as_str(),
                            "NX attach feature operations hole packages outputs membership",
                        )?,
                    }
                } else {
                    HoleProjection::default()
                };
                non_boolean_feature_definition_with_parameters(
                    ctx,
                    &label.value,
                    &operation_payload_strings,
                    block_dimension_values,
                    block_placement,
                    hole,
                    || {
                        let (parameters, _parameter_nodes) =
                            native_feature_parameters(ctx, operation_parameter_uses, expressions)?;
                        Ok(cadmpeg_core::text::named_entries_for_decode(
                            ctx, &label.id, parameters,
                        )?)
                    },
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
        for parameter_use in ctx.admit_iter(
            operation_parameter_uses,
            "NX operation parameter use traversal",
        )? {
            push_referenced_parameter(
                ctx,
                &mut parameter_reservation,
                &mut referenced_parameters,
                &parameter_use.expression,
            )?;
        }
        if let Some(dimensions) = ctx.get_btree_map(
            &block_dimensions_by_operation,
            label.id.as_str(),
            "NX attach feature operations block dimensions by operation lookup",
        )? {
            for dimension in &dimensions.dimensions {
                push_referenced_parameter(
                    ctx,
                    &mut parameter_reservation,
                    &mut referenced_parameters,
                    &dimension.expression,
                )?;
            }
        }
        let (owners, _owner_storage) = ctx
            .with_scoped_storage("NX parameter owner dependency staging", || {
                parameter_owner_dependencies(ctx, &parameter_owners, &referenced_parameters)
            })?;
        for owner in ctx.admit_iter(&owners, "NX parameter owner dependency traversal")? {
            push_unique_feature_dependency(ctx, &mut dependencies, owner)?;
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
        body_writer_history_storage.with_storage(|| {
            body_writer_history.record_writer(
                ctx,
                native_output,
                offset_store_output,
                &outputs,
                &id,
            )
        })?;
        for write in ctx.admit_iter(operation_body_writes, "NX operation body writer traversal")? {
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
            .then(|| {
                ctx.get_btree_map(
                    &booleans,
                    label.id.as_str(),
                    "NX attach feature operations booleans lookup",
                )
            })
            .transpose()?
            .flatten()
        {
            // A Boolean target writes its selected body image even when the
            // operation has no separate primary-body field.
            if !matches!(
                boolean_offset_store_resolution.as_ref(),
                Some(BooleanOffsetStoreResolution::Unresolved)
            ) {
                let (native_target, offset_store_target) = boolean_target_writer(
                    ctx,
                    &definition,
                    canonical_body(operation.target.token.value())?,
                )?;
                body_writer_history_storage.with_storage(|| {
                    body_writer_history.record_writer(
                        ctx,
                        native_target,
                        offset_store_target,
                        &[],
                        &id,
                    )
                })?;
            }
        }
        ctx.charge_collection_items(1, "NX feature records")?;
        ctx.reserve_capacity(&mut ir.model.features, 1, "allocate NX feature records")?;
        ir.model.features.push(Feature {
            id: id.try_clone_for_decode(ctx, "NX attach feature operations id copy")?,
            ordinal: base_ordinal + cadmpeg_core::decode::u64_from_index(ordinal),
            name: Some(ctx.copy_retained_text(&label.value, "NX feature record")?),
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies, ctx)
                .map_err(cadmpeg_core::CodecError::from)?,
            source_properties: cadmpeg_core::text::named_entries_for_decode(
                ctx,
                &label.id,
                source_properties,
            )?,
            source_tag: Some(ctx.copy_retained_text(&label.value, "NX feature record")?),
            source_text: None,
            source_content,

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                definition,
                DistinctMembers::try_from(outputs, ctx).map_err(CodecError::from)?,
            ),
            native_ref: Some(ctx.copy_retained_text(&label.id, "NX feature record")?),
        });
        if !deletes_body && !operation_body_writes.is_empty() {
            let key = label
                .id
                .strip_prefix("nx:feature-history:operation-label#")
                .unwrap_or(label.id.as_str());
            for write in
                ctx.admit_iter(operation_body_writes, "NX operation body writer traversal")?
            {
                const BODY_PREFIX: &str = "nx:feature-history:body-identity#";
                let result_members = operation_body_write_result_group_members(
                    ctx,
                    write.id.as_str(),
                    operation_body_partition_uses,
                    body_write_group_partition_uses,
                    parasolid_group_members,
                )?;
                ctx.charge_collection_items(1, "NX feature result bodies")?;
                let body_text = ctx.format_retained(
                    format_args!("{BODY_PREFIX}{:010}", write.frame.body_identity()),
                    "NX feature result body",
                )?;
                let body = cadmpeg_core::text::NonBlankString::for_decode(
                    ctx,
                    body_text,
                    "NX attach feature operations body text validation",
                )?
                .ok_or_else(|| CodecError::malformed("NX feature result body identity is blank"))?;
                let mut bodies = Vec::new();
                ctx.reserve_capacity(&mut bodies, 1, "allocate NX feature result bodies")?;
                bodies.push(body);
                let mut native_ref =
                    ctx.retained_string(write.id.len(), "NX result topology native reference")?;
                ctx.append_retained(
                    &mut native_ref,
                    &write.id,
                    "NX attach feature operations write id append",
                )?;
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
                ctx.get_btree_map(
                    &body_writer_references_by_operation,
                    label.id.as_str(),
                    "NX attach feature operations body writer references by operation lookup",
                )?
                .copied(),
                ctx.get_btree_map(
                    &booleans,
                    label.id.as_str(),
                    "NX attach feature operations booleans lookup",
                )?
                .copied(),
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
            if let Some(initial_feature_index) = initial_feature_index {
                const WITNESS_VALUE: &str = "primary-body-relations";
                let initial_feature = &mut ir.model.features[initial_feature_index];

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
                    cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        key,
                        "NX attach feature operations key validation",
                    )?
                    .ok_or_else(|| {
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
        let has_outputs = initial_feature_index
            .is_some_and(|index| !ir.model.features[index].evaluation.outputs().is_empty());
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

#[cfg(test)]
impl DecodeCost for FeatureResultGroupMembers {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (&self.faces, &self.edges, &self.vertices).decode_cost(ctx, operation)
    }
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
        cadmpeg_core::decode::u64_from_index(suffix_len),
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
    ctx.append_retained(&mut owned_key, key, "NX result topology key formatting")?;
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
    let members = match cadmpeg_ir::features::FeatureResultMembers::new(
        bodies,
        members.faces,
        members.edges,
        members.vertices,
        ctx,
        "NX result topology member validation",
    )? {
        Ok(members) => members,
        Err(error) => {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("{error}"),
                "NX result topology member error",
            )?))
        }
    };
    ctx.reserve_vec_limit(
        &mut ir.model.feature_result_topologies,
        1,
        "allocate NX result topology records",
    )?;
    let result = FeatureResultTopology::new(
        result_id,
        output_of.try_clone_for_decode(ctx, "NX append feature result topology output of copy")?,
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
    let mut image_use = None;
    let mut records_iter = image_partition_uses.iter();
    while let Some(use_) =
        ctx.next_charged(&mut records_iter, "NX result image partition use lookup")?
    {
        if ctx.equal_bytes(
            use_.operation_body_write.as_bytes(),
            body_write.as_bytes(),
            "NX operation body write result group members equality",
        )? {
            if image_use.is_some() {
                return Ok(FeatureResultGroupMembers::default());
            }
            image_use = Some(use_);
        }
    }
    let (image_members, image_storage) =
        ctx.with_scoped_storage("NX image result group candidate", || {
            image_use
                .map(|use_| {
                    feature_result_group_members(
                        ctx,
                        use_.partition_stream_ordinal,
                        &use_.parasolid_group_members,
                        members,
                    )
                })
                .transpose()
        })?;
    let mut group_use = None;
    let mut records_iter = group_partition_uses.iter();
    while let Some(use_) =
        ctx.next_charged(&mut records_iter, "NX result group partition use lookup")?
    {
        if ctx.equal_bytes(
            use_.body_write.as_bytes(),
            body_write.as_bytes(),
            "NX operation body write result group members equality",
        )? {
            if group_use.is_some() {
                return Ok(FeatureResultGroupMembers::default());
            }
            group_use = Some(use_);
        }
    }
    let (group_members, group_storage) =
        ctx.with_scoped_storage("NX group result group candidate", || {
            group_use
                .map(|use_| {
                    feature_result_group_members(
                        ctx,
                        use_.partition_stream_ordinal,
                        &use_.parasolid_group_members,
                        members,
                    )
                })
                .transpose()
        })?;
    Ok(match (image_members, group_members) {
        (Some(image), Some(group))
            if image.faces.len() == group.faces.len()
                && image.edges.len() == group.edges.len()
                && image.vertices.len() == group.vertices.len()
                && ctx.all_by(
                    image
                        .faces
                        .iter()
                        .zip(&group.faces)
                        .chain(image.edges.iter().zip(&group.edges))
                        .chain(image.vertices.iter().zip(&group.vertices)),
                    |(first, second)| {
                        ctx.equal_bytes(
                            first.as_str().as_bytes(),
                            second.as_str().as_bytes(),
                            "NX result group member identity",
                        )
                    },
                    "NX result group member equality",
                )? =>
        {
            image_storage.commit()?;
            image
        }
        (Some(_), Some(_)) => FeatureResultGroupMembers::default(),
        (Some(image), None) => {
            image_storage.commit()?;
            image
        }
        (None, Some(group)) => {
            group_storage.commit()?;
            group
        }
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
    let mut records_iter = member_ids.iter();
    while let Some(member_id) = ctx.next_charged(
        &mut records_iter,
        "NX feature result group member identities",
    )? {
        let mut matching_member = None;
        let mut multiple_members = false;
        let mut records_iter = members.iter();
        while let Some(member) =
            ctx.next_charged(&mut records_iter, "NX feature result group member lookup")?
        {
            if ctx.equal_bytes(
                member.id.as_bytes(),
                member_id.as_bytes(),
                "NX feature result group members equality",
            )? {
                if matching_member.is_some() {
                    multiple_members = true;
                    break;
                }
                matching_member = Some(member);
            }
        }
        let Some(member) = matching_member else {
            continue;
        };
        if multiple_members {
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
        ctx.charge_collection_items(1, "NX feature result group members")?;
        ctx.reserve_capacity(output, 1, "allocate NX feature result group members")?;
        let text = ctx.format_retained(
            format_args!("nx:s{partition_stream_ordinal}:{kind}#{xmt}"),
            "NX feature result group member identity",
        )?;
        let identity = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            text,
            "NX feature result group members text validation",
        )?
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
    let local = ctx.format_retained(
        format_args!("{native}{suffix}"),
        "NX result body local identity",
    )?;
    let Some(local) = cadmpeg_core::text::NonBlankString::for_decode(
        ctx,
        local,
        "NX native result body identity local validation",
    )?
    else {
        return Ok(None);
    };
    let mut native_ref = String::new();
    ctx.try_reserve_retained_text(&mut native_ref, native.len(), "NX result body identity")?;
    ctx.append_retained(
        &mut native_ref,
        native,
        "NX native result body identity native append",
    )?;
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
    for use_ in ctx.admit_iter(data_block_uses, "NX primary data-block uses")? {
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
    for use_ in ctx.admit_iter(segment_uses, "NX primary segment uses")? {
        bridge_reservation.with_storage(|| {
            ctx.insert_btree_set(
                &mut bridged_segment_references,
                use_.feature_body_reference.as_str(),
                "NX primary bridged references",
            )
        })?;
    }
    let mut output = BTreeMap::new();
    for (&operation, reference) in
        ctx.admit_iter(&unique_references, "NX primary unique references")?
    {
        if !ctx.contains_btree_set(
            &bridged_segment_references,
            reference.id.as_str(),
            "NX bridged primary body membership",
        )? && (ctx.contains_btree_set(
            &offset_store_references,
            reference.id.as_str(),
            "NX offset primary body membership",
        )? || ctx.contains_btree_set(
            &offset_store_operations,
            reference.operation_label.as_str(),
            "NX offset primary operation membership",
        )?) {
            continue;
        }

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
    for group in ctx.admit_iter(sources.point_groups, "NX sketch point groups")? {
        if !(ctx.equal_bytes(
            group.operation_label.as_bytes(),
            label.id.as_bytes(),
            "NX sketch point group operation equality",
        )?) {
            continue;
        }

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
    for &point in ctx.admit_iter(sources.fixed_points, "NX sketch fixed-point candidates")? {
        if !(ctx.equal_bytes(
            point.operation_label.as_bytes(),
            label.id.as_bytes(),
            "NX sketch fixed point operation equality",
        )?) {
            continue;
        }

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
        for &pair in ctx.admit_iter(
            sources.coordinate_pairs,
            "NX sketch coordinate-pair candidates",
        )? {
            if !(ctx.equal_bytes(
                pair.operation_label.as_bytes(),
                label.id.as_bytes(),
                "NX sketch coordinate pair operation equality",
            )?) {
                continue;
            }

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
        let mut records_iter = coordinate_pairs.iter().copied();
        while let Some(pair) = ctx.next_charged(&mut records_iter, "NX sketch coordinate pairs")? {
            if !ctx.insert_scoped_btree_set(
                &mut reservation,
                &mut pair_ids,
                pair.id.as_str(),
                "NX coordinate pair identity uniqueness",
                "NX coordinate pair identity index",
            )? || !ctx.insert_scoped_btree_set(
                &mut reservation,
                &mut pair_ordinals,
                (pair.payload.id(), pair.ordinal),
                "NX coordinate pair payload ordinal uniqueness",
                "NX coordinate pair payload ordinal index",
            )? {
                return Ok(None);
            }
            let pair_key = ctx
                .rsplit_once(&pair.id, "#", "NX sketch pair identity suffix")?
                .map_or(pair.id.as_str(), |(_, key)| key);
            if pair_key.is_empty()
                || ctx.any_by(
                    pair_key.chars(),
                    |ch| Ok(ch.is_whitespace()),
                    "NX sketch pair identity characters",
                )?
                || !ctx.insert_scoped_btree_set(
                    &mut reservation,
                    &mut pair_entity_keys,
                    pair_key,
                    "NX coordinate pair entity key uniqueness",
                    "NX coordinate pair entity key index",
                )?
            {
                return Ok(None);
            }
            let Some(entity_id) = sketch_entity_identity(ctx, "coordinate-pair-", pair_key)? else {
                return Ok(None);
            };
            let native_kind = cadmpeg_core::nonblank_literal!("nx-coordinate-pair");
            let native_ref =
                ctx.copy_retained_text(&pair.id, "NX attach sketch graph pair id copy")?;
            push_sketch_entity(
                ctx,
                &mut reservation,
                &mut entities,
                pair.source_offset,
                SketchEntity::new(
                    entity_id,
                    sketch_id.try_clone_for_decode(ctx, "NX attach sketch graph sketch id copy")?,
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
            |value| value.1.id(),
            Ord::cmp,
            "NX coordinate sketch entity identity order",
        )?;
        ctx.stable_sort_by(
            &mut entities,
            |value| &value.0,
            Ord::cmp,
            "NX coordinate sketch entity source order",
        )?;
        for (source_offset, entity) in ctx.admit_iter(&entities, "NX sketch entity annotations")? {
            let tag = match entity.geometry.definition() {
                SketchGeometryDefinition::Native { native_kind }
                    if native_kind.as_str() == "nx-coordinate-pair" =>
                {
                    "SKETCH_NATIVE_COORDINATE_PAIR"
                }
                SketchGeometryDefinition::Native { native_kind }
                    if native_kind.as_str() == "nx-fixed-point" =>
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
    let mut records_iter = operation_groups.iter();
    while let Some(group) =
        ctx.next_charged(&mut records_iter, "NX sketch operation group index")?
    {
        if !ctx.insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut groups_by_id,
            group.id.as_str(),
            group,
            "NX sketch point group record uniqueness",
            "NX sketch point group record index",
        )? {
            return Ok(None);
        }
    }
    let mut point_uses_by_group =
        BTreeMap::<&str, &crate::native::features::FeatureSketchPointUse>::new();
    let mut records_iter = sources.point_uses.iter();
    while let Some(point_use) = ctx.next_charged(&mut records_iter, "NX sketch point uses")? {
        if !ctx.equal_bytes(
            point_use.operation_label.as_bytes(),
            label.id.as_bytes(),
            "NX sketch point use operation equality",
        )? || !ctx.insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut point_uses_by_group,
            point_use.sketch_point_group.as_str(),
            point_use,
            "NX sketch point use record uniqueness",
            "NX sketch point use record index",
        )? {
            return Ok(None);
        }
    }
    let mut records_iter = point_uses_by_group.iter();
    while let Some((group, _)) =
        ctx.next_charged(&mut records_iter, "NX sketch point-use group scan")?
    {
        if !ctx.contains_key_btree_map(
            &groups_by_id,
            group,
            "NX attach sketch graph groups by id membership",
        )? {
            return Ok(None);
        }
    }
    let mut points_by_id = BTreeMap::<&str, &crate::native::features::FeatureSketchPoint>::new();
    let mut records_iter = sources.points.iter();
    while let Some(point) = ctx.next_charged(&mut records_iter, "NX sketch point records")? {
        if !ctx.insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut points_by_id,
            point.id.as_str(),
            point,
            "NX sketch point record uniqueness",
            "NX sketch point record index",
        )? {
            return Ok(None);
        }
    }
    let mut scalars_by_id = BTreeMap::<&str, &crate::native::features::FeaturePayloadScalar>::new();
    let mut records_iter = sources.payload_scalars.iter();
    while let Some(scalar) = ctx.next_charged(&mut records_iter, "NX sketch payload scalars")? {
        if !ctx.insert_scoped_btree_map_if_vacant(
            &mut reservation,
            &mut scalars_by_id,
            scalar.id.as_str(),
            scalar,
            "NX sketch payload scalar record uniqueness",
            "NX sketch payload scalar record index",
        )? {
            return Ok(None);
        }
    }
    let mut entities = Vec::new();
    let mut records_iter = operation_groups.iter().copied();
    while let Some(group) = ctx.next_charged(&mut records_iter, "NX sketch operation groups")? {
        let point_use = ctx
            .get_btree_map(
                &point_uses_by_group,
                group.id.as_str(),
                "NX attach sketch graph point uses by group lookup",
            )?
            .copied();
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
        let native_ref = ctx.copy_retained_text(
            native_ref_source,
            "NX attach sketch graph native ref source copy",
        )?;
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
                sketch_id.try_clone_for_decode(ctx, "NX attach sketch graph sketch id copy")?,
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
        |value| value.1.id(),
        Ord::cmp,
        "NX grouped sketch entity identity order",
    )?;
    ctx.stable_sort_by(
        &mut entities,
        |value| &value.0,
        Ord::cmp,
        "NX grouped sketch entity source order",
    )?;
    if entities.is_empty() {
        return Ok(None);
    }
    let mut records_iter = entities.iter();
    while let Some((source_offset, entity)) =
        ctx.next_charged(&mut records_iter, "NX sketch entity annotations")?
    {
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
                let tag = if native_kind.as_str() == "nx-fixed-point" {
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
        return Ok(ctx
            .admit_iter(&point_use.references, "NX sketch point-use offsets")?
            .map(|reference| reference.source_offset)
            .min());
    }
    let mut minimum = None::<u64>;
    let mut records_iter = group.points.iter();
    while let Some(point_id) = ctx.next_charged(&mut records_iter, "NX sketch group points")? {
        let Some(point) = ctx
            .get_btree_map(points_by_id, point_id.as_str(), "NX sketch point lookup")?
            .copied()
        else {
            return Ok(None);
        };
        if !ctx.equal_bytes(
            point.operation_label.as_bytes(),
            label.id.as_bytes(),
            "NX sketch source point operation equality",
        )? || !ctx.equal_bytes(
            point.name.as_bytes(),
            group.name.as_bytes(),
            "NX sketch source point group name equality",
        )? || point
            .coordinates
            .iter()
            .zip(group.coordinates.iter())
            .any(|(first, second)| first.to_bits() != second.to_bits())
        {
            return Ok(None);
        }
        let [first, second] = point.scalar_fields.as_slice() else {
            return Ok(None);
        };
        for (scalar_id, coordinate) in [first, second].into_iter().zip(group.coordinates) {
            let Some(scalar) = ctx
                .get_btree_map(scalars_by_id, scalar_id.as_str(), "NX sketch scalar lookup")?
                .copied()
            else {
                return Ok(None);
            };
            if !ctx.equal_bytes(
                scalar.operation_label.as_bytes(),
                label.id.as_bytes(),
                "NX sketch source scalar operation equality",
            )? || scalar.scalar.value().get().to_bits() != coordinate.to_bits()
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
    let name = ctx.copy_retained_text(&label.value, "NX emit sketch label value copy")?;
    let native_ref = ctx.copy_retained_text(&label.id, "NX emit sketch label id copy")?;
    ir.model.sketch_entities.extend(
        ctx.admit_iter(entities, "NX sketch output entity moves")?
            .map(|(_, entity)| entity),
    );
    ir.model.sketches.push(Sketch {
        id: sketch_id.try_clone_for_decode(ctx, "NX emit sketch sketch id copy")?,
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
    let mut records_iter = points.iter();
    while let Some(point) = ctx.next_charged(&mut records_iter, "NX sketch fixed-point entities")? {
        if !ctx.equal_bytes(
            point.operation_label.as_bytes(),
            label.id.as_bytes(),
            "NX native fixed point entities equality",
        )? || !ctx.insert_scoped_btree_set(
            reservation,
            &mut point_ids,
            point.id.as_str(),
            "NX fixed point identity uniqueness",
            "NX fixed point identity index",
        )? {
            return Ok(None);
        }
        let point_key = ctx
            .rsplit_once(&point.id, "#", "NX sketch fixed point suffix")?
            .map_or(point.id.as_str(), |(_, key)| key);
        if point_key.is_empty()
            || ctx.any_by(
                point_key.chars(),
                |ch| Ok(ch.is_whitespace()),
                "NX sketch point identity characters",
            )?
            || !ctx.insert_scoped_btree_set(
                reservation,
                &mut entity_keys,
                point_key,
                "NX fixed point entity key uniqueness",
                "NX fixed point entity key index",
            )?
        {
            return Ok(None);
        }
        let Some(entity_id) = sketch_entity_identity(ctx, "fixed-point-", point_key)? else {
            return Ok(None);
        };
        let native_kind = cadmpeg_core::nonblank_literal!("nx-fixed-point");
        let native_ref =
            ctx.copy_retained_text(&point.id, "NX native fixed point entities point id copy")?;
        push_sketch_entity(
            ctx,
            reservation,
            &mut entities,
            point.source_offset,
            SketchEntity::new(
                entity_id,
                sketch_id
                    .try_clone_for_decode(ctx, "NX native fixed point entities sketch id copy")?,
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
    entities.extend(ctx.admit_iter(fixed_entities, "NX sketch fixed entity merge moves")?);
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
    for binding in ctx.admit_iter(bindings, "NX segment body bindings")? {
        let (prefix, prefix_len) = stream_prefix(binding.stream_ordinal, false)?;
        let mut stream_bodies = Vec::new();
        for body in ctx.admit_iter(&ir.model.bodies, "NX segment body prefix scan")? {
            if !body
                .id
                .as_str()
                .as_bytes()
                .starts_with(&prefix[..prefix_len])
            {
                continue;
            }
            if ctx.any_by(
                &stream_bodies,
                |existing: &BodyId| {
                    ctx.equal_bytes(
                        existing.as_str().as_bytes(),
                        body.id.as_str().as_bytes(),
                        "NX segment body identity equality",
                    )
                },
                "NX segment body uniqueness",
            )? {
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
            for body in ctx.admit_iter(&stream_bodies, "NX segment body aliases")? {
                if let Some(existing) =
                    ctx.get_btree_map(&by_object, &identity, "NX segment body alias lookup")?
                {
                    if ctx.any_by(
                        existing,
                        |candidate| {
                            ctx.equal_bytes(
                                candidate.as_str().as_bytes(),
                                body.as_str().as_bytes(),
                                "NX segment body alias identity equality",
                            )
                        },
                        "NX segment body alias uniqueness",
                    )? {
                        continue;
                    }
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
                    "NX segment binding body indexes by object group index",
                )?;
            }
        }
        reservation.with_storage(|| {
            ctx.insert_btree_map(
                &mut by_binding,
                binding.id.as_str(),
                stream_bodies,
                "NX segment binding identity index",
            )
        })?;
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
    const PREFIX: &str = "operation_common_frame.";
    let mut matching_record = None;
    let mut records_iter = records.iter();
    while let Some(record) =
        ctx.next_charged(&mut records_iter, "NX operation source record lookup")?
    {
        if ctx.equal_bytes(
            record.operation_label.as_bytes(),
            operation_label.as_bytes(),
            "NX source record operation equality",
        )? {
            if matching_record.is_some() {
                return Ok(());
            }
            matching_record = Some(record);
        }
    }
    let Some(record) = matching_record else {
        return Ok(());
    };
    insert_operation_source_property(ctx, properties, "operation_record", &record.id)?;
    let mut contiguous_common_frames = true;
    let mut ordinal = 0_u64;
    let mut records_iter = common_frames.iter();
    while let Some(frame) =
        ctx.next_charged(&mut records_iter, "NX operation common frame order")?
    {
        if !ctx.equal_bytes(
            frame.operation_record.as_bytes(),
            record.id.as_bytes(),
            "NX common frame record owner equality",
        )? {
            continue;
        }
        if u64::from(frame.ordinal) != ordinal {
            contiguous_common_frames = false;
            break;
        }
        ordinal = ordinal.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit("NX operation common frame ordinal", ordinal, 1)
        })?;
    }
    if contiguous_common_frames {
        for frame in ctx.admit_iter(common_frames, "NX operation common frame properties")? {
            if !(ctx.equal_bytes(
                frame.operation_record.as_bytes(),
                record.id.as_bytes(),
                "NX common frame record owner equality",
            )?) {
                continue;
            }

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
            ctx.append_retained(
                &mut value,
                &frame.id,
                "NX operation source properties frame id append",
            )?;
            ctx.insert_btree_map(properties, key, value, "NX operation source properties")?;
        }
    }
    let mut matching_frame = None;
    let mut records_iter = terminal_frames.iter();
    while let Some(frame) =
        ctx.next_charged(&mut records_iter, "NX operation terminal frame lookup")?
    {
        if ctx.equal_bytes(
            frame.operation_record.as_bytes(),
            record.id.as_bytes(),
            "NX terminal frame record owner equality",
        )? {
            if matching_frame.is_some() {
                return Ok(());
            }
            matching_frame = Some(frame);
        }
    }
    let Some(frame) = matching_frame else {
        return Ok(());
    };
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
    ctx.append_retained(
        &mut owned_key,
        key,
        "NX insert operation source property key append",
    )?;
    let mut owned_value = String::new();
    ctx.try_reserve_retained_text(
        &mut owned_value,
        value.len(),
        "NX operation source property",
    )?;
    ctx.append_retained(
        &mut owned_value,
        value,
        "NX insert operation source property value append",
    )?;
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
    let owned_key = ctx.format_retained(key, "NX source property key formatting")?;
    let owned_value = ctx.format_retained(value, "NX source property value formatting")?;
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
    if ctx.any_by(
        &*dependencies,
        |existing| {
            ctx.equal_bytes(
                existing.as_str().as_bytes(),
                candidate.as_str().as_bytes(),
                "NX feature dependency uniqueness identity",
            )
        },
        "NX feature dependency uniqueness",
    )? {
        return Ok(());
    }
    ctx.charge_collection_items(1, "NX feature dependencies")?;
    ctx.reserve_capacity(dependencies, 1, "NX feature dependency")?;
    dependencies.push(candidate.try_clone_for_decode(ctx, "NX feature dependency")?);
    Ok(())
}

fn preceding_operation_dependency<'a>(
    ctx: &DecodeContext<'_>,
    operation: &str,
    consumer_position: usize,
    operation_positions: &BTreeMap<&str, usize>,
    feature_ids: &'a BTreeMap<&str, FeatureId>,
) -> Result<Option<&'a FeatureId>, CodecError> {
    let Some(position) = ctx.get_btree_map(
        operation_positions,
        operation,
        "NX preceding operation position lookup",
    )?
    else {
        return Ok(None);
    };
    if *position >= consumer_position {
        return Ok(None);
    }
    ctx.get_btree_map(
        feature_ids,
        operation,
        "NX preceding operation feature lookup",
    )
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
        ctx.append_retained(
            &mut owned,
            source,
            "NX text semantic annotation source append",
        )?;
        Ok(owned)
    };
    let mut text_values = Vec::new();
    ctx.reserve_capacity(&mut text_values, 1, "allocate NX TEXT annotation text list")?;
    text_values.push(copy(text)?);
    let key = cadmpeg_core::nonblank_const!(FONT_KEY);
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
    for parameter_id in ctx.admit_iter(parameter_references, "NX parameter owner references")? {
        let Some(owner) = ctx
            .get_btree_map(
                parameter_owners,
                parameter_id,
                "NX parameter owner dependencies parameter owners lookup",
            )?
            .and_then(Option::as_ref)
        else {
            continue;
        };
        if !ctx.any_by(
            &dependencies,
            |existing: &FeatureId| {
                ctx.equal_bytes(
                    existing.as_str().as_bytes(),
                    owner.as_str().as_bytes(),
                    "NX parameter owner dependency identity",
                )
            },
            "NX parameter owner dependency scan",
        )? {
            ctx.charge_collection_items(1, "NX parameter owner dependencies")?;
            ctx.reserve_capacity(&mut dependencies, 1, "NX parameter owner dependency")?;
            dependencies.push(owner.try_clone_for_decode(ctx, "NX parameter owner dependency")?);
        }
    }
    Ok(dependencies)
}

fn extrude_feature_definition(
    ctx: &DecodeContext<'_>,
    construction_profile: Option<&str>,
    structured_construction: Option<&str>,
    op: BooleanOp,
    output_kinds: &[cadmpeg_ir::topology::BodyKind],
) -> Result<FeatureDefinition, CodecError> {
    let profile = match (construction_profile, structured_construction) {
        (Some(construction), None) | (None, Some(construction)) => {
            ProfileRef::Planar(PlanarProfileRef::Native(ctx.format_retained(
                format_args!("{construction}"),
                "NX extrude construction profile",
            )?))
        }
        _ => ProfileRef::Planar(PlanarProfileRef::Unresolved(
            ctx.copy_retained_text("EXTRUDE", "NX unresolved extrude profile")?,
        )),
    };
    let solid = match output_kinds {
        [cadmpeg_ir::topology::BodyKind::Solid, rest @ ..]
            if ctx.all_by(
                rest,
                |kind| Ok(*kind == cadmpeg_ir::topology::BodyKind::Solid),
                "NX extrude output kinds",
            )? =>
        {
            Some(true)
        }
        [cadmpeg_ir::topology::BodyKind::Sheet, rest @ ..]
            if ctx.all_by(
                rest,
                |kind| Ok(*kind == cadmpeg_ir::topology::BodyKind::Sheet),
                "NX extrude output kinds",
            )? =>
        {
            Some(false)
        }
        _ => None,
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Extrude {
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
    }))
}

fn extrude_boolean_op(
    ctx: &DecodeContext<'_>,
    history: &BodyWriterHistory,
    native_primary_body: Option<u32>,
    offset_store_primary_body: Option<&str>,
    output_kinds: &[cadmpeg_ir::topology::BodyKind],
) -> Result<BooleanOp, CodecError> {
    let has_previous_writer =
        if native_primary_body.is_some() || offset_store_primary_body.is_some() {
            history.has_preceding_writer(
                ctx,
                None,
                native_primary_body,
                offset_store_primary_body,
                &[],
            )?
        } else {
            true
        };
    if !has_previous_writer
        && matches!(
            output_kinds,
            [cadmpeg_ir::topology::BodyKind::Solid | cadmpeg_ir::topology::BodyKind::Sheet]
        )
    {
        Ok(BooleanOp::NewBody)
    } else {
        Ok(BooleanOp::Unresolved)
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
        "NX boolean feature definition text",
    )?;
    let native_tools =
        selection_indices_native(ctx, None, &operation.tools, |token| token.token.value())?;
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
            for tool in ctx.admit_iter(&operation.tools, "NX Boolean tools")? {
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
                "NX delete body feature definition text",
            )?,
        )? {
            FeatureBodySelection::Native(native) => {
                let mut reservation = ctx.reserve_scoped(0, "NX DELETE local body")?;
                let mut bodies = Vec::new();
                ctx.reserve_scoped_vec(&mut reservation, &mut bodies, 1, "NX DELETE local body")?;
                bodies.push(ctx.format_scoped_text(
                    &mut reservation,
                    format_args!("nx:om-body-object#{body}"),
                    "NX delete body feature definition text",
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
                "NX delete body feature definition text",
            )?);
            local_body_selection(
                ctx,
                &bodies,
                ctx.format_retained(
                    format_args!("nx:om-object-index#{object_index}"),
                    "NX delete body feature definition text",
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
                "NX extract body feature definition text",
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
            "NX extract body feature definition text",
        )?);
        local_body_selection(
            ctx,
            &bodies,
            ctx.format_retained(
                format_args!("nx:om-object-index#{object_index}"),
                "NX extract body feature definition text",
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
    let primary_store = ctx
        .rsplit_once(data_block, ":block#", "NX trim primary store search")?
        .map(|(store, _)| store);
    let mut tool_data_blocks = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX trim offset tool blocks")?;
    let mut complete = true;
    let mut records_iter = operands.iter();
    while let Some(operand) = ctx.next_charged(&mut records_iter, "NX trim offset operands")? {
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
        let mut distinct_operand_indices = true;
        let mut records_iter = operands.iter().enumerate();
        while let Some((index, operand)) =
            ctx.next_charged(&mut records_iter, "NX trim operand uniqueness")?
        {
            if ctx.any_by(
                &operands[..index],
                |other| Ok(other.operand.atom.value() == operand.operand.atom.value()),
                "NX trim preceding operand indices",
            )? {
                distinct_operand_indices = false;
                break;
            }
        }
        let same_store = ctx.all_by(
            &tool_data_blocks,
            |block| match ctx.rsplit_once(block, ":block#", "NX operand block store search")? {
                Some((store, _)) => ctx.equal_bytes(
                    store.as_bytes(),
                    primary_store.as_bytes(),
                    "NX offset store trim body feature definition equality",
                ),
                None => Ok(false),
            },
            "NX trim tool block validation",
        )?;
        let distinct_tool_blocks = ctx.all_by(
            tool_data_blocks.iter().enumerate(),
            |(index, block)| {
                Ok(!ctx.any_by(
                    &tool_data_blocks[..index],
                    |other| {
                        ctx.equal_bytes(
                            other.as_bytes(),
                            block.as_bytes(),
                            "NX trim tool block uniqueness identity",
                        )
                    },
                    "NX trim tool block uniqueness preceding blocks",
                )?)
            },
            "NX trim tool block uniqueness",
        )?;
        let no_target_alias = ctx.all_by(
            &tool_data_blocks,
            |block| {
                Ok(!ctx.equal_bytes(
                    block.as_bytes(),
                    data_block.as_bytes(),
                    "NX offset store trim body feature definition equality",
                )?)
            },
            "NX trim tool block validation",
        )?;
        if ctx.all_by(
            operands,
            |operand| {
                Ok({
                    operand.body_object_index == *object_index
                        && operand.operand.atom.value() != *object_index
                })
            },
            "NX trim operand body validation",
        )? && distinct_operand_indices
            && same_store
            && distinct_tool_blocks
            && no_target_alias
        {
            let mut bodies = Vec::new();
            for block in ctx.admit_iter(&tool_data_blocks, "NX trim local tool bodies")? {
                ctx.reserve_scoped_vec(
                    &mut reservation,
                    &mut bodies,
                    1,
                    "NX trim local tool bodies",
                )?;
                bodies.push(ctx.format_scoped_text(
                    &mut reservation,
                    format_args!("{block}"),
                    "NX offset store trim body feature definition text",
                )?);
            }
            let native = selection_indices_native(ctx, None, operands, |operand| {
                operand.operand.atom.value()
            })?;
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
        "NX offset store trim body feature definition text",
    )?);
    let target = local_body_selection(
        ctx,
        &target,
        ctx.format_retained(
            format_args!("nx:om-object-index#{object_index}"),
            "NX offset store trim body feature definition text",
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
    let native =
        selection_indices_native(ctx, Some(&primary_body_object_index), operands, |operand| {
            operand.operand.atom.value()
        })?;
    let mut object_indices = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX sew body indices")?;
    for index in std::iter::once(primary_body_object_index).chain(
        ctx.admit_iter(operands, "NX sew operand body indices")?
            .map(|operand| operand.operand.atom.value()),
    ) {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut object_indices,
            1,
            "NX sew body indices",
        )?;
        object_indices.push(index);
    }
    let bodies = if primary_segment_body_object_index.is_some() {
        if ctx.all_by(
            operands,
            |operand| Ok(!operand.segment_body_bindings.is_empty()),
            "NX sew segment operands",
        )? {
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
        let primary_store = ctx
            .rsplit_once(primary_data_block, ":block#", "NX sew primary store search")?
            .map(|(store, _)| store);
        let mut blocks = Vec::new();
        let mut complete = true;
        let mut records_iter = operands.iter();
        while let Some(operand) =
            ctx.next_charged(&mut records_iter, "NX sew offset block inputs")?
        {
            let Some(block) = operand.operand_data_block.as_deref() else {
                complete = false;
                break;
            };
            ctx.reserve_scoped_vec(&mut reservation, &mut blocks, 1, "NX sew offset blocks")?;
            blocks.push(block);
        }
        let valid = complete
            && ctx.all_by(
                operands,
                |operand| Ok(operand.body_object_index == primary_object_index),
                "NX sew offset operands",
            )?
            && ctx.all_by(
                &blocks,
                |block| match ctx.rsplit_once(block, ":block#", "NX operand block store search")? {
                    Some((store, _)) => match primary_store {
                        Some(primary) => ctx.equal_bytes(
                            store.as_bytes(),
                            primary.as_bytes(),
                            "NX sew body feature definition equality",
                        ),
                        None => Ok(false),
                    },
                    None => Ok(false),
                },
                "NX sew block stores",
            )?
            && ctx.all_by(
                blocks.iter().enumerate(),
                |(index, block)| {
                    Ok(!ctx.any_by(
                        &blocks[..index],
                        |other| {
                            ctx.equal_bytes(
                                other.as_bytes(),
                                block.as_bytes(),
                                "NX sew offset block uniqueness identity",
                            )
                        },
                        "NX sew offset block uniqueness preceding blocks",
                    )?)
                },
                "NX sew offset block uniqueness",
            )?
            && !ctx.any_by(
                &blocks,
                |block| {
                    ctx.equal_bytes(
                        block.as_bytes(),
                        primary_data_block.as_bytes(),
                        "NX sew primary block equality",
                    )
                },
                "NX sew primary block membership",
            )?;
        if valid {
            let mut bodies = Vec::new();
            for block in std::iter::once(primary_data_block)
                .chain(ctx.admit_iter(&blocks, "NX sew tool blocks")?.copied())
            {
                ctx.reserve_scoped_vec(&mut reservation, &mut bodies, 1, "NX sew local bodies")?;
                bodies.push(ctx.format_scoped_text(
                    &mut reservation,
                    format_args!("{block}"),
                    "NX sew body feature definition text",
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
        "NX trim body feature definition text",
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
    let native_tools =
        selection_indices_native(ctx, None, operands, |operand| operand.operand.atom.value())?;
    let mut tool_object_indices = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX trim tool indices")?;
    for operand in ctx.admit_iter(operands, "NX trim tool operands")? {
        ctx.reserve_scoped_vec(
            &mut reservation,
            &mut tool_object_indices,
            1,
            "NX trim tool indices",
        )?;
        tool_object_indices.push(operand.operand.atom.value());
    }
    if ctx.any_by(
        operands,
        |operand| {
            Ok(
                {
                    operand.operand_data_block.is_some() || operand.segment_body_bindings.is_empty()
                },
            )
        },
        "NX trim operand completeness",
    )? {
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
    if crate::native::segments::unique_segment_body_binding(ctx, object_index, segment_bindings)?
        .is_none()
    {
        return Ok(Vec::new());
    }
    let Some([body]) = ctx
        .get_btree_map(
            bodies_by_object_index,
            &object_index,
            "NX feature output body lookup",
        )?
        .map(Vec::as_slice)
    else {
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
    for use_ in ctx.admit_iter(uses, "NX body image unique-use index")? {
        let write_key = use_.operation_body_write.as_str();
        match reservation.with_storage(|| {
            ctx.entry_btree_map(
                &mut unique_uses,
                write_key,
                "NX operation body image outputs by write unique uses entry",
            )
        })? {
            Entry::Vacant(entry) => {
                entry.insert(Some(use_));
            }
            Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
    }
    let mut outputs = BTreeMap::new();
    for (&write, use_) in ctx.admit_iter(&unique_uses, "NX unique body image uses")? {
        let Some(use_) = use_ else {
            continue;
        };
        let Some([body]) = ctx
            .get_btree_map(
                bodies_by_segment_binding,
                use_.segment_body_binding.as_str(),
                "NX operation body image outputs by write bodies by segment binding lookup",
            )?
            .map(Vec::as_slice)
        else {
            continue;
        };

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
    for (&write, body) in ctx.admit_iter(candidates, "NX body output candidate merge")? {
        if ctx.contains_btree_set(conflicts, write, "NX conflicting body output membership")? {
            continue;
        }

        let conflict = match reservation.with_storage(|| {
            ctx.entry_btree_map(
                &mut *outputs,
                write,
                "NX merge operation body outputs outputs entry",
            )
        })? {
            Entry::Vacant(entry) => {
                entry
                    .insert(reservation.with_storage(|| {
                        body.try_clone_for_decode(ctx, "NX merged body output")
                    })?);
                false
            }
            Entry::Occupied(entry)
                if ctx.equal_bytes(
                    entry.get().as_str().as_bytes(),
                    body.as_str().as_bytes(),
                    "NX merge operation body outputs equality",
                )? =>
            {
                false
            }
            Entry::Occupied(_) => true,
        };
        if conflict {
            ctx.remove_btree_map(outputs, write, "NX conflicting body output removal")?;
            reservation.with_storage(|| {
                ctx.insert_btree_set(conflicts, write, "NX conflicting body output")
            })?;
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
    for use_ in ctx.admit_iter(uses, "NX body identity output lookup")? {
        let Some([body]) = ctx
            .get_btree_map(
                bodies_by_segment_binding,
                use_.segment_body_binding.as_str(),
                "NX operation body identity outputs by write bodies by segment binding lookup",
            )?
            .map(Vec::as_slice)
        else {
            continue;
        };
        let write_key = use_.operation_body_write.as_str();
        let conflict = match reservation.with_storage(|| {
            ctx.entry_btree_map(
                &mut outputs,
                write_key,
                "NX operation body identity outputs by write outputs entry",
            )
        })? {
            Entry::Vacant(entry) => {
                entry.insert(reservation.with_storage(|| {
                    body.try_clone_for_decode(ctx, "NX body identity output index")
                })?);
                false
            }
            Entry::Occupied(entry)
                if ctx.equal_bytes(
                    entry.get().as_str().as_bytes(),
                    body.as_str().as_bytes(),
                    "NX operation body identity outputs by write equality",
                )? =>
            {
                false
            }
            Entry::Occupied(_) => true,
        };
        if conflict {
            ctx.remove_btree_map(
                &mut outputs,
                write_key,
                "NX conflicting body identity removal",
            )?;
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
    for use_ in ctx.admit_iter(uses, "NX body partition identity index")? {
        match reservation.with_storage(|| {
            ctx.entry_btree_map(
                &mut partitions_by_identity,
                use_.body_identity,
                "NX operation body group partition outputs by write partitions by identity entry",
            )
        })? {
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
    for (&identity, &partition) in
        ctx.admit_iter(&partitions_by_identity, "NX body partition identities")?
    {
        let Some(partition) = partition else {
            continue;
        };
        let (prefix, prefix_len) = stream_prefix(partition, true)?;
        let mut candidates = bodies.iter();
        let Some(body) = ctx.find_by(
            &mut candidates,
            |body| {
                Ok(body
                    .id
                    .as_str()
                    .as_bytes()
                    .starts_with(&prefix[..prefix_len]))
            },
            "NX body partition first body search",
        )?
        else {
            continue;
        };
        if ctx.any_by(
            candidates,
            |body| {
                Ok(body
                    .id
                    .as_str()
                    .as_bytes()
                    .starts_with(&prefix[..prefix_len]))
            },
            "NX body partition second body search",
        )? {
            continue;
        }

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
    for write in ctx.admit_iter(writes, "NX partition body write lookup")? {
        if let Some(body) = ctx.get_btree_map(
            &unique_bodies,
            &write.frame.body_identity(),
            "NX partition body identity lookup",
        )? {
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
    let mut outputs: Vec<&BodyId> = Vec::new();
    let mut storage = ctx.reserve_scoped(0, "NX complete body image output candidates")?;
    let mut records_iter = writes.iter();
    while let Some(write) =
        ctx.next_charged(&mut records_iter, "NX complete body image output lookup")?
    {
        let Some(body) = ctx.get_btree_map(
            outputs_by_write,
            write.id.as_str(),
            "NX complete operation body image outputs outputs by write lookup",
        )?
        else {
            return Ok(Vec::new());
        };
        if ctx.any_by(
            &*outputs,
            |existing| {
                ctx.equal_bytes(
                    existing.as_str().as_bytes(),
                    body.as_str().as_bytes(),
                    "NX complete body image output uniqueness identity",
                )
            },
            "NX complete body image output uniqueness",
        )? {
            return Ok(Vec::new());
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut outputs,
            body,
            "NX complete body image output candidates",
        )?;
    }
    ctx.try_collect_retained_with(outputs, "NX complete body image outputs", |body| {
        body.try_clone_for_decode(ctx, "NX complete body image output")
    })
}

fn body_writes_match_boolean_target(
    ctx: &DecodeContext<'_>,
    writes: &[&crate::native::features::FeatureOperationBodyWrite],
    boolean: Option<&crate::native::features::FeatureBooleanOperation>,
) -> Result<bool, CodecError> {
    let Some(boolean) = boolean else {
        return Ok(true);
    };
    if writes.is_empty() {
        return Ok(true);
    }
    let [write] = writes else {
        return Ok(false);
    };
    Ok(
        write.frame.body_image().value() == boolean.target.token.value()
            && !ctx.any_by(
                &boolean.tools,
                |token| Ok(token.token.value() == write.frame.body_image().value()),
                "NX Boolean body-write target tools",
            )?,
    )
}

#[cfg(test)]
mod tests;
