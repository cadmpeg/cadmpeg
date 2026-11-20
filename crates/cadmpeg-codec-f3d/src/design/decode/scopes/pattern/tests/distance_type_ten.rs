#[test]
fn rectangular_pattern_spacing_direction_admits_distance_type_ten() {
    let mut bytes = super::spacing_direction_frame();
    bytes[89..93].copy_from_slice(&10u32.to_le_bytes());
    let parse =
        |bytes: &[u8]| super::super::exact_rectangular_pattern_direction(bytes, 0, 221, 20, 23, 10);
    let direction = parse(&bytes).expect("spacing direction with distance type 10");
    assert_eq!(direction.distance_type, 10);
    assert_eq!(direction.selection_record_index, 23);
    assert_eq!(
        direction.direction().unwrap().as_raw(),
        &cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    );
    let mut flag = bytes.clone();
    flag[93] = 1;
    assert!(parse(&flag).is_none());
    let mut other_type = bytes;
    other_type[89..93].copy_from_slice(&8u32.to_le_bytes());
    assert!(parse(&other_type).is_none());
}
