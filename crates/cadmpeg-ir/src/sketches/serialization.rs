// SPDX-License-Identifier: Apache-2.0
//! Borrow sketch pattern rows, instances, and parameter identities for serialization.

use super::{SketchCircularPattern, SketchPatternDirection, SketchRectangularPattern};
use serde::ser::SerializeStruct as _;
use serde::{Serialize, Serializer};

impl Serialize for SketchPatternDirection {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = 2 + usize::from(self.distance.is_some())
            + usize::from(self.count_parameter.is_some());
        let mut wire = serializer.serialize_struct("SketchPatternDirectionWire", fields)?;
        wire.serialize_field("direction", &self.direction.get())?;
        wire.serialize_field("spacing", &self.spacing)?;
        if let Some(distance) = &self.distance {
            wire.serialize_field("distance", distance)?;
        }
        if let Some(parameter) = &self.count_parameter {
            wire.serialize_field("count_parameter", parameter)?;
        }
        wire.end()
    }
}

impl Serialize for SketchRectangularPattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut wire = serializer.serialize_struct("SketchRectangularPatternWire", 2)?;
        wire.serialize_field("directions", &self.directions)?;
        wire.serialize_field("rows", &self.rows)?;
        wire.end()
    }
}

impl Serialize for SketchCircularPattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = 4 + usize::from(self.angle_parameter.is_some())
            + usize::from(self.count_parameter.is_some());
        let mut wire = serializer.serialize_struct("SketchCircularPatternWire", fields)?;
        wire.serialize_field("center", &self.center)?;
        wire.serialize_field("angle", &self.angle)?;
        if let Some(parameter) = &self.angle_parameter {
            wire.serialize_field("angle_parameter", parameter)?;
        }
        if let Some(parameter) = &self.count_parameter {
            wire.serialize_field("count_parameter", parameter)?;
        }
        wire.serialize_field("seed", &self.seed)?;
        wire.serialize_field("instances", &self.instances.members)?;
        wire.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketches::{
        SketchCircularPatternInstance, SketchCircularPatternWire, SketchEntityId,
        SketchPatternDirectionWire, SketchPatternDistance, SketchPatternInstance,
        SketchRectangularPatternWire,
    };
    use crate::scalar::{Angle, Length, NonZeroAngle};

    #[test]
    fn borrowed_sketch_pattern_serialization_preserves_wire_bytes_and_omissions() {
        let parameter = crate::features::ParameterId::mint("test:model:parameter#🦀").unwrap();
        for mask in 0..4 {
            let distance_present = mask & 1 != 0;
            let count_present = mask & 2 != 0;
            let direction = SketchPatternDirection::new(
                [1.0, -0.0], Length::new(-2.0).unwrap(),
                distance_present.then(|| SketchPatternDistance::Spacing { parameter: parameter.clone() }),
                count_present.then(|| parameter.clone()),
            ).unwrap();
            let second = SketchPatternDirection::new(
                [0.0, 1.0], Length::new(0.0).unwrap(), None, None,
            ).unwrap();
            let rows = vec![vec![SketchPatternInstance {
                entities: vec![SketchEntityId::mint("test:model:sketch-entity#é").unwrap()],
            }]];
            let pattern = SketchRectangularPattern::new([direction, second], rows).unwrap();
            let [first, second] = &pattern.directions;
            let expected = SketchRectangularPatternWire {
                directions: [first, second].map(|value| SketchPatternDirectionWire {
                    direction: value.direction.get(), spacing: value.spacing,
                    distance: value.distance.clone(), count_parameter: value.count_parameter.clone(),
                }),
                rows: pattern.rows.clone(),
            };
            let bytes = serde_json::to_vec(&pattern).unwrap();
            assert_eq!(bytes, serde_json::to_vec(&expected).unwrap());
            assert_eq!(serde_json::from_slice::<SketchRectangularPattern>(&bytes).unwrap(), pattern);
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["directions"][0].get("distance").is_some(), distance_present);
            assert_eq!(value["directions"][0].get("count_parameter").is_some(), count_present);
            assert!(value["directions"][1].get("distance").is_none());

            let pattern = SketchCircularPattern::new(
                SketchEntityId::mint("test:model:sketch-entity#center").unwrap(),
                Angle::new(-1.0).unwrap(),
                distance_present.then(|| parameter.clone()), count_present.then(|| parameter.clone()),
                vec![SketchEntityId::mint("test:model:sketch-entity#seed").unwrap()],
                vec![SketchCircularPatternInstance {
                    angle: NonZeroAngle::new(-1.0).unwrap(),
                    entities: vec![SketchEntityId::mint("test:model:sketch-entity#copy").unwrap()],
                }],
            ).unwrap();
            let expected = SketchCircularPatternWire {
                center: pattern.center.clone(), angle: pattern.angle,
                angle_parameter: pattern.angle_parameter.clone(),
                count_parameter: pattern.count_parameter.clone(),
                seed: pattern.seed.clone(), instances: pattern.instances.members.clone(),
            };
            let bytes = serde_json::to_vec(&pattern).unwrap();
            assert_eq!(bytes, serde_json::to_vec(&expected).unwrap());
            assert_eq!(serde_json::from_slice::<SketchCircularPattern>(&bytes).unwrap(), pattern);
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value.get("angle_parameter").is_some(), distance_present);
            assert_eq!(value.get("count_parameter").is_some(), count_present);
        }
    }
}
