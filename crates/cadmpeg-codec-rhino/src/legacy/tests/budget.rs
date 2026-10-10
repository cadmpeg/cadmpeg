// SPDX-License-Identifier: Apache-2.0
use super::{chunk_at, legacy_face_archive, ArchiveVersion};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::{document::CadIr, math::Point3};
use cadmpeg_ir::{geometry::nurbs::NurbsCurve, topology::LoopBoundaryRole};

fn parsed_brep(data: &[u8]) -> super::super::LegacyBrep {
    let ctx = cadmpeg_test_support::service_decode_context();
    let comment = chunk_at(data, 32, data.len(), ArchiveVersion::V1, false).unwrap();
    let face = chunk_at(
        data,
        comment.next_offset(),
        data.len(),
        ArchiveVersion::V1,
        false,
    )
    .unwrap();
    let mut storage = ctx.reserve_scoped(0, "test parsing workspace").unwrap();
    super::super::legacy_brep(
        &ctx,
        &mut storage,
        data,
        &face,
        super::super::MillimeterScale::IDENTITY,
    )
    .unwrap()
}

#[test]
fn v1_brep_first_trim_index_refuses_before_topology_output() {
    let data = legacy_face_archive();
    for (dimension, operation) in [
        (
            ResourceDimension::MaterializedBytes,
            "Rhino V1 Brep first trim index",
        ),
        (
            ResourceDimension::WorkUnits,
            "Rhino V1 Brep first trim lookup",
        ),
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let brep = parsed_brep(&data);
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => unreachable!("test dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
            let mut ir = CadIr::empty();
            let result = super::super::append_legacy_brep(&ctx, &mut ir, brep, "indexed");
            assert_eq!(ir.model.entity_count(), 0);
            result
        });
    }
}

#[test]
fn v1_brep_first_trim_index_keeps_distinct_faces_and_loops() {
    let data = legacy_face_archive();
    let mut brep = parsed_brep(&data);
    let face = &mut brep.faces[0];
    face.loops.push(super::super::LegacyLoop {
        role: LoopBoundaryRole::Inner,
        trims: face.loops[0].trims.clone(),
    });
    let second = super::super::LegacyFace {
        reversed: face.reversed,
        seam_glue: Vec::new(),
        surface: face.surface.clone(),
        loops: face
            .loops
            .iter()
            .map(|row| super::super::LegacyLoop {
                role: row.role,
                trims: row.trims.clone(),
            })
            .collect(),
    };
    brep.faces.push(second);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    super::super::append_legacy_brep(&ctx, &mut ir, brep, "indexed").unwrap();
    assert_eq!(ir.model.faces.len(), 2);
    assert_eq!(ir.model.loops.len(), 4);
    assert_eq!(ir.model.edges.len(), 16);
    assert_eq!(ir.model.coedges.len(), 16);
    assert_eq!(ir.model.vertices.len(), 16);
    for row in &ir.model.loops {
        assert_eq!(row.coedges().len(), 4);
        let positions: Vec<_> = row
            .coedges()
            .iter()
            .map(|id| {
                ir.model
                    .coedges
                    .iter()
                    .position(|coedge| &coedge.id == id)
                    .unwrap()
            })
            .collect();
        for (slot, index) in positions.iter().enumerate() {
            let coedge = &ir.model.coedges[*index];
            assert_eq!(
                row.next_coedge(&coedge.id),
                Some(&ir.model.coedges[positions[(slot + 1) % 4]].id)
            );
            assert_eq!(
                row.previous_coedge(&coedge.id),
                Some(&ir.model.coedges[positions[(slot + 3) % 4]].id)
            );
            assert_eq!(coedge.radial_next, coedge.id);
        }
    }
}

#[test]
fn v1_topology_parent_walk_refuses_before_compression() {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino V1 topology parent walk",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut parents = [1, 2, 3, 3];
            let result = super::super::find_root(&ctx, &mut parents, 0);
            assert_eq!(parents, [1, 2, 3, 3]);
            result
        },
    );
}

#[test]
fn v1_curve_evaluation_charges_actual_quadratic_degree_steps() {
    let parse_ctx = cadmpeg_test_support::service_decode_context();
    let curve = NurbsCurve::from_checked_lanes(
        &parse_ctx,
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap()
    .unwrap();
    // Three pole initializations, two degree levels, and three blends.
    // The endpoint selects its span directly, so no knot-span search runs.
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let point = super::super::evaluate_nurbs(&ctx, &curve, 1.0).unwrap();
    assert_eq!(point, Point3::new(2.0, 0.0, 0.0));
    ctx.finish_session().unwrap();
}

#[test]
fn v1_fixed_annotation_points_need_no_work_units() {
    let data = [0_u8; 11 * 24];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut reader = super::super::BoundedReader::new(&data, 0, data.len()).unwrap();
    let points = super::super::v1_fixed_points::<11>(&ctx, &mut reader, "dimension point").unwrap();
    assert_eq!(points, vec![[cadmpeg_ir::scalar::FiniteReal::ZERO; 3]; 11]);
    assert_eq!(reader.remaining(), 0);
    ctx.finish_session().unwrap();
}

#[test]
fn v1_leader_point_traversal_refuses_before_coordinates() {
    let data = super::v1_annotation_records().remove(1);
    let chunk = chunk_at(&data, 0, data.len(), ArchiveVersion::V1, false).unwrap();
    let refusal = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino V1 leader point traversal",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
            super::super::v1_annotation(&ctx, &data, &chunk)
        },
    );
    assert!(
        matches!(refusal, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.used == 0 && limit.additional == 1)
    );
}

