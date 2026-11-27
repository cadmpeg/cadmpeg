use cadmpeg_core::serde_erase::json;

#[test]
fn split_face_methods_enforce_their_wire_shapes() {
    use crate::features::surfaces::SplitFaceMethod;
    for (wire, value) in [
        (
            cadmpeg_core::json!({"kind":"unresolved"}),
            SplitFaceMethod::Unresolved {},
        ),
        (
            cadmpeg_core::json!({"kind":"closest_point"}),
            SplitFaceMethod::ClosestPoint {},
        ),
    ] {
        assert_eq!(json::to_value(&value).unwrap(), wire);
        assert_eq!(
            json::from_value::<SplitFaceMethod>(wire.clone()).unwrap(),
            value
        );
        let mut invalid = wire;
        invalid["unexpected"] = cadmpeg_core::json!(1);
        assert!(json::from_value::<SplitFaceMethod>(invalid).is_err());
    }
    let wire = cadmpeg_core::json!({"kind":"along_vector", "native_inputs": [], "native_inputs_complete": false});
    assert!(json::from_value::<SplitFaceMethod>(wire.clone()).is_ok());
    for field in ["native_inputs", "native_inputs_complete"] {
        let mut invalid = wire.clone();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(json::from_value::<SplitFaceMethod>(invalid).is_err());
    }
    for field in ["direction", "native_carrier", "native_input_count"] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::Value::Null;
        assert!(json::from_value::<SplitFaceMethod>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}

#[test]
fn split_face_operation_writes_its_unresolved_method() {
    use crate::features::direct_edit::SplitFaceTool;
    use crate::features::selections::FaceSelection;
    use crate::features::surfaces::SplitFaceMethod;
    use crate::features::FeatureOperation;
    let operation = FeatureOperation::SplitFace {
        targets: FaceSelection::Native("native:targets".into()),
        tool: SplitFaceTool::Native("native:tool".into()),
        method: SplitFaceMethod::default(),
    };
    let mut wire = json::to_value(&operation).unwrap();
    assert_eq!(
        wire["value"]["method"],
        cadmpeg_core::json!({"kind":"unresolved"})
    );
    assert_eq!(
        json::from_value::<FeatureOperation>(wire.clone()).unwrap(),
        operation
    );
    wire["value"].as_object_mut().unwrap().remove("method");
    assert!(json::from_value::<FeatureOperation>(wire).is_err());
}
