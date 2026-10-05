use crate::families::standard::decode::merge_standard_edge_vertex_references;
use crate::families::standard::decode::split_bezier_half;
use cadmpeg_ir::math::Point3;
use std::collections::BTreeMap;

#[test]
fn bezier_control_windows_preserve_midpoints_and_propagate_work_refusals() {
    let control = std::array::from_fn(|index| {
        Point3::new(
            f64::from(u32::try_from(index).expect("six controls")),
            0.0,
            0.0,
        )
    });
    crate::test_support::with_service_context(|ctx| {
        let (left, right) =
            split_bezier_half(ctx, control).expect("control windows fit service limits");
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
    });
    crate::test_support::with_work_limit(0, |ctx| {
        let error = split_bezier_half(ctx, control).expect_err("control source must be admitted");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal required")
        };
        assert_eq!(limit.operation, "catia_bezier_control_windows");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn standard_edge_merge_propagates_source_work_refusal() {
    crate::test_support::with_work_limit(0, |ctx| {
        let mut target = BTreeMap::from([(70, [500, 300])]);
        let source = BTreeMap::from([(90, [100, 500])]);
        let error =
            merge_standard_edge_vertex_references(ctx, &mut target, &source, |vertices| *vertices)
                .expect_err("edge source must be admitted");
        assert_eq!(error.operation, "catia_standard_e5_topology_edges");
        assert_eq!(ctx.resource_refusal(), Some(error));
        assert_eq!(target, BTreeMap::from([(70, [500, 300])]));
    });
}
