// SPDX-License-Identifier: Apache-2.0
//! Bulkstream and sketch/design record encoders for source-less generation.

use crate::records::{
    ConstructionRecipeKind, PersistentReferenceKind, SketchCurveGeometry,
    SketchPointCompanionReferenceEncoding, SketchPointRecordForm, SketchText,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::ids::CoedgeId;
use cadmpeg_ir::math::Point3;

use super::index::NativeGenerationIndex;
use super::native_bytes::{native_f64, native_i64, native_ref};
use super::native_geometry::native_nurbs_curve;
use super::presentation::GeneratedDesignRegistry;
use crate::native::F3dNative;
use crate::writer::primitives::native_bool;
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;

/// Generated Design `BulkStream` and the primary offsets known while writing it.
pub(crate) struct EncodedDesignBulkStream {
    pub(crate) bytes: Vec<u8>,
    pub(crate) primary_records: Vec<crate::metastream::RecordIndexEntry>,
}

pub(crate) fn tolerant_coedge_range(
    index: &NativeGenerationIndex<'_>,
    coedge: &CoedgeId,
) -> Option<[f64; 2]> {
    index
        .tolerant_coedges
        .get(coedge.as_str())
        .map(|parameters| parameters.parameter_range)
}

pub(crate) fn native_tolerant_coedge_extension(
    records: &mut Vec<u8>,
    target: &CadIr,
    index: &NativeGenerationIndex<'_>,
    coedge: &CoedgeId,
) -> Result<(), CodecError> {
    let extension = index
        .tolerant_coedges
        .get(coedge.as_str())
        .map(|parameters| &parameters.extension);
    match extension {
        None
        | Some(
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::None
            | cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: None },
        ) => {
            native_ref(records, -1);
            native_i64(records, 0);
            native_i64(records, 0);
            Ok(())
        }
        Some(cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
            target: None,
            curve_reversed,
            parameter_range,
            ..
        }) => {
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
            if *curve_reversed {
                cadmpeg_asm::brep::geometry::reverse_nurbs_curve(&mut native_curve);
            }
            native_ref(records, -1);
            native_i64(records, 1);
            records.push(native_bool(*curve_reversed));
            records.push(0x0f);
            native_nurbs_curve(records, &native_curve)?;
            records.push(0x10);
            if let Some([start, end]) = *parameter_range {
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

pub(crate) fn encode_design_bulkstream(
    target: &CadIr,
    native: &F3dNative,
    registry: &GeneratedDesignRegistry,
) -> Result<Option<EncodedDesignBulkStream>, CodecError> {
    let (_, projected_parameters) =
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            &crate::design::feature_project::ProjectInputs {
                native: &native.design_parameters,
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                timelines: &native.design_feature_timelines,
                construction_groups: &native.design_construction_operand_groups,
                fillet_radius_groups: &native.design_fillet_radius_groups,
                edge_operands: &native.design_edge_operands,
                edge_identity_operands: &native.design_edge_identity_operands,
                entity_selection_operands: &native.design_entity_selection_operands,
                curve_identities: &native.sketch_curve_identities,
                face_operands: &native.design_face_operands,
                body_recipe_operands: &native.design_body_recipe_operands,
                placements: &native.design_sketch_placements,
                body_bindings: &native.design_body_bindings,
                histories: &native.asm_histories,
            },
        )?;
    if target.model.parameters != projected_parameters {
        return Err(CodecError::Malformed(
            "neutral F3D parameters must equal the projection of native Design parameters".into(),
        ));
    }
    if !native.design_parameter_companions.is_empty()
        || !native.design_dimension_annotation_frames.is_empty()
        || !native.design_dimension_presentation_frames.is_empty()
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
    let mut primary_records = Vec::new();
    for parameter in &native.design_parameters {
        encode_document_parameter(&mut out, parameter)?;
    }
    if !registry.body_map.is_empty() {
        let class_tag = registry.body_map_class_tag.as_deref().ok_or_else(|| {
            CodecError::Malformed("generated F3D body map has no registered type".into())
        })?;
        let record_index = registry.body_map_record_index.ok_or_else(|| {
            CodecError::Malformed("generated F3D body map has no record identity".into())
        })?;
        primary_records.push(primary_record(record_index, out.len())?);
        native_lp_ascii(&mut out, class_tag)?;
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
        let count = u32::try_from(registry.body_map.len())
            .map_err(|_| CodecError::Malformed("Design body map exceeds u32::MAX".into()))?;
        out.extend_from_slice(&count.to_le_bytes());
        for (&body_key, &entity_suffix) in &registry.body_map {
            out.extend_from_slice(&body_key.to_le_bytes());
            out.extend_from_slice(&entity_suffix.to_le_bytes());
        }
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        native_lp_utf16(&mut out, "BREP.generated.smbh")?;
    }
    encode_browser_nodes(&mut out, &mut primary_records, registry)?;
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
        primary_records.push(primary_record_u64(header.entity_suffix, out.len())?);
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(header.class_tag.as_bytes());
        out.extend_from_slice(&header.entity_suffix.to_le_bytes());
        out.extend_from_slice(&[0; 5]);
        out.push(u8::from(header.optional_slot_present));
        if header.optional_slot_present {
            out.extend_from_slice(&[0; 4]);
        }
        native_lp_utf16(&mut out, &header.entity_id)?;
        if header.in_sketch_module() {
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
        let expected = crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE;
        let mut companion_types = registry
            .types
            .iter()
            .enumerate()
            .filter(|(_, design_type)| {
                design_type.type_guid.eq_ignore_ascii_case(expected.0)
                    && design_type.version == expected.1
                    && design_type.module == expected.2
                    && design_type
                        .entity_ids
                        .contains(&u64::from(point.paired_reference))
            });
        let companion_type_ordinal = companion_types
            .next()
            .map(|(ordinal, _)| ordinal)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "generated sketch point {} has no registered companion type",
                    point.id
                ))
            })?;
        if companion_types.next().is_some() {
            return Err(CodecError::Malformed(format!(
                "generated sketch point {} has multiple registered companion types",
                point.id
            )));
        }
        let companion_class_tag = super::presentation::dynamic_class_tag(companion_type_ordinal)?;
        primary_records.push(primary_record(point.record_index, out.len())?);
        encode_sketch_point(&mut out, point)?;
        primary_records.push(primary_record(point.paired_reference, out.len())?);
        encode_sketch_point_companion(
            &mut out,
            &companion_class_tag,
            point.paired_reference,
            point.record_index,
            point.companion.as_ref(),
        )?;
    }
    for curve in &native.sketch_curve_identities {
        primary_records.push(primary_record(curve.record_index, out.len())?);
        encode_sketch_curve_identity(&mut out, curve)?;
    }
    for text in &native.sketch_texts {
        primary_records.push(primary_record(text.record_index, out.len())?);
        encode_sketch_text(&mut out, text)?;
    }
    for relation in &native.sketch_relations {
        primary_records.push(primary_record(relation.record_index, out.len())?);
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
    Ok(Some(EncodedDesignBulkStream {
        bytes: out,
        primary_records,
    }))
}

