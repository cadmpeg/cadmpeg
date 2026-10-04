// SPDX-License-Identifier: Apache-2.0

use super::{admit_constraint_row, available_parameter_ids, emitted_entity_views};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
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

fn transfer_empty_section(
    policy: &DecodePolicy,
) -> Result<cadmpeg_ir::document::CadIr, CodecError> {
    let scan = empty_section_scan();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    super::transfer_sketches(
        &ctx,
        &scan,
        &mut ir,
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &mut Vec::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )?;
    Ok(ir)
}

#[test]
fn segment_table_underflow_error_refuses_retained_text_limit() {
    let table = crate::feature::definitions::FeatureSegmentTable {
        declared_count: 0,
        has_elided_prototype: true,
        entity_ref: None,
        rows: crate::feature::segment_rows::SegmentRows::default(),
        offset: 0,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = super::expected_segment_rows(&ctx, 7, &table)
        .expect_err("underflow error text exceeds retained limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo segment table underflow error text"));
    crate::decode::with_test_decode_ctx(|ctx| {
        let error = super::expected_segment_rows(ctx, 7, &table)
            .expect_err("negative ordinary row count is malformed");
        assert!(error
            .to_string()
            .contains("feature 7 states segment table count 0 and 1 elided prototype row(s)"));
        Ok::<(), CodecError>(())
    })
    .expect("service error text admitted");
}

#[test]
fn empty_section_transfer_preserves_sketch_and_feature() {
    let ir = transfer_empty_section(&DecodePolicy::service()).expect("service section transfer");
    assert_eq!(ir.model.sketches.len(), 1);
    assert_eq!(ir.model.features.len(), 1);
    assert_eq!(ir.model.features[0].source_tag.as_deref(), Some("section"));
}

#[test]
fn sketch_native_reference_refuses_below_retained_limit() {
    let sketch = SketchId::mint("creo:model:sketch#7").expect("valid sketch ID");
    let need = cadmpeg_core::decode::u64_from_index("creo:featdefs:sketch#7".len());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = need - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = crate::decode::sketch_ids::sketch_native_ref_admitted(&ctx, &sketch)
        .expect_err("native reference exceeds retained cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo sketch native reference"));
    policy.limits.max_retained_bytes = need;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(
        crate::decode::sketch_ids::sketch_native_ref_admitted(&ctx, &sketch)
            .expect("exact cap admits reference"),
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
fn sketch_constraint_rows_refuse_before_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut rows = Vec::new();
    let error = admit_constraint_row(&ctx, &mut rows, disabled_constraint())
        .expect_err("one row exceeds zero items");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo sketch constraint rows"));
    assert!(rows.is_empty());
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
    admit_constraint_row(&ctx, &mut rows, disabled_constraint()).expect("service row admitted");
    assert_eq!(rows.len(), 1);
}

fn fixture() -> SketchEntity {
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch id");
    let entity = SketchEntityId::mint("creo:model:sketch_entity#1").expect("entity id");
    SketchEntity::new(
        entity,
        sketch,
        SketchGeometry::native(NonBlankString::try_from("native").expect("kind")),
    )
}

fn views_with_policy(policy: &DecodePolicy) -> Result<usize, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let (_, geometry) = emitted_entity_views(&ctx, &[fixture()])?;
    Ok(geometry.len())
}

#[test]
fn emitted_entity_views_refuse_each_tree_node() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = views_with_policy(&policy).expect_err("first node exceeds zero items");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo emitted sketch entity ID nodes"));
    policy.limits.max_collection_items = 1;
    let error = views_with_policy(&policy).expect_err("second node exceeds one item");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo emitted sketch geometry nodes"));
    assert_eq!(
        views_with_policy(&DecodePolicy::service()).expect("service views"),
        1
    );
}

#[test]
fn emitted_entity_views_refuse_nested_identity_and_geometry_copies() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index("creo:model:sketch_entity#1".len()) - 1;
    let error = views_with_policy(&policy).expect_err("first identity copy exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo emitted sketch entity IDs"));
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        Some("creo emitted sketch geometry"),
        |cap| {
            let mut trial = policy;
            trial.limits.max_retained_bytes = cap;
            views_with_policy(&trial)
        },
    );
    let error = views_with_policy(&policy).expect_err("native text exceeds remaining cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo emitted sketch geometry"));
    assert_eq!(
        views_with_policy(&DecodePolicy::service()).expect("service views"),
        1
    );
}

