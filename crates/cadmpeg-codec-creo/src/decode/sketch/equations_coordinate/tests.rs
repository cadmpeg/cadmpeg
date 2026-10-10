// SPDX-License-Identifier: Apache-2.0

use super::super::axis::SectionAxis;
use super::{SectionCoordinateVariable, SectionEqualLengthConstraint, SectionEquationFixture};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use std::collections::BTreeMap;

mod admission_visits;
mod range_admission;

#[test]
fn section_component_loops_refuse_before_traversal() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    crate::test_support::assert_work_boundaries(
        &[
            "creo section coordinate graph visits",
            "creo section coordinate components",
        ],
        |ctx| super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new()),
    );
}

#[test]
fn unsigned_component_loop_refuses_before_component_work() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let distances = [(1, 2, SectionAxis::U, 1.0)];
    crate::test_support::assert_work_boundaries(&["creo unsigned coordinate components"], |ctx| {
        super::solve_unsigned_dimension_coordinates(ctx, &equations, &BTreeMap::new(), &distances)
    });
}

#[test]
fn section_pivot_max_search_refuses_before_range_scan() {
    let solution = crate::test_support::assert_work_boundaries(
        &[
            "creo section pivot candidate rows",
            "creo section pivot selected coefficient lookup",
            "creo section pivot candidate coefficient lookup",
        ],
        |ctx| {
            let mut matrix = vec![
                super::SectionLinearRow {
                    coefficients: BTreeMap::from([(0, 1.0)]),
                    rhs: 1.0,
                },
                super::SectionLinearRow {
                    coefficients: BTreeMap::from([(0, 2.0)]),
                    rhs: 2.0,
                },
            ];
            super::uniquely_solved_linear_variables(ctx, &mut matrix, 1)
        },
    );
    assert_eq!(solution, Some(vec![(0, 1.0)]));
}

fn solve_matrix_with_limit(
    operation: &'static str,
    coefficients: &[BTreeMap<usize, f64>],
    variable_count: usize,
) -> cadmpeg_core::CodecError {
    crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, operation, |ctx| {
        let mut matrix = coefficients
            .iter()
            .map(|coefficients| super::SectionLinearRow {
                coefficients: coefficients.clone(),
                rhs: 1.0,
            })
            .collect::<Vec<_>>();
        super::uniquely_solved_linear_variables(ctx, &mut matrix, variable_count)
    })
}

#[test]
fn coordinate_equation_refuses_before_first_term_node() {
    assert!(
        matches!(crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo coordinate equation term nodes", |ctx| {
        super::SectionCoordinateEquation::point_value(ctx, 7, SectionAxis::U, 2.0)
    }), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo coordinate equation term nodes")
    );
    assert_eq!(
        SectionEquationFixture::point_value(7, SectionAxis::U, 2.0)
            .terms
            .len(),
        1
    );
}

#[test]
fn coordinate_difference_refuses_before_second_term_node() {
    assert!(
        matches!(crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo coordinate equation term nodes", |ctx| {
        super::SectionCoordinateEquation::point_difference(ctx, 7, 8, SectionAxis::V, 3.0)
    }), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo coordinate equation term nodes")
    );
    assert_eq!(
        SectionEquationFixture::point_difference(7, 8, SectionAxis::V, 3.0)
            .terms
            .len(),
        2
    );
}

#[test]
fn section_elimination_coefficients_refuse_before_tree_insert() {
    let error = solve_matrix_with_limit(
        "creo section elimination coefficients",
        &[
            BTreeMap::from([(0, 2.0), (1, 1.0)]),
            BTreeMap::from([(0, 1.0)]),
        ],
        2,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section elimination coefficients")
    );
}

#[test]
fn section_pivot_rows_refuse_before_tree_insert() {
    let error =
        solve_matrix_with_limit("creo section pivot rows", &[BTreeMap::from([(0, 1.0)])], 1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pivot rows")
    );
}

#[test]
fn section_free_columns_refuse_before_vector_growth() {
    let error = solve_matrix_with_limit(
        "creo section free columns",
        &[BTreeMap::from([(0, 1.0)])],
        2,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section free columns")
    );
}

