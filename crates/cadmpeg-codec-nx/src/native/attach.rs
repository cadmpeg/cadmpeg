// SPDX-License-Identifier: Apache-2.0
//! IR-writing attachment of the native object model.

use crate::grouping::group_pairs;
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
    counterbore_operations, feature_source_content, hole_axis_placements_for_operations,
    hole_body_projection, hole_package_projection, native_feature_parameters, new_body_boolean_op,
    non_boolean_feature_definition_with_parameters, non_modeling_history_definition,
    offset_surface_feature_definition, primary_hole_outputs, selection_indices_native,
    simple_hole_chamfers, simple_hole_native_properties, simple_hole_operations,
    sphere_body_projection, thicken_feature_definition, HolePackageSources, HoleProjection,
    NewBodyEvidence, NxBlendFamily,
};

const MIN_LINEAR_TOLERANCE: f64 = 1.0e-9;
const MIN_ANGULAR_TOLERANCE: f64 = 1.0e-12;

fn push_native_unknown(
    ctx: &DecodeContext<'_>,
    unknowns: &mut Vec<UnknownRecord>,
    record: UnknownRecord,
) -> Result<(), CodecError> {
    ctx.charge_entities(1, "NX native unknown record")?;

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
        annotations.note(&id, &annotation_stream, offset, Some(content.label()));
        annotations.exactness(&id, Exactness::ByteExact);
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "retain NX opaque container payload",
        )?;
        push_native_unknown(
            ctx,
            unknowns,
            UnknownRecord::retained(id, offset, bytes.to_vec(), Vec::new()),
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
    let annotation_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    let object_sections = scan.container.indexed_om_sections()?;
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
                    annotations.note(&id, &annotation_stream, offset, Some("OM_ENTITY_RECORD"));
                    annotations.exactness(&id, Exactness::ByteExact);
                    ctx.retain_bytes(
                        cadmpeg_core::decode::u64_from_index(record.bytes.len()),
                        "retain NX indexed object-model record",
                    )?;
                    push_native_unknown(
                        ctx,
                        unknowns,
                        UnknownRecord::retained(id, offset, record.bytes.to_vec(), Vec::new()),
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
                    annotations.note(&id, &annotation_stream, offset, Some("OM_DATA_BLOCK"));
                    annotations.exactness(&id, Exactness::ByteExact);
                    ctx.retain_bytes(
                        cadmpeg_core::decode::u64_from_index(record.bytes.len()),
                        "retain NX indexed object-model record",
                    )?;
                    push_native_unknown(
                        ctx,
                        unknowns,
                        UnknownRecord::retained(id, offset, record.bytes.to_vec(), Vec::new()),
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Attach the native object model completely or not at all.
///
/// A resource refusal propagates. Any other failure restores every value
/// [`attach_records`] wrote and reports one loss that names the failure.
pub(super) fn attach(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let snapshot = AttachSnapshot::capture(ir, annotations, unknowns, losses);
    match attach_records(ctx, ir, model, scan, annotations, unknowns, losses) {
        Ok(()) => Ok(()),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            snapshot.restore(ir, annotations, unknowns, losses);
            losses.push(NxLossCode::NativeAttachmentFailed.note(format!(
                "Native object-model attachment failed: {error}. Decoded geometry and \
                 topology are kept. None of the attachment's features, sketches, parameters, \
                 configurations, appearances, attributes, display tessellations, native \
                 records, or container passthrough records are kept."
            )));
            Ok(())
        }
    }
}

/// Geometry arenas moved out of a model while the rest is copied.
///
/// [`attach_records`] reads these arenas and writes only face colors.
struct GeometryArenas {
    bodies: Vec<cadmpeg_ir::topology::Body>,
    regions: Vec<cadmpeg_ir::topology::Region>,
    shells: Vec<cadmpeg_ir::topology::Shell>,
    faces: Vec<cadmpeg_ir::topology::Face>,
    loops: Vec<cadmpeg_ir::topology::Loop>,
    coedges: Vec<cadmpeg_ir::topology::Coedge>,
    edges: Vec<cadmpeg_ir::topology::Edge>,
    vertices: Vec<cadmpeg_ir::topology::Vertex>,
    points: Vec<cadmpeg_ir::topology::Point>,
    surfaces: Vec<cadmpeg_ir::geometry::Surface>,
    curves: Vec<cadmpeg_ir::geometry::Curve>,
    pcurves: Vec<cadmpeg_ir::geometry::pcurve::Pcurve>,
    procedural_surfaces: Vec<cadmpeg_ir::geometry::ProceduralSurface>,
    procedural_curves: Vec<cadmpeg_ir::geometry::ProceduralCurve>,
}

impl GeometryArenas {
    fn take(model: &mut cadmpeg_ir::document::Model) -> Self {
        Self {
            bodies: std::mem::take(&mut model.bodies),
            regions: std::mem::take(&mut model.regions),
            shells: std::mem::take(&mut model.shells),
            faces: std::mem::take(&mut model.faces),
            loops: std::mem::take(&mut model.loops),
            coedges: std::mem::take(&mut model.coedges),
            edges: std::mem::take(&mut model.edges),
            vertices: std::mem::take(&mut model.vertices),
            points: std::mem::take(&mut model.points),
            surfaces: std::mem::take(&mut model.surfaces),
            curves: std::mem::take(&mut model.curves),
            pcurves: std::mem::take(&mut model.pcurves),
            procedural_surfaces: std::mem::take(&mut model.procedural_surfaces),
            procedural_curves: std::mem::take(&mut model.procedural_curves),
        }
    }

    fn put(self, model: &mut cadmpeg_ir::document::Model) {
        model.bodies = self.bodies;
        model.regions = self.regions;
        model.shells = self.shells;
        model.faces = self.faces;
        model.loops = self.loops;
        model.coedges = self.coedges;
        model.edges = self.edges;
        model.vertices = self.vertices;
        model.points = self.points;
        model.surfaces = self.surfaces;
        model.curves = self.curves;
        model.pcurves = self.pcurves;
        model.procedural_surfaces = self.procedural_surfaces;
        model.procedural_curves = self.procedural_curves;
    }
}

/// Every value [`attach_records`] can write, as it was before the call.
///
/// The document is copied without its geometry arenas, so a write to any
/// other arena, the native namespaces or the source metadata is undone by
/// replacement. Unknown records and losses are only appended to.
struct AttachSnapshot {
    document: CadIr,
    face_colors: Vec<Option<Color>>,
    annotations: AnnotationBuilder,
    unknowns: usize,
    losses: usize,
}

impl AttachSnapshot {
    fn capture(
        ir: &mut CadIr,
        annotations: &AnnotationBuilder,
        unknowns: &[UnknownRecord],
        losses: &[LossNote],
    ) -> Self {
        let geometry = GeometryArenas::take(&mut ir.model);
        let document = ir.clone();
        geometry.put(&mut ir.model);
        Self {
            document,
            face_colors: ir.model.faces.iter().map(|face| face.color).collect(),
            annotations: annotations.clone(),
            unknowns: unknowns.len(),
            losses: losses.len(),
        }
    }

    fn restore(
        self,
        ir: &mut CadIr,
        annotations: &mut AnnotationBuilder,
        unknowns: &mut Vec<UnknownRecord>,
        losses: &mut Vec<LossNote>,
    ) {
        let geometry = GeometryArenas::take(&mut ir.model);
        *ir = self.document;
        geometry.put(&mut ir.model);
        for (face, color) in ir.model.faces.iter_mut().zip(self.face_colors) {
            face.color = color;
        }
        *annotations = self.annotations;
        unknowns.truncate(self.unknowns);
        losses.truncate(self.losses);
    }
}

fn attach_records(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    #[cfg(test)]
    super::attach_failure::check(super::attach_failure::Point::Entry)?;
    attach_container_payloads(ctx, ir, scan, annotations, unknowns, TypedNative::Available)?;
    for stream in &model.parasolid.attribute_definition_scan_bounded_streams {
        losses.push(NxLossCode::SourceScanBounded.note(format!(
            "The attribute-definition scan of Parasolid stream #{stream} reached its work bound; \
             the stream's attribute definitions were skipped."
        )));
    }
    for section in &model.om.object_record_identity_bounded_sections {
        losses.push(NxLossCode::ObjectRecordIdentityBounded.note(format!(
            "The identity graph of indexed OM entry {} reached its work bound of {} bytes; \
             {} of its object records were kept without a stable identity.",
            section.source_entry,
            crate::native::om::MAX_STABLE_GRAPH_WORK,
            section.records
        )));
    }
    let has_object_sections = !scan.container.indexed_om_sections()?.is_empty();
    let annotation_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    if model.is_empty() && !has_object_sections {
        return Ok(());
    }
    attach_rm_face_colors(ir, model, scan, annotations)?;
    attach_rm_appearances(ir, model, scan, annotations)?;
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
        losses,
    )?;
    for (tessellation, source_offset) in display_jt_tessellations {
        annotations.note(
            tessellation.id.as_str(),
            &annotation_stream,
            source_offset,
            Some("DISPLAY_JT_TESSELLATION"),
        );
        annotations.exactness(tessellation.id.as_str(), Exactness::Derived);
        ir.model.tessellations.push(tessellation);
    }
    NATIVE_CATALOGUE.note_phase(NotePhase::GroupA, model, annotations);
    attach_material_texture_assets(
        ctx,
        ir,
        &model.om.material_texture_assets,
        scan,
        annotations,
    )?;
    attach_part_attributes(
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
        ir,
        &model.parasolid.topology_attribute_list_references,
        &model.parasolid.topology_attribute_class_uses,
        &model.parasolid.attribute_definitions,
        &model.parasolid.attribute_field_uses,
        &model.parasolid.attribute_field_names,
    );
    attach_parasolid_topology_string_attributes(
        ir,
        &ParasolidStringAttributeSources {
            string_uses: &model.parasolid.entity_51_string_uses,
            strings: &model.parasolid.entity_54_string_records,
        },
        &topology_attribute_index,
        annotations,
    )?;
    attach_parasolid_topology_numeric_attributes(
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
    NATIVE_CATALOGUE.note_phase(NotePhase::GroupB, model, annotations);
    attach_indexed_om_unknowns(ctx, scan, annotations, unknowns)?;
    attach_configurations(
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
    attach_active_configuration_parameter_values(ir, annotations)?;
    attach_feature_operations(ctx, ir, model, annotations, losses)?;
    expressions::attach_block_dimension_parameter_consumers(
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
    #[cfg(test)]
    super::attach_failure::check(super::attach_failure::Point::Exit)?;
    Ok(())
}

fn attach_part_attributes<'a>(
    ir: &mut CadIr,
    attributes: impl IntoIterator<Item = (&'a str, &'a str, &'a str, u64)>,
    annotations: &mut AnnotationBuilder,
    annotation_stream: &StreamHandle,
) -> Result<(), CodecError> {
    for (attribute_id, attribute_title, attribute_value, source_offset) in attributes {
        annotations.note(
            attribute_id,
            annotation_stream,
            source_offset,
            Some("Attribute"),
        );
        annotations.exactness(attribute_id, Exactness::ByteExact);
        let id: AttributeId = extended_id(attribute_id, &cadmpeg_ir::identity_key!("neutral"))
            .ok_or_else(|| {
                CodecError::malformed(format_args!("NX part attribute id is not an identity"))
            })?;
        annotations.note(
            id.as_str(),
            annotation_stream,
            source_offset,
            Some("Attribute"),
        );
        annotations
            .derived(id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(id.as_str(), "name")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(id.as_str(), "values")
            .map_err(cadmpeg_core::CodecError::from)?;

        ir.model.attributes.push(SourceAttribute {
            id,
            target: AttributeTarget::Document,
            name: attribute_title.to_string(),
            values: vec![AttributeValue::String(attribute_value.to_string())],
        });
    }
    Ok(())
}

fn attach_configurations<'a>(
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
            CodecError::malformed(format_args!(
                "NX configuration ordinal {ordinal} exceeds u32"
            ))
        })?;
        let bodies = if active_attribute_use.is_some() {
            let mut selected = Vec::new();

            for body in &ir.model.bodies {
                selected.push(body.id.clone());
            }
            Some(
                cadmpeg_ir::features::DistinctMembers::try_from(selected)
                    .map_err(cadmpeg_core::CodecError::from)?,
            )
        } else {
            None
        };
        annotations.note(
            id.as_str(),
            annotation_stream,
            source_offset,
            Some("Arrangement"),
        );
        annotations
            .derived(id.as_str(), "ordinal")
            .map_err(cadmpeg_core::CodecError::from)?;
        if active_attribute_use.is_some() {
            annotations
                .derived(id.as_str(), "active")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        annotations
            .derived(id.as_str(), "source_index")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(id.as_str(), "name")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(id.as_str(), "native_ref")
            .map_err(cadmpeg_core::CodecError::from)?;
        if bodies
            .as_deref()
            .is_some_and(|bodies: &[_]| !bodies.is_empty())
        {
            annotations
                .derived(id.as_str(), "bodies")
                .map_err(cadmpeg_core::CodecError::from)?;
        }

        let mut properties = BTreeMap::new();
        if let Some(relation) = active_attribute_use {
            properties.insert(
                cadmpeg_core::nonblank_literal!("active_attribute_use"),
                relation.to_string(),
            );
        }

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
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let face_ids = collect_rm_face_ids(ir.model.faces.iter().map(|face| face.id.as_str()));
    let bindings = resolve_rm_face_colors(
        &face_ids,
        &model.om.rm_display_color_assignments,
        &model.om.part_color_definitions,
        &model.parasolid.deltas_records,
        &super::substrate::paired_delta_streams(scan),
    )?;
    for (face_id, color) in bindings {
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
                .derived(&face.id, "color")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    Ok(())
}

fn collect_rm_face_ids<'b>(faces: impl IntoIterator<Item = &'b str>) -> BTreeSet<String> {
    faces.into_iter().map(str::to_owned).collect()
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
) -> Result<(), CodecError> {
    let source_bindings = resolve_rm_source_color_bindings(&model.om.rm_display_color_assignments);
    let face_ids = collect_rm_face_ids(ir.model.faces.iter().map(|face| face.id.as_str()));
    let face_bindings = resolve_rm_face_color_bindings(
        &face_ids,
        &model.om.rm_display_color_assignments,
        &model.om.part_color_definitions,
        &model.parasolid.deltas_records,
        &super::substrate::paired_delta_streams(scan),
    );
    if source_bindings.is_empty() && face_bindings.is_empty() {
        return Ok(());
    }
    let annotation_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    let mut appearances = BTreeMap::<String, AppearanceId>::new();
    for binding in source_bindings {
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
            ir,
            annotations,
            &mut appearances,
            definition,
            &annotation_stream,
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
        annotations.note(
            binding_id.as_str(),
            &annotation_stream,
            binding.source_offset,
            Some("RMFASTLOAD_COLOR_ASSIGNMENT"),
        );
        annotations
            .derived(binding_id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(binding_id.as_str(), "appearance")
            .map_err(cadmpeg_core::CodecError::from)?;
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
        let Some(definition) = model
            .om
            .part_color_definitions
            .iter()
            .rev()
            .find(|definition| definition.id == binding.color_definition)
        else {
            continue;
        };

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
        let face_id = face.id.clone();
        let appearance_id = ensure_rm_color_appearance(
            ir,
            annotations,
            &mut appearances,
            definition,
            &annotation_stream,
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
        annotations.note(
            binding_id.as_str(),
            &annotation_stream,
            binding.source_offset,
            Some("RMFASTLOAD_FACE_COLOR_ASSIGNMENT"),
        );
        annotations
            .derived(binding_id.as_str(), "target")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(binding_id.as_str(), "appearance")
            .map_err(cadmpeg_core::CodecError::from)?;
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
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    appearances: &mut BTreeMap<String, AppearanceId>,
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

    if let Some(id) = appearances.get(&definition.id) {
        return Ok(id.clone());
    }
    let id: AppearanceId = IdScope::native(cadmpeg_ir::identity_component!("appearance")).id(
        &cadmpeg_ir::identity_component!("rmfastload-color"),
        native_entity_key(&definition.id).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "NX RMFASTLOAD_COLOR_APPEARANCE definition id is not identity key text"
            ))
        })?,
    );
    annotations.note(
        id.as_str(),
        annotation_stream,
        definition.source_offset,
        Some("RMFASTLOAD_COLOR_APPEARANCE"),
    );
    annotations
        .derived(id.as_str(), "name")
        .map_err(cadmpeg_core::CodecError::from)?;
    annotations
        .derived(id.as_str(), "schema")
        .map_err(cadmpeg_core::CodecError::from)?;
    annotations
        .derived(id.as_str(), "base_color")
        .map_err(cadmpeg_core::CodecError::from)?;
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

    let lookup_id = id.clone();
    appearances.insert(definition.id.clone(), lookup_id);
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
    assignments: &[RmDisplayColorAssignment],
) -> Vec<RmSourceColorBinding> {
    let mut choices = BTreeMap::<&str, RmColorChoice<'_>>::new();
    for assignment in assignments {
        let Some(source_id) = assignment.target_object_id.as_deref() else {
            continue;
        };
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
        bindings.push(RmSourceColorBinding {
            source_id: source_id.to_owned(),
            color_definition: definition.to_owned(),
            source_offset,
        });
    }
    bindings
}

fn resolve_rm_face_colors(
    face_ids: &BTreeSet<String>,
    assignments: &[RmDisplayColorAssignment],
    definitions: &[super::om::PartColorDefinition],
    records: &[super::parasolid::ParasolidDeltasRecord],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
) -> Result<Vec<(String, Color)>, CodecError> {
    let bindings =
        resolve_rm_face_color_bindings(face_ids, assignments, definitions, records, delta_pairs);
    let mut colors = Vec::new();
    for binding in bindings {
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

        colors.push((binding.face_id, color));
    }
    Ok(colors)
}

fn resolve_rm_face_color_bindings(
    face_ids: &BTreeSet<String>,
    assignments: &[RmDisplayColorAssignment],
    definitions: &[super::om::PartColorDefinition],
    records: &[super::parasolid::ParasolidDeltasRecord],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
) -> Vec<RmFaceColorBinding> {
    let mut bindings = Vec::new();
    for (position, assignment) in assignments.iter().enumerate() {
        let RmDisplayColorAssignmentEncoding::Linked(row) = assignment.frame.encoding() else {
            continue;
        };
        let object_index = row.first_index().atom.value();

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

        if !definitions
            .iter()
            .any(|candidate| candidate.id.as_str() == definition)
        {
            continue;
        }

        let mut candidate = None::<String>;
        let mut ambiguous = false;

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
        bindings.push(RmFaceColorBinding {
            face_id,
            color_definition: definition.to_owned(),
            source_offset,
        });
    }
    bindings.sort_by(|left, right| left.face_id.cmp(&right.face_id));
    bindings
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
            annotations.note(
                native_ref.as_str(),
                &stream,
                source_offset,
                Some("JPEG_PREVIEW_INVALID"),
            );
            annotations.exactness(native_ref.as_str(), Exactness::ByteExact);
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(bytes.len()),
                "retain NX invalid JPEG preview",
            )?;
            push_native_unknown(
                ctx,
                unknowns,
                UnknownRecord::retained(native_ref, source_offset, bytes.to_vec(), Vec::new()),
            )?;
            continue;
        }
        let id: AssetId = extended_id(native_ref.as_str(), &cadmpeg_ir::identity_key!("asset"))
            .ok_or_else(|| {
                CodecError::malformed(format_args!("NX JPEG preview id is not an identity"))
            })?;
        annotations.note(
            id.as_str(),
            &stream,
            source_offset,
            Some("JPEG_PREVIEW_ASSET"),
        );
        annotations.exactness(id.as_str(), Exactness::ByteExact);
        annotations
            .derived(id.as_str(), "id")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(id.as_str(), "name")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(id.as_str(), "media_type")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(id.as_str(), "native_ref")
            .map_err(cadmpeg_core::CodecError::from)?;

        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "retain NX JPEG preview asset",
        )?;
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
                    data: cadmpeg_ir::assets::AssetData::new(bytes.to_vec()).ok_or_else(|| {
                        CodecError::Malformed("asset data must not be empty".into())
                    })?,
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
        let digest = cadmpeg_ir::hash::digest::Sha256Digest::digest(bytes);
        if digest != texture.sha256 {
            return Ok(());
        }
        sources.push((texture, bytes));
    }

    let mut assets = Vec::new();
    for (texture, bytes) in sources {
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "retain NX TIFF material asset",
        )?;
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
                    data: cadmpeg_ir::assets::AssetData::new(bytes.to_vec()).ok_or_else(|| {
                        CodecError::Malformed("asset data must not be empty".into())
                    })?,
                },
                Some(texture.id.clone()),
            )
            .map_err(CodecError::Malformed)?,
        );
    }
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    for (texture, asset) in textures.iter().zip(&assets) {
        annotations.note(
            asset.id.as_str(),
            &stream,
            texture.source_offset,
            Some("MATERIAL_TEXTURE_ASSET"),
        );
        annotations.exactness(asset.id.as_str(), Exactness::ByteExact);
        annotations
            .derived(asset.id.as_str(), "id")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(asset.id.as_str(), "media_type")
            .map_err(cadmpeg_core::CodecError::from)?;
        annotations
            .derived(asset.id.as_str(), "native_ref")
            .map_err(cadmpeg_core::CodecError::from)?;
    }

    ir.model.assets.extend(assets);
    Ok(())
}

