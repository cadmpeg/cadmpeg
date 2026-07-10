// SPDX-License-Identifier: Apache-2.0
//! Native F3D regeneration for supported semantic edits.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use cadmpeg_ir::codec::{Codec, CodecError, DecodeOptions};
use cadmpeg_ir::design::{ActGuid, ActRootComponent, SketchCurveGeometry};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::history::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmHistory,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Coedge, Color, Edge, Face, Sense};
use cadmpeg_ir::transform::Transform;
use zip::write::SimpleFileOptions;

use crate::{asm_header, decode, sab, F3dCodec};

/// Regenerate an F3D archive for supported analytic B-rep carrier edits.
pub fn write_semantic(
    target: &CadIr,
    source_image: &[u8],
    writer: &mut dyn Write,
) -> Result<(), CodecError> {
    let baseline = F3dCodec.decode(&mut Cursor::new(source_image), &DecodeOptions::default())?;
    let baseline_point_ids = baseline
        .ir
        .model
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let target_point_ids = target
        .model
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if baseline_point_ids != target_point_ids
        || target.model.points.iter().any(|point| {
            !point.position.x.is_finite()
                || !point.position.y.is_finite()
                || !point.position.z.is_finite()
        })
    {
        return Err(CodecError::NotImplemented(
            "F3D point regeneration requires the unchanged point-id set and finite coordinates"
                .into(),
        ));
    }
    let edited_curves = validate_curve_edits(&baseline.ir.model.curves, &target.model.curves)?;
    let edited_surfaces =
        validate_surface_edits(&baseline.ir.model.surfaces, &target.model.surfaces)?;
    let sketch_point_edits = validate_sketch_point_edits(&baseline.ir, target)?;
    let sketch_curve_edits = validate_sketch_curve_edits(&baseline.ir, target)?;
    let sketch_relation_edits = validate_sketch_relation_edits(&baseline.ir, target)?;
    let persistent_reference_edits = validate_persistent_reference_edits(&baseline.ir, target)?;
    let construction_recipe_edits = validate_construction_recipe_edits(&baseline.ir, target)?;
    let body_member_edits = validate_body_member_edits(&baseline.ir, target)?;
    let entity_header_edits = validate_entity_header_edits(&baseline.ir, target)?;
    let design_object_edits = validate_design_object_edits(&baseline.ir, target)?;
    let act_guid_edits = validate_act_guid_edits(&baseline.ir, target)?;
    let act_root_edits = validate_act_root_edits(&baseline.ir, target)?;
    let body_transform_edits =
        validate_body_transform_edits(&baseline.ir.model.bodies, &target.model.bodies)?;
    let mut entity_color_edits =
        validate_body_color_edits(&baseline.ir.model.bodies, &target.model.bodies)?;
    entity_color_edits.extend(validate_face_color_edits(
        &baseline.ir.model.faces,
        &target.model.faces,
    )?);
    let edge_range_edits =
        validate_edge_range_edits(&baseline.ir.model.edges, &target.model.edges)?;
    let face_sense_edits =
        validate_face_sense_edits(&baseline.ir.model.faces, &target.model.faces)?;
    let coedge_sense_edits =
        validate_coedge_sense_edits(&baseline.ir.model.coedges, &target.model.coedges)?;
    let history_state_edits = validate_history_state_edits(&baseline.ir, target)?;
    let mut supported_target = baseline.ir.clone();
    supported_target
        .model
        .points
        .clone_from(&target.model.points);
    supported_target
        .model
        .curves
        .clone_from(&target.model.curves);
    supported_target
        .model
        .surfaces
        .clone_from(&target.model.surfaces);
    for body in &mut supported_target.model.bodies {
        if let Some(candidate) = target
            .model
            .bodies
            .iter()
            .find(|candidate| candidate.id == body.id)
        {
            body.transform = candidate.transform;
            body.color = candidate.color;
        }
    }
    supported_target.model.edges.clone_from(&target.model.edges);
    supported_target.model.faces.clone_from(&target.model.faces);
    supported_target
        .model
        .coedges
        .clone_from(&target.model.coedges);
    if let (Some(supported), Some(target_native)) = (
        supported_target.native.f3d.as_mut(),
        target.native.f3d.as_ref(),
    ) {
        supported
            .sketch_points
            .clone_from(&target_native.sketch_points);
        supported
            .sketch_curve_identities
            .clone_from(&target_native.sketch_curve_identities);
        supported
            .sketch_relations
            .clone_from(&target_native.sketch_relations);
        supported
            .persistent_references
            .clone_from(&target_native.persistent_references);
        supported
            .construction_recipes
            .clone_from(&target_native.construction_recipes);
        supported
            .design_body_members
            .clone_from(&target_native.design_body_members);
        supported
            .design_entity_headers
            .clone_from(&target_native.design_entity_headers);
        supported
            .design_objects
            .clone_from(&target_native.design_objects);
        supported.act_guids.clone_from(&target_native.act_guids);
        supported
            .act_root_components
            .clone_from(&target_native.act_root_components);
        supported
            .asm_histories
            .clone_from(&target_native.asm_histories);
    }
    if decode::semantic_hash(&supported_target) != decode::semantic_hash(target) {
        return Err(CodecError::NotImplemented(
            "modified F3D IR contains edits beyond supported point, line, and plane carriers"
                .into(),
        ));
    }

    let active_brep = baseline
        .ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_brep"))
        .ok_or_else(|| CodecError::Malformed("F3D baseline has no active BREP".into()))?;
    let positions = target
        .model
        .points
        .iter()
        .map(|point| (point.id.0.clone(), point.position))
        .collect::<BTreeMap<_, _>>();
    let lines = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Line { origin, direction } => edited_curves
                .contains(curve.id.as_str())
                .then(|| (curve.id.0.clone(), (origin, direction))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let conics = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => edited_curves.contains(curve.id.as_str()).then(|| {
                (
                    curve.id.0.clone(),
                    (center, axis, ref_direction, radius, radius),
                )
            }),
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => edited_curves.contains(curve.id.as_str()).then(|| {
                (
                    curve.id.0.clone(),
                    (center, axis, major_direction, major_radius, minor_radius),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let planes = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (origin, normal, u_axis))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let spheres = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (center, axis, ref_direction, radius))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let tori = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => edited_surfaces.contains(surface.id.as_str()).then(|| {
                (
                    surface.id.0.clone(),
                    (center, axis, ref_direction, major_radius, minor_radius),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let cones = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            }
            | SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ..
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (origin, axis, ref_direction, radius))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    let mut archive = zip::ZipArchive::new(Cursor::new(source_image))
        .map_err(|error| CodecError::Malformed(format!("retained F3D ZIP is invalid: {error}")))?;
    let output = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(output);
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| CodecError::Malformed(format!("invalid F3D ZIP entry: {error}")))?;
        let name = entry.name().to_owned();
        let options = SimpleFileOptions::default().compression_method(entry.compression());
        if entry.is_dir() {
            zip.add_directory(name, options).map_err(|error| {
                CodecError::Malformed(format!("cannot write F3D directory: {error}"))
            })?;
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        if name == *active_brep {
            patch_geometry(
                &mut bytes,
                &positions,
                &lines,
                &conics,
                &planes,
                &spheres,
                &tori,
                &cones,
                &body_transform_edits,
                &entity_color_edits,
                &edge_range_edits,
                &face_sense_edits,
                &coedge_sense_edits,
            )?;
            if let Some(edits) = history_state_edits.get(&name) {
                patch_history_states(&mut bytes, edits)?;
            }
        } else {
            if let Some(edits) = sketch_point_edits.get(&name) {
                patch_sketch_points(&mut bytes, edits)?;
            }
            if let Some(edits) = sketch_curve_edits.get(&name) {
                patch_sketch_curves(&mut bytes, edits)?;
            }
            if let Some(edits) = sketch_relation_edits.get(&name) {
                patch_sketch_relations(&mut bytes, edits)?;
            }
            if let Some(edits) = persistent_reference_edits.get(&name) {
                patch_persistent_references(&mut bytes, edits)?;
            }
            if let Some(edits) = construction_recipe_edits.get(&name) {
                patch_construction_recipes(&mut bytes, edits)?;
            }
            if let Some(edits) = body_member_edits.get(&name) {
                patch_body_members(&mut bytes, edits)?;
            }
            if let Some(edits) = entity_header_edits.get(&name) {
                patch_entity_headers(&mut bytes, edits)?;
            }
            if let Some(edits) = design_object_edits.get(&name) {
                patch_design_objects(&mut bytes, edits)?;
            }
            if let Some(edits) = act_guid_edits.get(&name) {
                patch_act_guids(&mut bytes, edits)?;
            }
            if let Some(edits) = act_root_edits.get(&name) {
                patch_act_roots(&mut bytes, edits)?;
            }
        }
        zip.start_file(name, options)
            .map_err(|error| CodecError::Malformed(format!("cannot write F3D entry: {error}")))?;
        zip.write_all(&bytes)?;
    }
    let output = zip
        .finish()
        .map_err(|error| CodecError::Malformed(format!("cannot finish F3D ZIP: {error}")))?
        .into_inner();
    writer.write_all(&output)?;
    Ok(())
}

type SketchPointEdit = (u64, u32, cadmpeg_ir::math::Point2);
type PersistentReferenceEdit = (u64, u32, u64);
type BodyMemberEdit = (u64, u64, u16);
type ActGuidEdit = (u64, Vec<u8>);

fn validate_act_guid_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ActGuidEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.act_guids[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.act_guids[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|guid| (guid.id.as_str(), guid))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|guid| (guid.id.as_str(), guid))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT GUID regeneration requires the unchanged GUID-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActGuidEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized: ActGuid = after.clone();
        normalized.guid.clone_from(&before.guid);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT GUID edit changes fields other than guid: {id}"
            )));
        }
        if after.guid == before.guid {
            continue;
        }
        if after.guid.encode_utf16().count() != before.guid.encode_utf16().count() {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT GUID {id} must retain its UTF-16 length"
            )));
        }
        if !canonical_guid(&after.guid) {
            return Err(CodecError::Malformed(format!(
                "F3D ACT GUID {id} is not canonical"
            )));
        }
        let encoded = after
            .guid
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let stream = native_stream(id, ":act-guid#")?;
        edits
            .entry(stream)
            .or_default()
            .push((after.guid_offset, encoded));
    }
    Ok(edits)
}

