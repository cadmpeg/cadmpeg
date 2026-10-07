// SPDX-License-Identifier: Apache-2.0

use super::super::decode_pcurves;
use super::{
    object_record, scan_with_objects, source_shaped_plane_brep, with_expand_bytes, ArchiveVersion,
    DecodeContext,
};

#[test]
fn shared_brep_c2_slot_preserves_each_trim_geometry() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.trims[1].curve = Some(0);
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate Brep with a shared C2 slot");
    let values = with_expand_bytes(&data, |expand| {
        decode_pcurves(
            (
                expand.ctx(),
                &mut expand
                    .ctx()
                    .reserve_scoped(0, "Rhino fixture arena scratch")
                    .expect("fixture scratch"),
            ),
            &data,
            ArchiveVersion::V5,
            brep.raw(),
            brep.resolved(),
            "plane",
            &std::collections::HashMap::new(),
        )
        .map(|decoded| decoded.values)
    })
    .expect("shared C2 slot decodes under the service profile");
    assert_eq!(values.len(), 3);
    assert_eq!(values[0].geometry, values[1].geometry);
    assert_ne!(values[0].id, values[1].id);
}

#[test]
fn brep_c2_cache_lookup_preserves_work_refusal() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.trims[1].curve = Some(0);
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate Brep with a shared C2 slot");
    let refused = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "Rhino Brep decoded C2 slots",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)?;
            let mut storage = ctx.reserve_scoped(0, "Rhino fixture arena scratch")?;
            decode_pcurves(
                (&ctx, &mut storage),
                &data,
                ArchiveVersion::V5,
                brep.raw(),
                brep.resolved(),
                "plane",
                &std::collections::HashMap::new(),
            )
            .map(|decoded| decoded.values)
            .map_err(|error| match error {
                crate::curves::GeometryError::Codec(error) => error,
                error => panic!("unexpected geometry error: {error}"),
            })
        },
    );
    assert!(
        matches!(refused, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "Rhino Brep decoded C2 slots")
    );
}

#[test]
fn brep_c2_cache_leaves_retention_for_output_knots() {
    let (data, raw) = source_shaped_plane_brep();
    let brep = with_expand_bytes(&data, |expand| {
        crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
    })
    .expect("validate Brep");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "Rhino Brep pcurve knots",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)?;
            let mut storage = ctx.reserve_scoped(0, "Rhino fixture arena scratch")?;
            decode_pcurves(
                (&ctx, &mut storage),
                &data,
                ArchiveVersion::V5,
                brep.raw(),
                brep.resolved(),
                "plane",
                &std::collections::HashMap::new(),
            )
            .map(|decoded| decoded.values)
            .map_err(|error| match error {
                crate::curves::GeometryError::Codec(error) => error,
                error => panic!("unexpected geometry error: {error}"),
            })
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.used == 0)
    );
}

#[test]
fn indexed_instance_dispatch_visits_only_selected_source() {
    let objects = (0..4)
        .map(|_| object_record(ArchiveVersion::V5, 8, [0; 16]))
        .collect::<Vec<_>>();
    let scan = scan_with_objects(&objects);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "Rhino object dispatch",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, root) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            let mut transaction =
                DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
            transaction.instance_selection = Some(super::super::InstanceSelection::new(
                &ctx,
                2,
                &[],
                crate::wire::Uuid::nil(),
            )?);
            transaction.decode_geometry()
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "Rhino object dispatch" && limit.additional == 1)
    );
}

#[test]
fn instance_curve_cache_moves_without_retained_copy() {
    let fixture_ctx = cadmpeg_test_support::service_decode_context();
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &fixture_ctx,
        1,
        (0..1002_u32).map(f64::from).collect::<Vec<_>>(),
        (0..1000_u32)
            .map(|index| cadmpeg_ir::math::Point3::new(f64::from(index), 0.0, 0.0))
            .collect::<Vec<_>>(),
        None,
        false,
    )
    .expect("fixture budget")
    .expect("valid curve");
    let scan = scan_with_objects(&[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 4096;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
            .expect("empty root");
    let mut transaction =
        DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).expect("transaction");
    let checkpoint =
        cadmpeg_ir::draft::ModelCheckpoint::capture(&transaction.session.document().model, &ctx)
            .expect("checkpoint");
    transaction
        .session
        .document_mut()
        .expect("fixture document")
        .model
        .curves
        .push(cadmpeg_ir::geometry::Curve {
            id: "rhino:object:curve#cache".try_into().expect("identity"),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Procedural {
                construction: "rhino:object:procedural-curve#cache"
                    .try_into()
                    .expect("identity"),
                cache: Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(curve)),
            },
            source_object: None,
        });
    let mut scratch = ctx
        .reserve_scoped(0, "instance cache move fixture")
        .expect("scratch");
    let links = transaction
        .transform_new_entities(
            &checkpoint.0,
            cadmpeg_ir::transform::Transform::identity(),
            &mut scratch,
        )
        .expect("move the cache and transform it within the retained limit");
    assert_eq!(links.len(), 1);
    let geometry = &transaction.session.document().model.curves[0].geometry;
    let cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(moved),
    ) = geometry
    else {
        panic!("instance carrier keeps the solved NURBS cache");
    };
    assert_eq!(moved.pole_count(), 1000);
    let cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } = moved.pole_rows() else {
        panic!("fixture has polynomial poles");
    };
    assert_eq!(
        points.first().expect("first pole").get(),
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(
        points.last().expect("last pole").get(),
        cadmpeg_ir::math::Point3::new(999.0, 0.0, 0.0)
    );
}

