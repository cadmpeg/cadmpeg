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