#[test]
fn section_solved_columns_refuse_before_vector_growth() {
    let error = solve_matrix_with_limit(
        "creo section solved columns",
        &[BTreeMap::from([(0, 1.0)])],
        1,
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved columns")
    );
}

#[test]
fn section_coordinate_unique_variables_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section unique variables", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section unique variables")
    );
}

#[test]
fn section_coordinate_ordered_variables_refuse_before_vector_reserve() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section ordered variables", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section ordered variables")
    );
}

#[test]
fn section_coordinate_variable_indices_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section variable indices", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section variable indices")
    );
}

#[test]
fn unsigned_dimension_unique_variables_refuse_before_tree_insert() {
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section unique variables", |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section unique variables")
    );
}

#[test]
fn section_remaining_variables_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section remaining variables", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section remaining variables")
    );
}

#[test]
fn section_component_seed_refuses_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section component seed", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component seed")
    );
}

#[test]
fn section_pending_seed_refuses_before_deque_growth() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section pending seed", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pending seed")
    );
}

#[test]
fn section_component_neighbors_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .is_ok());
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section component neighbors", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component neighbors")
    );
}

#[test]
fn section_pending_neighbors_refuse_before_deque_growth() {
    let equations = [SectionEquationFixture::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section pending neighbors", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pending neighbors")
    );
}

#[test]
fn unsigned_dimension_remaining_variables_refuse_before_tree_insert() {
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section remaining variables", |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section remaining variables")
    );
}

#[test]
fn unsigned_component_distances_refuse_before_vector_growth() {
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section component distances", |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component distances")
    );
}

#[test]
fn unsigned_component_equation_rows_refuse_before_vector_growth() {
    let equations = [SectionEquationFixture::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section component equation rows", |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &equations,
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component equation rows"),
        "{error:?}"
    );
}

fn unsigned_branch_with_collection_limit(operation: &'static str) -> cadmpeg_core::CodecError {
    let equations = [SectionEquationFixture::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, operation, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &equations,
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
}

#[test]
fn unsigned_branch_equation_rows_refuse_before_vector_reserve() {
    let error = unsigned_branch_with_collection_limit("creo section branch equation rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section branch equation rows")
    );
}

#[test]
fn unsigned_branch_equation_terms_refuse_before_tree_clone() {
    let error = unsigned_branch_with_collection_limit("creo section branch equation terms");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section branch equation terms")
    );
}

#[test]
fn unsigned_signed_equation_rows_refuse_before_vector_growth() {
    let error = unsigned_branch_with_collection_limit("creo section signed equation rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section signed equation rows")
    );
}

#[test]
fn unsigned_signed_equation_terms_refuse_before_tree_creation() {
    let error = unsigned_branch_with_collection_limit("creo section signed equation terms");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section signed equation terms")
    );
}


fn unsigned_value_fixture(
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<SectionCoordinateVariable, f64>, cadmpeg_core::CodecError> {
    let equations = [
        SectionEquationFixture::point_value(1, SectionAxis::U, 0.0),
        SectionEquationFixture::point_value(2, SectionAxis::U, 1.0),
    ];
    let stored = BTreeMap::from([((1, SectionAxis::U), 0.0)]);
    super::solve_unsigned_dimension_coordinates(
        ctx,
        &equations,
        &stored,
        &[(1, 2, SectionAxis::U, 1.0)],
    )
}

fn unsigned_value_with_limit(operation: &'static str) -> cadmpeg_core::CodecError {
    crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, operation, unsigned_value_fixture)
}

#[test]
fn unsigned_value_fixture_preserves_the_unique_distance_solution() {
    let solved = crate::decode::with_test_decode_ctx(unsigned_value_fixture)
        .expect("service profile admits the distance solution");
    assert_eq!(solved, BTreeMap::from([((2, SectionAxis::U), 1.0)]));
}

#[test]
fn unsigned_candidate_values_refuse_before_tree_insert() {
    let error = unsigned_value_with_limit("creo section candidate values");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section candidate values")
    );
}

#[test]
fn unsigned_candidate_solutions_refuse_before_vector_growth() {
    let error = unsigned_value_with_limit("creo section candidate solutions");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section candidate solutions")
    );
}

