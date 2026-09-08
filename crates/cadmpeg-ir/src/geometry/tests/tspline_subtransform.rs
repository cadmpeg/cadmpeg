use super::super::{InlineTSplineSubtransform, SubtypeTableIndex, TSplineSubtransform};
use serde_json::json;

#[test]
fn subtransform_admission_requires_nonempty_programs_and_nonnegative_indices() {
    assert!(InlineTSplineSubtransform::try_new("", None, "values").is_err());
    assert!(InlineTSplineSubtransform::try_new("program", None, "").is_err());
    assert!(InlineTSplineSubtransform::try_new(" ", None, "\n").is_ok());
    assert!(SubtypeTableIndex::try_new(-1).is_err());
    for index in [0, i64::MAX] {
        assert_eq!(SubtypeTableIndex::try_new(index).unwrap().get(), index);
    }
}

#[test]
fn subtransform_wire_preserves_inline_and_reference_shapes() {
    let inline =
        json!({"kind": "inline", "program": "program", "separator": true, "values": "values"});
    for wire in [
        inline.clone(),
        json!({"kind": "reference", "index": 0}),
        json!({"kind": "reference", "index": 4, "resolved": inline}),
    ] {
        let value: TSplineSubtransform = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
    }
}

#[test]
fn subtransform_wire_rejects_invalid_payloads_and_recursive_references() {
    for wire in [
        json!({"kind": "inline", "program": "", "values": "values"}),
        json!({"kind": "inline", "program": "program", "values": ""}),
        json!({"kind": "inline", "values": "values"}),
        json!({"kind": "reference", "index": -1}),
        json!({"kind": "reference", "index": 0, "resolved": {"kind": "reference", "index": 1}}),
        json!({"kind": "reference", "index": 0, "resolved": {"kind": "inline", "program": "", "values": "values"}}),
    ] {
        assert!(serde_json::from_value::<TSplineSubtransform>(wire).is_err());
    }
}
