// SPDX-License-Identifier: Apache-2.0
//! IR-writing attachment of the native object model.

use crate::loss::NxLossCode;
use cadmpeg_core::decode::id_from_index;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::assets::{Asset, AssetContent, AssetId};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    AppearanceBindingId, AppearanceId, AttributeId, BodyId,
    FeatureResultTopologyId, LoopId, SurfaceId, UnknownId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::semantic_annotations::{SemanticAnnotation, SemanticAnnotationKind};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement,
};
use cadmpeg_ir::topology::{BodyKind, Color, Face, FaceLoops, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferSpec, RadiusSpec},
        holes::{HoleForm, HoleKind, HolePlacement},
        patterns::PatternKind,
        BodyRetentionMode, BodySelection, BodyTrimSide, BooleanOp, ConfigurationFeatureState,
        ConfigurationId, CurveProjectionDirection, CurveProjectionDirectionState,
        DesignConfiguration, DesignParameter, DistinctMembers, EdgeSelection, ExtrudeExtent,
        ExtrudeSide, FaceSelection, Feature, FeatureContent, FeatureDefinition, FeatureId,
        FeatureOperation, FeatureResultTopology, FeatureSourceContent, FeatureTreeNodeRole,
        LinearTermination, ParameterId, ParameterValue, PathRef, PlanarProfileRef, ProfileRef,
        RibConstruction, RibDraft, SurfaceExtension, ThickenSide, TreeChildren, TrimRegion,
        UnresolvedFamily,
    },
    scalar::{Angle, FiniteReal, Length, NonZeroLength, PositiveLength},
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

const MIN_LINEAR_TOLERANCE: f64 = 1.0e-9;
const MIN_ANGULAR_TOLERANCE: f64 = 1.0e-12;

fn push_native_unknown(
    ctx: &DecodeContext<'_>,
    unknowns: &mut Vec<UnknownRecord>,
    record: UnknownRecord,
) -> Result<(), CodecError> {
    ctx.charge_entities(1, "NX native unknown record")?;
    ctx.charge_collection_items(1, "NX native unknown records")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<UnknownRecord>()),
        "retain NX native unknown record",
    )?;
    unknowns.try_reserve(1).map_err(|_| {
        ctx.refuse_codec_limit("allocate NX native unknown records", 0, 1)
    })?;
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
    let annotation_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
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
        annotations
            .note(&id, &annotation_stream, offset)
            .tag(content.label());
        annotations.exactness(&id, Exactness::ByteExact);
        push_native_unknown(ctx, unknowns, UnknownRecord::retained(
            id,
            offset,
            ctx.copy_retained(bytes, "retain NX opaque container payload")?,
            Vec::new(),
        ))?;
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
    let annotation_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
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
                    let offset = entry_offset + record.offset as u64;
                    annotations
                        .note(&id, &annotation_stream, offset)
                        .tag("OM_ENTITY_RECORD");
                    annotations.exactness(&id, Exactness::ByteExact);
                    push_native_unknown(ctx, unknowns, UnknownRecord::retained(
                        id,
                        offset,
                        ctx.copy_retained(record.bytes, "retain NX indexed object-model record")?,
                        Vec::new(),
                    ))?;
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
                    let offset = entry_offset + record.offset as u64;
                    annotations
                        .note(&id, &annotation_stream, offset)
                        .tag("OM_DATA_BLOCK");
                    annotations.exactness(&id, Exactness::ByteExact);
                    push_native_unknown(ctx, unknowns, UnknownRecord::retained(
                        id,
                        offset,
                        ctx.copy_retained(record.bytes, "retain NX indexed object-model record")?,
                        Vec::new(),
                    ))?;
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
    let annotation_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    if model.is_empty() && !has_object_sections {
        return Ok(());
    }
    attach_rm_face_colors(ctx, ir, model, scan, annotations)?;
    attach_rm_appearances(ctx, ir, model, scan, annotations)?;
    let display_jt_tessellations = display_jt_tessellations(
        ctx,
        &DisplayJtTessellationInputs {
            meshes: &model.display_jt.display_jt_polygon_meshes,
            coordinates: &model.display_jt.display_jt_vertex_coordinates,
            normals: &model.display_jt.display_jt_vertex_normals,
            colors: &model.display_jt.display_jt_vertex_colors,
            texture_coordinates: &model.display_jt.display_jt_vertex_texture_coordinates,
            vertex_flags: &model.display_jt.display_jt_vertex_flags,
            vertex_headers: &model.display_jt.display_jt_vertex_records_headers,
            coordinate_headers: &model.display_jt.display_jt_coordinate_array_headers,
            shape_elements: model.display_jt.graph.shape_lod_elements(),
            bindings: &model.display_jt.display_jt_shape_lod_bindings,
            shape_nodes: &model.display_jt.display_jt_tri_strip_shape_nodes,
            base_nodes: &model.display_jt.display_jt_base_node_data,
            group_nodes: &model.display_jt.display_jt_group_node_data,
            instance_nodes: &model.display_jt.display_jt_instance_nodes,
            transforms: &model.display_jt.display_jt_geometric_transform_attributes,
            materials: &model.display_jt.display_jt_material_attributes,
            compressed_elements: model.display_jt.graph.compressed_elements(),
        },
    )?;
    for (tessellation, source_offset) in display_jt_tessellations {
        ctx.charge_collection_items(1, "NX attached display tessellations")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of_val(&tessellation)),
            "NX attached display tessellations",
        )?;
        reserve_attach_vec(ctx, &mut ir.model.tessellations, 1, "NX attached display tessellations")?;
        annotations
            .note(tessellation.id.as_str(), &annotation_stream, source_offset)
            .tag("DISPLAY_JT_TESSELLATION");
        annotations.exactness(tessellation.id.as_str(), Exactness::Derived);
        ir.model.tessellations.push(tessellation);
    }
    NATIVE_CATALOGUE.note_phase(NotePhase::GroupA, model, annotations);
    attach_material_texture_assets(ctx, ir, &model.om.material_texture_assets, scan, annotations)?;
    attach_part_attributes(
        ctx,
        ir,
        model.om.part_attributes.iter().map(|attribute| (
            attribute.id.as_str(),
            attribute.title.as_str(),
            attribute.value.as_str(),
            attribute.source_offset,
        )),
        annotations,
        &annotation_stream,
    )?;
    let topology_attribute_index = ParasolidTopologyAttributeIndex::new(
        ir,
        &model.parasolid.parasolid_topology_attribute_list_references,
        &model.parasolid.parasolid_topology_attribute_class_uses,
        &model.parasolid.parasolid_attribute_definitions,
        &model.parasolid.parasolid_attribute_field_uses,
        &model.parasolid.parasolid_attribute_field_names,
    );
    attach_parasolid_topology_string_attributes(
        ir,
        &ParasolidStringAttributeSources {
            string_uses: &model.parasolid.parasolid_entity_51_string_uses,
            strings: &model.parasolid.parasolid_entity_54_string_records,
        },
        &topology_attribute_index,
        annotations,
    )?;
    attach_parasolid_topology_numeric_attributes(
        ir,
        &ParasolidNumericAttributeSources {
            numeric_uses: &model.parasolid.parasolid_entity_51_numeric_uses,
            integers: &model.parasolid.parasolid_entity_52_integer_records,
            doubles: &model.parasolid.parasolid_entity_53_double_records,
        },
        &topology_attribute_index,
        annotations,
    )?;
    attach_parasolid_topology_structured_attributes(
        ir,
        &ParasolidStructuredAttributeSources {
            structured_uses: &model.parasolid.parasolid_entity_51_structured_uses,
            vectors: &model.parasolid.parasolid_entity_vector_records,
            axes: &model.parasolid.parasolid_entity_57_axis_records,
            tags: &model.parasolid.parasolid_entity_58_tag_records,
            unicode: &model.parasolid.parasolid_entity_62_unicode_records,
        },
        &topology_attribute_index,
        annotations,
    )?;
    NATIVE_CATALOGUE.note_phase(NotePhase::GroupB, model, annotations);
    attach_indexed_om_unknowns(ctx, scan, annotations, unknowns)?;
    let configuration_lookup_work = model.om.configurations.len()
        .checked_mul(model.om.configuration_attribute_uses.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX configuration relation lookup", 0, cadmpeg_core::decode::u64_from_index(model.om.configurations.len())))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(configuration_lookup_work), "NX configuration relation lookup")?;
    attach_configurations(
        ctx,
        ir,
        model.om.configurations.iter().map(|configuration| (
            configuration.id.as_str(),
            configuration.name.as_str(),
            configuration.source_offset,
            model.om.configuration_attribute_uses.iter()
                .find(|relation| relation.configuration == configuration.id)
                .map(|relation| relation.id.as_str()),
        )),
        annotations,
        &annotation_stream,
    )?;
    attach_expression_parameters(
        ctx,
        ir,
        &model.om.expressions,
        &model.om.expression_declarations,
        &model.features.feature_parameter_uses,
        annotations,
    )?;
    attach_active_configuration_parameter_values(ctx, ir, annotations)?;
    attach_feature_operations(ctx, ir, model, annotations, losses)?;
    attach_block_dimension_parameter_consumers(
        ir,
        &model.features.feature_block_dimensions,
        annotations,
    )?;
    attach_current_feature_states(ir, annotations)?;
    attach_active_configuration_feature_states(ir, annotations)?;
    ir.model
        .features
        .sort_by(|first, second| first.id.cmp(&second.id));
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
        let id_bytes = attribute_id.len().checked_mul(2)
            .and_then(|bytes| bytes.checked_add(":neutral".len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX part attribute identity", 0, cadmpeg_core::decode::u64_from_index(attribute_id.len())))?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(id_bytes), "NX part attribute identity")?;
        annotations
            .note(attribute_id, &annotation_stream, source_offset)
            .tag("Attribute");
        annotations.exactness(attribute_id, Exactness::ByteExact);
        let id: AttributeId =
            extended_id(attribute_id, &cadmpeg_ir::identity_key!("neutral")).ok_or_else(
                || CodecError::malformed(format_args!("NX part attribute id is not an identity")),
            )?;
        annotations
            .note(id.as_str(), &annotation_stream, source_offset)
            .tag("Attribute");
        annotations
            .derived(id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(id.as_str(), "name")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(id.as_str(), "values")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        ctx.charge_collection_items(1, "NX attached part attributes")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<SourceAttribute>()),
            "NX attached part attributes",
        )?;
        ctx.charge_collection_items(1, "NX part attribute values")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(
                std::mem::size_of::<AttributeValue>()
                    .checked_add(attribute_title.len())
                    .and_then(|bytes| bytes.checked_add(attribute_value.len()))
                    .ok_or_else(|| ctx.refuse_codec_limit("NX part attribute value", 0, cadmpeg_core::decode::u64_from_index(attribute_value.len())))?,
            ),
            "NX part attribute values",
        )?;
        let mut values = Vec::new();
        reserve_attach_vec(ctx, &mut values, 1, "NX part attribute values")?;
        values.push(AttributeValue::String(attribute_value.to_string()));
        reserve_attach_vec(ctx, &mut ir.model.attributes, 1, "NX attached part attributes")?;
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
    for (ordinal, (configuration_id, configuration_name, source_offset, active_attribute_use)) in configurations.into_iter().enumerate() {

            let id: ConfigurationId =
                IdScope::native(cadmpeg_ir::identity_component!("arrangements"))
                    .id(&cadmpeg_ir::identity_component!("configuration"), ordinal);
            let ordinal_u32 = u32::try_from(ordinal)
                .map_err(|_| ctx.refuse_codec_limit("NX configuration ordinal", 0, cadmpeg_core::decode::u64_from_index(ordinal)))?;
            let bodies = if active_attribute_use.is_some() {
                let mut selected = Vec::new();
                ctx.charge_collection_items(
                    cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()),
                    "NX active configuration bodies",
                )?;
                let slots = ir.model.bodies.len().checked_mul(std::mem::size_of::<BodyId>())
                    .ok_or_else(|| ctx.refuse_codec_limit("NX active configuration bodies", 0, cadmpeg_core::decode::u64_from_index(ir.model.bodies.len())))?;
                ctx.charge_retained(cadmpeg_core::decode::u64_from_index(slots), "NX active configuration bodies")?;
                reserve_attach_vec(ctx, &mut selected, ir.model.bodies.len(), "NX active configuration bodies")?;
                for body in &ir.model.bodies {
                    ctx.charge_retained(
                        cadmpeg_core::decode::u64_from_index(body.id.as_str().len()),
                        "NX active configuration bodies",
                    )?;
                    selected.push(body.id.clone());
                }
                let unique_work = selected.len().checked_mul(selected.len())
                    .ok_or_else(|| ctx.refuse_codec_limit("NX active configuration body uniqueness", 0, cadmpeg_core::decode::u64_from_index(selected.len())))?;
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(unique_work), "NX active configuration body uniqueness")?;
                Some(DistinctMembers::try_from_unique_vec(selected)
                    .map_err(|error| CodecError::Malformed(error.to_owned()))?)
            } else {
                None
            };
            annotations
                .note(id.as_str(), &annotation_stream, source_offset)
                .tag("Arrangement");
            annotations
                .derived(id.as_str(), "ordinal")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            if active_attribute_use.is_some() {
                annotations
                    .derived(id.as_str(), "active")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            annotations
                .derived(id.as_str(), "source_index")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            annotations
                .derived(id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            annotations
                .derived(id.as_str(), "native_ref")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            if bodies
                .as_deref()
                .is_some_and(|bodies: &[_]| !bodies.is_empty())
            {
                annotations
                    .derived(id.as_str(), "bodies")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
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
                ctx.charge_collection_items(1, "NX active configuration property")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(
                        std::mem::size_of::<(String, String)>()
                            .checked_add(relation.len())
                            .ok_or_else(|| ctx.refuse_codec_limit("NX active configuration property", 0, cadmpeg_core::decode::u64_from_index(relation.len())))?,
                    ),
                    "NX active configuration property",
                )?;
                properties.insert(cadmpeg_core::nonblank_literal!("active_attribute_use"), relation.to_string());
            }
            reserve_attach_vec(ctx, &mut ir.model.configurations, 1, "NX attached configurations")?;
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
    let (face_ids, _face_ids_reservation) = collect_rm_face_ids(
        ctx,
        ir.model.faces.iter().map(|face| face.id.as_str()),
    )?;
    let bindings = resolve_rm_face_colors(
        ctx,
        &face_ids,
        &model.om.rm_display_color_assignments,
        &model.om.part_color_definitions,
        &model.parasolid.parasolid_deltas_records,
        &super::substrate::paired_delta_streams(ctx, scan)?,
    )?;
    for (face_id, color) in bindings {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ir.model.faces.len()),
            "NX RM face color target lookup",
        )?;
        let Some(index) = ir.model.faces.iter().rposition(|face| face.id.as_str() == face_id) else {
            continue;
        };
        let face = &mut ir.model.faces[index];
        if face.color.is_none() || face.color == Some(color) {
            face.color = Some(color);
            annotations
                .derived(&face.id, "color")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
    }
    Ok(())
}

fn collect_rm_face_ids<'a, 'b>(
    ctx: &'a DecodeContext<'_>,
    faces: impl IntoIterator<Item = &'b str>,
) -> Result<(BTreeSet<String>, cadmpeg_core::decode::ScopedReservation<'a>), CodecError> {
    let mut ids = BTreeSet::new();
    let mut reservation = ctx.reserve_scoped(0, "NX RM face identity lookup")?;
    for id in faces {
        ctx.charge_work(1, "NX RM face identity lookup")?;
        if ids.contains(id) {
            continue;
        }
        let bytes = std::mem::size_of::<String>()
            .checked_add(id.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX RM face identity lookup", 0, cadmpeg_core::decode::u64_from_index(id.len())))?;
        ctx.charge_collection_items(1, "NX RM face identity lookup")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        ids.insert(id.to_owned());
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
    let source_bindings = resolve_rm_source_color_bindings(ctx, &model.om.rm_display_color_assignments)?;
    let (face_ids, _face_ids_reservation) = collect_rm_face_ids(
        ctx,
        ir.model.faces.iter().map(|face| face.id.as_str()),
    )?;
    let face_bindings = resolve_rm_face_color_bindings(
        ctx,
        &face_ids,
        &model.om.rm_display_color_assignments,
        &model.om.part_color_definitions,
        &model.parasolid.parasolid_deltas_records,
        &super::substrate::paired_delta_streams(ctx, scan)?,
    )?;
    if source_bindings.is_empty() && face_bindings.is_empty() {
        return Ok(());
    }
    let annotation_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    let mut appearances = BTreeMap::<String, AppearanceId>::new();
    let mut appearances_reservation = ctx.reserve_scoped(0, "NX RM appearance identity lookup")?;
    for binding in source_bindings {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(model.om.part_color_definitions.len()), "NX RM appearance definition lookup")?;
        let Some(definition) = model.om.part_color_definitions.iter().rev().find(|definition| definition.id == binding.color_definition) else {
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
            cadmpeg_core::decode::u64_from_index(binding.source_id.len().checked_add(128).ok_or_else(|| ctx.refuse_codec_limit("NX RM source binding identity", 0, cadmpeg_core::decode::u64_from_index(binding.source_id.len())))?),
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
        annotations
            .note(
                binding_id.as_str(),
                &annotation_stream,
                binding.source_offset,
            )
            .tag("RMFASTLOAD_COLOR_ASSIGNMENT");
        annotations
            .derived(binding_id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(binding_id.as_str(), "appearance")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        let binding_bytes = std::mem::size_of::<AppearanceBinding>()
            .checked_add(binding_id.as_str().len())
            .and_then(|bytes| bytes.checked_add(binding.source_id.len()))
            .and_then(|bytes| bytes.checked_add(appearance_id.as_str().len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX RM source appearance binding", 0, cadmpeg_core::decode::u64_from_index(binding.source_id.len())))?;
        ctx.charge_collection_items(1, "NX RM source appearance bindings")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(binding_bytes), "NX RM source appearance binding")?;
        reserve_attach_vec(ctx, &mut ir.model.appearance_bindings, 1, "NX RM source appearance bindings")?;
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
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(model.om.part_color_definitions.len()), "NX RM appearance definition lookup")?;
        let Some(definition) = model.om.part_color_definitions.iter().rev().find(|definition| definition.id == binding.color_definition) else {
            continue;
        };
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.faces.len()), "NX RM appearance face lookup")?;
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
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(face.id.as_str().len()), "NX RM appearance face target")?;
        let face_id = face.id.clone();
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
            cadmpeg_core::decode::u64_from_index(binding.face_id.len().checked_add(128).ok_or_else(|| ctx.refuse_codec_limit("NX RM face binding identity", 0, cadmpeg_core::decode::u64_from_index(binding.face_id.len())))?),
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
        annotations
            .note(
                binding_id.as_str(),
                &annotation_stream,
                binding.source_offset,
            )
            .tag("RMFASTLOAD_FACE_COLOR_ASSIGNMENT");
        annotations
            .derived(binding_id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(binding_id.as_str(), "appearance")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        let binding_bytes = std::mem::size_of::<AppearanceBinding>()
            .checked_add(binding_id.as_str().len())
            .and_then(|bytes| bytes.checked_add(face_id.as_str().len()))
            .and_then(|bytes| bytes.checked_add(appearance_id.as_str().len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX RM face appearance binding", 0, cadmpeg_core::decode::u64_from_index(face_id.as_str().len())))?;
        ctx.charge_collection_items(1, "NX RM face appearance bindings")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(binding_bytes), "NX RM face appearance binding")?;
        reserve_attach_vec(ctx, &mut ir.model.appearance_bindings, 1, "NX RM face appearance bindings")?;
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
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(appearances.len()), "NX RM appearance reuse lookup")?;
    if let Some(id) = appearances.get(&definition.id) {
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(id.as_str().len()), "NX RM reused appearance identity")?;
        return Ok(id.clone());
    }
    let identity_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(definition.id.len().checked_add(128).ok_or_else(|| ctx.refuse_codec_limit("NX RM color appearance identity", 0, cadmpeg_core::decode::u64_from_index(definition.id.len())))?),
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
    annotations
        .note(id.as_str(), annotation_stream, definition.source_offset)
        .tag("RMFASTLOAD_COLOR_APPEARANCE");
    annotations
        .derived(id.as_str(), "name")
        .map_err(cadmpeg_core::CodecError::malformed)?;
    annotations
        .derived(id.as_str(), "schema")
        .map_err(cadmpeg_core::CodecError::malformed)?;
    annotations
        .derived(id.as_str(), "base_color")
        .map_err(cadmpeg_core::CodecError::malformed)?;
    let appearance_bytes = std::mem::size_of::<Appearance>()
        .checked_add(id.as_str().len())
        .and_then(|bytes| bytes.checked_add(definition.name.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("NX RM color appearance", 0, cadmpeg_core::decode::u64_from_index(definition.name.len())))?;
    ctx.charge_collection_items(1, "NX RM color appearances")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(appearance_bytes), "NX RM color appearance")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(id.as_str().len()), "NX RM color appearance binding identity")?;
    reserve_attach_vec(ctx, &mut ir.model.appearances, 1, "NX RM color appearances")?;
    ir.model.appearances.push(Appearance {
        id: id.clone(),
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
    let lookup_bytes = std::mem::size_of::<(String, AppearanceId)>()
        .checked_add(definition.id.len())
        .and_then(|bytes| bytes.checked_add(id.as_str().len()))
        .ok_or_else(|| ctx.refuse_codec_limit("NX RM appearance identity lookup", 0, cadmpeg_core::decode::u64_from_index(definition.id.len())))?;
    ctx.charge_collection_items(1, "NX RM appearance identity lookup")?;
    appearances_reservation.grow(cadmpeg_core::decode::u64_from_index(lookup_bytes))?;
    appearances.insert(definition.id.clone(), id.clone());
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
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(assignments.len()), "NX RM source color assignments")?;
    for assignment in assignments {
        let Some(source_id) = assignment.target_object_id.as_deref() else {
            continue;
        };
        if !choices.contains_key(source_id) {
            ctx.charge_collection_items(1, "NX RM source color choices")?;
            choices_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, RmColorChoice<'_>)>()))?;
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
        } = choice else {
            continue;
        };
        let bytes = std::mem::size_of::<RmSourceColorBinding>()
            .checked_add(source_id.len())
            .and_then(|bytes| bytes.checked_add(definition.len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX RM source color binding", 0, cadmpeg_core::decode::u64_from_index(source_id.len())))?;
        ctx.charge_collection_items(1, "NX RM source color bindings")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX RM source color bindings")?;
        reserve_attach_vec(ctx, &mut bindings, 1, "NX RM source color bindings")?;
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
    let bindings =
        resolve_rm_face_color_bindings(ctx, face_ids, assignments, definitions, records, delta_pairs)?;
    let mut colors = Vec::new();
    for binding in bindings {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(definitions.len()),
            "NX RM face color definition lookup",
        )?;
        let Some(definition) = definitions.iter().rev().find(|definition| definition.id.as_str() == binding.color_definition.as_str()) else {
            continue;
        };
        let color = Color::new(
            definition.components[0].0.value(),
            definition.components[1].0.value(),
            definition.components[2].0.value(),
            1.0,
        ).ok_or_else(|| CodecError::Malformed("RM color components must be in [0, 1]".into()))?;
        ctx.charge_collection_items(1, "NX resolved RM face colors")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(String, Color)>()),
            "NX resolved RM face colors",
        )?;
        reserve_attach_vec(ctx, &mut colors, 1, "NX resolved RM face colors")?;
        colors.push((binding.face_id, color));
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
        } = choice else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(definitions.len()),
            "NX RM face color definitions",
        )?;
        if !definitions.iter().any(|candidate| candidate.id.as_str() == definition) {
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
                let work = deltas.len().checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("NX RM face color delta links", 0, 1)
                })?;
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(work),
                    "NX RM face color delta links",
                )?;
                let Ok(partition_id) = u32::try_from(*raw_partition) else {
                    continue;
                };
                if !deltas.iter().any(|delta| u32::try_from(*delta).ok() == Some(record.stream_ordinal)) {
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
        let bytes = std::mem::size_of::<RmFaceColorBinding>()
            .checked_add(face_id.len())
            .and_then(|bytes| bytes.checked_add(definition.len()))
            .ok_or_else(|| ctx.refuse_codec_limit(
                "NX RM face color binding",
                0,
                cadmpeg_core::decode::u64_from_index(face_id.len()),
            ))?;
        ctx.charge_collection_items(1, "NX RM face color bindings")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(bytes),
            "NX RM face color bindings",
        )?;
        reserve_attach_vec(ctx, &mut bindings, 1, "NX RM face color bindings")?;
        bindings.push(RmFaceColorBinding {
            face_id,
            color_definition: definition.to_owned(),
            source_offset,
        });
    }
    let sort_work = bindings.len().checked_mul(bindings.len()).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX RM face color binding order",
            0,
            cadmpeg_core::decode::u64_from_index(bindings.len()),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(sort_work),
        "NX RM face color binding order",
    )?;
    bindings.sort_by(|left, right| left.face_id.cmp(&right.face_id));
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
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
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
            annotations
                .note(native_ref.as_str(), &stream, source_offset)
                .tag("JPEG_PREVIEW_INVALID");
            annotations.exactness(native_ref.as_str(), Exactness::ByteExact);
            push_native_unknown(ctx, unknowns, UnknownRecord::retained(
                native_ref,
                source_offset,
                ctx.copy_retained(bytes, "retain NX invalid JPEG preview")?,
                Vec::new(),
            ))?;
            continue;
        }
        let id: AssetId = extended_id(native_ref.as_str(), &cadmpeg_ir::identity_key!("asset"))
            .ok_or_else(|| {
                CodecError::malformed(format_args!("NX JPEG preview id is not an identity"))
            })?;
        annotations
            .note(id.as_str(), &stream, source_offset)
            .tag("JPEG_PREVIEW_ASSET");
        annotations.exactness(id.as_str(), Exactness::ByteExact);
        annotations
            .derived(id.as_str(), "id")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(id.as_str(), "name")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(id.as_str(), "media_type")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(id.as_str(), "native_ref")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        ctx.charge_collection_items(1, "NX JPEG preview assets")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Asset>()),
            "NX JPEG preview assets",
        )?;
        reserve_attach_vec(ctx, &mut ir.model.assets, 1, "NX JPEG preview assets")?;
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
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(bytes.len()), "NX material texture hash")?;
        if crate::native::hex::Sha256Hex::digest(bytes) != texture.sha256 {
            return Ok(());
        }
        ctx.charge_collection_items(1, "NX material texture source list")?;
        source_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&crate::native::om::material_texture::MaterialTextureAsset, &[u8])>()))?;
        reserve_attach_vec(ctx, &mut sources, 1, "NX material texture source list")?;
        sources.push((texture, bytes));
    }

    let mut assets = Vec::new();
    for (texture, bytes) in sources {
        let id_bytes = texture.id.len().checked_mul(2)
            .and_then(|bytes| bytes.checked_add(":asset".len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX material asset identity", 0, cadmpeg_core::decode::u64_from_index(texture.id.len())))?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(id_bytes), "NX material asset identity")?;
        let text_bytes = texture.name().len().checked_add(texture.id.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX material asset text", 0, cadmpeg_core::decode::u64_from_index(texture.id.len())))?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(text_bytes), "NX material asset text")?;
        ctx.charge_collection_items(1, "NX material asset records")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Asset>()),
            "NX material asset records",
        )?;
        reserve_attach_vec(ctx, &mut assets, 1, "NX material asset records")?;
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
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    drop(source_reservation);
    for (texture, asset) in textures.iter().zip(&assets) {
        annotations
            .note(asset.id.as_str(), &stream, texture.source_offset)
            .tag("MATERIAL_TEXTURE_ASSET");
        annotations.exactness(asset.id.as_str(), Exactness::ByteExact);
        annotations
            .derived(asset.id.as_str(), "id")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(asset.id.as_str(), "media_type")
            .map_err(cadmpeg_core::CodecError::malformed)?;
        annotations
            .derived(asset.id.as_str(), "native_ref")
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    let slots = assets.len().checked_mul(std::mem::size_of::<Asset>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX attached material assets", 0, cadmpeg_core::decode::u64_from_index(assets.len())))?;
    ctx.charge_collection_items(cadmpeg_core::decode::u64_from_index(assets.len()), "NX attached material assets")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(slots), "NX attached material assets")?;
    reserve_attach_vec(ctx, &mut ir.model.assets, assets.len(), "NX attached material assets")?;
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
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(index), "NX configuration parameter identity uniqueness")?;
        if ir.model.parameters[..index].iter().any(|previous| previous.id == parameter.id) {
            return Ok(());
        }
    }
    for parameter in &ir.model.parameters {
        for dependency in &parameter.dependencies {
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.parameters.len()), "NX configuration parameter dependencies")?;
            let Some(preceding) = ir.model.parameters.iter().find(|candidate| candidate.id == *dependency) else {
                return Ok(());
            };
            if preceding.owner != parameter.owner || preceding.ordinal >= parameter.ordinal {
                return Ok(());
            }
        }
    }
    // A parameter with no evaluated value leaves the configuration untouched,
    // which is what the dependency guard above does for its own case.
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.parameters.len()), "NX configuration parameter values")?;
    if ir.model.parameters.iter().any(|parameter| parameter.value.is_none()) {
        return Ok(());
    }
    let mut values = BTreeMap::new();
    for parameter in &ir.model.parameters {
        let Some(value) = parameter.value.as_ref() else {
            return Ok(());
        };
        ctx.charge_collection_items(1, "NX active configuration parameter values")?;
        let value_text_bytes = match value {
            ParameterValue::String(text) => text.len(),
            _ => 0,
        };
        let bytes = std::mem::size_of::<(ParameterId, ParameterValue)>()
            .checked_add(parameter.id.as_str().len())
            .and_then(|bytes| bytes.checked_add(value_text_bytes))
            .ok_or_else(|| ctx.refuse_codec_limit("NX active configuration parameter value", 0, cadmpeg_core::decode::u64_from_index(value_text_bytes)))?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX active configuration parameter values")?;
        values.insert(parameter.id.clone(), value.clone());
    }
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.parameter_values = values;
    annotations
        .derived(configuration.id.as_str(), "parameter_values")
        .map_err(cadmpeg_core::CodecError::malformed)?;
    Ok(())
}

