use super::*;

#[test]
fn rectangular_pattern_payload_counted_distribution_reads_the_distance_type() {
    use crate::records::feature::patterns::{
        DesignRectangularPatternConstruction, DesignRectangularPatternConstructionWire,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 20, 23]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let construction =
        DesignRectangularPatternConstruction::try_from(DesignRectangularPatternConstructionWire {
            u_count: 5,
            v_count: 3,
            u_extent: 1.7,
            v_extent: 0.0,
            owner_record_indices: [50, 51, 52, 53],
            value_offsets: [0; 4],
            instances: None,
            instance_search_cap: None,
            directions: Vec::new(),
            secondary_direction: None,
            distribution: None,
        })
        .unwrap();
    let direction = super::super::exact_rectangular_pattern_direction(
        &spacing_direction_frame(),
        0,
        221,
        20,
        23,
        10,
    )
    .unwrap();
    let parse = |bytes: &[u8]| {
        super::super::exact_rectangular_pattern_distribution(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            &scope,
            &construction,
            &[direction],
        )
    };

    // The counted form replaces the twelve zero bytes at offsets 134 through
    // 145 with u32 `1`, a marked record reference, and four zero bytes.
    let counted = |distance_type: u8| {
        let mut bytes = distribution_payload(0, None, distance_type);
        let mut head = 1u32.to_le_bytes().to_vec();
        push_marked_reference(&mut head, 30);
        head.extend_from_slice(&[0; 4]);
        bytes.splice(134..146, head);
        bytes
    };
    let spacing = parse(&counted(0)).expect("counted spacing distribution");
    assert_eq!(
        (spacing.total_extent.value, spacing.total_extent.offset),
        (false, 165)
    );
    let extent = parse(&counted(1)).expect("counted extent distribution");
    assert!(extent.total_extent.value);
    let bytes = counted(0);
    // The count, the reference mark, the zero run, the fixed word, the
    // sentinel, and the distance type.
    for (offset, value) in [(134, 2), (138, 2), (149, 1), (153, 3), (161, 0), (165, 2)] {
        let mut corrupt = bytes.clone();
        corrupt[offset] = value;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
}
