// SPDX-License-Identifier: Apache-2.0

use super::super::decode_pcurves;
use super::{
    object_record, scan_with_objects, source_shaped_plane_brep, with_expand_bytes, ArchiveVersion,
    DecodeContext, POINT_CLASS,
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
        "Rhino Brep pcurve output",
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
            && limit.operation == "Rhino Brep pcurve output"
            && limit.used == 0)
    );
}

#[test]
fn indexed_instance_dispatch_visits_only_selected_source() {
    let objects = (0..4)
        .map(|_| object_record(ArchiveVersion::V5, 8, [0; 16]))
        .collect::<Vec<_>>();
    let scan = scan_with_objects(&objects);
    let run = |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
        let mut transaction = DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
        transaction.instance_selection = Some(super::super::InstanceSelection::new(
            &ctx,
            2,
            &[],
            crate::wire::Uuid::nil(),
        )?);
        transaction.decode_geometry()
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "Rhino object dispatch",
        run,
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("dispatch refusal");
    };
    assert_eq!(limit.operation, "Rhino object dispatch");
    assert_eq!(limit.additional, 1);
    run(limit.used + limit.additional).expect("only the selected dispatch consumes work");
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
    assert_eq!(links.values.len(), 1);
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
    assert!(links.values.is_empty());
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

#[test]
fn instance_member_rejection_charges_only_the_visited_prefix() {
    use crate::test_support::test_dump as bytes;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let archive = ArchiveVersion::V5;
    let members = (1..=128_u8).map(|index| [index; 16]).collect::<Vec<_>>();
    let definition = bytes::v5_definition_payload(archive, 7, [0x51; 16], &members, false);
    let reference = bytes::object_record_with_payload(
        archive,
        0x1000,
        bytes::INSTANCE_REFERENCE_CLASS,
        &bytes::instance_reference_payload([0x51; 16], bytes::transform(1.0, [0.0; 3])),
    );
    let mut scan = crate::container::scan_owned(bytes::document_with_definitions(
        "50",
        archive,
        &[bytes::definition_record(archive, &definition)],
        &[reference],
    ))
    .expect("instance scan");
    bytes::set_test_units(&mut scan, 1.0);
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
        let mut transaction = DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
        let mut scratch = ctx.reserve_scoped(0, "instance member prefix fixture")?;
        let result = transaction.expand_reference_inner(
            0,
            cadmpeg_ir::transform::Transform::identity(),
            &mut Vec::new(),
            &mut Vec::new(),
            &mut scratch,
        );
        match result {
            Err(super::super::ReferenceFailure::Codec(error)) => Err(error),
            Err(super::super::ReferenceFailure::Semantic(message)) => {
                assert!(message.contains("is missing"));
                assert_eq!(transaction.expansion_budget.members, 1);
                assert!(ctx.resource_refusal().is_none());
                Ok(message)
            }
            Ok(_) => panic!("the first member is absent"),
        }
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino instance definition members",
        run,
    );
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
        panic!("member visit must preserve its refusal");
    };
    assert_eq!(refusal.additional, 1);
    run(u64::MAX).expect("no unused member suffix is charged");
}

#[test]
fn borrowed_geometry_association_uses_scratch_and_preserves_output_fields() {
    use cadmpeg_core::decode::{
        refusal_probe::RefusalProbe, DecodeArena, DecodePolicy, ResourceDimension,
    };
    let mut scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let crate::objects::ObjectRecord::Framed(object) = &mut scan.objects[0] else {
        panic!("framed fixture object");
    };
    object.identity.name = "named point".repeat(64);
    let expected_name = object.identity.name.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut transaction =
        DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).unwrap();
    let _probe = RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "Rhino source association object ID",
        None,
    );
    assert!(transaction
        .commit_geometry(
            0,
            crate::curves::DecodedGeometry::Point {
                position: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    1.0, 2.0, 3.0
                ))
                .unwrap(),
                scaled: false,
            }
        )
        .expect("the borrowed template does not consume retained bytes"));
    let point = &transaction.session.document().model.points[0];
    let association = point
        .source_object
        .as_ref()
        .expect("output source association");
    assert_eq!(association.name.as_deref(), Some(expected_name.as_str()));
    assert_eq!(
        association.object_id.as_str(),
        scan.objects[0].identity().unwrap().object_id.to_string()
    );
    assert_eq!(
        point.position().get(),
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    );
    assert!(ctx.resource_refusal().is_none());
    drop(transaction);
    ctx.finish_session().unwrap();
}