fn patch_act_guids(bytes: &mut [u8], edits: &[ActGuidEdit]) -> Result<(), CodecError> {
    for (offset, encoded) in edits {
        patch_bytes_at(bytes, *offset, encoded, "ACT GUID")?;
    }
    Ok(())
}

fn validate_act_root_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ActRootComponent>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.act_root_components[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.act_root_components[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|root| (root.id.as_str(), root))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|root| (root.id.as_str(), root))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT root regeneration requires the unchanged root-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActRootComponent>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_index = before.record_index;
        normalized.instance_root_record = before.instance_root_record;
        normalized.components_root_record = before.components_root_record;
        normalized.registry_flag = before.registry_flag;
        normalized.entity_id.clone_from(&before.entity_id);
        normalized.display_name.clone_from(&before.display_name);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT root edit changes fields outside fixed numeric graph links: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.registry_flag > 1 {
            return Err(CodecError::Malformed(format!(
                "F3D ACT root registry flag must be zero or one: {id}"
            )));
        }
        if after.entity_id.encode_utf16().count() != before.entity_id.encode_utf16().count()
            || after.display_name.encode_utf16().count()
                != before.display_name.encode_utf16().count()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT root strings must retain their UTF-16 lengths: {id}"
            )));
        }
        edits
            .entry(native_stream(id, ":act-root-component#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

fn patch_act_roots(bytes: &mut [u8], edits: &[ActRootComponent]) -> Result<(), CodecError> {
    for root in edits {
        for (offset, value, field) in [
            (
                root.record_index_offset,
                root.record_index,
                "ACT root record index",
            ),
            (
                root.instance_root_record_offset,
                root.instance_root_record,
                "ACT instance-root reference",
            ),
            (
                root.components_root_record_offset,
                root.components_root_record,
                "ACT components-root reference",
            ),
            (
                root.registry_flag_offset,
                root.registry_flag,
                "ACT registry flag",
            ),
        ] {
            patch_u32_at(bytes, offset, value, field)?;
        }
        patch_utf16_if_changed(
            bytes,
            root.entity_id_offset,
            &root.entity_id,
            "ACT root entity id",
        )?;
        patch_utf16_if_changed(
            bytes,
            root.display_name_offset,
            &root.display_name,
            "ACT root display name",
        )?;
    }
    Ok(())
}

fn patch_utf16_if_changed(
    bytes: &mut [u8],
    offset: u64,
    value: &str,
    field: &str,
) -> Result<(), CodecError> {
    let encoded = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    patch_bytes_at(bytes, offset, &encoded, field)
}

fn canonical_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn native_stream(id: &str, delimiter: &str) -> Result<String, CodecError> {
    id.strip_prefix("f3d:")
        .and_then(|id| id.rsplit_once(delimiter))
        .map(|(stream, _)| stream.to_owned())
        .ok_or_else(|| CodecError::Malformed(format!("invalid native record id {id}")))
}

fn patch_bytes_at(
    bytes: &mut [u8],
    offset: u64,
    encoded: &[u8],
    field: &str,
) -> Result<(), CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed(format!("{field} offset exceeds address space")))?;
    bytes
        .get_mut(start..start + encoded.len())
        .ok_or_else(|| CodecError::Malformed(format!("{field} is truncated")))?
        .copy_from_slice(encoded);
    Ok(())
}

struct DesignObjectEdit {
    integers: Vec<(u64, Vec<u8>)>,
    strings: Vec<(u64, Vec<u8>)>,
}

fn validate_design_object_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<DesignObjectEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.design_objects[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.design_objects[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D design-object regeneration requires the unchanged object-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<DesignObjectEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_ids.clone_from(&before.entity_ids);
        normalized.self_guid.clone_from(&before.self_guid);
        normalized.parent_guid.clone_from(&before.parent_guid);
        normalized.revision = before.revision;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D design-object edit changes fields outside its fixed object payload: {id}"
            )));
        }
        if after.entity_ids.len() != before.entity_ids.len()
            || after.entity_ids.len() != after.entity_id_offsets.len()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D design object {id} must retain its entity-id cardinality"
            )));
        }
        let mut integers = after
            .entity_ids
            .iter()
            .zip(&before.entity_ids)
            .zip(&after.entity_id_offsets)
            .filter(|((value, before), _)| value != before)
            .map(|((&value, _), &offset)| (offset, value.to_le_bytes().to_vec()))
            .collect::<Vec<_>>();
        if after.revision != before.revision {
            integers.push((after.revision_offset, after.revision.to_le_bytes().to_vec()));
        }
        let mut strings = Vec::new();
        if after.self_guid != before.self_guid {
            validate_fixed_design_string(id, &before.self_guid, &after.self_guid)?;
            strings.push((after.self_guid_offset, after.self_guid.as_bytes().to_vec()));
        }
        if after.parent_guid != before.parent_guid {
            let before_parent = before.parent_guid.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot add F3D object parent GUID: {id}"))
            })?;
            let after_parent = after.parent_guid.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D object parent GUID: {id}"))
            })?;
            validate_fixed_design_string(id, before_parent, after_parent)?;
            strings.push((
                after.parent_guid_offset.ok_or_else(|| {
                    CodecError::Malformed(format!("F3D object {id} has no parent-GUID offset"))
                })?,
                after_parent.as_bytes().to_vec(),
            ));
        }
        if integers.is_empty() && strings.is_empty() {
            continue;
        }
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":design-object#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid design-object id {id}")))?;
        edits
            .entry(stream)
            .or_default()
            .push(DesignObjectEdit { integers, strings });
    }
    Ok(edits)
}

