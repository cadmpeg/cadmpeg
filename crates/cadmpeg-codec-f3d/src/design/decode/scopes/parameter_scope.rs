// SPDX-License-Identifier: Apache-2.0
//! Decode parameter scopes and parse one scope payload.

use crate::bytes::lp_utf16_bounded_scoped;
use cadmpeg_core::decode::u64_from_index;

use super::assembly_alignment::exact_assembly_alignment;
use super::axial_assembly::bind_axial_assembly_operand_targets;
use super::axial_assembly::bind_joint_origin_frames_from_assemblies;
use super::base_feature::exact_base_feature_construction;
use super::coil::bind_coil_extent_from_parameters;
use super::coil::exact_coil_discriminators;
use super::coil::exact_coil_placement;
use super::coil::exact_long_coil_transform;
use super::combine::exact_combine_operation;
use super::component_constructions::bind_component_pattern_occurrences;
use super::component_constructions::exact_component_insert_construction;
use super::component_constructions::exact_copy_paste_component_operation;
use super::component_constructions::exact_derived_instance_construction;
use super::copy_paste_bodies::exact_copy_paste_bodies_operation;
use super::direct_face::exact_direct_face_operation;
use super::direct_face::exact_move_operation;
use super::direct_face::exact_scale_operation;
use super::draft::exact_draft_operation_with_owners;
use super::extrude::exact_extrude_prologue;
use super::extrude::ExtrudeScopeFrame;
use super::fixed_parameters::exact_fixed_chamfer_parameters;
use super::fixed_parameters::exact_fixed_extrude_parameters;
use super::fixed_parameters::exact_fixed_fillet_parameters;
use super::hole::exact_hole_construction;
use super::path_feature::exact_path_feature_construction;
use super::pattern::exact_circular_pattern_construction_with_owners;
use super::pattern::exact_rectangular_pattern_construction;
use super::point_data::exact_work_point_construction;
use super::sheet_metal::bind_hem_operation_from_parameters;
use super::sheet_metal::exact_base_flange_operation;
use super::sheet_metal::exact_edge_flange_operation;
use super::solid_primitive::exact_solid_primitive;
use super::surfaces::exact_ruled_surface_operation;
use super::surfaces::exact_surface_extend_operation;
use super::surfaces::exact_surface_offset_operation;
use super::surfaces::exact_surface_stitch_operation;
use super::thread::exact_thread_construction;
use super::work_geometry::exact_joint_origin_frame;
use super::work_geometry::exact_work_axis_construction;
use super::work_geometry::exact_work_plane_frame;
use crate::container::ContainerScan;
use crate::design::decode::assembly::exact_legacy_as_built_421_operands;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::record_streams::record_stream;
use crate::design::decode::sketch::{
    indexed_record_header_at, native_scope_charged, IndexedRecordOffsets,
};
use crate::design::decode::text::design_record_id_charged;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids;
use crate::records::feature::assembly;
use crate::records::feature::coil;
use crate::records::feature::direct_face;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Decode every canonical sketch or construction-operation scope, including
/// scopes that own no parameters and therefore have no owner-frame backlink.
pub(crate) fn decode_parameter_scopes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    native: &crate::native::F3dNative,
) -> Result<Vec<DesignParameterScope>, CodecError> {
    const STREAMS_OPERATION: &str = "f3d Design parameter-scope streams";
    let types = &native.design_types;
    let mut sketch_entities = None;
    let mut out = Vec::new();
    // Each Design stream decodes once: a repeated entry name names the same
    // payload and would repeat every scope ID.
    let mut names_storage = ctx.reserve_scoped(0, STREAMS_OPERATION)?;
    let mut names = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D parameter-scope streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        names_storage
            .with_storage(|| ctx.push_vec(&mut names, entry.name.as_str(), STREAMS_OPERATION))?;
    }
    ctx.sort_unstable_by(&mut names, |name| *name, Ord::cmp, STREAMS_OPERATION)?;
    ctx.dedup_vec(&mut names, STREAMS_OPERATION)?;
    for name in ctx.admit_iter(&names, STREAMS_OPERATION)? {
        let bytes = scan.entry_bytes(name)?;
        // The stream scope, record index and type table live for this stream only.
        let (stream, _stream_storage) = ctx
            .with_scoped_storage("f3d Design parameter-scope stream", || {
                native_scope_charged(ctx, name)
            })?;
        let (records, _records_storage) = IndexedRecordOffsets::build_scoped(ctx, bytes)?;
        let (stream_types, _stream_types_storage) = ctx
            .with_scoped_storage("f3d Design parameter-scope stream types", || {
                crate::design::decode::meta::stream_types_by_entity(ctx, types, name)
            })?;
        let stream_scope_start = out.len();
        // A scope is delimited by two headers carrying its record index; any
        // header can open one, and the last of its index finds no pair.
        for (record_index, offsets) in records.records(ctx)? {
            for &start in ctx.admit_iter(offsets, "scan F3D parameter-scope record frames")? {
                let Some(header) = indexed_record_header_at(bytes, start) else {
                    continue;
                };
                let Some(mut scope) =
                    parse_scope_at(ctx, bytes, &records, record_index, *header.class_tag, start)?
                else {
                    continue;
                };
                scope.id = design_record_id_charged(
                    ctx,
                    name,
                    ":design-parameter-scope#",
                    scope.byte_offset(),
                    "f3d Design parameter scope ID",
                )?;
                bind_sketch_entity(
                    ctx,
                    bytes,
                    &mut scope,
                    &stream,
                    &native.design_entity_headers,
                    &mut sketch_entities,
                )?;
                bind_scope_constructions(ctx, bytes, &records, &mut scope, &stream_types, native)?;
                ctx.push_vec(&mut out, scope, "f3d Design parameter scopes")?;
            }
        }
        bind_joint_origin_frames_from_assemblies(ctx, bytes, &mut out[stream_scope_start..])?;
        bind_axial_assembly_operand_targets(ctx, bytes, &records, &mut out[stream_scope_start..])?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design parameter_scope 1",
    )?;
    Ok(out)
}

/// Sketch-module entity headers keyed by stream scope and `u32` entity
/// suffix, with the scoped storage that holds the index.
struct SketchEntityIndex<'entities, 'ctx> {
    by_suffix: HashMap<(&'entities str, u32), SketchEntityMatch<'entities>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

/// The entity headers that share one stream scope and suffix.
#[derive(Clone, Copy)]
enum SketchEntityMatch<'entities> {
    Unique(&'entities crate::records::entity_header::DesignEntityHeader),
    Multiple,
}

/// Index every sketch-module entity header whose suffix fits in `u32` under
/// its stream scope and suffix. Each header is visited once.
fn sketch_entity_index<'entities, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    entities: &'entities [crate::records::entity_header::DesignEntityHeader],
) -> Result<SketchEntityIndex<'entities, 'ctx>, CodecError> {
    const OPERATION: &str = "index F3D sketch entity headers";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut by_suffix = HashMap::new();
    for entity in ctx.admit_iter(entities, OPERATION)? {
        if !entity.in_sketch_module() {
            continue;
        }
        let Ok(suffix) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        let Some(stream) = record_stream(ctx, &entity.id)? else {
            continue;
        };
        let key = (stream, suffix);
        if let Some(found) = ctx.get_mut_hash_map(&mut by_suffix, &key, OPERATION)? {
            *found = SketchEntityMatch::Multiple;
            continue;
        }
        storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut by_suffix,
                key,
                SketchEntityMatch::Unique(entity),
                OPERATION,
            )
        })?;
    }
    Ok(SketchEntityIndex {
        by_suffix,
        _storage: storage,
    })
}