#[test]
fn object_key_copy_refuses_scratch_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, [0; 16])]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "Rhino object key copy",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, root) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            let mut transaction =
                DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
            transaction
                .checked_object_key(scan.objects[0].identity().expect("framed identity"), 0)
                .map(|key| key.is_some())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "Rhino object key copy"
            && limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn duplicate_instance_member_stops_before_unused_unique_keys() {
    let member = crate::wire::Uuid::nil();
    let other = crate::wire::Uuid::from_canonical([1; 16]);
    let last = crate::wire::Uuid::from_canonical([2; 16]);
    let members = [member, member, other, last];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // The first key consumes one collection item; the duplicate consumes none.
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(!super::super::instance_members_are_unique(&ctx, &members)
        .expect("unused suffix keys need no allocation"));
}

#[test]
fn staged_curve_arena_preserves_materialized_refusal() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "Rhino Brep curve arena",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let mut storage = ctx.reserve_scoped(0, "fixture arena scratch")?;
            let mut metadata_storage = ctx.reserve_scoped(0, "fixture link scratch")?;
            let mut staged = super::BrepDraft::default();
            super::stage_curve_tree(
                (&ctx, &mut storage, &mut metadata_storage),
                &mut staged,
                super::decoded_nurbs(super::line_nurbs(0.0, 1.0, false)),
                "curve",
                "root",
                &super::test_association(),
                &DecodeContext::mint_unknown_id(0),
            )
            .map_err(|error| match error {
                crate::curves::GeometryError::Codec(error) => {
                    if let cadmpeg_core::CodecError::ResourceLimit(ref limit) = error {
                        assert_eq!(ctx.resource_refusal(), Some(*limit));
                    }
                    error
                }
                error => panic!("unexpected geometry error: {error}"),
            })
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "Rhino Brep curve arena")
    );
}

#[test]
fn rejected_candidate_releases_arena_storage() {
    let scan = scan_with_objects(&[]);
    let point = cadmpeg_ir::topology::Point::new(
        "rhino:test:point#candidate".try_into().expect("identity"),
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
            .expect("finite point"),
        None,
    );
    super::with_transaction_limits(&scan, u64::MAX, Some(0), Some(8192), |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("empty transaction");
        let result = context.validate_candidate_fallible(|candidate, _annotations, storage| {
            expand.ctx().push_scoped_vec(
                storage,
                &mut candidate.model.points,
                point,
                "fixture candidate arena",
            )?;
            Err::<(), super::CandidateError>(super::CandidateError::Admission(
                "rejected".to_string(),
            ))
        });
        assert!(
            matches!(result, Err(super::CandidateError::Admission(ref message)) if message == "rejected")
        );
        assert!(context.session.document().model.points.is_empty());
        let storage = expand
            .ctx()
            .reserve_scoped(8192, "candidate arena storage released")
            .expect("candidate arena backing is no longer live");
        drop(storage);
    });
}

#[test]
fn hatch_link_bridge_uses_scratch_storage() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let feature_id = cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "hatch", "feature"),
        cadmpeg_ir::identity_key!("fixture"),
    );
    {
        let mut storage = ctx
            .reserve_scoped(0, "fixture link scratch")
            .expect("scratch");
        let loop_ids = super::hatch_loop_ids(
            &ctx,
            "fixture",
            std::iter::once(crate::hatch::LoopKind::Outer),
            &mut storage,
        )
        .expect("loop bridge uses no retained bytes");
        let links = super::hatch_source_links(&ctx, loop_ids, &feature_id, &mut storage)
            .expect("source bridge uses no retained bytes");
        assert_eq!(
            links,
            [
                "rhino:object:curve#fixture.hatch-loop-0",
                feature_id.as_str()
            ]
        );
    }
    let storage = ctx
        .reserve_scoped(4096, "link bridge storage released")
        .expect("bridge backing and text are no longer live");
    drop(storage);
}

