// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;

#[test]
fn unit_feature_directions_admit_only_unit_vectors_on_the_wire() {
    use crate::features::geometry::FinitePoint3;
    use crate::features::operation_settings::BooleanOp;
    use crate::features::profiles::{PathRef, PlanarProfileRef};
    use crate::features::selections::{BodySelection, FaceSelection};
    use crate::features::surface_inputs::SurfaceProjectionMode;
    use crate::features::sweeps::{
        HelicalSweepConstruction, HelicalSweepLaw, HelicalSweepTravel, SweepOrientation,
    };
    use crate::features::FeatureOperation;
    use crate::scalar::{Angle, Length, NonNegativeLength, PositiveLength, PositiveReal};
    use crate::units::UnitVector3;

    fn refuses<T: serde::de::DeserializeOwned + std::fmt::Debug>(
        wire: &serde_json::Value,
        payload: &str,
        key: &str,
    ) {
        for refused in [
            cadmpeg_core::json!({"x": 0.0, "y": 0.0, "z": 2.0}),
            cadmpeg_core::json!({"x": 0.0, "y": 0.0, "z": 0.0}),
            cadmpeg_core::json!({"x": 0.0, "y": 0.0, "z": 1.0 + 2.0e-9}),
        ] {
            let mut wire = wire.clone();
            wire.pointer_mut(payload).unwrap()[key] = refused;
            let error = json::from_value::<T>(wire).unwrap_err().to_string();
            assert!(error.contains("direction must have unit length"), "{error}");
        }
        let mut near_unit = wire.clone();
        near_unit.pointer_mut(payload).unwrap()[key] =
            cadmpeg_core::json!({"x": 0.0, "y": 0.0, "z": 1.0 + 5.0e-10});
        assert!(json::from_value::<T>(near_unit).is_ok());
    }

    let operations = [
        (
            FeatureOperation::Torus {
                center: FinitePoint3::ZERO,
                axis: UnitVector3::Z_AXIS,
                major_radius: PositiveLength::new(5.0).unwrap(),
                minor_radius: PositiveLength::new(1.0).unwrap(),
                op: BooleanOp::NewBody,
            },
            "axis",
        ),
        (
            FeatureOperation::MirrorShape {
                source: BodySelection::Native("mirror:source".into()),
                plane_origin: FinitePoint3::ZERO,
                plane_normal: UnitVector3::Z_AXIS,
                plane_reference: None,
            },
            "plane_normal",
        ),
        (
            FeatureOperation::ProjectOnSurface {
                sources: PathRef::Native("project:sources".into()),
                support_face: FaceSelection::Native("project:support".into()),
                direction: UnitVector3::Z_AXIS,
                mode: SurfaceProjectionMode::All,
                height: NonNegativeLength::new(0.0).unwrap(),
                offset: Length::ZERO,
            },
            "direction",
        ),
    ];
    for (operation, key) in operations {
        let wire = json::to_value(&operation).unwrap();
        let decoded: FeatureOperation = json::from_value(wire.clone()).unwrap();
        assert_eq!(json::to_value(decoded).unwrap(), wire);
        refuses::<FeatureOperation>(&wire, "/value", key);
    }

    let binormal = json::to_value(SweepOrientation::Binormal {
        direction: UnitVector3::Z_AXIS,
    })
    .unwrap();
    refuses::<SweepOrientation>(&binormal, "", "direction");

    let helical = json::to_value(HelicalSweepConstruction {
        profile: PlanarProfileRef::Native("helix:profile".into()),
        axis_origin: FinitePoint3::ZERO,
        axis_direction: UnitVector3::Z_AXIS,
        law: HelicalSweepLaw::PitchTurnsAngle,
        pitch: NonNegativeLength::new(2.0).unwrap(),
        travel: HelicalSweepTravel::new(Length::new(10.0).unwrap(), Length::ZERO).unwrap(),
        turns: PositiveReal::new(5.0).unwrap(),
        cone_angle: Angle::ZERO,
        left_handed: false,
        reversed: false,
        tolerance: None,
        allow_multi_profile_faces: None,
    })
    .unwrap();
    refuses::<HelicalSweepConstruction>(&helical, "", "axis_direction");
}