#[test]
fn available_parameter_ids_refuse_existing_node_and_identity_copy() {
    let id = ParameterId::mint("creo:featdefs:parameter#1").expect("parameter ID");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = available_parameter_ids(&ctx, [&id], BTreeSet::new())
        .expect_err("existing parameter needs a tree node");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo available parameter ID nodes"));
    policy.limits.max_collection_items = DecodePolicy::service().limits.max_collection_items;
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(id.as_str().len()) - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = available_parameter_ids(&ctx, [&id], BTreeSet::new())
        .expect_err("existing parameter identity exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo available parameter identities"));
    let ids = crate::decode::with_test_decode_ctx(|ctx| {
        available_parameter_ids(ctx, [&id], BTreeSet::new())
    })
    .expect("service IDs admitted");
    assert_eq!(ids, BTreeSet::from([id]));
}

#[test]
fn available_parameter_ids_refuse_planned_tree_node() {
    let id = ParameterId::mint("creo:featdefs:parameter#1").expect("parameter ID");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = available_parameter_ids(&ctx, std::iter::empty(), BTreeSet::from([id.clone()]))
        .expect_err("planned parameter needs a destination tree node");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo available planned parameter ID nodes"));
    let ids = crate::decode::with_test_decode_ctx(|ctx| {
        available_parameter_ids(ctx, std::iter::empty(), BTreeSet::from([id.clone()]))
    })
    .expect("service IDs admitted");
    assert_eq!(ids, BTreeSet::from([id]));
}

#[test]
fn available_parameter_ids_refuse_membership_work() {
    let first = ParameterId::mint("creo:featdefs:parameter#first").expect("parameter ID");
    let second = ParameterId::mint("creo:featdefs:parameter#second").expect("parameter ID");
    let ids = crate::test_support::assert_work_boundaries(
        &["creo available parameter ID membership"],
        |ctx| available_parameter_ids(ctx, [&first, &second], BTreeSet::new()),
    );
    assert_eq!(ids, BTreeSet::from([first, second]));
}

#[test]
fn constraint_entity_membership_refuses_work() {
    let entity = SketchEntityId::mint("creo:model:sketch_entity#1").expect("entity ID");
    let emitted = BTreeSet::from([entity.clone()]);
    let mut definition = SketchConstraintDefinitionInput::Horizontal { entity };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = super::reconcile_constraint_entity_references(&ctx, &mut definition, &emitted)
        .expect_err("entity membership exceeds zero work units");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo constraint emitted entity membership"));
}

#[test]
fn constraint_parameter_membership_refuses_work() {
    let parameter = ParameterId::mint("creo:featdefs:parameter#1").expect("parameter ID");
    let emitted = BTreeSet::from([parameter.clone()]);
    let mut definition = SketchConstraintDefinitionInput::Radius {
        entity: SketchEntityId::mint("creo:model:sketch_entity#1").expect("entity ID"),
        parameter,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = super::reconcile_constraint_parameter_reference(&ctx, &mut definition, &emitted)
        .expect_err("parameter membership exceeds zero work units");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo constraint parameter membership"));
}

macro_rules! map_node_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut values = std::collections::BTreeMap::new();
            let error = ctx.insert_btree_map(&mut values, 7usize, 9u8, $operation)
                .expect_err("one tree node exceeds zero items");
            assert!(matches!(error, CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == $operation));
            assert!(values.is_empty());
            let service = DecodePolicy::service();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
            ctx.insert_btree_map(&mut values, 7usize, 9u8, $operation).expect("service node admitted");
            assert_eq!(values.get(&7), Some(&9));
        }
    };
}

macro_rules! set_node_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut values = BTreeSet::new();
            let error = ctx.insert_btree_set(&mut values, 7usize, $operation)
                .expect_err("one tree node exceeds zero items");
            assert!(matches!(error, CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == $operation));
            assert!(values.is_empty());
            let service = DecodePolicy::service();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
            ctx.insert_btree_set(&mut values, 7usize, $operation).expect("service node admitted");
            assert_eq!(values, BTreeSet::from([7]));
        }
    };
}

map_node_test!(
    resolved_sketch_point_node_refuses_limit,
    "creo resolved sketch point nodes"
);
map_node_test!(
    resolved_section_geometry_node_refuses_limit,
    "creo resolved section geometry nodes"
);
map_node_test!(
    section_geometry_node_refuses_limit,
    "creo section geometry nodes"
);
map_node_test!(
    section_circle_geometry_node_refuses_limit,
    "creo section circle geometry nodes"
);
map_node_test!(
    section_point_geometry_node_refuses_limit,
    "creo section point geometry nodes"
);
map_node_test!(
    section_centered_line_geometry_node_refuses_limit,
    "creo section centered-line geometry nodes"
);
map_node_test!(
    section_reference_line_geometry_node_refuses_limit,
    "creo section reference-line geometry nodes"
);
set_node_test!(
    solved_section_segment_node_refuses_limit,
    "creo solved section segment ID nodes"
);
set_node_test!(
    emitted_section_segment_node_refuses_limit,
    "creo emitted section segment ID nodes"
);
set_node_test!(
    resolved_section_offset_node_refuses_limit,
    "creo resolved section offset nodes"
);
set_node_test!(
    equation_offset_node_refuses_limit,
    "creo equation offset nodes"
);
set_node_test!(
    rejected_equation_offset_node_refuses_limit,
    "creo rejected equation offset nodes"
);
set_node_test!(
    typed_equation_offset_node_refuses_limit,
    "creo typed equation offset nodes"
);

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