fn attach_current_feature_states(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let current_bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let Ok(active_features) = active_feature_closure(ir, &current_bodies) else {
        return Ok(());
    };
    for index in active_features.into_values() {
        let feature = &mut ir.model.features[index];
        feature.suppressed = Some(false);
        annotations
            .derived(&feature.id, "suppressed")
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    Ok(())
}

fn attach_active_configuration_feature_states(
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
        .map(<[BodyId]>::to_vec)
    else {
        return Ok(());
    };
    if !ir.model.configurations[configuration_index]
        .feature_states
        .is_empty()
    {
        return Ok(());
    }
    let Ok(active_features) = active_feature_closure(ir, &configuration_bodies) else {
        return Ok(());
    };
    let states = active_features
        .iter()
        .map(|(id, &index)| {
            let feature = &ir.model.features[index];
            (
                id.clone(),
                ConfigurationFeatureState {
                    evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                        outputs: feature.evaluation.outputs().iter().cloned().collect(),
                    },
                    dependencies: feature.dependencies.clone(),
                    definition: feature.evaluation.definition().clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for index in active_features.into_values() {
        let feature = &mut ir.model.features[index];
        if feature.suppressed != Some(false) {
            feature.suppressed = Some(false);
            annotations
                .derived(&feature.id, "suppressed")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
    }
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.feature_states = states;
    annotations
        .derived(configuration.id.as_str(), "feature_states")
        .map_err(cadmpeg_core::CodecError::malformed)?;
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
    let scan_work = body_count
        .checked_mul(body_bindings.len())
        .ok_or_else(|| ctx.refuse_codec_limit(
            "NX retained-history body binding scan",
            0,
            cadmpeg_core::decode::u64_from_index(body_count),
        ))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan_work),
        "NX retained-history body binding scan",
    )?;
    let has_binding = ir.model.bodies.iter().any(|body| {
        body_bindings.iter().any(|binding| {
            body.id.as_str().starts_with(&format!("nx:s{}:", binding.stream_ordinal))
        })
    });
    if !has_binding {
        return Ok(None);
    }

    let sorting_work = body_count.checked_mul(body_count).ok_or_else(|| {
        ctx.refuse_codec_limit(
            "NX retained-history body order",
            0,
            cadmpeg_core::decode::u64_from_index(body_count),
        )
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(sorting_work),
        "NX retained-history body order",
    )?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(body_count),
        "NX retained-history body order",
    )?;
    let sorting_bytes = body_count
        .checked_mul(std::mem::size_of::<&cadmpeg_ir::topology::Body>())
        .ok_or_else(|| ctx.refuse_codec_limit(
            "NX retained-history body order",
            0,
            cadmpeg_core::decode::u64_from_index(body_count),
        ))?;
    let _sorting = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(sorting_bytes),
        "NX retained-history body order",
    )?;
    let mut sorted_bodies = Vec::new();
    reserve_attach_vec(ctx, &mut sorted_bodies, body_count, "NX retained-history body order")?;
    sorted_bodies.extend(&ir.model.bodies);
    sorted_bodies.sort_by(|first, second| first.id.cmp(&second.id));

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
        if sorted_bodies.get(position + 1).is_some_and(|next| next.id == body.id) {
            continue;
        }
        let mut matched = false;
        for binding in body_bindings {
            if !body.id.as_str().starts_with(&format!("nx:s{}:", binding.stream_ordinal)) {
                continue;
            }
            matched = true;
            let mut digits = 1usize;
            let mut value = binding_ordinal;
            while value >= 10 {
                value /= 10;
                digits += 1;
            }
            let key_bytes = "segment_body_binding.".len().checked_add(digits)
                .ok_or_else(|| ctx.refuse_codec_limit(
                    "NX retained-history binding property",
                    0,
                    cadmpeg_core::decode::u64_from_index(binding_ordinal),
                ))?;
            let bytes = std::mem::size_of::<(String, String)>()
                .checked_add(key_bytes)
                .and_then(|bytes| bytes.checked_add(binding.id.len()))
                .ok_or_else(|| ctx.refuse_codec_limit(
                    "NX retained-history binding property",
                    0,
                    cadmpeg_core::decode::u64_from_index(binding.id.len()),
                ))?;
            ctx.charge_collection_items(1, "NX retained-history binding properties")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(bytes),
                "NX retained-history binding properties",
            )?;
            source_properties.insert(
                cadmpeg_core::nonblank_literal!("segment_body_binding.{binding_ordinal}"),
                binding.id.clone(),
            );
            binding_ordinal = binding_ordinal.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("NX retained-history binding ordinal", 0, 1)
            })?;
        }
        if matched {
            ctx.charge_collection_items(2, "NX retained-history output bodies")?;
            let body_bytes = std::mem::size_of::<BodyId>()
                .checked_add(body.id.as_str().len())
                .ok_or_else(|| ctx.refuse_codec_limit(
                    "NX retained-history output body",
                    0,
                    cadmpeg_core::decode::u64_from_index(body.id.as_str().len()),
                ))?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(body_bytes),
                "NX retained-history output bodies",
            )?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(body_bytes),
                "NX retained-history output bodies",
            )?;
            reserve_attach_vec(ctx, &mut selection_bodies, 1, "NX retained-history output bodies")?;
            reserve_attach_vec(ctx, &mut feature_outputs, 1, "NX retained-history output bodies")?;
            selection_bodies.push(body.id.clone());
            feature_outputs.push(body.id.clone());
        }
    }

    annotations.note(&id, stream, 0).tag("FEATURE_HISTORY_INPUT");
    if annotations.derived(&id, "definition").is_err() {
        return Ok(None);
    }
    let uniqueness_work = feature_outputs.len()
        .checked_mul(feature_outputs.len())
        .and_then(|work| work.checked_mul(2))
        .ok_or_else(|| ctx.refuse_codec_limit(
            "NX retained-history output uniqueness",
            0,
            cadmpeg_core::decode::u64_from_index(feature_outputs.len()),
        ))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(uniqueness_work),
        "NX retained-history output uniqueness",
    )?;
    ctx.charge_collection_items(1, "NX retained-history input features")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Feature>()),
        "NX retained-history input features",
    )?;
    reserve_attach_vec(ctx, &mut ir.model.features, 1, "NX retained-history input features")?;
    ir.model.features.push(Feature {
        id: id.clone(),
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
                    bodies: DistinctMembers::try_from_unique_vec(selection_bodies)
                        .map_err(CodecError::malformed)?,
                    native: "nx:segment-body-bindings".to_string(),
                },
            }),
            DistinctMembers::try_from_unique_vec(feature_outputs)
                .map_err(CodecError::malformed)?,
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
    let parasolid_group_members = model.parasolid.parasolid_group_members.as_slice();
    let data_blocks = model.om.data_blocks.as_slice();
    let expressions = model.om.expressions.as_slice();
    let body_bindings = model.segments.segment_body_bindings.as_slice();
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
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    let initial_body_id = attach_initial_segment_bodies(ctx, ir, body_bindings, annotations, &stream)?;
    let base_ordinal = ir.model.features.len() as u64;
    let booleans = last_record_index(ctx, booleans
        .iter()
        .map(|operation| (operation.operation_label.as_str(), operation))
        )?;
    let body_references_by_id = last_record_index(ctx, body_references
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        )?;
    let mut group_reservation = ctx.reserve_scoped(0, "NX feature operation group indexes")?;
    let mut body_segment_uses_by_reference =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodySegmentUse>>::new();
    for use_ in body_segment_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut body_segment_uses_by_reference, use_.feature_body_reference.as_str(), || use_, 0)?;
    }
    let mut body_data_block_uses_by_reference =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodyDataBlockUse>>::new();
    for use_ in body_data_block_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut body_data_block_uses_by_reference, use_.feature_body_reference.as_str(), || use_, 0)?;
    }
    let body_writer_references_by_operation =
        crate::native::features::unique_feature_body_references(ctx, body_references)?;
    let mut offset_store_bodies_by_operation = BTreeMap::<&str, Vec<(u32, String)>>::new();
    for body_use in body_data_block_uses {
        let Some(reference) = body_references_by_id.get(body_use.feature_body_reference.as_str())
        else {
            continue;
        };
        push_grouped_operation(ctx, &mut group_reservation, &mut offset_store_bodies_by_operation, reference.operation_label.as_str(), || (reference.body.value(), body_use.data_block.clone()), body_use.data_block.len())?;
    }
    let body_references = admitted_body_references;
    let mut body_reference_occurrences_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBodyReference>>::new();
    for reference in body_reference_occurrences {
        push_grouped_operation(ctx, &mut group_reservation, &mut body_reference_occurrences_by_operation, reference.operation_label.as_str(), || reference, 0)?;
    }
    let mut body_writer_history = BodyWriterHistory::default();
    if let Some(feature) = initial_body_id
        .as_ref()
        .and_then(|id| ir.model.features.iter().find(|feature| feature.id == *id))
    {
        body_writer_history.record_writer(ctx, None, None, feature.evaluation.outputs(), &feature.id)?;
    }
    let body_alias_roots = crate::native::segments::body_alias_roots(ctx, body_bindings)?;
    let canonical_body =
        |identity: u32| body_alias_roots.get(&identity).copied().unwrap_or(identity);
    let mut input_blocks_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureInputBlock>>::new();
    for input in input_blocks {
        push_grouped_operation(ctx, &mut group_reservation, &mut input_blocks_by_operation, input.operation_label.as_str(), || input, 0)?;
    }
    let input_column_row_uses_by_operation =
        records_by_operation(ctx, input_column_row_uses, |use_| &use_.operation_label)?;
    let input_column_targets_by_operation =
        records_by_operation(ctx, input_column_targets, |target| &target.operation_label)?;
    let input_block_identity_group_by_input = last_record_index(ctx, input_block_identity_groups
        .iter()
        .flat_map(|group| {
            group
                .members
                .iter()
                .map(move |member| (member.input_block.as_str(), group.id.as_str()))
        })
        )?;
    let datum_csys_constructions_by_operation = last_record_index(ctx, datum_csys_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        )?;
    let datum_csys_payloads_by_operation =
        records_by_operation(ctx, datum_csys_payloads, |payload| &payload.operation_label)?;
    let datum_csys_payload_scalar_pairs_by_operation =
        records_by_operation(ctx, datum_csys_payload_scalar_pairs, |pair| {
            &pair.operation_label
        })?;
    let datum_csys_payload_fixed_pairs_by_operation =
        records_by_operation(ctx, datum_csys_payload_fixed_pairs, |pair| &pair.operation_label)?;
    let datum_csys_payload_scalars_by_operation =
        records_by_operation(ctx, datum_csys_payload_scalars, |scalar| &scalar.operation_label)?;
    let datum_csys_descriptors_by_operation =
        records_by_operation(ctx, datum_csys_descriptors, |descriptor| {
            &descriptor.operation_label
        })?;
    let datum_csys_column_row_uses_by_operation =
        records_by_operation(ctx, datum_csys_column_row_uses, |use_| &use_.operation_label)?;
    let mut datum_csys_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumCsysBlockUse>>::new();
    for block_use in datum_csys_block_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut datum_csys_uses_by_input_operation, block_use.input_operation_label.as_str(), || block_use, 0)?;
    }
    let datum_plane_headers_by_operation = last_record_index(ctx, datum_plane_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        )?;
    let datum_plane_payloads_by_operation = last_record_index(ctx, datum_plane_payloads
        .iter()
        .map(|payload| (payload.operation_label.as_str(), payload))
        )?;
    let datum_plane_payload_scalar_pairs_by_operation =
        records_by_operation(ctx, datum_plane_payload_scalar_pairs, |pair| {
            &pair.operation_label
        })?;
    let datum_plane_descriptors_by_operation =
        records_by_operation(ctx, datum_plane_descriptors, |descriptor| {
            &descriptor.operation_label
        })?;
    let mut datum_plane_uses_by_input_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneBlockUse>>::new();
    for block_use in datum_plane_block_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut datum_plane_uses_by_input_operation, block_use.input_operation_label.as_str(), || block_use, 0)?;
    }
    let chronological_labels =
        crate::native::features::feature_operation_chronological_labels(ctx, labels)?;
    let operation_positions = last_record_index(ctx, chronological_labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        )?;
    let sketch_datum_csys_dependencies = last_record_index(ctx, sketch_datum_csys_dependencies
        .iter()
        .map(|dependency| (dependency.datum_csys_operation_label.as_str(), dependency))
        )?;
    let mut datum_identity_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureDatumPlaneCsysIdentityUse>>::new();
    for identity_use in datum_plane_csys_identity_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut datum_identity_uses_by_operation, identity_use.datum_plane_operation_label.as_str(), || identity_use, 0)?;
        push_grouped_operation(ctx, &mut group_reservation, &mut datum_identity_uses_by_operation, identity_use.datum_csys_operation_label.as_str(), || identity_use, 0)?;
    }
    let mut sketch_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchReference>>::new();
    for reference in sketch_references {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_references_by_operation, reference.operation_label.as_str(), || reference, 0)?;
    }
    let projected_curve_references_by_operation =
        records_by_operation(ctx, projected_curve_references, |reference| {
            &reference.operation_label
        })?;
    let projected_curve_construction_payloads_by_operation =
        records_by_operation(ctx, projected_curve_construction_payloads, |payload| {
            &payload.operation_label
        })?;
    let projected_curve_construction_strings_by_operation =
        records_by_operation(ctx, projected_curve_construction_strings, |value| {
            &value.operation_label
        })?;
    let fset_reference_graphs_by_operation =
        records_by_operation(ctx, fset_reference_graphs, |graph| &graph.operation_label)?;
    let fset_construction_payloads_by_operation =
        records_by_operation(ctx, fset_construction_payloads, |payload| {
            &payload.operation_label
        })?;
    let delete_reference_fields_by_operation =
        records_by_operation(ctx, delete_reference_fields, |field| &field.operation_label)?;
    let delete_construction_payloads_by_operation =
        records_by_operation(ctx, delete_construction_payloads, |payload| {
            &payload.operation_label
        })?;
    let pattern_references_by_operation =
        records_by_operation(ctx, pattern_references, |reference| &reference.operation_label)?;
    let pattern_counted_reference_lanes_by_operation =
        records_by_operation(ctx, pattern_counted_reference_lanes, |lane| {
            &lane.operation_label
        })?;
    let pattern_construction_payloads_by_operation =
        records_by_operation(ctx, pattern_construction_payloads, |payload| {
            &payload.operation_label
        })?;
    let pattern_construction_strings_by_operation =
        records_by_operation(ctx, pattern_construction_strings, |value| &value.operation_label)?;
    let pattern_construction_fixed_lanes_by_operation =
        records_by_operation(ctx, pattern_construction_fixed_lanes, |lane| {
            &lane.operation_label
        })?;
    let pattern_transform_lanes_by_operation =
        records_by_operation(ctx, pattern_transform_lanes, |lane| &lane.operation_label)?;
    let multi_instance_output_lanes_by_operation =
        records_by_operation(ctx, multi_instance_output_lanes, |lane| &lane.operation_label)?;
    let identical_instance_output_lanes_by_operation =
        records_by_operation(ctx, identical_instance_output_lanes, |lane| {
            &lane.operation_label
        })?;
    let point_construction_headers_by_operation = last_record_index(ctx, point_construction_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        )?;
    let point_construction_scalar_lanes_by_operation = last_record_index(ctx, point_construction_scalar_lanes
        .iter()
        .map(|lane| (lane.operation_label.as_str(), lane))
        )?;
    let draft_construction_references_by_operation =
        records_by_operation(ctx, draft_construction_references, |reference| {
            &reference.operation_label
        })?;
    let draft_construction_index_lanes_by_operation =
        records_by_operation(ctx, draft_construction_index_lanes, |lane| &lane.operation_label)?;
    let draft_construction_payloads_by_operation =
        records_by_operation(ctx, draft_construction_payloads, |payload| {
            &payload.operation_label
        })?;
    let draft_construction_graph_payloads_by_operation =
        records_by_operation(ctx, draft_construction_graph_payloads, |payload| {
            &payload.operation_label
        })?;
    let draft_construction_fixed_lanes_by_operation =
        records_by_operation(ctx, draft_construction_fixed_lanes, |lane| &lane.operation_label)?;
    let draft_construction_binary32_lanes_by_operation =
        records_by_operation(ctx, draft_construction_binary32_lanes, |lane| {
            &lane.operation_label
        })?;
    let draft_construction_graph_strings_by_operation =
        records_by_operation(ctx, draft_construction_graph_strings, |value| {
            &value.operation_label
        })?;
    let draft_construction_identity_frames_by_operation =
        records_by_operation(ctx, draft_construction_identity_frames, |frame| {
            &frame.operation_label
        })?;
    let draft_construction_terminal_lanes_by_operation =
        records_by_operation(ctx, draft_construction_terminal_lanes, |lane| {
            &lane.operation_label
        })?;
    let surface_construction_references_by_operation =
        records_by_operation(ctx, surface_construction_references, |reference| {
            &reference.operation_label
        })?;
    let surface_construction_payloads_by_operation =
        records_by_operation(ctx, surface_construction_payloads, |payload| {
            &payload.operation_label
        })?;
    let surface_construction_scalar_pairs_by_operation =
        records_by_operation(ctx, surface_construction_scalar_pairs, |pair| {
            &pair.operation_label
        })?;
    let surface_construction_strings_by_operation =
        records_by_operation(ctx, surface_construction_strings, |value| &value.operation_label)?;
    let surface_construction_branches_by_operation =
        records_by_operation(ctx, surface_construction_branches, |branch| {
            &branch.operation_label
        })?;
    let mut sketch_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchNamedPointBlockUse>>::new();
    for block_use in sketch_named_point_block_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_named_point_uses_by_operation, block_use.operation_label.as_str(), || block_use, 0)?;
    }
    let mut sketch_preceding_named_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPrecedingNamedPointUse>>::new();
    for point_use in sketch_preceding_named_point_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_preceding_named_point_uses_by_operation, point_use.operation_label.as_str(), || point_use, 0)?;
    }
    let mut sketch_point_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPointUse>>::new();
    for point_use in sketch_point_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_point_uses_by_operation, point_use.operation_label.as_str(), || point_use, 0)?;
    }
    let mut sketch_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPointGroup>>::new();
    for group in sketch_point_groups {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_point_groups_by_operation, group.operation_label.as_str(), || group, 0)?;
    }
    let mut extrude_profile_references_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureExtrudeProfileReference>>::new();
    for reference in extrude_profile_references {
        push_grouped_operation(ctx, &mut group_reservation, &mut extrude_profile_references_by_operation, reference.operation_label.as_str(), || reference, 0)?;
    }
    let extrude_construction_profiles_by_operation = last_record_index(ctx, extrude_construction_profiles
        .iter()
        .map(|profile| (profile.operation_label.as_str(), profile))
        )?;
    let mut operation_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands {
        push_grouped_operation(ctx, &mut group_reservation, &mut operation_body_operands_by_operation, operand.operation_label.as_str(), || operand, 0)?;
    }
    let mut segment_body_operands_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyOperand>>::new();
    for operand in operation_body_operands
        .iter()
        .filter(|operand| !operand.segment_body_bindings.is_empty())
    {
        push_grouped_operation(ctx, &mut group_reservation, &mut segment_body_operands_by_operation, operand.operation_label.as_str(), || operand, 0)?;
    }
    let sketch_construction_inputs_by_operation = last_record_index(ctx, sketch_construction_inputs
        .iter()
        .map(|inputs| (inputs.operation_label.as_str(), inputs))
        )?;
    let sketch_records_by_operation =
        records_by_operation(ctx, sketch_records, |record| &record.operation_label)?;
    let sketch_construction_payloads_by_operation =
        records_by_operation(ctx, sketch_construction_payloads, |payload| {
            &payload.operation_label
        })?;
    let mut sketch_coordinate_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeaturePayloadScalarPair>>::new();
    for pair in sketch_coordinate_pairs {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_coordinate_pairs_by_operation, pair.operation_label.as_str(), || pair, 0)?;
    }
    let mut sketch_fixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadFixedPair>>::new();
    for pair in sketch_fixed_pairs {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_fixed_pairs_by_operation, pair.operation_label.as_str(), || pair, 0)?;
    }
    let mut sketch_mixed_pairs_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadMixedPair>>::new();
    for pair in sketch_mixed_pairs {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_mixed_pairs_by_operation, pair.operation_label.as_str(), || pair, 0)?;
    }
    let mut sketch_payload_scalar_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchPayloadScalarLane>>::new();
    for lane in sketch_payload_scalar_lanes {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_payload_scalar_lanes_by_operation, lane.operation_label.as_str(), || lane, 0)?;
    }
    let mut sketch_fixed_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureSketchFixedPoint>>::new();
    for point in sketch_fixed_points {
        push_grouped_operation(ctx, &mut group_reservation, &mut sketch_fixed_points_by_operation, point.operation_label.as_str(), || point, 0)?;
    }
    let block_constructions_by_operation = last_record_index(ctx, block_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        )?;
    let block_construction_payloads_by_operation =
        records_by_operation(ctx, block_construction_payloads, |payload| {
            &payload.operation_label
        })?;
    let block_dimensions_by_operation = last_record_index(ctx, block_dimensions
        .iter()
        .map(|dimensions| (dimensions.operation_label.as_str(), dimensions))
        )?;
    let mut block_payload_points_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBlockPayloadPoint>>::new();
    for point in block_payload_points {
        push_grouped_operation(ctx, &mut group_reservation, &mut block_payload_points_by_operation, point.operation_label.as_str(), || point, 0)?;
    }
    let mut block_payload_point_groups_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureBlockPayloadPointGroup>>::new();
    for group in block_payload_point_groups {
        push_grouped_operation(ctx, &mut group_reservation, &mut block_payload_point_groups_by_operation, group.operation_label.as_str(), || group, 0)?;
    }
    let extrude_32_constructions_by_operation = last_record_index(ctx, extrude_32_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        )?;
    let extrude_payload_headers_by_operation = last_record_index(ctx, extrude_payload_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        )?;
    let operation_terminal_discriminators_by_operation = last_record_index(ctx, operation_terminal_discriminators
        .iter()
        .map(|lane| (lane.operation_label.as_str(), lane))
        )?;
    let extrude_payload_32_branches_by_operation =
        records_by_operation(ctx, extrude_payload_32_branches, |branch| {
            &branch.operation_label
        })?;
    let mut operation_body_scalar_triples_by_operation = BTreeMap::<
        &str,
        Vec<&crate::native::features::body_scalar_triple::FeatureOperationBodyScalarTriple>,
    >::new();
    for triple in operation_body_scalar_triples {
        push_grouped_operation(ctx, &mut group_reservation, &mut operation_body_scalar_triples_by_operation, triple.operation_label.as_str(), || triple, 0)?;
    }
    for triples in operation_body_scalar_triples_by_operation.values_mut() {
        triples.sort_by_key(|triple| triple.body_reference_ordinal);
    }
    let mut operation_body_members_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyMember>>::new();
    for member in operation_body_members {
        push_grouped_operation(ctx, &mut group_reservation, &mut operation_body_members_by_operation, member.operation_label.as_str(), || member, 0)?;
    }
    let mut operation_body_11_continuations_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBody11Continuation>>::new();
    for continuation in operation_body_11_continuations {
        push_grouped_operation(ctx, &mut group_reservation, &mut operation_body_11_continuations_by_operation, continuation.operation_label.as_str(), || continuation, 0)?;
    }
    let mut operation_body_reference_lanes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyReferenceLane>>::new();
    for lane in operation_body_reference_lanes {
        push_grouped_operation(ctx, &mut group_reservation, &mut operation_body_reference_lanes_by_operation, lane.operation_label.as_str(), || lane, 0)?;
    }
    let (bodies_by_object_index, bodies_by_segment_binding, _body_index_reservation) =
        segment_binding_body_indexes(ctx, ir, body_bindings)?;
    let (mut body_image_outputs_by_write, mut body_output_reservation) = operation_body_image_outputs_by_write(
        ctx,
        operation_body_image_segment_uses,
        &bodies_by_segment_binding,
    )?;
    let mut conflicting_body_output_writes = BTreeSet::new();
    let (identity_candidates, _identity_candidate_reservation) = operation_body_identity_outputs_by_write(
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
    let (partition_candidates, _partition_candidate_reservation) = operation_body_group_partition_outputs_by_write(
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
    if let Some(projection) = hole_body_projection(ctx, ir, &simple_hole_operations, &hole_outputs)? {
        hole_outputs.extend(projection.outputs);
        simple_hole_diameters.extend(projection.diameters);
    }
    let counterbore_operations =
        counterbore_operations(ctx, simple_hole_templates, &operation_positions)?.unwrap_or_default();
    let mut counterbore_dimensions = BTreeMap::new();
    if let Some(projection) =
        counterbore_body_projection(ctx, ir, &counterbore_operations, &hole_outputs)?
    {
        hole_outputs.extend(projection.outputs);
        simple_hole_diameters.extend(projection.diameters);
        counterbore_dimensions.extend(projection.counterbores);
    }
    let blind_hole_operations =
        blind_hole_operations(ctx, simple_hole_templates, &operation_positions)?.unwrap_or_default();
    let mut blind_hole_depths = BTreeMap::new();
    if let Some(projection) = blind_hole_body_projection(ctx, ir, &blind_hole_operations, &hole_outputs)?
    {
        hole_outputs.extend(projection.outputs);
        simple_hole_diameters.extend(projection.diameters);
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
        &hole_outputs,
        &simple_hole_diameters,
        &simple_hole_chamfers,
    )?;
    let mut feature_ids_by_operation = BTreeMap::new();
    let mut feature_id_reservation = ctx.reserve_scoped(0, "NX operation feature identities")?;
    for label in labels {
        ctx.charge_work(1, "NX operation feature identity scan")?;
        if !projects_neutral_feature(&label.value)
            || hole_packages.internal_operations.contains(&label.id)
        {
            continue;
        }
        let key = label.id.strip_prefix("nx:feature-history:operation-label#")
            .unwrap_or(label.id.as_str());
        if key.is_empty() || key.contains('#') || key.chars().any(char::is_whitespace) {
            continue;
        }
        const PREFIX: &str = "nx:feature-history:feature#";
        let id_len = PREFIX.len().checked_add(key.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX operation feature identity", 0, cadmpeg_core::decode::u64_from_index(key.len())))?;
        let mut id_text = String::new();
        ctx.charge_work(1, "NX operation feature identity index")?;
        if !feature_ids_by_operation.contains_key(label.id.as_str()) {
            ctx.charge_collection_items(1, "NX operation feature identity index")?;
            feature_id_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, FeatureId)>()))?;
        }
        feature_id_reservation.grow(cadmpeg_core::decode::u64_from_index(id_len))?;
        id_text.try_reserve_exact(id_len).map_err(|_| ctx.refuse_codec_limit("NX operation feature identity", 0, cadmpeg_core::decode::u64_from_index(id_len)))?;
        id_text.push_str(PREFIX);
        id_text.push_str(key);
        let Ok(id) = FeatureId::mint(id_text) else {
            continue;
        };
        feature_ids_by_operation.insert(label.id.as_str(), id);
    }
    let mut parameter_bindings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterBinding>>::new();
    for binding in parameter_bindings {
        push_grouped_operation(ctx, &mut group_reservation, &mut parameter_bindings_by_operation, binding.operation_label.as_str(), || binding, 0)?;
    }
    let mut parameter_uses_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureParameterUse>>::new();
    for parameter_use in parameter_uses {
        push_grouped_operation(ctx, &mut group_reservation, &mut parameter_uses_by_operation, parameter_use.operation_label.as_str(), || parameter_use, 0)?;
    }
    let operation_labels_by_record = last_record_index(ctx, operation_records
        .iter()
        .map(|record| (record.id.as_str(), record.operation_label.as_str()))
        )?;
    let mut body_writes_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeatureOperationBodyWrite>>::new();
    for (write, operation_label) in operation_body_writes
        .iter()
        .filter_map(|write| write.operation_label.as_deref().map(|label| (write, label)))
    {
        push_grouped_operation(ctx, &mut group_reservation, &mut body_writes_by_operation, operation_label, || write, 0)?;
    }
    let mut body_identity_writers = BTreeMap::<u8, FeatureId>::new();
    let mut payload_strings_by_operation =
        BTreeMap::<&str, Vec<&crate::native::features::FeaturePayloadString>>::new();
    for value in payload_strings {
        let Some(operation) = operation_labels_by_record.get(value.operation_record.as_str())
        else {
            continue;
        };
        push_grouped_operation(ctx, &mut group_reservation, &mut payload_strings_by_operation, *operation, || value, 0)?;
    }
    let mut parameter_owners = BTreeMap::new();
    let mut parameter_owner_reservation = ctx.reserve_scoped(0, "NX parameter owner index")?;
    for parameter in &ir.model.parameters {
        ctx.charge_work(1, "NX parameter owner index")?;
        let owner_len = parameter.owner.as_ref().map_or(0, |owner| owner.as_str().len());
        let bytes = std::mem::size_of::<(ParameterId, Option<FeatureId>)>()
            .checked_add(parameter.id.as_str().len())
            .and_then(|bytes| bytes.checked_add(owner_len))
            .ok_or_else(|| ctx.refuse_codec_limit("NX parameter owner index", 0, cadmpeg_core::decode::u64_from_index(owner_len)))?;
        if !parameter_owners.contains_key(&parameter.id) {
            ctx.charge_collection_items(1, "NX parameter owner index")?;
        }
        parameter_owner_reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        parameter_owners.insert(parameter.id.clone(), parameter.owner.clone());
    }
    let annotation_base_order = id_from_index(ir.model.semantic_annotations.len());
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
        let order = annotation_base_order.and_then(|base| {
            id_from_index(annotation_ordinal).and_then(|ordinal| base.checked_add(ordinal))
        });
        let Some(order) = order else {
            losses.push(NxLossCode::SemanticAnnotationOrderUnstatable.note(format!(
                "NX TEXT label {} lies past the stated semantic-annotation order width, so it \
                 states no order and is not projected.",
                label.id
            )));
            continue;
        };
        let Some(annotation) = text_semantic_annotation(&label.id, order, &payload_strings) else {
            continue;
        };
        annotations
            .note(annotation.id.as_str(), &stream, label.source_offset)
            .tag("TEXT_SEMANTIC_ANNOTATION");
        annotations.exactness(annotation.id.as_str(), Exactness::Derived);
        ir.model.semantic_annotations.push(annotation);
    }
    for (ordinal, label) in chronological_labels.into_iter().enumerate() {
        if !projects_neutral_feature(&label.value)
            || hole_packages.internal_operations.contains(&label.id)
        {
            continue;
        }
        let Some(id) = feature_ids_by_operation.get(label.id.as_str()).cloned() else {
            continue;
        };
        let boolean_offset_store_resolution = booleans.get(label.id.as_str()).map(|operation| {
            crate::native::segments::boolean_offset_store_resolution(ctx, operation, data_blocks)
        }).transpose()?;
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
            if let Some(feature) = feature_ids_by_operation
                .get(dependency.sketch_operation_label.as_str())
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
            insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.body_identity"), format_args!("{}", write.frame.body_identity()))?;
            insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.group_node"), format_args!("{}", write.frame.group_node().value()))?;
            insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.endpoint_tag"), format_args!("{}", write.frame.endpoint_tag().code()))?;
            insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.body_image_object_index"), format_args!("{}", write.frame.body_image().value()))?;
            if let Some(use_) = operation_body_image_segment_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.body_image_segment_use"), format_args!("{}", use_.id))?;
                insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.segment_body_binding"), format_args!("{}", use_.segment_body_binding))?;
            }
            if let Some(use_) = operation_body_identity_segment_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.body_identity_segment_use"), format_args!("{}", use_.id))?;
                insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.body_identity_segment_binding"), format_args!("{}", use_.segment_body_binding))?;
            }
            if let Some(use_) = operation_body_partition_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.partition_use"), format_args!("{}", use_.id))?;
                insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.partition_stream_ordinal"), format_args!("{}", use_.partition_stream_ordinal))?;
                for (group_ordinal, group) in use_.parasolid_group_records.iter().enumerate() {
                    insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.parasolid_group.{group_ordinal}"), format_args!("{}", group))?;
                }
                for (member_ordinal, member) in use_.parasolid_group_members.iter().enumerate() {
                    insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.parasolid_group_member.{member_ordinal}"), format_args!("{}", member))?;
                }
            }
            if let Some(use_) = body_write_group_partition_uses
                .iter()
                .find(|use_| use_.body_write == write.id)
            {
                insert_source_property(ctx, &mut source_properties, format_args!("body_write.{ordinal}.group_partition_use"), format_args!("{}", use_.id))?;
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
            insert_source_property(ctx, &mut source_properties, format_args!("operation_stable_identity"), format_args!("{}", stable_identity))?;
        }
        for (use_ordinal, block_use) in datum_csys_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_csys_block_use.{use_ordinal}"), format_args!("{}", block_use.id))?;
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_point_dependency_use"), format_args!("{}", dependency.sketch_point_use))?;
            match &dependency.block_relation {
                crate::native::features::FeatureSketchDatumCsysBlockRelation::Shared {
                    data_block,
                } => {
                    insert_source_property(ctx, &mut source_properties, format_args!("sketch_point_dependency_shared_block"), format_args!("{}", data_block))?;
                }
                crate::native::features::FeatureSketchDatumCsysBlockRelation::Consecutive {
                    point_data_block,
                    construction_data_block,
                } => {
                    insert_source_property(ctx, &mut source_properties, format_args!("sketch_point_dependency_point_block"), format_args!("{}", point_data_block))?;
                    insert_source_property(ctx, &mut source_properties, format_args!("sketch_point_dependency_construction_block"), format_args!("{}", construction_data_block))?;
                }
            }
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_datum_csys_dependency"), format_args!("{}", dependency.id))?;
            for (alias_ordinal, alias) in dependency.scalar_aliases.iter().enumerate() {
                insert_source_property(ctx, &mut source_properties, format_args!("sketch_point_dependency_scalar.{alias_ordinal}"), format_args!("{}", alias.datum_csys_scalar))?;
                insert_source_property(ctx, &mut source_properties, format_args!("sketch_point_dependency_coordinate.{alias_ordinal}"), format_args!("{}", alias.sketch_coordinate_ordinal))?;
            }
        }
        let deletes_body = label.value == "DELETE";
        let mut outputs = if deletes_body {
            Vec::new()
        } else {
            match body_references.get(label.id.as_str()) {
                Some(body) => feature_body_outputs(ctx, *body, body_bindings, &bodies_by_object_index)?,
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
            outputs = hole_outputs
                .get(label.id.as_str())
                .or_else(|| hole_packages.outputs.get(label.id.as_str()))
                .cloned()
                .unwrap_or_default();
        }
        if outputs.is_empty() {
            if let Some(body) = boolean_target_output(boolean_definition.as_ref()) {
                outputs.push(body);
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
            insert_source_property(ctx, &mut source_properties, format_args!("{}", NATIVE_PRIMARY_BODY_OBJECT_INDEX), format_args!("{}", body))?;
        }
        if let Some(reference) = body_writer_references_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("primary_body_reference"), format_args!("{}", reference.id))?;
            if let Some(uses) = body_segment_uses_by_reference.get(reference.id.as_str()) {
                if let [use_] = uses.as_slice() {
                    insert_source_property(ctx, &mut source_properties, format_args!("primary_body_segment_use"), format_args!("{}", use_.id))?;
                    insert_source_property(ctx, &mut source_properties, format_args!("primary_body_segment_binding"), format_args!("{}", use_.segment_body_binding))?;
                }
            }
            if let Some(uses) = body_data_block_uses_by_reference.get(reference.id.as_str()) {
                if let [use_] = uses.as_slice() {
                    insert_source_property(ctx, &mut source_properties, format_args!("primary_body_data_block_use"), format_args!("{}", use_.id))?;
                    insert_source_property(ctx, &mut source_properties, format_args!("primary_body_data_block"), format_args!("{}", use_.data_block))?;
                }
            }
        }
        for (reference, ordinal) in body_reference_occurrences_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .filter_map(|reference| reference.ordinal.map(|ordinal| (reference, ordinal)))
        {
            insert_source_property(ctx, &mut source_properties, format_args!("body_reference.{ordinal}"), format_args!("{}", reference.body.value()))?;
            insert_source_property(ctx, &mut source_properties, format_args!("body_reference_occurrence.{ordinal}"), format_args!("{}", reference.id))?;
        }
        if let Some(inputs) = sketch_construction_inputs_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_construction_inputs"), format_args!("{}", inputs.id))?;
        }
        for (ordinal, record) in sketch_records_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_record.{ordinal}"), format_args!("{}", record.id))?;
        }
        for (ordinal, payload) in sketch_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_construction_payload.{ordinal}"), format_args!("{}", payload.id))?;
        }
        for pair in sketch_coordinate_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_coordinate_pair.{}", pair.ordinal), format_args!("{}", pair.id))?;
        }
        for pair in sketch_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_fixed_pair.{}", pair.ordinal), format_args!("{}", pair.id))?;
        }
        for pair in sketch_mixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_mixed_pair.{}", pair.ordinal), format_args!("{}", pair.id))?;
        }
        for lane in sketch_payload_scalar_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_scalar_lane.{}", lane.ordinal), format_args!("{}", lane.id))?;
        }
        for (ordinal, point) in sketch_fixed_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_fixed_point.{ordinal}"), format_args!("{}", point.id))?;
        }
        if let Some(construction) = block_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("block_construction"), format_args!("{}", construction.id))?;
        }
        for (ordinal, payload) in block_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("block_construction_payload.{ordinal}"), format_args!("{}", payload.id))?;
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("block_dimensions"), format_args!("{}", dimensions.id))?;
            for (dimension_ordinal, dimension) in dimensions.dimensions.iter().enumerate() {
                insert_source_property(ctx, &mut source_properties, format_args!("block_dimension_declaration.{dimension_ordinal}"), format_args!("{}", dimension.declaration))?;
                insert_source_property(ctx, &mut source_properties, format_args!("block_dimension_expression.{dimension_ordinal}"), format_args!("{}", dimension.expression))?;
            }
        }
        for (ordinal, point) in block_payload_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("block_payload_point.{ordinal}"), format_args!("{}", point.id))?;
        }
        for (ordinal, group) in block_payload_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("block_payload_point_group.{ordinal}"), format_args!("{}", group.id))?;
        }
        if let Some(construction) = extrude_32_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("extrude_32_construction"), format_args!("{}", construction.id))?;
        }
        if let Some(header) = extrude_payload_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("extrude_payload_header"), format_args!("{}", header.id))?;
        }
        if let Some(lane) = operation_terminal_discriminators_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("operation_terminal_discriminator"), format_args!("{}", lane.id))?;
        }
        for (ordinal, branch) in extrude_payload_32_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("extrude_payload_32_branch.{ordinal}"), format_args!("{}", branch.id))?;
        }
        for triple in operation_body_scalar_triples_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!(
                    "operation_body_scalar_triple.{}",
                    triple.body_reference_ordinal
                ), format_args!("{}", triple.id))?;
        }
        for member in operation_body_members_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!(
                    "operation_body_member.{}.{}",
                    member.body_reference_ordinal, member.ordinal
                ), format_args!("{}", member.id))?;
        }
        for continuation in operation_body_11_continuations_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!(
                    "operation_body_11_continuation.{}",
                    continuation.body_reference_ordinal
                ), format_args!("{}", continuation.id))?;
        }
        for lane in operation_body_reference_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!(
                    "operation_body_reference_lane.{}",
                    lane.body_reference_ordinal
                ), format_args!("{}", lane.id))?;
        }
        if let Some(construction) = datum_csys_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_csys_construction"), format_args!("{}", construction.id))?;
        }
        for (ordinal, use_) in datum_csys_column_row_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_csys_column_row_use.{ordinal}"), format_args!("{}", use_.id))?;
        }
        for (ordinal, payload) in datum_csys_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_csys_payload.{ordinal}"), format_args!("{}", payload.id))?;
        }
        for (ordinal, pair) in datum_csys_payload_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_csys_payload_scalar_pair.{ordinal}"), format_args!("{}", pair.id))?;
        }
        for (ordinal, pair) in datum_csys_payload_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_csys_payload_fixed_pair.{ordinal}"), format_args!("{}", pair.id))?;
        }
        for (ordinal, scalar) in datum_csys_payload_scalars_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_csys_payload_scalar.{ordinal}"), format_args!("{}", scalar.id))?;
        }
        for (ordinal, descriptor) in datum_csys_descriptors_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_csys_descriptor.{ordinal}"), format_args!("{}", descriptor.id))?;
        }
        if let Some(header) = datum_plane_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_plane_header"), format_args!("{}", header.id))?;
        }
        if let Some(payload) = datum_plane_payloads_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_plane_payload"), format_args!("{}", payload.id))?;
        }
        for (ordinal, pair) in datum_plane_payload_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_plane_payload_scalar_pair.{ordinal}"), format_args!("{}", pair.id))?;
        }
        for (ordinal, descriptor) in datum_plane_descriptors_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_plane_descriptor.{ordinal}"), format_args!("{}", descriptor.id))?;
        }
        for (ordinal, identity_use) in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_identity_use.{ordinal}"), format_args!("{}", identity_use.id))?;
        }
        for (use_ordinal, block_use) in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("datum_plane_block_use.{use_ordinal}"), format_args!("{}", block_use.id))?;
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
            insert_source_property(ctx, &mut source_properties, format_args!("hole_package_construction_group_lane"), format_args!("{}", lane.id))?;
        }
        for group_use in hole_package_construction_group_uses {
            let group = simple_hole_construction_groups
                .iter()
                .find(|group| group.id == group_use.simple_hole_construction_group);
            if group_use.operation_label == label.id {
                insert_source_property(ctx, &mut source_properties, format_args!("hole_package_construction_group_use"), format_args!("{}", group_use.id))?;
                insert_source_property(ctx, &mut source_properties, format_args!("simple_hole_construction_group"), format_args!("{}", group_use.simple_hole_construction_group))?;
            } else if group.is_some_and(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.operation_label == label.id)
            }) {
                insert_source_property(ctx, &mut source_properties, format_args!("hole_package_construction_group_use"), format_args!("{}", group_use.id))?;
                insert_source_property(ctx, &mut source_properties, format_args!("hole_package_operation"), format_args!("{}", group_use.operation_label))?;
            }
        }
        for (slot, value) in label.objects.0.iter().enumerate() {
            if let Some(value) = value { insert_source_property(ctx, &mut source_properties, format_args!("object_index.{slot}"), format_args!("{}", value.value()))?; } else { insert_source_property(ctx, &mut source_properties, format_args!("object_index.{slot}"), format_args!("null"))?; };
        }
        for input in input_blocks_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("input_block_record.{}", input.input_slot), format_args!("{}", input.id))?;
            insert_source_property(ctx, &mut source_properties, format_args!("input_block.{}", input.input_slot), format_args!("{}", input.data_block))?;
            if let Some(group) = input_block_identity_group_by_input.get(input.id.as_str()) {
                insert_source_property(ctx, &mut source_properties, format_args!("input_block_identity_group.{}", input.input_slot), format_args!("{}", (*group)))?;
            }
        }
        for (ordinal, use_) in input_column_row_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("input_column_row_use.{ordinal}"), format_args!("{}", use_.id))?;
        }
        for (ordinal, target) in input_column_targets_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("input_column_target.{ordinal}"), format_args!("{}", target.id))?;
        }
        for reference in sketch_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_reference_record.{}", reference.position.ordinal()), format_args!("{}", reference.id))?;
            insert_source_property_reference(ctx, &mut source_properties, format_args!("sketch_reference.{}", reference.position.ordinal()), reference.data_block.as_deref(), reference.token.value())?;
        }
        for reference in projected_curve_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("projected_curve_reference_record.{}", reference.ordinal), format_args!("{}", reference.id))?;
            insert_source_property_reference(ctx, &mut source_properties, format_args!("projected_curve_reference.{}", reference.ordinal), reference.data_block.as_deref(), reference.token.value())?;
        }
        for payload in projected_curve_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("projected_curve_construction_payload"), format_args!("{}", payload.id))?;
        }
        for value in projected_curve_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("projected_curve_construction_string.{}", value.ordinal), format_args!("{}", value.id))?;
        }
        for graph in fset_reference_graphs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("fset_reference_graph"), format_args!("{}", graph.id))?;
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
            insert_source_property(ctx, &mut source_properties, format_args!("fset_construction_payload.{group}"), format_args!("{}", payload.id))?;
        }
        for field in delete_reference_fields_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("delete_reference_field"), format_args!("{}", field.id))?;
        }
        for payload in delete_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("delete_construction_payload"), format_args!("{}", payload.id))?;
        }
        for reference in pattern_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("pattern_reference_record.{}", reference.ordinal), format_args!("{}", reference.id))?;
            insert_source_property_reference(ctx, &mut source_properties, format_args!("pattern_reference.{}", reference.ordinal), reference.data_block.as_deref(), reference.token.value())?;
        }
        for payload in pattern_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("pattern_construction_payload"), format_args!("{}", payload.id))?;
        }
        for lane in pattern_counted_reference_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("pattern_counted_reference_lane"), format_args!("{}", lane.id))?;
        }
        for value in pattern_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("pattern_construction_string.{}", value.ordinal), format_args!("{}", value.id))?;
        }
        for lane in pattern_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("pattern_construction_fixed_lane.{}", lane.ordinal), format_args!("{}", lane.id))?;
        }
        for lane in pattern_transform_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("pattern_transform_lane"), format_args!("{}", lane.id))?;
        }
        for lane in multi_instance_output_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("multi_instance_output_lane"), format_args!("{}", lane.id))?;
        }
        for lane in identical_instance_output_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("identical_instance_output_lane"), format_args!("{}", lane.id))?;
        }
        if let Some(header) = point_construction_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("point_construction_header"), format_args!("{}", header.id))?;
            insert_source_property_reference(ctx, &mut source_properties, format_args!("point_construction_reference"), header.data_block.as_deref(), header.token.value())?;
            insert_source_property(ctx, &mut source_properties, format_args!("point_construction_mode"), format_args!("{:02x}", u8::from(header.mode)))?;
        }
        if let Some(lane) = point_construction_scalar_lanes_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("point_construction_scalar_lane"), format_args!("{}", lane.id))?;
        }
        for reference in draft_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_reference_record.{}", reference.ordinal), format_args!("{}", reference.id))?;
            insert_source_property_reference(ctx, &mut source_properties, format_args!("draft_construction_reference.{}", reference.ordinal), reference.data_block.as_deref(), reference.token.value())?;
        }
        for lane in draft_construction_index_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_index_lane"), format_args!("{}", lane.id))?;
        }
        for payload in draft_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_payload"), format_args!("{}", payload.id))?;
        }
        for payload in draft_construction_graph_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_graph_payload"), format_args!("{}", payload.id))?;
        }
        for lane in draft_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_fixed_lane.{}", lane.ordinal), format_args!("{}", lane.id))?;
        }
        for lane in draft_construction_binary32_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_binary32_lane.{}", lane.ordinal), format_args!("{}", lane.id))?;
        }
        for value in draft_construction_graph_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_graph_string.{}", value.ordinal), format_args!("{}", value.id))?;
        }
        for frame in draft_construction_identity_frames_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_identity_frame.{}", frame.ordinal), format_args!("{}", frame.id))?;
        }
        for lane in draft_construction_terminal_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("draft_construction_terminal_lane"), format_args!("{}", lane.id))?;
        }
        for reference in surface_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!(
                    "surface_construction_reference_record.{}",
                    reference.ordinal
                ), format_args!("{}", reference.id))?;
            insert_source_property_reference(ctx, &mut source_properties, format_args!("surface_construction_reference.{}", reference.ordinal), reference.data_block.as_deref(), reference.token.value())?;
        }
        for payload in surface_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("surface_construction_payload"), format_args!("{}", payload.id))?;
        }
        for pair in surface_construction_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("surface_construction_scalar_pair.{}", pair.ordinal), format_args!("{}", pair.id))?;
        }
        for value in surface_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("surface_construction_string.{}", value.ordinal), format_args!("{}", value.id))?;
        }
        for branch in surface_construction_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            for (ordinal, (token, data_block)) in
                branch.references.members().as_slice().iter().enumerate()
            {
                insert_source_property_reference(ctx, &mut source_properties, format_args!(
                        "surface_construction_branch.{}.member.{}",
                        branch.ordinal(),
                        ordinal
                    ), data_block.as_deref(), token.value())?;
            }
            let (token, data_block) = branch.references.terminal();
            insert_source_property_reference(ctx, &mut source_properties, format_args!("surface_construction_branch.{}.terminal", branch.ordinal()), data_block.as_deref(), token.value())?;
        }
        for (ordinal, block_use) in sketch_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_named_point_block_use.{ordinal}"), format_args!("{}", block_use.id))?;
        }
        for (ordinal, point_use) in sketch_preceding_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_preceding_named_point_use.{ordinal}"), format_args!("{}", point_use.id))?;
        }
        for (ordinal, point_use) in sketch_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_point_use.{ordinal}"), format_args!("{}", point_use.id))?;
        }
        for (ordinal, group) in sketch_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("sketch_point_group.{ordinal}"), format_args!("{}", group.id))?;
        }
        for reference in extrude_profile_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("extrude_profile_reference_record.{}", reference.ordinal), format_args!("{}", reference.id))?;
            insert_source_property_reference(ctx, &mut source_properties, format_args!("extrude_profile_reference.{}", reference.ordinal), reference.data_block.as_deref(), reference.token.value())?;
        }
        if let Some(profile) = extrude_construction_profiles_by_operation.get(label.id.as_str()) {
            insert_source_property(ctx, &mut source_properties, format_args!("extrude_construction_profile"), format_args!("{}", profile.id))?;
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property_reference(ctx, &mut source_properties, format_args!("operation_body_operand.{}.{}", operand.body_reference_ordinal, operand.ordinal), operand.operand_data_block.as_deref(), operand.operand.atom.value())?;
            insert_source_property(ctx, &mut source_properties, format_args!(
                    "operation_body_operand_record.{}.{}",
                    operand.body_reference_ordinal, operand.ordinal
                ), format_args!("{}", operand.id))?;
            for (binding_ordinal, binding) in operand.segment_body_bindings.iter().enumerate() {
                insert_source_property(ctx, &mut source_properties, format_args!(
                        "operation_body_operand_segment_binding.{}.{}.{}",
                        operand.body_reference_ordinal, operand.ordinal, binding_ordinal
                    ), format_args!("{}", binding))?;
            }
        }
        for binding in parameter_bindings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(ctx, &mut source_properties, format_args!(
                    "input_parameter_declaration.{}.{}",
                    binding.input_slot, binding.reference_ordinal
                ), format_args!("{}", binding.expression_declaration))?;
            if let Some(expression) = &binding.expression {
                insert_source_property(ctx, &mut source_properties, format_args!(
                        "input_parameter_expression.{}.{}",
                        binding.input_slot, binding.reference_ordinal
                    ), format_args!("{}", expression))?;
            }
        }
        for (ordinal, parameter_use) in parameter_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(ctx, &mut source_properties, format_args!("parameter_use.{ordinal}"), format_args!("{}", parameter_use.id))?;
        }
        let operation_payload_string_records = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let payload_slots = operation_payload_string_records.len()
            .checked_mul(std::mem::size_of::<&str>())
            .ok_or_else(|| ctx.refuse_codec_limit("NX operation payload string references", 0, cadmpeg_core::decode::u64_from_index(operation_payload_string_records.len())))?;
        ctx.charge_collection_items(cadmpeg_core::decode::u64_from_index(operation_payload_string_records.len()), "NX operation payload string references")?;
        let _payload_reservation = ctx.reserve_scoped(cadmpeg_core::decode::u64_from_index(payload_slots), "NX operation payload string references")?;
        let mut operation_payload_strings = Vec::new();
        operation_payload_strings.try_reserve(operation_payload_string_records.len())
            .map_err(|_| ctx.refuse_codec_limit("allocate NX operation payload string references", 0, cadmpeg_core::decode::u64_from_index(operation_payload_string_records.len())))?;
        operation_payload_strings.extend(operation_payload_string_records.iter().map(|value| value.value.as_str()));
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
                ctx.charge_collection_items(1, "NX block output bodies")?;
                ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<BodyId>() + body.as_str().len()), "NX block output body")?;
                outputs.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("allocate NX block output bodies", 0, 1))?;
                outputs.push(body.clone());
            }
        }
        let sphere_projection = if label.value == "SPHERE" {
            sphere_body_projection(ctx, ir, &outputs)?
        } else {
            None
        };
        let sphere_outputs = if outputs.is_empty() {
            sphere_projection.as_ref().map_or([].as_slice(), |(body, _, _)| std::slice::from_ref(body))
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
        if sphere_op == BooleanOp::NewBody {
            if outputs.is_empty() {
                if let Some((body, _, _)) = &sphere_projection {
                    ctx.charge_collection_items(1, "NX sphere output bodies")?;
                    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<BodyId>() + body.as_str().len()), "NX sphere output body")?;
                    outputs.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("allocate NX sphere output bodies", 0, 1))?;
                    outputs.push(body.clone());
                }
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
                    .map(|primary| {
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
                        .map(Ok)
                    })
            })
            .flatten()
            .transpose()?;
        let offset_projection = (label.value == "OFFSET")
            .then(|| offset_surface_feature_definition(ir, &outputs))
            .flatten();
        if let Some((_, supports)) = &offset_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                insert_source_property(ctx, &mut source_properties, format_args!("offset_support_surface.{support_ordinal}"), format_args!("{}", support.as_str()))?;
            }
        }
        let thicken_projection = (label.value == "THICKEN_SHEET")
            .then(|| thicken_feature_definition(ir, &outputs))
            .flatten();
        if let Some((_, supports)) = &thicken_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                insert_source_property(ctx, &mut source_properties, format_args!("thicken_support_surface.{support_ordinal}"), format_args!("{}", support.as_str()))?;
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
                insert_source_property(ctx, &mut source_properties, format_args!("blend_result_surface.{surface_ordinal}"), format_args!("{}", surface.as_str()))?;
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
                let field = body_references
                    .get(label.id.as_str())
                    .copied()
                    .map(DeleteBodyField::Native)
                    .or_else(|| {
                        offset_store_bodies_by_operation
                            .get(label.id.as_str())
                            .and_then(|uses| match uses.as_slice() {
                                [(object_index, data_block)] => {
                                    Some(DeleteBodyField::OffsetStore {
                                        object_index: *object_index,
                                        data_block,
                                    })
                                }
                                _ => None,
                            })
                    })?;
                Some(delete_body_feature_definition(
                    field,
                    &body_alias_roots,
                    &bodies_by_object_index,
                ))
            })
            .flatten();
        let extract_body_projection = (label.value == "EXTRACT_BODY").then(|| {
            extract_body_feature_definition(
                body_references.get(label.id.as_str()).copied(),
                offset_store_bodies_by_operation
                    .get(label.id.as_str())
                    .map_or([].as_slice(), Vec::as_slice),
                &body_alias_roots,
                &bodies_by_object_index,
            )
        });
        let operation_parameter_uses = parameter_uses_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let native_parameters = native_feature_parameters(ctx, operation_parameter_uses, expressions)?;
        let sketch = (label.value == "SKETCH")
            .then(|| {
                attach_sketch_graph(
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
                )
            })
            .flatten();
        let definition = if let Some(definition) = boolean_definition
            .or(trim_body_projection)
            .or(delete_projection)
            .or(extract_body_projection)
            .or(sew_projection)
            .or(extrude_projection)
            .or_else(|| blend_projection.map(|(definition, _)| definition))
            .or_else(|| thicken_projection.map(|(definition, _)| definition))
            .or_else(|| offset_projection.map(|(definition, _)| definition))
            .or(sphere_definition)
            .or_else(|| {
                (label.value == "BREP")
                    .then(|| brep_feature_definition(&outputs))
                    .flatten()
            }) {
            definition
        } else if let Some(sketch) = sketch {
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            })
        } else {
            let mut definition = if let Some(definition) = non_modeling_history_definition(
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
            )
            .or_else(|| {
                body_writing_unresolved_feature_definition(&label.value, &source_properties)
            }) {
                definition
            } else {
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
                            .chain(counterbore_hole_placements.get(label.id.as_str()).cloned())
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
                    cadmpeg_core::text::named_entries(&label.id, native_parameters)?,
                )?
            };
            if let FeatureDefinition::Operation(FeatureOperation::Block { op, .. }) =
                &mut definition
            {
                *op = block_op;
            }
            definition
        };
        annotations
            .note(&id, &stream, label.source_offset)
            .tag("FEATURE_OPERATION");
        annotations.exactness(&id, Exactness::Derived);
        let source_content = feature_source_content(ctx, operation_payload_string_records)?;
        let mut referenced_parameters = Vec::new();
        let mut parameter_reservation = ctx.reserve_scoped(0, "NX referenced parameters")?;
        for parameter_use in operation_parameter_uses {
            push_referenced_parameter(ctx, &mut parameter_reservation, &mut referenced_parameters, &parameter_use.expression)?;
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            for dimension in &dimensions.dimensions {
                push_referenced_parameter(ctx, &mut parameter_reservation, &mut referenced_parameters, &dimension.expression)?;
            }
        }
        for owner in parameter_owner_dependencies(ctx, &parameter_owners, &referenced_parameters)? {
            push_unique_feature_dependency(ctx, &mut dependencies, &owner)?;
        }
        if !source_content.is_empty() {
            annotations
                .derived(&id, "source_content")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        let native_output = (!deletes_body).then_some(native_primary_body).flatten();
        let offset_store_output = (!deletes_body)
            .then_some(offset_store_primary_body)
            .flatten();
        body_writer_history.record_writer(ctx, native_output, offset_store_output, &outputs, &id)?;
        for write in operation_body_writes {
            body_identity_writers.insert(write.frame.body_identity(), id.clone());
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
                body_writer_history.record_writer(ctx, native_target, offset_store_target, &[], &id)?;
            }
        }
        let dependency_check_work = dependencies.len().checked_mul(dependencies.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX feature dependency validation", 0, cadmpeg_core::decode::u64_from_index(dependencies.len())))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(dependency_check_work), "NX feature dependency validation")?;
        ir.model.features.push(Feature {
            id: id.clone(),
            ordinal: base_ordinal + ordinal as u64,
            name: Some(label.value.clone()),
            suppressed: None,
            dependencies: DistinctMembers::try_from_unique_vec(dependencies)
                .map_err(CodecError::malformed)?,
            source_properties: cadmpeg_core::text::named_entries(&label.id, source_properties)?,
            source_tag: Some(label.value.clone()),
            source_text: None,
            source_content,

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                definition,
                outputs.try_into().map_err(CodecError::malformed)?,
            ),
            native_ref: Some(label.id.clone()),
        });
        if !deletes_body && !operation_body_writes.is_empty() {
            let key = label
                .id
                .strip_prefix("nx:feature-history:operation-label#")
                .unwrap_or(label.id.as_str());
            for write in operation_body_writes {
                let result_members = operation_body_write_result_group_members(
                    ctx,
                    write.id.as_str(),
                    operation_body_partition_uses,
                    body_write_group_partition_uses,
                    parasolid_group_members,
                )?;
                ir.model.feature_result_topologies.push(
                    FeatureResultTopology::new(
                        IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
                            .try_id::<FeatureResultTopologyId>(
                                &cadmpeg_ir::identity_component!("result-topology"),
                                format!("{key}-{:010}", write.ordinal),
                            )
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "NX operation label key is not identity key text"
                                ))
                            })?,
                        id.clone(),
                        vec![cadmpeg_core::nonblank_literal!(
                            "nx:feature-history:body-identity#{:010}",
                            write.frame.body_identity()
                        )],
                        result_members.faces,
                        result_members.edges,
                        result_members.vertices,
                        Some(write.id.clone()),
                    )
                    .map_err(|error| CodecError::Malformed(error.to_string()))?,
                );
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
                ir.model.feature_result_topologies.push(
                    FeatureResultTopology::new(
                        IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
                            .try_id::<FeatureResultTopologyId>(
                                &cadmpeg_ir::identity_component!("result-topology"),
                                key,
                            )
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "NX operation label key is not identity key text"
                                ))
                            })?,
                        id.clone(),
                        vec![local_id],
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        Some(native_ref),
                    )
                    .map_err(|error| CodecError::Malformed(error.to_string()))?,
                );
            }
        }
    }
    // Only namespace-admitted primary-body relations can open the state-only
    // witness. Unique fields rejected by native_primary_body_references remain
    // native evidence without becoming current-body state roots.
    if !body_references.is_empty() {
        if let Some(initial_body_id) = initial_body_id.as_ref() {
            if let Some(initial_feature) = ir
                .model
                .features
                .iter_mut()
                .find(|feature| feature.id == *initial_body_id)
            {
                initial_feature.source_properties.insert(
                    cadmpeg_core::nonblank_const!(NATIVE_PRIMARY_BODY_CLOSURE_WITNESS),
                    "primary-body-relations".to_string(),
                );
                annotations
                    .derived(initial_body_id, NATIVE_PRIMARY_BODY_CLOSURE_WITNESS)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
        }
    }
    if let Some(initial_body_id) = initial_body_id {
        let has_outputs = ir
            .model
            .features
            .iter()
            .find(|feature| feature.id == initial_body_id)
            .is_some_and(|feature| !feature.evaluation.outputs().is_empty());
        if has_outputs {
            annotations
                .derived(&initial_body_id, "outputs")
                .map_err(cadmpeg_core::CodecError::malformed)?;
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
    let image_members = image_use.map(|use_| {
        feature_result_group_members(
            ctx,
            use_.partition_stream_ordinal,
            &use_.parasolid_group_members,
            members,
        )
    }).transpose()?;
    let mut matching_group_uses = group_partition_uses
        .iter()
        .filter(|use_| use_.body_write == body_write);
    let group_use = matching_group_uses.next();
    if matching_group_uses.next().is_some() {
        return Ok(FeatureResultGroupMembers::default());
    }
    let group_members = group_use.map(|use_| {
        feature_result_group_members(
            ctx,
            use_.partition_stream_ordinal,
            &use_.parasolid_group_members,
            members,
        )
    }).transpose()?;
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
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(members.len()), "NX feature result group member lookup")?;
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
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(std::mem::size_of::<cadmpeg_core::text::NonBlankString>() + identity_len),
            "NX feature result group member identity",
        )?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(identity_len), "NX feature result group member identity formatting")?;
        output.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("allocate NX feature result group members", 0, 1))?;
        let mut text = String::new();
        text.try_reserve(identity_len).map_err(|_| ctx.refuse_codec_limit("allocate NX feature result group member identity", 0, cadmpeg_core::decode::u64_from_index(identity_len)))?;
        std::fmt::Write::write_fmt(&mut text, format_args!("nx:s{partition_stream_ordinal}:{kind}#{xmt}"))
            .map_err(|_| CodecError::InvalidInput("NX result member identity formatting failed".to_string()))?;
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
    let local_len = native.len().checked_add(suffix.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX result body local identity", 0, cadmpeg_core::decode::u64_from_index(native.len())))?;
    let retained_bytes = std::mem::size_of::<(cadmpeg_core::text::NonBlankString, String)>()
        .checked_add(local_len)
        .and_then(|bytes| bytes.checked_add(native.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("NX result body identity", 0, cadmpeg_core::decode::u64_from_index(local_len)))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(local_len), "NX result body identity formatting")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(retained_bytes), "NX result body identity")?;
    let mut local = String::new();
    local.try_reserve(local_len).map_err(|_| ctx.refuse_codec_limit("allocate NX result body local identity", 0, cadmpeg_core::decode::u64_from_index(local_len)))?;
    local.push_str(native);
    local.push_str(suffix);
    let Some(local) = cadmpeg_core::text::NonBlankString::new(local) else {
        return Ok(None);
    };
    let mut native_ref = String::new();
    native_ref.try_reserve(native.len()).map_err(|_| ctx.refuse_codec_limit("allocate NX result body native identity", 0, cadmpeg_core::decode::u64_from_index(native.len())))?;
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
    let unique_references = crate::native::features::unique_feature_body_references(ctx, references)?;
    let mut offset_reservation = ctx.reserve_scoped(0, "NX primary offset-store references")?;
    let mut offset_store_references = BTreeSet::new();
    for use_ in data_block_uses {
        ctx.charge_collection_items(1, "NX primary offset-store references")?;
        offset_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<&str>() * 4,
        ))?;
        offset_store_references.insert(use_.feature_body_reference.as_str());
    }
    let offset_store_operations =
        crate::native::features::feature_input_store_operations(ctx, inputs, data_blocks)?;
    let mut bridge_reservation = ctx.reserve_scoped(0, "NX primary bridged references")?;
    let mut bridged_segment_references = BTreeSet::new();
    for use_ in segment_uses {
        ctx.charge_collection_items(1, "NX primary bridged references")?;
        bridge_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<&str>() * 4,
        ))?;
        bridged_segment_references.insert(use_.feature_body_reference.as_str());
    }
    let mut output = BTreeMap::new();
    for (operation, reference) in unique_references {
        if !bridged_segment_references.contains(reference.id.as_str())
            && (offset_store_references.contains(reference.id.as_str())
                || offset_store_operations.contains(reference.operation_label.as_str())) {
            continue;
        }
        ctx.charge_collection_items(1, "NX primary body references")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<(&str, u32)>() * 4,
        ), "NX primary body references")?;
        output.insert(operation, reference.body.value());
    }
    Ok(output)
}