#[test]
fn unsigned_resolved_values_refuse_before_tree_insert() {
    let error = unsigned_value_with_limit("creo section resolved values");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section resolved values")
    );
}

#[test]
fn section_component_columns_refuse_before_vector_reserve() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section component columns", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component columns")
    );
}

#[test]
fn section_local_columns_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section local columns", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section local columns")
    );
}

#[test]
fn section_component_equations_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section component equations", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component equations")
    );
}

#[test]
fn section_matrix_rows_refuse_before_vector_reserve() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section matrix rows", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section matrix rows")
    );
}

#[test]
fn section_matrix_coefficients_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section matrix coefficients", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section matrix coefficients")
    );
}

#[test]
fn section_solved_coordinates_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section solved coordinates", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved coordinates")
    );
}

#[test]
fn section_stored_fallback_refuses_before_solved_node() {
    let equations = [
        SectionEquationFixture::point_value(1, SectionAxis::U, 1.0),
        SectionEquationFixture::point_value(1, SectionAxis::U, 3.0),
    ];
    let stored = BTreeMap::from([((1, SectionAxis::U), 2.0)]);
    let error = crate::test_support::last_refusal_at(
        &[0], ResourceDimension::CollectionItems, "creo section solved coordinates",
        |ctx| super::solve_section_coordinate_equations(ctx, &equations, &stored),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo section solved coordinates"));
    let solved = crate::decode::with_test_decode_ctx(|ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &stored)
    }).expect("stored fallback admitted");
    assert_eq!(solved, BTreeMap::from([(1, [Some(2.0), None])]));

}

#[test]
fn section_solved_points_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section solved points", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved points")
    );
}

#[test]
fn section_coordinate_adjacency_reports_collection_limit() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section coordinate adjacency", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate adjacency")
    );
}

#[test]
fn section_coordinate_equation_membership_reports_collection_limit() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section coordinate equation membership", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate equation membership")
    );
}

#[test]
fn unsigned_dimension_adjacency_reports_collection_limit() {
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section equation adjacency", |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section equation adjacency")
    );
}

#[test]
fn unsigned_dimension_adjacency_links_refuse_before_tree_insert() {
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section equation adjacency links", |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section equation adjacency links")
    );
}

#[test]
fn section_coordinate_adjacency_links_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section coordinate adjacency links", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate adjacency links")
    );
}

#[test]
fn section_coordinate_equation_links_refuse_before_tree_insert() {
    let equations = [SectionEquationFixture::point_value(1, SectionAxis::U, 1.0)];
    let error = crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo section coordinate equation links", |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate equation links")
    );
}

#[test]
fn numerical_followup_equal_length_tangency_keeps_its_single_coordinate() {
    // Segment 1-2 spans (0.3, 0.4). Segment 3-4 is vertical and spans 0.5,
    // so the equal-length constraint has the single solution u3 = 0.0. The
    // squared lengths cancel to -2.7755575615628914e-17 instead of zero,
    // which without the cancellation rule splits the double root into
    // -5.268356063861754e-09 and 5.2683560638617535e-09 and states no
    // coordinate.
    let coordinates = BTreeMap::from([
        (1, [Some(0.0), Some(0.0)]),
        (2, [Some(0.3), Some(0.4)]),
        (3, [None, Some(0.0)]),
        (4, [Some(0.0), Some(0.5)]),
    ]);
    let constraints = [SectionEqualLengthConstraint {
        first: [1, 2],
        second: [3, 4],
        equation_id: 7,
        offset: 0,
        active: true,
    }];
    let expected: BTreeMap<SectionCoordinateVariable, Option<f64>> =
        BTreeMap::from([((3, SectionAxis::U), Some(0.0))]);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::section_equal_length_coordinate_values(ctx, &constraints, &coordinates)
        })
        .expect("equal-length candidate"),
        expected
    );
}