fn attach_active_configuration_parameter_values(
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
        if ir.model.parameters[..index]
            .iter()
            .any(|previous| previous.id == parameter.id)
        {
            return Ok(());
        }
    }
    for parameter in &ir.model.parameters {
        for dependency in &parameter.dependencies {
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
        let id = parameter.id.clone();
        let value = value.clone();
        values.insert(id, value);
    }
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.parameter_values = values;
    annotations
        .derived(configuration.id.as_str(), "parameter_values")
        .map_err(cadmpeg_core::CodecError::from)?;
    Ok(())
}

fn attach_current_feature_states(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut current_bodies = Vec::new();
    for body in &ir.model.bodies {
        current_bodies.push(body.id.clone());
    }
    let Ok(active_features) = active_feature_closure(ir, &current_bodies) else {
        return Ok(());
    };
    for index in active_features.into_values() {
        let feature = &mut ir.model.features[index];
        feature.suppressed = Some(false);
        annotations
            .derived(&feature.id, "suppressed")
            .map_err(cadmpeg_core::CodecError::from)?;
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
    else {
        return Ok(());
    };
    if !ir.model.configurations[configuration_index]
        .feature_states
        .is_empty()
    {
        return Ok(());
    }
    let Ok(active_features) = active_feature_closure(ir, configuration_bodies) else {
        return Ok(());
    };
    let mut states = BTreeMap::new();
    for (id, &index) in &active_features {
        let feature = &ir.model.features[index];
        let outputs = feature.evaluation.outputs().clone();
        let dependencies = feature.dependencies.iter().cloned().collect::<Vec<_>>();
        let definition = feature.evaluation.definition().clone();
        let state_id = id.clone();
        states.insert(
            state_id,
            ConfigurationFeatureState {
                evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                    outputs: cadmpeg_ir::features::DistinctMembers::try_from(outputs)
                        .map_err(cadmpeg_core::CodecError::from)?,
                },
                dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies)
                    .map_err(cadmpeg_core::CodecError::from)?,
                definition,
            },
        );
    }
    for index in active_features.into_values() {
        let feature = &mut ir.model.features[index];
        if feature.suppressed != Some(false) {
            feature.suppressed = Some(false);
            annotations
                .derived(&feature.id, "suppressed")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    let configuration = &mut ir.model.configurations[configuration_index];
    configuration.feature_states = states;
    annotations
        .derived(configuration.id.as_str(), "feature_states")
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
    ir: &mut CadIr,
    body_bindings: &[crate::native::segments::SegmentBodyBinding],
    annotations: &mut AnnotationBuilder,
    stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Result<Option<FeatureId>, CodecError> {
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

    let mut sorted_bodies = ir.model.bodies.iter().collect::<Vec<_>>();
    sorted_bodies.sort_by(|first, second| first.id.cmp(&second.id));

    let id: FeatureId = IdScope::native(cadmpeg_ir::identity_component!("feature-history")).id(
        &cadmpeg_ir::identity_component!("feature"),
        cadmpeg_ir::identity_key!("initial-bodies"),
    );
    let mut selection_bodies = Vec::new();
    let mut feature_outputs = Vec::new();
    let mut source_properties = BTreeMap::new();
    let mut binding_ordinal = 0usize;

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
            source_properties.insert(
                cadmpeg_core::nonblank_literal!("segment_body_binding.{binding_ordinal}"),
                binding.id.clone(),
            );
            binding_ordinal += 1;
        }
        if matched {
            selection_bodies.push(body.id.clone());
            feature_outputs.push(body.id.clone());
        }
    }

    annotations.note(&id, stream, 0, Some("FEATURE_HISTORY_INPUT"));
    annotations
        .derived(&id, "definition")
        .map_err(cadmpeg_core::CodecError::from)?;
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
                    bodies: cadmpeg_ir::features::DistinctMembers::try_from(selection_bodies)
                        .map_err(cadmpeg_core::CodecError::from)?,
                    native: "nx:segment-body-bindings".to_string(),
                },
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(feature_outputs)
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
        body_references,
        body_data_block_uses,
        body_segment_uses,
        input_blocks,
        data_blocks,
    );
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    let initial_body_id = attach_initial_segment_bodies(ir, body_bindings, annotations, &stream)?;
    let base_ordinal = cadmpeg_core::decode::u64_from_index(ir.model.features.len());
    let booleans = booleans
        .iter()
        .map(|operation| (operation.operation_label.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let body_references_by_id = body_references
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let body_segment_uses_by_reference = group_pairs(
        body_segment_uses
            .iter()
            .map(|use_| (use_.feature_body_reference.as_str(), use_)),
    );
    let body_data_block_uses_by_reference = group_pairs(
        body_data_block_uses
            .iter()
            .map(|use_| (use_.feature_body_reference.as_str(), use_)),
    );
    let body_writer_references_by_operation =
        crate::native::features::unique_feature_body_references(body_references);
    let offset_store_bodies_by_operation =
        group_pairs(body_data_block_uses.iter().filter_map(|body_use| {
            let reference = body_references_by_id.get(body_use.feature_body_reference.as_str())?;
            Some((
                reference.operation_label.as_str(),
                (reference.body.value(), body_use.data_block.clone()),
            ))
        }));
    let body_references = admitted_body_references;
    let body_reference_occurrences_by_operation = group_pairs(
        body_reference_occurrences
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
    );
    let mut body_writer_history = BodyWriterHistory::default();
    if let Some(feature) = initial_body_id
        .as_ref()
        .and_then(|id| ir.model.features.iter().find(|feature| feature.id == *id))
    {
        body_writer_history.record_writer(None, None, feature.evaluation.outputs(), &feature.id);
    }
    let body_alias_roots = crate::native::segments::body_alias_roots(body_bindings);
    let canonical_body =
        |identity: u32| body_alias_roots.get(&identity).copied().unwrap_or(identity);
    let input_blocks_by_operation = group_pairs(
        input_blocks
            .iter()
            .map(|input| (input.operation_label.as_str(), input)),
    );
    let input_column_row_uses_by_operation = group_pairs(
        input_column_row_uses
            .iter()
            .map(|use_| (use_.operation_label.as_str(), use_)),
    );
    let input_column_targets_by_operation = group_pairs(
        input_column_targets
            .iter()
            .map(|target| (target.operation_label.as_str(), target)),
    );
    let input_block_identity_group_by_input = input_block_identity_groups
        .iter()
        .flat_map(|group| {
            group
                .members
                .iter()
                .map(move |member| (member.input_block.as_str(), group.id.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let datum_csys_constructions_by_operation = datum_csys_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let datum_csys_payloads_by_operation = group_pairs(
        datum_csys_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let datum_csys_payload_scalar_pairs_by_operation = group_pairs(
        datum_csys_payload_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
    );
    let datum_csys_payload_fixed_pairs_by_operation = group_pairs(
        datum_csys_payload_fixed_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
    );
    let datum_csys_payload_scalars_by_operation = group_pairs(
        datum_csys_payload_scalars
            .iter()
            .map(|scalar| (scalar.operation_label.as_str(), scalar)),
    );
    let datum_csys_descriptors_by_operation = group_pairs(
        datum_csys_descriptors
            .iter()
            .map(|descriptor| (descriptor.operation_label.as_str(), descriptor)),
    );
    let datum_csys_column_row_uses_by_operation = group_pairs(
        datum_csys_column_row_uses
            .iter()
            .map(|use_| (use_.operation_label.as_str(), use_)),
    );
    let datum_csys_uses_by_input_operation = group_pairs(
        datum_csys_block_uses
            .iter()
            .map(|block_use| (block_use.input_operation_label.as_str(), block_use)),
    );
    let datum_plane_headers_by_operation = datum_plane_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let datum_plane_payloads_by_operation = datum_plane_payloads
        .iter()
        .map(|payload| (payload.operation_label.as_str(), payload))
        .collect::<BTreeMap<_, _>>();
    let datum_plane_payload_scalar_pairs_by_operation = group_pairs(
        datum_plane_payload_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
    );
    let datum_plane_descriptors_by_operation = group_pairs(
        datum_plane_descriptors
            .iter()
            .map(|descriptor| (descriptor.operation_label.as_str(), descriptor)),
    );
    let datum_plane_uses_by_input_operation = group_pairs(
        datum_plane_block_uses
            .iter()
            .map(|block_use| (block_use.input_operation_label.as_str(), block_use)),
    );
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
    let datum_identity_uses_by_operation = group_pairs(
        datum_plane_csys_identity_uses
            .iter()
            .flat_map(|identity_use| {
                [
                    (
                        identity_use.datum_plane_operation_label.as_str(),
                        identity_use,
                    ),
                    (
                        identity_use.datum_csys_operation_label.as_str(),
                        identity_use,
                    ),
                ]
            }),
    );
    let sketch_references_by_operation = group_pairs(
        sketch_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
    );
    let projected_curve_references_by_operation = group_pairs(
        projected_curve_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
    );
    let projected_curve_construction_payloads_by_operation = group_pairs(
        projected_curve_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let projected_curve_construction_strings_by_operation = group_pairs(
        projected_curve_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
    );
    let fset_reference_graphs_by_operation = group_pairs(
        fset_reference_graphs
            .iter()
            .map(|graph| (graph.operation_label.as_str(), graph)),
    );
    let fset_construction_payloads_by_operation = group_pairs(
        fset_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let delete_reference_fields_by_operation = group_pairs(
        delete_reference_fields
            .iter()
            .map(|field| (field.operation_label.as_str(), field)),
    );
    let delete_construction_payloads_by_operation = group_pairs(
        delete_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let pattern_references_by_operation = group_pairs(
        pattern_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
    );
    let pattern_counted_reference_lanes_by_operation = group_pairs(
        pattern_counted_reference_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let pattern_construction_payloads_by_operation = group_pairs(
        pattern_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let pattern_construction_strings_by_operation = group_pairs(
        pattern_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
    );
    let pattern_construction_fixed_lanes_by_operation = group_pairs(
        pattern_construction_fixed_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let pattern_transform_lanes_by_operation = group_pairs(
        pattern_transform_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let multi_instance_output_lanes_by_operation = group_pairs(
        multi_instance_output_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let identical_instance_output_lanes_by_operation = group_pairs(
        identical_instance_output_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let point_construction_headers_by_operation = point_construction_headers
        .iter()
        .map(|header| (header.operation_label.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let point_construction_scalar_lanes_by_operation = point_construction_scalar_lanes
        .iter()
        .map(|lane| (lane.operation_label.as_str(), lane))
        .collect::<BTreeMap<_, _>>();
    let draft_construction_references_by_operation = group_pairs(
        draft_construction_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
    );
    let draft_construction_index_lanes_by_operation = group_pairs(
        draft_construction_index_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let draft_construction_payloads_by_operation = group_pairs(
        draft_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let draft_construction_graph_payloads_by_operation = group_pairs(
        draft_construction_graph_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let draft_construction_fixed_lanes_by_operation = group_pairs(
        draft_construction_fixed_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let draft_construction_binary32_lanes_by_operation = group_pairs(
        draft_construction_binary32_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let draft_construction_graph_strings_by_operation = group_pairs(
        draft_construction_graph_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
    );
    let draft_construction_identity_frames_by_operation = group_pairs(
        draft_construction_identity_frames
            .iter()
            .map(|frame| (frame.operation_label.as_str(), frame)),
    );
    let draft_construction_terminal_lanes_by_operation = group_pairs(
        draft_construction_terminal_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let surface_construction_references_by_operation = group_pairs(
        surface_construction_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
    );
    let surface_construction_payloads_by_operation = group_pairs(
        surface_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let surface_construction_scalar_pairs_by_operation = group_pairs(
        surface_construction_scalar_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
    );
    let surface_construction_strings_by_operation = group_pairs(
        surface_construction_strings
            .iter()
            .map(|value| (value.operation_label.as_str(), value)),
    );
    let surface_construction_branches_by_operation = group_pairs(
        surface_construction_branches
            .iter()
            .map(|branch| (branch.operation_label.as_str(), branch)),
    );
    let sketch_named_point_uses_by_operation = group_pairs(
        sketch_named_point_block_uses
            .iter()
            .map(|block_use| (block_use.operation_label.as_str(), block_use)),
    );
    let sketch_preceding_named_point_uses_by_operation = group_pairs(
        sketch_preceding_named_point_uses
            .iter()
            .map(|point_use| (point_use.operation_label.as_str(), point_use)),
    );
    let sketch_point_uses_by_operation = group_pairs(
        sketch_point_uses
            .iter()
            .map(|point_use| (point_use.operation_label.as_str(), point_use)),
    );
    let sketch_point_groups_by_operation = group_pairs(
        sketch_point_groups
            .iter()
            .map(|group| (group.operation_label.as_str(), group)),
    );
    let extrude_profile_references_by_operation = group_pairs(
        extrude_profile_references
            .iter()
            .map(|reference| (reference.operation_label.as_str(), reference)),
    );
    let extrude_construction_profiles_by_operation = extrude_construction_profiles
        .iter()
        .map(|profile| (profile.operation_label.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let operation_body_operands_by_operation = group_pairs(
        operation_body_operands
            .iter()
            .map(|operand| (operand.operation_label.as_str(), operand)),
    );
    let segment_body_operands_by_operation = group_pairs(
        operation_body_operands
            .iter()
            .filter(|operand| !operand.segment_body_bindings.is_empty())
            .map(|operand| (operand.operation_label.as_str(), operand)),
    );
    let sketch_construction_inputs_by_operation = sketch_construction_inputs
        .iter()
        .map(|inputs| (inputs.operation_label.as_str(), inputs))
        .collect::<BTreeMap<_, _>>();
    let sketch_records_by_operation = group_pairs(
        sketch_records
            .iter()
            .map(|record| (record.operation_label.as_str(), record)),
    );
    let sketch_construction_payloads_by_operation = group_pairs(
        sketch_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let sketch_coordinate_pairs_by_operation = group_pairs(
        sketch_coordinate_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
    );
    let sketch_fixed_pairs_by_operation = group_pairs(
        sketch_fixed_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
    );
    let sketch_mixed_pairs_by_operation = group_pairs(
        sketch_mixed_pairs
            .iter()
            .map(|pair| (pair.operation_label.as_str(), pair)),
    );
    let sketch_payload_scalar_lanes_by_operation = group_pairs(
        sketch_payload_scalar_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let sketch_fixed_points_by_operation = group_pairs(
        sketch_fixed_points
            .iter()
            .map(|point| (point.operation_label.as_str(), point)),
    );
    let block_constructions_by_operation = block_constructions
        .iter()
        .map(|construction| (construction.operation_label.as_str(), construction))
        .collect::<BTreeMap<_, _>>();
    let block_construction_payloads_by_operation = group_pairs(
        block_construction_payloads
            .iter()
            .map(|payload| (payload.operation_label.as_str(), payload)),
    );
    let block_dimensions_by_operation = block_dimensions
        .iter()
        .map(|dimensions| (dimensions.operation_label.as_str(), dimensions))
        .collect::<BTreeMap<_, _>>();
    let block_payload_points_by_operation = group_pairs(
        block_payload_points
            .iter()
            .map(|point| (point.operation_label.as_str(), point)),
    );
    let block_payload_point_groups_by_operation = group_pairs(
        block_payload_point_groups
            .iter()
            .map(|group| (group.operation_label.as_str(), group)),
    );
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
    let extrude_payload_32_branches_by_operation = group_pairs(
        extrude_payload_32_branches
            .iter()
            .map(|branch| (branch.operation_label.as_str(), branch)),
    );
    let mut operation_body_scalar_triples_by_operation = group_pairs(
        operation_body_scalar_triples
            .iter()
            .map(|triple| (triple.operation_label.as_str(), triple)),
    );
    for triples in operation_body_scalar_triples_by_operation.values_mut() {
        triples.sort_by(|left, right| {
            left.body_reference_ordinal
                .cmp(&right.body_reference_ordinal)
        });
    }
    let operation_body_members_by_operation = group_pairs(
        operation_body_members
            .iter()
            .map(|member| (member.operation_label.as_str(), member)),
    );
    let operation_body_11_continuations_by_operation = group_pairs(
        operation_body_11_continuations
            .iter()
            .map(|continuation| (continuation.operation_label.as_str(), continuation)),
    );
    let operation_body_reference_lanes_by_operation = group_pairs(
        operation_body_reference_lanes
            .iter()
            .map(|lane| (lane.operation_label.as_str(), lane)),
    );
    let SegmentBindingBodyIndexes {
        by_object: bodies_by_object_index,
        by_binding: bodies_by_segment_binding,
    } = segment_binding_body_indexes(ir, body_bindings)?;
    let mut body_image_outputs_by_write = operation_body_image_outputs_by_write(
        operation_body_image_segment_uses,
        &bodies_by_segment_binding,
    );
    let mut conflicting_body_output_writes = BTreeSet::new();
    let identity_candidates = operation_body_identity_outputs_by_write(
        operation_body_identity_segment_uses,
        &bodies_by_segment_binding,
    );
    merge_operation_body_outputs(
        &mut body_image_outputs_by_write,
        &mut conflicting_body_output_writes,
        &identity_candidates,
    );
    let partition_candidates = operation_body_group_partition_outputs_by_write(
        operation_body_writes,
        body_write_group_partition_uses,
        &ir.model.bodies,
    )?;
    merge_operation_body_outputs(
        &mut body_image_outputs_by_write,
        &mut conflicting_body_output_writes,
        &partition_candidates,
    );
    let explicit_hole_outputs = primary_hole_outputs(
        simple_hole_templates,
        &body_references,
        body_bindings,
        &bodies_by_object_index,
    );
    let simple_hole_operations = simple_hole_operations(
        simple_hole_templates,
        simple_hole_construction_groups,
        &operation_positions,
    )
    .unwrap_or_default();
    let mut hole_outputs = explicit_hole_outputs;
    let mut simple_hole_diameters = BTreeMap::new();
    admit_hole_recognition_tolerances(ir)?;
    if let Some(projection) = hole_body_projection(ir, &simple_hole_operations, &hole_outputs) {
        hole_outputs.extend(projection.outputs);
        simple_hole_diameters.extend(projection.diameters);
    }
    let counterbore_operations =
        counterbore_operations(simple_hole_templates, &operation_positions).unwrap_or_default();
    let mut counterbore_dimensions = BTreeMap::new();
    if let Some(projection) =
        counterbore_body_projection(ctx, ir, &counterbore_operations, &hole_outputs)?
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
    let counterbore_hole_placements = counterbore_axis_placements_for_operations(
        ctx,
        ir,
        &counterbore_operations,
        &hole_outputs,
    )?;
    let blind_hole_placements =
        blind_hole_axis_placements_for_operations(ir, &blind_hole_operations, &hole_outputs);
    let simple_hole_chamfers = simple_hole_chamfers(ctx, ir, simple_hole_templates, &hole_outputs)?;
    let hole_packages = hole_package_projection(
        ir,
        simple_hole_templates,
        simple_hole_construction_groups,
        hole_package_construction_group_uses,
        HolePackageSources {
            outputs: &hole_outputs,
            diameters: &simple_hole_diameters,
            chamfers: &simple_hole_chamfers,
        },
    );
    let mut feature_ids_by_operation = BTreeMap::new();
    for label in labels {
        const PREFIX: &str = "nx:feature-history:feature#";

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
        let Ok(id) = FeatureId::mint(format!("{PREFIX}{key}")) else {
            continue;
        };
        feature_ids_by_operation.insert(label.id.as_str(), id);
    }
    let parameter_bindings_by_operation = group_pairs(
        parameter_bindings
            .iter()
            .map(|binding| (binding.operation_label.as_str(), binding)),
    );
    let parameter_uses_by_operation = group_pairs(
        parameter_uses
            .iter()
            .map(|parameter_use| (parameter_use.operation_label.as_str(), parameter_use)),
    );
    let operation_labels_by_record = operation_records
        .iter()
        .map(|record| (record.id.as_str(), record.operation_label.as_str()))
        .collect::<BTreeMap<_, _>>();
    let body_writes_by_operation = group_pairs(
        operation_body_writes
            .iter()
            .filter_map(|write| write.operation_label.as_deref().map(|label| (label, write))),
    );
    let mut body_identity_writers = BTreeMap::<u8, FeatureId>::new();
    let payload_strings_by_operation = group_pairs(payload_strings.iter().filter_map(|value| {
        operation_labels_by_record
            .get(value.operation_record.as_str())
            .map(|operation| (*operation, value))
    }));
    let mut parameter_owners = BTreeMap::new();
    for parameter in &ir.model.parameters {
        let key = parameter.id.clone();
        let owner = parameter.owner.clone();
        parameter_owners.insert(key, owner);
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
            losses.push(
                NxLossCode::SemanticAnnotationOrderUnstatable
                    .note(format!("{PREFIX}{}{SUFFIX}", label.id)),
            );
            continue;
        };
        let [text, font_family] = payload_strings else {
            continue;
        };
        let Some(annotation) = text_semantic_annotation(
            &label.id,
            order,
            &[text.value.as_str(), font_family.value.as_str()],
        )?
        else {
            continue;
        };
        annotations.note(
            annotation.id.as_str(),
            &stream,
            label.source_offset,
            Some("TEXT_SEMANTIC_ANNOTATION"),
        );
        annotations.exactness(annotation.id.as_str(), Exactness::Derived);

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
        let id = source_id.clone();
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
                push_unique_feature_dependency(&mut dependencies, writer);
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
                    push_unique_feature_dependency(&mut dependencies, writer);
                }
                for body in &operation.tools {
                    if let Some(writer) = boolean_participant_writer(
                        tools,
                        body.token.value(),
                        offset_store_body_blocks,
                        &body_alias_roots,
                        &body_writer_history,
                    ) {
                        push_unique_feature_dependency(&mut dependencies, writer);
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
                push_unique_feature_dependency(&mut dependencies, writer);
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
            push_unique_feature_dependency(&mut dependencies, writer);
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
            push_unique_feature_dependency(&mut dependencies, dependency);
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
            push_unique_feature_dependency(&mut dependencies, dependency);
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
            push_unique_feature_dependency(&mut dependencies, dependency);
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            if let Some(feature) =
                feature_ids_by_operation.get(dependency.sketch_operation_label.as_str())
            {
                push_unique_feature_dependency(&mut dependencies, feature);
            }
        }
        let mut source_properties = BTreeMap::new();
        for write in retained_operation_body_writes {
            let ordinal = write.ordinal;
            insert_source_property(
                &mut source_properties,
                format_args!("body_write.{ordinal}"),
                format_args!("{}", write.id),
            );
            insert_source_property(
                &mut source_properties,
                format_args!("body_write.{ordinal}.body_identity"),
                format_args!("{}", write.frame.body_identity()),
            );
            insert_source_property(
                &mut source_properties,
                format_args!("body_write.{ordinal}.group_node"),
                format_args!("{}", write.frame.group_node().value()),
            );
            insert_source_property(
                &mut source_properties,
                format_args!("body_write.{ordinal}.endpoint_tag"),
                format_args!("{}", write.frame.endpoint_tag().code()),
            );
            insert_source_property(
                &mut source_properties,
                format_args!("body_write.{ordinal}.body_image_object_index"),
                format_args!("{}", write.frame.body_image().value()),
            );
            if let Some(use_) = operation_body_image_segment_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.body_image_segment_use"),
                    format_args!("{}", use_.id),
                );
                insert_source_property(
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.segment_body_binding"),
                    format_args!("{}", use_.segment_body_binding),
                );
            }
            if let Some(use_) = operation_body_identity_segment_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.body_identity_segment_use"),
                    format_args!("{}", use_.id),
                );
                insert_source_property(
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.body_identity_segment_binding"),
                    format_args!("{}", use_.segment_body_binding),
                );
            }
            if let Some(use_) = operation_body_partition_uses
                .iter()
                .find(|use_| use_.operation_body_write == write.id)
            {
                insert_source_property(
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.partition_use"),
                    format_args!("{}", use_.id),
                );
                insert_source_property(
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.partition_stream_ordinal"),
                    format_args!("{}", use_.partition_stream_ordinal),
                );
                for (group_ordinal, group) in use_.parasolid_group_records.iter().enumerate() {
                    insert_source_property(
                        &mut source_properties,
                        format_args!("body_write.{ordinal}.parasolid_group.{group_ordinal}"),
                        format_args!("{group}"),
                    );
                }
                for (member_ordinal, member) in use_.parasolid_group_members.iter().enumerate() {
                    insert_source_property(
                        &mut source_properties,
                        format_args!(
                            "body_write.{ordinal}.parasolid_group_member.{member_ordinal}"
                        ),
                        format_args!("{member}"),
                    );
                }
            }
            if let Some(use_) = body_write_group_partition_uses
                .iter()
                .find(|use_| use_.body_write == write.id)
            {
                insert_source_property(
                    &mut source_properties,
                    format_args!("body_write.{ordinal}.group_partition_use"),
                    format_args!("{}", use_.id),
                );
            }
        }
        operation_source_properties(
            &mut source_properties,
            &label.id,
            operation_records,
            operation_common_frames,
            operation_terminal_frames,
        );
        if let Some(stable_identity) = &label.stable_identity {
            insert_source_property(
                &mut source_properties,
                format_args!("operation_stable_identity"),
                format_args!("{stable_identity}"),
            );
        }
        for (use_ordinal, block_use) in datum_csys_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_csys_block_use.{use_ordinal}"),
                format_args!("{}", block_use.id),
            );
        }
        if let Some(dependency) = sketch_datum_csys_dependencies.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_point_dependency_use"),
                format_args!("{}", dependency.sketch_point_use),
            );
            match &dependency.block_relation {
                crate::native::features::FeatureSketchDatumCsysBlockRelation::Shared {
                    data_block,
                } => {
                    insert_source_property(
                        &mut source_properties,
                        format_args!("sketch_point_dependency_shared_block"),
                        format_args!("{data_block}"),
                    );
                }
                crate::native::features::FeatureSketchDatumCsysBlockRelation::Consecutive {
                    point_data_block,
                    construction_data_block,
                } => {
                    insert_source_property(
                        &mut source_properties,
                        format_args!("sketch_point_dependency_point_block"),
                        format_args!("{point_data_block}"),
                    );
                    insert_source_property(
                        &mut source_properties,
                        format_args!("sketch_point_dependency_construction_block"),
                        format_args!("{construction_data_block}"),
                    );
                }
            }
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_datum_csys_dependency"),
                format_args!("{}", dependency.id),
            );
            for (alias_ordinal, alias) in dependency.scalar_aliases.iter().enumerate() {
                insert_source_property(
                    &mut source_properties,
                    format_args!("sketch_point_dependency_scalar.{alias_ordinal}"),
                    format_args!("{}", alias.datum_csys_scalar),
                );
                insert_source_property(
                    &mut source_properties,
                    format_args!("sketch_point_dependency_coordinate.{alias_ordinal}"),
                    format_args!("{}", alias.sketch_coordinate_ordinal),
                );
            }
        }
        let deletes_body = label.value == "DELETE";
        let mut outputs = if deletes_body {
            Vec::new()
        } else {
            match body_references.get(label.id.as_str()) {
                Some(body) => feature_body_outputs(*body, body_bindings, &bodies_by_object_index),
                None => Vec::new(),
            }
        };
        if !deletes_body && outputs.is_empty() && !operation_body_writes.is_empty() {
            outputs = complete_operation_body_image_outputs(
                operation_body_writes,
                &body_image_outputs_by_write,
            );
        }
        if outputs.is_empty() {
            if let Some(bodies) = hole_outputs
                .get(label.id.as_str())
                .or_else(|| hole_packages.outputs.get(label.id.as_str()))
            {
                outputs = Vec::new();
                for body in bodies {
                    outputs.push(body.clone());
                }
            }
        }
        if outputs.is_empty() {
            if let Some(body) = boolean_target_output(boolean_definition.as_ref()) {
                outputs = Vec::new();
                outputs.push(body.clone());
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
                &mut source_properties,
                format_args!("{NATIVE_PRIMARY_BODY_OBJECT_INDEX}"),
                format_args!("{body}"),
            );
        }
        if let Some(reference) = body_writer_references_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("primary_body_reference"),
                format_args!("{}", reference.id),
            );
            if let Some(uses) = body_segment_uses_by_reference.get(reference.id.as_str()) {
                if let [use_] = uses.as_slice() {
                    insert_source_property(
                        &mut source_properties,
                        format_args!("primary_body_segment_use"),
                        format_args!("{}", use_.id),
                    );
                    insert_source_property(
                        &mut source_properties,
                        format_args!("primary_body_segment_binding"),
                        format_args!("{}", use_.segment_body_binding),
                    );
                }
            }
            if let Some(uses) = body_data_block_uses_by_reference.get(reference.id.as_str()) {
                if let [use_] = uses.as_slice() {
                    insert_source_property(
                        &mut source_properties,
                        format_args!("primary_body_data_block_use"),
                        format_args!("{}", use_.id),
                    );
                    insert_source_property(
                        &mut source_properties,
                        format_args!("primary_body_data_block"),
                        format_args!("{}", use_.data_block),
                    );
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
                &mut source_properties,
                format_args!("body_reference.{ordinal}"),
                format_args!("{}", reference.body.value()),
            );
            insert_source_property(
                &mut source_properties,
                format_args!("body_reference_occurrence.{ordinal}"),
                format_args!("{}", reference.id),
            );
        }
        if let Some(inputs) = sketch_construction_inputs_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_construction_inputs"),
                format_args!("{}", inputs.id),
            );
        }
        for (ordinal, record) in sketch_records_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_record.{ordinal}"),
                format_args!("{}", record.id),
            );
        }
        for (ordinal, payload) in sketch_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_construction_payload.{ordinal}"),
                format_args!("{}", payload.id),
            );
        }
        for pair in sketch_coordinate_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_coordinate_pair.{}", pair.ordinal),
                format_args!("{}", pair.id),
            );
        }
        for pair in sketch_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_fixed_pair.{}", pair.ordinal),
                format_args!("{}", pair.id),
            );
        }
        for pair in sketch_mixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_mixed_pair.{}", pair.ordinal),
                format_args!("{}", pair.id),
            );
        }
        for lane in sketch_payload_scalar_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_scalar_lane.{}", lane.ordinal),
                format_args!("{}", lane.id),
            );
        }
        for (ordinal, point) in sketch_fixed_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_fixed_point.{ordinal}"),
                format_args!("{}", point.id),
            );
        }
        if let Some(construction) = block_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("block_construction"),
                format_args!("{}", construction.id),
            );
        }
        for (ordinal, payload) in block_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("block_construction_payload.{ordinal}"),
                format_args!("{}", payload.id),
            );
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("block_dimensions"),
                format_args!("{}", dimensions.id),
            );
            for (dimension_ordinal, dimension) in dimensions.dimensions.iter().enumerate() {
                insert_source_property(
                    &mut source_properties,
                    format_args!("block_dimension_declaration.{dimension_ordinal}"),
                    format_args!("{}", dimension.declaration),
                );
                insert_source_property(
                    &mut source_properties,
                    format_args!("block_dimension_expression.{dimension_ordinal}"),
                    format_args!("{}", dimension.expression),
                );
            }
        }
        for (ordinal, point) in block_payload_points_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("block_payload_point.{ordinal}"),
                format_args!("{}", point.id),
            );
        }
        for (ordinal, group) in block_payload_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("block_payload_point_group.{ordinal}"),
                format_args!("{}", group.id),
            );
        }
        if let Some(construction) = extrude_32_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("extrude_32_construction"),
                format_args!("{}", construction.id),
            );
        }
        if let Some(header) = extrude_payload_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("extrude_payload_header"),
                format_args!("{}", header.id),
            );
        }
        if let Some(lane) = operation_terminal_discriminators_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("operation_terminal_discriminator"),
                format_args!("{}", lane.id),
            );
        }
        for (ordinal, branch) in extrude_payload_32_branches_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("extrude_payload_32_branch.{ordinal}"),
                format_args!("{}", branch.id),
            );
        }
        for triple in operation_body_scalar_triples_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!(
                    "operation_body_scalar_triple.{}",
                    triple.body_reference_ordinal
                ),
                format_args!("{}", triple.id),
            );
        }
        for member in operation_body_members_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!(
                    "operation_body_member.{}.{}",
                    member.body_reference_ordinal, member.ordinal
                ),
                format_args!("{}", member.id),
            );
        }
        for continuation in operation_body_11_continuations_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!(
                    "operation_body_11_continuation.{}",
                    continuation.body_reference_ordinal
                ),
                format_args!("{}", continuation.id),
            );
        }
        for lane in operation_body_reference_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!(
                    "operation_body_reference_lane.{}",
                    lane.body_reference_ordinal
                ),
                format_args!("{}", lane.id),
            );
        }
        if let Some(construction) = datum_csys_constructions_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_csys_construction"),
                format_args!("{}", construction.id),
            );
        }
        for (ordinal, use_) in datum_csys_column_row_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_csys_column_row_use.{ordinal}"),
                format_args!("{}", use_.id),
            );
        }
        for (ordinal, payload) in datum_csys_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_csys_payload.{ordinal}"),
                format_args!("{}", payload.id),
            );
        }
        for (ordinal, pair) in datum_csys_payload_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_csys_payload_scalar_pair.{ordinal}"),
                format_args!("{}", pair.id),
            );
        }
        for (ordinal, pair) in datum_csys_payload_fixed_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_csys_payload_fixed_pair.{ordinal}"),
                format_args!("{}", pair.id),
            );
        }
        for (ordinal, scalar) in datum_csys_payload_scalars_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_csys_payload_scalar.{ordinal}"),
                format_args!("{}", scalar.id),
            );
        }
        for (ordinal, descriptor) in datum_csys_descriptors_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_csys_descriptor.{ordinal}"),
                format_args!("{}", descriptor.id),
            );
        }
        if let Some(header) = datum_plane_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_plane_header"),
                format_args!("{}", header.id),
            );
        }
        if let Some(payload) = datum_plane_payloads_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_plane_payload"),
                format_args!("{}", payload.id),
            );
        }
        for (ordinal, pair) in datum_plane_payload_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_plane_payload_scalar_pair.{ordinal}"),
                format_args!("{}", pair.id),
            );
        }
        for (ordinal, descriptor) in datum_plane_descriptors_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_plane_descriptor.{ordinal}"),
                format_args!("{}", descriptor.id),
            );
        }
        for (ordinal, identity_use) in datum_identity_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_identity_use.{ordinal}"),
                format_args!("{}", identity_use.id),
            );
        }
        for (use_ordinal, block_use) in datum_plane_uses_by_input_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("datum_plane_block_use.{use_ordinal}"),
                format_args!("{}", block_use.id),
            );
        }
        simple_hole_native_properties(
            &mut source_properties,
            &label.id,
            simple_hole_templates,
            simple_hole_repeated_scalar_lanes,
            simple_hole_repeated_scalar_lane_block_references,
            simple_hole_construction_groups,
        );
        for lane in hole_package_construction_group_lanes
            .iter()
            .filter(|lane| lane.operation_label == label.id)
        {
            insert_source_property(
                &mut source_properties,
                format_args!("hole_package_construction_group_lane"),
                format_args!("{}", lane.id),
            );
        }
        for group_use in hole_package_construction_group_uses {
            let group = simple_hole_construction_groups
                .iter()
                .find(|group| group.id == group_use.simple_hole_construction_group);
            if group_use.operation_label == label.id {
                insert_source_property(
                    &mut source_properties,
                    format_args!("hole_package_construction_group_use"),
                    format_args!("{}", group_use.id),
                );
                insert_source_property(
                    &mut source_properties,
                    format_args!("simple_hole_construction_group"),
                    format_args!("{}", group_use.simple_hole_construction_group),
                );
            } else if group.is_some_and(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.operation_label == label.id)
            }) {
                insert_source_property(
                    &mut source_properties,
                    format_args!("hole_package_construction_group_use"),
                    format_args!("{}", group_use.id),
                );
                insert_source_property(
                    &mut source_properties,
                    format_args!("hole_package_operation"),
                    format_args!("{}", group_use.operation_label),
                );
            }
        }
        for (slot, value) in label.objects.0.iter().enumerate() {
            if let Some(value) = value {
                insert_source_property(
                    &mut source_properties,
                    format_args!("object_index.{slot}"),
                    format_args!("{}", value.value()),
                );
            } else {
                insert_source_property(
                    &mut source_properties,
                    format_args!("object_index.{slot}"),
                    format_args!("null"),
                );
            }
        }
        for input in input_blocks_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("input_block_record.{}", input.input_slot),
                format_args!("{}", input.id),
            );
            insert_source_property(
                &mut source_properties,
                format_args!("input_block.{}", input.input_slot),
                format_args!("{}", input.data_block),
            );
            if let Some(group) = input_block_identity_group_by_input.get(input.id.as_str()) {
                insert_source_property(
                    &mut source_properties,
                    format_args!("input_block_identity_group.{}", input.input_slot),
                    format_args!("{}", (*group)),
                );
            }
        }
        for (ordinal, use_) in input_column_row_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("input_column_row_use.{ordinal}"),
                format_args!("{}", use_.id),
            );
        }
        for (ordinal, target) in input_column_targets_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("input_column_target.{ordinal}"),
                format_args!("{}", target.id),
            );
        }
        for reference in sketch_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_reference_record.{}", reference.position.ordinal()),
                format_args!("{}", reference.id),
            );
            insert_source_property_reference(
                &mut source_properties,
                format_args!("sketch_reference.{}", reference.position.ordinal()),
                reference.data_block.as_deref(),
                reference.token.value(),
            );
        }
        for reference in projected_curve_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("projected_curve_reference_record.{}", reference.ordinal),
                format_args!("{}", reference.id),
            );
            insert_source_property_reference(
                &mut source_properties,
                format_args!("projected_curve_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            );
        }
        for payload in projected_curve_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("projected_curve_construction_payload"),
                format_args!("{}", payload.id),
            );
        }
        for value in projected_curve_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("projected_curve_construction_string.{}", value.ordinal),
                format_args!("{}", value.id),
            );
        }
        for graph in fset_reference_graphs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("fset_reference_graph"),
                format_args!("{}", graph.id),
            );
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
                &mut source_properties,
                format_args!("fset_construction_payload.{group}"),
                format_args!("{}", payload.id),
            );
        }
        for field in delete_reference_fields_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("delete_reference_field"),
                format_args!("{}", field.id),
            );
        }
        for payload in delete_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("delete_construction_payload"),
                format_args!("{}", payload.id),
            );
        }
        for reference in pattern_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("pattern_reference_record.{}", reference.ordinal),
                format_args!("{}", reference.id),
            );
            insert_source_property_reference(
                &mut source_properties,
                format_args!("pattern_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            );
        }
        for payload in pattern_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("pattern_construction_payload"),
                format_args!("{}", payload.id),
            );
        }
        for lane in pattern_counted_reference_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("pattern_counted_reference_lane"),
                format_args!("{}", lane.id),
            );
        }
        for value in pattern_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("pattern_construction_string.{}", value.ordinal),
                format_args!("{}", value.id),
            );
        }
        for lane in pattern_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("pattern_construction_fixed_lane.{}", lane.ordinal),
                format_args!("{}", lane.id),
            );
        }
        for lane in pattern_transform_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("pattern_transform_lane"),
                format_args!("{}", lane.id),
            );
        }
        for lane in multi_instance_output_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("multi_instance_output_lane"),
                format_args!("{}", lane.id),
            );
        }
        for lane in identical_instance_output_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("identical_instance_output_lane"),
                format_args!("{}", lane.id),
            );
        }
        if let Some(header) = point_construction_headers_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("point_construction_header"),
                format_args!("{}", header.id),
            );
            insert_source_property_reference(
                &mut source_properties,
                format_args!("point_construction_reference"),
                header.data_block.as_deref(),
                header.token.value(),
            );
            insert_source_property(
                &mut source_properties,
                format_args!("point_construction_mode"),
                format_args!("{:02x}", u8::from(header.mode)),
            );
        }
        if let Some(lane) = point_construction_scalar_lanes_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("point_construction_scalar_lane"),
                format_args!("{}", lane.id),
            );
        }
        for reference in draft_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_reference_record.{}", reference.ordinal),
                format_args!("{}", reference.id),
            );
            insert_source_property_reference(
                &mut source_properties,
                format_args!("draft_construction_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            );
        }
        for lane in draft_construction_index_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_index_lane"),
                format_args!("{}", lane.id),
            );
        }
        for payload in draft_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_payload"),
                format_args!("{}", payload.id),
            );
        }
        for payload in draft_construction_graph_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_graph_payload"),
                format_args!("{}", payload.id),
            );
        }
        for lane in draft_construction_fixed_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_fixed_lane.{}", lane.ordinal),
                format_args!("{}", lane.id),
            );
        }
        for lane in draft_construction_binary32_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_binary32_lane.{}", lane.ordinal),
                format_args!("{}", lane.id),
            );
        }
        for value in draft_construction_graph_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_graph_string.{}", value.ordinal),
                format_args!("{}", value.id),
            );
        }
        for frame in draft_construction_identity_frames_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_identity_frame.{}", frame.ordinal),
                format_args!("{}", frame.id),
            );
        }
        for lane in draft_construction_terminal_lanes_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("draft_construction_terminal_lane"),
                format_args!("{}", lane.id),
            );
        }
        for reference in surface_construction_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!(
                    "surface_construction_reference_record.{}",
                    reference.ordinal
                ),
                format_args!("{}", reference.id),
            );
            insert_source_property_reference(
                &mut source_properties,
                format_args!("surface_construction_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            );
        }
        for payload in surface_construction_payloads_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("surface_construction_payload"),
                format_args!("{}", payload.id),
            );
        }
        for pair in surface_construction_scalar_pairs_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("surface_construction_scalar_pair.{}", pair.ordinal),
                format_args!("{}", pair.id),
            );
        }
        for value in surface_construction_strings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("surface_construction_string.{}", value.ordinal),
                format_args!("{}", value.id),
            );
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
                    &mut source_properties,
                    format_args!(
                        "surface_construction_branch.{}.member.{}",
                        branch.ordinal(),
                        ordinal
                    ),
                    data_block.as_deref(),
                    token.value(),
                );
            }
            let (token, data_block) = branch.references.terminal();
            insert_source_property_reference(
                &mut source_properties,
                format_args!("surface_construction_branch.{}.terminal", branch.ordinal()),
                data_block.as_deref(),
                token.value(),
            );
        }
        for (ordinal, block_use) in sketch_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_named_point_block_use.{ordinal}"),
                format_args!("{}", block_use.id),
            );
        }
        for (ordinal, point_use) in sketch_preceding_named_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_preceding_named_point_use.{ordinal}"),
                format_args!("{}", point_use.id),
            );
        }
        for (ordinal, point_use) in sketch_point_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_point_use.{ordinal}"),
                format_args!("{}", point_use.id),
            );
        }
        for (ordinal, group) in sketch_point_groups_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("sketch_point_group.{ordinal}"),
                format_args!("{}", group.id),
            );
        }
        for reference in extrude_profile_references_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("extrude_profile_reference_record.{}", reference.ordinal),
                format_args!("{}", reference.id),
            );
            insert_source_property_reference(
                &mut source_properties,
                format_args!("extrude_profile_reference.{}", reference.ordinal),
                reference.data_block.as_deref(),
                reference.token.value(),
            );
        }
        if let Some(profile) = extrude_construction_profiles_by_operation.get(label.id.as_str()) {
            insert_source_property(
                &mut source_properties,
                format_args!("extrude_construction_profile"),
                format_args!("{}", profile.id),
            );
        }
        for operand in operation_body_operands_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property_reference(
                &mut source_properties,
                format_args!(
                    "operation_body_operand.{}.{}",
                    operand.body_reference_ordinal, operand.ordinal
                ),
                operand.operand_data_block.as_deref(),
                operand.operand.atom.value(),
            );
            insert_source_property(
                &mut source_properties,
                format_args!(
                    "operation_body_operand_record.{}.{}",
                    operand.body_reference_ordinal, operand.ordinal
                ),
                format_args!("{}", operand.id),
            );
            for (binding_ordinal, binding) in operand.segment_body_bindings.iter().enumerate() {
                insert_source_property(
                    &mut source_properties,
                    format_args!(
                        "operation_body_operand_segment_binding.{}.{}.{}",
                        operand.body_reference_ordinal, operand.ordinal, binding_ordinal
                    ),
                    format_args!("{binding}"),
                );
            }
        }
        for binding in parameter_bindings_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
        {
            insert_source_property(
                &mut source_properties,
                format_args!(
                    "input_parameter_declaration.{}.{}",
                    binding.input_slot, binding.reference_ordinal
                ),
                format_args!("{}", binding.expression_declaration),
            );
            if let Some(expression) = &binding.expression {
                insert_source_property(
                    &mut source_properties,
                    format_args!(
                        "input_parameter_expression.{}.{}",
                        binding.input_slot, binding.reference_ordinal
                    ),
                    format_args!("{expression}"),
                );
            }
        }
        for (ordinal, parameter_use) in parameter_uses_by_operation
            .get(label.id.as_str())
            .into_iter()
            .flatten()
            .enumerate()
        {
            insert_source_property(
                &mut source_properties,
                format_args!("parameter_use.{ordinal}"),
                format_args!("{}", parameter_use.id),
            );
        }
        let operation_payload_string_records = payload_strings_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);

        let operation_payload_strings = operation_payload_string_records
            .iter()
            .map(|value| value.value.as_str())
            .collect::<Vec<_>>();
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
                Some(dimensions) => block_placement(ir, dimensions, &outputs),
                None => None,
            }
        } else {
            None
        };
        if outputs.is_empty() {
            if let Some((body, _)) = &block_projection {
                outputs.push(body.clone());
            }
        }
        let sphere_projection = if label.value == "SPHERE" {
            sphere_body_projection(ir, &outputs)
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
                outputs.push(body.clone());
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
        } else {
            None
        };
        let trim_body_projection = if label.value == "TRIM BODY" {
            if let Some(primary) = body_references.get(label.id.as_str()) {
                Some(trim_body_feature_definition(
                    *primary,
                    operation_body_operands_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    &body_alias_roots,
                    &bodies_by_object_index,
                )?)
            } else {
                offset_store_trim_body_feature_definition(
                    offset_store_bodies_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                    operation_body_operands_by_operation
                        .get(label.id.as_str())
                        .map_or([].as_slice(), Vec::as_slice),
                )
            }
        } else {
            None
        };
        let offset_projection = if label.value == "OFFSET" {
            offset_surface_feature_definition(ir, &outputs)
        } else {
            None
        };
        if let Some((_, supports)) = &offset_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                insert_source_property(
                    &mut source_properties,
                    format_args!("offset_support_surface.{support_ordinal}"),
                    format_args!("{}", support.as_str()),
                );
            }
        }
        let thicken_projection = if label.value == "THICKEN_SHEET" {
            thicken_feature_definition(ir, &outputs)
        } else {
            None
        };
        if let Some((_, supports)) = &thicken_projection {
            for (support_ordinal, support) in supports.iter().enumerate() {
                insert_source_property(
                    &mut source_properties,
                    format_args!("thicken_support_surface.{support_ordinal}"),
                    format_args!("{}", support.as_str()),
                );
            }
        }
        let blend_family = match label.value.as_str() {
            "BLEND" => Some(NxBlendFamily::Edge),
            "FACE_BLEND" => Some(NxBlendFamily::Face),
            _ => None,
        };
        let blend_projection = if let Some(family) = blend_family {
            blend_feature_definition(ir, &outputs, family)
        } else {
            None
        };
        if let Some((_, surfaces)) = &blend_projection {
            for (surface_ordinal, surface) in surfaces.iter().enumerate() {
                insert_source_property(
                    &mut source_properties,
                    format_args!("blend_result_surface.{surface_ordinal}"),
                    format_args!("{}", surface.as_str()),
                );
            }
        }
        let extrude_projection = if label.value == "EXTRUDE" {
            let mut output_kinds = Vec::new();
            for output in &outputs {
                let Some(body) = ir.model.bodies.iter().find(|body| body.id == *output) else {
                    output_kinds.clear();
                    break;
                };
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
            field.map(|field| {
                delete_body_feature_definition(field, &body_alias_roots, &bodies_by_object_index)
            })
        } else {
            None
        };
        let extract_body_projection = if label.value == "EXTRACT_BODY" {
            Some(extract_body_feature_definition(
                body_references.get(label.id.as_str()).copied(),
                offset_store_bodies_by_operation
                    .get(label.id.as_str())
                    .map_or([].as_slice(), Vec::as_slice),
                &body_alias_roots,
                &bodies_by_object_index,
            ))
        } else {
            None
        };
        let operation_parameter_uses = parameter_uses_by_operation
            .get(label.id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let native_parameters = native_feature_parameters(operation_parameter_uses, expressions);
        let sketch = if label.value == "SKETCH" {
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
            brep_feature_definition(&outputs)
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
                None => {
                    body_writing_unresolved_feature_definition(&label.value, &source_properties)?
                }
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
                        placements.push(placement.clone());
                    }
                }
                non_boolean_feature_definition_with_parameters(
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
        annotations.note(&id, &stream, label.source_offset, Some("FEATURE_OPERATION"));
        annotations.exactness(&id, Exactness::Derived);
        let source_content = feature_source_content(operation_payload_string_records)?;
        let mut referenced_parameters = Vec::new();
        for parameter_use in operation_parameter_uses {
            push_referenced_parameter(&mut referenced_parameters, &parameter_use.expression);
        }
        if let Some(dimensions) = block_dimensions_by_operation.get(label.id.as_str()) {
            for dimension in &dimensions.dimensions {
                push_referenced_parameter(&mut referenced_parameters, &dimension.expression);
            }
        }
        for owner in parameter_owner_dependencies(&parameter_owners, &referenced_parameters) {
            push_unique_feature_dependency(&mut dependencies, &owner);
        }
        if !source_content.is_empty() {
            annotations
                .derived(&id, "source_content")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        let native_output = (!deletes_body).then_some(native_primary_body).flatten();
        let offset_store_output = (!deletes_body)
            .then_some(offset_store_primary_body)
            .flatten();
        body_writer_history.record_writer(native_output, offset_store_output, &outputs, &id);
        for write in operation_body_writes {
            let writer = id.clone();
            body_identity_writers.insert(write.frame.body_identity(), writer);
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
                body_writer_history.record_writer(native_target, offset_store_target, &[], &id);
            }
        }
        ir.model.features.push(Feature {
            id: id.clone(),
            ordinal: base_ordinal + cadmpeg_core::decode::u64_from_index(ordinal),
            name: Some(label.value.clone()),
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(dependencies)
                .map_err(cadmpeg_core::CodecError::from)?,
            source_properties: cadmpeg_core::text::named_entries(&label.id, source_properties)?,
            source_tag: Some(label.value.clone()),
            source_text: None,
            source_content,

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                definition,
                DistinctMembers::try_from(outputs).map_err(CodecError::from)?,
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
                    write.id.as_str(),
                    operation_body_partition_uses,
                    body_write_group_partition_uses,
                    parasolid_group_members,
                )?;
                let body = cadmpeg_core::text::NonBlankString::new(format!(
                    "{BODY_PREFIX}{:010}",
                    write.frame.body_identity()
                ))
                .ok_or_else(|| CodecError::malformed("NX feature result body identity is blank"))?;
                append_feature_result_topology(
                    ir,
                    PendingFeatureResult {
                        result_id: result_topology_id(key, Some(write.ordinal))?,
                        output_of: &id,
                        bodies: vec![body],
                        members: result_members,
                        native_ref: write.id.clone(),
                    },
                    losses,
                );
            }
        } else if !deletes_body {
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
                append_feature_result_topology(
                    ir,
                    PendingFeatureResult {
                        result_id: result_topology_id(key, None)?,
                        output_of: &id,
                        bodies: vec![local_id],
                        members: FeatureResultGroupMembers::default(),
                        native_ref,
                    },
                    losses,
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
                const WITNESS_VALUE: &str = "primary-body-relations";

                initial_feature.source_properties.insert(
                    cadmpeg_core::text::NonBlankString::new(
                        NATIVE_PRIMARY_BODY_CLOSURE_WITNESS.to_owned(),
                    )
                    .ok_or_else(|| {
                        CodecError::malformed("NX primary body closure witness key is blank")
                    })?,
                    WITNESS_VALUE.to_owned(),
                );
                annotations
                    .derived(initial_body_id, NATIVE_PRIMARY_BODY_CLOSURE_WITNESS)
                    .map_err(cadmpeg_core::CodecError::from)?;
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
    key: &str,
    ordinal: Option<u32>,
) -> Result<FeatureResultTopologyId, CodecError> {
    let owned_key = match ordinal {
        Some(ordinal) => format!("{key}-{ordinal:010}"),
        None => key.to_owned(),
    };
    IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
        .try_id::<FeatureResultTopologyId>(
            &cadmpeg_ir::identity_component!("result-topology"),
            owned_key,
        )
        .ok_or_else(|| CodecError::malformed("NX operation label key is not identity key text"))
}

/// A native result claim held until its member lists pass admission.
struct PendingFeatureResult<'feature> {
    result_id: FeatureResultTopologyId,
    output_of: &'feature FeatureId,
    bodies: Vec<cadmpeg_core::text::NonBlankString>,
    members: FeatureResultGroupMembers,
    native_ref: String,
}

fn append_feature_result_topology(
    ir: &mut CadIr,
    pending: PendingFeatureResult<'_>,
    losses: &mut Vec<LossNote>,
) {
    let PendingFeatureResult {
        result_id,
        output_of,
        bodies,
        members,
        native_ref,
    } = pending;
    let members = cadmpeg_ir::features::FeatureResultMembers::new(
        bodies,
        members.faces,
        members.edges,
        members.vertices,
    );
    let members = match members {
        Ok(members) => members,
        Err(error) => {
            let message = format!("Feature result {result_id} has unusable member claims: {error}; native result {native_ref} is retained");
            losses.push(NxLossCode::FeatureOutputLineageIncomplete.note(message));
            return;
        }
    };

    let result =
        FeatureResultTopology::new(result_id, output_of.clone(), members, Some(native_ref));
    ir.model.feature_result_topologies.push(result);
}

fn operation_body_write_result_group_members(
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
    partition_stream_ordinal: u32,
    member_ids: &[String],
    members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Result<FeatureResultGroupMembers, CodecError> {
    use crate::native::parasolid::group_member::{GroupMemberTarget, GroupNodeFamily};
    let mut result = FeatureResultGroupMembers::default();
    for member_id in member_ids {
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
        let identity = cadmpeg_core::text::NonBlankString::new(format!(
            "nx:s{partition_stream_ordinal}:{kind}#{xmt}"
        ))
        .ok_or_else(|| CodecError::malformed("NX result member identity is blank"))?;
        output.push(identity);
    }
    Ok(result)
}

/// Select the exact native writer identity for one intermediate body result.
/// A primary-body writer is canonical when both native forms are present. The
/// Boolean target is independently sufficient when the primary form is absent.
fn native_result_body_identity(
    primary: Option<&crate::native::features::FeatureBodyReference>,
    boolean: Option<&crate::native::features::FeatureBooleanOperation>,
) -> Option<(cadmpeg_core::text::NonBlankString, String)> {
    let (native, suffix) = if let Some(primary) = primary {
        (primary.id.as_str(), "")
    } else {
        let boolean = boolean?;
        (boolean.id.as_str(), ":target")
    };
    let local = cadmpeg_core::text::NonBlankString::new(format!("{native}{suffix}"))?;
    Some((local, native.to_owned()))
}

/// Return primary body fields that are proven to use the segment-object
/// namespace. An offset-store field may enter that namespace only when the
/// feature extractor has also retained one unique segment alias use for the
/// same field. Missing or ambiguous relations remain offset-store-local. An
/// operation with zero or multiple body fields has no primary-body selection.
fn native_primary_body_references<'a>(
    references: &'a [crate::native::features::FeatureBodyReference],
    data_block_uses: &[crate::native::features::FeatureBodyDataBlockUse],
    segment_uses: &[crate::native::features::FeatureBodySegmentUse],
    inputs: &[crate::native::features::FeatureInputBlock],
    data_blocks: &[crate::native::om::DataBlock],
) -> BTreeMap<&'a str, u32> {
    let unique_references = crate::native::features::unique_feature_body_references(references);
    let mut offset_store_references = BTreeSet::new();
    for use_ in data_block_uses {
        offset_store_references.insert(use_.feature_body_reference.as_str());
    }
    let offset_store_operations =
        crate::native::features::feature_input_store_operations(inputs, data_blocks);
    let mut bridged_segment_references = BTreeSet::new();
    for use_ in segment_uses {
        bridged_segment_references.insert(use_.feature_body_reference.as_str());
    }
    let mut output = BTreeMap::new();
    for (operation, reference) in unique_references {
        if !bridged_segment_references.contains(reference.id.as_str())
            && (offset_store_references.contains(reference.id.as_str())
                || offset_store_operations.contains(reference.operation_label.as_str()))
        {
            continue;
        }

        output.insert(operation, reference.body.value());
    }
    output
}