fn attach_sketch_graph(
    ir: &mut CadIr,
    label: &crate::native::features::FeatureOperationLabel,
    sources: &SketchSources<'_>,
    annotations: &mut AnnotationBuilder,
    stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Option<SketchId> {
    let operation_groups = sources
        .point_groups
        .iter()
        .filter(|group| group.operation_label == label.id)
        .collect::<Vec<_>>();
    let operation_key = label
        .id
        .strip_prefix("nx:feature-history:operation-label#")
        .unwrap_or(label.id.as_str());
    let sketch_id: SketchId = IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
        .try_id(&cadmpeg_ir::identity_component!("sketch"), operation_key)?;
    let operation_fixed_points = sources
        .fixed_points
        .iter()
        .copied()
        .filter(|point| point.operation_label == label.id)
        .collect::<Vec<_>>();
    if operation_groups.is_empty() {
        let coordinate_pairs = sources
            .coordinate_pairs
            .iter()
            .filter(|pair| pair.operation_label == label.id)
            .copied()
            .collect::<Vec<_>>();
        if coordinate_pairs.is_empty() && operation_fixed_points.is_empty() {
            return None;
        }
        let mut entities =
            Vec::with_capacity(coordinate_pairs.len() + operation_fixed_points.len());
        let mut pair_ids = BTreeSet::new();
        let mut pair_entity_keys = BTreeSet::new();
        let mut pair_ordinals = BTreeSet::new();
        for pair in coordinate_pairs {
            if !pair_ids.insert(pair.id.as_str())
                || !pair_ordinals.insert((pair.payload.id(), pair.ordinal))
            {
                return None;
            }
            let pair_key = pair
                .id
                .rsplit_once('#')
                .map_or(pair.id.as_str(), |(_, key)| key);
            if pair_key.is_empty()
                || pair_key.chars().any(char::is_whitespace)
                || !pair_entity_keys.insert(pair_key)
            {
                return None;
            }
            entities.push((
                pair.source_offset,
                SketchEntity::new(
                    IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
                        .try_id::<SketchEntityId>(
                        &cadmpeg_ir::identity_component!("sketch-entity"),
                        format!("coordinate-pair-{pair_key}"),
                    )?,
                    sketch_id.clone(),
                    SketchGeometry::native(cadmpeg_core::text::NonBlankString::new(
                        "nx-coordinate-pair",
                    )?),
                )
                .with_native_ref(Some(pair.id.clone())),
            ));
        }
        entities.extend(native_fixed_point_entities(
            label,
            &sketch_id,
            &operation_fixed_points,
        )?);
        entities.sort_by(|(first_offset, first), (second_offset, second)| {
            first_offset
                .cmp(second_offset)
                .then_with(|| first.id().cmp(second.id()))
        });
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
            annotations
                .note(entity.id().as_str(), stream, *source_offset)
                .tag(tag);
            annotations.exactness(entity.id().as_str(), Exactness::ByteExact);
        }
        annotations
            .note(sketch_id.as_str(), stream, label.source_offset)
            .tag("SKETCH");
        annotations.exactness(sketch_id.as_str(), Exactness::Derived);
        ir.model
            .sketch_entities
            .extend(entities.into_iter().map(|(_, entity)| entity));
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: Some(label.value.clone()),
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: Some(label.id.clone()),
        });
        return Some(sketch_id);
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
    let mut scalars_by_id = BTreeMap::<&str, &crate::native::features::FeaturePayloadScalar>::new();
    for scalar in sources.payload_scalars {
        if scalars_by_id.insert(scalar.id.as_str(), scalar).is_some() {
            return None;
        }
    }
    let mut entities = Vec::new();
    let mut represented_groups = BTreeSet::new();
    for group in operation_groups {
        if !represented_groups.insert(group.id.as_str()) {
            return None;
        }
        let point_use = point_uses_by_group.get(group.id.as_str()).copied();
        let source_offsets: Vec<_> = if let Some(point_use) = point_use {
            point_use
                .references
                .iter()
                .map(|reference| reference.source_offset)
                .collect()
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
                                    || scalar.scalar.value().get().to_bits() != coordinate.to_bits()
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
            SketchEntity::new(
                IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
                    .try_id::<SketchEntityId>(
                    &cadmpeg_ir::identity_component!("sketch-entity"),
                    format!("point-{entity_key}"),
                )?,
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(group.coordinates[0], group.coordinates[1]),
                })
                .ok()?,
            )
            .with_native_ref(Some(native_ref)),
        ));
    }
    entities.extend(native_fixed_point_entities(
        label,
        &sketch_id,
        &operation_fixed_points,
    )?);
    entities.sort_by(|(first_offset, first), (second_offset, second)| {
        first_offset
            .cmp(second_offset)
            .then_with(|| first.id().cmp(second.id()))
    });
    if entities.is_empty() {
        return None;
    }
    for (source_offset, entity) in &entities {
        match entity.geometry.definition() {
            SketchGeometryDefinition::Point { .. } => {
                annotations
                    .note(entity.id().as_str(), stream, *source_offset)
                    .tag("SKETCH_POINT");
                annotations.exactness(entity.id().as_str(), Exactness::Derived);
            }
            SketchGeometryDefinition::Native { native_kind } => {
                let tag = if native_kind == "nx-fixed-point" {
                    "SKETCH_NATIVE_FIXED_POINT"
                } else {
                    "SKETCH_NATIVE"
                };
                annotations
                    .note(entity.id().as_str(), stream, *source_offset)
                    .tag(tag);
                annotations.exactness(entity.id().as_str(), Exactness::ByteExact);
            }
            _ => return None,
        }
    }
    annotations
        .note(sketch_id.as_str(), stream, label.source_offset)
        .tag("SKETCH");
    annotations.exactness(sketch_id.as_str(), Exactness::Derived);
    ir.model
        .sketch_entities
        .extend(entities.into_iter().map(|(_, entity)| entity));
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some(label.value.clone()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(label.id.clone()),
    });
    Some(sketch_id)
}

