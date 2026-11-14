// SPDX-License-Identifier: Apache-2.0

#[test]
fn untrim_surface_preserves_loop_choices_and_signed_distances() {
    use crate::features::surfaces::UntrimLoops;
    use crate::features::{EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation};
    for loops in [
        UntrimLoops::Unresolved,
        UntrimLoops::All,
        UntrimLoops::Internal,
        UntrimLoops::External,
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
            let wire = serde_json::to_value(&definition).unwrap();
            assert_eq!(wire["definition"], "untrim_surface");
            assert_eq!(
                wire["extension"],
                extension
                    .map(crate::scalar::Length::get)
                    .map_or(serde_json::Value::Null, |value| serde_json::json!(value))
            );
            assert_eq!(
                serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
                definition
            );
        }
    }
}