#[test]
fn consumed_instance_link_buffers_release_backing_while_text_stays_live() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let scan = scan_with_objects(&[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
    let mut transaction =
        DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root)).unwrap();
    let checkpoint =
        cadmpeg_ir::draft::ModelCheckpoint::capture(&transaction.session.document().model, &ctx)
            .unwrap();
    transaction
        .ir_mut()
        .model
        .bodies
        .push(cadmpeg_ir::topology::Body {
            id: "rhino:test:body#one".try_into().unwrap(),
            name: None,
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: Vec::new(),
            color: None,
            visible: None,
            transform: None,
        });
    let mut text_storage = ctx.reserve_scoped(0, "fixture live instance text").unwrap();
    let (mut collected, mut backing) = ctx.temporary_vec(64, "fixture aggregate backing").unwrap();
    for _ in 0..64 {
        let child = transaction
            .transform_new_entities(
                &checkpoint.0,
                cadmpeg_ir::transform::Transform::identity(),
                &mut text_storage,
            )
            .expect("only live source and aggregate backing use temporary storage");
        backing
            .with_storage(|| {
                ctx.extend_vec(&mut collected, child.values, "fixture aggregate links")
            })
            .unwrap();
    }
    assert_eq!(collected.len(), 64);
    assert!(collected.iter().all(|id| id == "rhino:test:body#one"));
    assert!(ctx.resource_refusal().is_none());
    drop(collected);
    drop(backing);
    drop(text_storage);
    drop(checkpoint);
    drop(transaction);
    let released = ctx
        .reserve_scoped(4096, "all instance scratch released")
        .unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

#[test]
fn instance_point_placement_rejects_first_point_without_charging_unused_suffix() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let scan = scan_with_objects(&[]);
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
        let mut transaction = DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
        let checkpoint = cadmpeg_ir::draft::ModelCheckpoint::capture(
            &transaction.session.document().model,
            &ctx,
        )?;
        for index in 0..128 {
            transaction
                .ir_mut()
                .model
                .points
                .push(cadmpeg_ir::topology::Point::new(
                    format!("rhino:test:point#{index}").try_into().unwrap(),
                    cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                        f64::MAX,
                        0.0,
                        0.0,
                    ))
                    .unwrap(),
                    None,
                ));
        }
        let transform = cadmpeg_ir::transform::Transform::affine([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let mut scratch = ctx.reserve_scoped(0, "placement prefix fixture")?;
        let result = transaction.transform_new_entities(&checkpoint.0, transform, &mut scratch);
        match result {
            Err(super::super::ReferenceFailure::Codec(error)) => Err(error),
            Err(super::super::ReferenceFailure::Semantic(message)) => {
                assert_eq!(message, super::super::NON_FINITE_PLACEMENT);
                assert!(ctx.resource_refusal().is_none());
                assert_eq!(
                    transaction.session.document().model.points[0].position().x,
                    f64::MAX
                );
                Ok(())
            }
            Ok(_) => panic!("the first point must overflow"),
        }
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino transformed entity traversal",
        run,
    );
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
        panic!("point visit must preserve its refusal");
    };
    assert_eq!(refusal.additional, 1);
    run(refusal.used + 1).expect("one visited point reaches the semantic rejection");
}

#[test]
fn mesh_source_association_is_constructed_after_payload_decode() {
    use crate::test_support::test_dump as bytes;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let mut scan = scan_with_objects(&[bytes::object_record_with_payload(
        ArchiveVersion::V5,
        0x20,
        crate::mesh::ON_MESH.to_wire(),
        &crate::test_support::test_archive::mesh_payload(3, 0, false, false),
    )]);
    bytes::set_test_units(&mut scan, 1.0);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "Rhino source association object ID",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, root) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            let mut transaction =
                DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
            let result = transaction.decode_geometry();
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result {
                if refusal.operation == "Rhino source association object ID" {
                    assert!(
                        transaction.mesh_budget.used() > 0,
                        "mesh payload is decoded before its final association"
                    );
                    assert!(transaction
                        .session
                        .document()
                        .model
                        .tessellations
                        .is_empty());
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
            }
            result
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.operation == "Rhino source association object ID" && refusal.dimension == ResourceDimension::RetainedBytes)
    );
    super::with_expand(&scan, |expand| {
        let mut transaction = DecodeContext::new(&scan, expand).unwrap();
        transaction.decode_geometry().unwrap();
        let meshes = &transaction.session.document().model.tessellations;
        assert_eq!(meshes.len(), 1);
        let source = meshes[0].source_object.as_ref().expect("final association");
        assert_eq!(
            source.object_id.as_str(),
            scan.objects[0].identity().unwrap().object_id.to_string()
        );
    });
}