struct SketchSources<'a> {
    point_uses: &'a [&'a crate::native::features::FeatureSketchPointUse],
    point_groups: &'a [crate::native::features::FeatureSketchPointGroup],
    points: &'a [crate::native::features::FeatureSketchPoint],
    payload_scalars: &'a [crate::native::features::FeaturePayloadScalar],
    fixed_points: &'a [&'a crate::native::features::FeatureSketchFixedPoint],
    coordinate_pairs: &'a [&'a crate::native::features::FeaturePayloadScalarPair],
}

fn native_fixed_point_entities(
    label: &crate::native::features::FeatureOperationLabel,
    sketch_id: &SketchId,
    points: &[&crate::native::features::FeatureSketchFixedPoint],
) -> Option<Vec<(u64, SketchEntity)>> {
    let mut point_ids = BTreeSet::new();
    let mut entity_keys = BTreeSet::new();
    let mut entities = Vec::with_capacity(points.len());
    for point in points {
        if point.operation_label != label.id || !point_ids.insert(point.id.as_str()) {
            return None;
        }
        let point_key = point
            .id
            .rsplit_once('#')
            .map_or(point.id.as_str(), |(_, key)| key);
        if point_key.is_empty()
            || point_key.chars().any(char::is_whitespace)
            || !entity_keys.insert(point_key)
        {
            return None;
        }
        entities.push((
            point.source_offset,
            SketchEntity::new(
                IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
                    .try_id::<SketchEntityId>(
                    &cadmpeg_ir::identity_component!("sketch-entity"),
                    format!("fixed-point-{point_key}"),
                )?,
                sketch_id.clone(),
                SketchGeometry::native(cadmpeg_core::text::NonBlankString::new("nx-fixed-point")?),
            )
            .with_native_ref(Some(point.id.clone())),
        ));
    }
    Some(entities)
}

struct OperationRecords<'a, 'ctx, T> {
    grouped: BTreeMap<&'a str, Vec<&'a T>>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

struct ScopedIndex<'ctx, K, V> {
    entries: BTreeMap<K, V>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<K: Ord, V> std::ops::Deref for ScopedIndex<'_, K, V> {
    type Target = BTreeMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

fn last_record_index<'ctx, K: Ord + Copy, V: Copy>(
    ctx: &'ctx DecodeContext<'_>,
    records: impl IntoIterator<Item = (K, V)>,
) -> Result<ScopedIndex<'ctx, K, V>, CodecError> {
    let mut entries = BTreeMap::new();
    let mut reservation = ctx.reserve_scoped(0, "NX last-record index")?;
    for (key, value) in records {
        ctx.charge_work(1, "NX last-record index")?;
        if !entries.contains_key(&key) {
            ctx.charge_collection_items(1, "NX last-record index")?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(K, V)>()))?;
        }
        entries.insert(key, value);
    }
    Ok(ScopedIndex { entries, _reservation: reservation })
}

fn segment_binding_body_indexes<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &CadIr,
    bindings: &'a [crate::native::segments::SegmentBodyBinding],
) -> Result<(
    BTreeMap<u32, Vec<BodyId>>,
    BTreeMap<&'a str, Vec<BodyId>>,
    cadmpeg_core::decode::ScopedReservation<'ctx>,
), CodecError> {
    let mut by_object = BTreeMap::<u32, Vec<BodyId>>::new();
    let mut by_binding = BTreeMap::<&str, Vec<BodyId>>::new();
    let mut reservation = ctx.reserve_scoped(0, "NX segment binding body indexes")?;
    for binding in bindings {
        let (prefix, prefix_len) = stream_prefix(binding.stream_ordinal, false);
        let mut stream_bodies = Vec::new();
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()), "NX segment body prefix scan")?;
        for body in ir.model.bodies.iter().filter(|body| body.id.as_str().as_bytes().starts_with(&prefix[..prefix_len])) {
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(stream_bodies.len()), "NX segment body uniqueness")?;
            if stream_bodies.contains(&body.id) {
                continue;
            }
            let bytes = std::mem::size_of::<BodyId>().checked_add(body.id.as_str().len())
                .ok_or_else(|| ctx.refuse_codec_limit("NX segment body identity", 0, cadmpeg_core::decode::u64_from_index(body.id.as_str().len())))?;
            ctx.charge_collection_items(1, "NX segment body identity")?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
            reserve_attach_vec(ctx, &mut stream_bodies, 1, "NX segment body identity")?;
            stream_bodies.push(body.id.clone());
        }
        for identity in [binding.body_object_index, binding.body_alias_object_index] {
            for body in &stream_bodies {
                let existing = by_object.get(&identity).map_or(0, Vec::len);
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(existing), "NX segment body alias uniqueness")?;
                if by_object.get(&identity).is_some_and(|bodies| bodies.contains(body)) {
                    continue;
                }
                push_grouped_operation(ctx, &mut reservation, &mut by_object, identity, || body.clone(), body.as_str().len())?;
            }
        }
        ctx.charge_work(1, "NX segment binding identity index")?;
        if !by_binding.contains_key(binding.id.as_str()) {
            ctx.charge_collection_items(1, "NX segment binding identity index")?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, Vec<BodyId>)>()))?;
        }
        by_binding.insert(binding.id.as_str(), stream_bodies);
    }
    Ok((by_object, by_binding, reservation))
}

fn stream_prefix(ordinal: u32, body_marker: bool) -> ([u8; 20], usize) {
    let mut decimal = [0u8; 10];
    let mut digit_count = 0;
    let mut ordinal = ordinal;
    loop {
        decimal[digit_count] = b'0' + (ordinal % 10) as u8;
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
    let suffix = if body_marker { b":body#".as_slice() } else { b":".as_slice() };
    let suffix_start = 4 + digit_count;
    let prefix_len = suffix_start + suffix.len();
    prefix[suffix_start..prefix_len].copy_from_slice(suffix);
    (prefix, prefix_len)
}

fn insert_scoped_body_output<K: Ord + Copy>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    outputs: &mut BTreeMap<K, BodyId>,
    key: K,
    body: &BodyId,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_work(1, operation)?;
    if !outputs.contains_key(&key) {
        ctx.charge_collection_items(1, operation)?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(K, BodyId)>()))?;
    }
    reservation.grow(cadmpeg_core::decode::u64_from_index(body.as_str().len()))?;
    outputs.insert(key, body.clone());
    Ok(())
}

fn push_grouped_operation<K: Ord, V>(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    grouped: &mut BTreeMap<K, Vec<V>>,
    key: K,
    value: impl FnOnce() -> V,
    owned_bytes: usize,
) -> Result<(), CodecError> {
    ctx.charge_work(1, "NX feature operation group index")?;
    if !grouped.contains_key(&key) {
        ctx.charge_collection_items(1, "NX feature operation group key")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(K, Vec<V>)>()))?;
    }
    let bytes = std::mem::size_of::<V>().checked_add(owned_bytes)
        .ok_or_else(|| ctx.refuse_codec_limit("NX feature operation group member", 0, cadmpeg_core::decode::u64_from_index(owned_bytes)))?;
    ctx.charge_collection_items(1, "NX feature operation group member")?;
    reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
    let members = grouped.entry(key).or_default();
    reserve_attach_vec(ctx, members, 1, "NX feature operation group member")?;
    members.push(value());
    Ok(())
}

impl<'a, T> std::ops::Deref for OperationRecords<'a, '_, T> {
    type Target = BTreeMap<&'a str, Vec<&'a T>>;

    fn deref(&self) -> &Self::Target {
        &self.grouped
    }
}

fn records_by_operation<'a, 'ctx, T>(
    ctx: &'ctx DecodeContext<'_>,
    records: &'a [T],
    operation_label: impl Fn(&'a T) -> &'a str,
) -> Result<OperationRecords<'a, 'ctx, T>, CodecError> {
    let mut grouped = BTreeMap::new();
    let mut reservation = ctx.reserve_scoped(0, "NX operation record index")?;
    for record in records {
        ctx.charge_work(1, "NX operation record index")?;
        let label = operation_label(record);
        if !grouped.contains_key(label) {
            ctx.charge_collection_items(1, "NX operation record index keys")?;
            reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, Vec<&T>)>()))?;
        }
        ctx.charge_collection_items(1, "NX operation record index members")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<&T>()))?;
        let members = grouped.entry(label).or_default();
        reserve_attach_vec(ctx, members, 1, "NX operation record index members")?;
        members.push(record);
    }
    Ok(OperationRecords { grouped, _reservation: reservation })
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
        .all(|(ordinal, frame)| u64::from(frame.ordinal) == cadmpeg_core::decode::u64_from_index(ordinal));
    if contiguous_common_frames
    {
        for frame in common_frames.iter().filter(|frame| frame.operation_record == record.id) {
            const PREFIX: &str = "operation_common_frame.";
            let key_len = PREFIX.len().checked_add(10).ok_or_else(|| ctx.refuse_codec_limit("NX operation source property key", 0, 10))?;
            let retained_bytes = std::mem::size_of::<(String, String)>()
                .checked_add(key_len)
                .and_then(|bytes| bytes.checked_add(frame.id.len()))
                .ok_or_else(|| ctx.refuse_codec_limit("NX operation source property", 0, cadmpeg_core::decode::u64_from_index(frame.id.len())))?;
            ctx.charge_collection_items(1, "NX operation source properties")?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(retained_bytes), "NX operation source property")?;
            let mut key = String::new();
            key.try_reserve(key_len).map_err(|_| ctx.refuse_codec_limit("allocate NX operation source property key", 0, cadmpeg_core::decode::u64_from_index(key_len)))?;
            std::fmt::Write::write_fmt(&mut key, format_args!("{PREFIX}{}", frame.ordinal))
                .map_err(|_| CodecError::InvalidInput("NX operation source property key formatting failed".to_string()))?;
            let mut value = String::new();
            value.try_reserve(frame.id.len()).map_err(|_| ctx.refuse_codec_limit("allocate NX operation source property value", 0, cadmpeg_core::decode::u64_from_index(frame.id.len())))?;
            value.push_str(&frame.id);
            properties.insert(key, value);
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
    let retained_bytes = std::mem::size_of::<(String, String)>()
        .checked_add(key.len())
        .and_then(|bytes| bytes.checked_add(value.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("NX operation source property", 0, cadmpeg_core::decode::u64_from_index(value.len())))?;
    ctx.charge_collection_items(1, "NX operation source properties")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(retained_bytes), "NX operation source property")?;
    let mut owned_key = String::new();
    owned_key.try_reserve(key.len()).map_err(|_| ctx.refuse_codec_limit("allocate NX operation source property key", 0, cadmpeg_core::decode::u64_from_index(key.len())))?;
    owned_key.push_str(key);
    let mut owned_value = String::new();
    owned_value.try_reserve(value.len()).map_err(|_| ctx.refuse_codec_limit("allocate NX operation source property value", 0, cadmpeg_core::decode::u64_from_index(value.len())))?;
    owned_value.push_str(value);
    properties.insert(owned_key, owned_value);
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
    let text_bytes = key_length.0.checked_add(value_length.0)
        .ok_or_else(|| ctx.refuse_codec_limit("NX source property text", 0, cadmpeg_core::decode::u64_from_index(value_length.0)))?;
    let retained_bytes = std::mem::size_of::<(String, String)>().checked_add(text_bytes)
        .ok_or_else(|| ctx.refuse_codec_limit("NX source property", 0, cadmpeg_core::decode::u64_from_index(text_bytes)))?;
    let work = text_bytes.checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("NX source property formatting", 0, cadmpeg_core::decode::u64_from_index(text_bytes)))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), "NX source property formatting")?;
    ctx.charge_collection_items(1, "NX source properties")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(retained_bytes), "NX source property")?;
    let mut owned_key = String::new();
    owned_key.try_reserve(key_length.0).map_err(|_| ctx.refuse_codec_limit("allocate NX source property key", 0, cadmpeg_core::decode::u64_from_index(key_length.0)))?;
    std::fmt::write(&mut owned_key, key)
        .map_err(|_| CodecError::InvalidInput("NX source property key formatting failed".to_string()))?;
    let mut owned_value = String::new();
    owned_value.try_reserve(value_length.0).map_err(|_| ctx.refuse_codec_limit("allocate NX source property value", 0, cadmpeg_core::decode::u64_from_index(value_length.0)))?;
    std::fmt::write(&mut owned_value, value)
        .map_err(|_| CodecError::InvalidInput("NX source property value formatting failed".to_string()))?;
    properties.insert(owned_key, owned_value);
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
        Some(data_block) => insert_source_property(ctx, properties, key, format_args!("{data_block}")),
        None => insert_source_property(ctx, properties, key, format_args!("{fallback}")),
    }
}

struct ParasolidStringAttributeSources<'a> {
    string_uses: &'a [crate::native::parasolid::ParasolidEntity51StringUse],
    strings: &'a [crate::native::parasolid::ParasolidEntity54StringRecord],
}

fn attach_parasolid_topology_string_attributes(
    ir: &mut CadIr,
    sources: &ParasolidStringAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
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
        uses.sort_by_key(|string_use| string_use.position);
    }
    for context in &attribute_index.contexts {
        let reference = context.reference;
        let entity = context.entity;
        for string_use in uses_by_entity.get(entity).into_iter().flatten() {
            let Some(string) = strings_by_id.get(string_use.string_record.as_str()) else {
                continue;
            };
            let id = topology_attribute_id(
                reference,
                &cadmpeg_ir::identity_component!("topology-string-attribute"),
                string_use.position.reference_ordinal(),
                context.id_suffix.as_ref(),
            );
            let source_stream = StreamHandle::new(
                cadmpeg_ir::stream_name!("nx:s").with_suffix(reference.stream_ordinal),
            );
            annotations
                .note(id.as_str(), &source_stream, string.inflated_offset)
                .tag("ENTITY_54_STRING_ATTRIBUTE");
            annotations
                .derived(id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            annotations
                .derived(id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            let generic_name = format!(
                "parasolid_type_84_reference_{}",
                string_use.position.reference_ordinal()
            );
            let name = attribute_index
                .attribute_names
                .field_name(reference, string_use.id.as_str());
            ir.model.attributes.push(SourceAttribute {
                id,
                target: context.target.clone(),
                name: name
                    .or_else(|| {
                        attribute_index
                            .class_names
                            .get(reference.id.as_str())
                            .map(|class_name| format!("{class_name}.{generic_name}"))
                    })
                    .unwrap_or(generic_name),
                values: vec![AttributeValue::String(string.value.as_str().to_owned())],
            });
        }
    }
    ir.model
        .attributes
        .sort_by(|first, second| first.id.as_str().cmp(second.id.as_str()));
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
        class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
        definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
        field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
        field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    ) -> Self {
        let mut classes_by_entity = BTreeMap::new();
        for class_use in class_uses {
            insert_sole(
                &mut classes_by_entity,
                (
                    class_use.topology_attribute_reference.as_str(),
                    class_use.entity_51_record.as_str(),
                ),
                class_use,
            );
        }

        let mut fields_by_value_use = BTreeMap::new();
        for field_use in field_uses {
            insert_sole(
                &mut fields_by_value_use,
                field_use.value_use.as_str(),
                field_use,
            );
        }

        let mut definitions_by_id = BTreeMap::new();
        for definition in definitions {
            insert_sole(&mut definitions_by_id, definition.id.as_str(), definition);
        }

        let mut field_names_by_definition = BTreeMap::new();
        for names in field_names {
            insert_sole(
                &mut field_names_by_definition,
                names.attribute_definition.as_str(),
                names,
            );
        }

        Self {
            classes_by_entity,
            fields_by_value_use,
            definitions_by_id,
            field_names_by_definition,
        }
    }

    fn field_name(
        &self,
        topology_reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
        value_use: &str,
    ) -> Option<String> {
        let field_use = self.fields_by_value_use.get(value_use)?.as_ref()?;
        let class_use = self
            .classes_by_entity
            .get(&(
                topology_reference.id.as_str(),
                field_use.entity_51_record.as_str(),
            ))?
            .as_ref()?;
        if field_use.attribute_class_use != class_use.attribute_class_use
            || field_use.attribute_definition != class_use.attribute_definition
        {
            return None;
        }
        let definition = self
            .definitions_by_id
            .get(class_use.attribute_definition.as_str())?
            .as_ref()?;
        let field_name = match (definition.name.as_str(), field_use.position.field_ordinal()) {
            ("SDL/TYSA_DENSITY", 0) => "density".to_string(),
            ("SDL/TYSA_DENSITY", 1) => "units".to_string(),
            _ if self
                .field_names_by_definition
                .get(definition.id.as_str())
                .and_then(Option::as_ref)
                .is_some() =>
            {
                self.field_names_by_definition
                    .get(definition.id.as_str())
                    .and_then(Option::as_ref)?
                    .fields
                    .get(field_use.position.field_ordinal() as usize)?
                    .name
                    .clone()
            }
            _ => format!(
                "field_{}.parasolid_type_{}",
                field_use.position.field_ordinal(),
                field_use.value_kind.field_code().code()
            ),
        };
        Some(format!("{}.{}", definition.name.as_str(), field_name))
    }
}

/// Records `value` as the sole value for `key`, or `None` once the key repeats.
fn insert_sole<'a, K: Ord, V>(values: &mut BTreeMap<K, Option<&'a V>>, key: K, value: &'a V) {
    match values.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
        Entry::Occupied(mut entry) => {
            entry.insert(None);
        }
    }
}

fn parasolid_topology_attribute_class_names<'a>(
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
    definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
) -> BTreeMap<&'a str, &'a str> {
    let mut definitions_by_id = BTreeMap::<&str, BTreeSet<&str>>::new();
    for definition in definitions {
        definitions_by_id
            .entry(definition.id.as_str())
            .or_default()
            .insert(definition.name.as_str());
    }
    let mut classes_by_reference = BTreeMap::<&str, BTreeSet<&str>>::new();
    for class_use in class_uses {
        let Some(class_names) = definitions_by_id.get(class_use.attribute_definition.as_str())
        else {
            continue;
        };
        classes_by_reference
            .entry(class_use.topology_attribute_reference.as_str())
            .or_default()
            .extend(class_names.iter().copied());
    }
    classes_by_reference
        .into_iter()
        .filter_map(|(reference, names)| {
            let mut names = names.into_iter();
            let name = names.next()?;
            names.next().is_none().then_some((reference, name))
        })
        .collect()
}

fn parasolid_topology_attribute_targets(ir: &CadIr) -> BTreeMap<String, AttributeTarget> {
    ir.model
        .shells
        .iter()
        .map(|shell| {
            (
                shell.id.as_str().to_owned(),
                AttributeTarget::Shell(shell.id.clone()),
            )
        })
        .chain(ir.model.faces.iter().map(|face| {
            (
                face.id.as_str().to_owned(),
                AttributeTarget::Face(face.id.clone()),
            )
        }))
        .chain(ir.model.loops.iter().map(|loop_| {
            (
                loop_.id.as_str().to_owned(),
                AttributeTarget::Loop(loop_.id.clone()),
            )
        }))
        .chain(ir.model.edges.iter().map(|edge| {
            (
                edge.id.as_str().to_owned(),
                AttributeTarget::Edge(edge.id.clone()),
            )
        }))
        .chain(ir.model.coedges.iter().map(|coedge| {
            (
                coedge.id.as_str().to_owned(),
                AttributeTarget::Coedge(coedge.id.clone()),
            )
        }))
        .chain(ir.model.vertices.iter().map(|vertex| {
            (
                vertex.id.as_str().to_owned(),
                AttributeTarget::Vertex(vertex.id.clone()),
            )
        }))
        .collect()
}

