// SPDX-License-Identifier: Apache-2.0

use super::super::axis::SectionAxis;
use super::{SectionCoordinateEquation, SectionCoordinateVariable, SectionEqualLengthConstraint};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use std::collections::BTreeMap;

fn with_collection_limit<T>(limit: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
    run(&ctx)
}

fn solve_matrix_with_limit(
    limit: u64,
    coefficients: Vec<BTreeMap<usize, f64>>,
    variable_count: usize,
) -> Result<Option<Vec<(usize, f64)>>, cadmpeg_core::CodecError> {
    let mut matrix = coefficients
        .into_iter()
        .map(|coefficients| super::SectionLinearRow {
            coefficients,
            rhs: 1.0,
        })
        .collect::<Vec<_>>();
    with_collection_limit(limit, |ctx| {
        super::uniquely_solved_linear_variables(ctx, &mut matrix, variable_count)
    })
}

#[test]
fn section_elimination_coefficients_refuse_before_tree_insert() {
    let error = solve_matrix_with_limit(
        0,
        vec![
            BTreeMap::from([(0, 2.0), (1, 1.0)]),
            BTreeMap::from([(0, 1.0)]),
        ],
        2,
    )
    .expect_err("elimination adds the missing second coefficient");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section elimination coefficients")
    );
}

#[test]
fn section_pivot_rows_refuse_before_tree_insert() {
    let error = solve_matrix_with_limit(0, vec![BTreeMap::from([(0, 1.0)])], 1)
        .expect_err("the first pivot needs one tree node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pivot rows")
    );
}

#[test]
fn section_free_columns_refuse_before_vector_growth() {
    let error = solve_matrix_with_limit(1, vec![BTreeMap::from([(0, 1.0)])], 2)
        .expect_err("the free column follows one admitted pivot");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section free columns")
    );
}

#[test]
fn section_solved_columns_refuse_before_vector_growth() {
    let error = solve_matrix_with_limit(1, vec![BTreeMap::from([(0, 1.0)])], 1)
        .expect_err("the solved column follows one admitted pivot");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved columns")
    );
}

#[test]
fn section_coordinate_unique_variables_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(0, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("one unique variable exceeds zero collection items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section unique variables")
    );
}

#[test]
fn section_coordinate_ordered_variables_refuse_before_vector_reserve() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(1, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the ordered copy follows one admitted unique variable");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section ordered variables")
    );
}

#[test]
fn section_coordinate_variable_indices_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(2, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the index node follows the unique and ordered copies");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section variable indices")
    );
}

#[test]
fn unsigned_dimension_unique_variables_refuse_before_tree_insert() {
    let error = with_collection_limit(0, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
    .expect_err("the first distance endpoint needs a variable node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section unique variables")
    );
}

#[test]
fn section_remaining_variables_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(7, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the remaining set follows variable and equation admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section remaining variables")
    );
}

#[test]
fn section_component_seed_refuses_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(8, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the first component needs its own seed node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component seed")
    );
}

#[test]
fn section_pending_seed_refuses_before_deque_growth() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(9, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the search queue needs one seed slot");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pending seed")
    );
}

#[test]
fn section_component_neighbors_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .is_ok());
    let error = with_collection_limit(20, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the neighbor needs one component node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component neighbors")
    );
}

#[test]
fn section_pending_neighbors_refuse_before_deque_growth() {
    let equations = [SectionCoordinateEquation::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(21, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the admitted neighbor needs one queue slot");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section pending neighbors")
    );
}

#[test]
fn unsigned_dimension_remaining_variables_refuse_before_tree_insert() {
    let error = with_collection_limit(10, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
    .expect_err("remaining nodes follow variable and adjacency admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section remaining variables")
    );
}

#[test]
fn unsigned_component_distances_refuse_before_vector_growth() {
    let error = with_collection_limit(16, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
    .expect_err("the first component distance needs one vector item");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component distances")
    );
}

#[test]
fn unsigned_component_equation_rows_refuse_before_vector_growth() {
    let equations = [SectionCoordinateEquation::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(19, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &equations,
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
    .expect_err("the first component equation needs an outer slot");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component equation rows"),
        "{error:?}"
    );
}

#[test]
fn unsigned_component_equation_terms_refuse_before_tree_clone() {
    let equations = [SectionCoordinateEquation::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(21, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &equations,
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
    .expect_err("two BTreeMap terms need admission before cloning");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component equation terms"),
        "{error:?}"
    );
}

fn unsigned_branch_with_collection_limit(limit: u64) -> cadmpeg_core::CodecError {
    let equations = [SectionCoordinateEquation::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    with_collection_limit(limit, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &equations,
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
    .expect_err("the selected branch exceeds its collection allowance")
}

#[test]
fn unsigned_branch_equation_rows_refuse_before_vector_reserve() {
    let error = unsigned_branch_with_collection_limit(22);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section branch equation rows")
    );
}

#[test]
fn unsigned_branch_equation_terms_refuse_before_tree_clone() {
    let error = unsigned_branch_with_collection_limit(24);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section branch equation terms")
    );
}

#[test]
fn unsigned_signed_equation_rows_refuse_before_vector_growth() {
    let error = unsigned_branch_with_collection_limit(25);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section signed equation rows")
    );
}

#[test]
fn unsigned_signed_equation_terms_refuse_before_tree_creation() {
    let error = unsigned_branch_with_collection_limit(27);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section signed equation terms")
    );
}

