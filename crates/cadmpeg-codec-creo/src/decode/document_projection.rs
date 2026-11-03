// SPDX-License-Identifier: Apache-2.0
//! Neutral resources and configurations from decoded PSB declarations.

use crate::{
    container::{self, ContainerScan, SectionRole},
    legacy::StringValue,
    legacy_family::FamilyTableValuePayload,
    loss::CreoLossCode,
};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{
    assets::{Asset, AssetContent, AssetData, AssetId},
    document::CadIr,
    features::{ConfigurationId, DesignConfiguration, Expression},
    ids::UnknownId,
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
            let expanded = container::expanded_section_for(scan, section);
            let bytes = expanded
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
            // The retained thumbnail record mints its id from the same
            // namespace and offset as the passthrough retention, so the
            // asset names it. An unexpandable compressed thumbnail keeps its
            // whole section bytes at the section offset; an expanded one at
            // the expansion source offset; a raw one at the marker offset.
            // With no expansion and no retained record there is no home.
            let expansion_failed = scan
                .framing
                .expansion_losses
                .iter()
                .any(|loss| loss.section_offset == section.offset());
            let retained_offset = if expansion_failed {
                Some(section.offset())
            } else if let Some(expanded) = expanded {
                Some(expanded.source_offset)
            } else if container::section_region(&scan.framing.data, section).is_some() {
                Some(section.offset() + start)
            } else {
                None
            };
            let native_ref = retained_offset
                .map(|offset| {
                    if let Some(label) = scan
                        .framing
                        .incomplete_sections
                        .iter()
                        .find(|incomplete| {
                            incomplete.offset == section.offset() && incomplete.exclude_rows
                        })
                        .map(|incomplete| incomplete.label)
                    {
                        return crate::identity::part_stub_id(label, section.offset())
                            .map(|id| id.as_str().to_owned());
                    }
                    let namespace =
                        crate::identity::section_namespace(section.name()).ok_or_else(|| {
                            CodecError::malformed("thumbnail section without a namespace")
                        })?;
                    crate::identity::compose_checked::<UnknownId>(&namespace, offset)
                        .map(|id| id.as_str().to_owned())
                })
                .transpose()?;
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
                    native_ref,
                )
                .map_err(CodecError::Malformed)?,
            );
        }
        if matches!(section.name(), "NeuPrtSld" | "NeuAsmSld") {
            super::materials::project(ctx, scan, ir)?;
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
    } else if let Some(driver) = container::undecoded_driver_table_section(scan) {
        // The `drv_tbl_ptr` pointer is decoded into the typed
        // `creo:family_info:driver_table#root` record, but no grammar is
        // defined for the binary driver-table entity it names. The section
        // framing the entity is retained untyped (§1.3); the loss names it.
        let namespace = crate::identity::section_namespace(driver.name())
            .ok_or_else(|| CodecError::malformed("driver-table section without a namespace"))?;
        let retained = crate::identity::compose_checked::<UnknownId>(&namespace, driver.offset())
            .map(|id| id.as_str().to_owned())?;
        losses.push(CreoLossCode::ConfigurationDriverUnresolved.note(format!(
            "FamilyInf driver-table entity has no decoded grammar; its {} section bytes are retained untyped as {}.",
            driver.length(),
            retained
        )));
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count() - before),
        "admit Creo document projections",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::project;
    use cadmpeg_ir::features::{DesignParameter, Expression, ParameterId};

    fn named_length_parameter() -> DesignParameter {
        DesignParameter {
            product_definition: None,
            id: ParameterId::mint("creo:test:parameter#d0").expect("fixture value is valid"),
            owner: None,
            ordinal: 0,
            name: "d0".into(),
            expression: Expression::text("2.5"),
            value: None,
            driving: true,
            display: None,
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        }
    }

    #[test]
    fn legacy_family_table_projects_configurations() {
        let table = crate::legacy_family::parse(&crate::legacy_family::complete_table())
            .expect("complete family table");
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing.legacy_family_table = Some(table);
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        ir.model.parameters.push(named_length_parameter());
        let mut losses = Vec::new();
        crate::decode::with_service_context(|ctx| {
            project(ctx, &scan, &mut ir, &mut losses).expect("project configurations")
        });
        assert!(losses.is_empty(), "{losses:#?}");
        assert_eq!(ir.model.configurations.len(), 1);
        let configuration = &ir.model.configurations[0];
        assert_eq!(configuration.name.as_deref(), Some("SMALL"));
        assert!(!configuration.active);
        assert_eq!(
            configuration
                .parameter_overrides
                .get(&named_length_parameter().id),
            Some(&Expression::text("2.5"))
        );
        assert_eq!(
            configuration.native_ref.as_deref(),
            Some("creo:legacy_ascii:object#6")
        );
    }
}
