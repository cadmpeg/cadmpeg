// SPDX-License-Identifier: Apache-2.0
//! Bulkstream and sketch/design record encoders for source-less generation.

use std::collections::BTreeMap;

use crate::records::{
    ConstructionRecipeKind, DesignObjectKind, PersistentReferenceKind, SketchCurveGeometry,
    SketchText,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::ids::CoedgeId;
use cadmpeg_ir::math::Point3;

use super::attributes::source_less_body_key;
use super::bytes::{native_f64, native_i64, native_ref};
use super::native_geometry::native_nurbs_curve;
use super::validate::DesignBindingsValidated;
use crate::nurbs::reader::LEN_TO_MM;
use crate::writer::primitives::{f3d_native, native_bool};

pub(crate) fn tolerant_coedge_range(
    target: &CadIr,
    coedge: &CoedgeId,
) -> Result<Option<[f64; 2]>, CodecError> {
    Ok(f3d_native(target)?.and_then(|native| {
        native
            .tolerant_coedge_parameters
            .into_iter()
            .find(|parameters| parameters.coedge == *coedge)
            .map(|parameters| parameters.parameter_range)
    }))
}

pub(crate) fn native_tolerant_coedge_extension(
    records: &mut Vec<u8>,
    target: &CadIr,
    coedge: &CoedgeId,
) -> Result<(), CodecError> {
    let extension = f3d_native(target)?
        .and_then(|native| {
            native
                .tolerant_coedge_parameters
                .into_iter()
                .find(|parameters| parameters.coedge == *coedge)
        })
        .map(|parameters| parameters.extension)
        .unwrap_or_default();
    match extension {
        crate::records::TolerantCoedgeExtension::None
        | crate::records::TolerantCoedgeExtension::Empty { target: None } => {
            native_ref(records, -1);
            native_i64(records, 0);
            native_i64(records, 0);
            Ok(())
        }
        crate::records::TolerantCoedgeExtension::EmbeddedCurve {
            target: None,
            curve_reversed,
            parameter_range,
            ..
        } => {
            let model_coedge = target
                .model
                .coedges
                .iter()
                .find(|candidate| candidate.id == *coedge)
                .ok_or_else(|| CodecError::Malformed(format!("missing coedge {coedge}")))?;
            let curve_id = model_coedge.use_curve.as_ref().ok_or_else(|| {
                CodecError::Malformed(format!("tolerant coedge {coedge} has no use curve"))
            })?;
            let curve = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .ok_or_else(|| CodecError::Malformed(format!("missing use curve {curve_id}")))?;
            let CurveGeometry::Nurbs(curve) = &curve.geometry else {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D tolerant coedge {coedge} requires a NURBS use curve"
                )));
            };
            let mut native_curve = curve.clone();
            if curve_reversed {
                crate::brep::reverse_nurbs_curve(&mut native_curve);
            }
            native_ref(records, -1);
            native_i64(records, 1);
            records.push(native_bool(curve_reversed));
            records.push(0x0f);
            native_nurbs_curve(records, &native_curve)?;
            records.push(0x10);
            if let Some([start, end]) = parameter_range {
                records.push(0x0a);
                native_f64(records, start);
                records.push(0x0a);
                native_f64(records, end);
            } else {
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            native_i64(records, 0);
            Ok(())
        }
        _ => Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize nonempty tolerant-coedge extension for {coedge}"
        ))),
    }
}