#[test]
fn unsigned_signed_branch_charges_work_before_expansion() {
    let equations = [SectionCoordinateEquation::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("small input fits the policy");
    let error = super::solve_unsigned_dimension_coordinates(
        &ctx,
        &equations,
        &BTreeMap::new(),
        &[(1, 2, SectionAxis::U, 1.0)],
    )
    .expect_err("the first signed branch needs one work unit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "explore Creo section distance signs")
    );
}

fn unsigned_value_fixture(
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<SectionCoordinateVariable, f64>, cadmpeg_core::CodecError> {
    let equations = [
        SectionCoordinateEquation::point_value(1, SectionAxis::U, 0.0),
        SectionCoordinateEquation::point_value(2, SectionAxis::U, 1.0),
    ];
    let stored = BTreeMap::from([((1, SectionAxis::U), 0.0)]);
    super::solve_unsigned_dimension_coordinates(
        ctx,
        &equations,
        &stored,
        &[(1, 2, SectionAxis::U, 1.0)],
    )
}

fn unsigned_value_with_limit(limit: u64) -> cadmpeg_core::CodecError {
    with_collection_limit(limit, unsigned_value_fixture)
        .expect_err("the selected unsigned value boundary exceeds the allowance")
}

#[test]
fn unsigned_value_fixture_preserves_the_unique_distance_solution() {
    let solved = crate::decode::with_test_decode_ctx(unsigned_value_fixture)
        .expect("service profile admits the distance solution");
    assert_eq!(solved, BTreeMap::from([((2, SectionAxis::U), 1.0)]));
}

#[test]
fn unsigned_stored_coordinates_refuse_before_tree_clone() {
    let error = unsigned_value_with_limit(79);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section stored coordinate copies")
    );
}

#[test]
fn unsigned_branch_values_refuse_before_tree_insert() {
    let error = unsigned_value_with_limit(80);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section branch values")
    );
}

#[test]
fn unsigned_candidate_values_refuse_before_tree_insert() {
    let error = unsigned_value_with_limit(81);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section candidate values")
    );
}

#[test]
fn unsigned_candidate_solutions_refuse_before_vector_growth() {
    let error = unsigned_value_with_limit(82);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section candidate solutions")
    );
}

#[test]
fn unsigned_resolved_values_refuse_before_tree_insert() {
    let error = unsigned_value_with_limit(136);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section resolved values")
    );
}

#[test]
fn section_component_columns_refuse_before_vector_reserve() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(10, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the component needs an ordered column vector");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component columns")
    );
}

#[test]
fn section_local_columns_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(11, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the first local index follows one admitted column");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section local columns")
    );
}

#[test]
fn section_component_equations_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(12, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the component equation follows its local column");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section component equations")
    );
}

#[test]
fn section_matrix_rows_refuse_before_vector_reserve() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(13, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the first matrix row follows component equation admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section matrix rows")
    );
}

#[test]
fn section_matrix_coefficients_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(14, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the first sparse coefficient follows one matrix row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section matrix coefficients")
    );
}

#[test]
fn section_solved_coordinates_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(17, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the solved coordinate follows the admitted matrix result");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved coordinates")
    );
}

#[test]
fn section_stored_fallback_refuses_before_solved_node() {
    let error = with_collection_limit(0, |ctx| {
        let mut solved = BTreeMap::new();
        super::insert_solved_coordinate(ctx, &mut solved, (1, SectionAxis::U), 2.0)
    })
    .expect_err("a stored fallback value needs the same solved-map admission");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved coordinates")
    );
}

#[test]
fn section_solved_points_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(18, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the first point follows its solved coordinate");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section solved points")
    );
}

#[test]
fn section_coordinate_adjacency_reports_collection_limit() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(3, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("one adjacency row exceeds the collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate adjacency")
    );
}

#[test]
fn section_coordinate_equation_membership_reports_collection_limit() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(4, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("membership row exceeds the remaining collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate equation membership")
    );
}

#[test]
fn unsigned_dimension_adjacency_reports_collection_limit() {
    let error = with_collection_limit(7, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
    .expect_err("two adjacency rows exceed the collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section equation adjacency")
    );
}

#[test]
fn unsigned_dimension_equation_members_refuse_before_vector_growth() {
    let equations = [SectionCoordinateEquation::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let distances = [(1, 2, SectionAxis::U, 1.0)];
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::solve_unsigned_dimension_coordinates(ctx, &equations, &BTreeMap::new(), &distances)
    })
    .is_ok());
    let error = with_collection_limit(8, |ctx| {
        super::solve_unsigned_dimension_coordinates(ctx, &equations, &BTreeMap::new(), &distances)
    })
    .expect_err("the first equation member follows two adjacency rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section equation members")
    );
}

#[test]
fn unsigned_dimension_adjacency_links_refuse_before_tree_insert() {
    let error = with_collection_limit(8, |ctx| {
        super::solve_unsigned_dimension_coordinates(
            ctx,
            &[],
            &BTreeMap::new(),
            &[(1, 2, SectionAxis::U, 1.0)],
        )
    })
    .expect_err("the first adjacency link follows two admitted rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section equation adjacency links")
    );
}

#[test]
fn section_coordinate_members_refuse_before_vector_growth() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .is_ok());
    let error = with_collection_limit(5, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the first equation member follows two outer rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate members")
    );
}

#[test]
fn section_coordinate_adjacency_links_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_difference(
        1,
        2,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(12, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the first adjacency link follows outer and member slots");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo section coordinate adjacency links")
    );
}

#[test]
fn section_coordinate_equation_links_refuse_before_tree_insert() {
    let equations = [SectionCoordinateEquation::point_value(
        1,
        SectionAxis::U,
        1.0,
    )];
    let error = with_collection_limit(6, |ctx| {
        super::solve_section_coordinate_equations(ctx, &equations, &BTreeMap::new())
    })
    .expect_err("the first membership link follows two outer rows and one member");
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
        super::section_equal_length_coordinate_values(&constraints, &coordinates),
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
    super::section_equal_length_coordinate_values(&constraints, &coordinates)
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
