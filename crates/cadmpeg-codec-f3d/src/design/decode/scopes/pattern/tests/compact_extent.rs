use super::rectangular_pattern_direction_frame;

#[test]
fn rectangular_pattern_compact_extent_direction_validates_links_and_sense() {
    // The 196-byte form is the 220-byte form without the three f64 `-1.0`
    // values at offsets 93 through 116.
    let mut bytes = rectangular_pattern_direction_frame(8, [4.0, 0.0, 0.0]);
    bytes.drain(93..117);
    let parse =
        |bytes: &[u8]| super::super::exact_rectangular_pattern_direction(bytes, 0, 196, 20, 23, 10);
    let direction = parse(&bytes).expect("compact extent direction");
    assert_eq!(direction.distance_type, 8);
    assert_eq!(
        direction.displacement,
        crate::test_support::reals([4.0, 0.0, 0.0])
    );
    assert_eq!(
        direction.direction().unwrap().as_raw(),
        &cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    );
    assert_eq!(direction.origin_offset, 25);
    assert_eq!(direction.displacement_offset, 49);
    assert_eq!(direction.selection_record_index, 23);
    for offset in [
        11, 73, 89, 93, 97, 98, 108, 110, 114, 125, 133, 143, 155, 159, 160, 170, 171, 172, 173,
        184, 185, 186, 197, 203,
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 1;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_none());
    }
    assert!(
        super::super::exact_rectangular_pattern_direction(&bytes, 0, 196, 20, 24, 10).is_none()
    );
    // Distance type 7 does not occur in the 196-byte form.
    let mut spacing = bytes.clone();
    spacing[89..93].copy_from_slice(&7u32.to_le_bytes());
    assert!(parse(&spacing).is_none());
}
