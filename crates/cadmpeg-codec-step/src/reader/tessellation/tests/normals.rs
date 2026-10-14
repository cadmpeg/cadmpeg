// SPDX-License-Identifier: Apache-2.0
//! Tessellation normal-row normalization.

use crate::parse::Value;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::FiniteReal;

#[test]
fn tessellation_normal_rows_preserve_extreme_finite_directions() {
    let rows = Value::List(vec![
        Value::List(vec![
            Value::Real(FiniteReal::new(f64::MAX).expect("finite fixture")),
            Value::Real(FiniteReal::ZERO),
            Value::Real(FiniteReal::ZERO),
        ]),
        Value::List(vec![
            Value::Real(FiniteReal::new(2.0_f64.powi(-800)).expect("finite fixture")),
            Value::Real(FiniteReal::ZERO),
            Value::Real(FiniteReal::ZERO),
        ]),
        Value::List(vec![
            Value::Real(FiniteReal::new(f64::from_bits(1)).expect("finite fixture")),
            Value::Real(FiniteReal::ZERO),
            Value::Real(FiniteReal::ZERO),
        ]),
    ]);
    assert_eq!(
        super::super::normal_rows(Some(&rows)).map(|normals| normals
            .into_iter()
            .map(FiniteVector3::get)
            .collect::<Vec<_>>()),
        Some(vec![
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ])
    );
}
