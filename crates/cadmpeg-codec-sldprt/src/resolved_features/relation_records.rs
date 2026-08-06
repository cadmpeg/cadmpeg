//! Relation instance records and scalar roles.

use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
    FeatureInputRelationInstance, FeatureInputScalar, FeatureInputScalarRole,
};
use std::collections::{HashMap, HashSet};

pub(super) fn relation_instances(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Vec<FeatureInputRelationInstance> {
    let sketch_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| feature.xml_tag.eq_ignore_ascii_case("Sketch"))
        .map(|feature| feature.id.as_str())
        .collect::<HashSet<_>>();
    let declarations = lane
        .classes
        .iter()
        .filter_map(|class| {
            relation_family(&class.name).map(|family| (class.offset, family, class.id.as_str()))
        })
        .collect::<Vec<_>>();
    let mut groups = Vec::<(
        String,
        FeatureInputRelationFamily,
        String,
        Vec<FeatureInputOperand>,
        Vec<&FeatureInputScalar>,
        usize,
    )>::new();
    for (scalar_index, scalar) in lane.scalars.iter().enumerate() {
        let Some(feature_ref) = scalar
            .feature_ref
            .as_deref()
            .filter(|feature| sketch_features.contains(feature))
        else {
            continue;
        };
        let Some((_, family, class_ref)) = declarations
            .iter()
            .filter(|(offset, family, _)| {
                *offset < scalar.offset && relation_signature(*family, &scalar.operands)
            })
            .max_by_key(|(offset, _, _)| offset)
        else {
            continue;
        };
        let append = groups.last().is_some_and(
            |(owner, candidate, group_class, operands, scalars, last_index)| {
                owner == feature_ref
                    && candidate == family
                    && group_class == class_ref
                    && *last_index + 1 == scalar_index
                    && scalars.len() == 1
                    && operands
                        .iter()
                        .map(|operand| (operand.kind, operand.entity_index))
                        .eq(scalar
                            .operands
                            .iter()
                            .map(|operand| (operand.kind, operand.entity_index)))
            },
        );
        if append {
            let (_, _, _, _, scalars, last_index) = groups
                .last_mut()
                .expect("append requires an existing relation group");
            scalars.push(scalar);
            *last_index = scalar_index;
        } else {
            groups.push((
                feature_ref.to_string(),
                *family,
                (*class_ref).to_string(),
                scalar.operands.clone(),
                vec![scalar],
                scalar_index,
            ));
        }
    }
    let mut instances = groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (feature_ref, family, class_ref, operands, scalars, _))| {
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let display = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
                    .copied()
                    .collect::<Vec<_>>();
                let offset = scalars[0].offset;
                FeatureInputRelationInstance {
                    id: format!(
                        "sldprt:feature-input:relation-instance#{}:{offset}",
                        lane.id
                            .rsplit_once('#')
                            .map_or(lane.id.as_str(), |(_, key)| key)
                    ),
                    parent: lane.id.clone(),
                    ordinal: ordinal as u32,
                    offset,
                    family,
                    class_ref,
                    feature_ref,
                    scalar_refs: scalars.iter().map(|scalar| scalar.id.clone()).collect(),
                    parameter_scalar_ref: (driving.len() == 1).then(|| driving[0].id.clone()),
                    display_scalar_ref: (display.len() == 1).then(|| display[0].id.clone()),
                    operands,
                }
            },
        )
        .collect::<Vec<_>>();
    bind_detached_relation_drivers(&mut instances, lane);
    bind_circle_dimension_centers(&mut instances, lane);
    instances
}

pub(super) fn bind_circle_dimension_centers(
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) {
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    for relation in relations.iter_mut().filter(|relation| {
        relation.family == FeatureInputRelationFamily::CircleDiameter
            && relation.operands.len() == 1
    }) {
        let Some(display) = relation
            .display_scalar_ref
            .as_deref()
            .and_then(|id| scalars.get(id).copied())
        else {
            continue;
        };
        let Some(display_name) = names.get(display.name.as_str()) else {
            continue;
        };
        let Some(display_index) = lane
            .scalars
            .iter()
            .position(|scalar| scalar.id == display.id)
        else {
            continue;
        };
        let first = &relation.operands[0];
        let candidates = lane
            .scalars
            .iter()
            .enumerate()
            .filter(|scalar| {
                let scalar = scalar.1;
                scalar.feature_ref == display.feature_ref
                    && names.get(scalar.name.as_str()) == Some(display_name)
                    && matches!(scalar.operands.as_slice(), [candidate, _]
                        if candidate.kind == first.kind
                            && candidate.entity_index == first.entity_index)
            })
            .collect::<Vec<_>>();
        if candidates.first().map(|candidate| candidate.0) != Some(display_index + 1)
            || candidates.windows(2).any(|pair| pair[1].0 != pair[0].0 + 1)
        {
            continue;
        }
        let centers = candidates
            .iter()
            .filter_map(|(_, scalar)| scalar.operands.get(1))
            .map(|operand| {
                (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.clone(),
                )
            })
            .collect::<Vec<_>>();
        let Some(center) = centers.first() else {
            continue;
        };
        if centers.iter().any(|candidate| candidate != center) {
            continue;
        }
        let Some((_, source)) = candidates.iter().find(|(_, scalar)| {
            scalar.operands.get(1).is_some_and(|operand| {
                (
                    operand.kind,
                    operand.entity_index,
                    operand.entity_ref.clone(),
                ) == *center
            })
        }) else {
            continue;
        };
        relation.operands = source.operands.clone();
        for (_, scalar) in candidates {
            if !relation.scalar_refs.contains(&scalar.id) {
                relation.scalar_refs.push(scalar.id.clone());
            }
        }
    }
}