struct ParasolidTopologyAttributeContext<'a> {
    reference: &'a crate::native::parasolid::ParasolidTopologyAttributeListReference,
    entity: &'a str,
    id_suffix: Option<cadmpeg_ir::ids::IdentityKey>,
    target: AttributeTarget,
}

struct ParasolidTopologyAttributeIndex<'a> {
    class_names: BTreeMap<&'a str, &'a str>,
    attribute_names: ParasolidAttributeNameIndex<'a>,
    contexts: Vec<ParasolidTopologyAttributeContext<'a>>,
}

impl<'a> ParasolidTopologyAttributeIndex<'a> {
    fn new(
        ir: &CadIr,
        topology_references:
            &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
        class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
        definitions: &'a [crate::native::parasolid::ParasolidAttributeDefinition],
        field_uses: &'a [crate::native::parasolid::ParasolidAttributeFieldUse],
        field_names: &'a [crate::native::parasolid::ParasolidAttributeFieldNames],
    ) -> Self {
        Self {
            class_names: parasolid_topology_attribute_class_names(class_uses, definitions),
            attribute_names: ParasolidAttributeNameIndex::new(
                class_uses,
                definitions,
                field_uses,
                field_names,
            ),
            contexts: parasolid_topology_attribute_contexts(ir, topology_references, class_uses),
        }
    }
}

fn parasolid_topology_attribute_contexts<'a>(
    ir: &CadIr,
    topology_references: &'a [crate::native::parasolid::ParasolidTopologyAttributeListReference],
    class_uses: &'a [crate::native::parasolid::ParasolidTopologyAttributeClassUse],
) -> Vec<ParasolidTopologyAttributeContext<'a>> {
    let mut entities_by_reference = BTreeMap::<&str, BTreeSet<&str>>::new();
    for class_use in class_uses {
        entities_by_reference
            .entry(class_use.topology_attribute_reference.as_str())
            .or_default()
            .insert(class_use.entity_51_record.as_str());
    }
    let mut references_by_target = BTreeMap::<String, Vec<_>>::new();
    for reference in topology_references {
        let kind = reference.topology_type.as_str();
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
        .flat_map(|(target_key, references)| {
            let Some(&reference) = references.first().filter(|_| references.len() == 1) else {
                return Vec::new();
            };
            let Some(target) = emitted_targets.get(target_key.as_str()) else {
                return Vec::new();
            };
            let mut entities = BTreeSet::new();
            if let Some(entity) = reference.attribute_list_record.as_deref() {
                entities.insert(entity);
            }
            if let Some(class_entities) = entities_by_reference.get(reference.id.as_str()) {
                entities.extend(class_entities.iter().copied());
            }
            let multiple_entities = entities.len() > 1;
            entities
                .into_iter()
                .map(|entity| ParasolidTopologyAttributeContext {
                    reference,
                    entity,
                    id_suffix: multiple_entities
                        .then(|| entity_suffix_key(entity))
                        .flatten(),
                    target: target.clone(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn topology_attribute_id(
    reference: &crate::native::parasolid::ParasolidTopologyAttributeListReference,
    family: &cadmpeg_ir::ids::IdentityComponent,
    reference_ordinal: u32,
    entity_suffix: Option<&cadmpeg_ir::ids::IdentityKey>,
) -> AttributeId {
    let mut key = cadmpeg_ir::ids::IdentityKey::from(reference.topology_type.code())
        .dash(reference.topology_xmt)
        .dash(reference_ordinal);
    if let Some(suffix) = entity_suffix {
        key = key.dash(suffix);
    }
    IdScope::stream(reference.stream_ordinal).id(family, key)
}

/// The key half of an entity reference, which is what an id suffix names.
///
/// The reference reaches this as stored record text, so it is admitted here;
/// text that is not key text names no suffix.
fn entity_suffix_key(entity: &str) -> Option<cadmpeg_ir::ids::IdentityKey> {
    let suffix = entity.rsplit_once('#').map_or(entity, |(_, key)| key);
    let Ok(key) = cadmpeg_ir::ids::IdentityKey::try_new(suffix) else {
        return None;
    };
    Some(key)
}

fn attach_parasolid_topology_numeric_attributes(
    ir: &mut CadIr,
    sources: &ParasolidNumericAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
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
        uses.sort_by_key(|numeric_use| numeric_use.position);
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
                        record
                            .values
                            .as_slice()
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
                            .as_slice()
                            .iter()
                            .map(|value| AttributeValue::Float(*value))
                            .collect(),
                        record.inflated_offset,
                        "ENTITY_53_DOUBLE_ATTRIBUTE",
                        "double",
                    )
                }
            };
            let id = topology_attribute_id(
                reference,
                &cadmpeg_ir::identity_component!("topology-numeric-attribute"),
                numeric_use.position.reference_ordinal(),
                context.id_suffix.as_ref(),
            );
            let source_stream = StreamHandle::new(
                cadmpeg_ir::stream_name!("nx:s").with_suffix(reference.stream_ordinal),
            );
            annotations
                .note(id.as_str(), &source_stream, source_offset)
                .tag(tag);
            annotations
                .derived(id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            annotations
                .derived(id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            let generic_name = format!(
                "parasolid_type_{lane}_reference_{}",
                numeric_use.position.reference_ordinal()
            );
            let name = attribute_index
                .attribute_names
                .field_name(reference, numeric_use.id.as_str());
            ir.model.attributes.push(SourceAttribute {
                id,
                target: context.target.clone(),
                name: name
                    .or_else(|| {
                        attribute_index
                            .class_names
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
        .sort_by(|first, second| first.id.as_str().cmp(second.id.as_str()));
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
    ir: &mut CadIr,
    sources: &ParasolidStructuredAttributeSources<'_>,
    attribute_index: &ParasolidTopologyAttributeIndex<'_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
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
        uses.sort_by_key(|structured_use| structured_use.position);
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
                        record
                            .values
                            .as_slice()
                            .iter()
                            .map(|value| AttributeValue::Vector(value.finite_components().into()))
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
                            .as_slice()
                            .iter()
                            .map(|axis| {
                                AttributeValue::Vector(
                                    axis.iter()
                                        .flat_map(|vector| vector.finite_components())
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
                            .as_slice()
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
                        vec![AttributeValue::String(record.value.as_str().to_owned())],
                        record.inflated_offset,
                        "ENTITY_62_UNICODE_ATTRIBUTE",
                        "98_unicode",
                    )
                }
            };
            let id = topology_attribute_id(
                reference,
                &cadmpeg_ir::identity_component!("topology-structured-attribute"),
                structured_use.position.reference_ordinal(),
                context.id_suffix.as_ref(),
            );
            let source_stream = StreamHandle::new(
                cadmpeg_ir::stream_name!("nx:s").with_suffix(reference.stream_ordinal),
            );
            annotations
                .note(id.as_str(), &source_stream, source_offset)
                .tag(tag);
            annotations
                .derived(id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            annotations
                .derived(id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            let generic_name = format!(
                "parasolid_type_{family}_reference_{}",
                structured_use.position.reference_ordinal()
            );
            let name = attribute_index
                .attribute_names
                .field_name(reference, structured_use.id.as_str());
            ir.model.attributes.push(SourceAttribute {
                id,
                target: context.target.clone(),
                name: name
                    .or_else(|| {
                        attribute_index
                            .class_names
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
        .sort_by(|first, second| first.id.as_str().cmp(second.id.as_str()));
    Ok(())
}

fn push_referenced_parameter(
    ctx: &DecodeContext<'_>,
    reservation: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    referenced: &mut Vec<ParameterId>,
    expression: &str,
) -> Result<(), CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(expression.len()), "NX referenced parameter identity")?;
    let bytes = std::mem::size_of::<ParameterId>().checked_add(expression.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX referenced parameter", 0, cadmpeg_core::decode::u64_from_index(expression.len())))?;
    ctx.charge_collection_items(1, "NX referenced parameters")?;
    reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
    let Some(id) = expression_parameter_id(expression) else {
        return Ok(());
    };
    reserve_attach_vec(ctx, referenced, 1, "NX referenced parameters")?;
    referenced.push(id);
    Ok(())
}

fn push_unique_feature_dependency(
    ctx: &DecodeContext<'_>,
    dependencies: &mut Vec<FeatureId>,
    candidate: &FeatureId,
) -> Result<(), CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(dependencies.len()), "NX feature dependency uniqueness")?;
    if dependencies.contains(candidate) {
        return Ok(());
    }
    let bytes = std::mem::size_of::<FeatureId>().checked_add(candidate.as_str().len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX feature dependency", 0, cadmpeg_core::decode::u64_from_index(candidate.as_str().len())))?;
    ctx.charge_collection_items(1, "NX feature dependencies")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX feature dependency")?;
    reserve_attach_vec(ctx, dependencies, 1, "NX feature dependencies")?;
    dependencies.push(candidate.clone());
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
    native_ref: &str,
    order: u32,
    payload_strings: &[&str],
) -> Option<SemanticAnnotation> {
    let [text, font_family] = payload_strings else {
        return None;
    };
    Some(SemanticAnnotation {
        id: extended_id(native_ref, &cadmpeg_ir::identity_key!("semantic-text"))?,
        object: native_ref.to_string(),
        kind: SemanticAnnotationKind::Text,
        runtime_type: "TEXT".to_string(),
        order,
        text: vec![(*text).to_string()],
        references: BTreeMap::new(),
        value: None,
        format: None,
        position: None,
        parameters: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("font_family"),
            (*font_family).to_string(),
        )]),
        assets: Vec::new(),
        native_ref: native_ref.to_string(),
    })
}

pub(super) fn parameter_owner_dependencies(
    ctx: &DecodeContext<'_>,
    parameter_owners: &BTreeMap<ParameterId, Option<FeatureId>>,
    parameter_references: &[ParameterId],
) -> Result<Vec<FeatureId>, CodecError> {
    let mut dependencies = Vec::new();
    for parameter_id in parameter_references {
        let work = parameter_owners.len().checked_add(dependencies.len()).and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit("NX parameter owner dependency scan", 0, cadmpeg_core::decode::u64_from_index(parameter_owners.len())))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), "NX parameter owner dependency scan")?;
        let Some(owner) = parameter_owners.get(parameter_id).and_then(Option::as_ref) else {
            continue;
        };
        if !dependencies.contains(owner) {
            let bytes = std::mem::size_of::<FeatureId>().checked_add(owner.as_str().len())
                .ok_or_else(|| ctx.refuse_codec_limit("NX parameter owner dependency", 0, cadmpeg_core::decode::u64_from_index(owner.as_str().len())))?;
            ctx.charge_collection_items(1, "NX parameter owner dependencies")?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX parameter owner dependency")?;
            reserve_attach_vec(ctx, &mut dependencies, 1, "NX parameter owner dependencies")?;
            dependencies.push(owner.clone());
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
    let constructions = [construction_profile, structured_construction]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let profile = match constructions.as_slice() {
        [construction] => ProfileRef::Planar(PlanarProfileRef::Native((*construction).to_string())),
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
            for face_id in shell.faces() {
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

struct ScopedFaces<'a, 'ctx> {
    faces: Vec<&'a Face>,
    _reservation: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> std::ops::Deref for ScopedFaces<'a, 'ctx> {
    type Target = [&'a Face];

    fn deref(&self) -> &Self::Target {
        &self.faces
    }
}

fn connected_solid_body_faces<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &'a CadIr,
    body_id: &BodyId,
) -> Result<Option<ScopedFaces<'a, 'ctx>>, CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()), "NX connected solid body lookup")?;
    let Some(body) = ir.model.bodies.iter().find(|body| body.id == *body_id) else {
        return Ok(None);
    };
    if body.kind != cadmpeg_ir::topology::BodyKind::Solid {
        return Ok(None);
    }
    let [region_id] = body.regions.as_slice() else {
        return Ok(None);
    };
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.regions.len()), "NX connected solid region lookup")?;
    let Some(region) = ir
        .model
        .regions
        .iter()
        .find(|region| region.id == *region_id && region.body == body.id) else {
        return Ok(None);
    };
    let [shell_id] = region.shells.as_slice() else {
        return Ok(None);
    };
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.shells.len()), "NX connected solid shell lookup")?;
    let Some(shell) = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == *shell_id && shell.region == region.id) else {
        return Ok(None);
    };
    let mut faces = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX connected solid faces")?;
    for face_id in shell.faces() {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.faces.len()), "NX connected solid face lookup")?;
        let Some(face) = ir.model.faces.iter().find(|face| face.id == *face_id && face.shell == shell.id) else {
            return Ok(None);
        };
        ctx.charge_collection_items(1, "NX connected solid faces")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<&Face>()))?;
        reserve_attach_vec(ctx, &mut faces, 1, "NX connected solid faces")?;
        faces.push(face);
    }
    Ok(Some(ScopedFaces { faces, _reservation: reservation }))
}

fn connected_solid_body_exists(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body: &cadmpeg_ir::topology::Body,
) -> Result<bool, CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()), "NX connected solid identity scan")?;
    let Some(body) = ir.model.bodies.iter().find(|candidate| candidate.id == body.id) else {
        return Ok(false);
    };
    if body.kind != BodyKind::Solid {
        return Ok(false);
    }
    let [region_id] = body.regions.as_slice() else {
        return Ok(false);
    };
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.regions.len()), "NX connected solid region scan")?;
    let Some(region) = ir.model.regions.iter().find(|region| region.id == *region_id && region.body == body.id) else {
        return Ok(false);
    };
    let [shell_id] = region.shells.as_slice() else {
        return Ok(false);
    };
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.shells.len()), "NX connected solid shell scan")?;
    let Some(shell) = ir.model.shells.iter().find(|shell| shell.id == *shell_id && shell.region == region.id) else {
        return Ok(false);
    };
    let face_work = shell.faces().len().checked_mul(ir.model.faces.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX connected solid face scan", 0, cadmpeg_core::decode::u64_from_index(shell.faces().len())))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(face_work), "NX connected solid face scan")?;
    Ok(shell.faces().iter().all(|face_id| ir.model.faces.iter().any(|face| face.id == *face_id && face.shell == shell.id)))
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
        let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) else {
            continue;
        };
        if !body_surfaces.contains(owner) {
            continue;
        }
        let ProceduralSurfaceDefinition::Blend(definition_payload) = procedural.definition() else {
            continue;
        };
        let supports = definition_payload.supports();
        let radius = definition_payload.radius();
        let cross_section = definition_payload.cross_section();

        if *cross_section != BlendCrossSection::Circular {
            return None;
        }
        surfaces.push(owner.clone());
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
            BlendRadiusLaw::Constant { signed_radius } if signed_radius.get() != 0.0 => {
                Some(signed_radius.get().abs())
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
            || {
                if constant_radii.is_some() {
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
                    }
                } else if laws.iter().all(|law| {
                    matches!(
                        law,
                        BlendRadiusLaw::Linear { .. } | BlendRadiusLaw::Law { .. }
                    )
                }) {
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable),
                    }
                } else {
                    RadiusSpec::Unresolved { form: None }
                }
            },
            |radii| match cadmpeg_ir::scalar::PositiveLength::new(radii[0]) {
                Some(radius) => RadiusSpec::Constant { radius },
                None => RadiusSpec::Unresolved {
                    form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
                },
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
                        format!("{body}:blend-first-support-surfaces"),
                    );
                    let (second_faces, _) = support_face_projection(
                        ir,
                        &second,
                        format!("{body}:blend-second-support-surfaces"),
                    );
                    match (&first_faces, &second_faces) {
                        (FaceSelection::Resolved { .. }, FaceSelection::Resolved { .. }) => {
                            Some(FeatureDefinition::Operation(FeatureOperation::FaceBlend {
                                operands: cadmpeg_ir::features::FaceBlendOperands::new(
                                    first_faces,
                                    second_faces,
                                )
                                .ok()?,

                                radius: radius.clone(),
                            }))
                        }
                        _ => None,
                    }
                })
        })
        .flatten();
    let unresolved = match family {
        NxBlendFamily::Edge => FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius,
                    tangency_weight: None,
                },
            ),
        }),
        NxBlendFamily::Face => FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                FaceSelection::Unresolved,
                FaceSelection::Unresolved,
            )
            .ok()?,

            radius,
        }),
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
    let native = format!("{}:offset-support-surfaces", body.as_str());
    let (faces, senses) = support_face_projection(ir, &supports, native);
    let distance = senses
        .as_deref()
        .and_then(uniform_face_sense)
        .map(|sense| match sense {
            Sense::Forward => distance,
            Sense::Reversed => distance.negated(),
        });
    Some((
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces,
            distance: distance.map(Length::from_assigned_real),
        }),
        supports,
    ))
}

fn owned_offset_surface_data<'a>(
    ir: &CadIr,
    outputs: &'a [BodyId],
) -> Option<(&'a BodyId, FiniteReal, Vec<SurfaceId>)> {
    let (body, carriers) = owned_offset_carriers(ir, outputs)?;
    let distance = carriers[0].1;
    if carriers
        .iter()
        .any(|(_, candidate)| candidate.get().to_bits() != distance.get().to_bits())
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
) -> Option<(&'a BodyId, Vec<(SurfaceId, FiniteReal)>)> {
    let [body] = outputs else {
        return None;
    };
    let body_surfaces = body_surface_ids(ir, body)?;
    let mut carriers = Vec::new();
    for procedural in &ir.model.procedural_surfaces {
        let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) else {
            continue;
        };
        if !body_surfaces.contains(owner) {
            continue;
        }
        let ProceduralSurfaceDefinition::Offset(definition_payload) = procedural.definition()
        else {
            continue;
        };
        let support = definition_payload.support();
        let candidate = definition_payload.distance();
        carriers.push((support.clone(), candidate));
    }
    (!carriers.is_empty()).then_some((body, carriers))
}

fn thicken_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let (body, thickness, supports, direction) = owned_thicken_surface_data(ir, outputs)?;
    let native = format!("{}:thicken-support-surfaces", body.as_str());
    let (faces, senses) = support_face_projection(ir, &supports, native);
    let side = match direction {
        ThickenDirection::Both => Some(ThickenSide::Both),
        ThickenDirection::Signed(distance) => senses
            .as_deref()
            .and_then(uniform_face_sense)
            .map(|sense| thicken_side(distance, sense)),
    };
    Some((
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces,
            thickness: Some(thickness),
            side,
        }),
        supports,
    ))
}

enum ThickenDirection {
    Signed(NonZeroLength),
    Both,
}

fn owned_thicken_surface_data<'a>(
    ir: &CadIr,
    outputs: &'a [BodyId],
) -> Option<(&'a BodyId, PositiveLength, Vec<SurfaceId>, ThickenDirection)> {
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
        .all(|(_, candidate)| candidate.get().to_bits() == distance.get().to_bits())
    {
        if let Ok(distance) = NonZeroLength::try_from(Length::from_assigned_real(distance)) {
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

    let mut magnitude = None::<PositiveLength>;
    let mut positive = BTreeSet::new();
    let mut negative = BTreeSet::new();
    for (support, distance) in carriers {
        let distance = NonZeroLength::try_from(Length::from_assigned_real(distance)).ok()?;
        let candidate = distance.abs();
        if magnitude.is_some_and(|magnitude| magnitude.get().to_bits() != candidate.get().to_bits())
        {
            return None;
        }
        magnitude = Some(candidate);
        if distance.get().is_sign_positive() {
            positive.insert(support);
        } else {
            negative.insert(support);
        }
    }
    if positive.is_empty() || positive != negative {
        return None;
    }
    let thickness = PositiveLength::new(magnitude?.get() * 2.0)?;
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

fn thicken_side(distance: NonZeroLength, sense: Sense) -> ThickenSide {
    match (distance.get().is_sign_positive(), sense) {
        (true, Sense::Forward) | (false, Sense::Reversed) => ThickenSide::Forward,
        (true, Sense::Reversed) | (false, Sense::Forward) => ThickenSide::Reverse,
    }
}

fn uniform_face_sense(senses: &[Sense]) -> Option<Sense> {
    let (first, rest) = senses.split_first()?;
    rest.iter().all(|sense| sense == first).then_some(*first)
}

pub(super) fn feature_source_content(
    ctx: &DecodeContext<'_>,
    payload_strings: &[&crate::native::features::FeaturePayloadString],
) -> Result<cadmpeg_ir::features::FeatureContent, CodecError> {
    let mut sorted = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX feature source text order")?;
    for &value in payload_strings {
        ctx.charge_collection_items(1, "NX feature source text order")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<&crate::native::features::FeaturePayloadString>()))?;
        reserve_attach_vec(ctx, &mut sorted, 1, "NX feature source text order")?;
        sorted.push(value);
    }
    let count = sorted.len();
    let passes = usize::try_from(usize::BITS - count.leading_zeros())
        .map_err(|_| ctx.refuse_codec_limit("NX feature source text sort", 0, cadmpeg_core::decode::u64_from_index(count)))?;
    let work = count.checked_mul(passes)
        .ok_or_else(|| ctx.refuse_codec_limit("NX feature source text sort", 0, cadmpeg_core::decode::u64_from_index(count)))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), "NX feature source text sort")?;
    sorted.sort_by_key(|value| value.source_offset);
    let mut content = Vec::new();
    for value in sorted {
        let text = value.value.as_str();
        let bytes = std::mem::size_of::<FeatureSourceContent>()
            .checked_add(text.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX feature source text", 0, cadmpeg_core::decode::u64_from_index(text.len())))?;
        ctx.charge_collection_items(1, "NX feature source text")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX feature source text")?;
        let mut owned = String::new();
        owned.try_reserve(text.len()).map_err(|_| ctx.refuse_codec_limit("allocate NX feature source text", 0, cadmpeg_core::decode::u64_from_index(text.len())))?;
        owned.push_str(text);
        reserve_attach_vec(ctx, &mut content, 1, "NX feature source text")?;
        content.push(FeatureSourceContent::Text(owned));
    }
    cadmpeg_ir::features::FeatureContent::try_from(content).map_err(CodecError::malformed)
}

fn simple_hole_native_properties(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    operation_label: &str,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    repeated_lanes: &[crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLane],
    block_references: &[crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
    construction_groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
) -> Result<(), CodecError> {
    fn insert_property(
        ctx: &DecodeContext<'_>,
        properties: &mut BTreeMap<String, String>,
        key: &'static str,
        value: &str,
    ) -> Result<(), CodecError> {
        let bytes = std::mem::size_of::<(String, String)>()
            .checked_add(key.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX simple hole native property", 0, cadmpeg_core::decode::u64_from_index(value.len())))?;
        ctx.charge_collection_items(1, "NX simple hole native property")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX simple hole native property")?;
        properties.insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    if let Some(template) = templates
        .iter()
        .find(|template| template.operation_label == operation_label)
    {
        insert_property(ctx, properties, "simple_hole_template", &template.id)?;
    }
    if let Some(pair) = repeated_lanes
        .iter()
        .find(|pair| pair.operation_label == operation_label)
    {
        insert_property(ctx, properties, "simple_hole_repeated_scalar_lane", &pair.id)?;
    }
    if let Some(references) = block_references
        .iter()
        .find(|references| references.operation_label == operation_label)
    {
        insert_property(ctx, properties, "simple_hole_repeated_scalar_lane_block_references", &references.id)?;
    }
    if let Some(group) = construction_groups.iter().find(|group| {
        group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|label| label == operation_label)
    }) {
        insert_property(ctx, properties, "simple_hole_construction_group", &group.id)?;
    }
    Ok(())
}

fn block_placement(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    dimensions: [f64; 3],
    outputs: &[BodyId],
) -> Result<Option<(BodyId, Transform)>, CodecError> {
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

    fn canonical_normal(
        normal: cadmpeg_ir::units::UnitVector3,
        angular_tolerance: f64,
    ) -> Option<Vector3> {
        let mut normal = *normal.to_unit_length_charted().as_raw();
        let leading = [normal.x, normal.y, normal.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)?;
        if leading < 0.0 {
            normal = Vector3::new(-normal.x, -normal.y, -normal.z);
        }
        Some(normal)
    }

    fn plane_extent(
        ctx: &DecodeContext<'_>,
        band: &mut PlaneBand,
        linear_tolerance: f64,
    ) -> Result<Option<PlaneExtent>, CodecError> {
        let count = band.offsets.len();
        let passes = usize::try_from(usize::BITS - count.leading_zeros())
            .map_err(|_| ctx.refuse_codec_limit("NX block plane sort", 0, cadmpeg_core::decode::u64_from_index(count)))?;
        let work = count.checked_mul(passes)
            .ok_or_else(|| ctx.refuse_codec_limit("NX block plane sort", 0, cadmpeg_core::decode::u64_from_index(count)))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), "NX block plane sort")?;
        band.offsets.sort_by(f64::total_cmp);
        let mut first: Option<[f64; 2]> = None;
        let mut second: Option<[f64; 2]> = None;
        for &offset in &band.offsets {
            if !offset.is_finite() {
                return Ok(None);
            }
            if let Some(cluster) = second.as_mut().or(first.as_mut()) {
                if offset - cluster[0] <= linear_tolerance {
                    cluster[1] = offset;
                    continue;
                }
            }
            if first.is_none() {
                first = Some([offset, offset]);
            } else if second.is_none() {
                second = Some([offset, offset]);
            } else {
                return Ok(None);
            }
        }
        let (Some(minimum), Some(maximum)) = (first, second) else {
            return Ok(None);
        };
        if maximum[1] - minimum[0] <= linear_tolerance {
            return Ok(None);
        }
        Ok(Some(PlaneExtent {
            normal: band.normal,
            minimum: minimum[0],
            maximum: maximum[1],
        }))
    }

    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    if dimensions.iter().any(|dimension| *dimension <= linear_tolerance) {
        return Ok(None);
    }
    let body = match outputs {
        [body] => body,
        [] => {
            let mut unique = None;
            for candidate in &ir.model.bodies {
                if connected_solid_body_faces(ctx, ir, &candidate.id)?.is_some() {
                    if unique.replace(&candidate.id).is_some() {
                        return Ok(None);
                    }
                }
            }
            let Some(body) = unique else {
                return Ok(None);
            };
            body
        }
        _ => return Ok(None),
    };
    let Some(faces) = connected_solid_body_faces(ctx, ir, body)? else {
        return Ok(None);
    };
    let mut bands = Vec::<PlaneBand>::new();
    let mut band_reservation = ctx.reserve_scoped(0, "NX block plane bands")?;
    for face in faces.iter().copied() {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()), "NX block plane surface lookup")?;
        let Some(geometry) = ir.model.surfaces.iter().rev()
            .find(|surface| surface.id == face.surface)
            .map(|surface| &surface.geometry) else {
            return Ok(None);
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = geometry else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let Some(normal) = canonical_normal(*plane_surface.frame().axis(), angular_tolerance) else {
            return Ok(None);
        };
        let offset = normal.dot(Vector3::new(origin.x, origin.y, origin.z));
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(bands.len()), "NX block plane band lookup")?;
        let existing = bands.iter_mut()
            .find(|band| (1.0 - band.normal.dot(normal)).abs() <= angular_tolerance);
        ctx.charge_collection_items(1, "NX block plane offsets")?;
        band_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<f64>()))?;
        if let Some(band) = existing {
            reserve_attach_vec(ctx, &mut band.offsets, 1, "NX block plane offsets")?;
            band.offsets.push(offset);
        } else {
            ctx.charge_collection_items(1, "NX block plane bands")?;
            band_reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<PlaneBand>()))?;
            let mut offsets = Vec::new();
            reserve_attach_vec(ctx, &mut offsets, 1, "NX block plane offsets")?;
            offsets.push(offset);
            reserve_attach_vec(ctx, &mut bands, 1, "NX block plane bands")?;
            bands.push(PlaneBand { normal, offsets });
        }
    }
    if bands.len() != 3
        || (0..3).any(|first| {
            (first + 1..3).any(|second| {
                bands[first].normal.dot(bands[second].normal).abs() > angular_tolerance
            })
        })
    {
        return Ok(None);
    }
    let [first, second, third] = bands.as_mut_slice() else {
        return Ok(None);
    };
    let (Some(first), Some(second), Some(third)) = (
        plane_extent(ctx, first, linear_tolerance)?,
        plane_extent(ctx, second, linear_tolerance)?,
        plane_extent(ctx, third, linear_tolerance)?,
    ) else {
        return Ok(None);
    };
    let mut extents = [first, second, third];
    extents.sort_by(|left, right| {
        right.normal.x.total_cmp(&left.normal.x)
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
    let mut matched = None;
    for permutation in permutations {
        if (0..3).all(|axis| {
            let band = extents[permutation[axis]];
            ((band.maximum - band.minimum) - dimensions[axis]).abs() <= linear_tolerance
        }) && matched.replace(permutation).is_some() {
            return Ok(None);
        }
    }
    let Some(permutation) = matched else {
        return Ok(None);
    };
    let mut ordered = permutation.map(|index| extents[index]);
    if ordered[0].normal.cross(ordered[1].normal).dot(ordered[2].normal) < 0.0 {
        let third = &mut ordered[2];
        third.normal = Vector3::new(-third.normal.x, -third.normal.y, -third.normal.z);
        (third.minimum, third.maximum) = (-third.maximum, -third.minimum);
    }
    let origin = Point3::new(
        ordered.iter().map(|band| band.minimum * band.normal.x).sum(),
        ordered.iter().map(|band| band.minimum * band.normal.y).sum(),
        ordered.iter().map(|band| band.minimum * band.normal.z).sum(),
    );
    let [x_axis, y_axis, z_axis] = ordered.map(|band| band.normal);
    let Some(placement) = Transform::affine([
        [x_axis.x, y_axis.x, z_axis.x, origin.x],
        [x_axis.y, y_axis.y, z_axis.y, origin.y],
        [x_axis.z, y_axis.z, z_axis.z, origin.z],
    ]) else {
        return Ok(None);
    };
    let body_bytes = std::mem::size_of::<BodyId>().checked_add(body.as_str().len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX block output body", 0, cadmpeg_core::decode::u64_from_index(body.as_str().len())))?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(body_bytes), "NX block output body")?;
    Ok(Some((body.clone(), placement)))
}

/// Return the complete primitive witness for an NX `SPHERE` operation.
///
/// A spherical surface inside a larger result is not enough: a Boolean or a
/// later feature can leave the same carrier in the output. The primitive
/// projection therefore accepts only one connected solid body with exactly
/// one face whose surface is a finite positive-radius sphere. With no native
/// output relation, the candidate must also be unique across the model.
fn sphere_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    outputs: &[BodyId],
) -> Result<Option<(
    BodyId,
    cadmpeg_ir::features::FinitePoint3,
    cadmpeg_ir::scalar::PositiveLength,
)>, CodecError> {
    let body = match outputs {
        [body] => body,
        [] => {
            let mut unique = None;
            for candidate in &ir.model.bodies {
                let Some(faces) = connected_solid_body_faces(ctx, ir, &candidate.id)? else {
                    continue;
                };
                let [face] = &faces[..] else {
                    continue;
                };
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()), "NX sphere fallback surface scan")?;
                if !ir.model.surfaces.iter().any(|surface| {
                    surface.id == face.surface
                        && matches!(surface.geometry.solved(), Some(SolvedSurfaceGeometry::Sphere(_)))
                }) {
                    continue;
                }
                if unique.replace(&candidate.id).is_some() {
                    return Ok(None);
                }
            }
            let Some(body) = unique else {
                return Ok(None);
            };
            body
        }
        _ => return Ok(None),
    };
    let Some(faces) = connected_solid_body_faces(ctx, ir, body)? else {
        return Ok(None);
    };
    let [face] = &faces[..] else {
        return Ok(None);
    };
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()), "NX sphere surface lookup")?;
    let Some(surface) = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface) else {
        return Ok(None);
    };
    let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
        return Ok(None);
    };
    let center = sphere_surface.center();
    let Ok(radius) = cadmpeg_ir::scalar::PositiveLength::try_from(sphere_surface.radius()) else {
        return Ok(None);
    };
    let body_bytes = std::mem::size_of::<BodyId>()
        .checked_add(body.as_str().len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX sphere output body", 0, cadmpeg_core::decode::u64_from_index(body.as_str().len())))?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(body_bytes), "NX sphere output body")?;
    Ok(Some((body.clone(), center, radius)))
}

