// SPDX-License-Identifier: Apache-2.0
//! Bulkstream and sketch/design record encoders for source-less generation.

use crate::records::{
    recipes::ConstructionRecipeKind,
    references::PersistentReferenceKind,
    sketch_geometry::{SketchCurveGeometry, SketchPointRecordForm, SketchText},
};
use crate::writer::primitives::zero_fill_to;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CoedgeId;

use super::{
    index::NativeGenerationIndex,
    native_bytes::{native_f64, native_i64, native_ref},
    native_geometry::native_nurbs_curve,
    presentation::GeneratedDesignRegistry,
};
use crate::native::F3dNative;
use crate::writer::primitives::native_bool;
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;

/// Generated Design `BulkStream` and the primary offsets known while writing it.
pub(in crate::writer::generate) struct EncodedDesignBulkStream {
    pub(super) bytes: Vec<u8>,
    pub(super) primary_records: Vec<crate::metastream::RecordIndexEntry>,
    pub(super) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    pub(super) written_materials: std::collections::BTreeSet<String>,
    pub(super) written_recipes: std::collections::BTreeSet<String>,
}

pub(super) fn tolerant_coedge_range(
    index: &NativeGenerationIndex<'_>,
    coedge: &CoedgeId,
) -> Option<[f64; 2]> {
    index
        .tolerant_coedges
        .get(coedge.as_str())
        .map(|parameters| parameters.parameter_range.get())
}

