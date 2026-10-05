use crate::families::b2::records::tests::b2_nurbs_curve_stream;
use crate::families::b2::records::tests::b2_spatial_circle_stream;
use crate::test_support::test_b2::{
    b2_adjacent_face_counted_owner_stream, b2_adjacent_face_owner_stream, b2_class5b5c_stream,
    b2_cone_stream, b2_construction_use_stream, b2_counted_61_stream, b2_cylinder_stream,
    b2_face_node_5f_stream, b2_line_profile_stream, b2_long_61_stream, b2_owner_packet_stream,
    b2_parameter_point_stream, b2_reference_list_stream, b2_resolved_revolution_stream,
    b2_sphere_stream, b2_torus_stream, b2_width_coded_owner_packet_stream,
};

#[test]
fn indexed_analytic_carrier_decoders_match_one_shot_wrappers() {
    let cases = [
        ("cone", b2_cone_stream()),
        ("sphere", b2_sphere_stream()),
        ("torus", b2_torus_stream()),
        ("cylinder", b2_cylinder_stream()),
    ];
    for (name, bytes) in cases {
        let consolidated = crate::wire::records::consolidated_records(&bytes);
        let expected = match name {
            "cone" => crate::families::b2::records::b2_cones(&bytes)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "sphere" => crate::families::b2::records::b2_spheres(&bytes)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "torus" => crate::families::b2::records::b2_tori(&bytes)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "cylinder" => crate::families::b2::records::b2_cylinders(&bytes)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            _ => unreachable!("unknown analytic carrier fixture: {name}"),
        };
        let actual = crate::test_support::with_service_context(|ctx| match name {
            "cone" => ctx.collect_vec(
                crate::families::b2::records::b2_cones_from_records(ctx, &bytes, &consolidated)?
                    .map(|record| record.pos),
                "catia_test_indexed_cones",
            ),
            "sphere" => ctx.collect_vec(
                crate::families::b2::records::b2_spheres_from_records(ctx, &bytes, &consolidated)?
                    .map(|record| record.pos),
                "catia_test_indexed_spheres",
            ),
            "torus" => ctx.collect_vec(
                crate::families::b2::records::b2_tori_from_records(ctx, &bytes, &consolidated)?
                    .map(|record| record.pos),
                "catia_test_indexed_tori",
            ),
            "cylinder" => ctx.try_collect_vec(
                crate::families::b2::records::b2_cylinders_from_records(
                    ctx,
                    &bytes,
                    &consolidated,
                )?
                .map(|record| record.map(|record| record.pos)),
                "catia_test_indexed_cylinders",
            ),
            _ => unreachable!("unknown analytic carrier fixture: {name}"),
        })
        .expect("service context admits indexed carriers");
        assert_eq!(actual, expected, "carrier decoder changed for {name}");
    }

    let bytes = b2_resolved_revolution_stream();
    let consolidated = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_resolved_revolutions_from_records(
                ctx,
                &bytes,
                &consolidated,
            )
        })
        .expect("service decode"),
        crate::families::b2::records::b2_resolved_revolutions(&bytes)
    );
}

