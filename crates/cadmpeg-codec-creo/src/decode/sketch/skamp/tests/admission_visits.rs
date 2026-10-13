// SPDX-License-Identifier: Apache-2.0

#[test]
fn fixed_coordinate_adjacency_visits_refuse_before_each_present_link() {
    let definition = super::fixed_coordinate_graph_fixture();
    // Two directed adjacency lists each contain one link. Both graph nodes are visited.
    let coordinate = crate::test_support::assert_work_boundaries(
        &["creo fixed-coordinate adjacency links"],
        |ctx| super::super::section_line_entity_fixed_coordinate(ctx, &definition, 20),
    );
    assert_eq!(coordinate, Some(super::super::SectionAxis::U));
}