struct NewBodyEvidence<'a> {
    has_complete_projection: bool,
    has_complete_primitive_construction: bool,
    outputs: &'a [BodyId],
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
    if evidence.has_complete_projection
        && matches!(evidence.outputs, [_])
        && !evidence.history.has_preceding_writer(
            evidence.provisional_feature,
            evidence.native_primary_body,
            evidence.offset_store_primary_body,
            evidence.outputs,
        )
    {
        BooleanOp::NewBody
    } else {
        BooleanOp::Unresolved
    }
}

fn body_writing_unresolved_feature_definition(
    kind: &str,
    source_properties: &BTreeMap<String, String>,
) -> Option<FeatureDefinition> {
    if !source_properties
        .keys()
        .any(|key| key.starts_with("body_write."))
    {
        return None;
    }
    match kind {
        "BREP" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Brep,
        })),
        "CONE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Cone,
        })),
        "SPHERE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Sphere,
        })),
        "BLEND" => Some(FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                },
            ),
        })),
        "FACE_BLEND" => Some(FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                FaceSelection::Unresolved,
                FaceSelection::Unresolved,
            )
            .ok()?,

            radius: RadiusSpec::Unresolved { form: None },
        })),
        "DELETE FACE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DeleteFace,
        })),
        "MIRROR_FACE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MirrorFace,
        })),
        "SUBDIVISION_BODY" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::SubdivisionBody,
        })),
        "TOPOLOGY_OPTIMIZATION" => {
            Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::TopologyOptimization,
            }))
        }
        "THREADS" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Thread,
        })),
        "DETAILED_THREAD" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DetailedThread,
        })),
        _ => None,
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
    .unwrap()
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
        .then_some(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: TreeChildren::default(),
        }))
}

/// Permutation-invariant hole properties derived from one complete body partition.
#[derive(Default)]
struct HoleProjection {
    placements: Vec<HolePlacement>,
    diameter: Option<Length>,
    extent: Option<LinearTermination>,
    counterbore: Option<CounterboreDimensions>,
    chamfer: Option<HoleKind>,
    grouped_simple_through: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CounterboreDimensions {
    diameter: cadmpeg_ir::scalar::PositiveLength,
    depth: cadmpeg_ir::scalar::PositiveLength,
}

fn non_boolean_feature_definition_with_parameters(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole: HoleProjection,
    native_parameters: BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<FeatureDefinition, CodecError> {
    let hole_template = unique_simple_hole_template(payload_strings);
    if matches!(kind, "BLEND" | "FACE_BLEND") {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: kind.into(),
            parameters: native_parameters,
        }));
    }
    if let ("BLOCK", Some([Some(length), Some(width), Some(height)])) = (
        kind,
        block_dimensions.map(|dimensions| dimensions.map(cadmpeg_ir::scalar::PositiveLength::new)),
    ) {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions: Some([length, width, height]),
            placement: block_placement.and_then(cadmpeg_ir::features::FeatureRigidPlacement::new),
            op: BooleanOp::Unresolved,
        }));
    }
    if let Some(op) = match kind {
        "UNITE" => Some(cadmpeg_ir::features::BooleanKind::Join),
        "SUBTRACT" => Some(cadmpeg_ir::features::BooleanKind::Cut),
        "INTERSECT" => Some(cadmpeg_ir::features::BooleanKind::Intersect),
        _ => None,
    } {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Unresolved,
                BodySelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            op,
            keep_tools: false,
        }));
    }
    Ok(match kind {
        "DATUM_PLANE" | "EXTRACT_DATUM_PLANE" => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            })
        }
        "DATUM_AXIS" | "EXTRACT_DATUM_AXIS" => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumAxis,
            })
        }
        "BRIDGE_CURVE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::BridgeCurve,
        }),
        "POINT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPoint,
        }),
        "DATUM_CSYS" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem,
        }),
        "BLOCK" => FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        }),
        "SKETCH" => FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
        }),
        "EXTRACT_BODY" => FeatureDefinition::Operation(FeatureOperation::ExtractBody {
            source: BodySelection::Unresolved,
        }),
        "MASTER SNAPSHOT BODY" => FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Unresolved,
        }),
        "SKIN" | "THRU_CURVE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Loft,
        }),
        "THRU_CURVE_MESH" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::ThroughCurveMesh,
        }),
        "Studio Surface" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::FreeformSurface,
        }),
        "SWP104" => FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::unresolved(None),
            path: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            scale: None,
            allow_multi_profile_faces: None,
        }),
        "DRAFT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Draft,
        }),
        "CPROJ" | "CPROJ_CMB" => FeatureDefinition::Operation(FeatureOperation::ProjectedCurve {
            source: PathRef::Unresolved("nx:unresolved".into()),
            target_faces: FaceSelection::Unresolved,
            direction: CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved),
            bidirectional: None,
        }),
        "TRIMMED_SH" => FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: PathRef::Unresolved("nx:unresolved".into()),
            keep: TrimRegion::Unresolved,
        }),
        "EXTRACT_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::ExtractFace,
        }),
        "COPY_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::CopyFace,
        }),
        "LINKED_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::LinkedFace,
        }),
        "FILL_HOLE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::FillHole,
        }),
        "MOVE_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MoveFace,
        }),
        "MOVE_OBJECT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MoveObject,
        }),
        "CYLINDER" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Cylinder,
        }),
        "SYMBOLIC_THREAD" => symbolic_thread_feature_definition(),
        "EXTEND_SHEET" => FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        }),
        "SIMPLE HOLE" | "CBORE_HOLE" | "CSUNK_HOLE" => {
            let measured_chamfer = hole.chamfer;
            let (template_kind, template_exit_kind, template_extent) = hole_template.map_or(
                (
                    if matches!(kind, "CBORE_HOLE" | "CSUNK_HOLE") {
                        HoleKind::Unresolved(None)
                    } else {
                        HoleKind::Simple
                    },
                    None,
                    None,
                ),
                |(form, extent, start_treatment, end_treatment)| {
                    let kind = match start_treatment {
                        crate::native::features::holes::SimpleHoleEndTreatment::Chamfer => {
                            HoleKind::Unresolved(Some(HoleForm::Chamfer))
                        }
                        crate::native::features::holes::SimpleHoleEndTreatment::None => {
                            match form {
                                crate::native::features::holes::SimpleHoleForm::Simple => {
                                    HoleKind::Simple
                                }
                                crate::native::features::holes::SimpleHoleForm::Counterbored => {
                                    HoleKind::Unresolved(Some(HoleForm::Counterbore))
                                }
                                crate::native::features::holes::SimpleHoleForm::Countersunk => {
                                    HoleKind::Unresolved(Some(HoleForm::Countersink))
                                }
                            }
                        }
                    };
                    let exit_kind = match end_treatment {
                        crate::native::features::holes::SimpleHoleEndTreatment::Chamfer => {
                            Some(HoleKind::Unresolved(Some(HoleForm::Chamfer)))
                        }
                        crate::native::features::holes::SimpleHoleEndTreatment::None => None,
                    };
                    let extent = match extent {
                        crate::native::features::holes::SimpleHoleExtent::Through => {
                            Some(cadmpeg_ir::features::LinearTermination::ThroughAll {})
                        }
                        crate::native::features::holes::SimpleHoleExtent::Blind => None,
                    };
                    (kind, exit_kind, extent)
                },
            );
            let template_kind = match (
                hole.counterbore,
                matches!(
                    &template_kind,
                    HoleKind::Unresolved(Some(HoleForm::Counterbore))
                        | HoleKind::PartialCounterbore(..)
                ),
            ) {
                (Some(dimensions), true) => HoleKind::Counterbore {
                    diameter: dimensions.diameter,
                    depth: dimensions.depth,
                },
                _ => template_kind,
            };
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                direction: None,
                placements: Some(hole.placements).filter(|placements| !placements.is_empty()),
                shape: cadmpeg_ir::features::holes::HoleShape::new(
                    cadmpeg_ir::features::holes::HoleConstruction::Form {
                        kind: match (measured_chamfer, hole_template) {
                            (
                                Some(chamfer),
                                Some((
                                    crate::native::features::holes::SimpleHoleForm::Simple,
                                    crate::native::features::holes::SimpleHoleExtent::Through,
                                    crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                    crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                )),
                            ) => chamfer,
                            _ => template_kind,
                        },
                        specification: None,
                    },
                    match (measured_chamfer, hole_template) {
                        (
                            Some(chamfer),
                            Some((
                                crate::native::features::holes::SimpleHoleForm::Simple,
                                crate::native::features::holes::SimpleHoleExtent::Through,
                                crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                            )),
                        ) => Some(chamfer),
                        _ => template_exit_kind,
                    },
                    hole.diameter.and_then(|diameter| {
                        cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()
                    }),
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,

                extent: hole.extent.or(template_extent),
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            })
        }
        "HOLE PACKAGE" => FeatureDefinition::Operation(FeatureOperation::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            direction: None,
            placements: Some(hole.placements).filter(|placements| !placements.is_empty()),
            shape: cadmpeg_ir::features::holes::HoleShape::new(
                cadmpeg_ir::features::holes::HoleConstruction::Form {
                    kind: if hole.grouped_simple_through {
                        hole.chamfer.unwrap_or(HoleKind::Simple)
                    } else {
                        HoleKind::Unresolved(None)
                    },
                    specification: None,
                },
                hole.grouped_simple_through
                    .then_some(hole.chamfer)
                    .flatten(),
                hole.diameter.and_then(|diameter| {
                    cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()
                }),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            extent: hole
                .grouped_simple_through
                .then_some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        }),
        "RIB" => FeatureDefinition::Operation(FeatureOperation::Rib {
            construction: RibConstruction {
                profile: None,
                direction: None,
                thickness: None,
                side: None,
                draft: RibDraft::Unresolved,
            },
            op: BooleanOp::Unresolved,
        }),
        "SHELL" => shell_feature_definition(),
        "ENLARGE" => enlarge_feature_definition(),
        "CHAMFER" => FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Unresolved { form: None },
                },
            ),
            flip_direction: false,
        }),
        "BLEND" => FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                },
            ),
        }),
        "FACE_BLEND" => FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                FaceSelection::Unresolved,
                FaceSelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            radius: RadiusSpec::Unresolved { form: None },
        }),
        "SEW" => FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: BodySelection::Unresolved
                .try_into()
                .map_err(CodecError::malformed)?,
            gap_tolerance: None,
        }),
        "TRIM BODY" => FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Unresolved,
                BodySelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            keep: BodyTrimSide::Unresolved,
        }),
        "EXTRUDE" => extrude_feature_definition(None, None, BooleanOp::Unresolved, &[]),
        "OFFSET" => FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
        }),
        "THICKEN_SHEET" => FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        }),
        "Pattern Feature"
        | "Pattern Geometry"
        | "Geometry Instance"
        | "Multi Instance Output"
        | "IDENTICAL INSTANCE OUTPUT"
        | "Instance Feature" => FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::UNRESOLVED,
        }),
        "ASSOCIATIVE_INTERSECTION" | "Intersection Curve" => {
            FeatureDefinition::Operation(FeatureOperation::SectionShape {
                operands: cadmpeg_ir::features::SectionOperands::new(
                    BodySelection::Unresolved,
                    BodySelection::Unresolved,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,

                approximate: None,
            })
        }
        _ => FeatureDefinition::Operation(FeatureOperation::Native {
            kind: kind.into(),
            parameters: native_parameters,
        }),
    })
}

/// Project a BREP operation as direct stored geometry when its result bodies
/// are resolved. A BREP record carries boundary representation rather than a
/// replayable parametric construction; without a closed result-body relation,
/// retaining the native definition preserves the unresolved history edge.
fn brep_feature_definition(outputs: &[BodyId]) -> Option<FeatureDefinition> {
    (!outputs.is_empty() && outputs.iter().collect::<BTreeSet<_>>().len() == outputs.len())
        .then_some(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        ))
}

/// Preserve a SHELL operation as a typed neutral family while its construction roles remain
/// unresolved. The operation label identifies the family, but does not assign bodies, opening
/// faces, thickness, side, offset mode, corner join, or intersection policy.
fn shell_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: None,
        removed_faces: FaceSelection::Unresolved,
        thickness: None,
        outward: None,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    })
}

/// Preserve an ENLARGE operation as a typed surface-extension family while its selected faces,
/// extension law, and extent remain unresolved.
fn enlarge_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
        faces: FaceSelection::Unresolved,
        distance: None,
        method: SurfaceExtension::Unresolved,
    })
}

/// Preserve a `SYMBOLIC_THREAD` operation as a cosmetic-thread family while its cylindrical face,
/// nominal diameter, and axial extent remain unresolved.
fn symbolic_thread_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
        face: FaceSelection::Unresolved,
        diameter: None,
        extent: None,
    })
}

fn native_feature_parameters(
    ctx: &DecodeContext<'_>,
    uses: &[&crate::native::features::FeatureParameterUse],
    expressions: &[crate::native::om::Expression],
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut parameters = BTreeMap::new();
    for parameter_use in uses {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(expressions.len()), "NX native parameter expression lookup")?;
        let Some(expression) = expressions.iter().rev().find(|expression| expression.id == parameter_use.expression) else {
            return Ok(BTreeMap::new());
        };
        if parameters.contains_key(expression.name.as_str()) {
            return Ok(BTreeMap::new());
        }
        let bytes = std::mem::size_of::<(String, String)>()
            .checked_add(expression.name.as_str().len())
            .and_then(|bytes| bytes.checked_add(expression.expression.len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX native feature parameter", 0, cadmpeg_core::decode::u64_from_index(expression.expression.len())))?;
        ctx.charge_collection_items(1, "NX native feature parameter")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX native feature parameter")?;
        parameters.insert(expression.name.as_str().to_owned(), expression.expression.clone());
    }
    Ok(parameters)
}

/// Resolve explicit hole outputs only from the proven segment-body namespace.
/// Offset-store body fields remain absent so a complete unique-solid topology
/// witness can apply the documented fallback.
fn primary_hole_outputs(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    body_references: &BTreeMap<&str, u32>,
    body_bindings: &[crate::native::segments::SegmentBodyBinding],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<BTreeMap<String, Vec<BodyId>>, CodecError> {
    let mut outputs = BTreeMap::new();
    for template in templates {
        ctx.charge_work(1, "NX primary hole output scan")?;
        let Some(object_index) = body_references.get(template.operation_label.as_str()) else {
            continue;
        };
        let bodies = feature_body_outputs(ctx, *object_index, body_bindings, bodies_by_object_index)?;
        charge_hole_map_entry::<Vec<BodyId>>(ctx, &template.operation_label, 0, "NX primary hole output map")?;
        outputs.insert(template.operation_label.clone(), bodies);
    }
    Ok(outputs)
}

fn push_hole_operation_label(
    ctx: &DecodeContext<'_>,
    operations: &mut Vec<String>,
    label: &str,
) -> Result<(), CodecError> {
    let bytes = std::mem::size_of::<String>().checked_add(label.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX hole operation labels", 0, cadmpeg_core::decode::u64_from_index(label.len())))?;
    ctx.charge_collection_items(1, "NX hole operation labels")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX hole operation labels")?;
    reserve_attach_vec(ctx, operations, 1, "NX hole operation labels")?;
    operations.push(label.to_owned());
    Ok(())
}

fn charge_hole_sort_work(ctx: &DecodeContext<'_>, count: usize) -> Result<(), CodecError> {
    let work = count.checked_mul(count)
        .ok_or_else(|| ctx.refuse_codec_limit("NX hole operation sort", 0, cadmpeg_core::decode::u64_from_index(count)))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), "NX hole operation sort")
}

fn simple_hole_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
    operation_positions: &BTreeMap<&str, usize>,
) -> Result<Option<Vec<String>>, CodecError> {
    let mut ordered_templates = Vec::new();
    let mut reservation = ctx.reserve_scoped(0, "NX simple hole selected templates")?;
    for template in templates {
        ctx.charge_work(1, "NX simple hole template scan")?;
        if template.form != crate::native::features::holes::SimpleHoleForm::Simple
            || template.extent != crate::native::features::holes::SimpleHoleExtent::Through
        {
            continue;
        }
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(templates.len()), "NX simple hole template identity scan")?;
        if templates.iter().filter(|candidate| candidate.operation_label == template.operation_label).count() != 1
            || !operation_positions.contains_key(template.operation_label.as_str())
        {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "NX simple hole selected templates")?;
        reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<&crate::native::features::holes::FeatureSimpleHoleTemplate>()))?;
        reserve_attach_vec(ctx, &mut ordered_templates, 1, "NX simple hole selected templates")?;
        ordered_templates.push(template);
    }
    if ordered_templates.is_empty() {
        return Ok(None);
    }
    charge_hole_sort_work(ctx, ordered_templates.len())?;
    ordered_templates.sort_by(|first, second| {
        operation_positions.get(first.operation_label.as_str())
            .cmp(&operation_positions.get(second.operation_label.as_str()))
            .then_with(|| first.operation_label.cmp(&second.operation_label))
    });
    let mut selected_group = None;
    for group in groups {
        let comparisons = group.members.len().checked_mul(ordered_templates.len())
            .and_then(|count| count.checked_mul(2))
            .ok_or_else(|| ctx.refuse_codec_limit("NX simple hole group membership", 0, cadmpeg_core::decode::u64_from_index(group.members.len())))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(comparisons), "NX simple hole group membership")?;
        let same_operations = group.members.iter().all(|member| ordered_templates.iter().any(|template| template.operation_label == member.operation_label))
            && ordered_templates.iter().all(|template| group.members.iter().any(|member| member.operation_label == template.operation_label));
        if same_operations {
            if selected_group.replace(group).is_some() {
                return Ok(None);
            }
        }
    }
    let mut operations = Vec::new();
    if let Some(group) = selected_group {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(group.members.len()), "NX simple hole group order")?;
        if group.members.iter().any(|member| !operation_positions.contains_key(member.operation_label.as_str()))
            || group.members.windows(2).any(|pair| {
                operation_positions[pair[0].operation_label.as_str()]
                    >= operation_positions[pair[1].operation_label.as_str()]
            })
        {
            return Ok(None);
        }
        for member in group.members.iter() {
            push_hole_operation_label(ctx, &mut operations, &member.operation_label)?;
        }
    } else {
        for template in ordered_templates {
            push_hole_operation_label(ctx, &mut operations, &template.operation_label)?;
        }
    }
    Ok(Some(operations))
}

/// Select uniquely typed hole operations in feature-history order.
fn selected_hole_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
    accepts: impl Fn(&crate::native::features::holes::FeatureSimpleHoleTemplate) -> bool,
) -> Result<Option<Vec<String>>, CodecError> {
    let mut operations = Vec::new();
    for template in templates {
        ctx.charge_work(1, "NX selected hole template scan")?;
        if !accepts(template) {
            continue;
        }
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(templates.len()), "NX selected hole template identity scan")?;
        if templates.iter().filter(|candidate| candidate.operation_label == template.operation_label).count() == 1 {
            push_hole_operation_label(ctx, &mut operations, &template.operation_label)?;
        }
    }
    if operations.is_empty() || !hole_operations_are_unique(ctx, &operations)? {
        return Ok(None);
    }
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(operations.len()), "NX selected hole operation positions")?;
    if operations.iter().any(|operation| !operation_positions.contains_key(operation.as_str())) {
        return Ok(None);
    }
    charge_hole_sort_work(ctx, operations.len())?;
    operations.sort_by(|first, second| {
        operation_positions.get(first.as_str())
            .cmp(&operation_positions.get(second.as_str()))
            .then_with(|| first.cmp(second))
    });
    Ok(Some(operations))
}

/// Return simple blind-hole operations in feature-history order. A blind
/// operation with competing typed templates is not assignable to one body
/// witness and remains native-only.
fn blind_hole_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
) -> Result<Option<Vec<String>>, CodecError> {
    selected_hole_operations(ctx, templates, operation_positions, |template| {
        template.form == crate::native::features::holes::SimpleHoleForm::Simple
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Blind
    })
}

/// Return counterbored through-hole operations in feature-history order.
/// Counterbore construction groups are not inferred from the scalar lanes:
/// each operation must have its own unambiguous body and topology witness.
fn counterbore_operations(
    ctx: &DecodeContext<'_>,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
) -> Result<Option<Vec<String>>, CodecError> {
    selected_hole_operations(ctx, templates, operation_positions, |template| {
        template.form == crate::native::features::holes::SimpleHoleForm::Counterbored
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Through
            && template.start_treatment == crate::native::features::holes::SimpleHoleEndTreatment::None
            && template.end_treatment == crate::native::features::holes::SimpleHoleEndTreatment::None
    })
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
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
    uses: &[crate::native::features::holes::FeatureHolePackageConstructionGroupUse],
    outputs: &BTreeMap<String, Vec<BodyId>>,
    diameters: &BTreeMap<String, Length>,
    chamfers: &BTreeMap<String, HoleKind>,
) -> Result<HolePackageProjection, CodecError> {
    let mut projection = HolePackageProjection::default();
    for use_ in uses {
        let use_scans = uses.len().checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit("NX hole package use uniqueness", 0, cadmpeg_core::decode::u64_from_index(uses.len())))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(use_scans), "NX hole package use uniqueness")?;
        if uses.iter().filter(|candidate| candidate.operation_label == use_.operation_label).count() != 1
            || uses.iter().filter(|candidate| candidate.simple_hole_construction_group == use_.simple_hole_construction_group).count() != 1
        {
            continue;
        }
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(groups.len()), "NX hole package group lookup")?;
        let Some(group) = groups
            .iter()
            .find(|group| group.id == use_.simple_hole_construction_group)
        else {
            continue;
        };
        if group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|operation| projection.internal_operations.contains(operation))
        {
            continue;
        }
        let template_scans = group.members.len().checked_mul(templates.len())
            .and_then(|count| count.checked_mul(2))
            .ok_or_else(|| ctx.refuse_codec_limit("NX hole package template lookup", 0, cadmpeg_core::decode::u64_from_index(group.members.len())))?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(template_scans), "NX hole package template lookup")?;
        let mut requests_chamfer = true;
        let mut requests_no_treatment = true;
        let mut complete_templates = true;
        for member in group.members.iter() {
            let mut matches = templates.iter().filter(|template| template.operation_label == member.operation_label);
            let Some(template) = matches.next() else {
                complete_templates = false;
                break;
            };
            if matches.next().is_some()
                || template.form != crate::native::features::holes::SimpleHoleForm::Simple
                || template.extent != crate::native::features::holes::SimpleHoleExtent::Through
            {
                complete_templates = false;
                break;
            }
            requests_chamfer &= template.start_treatment == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer
                && template.end_treatment == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer;
            requests_no_treatment &= template.start_treatment == crate::native::features::holes::SimpleHoleEndTreatment::None
                && template.end_treatment == crate::native::features::holes::SimpleHoleEndTreatment::None;
        }
        if !complete_templates {
            continue;
        }
        let Some(body) = group.members.first()
            .and_then(|member| outputs.get(&member.operation_label))
            .and_then(|bodies| bodies.as_slice().first().filter(|_| bodies.len() == 1))
        else {
            continue;
        };
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(group.members.len()), "NX hole package output lookup")?;
        if group.members.iter().any(|member| !matches!(outputs.get(&member.operation_label).map(Vec::as_slice), Some([candidate]) if candidate == body))
        {
            continue;
        }
        let Some(diameter) = group
            .members
            .first()
            .map(|member| &member.operation_label)
            .and_then(|operation| diameters.get(operation))
            .copied()
        else {
            continue;
        };
        if group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|operation| diameters.get(operation).copied() != Some(diameter))
        {
            continue;
        }
        if !requests_chamfer && !requests_no_treatment {
            continue;
        }
        let chamfer = if requests_chamfer {
            let Some(chamfer) = group
                .members
                .first()
                .map(|member| &member.operation_label)
                .and_then(|operation| chamfers.get(operation))
                .copied()
            else {
                continue;
            };
            if group
                .members
                .iter()
                .map(|member| &member.operation_label)
                .any(|operation| chamfers.get(operation).copied() != Some(chamfer))
            {
                continue;
            }
            Some(chamfer)
        } else {
            None
        };
        for member in group.members.iter() {
            ctx.charge_work(1, "NX hole package internal operation")?;
            if projection.internal_operations.contains(&member.operation_label) {
                continue;
            }
            ctx.charge_collection_items(1, "NX hole package internal operations")?;
            let bytes = std::mem::size_of::<String>()
                .checked_add(member.operation_label.len())
                .ok_or_else(|| ctx.refuse_codec_limit("NX hole package internal operations", 0, cadmpeg_core::decode::u64_from_index(member.operation_label.len())))?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX hole package internal operations")?;
            projection.internal_operations.insert(member.operation_label.clone());
        }
        insert_hole_output_body(ctx, &mut projection.outputs, &use_.operation_label, body)?;
        charge_hole_map_entry::<Length>(ctx, &use_.operation_label, 0, "NX hole package diameter map")?;
        projection.diameters.insert(use_.operation_label.clone(), diameter);
        if let Some(chamfer) = chamfer {
            charge_hole_map_entry::<HoleKind>(ctx, &use_.operation_label, 0, "NX hole package chamfer map")?;
            projection.chamfers.insert(use_.operation_label.clone(), chamfer);
        }
        let placements = hole_axis_placements_for_body(ctx, ir, body)?;
        if placements.len() == group.members.len() {
            charge_hole_map_entry::<Vec<HolePlacement>>(ctx, &use_.operation_label, 0, "NX hole package placement map")?;
            projection.placements.insert(use_.operation_label.clone(), placements);
        }
    }
    Ok(projection)
}

struct HoleBodyProjection {
    outputs: BTreeMap<String, Vec<BodyId>>,
    diameters: BTreeMap<String, Length>,
    blind_depths: BTreeMap<String, cadmpeg_ir::scalar::NonZeroLength>,
    counterbores: BTreeMap<String, CounterboreDimensions>,
}

fn hole_operations_are_unique(
    ctx: &DecodeContext<'_>,
    operations: &[String],
) -> Result<bool, CodecError> {
    for (index, operation) in operations.iter().enumerate() {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(index), "NX hole operation uniqueness")?;
        if operations[..index].contains(operation) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn charge_hole_map_entry<T>(
    ctx: &DecodeContext<'_>,
    key: &str,
    nested_bytes: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let bytes = std::mem::size_of::<(String, T)>()
        .checked_add(key.len())
        .and_then(|bytes| bytes.checked_add(nested_bytes))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, cadmpeg_core::decode::u64_from_index(key.len())))?;
    ctx.charge_collection_items(1, operation)?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), operation)?;
    ctx.charge_work(1, operation)
}

fn insert_hole_output_body(
    ctx: &DecodeContext<'_>,
    outputs: &mut BTreeMap<String, Vec<BodyId>>,
    operation: &str,
    body: &BodyId,
) -> Result<(), CodecError> {
    let nested_bytes = std::mem::size_of::<BodyId>()
        .checked_add(body.as_str().len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX hole output body", 0, cadmpeg_core::decode::u64_from_index(body.as_str().len())))?;
    ctx.charge_collection_items(1, "NX hole output body")?;
    charge_hole_map_entry::<Vec<BodyId>>(ctx, operation, nested_bytes, "NX hole output map")?;
    let mut bodies = Vec::new();
    reserve_attach_vec(ctx, &mut bodies, 1, "NX hole output body")?;
    bodies.push(body.clone());
    outputs.insert(operation.to_owned(), bodies);
    Ok(())
}

fn hole_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<HoleBodyProjection>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)?
    {
        return Ok(None);
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(None);
    };

    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let Some(bores) = through_bore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(None);
        };
        let Some(radius) = bores.first().map(|(_, _, radius)| *radius) else {
            return Ok(None);
        };
        if bores.len() != operations.len()
            || bores
                .iter()
                .any(|(_, _, candidate)| candidate.to_bits() != radius.to_bits())
        {
            return Ok(None);
        }
        for operation in operations {
            insert_hole_output_body(ctx, &mut projected_outputs, &operation, &body)?;
            let Some(diameter) = Length::new(radius * 2.0) else {
                return Ok(None);
            };
            charge_hole_map_entry::<Length>(ctx, &operation, 0, "NX hole diameter map")?;
            diameters.insert(operation, diameter);
        }
    }
    Ok(Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths: BTreeMap::new(),
        counterbores: BTreeMap::new(),
    }))
}

