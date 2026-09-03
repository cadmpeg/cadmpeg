// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]
//! Relation, decode, and projection unit tests for the design modules.

use crate::design::constraints::project_sketch_constraints;
use crate::design::decode::operands::has_typed_edge_treatment_group;
use crate::design::decode::parameters::parse_design_parameter;
use crate::design::decode::sketch::{bind_sketch_graph, identity_matrix};
use crate::design::edge_resolve::feature_input_topology_id;
use crate::design::feature_project::project_parameter_design;
use crate::design::sketch_project::project_sketch_design;
use crate::design::test_support::parameter_record;
use crate::design::{design_feature_family, is_localized_edge_treatment_kind, DesignFeatureFamily};
use crate::ids::{
    neutral_dimension_constraint_id, neutral_feature_id_parts, neutral_parameter_id_parts,
    neutral_sketch_curve_id, neutral_sketch_point_id,
};
use crate::records::{
    DesignEntityHeader, DesignSketchPlacement, SketchConstraintKind, SketchPoint, SketchRelation,
    DESIGN_MODULE_SKETCH,
};
use cadmpeg_ir::math::Point2;
use std::collections::HashSet;

#[test]
fn feature_family_tokens_are_localized() {
    assert_eq!(
        design_feature_family("As-built"),
        Some(DesignFeatureFamily::Assemble)
    );
    assert_eq!(
        design_feature_family("Esquisse"),
        Some(DesignFeatureFamily::Sketch)
    );
    assert_eq!(
        design_feature_family("Extrusion"),
        Some(DesignFeatureFamily::Extrude)
    );
    assert_eq!(
        design_feature_family("Extrusão"),
        Some(DesignFeatureFamily::Extrude)
    );
    for token in ["Skizze", "Esboço"] {
        assert_eq!(
            design_feature_family(token),
            Some(DesignFeatureFamily::Sketch)
        );
    }
    assert_eq!(
        design_feature_family("Congé"),
        Some(DesignFeatureFamily::Fillet)
    );
    for token in ["Abrundung", "Arredondamento"] {
        assert_eq!(
            design_feature_family(token),
            Some(DesignFeatureFamily::Fillet)
        );
        assert!(has_typed_edge_treatment_group(token));
    }
    assert_eq!(
        design_feature_family("Chanfrein"),
        Some(DesignFeatureFamily::Chamfer)
    );
    for token in ["Congé", "Abrundung", "Arredondamento", "Chanfrein"] {
        assert!(is_localized_edge_treatment_kind(token));
    }
    for token in ["Fillet", "Chamfer", "Extrusion", "unknown"] {
        assert!(!is_localized_edge_treatment_kind(token));
    }
    for token in ["C-Pattern", "Réseau C"] {
        assert_eq!(
            design_feature_family(token),
            Some(DesignFeatureFamily::CircularPattern)
        );
    }
    assert_eq!(
        design_feature_family("Symétrie miroir"),
        Some(DesignFeatureFamily::Mirror)
    );
    assert_eq!(
        design_feature_family("DécalerLesFaces"),
        Some(DesignFeatureFamily::OffsetFaces)
    );
    assert_eq!(
        design_feature_family("ReplaceFace"),
        Some(DesignFeatureFamily::ReplaceFace)
    );
    assert_eq!(
        design_feature_family("Schale"),
        Some(DesignFeatureFamily::Shell)
    );
    assert_eq!(
        design_feature_family("SpirePrimitive"),
        Some(DesignFeatureFamily::Coil)
    );
    assert_eq!(
        design_feature_family("Hem"),
        Some(DesignFeatureFamily::SheetMetalHem)
    );
    assert!(crate::design::decode::operands::has_edge_recipe_operands(
        "Hem"
    ));
    assert_eq!(
        design_feature_family("SurfacePatch"),
        Some(DesignFeatureFamily::SurfacePatch)
    );
    assert!(crate::design::decode::operands::has_edge_recipe_operands(
        "SurfacePatch"
    ));
    assert!(crate::design::decode::operands::has_edge_recipe_operands(
        "WorkPoint"
    ));
    assert_eq!(
        design_feature_family("SurfaceRuled"),
        Some(DesignFeatureFamily::SurfaceRuled)
    );
    assert_eq!(
        design_feature_family("BoundaryFill"),
        Some(DesignFeatureFamily::BoundaryFill)
    );
    assert_eq!(
        design_feature_family("SurfaceTrim"),
        Some(DesignFeatureFamily::SurfaceTrim)
    );
    assert_eq!(
        design_feature_family("Hole"),
        Some(DesignFeatureFamily::Hole)
    );
    assert_eq!(
        design_feature_family("Split"),
        Some(DesignFeatureFamily::Split)
    );
    assert_eq!(
        design_feature_family("Loft"),
        Some(DesignFeatureFamily::Loft)
    );
    assert_eq!(
        design_feature_family("Sweep"),
        Some(DesignFeatureFamily::Sweep)
    );
    assert_eq!(
        design_feature_family("Pipe"),
        Some(DesignFeatureFamily::Pipe)
    );
}

