use super::rectangular_pattern_direction_frame;

#[test]
fn rectangular_pattern_compact_spacing_direction_validates_links_and_sense() {
    let mut bytes = rectangular_pattern_direction_frame(8, [0.0, 0.0, 4.0]);
    bytes[89..93].copy_from_slice(&7u32.to_le_bytes());
    bytes.remove(194);
    bytes.drain(93..117);
    let parse =
        |bytes: &[u8]| super::super::exact_rectangular_pattern_direction(bytes, 0, 195, 20, 23, 10);
    let direction = parse(&bytes).expect("compact spacing direction");
    assert_eq!(direction.distance_type, 7);
    assert_eq!(
        direction.origin,
        crate::test_support::reals([-2.0, 1.0, 0.5])
    );
    assert_eq!(
        direction.displacement,
        crate::test_support::reals([0.0, 0.0, 4.0])
    );
    assert_eq!(
        direction.direction().unwrap().as_raw(),
        &cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    );
    assert_eq!(direction.origin_offset, 25);
    assert_eq!(direction.displacement_offset, 49);
    assert_eq!(direction.selection_record_index, 23);
    for offset in [
        11, 73, 89, 93, 97, 98, 108, 110, 114, 125, 133, 143, 155, 159, 160, 170, 171, 172, 173,
        183, 184, 185, 196, 202,
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 1;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_none());
    }
    assert!(
        super::super::exact_rectangular_pattern_direction(&bytes, 0, 195, 20, 24, 10).is_none()
    );
    assert!(
        super::super::exact_rectangular_pattern_direction(&bytes, 0, 195, 20, 23, 11).is_none()
    );
    bytes[65..73].copy_from_slice(&(-4.0f64).to_le_bytes());
    assert_eq!(
        parse(&bytes).unwrap().direction().unwrap().as_raw(),
        &cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
    );
}

#[test]
fn rectangular_pattern_flagged_compact_direction_reads_the_shifted_references() {
    let mut bytes = rectangular_pattern_direction_frame(8, [0.0, 0.0, 4.0]);
    bytes[89..93].copy_from_slice(&10u32.to_le_bytes());
    bytes.drain(93..117);
    let parse =
        |bytes: &[u8]| super::super::exact_rectangular_pattern_direction(bytes, 0, 196, 20, 23, 10);
    let direction = parse(&bytes).expect("flagged compact direction");
    assert_eq!(direction.distance_type, 10);
    assert_eq!(direction.selection_record_index, 23);
    assert_eq!(direction.origin_offset, 25);
    assert_eq!(direction.displacement_offset, 49);
    assert_eq!(
        direction.direction().unwrap().as_raw(),
        &cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    );
    // The selection count, its reference, the two zero bytes, and the
    // owner and seed references after the shifted fields.
    for offset in [93, 97, 98, 108, 110, 114, 125, 159, 160, 172, 173, 185] {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 1;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_none());
    }
    // Distance type 10 belongs to the 196-byte form only.
    assert!(
        super::super::exact_rectangular_pattern_direction(&bytes, 0, 195, 20, 23, 10).is_none()
    );
    let mut long = rectangular_pattern_direction_frame(8, [0.0, 0.0, 4.0]);
    long[89..93].copy_from_slice(&10u32.to_le_bytes());
    assert!(super::super::exact_rectangular_pattern_direction(&long, 0, 220, 20, 23, 10).is_none());
}

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