fn attach_sketch_graph(
    ir: &mut CadIr,
    label: &crate::native::features::FeatureOperationLabel,
    sources: &SketchSources<'_>,
    annotations: &mut AnnotationBuilder,
    stream: &cadmpeg_ir::annotations::StreamHandle,
) -> Option<SketchId> {
    let mut operation_groups = Vec::new();
    for group in sources
        .point_groups
        .iter()
        .filter(|group| group.operation_label == label.id)
    {
        operation_groups.push(group);
    }
    let operation_key = label
        .id
        .strip_prefix("nx:feature-history:operation-label#")
        .unwrap_or(label.id.as_str());
    let sketch_id: SketchId = IdScope::native(cadmpeg_ir::identity_component!("feature-history"))
        .try_id(
        &cadmpeg_ir::identity_component!("sketch"),
        operation_key.to_owned(),
    )?;
    let mut operation_fixed_points = Vec::new();
    for &point in sources
        .fixed_points
        .iter()
        .filter(|point| point.operation_label == label.id)
    {
        operation_fixed_points.push(point);
    }
    if operation_groups.is_empty() {
        let mut coordinate_pairs = Vec::new();
        for &pair in sources
            .coordinate_pairs
            .iter()
            .filter(|pair| pair.operation_label == label.id)
        {
            coordinate_pairs.push(pair);
        }
        if coordinate_pairs.is_empty() && operation_fixed_points.is_empty() {
            return None;
        }
        let mut entities = Vec::new();
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
            let entity_id = sketch_entity_identity("coordinate-pair-", pair_key)?;

            let native_kind = cadmpeg_core::text::NonBlankString::new("nx-coordinate-pair")?;
            let native_ref = pair.id.clone();
            entities.push((
                pair.source_offset,
                SketchEntity::new(
                    entity_id,
                    sketch_id.clone(),
                    SketchGeometry::native(native_kind),
                )
                .with_native_ref(Some(native_ref)),
            ));
        }
        if !append_fixed_sketch_entities(&mut entities, label, &sketch_id, &operation_fixed_points)
        {
            return None;
        }
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
            annotations.note(entity.id().as_str(), stream, *source_offset, Some(tag));
            annotations.exactness(entity.id().as_str(), Exactness::ByteExact);
        }
        annotations.note(
            sketch_id.as_str(),
            stream,
            label.source_offset,
            Some("SKETCH"),
        );
        annotations.exactness(sketch_id.as_str(), Exactness::Derived);
        emit_sketch(ir, label, &sketch_id, entities);
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
    for group in operation_groups {
        let point_use = point_uses_by_group.get(group.id.as_str()).copied();
        let source_offset =
            sketch_group_source_offset(label, group, point_use, &points_by_id, &scalars_by_id)?;
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
        let entity_id = sketch_entity_identity("point-", entity_key)?;
        let native_ref = native_ref_source.to_owned();
        let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(group.coordinates[0], group.coordinates[1]),
        }) else {
            return None;
        };
        entities.push((
            source_offset,
            SketchEntity::new(entity_id, sketch_id.clone(), geometry)
                .with_native_ref(Some(native_ref)),
        ));
    }
    if !append_fixed_sketch_entities(&mut entities, label, &sketch_id, &operation_fixed_points) {
        return None;
    }
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
                annotations.note(
                    entity.id().as_str(),
                    stream,
                    *source_offset,
                    Some("SKETCH_POINT"),
                );
                annotations.exactness(entity.id().as_str(), Exactness::Derived);
            }
            SketchGeometryDefinition::Native { native_kind } => {
                let tag = if native_kind == "nx-fixed-point" {
                    "SKETCH_NATIVE_FIXED_POINT"
                } else {
                    "SKETCH_NATIVE"
                };
                annotations.note(entity.id().as_str(), stream, *source_offset, Some(tag));
                annotations.exactness(entity.id().as_str(), Exactness::ByteExact);
            }
            _ => return None,
        }
    }
    annotations.note(
        sketch_id.as_str(),
        stream,
        label.source_offset,
        Some("SKETCH"),
    );
    annotations.exactness(sketch_id.as_str(), Exactness::Derived);
    emit_sketch(ir, label, &sketch_id, entities);
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

