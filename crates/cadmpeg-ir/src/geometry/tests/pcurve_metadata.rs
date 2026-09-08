// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{PcurveGeneralForm, PcurveInlineForm, PcurveMetadata};

#[test]
fn pcurve_metadata_preserves_directed_and_zero_width_ranges() {
    for range in [[-2.0, 3.0], [3.0, -2.0], [2.0, 2.0]] {
        let general = PcurveMetadata::try_general(Some(false), Some(range), Some(-2.0)).unwrap();
        let wire = serde_json::json!({
            "wrapper_reversed": false,
            "parameter_range": range,
            "fit_tolerance": -2.0,
        });
        assert_eq!(general.parameter_range(), Some(range));
        assert_eq!(serde_json::to_value(&general).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<PcurveMetadata>(wire).unwrap(),
            general
        );
        let inline =
            PcurveInlineForm::try_new(false, [true, false, true, false], range, -2.0).unwrap();
        assert_eq!(inline.parameter_range(), range);
        let metadata = PcurveMetadata::AsmInline(inline);
        let wire = serde_json::to_value(&metadata).unwrap();
        assert_eq!(
            serde_json::from_value::<PcurveMetadata>(wire).unwrap(),
            metadata
        );
    }
    assert_eq!(
        serde_json::to_value(PcurveMetadata::default()).unwrap(),
        serde_json::json!({})
    );
    assert_eq!(
        serde_json::from_str::<PcurveMetadata>("{}").unwrap(),
        PcurveMetadata::default()
    );
}

#[test]
fn pcurve_range_admission_and_mutation_reject_nonfinite_endpoints() {
    let mut inline =
        PcurveInlineForm::try_new(false, [false, false, false, false], [0.0, 1.0], 0.0).unwrap();
    let mut general = PcurveGeneralForm::default();
    for range in [
        [f64::NAN, 1.0],
        [0.0, f64::INFINITY],
        [f64::NEG_INFINITY, 0.0],
    ] {
        assert!(
            PcurveInlineForm::try_new(false, [false, false, false, false], range, 0.0).is_err()
        );
        assert!(PcurveGeneralForm::try_new(None, Some(range), None).is_err());
        assert!(PcurveMetadata::try_general(None, Some(range), None).is_err());
        assert!(inline.set_parameter_range(range).is_err());
        assert_eq!(inline.parameter_range(), [0.0, 1.0]);
        assert!(general.set_parameter_range(Some(range)).is_err());
        assert_eq!(general.parameter_range(), None);
    }
    inline.set_parameter_range([1.0, 1.0]).unwrap();
    general.set_parameter_range(Some([3.0, -2.0])).unwrap();
    assert_eq!(inline.parameter_range(), [1.0, 1.0]);
    assert_eq!(general.parameter_range(), Some([3.0, -2.0]));
    general.set_parameter_range(None).unwrap();
    assert_eq!(general.parameter_range(), None);
}

#[test]
fn pcurve_metadata_wire_rejects_invalid_ranges_and_incomplete_inline_forms() {
    for wire in [
        serde_json::json!({"parameter_range": [null, 1.0]}),
        serde_json::json!({"parameter_range": [0.0, null]}),
        serde_json::json!({"native_tail_flags": null}),
        serde_json::json!({"native_tail_flags": [false, false, false, false]}),
        serde_json::json!({"wrapper_reversed": false, "native_tail_flags": [false, false, false, false], "parameter_range": [0.0, 1.0]}),
        serde_json::json!({"wrapper_reversed": false, "native_tail_flags": [false, false, false, false], "parameter_range": [0.0, null], "fit_tolerance": 0.0}),
    ] {
        assert!(serde_json::from_value::<PcurveMetadata>(wire).is_err());
    }
    assert!(serde_json::from_str::<PcurveInlineForm>(r#"{"wrapper_reversed":false,"native_tail_flags":[false,false,false,false],"parameter_range":[null,1.0],"fit_tolerance":0.0}"#).is_err());
    assert!(
        serde_json::from_str::<PcurveGeneralForm>(r#"{"parameter_range":[0.0,null]}"#).is_err()
    );
}
