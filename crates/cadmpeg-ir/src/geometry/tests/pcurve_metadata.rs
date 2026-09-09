// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{PcurveGeneralForm, PcurveInlineForm, PcurveMetadata};

#[test]
fn pcurve_metadata_preserves_directed_and_zero_width_ranges() {
    for range in [[-2.0, 3.0], [3.0, -2.0], [2.0, 2.0]] {
        let general = PcurveMetadata::try_general(Some(false), Some(range), Some(2.0)).unwrap();
        let wire = serde_json::json!({
            "wrapper_reversed": false,
            "parameter_range": range,
            "fit_tolerance": 2.0,
        });
        assert_eq!(general.parameter_range(), Some(range));
        assert_eq!(serde_json::to_value(&general).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<PcurveMetadata>(wire).unwrap(),
            general
        );
        let inline =
            PcurveInlineForm::try_new(false, [true, false, true, false], range, 2.0).unwrap();
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

#[test]
fn pcurve_fit_tolerance_admission_and_mutation_preserve_valid_values() {
    let mut inline = PcurveInlineForm::try_new(false, [false; 4], [1.0, 0.0], 2.0).unwrap();
    let mut general = PcurveGeneralForm::try_new(None, Some([1.0, 0.0]), Some(2.0)).unwrap();
    for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(PcurveInlineForm::try_new(false, [false; 4], [1.0, 0.0], value).is_err());
        assert!(PcurveGeneralForm::try_new(None, None, Some(value)).is_err());
        assert!(PcurveMetadata::try_general(None, None, Some(value)).is_err());
        assert!(inline.set_fit_tolerance(value).is_err());
        assert!(general.set_fit_tolerance(Some(value)).is_err());
        assert_eq!(inline.fit_tolerance(), 2.0);
        assert_eq!(general.fit_tolerance(), Some(2.0));
    }
    inline.set_fit_tolerance(0.0).unwrap();
    general.set_fit_tolerance(Some(0.0)).unwrap();
    assert_eq!(inline.fit_tolerance(), 0.0);
    assert_eq!(general.fit_tolerance(), Some(0.0));
    general.set_fit_tolerance(None).unwrap();
    assert_eq!(general.fit_tolerance(), None);
}

#[test]
fn pcurve_fit_tolerance_wire_rejects_negative_values_and_keeps_numeric_tokens() {
    for value in [0.0, 2.0] {
        let general = serde_json::json!({"fit_tolerance": value});
        let inline = serde_json::json!({
            "wrapper_reversed": false,
            "native_tail_flags": [false, false, false, false],
            "parameter_range": [1.0, 0.0],
            "fit_tolerance": value,
        });
        let general_form: PcurveGeneralForm = serde_json::from_value(general.clone()).unwrap();
        let inline_form: PcurveInlineForm = serde_json::from_value(inline.clone()).unwrap();
        assert_eq!(serde_json::to_value(general_form).unwrap(), general);
        assert_eq!(serde_json::to_value(inline_form).unwrap(), inline);
        for mut wire in [general, inline] {
            let metadata: PcurveMetadata = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(metadata.fit_tolerance(), Some(value));
            assert_eq!(serde_json::to_value(metadata).unwrap(), wire);
            wire["fit_tolerance"] = serde_json::json!(-1.0);
            assert!(serde_json::from_value::<PcurveMetadata>(wire.clone()).is_err());
            if wire.get("native_tail_flags").is_some() {
                assert!(serde_json::from_value::<PcurveInlineForm>(wire).is_err());
            } else {
                assert!(serde_json::from_value::<PcurveGeneralForm>(wire).is_err());
            }
        }
    }
}
