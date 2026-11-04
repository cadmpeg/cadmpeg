// SPDX-License-Identifier: Apache-2.0
//! Source-less direct `SubD` archives retain the actual admitted control cage.

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::write::{target::TargetRequest, ArenaDisposition, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::subd::{SubdCage, SubdEdge, SubdEdgeUse, SubdFace, SubdScheme, SubdVertex};
use std::io::Cursor;

fn triangle() -> SubdSurface {
    let vertices = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 1.0]]
        .into_iter()
        .map(|[x, y, z]| {
            SubdVertex::new(Point3::new(x, y, z), SubdVertexTag::Smooth, None).unwrap()
        })
        .collect();
    let edges = [[0, 1], [1, 2], [2, 0]]
        .into_iter()
        .map(|ends| SubdEdge::new(ends, [0.0, 0.0], SubdEdgeTag::Smooth, None, [0.0, 0.0]).unwrap())
        .collect();
    let face = SubdFace::new(
        (0..3)
            .map(|edge| SubdEdgeUse {
                edge,
                reversed: false,
            })
            .collect(),
    )
    .unwrap();
    SubdSurface {
        native_ref: None,
        id: "test:model:subd#triangle".try_into().unwrap(),
        scheme: SubdScheme::CatmullClark,
        cage: SubdCage::new(vertices, edges, vec![face], Vec::new()).unwrap(),
        source_object: None,
    }
}

#[test]
fn oversized_face_is_named_and_does_not_remove_another_subd() {
    let count = u32::from(u16::MAX) + 1;
    let vertex = SubdVertex::new(Point3::new(0.0, 0.0, 0.0), SubdVertexTag::Smooth, None)
        .expect("finite vertex");
    let edges = (0..count)
        .map(|index| {
            SubdEdge::new(
                [index, (index + 1) % count],
                [0.0, 0.0],
                SubdEdgeTag::Smooth,
                None,
                [0.0, 0.0],
            )
            .expect("edge")
        })
        .collect();
    let face = SubdFace::new(
        (0..count)
            .map(|edge| SubdEdgeUse {
                edge,
                reversed: false,
            })
            .collect(),
    )
    .expect("face");
    let oversized = SubdSurface {
        native_ref: None,
        id: "test:model:subd#oversized".try_into().expect("identity"),
        scheme: SubdScheme::CatmullClark,
        cage: SubdCage::new(
            vec![vertex; usize::try_from(count).expect("count")],
            edges,
            vec![face],
            Vec::new(),
        )
        .expect("closed cage"),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.subds.push(oversized);
    ir.model.subds.push(triangle());
    for version in [RhinoArchiveVersion::V5, RhinoArchiveVersion::V8] {
        let mut bytes = Vec::new();
        let report = crate::RhinoCodec
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("write supported sibling");
        assert!(report.losses.iter().any(|loss| {
            loss.code == RhinoLossCode::WriterSubdOmitted.kind()
                && loss.message.contains("test:model:subd#oversized")
        }));
        let decoded = crate::RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode sibling");
        assert_eq!(decoded.ir().model.subds.len(), 1);
        assert_eq!(decoded.ir().model.subds[0].cage, triangle().cage);
    }
}

#[test]
fn proxy_counts_use_the_signed_wire_width() {
    let limit = usize::try_from(i32::MAX).expect("i32 fits the host address width");
    assert_eq!(proxy_count(limit).expect("i32 limit"), i32::MAX);
    assert!(matches!(proxy_count(limit + 1), Err(WriteError::Omit(_))));
}

fn two_faces() -> SubdSurface {
    let vertices = [
        ([0.0, 0.0, 0.0], SubdVertexTag::Smooth),
        ([2.0, 0.0, 0.0], SubdVertexTag::Crease),
        ([0.0, 3.0, 1.0], SubdVertexTag::Corner),
        ([0.0, -1.0, 2.0], SubdVertexTag::Dart),
    ]
    .into_iter()
    .map(|([x, y, z], tag)| SubdVertex::new(Point3::new(x, y, z), tag, None).unwrap())
    .collect();
    let edges = [
        ([0, 1], SubdEdgeTag::Crease, [0.25, 0.75], [0.125, 0.875]),
        ([1, 2], SubdEdgeTag::Smooth, [0.0, 0.0], [0.0, 0.0]),
        ([2, 0], SubdEdgeTag::SmoothX, [1.0, 1.0], [0.5, 0.25]),
        ([0, 3], SubdEdgeTag::Smooth, [0.0, 0.0], [0.0, 0.0]),
        ([3, 1], SubdEdgeTag::Crease, [2.0, 3.0], [0.25, 0.75]),
    ]
    .into_iter()
    .map(|(ends, tag, sharpness, coefficients)| {
        SubdEdge::new(ends, sharpness, tag, None, coefficients).unwrap()
    })
    .collect();
    let face = |uses: &[(u32, bool)]| {
        SubdFace::new(
            uses.iter()
                .map(|&(edge, reversed)| SubdEdgeUse { edge, reversed })
                .collect(),
        )
        .unwrap()
    };
    SubdSurface {
        native_ref: None,
        id: "test:model:subd#two-faces".try_into().unwrap(),
        scheme: SubdScheme::CatmullClark,
        cage: SubdCage::new(
            vertices,
            edges,
            vec![
                face(&[(0, false), (1, false), (2, false)]),
                face(&[(0, true), (3, false), (4, false)]),
            ],
            Vec::new(),
        )
        .unwrap(),
        source_object: None,
    }
}

fn direct_round_trip(source: &SubdSurface, version: RhinoArchiveVersion) -> SubdSurface {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).unwrap();
    let bytes = payload(&ctx, &session, source, version).unwrap().unwrap();
    let decoded = crate::subd::decode(
        bytes.as_bytes(),
        0..bytes.len(),
        crate::chunks::ArchiveVersion::from_word(version.value()),
        crate::settings::MillimeterScale::IDENTITY,
        source.id.clone(),
    )
    .unwrap()
    .unwrap();
    assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
    assert!(decoded.enum_diagnostics.is_empty());
    assert!(!decoded.neutral_metadata);
    decoded.surface
}

