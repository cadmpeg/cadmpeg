// SPDX-License-Identifier: Apache-2.0
//! Body and face motion in a `PmDc` feature.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    AxisAngle, BodySelection, FaceMotion, FaceSelection, Feature, FeatureDefinition,
    FeatureDirection3, FeatureOperation, FeatureResultMembers, FeatureResultTopology, FinitePoint3,
    FiniteVector3,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::UnitVector3;

use super::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectionIndex,
};
use crate::pmdc::PmDcReference;
use crate::sketch::PmDcTransform;

pub(super) const TRANSFORM_LIST_TYPE: [u8; 16] = [
    0x29, 0xfe, 0x00, 0x08, 0xd5, 0x11, 0xdd, 0x35, 0x10, 0x00, 0x36, 0x97, 0xab, 0x9f, 0x0a, 0xb5,
];
pub(super) const BODY_SELECTION_TYPE: [u8; 16] = [
    0x83, 0xaa, 0xda, 0xd5, 0x4f, 0x43, 0x75, 0xda, 0xcd, 0x10, 0x03, 0xb2, 0x60, 0xa8, 0xa4, 0x22,
];
pub(super) const MOTION_MODE_TYPE: [u8; 16] = [
    0xfb, 0x79, 0x82, 0xef, 0x33, 0x47, 0xf2, 0x4f, 0x71, 0x81, 0x33, 0x96, 0x8e, 0xff, 0xfa, 0x3f,
];

pub(super) const MOVE_CLASS_ID: super::ClassId =
    super::ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
        0xde, 0xa6, 0xa6, 0x8f, 0x4c, 0x4b, 0x52, 0x91, 0x88, 0xca, 0x31, 0xb5, 0x63, 0x01, 0x98,
        0xc8,
    ]));
pub(super) const ROTATE_CLASS_ID: super::ClassId =
    super::ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
        0xe6, 0xda, 0x3f, 0x26, 0x92, 0x4b, 0x5d, 0xab, 0x8e, 0x1b, 0xc5, 0xa9, 0x3f, 0xea, 0xd7,
        0xde,
    ]));
pub(super) const MOVE_BODY_CLASS_ID: super::ClassId =
    super::ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
        0xe9, 0x9f, 0x83, 0x2b, 0x7a, 0x42, 0x95, 0xa1, 0xd9, 0x76, 0x33, 0x83, 0xbe, 0xa1, 0x7c,
        0xce,
    ]));

type PropertyIndex<'a> = HashMap<(&'a str, u32), Option<&'a PmDcFeatureProperty>>;
type TransformIndex<'a> = HashMap<(&'a str, u32), Option<&'a PmDcTransform>>;

struct Motion<'a> {
    operation: FeatureOperation,
    selection: &'a PmDcFeatureProperty,
    bodies: Vec<String>,
}

fn resolve<'a, T>(
    source: &PmDcFeature,
    reference: PmDcReference,
    records: &HashMap<(&str, u32), Option<&'a T>>,
) -> Option<&'a T> {
    records
        .get(&(
            source.identity.segment_token.as_str(),
            reference.record_ordinal()?,
        ))
        .copied()
        .flatten()
}

fn slot<'a>(
    source: &PmDcFeature,
    slot: usize,
    records: &PropertyIndex<'a>,
) -> Option<&'a PmDcFeatureProperty> {
    resolve(source, *source.properties.references().get(slot)?, records)
}

fn selection<'a>(
    source: &PmDcFeature,
    slot_index: usize,
    family: PmDcFeatureReferenceFamily,
    records: &PropertyIndex<'a>,
) -> Option<(&'a PmDcFeatureProperty, Vec<String>)> {
    let selection = slot(source, slot_index, records)?;
    let PmDcFeaturePropertyKind::References {
        family: actual,
        items,
    } = &selection.kind
    else {
        return None;
    };
    if *actual != family || items.references().is_empty() {
        return None;
    }
    let mut bodies = Vec::new();
    let mut distinct = HashSet::new();
    for reference in items.references() {
        let body = resolve(source, *reference, records)?;
        if !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. })
            || !distinct.insert(body.identity.record_ordinal)
        {
            return None;
        }
        bodies.push(body.id());
    }
    Some((selection, bodies))
}

fn transform(
    source: &PmDcFeature,
    reference: PmDcReference,
    transforms: &TransformIndex<'_>,
) -> Option<[[f64; 4]; 4]> {
    let transform = resolve(source, reference, transforms)?;
    (transform.identity.type_id.as_str() == "184d8790d011f8d10008cabc0663dc09")
        .then(|| transform.matrix.rows())
}

