// SPDX-License-Identifier: Apache-2.0
//! Neutral homes for independently decoded CATIA document records.

use crate::{loss::CatiaLossCode, native::CatiaNative};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    assets::{Asset, AssetContent, AssetData, AssetId},
    document::CadIr,
    report::loss::LossNote,
};

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    native: &CatiaNative,
    losses: &mut Vec<LossNote>,
    source_untyped: &mut cadmpeg_ir::ArenaClasses,
) -> Result<(), CodecError> {
    let before = ir.model.entity_count();
    for reference in &native.external_references {
        losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!(
            "External-reference record {} identifies {}, but no occurrence placement is decoded; its typed reference remains held without inventing an identity transform.",
            reference.id, reference.target
        )));
    }
    for (ordinal, preview) in native.preview_images.iter().enumerate() {
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(preview.data.len()),
            "admit CATIA JPEG asset",
        )?;
        ir.model.assets.push(
            Asset::try_new(
                AssetId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "document", "asset"),
                    cadmpeg_ir::identity_key!("preview-").then(ordinal),
                ),
                Some("document preview".into()),
                Some("image/jpeg".into()),
                AssetContent::Embedded {
                    data: AssetData::new(preview.data.to_vec())
                        .ok_or_else(|| CodecError::malformed("empty CATIA JPEG preview"))?,
                },
                Some(preview.id.clone()),
            )
            .map_err(CodecError::Malformed)?,
        );
    }
    // Scalar value-block fields without a decoded design quantity still have
    // a neutral attribute home. Formula and color blocks are handled by their
    // dedicated projections and must not be copied into generic attributes.
    for (block_ordinal, block) in native.value_blocks.iter().enumerate() {
        if block
            .schema_selections
            .iter()
            .filter_map(|selection| selection.name())
            .any(|name| {
                let name = name.to_ascii_lowercase();
                name.contains("formula")
                    || name.contains("relation")
                    || name.contains("color")
                    || name.contains("rgb")
            })
        {
            continue;
        }
        for (field_ordinal, field) in block.fields().into_iter().enumerate() {
            let value = match field {
                crate::value_block::ValueField::Binary64 { bits, .. } => {
                    let Some(value) = cadmpeg_ir::scalar::FiniteReal::new(f64::from_bits(bits))
                    else {
                        losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!("Value block {} scalar {field_ordinal} is non-finite and has no neutral attribute value.", block.id)));
                        continue;
                    };
                    Some(cadmpeg_ir::attributes::AttributeValue::Float(value))
                }
                crate::value_block::ValueField::ByteString { bytes, .. } => {
                    match String::from_utf8(bytes.to_vec()) {
                        Ok(value) => Some(cadmpeg_ir::attributes::AttributeValue::String(value)),
                        Err(_) => {
                            source_untyped.insert(cadmpeg_ir::ArenaClass::Design);
                            losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!("Value block {} string {field_ordinal} has no decoded text encoding and no neutral attribute value.", block.id)));
                            None
                        }
                    }
                }
                _ => None,
            };
            if let Some(value) = value {
                ir.model
                    .attributes
                    .push(cadmpeg_ir::attributes::SourceAttribute {
                        native_ref: Some(block.id.clone()),
                        id: cadmpeg_ir::ids::AttributeId::compose(
                            &cadmpeg_ir::identity_namespace!("catia", "document", "attribute"),
                            cadmpeg_ir::identity_key!("value-")
                                .then(block_ordinal)
                                .dash(field_ordinal),
                        ),
                        target: cadmpeg_ir::attributes::AttributeTarget::Source {
                            source_id: block.id.clone(),
                        },
                        name: format!("value.{field_ordinal}"),
                        values: vec![value],
                    });
            }
        }
    }
    let held_features = ir
        .model
        .features
        .iter()
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<std::collections::HashSet<_>>();
    for object in &native.design_objects {
        if !held_features.contains(object.id.as_str()) {
            losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!(
                "Design object {} ({:?}) has no constructible neutral feature; its typed native record remains held.",
                object.id, object.owner_class
            )));
        }
    }
    if ir.model.pcurves.is_empty() {
        for pcurve in &native.consolidated_pcurves {
            losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!(
                "Consolidated pcurve {} has no neutral pcurve in this projection.",
                pcurve.id
            )));
        }
    }
    if ir.model.edges.is_empty() {
        for edge in &native.consolidated_edge_runs {
            losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!(
                "Consolidated edge run {} has no neutral edge in this projection.",
                edge.id
            )));
        }
    }
    for record in &native.zero_entity_records {
        losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!(
            "Zero-entity record {} has no independently constructible neutral record.",
            record.id
        )));
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count() - before),
        "admit CATIA document projections",
    )?;
    Ok(())
}
