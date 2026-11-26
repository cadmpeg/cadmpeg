// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;

#[test]
fn untrim_surface_preserves_loop_choices_and_signed_distances() {
    use crate::features::selections::{EdgeSelection, FaceSelection};
    use crate::features::surfaces::UntrimLoops;
    use crate::features::{FeatureDefinition, FeatureOperation};
    for loops in [
        UntrimLoops::Unresolved {},
        UntrimLoops::All {},
        UntrimLoops::Internal {},
        UntrimLoops::External {},
        UntrimLoops::Selected {
            edges: EdgeSelection::Native("native:loops".into()),
        },
    ] {
        for extension in [
            None,
            crate::scalar::Length::new(3.0),
            crate::scalar::Length::new(-3.0),
        ] {
            let definition = FeatureDefinition::Operation(FeatureOperation::UntrimSurface {
                faces: FaceSelection::Native("native:faces".into()),
                loops: loops.clone(),
                extension,
            });
            let wire = json::to_value(&definition).unwrap();
            assert_eq!(wire["value"]["kind"], "untrim_surface");
            assert_eq!(
                wire["value"]["value"]["extension"],
                extension
                    .map(crate::scalar::Length::get)
                    .map_or(serde_json::Value::Null, |value| cadmpeg_core::json!(value))
            );
            assert_eq!(
                json::from_value::<FeatureDefinition>(wire).unwrap(),
                definition
            );
        }
    }
}

#[test]
fn untrim_loops_reject_unknown_fields() {
    use crate::features::surfaces::UntrimLoops;

    for mut wire in [
        cadmpeg_core::json!({"kind": "unresolved"}),
        cadmpeg_core::json!({"kind": "all"}),
        cadmpeg_core::json!({"kind": "internal"}),
        cadmpeg_core::json!({"kind": "external"}),
        cadmpeg_core::json!({
            "kind": "selected",
            "edges": {"kind": "native", "value": "native:loops"}
        }),
    ] {
        assert!(json::from_value::<UntrimLoops>(wire.clone()).is_ok());
        wire["unexpected"] = cadmpeg_core::json!(true);
        assert!(json::from_value::<UntrimLoops>(wire).is_err());
    }
}
