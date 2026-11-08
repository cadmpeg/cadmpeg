// SPDX-License-Identifier: Apache-2.0

fn append_record(bytes: &mut Vec<u8>, family: u8, class: u8, payload: &[u8]) -> usize {
    let pos = bytes.len();
    bytes.extend_from_slice(&[family, 0x03, class]);
    if family == 0xa5 {
        bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    } else {
        bytes.push(u8::try_from(payload.len()).unwrap());
    }
    bytes.push(0x05);
    bytes.extend_from_slice(payload);
    pos
}

fn construction_stream() -> (Vec<u8>, [usize; 6]) {
    let mut bytes = Vec::new();
    let mut positions = [0; 6];
    positions[0] = append_record(&mut bytes, 0xa5, 0x32, &[0x05]);
    positions[1] = append_record(&mut bytes, 0xb2, 0x30, &[0x05]);
    positions[2] = append_record(&mut bytes, 0xb2, 0x30, &[0x05]);
    positions[3] = append_record(&mut bytes, 0xb2, 0x20, &[0x05]);
    positions[4] = append_record(&mut bytes, 0xb2, 0x20, &[0x05]);
    let mut payload = vec![0x85, 0x15, 0x11, 0x0d, 0x09, 0x05, 0x11, 0x05];
    payload.extend_from_slice(&1.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x03, 0x05]);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x01, 0x05]);
    positions[5] = append_record(&mut bytes, 0xb2, 0x37, &payload);
    (bytes, positions)
}

#[test]
fn rolling_ball_construction_resolves_five_source_local_targets() {
    let (bytes, positions) = construction_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let [construction] = super::super::b2_rolling_ball_constructions_from_records(&bytes, &records)
        .try_into()
        .expect("one rolling-ball construction");
    assert_eq!(construction.pos, positions[5]);
    assert_eq!(construction.carrier_pos, positions[0]);
    assert_eq!(
        construction.support_surface_positions,
        [positions[1], positions[2]]
    );
    assert_eq!(
        construction.support_pcurve_positions,
        [positions[3], positions[4]]
    );
    assert_eq!(construction.construction_radius.get(), 1.0);
}

#[test]
fn rolling_ball_construction_refuses_wrong_target_and_source_gap() {
    let (mut bytes, positions) = construction_stream();
    bytes[positions[5] + 6] = 0x11;
    let records = crate::wire::records::consolidated_records(&bytes);
    assert!(super::super::b2_rolling_ball_constructions_from_records(&bytes, &records).is_empty());

    let (mut bytes, positions) = construction_stream();
    bytes.insert(positions[1], 0);
    let records = crate::wire::records::consolidated_records(&bytes);
    assert!(super::super::b2_rolling_ball_constructions_from_records(&bytes, &records).is_empty());
}