fn decode<'a>(
    source: &PmDcFeature,
    properties: &PropertyIndex<'a>,
    transforms: &TransformIndex<'_>,
) -> Option<Motion<'a>> {
    if source.properties.marker() != 2 {
        return None;
    }
    let dimensions = match source.properties.references().len() {
        3 => 2,
        13 => 9,
        _ => return None,
    };
    if !matches!(
        slot(source, dimensions, properties)?.kind,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            ..
        }
    ) {
        return None;
    }
    match source.properties.references().len() {
        3 => {
            let (selection, bodies) = selection(
                source,
                0,
                PmDcFeatureReferenceFamily::ObjectCollection,
                properties,
            )?;
            let PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::TransformList,
                items,
            } = &slot(source, 1, properties)?.kind
            else {
                return None;
            };
            let [reference] = items.references() else {
                return None;
            };
            let matrix = transform(source, *reference, transforms)?;
            let (translation, rotation) = rigid_motion(matrix)?;
            Some(Motion {
                operation: FeatureOperation::MoveBody {
                    bodies: BodySelection::local(bodies.clone(), selection.id()).ok()?,
                    translation: {
                        let vector = translation.get();
                        [
                            cadmpeg_ir::scalar::Length::new(vector.x)?.into(),
                            cadmpeg_ir::scalar::Length::new(vector.y)?.into(),
                            cadmpeg_ir::scalar::Length::new(vector.z)?.into(),
                        ]
                    },
                    rotation,
                    copies: 0.into(),
                },
                selection,
                bodies,
            })
        }
        13 => {
            let (selection, bodies) = selection(
                source,
                8,
                PmDcFeatureReferenceFamily::BodySelection,
                properties,
            )?;
            let faces = slot(source, 0, properties)?;
            let PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items,
            } = &faces.kind
            else {
                return None;
            };
            if items.references().is_empty() {
                return None;
            }
            let mut distinct = HashSet::new();
            let mut has_face_indices = false;
            for reference in items.references() {
                let item = resolve(source, *reference, properties)?;
                let PmDcFeaturePropertyKind::FaceItem {
                    index_references, ..
                } = &item.kind
                else {
                    return None;
                };
                if !distinct.insert(item.identity.record_ordinal) {
                    return None;
                }
                has_face_indices |= !index_references.values().is_empty();
            }
            if !has_face_indices {
                return None;
            }
            if !matches!(
                slot(source, 1, properties)?.kind,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Motion,
                    type_value: 2,
                    value: 2,
                }
            ) || [2, 3, 4, 5, 6, 7, 10]
                .into_iter()
                .any(|slot| source.properties.references()[slot].index() != 0)
                || !matches!(
                    slot(source, 11, properties)?.kind,
                    PmDcFeaturePropertyKind::Boolean { value: true, .. }
                )
            {
                return None;
            }
            let matrix = transform(source, source.properties.references()[12], transforms)?;
            if (0..3)
                .any(|row| (0..3).any(|col| matrix[row][col] != if row == col { 1.0 } else { 0.0 }))
            {
                return None;
            }
            let (translation, rotation) = rigid_motion(matrix)?;
            if rotation.is_some() {
                return None;
            }
            let vector = translation.get();
            let distance = vector.x.hypot(vector.y).hypot(vector.z);
            let direction = if distance == 0.0 {
                UnitVector3::normalized(Vector3::new(1.0, 0.0, 0.0))?
            } else {
                UnitVector3::normalized(vector)?
            };
            Some(Motion {
                operation: FeatureOperation::MoveFace {
                    faces: FaceSelection::Native(faces.id()),
                    motion: FaceMotion::Translate {
                        direction: FeatureDirection3::from(direction),
                        distance: cadmpeg_ir::scalar::Length::new(distance)?.into(),
                    },
                },
                selection,
                bodies,
            })
        }
        _ => None,
    }
}