#[test]
fn hatch_placement_rejection_charges_only_the_first_loop() {
    use crate::test_support::{test_archive as curves, test_dump as bytes};
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let run = |count, cap, measure_end| {
        let archive = ArchiveVersion::V5;
        let mut payload = vec![0x10];
        for value in [
            0.0,
            0.0,
            0.0, // origin
            f64::MAX,
            0.0,
            0.0, // x axis
            0.0,
            1.0,
            0.0, // y axis
            0.0,
            0.0,
            1.0, // z axis
            0.0,
            0.0,
            1.0,
            0.0, // equation
            1.0,
            0.0, // pattern scale and rotation
        ] {
            bytes::push_f64(&mut payload, value);
        }
        bytes::push_i32(&mut payload, 0);
        bytes::push_i32(&mut payload, i32::try_from(count).unwrap());
        let mut line = curves::line_payload([2.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]);
        line.truncate(line.len() - std::mem::size_of::<i32>());
        bytes::push_i32(&mut line, 2);
        let child = bytes::class_wrapper(archive, curves::LINE_CLASS, &line);
        for _ in 0..count {
            payload.push(0x10);
            bytes::push_i32(&mut payload, 0);
            payload.extend_from_slice(&child);
        }
        let scan = scan_with_objects(&[bytes::object_record_with_payload(
            archive,
            0x1_0000,
            crate::hatch::CLASS.to_wire(),
            &payload,
        )]);

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
        let mut transaction = DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
        transaction.decode_geometry()?;
        assert!(transaction.session.document().model.curves.is_empty());
        assert_eq!(
            transaction.statuses[0],
            Some(super::GeometryOutcome::Failed)
        );
        assert!(transaction
            .report
            .phase_warnings
            .iter()
            .any(|warning| warning.contains("hatch loop placement failed")));
        assert!(ctx.resource_refusal().is_none());
        if measure_end {
            ctx.next_charged(&mut std::iter::once(()), "prefix completed")?;
        }
        Ok::<(), cadmpeg_core::CodecError>(())
    };
    assert_prefix_work("Rhino hatch placement traversal", run);
}

#[test]
fn brep_scaled_vertex_rejection_charges_only_the_first_vertex() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let run = |count, cap, measure_end| {
        let (data, mut raw) = source_shaped_plane_brep();
        raw.c2.slots.clear();
        raw.c3.slots.clear();
        raw.surfaces.slots.clear();
        raw.edges.clear();
        raw.trims.clear();
        raw.loops.clear();
        raw.faces.clear();
        raw.vertices = (0..i32::try_from(count).unwrap())
            .map(|index| crate::brep::RawBrepVertex {
                index,
                point: crate::settings::CoordinateLane::Admitted(
                    crate::test_support::point3([
                        if index == 0 { f64::MAX } else { 0.0 },
                        0.0,
                        0.0,
                    ])
                    .0,
                ),
                edges: Vec::new(),
                tolerance: 0.0,
                source_range: 0..0,
            })
            .collect();
        let brep = super::with_expand_bytes(&data, |expand| {
            crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw)
        })
        .unwrap();

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&data, &arena, &policy)?;
        let mut backing = ctx.reserve_scoped(0, "vertex prefix fixture backing")?;
        let mut metadata = ctx.reserve_scoped(0, "vertex prefix fixture metadata")?;
        let association = super::test_association();
        let unknown = DecodeContext::mint_unknown_id(0);
        let mut budget = crate::mesh::MeshBudget::new();
        match super::stage_brep(
            super::BrepTransferInput {
                expand: crate::mesh::MeshExpand::new(&ctx, root),
                data: &data,
                archive: ArchiveVersion::V5,
                writer_version: Some(200_206_180),
                brep: &brep,
                key: "prefix",
                association: &association,
                unknown: &unknown,
                scale: crate::test_support::millimeter_scale(2.0),
                mesh_budget: &mut budget,
            },
            &mut backing,
            &mut metadata,
        ) {
            Err(crate::curves::GeometryError::Codec(error)) => Err(error),
            Err(error) => {
                assert!(error
                    .to_string()
                    .contains("scaled Brep vertex coordinate is invalid"));
                assert!(ctx.resource_refusal().is_none());
                if measure_end {
                    ctx.next_charged(&mut std::iter::once(()), "prefix completed")?;
                }
                Ok(())
            }
            Ok(_) => panic!("the first vertex must overflow"),
        }
    };
    assert_prefix_work("Rhino stage brep traversal", run);
}