fn scalar_equality_equation_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = empty_section_scan();
    let definition = &mut scan.features.definitions[0];
    definition.offset = 1000;
    definition.saved_section = None;
    definition.body = vec![0; 20];
    definition.body.extend_from_slice(
        b"eqtn_arr\0\xf2\xf8\x02\xf7\x80\x9f\xfb\xe2\
            \xe0\x01id\0\0\xf1\xf7\x80\x9f\xe2\
            \x01\x05\xf8\x03\x00\x01\x02\xf6\xe2",
    );
    let row = |variable_type, key, value: Option<f64>| {
        let value = value.map_or(
            crate::feature::definitions::ScalarLane::DimensionDriven,
            crate::feature::definitions::ScalarLane::Value,
        );
        crate::feature::definitions::FeatureVariableRow {
            variable_type: crate::feature::definitions::VariableType::from(variable_type),
            key,
            value,
            value_body: Vec::new(),
            guess: value,
            guess_body: Vec::new(),
            known: Some(0),
            homogeneity: Some(1),
            uvar_id: None,
            offset: 0,
        }
    };
    definition.variables = Some(crate::feature::definitions::FeatureVariableTable {
        declared_count: 3,
        entity_ref: None,
        rows: vec![
            row(6, 10, Some(2.5)),
            row(6, 11, Some(2.5)),
            row(5, 20, Some(0.0)),
        ],
        offset: 0,
    });
    scan
}

fn scalar_equality_candidates(
    scan: &crate::container::ContainerScan<'_>,
) -> Vec<(SketchConstraint, usize)> {
    crate::decode::with_test_decode_ctx(|ctx| {
        let sketch = SketchId::mint("creo:model:sketch#source-admission")
            .expect("valid source-admission sketch ID");
        super::section_equation_function_five_scalar_equality_constraints(
            ctx,
            &scan.features.definitions[0],
            &sketch,
        )
    })
    .expect("function-five scalar-equality fixture is typed")
}

fn scalar_equality_transfer_result(
    ctx: &DecodeContext<'_>,
    scan: &crate::container::ContainerScan<'_>,
) -> Result<(Vec<(u32, u32)>, Vec<u64>), CodecError> {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    super::transfer_sketches(
        ctx,
        scan,
        &mut ir,
        &mut annotations,
        &mut Vec::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )?;
    let scalar_equalities = ir
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| match constraint.definition.kind() {
            SketchConstraintDefinitionInput::ScalarEquality { first, second } => {
                Some((*first, *second))
            }
            _ => None,
        })
        .collect();
    let equation_annotation_offsets = annotations
        .build()
        .provenance
        .values()
        .filter(|note| note.tag.as_deref() == Some("section_equation_constraint"))
        .map(|note| note.offset)
        .collect();
    Ok((scalar_equalities, equation_annotation_offsets))
}

#[test]
fn equation_offset_source_admission_refuses_work_and_emits_typed_scalar_equality() {
    let scan = scalar_equality_equation_scan();
    let typed_rows = scalar_equality_candidates(&scan);
    assert_eq!(typed_rows.len(), 1, "fixture produces one typed row");
    let source_offsets = typed_rows
        .iter()
        .map(|(_, offset)| *offset)
        .collect::<BTreeSet<_>>();
    assert_eq!(source_offsets.len(), 1, "typed source set is nonempty");
    let expected_offset = cadmpeg_core::decode::u64_from_index(
        1000 + *source_offsets
            .iter()
            .next()
            .expect("one typed source offset"),
    );
    let (scalar_equalities, equation_annotation_offsets) =
        crate::test_support::assert_work_boundaries(
            &["creo equation offset source"],
            |ctx| scalar_equality_transfer_result(ctx, &scan),
        );
    assert_eq!(scalar_equalities, vec![(10, 11)]);
    assert_eq!(equation_annotation_offsets, vec![expected_offset]);
}