pub(super) fn bind_detached_relation_drivers(
    relations: &mut [FeatureInputRelationInstance],
    lane: &FeatureInputLane,
) {
    let scalars = lane
        .scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<HashMap<_, _>>();
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<HashMap<_, _>>();
    let claimed = relations
        .iter()
        .flat_map(|relation| &relation.scalar_refs)
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut drivers = HashMap::<(String, String), Vec<&FeatureInputScalar>>::new();
    for scalar in lane.scalars.iter().filter(|scalar| {
        scalar.role == FeatureInputScalarRole::Driving
            && scalar.operands.is_empty()
            && !claimed.contains(scalar.id.as_str())
    }) {
        let (Some(feature), Some(name)) = (
            scalar.feature_ref.as_deref(),
            names.get(scalar.name.as_str()).copied(),
        ) else {
            continue;
        };
        drivers
            .entry((feature.to_string(), name.to_string()))
            .or_default()
            .push(scalar);
    }
    let mut candidates = HashMap::<(String, String), Vec<usize>>::new();
    for (index, relation) in relations.iter().enumerate() {
        if relation.parameter_scalar_ref.is_some() {
            continue;
        }
        let relation_names = relation
            .scalar_refs
            .iter()
            .filter_map(|id| scalars.get(id.as_str()))
            .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)
            .filter_map(|scalar| names.get(scalar.name.as_str()).copied())
            .collect::<HashSet<_>>();
        if relation_names.len() != 1 {
            continue;
        }
        let name = *relation_names
            .iter()
            .next()
            .expect("one display scalar name");
        candidates
            .entry((relation.feature_ref.clone(), name.to_string()))
            .or_default()
            .push(index);
    }
    for (key, relation_indices) in candidates {
        let [relation_index] = relation_indices.as_slice() else {
            continue;
        };
        let Some([driver]) = drivers.get(&key).map(Vec::as_slice) else {
            continue;
        };
        let relation = &mut relations[*relation_index];
        relation.scalar_refs.push(driver.id.clone());
        relation.parameter_scalar_ref = Some(driver.id.clone());
    }
}

fn relation_family(name: &str) -> Option<FeatureInputRelationFamily> {
    match native_object_class(name).kind {
        NativeClassKind::SketchRelation(family) => Some(family),
        _ => None,
    }
}

pub(super) fn relation_signature(
    family: FeatureInputRelationFamily,
    operands: &[FeatureInputOperand],
) -> bool {
    use FeatureInputOperandKind::{Native, D6, E1};
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    if family == CircleDiameter {
        return matches!(
            operands,
            [operand]
                if matches!(operand.kind, Native(_))
        );
    }
    let [first, second] = operands else {
        return false;
    };
    match family {
        PointPointDistance => {
            (first.kind == D6 && second.kind == D6)
                || (first.kind == Native(0x8152) && second.kind == Native(0x8152))
                || (first.kind == Native(0x837b) && second.kind == Native(0x837b))
                || (first.kind == Native(0xbc7c) && second.kind == Native(0xbc7c))
        }
        LineLineDistance => {
            (first.kind == E1 && second.kind == E1)
                || (first.kind == Native(0x8386) && second.kind == Native(0x8386))
                || (first.kind == Native(0xbc87) && second.kind == Native(0xbc87))
        }
        PointLineDistance => {
            (first.kind == D6 && second.kind == E1)
                || (first.kind == Native(0x837b) && second.kind == Native(0x8386))
                || (first.kind == Native(0xbc7c) && second.kind == Native(0xbc87))
        }
        PointPointHorizontalDistance | PointPointVerticalDistance => {
            (first.kind == Native(0x8152) && second.kind == Native(0x8152))
                || (first.kind == Native(0x8dcb) && second.kind == Native(0x8dcb))
        }
        Angle => first.kind == Native(0x8dda) && second.kind == Native(0x8dda),
        CircleDiameter => unreachable!("handled as a unary relation"),
    }
}

pub(super) fn scalar_role(payload: &[u8], trailer_offset: usize) -> FeatureInputScalarRole {
    let fixed_layout = payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 21)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 24..trailer_offset + 29) == Some(&[0, 0, 0, 2, 0]);
    let role_offset = if compact_scalar_layout(payload, trailer_offset) {
        trailer_offset + 27
    } else if fixed_layout {
        trailer_offset + 29
    } else if legacy_scalar_layout(payload, trailer_offset) {
        trailer_offset + 30
    } else {
        return FeatureInputScalarRole::Native;
    };
    match payload.get(role_offset) {
        Some(0) => FeatureInputScalarRole::Driving,
        Some(1) => FeatureInputScalarRole::Display,
        _ => FeatureInputScalarRole::Native,
    }
}

pub(super) fn compact_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 21)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 21..trailer_offset + 27) == Some(&[1, 0, 0, 0, 2, 0])
        && payload
            .get(trailer_offset + 28..trailer_offset + 35)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 39..trailer_offset + 43) == Some(&[0xff; 4])
        && payload.get(trailer_offset + 47..trailer_offset + 51) == Some(&[0xff; 4])
}

pub(super) fn legacy_scalar_layout(payload: &[u8], trailer_offset: usize) -> bool {
    payload.get(trailer_offset..trailer_offset + 3) == Some(&[0, 0, 0])
        && payload
            .get(trailer_offset + 7..trailer_offset + 24)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(trailer_offset + 24..trailer_offset + 30) == Some(&[0x0f, 0, 0, 0, 2, 0])
}
