// SPDX-License-Identifier: Apache-2.0
use crate::test_support::test_rolling_ball_continuation::{fixture, TailForm};

#[test]
fn native_rolling_ball_continuation_preserves_current_and_maximal_limits() {
    let (repeated, repeated_tail) = fixture(2, false, TailForm::Absent87);
    assert_eq!(repeated.len() - repeated_tail, 87);
    let (bytes, tail) = fixture(2, false, TailForm::Absent79);
    let native = crate::native::CatiaNative::decode(&bytes);
    let value = serde_json::to_value(&native).expect("native wire");
    let rows = value["consolidated_rolling_ball_continuations"]
        .as_array()
        .expect("continuation arena");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["current_u_bits"],
        serde_json::json!([0.25_f64.to_bits(), 0.75_f64.to_bits()])
    );
    assert_eq!(
        rows[0]["maximal_u_bits"],
        serde_json::json!([0.0_f64.to_bits(), 1.0_f64.to_bits()])
    );
    assert_eq!(rows[0]["current_v_bits"][0], (-0.0_f64).to_bits());
    assert_eq!(rows[0]["raw_control"], 3);
    assert_eq!(
        rows[0]["reference_scalar_repeat_bits"],
        serde_json::Value::Null
    );
    assert_eq!(rows[0]["scalar_jet"], serde_json::json!({"kind":"absent"}));
    assert_eq!(rows[0]["tail_offset"], u64::try_from(tail).expect("offset"));
}

#[test]
fn native_rolling_ball_continuation_keeps_explicit_scalar_jet_unbound() {
    let (bytes, _) = fixture(2, true, TailForm::Explicit);
    let native = crate::native::CatiaNative::decode(&bytes);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).expect("native store");
    let value = serde_json::to_value(&namespace).expect("namespace wire");
    let rows = value["consolidated_rolling_ball_continuations"]
        .as_array()
        .expect("continuation arena");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["scalar_jet"]["kind"], "explicit");
    assert_eq!(rows[0]["scalar_jet"]["terminal"], 1);
    assert_eq!(
        rows[0]["scalar_jet"]["values_bits"],
        serde_json::json!([2.0_f64.to_bits(), 1.0_f64.to_bits()])
    );
    assert!(rows[0].get("source_curve").is_none());
    assert!(rows[0].get("sense").is_none());
    assert!(rows[0].get("unit").is_none());
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("test-only checked namespace"),
        native
    );
}