#[test]
fn triangle_cage_round_trips_without_source_in_all_direct_archive_versions() {
    let source = triangle();
    for version in [
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        assert_eq!(direct_round_trip(&source, version), source);
    }
}

#[test]
fn shared_edges_directed_rings_tags_coefficients_and_endpoint_sharpness_round_trip() {
    let source = two_faces();
    let output = direct_round_trip(&source, RhinoArchiveVersion::V8);
    assert_eq!(output, source);
    assert_eq!(output.cage.vertices()[3].tag, SubdVertexTag::Dart);
    assert_eq!(output.cage.edges()[2].tag, SubdEdgeTag::SmoothX);
    assert_eq!(
        output.cage.edges()[0]
            .sharpness()
            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
        [0.25, 0.75]
    );
    assert_eq!(
        output.cage.edges()[0]
            .sector_coefficients()
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [0.125, 0.875]
    );
    assert!(output.cage.faces()[1].edges()[0].reversed);
}

#[test]
fn source_less_plan_writes_a_real_subd_object_and_reports_actual_carry() {
    let source = two_faces();
    let mut ir = CadIr::empty();
    ir.model.subds.push(source.clone());
    let plan = crate::RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("rhino:archive-80"),
        )
        .unwrap();
    assert_eq!(
        plan.report()
            .coverage
            .disposition(cadmpeg_ir::schema::EntityKind::SubdSurface),
        ArenaDisposition::Reported
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != RhinoLossCode::WriterSubdOmitted.kind()));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.subds.len(), 1);
    assert_eq!(decoded.ir().model.subds[0].cage, source.cage);
    assert_eq!(decoded.ir().model.subds[0].scheme, source.scheme);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != RhinoLossCode::IntegrityFailure.kind()));
}

