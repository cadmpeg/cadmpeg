// SPDX-License-Identifier: Apache-2.0
//! Hole center selections and termination targets.

use std::collections::HashSet;

use cadmpeg_ir::features::{holes::HolePlacement, LinearTermination};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::Length;

use super::{
    references, slot_property, target_plane, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectionIndex, EXTENT_DISTANCE,
    EXTENT_THROUGH_ALL, EXTENT_THROUGH_NEXT, EXTENT_TO,
};
use crate::sketch::{PmDcSketchEntityKind, PmDcTransform};

pub(super) const CENTERS_TYPE: [u8; 16] = [
    0xb1, 0x22, 0x65, 0x93, 0xd2, 0x11, 0x53, 0x59, 0xa0, 0x00, 0x21, 0x80, 0x36, 0x03, 0xc8, 0xc9,
];
pub(super) fn termination(
    source: &PmDcFeature,
    index: &ProjectionIndex<'_>,
    depth: Length,
) -> Option<LinearTermination> {
    let PmDcFeaturePropertyKind::Enumeration {
        family: PmDcFeatureEnumFamily::Extent,
        type_value: 11 | 12,
        value,
    } = slot_property(source, 9, index)?.kind
    else {
        return None;
    };
    match value {
        EXTENT_DISTANCE => Some(LinearTermination::Blind {
            length: cadmpeg_ir::scalar::PositiveLength::try_from(depth)
                .ok()?
                .into(),
        }),
        EXTENT_THROUGH_NEXT => Some(LinearTermination::ThroughNext {}),
        EXTENT_THROUGH_ALL => Some(LinearTermination::ThroughAll {}),
        EXTENT_TO => {
            let PmDcFeaturePropertyKind::TerminationFlag { value: flag } =
                slot_property(source, 15, index)?.kind
            else {
                return None;
            };
            let refs = source.properties.references();
            match (refs.get(13)?.index(), refs.get(14)?.index(), flag) {
                (plane, 0, 1) if plane != 0 => {
                    let plane = index
                        .work_planes
                        .get(&(
                            source.identity.segment_token.as_str(),
                            plane.checked_sub(1)?,
                        ))?
                        .as_ref()?;
                    let (plane, native) =
                        target_plane(plane.origin, plane.u_axis, plane.v_axis, plane.id())?;
                    Some(LinearTermination::ToPlane { plane, native })
                }
                (0, face, 0) if face != 0 => {
                    let record = slot_property(source, 14, index)?;
                    let PmDcFeaturePropertyKind::FaceItem {
                        index_references: items,
                    } = &record.kind
                    else {
                        return None;
                    };
                    if !matches!(items.values(), [value] if *value != 0) {
                        return None;
                    }
                    Some(LinearTermination::ToFace {
                        face: cadmpeg_ir::features::FaceSelection::Native(record.id()),
                        offset: None,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

pub(super) fn placements(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
    transform: &PmDcTransform,
    direction: cadmpeg_ir::units::UnitVector3,
) -> Option<Vec<HolePlacement>> {
    let refs = source.properties.references();
    for slot in [26, 27] {
        if refs
            .get(slot)
            .is_some_and(|reference| reference.index() != 0)
            && !matches!(
                slot_property(source, slot, index)?.kind,
                PmDcFeaturePropertyKind::Boolean { value: false, .. }
            )
        {
            return None;
        }
    }
    let transform_reference = refs.get(8)?.index();
    let positions = if refs.get(21)?.index() != 0 {
        let PmDcFeaturePropertyKind::Placement {
            transform: placed,
            point,
            value,
        } = &slot_property(source, 21, index)?.kind
        else {
            return None;
        };
        if placed.index() != transform_reference || point.index() == 0 || value.index() == 0 {
            return None;
        }
        vec![[0.0, 0.0]]
    } else {
        let [sketch_reference] = label.participants.references() else {
            return None;
        };
        let token = source.identity.segment_token.as_str();
        let sketch = index
            .sketches
            .get(&(token, sketch_reference.index().checked_sub(1)?))?
            .as_ref()?;
        if sketch.transform.index() != transform_reference
            || sketch.direction.index() != refs.get(16)?.index()
        {
            return None;
        }
        crate::sketch::project_placement(sketch, &index.transforms, &index.directions)?;
        let centers = references(source, 7, PmDcFeatureReferenceFamily::HoleCenters, index)?;
        if centers.references().is_empty() {
            return None;
        }
        let mut seen = HashSet::new();
        let members: HashSet<_> = sketch
            .entities
            .references()
            .iter()
            .map(|member| member.index())
            .collect();
        let mut positions = Vec::new();
        for reference in centers.references() {
            if !seen.insert(reference.index()) || !members.contains(&reference.index()) {
                return None;
            }
            let point = index
                .sketch_entities
                .get(&(token, reference.index().checked_sub(1)?))?
                .as_ref()?;
            if point.sketch.index() != sketch_reference.index() {
                return None;
            }
            let PmDcSketchEntityKind::Point { position, .. } = point.kind else {
                return None;
            };
            positions.push(position.map(cadmpeg_ir::scalar::FiniteReal::get));
        }
        positions
    };
    let rows = transform.matrix.rows();
    positions
        .into_iter()
        .map(|[u, v]| {
            let coordinate =
                |row: usize| (rows[row][0] * u + rows[row][1] * v + rows[row][3]) * 10.0;
            Some(HolePlacement::Directed {
                position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                    coordinate(0),
                    coordinate(1),
                    coordinate(2),
                ))?,
                direction: cadmpeg_ir::features::FeatureDirection3::from(direction),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