fn validate_fixed_design_string(id: &str, before: &str, after: &str) -> Result<(), CodecError> {
    if before.len() != after.len()
        || !after
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(CodecError::NotImplemented(format!(
            "F3D object string {id} must retain its encoded length and alphabet"
        )));
    }
    Ok(())
}

fn patch_design_objects(bytes: &mut [u8], edits: &[DesignObjectEdit]) -> Result<(), CodecError> {
    for edit in edits {
        for (offset, encoded) in edit.integers.iter().chain(&edit.strings) {
            let start = usize::try_from(*offset).map_err(|_| {
                CodecError::Malformed("design-object offset exceeds address space".into())
            })?;
            bytes
                .get_mut(start..start + encoded.len())
                .ok_or_else(|| CodecError::Malformed("design-object field is truncated".into()))?
                .copy_from_slice(encoded);
        }
    }
    Ok(())
}

struct EntityHeaderEdit {
    record_reference: Option<(u64, u32)>,
    references: Vec<(u64, u32)>,
}

fn validate_entity_header_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<EntityHeaderEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.design_entity_headers[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.design_entity_headers[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D entity-header regeneration requires the unchanged header-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<EntityHeaderEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_reference = before.record_reference;
        normalized
            .reference_indices
            .clone_from(&before.reference_indices);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D entity-header edit changes fields outside fixed record references: {id}"
            )));
        }
        if after.record_reference == before.record_reference
            && after.reference_indices == before.reference_indices
        {
            continue;
        }
        if after.reference_indices.len() != after.reference_offsets.len() {
            return Err(CodecError::Malformed(format!(
                "F3D entity header {id} has mismatched reference values and offsets"
            )));
        }
        let record_reference = if after.record_reference == before.record_reference {
            None
        } else {
            Some((
                after.record_reference_offset.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "F3D entity header {id} has no writable owning-record reference"
                    ))
                })?,
                after.record_reference.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "cannot remove F3D entity-header record reference: {id}"
                    ))
                })?,
            ))
        };
        let references = after
            .reference_offsets
            .iter()
            .copied()
            .zip(after.reference_indices.iter().copied())
            .zip(&before.reference_indices)
            .filter_map(|((offset, value), before)| (value != *before).then_some((offset, value)))
            .collect();
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":design-entity-header#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid design-entity-header id {id}"))
            })?;
        edits.entry(stream).or_default().push(EntityHeaderEdit {
            record_reference,
            references,
        });
    }
    Ok(edits)
}

fn patch_entity_headers(bytes: &mut [u8], edits: &[EntityHeaderEdit]) -> Result<(), CodecError> {
    for edit in edits {
        if let Some((offset, value)) = edit.record_reference {
            patch_u32_at(bytes, offset, value, "entity-header record reference")?;
        }
        for &(offset, value) in &edit.references {
            patch_u32_at(bytes, offset, value, "entity-header child reference")?;
        }
    }
    Ok(())
}

fn patch_u32_at(bytes: &mut [u8], offset: u64, value: u32, field: &str) -> Result<(), CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed(format!("{field} offset exceeds address space")))?;
    bytes
        .get_mut(start..start + 4)
        .ok_or_else(|| CodecError::Malformed(format!("{field} is truncated")))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn validate_body_member_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<BodyMemberEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.design_body_members[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.design_body_members[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|member| (member.id.as_str(), member))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|member| (member.id.as_str(), member))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-membership regeneration requires the unchanged member-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<BodyMemberEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_suffix = before.entity_suffix;
        normalized.flags = before.flags;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body-member edit changes fields outside its fixed payload: {id}"
            )));
        }
        if after.entity_suffix == before.entity_suffix && after.flags == before.flags {
            continue;
        }
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":design-body-member#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid design-body-member id {id}")))?;
        edits.entry(stream).or_default().push((
            after.byte_offset,
            after.entity_suffix,
            after.flags,
        ));
    }
    Ok(edits)
}

fn patch_body_members(bytes: &mut [u8], edits: &[BodyMemberEdit]) -> Result<(), CodecError> {
    for &(offset, entity_suffix, flags) in edits {
        let start = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("design-body-member offset exceeds address space".into())
        })?;
        if bytes.get(start) != Some(&1) {
            return Err(CodecError::Malformed(format!(
                "design-body-member at byte {start} has no presence marker"
            )));
        }
        bytes
            .get_mut(start + 1..start + 9)
            .ok_or_else(|| CodecError::Malformed("design-body-member id is truncated".into()))?
            .copy_from_slice(&entity_suffix.to_le_bytes());
        bytes
            .get_mut(start + 9..start + 11)
            .ok_or_else(|| CodecError::Malformed("design-body-member flags are truncated".into()))?
            .copy_from_slice(&flags.to_le_bytes());
    }
    Ok(())
}
struct ConstructionRecipeEdit {
    record_index: Option<(u64, i32)>,
    design_id: Option<(u64, Vec<u8>)>,
}

fn validate_construction_recipe_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ConstructionRecipeEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.construction_recipes[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.construction_recipes[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D construction-recipe regeneration requires the unchanged recipe-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ConstructionRecipeEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_index = before.record_index;
        normalized.design_id.clone_from(&before.design_id);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D construction-recipe edit changes fields other than record_index or design_id: {id}"
            )));
        }
        if after.record_index == before.record_index && after.design_id == before.design_id {
            continue;
        }
        let record_index = (after.record_index != before.record_index)
            .then(|| {
                after
                    .record_index_offset
                    .map(|offset| (offset, after.record_index))
                    .ok_or_else(|| {
                        CodecError::NotImplemented(format!(
                            "F3D construction recipe {id} has no writable record-index carrier"
                        ))
                    })
            })
            .transpose()?;
        let design_id = if after.design_id == before.design_id {
            None
        } else {
            let before_value = before.design_id.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot add F3D recipe design id: {id}"))
            })?;
            let after_value = after.design_id.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D recipe design id: {id}"))
            })?;
            let offset = after.design_id_offset.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "F3D construction recipe {id} has no writable design-id carrier"
                ))
            })?;
            let encoded = if after.design_id_binary_u32 {
                after_value
                    .parse::<u32>()
                    .map_err(|_| {
                        CodecError::Malformed(format!(
                            "binary F3D recipe design id is not a u32: {after_value}"
                        ))
                    })?
                    .to_le_bytes()
                    .to_vec()
            } else {
                if after_value.len() != before_value.len()
                    || !after_value.bytes().all(|byte| byte.is_ascii_alphanumeric())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "ASCII F3D recipe design id {id} must retain its encoded length"
                    )));
                }
                after_value.as_bytes().to_vec()
            };
            Some((offset, encoded))
        };
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":construction-recipe#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid construction-recipe id {id}")))?;
        edits
            .entry(stream)
            .or_default()
            .push(ConstructionRecipeEdit {
                record_index,
                design_id,
            });
    }
    Ok(edits)
}

fn patch_construction_recipes(
    bytes: &mut [u8],
    edits: &[ConstructionRecipeEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        if let Some((offset, record_index)) = edit.record_index {
            let start = usize::try_from(offset).map_err(|_| {
                CodecError::Malformed("construction-recipe offset exceeds address space".into())
            })?;
            bytes
                .get_mut(start..start + 4)
                .ok_or_else(|| {
                    CodecError::Malformed("construction-recipe record index is truncated".into())
                })?
                .copy_from_slice(&record_index.to_le_bytes());
        }
        if let Some((offset, encoded)) = &edit.design_id {
            let start = usize::try_from(*offset).map_err(|_| {
                CodecError::Malformed(
                    "construction-recipe design-id offset exceeds address space".into(),
                )
            })?;
            bytes
                .get_mut(start..start + encoded.len())
                .ok_or_else(|| {
                    CodecError::Malformed("construction-recipe design id is truncated".into())
                })?
                .copy_from_slice(encoded);
        }
    }
    Ok(())
}

