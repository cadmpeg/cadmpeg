// SPDX-License-Identifier: Apache-2.0
//! Unrepresented-content accounting charges every check it owns.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::export::write_step;
use crate::loss::StepLossCode;
use crate::{StepCodec, StepSchema, StepWriteOptions};

/// Charging dropped PMI does not end the accounting: content checked after
/// PMI on a target without semantic PMI is still charged.
#[test]
fn pmi_dropped_by_schema_does_not_skip_later_losses() {
    let mut ir = StepCodec::default()
        .decode(
            &mut Cursor::new(include_bytes!(
                "../../tests/fixtures/ap242_semantic_pmi.p21"
            )),
            &DecodeOptions::default(),
        )
        .expect("decode semantic PMI")
        .into_parts()
        .0;
    ir.native.namespace_mut("f3d").arenas_mut().insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new("f3d:test:asm-history#0").expect("valid identity"),
            serde_json::Map::default(),
        )
        .expect("valid native identity")],
    );

    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .expect("report-mode STEP write");
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::PmiAnnotationNotWritten.kind()));
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.message.contains("source-native record(s)")),
        "{:?}",
        report.losses
    );
}

fn product(ir: &mut cadmpeg_ir::CadIr) {
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            sketches: Vec::default(),

            tessellations: Vec::new(),
            id: "test:model:product-definition#empty".try_into().unwrap(),
            kind: Some(cadmpeg_ir::products::ProductDefinitionKind::Part),
            source_name: None,
            label: None,
            description: None,
            part_number: None,
            bom_properties: std::collections::BTreeMap::default(),
            bodies: Vec::new(),
            native_ref: None,
            generated_from: None,
            storage: cadmpeg_ir::products::Storage::Embedded,
            occurrences: Vec::new(),
            instance_bodies: Vec::new(),
            representations: Vec::new(),
            units: None,
            general_tolerance: None,
            active: None,
        });
}

#[test]
fn bodies_outside_products_have_a_counted_root_assignment_note() {
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    product(&mut ir);
    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == StepLossCode::BodyWithoutProductRepresentation.kind())
        .unwrap();
    assert_eq!(
        loss.message,
        "1 body record(s) not assigned to a component were written in the root product in model coordinates"
    );
    ir.model.product_definitions[0]
        .bodies
        .push(ir.geometry.bodies[0].id.clone());
    let report = write_step(
        &ir,
        &mut Vec::new(),
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == StepLossCode::BodyWithoutProductRepresentation.kind()));
}

#[test]
fn solid_and_sheet_shell_wire_members_have_counted_export_losses() {
    for kind in [
        cadmpeg_ir::topology::BodyKind::Solid,
        cadmpeg_ir::topology::BodyKind::Sheet,
    ] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        ir.geometry.bodies[0].kind = kind;
        let shell = &ir.geometry.shells[0];
        ir.geometry.shells[0] = cadmpeg_ir::topology::Shell::with_members(
            shell.id.clone(),
            shell.region.clone(),
            vec![ir.geometry.edges[0].id.clone()],
            vec![ir.geometry.vertices[0].id.clone()],
        );
        let report = write_step(
            &ir,
            &mut Vec::new(),
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap();
        let loss = report
            .losses
            .iter()
            .find(|loss| loss.code == StepLossCode::SurfaceShellWireTopologyOmitted.kind())
            .unwrap();
        assert!(loss
            .message
            .starts_with("1 wire edge record(s) and 1 free vertex record(s)"));
    }
}

#[test]
fn missing_local_product_refuses_occurrences_of_the_root_and_nested_definitions_before_output() {
    use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
    use cadmpeg_ir::transform::Transform;
    for nested in [false, true] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        product(&mut ir);
        let missing: cadmpeg_ir::ids::OccurrenceId =
            "test:model:occurrence#missing".try_into().unwrap();
        // The definition that owns the dangling occurrence is the root, or
        // a definition the root places.
        let owner = &mut ir.model.product_definitions[0];
        owner.occurrences.push(missing.clone());
        if nested {
            let placed: cadmpeg_ir::ids::OccurrenceId =
                "test:model:occurrence#placed".try_into().unwrap();
            let mut root = ProductDefinition::root(
                "test:model:product-definition#root".try_into().unwrap(),
                ExternalDocument::default(),
            );
            root.occurrences.push(placed.clone());
            ir.model.occurrences.push(Occurrence::new(
                placed,
                owner.id.clone(),
                Some(Transform::identity()),
            ));
            ir.model.product_definitions.push(root);
        } else {
            owner.storage = Storage::File {
                document: ExternalDocument::default(),
                source: None,
            };
        }
        ir.model.occurrences.push(Occurrence::new(
            missing,
            "test:model:product-definition#missing".try_into().unwrap(),
            Some(Transform::identity()),
        ));
        let mut output = vec![0xaa];
        let error = write_step(
            &ir,
            &mut output,
            StepSchema::Ap242Edition3,
            &StepWriteOptions::default(),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("1 occurrence record(s) whose local product definition is missing"));
        assert_eq!(output, [0xaa]);
    }
}