#[test]
fn v1_truncated_leader_points_have_no_traversal_charge() {
    let complete = super::v1_annotation_records().remove(1);
    let chunk = chunk_at(&complete, 0, complete.len(), ArchiveVersion::V1, false).unwrap();
    let body = chunk.body();
    let data = super::chunk(
        super::TCODE_ANNOTATION_LEADER,
        &complete[body.start..body.end - 24],
    );
    let chunk = chunk_at(&data, 0, data.len(), ArchiveVersion::V1, false).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let result = super::super::v1_annotation(&ctx, &data, &chunk);
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::Malformed(message))
        if message == "V1 leader point count exceeds the remaining bytes")
    );
    ctx.finish_session().unwrap();
}

#[test]
fn rejected_direct_records_release_partial_text_before_the_next_record() {
    const TEXT_BYTES: usize = 16 * 1024;
    const RECORD_COUNT: usize = 16;
    // One parser holds source and decoded text together. The remaining half
    // covers the source descriptors and copied diagnostics for all records.
    const SCRATCH_BYTES: u64 = 4 * 16 * 1024;
    let mut body = 2_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend(super::v1_plane());
    body.extend(super::v1_string(&"a".repeat(TEXT_BYTES)));
    // The flags field is absent, after both owned text buffers were built.
    let record = super::chunk(super::TCODE_TEXT_BLOCK, &body);
    let mut data = super::archive(&[]);
    for _ in 0..RECORD_COUNT {
        data.extend(&record);
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let decoded = super::super::decode_v1(&ctx, &data)
        .expect("discarded partial texts do not accumulate between records");
    assert_eq!(decoded.ir.model.entity_count(), 0);
    assert_eq!(
        decoded.source_fidelity.retained_records().len(),
        RECORD_COUNT
    );
    for retained in decoded.source_fidelity.retained_records().values() {
        assert_eq!(retained.data(), Some(record.as_slice()));
    }
    assert_eq!(
        decoded
            .body
            .notes
            .iter()
            .filter(|note| note.starts_with("V1 direct record at offset "))
            .count(),
        RECORD_COUNT
    );
    {
        let _reclaimed = ctx
            .reserve_scoped(SCRATCH_BYTES, "reclaimed V1 parser scratch")
            .expect("all source parsing storage is released when decoding returns");
    }
    ctx.finish_session().unwrap();
}

#[test]
fn direct_record_partial_text_preserves_original_scratch_refusal() {
    let mut body = 2_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend(super::v1_plane());
    body.extend(super::v1_string("text read before the missing flags"));
    let mut data = super::archive(&[]);
    data.extend(super::chunk(super::TCODE_TEXT_BLOCK, &body));
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "Rhino V1 decoded text",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy)?;
            let result = super::super::decode_v1(&ctx, &data);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*refusal));
                assert!(
                    matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == *refusal)
                );
            }
            result
        },
    );
}

#[test]
fn v1_curve_evaluation_refuses_first_blends_before_unused_degree_levels() {
    let parse_ctx = cadmpeg_test_support::service_decode_context();
    let curve = NurbsCurve::from_checked_lanes(
        &parse_ctx,
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap()
    .unwrap();
    // Three pole initializations and one executed degree level precede
    // admission of its two infallible blends. The later level is unvisited.
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::evaluate_nurbs(&ctx, &curve, 1.0).unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
        panic!("first blend refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino V1 curve evaluation");
    assert_eq!((refusal.used, refusal.additional), (4, 2));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == refusal)
    );
}

#[test]
fn standalone_v1_curve_promotes_its_owned_lanes() {
    let data = super::legacy_line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 3);
    let chunk = chunk_at(&data, 0, data.len(), ArchiveVersion::V1, false).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut workspace = ctx.reserve_scoped(0, "curve fixture backing").unwrap();
    let mut segments = super::super::legacy_curve_segments(
        &ctx,
        &mut workspace,
        &data,
        chunk.body(),
        super::super::MillimeterScale::IDENTITY,
    )
    .expect("valid curve lanes are scoped during parsing");
    assert_eq!(segments.len(), 1);
    let (curve, storage) = segments.pop().unwrap();
    assert_eq!(curve.pole_count(), 2);
    assert_eq!(curve.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
    let error = storage
        .commit_value(curve)
        .expect_err("surviving lanes need retained admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "Rhino V1 curve segment" && limit.additional > 0)
    );
}

