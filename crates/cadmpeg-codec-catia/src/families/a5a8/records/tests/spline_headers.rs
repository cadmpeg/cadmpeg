// SPDX-License-Identifier: Apache-2.0

use super::super::{a5_surface, ConsolidatedFrame};
use crate::nurbs::LaneRefusals;
use crate::test_support::test_a5a8::a5_surface_tail;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;

fn decode(bytes: &[u8]) -> Option<NurbsSurface> {
    a5_surface(
        bytes,
        ConsolidatedFrame {
            pos: 0,
            payload: 0,
            end: bytes.len(),
            header_token: 5,
        },
        &mut LaneRefusals::new(),
        true,
    )
    .expect("bounded surface parser")
    .map(|surface| surface.geometry)
}

fn scalar(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend(value.to_le_bytes());
}

fn assert_polynomial(surface: &NurbsSurface, u: f64, v: f64) {
    let point = eval::decode::nurbs_surface_point(EvaluationAdmission::Standard, surface, u, v)
        .expect("finite plane point")
        .get();
    assert!((point.x - u).abs() < 1.0e-10);
    assert!((point.y - v).abs() < 1.0e-10);
    assert!((point.z - u * u - v.powi(4)).abs() < 1.0e-10);
}

#[test]
fn single_span_surface_preserves_quadratic_and_quartic_bezier_geometry() {
    let mut bytes = vec![9, 9, 0x0c];
    scalar(&mut bytes, 0.0);
    scalar(&mut bytes, 1.0);
    bytes.extend([17, 9, 0x0c]);
    scalar(&mut bytes, 0.0);
    scalar(&mut bytes, 1.0);
    bytes.push(1);
    for i in 0..=2 {
        for j in 0..=4 {
            let x = f64::from(i) / 2.0;
            let y = f64::from(j) / 4.0;
            let z = f64::from(i * (i - 1)) / 2.0 + if j == 4 { 1.0 } else { 0.0 };
            for value in [x, y, z] {
                scalar(&mut bytes, value);
            }
        }
    }
    bytes.extend(a5_surface_tail());
    let surface = decode(&bytes).expect("quadratic by quartic surface");
    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 4));
    assert_eq!((surface.u_count(), surface.v_count()), (3, 5));
    for u in [0.0, 0.23, 0.61, 1.0] {
        for v in [0.0, 0.17, 0.79, 1.0] {
            assert_polynomial(&surface, u, v);
        }
    }
    bytes.pop();
    assert!(decode(&bytes).is_none());
}