fn rigid_motion(matrix: [[f64; 4]; 4]) -> Option<(FiniteVector3, Option<AxisAngle>)> {
    if matrix[3] != [0.0, 0.0, 0.0, 1.0] {
        return None;
    }
    let transform = Transform::affine([matrix[0], matrix[1], matrix[2]])?;
    if !transform.is_proper_rigid() {
        return None;
    }
    let translation = FiniteVector3::new(Vector3::new(
        matrix[0][3] * 10.0,
        matrix[1][3] * 10.0,
        matrix[2][3] * 10.0,
    ))?;
    let diagonal = [
        1.0 + matrix[0][0] + matrix[1][1] + matrix[2][2],
        1.0 + matrix[0][0] - matrix[1][1] - matrix[2][2],
        1.0 - matrix[0][0] + matrix[1][1] - matrix[2][2],
        1.0 - matrix[0][0] - matrix[1][1] + matrix[2][2],
    ];
    let largest = (0..4).max_by(|&left, &right| diagonal[left].total_cmp(&diagonal[right]))?;
    let component = diagonal[largest].sqrt() * 0.5;
    let divisor = component * 4.0;
    let mut quaternion = match largest {
        0 => [
            component,
            (matrix[2][1] - matrix[1][2]) / divisor,
            (matrix[0][2] - matrix[2][0]) / divisor,
            (matrix[1][0] - matrix[0][1]) / divisor,
        ],
        1 => [
            (matrix[2][1] - matrix[1][2]) / divisor,
            component,
            (matrix[0][1] + matrix[1][0]) / divisor,
            (matrix[0][2] + matrix[2][0]) / divisor,
        ],
        2 => [
            (matrix[0][2] - matrix[2][0]) / divisor,
            (matrix[0][1] + matrix[1][0]) / divisor,
            component,
            (matrix[1][2] + matrix[2][1]) / divisor,
        ],
        _ => [
            (matrix[1][0] - matrix[0][1]) / divisor,
            (matrix[0][2] + matrix[2][0]) / divisor,
            (matrix[1][2] + matrix[2][1]) / divisor,
            component,
        ],
    };
    if quaternion[0] < 0.0 {
        quaternion = quaternion.map(|value| -value);
    }
    let [w, x, y, z] = quaternion;
    let sine = x.hypot(y).hypot(z);
    if sine == 0.0 {
        return Some((translation, None));
    }
    let direction = UnitVector3::normalized(Vector3::new(x / sine, y / sine, z / sine))?;
    let angle = Angle::new(2.0 * sine.atan2(w))?;
    Some((
        translation,
        Some(AxisAngle {
            origin: FinitePoint3::ZERO,
            direction: FeatureDirection3::from(direction),
            angle: angle.into(),
        }),
    ))
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
    if !class_admits(label.class_id(), source.properties.references().len()) {
        return None;
    }
    let motion = decode(source, &index.properties, &index.transforms)?;
    let members = FeatureResultMembers::new(
        motion
            .bodies
            .into_iter()
            .map(cadmpeg_core::text::NonBlankString::new)
            .collect::<Option<Vec<_>>>()?,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .ok()?;
    Some((|| {
        ctx.charge_entities(1, "project Inventor motion result topology")?;
        let id = super::history_feature_id(&source.identity)?;
        let result = FeatureResultTopology::new(
            FeatureResultTopologyId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                source.identity.key()?,
            ),
            id.clone(),
            members,
            Some(motion.selection.id()),
        );
        let mut feature = super::native_feature(ctx, &source.identity, Some(label), source.id())?;
        feature.id = id;
        feature.source_properties = super::boolean_properties(source, &[11], index);
        feature.source_tag = Some(
            if matches!(motion.operation, FeatureOperation::MoveFace { .. }) {
                "move_face"
            } else {
                "move_body"
            }
            .into(),
        );
        feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(motion.operation),
        );
        Ok((feature, result))
    })())
}

pub(crate) struct ValidationIndex<'a> {
    properties: PropertyIndex<'a>,
    transforms: TransformIndex<'a>,
}

impl<'a> ValidationIndex<'a> {
    pub(crate) fn new(
        properties: &'a [PmDcFeatureProperty],
        transforms: &'a [PmDcTransform],
    ) -> Self {
        Self {
            properties: unique_index(properties.iter().map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    record,
                )
            })),
            transforms: unique_index(transforms.iter().map(|record| {
                (
                    (
                        record.identity.segment_token.as_str(),
                        record.identity.record_ordinal,
                    ),
                    record,
                )
            })),
        }
    }

    pub(crate) fn matches(&self, source: &PmDcFeature, definition: &FeatureDefinition) -> bool {
        decode(source, &self.properties, &self.transforms)
            .is_some_and(|motion| definition == &FeatureDefinition::Operation(motion.operation))
    }
}

#[cfg(test)]
mod tests;

pub(super) fn parser(type_id: [u8; 16]) -> Option<super::PropertyParser> {
    match type_id {
        TRANSFORM_LIST_TYPE => Some(parse_transform_list),
        BODY_SELECTION_TYPE => Some(parse_body_selection),
        MOTION_MODE_TYPE => Some(parse_mode),
        _ => None,
    }
}