fn primary_record(
    entity_id: u32,
    bulk_offset: usize,
) -> Result<crate::metastream::RecordIndexEntry, CodecError> {
    primary_record_u64(u64::from(entity_id), bulk_offset)
}

fn primary_record_u64(
    entity_id: u64,
    bulk_offset: usize,
) -> Result<crate::metastream::RecordIndexEntry, CodecError> {
    Ok(crate::metastream::RecordIndexEntry {
        entity_id,
        bulk_offset: u64::try_from(bulk_offset).map_err(|_| {
            CodecError::Malformed("generated Design record offset exceeds u64".into())
        })?,
    })
}

fn encode_document_parameter(
    out: &mut Vec<u8>,
    parameter: &crate::records::DesignParameter,
) -> Result<(), CodecError> {
    validate_dynamic_class_tag(&parameter.class_tag, "Design parameter")?;
    native_lp_ascii(out, &parameter.class_tag)?;
    out.extend_from_slice(&parameter.record_index.to_le_bytes());
    out.extend_from_slice(&[0; 11]);
    out.extend_from_slice(
        &parameter
            .family_discriminator
            .expect("source-less parameter preconditions require a discriminator")
            .to_le_bytes(),
    );
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
    if !point.coordinates.u.is_finite()
        || !point.coordinates.v.is_finite()
        || !point.depth.is_finite()
    {
        return Err(CodecError::Malformed(
            "source-less sketch point coordinates must be finite".into(),
        ));
    }
    let owner_reference = point.owner_reference.ok_or_else(|| {
        CodecError::Malformed(format!(
            "source-less sketch point {} has no direct owner",
            point.id
        ))
    })?;
    let shift = usize::from(point.entity_genesis.is_some()) * 52;
    let &SketchPointRecordForm::Version11 {
        padded_paired_reference,
    } = &point.record_form
    else {
        return Err(CodecError::NotImplemented(format!(
            "source-less sketch point {} requires the version-11 member sequence",
            point.id
        )));
    };
    let persistent_id = point.persistent_id.ok_or_else(|| {
        CodecError::Malformed(format!(
            "source-less sketch point {} has no persistent identity",
            point.id
        ))
    })?;
    let mut record = vec![0u8; 105 + shift];
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
    record[62 + shift..70 + shift].copy_from_slice(&persistent_id.to_le_bytes());
    record[70 + shift] = 1;
    record[71 + shift..75 + shift].copy_from_slice(&point.paired_reference.to_le_bytes());
    if point.flags.iter().any(|flag| *flag > 1) {
        return Err(CodecError::Malformed(format!(
            "source-less sketch point {} has a flag outside zero or one",
            point.id
        )));
    }
    record[81 + shift..89 + shift].copy_from_slice(&point.flags);
    record[89 + shift..97 + shift]
        .copy_from_slice(&(point.coordinates.u / LEN_TO_MM).to_le_bytes());
    record[97 + shift..105 + shift]
        .copy_from_slice(&(point.coordinates.v / LEN_TO_MM).to_le_bytes());
    let closure = point.closure.as_ref().ok_or_else(|| {
        CodecError::Malformed(format!(
            "source-less sketch point {} has no version-11 closure",
            point.id
        ))
    })?;
    if !point.record_form.closure_is_valid(Some(closure)) {
        return Err(CodecError::Malformed(format!(
            "source-less sketch point {} has an invalid closure selector or state",
            point.id
        )));
    }
    record.extend_from_slice(&(point.depth / LEN_TO_MM).to_le_bytes());
    record.extend_from_slice(&closure.selector.to_le_bytes());
    record.push(closure.state);
    record.extend_from_slice(&[0; 12]);
    record.extend_from_slice(&1.0f32.to_le_bytes());
    record.extend_from_slice(&1.0f32.to_le_bytes());
    record.extend_from_slice(&[0, 1, 0, 0, 0]);
    write_reference(&mut record, point.paired_reference);
    if padded_paired_reference {
        record.extend_from_slice(&[0; 4]);
    }
    write_reference(&mut record, owner_reference);
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_sketch_point_companion(
    out: &mut Vec<u8>,
    class_tag: &str,
    record_index: u32,
    point_record_index: u32,
    companion: Option<&crate::records::SketchPointCompanion>,
) -> Result<(), CodecError> {
    let companion = companion.ok_or_else(|| {
        CodecError::Malformed(format!(
            "source-less sketch point {point_record_index} has no inverse companion"
        ))
    })?;
    let prefix_present_zero = companion.prefix_present_zero;
    if companion.reference_encoding != SketchPointCompanionReferenceEncoding::SameSegment {
        return Err(CodecError::NotImplemented(
            "source-less sketch point companions require same-segment references".into(),
        ));
    }
    let incident_curves = companion.incident_curves.as_slice();
    let count = u32::try_from(incident_curves.len()).map_err(|_| {
        CodecError::Malformed("source-less sketch point companion exceeds u32::MAX curves".into())
    })?;
    let prefix_len = if prefix_present_zero { 25 } else { 21 };
    let mut record = vec![0u8; prefix_len];
    encode_sketch_record_header(&mut record, class_tag, record_index)?;
    if prefix_present_zero {
        record[20] = 1;
    }
    record.extend_from_slice(&count.to_le_bytes());
    for incident_curve in incident_curves {
        write_reference(&mut record, *incident_curve);
    }
    record.push(0);
    write_reference(&mut record, point_record_index);
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_sketch_curve_identity(
    out: &mut Vec<u8>,
    curve: &crate::records::SketchCurveIdentity,
) -> Result<(), CodecError> {
    let owner_reference = curve.owner_reference.ok_or_else(|| {
        CodecError::Malformed(format!(
            "source-less sketch curve {} has no direct owner",
            curve.id
        ))
    })?;
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
        None => {
            return Err(CodecError::NotImplemented(format!(
                "source-less sketch curve {} has no writable geometry",
                curve.id
            )))
        }
    }
    write_reference(&mut record, owner_reference);
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
        text.class_version,
        text.record_index,
        0,
    )
    .ok_or_else(|| CodecError::Malformed(format!("invalid raw sketch-text record {}", text.id)))?;
    let common_header_matches = text.raw_bytes.get(0..4) == Some(&3u32.to_le_bytes())
        && text.raw_bytes.get(4..7) == Some(text.class_tag.as_bytes());
    let legacy_index_matches =
        text.raw_bytes.get(7..15) == Some(&u64::from(text.record_index).to_le_bytes());
    let indexed_index_matches = text.raw_bytes.get(7..11) == Some(&text.record_index.to_le_bytes())
        && text.raw_bytes.get(11..20) == Some(&[0; 9]);
    let header_matches = common_header_matches && (legacy_index_matches || indexed_index_matches);
    let fields_match = decoded.owner_reference == text.owner_reference
        && decoded.entity_genesis == text.entity_genesis
        && decoded.persistent_id == text.persistent_id
        && decoded.base_id == text.base_id
        && decoded.text == text.text
        && decoded.font_family == text.font_family
        && decoded.height == text.height
        && decoded.width_factor == text.width_factor
        && decoded.color == text.color
        && decoded.anchor == text.anchor
        && decoded.rotation == text.rotation
        && decoded.horizontal_alignment == text.horizontal_alignment
        && decoded.vertical_alignment == text.vertical_alignment
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
    // An authored relation may omit the ordinals; a decoded one always pairs
    // them with its members.
    if !relation.member_relation_ordinals.is_empty()
        && relation.member_relation_ordinals.len() != relation.members.len()
    {
        return Err(CodecError::Malformed(format!(
            "F3D sketch relation {} has a relation-ordinal run that does not pair with its members",
            relation.id
        )));
    }
    let mut record = vec![0u8; 19];
    encode_sketch_record_header(&mut record, &relation.class_tag, relation.record_index)?;
    record.push(1);
    let member_count = u32::try_from(relation.members.len())
        .map_err(|_| CodecError::Malformed("sketch relation has too many members".into()))?;
    record.extend_from_slice(&member_count.to_le_bytes());
    for (ordinal, member) in relation.members.iter().enumerate() {
        write_reference(&mut record, *member);
        let relation_ordinal = relation
            .member_relation_ordinals
            .get(ordinal)
            .copied()
            .unwrap_or(0);
        record.extend_from_slice(&relation_ordinal.to_le_bytes());
    }
    // The base level's property-block presence byte, then the block when the
    // relation carries an `EntityGenesis` origin.
    match relation.entity_genesis {
        Some(genesis) => {
            record.push(1);
            record.extend_from_slice(&1u32.to_le_bytes());
            native_lp_ascii(&mut record, "EntityGenesis")?;
            native_lp_ascii(&mut record, "IntrinsicMetaTypeuint64")?;
            record.extend_from_slice(&genesis.to_le_bytes());
        }
        None => record.push(0),
    }
    for reference in relation
        .auxiliary_references
        .iter()
        .chain(std::iter::once(&relation.owner_reference))
    {
        write_reference(&mut record, *reference);
    }
    record.extend_from_slice(&relation.state.to_le_bytes());
    let return_count = u32::try_from(relation.return_members.len())
        .map_err(|_| CodecError::Malformed("sketch relation has too many return members".into()))?;
    record.extend_from_slice(&return_count.to_le_bytes());
    for reference in &relation.return_members {
        write_reference(&mut record, *reference);
    }
    record.push(0);
    record.resize(record.len().max(101), 0);
    out.extend_from_slice(&record);
    Ok(())
}

