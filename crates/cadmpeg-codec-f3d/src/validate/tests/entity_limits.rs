// SPDX-License-Identifier: Apache-2.0

fn validation_entity_header(
    references: Vec<crate::records::identity::Located<u32>>,
) -> crate::records::entity_header::DesignEntityHeader {
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, SketchHeaderReferences,
    };
    use crate::records::identity::{DesignEntityId, ReferenceRun};
    DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity-header#1".into(),
        byte_offset: 10,
        entity_id: DesignEntityId::from_parts("sketch", 1),
        class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.into()),
            (!references.is_empty()).then_some(SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: 10,
                references,
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    }
}

#[test]
fn native_entity_suffix_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_entity_headers.push(validation_entity_header(Vec::new()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    let error = super::super::validate_entity_headers(&ctx, &mut Vec::new()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D design entity suffixes"));
}

#[test]
fn native_entity_duplicate_finding_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let header = validation_entity_header(Vec::new());
    let mut native = crate::native::F3dNative::default();
    native.design_entity_headers.push(header.clone());
    native.design_entity_headers.push(header);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    let error = super::super::validate_entity_headers(&ctx, &mut Vec::new()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}

#[test]
fn native_entity_reference_finding_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_entity_headers.push(validation_entity_header(vec![
        crate::records::identity::Located { value: 999, offset: 12 },
    ]));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    let error = super::super::validate_entity_headers(&ctx, &mut Vec::new()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity"));
}

#[test]
fn native_sketch_relation_finding_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::records::identity::ReferenceRun;
    use crate::records::sketch_relations::{
        SketchRelation, SketchRelationDefinition, SketchRelationDraft, SketchRelationMembers,
        SketchRelationReturnMembers,
    };

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let service = cadmpeg_test_support::service_decode_context();
    let relation = SketchRelation::try_new(SketchRelationDraft {
        id: "f3d:Design/BulkStream.dat:sketch-relation#1".into(),
        record_index: 1,
        class_tag: crate::records::references::DesignClassTag::try_from("296".to_owned()).unwrap(),
        byte_offset: 10,
        state_offset: 0,
        owner_reference: 999,
        owner_entity_id: None,
        auxiliary_references: ReferenceRun::from_columns(vec![1], vec![4], "auxiliary").unwrap(),
        rectangular_counted_reference_count: None,
        members: SketchRelationMembers::from_indices(&service, std::iter::empty()).unwrap(),
        owner_reference_offset: 8,
        definition: SketchRelationDefinition::new(0, None).unwrap(),
        entity_genesis: None,
        return_members: SketchRelationReturnMembers::from_indices(&service, std::iter::empty()).unwrap(),
        raw_bytes: vec![0; 24],
    }).unwrap();
    let mut native = crate::native::F3dNative::default();
    native.sketch_relations.push(relation);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    let error = super::super::validate_sketch_relations(&ctx, &mut Vec::new()).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity"));
}

fn sketch_geometry_fixture() -> crate::native::F3dNative {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = crate::F3dCodec
        .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (ir, _, _) = decoded.into_parts();
    crate::native::F3dNative::load(ir.native.namespace("f3d").unwrap()).unwrap()
}

fn sketch_geometry_error(
    native: crate::native::F3dNative,
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    super::super::validate_sketch_geometry_identities(&ctx, &mut Vec::new()).unwrap_err()
}

#[test]
fn sketch_point_identity_index_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = Some(100);
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch point identities"));
}

#[test]
fn sketch_geometry_record_index_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = None;
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch geometry records"));
}

#[test]
fn sketch_curve_identity_index_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.clear();
    native.sketch_curve_identities.truncate(1);
    native.sketch_curve_identities[0].owner_reference = Some(100);
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch curve identities"));
}

fn validation_sketch_surface() -> crate::records::sketch_geometry::SketchSurface {
    use crate::records::sketch_geometry::{SketchSurface, SketchSurfaceGeometry};
    SketchSurface {
        id: "f3d:Design/BulkStream.dat:sketch-surface#1".into(),
        record_index: 1,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("296".to_owned()).unwrap(),
        byte_offset: 10,
        entity_genesis: None,
        persistent_id: std::num::NonZeroU64::new(1).unwrap(),
        geometry: SketchSurfaceGeometry {
            u_degree: std::num::NonZeroU32::new(1).unwrap(),
            v_degree: std::num::NonZeroU32::new(1).unwrap(),
            u_knots: Vec::new(),
            v_knots: Vec::new(),
            control_points: Vec::new(),
        },
    }
}

#[test]
fn sketch_surface_identity_index_refuses_collection_limit() {
    let mut native = crate::native::F3dNative::default();
    native.sketch_surfaces.push(validation_sketch_surface());
    let error = sketch_geometry_error(native, 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D sketch surface identities"));
}

#[test]
fn sketch_point_identity_finding_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = Some(100);
    let duplicate = native.sketch_points[0].clone();
    native.sketch_points.push(duplicate);
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 2, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}

#[test]
fn sketch_point_identity_finding_refuses_retained_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = Some(100);
    let duplicate = native.sketch_points[0].clone();
    native.sketch_points.push(duplicate);
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, u64::MAX, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity"));
}

#[test]
fn sketch_curve_identity_finding_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.clear();
    native.sketch_curve_identities.truncate(1);
    native.sketch_curve_identities[0].owner_reference = Some(100);
    let duplicate = native.sketch_curve_identities[0].clone();
    native.sketch_curve_identities.push(duplicate);
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 2, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}

#[test]
fn sketch_surface_identity_finding_refuses_collection_limit() {
    let surface = validation_sketch_surface();
    let mut native = crate::native::F3dNative::default();
    native.sketch_surfaces.push(surface.clone());
    native.sketch_surfaces.push(surface);
    let error = sketch_geometry_error(native, 2, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}

#[test]
fn sketch_geometry_alias_finding_refuses_collection_limit() {
    let mut native = sketch_geometry_fixture();
    native.sketch_points.truncate(1);
    native.sketch_points[0].owner_reference = None;
    let duplicate = native.sketch_points[0].clone();
    native.sketch_points.push(duplicate);
    native.sketch_curve_identities.clear();
    native.sketch_surfaces.clear();
    let error = sketch_geometry_error(native, 1, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}