pub(super) fn native_tolerant_coedge_extension(
    records: &mut Vec<u8>,
    target: &CadIr,
    index: &NativeGenerationIndex<'_>,
    coedge: &CoedgeId,
) -> Result<(), crate::writer::WriteError> {
    let extension = index
        .tolerant_coedges
        .get(coedge.as_str())
        .map(|parameters| &parameters.extension);
    match extension {
        None
        | Some(
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::None {}
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
                .ok_or_else(|| {
                    crate::writer::WriteError::Unwritable(format!("missing coedge {coedge}"))
                })?;
            let use_curve = model_coedge.use_curve.as_ref().ok_or_else(|| {
                crate::writer::WriteError::Unwritable(format!(
                    "tolerant coedge {coedge} has no use curve"
                ))
            })?;
            let curve = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == use_curve.curve)
                .ok_or_else(|| {
                    crate::writer::WriteError::Unwritable(format!(
                        "missing use curve {}",
                        use_curve.curve
                    ))
                })?;
            let Some(SolvedCurveGeometry::Nurbs(curve)) = curve.geometry.solved() else {
                return Err(crate::writer::WriteError::Unsupported(format!(
                    "source-less F3D tolerant coedge {coedge} requires a NURBS use curve"
                )));
            };
            let mut native_curve = curve.clone();
            if *curve_reversed {
                native_curve.reverse_parameterization();
            }
            native_ref(records, -1);
            native_i64(records, 1);
            records.push(native_bool(*curve_reversed));
            records.push(0x0f);
            native_nurbs_curve(records, &native_curve)?;
            records.push(0x10);
            if let Some(range) = *parameter_range {
                let [start, end] = range.get();
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
        _ => Err(crate::writer::WriteError::Unsupported(format!(
            "source-less F3D cannot serialize nonempty tolerant-coedge extension for {coedge}"
        ))),
    }
}

pub(super) fn needs_design_bulk(target: &CadIr, native: &F3dNative) -> bool {
    !native.construction_recipes.is_empty()
        || !native.persistent_references.is_empty()
        || !native.lost_edge_references.is_empty()
        || !native.design_body_members.is_empty()
        || !native.design_entity_headers.is_empty()
        || !native.design_record_headers.is_empty()
        || !native.design_face_source_groups.is_empty()
        || !native.design_material_assignments.is_empty()
        || !native.sketch_points.is_empty()
        || !native.sketch_curve_identities.is_empty()
        || !native.sketch_relations.is_empty()
        || !native.sketch_texts.is_empty()
        || target
            .model
            .bodies
            .iter()
            .any(|body| body.visible.is_some())
}

pub(super) fn encode_design_bulkstream(
    output_ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    target: &CadIr,
    native: &F3dNative,
    registry: &GeneratedDesignRegistry,
    parameter_bytes: Vec<u8>,
    brep_name: &str,
    extra_body_pairs: &[(u64, u64)],
) -> Result<Option<EncodedDesignBulkStream>, crate::writer::WriteError> {
    let body_key_bodies = target
        .model
        .bodies
        .iter()
        .filter_map(|body| {
            body.native_ref
                .as_ref()
                .map(|native_ref| (native_ref.clone(), body.id.clone()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let projected_parameters =
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            output_ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &native.design_parameters,
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                timelines: &native.design_feature_timelines,
                construction_groups: &native.design_construction_operand_groups,
                fillet_radius_groups: &native.design_fillet_radius_groups,
                edge_operands: &native.design_edge_operands,
                edge_identity_operands: &native.design_edge_identity_operands,
                edge_treatment_vertex_operands: &native.design_edge_treatment_vertex_operands,
                entity_selection_operands: &native.design_entity_selection_operands,
                curve_identities: &native.sketch_curve_identities,
                face_operands: &native.design_face_operands,
                body_recipe_operands: &native.design_body_recipe_operands,
                legacy_loft_body_carriers: &native.design_loft_legacy_body_carriers,
                placements: &native.design_sketch_placements,
                body_bindings: &native.design_body_bindings,
                body_key_bodies: Some(&body_key_bodies),
                histories: &native.asm_histories,
                face_boundaries: &native.asm_face_boundaries,
                sheet_metal_settings: &native.design_sheet_metal_settings,
            },
        )?
        .parameters;
    // W2: document parameters are rewritten from the regenerated Design projection.
    debug_assert!(
        target.model.parameters.iter().filter(|parameter| parameter.owner.is_none()).eq(projected_parameters.iter()),
        "regenerated F3D Design parameters must project back to the neutral parameters they were authored from"
    );
    if !native.design_parameter_companions.is_empty()
        || !native.design_dimension_annotation_frames.is_empty()
        || !native.design_dimension_presentation_frames.is_empty()
        || !native.design_face_source_groups.is_empty()
        || !native.design_dimension_locus_pairs.is_empty()
        || !native.design_dimension_locus_groups.is_empty()
        || !native.design_dimension_null_locus_pairs.is_empty()
        || !native.design_parameter_owners.is_empty()
        || native.design_parameter_scopes.iter().any(|scope| {
            scope.copy_paste_bodies_operation().is_none()
                && scope.copy_paste_component_operation().is_none()
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::Sketch
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::Extrude
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::BaseFeature
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::BaseFlange
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::RemoveBody
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::Form
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::Canvas
                && scope.kind() != crate::records::feature::scope::DesignFeatureKind::Decal
                && scope.kind()
                    != crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature
        })
        || !native.design_sketch_placements.is_empty()
        || native
            .design_entity_headers
            .iter()
            .any(|header| header.member_values().next().is_some())
    {
        return Err(crate::writer::WriteError::Unsupported(
            "source-less F3D Design parameter records are not writable".into(),
        ));
    }
    if parameter_bytes.is_empty() && !needs_design_bulk(target, native) {
        return Ok(None);
    }

    let mut out = parameter_bytes;
    let mut primary_records = Vec::new();
    let mut losses = Vec::new();
    let mut written_materials = std::collections::BTreeSet::new();
    let mut written_recipes = std::collections::BTreeSet::new();
    if let Some(body_map) = &registry.body_map {
        let class_tag = body_map.class_tag.as_str();
        let record_index = body_map.record_index;
        primary_records.push(primary_record(output_ctx, record_index, out.len())?);
        native_lp_ascii(&mut out, class_tag)?;
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
        let extra = extra_body_pairs
            .iter()
            .filter(|(key, suffix)| body_map.entries.as_map().get(key) != Some(suffix))
            .collect::<std::collections::BTreeSet<_>>();
        let total = body_map
            .entries
            .as_map()
            .len()
            .checked_add(extra.len())
            .ok_or_else(|| {
                crate::writer::WriteError::Unwritable("Design body map count overflows".into())
            })?;
        let count = u32::try_from(total).map_err(|_| {
            crate::writer::WriteError::Unwritable("Design body map exceeds u32::MAX".into())
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for (&body_key, &entity_suffix) in body_map.entries.as_map() {
            out.extend_from_slice(&body_key.to_le_bytes());
            out.extend_from_slice(&entity_suffix.to_le_bytes());
        }
        for &&(body_key, entity_suffix) in &extra {
            out.extend_from_slice(&body_key.to_le_bytes());
            out.extend_from_slice(&entity_suffix.to_le_bytes());
        }
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        native_lp_utf16(&mut out, brep_name)?;
    }
    encode_browser_nodes(output_ctx, &mut out, &mut primary_records, registry)?;
    for assignment in &native.design_material_assignments {
        if encode_material_assignment(
            output_ctx,
            &mut out,
            &mut primary_records,
            registry,
            target,
            assignment,
        )? {
            if let Some(binding) = target.model.appearance_bindings.iter().find(|binding| {
                binding.source_entity_id.as_deref() == Some(assignment.entity_id.as_str())
            }) {
                written_materials.insert(binding.id.to_string());
            }
            losses.push(
                cadmpeg_ir::codec::write::ExportLossCode::RecordResidualUnavailable.note(format!(
                    "material assignment {}: the physical asset identity is generated",
                    assignment.id
                )),
            );
        } else {
            losses.push(
                crate::loss::F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                    "material assignment {} has no complete registered body presentation",
                    assignment.id
                )),
            );
        }
    }
    if !native.design_body_members.is_empty() {
        let (ordinal, kind) = registry
            .types
            .iter()
            .enumerate()
            .find(|(_, kind)| {
                kind.type_guid
                    .as_str()
                    .eq_ignore_ascii_case(super::presentation::BODY_ROOT_TYPE_GUID)
            })
            .ok_or_else(|| {
                crate::writer::WriteError::Unwritable("missing generated BodiesRoot type".into())
            })?;
        let record_index = *kind.entity_ids.first().ok_or_else(|| {
            crate::writer::WriteError::Unwritable("missing generated BodiesRoot identity".into())
        })?;
        primary_records.push(primary_record_u64(output_ctx, record_index, out.len())?);
        native_lp_ascii(
            &mut out,
            super::presentation::dynamic_class_tag(ordinal)?.as_str(),
        )?;
        out.extend_from_slice(
            &u32::try_from(record_index)
                .map_err(|_| {
                    crate::writer::WriteError::Unwritable("BodiesRoot index exceeds u32".into())
                })?
                .to_le_bytes(),
        );
        out.extend_from_slice(&[0; 4]);
        native_lp_ascii(&mut out, "BodiesRoot")?;
        out.extend_from_slice(&0u16.to_le_bytes());
        native_lp_ascii(&mut out, "BodiesRoot")?;
        let count = u32::try_from(native.design_body_members.len()).map_err(|_| {
            crate::writer::WriteError::Unwritable(
                "Design BodiesRoot exceeds u32::MAX members".into(),
            )
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for member in &native.design_body_members {
            out.push(1);
            out.extend_from_slice(&member.entity_suffix.to_le_bytes());
            out.extend_from_slice(&member.flags.to_le_bytes());
        }
        out.push(0);
    }
    for header in &native.design_record_headers {
        let typed_record = native.sketch_points.iter().any(|record| {
            record.record_index == header.record_index
                || record.paired_reference == header.record_index
        }) || native
            .sketch_curve_identities
            .iter()
            .any(|record| record.record_index == header.record_index)
            || native
                .sketch_texts
                .iter()
                .any(|record| record.record_index == header.record_index)
            || native
                .sketch_relations
                .iter()
                .any(|record| record.record_index == header.record_index)
            || native
                .design_entity_headers
                .iter()
                .any(|record| record.entity_id.suffix() == u64::from(header.record_index))
            || primary_records
                .iter()
                .any(|record| record.entity_id == u64::from(header.record_index));
        if typed_record {
            continue;
        }
        let registered = header
            .class_tag
            .as_str()
            .parse::<usize>()
            .ok()
            .and_then(|tag| tag.checked_sub(256))
            .and_then(|ordinal| registry.types.get(ordinal))
            .is_some_and(|kind| kind.entity_ids.contains(&u64::from(header.record_index)));
        if registered {
            primary_records.push(primary_record(output_ctx, header.record_index, out.len())?);
        } else {
            output_ctx.charge_entities(1, "F3D Design record")?;
        }
        native_lp_ascii(&mut out, header.class_tag.as_str())?;
        out.extend_from_slice(&header.record_index.to_le_bytes());
    }
    for header in &native.design_entity_headers {
        primary_records.push(primary_record_u64(
            output_ctx,
            header.entity_id.suffix(),
            out.len(),
        )?);
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(header.class_tag.as_bytes());
        out.extend_from_slice(&header.entity_id.suffix().to_le_bytes());
        out.extend_from_slice(&[0; 5]);
        // Optional padding is a source layout choice; canonical output omits it.
        out.push(0);
        native_lp_utf16(&mut out, header.entity_id.as_str())?;
        if header.in_sketch_module() {
            let count = u32::try_from(header.reference_values().count()).map_err(|_| {
                crate::writer::WriteError::Unwritable(
                    "Design sketch header exceeds u32::MAX references".into(),
                )
            })?;
            match header
                .sketch_references()
                .and_then(|list| list.record_reference)
            {
                Some(record_reference) => {
                    out.extend_from_slice(&record_reference.to_le_bytes());
                    out.extend_from_slice(&[0; 4]);
                }
                // The sentinel base-record slot of a sketch with no base record.
                None => out.extend_from_slice(&[0xFF; 8]),
            }
            out.push(1);
            out.extend_from_slice(&count.to_le_bytes());
            for reference in header.reference_values() {
                out.push(1);
                out.extend_from_slice(&reference.to_le_bytes());
                out.extend_from_slice(&[0; 6]);
            }
        }
    }
    for point in &native.sketch_points {
        point
            .validate_for_write(&native.sketch_curve_identities)
            .map_err(crate::writer::WriteError::Unwritable)?;
        let expected = crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE;
        let mut companion_types = registry
            .types
            .iter()
            .enumerate()
            .filter(|(_, design_type)| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(expected.0)
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
                crate::writer::WriteError::Unwritable(format!(
                    "generated sketch point {} has no registered companion type",
                    point.id
                ))
            })?;
        if companion_types.next().is_some() {
            return Err(crate::writer::WriteError::Unwritable(format!(
                "generated sketch point {} has multiple registered companion types",
                point.id
            )));
        }
        let companion_class_tag = super::presentation::dynamic_class_tag(companion_type_ordinal)?;
        primary_records.push(primary_record(output_ctx, point.record_index, out.len())?);
        encode_sketch_point(&mut out, point)?;
        primary_records.push(primary_record(
            output_ctx,
            point.paired_reference,
            out.len(),
        )?);
        encode_sketch_point_companion(
            &mut out,
            &companion_class_tag,
            point.paired_reference,
            point.record_index,
            point.companion(),
        )?;
    }
    for curve in &native.sketch_curve_identities {
        primary_records.push(primary_record(output_ctx, curve.record_index, out.len())?);
        encode_sketch_curve_identity(&mut out, curve)?;
    }
    for text in &native.sketch_texts {
        primary_records.push(primary_record(output_ctx, text.record_index, out.len())?);
        encode_sketch_text(&mut out, text)?;
    }
    for relation in &native.sketch_relations {
        primary_records.push(primary_record(
            output_ctx,
            relation.record_index,
            out.len(),
        )?);
        encode_sketch_relation(&mut out, relation)?;
    }
    let mut used = primary_records
        .iter()
        .map(|record| record.entity_id)
        .collect::<std::collections::BTreeSet<_>>();
    let registered = |tag: &crate::records::references::DesignClassTag, index: u32| {
        tag.as_str()
            .parse::<usize>()
            .ok()
            .and_then(|tag| tag.checked_sub(256))
            .and_then(|ordinal| registry.types.get(ordinal))
            .is_some_and(|kind| kind.entity_ids.contains(&u64::from(index)))
    };
    let mut writable = true;
    for reference in &native.lost_edge_references {
        writable &= registered(&reference.class_tag, reference.record_index)
            && registered(&reference.next_class_tag, reference.next_record_index)
            && used.insert(u64::from(reference.record_index));
    }
    if let Some(last) = native.lost_edge_references.last() {
        writable &= used.insert(u64::from(last.next_record_index));
    }
    for (ordinal, reference) in native.lost_edge_references.iter().enumerate() {
        if !writable {
            losses.push(
                crate::loss::F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                    "lost-edge reference {} has no distinct registered primary records",
                    reference.id
                )),
            );
            continue;
        }
        if let Some(previous) = ordinal
            .checked_sub(1)
            .and_then(|ordinal| native.lost_edge_references.get(ordinal))
        {
            if previous.next_class_tag != reference.class_tag
                || previous.next_record_index != reference.record_index
            {
                return Err(crate::writer::WriteError::Unwritable(format!(
                    "F3D lost-edge record {} does not continue the preceding indexed run",
                    reference.id
                )));
            }
        } else {
            primary_records.push(primary_record(
                output_ctx,
                reference.record_index,
                out.len(),
            )?);
            native_lp_ascii(&mut out, reference.class_tag.as_str())?;
            out.extend_from_slice(&reference.record_index.to_le_bytes());
        }
        out.extend_from_slice(&[0; 14]);
        out.extend_from_slice(&19u32.to_le_bytes());
        out.extend_from_slice(b"EDGE_REFERENCE_LOST");
        primary_records.push(primary_record(
            output_ctx,
            reference.next_record_index,
            out.len(),
        )?);
        native_lp_ascii(&mut out, reference.next_class_tag.as_str())?;
        out.extend_from_slice(&reference.next_record_index.to_le_bytes());
    }
    let mut recipes = native.construction_recipes.iter().collect::<Vec<_>>();
    recipes.sort_by_key(|recipe| recipe.recipe_index);
    let mut recipe_ordinals = std::collections::HashMap::new();
    for recipe in recipes {
        let key = (
            recipe.kind,
            recipe
                .design
                .as_ref()
                .map(|design| design.id.value.as_str()),
        );
        let expected = recipe_ordinals.entry(key).or_insert(0u32);
        if recipe.recipe_index != *expected
            || recipe.record_index.is_none()
            || recipe.design.as_ref().is_some_and(|design| {
                design.selector.is_none()
                    || design.id.value.len() != 3
                    || !design.id.value.bytes().all(|byte| byte.is_ascii_digit())
            })
        {
            losses.push(
                crate::loss::F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                    "construction recipe {} has no representable indexed form",
                    recipe.id
                )),
            );
            continue;
        }
        if primary_records.is_empty() {
            losses.push(
                crate::loss::F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                    "construction recipe {} has no indexed Design frame",
                    recipe.id
                )),
            );
            continue;
        }
        let name = construction_recipe_name(recipe.kind);
        let mut prefix = [0u8; 27];
        if let Some(design) = &recipe.design {
            let design_id = &design.id.value;
            prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
            prefix[4..7].copy_from_slice(design_id.as_bytes());
            if let Some(selector) = &design.selector {
                prefix[7..11].copy_from_slice(&selector.value.to_le_bytes());
            }
        }
        let Some(record_index) = recipe.record_index else {
            continue;
        };
        output_ctx.charge_entities(1, "F3D Design record")?;
        prefix[11..15].copy_from_slice(&record_index.value.to_le_bytes());
        prefix[23..27].copy_from_slice(
            &u32::try_from(name.len())
                .map_err(|_| {
                    crate::writer::WriteError::Unwritable(
                        "Design recipe name exceeds u32::MAX".into(),
                    )
                })?
                .to_le_bytes(),
        );
        out.extend_from_slice(&prefix);
        out.extend_from_slice(name);
        out.extend_from_slice(&(-1i64).to_le_bytes());
        for value in [2i32, 0, -1, 1, -1] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        written_recipes.insert(recipe.id.clone());
        *expected = expected.checked_add(1).ok_or_else(|| {
            crate::writer::WriteError::Unwritable("F3D recipe count exceeds u32".into())
        })?;
    }
    for reference in &native.persistent_references {
        if primary_records.is_empty() {
            losses.push(
                crate::loss::F3dLossCode::WriterNativeRecordsOmitted.note(format!(
                    "persistent reference {} has no indexed Design frame",
                    reference.id
                )),
            );
            continue;
        }
        output_ctx.charge_entities(1, "F3D Design record")?;
        out.extend_from_slice(persistent_reference_name(reference.kind));
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&14u32.to_le_bytes());
        out.extend_from_slice(&[0; 14]);
        out.extend_from_slice(&23u32.to_le_bytes());
        out.extend_from_slice(b"IntrinsicMetaTypeuint64");
        out.extend_from_slice(&reference.value.to_le_bytes());
    }
    Ok(Some(EncodedDesignBulkStream {
        bytes: out,
        primary_records,
        losses,
        written_materials,
        written_recipes,
    }))
}