fn counterbore_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<HoleBodyProjection>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)?
    {
        return Ok(None);
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(None);
    };
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut counterbores = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            // A counterbore pair has no serialized operation-to-pair relation
            // once multiple operations share one result body. Do not assign
            // geometry to history order.
            return Ok(None);
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let Some(witnesses) = counterbore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(None);
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(None);
        };
        insert_hole_output_body(ctx, &mut projected_outputs, operation, &body)?;
        let Some(diameter) = Length::new(witness.bore_radius * 2.0) else {
            return Ok(None);
        };
        charge_hole_map_entry::<Length>(ctx, operation, 0, "NX counterbore diameter map")?;
        diameters.insert(operation.clone(), diameter);
        let (Some(diameter), Some(depth)) = (
            cadmpeg_ir::scalar::PositiveLength::new(witness.counterbore_radius * 2.0),
            cadmpeg_ir::scalar::PositiveLength::new(witness.depth),
        ) else {
            return Ok(None);
        };
        charge_hole_map_entry::<CounterboreDimensions>(ctx, operation, 0, "NX counterbore dimension map")?;
        counterbores.insert(operation.clone(), CounterboreDimensions { diameter, depth });
    }
    Ok(Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths: BTreeMap::new(),
        counterbores,
    }))
}

fn blind_hole_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<HoleBodyProjection>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)?
    {
        return Ok(None);
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(None);
    };
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut blind_depths = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return Ok(None);
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(None);
        };
        let Some(witnesses) = blind_bore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(None);
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(None);
        };
        insert_hole_output_body(ctx, &mut projected_outputs, operation, &body)?;
        let Some(diameter) = Length::new(witness.bore_radius * 2.0) else {
            return Ok(None);
        };
        charge_hole_map_entry::<Length>(ctx, operation, 0, "NX blind hole diameter map")?;
        diameters.insert(operation.clone(), diameter);
        let Some(depth) = cadmpeg_ir::scalar::NonZeroLength::new(witness.depth) else {
            return Ok(None);
        };
        charge_hole_map_entry::<cadmpeg_ir::scalar::NonZeroLength>(ctx, operation, 0, "NX blind hole depth map")?;
        blind_depths.insert(
            operation.clone(),
            depth,
        );
    }
    Ok(Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths,
        counterbores: BTreeMap::new(),
    }))
}

/// Derive one complete unoriented placement when one operation owns exactly
/// one through bore. The closest point to the model origin is invariant under
/// axial shifts of the serialized cylinder origin. Canonical axis sign makes
/// serialization deterministic but carries no drilling-direction semantics.
fn hole_axis_placements_for_operations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HolePlacement>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)?
    {
        return Ok(BTreeMap::new());
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(BTreeMap::new());
    };

    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            continue;
        };
        let mut body_placements = hole_axis_placements_for_body(ctx, ir, &body)?;
        if body_placements.len() != 1 {
            continue;
        }
        charge_hole_map_entry::<HolePlacement>(ctx, operation, 0, "NX hole placement map")?;
        placements.insert(operation.clone(), body_placements.remove(0));
    }
    Ok(placements)
}

fn counterbore_axis_placements_for_operations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HolePlacement>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)?
    {
        return Ok(BTreeMap::new());
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(BTreeMap::new());
    };
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let Some(witnesses) = counterbore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(BTreeMap::new());
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let (Some(point), Some(direction)) = (
            cadmpeg_ir::features::FinitePoint3::new(witness.line_origin),
            cadmpeg_ir::features::FeatureDirection3::new(witness.axis),
        ) else {
            return Ok(BTreeMap::new());
        };
        charge_hole_map_entry::<HolePlacement>(ctx, operation, 0, "NX counterbore placement map")?;
        placements.insert(
            operation.clone(),
            HolePlacement::Axis {
                origin: point,
                axis: direction,
            },
        );
    }
    Ok(placements)
}

fn blind_hole_axis_placements_for_operations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HolePlacement>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(ctx, operations)?
    {
        return Ok(BTreeMap::new());
    }
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, operations, outputs)? else {
        return Ok(BTreeMap::new());
    };
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let Some(witnesses) = blind_bore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(BTreeMap::new());
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let (Some(point), Some(direction)) = (
            cadmpeg_ir::features::FinitePoint3::new(witness.position),
            cadmpeg_ir::features::FeatureDirection3::new(witness.direction),
        ) else {
            return Ok(BTreeMap::new());
        };
        charge_hole_map_entry::<HolePlacement>(ctx, operation, 0, "NX blind hole placement map")?;
        placements.insert(
            operation.clone(),
            HolePlacement::Directed {
                position: point,
                direction,
            },
        );
    }
    Ok(placements)
}

fn hole_axis_placements_for_body(ctx: &DecodeContext<'_>, ir: &CadIr, body: &BodyId) -> Result<Vec<HolePlacement>, CodecError> {
    let Some(body_faces) = connected_solid_body_faces(ctx, ir, body)? else {
        return Ok(Vec::new());
    };
    let Some(bores) = through_bore_cylinders(ctx, ir, &body_faces)? else {
        return Ok(Vec::new());
    };
    let angular_tolerance = ir.tolerances.angular.get();
    let mut placements = Vec::new();
    for (origin, axis, _) in bores {
        let Some(mut axis) = FiniteVector3::new(axis).and_then(FiniteVector3::unit_nonzero) else {
            return Ok(Vec::new());
        };
        let Some(leading) = [axis.x, axis.y, axis.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)
        else {
            return Ok(Vec::new());
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
        let (Some(origin), Some(axis)) = (
            cadmpeg_ir::features::FinitePoint3::new(origin),
            cadmpeg_ir::features::FeatureDirection3::new(axis),
        ) else {
            return Ok(Vec::new());
        };
        ctx.charge_collection_items(1, "NX hole axis placements")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<HolePlacement>()), "NX hole axis placements")?;
        reserve_attach_vec(ctx, &mut placements, 1, "NX hole axis placements")?;
        placements.push(HolePlacement::Axis { origin, axis });
    }
    placements.sort_by_key(hole_placement_key);
    Ok(placements)
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

fn canonical_axis(axis: cadmpeg_ir::units::UnitVector3, angular_tolerance: f64) -> Option<Vector3> {
    let mut axis = *axis.to_unit_length_charted().as_raw();
    let leading = [axis.x, axis.y, axis.z]
        .into_iter()
        .find(|component| component.abs() > angular_tolerance)?;
    if leading < 0.0 {
        axis = Vector3::new(-axis.x, -axis.y, -axis.z);
    }
    Some(axis)
}

fn face_two_loops(face: &Face) -> Option<[&LoopId; 2]> {
    match &face.loops {
        FaceLoops::Unspecified { loops } => {
            let [first, second] = loops.as_slice() else { return None; };
            Some([first, second])
        }
        FaceLoops::Classified { outer, inner } => {
            let [second] = inner.as_slice() else { return None; };
            Some([outer, second])
        }
    }
}

fn face_one_loop(face: &Face) -> Option<&LoopId> {
    match &face.loops {
        FaceLoops::Unspecified { loops } => {
            let [only] = loops.as_slice() else { return None; };
            Some(only)
        }
        FaceLoops::Classified { outer, inner } if inner.is_empty() => Some(outer),
        FaceLoops::Classified { .. } => None,
    }
}

fn circular_loop_geometry(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    loop_id: &LoopId,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> Result<Option<(Point3, Vector3, f64)>, CodecError> {
    let coedge_scans = ir.model.coedges.len().checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit("NX circular loop coedge scan", 0, cadmpeg_core::decode::u64_from_index(ir.model.coedges.len())))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(coedge_scans), "NX circular loop coedge scan")?;
    if !ir.model.coedges.iter().any(|coedge| &coedge.owner_loop == loop_id) {
        return Ok(None);
    }
    let mut witness: Option<(Point3, Vector3, f64)> = None;
    for coedge in ir.model.coedges.iter().filter(|coedge| &coedge.owner_loop == loop_id) {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.edges.len()), "NX circular loop edge scan")?;
        let Some(curve_id) = ir.model.edges.iter().rev().find(|edge| edge.id == coedge.edge).and_then(|edge| edge.curve()) else {
            return Ok(None);
        };
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.curves.len()), "NX circular loop curve scan")?;
        let Some(curve) = ir.model.curves.iter().rev().find(|curve| &curve.id == curve_id) else {
            return Ok(None);
        };
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) =
            &curve.geometry
        else {
            return Ok(None);
        };
        let center = circle_curve.center().get();
        let axis = circle_curve.frame().axis();
        let radius = circle_curve.radius().get();
        let Some(axis) = canonical_axis(*axis, angular_tolerance) else {
            return Ok(None);
        };
        if let Some((previous_center, previous_axis, previous_radius)) = witness {
            if (radius - previous_radius).abs() > linear_tolerance
                || (1.0 - axis.dot(previous_axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - previous_center.x,
                    center.y - previous_center.y,
                    center.z - previous_center.z,
                )
                .norm()
                    > linear_tolerance
            {
                return Ok(None);
            }
        }
        witness = Some((center, axis, radius));
    }
    Ok(witness)
}

fn same_loop_edges(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    first: &LoopId,
    second: &LoopId,
) -> Result<bool, CodecError> {
    let coedge_count = ir.model.coedges.len();
    let work = coedge_count.checked_mul(coedge_count)
        .and_then(|count| count.checked_mul(2))
        .and_then(|count| coedge_count.checked_mul(4).and_then(|scans| count.checked_add(scans)))
        .ok_or_else(|| ctx.refuse_codec_limit("NX loop edge comparison", 0, cadmpeg_core::decode::u64_from_index(coedge_count)))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(work), "NX loop edge comparison")?;
    let first_has_edges = ir.model.coedges.iter().any(|coedge| &coedge.owner_loop == first);
    let second_has_edges = ir.model.coedges.iter().any(|coedge| &coedge.owner_loop == second);
    if first_has_edges != second_has_edges {
        return Ok(false);
    }
    let first_in_second = ir.model.coedges.iter().filter(|coedge| &coedge.owner_loop == first)
        .all(|coedge| ir.model.coedges.iter().any(|other| &other.owner_loop == second && other.edge == coedge.edge));
    let second_in_first = ir.model.coedges.iter().filter(|coedge| &coedge.owner_loop == second)
        .all(|coedge| ir.model.coedges.iter().any(|other| &other.owner_loop == first && other.edge == coedge.edge));
    Ok(first_in_second && second_in_first)
}

fn cylindrical_face_witnesses(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
) -> Result<Option<Vec<CylindricalFaceWitness>>, CodecError> {
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut witnesses = Vec::new();
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(body_faces.len()), "NX cylindrical face scan")?;
    for face in body_faces
        .iter()
        .copied()
        .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
    {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()), "NX cylindrical surface lookup")?;
        let Some(surface) = ir.model.surfaces.iter().rev().find(|surface| surface.id == face.surface) else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
            &surface.geometry
        else {
            continue;
        };
        let origin = cylinder_surface.origin().get();
        let axis = cylinder_surface.frame().axis();
        let radius = cylinder_surface.radius().get();
        let Some(axis) = canonical_axis(*axis, angular_tolerance) else {
            return Ok(None);
        };
        let axial_offset = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
        let line_origin = Point3::new(
            origin.x - axial_offset * axis.x,
            origin.y - axial_offset * axis.y,
            origin.z - axial_offset * axis.z,
        );
        let Some([first_loop, second_loop]) = face_two_loops(face) else {
            return Ok(None);
        };
        let mut stations = [0.0; 2];
        for (ordinal, loop_id) in [first_loop, second_loop].into_iter().enumerate() {
            let Some((center, circle_axis, circle_radius)) = circular_loop_geometry(
                ctx,
                ir,
                loop_id,
                linear_tolerance,
                angular_tolerance,
            )? else {
                return Ok(None);
            };
            if (circle_radius - radius).abs() > linear_tolerance
                || (1.0 - axis.dot(circle_axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                )
                .cross(axis)
                .norm()
                    > linear_tolerance
            {
                return Ok(None);
            }
            let station = Vector3::new(center.x, center.y, center.z).dot(axis);
            if !station.is_finite() {
                return Ok(None);
            }
            stations[ordinal] = station;
        }
        let [first, second] = stations.as_slice() else {
            return Ok(None);
        };
        if (first - second).abs() <= linear_tolerance {
            return Ok(None);
        }
        let bytes = std::mem::size_of::<CylindricalFaceWitness>()
            .checked_add(first_loop.as_str().len())
            .and_then(|bytes| bytes.checked_add(second_loop.as_str().len()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX cylindrical face witness", 0, cadmpeg_core::decode::u64_from_index(first_loop.as_str().len())))?;
        ctx.charge_collection_items(1, "NX cylindrical face witnesses")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX cylindrical face witness")?;
        reserve_attach_vec(ctx, &mut witnesses, 1, "NX cylindrical face witnesses")?;
        witnesses.push(CylindricalFaceWitness {
            line_origin,
            axis,
            radius,
            stations: [*first, *second],
            loop_ids: [first_loop.clone(), second_loop.clone()],
        });
    }
    Ok(Some(witnesses))
}

fn plane_annulus_witness(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
    small: &CylindricalFaceWitness,
    small_station_ordinal: usize,
    large: &CylindricalFaceWitness,
    large_station_ordinal: usize,
) -> Result<bool, CodecError> {
    let line_origin = small.line_origin;
    let axis = small.axis;
    let station = small.stations[small_station_ordinal];
    let inner_radius = small.radius;
    let outer_radius = large.radius;
    let inner_loop = &small.loop_ids[small_station_ordinal];
    let outer_loop = &large.loop_ids[large_station_ordinal];
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut matches = 0;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(body_faces.len()), "NX annulus face scan")?;
    for face in body_faces {
        if face.loops.len() != 2 {
            continue;
        }
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()), "NX annulus plane lookup")?;
        let Some(surface) = ir.model.surfaces.iter().rev().find(|surface| surface.id == face.surface) else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
            &surface.geometry
        else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let normal = plane_surface.frame().axis();
        let Some(normal) = canonical_axis(*normal, angular_tolerance) else {
            continue;
        };
        if (1.0 - normal.dot(axis).abs()) > angular_tolerance
            || (Vector3::new(
                origin.x - line_origin.x,
                origin.y - line_origin.y,
                origin.z - line_origin.z,
            )
            .dot(axis)
                - station)
                .abs()
                > linear_tolerance
        {
            continue;
        }
        let Some([first_loop, second_loop]) = face_two_loops(face) else {
            continue;
        };
        let mut boundaries = [(0.0, first_loop), (0.0, second_loop)];
        let mut valid = true;
        for (ordinal, loop_id) in [first_loop, second_loop].into_iter().enumerate() {
            let Some((center, circle_axis, radius)) = circular_loop_geometry(
                ctx,
                ir,
                loop_id,
                linear_tolerance,
                angular_tolerance,
            )? else {
                valid = false;
                break;
            };
            if (1.0 - circle_axis.dot(normal).abs()) > angular_tolerance
                || (Vector3::new(
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                )
                .dot(normal))
                .abs()
                    > linear_tolerance
                || Vector3::new(
                    center.x - line_origin.x,
                    center.y - line_origin.y,
                    center.z - line_origin.z,
                )
                .cross(axis)
                .norm()
                    > linear_tolerance
                || (Vector3::new(center.x, center.y, center.z).dot(axis) - station).abs()
                    > linear_tolerance
            {
                valid = false;
                break;
            }
            boundaries[ordinal] = (radius, loop_id);
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
            && same_loop_edges(ctx, ir, inner_boundary, inner_loop)?
            && same_loop_edges(ctx, ir, outer_boundary, outer_loop)?
        {
            matches += 1;
        }
    }
    Ok(matches == 1)
}

fn counterbore_cylinders(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
) -> Result<Option<Vec<CounterboreCylinderWitness>>, CodecError> {
    let Some(cylinders) = cylindrical_face_witnesses(ctx, ir, body_faces)? else {
        return Ok(None);
    };
    if cylinders.is_empty() || cylinders.len() % 2 != 0 {
        return Ok(None);
    }
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let pair_work = cylinders.len().checked_mul(cylinders.len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX counterbore pair scan", 0, cadmpeg_core::decode::u64_from_index(cylinders.len())))?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(pair_work), "NX counterbore pair scan")?;
    let mut candidates = ctx.alloc_filled(
        cylinders.len(),
        Vec::<(usize, CounterboreCylinderWitness)>::new(),
        "nx counterbore cylinder candidates",
    )?;
    for (first_index, first) in cylinders.iter().enumerate() {
        for (second_index, second) in cylinders.iter().enumerate().skip(first_index + 1) {
            let (small, large) = if first.radius < second.radius {
                (first, second)
            } else {
                (second, first)
            };
            if large.radius - small.radius <= linear_tolerance
                || (1.0 - small.axis.dot(large.axis).abs()) > angular_tolerance
                || Vector3::new(
                    large.line_origin.x - small.line_origin.x,
                    large.line_origin.y - small.line_origin.y,
                    large.line_origin.z - small.line_origin.z,
                )
                .cross(small.axis)
                .norm()
                    > linear_tolerance
            {
                continue;
            }
            let mut common = None;
            let mut multiple_common = false;
            for (small_ordinal, small_station) in small.stations.iter().enumerate() {
                for (large_ordinal, large_station) in large.stations.iter().enumerate() {
                    if (small_station - large_station).abs() <= linear_tolerance
                        && common
                            .replace((small_ordinal, large_ordinal, *small_station))
                            .is_some()
                    {
                        multiple_common = true;
                    }
                }
            }
            let Some((small_shared, large_shared, shared_station)) = common else {
                continue;
            };
            if multiple_common {
                continue;
            }
            let small_other = small.stations[1 - small_shared];
            let large_other = large.stations[1 - large_shared];
            let depth = (large_other - shared_station).abs();
            if depth <= linear_tolerance
                || (small_other - shared_station).abs() <= linear_tolerance
                || !plane_annulus_witness(ctx, ir, body_faces, small, small_shared, large, large_shared)?
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
            ctx.charge_collection_items(2, "nx counterbore candidate pair")?;
            let pair_bytes = std::mem::size_of::<(usize, CounterboreCylinderWitness)>()
                .checked_mul(2)
                .ok_or_else(|| ctx.refuse_codec_limit("nx counterbore candidate pair", 0, 2))?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(pair_bytes), "nx counterbore candidate pair")?;
            reserve_attach_vec(
                ctx,
                &mut candidates[first_index],
                1,
                "nx counterbore candidate pair",
            )?;
            reserve_attach_vec(
                ctx,
                &mut candidates[second_index],
                1,
                "nx counterbore candidate pair",
            )?;
            candidates[first_index].push((second_index, witness));
            candidates[second_index].push((first_index, witness));
        }
    }
    if candidates.iter().any(|candidates| candidates.len() != 1) {
        return Ok(None);
    }
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(cylinders.len() / 2),
        "nx counterbore cylinder witnesses",
    )?;
    let witness_bytes = (cylinders.len() / 2)
        .checked_mul(std::mem::size_of::<CounterboreCylinderWitness>())
        .ok_or_else(|| ctx.refuse_codec_limit("nx counterbore cylinder witnesses", 0, cadmpeg_core::decode::u64_from_index(cylinders.len() / 2)))?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(witness_bytes), "nx counterbore cylinder witnesses")?;
    let mut witnesses = Vec::new();
    reserve_attach_vec(
        ctx,
        &mut witnesses,
        cylinders.len() / 2,
        "nx counterbore cylinder witnesses",
    )?;
    let mut used = ctx.alloc_filled(
        cylinders.len(),
        false,
        "nx counterbore cylinder assignments",
    )?;
    for first_index in 0..cylinders.len() {
        if used[first_index] {
            continue;
        }
        let (second_index, witness) = candidates[first_index][0];
        if used[second_index]
            || candidates[second_index][0].0 != first_index
            || first_index == second_index
        {
            return Ok(None);
        }
        used[first_index] = true;
        used[second_index] = true;
        witnesses.push(witness);
    }
    Ok(Some(witnesses))
}

fn reserve_attach_vec<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let count = cadmpeg_core::decode::u64_from_index(additional);
    values
        .try_reserve_exact(additional)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, count))
}

/// Identify one blind bore from its unique planar termination. The cylinder
/// boundary and cap loop must share the exact edge identities; a radius or
/// station match alone is not a topology relation.
fn blind_bore_cylinders(ctx: &DecodeContext<'_>, ir: &CadIr, body_faces: &[&Face]) -> Result<Option<Vec<BlindBoreCylinderWitness>>, CodecError> {
    let Some(cylinders) = cylindrical_face_witnesses(ctx, ir, body_faces)? else {
        return Ok(None);
    };
    let [cylinder] = cylinders.as_slice() else {
        return Ok(None);
    };
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut cap_station = None;
    let mut cap_count = 0usize;
    for (station_ordinal, station) in cylinder.stations.iter().enumerate() {
        let cylinder_loop = &cylinder.loop_ids[station_ordinal];
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.coedges.len()), "NX blind bore cylinder edge scan")?;
        if !ir.model.coedges.iter().any(|coedge| &coedge.owner_loop == cylinder_loop) {
            return Ok(None);
        }
        for face in body_faces {
            let Some(cap_loop) = face_one_loop(face) else {
                continue;
            };
            if !same_loop_edges(ctx, ir, cap_loop, cylinder_loop)? {
                continue;
            }
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()), "NX blind bore cap surface lookup")?;
            let Some(surface) = ir.model.surfaces.iter().rev().find(|surface| surface.id == face.surface) else {
                continue;
            };
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
                &surface.geometry
            else {
                continue;
            };
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis();
            let Some(normal) = canonical_axis(*normal, angular_tolerance) else {
                continue;
            };
            let Some((center, circle_axis, circle_radius)) = circular_loop_geometry(
                ctx,
                ir,
                cap_loop,
                linear_tolerance,
                angular_tolerance,
            )? else {
                continue;
            };
            if (circle_radius - cylinder.radius).abs() > linear_tolerance
                || (1.0 - circle_axis.dot(normal).abs()) > angular_tolerance
                || (1.0 - normal.dot(cylinder.axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - cylinder.line_origin.x,
                    center.y - cylinder.line_origin.y,
                    center.z - cylinder.line_origin.z,
                )
                .cross(cylinder.axis)
                .norm()
                    > linear_tolerance
                || (Vector3::new(center.x, center.y, center.z).dot(cylinder.axis) - *station).abs()
                    > linear_tolerance
                || (Vector3::new(origin.x, origin.y, origin.z).dot(cylinder.axis) - *station).abs()
                    > linear_tolerance
            {
                continue;
            }
            cap_count = cap_count.checked_add(1).ok_or_else(|| ctx.refuse_codec_limit("NX blind bore cap count", 0, cadmpeg_core::decode::u64_from_index(cap_count)))?;
            cap_station = Some((station_ordinal, *station));
        }
    }
    let Some((cap_ordinal, cap_station)) = cap_station.filter(|_| cap_count == 1) else {
        return Ok(None);
    };
    let entry_ordinal = 1 - cap_ordinal;
    let entry_station = cylinder.stations[entry_ordinal];
    let depth = (cap_station - entry_station).abs();
    if !depth.is_finite() || depth <= linear_tolerance {
        return Ok(None);
    }
    let position = Point3::new(
        cylinder.line_origin.x + entry_station * cylinder.axis.x,
        cylinder.line_origin.y + entry_station * cylinder.axis.y,
        cylinder.line_origin.z + entry_station * cylinder.axis.z,
    );
    let direction = if cap_station > entry_station {
        cylinder.axis
    } else {
        Vector3::new(-cylinder.axis.x, -cylinder.axis.y, -cylinder.axis.z)
    };
    ctx.charge_collection_items(1, "NX blind bore witness")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<BlindBoreCylinderWitness>()), "NX blind bore witness")?;
    let mut witnesses = Vec::new();
    reserve_attach_vec(ctx, &mut witnesses, 1, "NX blind bore witness")?;
    witnesses.push(BlindBoreCylinderWitness {
        position,
        direction,
        bore_radius: cylinder.radius,
        depth,
    });
    Ok(Some(witnesses))
}

/// Resolve hole operations to their explicit output bodies, or to the one
/// connected solid when NX omits every operation-output relation. An output
/// entry with no body is an explicit unresolved relation and blocks fallback.
fn hole_operations_by_body(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<BTreeMap<BodyId, Vec<String>>>, CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(operations.len()), "NX hole output relation scan")?;
    let related = operations
        .iter()
        .filter(|operation| outputs.contains_key(*operation))
        .count();
    if related != 0 && related != operations.len() {
        return Ok(None);
    }
    if related == operations.len() {
        let mut operations_by_body = BTreeMap::<BodyId, Vec<String>>::new();
        for operation in operations {
            let Some([body]) = outputs.get(operation).map(Vec::as_slice) else {
                return Ok(None);
            };
            if !operations_by_body.contains_key(body) {
                ctx.charge_collection_items(1, "NX hole operation body groups")?;
                let bytes = std::mem::size_of::<(BodyId, Vec<String>)>()
                    .checked_add(body.as_str().len())
                    .ok_or_else(|| ctx.refuse_codec_limit("NX hole operation body groups", 0, cadmpeg_core::decode::u64_from_index(body.as_str().len())))?;
                ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX hole operation body groups")?;
                operations_by_body.insert(body.clone(), Vec::new());
            }
            let group = operations_by_body.get_mut(body).ok_or_else(|| ctx.refuse_codec_limit("NX hole operation body groups", 0, 1))?;
            ctx.charge_collection_items(1, "NX hole operations per body")?;
            let bytes = std::mem::size_of::<String>().checked_add(operation.len())
                .ok_or_else(|| ctx.refuse_codec_limit("NX hole operations per body", 0, cadmpeg_core::decode::u64_from_index(operation.len())))?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX hole operations per body")?;
            reserve_attach_vec(ctx, group, 1, "NX hole operations per body")?;
            group.push(operation.clone());
        }
        return Ok(Some(operations_by_body));
    }

    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.bodies.len()), "NX connected solid body scan")?;
    let mut selected = None;
    for body in &ir.model.bodies {
        if connected_solid_body_exists(ctx, ir, body)? {
            if selected.replace(body).is_some() {
                return Ok(None);
            }
        }
    }
    let Some(body) = selected else { return Ok(None); };
    ctx.charge_collection_items(1, "NX hole operation body groups")?;
    let map_bytes = std::mem::size_of::<(BodyId, Vec<String>)>()
        .checked_add(body.id.as_str().len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX hole operation body groups", 0, cadmpeg_core::decode::u64_from_index(body.id.as_str().len())))?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(map_bytes), "NX hole operation body groups")?;
    let mut group = Vec::new();
    for operation in operations {
        ctx.charge_collection_items(1, "NX hole operations per body")?;
        let bytes = std::mem::size_of::<String>().checked_add(operation.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX hole operations per body", 0, cadmpeg_core::decode::u64_from_index(operation.len())))?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX hole operations per body")?;
        reserve_attach_vec(ctx, &mut group, 1, "NX hole operations per body")?;
        group.push(operation.clone());
    }
    Ok(Some(BTreeMap::from([(body.id.clone(), group)])))
}

fn through_bore_cylinders(ctx: &DecodeContext<'_>, ir: &CadIr, body_faces: &[&Face]) -> Result<Option<Vec<(Point3, Vector3, f64)>>, CodecError> {
    let Some(cylinders) = cylindrical_face_witnesses(ctx, ir, body_faces)? else {
        return Ok(None);
    };
    let mut bores = Vec::new();
    ctx.charge_collection_items(cadmpeg_core::decode::u64_from_index(cylinders.len()), "NX through bore witnesses")?;
    let bytes = cylinders.len().checked_mul(std::mem::size_of::<(Point3, Vector3, f64)>())
        .ok_or_else(|| ctx.refuse_codec_limit("NX through bore witnesses", 0, cadmpeg_core::decode::u64_from_index(cylinders.len())))?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX through bore witnesses")?;
    reserve_attach_vec(ctx, &mut bores, cylinders.len(), "NX through bore witnesses")?;
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(cylinders.len()), "NX through bore projection")?;
    for witness in cylinders {
        bores.push((witness.line_origin, witness.axis, witness.radius));
    }
    Ok(Some(bores))
}