pub(crate) fn encode_act_bulkstream(target: &CadIr) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(native) = f3d_native(target)? else {
        return Ok(None);
    };
    if native.act_entities.is_empty()
        && native.act_guids.is_empty()
        && native.act_root_components.is_empty()
    {
        return Ok(None);
    }

    let mut out = Vec::new();
    native_lp_ascii(&mut out, "ACTTable")?;
    out.extend_from_slice(&0u16.to_le_bytes());
    let table_entities = native
        .act_entities
        .iter()
        .filter(|entity| entity.in_table)
        .collect::<Vec<_>>();
    let count = u32::try_from(table_entities.len())
        .map_err(|_| CodecError::Malformed("ACT table exceeds u32::MAX entities".into()))?;
    out.extend_from_slice(&count.to_le_bytes());
    for entity in table_entities {
        out.push(1);
        out.extend_from_slice(&entity.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        native_lp_utf16(&mut out, &entity.entity_id)?;
    }

    let channel_guids = native
        .act_entities
        .iter()
        .flat_map(|entity| entity.channels.values())
        .collect::<Vec<_>>();
    let mut emitted_channel_guids = BTreeMap::<&str, usize>::new();
    for guid in &channel_guids {
        *emitted_channel_guids.entry(guid.as_str()).or_default() += 1;
    }
    for guid in &native.act_guids {
        validate_guid(&guid.guid, "ACT GUID")?;
        let remaining = emitted_channel_guids.entry(guid.guid.as_str()).or_default();
        if *remaining > 0 {
            *remaining -= 1;
        } else {
            native_lp_utf16(&mut out, &guid.guid)?;
        }
    }
    for entity in native
        .act_entities
        .iter()
        .filter(|entity| !entity.channels.is_empty())
    {
        let class_tag = entity.channel_class_tag.as_deref().ok_or_else(|| {
            CodecError::Malformed("ACT channel entity lacks a dynamic class tag".into())
        })?;
        validate_dynamic_class_tag(class_tag, "ACT channel entity")?;
        native_lp_ascii(&mut out, class_tag)?;
        out.extend_from_slice(&entity.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        let channel_count = u32::try_from(entity.channels.len())
            .map_err(|_| CodecError::Malformed("ACT entity exceeds u32::MAX channels".into()))?;
        if !(1..=8).contains(&channel_count) {
            return Err(CodecError::NotImplemented(
                "source-less ACT channel groups require one to eight channels".into(),
            ));
        }
        out.extend_from_slice(&channel_count.to_le_bytes());
        for (name, guid) in &entity.channels {
            validate_guid(guid, "ACT channel GUID")?;
            native_lp_ascii(&mut out, name)?;
            native_lp_utf16(&mut out, guid)?;
        }
        native_lp_utf16(&mut out, &entity.entity_id)?;
    }
    for root in &native.act_root_components {
        validate_dynamic_class_tag(&root.class_tag, "ACT root component")?;
        native_lp_ascii(&mut out, &root.class_tag)?;
        out.extend_from_slice(&root.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        out.push(1);
        out.extend_from_slice(&root.instance_root_record.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        native_lp_utf16(&mut out, &root.entity_id)?;
        out.push(1);
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&[0; 5]);
        out.push(1);
        out.extend_from_slice(&root.registry_flag.to_le_bytes());
        native_lp_utf16(&mut out, &root.display_name)?;
        out.push(0);
        out.push(1);
        out.extend_from_slice(&root.components_root_record.to_le_bytes());
    }
    Ok(Some(out))
}

pub(crate) fn encode_design_bulkstream(target: &CadIr) -> Result<Option<Vec<u8>>, CodecError> {
    let native = f3d_native(target)?.unwrap_or_default();
    let (_, projected_parameters) =
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            &crate::design::feature_project::ProjectInputs {
                native: &native.design_parameters,
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                construction_groups: &native.design_construction_operand_groups,
                fillet_radius_groups: &native.design_fillet_radius_groups,
                edge_operands: &native.design_edge_operands,
                edge_identity_operands: &native.design_edge_identity_operands,
                face_operands: &native.design_face_operands,
                placements: &native.design_sketch_placements,
                body_bindings: &native.design_body_bindings,
            },
        );
    if target.model.parameters != projected_parameters {
        return Err(CodecError::Malformed(
            "neutral F3D parameters must equal the projection of native Design parameters".into(),
        ));
    }
    if !native.design_parameter_companions.is_empty()
        || !native.design_dimension_annotation_frames.is_empty()
        || !native.design_dimension_locus_pairs.is_empty()
        || !native.design_dimension_locus_groups.is_empty()
        || !native.design_dimension_null_locus_pairs.is_empty()
        || !native.design_dimension_recipe_records.is_empty()
        || !native.design_parameter_owners.is_empty()
        || !native.design_parameter_scopes.is_empty()
        || !native.design_sketch_placements.is_empty()
        || native
            .design_entity_headers
            .iter()
            .any(|header| !header.member_indices.is_empty())
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D Design parameter records are not writable".into(),
        ));
    }
    let has_body_visibility = target
        .model
        .bodies
        .iter()
        .any(|body| body.visible.is_some());
    if native.design_parameters.is_empty()
        && native.construction_recipes.is_empty()
        && native.persistent_references.is_empty()
        && native.lost_edge_references.is_empty()
        && native.design_body_members.is_empty()
        && native.design_entity_headers.is_empty()
        && native.design_record_headers.is_empty()
        && native.design_material_assignments.is_empty()
        && native.sketch_points.is_empty()
        && native.sketch_curve_identities.is_empty()
        && native.sketch_relations.is_empty()
        && native.sketch_texts.is_empty()
        && !has_body_visibility
    {
        return Ok(None);
    }

    let mut out = Vec::new();
    for parameter in &native.design_parameters {
        encode_document_parameter(&mut out, parameter)?;
    }
    let mut body_map = native
        .design_material_assignments
        .iter()
        .map(|assignment| (assignment.asm_body_key, assignment.entity_suffix))
        .collect::<BTreeMap<_, _>>();
    let mut visibility_rows = Vec::new();
    for (ordinal, body) in target.model.bodies.iter().enumerate() {
        let Some(visible) = body.visible else {
            continue;
        };
        let metadata = native
            .body_visibilities
            .iter()
            .find(|metadata| metadata.body == body.id);
        let asm_body_key = match metadata {
            Some(metadata) => metadata.asm_body_key,
            None => u64::try_from(source_less_body_key(target, body, ordinal)?).map_err(|_| {
                CodecError::Malformed("source-less ASM body key is negative".into())
            })?,
        };
        let entity_suffix = metadata
            .map(|metadata| metadata.entity_suffix)
            .or_else(|| body_map.get(&asm_body_key).copied())
            .unwrap_or(asm_body_key);
        body_map.insert(asm_body_key, entity_suffix);
        visibility_rows.push((entity_suffix, visible));
    }
    if !body_map.is_empty() {
        let count = u32::try_from(body_map.len())
            .map_err(|_| CodecError::Malformed("Design body map exceeds u32::MAX".into()))?;
        out.extend_from_slice(&count.to_le_bytes());
        for (body_key, entity_suffix) in body_map {
            out.extend_from_slice(&body_key.to_le_bytes());
            out.extend_from_slice(&entity_suffix.to_le_bytes());
        }
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        native_lp_utf16(&mut out, "BREP.generated.smbh")?;
    }
    for assignment in &native.design_material_assignments {
        native_lp_utf16(&mut out, &assignment.entity_id)?;
        native_lp_utf16(
            &mut out,
            assignment
                .physical_token
                .as_deref()
                .expect("validated source-less material token"),
        )?;
        native_lp_utf16(&mut out, "Body")?;
        native_lp_utf16(&mut out, "00000000-0000-0000-0000-000000000000")?;
        native_lp_utf16(&mut out, &assignment.visual_guid)?;
        native_lp_utf16(&mut out, "BA5EE55E-9982-449B-9D66-9F036540E140")?;
        if let Some(visual_preset) = &assignment.visual_preset {
            native_lp_utf16(&mut out, visual_preset)?;
        }
    }
    for (ordinal, (entity_suffix, visible)) in visibility_rows.into_iter().enumerate() {
        native_lp_utf16(
            &mut out,
            &format!("00000000-0000-0000-0000-{:012X}", ordinal + 1),
        )?;
        out.push(u8::from(!visible));
        out.extend_from_slice(&[0x01, 0x01]);
        out.extend_from_slice(&entity_suffix.to_le_bytes());
    }
    for recipe in &native.construction_recipes {
        let name = construction_recipe_name(recipe.kind);
        let mut prefix = [0u8; 27];
        if let Some(design_id) = &recipe.design_id {
            if design_id.len() != 3 || !design_id.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(CodecError::Malformed(format!(
                    "source-less Design recipe id must be three ASCII digits: {design_id}"
                )));
            }
            prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
            prefix[4..7].copy_from_slice(design_id.as_bytes());
        }
        prefix[11..15].copy_from_slice(&recipe.record_index.to_le_bytes());
        prefix[23..27].copy_from_slice(
            &u32::try_from(name.len())
                .map_err(|_| CodecError::Malformed("Design recipe name exceeds u32::MAX".into()))?
                .to_le_bytes(),
        );
        out.extend_from_slice(&prefix);
        out.extend_from_slice(name);
        out.extend_from_slice(&(-1i64).to_le_bytes());
        for value in [2i32, 0, -1, 1, -1] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    if !native.design_body_members.is_empty() {
        native_lp_ascii(&mut out, "BodiesRoot")?;
        out.extend_from_slice(&0u16.to_le_bytes());
        native_lp_ascii(&mut out, "BodiesRoot")?;
        let count = u32::try_from(native.design_body_members.len()).map_err(|_| {
            CodecError::Malformed("Design BodiesRoot exceeds u32::MAX members".into())
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for member in &native.design_body_members {
            out.push(1);
            out.extend_from_slice(&member.entity_suffix.to_le_bytes());
            out.extend_from_slice(&member.flags.to_le_bytes());
        }
        out.push(0);
    }
    for header in &native.design_entity_headers {
        validate_dynamic_class_tag(&header.class_tag, "Design entity header")?;
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(header.class_tag.as_bytes());
        out.extend_from_slice(&header.entity_suffix.to_le_bytes());
        out.extend_from_slice(&[0; 5]);
        out.push(u8::from(header.optional_slot_present));
        if header.optional_slot_present {
            out.extend_from_slice(&[0; 4]);
        }
        native_lp_utf16(&mut out, &header.entity_id)?;
        if header.object_kind == Some(DesignObjectKind::Sketch) {
            let count = u32::try_from(header.reference_indices.len()).map_err(|_| {
                CodecError::Malformed("Design sketch header exceeds u32::MAX references".into())
            })?;
            match header.record_reference {
                Some(record_reference) => {
                    out.extend_from_slice(&record_reference.to_le_bytes());
                    out.extend_from_slice(&[0; 4]);
                }
                // The sentinel base-record slot of a sketch with no base record.
                None => out.extend_from_slice(&[0xFF; 8]),
            }
            out.push(1);
            out.extend_from_slice(&count.to_le_bytes());
            for reference in &header.reference_indices {
                out.push(1);
                out.extend_from_slice(&reference.to_le_bytes());
                out.extend_from_slice(&[0; 6]);
            }
        }
    }
    for header in &native.design_record_headers {
        validate_dynamic_class_tag(&header.class_tag, "Design record header")?;
        native_lp_ascii(&mut out, &header.class_tag)?;
        out.extend_from_slice(&header.record_index.to_le_bytes());
    }
    for point in &native.sketch_points {
        encode_sketch_point(&mut out, point)?;
    }
    for curve in &native.sketch_curve_identities {
        encode_sketch_curve_identity(&mut out, curve)?;
    }
    for text in &native.sketch_texts {
        encode_sketch_text(&mut out, text)?;
    }
    for relation in &native.sketch_relations {
        encode_sketch_relation(&mut out, relation)?;
    }
    for reference in &native.persistent_references {
        out.extend_from_slice(persistent_reference_name(reference.kind));
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&14u32.to_le_bytes());
        out.extend_from_slice(&[0; 14]);
        out.extend_from_slice(&23u32.to_le_bytes());
        out.extend_from_slice(b"IntrinsicMetaTypeuint64");
        out.extend_from_slice(&reference.value.to_le_bytes());
    }
    for (ordinal, reference) in native.lost_edge_references.iter().enumerate() {
        validate_dynamic_class_tag(&reference.class_tag, "lost-edge reference")?;
        validate_dynamic_class_tag(&reference.next_class_tag, "lost-edge next record")?;
        if let Some(previous) = ordinal
            .checked_sub(1)
            .and_then(|ordinal| native.lost_edge_references.get(ordinal))
        {
            if previous.next_class_tag != reference.class_tag
                || previous.next_record_index != reference.record_index
            {
                return Err(CodecError::Malformed(format!(
                    "F3D lost-edge record {} does not continue the preceding indexed run",
                    reference.id
                )));
            }
        } else {
            native_lp_ascii(&mut out, &reference.class_tag)?;
            out.extend_from_slice(&reference.record_index.to_le_bytes());
        }
        out.extend_from_slice(&[0; 14]);
        out.extend_from_slice(&19u32.to_le_bytes());
        out.extend_from_slice(b"EDGE_REFERENCE_LOST");
        native_lp_ascii(&mut out, &reference.next_class_tag)?;
        out.extend_from_slice(&reference.next_record_index.to_le_bytes());
    }
    Ok(Some(out))
}