#[test]
fn feature_identity_uses_stream_family_ordinal_and_scope_record() {
    let first = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 41);
    let same = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 41);
    let different_stream = neutral_feature_id_parts("Design/A", "B:Kind:12", 3, 41);
    let different_family = neutral_feature_id_parts("Design/A:B", "Kind", 123, 41);
    let different_scope = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 42);

    assert_eq!(first, same);
    assert_ne!(first, different_stream);
    assert_ne!(first, different_family);
    assert_ne!(first, different_scope);

    let localized = neutral_feature_id_parts("Design Name", "Symétrie miroir", 1, 41);
    let literal_escape = neutral_feature_id_parts("Design%20Name", "Symétrie%20miroir", 1, 41);
    assert!(!localized.0.chars().any(char::is_whitespace));
    assert!(localized.0.contains("Design%20Name"));
    assert!(localized.0.contains("Symétrie%20miroir"));
    assert_ne!(localized, literal_escape);
    assert!(!feature_input_topology_id(&localized, 2)
        .0
        .chars()
        .any(char::is_whitespace));
}

#[test]
fn parameter_identity_uses_stream_and_native_record_index() {
    let first = neutral_parameter_id_parts("Design/A:12", 3);
    let same = neutral_parameter_id_parts("Design/A:12", 3);
    let different_stream = neutral_parameter_id_parts("Design/A", 123);
    let different_record = neutral_parameter_id_parts("Design/A:12", 4);

    assert_eq!(first, same);
    assert_ne!(first, different_stream);
    assert_ne!(first, different_record);
}

#[test]
fn parameter_identity_distinguishes_repeated_source_ordinals() {
    let mut first = parse_design_parameter(&parameter_record(
        Some(40),
        "1 cm",
        "AlongDistance",
        Some("cm"),
        "d9",
        1.0,
    ))
    .expect("first parameter");
    first.id = "f3d:Design/A:design-parameter#100".into();

    let mut second = first.clone();
    second.id = "f3d:Design/A:design-parameter#200".into();
    second.record_index = first.record_index + 1;

    assert_eq!(first.source_ordinal, second.source_ordinal);
    assert_ne!(
        crate::ids::neutral_parameter_id(&first),
        crate::ids::neutral_parameter_id(&second)
    );
}

#[test]
fn sketch_geometry_identity_uses_owner_and_native_persistent_ids() {
    use cadmpeg_ir::sketches::{SketchId, SpatialSketchId};

    let sketch = SketchId("f3d:model:sketch#Design/A@10".into());
    let other_sketch = SketchId("f3d:model:sketch#Design/A@11".into());
    let point = neutral_sketch_point_id(&sketch, 42);
    let same_point = neutral_sketch_point_id(&sketch, 42);
    let curve = neutral_sketch_curve_id(&sketch, 42, 0);
    let same_curve = neutral_sketch_curve_id(&sketch, 42, 0);

    assert_eq!(point, same_point);
    assert_eq!(curve, same_curve);
    assert_ne!(point, curve);
    assert_ne!(curve, neutral_sketch_curve_id(&sketch, 42, 1));
    assert_ne!(point, neutral_sketch_point_id(&other_sketch, 42));
    assert_ne!(curve, neutral_sketch_curve_id(&other_sketch, 42, 0));

    let spatial = SpatialSketchId("f3d:model:spatial-sketch#Design/A@10".into());
    let other_spatial = SpatialSketchId("f3d:model:spatial-sketch#Design/A@11".into());
    assert_ne!(
        crate::ids::neutral_spatial_sketch_point_id(&spatial, 42),
        crate::ids::neutral_spatial_sketch_point_id(&other_spatial, 42)
    );
    assert_ne!(
        crate::ids::neutral_spatial_sketch_curve_id(&spatial, 42, 0),
        crate::ids::neutral_spatial_sketch_curve_id(&other_spatial, 42, 0)
    );
}

#[test]
fn governing_dimension_identity_uses_parameter_identity() {
    let parameter = cadmpeg_ir::features::ParameterId("f3d:model:parameter#Design/A:12".into());
    let relocated = neutral_dimension_constraint_id(&parameter, "pair");
    let same = neutral_dimension_constraint_id(&parameter, "pair");
    let other_form = neutral_dimension_constraint_id(&parameter, "null-pair");
    let other_parameter = neutral_dimension_constraint_id(
        &cadmpeg_ir::features::ParameterId("parameter:Design/A".into()),
        "12:pair",
    );

    assert_eq!(relocated, same);
    assert_ne!(relocated, other_form);
    assert_ne!(relocated, other_parameter);
    assert_eq!(relocated.0.matches('#').count(), 1);
}

