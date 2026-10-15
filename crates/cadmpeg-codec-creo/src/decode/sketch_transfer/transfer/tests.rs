// SPDX-License-Identifier: Apache-2.0

use super::{admit_constraint_row, available_parameter_ids, emitted_entity_views};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
};
use std::collections::BTreeSet;

fn empty_section_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(7),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: Some(crate::feature::definitions::FeatureSavedSection {
                entities: Vec::new(),
                offset: 0,
            }),
            offset: 0,
        });
    scan
}

#[test]
fn empty_section_transfer_preserves_sketch_and_feature() {
    let scan = empty_section_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::decode::with_service_context(|ctx| {
        super::transfer_sketches(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("service section transfer");
    assert_eq!(ir.model.sketches.len(), 1);
    assert_eq!(ir.model.features.len(), 1);
    assert_eq!(ir.model.features[0].source_tag.as_deref(), Some("section"));
}

#[test]
fn sketch_native_reference_names_featdefs_sketch() {
    let sketch = SketchId::mint("creo:model:sketch#7").expect("valid sketch ID");
    assert_eq!(
        crate::decode::sketch_ids::sketch_native_ref_admitted(&sketch),
        "creo:featdefs:sketch#7"
    );
}

fn disabled_constraint() -> SketchConstraint {
    SketchConstraint {
        id: SketchConstraintId::mint("creo:model:sketch_constraint#1").expect("constraint id"),
        sketch: SketchId::mint("creo:model:sketch#1").expect("sketch id"),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Disabled {},
        )
        .expect("disabled constraint"),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    }
}

#[test]
fn admit_constraint_row_appends_one_row() {
    let mut rows = Vec::new();
    crate::decode::with_service_context(|ctx| {
        admit_constraint_row(ctx, &mut rows, disabled_constraint())
    })
    .expect("service row admitted");
    assert_eq!(rows.len(), 1);
}

fn fixture() -> SketchEntity {
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch id");
    let entity = SketchEntityId::mint("creo:model:sketch_entity#1").expect("entity id");
    SketchEntity::new(
        entity,
        sketch,
        SketchGeometry::native(NonBlankString::new("native").expect("kind")),
    )
}

#[test]
fn emitted_entity_views_hold_one_geometry() {
    let (_, geometry) = emitted_entity_views(&[fixture()]);
    assert_eq!(geometry.len(), 1);
}

#[test]
fn available_parameter_ids_include_existing_ids() {
    let id = ParameterId::mint("creo:featdefs:parameter#1").expect("parameter ID");
    let ids = { available_parameter_ids([&id], BTreeSet::new()) };
    assert_eq!(ids, BTreeSet::from([id]));
}

#[test]
fn available_parameter_ids_include_planned_ids() {
    let id = ParameterId::mint("creo:featdefs:parameter#1").expect("parameter ID");
    let ids = { available_parameter_ids(std::iter::empty(), BTreeSet::from([id.clone()])) };
    assert_eq!(ids, BTreeSet::from([id]));
}

fn equation_scan() -> (crate::container::ContainerScan<'static>, usize) {
    let mut scan = empty_section_scan();
    let definition = &mut scan.features.definitions[0];
    definition.offset = 1000;
    definition.saved_section = None;
    definition.body = vec![0; 20];
    definition.body.extend_from_slice(b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\xe0\x01id\0\x00\xe0\x05fcn_id\0\x02\xe0\x08arg_arr\0\xf8\x02\x11\x12\xe0\x01aux_data\0\xf6\xf1\xf7\x80\x9f\xe2");
    let row_start = definition.body.len();
    definition
        .body
        .extend_from_slice(b"\x01\x04\x11\x12\xf6\xf2\xf7\x39\x99\x88\xe0\x02scale\0\x99\x88");
    (scan, row_start)
}

#[test]
fn equation_header_uses_definition_source_base() {
    let (scan, _) = equation_scan();
    let headers =
        { crate::decode::sketch_ids::sketch_table_headers(&scan.features.definitions[0]) }
            .expect("headers");
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].offset, 1020);
}

#[test]
fn native_equation_row_uses_definition_source_base() {
    let (scan, row_start) = equation_scan();
    crate::decode::with_service_context(|ctx| {
        let records = crate::decode::records::sketch_records(ctx, &scan).expect("records");
        let record = serde_json::to_value(&records[0]).expect("native record");
        assert_eq!(record["equations"].as_array().expect("equations").len(), 1);
        assert_eq!(record["equations"][0]["offset"], 1000 + row_start);
    });
}

#[test]
fn equation_annotation_uses_definition_source_base() {
    let (scan, row_start) = equation_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::decode::with_service_context(|ctx| {
        super::transfer_sketches(
            ctx,
            &scan,
            &mut ir,
            &mut annotations,
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("transfer");
    let annotations = annotations.build();
    let equations = annotations
        .provenance
        .values()
        .filter(|note| note.tag.as_deref() == Some("section_native_equation_constraint"))
        .collect::<Vec<_>>();
    assert_eq!(equations.len(), 1);
    assert_eq!(
        equations[0].offset,
        cadmpeg_core::decode::u64_from_index(1000 + row_start)
    );
}

#[test]
fn definition_body_position_refuses_out_of_bounds_and_source_overflow() {
    let (mut scan, _) = equation_scan();
    let definition = &mut scan.features.definitions[0];
    assert!(matches!(
        definition.body_position(definition.body.len()),
        Err(CodecError::Malformed(_))
    ));
    definition.offset = usize::MAX;
    assert!(matches!(
        definition.body_position(1).expect("body position").source(),
        Err(CodecError::Malformed(_))
    ));
}