fn parse_transform_list(
    _: &DecodeContext<'_>,
    source: cadmpeg_core::decode::View<'_>,
    version: u8,
) -> Result<super::PmDcFeaturePropertyPayload, CodecError> {
    super::parse_references(source, version, PmDcFeatureReferenceFamily::TransformList)
}

fn parse_body_selection(
    _: &DecodeContext<'_>,
    source: cadmpeg_core::decode::View<'_>,
    version: u8,
) -> Result<super::PmDcFeaturePropertyPayload, CodecError> {
    super::parse_references(source, version, PmDcFeatureReferenceFamily::BodySelection)
}

fn parse_mode(
    _: &DecodeContext<'_>,
    source: cadmpeg_core::decode::View<'_>,
    version: u8,
) -> Result<super::PmDcFeaturePropertyPayload, CodecError> {
    super::parse_enumeration(source, version, PmDcFeatureEnumFamily::Motion)
}

pub(crate) fn property_is_valid<'a>(
    property: &PmDcFeatureProperty,
    record_type: impl Fn(PmDcReference) -> Option<&'a str>,
) -> bool {
    let id = property.identity.type_id.as_str();
    let (expected, member_type) = match &property.kind {
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::ObjectCollection,
            ..
        } => ("ae70680ed14a1e86d76248b03c2a96e1", None),
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            ..
        } => ("71f23ed8d2115094a00049803603c8c9", None),
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::TransformList,
            ..
        } => (
            "29fe0008d511dd3510003697ab9f0ab5",
            Some("184d8790d011f8d10008cabc0663dc09"),
        ),
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            ..
        } => (
            "83aadad54f4375dacd1003b260a8a422",
            Some("474d8790d011f8d10008cabc0663dc09"),
        ),
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceCollection,
            ..
        } => (
            "f1b2bc24d01121dd0008d0bc0663dc09",
            Some("ccb59f50d1114f6b0008a1ba32a3dc09"),
        ),
        PmDcFeaturePropertyKind::FaceItem { index_references } => {
            return id == "ccb59f50d1114f6b0008a1ba32a3dc09" && index_references.marker() == 2;
        }
        PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::Motion,
            type_value,
            ..
        } => {
            return id == "fb7982ef3347f24f718133968efffa3f" && *type_value == 2;
        }
        PmDcFeaturePropertyKind::SurfaceBody { .. } => {
            return id == "474d8790d011f8d10008cabc0663dc09";
        }
        PmDcFeaturePropertyKind::Boolean { .. } => {
            return matches!(
                id,
                "284d8790d011f8d10008cabc0663dc09" | "9f8f2a40d21120e66000aaabc8433cb0"
            );
        }
        _ => {
            return ![
                "ae70680ed14a1e86d76248b03c2a96e1",
                "71f23ed8d2115094a00049803603c8c9",
                "474d8790d011f8d10008cabc0663dc09",
                "284d8790d011f8d10008cabc0663dc09",
                "29fe0008d511dd3510003697ab9f0ab5",
                "83aadad54f4375dacd1003b260a8a422",
                "f1b2bc24d01121dd0008d0bc0663dc09",
                "ccb59f50d1114f6b0008a1ba32a3dc09",
                "fb7982ef3347f24f718133968efffa3f",
            ]
            .contains(&id)
        }
    };
    id == expected
        && match &property.kind {
            PmDcFeaturePropertyKind::References { family, items } => {
                let selection = matches!(
                    family,
                    PmDcFeatureReferenceFamily::BodySelection
                        | PmDcFeatureReferenceFamily::FaceCollection
                );
                let mut distinct = HashSet::new();
                let null_admitted = matches!(family, PmDcFeatureReferenceFamily::BodySelection);
                items.marker() == 2
                    && items.references().iter().all(|reference| {
                        (null_admitted && reference.index() == 0)
                            || ((!selection || distinct.insert(reference.index()))
                                && member_type.is_none_or(|expected| {
                                    record_type(*reference) == Some(expected)
                                }))
                    })
            }
            _ => false,
        }
}

pub(crate) fn class_admits(class: super::ClassId, property_count: usize) -> bool {
    match property_count {
        3 => [MOVE_CLASS_ID, MOVE_BODY_CLASS_ID, ROTATE_CLASS_ID].contains(&class),
        13 => class == MOVE_CLASS_ID,
        _ => false,
    }
}