fn sketch_group_source_offset(
    label: &crate::native::features::FeatureOperationLabel,
    group: &crate::native::features::FeatureSketchPointGroup,
    point_use: Option<&crate::native::features::FeatureSketchPointUse>,
    points_by_id: &BTreeMap<&str, &crate::native::features::FeatureSketchPoint>,
    scalars_by_id: &BTreeMap<&str, &crate::native::features::FeaturePayloadScalar>,
) -> Option<u64> {
    if let Some(point_use) = point_use {
        return point_use
            .references
            .iter()
            .map(|reference| reference.source_offset)
            .min();
    }
    let mut minimum = None::<u64>;
    for point_id in &group.points {
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
        let [first, second] = point.scalar_fields.as_slice() else {
            return None;
        };
        for (scalar_id, coordinate) in [first, second].into_iter().zip(group.coordinates) {
            let scalar = scalars_by_id.get(scalar_id.as_str()).copied()?;
            if scalar.operation_label != label.id
                || scalar.scalar.value().get().to_bits() != coordinate.to_bits()
            {
                return None;
            }
            minimum = Some(minimum.map_or(scalar.source_offset, |current| {
                current.min(scalar.source_offset)
            }));
        }
    }
    minimum
}

fn sketch_entity_identity(prefix: &str, key: &str) -> Option<SketchEntityId> {
    IdScope::native(cadmpeg_ir::identity_component!("feature-history")).try_id(
        &cadmpeg_ir::identity_component!("sketch-entity"),
        format!("{prefix}{key}"),
    )
}