fn validate_persistent_reference_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<PersistentReferenceEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.persistent_references[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.persistent_references[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D persistent-reference regeneration requires the unchanged reference-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<PersistentReferenceEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.value = before.value;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D persistent-reference edit changes fields other than value: {id}"
            )));
        }
        if after.value == before.value {
            continue;
        }
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":persistent-reference#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid persistent-reference id {id}"))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push((after.byte_offset, after.value_offset, after.value));
    }
    Ok(edits)
}

fn patch_persistent_references(
    bytes: &mut [u8],
    edits: &[PersistentReferenceEdit],
) -> Result<(), CodecError> {
    for &(record_offset, value_offset, value) in edits {
        let start = usize::try_from(record_offset)
            .ok()
            .and_then(|offset| offset.checked_add(value_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("persistent-reference offset exceeds address space".into())
            })?;
        bytes
            .get_mut(start..start + 8)
            .ok_or_else(|| CodecError::Malformed("persistent-reference value is truncated".into()))?
            .copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

#[derive(Default)]
struct HistoryEdits {
    preamble: Option<AsmHistory>,
    states: Vec<AsmDeltaState>,
    boards: Vec<AsmBulletinBoard>,
    changes: Vec<AsmEntityChange>,
}

fn validate_history_state_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, HistoryEdits>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.asm_histories[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.asm_histories[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|history| (history.id.as_str(), history))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id
        .keys()
        .copied()
        .ne(target.iter().map(|history| history.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D history regeneration requires the unchanged history-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, HistoryEdits> = BTreeMap::new();
    for history in target {
        let before = baseline_by_id[history.id.as_str()];
        if before.states.len() != history.states.len() {
            return Err(CodecError::NotImplemented(format!(
                "F3D history edit changes the state count: {}",
                history.id
            )));
        }
        let mut normalized = history.clone();
        normalized.stream_size = before.stream_size;
        normalized.high_water_mark = before.high_water_mark;
        for (state, before_state) in normalized.states.iter_mut().zip(&before.states) {
            state.state_id = before_state.state_id;
            state.version_flag = before_state.version_flag;
            state.state_flag = before_state.state_flag;
            state.previous_ref = before_state.previous_ref;
            state.next_ref = before_state.next_ref;
            state.node_index = before_state.node_index;
            state.partner_ref = before_state.partner_ref;
            state.owner_ref = before_state.owner_ref;
            if state.bulletin_boards.len() != before_state.bulletin_boards.len() {
                return Err(CodecError::NotImplemented(format!(
                    "F3D history edit changes the bulletin-board count: {}",
                    history.id
                )));
            }
            for (board, before_board) in state
                .bulletin_boards
                .iter_mut()
                .zip(&before_state.bulletin_boards)
            {
                board.owner_ref = before_board.owner_ref;
                board.number = before_board.number;
                if board.changes.len() != before_board.changes.len() {
                    return Err(CodecError::NotImplemented(format!(
                        "F3D history edit changes the entity-change count: {}",
                        board.id
                    )));
                }
                for (change, before_change) in board.changes.iter_mut().zip(&before_board.changes) {
                    change.kind = before_change.kind;
                    change.old_ref = before_change.old_ref;
                    change.new_ref = before_change.new_ref;
                }
            }
        }
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D history edit changes fields outside the fixed delta-state header: {}",
                history.id
            )));
        }
        let stream = history
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":asm-history#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid ASM history id {}", history.id))
            })?;
        if let Some(size) = history.stream_size {
            if history
                .states
                .first()
                .is_none_or(|state| state.state_id != size)
                || history
                    .high_water_mark
                    .is_none_or(|high_water| high_water < size)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size <= high_water_mark",
                    history.id
                )));
            }
        }
        if history.stream_size != before.stream_size
            || history.high_water_mark != before.high_water_mark
        {
            let (Some(size), Some(high_water)) = (history.stream_size, history.high_water_mark)
            else {
                return Err(CodecError::NotImplemented(format!(
                    "cannot add or remove the F3D history preamble: {}",
                    history.id
                )));
            };
            if history.byte_offset == 0 || high_water < size {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size <= high_water_mark",
                    history.id
                )));
            }
            edits.entry(stream.clone()).or_default().preamble = Some(history.clone());
        }
        for (state, before_state) in history.states.iter().zip(&before.states) {
            if state != before_state {
                edits
                    .entry(stream.clone())
                    .or_default()
                    .states
                    .push(state.clone());
            }
            for (board, before_board) in state
                .bulletin_boards
                .iter()
                .zip(&before_state.bulletin_boards)
            {
                if board.owner_ref != before_board.owner_ref || board.number != before_board.number
                {
                    edits
                        .entry(stream.clone())
                        .or_default()
                        .boards
                        .push(board.clone());
                }
                for (change, before_change) in board.changes.iter().zip(&before_board.changes) {
                    if change != before_change {
                        if change.kind != history_change_kind(change.old_ref, change.new_ref)? {
                            return Err(CodecError::Malformed(format!(
                                "F3D entity change {} has a kind inconsistent with its references",
                                change.id
                            )));
                        }
                        edits
                            .entry(stream.clone())
                            .or_default()
                            .changes
                            .push(change.clone());
                    }
                }
            }
        }
    }
    Ok(edits)
}