/// Derive identical entry and exit chamfer treatments only when every simple
/// through-hole bore has exactly two coaxial conical faces and every cone is
/// bounded by the bore circle and one equal larger circle.
fn simple_hole_chamfers(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HoleKind>, CodecError> {
    let mut operations = Vec::new();
    let mut operation_reservation = ctx.reserve_scoped(0, "NX chamfer selected operations")?;
    for template in templates {
        ctx.charge_work(1, "NX chamfer template scan")?;
        if !(
            template.form == crate::native::features::holes::SimpleHoleForm::Simple
                && template.extent == crate::native::features::holes::SimpleHoleExtent::Through
                && template.start_treatment
                    == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer
                && template.end_treatment
                    == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer
        ) {
            continue;
        }
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(templates.len()), "NX chamfer template identity scan")?;
        if templates.iter().filter(|candidate| candidate.operation_label == template.operation_label).count() != 1 {
            continue;
        }
        let bytes = std::mem::size_of::<String>().checked_add(template.operation_label.len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX chamfer selected operations", 0, cadmpeg_core::decode::u64_from_index(template.operation_label.len())))?;
        ctx.charge_collection_items(1, "NX chamfer selected operations")?;
        operation_reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
        reserve_attach_vec(ctx, &mut operations, 1, "NX chamfer selected operations")?;
        operations.push(template.operation_label.clone());
    }
    if operations.is_empty() {
        return Ok(BTreeMap::new());
    }
    charge_hole_sort_work(ctx, operations.len())?;
    operations.sort();
    let Some(operations_by_body) = hole_operations_by_body(ctx, ir, &operations, outputs)? else {
        return Ok(BTreeMap::new());
    };

    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut treatments = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let Some(body_faces) = connected_solid_body_faces(ctx, ir, &body)? else {
            return Ok(BTreeMap::new());
        };
        let Some(bores) = through_bore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(BTreeMap::new());
        };
        let [(_, _, bore_radius), ..] = bores.as_slice() else {
            return Ok(BTreeMap::new());
        };
        if bores.len() != operations.len()
            || bores
                .iter()
                .any(|(_, _, radius)| radius.to_bits() != bore_radius.to_bits())
        {
            return Ok(BTreeMap::new());
        }
        let cone_count_bytes = bores.len().checked_mul(std::mem::size_of::<usize>())
            .ok_or_else(|| ctx.refuse_codec_limit("NX chamfer cone counts", 0, cadmpeg_core::decode::u64_from_index(bores.len())))?;
        let _cone_count_reservation = ctx.reserve_scoped(cadmpeg_core::decode::u64_from_index(cone_count_bytes), "NX chamfer cone counts")?;
        let mut cone_counts =
            ctx.alloc_filled(bores.len(), 0usize, "nx simple-hole chamfer cone counts")?;
        let mut outer_radii = Vec::new();
        let mut included_angles = Vec::new();
        let mut geometry_reservation = ctx.reserve_scoped(0, "NX chamfer cone geometry")?;
        for face in body_faces
            .faces
            .iter()
            .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
        {
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.surfaces.len()), "NX chamfer cone surface scan")?;
            let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))) =
                ir.model.surfaces.iter().rev().find(|surface| surface.id == face.surface).map(|surface| &surface.geometry)
            else {
                continue;
            };
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let half_angle = cone_surface.half_angle().get();
            if half_angle <= 0.0 || half_angle >= std::f64::consts::FRAC_PI_2 {
                return Ok(BTreeMap::new());
            }
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(bores.len()),
                "nx chamfer bore matching",
            )?;
            let mut matching_bore = None;
            let mut multiple_bores = false;
            for (ordinal, (bore_origin, bore_axis, _)) in bores.iter().enumerate() {
                let dot = axis.dot(*bore_axis);
                if (1.0 - dot.abs()) > angular_tolerance {
                    continue;
                }
                let delta = Vector3::new(
                    origin.x - bore_origin.x,
                    origin.y - bore_origin.y,
                    origin.z - bore_origin.z,
                );
                if delta.cross(*bore_axis).norm() <= linear_tolerance
                    && matching_bore.replace(ordinal).is_some()
                {
                    multiple_bores = true;
                }
            }
            let Some(bore_ordinal) = matching_bore else {
                return Ok(BTreeMap::new());
            };
            if multiple_bores {
                return Ok(BTreeMap::new());
            }
            cone_counts[bore_ordinal] += 1;

            let mut radii = [None, None];
            let Some(loops) = face_two_loops(face) else {
                return Ok(BTreeMap::new());
            };
            let mut radius_count = 0;
            for loop_id in loops {
                ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.coedges.len()), "NX chamfer coedge scan")?;
                for coedge in ir.model.coedges.iter().filter(|coedge| coedge.owner_loop == *loop_id) {
                    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.edges.len()), "NX chamfer edge lookup")?;
                    let Some(curve_id) = ir.model.edges.iter().rev().find(|edge| edge.id == coedge.edge).and_then(|edge| edge.curve()) else {
                        continue;
                    };
                    ctx.charge_work(cadmpeg_core::decode::u64_from_index(ir.model.curves.len()), "NX chamfer curve lookup")?;
                    let Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))) = ir.model.curves.iter().rev().find(|curve| curve.id == *curve_id).map(|curve| &curve.geometry) else {
                        continue;
                    };
                    let radius = circle_curve.radius().get();
                    if radius_count == radii.len() {
                        return Ok(BTreeMap::new());
                    }
                    radii[radius_count] = Some(radius);
                    radius_count += 1;
                }
            }
            let [Some(mut inner), Some(mut outer)] = radii else {
                return Ok(BTreeMap::new());
            };
            if inner.total_cmp(&outer).is_gt() {
                std::mem::swap(&mut inner, &mut outer);
            }
            if inner.to_bits() != bore_radius.to_bits() || outer <= inner {
                return Ok(BTreeMap::new());
            }
            ctx.charge_collection_items(2, "nx chamfer cone geometry")?;
            geometry_reservation.grow(cadmpeg_core::decode::u64_from_index(2 * std::mem::size_of::<f64>()))?;
            reserve_attach_vec(ctx, &mut outer_radii, 1, "nx chamfer outer radii")?;
            reserve_attach_vec(ctx, &mut included_angles, 1, "nx chamfer included angles")?;
            outer_radii.push(outer);
            included_angles.push(half_angle * 2.0);
        }
        if cone_counts.iter().any(|count| *count != 2)
            || outer_radii.len() != bores.len() * 2
            || included_angles.len() != outer_radii.len()
        {
            return Ok(BTreeMap::new());
        }
        charge_hole_sort_work(ctx, outer_radii.len())?;
        charge_hole_sort_work(ctx, included_angles.len())?;
        outer_radii.sort_by(f64::total_cmp);
        included_angles.sort_by(f64::total_cmp);
        let (Some(&widest), Some(&narrowest), Some(&largest), Some(&smallest)) = (
            outer_radii.last(),
            outer_radii.first(),
            included_angles.last(),
            included_angles.first(),
        ) else {
            return Ok(BTreeMap::new());
        };
        if widest - narrowest > linear_tolerance || largest - smallest > angular_tolerance {
            return Ok(BTreeMap::new());
        }
        let (Some(diameter), Some(angle)) = (
            cadmpeg_ir::scalar::PositiveLength::new(
                2.0 * outer_radii.iter().sum::<f64>() / outer_radii.len() as f64,
            ),
            cadmpeg_ir::scalar::InteriorAngle::new(
                included_angles.iter().sum::<f64>() / included_angles.len() as f64,
            ),
        ) else {
            return Ok(BTreeMap::new());
        };
        let treatment = HoleKind::Chamfer { diameter, angle };
        for operation in operations {
            let bytes = std::mem::size_of::<(String, HoleKind)>().checked_add(operation.len())
                .ok_or_else(|| ctx.refuse_codec_limit("NX chamfer treatments", 0, cadmpeg_core::decode::u64_from_index(operation.len())))?;
            ctx.charge_collection_items(1, "NX chamfer treatments")?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX chamfer treatments")?;
            treatments.insert(operation, treatment);
        }
    }
    Ok(treatments)
}

fn unique_simple_hole_template(
    payload_strings: &[&str],
) -> Option<(
    crate::native::features::holes::SimpleHoleForm,
    crate::native::features::holes::SimpleHoleExtent,
    crate::native::features::holes::SimpleHoleEndTreatment,
    crate::native::features::holes::SimpleHoleEndTreatment,
)> {
    let mut candidates = payload_strings
        .iter()
        .copied()
        .filter(|value| value.starts_with("Hole_"));
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    crate::native::features::holes::parse_simple_hole_template(candidate)
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

enum FeatureBodySelection {
    Native(String),
    Local {
        bodies: Vec<String>,
        native: String,
        identity_keys: Vec<FeatureBodyIdentity>,
    },
    Resolved {
        bodies: Vec<BodyId>,
        native: String,
        identity_keys: Vec<FeatureBodyIdentity>,
    },
}

fn local_body_selection(bodies: Vec<String>, native: String) -> BodySelection {
    BodySelection::local(bodies, native.clone()).unwrap_or(BodySelection::Native(native))
}

impl FeatureBodySelection {
    fn into_selection(self) -> BodySelection {
        match self {
            Self::Native(native) => BodySelection::Native(native),
            Self::Local { bodies, native, .. } => local_body_selection(bodies, native),
            Self::Resolved { bodies, native, .. } => match bodies.try_into() {
                Ok(bodies) => BodySelection::Resolved { bodies, native },
                Err(_) => BodySelection::Native(native),
            },
        }
    }

    fn into_native(self) -> BodySelection {
        let (Self::Native(native) | Self::Local { native, .. } | Self::Resolved { native, .. }) =
            self;
        BodySelection::Native(native)
    }
}

/// Resolve a complete object-index selection only when every alias root owns one
/// decoded body image. Retain the complete feature-input-local identities when
/// current topology cannot represent a consumed historical body. An offset-store
/// selection uses the exact data-block identities from its feature-history
/// section. A complete operation-local offset-store map takes precedence over
/// a segment alias with the same integer; mixed namespace coverage remains
/// native.
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
            (Some(_), Some(data_block)) => {
                if !offset_blocks.contains(data_block) {
                    offset_blocks.push(data_block.clone());
                }
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
                return FeatureBodySelection::Native(native);
            }
        }
    }
    if !roots.is_empty() && !offset_blocks.is_empty() {
        return FeatureBodySelection::Native(native);
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
        return FeatureBodySelection::Native(native);
    }
    if !offset_blocks.is_empty() {
        return FeatureBodySelection::Local {
            bodies: offset_blocks.clone(),
            native,
            identity_keys: offset_blocks
                .into_iter()
                .map(FeatureBodyIdentity::OffsetStore)
                .collect(),
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
        return FeatureBodySelection::Resolved {
            bodies,
            native,
            identity_keys: roots
                .into_iter()
                .map(FeatureBodyIdentity::Segment)
                .collect(),
        };
    }
    FeatureBodySelection::Local {
        bodies: roots
            .iter()
            .map(|root| format!("nx:om-body-object#{root}"))
            .collect(),
        native,
        identity_keys: roots
            .into_iter()
            .map(FeatureBodyIdentity::Segment)
            .collect(),
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
        if let Ok(bodies) = bodies.try_into() {
            return BodySelection::Resolved { bodies, native };
        }
    }
    local_body_selection(
        roots
            .iter()
            .map(|root| format!("nx:om-body-object#{root}"))
            .collect(),
        native,
    )
}

fn atomic_disjoint_body_selections(
    left: FeatureBodySelection,
    right: FeatureBodySelection,
) -> (BodySelection, BodySelection) {
    let complete = match (&left, &right) {
        (
            FeatureBodySelection::Local {
                identity_keys: left,
                ..
            }
            | FeatureBodySelection::Resolved {
                identity_keys: left,
                ..
            },
            FeatureBodySelection::Local {
                identity_keys: right,
                ..
            }
            | FeatureBodySelection::Resolved {
                identity_keys: right,
                ..
            },
        ) => {
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
        }
        _ => false,
    };
    if complete {
        (left.into_selection(), right.into_selection())
    } else {
        (left.into_native(), right.into_native())
    }
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
    if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = definition {
        if let BodySelection::Local { bodies, .. } = operands.target() {
            if let [body] = bodies.as_slice() {
                if offset_store_identity(body).is_some() {
                    return (None, Some(body.as_str()));
                }
            }
        }
    }
    (Some(native_body), None)
}

fn boolean_target_output(definition: Option<&FeatureDefinition>) -> Option<BodyId> {
    let Some(FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })) = definition
    else {
        return None;
    };
    let BodySelection::Resolved { bodies, .. } = operands.target() else {
        return None;
    };
    bodies.first().cloned()
}

pub(super) fn boolean_feature_definition(
    operation: &crate::native::features::FeatureBooleanOperation,
    body_alias_roots: &BTreeMap<u32, u32>,
    offset_store_resolution: &BooleanOffsetStoreResolution,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<FeatureDefinition, CodecError> {
    let empty_offset_store_body_blocks = BTreeMap::new();
    let native_target = format!("nx:om-object-index#{}", operation.target.token.value());
    let native_tools = format!(
        "nx:om-object-indices#{}",
        operation
            .tools
            .iter()
            .map(|token| token.token.value().to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
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
        Some(offset_store_body_blocks) => atomic_disjoint_body_selections(
            feature_body_selection_with_offset_blocks(
                &[operation.target.token.value()],
                body_alias_roots,
                offset_store_body_blocks,
                bodies_by_object_index,
                native_target.clone(),
            ),
            feature_body_selection_with_offset_blocks(
                &operation
                    .tools
                    .iter()
                    .map(|token| token.token.value())
                    .collect::<Vec<_>>(),
                body_alias_roots,
                offset_store_body_blocks,
                bodies_by_object_index,
                native_tools.clone(),
            ),
        ),
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Combine {
        operands: cadmpeg_ir::features::CombineOperands::new(target, tools)
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
    field: DeleteBodyField<'_>,
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> FeatureDefinition {
    let bodies = match field {
        DeleteBodyField::Native(body) => match feature_body_selection(
            &[body],
            body_alias_roots,
            bodies_by_object_index,
            format!("nx:om-object-index#{body}"),
        ) {
            FeatureBodySelection::Native(native) => {
                local_body_selection(vec![format!("nx:om-body-object#{body}")], native)
            }
            selection => selection.into_selection(),
        },
        DeleteBodyField::OffsetStore {
            object_index,
            data_block,
        } => local_body_selection(
            vec![data_block.to_string()],
            format!("nx:om-object-index#{object_index}"),
        ),
    };
    FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        // A typed DELETE primary-body field names one exact feature input. It
        // needs no cross-selection alias proof when it has no segment binding.
        bodies,
        mode: BodyRetentionMode::DeleteSelected,
    })
}

/// Project the exact source body of an `EXTRACT_BODY` operation.
fn extract_body_feature_definition(
    body_object_index: Option<u32>,
    offset_store_bodies: &[(u32, String)],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> FeatureDefinition {
    let source = body_object_index.map_or_else(
        || match offset_store_bodies {
            [(object_index, data_block)] => local_body_selection(
                vec![data_block.clone()],
                format!("nx:om-object-index#{object_index}"),
            ),
            _ => BodySelection::Unresolved,
        },
        |body| {
            feature_body_selection(
                &[body],
                body_alias_roots,
                bodies_by_object_index,
                format!("nx:om-object-index#{body}"),
            )
            .into_selection()
        },
    );
    FeatureDefinition::Operation(FeatureOperation::ExtractBody { source })
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
    let primary_store = data_block.rsplit_once(":block#").map(|(store, _)| store);
    let tool_data_blocks = operands
        .iter()
        .map(|operand| operand.operand_data_block.clone())
        .collect::<Option<Vec<_>>>();
    let tools = match (operands.is_empty(), primary_store, tool_data_blocks) {
        (true, _, _) | (_, None, _) | (_, _, None) => BodySelection::Unresolved,
        (false, Some(primary_store), Some(tool_data_blocks)) => {
            let mut operand_indices = BTreeSet::new();
            let distinct_operand_indices = operands
                .iter()
                .all(|operand| operand_indices.insert(operand.operand.atom.value()));
            let same_store = tool_data_blocks.iter().all(|tool_data_block| {
                tool_data_block
                    .rsplit_once(":block#")
                    .is_some_and(|(store, _)| store == primary_store)
            });
            let distinct_tool_blocks =
                tool_data_blocks.iter().collect::<BTreeSet<_>>().len() == tool_data_blocks.len();
            let no_target_alias = tool_data_blocks
                .iter()
                .all(|tool_data_block| tool_data_block != data_block);
            if operands.iter().all(|operand| {
                operand.body_object_index == *object_index
                    && operand.operand.atom.value() != *object_index
            }) && distinct_operand_indices
                && same_store
                && distinct_tool_blocks
                && no_target_alias
            {
                local_body_selection(
                    tool_data_blocks,
                    format!(
                        "nx:om-object-indices#{}",
                        operands
                            .iter()
                            .map(|operand| operand.operand.atom.value().to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                )
            } else {
                BodySelection::Unresolved
            }
        }
    };
    Some(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
        operands: cadmpeg_ir::features::TrimBodyOperands::new(
            local_body_selection(
                vec![data_block.clone()],
                format!("nx:om-object-index#{object_index}"),
            ),
            tools,
        )
        .ok()?,

        keep: BodyTrimSide::Unresolved,
    }))
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
        .chain(operands.iter().map(|operand| operand.operand.atom.value()))
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
            local_body_selection(
                std::iter::once(primary_data_block.to_string())
                    .chain(blocks.iter().map(|block| (*block).to_string()))
                    .collect(),
                native,
            )
        } else {
            BodySelection::Native(native.clone())
        }
    } else {
        BodySelection::Native(native)
    };
    Some(FeatureDefinition::Operation(FeatureOperation::SewBodies {
        bodies: (bodies).try_into().ok()?,
        gap_tolerance: None,
    }))
}

fn trim_body_feature_definition(
    target_object_index: u32,
    operands: &[&crate::native::features::FeatureOperationBodyOperand],
    body_alias_roots: &BTreeMap<u32, u32>,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<FeatureDefinition, CodecError> {
    let native_target = format!("nx:om-object-index#{target_object_index}");
    if operands.is_empty() {
        return Ok(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                feature_body_selection(
                    &[target_object_index],
                    body_alias_roots,
                    bodies_by_object_index,
                    native_target,
                )
                .into_selection(),
                BodySelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            keep: BodyTrimSide::Unresolved,
        }));
    }
    let tool_object_indices = operands
        .iter()
        .map(|operand| operand.operand.atom.value())
        .collect::<Vec<_>>();
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
        return Ok(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Native(native_target),
                BodySelection::Native(native_tools),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            keep: BodyTrimSide::Unresolved,
        }));
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
    Ok(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
        operands: cadmpeg_ir::features::TrimBodyOperands::new(targets, tools)
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
    let bytes = std::mem::size_of::<BodyId>().checked_add(body.as_str().len())
        .ok_or_else(|| ctx.refuse_codec_limit("NX feature body output", 0, cadmpeg_core::decode::u64_from_index(body.as_str().len())))?;
    ctx.charge_collection_items(1, "NX feature body output")?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX feature body output")?;
    let mut outputs = Vec::new();
    reserve_attach_vec(ctx, &mut outputs, 1, "NX feature body output")?;
    outputs.push(body.clone());
    Ok(outputs)
}

fn operation_body_image_outputs_by_write<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    uses: &'a [crate::native::features::FeatureOperationBodyImageSegmentUse],
    bodies_by_segment_binding: &BTreeMap<&str, Vec<BodyId>>,
) -> Result<(BTreeMap<&'a str, BodyId>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    let mut unique_uses = BTreeMap::new();
    let mut reservation = ctx.reserve_scoped(0, "NX body image output indexes")?;
    for use_ in uses {
        ctx.charge_work(1, "NX body image unique-use index")?;
        match unique_uses.entry(use_.operation_body_write.as_str()) {
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "NX body image unique-use index")?;
                reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(&str, Option<&crate::native::features::FeatureOperationBodyImageSegmentUse>)>()))?;
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
        insert_scoped_body_output(ctx, &mut reservation, &mut outputs, write, body, "NX body image outputs")?;
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
        match outputs.entry(write) {
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "NX merged body output")?;
                let bytes = std::mem::size_of::<(&str, BodyId)>().checked_add(body.as_str().len())
                    .ok_or_else(|| ctx.refuse_codec_limit("NX merged body output", 0, cadmpeg_core::decode::u64_from_index(body.as_str().len())))?;
                reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
                entry.insert(body.clone());
            }
            Entry::Occupied(entry) if entry.get() == body => {}
            Entry::Occupied(entry) => {
                entry.remove();
                ctx.charge_collection_items(1, "NX conflicting body output")?;
                reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<&str>()))?;
                conflicts.insert(write);
            }
        }
    }
    Ok(())
}

fn operation_body_identity_outputs_by_write<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    uses: &'a [crate::native::features::FeatureOperationBodyIdentitySegmentUse],
    bodies_by_segment_binding: &BTreeMap<&str, Vec<BodyId>>,
) -> Result<(BTreeMap<&'a str, BodyId>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
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
        match outputs.entry(use_.operation_body_write.as_str()) {
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "NX body identity output index")?;
                let bytes = std::mem::size_of::<(&str, BodyId)>().checked_add(body.as_str().len())
                    .ok_or_else(|| ctx.refuse_codec_limit("NX body identity output index", 0, cadmpeg_core::decode::u64_from_index(body.as_str().len())))?;
                reservation.grow(cadmpeg_core::decode::u64_from_index(bytes))?;
                entry.insert(body.clone());
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
) -> Result<(BTreeMap<&'a str, BodyId>, cadmpeg_core::decode::ScopedReservation<'ctx>), CodecError> {
    let mut partitions_by_identity = BTreeMap::<u8, Option<u32>>::new();
    let mut reservation = ctx.reserve_scoped(0, "NX body partition output indexes")?;
    for use_ in uses {
        ctx.charge_work(1, "NX body partition identity index")?;
        match partitions_by_identity.entry(use_.body_identity) {
            Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "NX body partition identity index")?;
                reservation.grow(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(u8, Option<u32>)>()))?;
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
        let (prefix, prefix_len) = stream_prefix(partition, true);
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(bodies.len()), "NX body partition prefix scan")?;
        let mut matches = bodies.iter().filter(|body| body.id.as_str().as_bytes().starts_with(&prefix[..prefix_len]));
        let Some(body) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        insert_scoped_body_output(ctx, &mut reservation, &mut unique_bodies, identity, &body.id, "NX unique partition body")?;
    }
    let mut outputs = BTreeMap::new();
    for write in writes {
        ctx.charge_work(1, "NX partition body write lookup")?;
        if let Some(body) = unique_bodies.get(&write.frame.body_identity()) {
            insert_scoped_body_output(ctx, &mut reservation, &mut outputs, write.id.as_str(), body, "NX partition body output")?;
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
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(outputs.len()), "NX complete body image output uniqueness")?;
        if outputs.contains(body) {
            return Ok(Vec::new());
        }
        let bytes = std::mem::size_of::<BodyId>().checked_add(body.as_str().len())
            .ok_or_else(|| ctx.refuse_codec_limit("NX complete body image output", 0, cadmpeg_core::decode::u64_from_index(body.as_str().len())))?;
        ctx.charge_collection_items(1, "NX complete body image output")?;
        ctx.charge_retained(cadmpeg_core::decode::u64_from_index(bytes), "NX complete body image output")?;
        reserve_attach_vec(ctx, &mut outputs, 1, "NX complete body image output")?;
        outputs.push(body.clone());
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

pub(super) fn attach_expression_parameters(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    expressions: &[crate::native::om::Expression],
    declarations: &[crate::native::om::ExpressionDeclaration],
    parameter_uses: &[crate::native::features::FeatureParameterUse],
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let declarations = declarations
        .iter()
        .map(|declaration| (declaration.id.as_str(), declaration))
        .collect::<BTreeMap<_, _>>();
    let mut tables = BTreeMap::<String, Vec<&crate::native::om::Expression>>::new();
    for expression in expressions {
        let table = expression.source_table.as_str();
        tables
            .entry(table.to_string())
            .or_default()
            .push(expression);
    }
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
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
                .bindings
                .first()
                .map(|binding| binding.source_offset)
                .cmp(&second.bindings.first().map(|binding| binding.source_offset))
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
        let feature_id: FeatureId = match table.split_once(":expression-table#") {
            None => IdScope::of(&table)
                .map(|scope| {
                    scope.id(
                        &cadmpeg_ir::identity_component!("feature"),
                        cadmpeg_ir::identity_key!("equations"),
                    )
                })
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "NX expression table id is not an NX identity"
                    ))
                })?,
            Some((scope, key)) => {
                let key = cadmpeg_ir::ids::IdentityKey::try_new(key).map_err(|error| {
                    cadmpeg_core::CodecError::malformed(format_args!(
                        "NX expression table key is not identity key text: {error}"
                    ))
                })?;
                IdScope::of(scope)
                    .map(|scope| {
                        scope.id(
                            &cadmpeg_ir::identity_component!("feature"),
                            cadmpeg_ir::identity_key!("equations-").then(key),
                        )
                    })
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(format_args!(
                            "NX expression table id is not an NX identity"
                        ))
                    })?
            }
        };
        let first_offset = expressions
            .iter()
            .map(|expression| expression.source_offset)
            .min()
            .unwrap_or(0);
        annotations
            .note(&feature_id, &stream, first_offset)
            .tag("hostglobalvariables");
        annotations.exactness(&feature_id, Exactness::Derived);
        let source_content = expressions
            .iter()
            .filter_map(|expression| {
                expression_parameter_id(&expression.id).map(FeatureSourceContent::Parameter)
            })
            .collect::<Vec<_>>();
        let source_content = cadmpeg_ir::features::FeatureContent::try_from(source_content)
            .map_err(|message| CodecError::Malformed(message.into()))?;
        if !source_content.is_empty() {
            annotations
                .derived(&feature_id, "source_content")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: base_ordinal + table_ordinal as u64,
            name: Some("NX expressions".to_string()),
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("hostglobalvariables".to_string()),
            source_text: None,
            source_content,

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: FeatureTreeNodeRole::Equations,
                    children: TreeChildren::default(),
                }),
            ),
            native_ref: None,
        });
        let mut parameter_ids =
            BTreeMap::<(&str, crate::native::om::ExpressionUnit), Vec<ParameterId>>::new();
        for expression in &expressions {
            let Some(id) = expression_parameter_id(&expression.id) else {
                continue;
            };
            parameter_ids
                .entry((expression.name.as_str(), expression.unit.clone()))
                .or_default()
                .push(id);
        }
        for (ordinal, expression) in expressions.into_iter().enumerate() {
            let Some(id) = expression_parameter_id(&expression.id) else {
                continue;
            };
            annotations
                .note(id.as_str(), &stream, expression.source_offset)
                .tag("Number");
            annotations
                .derived(id.as_str(), "owner")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            annotations
                .derived(id.as_str(), "ordinal")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            annotations
                .derived(id.as_str(), "value")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            annotations
                .derived(id.as_str(), "native_ref")
                .map_err(cadmpeg_core::CodecError::malformed)?;
            let dependencies = if dependency_ordered_expressions.contains(&expression.id) {
                let mut seen_dependencies = BTreeSet::new();
                crate::native::om::expression_parameter_names(&expression.expression)
                    .into_iter()
                    .filter_map(|name| {
                        let candidates = parameter_ids.get(&(name, expression.unit.clone()))?;
                        (candidates.len() == 1).then(|| candidates[0].clone())
                    })
                    .filter(|dependency| seen_dependencies.insert(dependency.clone()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            if !dependencies.is_empty() {
                annotations
                    .derived(id.as_str(), "dependencies")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            let value = expression.value.and_then(|value| match &expression.unit {
                crate::native::om::ExpressionUnit::Millimeter => {
                    Some(ParameterValue::Length(Length::from_assigned_real(value)))
                }
                crate::native::om::ExpressionUnit::Inch => {
                    crate::native::om::expression_length_in_millimeters(
                        &expression.unit,
                        value.get(),
                    )
                    .and_then(Length::new)
                    .map(ParameterValue::Length)
                }
                crate::native::om::ExpressionUnit::Degree => {
                    Some(ParameterValue::Angle(Angle::new(value.get().to_radians())?))
                }
                crate::native::om::ExpressionUnit::Native(_) => None,
            });
            let mut properties = BTreeMap::new();
            properties.insert("unit".to_string(), expression.unit.property_name(ctx)?);
            annotations
                .derived(id.as_str(), "properties")
                .map_err(cadmpeg_core::CodecError::malformed)?;
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
                annotations
                    .derived(id.as_str(), "properties")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
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
                annotations
                    .derived(id.as_str(), "properties")
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            ir.model.parameters.push(DesignParameter {
                id,
                owner: Some(feature_id.clone()),
                ordinal: ordinal as u32,
                name: expression.name.as_str().to_string(),
                expression: expression.expression.clone(),
                display: None,
                value,
                dependencies: dependencies.into_iter().collect(),
                properties: cadmpeg_core::text::named_entries(&expression.id, properties)?,
                pmi: None,
                native_ref: Some(expression.id.clone()),
            });
        }
    }
    Ok(())
}

fn order_expression_dependencies(
    expressions: &mut Vec<&crate::native::om::Expression>,
) -> BTreeSet<String> {
    let mut indices_by_name =
        BTreeMap::<(&str, crate::native::om::ExpressionUnit), Vec<usize>>::new();
    for (index, expression) in expressions.iter().enumerate() {
        indices_by_name
            .entry((expression.name.as_str(), expression.unit.clone()))
            .or_default()
            .push(index);
    }
    let dependencies = expressions
        .iter()
        .map(|expression| {
            crate::native::om::expression_parameter_names(&expression.expression)
                .into_iter()
                .filter_map(|name| {
                    let [index] = indices_by_name
                        .get(&(name, expression.unit.clone()))?
                        .as_slice()
                    else {
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
) -> Result<(), cadmpeg_core::CodecError> {
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
        for (ordinal, dimension) in dimension_set.dimensions.iter().enumerate() {
            let Some(parameter_id) = expression_parameter_id(&dimension.expression) else {
                continue;
            };
            let Some(parameter) = parameters.get_mut(&parameter_id) else {
                continue;
            };
            parameter.properties.insert(
                cadmpeg_core::nonblank_literal!("block_dimension.{ordinal}"),
                dimension_set.id.clone(),
            );
            if !parameter
                .properties
                .values()
                .any(|value| value == &consumer)
            {
                // One more candidate than the map holds entries, so one of
                // them is free; the `else` states that rather than asserting it.
                let Some(consumer_ordinal) = (0..=parameter.properties.len()).find(|candidate| {
                    !parameter
                        .properties
                        .contains_key(format!("consumer.{candidate}").as_str())
                }) else {
                    return Err(cadmpeg_core::CodecError::malformed(format_args!(
                        "NX parameter properties hold no free consumer ordinal"
                    )));
                };
                parameter.properties.insert(
                    cadmpeg_core::nonblank_literal!("consumer.{consumer_ordinal}"),
                    consumer.clone(),
                );
            }
            annotations
                .derived(parameter.id.as_str(), "properties")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
    }
    Ok(())
}

fn expression_parameter_id(expression_id: &str) -> Option<ParameterId> {
    let (section, key) = expression_id.split_once(":expression#")?;
    IdScope::of(section)?.try_id(&cadmpeg_ir::identity_component!("parameter"), key)
}

#[cfg(test)]
mod tests;
