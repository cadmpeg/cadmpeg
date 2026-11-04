// SPDX-License-Identifier: Apache-2.0
//! Reporting for unhandled neutral records.

use super::*;

#[test]
fn writer_reports_unhandled_neutral_arenas_and_product_metadata() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    ir.model.assets.push(
        cadmpeg_ir::assets::Asset::try_new(
            cadmpeg_ir::assets::AssetId::mint("test:model:asset#texture")
                .expect("identity grammar"),
            Some("texture".into()),
            Some("image/png".into()),
            cadmpeg_ir::assets::AssetContent::External {
                uri: cadmpeg_core::text::NonBlankString::new("urn:test:texture")
                    .expect("nonempty uri"),
            },
            None,
        )
        .expect("valid asset"),
    );
    ir.model
        .semantic_annotations
        .push(cadmpeg_ir::semantic_annotations::SemanticAnnotation {
            text: vec!["inspection note".into()],
            references: std::collections::BTreeMap::new(),
            value: None,
            format: None,
            position: None,
            parameters: std::collections::BTreeMap::new(),
            assets: Vec::new(),
            ..cadmpeg_ir::semantic_annotations::SemanticAnnotation::new(
                cadmpeg_ir::semantic_annotations::SemanticAnnotationId::mint(
                    "test:model:semantic#note",
                )
                .expect("identity grammar"),
                "note".into(),
                cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text,
                "TextNote".into(),
                0,
                "native-note".into(),
            )
        });
    let mut bom_properties = std::collections::BTreeMap::new();
    bom_properties.insert(cadmpeg_core::nonblank_literal!("stock_code"), "A-1".into());
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            sketches: Vec::default(),
            id: "test:model:product#group"
                .try_into()
                .expect("valid identity"),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Group,
            source_name: Some("Group".into()),
            label: Some("Group".into()),
            description: None,
            part_number: None,
            bom_properties,
            bodies: Vec::new(),
            native_ref: None,
        });

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report mode writes representable geometry");
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::DocumentAssetOmitted.kind()
            && loss.message.contains("1 document asset")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::SemanticAnnotationOmitted.kind()
            && loss.message.contains("1 semantic annotation")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::ProductNonPartKind.kind()
            && loss.message.contains("non-part kind")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.code == StepLossCode::ProductBomPropertyOmitted.kind()
            && loss.message.contains("1 product BOM property")
    }));
}