fn patch_history_states(bytes: &mut [u8], edits: &HistoryEdits) -> Result<(), CodecError> {
    const DELTA_HEADER_LEN: usize = b"\x11\x0d\x0bdelta_state".len();
    const PREAMBLE_LEN: usize = b"\x0d\x0ehistory_stream".len();
    if let Some(history) = &edits.preamble {
        let start = usize::try_from(history.byte_offset)
            .ok()
            .and_then(|offset| offset.checked_add(PREAMBLE_LEN))
            .ok_or_else(|| {
                CodecError::Malformed("ASM preamble offset exceeds address space".into())
            })?;
        let size = history.stream_size.expect("validated history preamble");
        let high_water = history.high_water_mark.expect("validated history preamble");
        for (ordinal, value) in [(0, size), (1, size), (3, high_water)] {
            let tag = start + ordinal * 9;
            if bytes.get(tag) != Some(&0x04) {
                return Err(CodecError::Malformed(format!(
                    "ASM history-preamble field {ordinal} at byte {tag} is not a long token"
                )));
            }
            bytes
                .get_mut(tag + 1..tag + 9)
                .ok_or_else(|| CodecError::Malformed("ASM history preamble is truncated".into()))?
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for state in &edits.states {
        let first_tag = usize::try_from(state.byte_offset)
            .ok()
            .and_then(|offset| offset.checked_add(DELTA_HEADER_LEN))
            .ok_or_else(|| {
                CodecError::Malformed("ASM history offset exceeds address space".into())
            })?;
        let values = [
            (0, 0x04, state.state_id),
            (1, 0x04, state.version_flag),
            (2, 0x04, state.state_flag),
            (3, 0x0c, state.previous_ref.unwrap_or(-1)),
            (4, 0x0c, state.next_ref.unwrap_or(-1)),
            (5, 0x0c, state.node_index),
            (6, 0x0c, state.partner_ref.unwrap_or(-1)),
            (7, 0x0c, state.owner_ref),
        ];
        for (ordinal, expected_tag, value) in values {
            let tag = first_tag + ordinal * 9;
            if bytes.get(tag) != Some(&expected_tag) {
                return Err(CodecError::Malformed(format!(
                    "ASM delta-state field {ordinal} at byte {tag} has the wrong token tag"
                )));
            }
            bytes
                .get_mut(tag + 1..tag + 9)
                .ok_or_else(|| CodecError::Malformed("ASM delta-state field is truncated".into()))?
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for board in &edits.boards {
        patch_tagged_i64(bytes, board.byte_offset, 1, 0x0c, board.owner_ref)?;
        patch_tagged_i64(bytes, board.byte_offset, 2, 0x04, board.number)?;
    }
    for change in &edits.changes {
        patch_tagged_i64(
            bytes,
            change.byte_offset,
            1,
            0x0c,
            change.old_ref.unwrap_or(-1),
        )?;
        patch_tagged_i64(
            bytes,
            change.byte_offset,
            2,
            0x0c,
            change.new_ref.unwrap_or(-1),
        )?;
    }
    Ok(())
}

fn history_change_kind(
    old_ref: Option<i64>,
    new_ref: Option<i64>,
) -> Result<AsmEntityChangeKind, CodecError> {
    match (old_ref, new_ref) {
        (None, Some(_)) => Ok(AsmEntityChangeKind::Insert),
        (Some(_), None) => Ok(AsmEntityChangeKind::Delete),
        (Some(_), Some(_)) => Ok(AsmEntityChangeKind::Update),
        (None, None) => Err(CodecError::Malformed(
            "ASM entity change cannot have two null references".into(),
        )),
    }
}

fn patch_tagged_i64(
    bytes: &mut [u8],
    record_offset: u64,
    ordinal: usize,
    expected_tag: u8,
    value: i64,
) -> Result<(), CodecError> {
    let tag = usize::try_from(record_offset)
        .ok()
        .and_then(|offset| offset.checked_add(ordinal * 9))
        .ok_or_else(|| CodecError::Malformed("ASM record offset exceeds address space".into()))?;
    if bytes.get(tag) != Some(&expected_tag) {
        return Err(CodecError::Malformed(format!(
            "ASM field {ordinal} at byte {tag} has the wrong token tag"
        )));
    }
    bytes
        .get_mut(tag + 1..tag + 9)
        .ok_or_else(|| CodecError::Malformed("ASM tagged integer is truncated".into()))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn validate_sketch_point_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchPointEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|point| point.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-point regeneration requires the unchanged point-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchPointEdit>> = BTreeMap::new();
    for point in target {
        let before = by_id[point.id.as_str()];
        let mut normalized = point.clone();
        normalized.coordinates = before.coordinates;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-point edit changes fields other than coordinates: {}",
                point.id
            )));
        }
        if point.coordinates == before.coordinates {
            continue;
        }
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            return Err(CodecError::Malformed(format!(
                "F3D sketch point {} has non-finite coordinates",
                point.id
            )));
        }
        let stream = point
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-point#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-point id {}", point.id))
            })?;
        edits.entry(stream).or_default().push((
            point.byte_offset,
            point.coordinate_offset,
            point.coordinates,
        ));
    }
    Ok(edits)
}

fn patch_sketch_points(bytes: &mut [u8], edits: &[SketchPointEdit]) -> Result<(), CodecError> {
    for (record_offset, coordinate_offset, coordinates) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*coordinate_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-point offset exceeds address space".into())
            })?;
        let payload = bytes.get_mut(start..start + 16).ok_or_else(|| {
            CodecError::Malformed("sketch-point coordinate payload is outside BulkStream".into())
        })?;
        payload[..8].copy_from_slice(&(coordinates.u / 10.0).to_le_bytes());
        payload[8..].copy_from_slice(&(coordinates.v / 10.0).to_le_bytes());
    }
    Ok(())
}

type SketchCurveEdit = (u64, u32, SketchCurveGeometry);

fn validate_sketch_curve_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchCurveEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|curve| curve.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchCurveEdit>> = BTreeMap::new();
    for curve in target {
        let before = by_id[curve.id.as_str()];
        let mut normalized = curve.clone();
        normalized.geometry.clone_from(&before.geometry);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit changes fields other than geometry: {}",
                curve.id
            )));
        }
        if curve.geometry == before.geometry {
            continue;
        }
        let geometry = curve.geometry.clone().ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove sketch-curve geometry: {}", curve.id))
        })?;
        if !valid_sketch_geometry(&geometry)
            || !same_sketch_layout(before.geometry.as_ref(), &geometry)
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit requires valid geometry with the original native layout: {}",
                curve.id
            )));
        }
        let stream = curve
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-curve-identity#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-curve id {}", curve.id))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push((curve.byte_offset, curve.geometry_offset, geometry));
    }
    Ok(edits)
}

fn same_sketch_layout(before: Option<&SketchCurveGeometry>, after: &SketchCurveGeometry) -> bool {
    match (before, after) {
        (Some(SketchCurveGeometry::Line { .. }), SketchCurveGeometry::Line { .. })
        | (Some(SketchCurveGeometry::Arc { .. }), SketchCurveGeometry::Arc { .. }) => true,
        (
            Some(SketchCurveGeometry::Nurbs {
                carrier_reference: old_carrier,
                subtype_class_tag: old_tag,
                subtype_record_index: old_index,
                degree: old_degree,
                scalar_width: old_width,
                knots: old_knots,
                weights: old_weights,
                control_points: old_points,
                ..
            }),
            SketchCurveGeometry::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                scalar_width,
                knots,
                weights,
                control_points,
                ..
            },
        ) => {
            old_carrier == carrier_reference
                && old_tag == subtype_class_tag
                && old_index == subtype_record_index
                && old_degree == degree
                && old_width == scalar_width
                && old_knots.len() == knots.len()
                && old_weights.len() == weights.len()
                && old_points.len() == control_points.len()
        }
        _ => false,
    }
}

fn valid_sketch_geometry(geometry: &SketchCurveGeometry) -> bool {
    match geometry {
        SketchCurveGeometry::Line {
            start,
            end,
            direction,
            normal,
        } => finite_point(*start) && finite_point(*end) && orthonormal_pair(*direction, *normal),
        SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            finite_point(*center)
                && orthonormal_pair(*normal, *reference_direction)
                && radius.is_finite()
                && *radius > 0.0
                && start_angle.is_finite()
                && end_angle.is_finite()
        }
        SketchCurveGeometry::Nurbs {
            degree,
            fit_tolerance,
            scalar_width,
            knots,
            weights,
            control_points,
            ..
        } => {
            *scalar_width == 8
                && fit_tolerance.is_finite()
                && *fit_tolerance >= 0.0
                && knots.len() == control_points.len() + *degree as usize + 1
                && knots.iter().all(|knot| knot.is_finite())
                && knots.windows(2).all(|pair| pair[0] <= pair[1])
                && (weights.is_empty() || weights.len() == control_points.len())
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
                && control_points.iter().all(|point| finite_point(*point))
        }
    }
}