fn encode_document_parameter(
    out: &mut Vec<u8>,
    parameter: &crate::records::DesignParameter,
) -> Result<(), CodecError> {
    validate_dynamic_class_tag(&parameter.class_tag, "Design parameter")?;
    native_lp_ascii(out, &parameter.class_tag)?;
    out.extend_from_slice(&parameter.record_index.to_le_bytes());
    out.extend_from_slice(&[0; 11]);
    out.extend_from_slice(&parameter.prefix_value.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&parameter.source_ordinal.to_le_bytes());
    out.push(0);
    native_lp_utf16(out, &parameter.expression)?;
    out.extend_from_slice(&[0; 8]);
    out.push(1);
    native_lp_utf16(out, &parameter.source_kind)?;
    out.extend_from_slice(&0u32.to_le_bytes());
    if let Some(unit) = &parameter.unit {
        native_lp_utf16(out, unit)?;
    } else {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    native_lp_utf16(out, &parameter.name)?;
    out.extend_from_slice(&parameter.evaluated_value.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    Ok(())
}

fn encode_sketch_record_header(
    out: &mut [u8],
    class_tag: &str,
    record_index: u32,
) -> Result<(), CodecError> {
    validate_dynamic_class_tag(class_tag, "sketch record")?;
    out[0..4].copy_from_slice(&3u32.to_le_bytes());
    out[4..7].copy_from_slice(class_tag.as_bytes());
    out[7..11].copy_from_slice(&record_index.to_le_bytes());
    Ok(())
}

fn encode_sketch_point(
    out: &mut Vec<u8>,
    point: &crate::records::SketchPoint,
) -> Result<(), CodecError> {
    if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
        return Err(CodecError::Malformed(
            "source-less sketch point coordinates must be finite".into(),
        ));
    }
    let shift = usize::from(point.entity_genesis.is_some()) * 52;
    let mut record = vec![0u8; 112 + shift];
    encode_sketch_record_header(&mut record, &point.class_tag, point.record_index)?;
    record[20] = 1;
    record[21..25].copy_from_slice(&(1 + u32::from(point.entity_genesis.is_some())).to_le_bytes());
    if let Some(entity_genesis) = point.entity_genesis {
        encode_entity_genesis(&mut record, entity_genesis);
    }
    record[25 + shift..29 + shift].copy_from_slice(&6u32.to_le_bytes());
    record[29 + shift..35 + shift].copy_from_slice(b"pt_tag");
    record[35 + shift..39 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[39 + shift..62 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[62 + shift..70 + shift].copy_from_slice(&point.persistent_id.to_le_bytes());
    record[70 + shift] = 1;
    record[71 + shift..75 + shift].copy_from_slice(&point.paired_reference.to_le_bytes());
    record[89 + shift..97 + shift]
        .copy_from_slice(&(point.coordinates.u / LEN_TO_MM).to_le_bytes());
    record[97 + shift..105 + shift]
        .copy_from_slice(&(point.coordinates.v / LEN_TO_MM).to_le_bytes());
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_sketch_curve_identity(
    out: &mut Vec<u8>,
    curve: &crate::records::SketchCurveIdentity,
) -> Result<(), CodecError> {
    let shift = usize::from(curve.entity_genesis.is_some()) * 52;
    let mut record = vec![0u8; 133 + shift];
    encode_sketch_record_header(&mut record, &curve.class_tag, curve.record_index)?;
    record[20] = 1;
    record[21..25].copy_from_slice(&(2 + u32::from(curve.entity_genesis.is_some())).to_le_bytes());
    if let Some(entity_genesis) = curve.entity_genesis {
        encode_entity_genesis(&mut record, entity_genesis);
    }
    record[25 + shift..29 + shift].copy_from_slice(&14u32.to_le_bytes());
    record[29 + shift..43 + shift].copy_from_slice(b"crv_primary_id");
    record[43 + shift..47 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[47 + shift..70 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[70 + shift..78 + shift].copy_from_slice(&curve.primary_id.to_le_bytes());
    record[78 + shift..82 + shift].copy_from_slice(&16u32.to_le_bytes());
    record[82 + shift..98 + shift].copy_from_slice(b"crv_secondary_id");
    record[98 + shift..102 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[102 + shift..125 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[125 + shift..133 + shift].copy_from_slice(&curve.secondary_id.to_le_bytes());
    match curve.geometry.as_ref() {
        Some(SketchCurveGeometry::Line {
            start,
            end,
            direction,
            normal,
        }) => {
            let values = [
                start.x / LEN_TO_MM,
                start.y / LEN_TO_MM,
                start.z / LEN_TO_MM,
                (end.x - start.x) / LEN_TO_MM,
                (end.y - start.y) / LEN_TO_MM,
                (end.z - start.z) / LEN_TO_MM,
                direction.x,
                direction.y,
                direction.z,
                normal.x,
                normal.y,
                normal.z,
            ];
            encode_f64_sequence(&mut record, &values)?;
        }
        Some(SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        }) => {
            let values = [
                center.x / LEN_TO_MM,
                center.y / LEN_TO_MM,
                center.z / LEN_TO_MM,
                normal.x,
                normal.y,
                normal.z,
                reference_direction.x,
                reference_direction.y,
                reference_direction.z,
                radius / LEN_TO_MM,
                *start_angle,
                *end_angle,
            ];
            encode_f64_sequence(&mut record, &values)?;
        }
        Some(SketchCurveGeometry::Nurbs {
            carrier_reference,
            subtype_class_tag,
            subtype_record_index,
            degree,
            fit_tolerance,
            scalar_width,
            knots,
            weights,
            control_points,
        }) => encode_sketch_nurbs(
            &mut record,
            *carrier_reference,
            subtype_class_tag,
            *subtype_record_index,
            *degree,
            *fit_tolerance,
            *scalar_width,
            knots,
            weights,
            control_points,
        )?,
        None => {}
    }
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_entity_genesis(record: &mut [u8], entity_genesis: u64) {
    record[25..29].copy_from_slice(&13u32.to_le_bytes());
    record[29..42].copy_from_slice(b"EntityGenesis");
    record[42..46].copy_from_slice(&23u32.to_le_bytes());
    record[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[69..77].copy_from_slice(&entity_genesis.to_le_bytes());
}

fn encode_f64_sequence(out: &mut Vec<u8>, values: &[f64]) -> Result<(), CodecError> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(CodecError::Malformed(
            "source-less sketch geometry must contain finite scalars".into(),
        ));
    }
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_sketch_nurbs(
    record: &mut Vec<u8>,
    carrier_reference: Option<u64>,
    subtype_class_tag: &str,
    subtype_record_index: u32,
    degree: u32,
    fit_tolerance: f64,
    scalar_width: u32,
    knots: &[f64],
    weights: &[f64],
    control_points: &[Point3],
) -> Result<(), CodecError> {
    validate_dynamic_class_tag(subtype_class_tag, "sketch NURBS subtype")?;
    if scalar_width != 8 || (!weights.is_empty() && weights.len() != control_points.len()) {
        return Err(CodecError::Malformed(
            "source-less sketch NURBS requires scalar width 8 and parallel weights".into(),
        ));
    }
    let expected_knots = control_points
        .len()
        .checked_add(usize::try_from(degree).unwrap_or(usize::MAX))
        .and_then(|count| count.checked_add(1));
    if expected_knots != Some(knots.len()) {
        return Err(CodecError::Malformed(
            "source-less sketch NURBS knot count must equal control points + degree + 1".into(),
        ));
    }
    record.extend_from_slice(&carrier_reference.unwrap_or(u64::MAX).to_le_bytes());
    record.extend_from_slice(&3u32.to_le_bytes());
    record.extend_from_slice(subtype_class_tag.as_bytes());
    record.extend_from_slice(&subtype_record_index.to_le_bytes());
    record.resize(133 + 88, 0);
    record.push(1);
    record.push(0);
    record.extend_from_slice(&degree.to_le_bytes());
    record.extend_from_slice(&(fit_tolerance / LEN_TO_MM).to_le_bytes());
    let knot_count = u32::try_from(knots.len())
        .map_err(|_| CodecError::Malformed("sketch NURBS has too many knots".into()))?;
    record.extend_from_slice(&knot_count.to_le_bytes());
    record.extend_from_slice(&knot_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    encode_f64_sequence(record, knots)?;
    let weight_count = u32::try_from(weights.len())
        .map_err(|_| CodecError::Malformed("sketch NURBS has too many weights".into()))?;
    record.extend_from_slice(&weight_count.to_le_bytes());
    record.extend_from_slice(&weight_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    encode_f64_sequence(record, weights)?;
    let point_count = u32::try_from(control_points.len())
        .map_err(|_| CodecError::Malformed("sketch NURBS has too many control points".into()))?;
    record.extend_from_slice(&point_count.to_le_bytes());
    record.extend_from_slice(&point_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    let coordinates = control_points
        .iter()
        .flat_map(|point| {
            [
                point.x / LEN_TO_MM,
                point.y / LEN_TO_MM,
                point.z / LEN_TO_MM,
            ]
        })
        .collect::<Vec<_>>();
    encode_f64_sequence(record, &coordinates)
}

fn encode_sketch_text(out: &mut Vec<u8>, text: &SketchText) -> Result<(), CodecError> {
    validate_dynamic_class_tag(&text.class_tag, "sketch text")?;
    let decoded = crate::design::decode::sketch::decode_sketch_text_record(
        &text.raw_bytes,
        "Design/BulkStream.dat",
        text.class_tag.clone(),
        text.record_index,
        0,
    )
    .ok_or_else(|| CodecError::Malformed(format!("invalid raw sketch-text record {}", text.id)))?;
    let header_matches = text.raw_bytes.get(0..4) == Some(&3u32.to_le_bytes())
        && text.raw_bytes.get(4..7) == Some(text.class_tag.as_bytes())
        && text.raw_bytes.get(7..15) == Some(&u64::from(text.record_index).to_le_bytes());
    let fields_match = decoded.owner_reference == text.owner_reference
        && decoded.entity_genesis == text.entity_genesis
        && decoded.persistent_id == text.persistent_id
        && decoded.base_id == text.base_id
        && decoded.text == text.text
        && decoded.font_family == text.font_family
        && decoded.height == text.height
        && decoded.width_factor == text.width_factor
        && decoded.first_reference == text.first_reference
        && decoded.second_reference == text.second_reference;
    if !header_matches || !fields_match {
        return Err(CodecError::Malformed(format!(
            "sketch-text record {} fields disagree with its raw bytes",
            text.id
        )));
    }
    out.extend_from_slice(&text.raw_bytes);
    Ok(())
}

fn encode_sketch_relation(
    out: &mut Vec<u8>,
    relation: &crate::records::SketchRelation,
) -> Result<(), CodecError> {
    let (constraint_kinds, unknown_constraint_bits) =
        crate::design::decode::sketch::decode_constraint_kinds(relation.state);
    if constraint_kinds != relation.constraint_kinds
        || unknown_constraint_bits != relation.unknown_constraint_bits
    {
        return Err(CodecError::Malformed(format!(
            "F3D sketch relation {} has a mask inconsistent with its typed constraint kinds",
            relation.id
        )));
    }
    let reference_count = relation
        .members
        .len()
        .checked_add(relation.auxiliary_references.len())
        .and_then(|count| count.checked_add(1))
        .and_then(|count| count.checked_add(relation.return_members.len()))
        .ok_or_else(|| CodecError::Malformed("sketch relation reference count overflow".into()))?;
    // A u64 mask needs the `EntityGenesis` block that signals the u64 dialect.
    let entity_genesis = if u32::try_from(relation.state).is_err() {
        Some(relation.entity_genesis.unwrap_or(0))
    } else {
        relation.entity_genesis
    };
    // Marker + u32 1, the two length-prefixed key strings, and the u64 value.
    let genesis_len = entity_genesis.map_or(0usize, |_| 5 + 17 + 27 + 8);
    // 24-byte prefix, the 5-byte marked-u32 or 14-byte padded-u64 mask, and
    // the 4-byte return count, plus five bytes per reference.
    let state_len = if u32::try_from(relation.state).is_ok() {
        5usize
    } else {
        14
    };
    let required_len = 28usize
        .checked_add(state_len)
        .and_then(|len| len.checked_add(genesis_len))
        .and_then(|len| len.checked_add(reference_count.checked_mul(5)?))
        .ok_or_else(|| CodecError::Malformed("sketch relation byte length overflow".into()))?;
    let mut record = vec![0u8; required_len.max(101)];
    encode_sketch_record_header(&mut record, &relation.class_tag, relation.record_index)?;
    record[19] = 1;
    let member_count = u32::try_from(relation.members.len())
        .map_err(|_| CodecError::Malformed("sketch relation has too many members".into()))?;
    record[20..24].copy_from_slice(&member_count.to_le_bytes());
    let mut cursor = 24usize;
    for reference in &relation.members {
        write_marked_u32(&mut record, &mut cursor, *reference)?;
    }
    if let Some(genesis) = entity_genesis {
        let end = cursor.checked_add(genesis_len).ok_or_else(|| {
            CodecError::Malformed("sketch relation record offset overflow".into())
        })?;
        let block = record.get_mut(cursor..end).ok_or_else(|| {
            CodecError::Malformed("sketch relation exceeds its planned record length".into())
        })?;
        block[0] = 1;
        block[1..5].copy_from_slice(&1u32.to_le_bytes());
        block[5..9].copy_from_slice(&13u32.to_le_bytes());
        block[9..22].copy_from_slice(b"EntityGenesis");
        block[22..26].copy_from_slice(&23u32.to_le_bytes());
        block[26..49].copy_from_slice(b"IntrinsicMetaTypeuint64");
        block[49..57].copy_from_slice(&genesis.to_le_bytes());
        cursor = end;
    }
    for reference in relation
        .auxiliary_references
        .iter()
        .chain(std::iter::once(&relation.owner_reference))
    {
        write_marked_u32(&mut record, &mut cursor, *reference)?;
    }
    if let Ok(state) = u32::try_from(relation.state) {
        write_marked_u32(&mut record, &mut cursor, state)?;
    } else {
        // Six zero bytes (already present) then the unmarked u64 mask.
        cursor = cursor.checked_add(6).ok_or_else(|| {
            CodecError::Malformed("sketch relation record offset overflow".into())
        })?;
        let end = cursor.checked_add(8).ok_or_else(|| {
            CodecError::Malformed("sketch relation record offset overflow".into())
        })?;
        record
            .get_mut(cursor..end)
            .ok_or_else(|| {
                CodecError::Malformed("sketch relation exceeds its planned record length".into())
            })?
            .copy_from_slice(&relation.state.to_le_bytes());
        cursor = end;
    }
    let return_count = u32::try_from(relation.return_members.len())
        .map_err(|_| CodecError::Malformed("sketch relation has too many return members".into()))?;
    write_u32(&mut record, &mut cursor, return_count)?;
    for reference in &relation.return_members {
        write_marked_u32(&mut record, &mut cursor, *reference)?;
    }
    out.extend_from_slice(&record);
    Ok(())
}

fn write_marked_u32(out: &mut [u8], cursor: &mut usize, value: u32) -> Result<(), CodecError> {
    let end = cursor
        .checked_add(5)
        .ok_or_else(|| CodecError::Malformed("sketch relation record offset overflow".into()))?;
    let target = out.get_mut(*cursor..end).ok_or_else(|| {
        CodecError::Malformed("sketch relation exceeds its planned record length".into())
    })?;
    target[0] = 1;
    target[1..5].copy_from_slice(&value.to_le_bytes());
    *cursor = end;
    Ok(())
}

fn write_u32(out: &mut [u8], cursor: &mut usize, value: u32) -> Result<(), CodecError> {
    let end = cursor
        .checked_add(4)
        .ok_or_else(|| CodecError::Malformed("sketch relation record offset overflow".into()))?;
    out.get_mut(*cursor..end)
        .ok_or_else(|| {
            CodecError::NotImplemented("sketch relation does not fit canonical record".into())
        })?
        .copy_from_slice(&value.to_le_bytes());
    *cursor = end;
    Ok(())
}

fn construction_recipe_name(kind: ConstructionRecipeKind) -> &'static [u8] {
    match kind {
        ConstructionRecipeKind::Body => b"body_recipe_data",
        ConstructionRecipeKind::Face => b"face_recipe_data",
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data",
        ConstructionRecipeKind::Edge => b"edge_recipe_data",
        ConstructionRecipeKind::Vertex => b"vertex_recipe_data",
    }
}

fn persistent_reference_name(kind: PersistentReferenceKind) -> &'static [u8] {
    match kind {
        PersistentReferenceKind::Point => b"pt_tag",
        PersistentReferenceKind::CurvePrimary => b"crv_primary_id",
        PersistentReferenceKind::CurveSecondary => b"crv_secondary_id",
    }
}

pub(crate) fn validate_dynamic_class_tag(value: &str, field: &str) -> Result<(), CodecError> {
    if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "{field} class tag must be three ASCII digits: {value}"
        )))
    }
}

pub(crate) fn encode_design_metastream(
    bindings: DesignBindingsValidated<'_>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let native = bindings.native();
    if native.design_objects.is_empty() {
        return Ok(None);
    }

    let mut out = Vec::new();
    for object in &native.design_objects {
        let kind_name = design_object_kind_name(&object.kind);
        if kind_name.is_empty() || crate::bytes::is_guid_relaxed(kind_name) {
            return Err(CodecError::Malformed(format!(
                "Design object class is empty or GUID-shaped: {kind_name}"
            )));
        }
        native_lp_ascii(&mut out, kind_name)?;
        let count = u32::try_from(object.entity_ids.len()).map_err(|_| {
            CodecError::Malformed("Design object owns more than u32::MAX entities".into())
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for entity_id in &object.entity_ids {
            out.extend_from_slice(&entity_id.to_le_bytes());
        }
        validate_guid(&object.self_guid, "Design object self GUID")?;
        native_lp_ascii(&mut out, &object.self_guid)?;
        let zero_run_length = usize::try_from(object.zero_run_length).map_err(|_| {
            CodecError::Malformed("Design object zero-run length exceeds address space".into())
        })?;
        out.resize(
            out.len().checked_add(zero_run_length).ok_or_else(|| {
                CodecError::Malformed("Design MetaStream zero run exceeds address space".into())
            })?,
            0,
        );
        if let Some(parent_guid) = &object.parent_guid {
            validate_guid(parent_guid, "Design object parent GUID")?;
            native_lp_ascii(&mut out, parent_guid)?;
        }
        out.extend_from_slice(&object.revision.to_le_bytes());
    }
    Ok(Some(out))
}

fn design_object_kind_name(kind: &DesignObjectKind) -> &str {
    match kind {
        DesignObjectKind::Fusion => "Fusion",
        DesignObjectKind::Body => "Body",
        DesignObjectKind::Component => "Component",
        DesignObjectKind::Geometry => "Geometry",
        DesignObjectKind::Sketch => "MSketch",
        DesignObjectKind::Dimension => "Dimension",
        DesignObjectKind::Scene => "Scene",
        DesignObjectKind::EntityTracking => "EntityTracking",
        DesignObjectKind::CommonData => "CommonData",
        DesignObjectKind::Other(name) => name,
    }
}

fn native_lp_ascii(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(CodecError::Malformed(
            "Design MetaStream strings must contain printable ASCII".into(),
        ));
    }
    let length = u32::try_from(value.len())
        .map_err(|_| CodecError::Malformed("Design MetaStream string exceeds u32::MAX".into()))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn native_lp_utf16(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    let length = u32::try_from(units.len())
        .map_err(|_| CodecError::Malformed("Design UTF-16 string exceeds u32::MAX".into()))?;
    out.extend_from_slice(&length.to_le_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(())
}

fn validate_guid(value: &str, field: &str) -> Result<(), CodecError> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes.get(index) == Some(&b'-'))
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "{field} is not a canonical GUID: {value}"
        )))
    }
}
