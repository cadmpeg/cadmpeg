// SPDX-License-Identifier: Apache-2.0
//! Retention and source-order validation for freeform chart sides.

use super::{derive, CatiaConsolidatedA5Chart};
use crate::native::CatiaNative;
use crate::wire::records::{consolidated_records, consolidated_records_in_range_sources};

fn side(increment: u8, operand: u8, control: u8, scalars: &[f64]) -> Vec<u8> {
    let mut payload = vec![operand * 4 + 1, control];
    for value in scalars {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let mut frame = vec![
        0xb2,
        3,
        0x18,
        u8::try_from(payload.len()).expect("small fixture"),
        increment * 4 + 1,
    ];
    frame.extend(payload);
    frame
}

fn stream() -> (Vec<u8>, usize) {
    let mut bytes = crate::test_support::test_a5a8::a5_surface_stream();
    let boundary = bytes.len();
    bytes.extend(side(2, 2, 0x15, &[1.0]));
    bytes.extend(side(7, 9, 0x05, &[1.0, 1.0]));
    bytes.extend(side(2, 11, 0x80, &[-0.5, 0.0, 0.5, 0.0]));
    bytes.extend(side(7, 18, 0x00, &[-0.5, 0.5, 0.5, 1.0, -1.0, 0.0]));
    (bytes, boundary)
}

#[test]
fn source_chart_retains_frames_and_checks_its_wire() {
    let (bytes, boundary) = stream();
    let native = CatiaNative::decode(&bytes);
    let [chart] = native.consolidated_a5_charts.as_slice() else {
        panic!("one chart");
    };
    let wire = serde_json::to_value(chart).expect("chart wire");
    assert_eq!(wire["sides"][0]["source_offset"], boundary);
    assert_eq!(wire["sides"][1]["operand"]["value"], 9);
    assert_eq!(wire["sides"][3]["operand"]["value"], 18);
    assert_eq!(wire["sides"][0]["axis"], "u");
    assert_eq!(wire["sides"][2]["sign_ambiguous"], true);
    let parameter = wire["sides"][2]["parameter_range"][0]
        .as_f64()
        .expect("parameter");
    let coordinate = wire["sides"][2]["interval"][0]
        .as_f64()
        .expect("coordinate");
    assert!((parameter + 0.5).abs() < 1.0e-12);
    assert!(coordinate.abs() < 1.0e-12);
    assert_eq!(
        serde_json::from_value::<CatiaConsolidatedA5Chart>(wire.clone()).expect("load chart"),
        *chart
    );
    for (side, field, value) in [
        (0, "source_offset", serde_json::json!(boundary + 1)),
        (
            1,
            "operand",
            serde_json::json!({"value": 10, "encoding": "inline"}),
        ),
        (2, "interval", serde_json::json!([0.0, 4.0])),
        (2, "sign_ambiguous", serde_json::json!(false)),
    ] {
        let mut wrong = wire.clone();
        wrong["sides"][side][field] = value;
        assert!(serde_json::from_value::<CatiaConsolidatedA5Chart>(wrong).is_err());
    }
    let mut wrong = wire;
    wrong["sides"][3]["frame"]["byte_offset"] = serde_json::json!(u64::MAX);
    assert!(serde_json::from_value::<CatiaConsolidatedA5Chart>(wrong).is_err());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("store chart");
    assert_eq!(CatiaNative::load(&namespace).expect("load chart"), native);
}

#[test]
fn chart_follows_logical_source_across_reordered_extents() {
    let (bytes, boundary) = stream();
    let mut reordered = bytes[boundary..].to_vec();
    let split = reordered.len();
    reordered.extend_from_slice(&bytes[..boundary]);
    let records =
        consolidated_records_in_range_sources(&reordered, [[split..reordered.len(), 0..split]]);
    let charts = derive(&reordered, &records).expect("logical source");
    let [chart] = charts.as_slice() else {
        panic!("one chart");
    };
    assert_eq!(chart.carrier_byte_offset, u64::try_from(split).unwrap());
    assert_eq!(chart.sides[0].frame.pos, 0);
    assert_eq!(
        chart.sides[0].source_offset,
        u64::try_from(boundary).unwrap()
    );
    assert_eq!(
        derive(&bytes, &consolidated_records(&bytes)).unwrap().len(),
        1
    );
}
