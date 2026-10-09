// SPDX-License-Identifier: Apache-2.0

use super::{
    bodies_containing_edges, copy_body_id, decoded_feature_reference_name,
    evaluated_sweep_body_kind, feature_output_bodies, feature_reference_name,
    generated_edge_output_bodies, generated_input_output_bodies,
};

#[test]
fn invalid_feature_reference_name_refuses_before_lossy_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo decoded feature reference name"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            decoded_feature_reference_name(&ctx, b"A\xff").map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = decoded_feature_reference_name(&ctx, b"A\xff")
        .expect_err("replacement needs four retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo decoded feature reference name")
    );

    crate::decode::with_test_decode_ctx(|ctx| {
        assert_eq!(decoded_feature_reference_name(ctx, b"A\xff")?, "A\u{fffd}");
        assert_eq!(decoded_feature_reference_name(ctx, b"ASCII")?, "ASCII");
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("service-profile reference names");
}

#[test]
fn feature_reference_name_admits_byte_comparison() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.reference_names.extend([
        crate::feature::operations::FeatureReferenceName {
            feature_id: 42,
            name_bytes: b"feature-reference".to_vec(),
            own_reference_id: 7,
            reference_type: 1,
            offset: 0,
        },
        crate::feature::operations::FeatureReferenceName {
            feature_id: 42,
            name_bytes: b"feature-reference".to_vec(),
            own_reference_id: 8,
            reference_type: 1,
            offset: 10,
        },
    ]);

    let name = crate::test_support::assert_work_boundaries(
        &[
            "creo feature reference names",
            "creo feature reference name agreement",
        ],
        |ctx| feature_reference_name(ctx, &scan, 42),
    );
    assert_eq!(name, Some(b"feature-reference".as_slice()));
}
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Face, Loop as IrLoop, Region, Sense, Shell};
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{insert_feature_parameter, insert_feature_source_property, replace_feature_parameter};

fn sweep_output_ir() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#40:body").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir
}

#[test]
fn feature_output_history_refuses_before_visiting_node() {
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo feature output visiting nodes"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40)
        .expect_err("one history node exceeds the limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature output visiting nodes")
    );
}

#[test]
fn feature_output_history_refuses_before_recursive_step() {
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = crate::test_support::allocation_limit_at(
        ResourceDimension::RecursionDepth,
        Some("creo feature output history"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_recursion_depth = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40)
        .expect_err("the first history step exceeds zero recursion depth");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RecursionDepth
            && resource.operation == "creo feature output history")
    );
}

#[test]
fn evaluated_sweep_candidate_refuses_before_scoped_text() {
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        Some("creo evaluated sweep body candidate"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40)
        .expect_err("one candidate needs scoped text");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo evaluated sweep body candidate")
    );
}

#[test]
fn evaluated_sweep_body_refuses_before_retained_id() {
    let scan = crate::test_support::empty_container_scan();
    let ir = sweep_output_ir();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo feature output body IDs"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            feature_output_bodies(&ctx, &scan, &ir, 40).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &ir, 40)
        .expect_err("one output needs a retained ID");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature output body IDs")
    );
}

#[test]
fn evaluated_sweep_body_refuses_before_output_row() {
    let scan = crate::test_support::empty_container_scan();
    let ir = sweep_output_ir();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo evaluated sweep output bodies"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            feature_output_bodies(&ctx, &scan, &ir, 40).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &ir, 40)
        .expect_err("one output needs a Vec row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo evaluated sweep output bodies")
    );
}

#[test]
fn copied_output_body_id_refuses_before_retained_bytes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo feature output body IDs"),
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = policy;
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let body = BodyId::mint("creo:test:body#1").expect("identity grammar");
            copy_body_id(&ctx, &body).map(|_| ())
        },
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let body = BodyId::mint("creo:test:body#1").expect("identity grammar");
    let error = copy_body_id(&ctx, &body).expect_err("body ID needs retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature output body IDs")
    );
}

#[test]
fn copied_unicode_output_body_id_charges_only_retained_copy_work() {
    let arena = DecodeArena::new();
    let body = BodyId::mint("creo:test:body#é").expect("identity grammar");
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index(body.as_str().len());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");

    assert_eq!(
        copy_body_id(&ctx, &body).expect("typed identity copy fits its byte-work limit"),
        body
    );
}

#[test]
fn section_feature_lookups_keep_unique_source_selection() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Section),
        stream_offset: 0,
        body: vec![0; 8].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(17),
                owner_feature_id: Some(40),
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
            saved_section: None,
            offset: 4,
        });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::owned_section_feature_id(ctx, &scan, 17))
            .expect("admitted section owner"),
        Some(40)
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::section_definition_for_history_feature(
            ctx, &scan, 40
        ))
        .expect("admitted section definition")
        .map(|value| value.offset),
        Some(4)
    );
    scan.features
        .definitions
        .push(scan.features.definitions[0].clone());
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::owned_section_feature_id(ctx, &scan, 17))
            .expect("admitted section owner"),
        None
    );
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::section_definition_for_history_feature(ctx, &scan, 40)
    })
    .expect("admitted section definition")
    .is_none());
}

#[test]
fn evaluated_sweep_body_joins_reject_duplicate_ids() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(ctx, &scan, &ir, 40))
            .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar")]
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| evaluated_sweep_body_kind(
            ctx,
            &ir,
            "extrusion",
            40
        ))
        .expect("service profile admits scalar parsing"),
        Some(BodyKind::Solid)
    );

    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Sheet,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(ctx, &scan, &ir, 40))
            .expect("service profile admits output bodies")
            .is_empty()
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| evaluated_sweep_body_kind(
            ctx,
            &ir,
            "extrusion",
            40
        ))
        .expect("service profile admits scalar parsing"),
        None
    );
}

#[test]
fn evaluated_sweep_body_scan_refuses_before_identity_comparison() {
    let scan = crate::test_support::empty_container_scan();
    let ir = sweep_output_ir();
    let outputs = crate::test_support::assert_work_boundaries(
        &["creo evaluated sweep body lookup traversal"],
        |ctx| feature_output_bodies(ctx, &scan, &ir, 40),
    );
    assert_eq!(
        outputs,
        vec![BodyId::mint("creo:feature:extrusion#40:body").expect("identity grammar")],
    );
}

#[test]
fn evaluated_sweep_body_identity_comparison_refuses_at_work_boundary() {
    let scan = crate::test_support::empty_container_scan();
    let ir = sweep_output_ir();
    let bodies = crate::test_support::assert_work_boundaries(
        &["creo evaluated sweep body identity comparison"],
        |ctx| feature_output_bodies(ctx, &scan, &ir, 40),
    );
    assert_eq!(
        bodies,
        vec![BodyId::mint("creo:feature:extrusion#40:body").expect("fixture body ID")],
    );
}

mod admission_recovery;
mod generated;
mod properties;
mod selected_edges;
mod storage_lifetime;
