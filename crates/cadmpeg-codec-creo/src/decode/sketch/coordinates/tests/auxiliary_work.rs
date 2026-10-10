// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::decode::sketch::axis::SectionAxis;
use crate::decode::sketch::equations_coordinate::SectionCoordinateEquation;
use crate::decode::sketch::equations_scalar::{
    SectionEquationAuxiliaryConstraints, SectionEquationMidpointConstraint,
};
use crate::feature::definitions::VariableType;

#[test]
fn auxiliary_equation_dedup_and_suffix_movement_refuse_before_each_pass() {
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
            "creo auxiliary coordinate suffix movement",
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
fn empty_coordinate_solver_passes_are_free() {
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let mut equations = Vec::new();
    let mut scalar_values = BTreeMap::new();
    let coordinates = super::super::solve_section_coordinates_with_derived_constraints(
        &ctx,
        &definition,
        &mut equations,
        &BTreeMap::new(),
        (&[], &[]),
        &SectionEquationAuxiliaryConstraints::default(),
        &mut scalar_values,
    )
    .expect("empty solver sources need no work");
    assert!(coordinates.is_empty());
}

#[test]
fn coefficient_index_preserves_scalar_equality_and_all_rhs_witnesses() {
    const EPS_RHS_WITNESS_AGREEMENT: f64 = 1.0e-9;
    crate::decode::with_test_decode_ctx(|ctx| {
        let equation = |coefficient, rhs| SectionCoordinateEquation {
            terms: BTreeMap::from([((7, SectionAxis::U), coefficient)]),
            rhs,
        };
        let rows = [
            equation(1.0, 1.0),
            equation(1.0, 1.0 + 0.75 * EPS_RHS_WITNESS_AGREEMENT),
        ];
        let index = super::super::CoordinateEquationIndex::from_equations(ctx, &rows)?;
        assert!(index.contains(ctx, &equation(1.0, 1.0 + 1.5 * EPS_RHS_WITNESS_AGREEMENT))?);
        let zero =
            super::super::CoordinateEquationIndex::from_equations(ctx, &[equation(-0.0, 2.0)])?;
        assert!(zero.contains(ctx, &equation(0.0, 2.0))?);
        assert!(!zero.contains(ctx, &equation(f64::NAN, 2.0))?);
        assert!(!zero.contains(ctx, &equation(0.0, f64::INFINITY))?);
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("coefficient index admission");
}

#[test]
fn coefficient_index_lookup_refuses_before_key_and_rhs_search() {
    let equation = SectionCoordinateEquation {
        terms: BTreeMap::from([((7, SectionAxis::U), 1.0)]),
        rhs: 2.0,
    };
    let found = crate::test_support::assert_work_boundaries(
        &[
            "creo coordinate equation index terms",
            "creo coordinate equation index lookup",
            "creo coordinate equation right-hand-side witnesses",
        ],
        |ctx| {
            let index = super::super::CoordinateEquationIndex::from_equations(
                ctx,
                std::slice::from_ref(&equation),
            )?;
            index.contains(ctx, &equation)
        },
    );
    assert!(found);
}