#[test]
fn archive_50_writes_a_mesh_proxy_that_reads_back_as_the_exact_subd() {
    let source = triangle();
    let mut ir = CadIr::empty();
    ir.model.subds.push(source.clone());
    let plan = crate::RhinoCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("rhino:archive-50"),
        )
        .unwrap();
    assert!(
        plan.report()
            .coverage
            .disposition(cadmpeg_ir::schema::EntityKind::SubdSurface)
            .carries(),
        "the proxy carries SubD content"
    );
    assert!(
        plan.report()
            .losses
            .iter()
            .all(|loss| loss.code != RhinoLossCode::WriterSubdOmitted.kind()),
        "an exactly promoted proxy charges no SubD omission"
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.subds.len(), 1);
    assert_eq!(decoded.ir().model.subds[0].cage, source.cage);
    assert_eq!(decoded.ir().model.subds[0].scheme, source.scheme);
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != RhinoLossCode::IntegrityFailure.kind()),
        "the proxy verifies without integrity failures"
    );
}

#[test]
fn pre_eighty_does_not_silently_coerce_distinct_endpoint_sharpness() {
    let source = two_faces();
    assert_eq!(
        omission(&source, RhinoArchiveVersion::V6),
        Some("distinct endpoint sharpness requires archive 80")
    );
    assert_eq!(
        omission(&source, RhinoArchiveVersion::V7),
        Some("distinct endpoint sharpness requires archive 80")
    );
    assert_eq!(omission(&source, RhinoArchiveVersion::V8), None);
}

#[test]
fn an_admitted_ring_that_repeats_an_edge_is_not_written_as_invalid_native_topology() {
    let original = triangle();
    let repeated = SubdFace::new(
        original.cage.faces()[0]
            .edges()
            .iter()
            .copied()
            .cycle()
            .take(6)
            .collect(),
    )
    .unwrap();
    let cage = SubdCage::new(
        original.cage.vertices().to_vec(),
        original.cage.edges().to_vec(),
        vec![repeated],
        Vec::new(),
    )
    .unwrap();
    let source = SubdSurface { cage, ..original };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).unwrap();
    assert!(payload(&ctx, &session, &source, RhinoArchiveVersion::V8)
        .unwrap()
        .is_none());
}

#[test]
fn incidence_scratch_is_not_retained_and_payload_bytes_keep_the_actual_session_budget() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let staged = incidence(
        &ctx,
        3,
        (0..3).map(|owner| Incidence {
            owner,
            pointer: Pointer::default(),
        }),
    )
    .unwrap();
    assert_eq!(staged.len(), 3);
    let session = EncodeSession::from_context(&ctx).unwrap();
    let error = payload(&ctx, &session, &triangle(), RhinoArchiveVersion::V8)
        .err()
        .unwrap();
    let WriteError::Fail(CodecError::ResourceLimit(limit)) = error else {
        panic!("actual budget refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, OPERATION);
}

#[test]
fn subd_component_records_use_the_caller_entity_budget() {
    let source = triangle();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 6;
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).expect("session");
    let error = match payload(&ctx, &session, &source, RhinoArchiveVersion::V8) {
        Err(error) => error,
        Ok(_) => panic!("the seven component records exceed the limit"),
    };
    let WriteError::Fail(CodecError::ResourceLimit(limit)) = error else {
        panic!("the original caller limit is returned");
    };
    assert_eq!(limit.dimension, ResourceDimension::Entities);
    assert_eq!((limit.limit, limit.used, limit.additional), (6, 6, 1));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 7;
    let ctx = DecodeContext::new(&arena, &policy, cadmpeg_core::decode::DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).expect("session");
    let bytes = payload(&ctx, &session, &source, RhinoArchiveVersion::V8)
        .expect("all seven component records fit")
        .expect("triangle is writable");
    let decoded = crate::subd::decode(
        bytes.as_bytes(),
        0..bytes.len(),
        crate::chunks::ArchiveVersion::from_word(80),
        crate::settings::MillimeterScale::IDENTITY,
        source.id.clone(),
    )
    .expect("decode the written cage")
    .expect("SubD exists");
    assert_eq!(decoded.surface.cage.vertices().len(), 3);
    assert_eq!(decoded.surface.cage.edges().len(), 3);
    assert_eq!(decoded.surface.cage.faces().len(), 1);
}