#[test]
fn design_streams_scope_sketch_graphs_identities_and_parameter_names() {
    let placement = |stream: &str| DesignSketchPlacement {
        member_run_head: false,
        id: format!("f3d:{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: format!("{stream}_100"),
        entity_suffix: 100,
        visibility: None,
        byte_offset: 0,
        class_tag: "356".into(),
        record_index: 11,
        frame_length: 201,
        transform: identity_matrix(),
        transform_offset: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: 201,
    };
    let header = |stream: &str| DesignEntityHeader {
        id: format!("f3d:{stream}:design-entity-header#0"),
        byte_offset: 0,
        entity_suffix: 100,
        entity_id: format!("{stream}_100"),
        class_tag: "300".into(),
        optional_slot_present: true,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: Some(1),
        reference_indices: vec![30],
        reference_offsets: vec![0],
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };
    let point = |stream: &str| SketchPoint {
        id: format!("f3d:{stream}:sketch-point#0"),
        record_index: 20,
        owner_reference: None,
        class_tag: "301".into(),
        byte_offset: 0,
        coordinate_offset: 89,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::default(),
        persistent_id: Some(20),
        paired_reference: 0,
        flags: [0; 8],
        coordinates: Point2::new(1.0, 2.0),
        depth: 0.0,
        closure: None,
        companion: None,
    };
    let relation = |stream: &str| SketchRelation {
        id: format!("f3d:{stream}:sketch-relation#30"),
        record_index: 30,
        class_tag: "302".into(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 100,
        owner_entity_id: String::new(),
        auxiliary_references: Vec::new(),
        auxiliary_reference_offsets: Vec::new(),
        rectangular_counted_reference_count: None,
        members: vec![20],
        resolved_members: Vec::new(),
        member_offsets: vec![0],
        owner_reference_offset: 0,
        state: 0,
        constraint_kinds: vec![SketchConstraintKind::Coincident],
        unknown_constraint_bits: 0,
        member_relation_ordinals: Vec::new(),
        entity_genesis: None,
        pattern: None,
        return_members: vec![20],
        resolved_return_members: Vec::new(),
        return_member_offsets: vec![0],
        raw_bytes: Vec::new(),
    };

    let placements = [placement("A"), placement("B")];
    let mut points = [point("A"), point("B")];
    let mut relations = [relation("A"), relation("B")];
    bind_sketch_graph(
        &[header("A"), header("B")],
        &mut points,
        &mut [],
        &mut [],
        &mut relations,
    )
    .expect("stream-local sketch graphs bind independently");
    assert_eq!(relations[0].owner_entity_id, "A_100");
    assert_eq!(relations[1].owner_entity_id, "B_100");

    let mut overflowing_header = header("A");
    overflowing_header.entity_suffix = u64::from(u32::MAX) + 101;
    overflowing_header.entity_id = "A_overflow".into();
    assert!(bind_sketch_graph(
        &[overflowing_header],
        &mut [point("A")],
        &mut [],
        &mut [],
        &mut [relation("A")],
    )
    .is_err());

    let (mut sketches, mut entities) =
        project_sketch_design(&placements, &points, &[], &[], &[], 1.0e-6);
    let mut constraints =
        project_sketch_constraints(&placements, &[], &points, &[], &[], &relations, &entities);
    assert_eq!(sketches.len(), 2);
    assert_eq!(entities.len(), 2);
    assert_eq!(constraints.len(), 2);
    assert_eq!(
        sketches
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        entities
            .iter()
            .map(|item| item.id())
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        constraints
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );

    let parameter = |stream: &str, record_index, name: &str, expression: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            None,
            expression,
            "User Parameter",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated user parameter is canonical");
        parameter.id = format!("f3d:{stream}:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let (_, parameters) = project_parameter_design(
        &[
            parameter("A", 40, "Width", "1 mm"),
            parameter("A", 41, "Half", "Width / 2"),
            parameter("B", 40, "Width", "2 mm"),
        ],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    let half = parameters
        .iter()
        .find(|parameter| parameter.name == "Half")
        .expect("projected Half parameter");
    let a_width = parameters
        .iter()
        .find(|parameter| {
            parameter.name == "Width"
                && parameter.native_ref.as_deref() == Some("f3d:A:parameter#40")
        })
        .expect("projected stream A Width parameter");
    assert_eq!(half.dependencies, std::slice::from_ref(&a_width.id));
    assert_eq!(
        parameters
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        3
    );

    for sketch in &mut sketches {
        sketch.native_ref = None;
    }
    for entity in &mut entities {
        entity.native_ref = None;
    }
    for constraint in &mut constraints {
        constraint.native_ref = None;
    }
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.sketches = sketches;
    ir.model.sketch_entities = entities;
    ir.model.sketch_constraints = constraints;
    ir.finalize();
    let report = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}