fn patch_sketch_curves(bytes: &mut [u8], edits: &[SketchCurveEdit]) -> Result<(), CodecError> {
    for (record_offset, geometry_offset, geometry) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*geometry_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-curve offset exceeds address space".into())
            })?;
        if let SketchCurveGeometry::Nurbs {
            fit_tolerance,
            knots,
            weights,
            control_points,
            ..
        } = geometry
        {
            patch_sketch_nurbs(bytes, start, *fit_tolerance, knots, weights, control_points)?;
            continue;
        }
        let payload = bytes.get_mut(start..start + 96).ok_or_else(|| {
            CodecError::Malformed("sketch-curve analytic payload is outside BulkStream".into())
        })?;
        let values = match geometry {
            SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            } => [
                start.x / 10.0,
                start.y / 10.0,
                start.z / 10.0,
                (end.x - start.x) / 10.0,
                (end.y - start.y) / 10.0,
                (end.z - start.z) / 10.0,
                direction.x,
                direction.y,
                direction.z,
                normal.x,
                normal.y,
                normal.z,
            ],
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => [
                center.x / 10.0,
                center.y / 10.0,
                center.z / 10.0,
                normal.x,
                normal.y,
                normal.z,
                reference_direction.x,
                reference_direction.y,
                reference_direction.z,
                radius / 10.0,
                *start_angle,
                *end_angle,
            ],
            SketchCurveGeometry::Nurbs { .. } => unreachable!("NURBS handled before fixed payload"),
        };
        for (ordinal, value) in values.into_iter().enumerate() {
            payload[ordinal * 8..ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

fn patch_sketch_nurbs(
    bytes: &mut [u8],
    start: usize,
    fit_tolerance: f64,
    knots: &[f64],
    weights: &[f64],
    control_points: &[Point3],
) -> Result<(), CodecError> {
    let fit_at = start + 94;
    let knots_at = start + 114;
    let weights_header = knots_at + knots.len() * 8;
    let weights_at = weights_header + 12;
    let points_header = weights_at + weights.len() * 8;
    let points_at = points_header + 12;
    let end = points_at + control_points.len() * 24;
    if end > bytes.len() {
        return Err(CodecError::Malformed(
            "sketch NURBS arrays extend beyond BulkStream".into(),
        ));
    }
    bytes[fit_at..fit_at + 8].copy_from_slice(&(fit_tolerance / 10.0).to_le_bytes());
    for (ordinal, value) in knots.iter().enumerate() {
        let at = knots_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, value) in weights.iter().enumerate() {
        let at = weights_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, point) in control_points.iter().enumerate() {
        let at = points_at + ordinal * 24;
        for (component, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            let component_at = at + component * 8;
            bytes[component_at..component_at + 8].copy_from_slice(&(value / 10.0).to_le_bytes());
        }
    }
    Ok(())
}

type SketchRelationEdit = (u64, u32, u32);

fn validate_sketch_relation_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchRelationEdit>>, CodecError> {
    let baseline = baseline
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let target = target
        .native
        .f3d
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|relation| (relation.id.as_str(), relation))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|relation| relation.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-relation regeneration requires the unchanged relation-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchRelationEdit>> = BTreeMap::new();
    for relation in target {
        let before = by_id[relation.id.as_str()];
        let mut normalized = relation.clone();
        normalized.state = before.state;
        normalized
            .constraint_kinds
            .clone_from(&before.constraint_kinds);
        normalized.unknown_constraint_bits = before.unknown_constraint_bits;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-relation edit changes fields other than its constraint mask: {}",
                relation.id
            )));
        }
        if relation.state == before.state {
            continue;
        }
        let (kinds, unknown) = crate::design::decode_constraint_kinds(relation.state);
        if kinds != relation.constraint_kinds || unknown != relation.unknown_constraint_bits {
            return Err(CodecError::Malformed(format!(
                "F3D sketch relation {} has a mask inconsistent with its typed constraint kinds",
                relation.id
            )));
        }
        let stream = relation
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-relation#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-relation id {}", relation.id))
            })?;
        edits.entry(stream).or_default().push((
            relation.byte_offset,
            relation.state_offset,
            relation.state,
        ));
    }
    Ok(edits)
}

fn patch_sketch_relations(
    bytes: &mut [u8],
    edits: &[SketchRelationEdit],
) -> Result<(), CodecError> {
    for (record_offset, state_offset, state) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*state_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-relation offset exceeds address space".into())
            })?;
        let payload = bytes.get_mut(start..start + 4).ok_or_else(|| {
            CodecError::Malformed("sketch-relation mask is outside BulkStream".into())
        })?;
        payload.copy_from_slice(&state.to_le_bytes());
    }
    Ok(())
}

fn validate_body_transform_edits(
    baseline: &[Body],
    target: &[Body],
) -> Result<BTreeMap<String, Transform>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-transform regeneration requires the unchanged body-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.transform = before.transform;
        normalized.color = before.color;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body edit changes fields other than transform: {id}"
            )));
        }
        if after.transform == before.transform {
            continue;
        }
        let transform = after.transform.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body transform: {id}"))
        })?;
        if !valid_transform(transform) || before.transform.is_none() {
            return Err(CodecError::NotImplemented(format!(
                "F3D body transform {id} must replace an existing finite affine transform"
            )));
        }
        edits.insert(id.to_owned(), transform);
    }
    Ok(edits)
}

fn validate_body_color_edits(
    baseline: &[Body],
    target: &[Body],
) -> Result<BTreeMap<String, Color>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-color regeneration requires the unchanged body-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.color = before.color;
        normalized.transform = before.transform;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body edit changes fields other than transform or color: {id}"
            )));
        }
        if after.color == before.color {
            continue;
        }
        let color = after.color.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body color: {id}"))
        })?;
        if before.color.is_none()
            || ![color.r, color.g, color.b, color.a]
                .into_iter()
                .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
            || color.a != 1.0
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D body color {id} must replace an existing opaque finite RGB color"
            )));
        }
        edits.insert(id.to_owned(), color);
    }
    Ok(edits)
}

fn validate_edge_range_edits(
    baseline: &[Edge],
    target: &[Edge],
) -> Result<BTreeMap<String, [f64; 2]>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-range regeneration requires the unchanged edge-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.param_range = before.param_range;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge edit changes fields other than parameter range: {id}"
            )));
        }
        if after.param_range == before.param_range {
            continue;
        }
        let range = after.param_range.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D edge range: {id}"))
        })?;
        if before.param_range.is_none()
            || !range[0].is_finite()
            || !range[1].is_finite()
            || range[0] == range[1]
        {
            return Err(CodecError::Malformed(format!(
                "edited F3D edge range {id} must replace an existing finite non-degenerate range"
            )));
        }
        edits.insert(id.to_owned(), range);
    }
    Ok(edits)
}

fn validate_face_sense_edits(
    baseline: &[Face],
    target: &[Face],
) -> Result<BTreeMap<String, Sense>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-sense regeneration requires the unchanged face-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        normalized.color = before.color;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face edit changes fields other than sense: {id}"
            )));
        }
        if after.sense != before.sense {
            edits.insert(id.to_owned(), after.sense);
        }
    }
    Ok(edits)
}

fn validate_face_color_edits(
    baseline: &[Face],
    target: &[Face],
) -> Result<BTreeMap<String, Color>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-color regeneration requires the unchanged face-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.color = before.color;
        normalized.sense = before.sense;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face edit changes fields other than sense or color: {id}"
            )));
        }
        if after.color == before.color {
            continue;
        }
        let color = after.color.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D face color: {id}"))
        })?;
        if before.color.is_none()
            || ![color.r, color.g, color.b, color.a]
                .into_iter()
                .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
            || color.a != 1.0
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D face color {id} must replace an existing opaque finite RGB color"
            )));
        }
        edits.insert(id.to_owned(), color);
    }
    Ok(edits)
}

fn validate_coedge_sense_edits(
    baseline: &[Coedge],
    target: &[Coedge],
) -> Result<BTreeMap<String, Sense>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D coedge-sense regeneration requires the unchanged coedge-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D coedge edit changes fields other than sense: {id}"
            )));
        }
        if after.sense != before.sense {
            edits.insert(id.to_owned(), after.sense);
        }
    }
    Ok(edits)
}

fn valid_transform(transform: Transform) -> bool {
    transform
        .rows
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        && transform.rows[3][0] == 0.0
        && transform.rows[3][1] == 0.0
        && transform.rows[3][2] == 0.0
        && transform.rows[3][3] != 0.0
}

