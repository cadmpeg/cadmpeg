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
) -> Result<(), CodecError> {
    let before = ir.model.entity_count();
    for (ordinal, reference) in native.external_references.iter().enumerate() {
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("CATIA external occurrence ordinal exceeds u32"))?;
        let bytes = reference
            .id
            .len()
            .checked_add(reference.target.len())
            .ok_or_else(|| CodecError::malformed("CATIA external occurrence strings overflow"))?;
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes),
            "CATIA external occurrence strings",
        )?;
        ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
            id: cadmpeg_ir::ids::OccurrenceId::compose(
                &cadmpeg_ir::identity_namespace!("catia", "document", "occurrence"),
                cadmpeg_ir::identity_key!("external-").then(ordinal),
            ),
            prototype: cadmpeg_ir::products::PrototypeReference::External {
                document: cadmpeg_ir::products::ExternalDocument::path(reference.target.clone()),
                object: None,
            },
            parent: cadmpeg_ir::products::OccurrenceParent::Root {},
            ordinal,
            transform: None,
            linked_prototype: None,
            name: None,
            visible: None,
            link: None,
            native_ref: Some(reference.id.clone()),
        });
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
    // Each schema selection's encoded value routes by field kind. Colour
    // packets belong to the appearance transfer and formula-program tokens to
    // the formula transfer; every other scalar value projects as an attribute
    // on the first neutral record linked to its block. Selector framing never
    // projects: only selection values route.
    for (block_ordinal, block) in native.value_blocks.iter().enumerate() {
        for (selection_ordinal, selection) in block.schema_selections.iter().enumerate() {
            for (field_ordinal, field) in selection.encoded_value().iter().enumerate() {
                let value = match field {
                    crate::value_block::ValueField::Binary64 { bits, .. } => {
                        let Some(value) =
                            cadmpeg_ir::scalar::FiniteReal::new(f64::from_bits(*bits))
                        else {
                            losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!("Value block {} selection {selection_ordinal} scalar {field_ordinal} is non-finite and has no neutral attribute value.", block.id)));
                            continue;
                        };
                        Some(cadmpeg_ir::attributes::AttributeValue::Float(value))
                    }
                    crate::value_block::ValueField::ByteString { bytes, .. } => {
                        match String::from_utf8(bytes.to_vec()) {
                            Ok(value) => {
                                Some(cadmpeg_ir::attributes::AttributeValue::String(value))
                            }
                            // Non-UTF-8 strings keep their exact bytes in the
                            // retained native record; no loss and no untyped.
                            Err(_) => continue,
                        }
                    }
                    crate::value_block::ValueField::Atom { value, .. } => Some(
                        cadmpeg_ir::attributes::AttributeValue::Integer(i64::from(*value)),
                    ),
                    crate::value_block::ValueField::Literal { value, .. } => Some(
                        cadmpeg_ir::attributes::AttributeValue::Integer(i64::from(*value)),
                    ),
                    crate::value_block::ValueField::Inline { bytes, .. } => {
                        // Colour packets are appearances, chosen by packet
                        // kind. Any other inline value projects as an integer.
                        if is_color_packet(bytes.as_slice()) {
                            continue;
                        }
                        let mut padded = [0u8; 8];
                        padded[8 - bytes.as_slice().len()..].copy_from_slice(bytes.as_slice());
                        let Some(raw) = cadmpeg_core::decode::View::u64_be_at(&padded, 0) else {
                            continue;
                        };
                        let Some(value) = i64::try_from(raw).ok() else {
                            losses.push(CatiaLossCode::NeutralRecordUnprojected.note(format!("Value block {} selection {selection_ordinal} inline {field_ordinal} exceeds the neutral integer range.", block.id)));
                            continue;
                        };
                        Some(cadmpeg_ir::attributes::AttributeValue::Integer(value))
                    }
                    // Formula-program tokens structure value programs owned by
                    // the formula transfer; they are not attribute values.
                    _ => continue,
                };
                if let Some(value) = value {
                    let Some(target) = cadmpeg_ir::attributes::AttributeTarget::record_for(
                        &ir.model,
                        block.id.as_str(),
                    ) else {
                        continue;
                    };
                    ir.model
                        .attributes
                        .push(cadmpeg_ir::attributes::SourceAttribute::new(
                            ctx,
                            cadmpeg_ir::ids::AttributeId::compose(
                                &cadmpeg_ir::identity_namespace!("catia", "document", "attribute"),
                                cadmpeg_ir::identity_key!("value-")
                                    .then(block_ordinal)
                                    .dash(selection_ordinal)
                                    .dash(field_ordinal),
                            ),
                            Some(block.id.clone()),
                            target,
                            format!("value.{selection_ordinal}.{field_ordinal}"),
                            vec![value],
                        )?);
                }
            }
        }
    }
    // Consolidated pcurves and edge runs stay native: the neutral pcurve and
    // edge are occurrence-grouped rebuilds keyed by stream object id (one
    // object id yields several neutrals with derived ranges), not 1:1
    // projections of one consolidated record, so no single consolidated record
    // is the neutral record's source. The consolidated arenas stay native as
    // the typed source of the rebuild and charge no loss.
    //
    // Zero-entity records hold layout only, have no neutral home, stay native
    // and charge no loss.
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count() - before),
        "admit CATIA document projections",
    )?;
    Ok(())
}

/// Whether inline value bytes are a display-colour packet owned by the
/// appearance transfer: opaque RGB or RGBA, the same shapes `appearance`
/// admits.
fn is_color_packet(bytes: &[u8]) -> bool {
    matches!(bytes, [0x01, _, _, _] | [0x03, _, _, _, _])
}
