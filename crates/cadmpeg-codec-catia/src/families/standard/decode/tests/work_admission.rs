use crate::families::standard::decode::merge_standard_edge_vertex_references;
use crate::families::standard::decode::split_bezier_half;
use cadmpeg_ir::math::Point3;
use std::collections::BTreeMap;

#[test]
fn bezier_half_split_preserves_midpoints() {
    let control = std::array::from_fn(|index| {
        Point3::new(
            f64::from(u32::try_from(index).expect("six controls")),
            0.0,
            0.0,
        )
    });
    let (left, right) = split_bezier_half(control);
    for index in 0..6 {
        let index = u32::try_from(index).expect("six controls");
        assert_eq!(
            left[usize::try_from(index).expect("control index")],
            Point3::new(f64::from(index) * 0.5, 0.0, 0.0)
        );
        assert_eq!(
            right[usize::try_from(index).expect("control index")],
            Point3::new(2.5 + f64::from(index) * 0.5, 0.0, 0.0)
        );
    }
}

#[test]
fn standard_edge_merge_propagates_source_work_refusal() {
    crate::test_support::with_work_limit(0, |ctx| {
        let mut target = BTreeMap::from([(70, [500, 300])]);
        let source = BTreeMap::from([(90, [100, 500])]);
        let error =
            merge_standard_edge_vertex_references(ctx, &mut target, &source, |vertices| *vertices)
                .expect_err("edge source must be admitted");
        let cadmpeg_core::CodecError::ResourceLimit(error) = error else {
            panic!("resource refusal required")
        };
        assert_eq!(error.operation, "catia_standard_e5_topology_edges");
        assert_eq!(ctx.resource_refusal(), Some(error));
        assert_eq!(target, BTreeMap::from([(70, [500, 300])]));
    });
}