/// The single U coordinate an equal-length tangency states.
///
/// Point 1 sits at the origin and point 2 at `segment`. Point 3 carries the
/// missing U coordinate and sits on the U axis; point 4 sits at
/// `(partner_u, length)`, where `length` is the length of segment 1-2. The
/// equal-length constraint 1-2 against 3-4 is then tangent, with the single
/// solution u3 = `partner_u`.
fn equal_length_tangency_u(
    segment: [f64; 2],
    partner_u: f64,
    length: f64,
) -> BTreeMap<SectionCoordinateVariable, Option<f64>> {
    let coordinates = BTreeMap::from([
        (1, [Some(0.0), Some(0.0)]),
        (2, [Some(segment[0]), Some(segment[1])]),
        (3, [None, Some(0.0)]),
        (4, [Some(partner_u), Some(length)]),
    ]);
    let constraints = [SectionEqualLengthConstraint {
        first: [1, 2],
        second: [3, 4],
        equation_id: 7,
        offset: 0,
        active: true,
    }];
    crate::decode::with_test_decode_ctx(|ctx| {
        super::section_equal_length_coordinate_values(ctx, &constraints, &coordinates)
    })
    .expect("equal-length candidate")
}

#[test]
fn equal_length_candidate_node_refuses_before_insertion() {
    let coordinates = BTreeMap::from([
        (1, [Some(0.0), Some(0.0)]),
        (2, [Some(0.3), Some(0.4)]),
        (3, [None, Some(0.0)]),
        (4, [Some(0.0), Some(0.5)]),
    ]);
    let constraints = [SectionEqualLengthConstraint {
        first: [1, 2],
        second: [3, 4],
        equation_id: 7,
        offset: 0,
        active: true,
    }];
    assert!(
        matches!(crate::test_support::last_refusal_at(&[0], ResourceDimension::CollectionItems, "creo equal-length coordinate candidates", |ctx| {
        super::section_equal_length_coordinate_values(ctx, &constraints, &coordinates)
    }), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo equal-length coordinate candidates")
    );
}

#[test]
fn numerical_followup_equal_length_tangency_states_its_off_axis_root() {
    // The exact constant of the quadratic is partner_u^2, and the two
    // squared segment lengths cancel on top of it. That residue is far
    // larger than a discriminant band read from the coefficient values
    // alone admits: the first and third witnesses stated no root at all and
    // the second split the double root into 0.000999992616836453 and
    // 0.0010000073831635471, which no longer agree to one coordinate.
    for (segment, partner_u, length) in [
        ([3.3, 4.3], 0.001, 5.420_332_093_147_061),
        ([0.3, 0.4], 0.001, 0.5),
        ([1.1, 2.2], 0.01, 2.459_674_775_249_769),
    ] {
        let expected: BTreeMap<SectionCoordinateVariable, Option<f64>> =
            BTreeMap::from([((3, SectionAxis::U), Some(partner_u))]);
        assert_eq!(
            equal_length_tangency_u(segment, partner_u, length),
            expected
        );
    }
}

#[test]
fn coordinate_variable_scan_refuses_before_unique_variable_node() {
    let equations = [SectionEquationFixture::point_value(7, SectionAxis::U, 2.0)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = super::admitted_coordinate_variables(&ctx, &equations, &[])
        .expect_err("equation admission precedes variable insertion");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "creo coordinate variable equations")
    );
}

#[test]
fn indexed_disconnected_distance_components_preserve_each_unique_solution() {
    let mut equations = Vec::new();
    let mut stored = BTreeMap::new();
    let mut distances = Vec::new();
    let mut expected = BTreeMap::new();
    for group in 0..16 {
        let first = 2 * group + 1;
        let second = first + 1;
        equations.push(SectionEquationFixture::point_value(
            first,
            SectionAxis::U,
            0.0,
        ));
        equations.push(SectionEquationFixture::point_value(
            second,
            SectionAxis::U,
            1.0,
        ));
        stored.insert((first, SectionAxis::U), 0.0);
        distances.push((first, second, SectionAxis::U, 1.0));
        expected.insert((second, SectionAxis::U), 1.0);
    }
    let solved = crate::decode::with_test_decode_ctx(|ctx| {
        super::solve_unsigned_dimension_coordinates(ctx, &equations, &stored, &distances)
    })
    .expect("disconnected component admission");
    assert_eq!(solved, expected);
}
