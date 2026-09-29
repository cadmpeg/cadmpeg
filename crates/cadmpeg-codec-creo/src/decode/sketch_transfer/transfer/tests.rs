// SPDX-License-Identifier: Apache-2.0

use super::{admit_constraint_row, available_parameter_ids, emitted_entity_views, insert_set, insert_tree};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
};
use cadmpeg_ir::features::ParameterId;
use std::collections::BTreeSet;

fn empty_section_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.definitions.push(crate::feature::definitions::FeatureDefinition {
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

fn transfer_empty_section(policy: &DecodePolicy) -> Result<cadmpeg_ir::document::CadIr, CodecError> {
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
fn empty_section_transfer_preserves_sketch_and_feature() {
    let ir = transfer_empty_section(&DecodePolicy::service()).expect("service section transfer");
    assert_eq!(ir.model.sketches.len(), 1);
    assert_eq!(ir.model.features.len(), 1);
    assert_eq!(ir.model.features[0].source_tag.as_deref(), Some("section"));
}

#[test]
fn sketch_native_reference_refuses_below_retained_limit() {
    let sketch = SketchId::mint("creo:model:sketch#7").expect("valid sketch ID");
    let need = "creo:featdefs:sketch#7".len() as u64;
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
    assert_eq!(crate::decode::sketch_ids::sketch_native_ref_admitted(&ctx, &sketch)
        .expect("exact cap admits reference"), "creo:featdefs:sketch#7");
}

fn disabled_constraint() -> SketchConstraint {
    SketchConstraint {
        id: SketchConstraintId::mint("creo:model:sketch_constraint#1").expect("constraint id"),
        sketch: SketchId::mint("creo:model:sketch#1").expect("sketch id"),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Disabled {},
        ).expect("disabled constraint"),
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
        SketchGeometry::native(NonBlankString::new("native").expect("kind")),
    )
}

fn views_with_policy(
    policy: &DecodePolicy,
) -> Result<usize, CodecError> {
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
    assert_eq!(views_with_policy(&DecodePolicy::service()).expect("service views"), 1);
}

#[test]
fn emitted_entity_views_refuse_nested_identity_and_geometry_copies() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = "creo:model:sketch_entity#1".len() as u64 - 1;
    let error = views_with_policy(&policy).expect_err("first identity copy exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo emitted sketch entity IDs"));
    policy.limits.max_retained_bytes = ("creo:model:sketch_entity#1".len() * 2 + "native".len() - 1) as u64;
    let error = views_with_policy(&policy).expect_err("native text exceeds remaining cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo emitted sketch geometry"));
    assert_eq!(views_with_policy(&DecodePolicy::service()).expect("service views"), 1);
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
    policy.limits.max_retained_bytes = id.as_str().len() as u64 - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = available_parameter_ids(&ctx, [&id], BTreeSet::new())
        .expect_err("existing parameter identity exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo available parameter identities"));
    let ids = crate::decode::with_test_decode_ctx(|ctx| {
        available_parameter_ids(ctx, [&id], BTreeSet::new())
    }).expect("service IDs admitted");
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
    }).expect("service IDs admitted");
    assert_eq!(ids, BTreeSet::from([id]));
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
            let error = insert_tree(&ctx, &mut values, 7usize, 9u8, $operation)
                .expect_err("one tree node exceeds zero items");
            assert!(matches!(error, CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == $operation));
            assert!(values.is_empty());
            let service = DecodePolicy::service();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
            insert_tree(&ctx, &mut values, 7usize, 9u8, $operation).expect("service node admitted");
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
            let error = insert_set(&ctx, &mut values, 7usize, $operation)
                .expect_err("one tree node exceeds zero items");
            assert!(matches!(error, CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == $operation));
            assert!(values.is_empty());
            let service = DecodePolicy::service();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
            insert_set(&ctx, &mut values, 7usize, $operation).expect("service node admitted");
            assert_eq!(values, BTreeSet::from([7]));
        }
    };
}

map_node_test!(resolved_sketch_point_node_refuses_limit, "creo resolved sketch point nodes");
map_node_test!(resolved_section_geometry_node_refuses_limit, "creo resolved section geometry nodes");
map_node_test!(section_geometry_node_refuses_limit, "creo section geometry nodes");
map_node_test!(section_circle_geometry_node_refuses_limit, "creo section circle geometry nodes");
map_node_test!(section_point_geometry_node_refuses_limit, "creo section point geometry nodes");
map_node_test!(section_centered_line_geometry_node_refuses_limit, "creo section centered-line geometry nodes");
map_node_test!(section_reference_line_geometry_node_refuses_limit, "creo section reference-line geometry nodes");
set_node_test!(solved_section_segment_node_refuses_limit, "creo solved section segment ID nodes");
set_node_test!(emitted_section_segment_node_refuses_limit, "creo emitted section segment ID nodes");
set_node_test!(resolved_section_offset_node_refuses_limit, "creo resolved section offset nodes");
set_node_test!(equation_offset_node_refuses_limit, "creo equation offset nodes");
set_node_test!(rejected_equation_offset_node_refuses_limit, "creo rejected equation offset nodes");
set_node_test!(typed_equation_offset_node_refuses_limit, "creo typed equation offset nodes");
