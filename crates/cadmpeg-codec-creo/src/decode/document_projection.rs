// SPDX-License-Identifier: Apache-2.0
//! Neutral resources and configurations from decoded PSB declarations.

use crate::{
    container::{self, ContainerScan, FamilyTablePointer, SectionRole},
    legacy::StringValue,
    legacy_family::FamilyTableValuePayload,
    loss::CreoLossCode,
};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    assets::{Asset, AssetContent, AssetData, AssetId},
    document::CadIr,
    features::{ConfigurationId, DesignConfiguration, Expression},
    report::loss::LossNote,
};

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let before = ir.model.entity_count();
    for section in &scan.framing.sections {
        if section.role() == SectionRole::Thumbnail {
            let bytes = container::expanded_section_for(scan, section)
                .map(|expanded| expanded.data.as_ref())
                .or_else(|| container::section_region(&scan.framing.data, section));
            let Some(bytes) = bytes else {
                continue;
            };
            let Some(start) = bytes
                .windows(3)
                .position(|bytes| bytes == container::JPEG_MAGIC)
            else {
                losses.push(CreoLossCode::NeutralRecordUnprojected.note(format!(
                    "Thumbnail section {} has no decoded JPEG resource.",
                    section.name()
                )));
                continue;
            };
            let data = bytes[start..].to_vec();
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(data.len()),
                "admit Creo JPEG asset",
            )?;
            ir.model.assets.push(
                Asset::try_new(
                    AssetId::compose(
                        &cadmpeg_ir::identity_namespace!("creo", "document", "asset"),
                        cadmpeg_ir::identity_key!("thumbnail-").then(section.offset()),
                    ),
                    Some(section.name().to_owned()),
                    Some("image/jpeg".into()),
                    AssetContent::Embedded {
                        data: AssetData::new(data)
                            .ok_or_else(|| CodecError::malformed("empty Creo thumbnail"))?,
                    },
                    None,
                )
                .map_err(CodecError::Malformed)?,
            );
        }
        if matches!(section.name(), "NeuPrtSld" | "NeuAsmSld") {
            losses.push(CreoLossCode::NeutralRecordUnprojected.note(format!("Material/display section {} has no decoded material fields from which to construct neutral appearances; its source record is retained.", section.name())));
        }
    }
    if let Some(table) = &scan.framing.legacy_family_table {
        for (ordinal, instance) in table.instances.iter().enumerate() {
            let mut overrides = std::collections::BTreeMap::new();
            for (column, cell) in table.items.iter().zip(&instance.values) {
                let StringValue::Utf8 { text: name } = &column.name else {
                    losses.push(CreoLossCode::NeutralRecordUnprojected.note(format!(
                        "Family-table cell {} has no decoded column name.",
                        cell.source_object_id
                    )));
                    continue;
                };
                let matches = ir
                    .model
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.name.eq_ignore_ascii_case(name))
                    .collect::<Vec<_>>();
                let [parameter] = matches.as_slice() else {
                    losses.push(CreoLossCode::NeutralRecordUnprojected.note(format!(
                        "Family-table cell {} column {name:?} has no unique neutral parameter.",
                        cell.source_object_id
                    )));
                    continue;
                };
                let text = match &cell.value {
                    FamilyTableValuePayload::Real { value } => value.value().to_string(),
                    FamilyTableValuePayload::Integer { value } => value.to_string(),
                    FamilyTableValuePayload::String {
                        value: StringValue::Utf8 { text },
                    } => serde_json::to_string(text)
                        .map_err(|error| CodecError::malformed(error.to_string()))?,
                    FamilyTableValuePayload::String { .. } => {
                        losses.push(CreoLossCode::NeutralRecordUnprojected.note(format!(
                            "Family-table cell {} has an undecoded string encoding.",
                            cell.source_object_id
                        )));
                        continue;
                    }
                };
                overrides.insert(parameter.id.clone(), Expression::text(text));
            }
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId::compose(
                    &cadmpeg_ir::identity_namespace!("creo", "document", "configuration"),
                    cadmpeg_ir::identity_key!("family-").then(instance.offset),
                ),
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| CodecError::malformed("Creo configuration ordinal exceeds u32"))?,
                active: false,
                source_index: None,
                name: Some(instance.name.clone()),
                material: None,
                properties: std::collections::BTreeMap::new(),
                parameter_overrides: overrides,
                bodies: None,
                parameter_values: std::collections::BTreeMap::new(),
                feature_states: std::collections::BTreeMap::new(),
                native_ref: Some(format!("creo:legacy_ascii:object#{}", instance.offset)),
            });
        }
    } else if let Some(table) = scan
        .framing
        .family_table
        .filter(|table| matches!(table.pointer, FamilyTablePointer::Entity(_)))
    {
        losses.push(CreoLossCode::NeutralRecordUnprojected.note(format!(
            "FamilyInf driver-table reference {:?} has no decoded configuration rows.",
            table.pointer
        )));
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count() - before),
        "admit Creo document projections",
    )?;
    Ok(())
}
