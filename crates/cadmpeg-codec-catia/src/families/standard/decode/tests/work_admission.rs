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
