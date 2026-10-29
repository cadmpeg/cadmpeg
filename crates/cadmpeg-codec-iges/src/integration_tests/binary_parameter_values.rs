// SPDX-License-Identifier: Apache-2.0
//! A Binary Parameter real is read with the Binary Flag Section widths; the
//! Global numeric declarations set no range for it (IGES 5.3 Appendix H.2.3).

use cadmpeg_ir::codec::DecodeResult;

use crate::test_support::binary_parts::{
    arc_with_real_start_then_point, point_with_real_y_then_point,
};

/// A double-precision real whose decimal order exceeds the single-precision
/// magnitude that the fixture Global declares (field 8 is 38).
const UNADMITTED_REAL: f64 = 1e100;
/// Relative agreement of a Binary double with its rendered literal.
const EPS_RELATIVE: f64 = 1e-15;

fn entity(result: &DecodeResult, index: usize) -> serde_json::Value {
    let native = result.ir().native.namespace("iges").unwrap();
    serde_json::to_value(&native.arenas()["entities"][index]).unwrap()
}

/// The first entity holds the double at `field`, as the baseline holds its
/// own value there; no part is given up and no record is quarantined. The
/// Binary Flag Section supersedes Global fields 8 and 10, so they set no
/// range for a Binary real (IGES 5.3 Appendix H.2.3; iges.md, Binary).
fn assert_value_admitted(baseline: &DecodeResult, decoded: &DecodeResult, field: usize) {
    let first = entity(decoded, 0);
    let expected = entity(baseline, 0);
    assert!(!first["parameters"][field].is_null());
    assert!(!expected["parameters"][field].is_null());
    assert_eq!(
        first["parameters"].as_array().map(Vec::len),
        expected["parameters"].as_array().map(Vec::len)
    );
    assert_eq!(entity(decoded, 1), entity(baseline, 1));
    let native = decoded.ir().native.namespace("iges").unwrap();
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "record.unreadable"));
}

#[test]
fn a_binary_point_coordinate_beyond_global_field_8_is_admitted() {
    let baseline = crate::test_support::decode(point_with_real_y_then_point(2.0));
    let decoded = crate::test_support::decode(point_with_real_y_then_point(UNADMITTED_REAL));
    // Y is the second coordinate: token 2 after the entity type.
    assert_value_admitted(&baseline, &decoded, 2);
    assert_eq!(decoded.ir().model.points.len(), 2);
    assert_eq!(baseline.ir().model.points.len(), 2);
    let y = decoded.ir().model.points[0].position().get().y;
    assert!((y / UNADMITTED_REAL - 1.0).abs() < EPS_RELATIVE);
}

#[test]
fn a_binary_arc_value_beyond_global_field_8_is_admitted_and_no_record_is_quarantined() {
    let baseline = crate::test_support::decode(arc_with_real_start_then_point(1.0));
    let decoded = crate::test_support::decode(arc_with_real_start_then_point(UNADMITTED_REAL));
    // The arc start X is the fourth value: token 4 after the entity type.
    assert_value_admitted(&baseline, &decoded, 4);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "parameter.data-quarantined"));
}
