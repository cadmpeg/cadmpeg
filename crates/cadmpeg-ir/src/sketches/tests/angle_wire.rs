// SPDX-License-Identifier: Apache-2.0

#[test]
fn fixed_arc_and_ellipse_angles_admit_only_a_positive_angle_on_the_wire() {
    use crate::scalar::PositiveAngle;
    use crate::sketches::{
        SketchConstraintDefinition, SketchConstraintDefinitionInput, SketchEntityId,
    };

    let entity = SketchEntityId::mint("synthetic:test:sketch-entity#fixed-angle").unwrap();
    for input in [
        SketchConstraintDefinitionInput::ArcAngle {
            entity: entity.clone(),
            angle: PositiveAngle::QUARTER_TURN,
        },
        SketchConstraintDefinitionInput::EllipseAngle {
            entity: entity.clone(),
            angle: PositiveAngle::THREE_QUARTER_TURN,
        },
    ] {
        let definition = SketchConstraintDefinition::try_from(input).unwrap();
        let wire = serde_json::to_value(&definition).unwrap();
        assert_eq!(
            serde_json::from_value::<SketchConstraintDefinition>(wire.clone()).unwrap(),
            definition
        );
        for refused in [0.0, -0.0, -std::f64::consts::FRAC_PI_2] {
            let mut refused_wire = wire.clone();
            refused_wire["angle"] = serde_json::json!(refused);
            let error = serde_json::from_value::<SketchConstraintDefinition>(refused_wire)
                .unwrap_err()
                .to_string();
            assert!(error.contains("PositiveAngle must be positive"), "{error}");
        }
    }
}