fn emit_sketch(
    ir: &mut CadIr,
    label: &crate::native::features::FeatureOperationLabel,
    sketch_id: &SketchId,
    entities: Vec<(u64, SketchEntity)>,
) {
    let name = label.value.clone();
    let native_ref = label.id.clone();
    ir.model
        .sketch_entities
        .extend(entities.into_iter().map(|(_, entity)| entity));
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some(name),
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some(native_ref),
    });
}

fn native_fixed_point_entities(
    label: &crate::native::features::FeatureOperationLabel,
    sketch_id: &SketchId,
    points: &[&crate::native::features::FeatureSketchFixedPoint],
) -> Option<Vec<(u64, SketchEntity)>> {
    let mut point_ids = BTreeSet::new();
    let mut entity_keys = BTreeSet::new();
    let mut entities = Vec::new();
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
        let entity_id = sketch_entity_identity("fixed-point-", point_key)?;

        let native_kind = cadmpeg_core::text::NonBlankString::new("nx-fixed-point")?;
        let native_ref = point.id.clone();
        entities.push((
            point.source_offset,
            SketchEntity::new(
                entity_id,
                sketch_id.clone(),
                SketchGeometry::native(native_kind),
            )
            .with_native_ref(Some(native_ref)),
        ));
    }
    Some(entities)
}