#[test]
fn typed_equation_offset_source_admission_refuses_work_and_emits_typed_scalar_equality() {
    let scan = scalar_equality_equation_scan();
    let typed_rows = scalar_equality_candidates(&scan);
    assert_eq!(typed_rows.len(), 1, "fixture produces one typed row");
    let source_offsets = typed_rows
        .iter()
        .map(|(_, offset)| *offset)
        .collect::<BTreeSet<_>>();
    assert_eq!(source_offsets.len(), 1, "typed source set is nonempty");
    let expected_offset = cadmpeg_core::decode::u64_from_index(
        1000 + *source_offsets
            .iter()
            .next()
            .expect("one typed source offset"),
    );
    let (scalar_equalities, equation_annotation_offsets) =
        crate::test_support::assert_work_boundaries(
            &["creo typed equation offset source"],
            |ctx| scalar_equality_transfer_result(ctx, &scan),
        );
    assert_eq!(scalar_equalities, vec![(10, 11)]);
    assert_eq!(equation_annotation_offsets, vec![expected_offset]);
}

#[test]
fn equation_header_uses_definition_source_base() {
    let (scan, _) = equation_scan();
    let headers = crate::decode::with_test_decode_ctx(|ctx| {
        crate::decode::sketch_ids::sketch_table_headers(ctx, &scan.features.definitions[0])
    })
    .expect("headers");
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].offset, 1020);
}

#[test]
fn native_equation_row_uses_definition_source_base() {
    let (scan, row_start) = equation_scan();
    crate::decode::with_test_decode_ctx(|ctx| {
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
    crate::decode::with_test_decode_ctx(|ctx| {
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



#[test]
fn sketch_profile_entity_membership_refuses_work_and_preserves_resolved_chain() {
    let mut scan = empty_section_scan();
    let definition = &mut scan.features.definitions[0];
    let variable = |variable_type: crate::feature::definitions::VariableType,
                    key: u32,
                    value: f64| crate::feature::definitions::FeatureVariableRow {
        variable_type,
        key,
        value: crate::feature::definitions::ScalarLane::Value(value),
        value_body: Vec::new(),
        guess: crate::feature::definitions::ScalarLane::Undefined,
        guess_body: Vec::new(),
        known: None,
        homogeneity: None,
        uvar_id: None,
        offset: usize::try_from(key).expect("fixture index fits usize"),
    };
    definition.variables = Some(crate::feature::definitions::FeatureVariableTable {
        declared_count: 6,
        entity_ref: None,
        rows: vec![
            variable(crate::feature::definitions::VariableType::U, 1, 0.0),
            variable(crate::feature::definitions::VariableType::V, 1, 0.0),
            variable(crate::feature::definitions::VariableType::U, 2, 1.0),
            variable(crate::feature::definitions::VariableType::V, 2, 0.0),
            variable(crate::feature::definitions::VariableType::U, 3, 0.0),
            variable(crate::feature::definitions::VariableType::V, 3, 1.0),
        ],
        offset: 0,
    });
    let segment = |external_id: u32, point_ids: [u32; 2]| {
        crate::feature::definitions::FeatureSegment {
            kind: crate::feature::definitions::FeatureSegmentKind::Line(point_ids),
            directions: [None; 3],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: usize::try_from(external_id).expect("fixture index fits usize"),
        }
    };
    definition.segments = Some(crate::feature::definitions::FeatureSegmentTable {
        declared_count: 3,
        has_elided_prototype: false,
        entity_ref: None,
        rows: [segment(10, [1, 2]), segment(11, [2, 3]), segment(12, [3, 1])]
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
        offset: 0,
    });

    let service = crate::test_support::assert_work_boundaries(
        &["creo sketch profile entity membership"],
        |ctx| {
            let mut output = cadmpeg_ir::document::CadIr::empty();
            super::transfer_sketches(
                ctx,
                &scan,
                &mut output,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &mut Vec::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )?;
            Ok(output)
        },
    );
    assert_eq!(service.model.sketches.len(), 1);
    assert_eq!(service.model.sketch_entities.len(), 3);
    let sketch = &service.model.sketches[0];
    assert_eq!(sketch.profiles.len(), 1);
    assert_eq!(sketch.profiles[0].len(), 3);
    assert_eq!(
        sketch.profiles[0]
            .iter()
            .map(|entity_use| (entity_use.entity.as_str(), entity_use.reversed))
            .collect::<Vec<_>>(),
        vec![
            ("creo:featdefs:sketch_entity#7:10", false),
            ("creo:featdefs:sketch_entity#7:11", false),
            ("creo:featdefs:sketch_entity#7:12", false),
        ]
    );
    assert!(service
        .model
        .sketch_entities
        .iter()
        .all(|entity| !entity.construction));
}