#[test]
fn rejected_v1_meshes_release_partial_vertices_before_the_next_record() {
    const POINTS: i32 = 256;
    const RECORDS: usize = 16;
    const SCRATCH_BYTES: u64 = 32 * 1024;
    let mut body = Vec::new();
    for value in [POINTS, 1, 0, 0] {
        body.extend(value.to_le_bytes());
    }
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
        body.extend(value.to_le_bytes());
    }
    for _ in 0..POINTS {
        body.extend([0; 6]);
    }
    for value in [256_u16, 0, 0, 0] {
        body.extend(value.to_le_bytes());
    }
    let record = super::legacy_chunk(
        super::super::TCODE_MESH_OBJECT,
        &super::legacy_chunk(super::super::TCODE_COMPRESSED_MESH_GEOMETRY, &body),
    );
    let mut data = super::archive(&[]);
    for _ in 0..RECORDS {
        data.extend(&record);
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let decoded = super::super::decode_v1(&ctx, &data)
        .expect("failed mesh candidates release their partial vertex lanes");
    assert!(decoded.ir.model.tessellations.is_empty());
    assert_eq!(decoded.source_fidelity.retained_records().len(), RECORDS);
    assert_eq!(
        decoded
            .body
            .notes
            .iter()
            .filter(|note| note.starts_with("V1 mesh at offset "))
            .count(),
        RECORDS
    );
    let reclaimed = ctx
        .reserve_scoped(SCRATCH_BYTES, "reclaimed mesh scratch")
        .expect("mesh parsing storage is released after decode");
    drop(reclaimed);
    ctx.finish_session().unwrap();
}

#[test]
fn v1_brep_session_keeps_consecutive_commits_and_direct_model_mutations() {
    let data = legacy_face_archive();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = cadmpeg_ir::draft::CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
    for suffix in ["first", "second"] {
        super::super::append_legacy_brep_in_session(&ctx, &mut session, parsed_brep(&data), suffix)
            .unwrap();
    }
    assert_eq!(session.document().model.bodies.len(), 2);
    let mut existing = session.document().model.bodies[0].clone();
    existing.id = cadmpeg_ir::ids::BodyId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
        super::super::legacy_identity_key("third".to_owned()).unwrap(),
    );
    session.document_mut().unwrap().model.bodies.push(existing);
    let error = super::super::append_legacy_brep_in_session(
        &ctx,
        &mut session,
        parsed_brep(&data),
        "third",
    )
    .expect_err("direct mutation invalidates the cached identity index");
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(message) if message.contains("collision"))
    );
    assert_eq!(session.document().model.bodies.len(), 3);
}

#[test]
fn standalone_v1_curve_decode_requires_retained_admission() {
    let mut data = super::archive(&[]);
    data.extend(super::legacy_line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 3));
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "Rhino V1 curve segment",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy)?;
            super::super::decode_v1(&ctx, &data)
        },
    );
}


fn work_with_rejected_records_between_breps(count: usize, typecode: u32, diagnostic: &str) -> u64 {
    let fixture = legacy_face_archive();
    let comment = chunk_at(&fixture, 32, fixture.len(), ArchiveVersion::V1, false).unwrap();
    let brep = &fixture[comment.next_offset()..];
    let rejected = super::legacy_chunk(typecode, &[]);
    let mut data = super::archive(&[]);
    for _ in 0..count {
        data.extend(brep);
        data.extend(&rejected);
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let decoded = super::super::decode_v1(&ctx, &data).unwrap();
    assert_eq!(decoded.ir.model.bodies.len(), count);
    assert_eq!(decoded.source_fidelity.retained_records().len(), count * 2);
    assert_eq!(decoded.body.notes.iter().filter(|note| note.starts_with(diagnostic)).count(), count);
    // A final refusal exposes completed work without pinning individual charges.
    let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "completed V1 work").unwrap_err() else {
        panic!("work counter probe refuses");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn rejected_v1_meshes_do_not_restore_quadratic_brep_index_work() {
    let small = work_with_rejected_records_between_breps(64, super::super::TCODE_MESH_OBJECT, "V1 mesh at offset ");
    let large = work_with_rejected_records_between_breps(128, super::super::TCODE_MESH_OBJECT, "V1 mesh at offset ");
    assert!(large < 3 * small, "doubling records must remain below quadratic growth: {small} -> {large}");
}

#[test]
fn rejected_v1_curves_do_not_restore_quadratic_brep_index_work() {
    let small = work_with_rejected_records_between_breps(64, super::super::TCODE_LEGACY_CRV, "V1 curve at offset ");
    let large = work_with_rejected_records_between_breps(128, super::super::TCODE_LEGACY_CRV, "V1 curve at offset ");
    assert!(large < 3 * small, "doubling records must remain below quadratic growth: {small} -> {large}");
}
