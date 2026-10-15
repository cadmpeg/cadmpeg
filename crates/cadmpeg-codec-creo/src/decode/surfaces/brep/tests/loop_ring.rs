// SPDX-License-Identifier: Apache-2.0

use super::super::native_loop_ring;

fn native_loop() -> crate::topology::Loop {
    crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        vec![
            crate::topology::HalfEdgeId {
                curve_id: 10,
                side: crate::topology::Side::Zero,
            },
            crate::topology::HalfEdgeId {
                curve_id: 11,
                side: crate::topology::Side::One,
            },
        ],
    )
}

#[test]
fn brep_native_loop_ring_preserves_service_order() {
    let ring = (native_loop_ring(&native_loop(), 5)).expect("service native ring admitted");
    assert_eq!(
        ring.coedges()
            .iter()
            .map(cadmpeg_ir::ids::CoedgeId::as_str)
            .collect::<Vec<_>>(),
        ["creo:visibgeom:coedge#10:0", "creo:visibgeom:coedge#11:1"]
    );
}

#[test]
fn brep_loop_ring_error_refuses_empty_source_ring_before_projection() {
    assert!(crate::topology::Loop::new(std::num::NonZeroU32::new(5), Vec::new(), &[]).is_none());
}