/// Write one same-segment reference ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata) "**References.**"): the
/// presence byte, the u64 target entity id, and the two zero flag bytes.
fn write_reference(out: &mut Vec<u8>, target: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(target).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
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
    registry: &GeneratedDesignRegistry,
    primary_records: &[crate::metastream::RecordIndexEntry],
) -> Result<Option<Vec<u8>>, CodecError> {
    if registry.types.is_empty() {
        return Ok(None);
    }

    // A generated segment writes the modern header shape, the type table, and
    // the primary offsets captured while writing its sibling BulkStream.
    let mut out = Vec::new();
    native_lp_ascii(&mut out, "Design")?;
    out.extend_from_slice(&0u32.to_le_bytes());
    native_lp_utf16(&mut out, GENERATED_ASSET_GUID)?;
    out.extend_from_slice(&1234u32.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    native_lp_ascii(&mut out, "FusionDesignSegmentType")?;
    native_lp_ascii(&mut out, "Fusion")?;
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let type_count = u32::try_from(registry.types.len()).map_err(|_| {
        CodecError::Malformed("Design MetaStream registers more than u32::MAX types".into())
    })?;
    out.extend_from_slice(&type_count.to_le_bytes());
    let mut next_entity_id = 1u64;
    for design_type in &registry.types {
        validate_guid(&design_type.type_guid, "Design type GUID")?;
        native_lp_ascii(&mut out, &design_type.type_guid)?;
        match &design_type.base_type_guid {
            Some(base) => {
                validate_guid(base, "Design base type GUID")?;
                native_lp_ascii(&mut out, base)?;
            }
            None => out.extend_from_slice(&0u32.to_le_bytes()),
        }
        out.extend_from_slice(&design_type.version.to_le_bytes());
        if crate::bytes::is_guid_relaxed(&design_type.module) {
            return Err(CodecError::Malformed(format!(
                "Design type module name is GUID-shaped: {}",
                design_type.module
            )));
        }
        native_lp_ascii(&mut out, &design_type.module)?;
        let count = u32::try_from(design_type.entity_ids.len()).map_err(|_| {
            CodecError::Malformed("Design type owns more than u32::MAX entities".into())
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for entity_id in &design_type.entity_ids {
            out.extend_from_slice(&entity_id.to_le_bytes());
            next_entity_id = next_entity_id.max(entity_id.saturating_add(1));
        }
    }
    // Named-entity list, primary record index, and secondary index.
    out.extend_from_slice(&0u32.to_le_bytes());
    let record_count = u32::try_from(primary_records.len()).map_err(|_| {
        CodecError::Malformed("Design primary index exceeds u32::MAX records".into())
    })?;
    out.extend_from_slice(&record_count.to_le_bytes());
    let registered_entities = registry
        .types
        .iter()
        .flat_map(|design_type| design_type.entity_ids.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    let mut indexed_entities = std::collections::BTreeSet::new();
    let mut previous_offset = None;
    for record in primary_records {
        if !registered_entities.contains(&record.entity_id) {
            return Err(CodecError::Malformed(format!(
                "generated Design primary entity {} has no type registration",
                record.entity_id
            )));
        }
        if !indexed_entities.insert(record.entity_id) {
            return Err(CodecError::Malformed(format!(
                "generated Design primary entity {} is indexed more than once",
                record.entity_id
            )));
        }
        if previous_offset.is_some_and(|previous| previous >= record.bulk_offset) {
            return Err(CodecError::Malformed(
                "generated Design primary offsets are not strictly increasing".into(),
            ));
        }
        previous_offset = Some(record.bulk_offset);
        out.extend_from_slice(&record.entity_id.to_le_bytes());
        out.extend_from_slice(&record.bulk_offset.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&next_entity_id.to_le_bytes());
    // Trailing flag and empty property block.
    out.extend_from_slice(&[0; 8]);
    Ok(Some(out))
}

fn encode_browser_nodes(
    out: &mut Vec<u8>,
    primary_records: &mut Vec<crate::metastream::RecordIndexEntry>,
    registry: &GeneratedDesignRegistry,
) -> Result<(), CodecError> {
    if registry.browser_nodes.is_empty() {
        return Ok(());
    }
    let node_class_tag = registry.browser_node_class_tag.as_deref().ok_or_else(|| {
        CodecError::Malformed("generated F3D browser nodes have no registered type".into())
    })?;
    for node in &registry.browser_nodes {
        primary_records.push(primary_record(node.record_index, out.len())?);
        native_lp_ascii(out, node_class_tag)?;
        out.extend_from_slice(&node.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        native_lp_utf16(out, &node.node_guid)?;
        out.push(u8::from(!node.visible));
        out.extend_from_slice(&[0x01, 0x01]);
        out.extend_from_slice(&node.entity_suffix.to_le_bytes());
    }
    Ok(())
}

/// Asset GUID written into a generated Design `MetaStream`, which has no source
/// asset to name.
const GENERATED_ASSET_GUID: &str = "00000000-0000-0000-0000-000000000000";

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