fn validate_curve_edits(
    baseline: &[Curve],
    target: &[Curve],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        edited.insert(id.to_owned());
        let valid = match after {
            CurveGeometry::Line { origin, direction } => {
                finite_point(*origin)
                    && finite_vector(*direction)
                    && (direction.norm() - 1.0).abs() <= 1e-9
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } if matches!(before, CurveGeometry::Circle { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius > 0.0
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } if matches!(before, CurveGeometry::Ellipse { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *major_direction)
                    && major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius > 0.0
                    && *minor_radius > 0.0
                    && *minor_radius <= *major_radius
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to curve {id}"
                )));
            }
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "edited F3D curve {id} has an invalid frame or radius"
            )));
        }
    }
    Ok(edited)
}

fn validate_surface_edits(
    baseline: &[Surface],
    target: &[Surface],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|surface| (surface.id.as_str(), &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|surface| (surface.id.as_str(), &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D surface regeneration requires the unchanged surface-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        let valid = match after {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => finite_point(*origin) && orthonormal_pair(*normal, *u_axis),
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius != 0.0
                    && *minor_radius != 0.0
            }
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } if matches!(before, SurfaceGeometry::Cylinder { .. }) => {
                finite_point(*origin)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                half_angle,
            } => {
                let unchanged_angle = matches!(
                    before,
                    SurfaceGeometry::Cone {
                        half_angle: old,
                        ..
                    } if old == half_angle
                );
                unchanged_angle
                    && finite_point(*origin)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to surface {id}"
                )));
            }
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "edited F3D surface {id} requires a finite orthonormal frame and valid radius"
            )));
        }
        edited.insert(id.to_owned());
    }
    Ok(edited)
}

fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn finite_vector(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn orthonormal_pair(first: Vector3, second: Vector3) -> bool {
    finite_vector(first)
        && finite_vector(second)
        && (first.norm() - 1.0).abs() <= 1e-9
        && (second.norm() - 1.0).abs() <= 1e-9
        && (first.x * second.x + first.y * second.y + first.z * second.z).abs() <= 1e-9
}

#[allow(clippy::too_many_arguments)]
fn patch_geometry(
    bytes: &mut [u8],
    positions: &BTreeMap<String, Point3>,
    lines: &BTreeMap<String, (Point3, Vector3)>,
    conics: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    planes: &BTreeMap<String, (Point3, Vector3, Vector3)>,
    spheres: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    tori: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    cones: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    body_transforms: &BTreeMap<String, Transform>,
    entity_colors: &BTreeMap<String, Color>,
    edge_ranges: &BTreeMap<String, [f64; 2]>,
    face_senses: &BTreeMap<String, Sense>,
    coedge_senses: &BTreeMap<String, Sense>,
) -> Result<(), CodecError> {
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let records = sab::frame(bytes, start, limit, 8)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    let header_scale = asm_header::parse(bytes)
        .and_then(|header| header.scale)
        .unwrap_or(1.0);
    patch_framed_geometry(
        bytes,
        &records,
        positions,
        lines,
        conics,
        planes,
        spheres,
        tori,
        cones,
        body_transforms,
        entity_colors,
        edge_ranges,
        face_senses,
        coedge_senses,
        header_scale,
    )
}

#[allow(clippy::too_many_arguments)]
fn patch_framed_geometry(
    bytes: &mut [u8],
    records: &[sab::Record],
    positions: &BTreeMap<String, Point3>,
    lines: &BTreeMap<String, (Point3, Vector3)>,
    conics: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    planes: &BTreeMap<String, (Point3, Vector3, Vector3)>,
    spheres: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    tori: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    cones: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    body_transforms: &BTreeMap<String, Transform>,
    entity_colors: &BTreeMap<String, Color>,
    edge_ranges: &BTreeMap<String, [f64; 2]>,
    face_senses: &BTreeMap<String, Sense>,
    coedge_senses: &BTreeMap<String, Sense>,
    header_scale: f64,
) -> Result<(), CodecError> {
    let records_by_index = records
        .iter()
        .map(|record| (record.index, record))
        .collect::<BTreeMap<_, _>>();
    let transform_records = records
        .iter()
        .filter(|record| record.head == "body")
        .filter_map(|body| {
            body_transforms
                .get(&format!("f3d:brep:entity#{}", body.index))
                .and_then(|transform| {
                    body.ref_at(5)
                        .map(|reference| (reference as usize, *transform))
                })
        })
        .collect::<BTreeMap<_, _>>();
    let mut color_records = BTreeMap::new();
    for entity in records
        .iter()
        .filter(|record| record.head == "body" || record.head == "face")
    {
        let id = format!("f3d:brep:entity#{}", entity.index);
        let Some(color) = entity_colors.get(&id) else {
            continue;
        };
        let mut next = entity.ref_at(0);
        let mut found = false;
        while let Some(index) = next.and_then(|index| usize::try_from(index).ok()) {
            let Some(attribute) = records_by_index.get(&index) else {
                break;
            };
            if attribute.head.contains("rgb_color") {
                color_records.insert(index, *color);
                found = true;
                break;
            }
            next = attribute.ref_at(0);
        }
        if !found {
            return Err(CodecError::NotImplemented(format!(
                "F3D entity color {id} has no writable rgb_color attribute"
            )));
        }
    }
    for record in records {
        if let Some(color) = color_records.get(&record.index) {
            patch_double_token(bytes, record, 0, f64::from(color.r))?;
            patch_double_token(bytes, record, 1, f64::from(color.g))?;
            patch_double_token(bytes, record, 2, f64::from(color.b))?;
            continue;
        }
        if let Some(transform) = transform_records.get(&record.index) {
            patch_transform_record(bytes, record, *transform, header_scale)?;
            continue;
        }
        let id = format!("f3d:brep:entity#{}", record.index);
        if record.head == "face" {
            if let Some(sense) = face_senses.get(&id) {
                patch_sense_token(bytes, record, 0, *sense)?;
            }
        } else if record.head == "coedge" {
            if let Some(sense) = coedge_senses.get(&id) {
                patch_sense_token(bytes, record, 0, *sense)?;
            }
        } else if record.head == "edge" {
            if let Some(range) = edge_ranges.get(&id) {
                patch_double_token(bytes, record, 0, range[0])?;
                patch_double_token(bytes, record, 1, range[1])?;
            }
        } else if record.head == "point" {
            if let Some(position) = positions.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [position.x / 10.0, position.y / 10.0, position.z / 10.0],
                )?;
            }
        } else if record.head == "straight" {
            if let Some((origin, direction)) = lines.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                )?;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    0,
                    [direction.x, direction.y, direction.z],
                )?;
            }
        } else if record.head == "ellipse" {
            if let Some((center, axis, direction, major_radius, minor_radius)) = conics.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                )?;
                patch_vec3_token(bytes, record, 0x14, 0, [axis.x, axis.y, axis.z])?;
                let major = major_radius / 10.0;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    1,
                    [
                        direction.x * major,
                        direction.y * major,
                        direction.z * major,
                    ],
                )?;
                patch_signed_ratio_token(bytes, record, minor_radius / major_radius)?;
            }
        } else if record.head == "plane" {
            if let Some((origin, normal, u_axis)) = planes.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                )?;
                patch_vec3_token(bytes, record, 0x14, 0, [normal.x, normal.y, normal.z])?;
                patch_vec3_token(bytes, record, 0x14, 1, [u_axis.x, u_axis.y, u_axis.z])?;
            }
        } else if record.head == "sphere" {
            if let Some((center, axis, ref_direction, radius)) = spheres.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                )?;
                patch_double_token(bytes, record, 0, radius / 10.0)?;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    0,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                )?;
                patch_vec3_token(bytes, record, 0x14, 1, [axis.x, axis.y, axis.z])?;
            }
        } else if record.head == "torus" {
            if let Some((center, axis, ref_direction, major_radius, minor_radius)) = tori.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                )?;
                patch_vec3_token(bytes, record, 0x14, 0, [axis.x, axis.y, axis.z])?;
                patch_double_token(bytes, record, 0, major_radius / 10.0)?;
                patch_double_token(bytes, record, 1, minor_radius / 10.0)?;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    1,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                )?;
            }
        } else if record.head == "cone" {
            if let Some((origin, axis, ref_direction, radius)) = cones.get(&id) {
                patch_vec3_token(
                    bytes,
                    record,
                    0x13,
                    0,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                )?;
                patch_vec3_token(bytes, record, 0x14, 0, [axis.x, axis.y, axis.z])?;
                let scaled_radius = radius / 10.0;
                patch_vec3_token(
                    bytes,
                    record,
                    0x14,
                    1,
                    [
                        ref_direction.x * scaled_radius,
                        ref_direction.y * scaled_radius,
                        ref_direction.z * scaled_radius,
                    ],
                )?;
                patch_double_token(bytes, record, 3, scaled_radius)?;
            }
        }
    }
    Ok(())
}

