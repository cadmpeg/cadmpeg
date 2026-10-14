use super::solve_carriers_with_diagnostics;
use crate::decode::analytic::equations::{CarrierEquation, PlaneEquation, SphereEquation};

fn tangent_pair() -> [CarrierEquation; 2] {
    [
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
        }),
        CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 1.0,
        }),
    ]
}

fn orthogonal_planes() -> [CarrierEquation; 3] {
    [
        CarrierEquation::Plane(PlaneEquation {
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        }),
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 2.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        }),
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 3.0],
            normal: [0.0, 0.0, 1.0],
        }),
    ]
}

#[test]
fn carrier_solver_keeps_pair_and_triple_solutions() {
    crate::decode::with_test_decode_ctx(|ctx| {
        assert_eq!(
            solve_carriers_with_diagnostics(ctx, &tangent_pair())?.0,
            Some([0.0, 0.0, 1.0])
        );
        assert_eq!(
            solve_carriers_with_diagnostics(ctx, &orthogonal_planes())?.0,
            Some([1.0, 2.0, 3.0])
        );
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("carrier witnesses solve");
}
