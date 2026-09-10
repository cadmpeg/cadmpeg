use super::*;

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
        let actual = match name {
            "cone" => crate::families::b2::records::b2_cones_from_records(&bytes, &consolidated)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "sphere" => {
                crate::families::b2::records::b2_spheres_from_records(&bytes, &consolidated)
                    .into_iter()
                    .map(|record| record.pos)
                    .collect::<Vec<_>>()
            }
            "torus" => crate::families::b2::records::b2_tori_from_records(&bytes, &consolidated)
                .into_iter()
                .map(|record| record.pos)
                .collect::<Vec<_>>(),
            "cylinder" => {
                crate::families::b2::records::b2_cylinders_from_records(&bytes, &consolidated)
                    .into_iter()
                    .map(|record| record.pos)
                    .collect::<Vec<_>>()
            }
            _ => unreachable!("unknown analytic carrier fixture: {name}"),
        };
        assert_eq!(actual, expected, "carrier decoder changed for {name}");
    }

    let bytes = b2_resolved_revolution_stream();
    let consolidated = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::families::b2::records::b2_resolved_revolutions_from_records(&bytes, &consolidated,),
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
        crate::families::b2::records::b2_reference_lists_from_records(&bytes, &records)
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
        crate::families::b2::records::b2_owner_packets_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_width_coded_owner_packet_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "width-coded owner packets",
        crate::families::b2::records::b2_owner_packets(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_owner_packets_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_counted_61_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "counted class 61",
        crate::families::b2::records::b2_counted_61(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_counted_61_from_records(&bytes, &records)
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
        crate::families::b2::records::b2_long_61_from_records(&bytes, &records)
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
        crate::families::b2::records::b2_class5b5c_records_from_records(&bytes, &records)
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
        crate::families::b2::records::b2_face_nodes_5f_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_adjacent_face_owner_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::families::b2::records::b2_adjacent_face_owners_from_records(&bytes, &records),
        crate::families::b2::records::b2_adjacent_face_owners(&bytes)
    );

    let bytes = b2_adjacent_face_counted_owner_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
            &bytes, &records
        ),
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
        crate::families::b2::records::b2_parameter_points_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_line_profile_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "line profiles",
        crate::families::b2::records::b2_line_profiles(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_line_profiles_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_spatial_circle_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "spatial circles",
        crate::families::b2::records::b2_spatial_circles(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_spatial_circles_from_records(&bytes, &records)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
    );

    let bytes = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    let records = crate::wire::records::consolidated_records(&bytes);
    compare(
        "NURBS curves",
        crate::families::b2::records::b2_nurbs_curves(&bytes)
            .into_iter()
            .map(|record| record.pos)
            .collect(),
        crate::families::b2::records::b2_nurbs_curves_from_records(&bytes, &records)
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
            record.domain,
        )
    };
    assert_eq!(
        crate::families::b2::records::b2_offset_supports(&bytes)
            .iter()
            .map(offset_signature)
            .collect::<Vec<_>>(),
        crate::families::b2::records::b2_offset_supports_from_records(&bytes, &records)
            .iter()
            .map(offset_signature)
            .collect::<Vec<_>>()
    );
}