fn patch_double_token(
    bytes: &mut [u8],
    record: &sab::Record,
    ordinal: usize,
    value: f64,
) -> Result<(), CodecError> {
    let offset = *sab::payload_token_offsets(bytes, record, 8, 0x06)
        .map_err(|error| CodecError::Malformed(error.to_string()))?
        .get(ordinal)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "{} record {} lacks double token [{ordinal}]",
                record.head, record.index
            ))
        })?;
    bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn patch_signed_ratio_token(
    bytes: &mut [u8],
    record: &sab::Record,
    magnitude: f64,
) -> Result<(), CodecError> {
    let offset = *sab::payload_token_offsets(bytes, record, 8, 0x06)
        .map_err(|error| CodecError::Malformed(error.to_string()))?
        .first()
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "{} record {} lacks ratio token",
                record.head, record.index
            ))
        })?;
    let raw = f64::from_le_bytes(
        bytes[offset + 1..offset + 9]
            .try_into()
            .expect("framed double token contains eight payload bytes"),
    );
    let signed = if raw.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    };
    bytes[offset + 1..offset + 9].copy_from_slice(&signed.to_le_bytes());
    Ok(())
}

fn patch_transform_record(
    bytes: &mut [u8],
    record: &sab::Record,
    transform: Transform,
    header_scale: f64,
) -> Result<(), CodecError> {
    let mut offsets = sab::payload_token_offsets(bytes, record, 8, 0x13)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    offsets.extend(
        sab::payload_token_offsets(bytes, record, 8, 0x14)
            .map_err(|error| CodecError::Malformed(error.to_string()))?,
    );
    offsets.sort_unstable();
    if offsets.len() != 4 || header_scale == 0.0 {
        return Err(CodecError::Malformed(format!(
            "transform record {} does not contain four vectors or has zero header scale",
            record.index
        )));
    }
    let vectors = [
        [
            transform.rows[0][0],
            transform.rows[1][0],
            transform.rows[2][0],
        ],
        [
            transform.rows[0][1],
            transform.rows[1][1],
            transform.rows[2][1],
        ],
        [
            transform.rows[0][2],
            transform.rows[1][2],
            transform.rows[2][2],
        ],
        [
            transform.rows[0][3] / (header_scale * 10.0),
            transform.rows[1][3] / (header_scale * 10.0),
            transform.rows[2][3] / (header_scale * 10.0),
        ],
    ];
    for (offset, vector) in offsets.into_iter().zip(vectors) {
        for (component, value) in vector.into_iter().enumerate() {
            let at = offset + 1 + component * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    let scale_offsets = sab::payload_token_offsets(bytes, record, 8, 0x06)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    let scale = *scale_offsets.last().ok_or_else(|| {
        CodecError::Malformed(format!("transform record {} lacks scale", record.index))
    })?;
    bytes[scale + 1..scale + 9].copy_from_slice(&transform.rows[3][3].to_le_bytes());
    Ok(())
}

fn patch_sense_token(
    bytes: &mut [u8],
    record: &sab::Record,
    ordinal: usize,
    sense: Sense,
) -> Result<(), CodecError> {
    let mut offsets = sab::payload_token_offsets(bytes, record, 8, 0x0a)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    offsets.extend(
        sab::payload_token_offsets(bytes, record, 8, 0x0b)
            .map_err(|error| CodecError::Malformed(error.to_string()))?,
    );
    offsets.sort_unstable();
    let offset = *offsets.get(ordinal).ok_or_else(|| {
        CodecError::Malformed(format!(
            "{} record {} lacks sense token [{ordinal}]",
            record.head, record.index
        ))
    })?;
    bytes[offset] = match sense {
        Sense::Forward => 0x0b,
        Sense::Reversed => 0x0a,
    };
    Ok(())
}

fn patch_vec3_token(
    bytes: &mut [u8],
    record: &sab::Record,
    tag: u8,
    ordinal: usize,
    values: [f64; 3],
) -> Result<(), CodecError> {
    let offset = *sab::payload_token_offsets(bytes, record, 8, tag)
        .map_err(|error| CodecError::Malformed(error.to_string()))?
        .get(ordinal)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "{} record {} lacks payload token {tag:#04x}[{ordinal}]",
                record.head, record.index,
            ))
        })?;
    for (component, value) in values.into_iter().enumerate() {
        let at = offset + 1 + component * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_straight_record_patches_by_token_boundaries() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x08straight");
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [1.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated straight record");
        let lines = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (Point3::new(40.0, 50.0, 60.0), Vector3::new(0.0, 1.0, 0.0)),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &lines,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated line edit");
        let decoded =
            sab::frame(&bytes, 0, bytes.len(), 8).expect("patched generated straight record");
        assert!(matches!(
            crate::brep::decode_curve(&decoded[0]),
            Some(CurveGeometry::Line { origin, direction })
                if origin == Point3::new(40.0, 50.0, 60.0)
                    && direction == Vector3::new(0.0, 1.0, 0.0)
        ));
    }

    #[test]
    fn generated_signed_sphere_patches_exact_frame_and_radius() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x06sphere");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        for vector in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated sphere record");
        let spheres = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                -25.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &spheres,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated sphere edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched sphere record");
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Sphere { center, axis, ref_direction, radius }, false))
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && radius == -25.0
        ));
    }

    #[test]
    fn generated_torus_preserves_signed_self_intersecting_radii() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x05torus");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [0.0f64, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1.0f64, 0.25] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [1.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated torus record");
        let tori = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                20.0,
                -35.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &tori,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated torus edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched torus record");
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            }, false))
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && major_radius == 20.0
                    && minor_radius == -35.0
        ));
    }

    #[test]
    fn generated_cylinder_preserves_native_angle_branch() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x04cone");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in [1.0f64, 0.0, -1.0, 1.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated cylinder record");
        let cones = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                40.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &cones,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated cylinder edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched cylinder record");
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            }, false))
                if origin == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && radius == 40.0
        ));
    }

    #[test]
    fn generated_ellipse_preserves_negative_ratio_phase() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x07ellipse");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&(-0.5f64).to_le_bytes());
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated ellipse record");
        let conics = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                40.0,
                10.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &conics,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated ellipse edit");
        let ratio_offset =
            sab::payload_token_offsets(&bytes, &records[0], 8, 0x06).expect("ellipse tokens")[0];
        assert_eq!(
            f64::from_le_bytes(
                bytes[ratio_offset + 1..ratio_offset + 9]
                    .try_into()
                    .expect("ratio payload"),
            ),
            -0.25
        );
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched ellipse record");
        assert!(matches!(
            crate::brep::decode_curve(&decoded[0]),
            Some(CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            })
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && major_direction == Vector3::new(1.0, 0.0, 0.0)
                    && major_radius == 40.0
                    && minor_radius == 10.0
        ));
    }
}
