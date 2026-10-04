// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::decode::sketch::axis::SectionAxis;
use crate::decode::sketch::equations_coordinate::SectionCoordinateEquation;
use crate::decode::sketch::equations_scalar::{
    SectionEquationAuxiliaryConstraints, SectionEquationMidpointConstraint,
};
use crate::feature::definitions::VariableType;

#[test]
fn auxiliary_equation_dedup_and_tail_shift_refuse_before_each_pass() {
    let constraints = SectionEquationAuxiliaryConstraints {
        midpoints: vec![
            SectionEquationMidpointConstraint::new_for_test(
                (1, SectionAxis::U),
                (2, SectionAxis::U),
                (VariableType::Result, 10),
            ),
            SectionEquationMidpointConstraint::new_for_test(
                (3, SectionAxis::U),
                (4, SectionAxis::U),
                (VariableType::Result, 11),
            ),
        ],
        point_bindings: Vec::new(),
    };
    let scalar_values = BTreeMap::from([
        ((VariableType::Result, 10), Some(2.0)),
        ((VariableType::Result, 11), Some(3.0)),
    ]);
    let original = crate::decode::with_test_decode_ctx(|ctx| {
        let mut equation = SectionCoordinateEquation::default();
        equation.add_point(ctx, 1, SectionAxis::U, 1.0)?;
        equation.add_point(ctx, 2, SectionAxis::U, 1.0)?;
        equation.rhs = 4.0;
        Ok::<_, cadmpeg_core::CodecError>(equation)
    })
    .expect("original auxiliary equation");

    let (appended, equations) = crate::test_support::assert_work_boundaries(
        &[
            "creo auxiliary coordinate dedup passes",
            "creo auxiliary coordinate tail shift bytes",
        ],
        |ctx| {
            let mut equations = vec![original.clone()];
            let appended = super::super::append_unique_auxiliary_coordinate_constraints(
                ctx,
                &constraints,
                &scalar_values,
                &BTreeMap::new(),
                &mut equations,
            )?;
            Ok((appended, equations))
        },
    );
    assert!(appended);
    assert_eq!(equations.len(), 2);
    assert_eq!(equations[0].rhs, 4.0);
    assert_eq!(equations[1].rhs, 6.0);
}