fn append_fixed_sketch_entities(
    entities: &mut Vec<(u64, SketchEntity)>,
    label: &crate::native::features::FeatureOperationLabel,
    sketch_id: &SketchId,
    fixed_points: &[&crate::native::features::FeatureSketchFixedPoint],
) -> bool {
    let Some(fixed_entities) = native_fixed_point_entities(label, sketch_id, fixed_points) else {
        return false;
    };

    entities.extend(fixed_entities);
    true
}

struct SegmentBindingBodyIndexes<'a> {
    by_object: BTreeMap<u32, Vec<BodyId>>,
    by_binding: BTreeMap<&'a str, Vec<BodyId>>,
}

fn segment_binding_body_indexes<'a>(
    ir: &CadIr,
    bindings: &'a [crate::native::segments::SegmentBodyBinding],
) -> Result<SegmentBindingBodyIndexes<'a>, CodecError> {
    let mut by_object = BTreeMap::<u32, Vec<BodyId>>::new();
    let mut by_binding = BTreeMap::<&str, Vec<BodyId>>::new();
    for binding in bindings {
        let (prefix, prefix_len) = stream_prefix(binding.stream_ordinal, false)?;
        let mut stream_bodies = Vec::new();

        for body in ir.model.bodies.iter().filter(|body| {
            body.id
                .as_str()
                .as_bytes()
                .starts_with(&prefix[..prefix_len])
        }) {
            if stream_bodies.contains(&body.id) {
                continue;
            }

            stream_bodies.push(body.id.clone());
        }
        for identity in [binding.body_object_index, binding.body_alias_object_index] {
            for body in &stream_bodies {
                if by_object
                    .get(&identity)
                    .is_some_and(|bodies| bodies.contains(body))
                {
                    continue;
                }
                let body = body.clone();
                by_object.entry(identity).or_default().push(body);
            }
        }

        by_binding.insert(binding.id.as_str(), stream_bodies);
    }
    Ok(SegmentBindingBodyIndexes {
        by_object,
        by_binding,
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
    properties: &mut BTreeMap<String, String>,
    operation_label: &str,
    records: &[crate::native::features::operation_record::FeatureOperationRecord],
    common_frames: &[crate::native::features::FeatureOperationCommonFrame],
    terminal_frames: &[crate::native::features::FeatureOperationTerminalFrame],
) {
    let mut matching_records = records
        .iter()
        .filter(|record| record.operation_label == operation_label);
    let Some(record) = matching_records.next() else {
        return;
    };
    if matching_records.next().is_some() {
        return;
    }

    properties.insert("operation_record".to_owned(), record.id.clone());
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
            properties.insert(format!("{PREFIX}{}", frame.ordinal), frame.id.clone());
        }
    }
    let mut matching_frames = terminal_frames
        .iter()
        .filter(|frame| frame.operation_record == record.id);
    let Some(frame) = matching_frames.next() else {
        return;
    };
    if matching_frames.next().is_some() {
        return;
    }
    properties.insert("operation_terminal_frame".to_owned(), frame.id.clone());
}