/// The only sketch-module entity of `stream` whose suffix a marked reference
/// in `frame` names (a one byte, a `u32` suffix, six zero bytes), with the
/// frame-relative offset of the suffix at its first reference. Two matching
/// entities leave the binding unresolved, and the scan stops there.
fn unique_sketch_entity_reference<'entities>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frame: &[u8],
    stream: &str,
    index: &SketchEntityIndex<'entities, '_>,
) -> Result<
    Option<(
        &'entities crate::records::entity_header::DesignEntityHeader,
        usize,
    )>,
    CodecError,
> {
    const OPERATION: &str = "scan F3D sketch scope marked references";
    let mut found = None;
    let mut multiple = false;
    let mut at = 0;
    ctx.position_by(
        frame,
        |byte| {
            let here = at;
            at += 1;
            if *byte != 1 || !zeros_at::<6>(frame, here + 5) {
                return Ok(false);
            }
            let Some(suffix) = View::u32_le_at(frame, here + 1) else {
                return Ok(false);
            };
            if matches!(found, Some((found_suffix, _, _)) if found_suffix == suffix) {
                return Ok(false);
            }
            match ctx.get_hash_map(&index.by_suffix, &(stream, suffix), OPERATION)? {
                None => {}
                Some(SketchEntityMatch::Multiple) => multiple = true,
                Some(SketchEntityMatch::Unique(entity))
                    if found.replace((suffix, *entity, here + 1)).is_some() =>
                {
                    multiple = true;
                }
                Some(SketchEntityMatch::Unique(_)) => {}
            }
            Ok(multiple)
        },
        OPERATION,
    )?;
    if multiple {
        return Ok(None);
    }
    Ok(found.map(|(_, entity, suffix_at)| (entity, suffix_at)))
}

/// Bind a sketch scope to the only sketch-module entity of its stream that a
/// marked reference in its frame names.
fn bind_sketch_entity<'entities, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    scope: &mut DesignParameterScope,
    stream: &str,
    entities: &'entities [crate::records::entity_header::DesignEntityHeader],
    sketch_entities: &mut Option<SketchEntityIndex<'entities, 'ctx>>,
) -> Result<(), CodecError> {
    if !matches!(
        scope.payload(),
        scope::DesignScopePayload::Sketch(_)
            | scope::DesignScopePayload::Esquisse(_)
            | scope::DesignScopePayload::Skizze(_)
            | scope::DesignScopePayload::Esboco(_)
    ) {
        return Ok(());
    }
    let Some(frame) = usize::try_from(scope.byte_offset())
        .ok()
        .zip(usize::try_from(scope.paired_byte_offset()).ok())
        .and_then(|(start, end)| bytes.get(start..end))
    else {
        return Ok(());
    };
    let index = match sketch_entities {
        Some(index) => index,
        None => sketch_entities.insert(sketch_entity_index(ctx, entities)?),
    };
    let Some((entity, relative_offset)) =
        unique_sketch_entity_reference(ctx, frame, stream, index)?
    else {
        return Ok(());
    };
    let Some(entity_reference_offset) = scope
        .byte_offset()
        .checked_add(u64_from_index(relative_offset))
    else {
        return Ok(());
    };
    let entity_id = copy_sketch_entity_id(ctx, &entity.entity_id)?;
    if let scope::DesignScopePayloadMut::Sketch(slot)
    | scope::DesignScopePayloadMut::Esquisse(slot)
    | scope::DesignScopePayloadMut::Skizze(slot)
    | scope::DesignScopePayloadMut::Esboco(slot) = scope.payload_mut()
    {
        *slot = Some(scope::DesignSketchEntityBinding {
            entity_id,
            entity_reference_offset,
        });
    }
    Ok(())
}