#[test]
fn replacing_brep_fallback_cause_releases_previous_text() {
    let (data, mut raw) = source_shaped_plane_brep();
    raw.c3.slots = (0..100)
        .map(|_| {
            Some(crate::brep::RawBrepChild {
                class_uuid: crate::wire::Uuid::nil(),
                class_data_range: 0..0,
                source_range: 0..0,
            })
        })
        .collect();
    raw.surfaces.slots.clear();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)
        .expect("root bytes");
    let association = super::test_association();
    let unknown = DecodeContext::mint_unknown_id(0);
    let mut mesh_budget = crate::mesh::MeshBudget::new();
    let mut storage = ctx
        .reserve_scoped(0, "fixture Brep arena scratch")
        .expect("scratch");
    let mut metadata_storage = ctx
        .reserve_scoped(0, "fixture Brep link scratch")
        .expect("scratch");
    let carriers = super::super::stage_brep_carriers(
        super::super::BrepCarrierInput {
            expand: crate::mesh::MeshExpand::new(&ctx, root),
            data: &data,
            archive: ArchiveVersion::V5,
            writer_version: Some(200_206_180),
            raw: &raw,
            key: "fixture",
            association: &association,
            unknown: &unknown,
            scale: crate::settings::MillimeterScale::IDENTITY,
            mesh_budget: &mut mesh_budget,
        },
        true,
        &mut storage,
        &mut metadata_storage,
    )
    .expect("only the latest fallback cause remains live");
    assert!(carriers.staged.draft.model().curves.is_empty());
    assert!(carriers
        .child_cause
        .as_ref()
        .expect("fallback cause")
        .0
        .starts_with("C3 slot 99:"));
    drop(carriers);
    drop(storage);
    drop(metadata_storage);
    let storage = ctx
        .reserve_scoped(1024, "fallback cause storage released")
        .expect("all cause reservations are released");
    drop(storage);
}

#[test]
fn transformed_annotation_identity_uses_one_text_bridge() {
    let id = format!("rhino:test:point#{}", "x".repeat(1024));
    let scan = scan_with_objects(&[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1536;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
            .expect("empty root");
    let mut transaction =
        DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).expect("transaction");
    let checkpoint =
        cadmpeg_ir::draft::ModelCheckpoint::capture(&transaction.session.document().model, &ctx)
            .expect("checkpoint");
    transaction
        .ir_mut()
        .model
        .points
        .push(cadmpeg_ir::topology::Point::new(
            id.clone().try_into().expect("identity"),
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
                .expect("point"),
            None,
        ));
    let mut scratch = ctx
        .reserve_scoped(0, "fixture traversal scratch")
        .expect("scratch");
    let links = transaction
        .transform_new_entities(
            &checkpoint.0,
            cadmpeg_ir::transform::Transform::identity(),
            &mut scratch,
        )
        .expect("borrow the identity until annotation copies it");
    assert!(links.is_empty());
    assert_eq!(
        transaction.session.document().model.points[0].id.as_str(),
        id
    );
    assert_eq!(
        transaction
            .annotations
            .exactness()
            .get(&id)
            .expect("derived annotation")
            .entity(),
        cadmpeg_ir::Exactness::Derived
    );
    drop(links);
    drop(scratch);
    let storage = ctx
        .reserve_scoped(1536, "annotation bridge storage released")
        .expect("all annotation scratch is released");
    drop(storage);
}

#[test]
fn staged_curve_links_preserve_work_and_storage_refusals() {
    // Link text uses the same bytes as the earlier exactness lookup and does
    // not raise its scratch peak. Probe its work and the link buffer's storage.
    for (dimension, operation) in [
        (
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "Rhino Brep curve link text",
        ),
        (
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            "Rhino Brep carrier links",
        ),
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            if dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits {
                policy.limits.max_work_units = cap;
            } else {
                policy.limits.max_materialized_bytes = cap;
            }
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let mut arena_storage = ctx.reserve_scoped(0, "fixture arena scratch")?;
            let mut link_storage = ctx.reserve_scoped(0, "fixture link scratch")?;
            let mut staged = super::BrepDraft::default();
            super::stage_curve_tree(
                (&ctx, &mut arena_storage, &mut link_storage),
                &mut staged,
                super::decoded_nurbs(super::line_nurbs(0.0, 1.0, false)),
                "curve",
                "root",
                &super::test_association(),
                &DecodeContext::mint_unknown_id(0),
            )
            .map_err(|error| match error {
                crate::curves::GeometryError::Codec(error) => {
                    if let cadmpeg_core::CodecError::ResourceLimit(ref limit) = error {
                        assert_eq!(ctx.resource_refusal(), Some(*limit));
                    }
                    error
                }
                error => panic!("unexpected geometry error: {error}"),
            })
        });
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation && limit.dimension == dimension)
        );
    }
}
