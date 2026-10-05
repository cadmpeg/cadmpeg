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

#[test]
fn coordinate_solver_pass_range_refuses_work_and_preserves_service_result() {
    let definition = crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(1),
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let coordinates = crate::test_support::assert_work_boundaries(
        &["creo section solver pass scan"],
        |ctx| {
            let mut equations = Vec::new();
            let mut scalar_values = BTreeMap::new();
            super::super::solve_section_coordinates_with_derived_constraints(
                ctx,
                &definition,
                &mut equations,
                &BTreeMap::new(),
                (&[], &[]),
                &SectionEquationAuxiliaryConstraints::default(),
                &mut scalar_values,
            )
        },
    );
    assert!(coordinates.is_empty());
}
