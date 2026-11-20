// SPDX-License-Identifier: Apache-2.0
//! Direction records of active and inactive rectangular-pattern directions.

#[test]
fn an_inactive_direction_keeps_its_stored_record_out_of_the_direction_list() {
    use crate::records::feature::patterns::{
        DesignRectangularPatternConstruction, DesignRectangularPatternConstructionWire,
    };
    let construction = |v_count, v_extent| {
        DesignRectangularPatternConstruction::try_from(DesignRectangularPatternConstructionWire {
            u_count: 4,
            v_count,
            u_extent: 0.5,
            v_extent,
            owner_record_indices: [50, 51, 52, 53],
            value_offsets: [0; 4],
            instances: None,
            instance_search_cap: None,
            directions: Vec::new(),
            secondary_direction: None,
            distribution: None,
        })
        .unwrap()
    };
    let x = super::super::exact_rectangular_pattern_direction(
        &super::spacing_direction_frame(),
        0,
        221,
        20,
        23,
        10,
    )
    .unwrap();
    let mut y = x;
    y.record_index = 30;
    let records = |construction| {
        super::super::active_direction_records(&construction, vec![x, y])
            .iter()
            .map(|record| record.record_index)
            .collect::<Vec<_>>()
    };
    // Direction two with count one keeps its distance.
    assert_eq!(records(construction(1, 0.5)), [20]);
    // Direction two with a zero distance is coincident.
    assert_eq!(records(construction(3, 0.0)), [20]);
    assert_eq!(records(construction(3, 0.5)), [20, 30]);
    let one = super::super::active_direction_records(&construction(1, 0.5), vec![x]);
    assert_eq!(one.len(), 1);
}
