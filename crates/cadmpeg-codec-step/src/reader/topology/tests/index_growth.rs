// SPDX-License-Identifier: Apache-2.0
//! Growth requirement for pcurve lookups shared across topology roots.

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::topology::Point;

fn root_index_work(count: u64) -> u64 {
    let mut ir = cadmpeg_ir::CadIr::empty();
    for id in 0..count {
        ir.model.points.push(Point::new(
            PointId::from(crate::ids::data(crate::ids::kind!("point"), id)),
            cadmpeg_ir::features::FinitePoint3::ZERO,
            None,
        ));
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    // Each root currently starts with no pcurve index and rebuilds these carriers.
    for _ in 0..count {
        drop(super::super::PcurveSelectionIndex::build(&ir, &ctx).expect("root index fits"));
    }
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "root index work probe")
        .expect_err("probe work counter")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
#[ignore = "requires an IR carrier index reusable across topology commits"]
fn pcurve_root_indexes_do_not_rebuild_all_carriers() {
    let small = root_index_work(32);
    let large = root_index_work(64);
    // Carrier count and root count double; one shared index grows with n log n.
    assert!(
        large < 3 * small,
        "root index work grew from {small} to {large}"
    );
}