#[test]
fn extrusion_cap_rejection_charges_only_the_first_boundary() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let mut extrusion = super::cap_extrusion([true, false]);
    extrusion.boundaries[0].start_pcurve.knots.clear();
    let boundaries = (0..128)
        .map(|_| super::CommittedExtrusionBoundary {
            boundary: &extrusion.boundaries[0],
            directrix: "rhino:test:curve#cap".try_into().unwrap(),
        })
        .collect::<Vec<_>>();
    let run = |count, cap, measure_end| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let mut backing = ctx.reserve_scoped(0, "cap prefix fixture backing")?;
        match super::stage_extrusion_caps(
            (&ctx, &mut backing),
            &mut cadmpeg_ir::CadIr::empty(),
            &mut cadmpeg_ir::Annotations::default(),
            "caps",
            &super::test_association(),
            &extrusion,
            &boundaries[..count],
        ) {
            Err(super::CandidateError::Codec(error)) => Err(error),
            Err(super::CandidateError::Admission(message)) => {
                assert!(message.contains("pcurve knot count 0"));
                assert!(ctx.resource_refusal().is_none());
                if measure_end {
                    ctx.next_charged(&mut std::iter::once(()), "prefix completed")?;
                }
                Ok(())
            }
            other => panic!("unexpected cap rejection: {other:?}"),
        }
    };
    assert_prefix_work("Rhino stage extrusion caps traversal", run);
}

#[test]
fn plane_pcurve_overflow_charges_only_the_first_pcurve() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let (data, raw) = source_shaped_plane_brep();
    let mut fixture = super::with_expand_bytes(&data, |expand| {
        let brep = crate::brep::ValidatedRawBrep::try_new(expand.ctx(), raw).unwrap();
        super::stage_brep(
            super::BrepTransferInput {
                expand,
                data: &data,
                archive: ArchiveVersion::V5,
                writer_version: Some(200_206_180),
                brep: &brep,
                key: "plane-prefix",
                association: &super::test_association(),
                unknown: &DecodeContext::mint_unknown_id(0),
                scale: crate::test_support::millimeter_scale(1.0),
                mesh_budget: &mut crate::mesh::MeshBudget::new(),
            },
            &mut expand
                .ctx()
                .reserve_scoped(0, "plane prefix fixture backing")
                .unwrap(),
            &mut expand
                .ctx()
                .reserve_scoped(0, "plane prefix fixture metadata")
                .unwrap(),
        )
        .unwrap()
    });
    let pcurve = &mut fixture.draft.model_mut().pcurves[0];
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &mut pcurve.geometry else {
        panic!("fixture has a NURBS pcurve");
    };
    nurbs
        .try_map_control_points(
            |_, _| {
                Ok::<_, ()>(
                    cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                        f64::MAX,
                        0.0,
                    ))
                    .unwrap(),
                )
            },
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap()
        .unwrap();
    let pcurve = pcurve.clone();
    fixture.draft.model_mut().pcurves = (0..128).map(|_| pcurve.clone()).collect();
    let run = |count, cap, measure_end| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let mut staged = super::BrepDraft::default();
        *staged.draft.model_mut() = fixture.draft.model().clone();
        staged.draft.model_mut().pcurves.truncate(count);
        match crate::decode::scale_plane_pcurves(
            &ctx,
            &mut staged,
            crate::test_support::millimeter_scale(2.0),
        ) {
            Err(crate::curves::GeometryError::Codec(error)) => Err(error),
            Err(error) => {
                assert!(error
                    .to_string()
                    .contains("control_points contains a non-finite point"));
                assert!(ctx.resource_refusal().is_none());
                if measure_end {
                    ctx.next_charged(&mut std::iter::once(()), "prefix completed")?;
                }
                Ok(())
            }
            Ok(()) => panic!("the first pcurve must overflow"),
        }
    };
    assert_prefix_work("Rhino plane pcurve traversal", run);
}

fn assert_prefix_work(
    operation: &str,
    mut run: impl FnMut(usize, u64, bool) -> Result<(), cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::ResourceDimension;
    let mut suffix_cost = None;
    for count in [1, 128] {
        let start = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| run(count, cap, false),
        );
        let cadmpeg_core::CodecError::ResourceLimit(start) = start else {
            panic!("prefix visit refusal");
        };
        assert_eq!(start.additional, 1);
        let end = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "prefix completed",
            |cap| run(count, cap, true),
        );
        let cadmpeg_core::CodecError::ResourceLimit(end) = end else {
            panic!("completed prefix refusal");
        };
        let cost = end
            .used
            .checked_sub(start.used)
            .expect("completion follows first visit");
        if let Some(previous) = suffix_cost {
            assert_eq!(cost, previous, "unused suffix adds no work");
        }
        suffix_cost = Some(cost);
        run(count, end.used, false).expect("exact completed-prefix budget suffices");
    }
}
