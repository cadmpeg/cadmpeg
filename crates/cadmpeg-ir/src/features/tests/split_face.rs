#[test]
fn split_face_methods_enforce_their_wire_shapes() {
    use crate::features::surfaces::SplitFaceMethod;
    for (wire, value) in [
        (
            serde_json::json!({"kind":"unresolved"}),
            SplitFaceMethod::Unresolved {},
        ),
        (
            serde_json::json!({"kind":"closest_point"}),
            SplitFaceMethod::ClosestPoint {},
        ),
    ] {
        assert_eq!(serde_json::to_value(&value).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<SplitFaceMethod>(wire.clone()).unwrap(),
            value
        );
        let mut invalid = wire;
        invalid["unexpected"] = serde_json::json!(1);
        assert!(serde_json::from_value::<SplitFaceMethod>(invalid).is_err());
    }
    let wire = serde_json::json!({"kind":"along_vector"});
    assert!(serde_json::from_value::<SplitFaceMethod>(wire.clone()).is_ok());
    for field in ["direction", "native_carrier", "native_input_count"] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::Value::Null;
        assert!(serde_json::from_value::<SplitFaceMethod>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}