/// Bind the specialized construction fields that each scope family decodes
/// beside its generic envelope.
fn bind_scope_constructions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &mut DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
    native: &crate::native::F3dNative,
) -> Result<(), CodecError> {
    let parameters = &native.design_parameters;
    let parameter_owners = &native.design_parameter_owners;
    let component_occurrences = &native.design_component_occurrences;
    let recipes = &native.construction_recipes;
    bind_coil_extent_from_parameters(ctx, scope, parameters, parameter_owners)?;
    bind_hem_operation_from_parameters(ctx, bytes, scope, parameters, parameter_owners)?;
    if matches!(scope.payload(), scope::DesignScopePayload::WorkPlane(_)) {
        if let Some(frame) = exact_work_plane_frame(ctx, bytes, records, scope)? {
            if let scope::DesignScopePayloadMut::WorkPlane(slot) = scope.payload_mut() {
                *slot = Some(scope::DesignWorkPlaneTransform {
                    work_plane_transform: frame.transform,
                    work_plane_transform_offset: frame.transform_offset,
                    reference: frame.reference.map(|(record_index, offset)| {
                        scope::DesignWorkPlaneReference {
                            work_plane_reference: record_index,
                            work_plane_reference_offset: offset,
                        }
                    }),
                    work_plane_construction: None,
                });
            }
        }
    }
    if let Some(construction) = exact_work_axis_construction(bytes, records, scope) {
        if let scope::DesignScopePayloadMut::WorkAxis(slot) = scope.payload_mut() {
            *slot = Some(construction);
        }
    }
    if matches!(scope.payload(), scope::DesignScopePayload::JointOrigin(_)) {
        if let Some(frame) = exact_joint_origin_frame(ctx, bytes, records, scope)? {
            if let scope::DesignScopePayloadMut::JointOrigin(slot) = scope.payload_mut() {
                *slot = Some(scope::DesignJointOriginTransform {
                    joint_origin_transform: frame.transform,
                    joint_origin_transform_offset: frame.transform_offset,
                    reference: frame.reference.map(|(record_index, offset)| {
                        scope::DesignJointOriginReference {
                            joint_origin_reference: record_index,
                            joint_origin_reference_offset: offset,
                        }
                    }),
                });
            }
        }
    }
    {
        let construction = exact_work_point_construction(ctx, bytes, records, scope, stream_types)?;
        if let scope::DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction = exact_hole_construction(
            ctx,
            bytes,
            records,
            scope,
            stream_types,
            &scope::DesignFeatureKind::Hole,
        )?;
        if let scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    if let Some(placement) = exact_coil_placement(ctx, bytes, records, scope, recipes)? {
        if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
        | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).placement = Some(placement);
        }
    }
    if let Some(construction) = exact_solid_primitive(ctx, bytes, records, scope, parameter_owners)?
    {
        match (scope.payload_mut(), construction) {
            (
                scope::DesignScopePayloadMut::SpherePrimitive(slot),
                crate::records::feature::primitives::DesignSolidPrimitive::Sphere(value),
            ) => *slot = Some(value),
            (
                scope::DesignScopePayloadMut::TorusPrimitive(slot),
                crate::records::feature::primitives::DesignSolidPrimitive::Torus(value),
            ) => *slot = Some(value),
            (
                scope::DesignScopePayloadMut::BoxPrimitive(slot),
                crate::records::feature::primitives::DesignSolidPrimitive::Box(value),
            ) => *slot = Some(value),
            (
                scope::DesignScopePayloadMut::CylinderPrimitive(slot),
                crate::records::feature::primitives::DesignSolidPrimitive::Cylinder(value),
            ) => *slot = Some(value),
            _ => {
                return Err(CodecError::NotImplemented(
                    "F3D solid primitive payload kind mismatch".into(),
                ))
            }
        }
    }
    {
        let construction = exact_direct_face_operation(ctx, bytes, records, scope)?;
        match (scope.payload_mut(), construction) {
            (
                scope::DesignScopePayloadMut::OffsetFaces(slot)
                | scope::DesignScopePayloadMut::DecalerLesFaces(slot),
                Some(direct_face::DesignDirectFaceOperation::OffsetFaces(value)),
            ) => *slot = Some(value),
            (
                scope::DesignScopePayloadMut::Shell(slot)
                | scope::DesignScopePayloadMut::Schale(slot),
                Some(direct_face::DesignDirectFaceOperation::Shell(value)),
            ) => *slot = Some(value),
            (
                scope::DesignScopePayloadMut::Thicken(slot),
                Some(direct_face::DesignDirectFaceOperation::Thicken(value)),
            ) => *slot = Some(value),
            _ => {}
        }
    }
    {
        let construction = exact_move_operation(ctx, bytes, records, scope)?;
        if let scope::DesignScopePayloadMut::Move(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction = exact_scale_operation(ctx, bytes, records, scope, stream_types)?;
        if let scope::DesignScopePayloadMut::Scale(slot)
        | scope::DesignScopePayloadMut::Massstab(slot) = scope.payload_mut()
        {
            *slot = construction;
        }
    }
    {
        let construction = exact_surface_extend_operation(ctx, bytes, records, scope)?;
        if let scope::DesignScopePayloadMut::SurfaceExtend(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction = exact_surface_offset_operation(ctx, bytes, records, scope)?;
        if let scope::DesignScopePayloadMut::SurfaceOffset(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    if let Some(parameters) =
        exact_fixed_extrude_parameters(ctx, bytes, records, scope, parameters, parameter_owners)?
    {
        {
            let value = Some(parameters);
            if let scope::DesignScopePayloadMut::Extrude(slot)
            | scope::DesignScopePayloadMut::Extrusion(slot)
            | scope::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
            {
                slot.get_or_insert_with(Default::default)
                    .fixed_extrude_parameters = value;
            }
        }
    }
    {
        let construction = exact_fixed_fillet_parameters(ctx, bytes, records, scope)?;
        if let scope::DesignScopePayloadMut::Fillet(slot)
        | scope::DesignScopePayloadMut::Conge(slot)
        | scope::DesignScopePayloadMut::Abrundung(slot)
        | scope::DesignScopePayloadMut::Arredondamento(slot) = scope.payload_mut()
        {
            *slot = construction;
        }
    }
    {
        let construction =
            exact_fixed_chamfer_parameters(ctx, bytes, records, scope, parameter_owners)?;
        if let scope::DesignScopePayloadMut::Chamfer(slot)
        | scope::DesignScopePayloadMut::Chanfrein(slot) = scope.payload_mut()
        {
            *slot = construction;
        }
    }
    if let Some(construction) =
        exact_path_feature_construction(ctx, bytes, records, scope, parameter_owners)?
    {
        match (scope.payload_mut(), construction) {
            (
                scope::DesignScopePayloadMut::Revolve(slot),
                crate::records::feature::path_features::DesignPathFeatureConstruction::Revolve(
                    value,
                ),
            ) => *slot = Some(value),
            (
                scope::DesignScopePayloadMut::Loft(slot),
                crate::records::feature::path_features::DesignPathFeatureConstruction::Loft(value),
            ) => *slot = Some(value),
            (
                scope::DesignScopePayloadMut::Pipe(slot),
                crate::records::feature::path_features::DesignPathFeatureConstruction::Pipe(value),
            ) => *slot = Some(value),
            (
                scope::DesignScopePayloadMut::Sweep(slot),
                crate::records::feature::path_features::DesignPathFeatureConstruction::Sweep(value),
            ) => {
                *slot = Some(scope::DesignSweepScope {
                    construction: Some(value),
                    sweep_profile: None,
                });
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "F3D path feature payload kind mismatch".into(),
                ))
            }
        }
    }
    {
        let construction = exact_combine_operation(ctx, bytes, records, scope)?;
        if let scope::DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction = exact_thread_construction(ctx, bytes, scope)?;
        if let scope::DesignScopePayloadMut::Thread(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction =
            exact_draft_operation_with_owners(ctx, bytes, records, scope, parameter_owners)?;
        if let scope::DesignScopePayloadMut::Draft(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction = exact_circular_pattern_construction_with_owners(
            ctx,
            bytes,
            records,
            scope,
            parameter_owners,
        )?;
        if let scope::DesignScopePayloadMut::CPattern(slot)
        | scope::DesignScopePayloadMut::CircularPattern(slot)
        | scope::DesignScopePayloadMut::ReseauC(slot) = scope.payload_mut()
        {
            *slot = construction;
        }
    }
    {
        let construction =
            exact_rectangular_pattern_construction(ctx, bytes, records, scope, parameter_owners)?;
        if let scope::DesignScopePayloadMut::RPattern(slot)
        | scope::DesignScopePayloadMut::RectangularPattern(slot) = scope.payload_mut()
        {
            *slot = construction;
        }
    }
    {
        let construction = exact_assembly_alignment(ctx, bytes, records, scope, parameter_owners)?;
        if let scope::DesignScopePayloadMut::Assemble(slot)
        | scope::DesignScopePayloadMut::AsBuilt(slot) = scope.payload_mut()
        {
            *slot = construction;
        }
    }
    let carriers = match scope.assembly_alignment() {
        Some(assembly::DesignAssemblyAlignment {
            form: Some(assembly::DesignAssemblyAlignmentForm::SolvedOnly { solved_frame, .. }),
            ..
        }) => exact_legacy_as_built_421_operands(
            ctx,
            bytes,
            records,
            scope,
            stream_types,
            recipes,
            solved_frame,
        )?,
        _ => None,
    };
    if let (Some(carriers), Some(alignment)) = (carriers, scope.assembly_alignment_mut()) {
        alignment.form = match alignment.form.take() {
            Some(assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                solved_frame,
                limits,
            }) => Some(assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers,
                solved_frame,
                limits,
                frames_field_present: true,
            }),
            form => form,
        };
    }
    {
        let construction = exact_component_insert_construction(ctx, bytes, records, scope)?;
        if let scope::DesignScopePayloadMut::ComponentInsert(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction =
            exact_derived_instance_construction(ctx, bytes, records, scope, component_occurrences)?;
        if let scope::DesignScopePayloadMut::DerivedInstance(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction = exact_copy_paste_component_operation(
            ctx,
            bytes,
            records,
            scope,
            component_occurrences,
        )?;
        if let scope::DesignScopePayloadMut::CopyPaste(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    bind_component_pattern_occurrences(ctx, scope, component_occurrences)?;
    {
        let construction = exact_copy_paste_bodies_operation(ctx, bytes, records, scope)?;
        if let scope::DesignScopePayloadMut::CopyPasteBodies(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }
    {
        let construction = exact_base_feature_construction(ctx, bytes, scope)?;
        if let scope::DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
            *slot = construction;
        }
    }

    Ok(())
}

fn copy_sketch_entity_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &crate::records::identity::DesignEntityId,
) -> Result<crate::records::identity::DesignEntityId, CodecError> {
    let text = ctx.copy_retained_text(source.as_str(), "f3d Sketch scope entity ID")?;
    crate::records::identity::DesignEntityId::try_from(text).map_err(CodecError::NotImplemented)
}

/// Admit one envelope for every logical scope identity.
///
/// Some Design streams retain more than one complete envelope for one record
/// index. A history-bound envelope is authoritative when exactly one
/// candidate resolves to a unique ASM state transition; an unresolved group
/// remains an error so a duplicate cannot be selected by byte order.
pub(crate) fn admit_history_bound_scope_variants(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut Vec<DesignParameterScope>,
    histories: &[crate::history_records::AsmHistory],
) -> Result<(), CodecError> {
    const OPERATION: &str = "f3d scope admission";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut admitted = storage.with_storage(|| {
        ctx.collect_indexed_vec(scopes.len(), "f3d scope admission flags", |_| Ok(true))
    })?;
    {
        let mut identities = storage.with_storage(|| {
            ctx.collect_indexed_vec(scopes.len(), "f3d scope admission identities", |index| {
                let scope = &scopes[index];
                let stream = record_stream(ctx, &scope.id)?.unwrap_or(ids::DEFAULT_STREAM);
                Ok((stream, scope.record_index, index))
            })
        })?;
        ctx.stable_sort_by_key(
            &mut identities,
            |(stream, record_index, _)| (*stream, *record_index),
            Ord::cmp,
            "sort F3D scope admission identities",
        )?;
        let mut rest = identities.as_slice();
        while let Some(((stream, record_index, _), following)) = rest.split_first() {
            ctx.charge_work(1, "group F3D scope admission identities")?;
            let group_length = 1 + ctx
                .position_by(
                    following,
                    |(other_stream, other_record_index, _)| {
                        Ok(other_record_index != record_index
                            || !ctx.equal_bytes(
                                other_stream.as_bytes(),
                                stream.as_bytes(),
                                "group F3D scope admission identities",
                            )?)
                    },
                    "group F3D scope admission identities",
                )?
                .unwrap_or(following.len());
            let (group, tail) = rest.split_at(group_length);
            rest = tail;
            if let Some(keep) = admitted_scope_variant(ctx, scopes, group, histories)? {
                for (_, _, index) in
                    ctx.admit_iter(group, "mark F3D retained scope admission candidate")?
                {
                    admitted[*index] = *index == keep;
                }
            }
        }
    }

    let retained_count = ctx
        .admit_iter(&admitted, "count F3D retained scope admission candidates")?
        .filter(|selected| **selected)
        .count();
    // Every admission precedes the move, so a refusal leaves `scopes` intact.
    let mut retained = ctx.collection_vec(retained_count, "f3d scope admission retained output")?;
    for (selected, scope) in ctx
        .admit_iter(&admitted, "move F3D retained scope admission candidates")?
        .zip(std::mem::take(scopes))
    {
        if *selected {
            retained.push(scope);
        }
    }
    *scopes = retained;
    Ok(())
}

/// The envelope a group of same-identity scopes keeps, or `None` for a group
/// of one. A history-bound envelope is kept when it is the only one. Without
/// one, equivalent envelopes keep the latest by byte offset: the later
/// envelope supersedes the earlier one when no decoded ASM state pair can
/// select a revision.
fn admitted_scope_variant(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &[DesignParameterScope],
    group: &[(&str, u32, usize)],
    histories: &[crate::history_records::AsmHistory],
) -> Result<Option<usize>, CodecError> {
    const HISTORY_OPERATION: &str = "scan F3D scope admission history candidates";
    let [(_, _, first), following @ ..] = group else {
        return Ok(None);
    };
    if following.is_empty() {
        return Ok(None);
    }
    let is_history_bound = |(_, _, index): &(&str, u32, usize)| {
        let scope = &scopes[*index];
        let bound = scope.history_state_id().is_some_and(|state_id| {
            crate::history::effective_scope_previous_history_state_id(scope, histories).is_some_and(
                |previous_state_id| {
                    crate::history::unique_history_state_pair(
                        histories,
                        state_id,
                        previous_state_id,
                    )
                    .is_some()
                },
            )
        });
        Ok(bound)
    };
    let history_bound = ctx.position_by(group, is_history_bound, HISTORY_OPERATION)?;
    let keep = match history_bound {
        Some(position) => {
            if ctx
                .position_by(&group[position + 1..], is_history_bound, HISTORY_OPERATION)?
                .is_some()
            {
                None
            } else {
                Some(group[position].2)
            }
        }
        None => {
            // An equivalent envelope is one serialization of the same logical
            // scope without its source-location and dynamic-class fields.
            let equivalent = match scope_variant_json(ctx, &scopes[*first])? {
                Some((first_tree, _first_storage)) => ctx.all_by(
                    following,
                    |(_, _, index)| match scope_variant_json(ctx, &scopes[*index])? {
                        Some((tree, _storage)) => {
                            equivalent_scope_json(ctx, &first_tree, &tree, true)
                        }
                        None => Ok(false),
                    },
                    "scan F3D equivalent scope admission candidates",
                )?,
                None => false,
            };
            if equivalent {
                Some(ctx.fold(
                    following,
                    *first,
                    |keep, (_, _, index)| {
                        Ok(
                            if scopes[*index].byte_offset() >= scopes[keep].byte_offset() {
                                *index
                            } else {
                                keep
                            },
                        )
                    },
                    "select F3D latest equivalent scope envelope",
                )?)
            } else {
                None
            }
        }
    };
    keep.map(Some).ok_or_else(|| {
        CodecError::Malformed(
            "Design scope record identity has unresolved duplicate envelopes".into(),
        )
    })
}

/// Serialized JSON text. Each write admits its bytes as work and storage
/// before copying them; a refusal stops the serializer and is kept for the
/// caller.
struct ChargedJsonWriter<'a, 'ctx> {
    ctx: &'a cadmpeg_core::decode::DecodeContext<'ctx>,
    bytes: Vec<u8>,
    refusal: Option<CodecError>,
}

impl std::io::Write for ChargedJsonWriter<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Err(error) =
            self.ctx
                .extend_retained_bytes(&mut self.bytes, bytes, "f3d scope variant JSON")
        {
            self.refusal = Some(error);
            return Err(std::io::Error::other("scope JSON refused"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Serialize `scope` into scoped temporary storage and parse the text as a
/// value tree, returned with the scoped storage of the tree. A scope that does
/// not serialize compares as different.
fn scope_variant_json<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scope: &DesignParameterScope,
) -> Result<
    Option<(
        serde_json::Value,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    CodecError,
> {
    const OPERATION: &str = "f3d scope variant JSON";
    let mut text_storage = ctx.reserve_scoped(0, OPERATION)?;
    let mut writer = ChargedJsonWriter {
        ctx,
        bytes: Vec::new(),
        refusal: None,
    };
    let serialized = text_storage.with_storage(|| {
        let serialized = serde_json::to_writer(&mut writer, scope).is_ok();
        writer.refusal.take().map_or(Ok(serialized), Err)
    })?;
    if !serialized {
        return Ok(None);
    }
    let Ok(text) = ctx.validate_utf8(&writer.bytes, OPERATION)? else {
        return Ok(None);
    };
    match ctx.parse_json_value(text, OPERATION) {
        Ok(tree) => Ok(Some(tree)),
        Err(CodecError::ResourceLimit(limit)) => Err(CodecError::ResourceLimit(limit)),
        Err(_) => Ok(None),
    }
}

/// Whether a member name records a source location, or at the top level a
/// scope identity or dynamic-class field. These members do not take part in
/// envelope equivalence.
fn is_scope_variant_provenance(key: &str, top_level: bool) -> bool {
    key.ends_with("_offset")
        || key.ends_with("_offsets")
        || (top_level
            && matches!(
                key,
                "id" | "class_tag"
                    | "frame_length"
                    | "history_state_id"
                    | "previous_history_state_id"
                    | "paired_class_tag"
            ))
}

/// Compare two scope value trees without their provenance members. Objects
/// and arrays admit only the members and items visited before the first
/// difference.
fn equivalent_scope_json(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    left: &serde_json::Value,
    right: &serde_json::Value,
    top_level: bool,
) -> Result<bool, CodecError> {
    use serde_json::Value;
    const OPERATION: &str = "f3d scope variant comparison";
    let _depth = ctx.enter_nested("f3d scope variant comparison depth")?;
    match (left, right) {
        (Value::Null, Value::Null) => Ok(true),
        (Value::Bool(left), Value::Bool(right)) => Ok(left == right),
        (Value::Number(left), Value::Number(right)) => Ok(left == right),
        (Value::String(left), Value::String(right)) => {
            ctx.equal_bytes(left.as_bytes(), right.as_bytes(), OPERATION)
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                return Ok(false);
            }
            let mut right_items = right.iter();
            ctx.all_by(
                left,
                |item| {
                    right_items.next().map_or(Ok(false), |other| {
                        equivalent_scope_json(ctx, item, other, false)
                    })
                },
                OPERATION,
            )
        }
        (Value::Object(left), Value::Object(right)) => {
            // Both maps iterate in key order, so equal member sets pair up.
            // Each member is admitted as the walk reaches it.
            fn next_member<'value>(
                ctx: &cadmpeg_core::decode::DecodeContext<'_>,
                members: &mut serde_json::map::Iter<'value>,
                top_level: bool,
            ) -> Result<Option<(&'value String, &'value Value)>, CodecError> {
                for (key, value) in members.by_ref() {
                    ctx.charge_work(1, OPERATION)?;
                    if !is_scope_variant_provenance(key, top_level) {
                        return Ok(Some((key, value)));
                    }
                }
                Ok(None)
            }
            let mut left_members = left.iter();
            let mut right_members = right.iter();
            loop {
                match (
                    next_member(ctx, &mut left_members, top_level)?,
                    next_member(ctx, &mut right_members, top_level)?,
                ) {
                    (None, None) => return Ok(true),
                    (Some((left_key, left_value)), Some((right_key, right_value))) => {
                        if !ctx.equal_bytes(left_key.as_bytes(), right_key.as_bytes(), OPERATION)?
                            || !equivalent_scope_json(ctx, left_value, right_value, false)?
                        {
                            return Ok(false);
                        }
                    }
                    _ => return Ok(false),
                }
            }
        }
        _ => Ok(false),
    }
}

/// Skip the payload prologue at `at`: a leading-block presence byte, a property
/// presence byte, and the property block that byte gates. The leading-block
/// byte belongs to classes that write one, so this reader only steps over it.
/// A block holds at most 16 properties of two fields of at most 64 bytes, so
/// the reader does fixed work.
pub(in crate::design::decode) fn payload_prologue(
    bytes: &[u8],
    at: usize,
    end: usize,
) -> Option<usize> {
    /// A counted field of 1 to 64 printable ASCII bytes, and the offset after it.
    fn graphic_ascii_at(bytes: &[u8], at: usize) -> Option<(&[u8], usize)> {
        let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        if !(1..=64).contains(&length) {
            return None;
        }
        let start = at.checked_add(4)?;
        let end = start.checked_add(length)?;
        let raw = bytes.get(start..end)?;
        raw.iter().all(u8::is_ascii_graphic).then_some((raw, end))
    }

    let mut cursor = at.checked_add(1)?;
    let present = *bytes.get(cursor)?;
    cursor += 1;
    match present {
        0 => Some(cursor),
        1 => {
            let count = View::u32_le_at(bytes, cursor)?;
            if count > 16 {
                return None;
            }
            cursor += 4;
            for _ in 0..count {
                let (_key, after_key) = graphic_ascii_at(bytes, cursor)?;
                let (type_name, after_type) = graphic_ascii_at(bytes, after_key)?;
                if type_name != b"IntrinsicMetaTypeuint64" {
                    return None;
                }
                cursor = after_type.checked_add(8)?;
            }
            (cursor <= end).then_some(cursor)
        }
        _ => None,
    }
}

pub(crate) fn parameter_scope_tail_length_is_valid(
    kind: impl AsRef<str>,
    tail_length: usize,
) -> bool {
    let kind = kind.as_ref();
    if (80..=590).contains(&tail_length) && tail_length.is_multiple_of(2) {
        return true;
    }
    match kind {
        "CopyPasteBodies" => tail_length == 110,
        "CoilPrimitive" => matches!(tail_length, 72 | 76 | 77 | 78 | 87 | 88),
        _ => matches!(tail_length, 72 | 76 | 77 | 78 | 87),
    }
}

pub(crate) fn parameter_scope_previous_history_offset(
    kind: impl AsRef<str>,
    tail_length: usize,
) -> Option<usize> {
    parameter_scope_previous_history_offset_for_form(
        kind.as_ref(),
        tail_length,
        ScopeTailForm::Fixed,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeTailForm {
    Fixed,
    Named,
}

fn parameter_scope_previous_history_offset_for_form(
    kind: &str,
    tail_length: usize,
    tail_form: ScopeTailForm,
) -> Option<usize> {
    if tail_form == ScopeTailForm::Named {
        return None;
    }
    match (kind, tail_length) {
        ("CopyPasteBodies", 110) => Some(53),
        ("CoilPrimitive", 88) => None,
        (_, 72 | 76) => Some(30),
        (_, 77 | 78) => Some(31),
        (_, 87) => Some(41),
        _ => None,
    }
}

/// Parse the parameter scope whose frame opens at `byte_offset` and closes at
/// the next header carrying `record_index`.
pub(in crate::design::decode) fn parse_parameter_scope(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: &crate::records::references::DesignClassTag,
    byte_offset: u64,
) -> Result<Option<DesignParameterScope>, CodecError> {
    let Some(start) = usize::try_from(byte_offset).ok() else {
        return Ok(None);
    };
    let Ok(class_tag) = <&[u8; 3]>::try_from(class_tag.as_str().as_bytes()) else {
        return Ok(None);
    };
    parse_scope_at(ctx, bytes, records, record_index, *class_tag, start)
}

/// Parse the parameter scope whose frame opens at `start` and closes at the
/// first header carrying `record_index` at least one header length later.
fn parse_scope_at(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: [u8; 3],
    start: usize,
) -> Result<Option<DesignParameterScope>, CodecError> {
    let Some(search) = start.checked_add(11) else {
        return Ok(None);
    };
    let Some(paired_at) = records.first_at_or_after(ctx, search, record_index)? else {
        return Ok(None);
    };
    parse_scope_frame(
        ctx,
        bytes,
        records,
        record_index,
        class_tag,
        start,
        paired_at,
    )
}

/// The farthest a scope kind field opens before the paired header: the
/// longest tail, the kind's length prefix and a 256-unit kind.
const KIND_SCAN_SPAN: usize = 590 + 4 + 2 * 256;

/// A marked reference slot of a scope's reference table: a one byte, a `u32`
/// record index and six zero bytes.
const REFERENCE_SLOT_LEN: usize = 11;

/// Parse the parameter scope framed by the headers at `start` and `paired_at`,
/// whose first header carries `class_tag`.
///
/// The kind field is the only counted UTF-16 field among the last
/// [`KIND_SCAN_SPAN`] positions before the paired header whose tail has a
/// fixed or named-label form; a named tail takes precedence and two candidates
/// of the chosen form leave the frame unparsed. The reference table is the
/// only counted run of marked reference slots that ends four bytes before the
/// kind field.
fn parse_scope_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    class_tag: [u8; 3],
    start: usize,
    paired_at: usize,
) -> Result<Option<DesignParameterScope>, CodecError> {
    let (Ok(class_tag_text), Some(paired_header)) = (
        std::str::from_utf8(&class_tag),
        indexed_record_header_at(bytes, paired_at),
    ) else {
        return Ok(None);
    };
    let Ok(paired_class_tag) = std::str::from_utf8(paired_header.class_tag) else {
        return Ok(None);
    };
    let (Some(scan_start), Some(kind_scan_end)) =
        (start.checked_add(11), paired_at.checked_sub(72))
    else {
        return Ok(None);
    };
    let mut fixed_candidate = None;
    let mut fixed_ambiguous = false;
    let mut named_candidate = None;
    let mut named_ambiguous = false;
    // The window holds at most `KIND_SCAN_SPAN` positions; each decode admits
    // its own text. A frame shorter than the span opens the window at its start.
    let window_start = paired_at
        .checked_sub(KIND_SCAN_SPAN)
        .map_or(scan_start, |span_start| span_start.max(scan_start));
    for at in window_start..kind_scan_end {
        let Some((kind, kind_end, _kind_storage)) =
            lp_utf16_bounded_scoped(ctx, bytes, at, 1..=256, "f3d Design temporary UTF-16 text")?
        else {
            continue;
        };
        if utf16_has_control(ctx, bytes.get(at + 4..kind_end).unwrap_or(&[]))? {
            continue;
        }
        let Some(tail_length) = paired_at.checked_sub(kind_end) else {
            continue;
        };
        let fixed_tail = matches!(tail_length, 72 | 76 | 77 | 78 | 82 | 87 | 88 | 104 | 110);
        if fixed_tail
            && parameter_scope_tail_length_is_valid(&kind, tail_length)
            && fixed_candidate
                .replace((at, kind_end, tail_length, ScopeTailForm::Fixed))
                .is_some()
        {
            fixed_ambiguous = true;
        }
        let named_tail_possible = (78..=590).contains(&tail_length)
            && tail_length.is_multiple_of(2)
            && (parameter_scope_tail_length_is_valid(&kind, tail_length) || tail_length == 78);
        if named_tail_possible
            && named_parameter_scope_tail_is_valid(ctx, bytes, kind_end, paired_at, tail_length)?
                == Some(true)
            && named_candidate
                .replace((at, kind_end, tail_length, ScopeTailForm::Named))
                .is_some()
        {
            named_ambiguous = true;
        }
    }
    let candidate = if named_ambiguous {
        None
    } else if named_candidate.is_some() {
        named_candidate
    } else if fixed_ambiguous {
        None
    } else {
        fixed_candidate
    };
    let Some((kind_at, kind_end, tail_length, tail_form)) = candidate else {
        return Ok(None);
    };
    let Some((kind_text, confirmed_kind_end, _kind_storage)) = lp_utf16_bounded_scoped(
        ctx,
        bytes,
        kind_at,
        1..=256,
        "f3d Design temporary UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    if confirmed_kind_end != kind_end {
        return Ok(None);
    }
    let Ok(kind) = scope::DesignFeatureKind::try_from(kind_text) else {
        return Ok(None);
    };
    if let scope::DesignFeatureKind::Native(_) = &kind {
        // The output keeps a native kind name: at most 256 UTF-16 units.
        ctx.charge_retained(
            u64_from_index(kind.as_str().len()),
            "f3d Design scope kind storage",
        )?;
    }
    let Some(reference_table_end) = kind_at.checked_sub(4) else {
        return Ok(None);
    };
    let Some(feature_ordinal) =
        View::u32_le_at(bytes, kind_end).and_then(std::num::NonZeroU32::new)
    else {
        return Ok(None);
    };
    let Some(history_state_id) = View::u32_le_at(bytes, reference_table_end) else {
        return Ok(None);
    };
    let history_state_id = (history_state_id != u32::MAX).then(|| i64::from(history_state_id));
    let previous_history_state_id_offset = match parameter_scope_previous_history_offset_for_form(
        kind.as_str(),
        tail_length,
        tail_form,
    ) {
        Some(offset) => {
            let Some(offset) = kind_end.checked_add(offset) else {
                return Ok(None);
            };
            Some(offset)
        }
        None => None,
    };
    let previous_history_state_id = previous_history_state_id_offset
        .and_then(|offset| View::u32_le_at(bytes, offset))
        .filter(|state_id| *state_id != u32::MAX)
        .map(i64::from);
    let Some(table) = reference_table(ctx, bytes, start, reference_table_end)? else {
        return Ok(None);
    };
    let mut members_storage = ctx.reserve_scoped(0, "f3d Design scope reference members")?;
    let reference_members = members_storage.with_storage(|| {
        ctx.collect_indexed_vec(
            table.count,
            "f3d Design scope reference members",
            |ordinal| {
                table.member(bytes, ordinal).ok_or_else(|| {
                    CodecError::malformed("F3D scope reference slot lies outside its frame")
                })
            },
        )
    })?;
    let reference_members = reference_members.as_slice();
    let reference_count_at = table.count_at;
    let surface_stitch_operation = if matches!(kind, scope::DesignFeatureKind::SurfaceStitch) {
        exact_surface_stitch_operation(ctx, bytes, records, record_index, reference_members)?
    } else {
        None
    };
    let surface_patch_boundaries = if matches!(kind, scope::DesignFeatureKind::SurfacePatch) {
        crate::design::decode::patch::surface_patch_boundaries(
            ctx,
            bytes,
            records,
            reference_members,
        )?
    } else {
        Vec::new()
    };
    let base_flange_operation = if matches!(kind, scope::DesignFeatureKind::BaseFlange) {
        exact_base_flange_operation(bytes, start, paired_at, reference_members)
    } else {
        None
    };
    let edge_flange_operation = if matches!(kind, scope::DesignFeatureKind::EdgeFlange) {
        exact_edge_flange_operation(
            bytes,
            start,
            paired_at,
            class_tag_text,
            paired_class_tag,
            reference_members,
        )
    } else {
        None
    };
    let ruled_surface_operation = if matches!(kind, scope::DesignFeatureKind::SurfaceRuled) {
        exact_ruled_surface_operation(
            ctx,
            bytes,
            start,
            paired_at,
            reference_count_at,
            reference_members,
        )?
    } else {
        None
    };
    let family = design_feature_family(&kind);
    // A `Sketch` scope carries either the single entity-suffix reference form
    // or, when the stream's sketch entity headers use the `EntityGenesis`
    // form, the generic ordered reference table. Both parse here; the entity
    // binding in `decode_parameter_scopes` requires a unique suffix match.
    let extrude_prologue = if family == Some(DesignFeatureFamily::Extrude) {
        // The generic scope envelope is independently self-delimiting. An
        // unrecognized Extrude prologue therefore withholds only the typed
        // fields, not the scope and its ordered reference table.
        exact_extrude_prologue(
            ctx,
            bytes,
            ExtrudeScopeFrame {
                start,
                paired_at,
                class_tag: class_tag_text,
                paired_class_tag,
                reference_count_at,
            },
            reference_members,
        )?
    } else {
        None
    };
    let coil = if family == Some(DesignFeatureFamily::Coil) {
        let coil_discriminators =
            exact_coil_discriminators(bytes, start, paired_at, &kind, reference_members);
        let coil_transform =
            exact_long_coil_transform(bytes, start, paired_at, &kind, reference_members);
        Some(coil_scope(coil_discriminators.as_ref(), coil_transform))
    } else {
        None
    };
    let payload = match kind {
        scope::DesignFeatureKind::SurfaceStitch => {
            let Some(operation) = surface_stitch_operation else {
                return Ok(None);
            };
            scope::DesignScopePayload::SurfaceStitch(operation)
        }
        scope::DesignFeatureKind::SurfaceRuled => {
            let Some(operation) = ruled_surface_operation else {
                return Ok(None);
            };
            scope::DesignScopePayload::SurfaceRuled(operation)
        }
        kind => {
            let Ok(payload) = kind.try_into() else {
                return Ok(None);
            };
            payload
        }
    };
    let (
        Some(frame_length),
        Ok(kind_offset),
        Ok(feature_ordinal_offset),
        Ok(reference_count_offset),
    ) = (
        paired_at.checked_sub(start).map(u64_from_index),
        u64::try_from(kind_at + 4),
        u64::try_from(kind_end),
        u64::try_from(reference_count_at),
    )
    else {
        return Ok(None);
    };
    let located_references = ctx.collect_indexed_vec(
        table.count,
        "f3d Design scope located references",
        |ordinal| {
            Ok(crate::records::identity::Located {
                value: reference_members[ordinal],
                offset: u64_from_index(table.member_offset(ordinal)),
            })
        },
    )?;
    let class_tag = crate::design::decode::text::retain_class_tag(
        ctx,
        class_tag,
        "f3d Design scope class tag",
    )?;
    let paired_class_tag = paired_header.retain_class_tag(ctx, "f3d Design scope class tag")?;
    let Ok(mut scope) = DesignParameterScope::try_new(scope::DesignParameterScopeDraft {
        id: String::new(),
        byte_offset: u64_from_index(start),
        class_tag,
        record_index,
        frame_length,
        kind_offset,
        feature_ordinal,
        feature_ordinal_offset,
        history_state_id,
        previous_history_state_id,
        previous_history_state_id_offset: previous_history_state_id_offset
            .map(u64_from_index)
            .filter(|&offset| offset != 0),
        reference_count_offset,
        reference_members: crate::records::identity::ReferenceRun::located(located_references),
        payload,
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag,
        paired_byte_offset: u64_from_index(paired_at),
    }) else {
        return Ok(None);
    };
    if let Some(prologue) = extrude_prologue {
        if let scope::DesignScopePayloadMut::Extrude(slot)
        | scope::DesignScopePayloadMut::Extrusion(slot)
        | scope::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
        {
            *slot = Some(scope::DesignExtrudeScope {
                extrude_prologue: Some(prologue),
                ..scope::DesignExtrudeScope::default()
            });
        }
    }
    if let Some(coil) = coil {
        if let scope::DesignScopePayloadMut::SpirePrimitive(slot)
        | scope::DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut()
        {
            *slot = Some(coil);
        }
    }
    if !surface_patch_boundaries.is_empty() {
        if let scope::DesignScopePayloadMut::SurfacePatch(slot) = scope.payload_mut() {
            *slot = surface_patch_boundaries;
        }
    }
    if let Some(operation) = base_flange_operation {
        if let scope::DesignScopePayloadMut::BaseFlange(slot) = scope.payload_mut() {
            *slot = Some(scope::DesignBaseFlangeScope {
                base_flange_operation: Some(operation),
                ..scope::DesignBaseFlangeScope::default()
            });
        }
    }
    if let Some(operation) = edge_flange_operation {
        if let scope::DesignScopePayloadMut::EdgeFlange(slot) = scope.payload_mut() {
            *slot = Some(operation);
        }
    }
    Ok(Some(scope))
}

/// Whether UTF-16LE `units` hold a control character. Every control
/// character is one BMP code unit outside the surrogate range, so the test
/// reads code units and stops at the first control character.
fn utf16_has_control(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    units: &[u8],
) -> Result<bool, CodecError> {
    let (units, _) = units.as_chunks::<2>();
    ctx.any_by(
        units,
        |unit| {
            Ok(View::u16_le_at(unit, 0)
                .and_then(|unit| char::from_u32(u32::from(unit)))
                .is_some_and(char::is_control))
        },
        "validate F3D scope text characters",
    )
}

/// A scope's reference table: a `u32` count at `count_at`, then `count`
/// marked reference slots.
struct ReferenceTable {
    count_at: usize,
    count: usize,
}

impl ReferenceTable {
    /// Offset of the record index in slot `ordinal`.
    fn member_offset(&self, ordinal: usize) -> usize {
        self.count_at + 4 + ordinal * REFERENCE_SLOT_LEN + 1
    }

    /// Record index of slot `ordinal`.
    fn member(&self, bytes: &[u8], ordinal: usize) -> Option<u32> {
        View::u32_le_at(bytes, self.member_offset(ordinal))
    }
}

/// The reference table that ends at `table_end` and whose count lies at or
/// after `start + 11`. Such a table of `k` slots has its count at
/// `table_end - 4 - 11k`, so its slots are the last `k` of the slot lattice
/// that ends at `table_end`. One forward pass over that lattice tests every
/// candidate count; a slot that is not a marked reference rules out every
/// table that contains it. A nonzero count lies in the zero padding of the
/// preceding slot, so at most one complete table exists.
fn reference_table(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    table_end: usize,
) -> Result<Option<ReferenceTable>, CodecError> {
    let Some(first_slot_min) = start.checked_add(15) else {
        return Ok(None);
    };
    let Some(slot_span) = table_end.checked_sub(first_slot_min) else {
        return Ok(None);
    };
    let slot_count = slot_span / REFERENCE_SLOT_LEN;
    let slots_start = table_end - slot_count * REFERENCE_SLOT_LEN;
    let Some(lattice) = bytes.get(slots_start..table_end) else {
        return Ok(None);
    };
    let (slots, _) = lattice.as_chunks::<REFERENCE_SLOT_LEN>();
    let mut table = None;
    for (ordinal, slot) in ctx
        .admit_iter(slots, "scan F3D parameter-scope reference slots")?
        .enumerate()
    {
        let count_at = slots_start + ordinal * REFERENCE_SLOT_LEN - 4;
        let count = slot_count - ordinal;
        if View::u32_le_at(bytes, count_at).and_then(|value| usize::try_from(value).ok())
            == Some(count)
        {
            table = Some(ReferenceTable { count_at, count });
        }
        if slot[0] != 1 || slot[5..] != [0; 6] {
            table = None;
        }
    }
    Ok(table)
}

/// The coil fields of a coil-family scope envelope.
fn coil_scope(
    discriminators: Option<&super::coil::CoilDiscriminators>,
    transform: Option<coil::DesignCoilTransform>,
) -> coil::DesignCoilScope {
    use crate::records::identity::{MaybeRecordedValue, RecordedValue};
    fn recorded<T>(value: T, offset: Option<u64>) -> MaybeRecordedValue<T> {
        match offset {
            Some(offset) => MaybeRecordedValue::Located(RecordedValue { value, offset }),
            None => MaybeRecordedValue::Unlocated(value),
        }
    }
    coil::DesignCoilScope {
        operation: discriminators.map(|fields| RecordedValue {
            value: fields.operation,
            offset: fields.operation_offset,
        }),
        extent: discriminators.and_then(|fields| fields.extent),
        section: discriminators.map(|fields| recorded(fields.section, fields.section_offset)),
        section_placement: discriminators
            .map(|fields| recorded(fields.section_placement, fields.section_placement_offset)),
        clockwise: discriminators.map(|fields| recorded(fields.clockwise, fields.clockwise_offset)),
        placement: None,
        transform,
    }
}

/// Whether the named-label tail after the kind field at `kind_end` is valid:
/// a zero `u32`, a counted UTF-16 label without control characters, seven
/// zero bytes and three repeated binary lane values.
fn named_parameter_scope_tail_is_valid(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    kind_end: usize,
    paired_at: usize,
    tail_length: usize,
) -> Result<Option<bool>, CodecError> {
    let Some(label_at) = kind_end.checked_add(8) else {
        return Ok(None);
    };
    let Some((_label, label_end, _label_storage)) = lp_utf16_bounded_scoped(
        ctx,
        bytes,
        label_at,
        0..=256,
        "f3d Design temporary UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    // The counted field holds the label's UTF-16 code units after its prefix.
    let label_bytes = label_end - label_at - 4;
    let marker = kind_end + 19 + label_bytes;
    if tail_length != 78 + label_bytes || label_end + 7 != marker {
        return Ok(Some(false));
    }
    if utf16_has_control(ctx, bytes.get(label_at + 4..label_end).unwrap_or(&[]))? {
        return Ok(Some(false));
    }
    if marker.checked_add(59) != Some(paired_at) || !zeros_at::<7>(bytes, label_end) {
        return Ok(Some(false));
    }
    let (Some(first_lane_value), Some(second_lane_value), Some(third_lane_value)) = (
        View::u64_le_at(bytes, marker + 2),
        View::u64_le_at(bytes, marker + 34),
        View::u64_le_at(bytes, marker + 48),
    ) else {
        return Ok(None);
    };
    let field_id_at = |at: usize| bytes.get(at).is_some_and(|field_id| *field_id != 0);
    Ok(Some(
        zeros_at::<4>(bytes, kind_end + 4)
            && bytes.get(marker) == Some(&1)
            && field_id_at(marker + 1)
            && matches!(first_lane_value, 0 | 1)
            && second_lane_value == first_lane_value
            && third_lane_value == first_lane_value
            && zeros_at::<2>(bytes, marker + 10)
            && View::u32_le_at(bytes, marker + 12).is_some_and(|value| value > 0)
            && View::u32_le_at(bytes, marker + 16) == Some(0xfc)
            && View::f64_le_at(bytes, marker + 20).is_some_and(f64::is_finite)
            && View::u32_le_at(bytes, marker + 28) == Some(0xfc)
            && bytes.get(marker + 32) == Some(&1)
            && field_id_at(marker + 33)
            && bytes_at::<4>(bytes, marker + 42) == Some(&[0, 1, 0, 0])
            && bytes.get(marker + 46) == Some(&1)
            && field_id_at(marker + 47)
            && zeros_at::<3>(bytes, marker + 56),
    ))
}

pub(crate) fn parameter_scope_payload_length(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &DesignParameterScope,
) -> Result<Option<u64>, CodecError> {
    // A character uses at most its UTF-8 byte count in UTF-16 code units.
    let code_units: usize = ctx
        .admit_iter(scope.kind_name(), "count F3D scope kind UTF-16 units")?
        .encode_utf16()
        .count();
    let kind_bytes = u64_from_index(code_units).checked_mul(2).ok_or_else(|| {
        ctx.refuse_codec_limit("count F3D scope kind UTF-16 units", u64::MAX, u64::MAX)
    })?;
    Ok(scope.frame_length().checked_sub(kind_bytes))
}

#[cfg(test)]
mod tests;