#[test]
fn indexed_native_record_decoders_match_one_shot_wrappers() {
    let compare = |name: &str, one_shot: Vec<usize>, indexed: Vec<usize>| {
        assert_eq!(indexed, one_shot, "indexed decoder changed for {name}");
    };

    let bytes = b2_reference_list_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "reference lists",
        crate::families::b2::records::b2_reference_lists(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_reference_lists_from_records(ctx, &bytes, &records)
                .expect("service decode")
        })
        .into_iter()
        .map(|record| record.pos)
        .collect(),
    );

    let bytes = b2_owner_packet_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "owner packets",
        crate::families::b2::records::b2_owner_packets(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            ctx.collect_vec(
                crate::families::b2::records::b2_owner_packets_from_records(ctx, &bytes, &records)?
                    .map(|record| record.pos),
                "catia_test_indexed_owner_packets",
            )
        })
        .expect("service context admits indexed owner packets"),
    );

    let bytes = b2_width_coded_owner_packet_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "width-coded owner packets",
        crate::families::b2::records::b2_owner_packets(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            ctx.collect_vec(
                crate::families::b2::records::b2_owner_packets_from_records(ctx, &bytes, &records)?
                    .map(|record| record.pos),
                "catia_test_indexed_owner_packets",
            )
        })
        .expect("service context admits indexed owner packets"),
    );

    let bytes = b2_counted_61_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "counted class 61",
        crate::families::b2::records::b2_counted_61(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_counted_61_from_records(ctx, &bytes, &records)
                .expect("service decode")
        })
        .into_iter()
        .map(|record| record.pos)
        .collect(),
    );

    let bytes = b2_long_61_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "long class 61",
        crate::families::b2::records::b2_long_61(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_long_61_from_records(ctx, &bytes, &records)
                .expect("service decode")
        })
        .into_iter()
        .map(|record| record.pos)
        .collect(),
    );

    let bytes = b2_class5b5c_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "class 5b/5c control records",
        crate::families::b2::records::b2_class5b5c_records(&bytes)
            .into_iter()
            .map(|record| record.frame.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_class5b5c_records_from_records(ctx, &bytes, &records)
                .expect("service decode")
        })
        .into_iter()
        .map(|record| record.frame.pos)
        .collect(),
    );

    let bytes = b2_face_node_5f_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "class 5f face nodes",
        crate::families::b2::records::b2_face_nodes_5f(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            ctx.collect_vec(
                crate::families::b2::records::b2_face_nodes_5f_from_records(ctx, &bytes, &records)?
                    .map(|record| record.pos),
                "catia_test_indexed_face_nodes",
            )
        })
        .expect("service context admits indexed face nodes"),
    );

    let bytes = b2_adjacent_face_owner_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_adjacent_face_owners_from_records(
                ctx, &bytes, &records,
            )
            .expect("service decode")
        }),
        crate::families::b2::records::b2_adjacent_face_owners(&bytes)
    );

    let bytes = b2_adjacent_face_counted_owner_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
                ctx, &bytes, &records,
            )
        })
        .expect("service context admits adjacent counted owners"),
        crate::families::b2::records::b2_adjacent_face_counted_owners(&bytes)
    );

    let bytes = b2_parameter_point_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "parameter points",
        crate::families::b2::records::b2_parameter_points(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            ctx.collect_vec(
                crate::families::b2::records::b2_parameter_points_from_records(
                    ctx, &bytes, &records,
                )?
                .map(|record| record.pos),
                "catia_test_indexed_parameter_points",
            )
        })
        .expect("service context admits indexed parameter points"),
    );

    let bytes = b2_line_profile_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "line profiles",
        crate::families::b2::records::b2_line_profiles(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            ctx.collect_vec(
                crate::families::b2::records::b2_line_profiles_from_records(ctx, &bytes, &records)?
                    .map(|record| record.pos),
                "catia_test_indexed_line_profiles",
            )
        })
        .expect("service context admits indexed line profiles"),
    );

    let bytes = b2_spatial_circle_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "spatial circles",
        crate::families::b2::records::b2_spatial_circles(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            ctx.collect_vec(
                crate::families::b2::records::b2_spatial_circles_from_records(
                    ctx, &bytes, &records,
                )?
                .map(|record| record.pos),
                "catia_test_indexed_spatial_circles",
            )
        })
        .expect("service context admits indexed spatial circles"),
    );

    let bytes = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "NURBS curves",
        super::parsed_b2_nurbs_curves(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_nurbs_curves_from_records(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        })
        .into_iter()
        .map(|record| record.pos)
        .collect(),
    );

    let bytes = b2_construction_use_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let offset_signature = |record: &crate::families::b2::records::B2OffsetSupport| {
        (
            record.pos,
            record.support_id,
            record.distance,
            record.u_range,
            record.v_range,
        )
    };
    assert_eq!(
        crate::families::b2::records::b2_offset_supports(&bytes)
            .iter()
            .map(offset_signature)
            .collect::<Vec<_>>(),
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_offset_supports_from_records(ctx, &bytes, &records)
                .expect("service decode")
        })
        .iter()
        .map(offset_signature)
        .collect::<Vec<_>>()
    );
}

#[test]
fn b2_long61_member_scan_propagates_caller_work_refusal() {
    let bytes = b2_long_61_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    // One record source visit precedes admission of the three members.
    crate::test_support::with_work_limit(1, |ctx| {
        let result = crate::families::b2::records::b2_long_61_from_records(ctx, &bytes, &records);
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("member order scan work refusal required")
        };
        assert_eq!(limit.operation, "catia_b2_long61_member_order");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn b2_revolution_identity_scan_propagates_caller_work_refusal() {
    let bytes = b2_resolved_revolution_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    // Two record admissions (2 + 2), two circle collector steps, and one outer collector step.
    crate::test_support::with_work_limit(7, |ctx| {
        let result = crate::families::b2::records::b2_resolved_revolutions_from_records(
            ctx, &bytes, &records,
        );
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("revolution identity scan work refusal required")
        };
        assert_eq!(limit.operation, "catia_b2_revolution_identity_profiles");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn b2_revolution_circle_record_scan_propagates_work_refusal() {
    let bytes = b2_resolved_revolution_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let service = crate::test_support::with_service_context(|ctx| {
        crate::families::b2::records::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
    })
    .expect("service context admits the circle and revolution records");
    assert_eq!(service.len(), 1);

    let operation = "catia_b2_family_record_scan";
    let refused = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = crate::families::b2::records::b2_resolved_revolutions_from_records(
            ctx, &bytes, &records,
        )
        .map(|_| ());
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == operation
    ));
}

#[test]
fn b2_offset_source_scan_refuses_even_without_carriers() {
    let offsets = crate::families::b2::records::b2_offset_supports(
        &crate::test_support::test_b2::b2_offset_support_stream(),
    );
    crate::test_support::with_work_limit(0, |ctx| {
        let result = crate::families::b2::records::offset_support_carriers(ctx, &offsets, &[]);
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("offset source scan work refusal required")
        };
        assert_eq!(limit.operation, "catia_b2_offset_carrier_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn b2_cone_face_byte_scan_propagates_caller_work_refusal() {
    let bytes = crate::test_support::test_b2::b2_cone_face_stream();
    crate::test_support::with_work_limit(0, |ctx| {
        let result = crate::families::b2::records::b2_cone_faces(ctx, &bytes);
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
            panic!("cone face byte scan work refusal required")
        };
        assert_eq!(limit.operation, "catia_b2_cone_face_byte_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn counted_owner_index_collection_propagates_work_refusal() {
    let bytes = b2_adjacent_face_counted_owner_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let operation = "catia_b2_counted_owner_index";
    let mut run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let result = crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
            ctx, &bytes, &records,
        );
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    };
    let admitted = crate::test_support::with_service_context(&mut run).expect("service budget");
    assert!(!admitted.is_empty());
    let refused = crate::test_support::with_work_refusal(operation, &mut run);
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == operation
    ));
}