#[test]
fn circle_selection_admits_caller_recursion_depth() {
    use crate::families::standard::decode::edge_geometry::circular_range_choices_have_simple_selection;
    let choices = vec![[[0.0, 1.0]]; 32];
    assert!(crate::test_support::with_service_context(|ctx| {
        circular_range_choices_have_simple_selection(ctx, &choices)
    })
    .expect("compatible coincident ranges"));
    crate::test_support::with_depth_limit(4, |ctx| {
        let error = circular_range_choices_have_simple_selection(ctx, &choices)
            .expect_err("fifth selection frame must refuse");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("depth refusal")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        assert_eq!(limit.operation, "catia_standard_circle_range_selection");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn bezier_heap_admits_sifting_and_releases_search_storage() {
    use crate::families::standard::decode::collect_bezier_point_parameters;
    let control = std::array::from_fn(|index| {
        Point3::new(
            f64::from(u32::try_from(index).expect("six poles")),
            0.0,
            0.0,
        )
    });
    for operation in ["catia_bezier_search_queue", "catia_bezier_search_work"] {
        let error = crate::test_support::with_work_refusal(operation, |ctx| {
            let mut parameters = Vec::new();
            collect_bezier_point_parameters(
                ctx,
                control,
                [0.0, 1.0],
                Point3::new(2.5, 0.0, 0.0),
                0.001,
                1.0,
                &mut parameters,
            )?;
            Ok(parameters)
        })
        .expect_err("heap work must be admitted");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }
    crate::test_support::with_retained_limit(0, |ctx| {
        // The retained destination already owns two slots. All search storage
        // must use materialized bytes, and both admitted solutions stay intact.
        let mut parameters = Vec::with_capacity(2);
        collect_bezier_point_parameters(
            ctx,
            control,
            [0.0, 1.0],
            Point3::new(2.5, 0.0, 0.0),
            0.001,
            1.0,
            &mut parameters,
        )
        .expect("search storage is temporary");
        assert_eq!(parameters, vec![(0.5, 0.0), (0.5, 0.0)]);
    });
}

#[test]
fn native_point_index_filters_all_coordinates_on_a_constant_x_plane() {
    use crate::families::b5::graph::B5LogicalVertex;
    use crate::families::standard::decode::unique_native_identity_points;
    use cadmpeg_ir::ids::PointId;
    use cadmpeg_ir::topology::Point;
    let points = (0..1024_u32)
        .map(|index| {
            Point::new(
                PointId::mint(format!("catia:test:point#{index}")).expect("identity"),
                crate::test_support::test_b5::point([0.0, f64::from(index), 0.0]),
                None,
            )
        })
        .collect::<Vec<_>>();
    let vertices = points
        .iter()
        .enumerate()
        .map(|(index, point)| B5LogicalVertex {
            object_id: u32::try_from(index).expect("bounded index"),
            point: point.position(),
        })
        .collect::<Vec<_>>();
    let error =
        crate::test_support::with_work_refusal("catia_native_identity_point_match", |ctx| {
            unique_native_identity_points(ctx, &vertices, points.len(), &BTreeMap::new(), &points)
        })
        .expect_err("probe first query");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal")
    };
    // Admit the measured construction prefix, then 500,000 units for all
    // queries and output growth. A Cartesian query needs 1,048,576 visits.
    crate::test_support::with_work_limit(limit.used + 500_000, |ctx| {
        let matches =
            unique_native_identity_points(ctx, &vertices, points.len(), &BTreeMap::new(), &points)
                .expect("balanced spatial lookup");
        assert_eq!(matches.len(), points.len());
        for (index, vertex) in vertices.iter().enumerate() {
            assert_eq!(matches.get(&vertex.object_id), Some(&index));
        }
    });
}

#[test]
fn face_witness_spatial_dedup_preserves_first_order_and_limits_visits() {
    use super::super::distinct_face_witnesses;
    let points = (0..1024)
        .map(|index| Point3::new(0.0, f64::from(index), 0.0))
        .collect::<Vec<_>>();
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| distinct_face_witnesses(ctx, &points);
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        crate::test_support::with_work_refusal("catia_a5_distinct_face_witnesses", run)
            .expect_err("first visit")
    else {
        panic!("work refusal");
    };
    assert_eq!(
        crate::test_support::with_work_limit(limit.used + 200_000, run).expect("spatial visits"),
        points
    );
    let tolerance = super::super::NURBS_SURFACE_MEMBERSHIP_TOLERANCE;
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(tolerance * 0.75, 0.0, 0.0),
        Point3::new(tolerance * 1.5, 0.0, 0.0),
    ];
    assert_eq!(
        crate::test_support::with_service_context(|ctx| distinct_face_witnesses(ctx, &points))
            .expect("service"),
        vec![points[0], points[2]]
    );
}

#[test]
fn a5_bounds_binding_avoids_separated_owner_carrier_and_face_products() {
    use super::super::A5BindingIndex;
    use crate::families::a5a8::records::FreeformSurface;
    use crate::families::standard::records::StandardFaceBounds;
    use crate::native::owner_numeric_tail::CatiaOwnerNumericTail;
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let packet_bytes = crate::test_support::test_b2::b2_all_compact_owner_packet_stream();
    let records = crate::wire::records::consolidated_records(&packet_bytes);
    let template = crate::test_support::with_service_context(|ctx| {
        crate::families::b2::records::b2_owner_packets_from_records(ctx, &packet_bytes, &records)
            .map(|mut packets| packets.next().expect("one packet"))
    })
    .expect("fixture");
    let mut carriers = Vec::new();
    let mut owners = Vec::new();
    let mut faces = Vec::new();
    for item in 0..1024 {
        let x = f64::from(item) * 4.0;
        let geometry = NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)],
                    vec![
                        Point3::new(x + 1.0, 0.0, 0.0),
                        Point3::new(x + 1.0, 1.0, 0.0),
                    ],
                ],
                None,
            ),
            false,
        )
        .expect("admitted fixture")
        .expect("square");
        carriers.push(FreeformSurface {
            pos: usize::try_from(item).expect("bounded"),
            identity: None,
            geometry,
        });
        let mut owner = template.clone();
        let lower = f32::from(u16::try_from(item * 4).expect("bounded"));
        owner.numeric_tail = CatiaOwnerNumericTail::new(
            [0x84, 0x41, 0, 0, 0x0d],
            [0.0; 2],
            [1.0; 2],
            [[lower, lower + 1.0], [0.0, 1.0], [-0.1, 0.1]],
        )
        .expect("tail");
        owners.push(owner);
        faces.push(StandardFaceBounds {
            aabb_center: [
                crate::test_support::test_b5::finite(x + 0.5),
                crate::test_support::test_b5::finite(0.5),
                crate::test_support::test_b5::finite(0.0),
            ],
            aabb_half_extents: [
                crate::test_support::test_b5::nonnegative_length(0.5),
                crate::test_support::test_b5::nonnegative_length(0.5),
                crate::test_support::test_b5::nonnegative_length(0.0),
            ],
            sphere_center: [crate::test_support::test_b5::finite(0.0); 3],
            sphere_radius: crate::test_support::test_b5::nonnegative_length(1.0),
        });
    }
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let index = A5BindingIndex::new(ctx, &carriers, &owners)?;
        let mut owner_carriers = Vec::new();
        for (item, owner) in owners.iter().enumerate() {
            let matched = index.matching_carriers(ctx, &owner.numeric_tail)?;
            assert_eq!(matched, vec![item]);
            owner_carriers.push(matched);
        }
        for (item, face) in faces.iter().enumerate() {
            assert_eq!(
                index.containing_owners(ctx, *face, &owner_carriers)?,
                vec![item]
            );
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    };
    let cadmpeg_core::CodecError::ResourceLimit(limit) =
        crate::test_support::with_work_refusal("catia_a5_owner_carriers", run)
            .expect_err("first query")
    else {
        panic!("work refusal");
    };
    crate::test_support::with_work_limit(limit.used + 900_000, run).expect("both indexed products");
}