fn insert_source_property(
    properties: &mut BTreeMap<String, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) {
    properties.insert(std::fmt::format(key), std::fmt::format(value));
}

fn insert_source_property_reference(
    properties: &mut BTreeMap<String, String>,
    key: std::fmt::Arguments<'_>,
    data_block: Option<&str>,
    fallback: impl std::fmt::Display,
) {
    match data_block {
        Some(data_block) => insert_source_property(properties, key, format_args!("{data_block}")),
        None => insert_source_property(properties, key, format_args!("{fallback}")),
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
    let mut uses_by_entity = group_pairs(
        sources
            .string_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
    );
    for uses in uses_by_entity.values_mut() {
        uses.sort_by_key(|left| left.position);
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
            annotations.note(
                id.as_str(),
                &source_stream,
                string.inflated_offset,
                Some("ENTITY_54_STRING_ATTRIBUTE"),
            );
            annotations
                .derived(id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::from)?;
            let field_name = attribute_index
                .attribute_names
                .field_name(reference, string_use.id.as_str());
            let name = topology_attribute_name(
                field_name,
                attribute_index
                    .class_names
                    .get(reference.id.as_str())
                    .and_then(Option::as_ref)
                    .copied(),
                "84",
                string_use.position.reference_ordinal(),
            );
            let values = vec![AttributeValue::String(string.value.as_str().to_owned())];
            push_topology_attribute(ir, context, id, name, values);
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
        let field_use = self
            .fields_by_value_use
            .get(value_use)
            .and_then(Option::as_ref)?;
        let class_use = self
            .classes_by_entity
            .get(&(
                topology_reference.id.as_str(),
                field_use.entity_51_record.as_str(),
            ))
            .and_then(Option::as_ref)?;
        if field_use.attribute_class_use != class_use.attribute_class_use
            || field_use.attribute_definition != class_use.attribute_definition
        {
            return None;
        }
        let definition = self
            .definitions_by_id
            .get(class_use.attribute_definition.as_str())
            .and_then(Option::as_ref)?;
        let field_name = match (definition.name.as_str(), field_use.position.field_ordinal()) {
            ("SDL/TYSA_DENSITY", 0) => std::borrow::Cow::Borrowed("density"),
            ("SDL/TYSA_DENSITY", 1) => std::borrow::Cow::Borrowed("units"),
            _ => {
                if let Some(names) = self
                    .field_names_by_definition
                    .get(definition.id.as_str())
                    .and_then(Option::as_ref)
                {
                    let name = names
                        .fields
                        .get(cadmpeg_core::decode::index_from_u32(
                            field_use.position.field_ordinal(),
                        ))
                        .map(|field| field.name.as_str())?;
                    std::borrow::Cow::Borrowed(name)
                } else {
                    std::borrow::Cow::Owned(format!(
                        "field_{}.parasolid_type_{}",
                        field_use.position.field_ordinal(),
                        field_use.value_kind.field_code().code()
                    ))
                }
            }
        };
        Some(format!("{}.{field_name}", definition.name.as_str()))
    }
}

fn topology_attribute_name(
    field_name: Option<String>,
    class_name: Option<&str>,
    family: &str,
    reference_ordinal: u32,
) -> String {
    if let Some(name) = field_name {
        return name;
    }
    let fallback = format!("parasolid_type_{family}_reference_{reference_ordinal}");
    match class_name {
        Some(class_name) => format!("{class_name}.{fallback}"),
        None => fallback,
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
) -> BTreeMap<&'a str, Option<&'a str>> {
    let mut classes_by_reference = BTreeMap::<&str, Option<&str>>::new();
    for class_use in class_uses {
        for definition in definitions
            .iter()
            .filter(|definition| definition.id == class_use.attribute_definition)
        {
            let key = class_use.topology_attribute_reference.as_str();
            let name = definition.name.as_str();

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
    classes_by_reference
}

fn parasolid_topology_attribute_targets(ir: &CadIr) -> BTreeMap<String, AttributeTarget> {
    let mut targets = BTreeMap::new();
    for shell in &ir.model.shells {
        targets.insert(
            shell.id.to_string(),
            AttributeTarget::Shell(shell.id.clone()),
        );
    }
    for face in &ir.model.faces {
        targets.insert(face.id.to_string(), AttributeTarget::Face(face.id.clone()));
    }
    for loop_ in &ir.model.loops {
        targets.insert(
            loop_.id.to_string(),
            AttributeTarget::Loop(loop_.id.clone()),
        );
    }
    for edge in &ir.model.edges {
        targets.insert(edge.id.to_string(), AttributeTarget::Edge(edge.id.clone()));
    }
    for coedge in &ir.model.coedges {
        targets.insert(
            coedge.id.to_string(),
            AttributeTarget::Coedge(coedge.id.clone()),
        );
    }
    for vertex in &ir.model.vertices {
        targets.insert(
            vertex.id.to_string(),
            AttributeTarget::Vertex(vertex.id.clone()),
        );
    }
    targets
}

struct ParasolidTopologyAttributeContext<'a> {
    reference: &'a crate::native::parasolid::ParasolidTopologyAttributeListReference,
    entity: &'a str,
    id_suffix: Option<cadmpeg_ir::ids::IdentityKey>,
    target: AttributeTarget,
}

struct ParasolidTopologyAttributeIndex<'a> {
    class_names: BTreeMap<&'a str, Option<&'a str>>,
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
    ) -> crate::native::attach::ParasolidTopologyAttributeIndex<'a> {
        let attribute_names =
            ParasolidAttributeNameIndex::new(class_uses, definitions, field_uses, field_names);
        let class_names = parasolid_topology_attribute_class_names(class_uses, definitions);
        Self {
            class_names,
            attribute_names,
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
        let key = class_use.topology_attribute_reference.as_str();

        let entities = entities_by_reference.entry(key).or_default();

        entities.insert(class_use.entity_51_record.as_str());
    }
    let mut references_by_target = BTreeMap::<String, Vec<_>>::new();
    for reference in topology_references {
        let kind = reference.topology_type.as_str();
        let key = format!(
            "nx:s{}:{kind}#{}",
            reference.stream_ordinal, reference.topology_xmt
        );
        references_by_target.entry(key).or_default().push(reference);
    }
    let emitted_targets = parasolid_topology_attribute_targets(ir);
    let mut contexts = Vec::new();
    for (target_key, references) in references_by_target {
        let Some(&reference) = references.first().filter(|_| references.len() == 1) else {
            continue;
        };

        let Some(target) = emitted_targets.get(target_key.as_str()) else {
            continue;
        };
        let mut entities = BTreeSet::new();
        if let Some(entity) = reference.attribute_list_record.as_deref() {
            entities.insert(entity);
        }

        if let Some(class_entities) = entities_by_reference.get(reference.id.as_str()) {
            for entity in class_entities {
                entities.insert(entity);
            }
        }
        let multiple_entities = entities.len() > 1;
        for entity in entities {
            let id_suffix = if multiple_entities {
                entity_suffix_key(entity)
            } else {
                None
            };

            contexts.push(ParasolidTopologyAttributeContext {
                reference,
                entity,
                id_suffix,
                target: target.clone(),
            });
        }
    }
    contexts
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

fn mapped_vector_attribute_values<T, const N: usize>(
    input: &[T],
    map: impl Fn(&T) -> [FiniteReal; N],
) -> Vec<AttributeValue> {
    input
        .iter()
        .map(|item| AttributeValue::Vector(map(item).to_vec()))
        .collect()
}

fn push_topology_attribute(
    ir: &mut CadIr,
    context: &ParasolidTopologyAttributeContext<'_>,
    id: AttributeId,
    name: String,
    values: Vec<AttributeValue>,
) {
    ir.model.attributes.push(SourceAttribute {
        id,
        target: context.target.clone(),
        name,
        values,
    });
}

/// The key half of an entity reference, which is what an id suffix names.
///
/// The reference reaches this as stored record text, so it is admitted here;
/// text that is not key text names no suffix.
fn entity_suffix_key(entity: &str) -> Option<cadmpeg_ir::ids::IdentityKey> {
    let suffix = entity.rsplit_once('#').map_or(entity, |(_, key)| key);
    cadmpeg_ir::ids::IdentityKey::try_new(suffix).ok()
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
    let mut uses_by_entity = group_pairs(
        sources
            .numeric_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
    );
    for uses in uses_by_entity.values_mut() {
        uses.sort_by_key(|left| left.position);
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
            annotations.note(id.as_str(), &source_stream, source_offset, Some(tag));
            annotations
                .derived(id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::from)?;
            let field_name = attribute_index
                .attribute_names
                .field_name(reference, numeric_use.id.as_str());
            let name = topology_attribute_name(
                field_name,
                attribute_index
                    .class_names
                    .get(reference.id.as_str())
                    .and_then(Option::as_ref)
                    .copied(),
                lane,
                numeric_use.position.reference_ordinal(),
            );
            push_topology_attribute(ir, context, id, name, values);
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
    let mut uses_by_entity = group_pairs(
        sources
            .structured_uses
            .iter()
            .map(|value_use| (value_use.entity_51_record.as_str(), value_use)),
    );
    for uses in uses_by_entity.values_mut() {
        uses.sort_by_key(|left| left.position);
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
                        mapped_vector_attribute_values(record.values.as_slice(), |value| {
                            value.finite_components()
                        }),
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
                        mapped_vector_attribute_values(record.values.as_slice(), |axis| {
                            let first = axis[0].finite_components();
                            let second = axis[1].finite_components();
                            [
                                first[0], first[1], first[2], second[0], second[1], second[2],
                            ]
                        }),
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
            annotations.note(id.as_str(), &source_stream, source_offset, Some(tag));
            annotations
                .derived(id.as_str(), "target")
                .map_err(cadmpeg_core::CodecError::from)?;
            annotations
                .derived(id.as_str(), "name")
                .map_err(cadmpeg_core::CodecError::from)?;
            let field_name = attribute_index
                .attribute_names
                .field_name(reference, structured_use.id.as_str());
            let name = topology_attribute_name(
                field_name,
                attribute_index
                    .class_names
                    .get(reference.id.as_str())
                    .and_then(Option::as_ref)
                    .copied(),
                family,
                structured_use.position.reference_ordinal(),
            );
            push_topology_attribute(ir, context, id, name, values);
        }
    }
    ir.model
        .attributes
        .sort_by(|first, second| first.id.as_str().cmp(second.id.as_str()));
    Ok(())
}

fn push_referenced_parameter(referenced: &mut Vec<ParameterId>, expression: &str) {
    if let Some(id) = expressions::expression_parameter_id(expression) {
        referenced.push(id);
    }
}

fn push_unique_feature_dependency(dependencies: &mut Vec<FeatureId>, candidate: &FeatureId) {
    if dependencies.contains(candidate) {
        return;
    }

    dependencies.push(candidate.clone());
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
) -> Result<Option<SemanticAnnotation>, CodecError> {
    const FONT_KEY: &str = "font_family";
    let [text, font_family] = payload_strings else {
        return Ok(None);
    };
    let Some(id) = extended_id(native_ref, &cadmpeg_ir::identity_key!("semantic-text")) else {
        return Ok(None);
    };
    let key = cadmpeg_core::text::NonBlankString::new(FONT_KEY.to_owned())
        .ok_or_else(|| CodecError::malformed("NX TEXT annotation font key is blank"))?;
    let mut parameters = BTreeMap::new();
    parameters.insert(key, (*font_family).to_owned());
    Ok(Some(SemanticAnnotation {
        id,
        object: native_ref.to_owned(),
        kind: SemanticAnnotationKind::Text,
        runtime_type: "TEXT".to_owned(),
        order,
        text: vec![(*text).to_owned()],
        references: BTreeMap::new(),
        value: None,
        format: None,
        position: None,
        parameters,
        assets: Vec::new(),
        native_ref: native_ref.to_owned(),
    }))
}

pub(super) fn parameter_owner_dependencies(
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
    operation: &crate::native::features::FeatureBooleanOperation,
    body_alias_roots: &BTreeMap<u32, u32>,
    offset_store_resolution: &BooleanOffsetStoreResolution,
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> Result<FeatureDefinition, CodecError> {
    let empty_offset_store_body_blocks = BTreeMap::new();
    let native_target = format!("nx:om-object-index#{}", operation.target.token.value());
    let native_tools =
        selection_indices_native(operation.tools.iter().map(|token| token.token.value()));
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
            for tool in &operation.tools {
                tool_indices.push(tool.token.value());
            }
            atomic_disjoint_body_selections(
                feature_body_selection_with_offset_blocks(
                    &[operation.target.token.value()],
                    body_alias_roots,
                    offset_store_body_blocks,
                    bodies_by_object_index,
                    native_target,
                ),
                feature_body_selection_with_offset_blocks(
                    &tool_indices,
                    body_alias_roots,
                    offset_store_body_blocks,
                    bodies_by_object_index,
                    native_tools,
                ),
            )
        }
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
                local_body_selection(&[format!("nx:om-body-object#{body}")], native)
            }
            selection => selection.into_selection(),
        },
        DeleteBodyField::OffsetStore {
            object_index,
            data_block,
        } => local_body_selection(
            &[data_block.to_owned()],
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
    let source = if let Some(body) = body_object_index {
        feature_body_selection(
            &[body],
            body_alias_roots,
            bodies_by_object_index,
            format!("nx:om-object-index#{body}"),
        )
        .into_selection()
    } else if let [(object_index, data_block)] = offset_store_bodies {
        local_body_selection(
            std::slice::from_ref(data_block),
            format!("nx:om-object-index#{object_index}"),
        )
    } else {
        BodySelection::Unresolved
    };
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
    let mut tool_data_blocks = Vec::new();
    let mut complete = true;
    for operand in operands {
        let Some(block) = operand.operand_data_block.as_deref() else {
            complete = false;
            break;
        };
        tool_data_blocks.push(block);
    }
    let tools = if let (false, Some(primary_store), true) =
        (operands.is_empty(), primary_store, complete)
    {
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
            let bodies = tool_data_blocks
                .iter()
                .map(|block| (*block).to_owned())
                .collect::<Vec<_>>();
            let native = selection_indices_native(
                operands.iter().map(|operand| operand.operand.atom.value()),
            );
            local_body_selection(&bodies, native)
        } else {
            BodySelection::Unresolved
        }
    } else {
        BodySelection::Unresolved
    };
    let target = local_body_selection(
        std::slice::from_ref(data_block),
        format!("nx:om-object-index#{object_index}"),
    );
    let Ok(operands) = cadmpeg_ir::features::TrimBodyOperands::new(target, tools) else {
        return None;
    };
    Some(FeatureDefinition::Operation(FeatureOperation::TrimBodies {
        operands,
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
    let indices = std::iter::once(primary_body_object_index)
        .chain(operands.iter().map(|operand| operand.operand.atom.value()));
    let native = selection_indices_native(indices.clone());
    let mut object_indices = Vec::new();
    for index in indices {
        object_indices.push(index);
    }
    let bodies = if primary_segment_body_object_index.is_some() {
        if operands
            .iter()
            .all(|operand| !operand.segment_body_bindings.is_empty())
        {
            feature_body_set_selection(
                &object_indices,
                body_alias_roots,
                bodies_by_object_index,
                native,
            )
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
            blocks.push(block);
        }
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
            let bodies = std::iter::once(primary_data_block)
                .chain(blocks)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            local_body_selection(&bodies, native)
        } else {
            BodySelection::Native(native)
        }
    } else {
        BodySelection::Native(native)
    };
    let Ok(bodies) = bodies.try_into() else {
        return None;
    };
    Some(FeatureDefinition::Operation(FeatureOperation::SewBodies {
        bodies,
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
    let native_tools =
        selection_indices_native(operands.iter().map(|operand| operand.operand.atom.value()));
    let mut tool_object_indices = Vec::new();
    for operand in operands {
        tool_object_indices.push(operand.operand.atom.value());
    }
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

fn operation_body_image_outputs_by_write<'a>(
    uses: &'a [crate::native::features::FeatureOperationBodyImageSegmentUse],
    bodies_by_segment_binding: &BTreeMap<&str, Vec<BodyId>>,
) -> BTreeMap<&'a str, BodyId> {
    let mut unique_uses = BTreeMap::new();
    for use_ in uses {
        let write_key = use_.operation_body_write.as_str();
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

        outputs.insert(write, body.clone());
    }
    outputs
}

fn merge_operation_body_outputs<'a>(
    outputs: &mut BTreeMap<&'a str, BodyId>,
    conflicts: &mut BTreeSet<&'a str>,
    candidates: &BTreeMap<&'a str, BodyId>,
) {
    for (&write, body) in candidates {
        if conflicts.contains(write) {
            continue;
        }
        match outputs.entry(write) {
            Entry::Vacant(entry) => {
                entry.insert(body.clone());
            }
            Entry::Occupied(entry) if entry.get() == body => {}
            Entry::Occupied(entry) => {
                entry.remove();

                conflicts.insert(write);
            }
        }
    }
}

fn operation_body_identity_outputs_by_write<'a>(
    uses: &'a [crate::native::features::FeatureOperationBodyIdentitySegmentUse],
    bodies_by_segment_binding: &BTreeMap<&str, Vec<BodyId>>,
) -> BTreeMap<&'a str, BodyId> {
    let mut outputs = BTreeMap::new();
    for use_ in uses {
        let Some([body]) = bodies_by_segment_binding
            .get(use_.segment_body_binding.as_str())
            .map(Vec::as_slice)
        else {
            continue;
        };
        let write_key = use_.operation_body_write.as_str();
        match outputs.entry(write_key) {
            Entry::Vacant(entry) => {
                entry.insert(body.clone());
            }
            Entry::Occupied(entry) if entry.get() == body => {}
            Entry::Occupied(entry) => {
                entry.remove();
            }
        }
    }
    outputs
}

fn operation_body_group_partition_outputs_by_write<'a>(
    writes: &'a [crate::native::features::FeatureOperationBodyWrite],
    uses: &[crate::native::features::FeatureBodyWriteGroupPartitionUse],
    bodies: &[cadmpeg_ir::topology::Body],
) -> Result<BTreeMap<&'a str, BodyId>, CodecError> {
    let mut partitions_by_identity = BTreeMap::<u8, Option<u32>>::new();
    for use_ in uses {
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

        unique_bodies.insert(identity, body.id.clone());
    }
    let mut outputs = BTreeMap::new();
    for write in writes {
        if let Some(body) = unique_bodies.get(&write.frame.body_identity()) {
            outputs.insert(write.id.as_str(), body.clone());
        }
    }
    Ok(outputs)
}

fn complete_operation_body_image_outputs(
    writes: &[&crate::native::features::FeatureOperationBodyWrite],
    outputs_by_write: &BTreeMap<&str, BodyId>,
) -> Vec<BodyId> {
    let mut outputs = Vec::new();
    for write in writes {
        let Some(body) = outputs_by_write.get(write.id.as_str()) else {
            return Vec::new();
        };

        if outputs.contains(body) {
            return Vec::new();
        }

        outputs.push(body.clone());
    }
    outputs
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