fn primary_record(
    output_ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity_id: u32,
    bulk_offset: usize,
) -> Result<crate::metastream::RecordIndexEntry, crate::writer::WriteError> {
    primary_record_u64(output_ctx, u64::from(entity_id), bulk_offset)
}

fn encode_material_assignment(
    output_ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut Vec<u8>,
    primary_records: &mut Vec<crate::metastream::RecordIndexEntry>,
    registry: &GeneratedDesignRegistry,
    target: &CadIr,
    assignment: &crate::records::references::DesignMaterialAssignment,
) -> Result<bool, crate::writer::WriteError> {
    use crate::design::presentation::{
        APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_TYPE_GUID, BODY_SCENE_NODE_TYPE_GUID,
        BREP_CONTAINER_TYPE_GUID, PHYSICAL_MATERIAL_LIBRARY_ID,
    };
    let entity = assignment.entity_id.suffix();
    let Some((ordinal, _)) = registry.types.iter().enumerate().find(|(_, kind)| {
        kind.type_guid
            .as_str()
            .eq_ignore_ascii_case(BODY_PRESENTATION_TYPE_GUID)
            && kind.entity_ids.contains(&entity)
    }) else {
        return Ok(false);
    };
    let Some(container) = registry
        .types
        .iter()
        .find(|kind| {
            kind.type_guid
                .as_str()
                .eq_ignore_ascii_case(BREP_CONTAINER_TYPE_GUID)
                && kind.version == 4
                && kind.entity_ids.len() == 1
        })
        .and_then(|kind| kind.entity_ids.first())
        .copied()
    else {
        return Ok(false);
    };
    let Some(scene) = entity.checked_add(1).filter(|scene| {
        registry.types.iter().any(|kind| {
            kind.type_guid
                .as_str()
                .eq_ignore_ascii_case(BODY_SCENE_NODE_TYPE_GUID)
                && kind.version == 1
                && kind.entity_ids.contains(scene)
        })
    }) else {
        return Ok(false);
    };
    let Some(node) = registry
        .browser_nodes
        .as_ref()
        .and_then(|nodes| nodes.nodes.iter().find(|node| node.entity_suffix == entity))
    else {
        return Ok(false);
    };
    let Some(binding) =
        target.model.appearance_bindings.iter().find(|binding| {
            binding.source_entity_id.as_deref() == Some(assignment.entity_id.as_str())
        })
    else {
        return Ok(false);
    };
    let cadmpeg_ir::appearance::AppearanceTarget::Body(body_id) = &binding.target else {
        return Ok(false);
    };
    let Some(body) = target.model.bodies.iter().find(|body| &body.id == body_id) else {
        return Ok(false);
    };
    let Some(appearance) = target
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
    else {
        return Ok(false);
    };
    let Some(physical) = appearance.physical_token.as_deref() else {
        return Ok(false);
    };
    let Some(visual) = appearance
        .visual_guid
        .as_deref()
        .or(appearance.asset_guid.as_deref())
    else {
        return Ok(false);
    };
    let tag = super::presentation::dynamic_class_tag(ordinal)?;
    primary_records.push(primary_record_u64(output_ctx, entity, out.len())?);
    native_lp_ascii(out, tag.as_str())?;
    out.extend_from_slice(&entity.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    native_lp_utf16(out, assignment.entity_id.as_str())?;
    native_lp_utf16(out, &node.node_guid)?;
    out.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    native_lp_utf16(
        out,
        &super::presentation::deterministic_guid("physical-asset", physical),
    )?;
    native_lp_utf16(out, PHYSICAL_MATERIAL_LIBRARY_ID)?;
    native_lp_utf16(out, physical)?;
    out.push(1);
    out.extend_from_slice(&container.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out.push(0);
    out.push(1);
    out.extend_from_slice(&scene.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    native_lp_utf16(out, body.name.as_deref().unwrap_or(""))?;
    out.extend_from_slice(&1.0f32.to_le_bytes());
    out.extend_from_slice(&[1, 1]);
    native_lp_utf16(out, visual)?;
    native_lp_utf16(out, APPEARANCE_LIBRARY_ID)?;
    if let Some(preset) = &assignment.visual_preset {
        native_lp_utf16(out, &preset.value)?;
    }
    Ok(true)
}

fn primary_record_u64(
    output_ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entity_id: u64,
    bulk_offset: usize,
) -> Result<crate::metastream::RecordIndexEntry, crate::writer::WriteError> {
    output_ctx.charge_entities(1, "F3D Design record")?;
    Ok(crate::metastream::RecordIndexEntry {
        entity_id,
        bulk_offset: cadmpeg_core::decode::u64_from_index(bulk_offset),
    })
}

fn encode_sketch_record_header(
    out: &mut [u8],
    class_tag: &crate::records::references::DesignClassTag,
    record_index: u32,
) {
    out[0..4].copy_from_slice(&3u32.to_le_bytes());
    out[4..7].copy_from_slice(class_tag.as_bytes());
    out[7..11].copy_from_slice(&record_index.to_le_bytes());
}

fn encode_sketch_point(
    out: &mut Vec<u8>,
    point: &crate::records::sketch_geometry::SketchPoint,
) -> Result<(), crate::writer::WriteError> {
    let owner_reference = point.owner_reference.ok_or_else(|| {
        crate::writer::WriteError::Unwritable(format!(
            "source-less sketch point {} has no direct owner",
            point.id
        ))
    })?;
    let SketchPointRecordForm::Version11 {
        padded_paired_reference,
        derived_type: false,
        entity_genesis,
        depth,
        persistent_id,
        flags,
        closure,
        ..
    } = point.record_form()
    else {
        return Err(crate::writer::WriteError::Unsupported(format!(
            "source-less sketch point {} requires the version-11 member sequence",
            point.id
        )));
    };
    let shift = usize::from(entity_genesis.is_some()) * 52;
    let mut record = Vec::new();
    zero_fill_to(&mut record, 105 + shift);
    encode_sketch_record_header(&mut record, &point.class_tag, point.record_index);
    record[20] = 1;
    record[21..25].copy_from_slice(&(1 + u32::from(entity_genesis.is_some())).to_le_bytes());
    if let Some(entity_genesis) = entity_genesis {
        encode_entity_genesis(&mut record, *entity_genesis);
    }
    record[25 + shift..29 + shift].copy_from_slice(&6u32.to_le_bytes());
    record[29 + shift..35 + shift].copy_from_slice(b"pt_tag");
    record[35 + shift..39 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[39 + shift..62 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[62 + shift..70 + shift].copy_from_slice(&persistent_id.get().to_le_bytes());
    record[70 + shift] = 1;
    record[71 + shift..75 + shift].copy_from_slice(&point.paired_reference.to_le_bytes());
    record[81 + shift..89 + shift].copy_from_slice(&flags.map(u8::from));
    record[89 + shift..97 + shift]
        .copy_from_slice(&(point.coordinates().u / LEN_TO_MM).to_le_bytes());
    record[97 + shift..105 + shift]
        .copy_from_slice(&(point.coordinates().v / LEN_TO_MM).to_le_bytes());
    record.extend_from_slice(&(depth.get() / LEN_TO_MM).to_le_bytes());
    record.extend_from_slice(&closure.selector().to_le_bytes());
    record.push(closure.state());
    record.extend_from_slice(&[0; 12]);
    record.extend_from_slice(&1.0f32.to_le_bytes());
    record.extend_from_slice(&1.0f32.to_le_bytes());
    record.extend_from_slice(&[0, 1, 0, 0, 0]);
    write_reference(&mut record, point.paired_reference);
    if *padded_paired_reference {
        record.extend_from_slice(&[0; 4]);
    }
    write_reference(&mut record, owner_reference);
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_sketch_point_companion(
    out: &mut Vec<u8>,
    class_tag: &crate::records::references::DesignClassTag,
    record_index: u32,
    point_record_index: u32,
    companion: crate::records::sketch_geometry::SketchPointCompanionRef<'_>,
) -> Result<(), crate::writer::WriteError> {
    let prefix_present_zero = companion.prefix_present_zero;
    let incident_curves = companion.incident_curves;
    let count = u32::try_from(incident_curves.len()).map_err(|_| {
        crate::writer::WriteError::Unwritable(
            "source-less sketch point companion exceeds u32::MAX curves".into(),
        )
    })?;
    let prefix_len = if prefix_present_zero { 25 } else { 21 };
    let mut record = Vec::new();
    zero_fill_to(&mut record, prefix_len);
    encode_sketch_record_header(&mut record, class_tag, record_index);
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
    curve: &crate::records::sketch_geometry::SketchCurveIdentity,
) -> Result<(), crate::writer::WriteError> {
    let owner_reference = curve.owner_reference.ok_or_else(|| {
        crate::writer::WriteError::Unwritable(format!(
            "source-less sketch curve {} has no direct owner",
            curve.id
        ))
    })?;
    let shift = usize::from(curve.entity_genesis.is_some()) * 52;
    let mut record = Vec::new();
    zero_fill_to(&mut record, 133 + shift);
    encode_sketch_record_header(&mut record, &curve.class_tag, curve.record_index);
    record[20] = 1;
    record[21..25].copy_from_slice(&(2 + u32::from(curve.entity_genesis.is_some())).to_le_bytes());
    if let Some(entity_genesis) = curve.entity_genesis {
        encode_entity_genesis(&mut record, entity_genesis);
    }
    record[25 + shift..29 + shift].copy_from_slice(&14u32.to_le_bytes());
    record[29 + shift..43 + shift].copy_from_slice(b"crv_primary_id");
    record[43 + shift..47 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[47 + shift..70 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[70 + shift..78 + shift].copy_from_slice(&curve.primary_id.get().to_le_bytes());
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
            let start = start.get();
            let end = end.get();
            let direction = direction.as_raw();
            let normal = normal.as_raw();
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
            encode_f64_sequence(&mut record, &values);
        }
        Some(SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        }) => {
            let center = center.as_raw();
            let normal = normal.as_raw();
            let reference_direction = reference_direction.as_raw();
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
                radius.get() / LEN_TO_MM,
                start_angle.get(),
                end_angle.get(),
            ];
            encode_f64_sequence(&mut record, &values);
        }
        Some(SketchCurveGeometry::Nurbs {
            carrier_reference,
            subtype_class_tag,
            subtype_record_index,
            geometry,
        }) => encode_sketch_nurbs(
            &mut record,
            *carrier_reference,
            subtype_class_tag,
            *subtype_record_index,
            geometry,
        )?,
        None => {
            return Err(crate::writer::WriteError::Unsupported(format!(
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

fn encode_f64_sequence(out: &mut Vec<u8>, values: &[f64]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// The sketch NURBS null carrier reference.
///
/// `docs/formats/f3d.md`: the payload begins with "either an eight-byte
/// all-`0xff` null sentinel or a non-null u64 carrier reference".
const NULL_CARRIER_REFERENCE: u64 = u64::MAX;

pub(super) fn encode_sketch_nurbs(
    record: &mut Vec<u8>,
    carrier_reference: Option<u64>,
    subtype_class_tag: &crate::records::references::DesignClassTag,
    subtype_record_index: u32,
    geometry: &crate::records::sketch_geometry::SketchNurbsGeometry,
) -> Result<(), crate::writer::WriteError> {
    const NURBS_PREFIX_PADDING: usize = 88 - (8 + 4 + 3 + 4);
    let knots = geometry.knots();
    let poles = geometry.poles();
    record.extend_from_slice(
        &carrier_reference
            .unwrap_or(NULL_CARRIER_REFERENCE)
            .to_le_bytes(),
    );
    record.extend_from_slice(&3u32.to_le_bytes());
    record.extend_from_slice(subtype_class_tag.as_bytes());
    record.extend_from_slice(&subtype_record_index.to_le_bytes());
    record.extend_from_slice(&[0; NURBS_PREFIX_PADDING]);
    record.push(1);
    record.push(0);
    record.extend_from_slice(&geometry.degree().to_le_bytes());
    record.extend_from_slice(&(geometry.fit_tolerance().get() / LEN_TO_MM).to_le_bytes());
    let knot_count = u32::try_from(knots.len()).map_err(|_| {
        crate::writer::WriteError::Unwritable("sketch NURBS has too many knots".into())
    })?;
    record.extend_from_slice(&knot_count.to_le_bytes());
    record.extend_from_slice(&knot_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    encode_f64_sequence(record, knots);
    let weight_count = u32::try_from(poles.weights().len()).map_err(|_| {
        crate::writer::WriteError::Unwritable("sketch NURBS has too many weights".into())
    })?;
    record.extend_from_slice(&weight_count.to_le_bytes());
    record.extend_from_slice(&weight_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    encode_f64_sequence(record, &poles.weights().collect::<Vec<_>>());
    let point_count = u32::try_from(poles.point_count()).map_err(|_| {
        crate::writer::WriteError::Unwritable("sketch NURBS has too many control points".into())
    })?;
    record.extend_from_slice(&point_count.to_le_bytes());
    record.extend_from_slice(&point_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    let coordinates = poles
        .points()
        .flat_map(|point| {
            [
                point.x / LEN_TO_MM,
                point.y / LEN_TO_MM,
                point.z / LEN_TO_MM,
            ]
        })
        .collect::<Vec<_>>();
    encode_f64_sequence(record, &coordinates);
    Ok(())
}

pub(in crate::writer) fn sketch_text_supported(text: &SketchText) -> bool {
    use crate::records::sketch_geometry::SketchTextLayout;
    let SketchTextLayout::TextexTag {
        alignment: Some(_),
        first_reference: None,
        second_reference: None,
        placement: Some(placement),
        ..
    } = &text.layout
    else {
        return false;
    };
    let anchor = placement.anchor.get();
    let rotation = placement.rotation.get();
    let (sine, cosine) = rotation.sin_cos();
    sine.atan2(cosine) == rotation
        && ![anchor.u, anchor.v, text.height.get()]
            .into_iter()
            .any(|value| value != 0.0 && value / LEN_TO_MM == 0.0)
        && !text.font_family.is_empty()
        && text.font_family.encode_utf16().count() <= 1024
        && !text.text.is_empty()
        && text.text.encode_utf16().count() <= 1_048_576
        && text.persistent_id.is_some()
}

pub(super) fn encode_sketch_text(
    out: &mut Vec<u8>,
    text: &SketchText,
) -> Result<(), crate::writer::WriteError> {
    use crate::records::sketch_geometry::SketchTextLayout;
    if !sketch_text_supported(text) {
        return Err(crate::writer::WriteError::Unsupported(format!(
            "sketch text {} has unsupported F3D metrics or identity",
            text.id,
        )));
    }
    let SketchTextLayout::TextexTag {
        width_factor,
        alignment: Some(alignment),
        placement: Some(placement),
        ..
    } = &text.layout
    else {
        return Err(crate::writer::WriteError::Unsupported(format!(
            "sketch text {} has no supported F3D member layout",
            text.id,
        )));
    };
    let anchor = placement.anchor.get();
    let height = text.height.get() / LEN_TO_MM;
    let (sine, cosine) = placement.rotation.get().sin_cos();
    native_lp_ascii(out, text.class_tag.as_str())?;
    out.extend_from_slice(&u64::from(text.record_index).to_le_bytes());
    native_lp_ascii(out, "")?;
    out.push(0);
    let properties = [
        ("EntityGenesis", text.entity_genesis),
        ("textex_tag", text.persistent_id),
        ("txt_tag_base", text.base_id),
    ];
    out.push(1);
    let count = properties
        .iter()
        .filter(|(_, value)| value.is_some())
        .count();
    out.extend_from_slice(
        &u32::try_from(count)
            .map_err(crate::writer::WriteError::unwritable)?
            .to_le_bytes(),
    );
    for (name, value) in properties {
        if let Some(value) = value {
            native_lp_ascii(out, name)?;
            native_lp_ascii(out, "IntrinsicMetaTypeuint64")?;
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.push(1);
    out.extend_from_slice(&width_factor.get().to_le_bytes());
    for value in [
        text.color.r(),
        text.color.g(),
        text.color.b(),
        text.color.a(),
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    native_lp_utf16(out, &text.font_family)?;
    out.push(0);
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&alignment.horizontal.to_le_bytes());
    out.extend_from_slice(&[0; 3]);
    native_lp_utf16(out, &text.text)?;
    out.extend_from_slice(&alignment.vertical.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&text.font_weight.to_le_bytes());
    out.extend_from_slice(&[0; 5]);
    for row in [
        [cosine, -sine, 0.0, anchor.u / LEN_TO_MM],
        [sine, cosine, 0.0, anchor.v / LEN_TO_MM],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ] {
        for value in row {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&[0, 0, 0, 0, 1]);
    for value in [0.0f32, 0.0, 0.0, 1.0, 1.0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&[0; 5]);
    write_reference(out, text.owner_reference);
    Ok(())
}

/// Least sketch-relation record the Design `BulkStream` writer emits. A
/// relation record ends at its trailing zero byte, and the reader requires
/// every byte from there to the next indexed-record header to be zero, so the
/// filler carries no field. It is a fixed width of the writer's own and bounds
/// no value the IR states.
const SKETCH_RELATION_RECORD_FILLED_LEN: usize = 101;

fn encode_sketch_relation(
    out: &mut Vec<u8>,
    relation: &crate::records::sketch_relations::SketchRelation,
) -> Result<(), crate::writer::WriteError> {
    let mut record = vec![0u8; 19];
    encode_sketch_record_header(&mut record, &relation.class_tag, relation.record_index);
    record.push(1);
    let member_count = u32::try_from(relation.members().len()).map_err(|_| {
        crate::writer::WriteError::Unwritable(
            "sketch relation members: the Design record states a u32 member count".into(),
        )
    })?;
    record.extend_from_slice(&member_count.to_le_bytes());
    for member in relation.members().iter() {
        write_reference(&mut record, member.reference.record_index());
        // The member's ordinal is a stated u32 in the record, and `0` is a
        // stated ordinal there, not an absent one. A member that retains none
        // has no bytes to write.
        let relation_ordinal = member.relation_ordinal.ok_or_else(|| {
            crate::writer::WriteError::Unwritable(
                "sketch relation member relation_ordinal: the Design record states a u32 \
                 ordinal for every member, and this member retains none"
                    .into(),
            )
        })?;
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
        .auxiliary_references()
        .values()
        .chain(std::iter::once(&relation.owner_reference))
    {
        write_reference(&mut record, *reference);
    }
    record.extend_from_slice(&relation.definition.state().to_le_bytes());
    let return_count = u32::try_from(relation.return_members().len()).map_err(|_| {
        crate::writer::WriteError::Unwritable(
            "sketch relation return_members: the Design record states a u32 return count".into(),
        )
    })?;
    record.extend_from_slice(&return_count.to_le_bytes());
    for member in relation.return_members().iter() {
        write_reference(&mut record, member.reference.record_index());
    }
    record.push(0);
    if record.len() < SKETCH_RELATION_RECORD_FILLED_LEN {
        zero_fill_to(&mut record, SKETCH_RELATION_RECORD_FILLED_LEN);
    }
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

pub(super) fn encode_design_metastream(
    registry: &GeneratedDesignRegistry,
    primary_records: &[crate::metastream::RecordIndexEntry],
    secondary_records: &[crate::metastream::RecordIndexEntry],
) -> Result<Option<Vec<u8>>, crate::writer::WriteError> {
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
        crate::writer::WriteError::Unwritable(
            "Design MetaStream registers more than u32::MAX types".into(),
        )
    })?;
    out.extend_from_slice(&type_count.to_le_bytes());
    let mut next_entity_id = 1u64;
    for design_type in &registry.types {
        native_lp_ascii(&mut out, design_type.type_guid.as_str())?;
        match &design_type.base_type_guid {
            Some(base) => {
                native_lp_ascii(&mut out, base.as_str())?;
            }
            None => out.extend_from_slice(&0u32.to_le_bytes()),
        }
        out.extend_from_slice(&design_type.version.to_le_bytes());
        if crate::bytes::is_guid_relaxed(&design_type.module) {
            return Err(crate::writer::WriteError::Unwritable(format!(
                "Design type module name is GUID-shaped: {}",
                design_type.module
            )));
        }
        native_lp_ascii(&mut out, &design_type.module)?;
        let count = u32::try_from(design_type.entity_ids.len()).map_err(|_| {
            crate::writer::WriteError::Unwritable(
                "Design type owns more than u32::MAX entities".into(),
            )
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for entity_id in &design_type.entity_ids {
            out.extend_from_slice(&entity_id.to_le_bytes());
            let next = entity_id.checked_add(1).ok_or_else(|| {
                crate::writer::WriteError::Unwritable(
                    "generated Design next entity id overflows".into(),
                )
            })?;
            if next > next_entity_id {
                next_entity_id = next;
            }
        }
    }
    // Named-entity list, primary record index, and secondary index.
    out.extend_from_slice(&0u32.to_le_bytes());
    let record_count = u32::try_from(primary_records.len()).map_err(|_| {
        crate::writer::WriteError::Unwritable(
            "Design primary index exceeds u32::MAX records".into(),
        )
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
            return Err(crate::writer::WriteError::Unwritable(format!(
                "generated Design primary entity {} has no type registration",
                record.entity_id
            )));
        }
        if !indexed_entities.insert(record.entity_id) {
            return Err(crate::writer::WriteError::Unwritable(format!(
                "generated Design primary entity {} is indexed more than once",
                record.entity_id
            )));
        }
        if previous_offset.is_some_and(|previous| previous >= record.bulk_offset) {
            return Err(crate::writer::WriteError::Unwritable(
                "generated Design primary offsets are not strictly increasing".into(),
            ));
        }
        previous_offset = Some(record.bulk_offset);
        out.extend_from_slice(&record.entity_id.to_le_bytes());
        out.extend_from_slice(&record.bulk_offset.to_le_bytes());
    }
    out.extend_from_slice(
        &u32::try_from(secondary_records.len())
            .map_err(|_| {
                crate::writer::WriteError::Unwritable(
                    "Design secondary index exceeds u32::MAX records".into(),
                )
            })?
            .to_le_bytes(),
    );
    for record in secondary_records {
        out.extend_from_slice(&record.entity_id.to_le_bytes());
        out.extend_from_slice(&record.bulk_offset.to_le_bytes());
    }
    out.extend_from_slice(&next_entity_id.to_le_bytes());
    // Trailing flag and empty property block.
    out.extend_from_slice(&[0; 8]);
    Ok(Some(out))
}

fn encode_browser_nodes(
    output_ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut Vec<u8>,
    primary_records: &mut Vec<crate::metastream::RecordIndexEntry>,
    registry: &GeneratedDesignRegistry,
) -> Result<(), crate::writer::WriteError> {
    let Some(browser_nodes) = &registry.browser_nodes else {
        return Ok(());
    };
    let node_class_tag = browser_nodes.class_tag.as_str();
    for node in &browser_nodes.nodes {
        primary_records.push(primary_record(output_ctx, node.record_index, out.len())?);
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

pub(super) fn native_lp_ascii(
    out: &mut Vec<u8>,
    value: &str,
) -> Result<(), crate::writer::WriteError> {
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(crate::writer::WriteError::Unwritable(
            "Design MetaStream strings must contain printable ASCII".into(),
        ));
    }
    let length = u32::try_from(value.len()).map_err(|_| {
        crate::writer::WriteError::Unwritable("Design MetaStream string exceeds u32::MAX".into())
    })?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

pub(super) fn native_lp_utf16(
    out: &mut Vec<u8>,
    value: &str,
) -> Result<(), crate::writer::WriteError> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    let length = u32::try_from(units.len()).map_err(|_| {
        crate::writer::WriteError::Unwritable("Design UTF-16 string exceeds u32::MAX".into())
    })?;
    out.extend_from_slice(&length.to_le_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Hand-built relations assert the written bytes; the drafts are valid by construction.
    #![allow(clippy::unwrap_used)]
    use super::{encode_sketch_relation, SKETCH_RELATION_RECORD_FILLED_LEN};
    use crate::records::{
        identity::ReferenceRun,
        references::DesignClassTag,
        sketch_relations::{
            SketchRelation, SketchRelationContent, SketchRelationDefinition, SketchRelationMember,
            SketchRelationReturnMember,
        },
    };

    #[test]
    fn design_records_share_the_output_allowance() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_entities = 2;
        let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
        ctx.charge_entities(1, "earlier output records").unwrap();
        let first = super::primary_record(&ctx, 17, 3).unwrap();
        assert_eq!((first.entity_id, first.bulk_offset), (17, 3));
        let error = super::primary_record(&ctx, 18, 7).unwrap_err();
        assert!(
            matches!(error, crate::writer::WriteError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Entities
                && (limit.limit, limit.used, limit.additional) == (2, 2, 1))
        );
    }

    fn registry_with_entity(entity_id: u64) -> super::GeneratedDesignRegistry {
        super::GeneratedDesignRegistry {
            types: vec![super::super::presentation::GeneratedDesignType {
                type_guid: "11111111-2222-3333-4444-555555555555"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                base_type_guid: None,
                version: 1,
                module: "Fusion".into(),
                entity_ids: vec![entity_id],
            }],
            body_map: None,
            browser_nodes: None,
        }
    }

    #[test]
    fn neutral_text_members_survive_typed_serialization() {
        let text = crate::test_support::sketch_text::neutral_text_record();
        let mut bytes = Vec::new();
        super::encode_sketch_text(&mut bytes, &text).unwrap();
        let decoded = crate::design::decode::sketch::decode_sketch_text_record(
            &bytes,
            "Design/BulkStream.dat",
            text.class_tag.clone(),
            text.class_version,
            text.record_index,
            0,
        )
        .unwrap()
        .expect("typed text record");
        assert_eq!(decoded.text, text.text);
        assert_eq!(decoded.font_family, text.font_family);
        assert_eq!(decoded.font_weight, text.font_weight);
        assert_eq!(decoded.height, text.height);
        assert_eq!(decoded.color, text.color);
        assert_eq!(decoded.layout, text.layout);
        assert_eq!(decoded.owner_reference, text.owner_reference);
        assert_eq!(decoded.persistent_id, text.persistent_id);
    }

    #[test]
    fn metastream_rejects_overflowed_next_entity_id() {
        let error = super::encode_design_metastream(&registry_with_entity(u64::MAX), &[], &[])
            .expect_err("the next identity cannot be represented");
        assert!(
            matches!(error, crate::writer::WriteError::Unwritable(ref message)
            if message == "generated Design next entity id overflows")
        );
    }

    #[test]
    fn metastream_preserves_largest_representable_next_entity_id() {
        let bytes = super::encode_design_metastream(&registry_with_entity(u64::MAX - 1), &[], &[])
            .unwrap()
            .unwrap();
        let next_offset = bytes.len() - 16;
        assert_eq!(
            cadmpeg_core::decode::View::u64_le_at(&bytes, next_offset),
            Some(u64::MAX)
        );
    }

    /// One member whose ordinal the wire states, which is what the writer
    /// needs: `SketchRelationMember::from_index` retains none.
    fn stated_member(record_index: u32) -> SketchRelationMember {
        SketchRelationMember {
            relation_ordinal: Some(0),
            ..SketchRelationMember::from_index(record_index)
        }
    }

    fn relation(member_count: u32) -> SketchRelation {
        let members = (1..=member_count).collect::<Vec<_>>();
        SketchRelation::authored(SketchRelationContent {
            id: "f3d:native:sketch-relation#filler".into(),
            record_index: 10,
            class_tag: DesignClassTag::try_from("300".to_owned()).unwrap(),
            owner_reference: 1,
            owner_entity_id: None,
            auxiliary_references: ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: members
                .iter()
                .copied()
                .map(stated_member)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            definition: SketchRelationDefinition::new(1, None).unwrap(),
            entity_genesis: None,
            return_members: members
                .iter()
                .copied()
                .map(SketchRelationReturnMember::from_index)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        })
    }

    fn encoded(member_count: u32) -> Vec<u8> {
        let mut out = Vec::new();
        encode_sketch_relation(&mut out, &relation(member_count)).unwrap();
        out
    }

    #[test]
    fn the_relation_record_filler_is_a_writer_width_and_never_bounds_a_stated_count() {
        // A short relation is filled to the writer's own width with the zero
        // bytes the reader requires between the record's trailing zero and the
        // next indexed-record header.
        let short = encoded(1);
        assert_eq!(short.len(), SKETCH_RELATION_RECORD_FILLED_LEN);

        // Every byte the relation itself does not state is zero: take the same
        // record one member longer and the shared prefix is identical up to
        // the point the counts differ, with zeros beyond each content end.
        let content_end = short
            .iter()
            .rposition(|byte| *byte != 0)
            .expect("the record states nonzero bytes")
            + 1;
        assert!(content_end < SKETCH_RELATION_RECORD_FILLED_LEN);
        assert!(short[content_end..].iter().all(|byte| *byte == 0));

        // The stated member count reaches the bytes unfloored, and a relation
        // whose own content passes the filler width is written whole: the
        // width is a floor on the record, never a ceiling on a stated count.
        assert_eq!(
            u32::from_le_bytes(short[20..24].try_into().unwrap()),
            1,
            "the member count the relation states"
        );
        let long = encoded(6);
        assert!(long.len() > SKETCH_RELATION_RECORD_FILLED_LEN);
        assert_eq!(u32::from_le_bytes(long[20..24].try_into().unwrap()), 6);
        let longer = encoded(7);
        assert!(longer.len() > long.len());
        assert_eq!(u32::from_le_bytes(longer[20..24].try_into().unwrap()), 7);
    }
}

#[cfg(test)]
mod relation_ordinal_tests {
    #![allow(clippy::unwrap_used)]
    use super::encode_sketch_relation;
    use crate::records::{
        identity::ReferenceRun,
        references::DesignClassTag,
        sketch_relations::{
            SketchRelation, SketchRelationContent, SketchRelationDefinition, SketchRelationMember,
            SketchRelationReturnMember,
        },
    };

    /// The record states a u32 ordinal for every member, so a member that
    /// retains none has no bytes to write. Writing `0` there would state the
    /// ordinal `0`, which the reader reads back as a stated ordinal.
    #[test]
    fn a_member_with_no_retained_ordinal_is_refused_by_name() {
        let content = |relation_ordinal: Option<u32>| SketchRelationContent {
            id: "f3d:native:sketch-relation#ordinal".into(),
            record_index: 10,
            class_tag: DesignClassTag::try_from("300".to_owned()).unwrap(),
            owner_reference: 1,
            owner_entity_id: None,
            auxiliary_references: ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: vec![SketchRelationMember {
                relation_ordinal,
                ..SketchRelationMember::from_index(1)
            }]
            .try_into()
            .unwrap(),
            definition: SketchRelationDefinition::new(1, None).unwrap(),
            entity_genesis: None,
            return_members: vec![SketchRelationReturnMember::from_index(1)]
                .try_into()
                .unwrap(),
        };

        let mut out = Vec::new();
        encode_sketch_relation(&mut out, &SketchRelation::authored(content(Some(0))))
            .expect("a stated ordinal writes");

        let mut out = Vec::new();
        let error = encode_sketch_relation(&mut out, &SketchRelation::authored(content(None)))
            .expect_err("a member that retains no ordinal has no bytes to write");
        assert!(
            error.to_string().contains("relation_ordinal"),
            "the refusal names the field: {error}"
        );
    }
}
