// SPDX-License-Identifier: Apache-2.0
//! Typed views over `SolidWorks` `ResolvedFeatures` sketch records.

use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputLane, FeatureInputName,
    FeatureInputOperand, FeatureInputOperandKind, FeatureInputReference,
    FeatureInputRelationBinding, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus, SketchNativeOperand,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{Read, Write};

use crate::container::ContainerScan;

const SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];
const CLASS_MARKER: &[u8] = &[0xff, 0xff, 0x01, 0x00];
const NAME_MARKER: &[u8] = &[0x04, 0x80, 0xff, 0xfe, 0xff];
const SCALAR_HEADER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
];
const SKETCH_POINT_TOLERANCE: f64 = 1.0e-9;

pub fn lanes(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<FeatureInputLane> {
    scan.blocks
        .iter()
        .filter_map(|block| {
            let section = block.section.as_deref()?;
            if !section.to_ascii_lowercase().contains("resolvedfeatures") {
                return None;
            }
            let parent = format!("sldprt:feature-input:resolved-features#{}", block.offset);
            let classes = class_declarations(&block.payload, &parent);
            let names = object_names(&block.payload, &parent);
            let scalars = named_scalars(&block.payload, &parent, &names);
            let relation_bindings = relation_bindings(&parent, &classes, &scalars);
            let references = reference_cells(&scalars);
            let sketch_entities = block
                .payload
                .windows(SKETCH_MARKER.len())
                .enumerate()
                .filter_map(|(offset, bytes)| (bytes == SKETCH_MARKER).then_some(offset))
                .filter_map(|offset| {
                    let code = u32::from_le_bytes(
                        block
                            .payload
                            .get(offset + 17..offset + 21)?
                            .try_into()
                            .ok()?,
                    );
                    Some((offset, code))
                })
                .enumerate()
                .map(|(ordinal, (offset, code))| {
                    let id = format!(
                        "sldprt:feature-input:sketch-entity#{}:{offset}",
                        block.offset
                    );
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        section,
                        offset as u64,
                        "ff_ff_1f_00_03",
                        Exactness::ByteExact,
                    );
                    SketchInputEntity {
                        id,
                        parent: parent.clone(),
                        ordinal: ordinal as u32,
                        offset: offset as u64,
                        local_id: marker_local_id(&block.payload, offset),
                        kind: SketchInputKind::from_native_code(code),
                        state_value: marker_state_value(&block.payload, offset),
                    }
                })
                .collect::<Vec<_>>();
            let id = parent;
            crate::annotations::note(
                annotations,
                id.clone(),
                section,
                0,
                "ResolvedFeatures",
                Exactness::ByteExact,
            );
            Some(FeatureInputLane {
                id,
                configuration: configuration(section),
                native_payload: block.payload.clone(),
                classes,
                names,
                scalars,
                relation_bindings,
                relation_instances: Vec::new(),
                references,
                sketch_entities,
            })
        })
        .collect()
}

pub(crate) fn relation_bindings(
    parent: &str,
    classes: &[FeatureInputClass],
    scalars: &[FeatureInputScalar],
) -> Vec<FeatureInputRelationBinding> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    classes
        .iter()
        .filter_map(|class| {
            let family = match class.name.as_str() {
                "sgLLDist" => FeatureInputRelationFamily::LineLineDistance,
                "sgPntPntDist" => FeatureInputRelationFamily::PointPointDistance,
                "sgPntLineDist" => FeatureInputRelationFamily::PointLineDistance,
                "sgPntPntHorDist" => FeatureInputRelationFamily::PointPointHorizontalDistance,
                "sgPntPntVertDist" => FeatureInputRelationFamily::PointPointVerticalDistance,
                "sgAnglDim" => FeatureInputRelationFamily::Angle,
                _ => return None,
            };
            let scalar = scalars
                .iter()
                .filter(|scalar| scalar.offset > class.offset)
                .min_by_key(|scalar| scalar.offset)?;
            (scalar.offset - class.offset <= 128).then_some((class, scalar, family))
        })
        .enumerate()
        .map(
            |(ordinal, (class, scalar, family))| FeatureInputRelationBinding {
                id: format!(
                    "sldprt:feature-input:relation-binding#{lane_key}:{}",
                    class.offset
                ),
                parent: parent.to_string(),
                ordinal: ordinal as u32,
                offset: class.offset,
                class_ref: class.id.clone(),
                family,
                scalar_ref: scalar.id.clone(),
                feature_ref: scalar.feature_ref.clone(),
            },
        )
        .collect()
}

pub(crate) fn reference_cells(scalars: &[FeatureInputScalar]) -> Vec<FeatureInputReference> {
    let mut cells = scalars
        .iter()
        .flat_map(|scalar| {
            scalar.operands.iter().map(|operand| FeatureInputReference {
                id: operand.reference_ref.clone(),
                parent: scalar.parent.clone(),
                feature_ref: scalar.feature_ref.clone(),
                ordinal: 0,
                offset: operand.offset,
                kind: operand.kind,
                object_index: operand.entity_index,
            })
        })
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| cell.offset);
    cells.dedup_by_key(|cell| cell.offset);
    for (ordinal, cell) in cells.iter_mut().enumerate() {
        cell.ordinal = ordinal as u32;
    }
    cells
}

pub(crate) fn marker_local_id(payload: &[u8], offset: usize) -> Option<u32> {
    let start = offset.checked_sub(4)?;
    let id = u32::from_le_bytes(payload.get(start..offset)?.try_into().ok()?);
    (id != u32::MAX).then_some(id)
}

fn marker_state_value(payload: &[u8], offset: usize) -> Option<f64> {
    let offset = offset.checked_add(48)?;
    let value = f64::from_le_bytes(payload.get(offset..offset + 8)?.try_into().ok()?);
    value.is_finite().then_some(value)
}

pub(crate) fn named_scalars(
    payload: &[u8],
    parent: &str,
    names: &[FeatureInputName],
) -> Vec<FeatureInputScalar> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    names
        .iter()
        .filter_map(|name| {
            let name_offset = usize::try_from(name.offset).ok()?;
            let value_offset = name_offset
                .checked_add(NAME_MARKER.len() + 1)?
                .checked_add(name.value.encode_utf16().count().checked_mul(2)?)?
                .checked_add(SCALAR_HEADER.len())?;
            let header_offset = value_offset.checked_sub(SCALAR_HEADER.len())?;
            if payload.get(header_offset..value_offset)? != SCALAR_HEADER {
                return None;
            }
            let value = f64::from_le_bytes(
                payload
                    .get(value_offset..value_offset + 8)?
                    .try_into()
                    .ok()?,
            );
            let object_id = u32::from_le_bytes(
                payload
                    .get(name_offset + 43..name_offset + 47)?
                    .try_into()
                    .ok()?,
            );
            let role = scalar_role(payload, name_offset);
            let operands = scalar_operands(payload, name_offset, parent);
            let entity_indices = operands
                .iter()
                .filter(|operand| operand.kind == FeatureInputOperandKind::D6)
                .map(|operand| operand.entity_index)
                .collect();
            value.is_finite().then_some((
                name,
                value_offset,
                object_id,
                value,
                role,
                entity_indices,
                operands,
            ))
        })
        .enumerate()
        .map(
            |(ordinal, (name, offset, object_id, value, role, entity_indices, operands))| {
                FeatureInputScalar {
                    id: format!("sldprt:feature-input:scalar#{lane_key}:{offset}"),
                    parent: parent.to_string(),
                    feature_ref: None,
                    ordinal: ordinal as u32,
                    offset: offset as u64,
                    object_id,
                    name: name.id.clone(),
                    value,
                    role,
                    entity_indices,
                    operands,
                }
            },
        )
        .collect()
}

pub(crate) fn scalar_indices_match(
    actual: &[FeatureInputScalar],
    expected: &[FeatureInputScalar],
) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            actual.id == expected.id
                && actual.parent == expected.parent
                && actual.feature_ref == expected.feature_ref
                && actual.ordinal == expected.ordinal
                && actual.offset == expected.offset
                && actual.object_id == expected.object_id
                && actual.name == expected.name
                && ulp_distance(actual.value, expected.value) <= 4
                && actual.role == expected.role
                && actual.entity_indices == expected.entity_indices
                && actual.operands == expected.operands
        })
}

fn ulp_distance(left: f64, right: f64) -> u64 {
    fn ordered(value: f64) -> u64 {
        let bits = value.to_bits();
        if bits & (1 << 63) == 0 {
            bits | (1 << 63)
        } else {
            !bits
        }
    }
    ordered(left).abs_diff(ordered(right))
}

fn scalar_operands(payload: &[u8], name_offset: usize, parent: &str) -> Vec<FeatureInputOperand> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    [75usize, 87]
        .into_iter()
        .filter_map(|relative| {
            let offset = name_offset.checked_add(relative)?;
            let cell = payload.get(offset..offset + 12)?;
            if cell[4..8] != [0xff; 4] || cell[8..12] != [0; 4] {
                return None;
            }
            let kind = operand_kind([cell[0], cell[1]])?;
            Some(FeatureInputOperand {
                offset: offset as u64,
                reference_ref: format!("sldprt:feature-input:reference#{lane_key}:{offset}"),
                kind,
                entity_index: u16::from_le_bytes([cell[2], cell[3]]),
                entity_ref: None,
            })
        })
        .collect()
}

fn operand_kind(tag: [u8; 2]) -> Option<FeatureInputOperandKind> {
    match tag {
        [0, 0] | [0xff, 0xff] => None,
        [0xd6, 0x80] => Some(FeatureInputOperandKind::D6),
        [0xe1, 0x80] => Some(FeatureInputOperandKind::E1),
        bytes => Some(FeatureInputOperandKind::Native(u16::from_le_bytes(bytes))),
    }
}

fn feature_object_name<'a>(
    feature: &crate::records::Feature,
    lane: &'a FeatureInputLane,
) -> Option<&'a FeatureInputName> {
    if let Some(source_id) = feature
        .source_id
        .as_deref()
        .and_then(|value| value.parse::<u32>().ok())
    {
        let mut matches = lane
            .names
            .iter()
            .filter(|name| name.object_id == Some(source_id));
        if let Some(first) = matches.next() {
            if matches.next().is_none() {
                return Some(first);
            }
            return None;
        }
    }
    let mut matches = lane.names.iter().filter(|name| name.value == feature.name);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

/// Resolve scalar operand indices within their owning feature-object interval.
pub(crate) fn bind_scalar_operands(
    histories: &[crate::records::FeatureHistory],
    lanes: &mut [FeatureInputLane],
) {
    for lane in lanes {
        let mut starts = histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                Some((
                    feature_object_name(feature, lane)?.offset,
                    feature.id.as_str(),
                ))
            })
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|start| start.0);
        for (index, &(start, feature_id)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            for reference in lane
                .references
                .iter_mut()
                .filter(|reference| reference.offset > start && reference.offset < end)
            {
                reference.feature_ref = Some(feature_id.to_string());
            }
            let entities = lane
                .sketch_entities
                .iter()
                .filter(|entity| entity.offset > start && entity.offset < end)
                .collect::<Vec<_>>();
            for scalar in lane
                .scalars
                .iter_mut()
                .filter(|scalar| scalar.offset > start && scalar.offset < end)
            {
                scalar.feature_ref = Some(feature_id.to_string());
                for operand in &mut scalar.operands {
                    if !matches!(
                        operand.kind,
                        FeatureInputOperandKind::D6
                            | FeatureInputOperandKind::Native(0x837b | 0x8dcb | 0xbc7c)
                    ) {
                        continue;
                    }
                    let mut matches = entities
                        .iter()
                        .filter(|entity| entity.local_id == Some(u32::from(operand.entity_index)));
                    let Some(entity) = matches.next() else {
                        continue;
                    };
                    if matches.next().is_none() {
                        operand.entity_ref = Some(entity.id.clone());
                    }
                }
            }
        }
        let scalar_owners = lane
            .scalars
            .iter()
            .map(|scalar| (scalar.id.as_str(), scalar.feature_ref.clone()))
            .collect::<HashMap<_, _>>();
        for binding in &mut lane.relation_bindings {
            binding.feature_ref = scalar_owners
                .get(binding.scalar_ref.as_str())
                .cloned()
                .flatten();
        }
        lane.relation_instances = relation_instances(histories, lane);
    }
}

fn relation_instances(
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
    )>::new();
    for scalar in &lane.scalars {
        let Some(feature_ref) = scalar
            .feature_ref
            .as_deref()
            .filter(|feature| sketch_features.contains(feature))
        else {
            continue;
        };
        let [first, second] = scalar.operands.as_slice() else {
            continue;
        };
        let Some((_, family, class_ref)) = declarations
            .iter()
            .filter(|(offset, family, _)| {
                *offset < scalar.offset && relation_signature(*family, first.kind, second.kind)
            })
            .max_by_key(|(offset, _, _)| offset)
        else {
            continue;
        };
        if let Some((_, _, _, _, scalars)) =
            groups
                .iter_mut()
                .find(|(owner, candidate, _, operands, _)| {
                    owner == feature_ref
                        && candidate == family
                        && operands
                            .iter()
                            .map(|operand| (operand.kind, operand.entity_index))
                            .eq(scalar
                                .operands
                                .iter()
                                .map(|operand| (operand.kind, operand.entity_index)))
                })
        {
            scalars.push(scalar);
        } else {
            groups.push((
                feature_ref.to_string(),
                *family,
                (*class_ref).to_string(),
                scalar.operands.clone(),
                vec![scalar],
            ));
        }
    }
    groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (feature_ref, family, class_ref, operands, scalars))| {
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
        .collect()
}

fn relation_family(name: &str) -> Option<FeatureInputRelationFamily> {
    match native_object_class(name).kind {
        NativeClassKind::SketchRelation(family) => Some(family),
        _ => None,
    }
}

fn relation_signature(
    family: FeatureInputRelationFamily,
    first: FeatureInputOperandKind,
    second: FeatureInputOperandKind,
) -> bool {
    use FeatureInputOperandKind::{Native, D6, E1};
    use FeatureInputRelationFamily::{
        Angle, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    match family {
        PointPointDistance => {
            (first == D6 && second == D6)
                || (first == Native(0x837b) && second == Native(0x837b))
                || (first == Native(0xbc7c) && second == Native(0xbc7c))
        }
        LineLineDistance => {
            (first == E1 && second == E1)
                || (first == Native(0x8386) && second == Native(0x8386))
                || (first == Native(0xbc87) && second == Native(0xbc87))
        }
        PointLineDistance => {
            (first == D6 && second == E1)
                || (first == Native(0x837b) && second == Native(0x8386))
                || (first == Native(0xbc7c) && second == Native(0xbc87))
        }
        PointPointHorizontalDistance | PointPointVerticalDistance => {
            first == Native(0x8dcb) && second == Native(0x8dcb)
        }
        Angle => first == Native(0x8dda) && second == Native(0x8dda),
    }
}

fn scalar_role(payload: &[u8], name_offset: usize) -> FeatureInputScalarRole {
    let fixed_layout = payload.get(name_offset + 40..name_offset + 43) == Some(&[0, 0, 0])
        && payload
            .get(name_offset + 47..name_offset + 61)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(name_offset + 64..name_offset + 69) == Some(&[0, 0, 0, 2, 0]);
    if !fixed_layout {
        return FeatureInputScalarRole::Native;
    }
    match payload.get(name_offset + 69) {
        Some(0) => FeatureInputScalarRole::Driving,
        Some(1) => FeatureInputScalarRole::Display,
        _ => FeatureInputScalarRole::Native,
    }
}

/// Add unambiguous `ResolvedFeatures` length parameters to a projection copy of history.
pub(crate) fn enrich_history_parameters(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut candidates = BTreeMap::<(usize, usize, String), Vec<f64>>::new();
    for lane in lanes {
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name))
            .collect::<HashMap<_, _>>();
        let mut starts = Vec::<(u64, usize, usize)>::new();
        for (history_index, history) in histories.iter().enumerate() {
            for (feature_index, feature) in history.features.iter().enumerate() {
                let Some(name) = feature_object_name(feature, lane) else {
                    continue;
                };
                starts.push((name.offset, history_index, feature_index));
            }
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, history_index, feature_index)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let mut owned = BTreeMap::<&str, Vec<&FeatureInputScalar>>::new();
            for scalar in lane
                .scalars
                .iter()
                .filter(|scalar| scalar.offset > start && scalar.offset < end)
            {
                let Some(name) = names_by_id.get(scalar.name.as_str()) else {
                    continue;
                };
                owned.entry(&name.value).or_default().push(scalar);
            }
            for (name, scalars) in owned {
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates_for_name = if driving.is_empty() {
                    scalars
                        .into_iter()
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                if let [scalar] = candidates_for_name.as_slice() {
                    candidates
                        .entry((history_index, feature_index, name.to_string()))
                        .or_default()
                        .push(scalar.value);
                }
            }
        }
    }

    for ((history_index, feature_index, name), values) in candidates {
        let Some((&first, rest)) = values.split_first() else {
            continue;
        };
        if rest.iter().any(|value| value.to_bits() != first.to_bits()) {
            continue;
        }
        let feature = &mut histories[history_index].features[feature_index];
        let expression = crate::history::format_native_scalar(feature, &name, first);
        feature.parameters.entry(name).or_insert(expression);
    }
}

/// Bind Keywords history records to their serialized feature-input object classes.
pub(crate) fn bind_history_classes(
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let mut classes_by_object = HashMap::<u32, Vec<&str>>::new();
    for lane in lanes {
        let names_by_offset = lane
            .names
            .iter()
            .map(|name| (name.offset, name))
            .collect::<HashMap<_, _>>();
        for class in &lane.classes {
            let name_offset = class.offset + 6 + class.name.len() as u64;
            let Some(name) = names_by_offset.get(&name_offset) else {
                continue;
            };
            if let Some(object_id) = name.object_id {
                classes_by_object
                    .entry(object_id)
                    .or_default()
                    .push(&class.name);
            }
        }
    }

    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let classes = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
            .and_then(|object_id| classes_by_object.get(&object_id));
        let Some(classes) = classes else {
            continue;
        };
        let Some((&first, rest)) = classes.split_first() else {
            continue;
        };
        if rest.iter().all(|class| *class == first) {
            feature.input_class = Some(first.to_string());
        }
    }
}

/// Bind profile streams to uniquely enclosing sketch feature records.
pub(crate) fn bind_sketch_profiles(
    features: &mut [cadmpeg_ir::features::Feature],
    sketches: &mut [Sketch],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
    annotations: &Annotations,
) {
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let mut starts = Vec::<(u64, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            let Some(name) = feature_object_name(feature, lane) else {
                continue;
            };
            starts.push((name.offset, feature));
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, native_feature)) in starts.iter().enumerate() {
            let Some(feature) = features.iter_mut().find(|feature| {
                feature.native_ref.as_deref() == Some(native_feature.id.as_str())
                    && matches!(
                        feature.definition,
                        cadmpeg_ir::features::FeatureDefinition::Sketch { .. }
                    )
            }) else {
                continue;
            };
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let mut enclosed = sketches.iter_mut().filter(|sketch| {
                sketch.native_ref.as_deref() == Some(lane.id.as_str())
                    && annotations
                        .provenance
                        .get(&sketch.id.0)
                        .is_some_and(|source| source.offset > start && source.offset < end)
            });
            let Some(sketch) = enclosed.next() else {
                continue;
            };
            if enclosed.next().is_some() {
                continue;
            }
            sketch.name = Some(native_feature.name.clone());
            if let cadmpeg_ir::features::FeatureDefinition::Sketch {
                sketch: feature_sketch,
                ..
            } = &mut feature.definition
            {
                *feature_sketch = Some(sketch.id.clone());
            }
        }
    }
}

/// Bind neutral parameters to uniquely owned native scalar records.
pub(crate) fn bind_parameter_scalars(
    parameters: &mut [cadmpeg_ir::features::DesignParameter],
    features: &[cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let neutral_owners = features
        .iter()
        .filter_map(|feature| Some((&feature.id, feature.native_ref.as_deref()?)))
        .collect::<HashMap<_, _>>();
    let native_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for lane in lanes {
        let names_by_id = lane
            .names
            .iter()
            .map(|name| (name.id.as_str(), name.value.as_str()))
            .collect::<HashMap<_, _>>();
        let mut starts = Vec::<(u64, &crate::records::Feature)>::new();
        for feature in native_features.values() {
            let Some(name) = feature_object_name(feature, lane) else {
                continue;
            };
            starts.push((name.offset, feature));
        }
        starts.sort_by_key(|start| start.0);
        for (index, &(start, native_feature)) in starts.iter().enumerate() {
            let end = starts.get(index + 1).map_or(u64::MAX, |next| next.0);
            let owner_parameters = parameters.iter_mut().filter(|parameter| {
                parameter
                    .owner
                    .as_ref()
                    .and_then(|owner| neutral_owners.get(owner))
                    .copied()
                    == Some(native_feature.id.as_str())
            });
            for parameter in owner_parameters {
                if parameter.native_ref.is_some() {
                    continue;
                }
                let scalars = lane
                    .scalars
                    .iter()
                    .filter(|scalar| scalar.offset > start && scalar.offset < end)
                    .filter(|scalar| {
                        names_by_id.get(scalar.name.as_str()).copied()
                            == Some(parameter.name.as_str())
                    })
                    .collect::<Vec<_>>();
                let driving = scalars
                    .iter()
                    .filter(|scalar| scalar.role == FeatureInputScalarRole::Driving)
                    .copied()
                    .collect::<Vec<_>>();
                let candidates = if driving.is_empty() {
                    scalars
                        .into_iter()
                        .filter(|scalar| scalar.role == FeatureInputScalarRole::Native)
                        .collect::<Vec<_>>()
                } else {
                    driving
                };
                if let [scalar] = candidates.as_slice() {
                    parameter.native_ref = Some(scalar.id.clone());
                }
            }
        }
    }
}

/// Project owned native relation bindings into their neutral sketches.
pub(crate) fn project_relation_bindings(
    constraints: &mut Vec<SketchConstraint>,
    features: &[cadmpeg_ir::features::Feature],
    parameters: &[cadmpeg_ir::features::DesignParameter],
    lanes: &[FeatureInputLane],
) {
    let sketches_by_feature = features
        .iter()
        .filter_map(|feature| {
            let cadmpeg_ir::features::FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } = &feature.definition
            else {
                return None;
            };
            Some((feature.native_ref.as_deref()?, sketch))
        })
        .collect::<HashMap<_, _>>();
    let parameters_by_scalar = parameters
        .iter()
        .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, &parameter.id)))
        .collect::<HashMap<_, _>>();

    for lane in lanes {
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        for relation in &lane.relation_instances {
            let Some(sketch) = sketches_by_feature.get(relation.feature_ref.as_str()) else {
                continue;
            };
            let parameter = relation
                .parameter_scalar_ref
                .as_deref()
                .and_then(|scalar| parameters_by_scalar.get(scalar))
                .map(|parameter| (*parameter).clone());
            let native_kind = match relation.family {
                FeatureInputRelationFamily::LineLineDistance => "sgLLDist",
                FeatureInputRelationFamily::PointPointDistance => "sgPntPntDist",
                FeatureInputRelationFamily::PointLineDistance => "sgPntLineDist",
                FeatureInputRelationFamily::PointPointHorizontalDistance => "sgPntPntHorDist",
                FeatureInputRelationFamily::PointPointVerticalDistance => "sgPntPntVertDist",
                FeatureInputRelationFamily::Angle => "sgAnglDim",
            };
            constraints.push(SketchConstraint {
                id: SketchConstraintId(format!(
                    "sldprt:model:sketch-constraint#relation:{lane_key}:{}",
                    relation.offset
                )),
                sketch: (*sketch).clone(),
                definition: SketchConstraintDefinition::Native {
                    native_kind: native_kind.into(),
                    native_state: None,
                    entities: Vec::new(),
                    parameter,
                    operands: relation
                        .operands
                        .iter()
                        .map(|operand| SketchNativeOperand {
                            native_kind: operand_kind_name(operand.kind),
                            native_field: None,
                            native_role: None,
                            object_index: u32::from(operand.entity_index),
                            native_ref: operand.entity_ref.clone(),
                        })
                        .collect(),
                },
                name: None,
                driving: None,
                active: None,
                virtual_space: None,
                visible: None,
                orientation: None,
                label_distance: None,
                label_position: None,
                metadata: None,
                native_ref: Some(relation.id.clone()),
            });
        }
    }
}

fn operand_kind_name(kind: FeatureInputOperandKind) -> String {
    match kind {
        FeatureInputOperandKind::D6 => "d6".into(),
        FeatureInputOperandKind::E1 => "e1".into(),
        FeatureInputOperandKind::Native(tag) => {
            let [first, second] = tag.to_le_bytes();
            format!("{first:02x}{second:02x}")
        }
    }
}

pub(crate) fn object_names(payload: &[u8], parent: &str) -> Vec<FeatureInputName> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    payload
        .windows(NAME_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == NAME_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(*payload.get(offset + NAME_MARKER.len())?);
            if !(1..=128).contains(&length) {
                return None;
            }
            let start = offset + NAME_MARKER.len() + 1;
            let end = start.checked_add(length.checked_mul(2)?)?;
            let units = payload
                .get(start..end)?
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>();
            let value = String::from_utf16(&units).ok()?;
            let object_id = end.checked_add(8).and_then(|offset| {
                Some(u32::from_le_bytes(
                    payload.get(offset..offset + 4)?.try_into().ok()?,
                ))
            });
            (!value.chars().any(char::is_control)).then_some((offset, object_id, value))
        })
        .enumerate()
        .map(|(ordinal, (offset, object_id, value))| FeatureInputName {
            id: format!("sldprt:feature-input:name#{lane_key}:{offset}"),
            parent: parent.to_string(),
            ordinal: ordinal as u32,
            offset: offset as u64,
            object_id,
            value,
        })
        .collect()
}

pub(crate) fn class_declarations(payload: &[u8], parent: &str) -> Vec<FeatureInputClass> {
    let lane_key = parent.rsplit_once('#').map_or(parent, |(_, key)| key);
    payload
        .windows(CLASS_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == CLASS_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length = usize::from(u16::from_le_bytes(
                payload.get(offset + 4..offset + 6)?.try_into().ok()?,
            ));
            if !(1..=128).contains(&length) {
                return None;
            }
            let bytes = payload.get(offset + 6..offset + 6 + length)?;
            if !bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return None;
            }
            Some((offset, std::str::from_utf8(bytes).ok()?.to_string()))
        })
        .enumerate()
        .map(|(ordinal, (offset, name))| {
            let role = class_role(&name);
            FeatureInputClass {
                id: format!("sldprt:feature-input:class#{lane_key}:{offset}"),
                parent: parent.to_string(),
                ordinal: ordinal as u32,
                offset: offset as u64,
                name,
                role,
            }
        })
        .collect()
}

fn class_role(name: &str) -> FeatureInputClassRole {
    native_object_class(name).role
}

fn configuration(section: &str) -> Option<String> {
    let start = section.find("Config-")? + "Config-".len();
    let tail = &section[start..];
    let end = tail
        .find("-ResolvedFeatures")
        .or_else(|| tail.find('/'))
        .unwrap_or(tail.len());
    (!tail[..end].is_empty()).then(|| tail[..end].to_string())
}

/// Decode nested feature-input Parasolid streams as placed planar sketches.
pub fn sketches(
    scan: &ContainerScan,
    annotations: &mut Annotations,
) -> (Vec<Sketch>, Vec<SketchEntity>, Vec<SketchConstraint>) {
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut constraints = Vec::new();
    for block in &scan.blocks {
        let Some(section) = block.section.as_deref() else {
            continue;
        };
        if !section.to_ascii_lowercase().contains("resolvedfeatures") {
            continue;
        }
        let native_ref = format!("sldprt:feature-input:resolved-features#{}", block.offset);
        for (stream_ordinal, payload) in block.ps_streams.iter().enumerate() {
            let stream_offset = block.ps_stream_offsets[stream_ordinal];
            let Some(header) = crate::parasolid::stream_header(payload) else {
                continue;
            };
            let brep = crate::brep::decode(payload, &header, section);
            project_brep(
                &brep,
                block.offset,
                stream_ordinal,
                stream_offset,
                section,
                &header.description,
                configuration(section).as_deref(),
                &native_ref,
                annotations,
                &mut sketches,
                &mut entities,
                &mut constraints,
            );
        }
    }
    (sketches, entities, constraints)
}

#[allow(clippy::too_many_arguments)]
fn project_brep(
    brep: &crate::brep::Brep,
    block_offset: usize,
    stream_ordinal: usize,
    stream_offset: usize,
    section: &str,
    sketch_name: &str,
    configuration: Option<&str>,
    native_ref: &str,
    annotations: &mut Annotations,
    sketches: &mut Vec<Sketch>,
    entities: &mut Vec<SketchEntity>,
    constraints: &mut Vec<SketchConstraint>,
) {
    let surfaces = brep
        .surfaces
        .iter()
        .map(|surface| (&surface.id, &surface.geometry))
        .collect::<HashMap<_, _>>();
    let loops = brep
        .loops
        .iter()
        .map(|loop_| (&loop_.id, loop_))
        .collect::<HashMap<_, _>>();
    let coedges = brep
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let edges = brep
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let vertices = brep
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, &vertex.point))
        .collect::<HashMap<_, _>>();
    let points = brep
        .points
        .iter()
        .map(|point| (&point.id, point.position))
        .collect::<HashMap<_, _>>();
    let curves = brep
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect::<HashMap<_, _>>();

    for (face_ordinal, face) in brep.faces.iter().enumerate() {
        let Some(SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        }) = surfaces.get(&face.surface).copied()
        else {
            continue;
        };
        let sketch_id = SketchId(format!(
            "sldprt:model:sketch#{block_offset}:{stream_ordinal}:{face_ordinal}"
        ));
        let v_axis = cross(*normal, *u_axis);
        let first_entity = entities.len();
        let mut edge_entities = HashMap::<&cadmpeg_ir::ids::EdgeId, SketchEntityId>::new();
        let mut used_vertices = HashSet::new();
        let mut profiles = Vec::new();
        for loop_id in &face.loops {
            let Some(loop_) = loops.get(loop_id) else {
                continue;
            };
            let mut profile = Vec::new();
            for coedge_id in &loop_.coedges {
                let Some(coedge) = coedges.get(coedge_id) else {
                    continue;
                };
                let Some(edge) = edges.get(&coedge.edge) else {
                    continue;
                };
                used_vertices.insert(edge.start.clone());
                used_vertices.insert(edge.end.clone());
                let entity_id = if let Some(id) = edge_entities.get(&edge.id) {
                    id.clone()
                } else {
                    let id = SketchEntityId(format!(
                        "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                        edge_entities.len()
                    ));
                    let Some(geometry) =
                        project_edge(edge, &vertices, &points, &curves, *origin, *u_axis, v_axis)
                    else {
                        continue;
                    };
                    let Some(start_point) = vertices.get(&edge.start) else {
                        continue;
                    };
                    let Some(end_point) = vertices.get(&edge.end) else {
                        continue;
                    };
                    crate::annotations::note(
                        annotations,
                        id.0.clone(),
                        section,
                        0,
                        "feature_input_profile_edge",
                        Exactness::Derived,
                    );
                    entities.push(SketchEntity {
                        id: id.clone(),
                        sketch: sketch_id.clone(),
                        construction: false,
                        native_ref: Some(format!("{stream_ordinal}:{}", edge.id.0)),
                        geometry_ref: edge
                            .curve
                            .as_ref()
                            .map(|id| format!("{stream_ordinal}:{}", id.0)),
                        endpoint_refs: vec![
                            format!("{stream_ordinal}:{}", start_point.0),
                            format!("{stream_ordinal}:{}", end_point.0),
                        ],
                        geometry,
                    });
                    edge_entities.insert(&edge.id, id.clone());
                    id
                };
                if edge.curve.is_some() || edge.start != edge.end {
                    profile.push(SketchEntityUse {
                        entity: entity_id,
                        reversed: coedge.sense == Sense::Reversed,
                    });
                }
            }
            if !profile.is_empty() {
                profiles.push(profile);
            }
        }
        for vertex in &brep.vertices {
            if used_vertices.contains(&vertex.id) {
                continue;
            }
            let Some(position) = points.get(&vertex.point) else {
                continue;
            };
            let id = SketchEntityId(format!(
                "sldprt:model:sketch-entity#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                edge_entities.len()
                    + entities
                        .iter()
                        .filter(|entity| entity.sketch == sketch_id)
                        .count()
            ));
            crate::annotations::note(
                annotations,
                id.0.clone(),
                section,
                0,
                "feature_input_profile_point",
                Exactness::Derived,
            );
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: Some(format!("{stream_ordinal}:{}", vertex.id.0)),
                geometry_ref: None,
                endpoint_refs: vec![format!("{stream_ordinal}:{}", vertex.point.0)],
                geometry: SketchGeometry::Point {
                    position: project_point(*position, *origin, *u_axis, v_axis),
                },
            });
        }
        if profiles.is_empty() && !entities.iter().any(|entity| entity.sketch == sketch_id) {
            continue;
        }
        crate::annotations::note(
            annotations,
            sketch_id.0.clone(),
            section,
            stream_offset as u64,
            "feature_input_profile",
            Exactness::Derived,
        );
        project_endpoint_constraints(
            &sketch_id,
            &entities[first_entity..],
            block_offset,
            stream_ordinal,
            face_ordinal,
            section,
            annotations,
            constraints,
        );
        sketches.push(Sketch {
            id: sketch_id,
            name: (!sketch_name.is_empty()).then(|| sketch_name.to_string()),
            configuration: configuration.map(str::to_string),
            origin: *origin,
            normal: *normal,
            u_axis: *u_axis,
            profiles,
            native_ref: Some(native_ref.to_string()),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn project_endpoint_constraints(
    sketch: &SketchId,
    entities: &[SketchEntity],
    block_offset: usize,
    stream_ordinal: usize,
    face_ordinal: usize,
    section: &str,
    annotations: &mut Annotations,
    constraints: &mut Vec<SketchConstraint>,
) {
    let mut loci_by_endpoint = BTreeMap::<&str, Vec<SketchLocus>>::new();
    for entity in entities {
        if entity.endpoint_refs.len() != 2 {
            continue;
        }
        for (index, endpoint) in entity.endpoint_refs.iter().enumerate() {
            let locus = if index == 0 {
                SketchLocus::Start(entity.id.clone())
            } else {
                SketchLocus::End(entity.id.clone())
            };
            loci_by_endpoint.entry(endpoint).or_default().push(locus);
        }
    }
    for (_endpoint, loci) in loci_by_endpoint {
        let distinct_entities = loci
            .iter()
            .map(|locus| match locus {
                SketchLocus::Start(entity)
                | SketchLocus::End(entity)
                | SketchLocus::Center(entity)
                | SketchLocus::Entity(entity) => entity,
            })
            .collect::<HashSet<_>>();
        if distinct_entities.len() < 2 {
            continue;
        }
        let id = SketchConstraintId(format!(
            "sldprt:model:sketch-constraint#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
            constraints.len()
        ));
        crate::annotations::note(
            annotations,
            id.0.clone(),
            section,
            0,
            "feature_input_shared_endpoint",
            Exactness::Derived,
        );
        constraints.push(SketchConstraint {
            id,
            sketch: sketch.clone(),
            definition: SketchConstraintDefinition::CoincidentLoci { loci },
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    }
}

fn project_edge(
    edge: &cadmpeg_ir::topology::Edge,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::ids::PointId>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
) -> Option<SketchGeometry> {
    let start = project_point(
        *points.get(vertices.get(&edge.start)?)?,
        origin,
        u_axis,
        v_axis,
    );
    let end = project_point(
        *points.get(vertices.get(&edge.end)?)?,
        origin,
        u_axis,
        v_axis,
    );
    match edge.curve.as_ref().and_then(|id| curves.get(id).copied()) {
        Some(CurveGeometry::Circle { center, radius, .. }) => {
            let center = project_point(*center, origin, u_axis, v_axis);
            if (start.u - end.u).hypot(start.v - end.v) <= 1.0e-9 {
                Some(SketchGeometry::Circle {
                    center,
                    radius: cadmpeg_ir::features::Length(*radius),
                })
            } else {
                Some(SketchGeometry::Arc {
                    center,
                    radius: cadmpeg_ir::features::Length(*radius),
                    start_angle: cadmpeg_ir::features::Angle(
                        (start.v - center.v).atan2(start.u - center.u),
                    ),
                    end_angle: cadmpeg_ir::features::Angle(
                        (end.v - center.v).atan2(end.u - center.u),
                    ),
                })
            }
        }
        Some(CurveGeometry::Ellipse {
            center,
            major_direction,
            major_radius,
            minor_radius,
            ..
        }) => {
            let center = project_point(*center, origin, u_axis, v_axis);
            let major_u = dot(*major_direction, u_axis);
            let major_v = dot(*major_direction, v_axis);
            let major_angle = major_v.atan2(major_u);
            let full = (start.u - end.u).hypot(start.v - end.v) <= 1.0e-9;
            let parameter = |point: Point2| {
                let du = point.u - center.u;
                let dv = point.v - center.v;
                let major_component = du * major_angle.cos() + dv * major_angle.sin();
                let minor_component = -du * major_angle.sin() + dv * major_angle.cos();
                (minor_component / *minor_radius).atan2(major_component / *major_radius)
            };
            Some(SketchGeometry::Ellipse {
                center,
                major_angle: cadmpeg_ir::features::Angle(major_angle),
                major_radius: cadmpeg_ir::features::Length(*major_radius),
                minor_radius: cadmpeg_ir::features::Length(*minor_radius),
                start_angle: (!full).then(|| cadmpeg_ir::features::Angle(parameter(start))),
                end_angle: (!full).then(|| cadmpeg_ir::features::Angle(parameter(end))),
            })
        }
        Some(CurveGeometry::Nurbs(nurbs)) => Some(SketchGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots.clone(),
            control_points: nurbs
                .control_points
                .iter()
                .map(|point| project_point(*point, origin, u_axis, v_axis))
                .collect(),
            weights: nurbs.weights.clone(),
            periodic: nurbs.periodic,
        }),
        None if edge.start == edge.end => Some(SketchGeometry::Point { position: start }),
        Some(CurveGeometry::Line { .. }) | None => Some(SketchGeometry::Line { start, end }),
        Some(other) => Some(SketchGeometry::Native {
            native_kind: format!("{other:?}"),
        }),
    }
}

fn project_point(point: Point3, origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Point2 {
    let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    Point2::new(dot(delta, u_axis), dot(delta, v_axis))
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

/// Stable hash of neutral sketch records.
pub fn sketch_hash(ir: &cadmpeg_ir::CadIr) -> String {
    hash_debug(&(
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &ir.model.sketch_constraints,
    ))
}

/// Stable hash of neutral sketch constraints.
pub fn constraint_hash(ir: &cadmpeg_ir::CadIr) -> String {
    hash_debug(&ir.model.sketch_constraints)
}

/// Stable hash of retained native feature-input lanes.
pub fn lane_hash(native: &crate::native::SldprtNative) -> String {
    hash_debug(&native.feature_input_lanes)
}

fn hash_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> String {
    let bytes = format!("{value:?}");
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes.as_bytes()) {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}

/// Reject unsupported neutral sketch edits before native lane replay.
pub fn prepare_sketches_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let baseline_neutral = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_neutral_sketch_sha256"));
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_sketch_sha256"));
    let baseline_constraints = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_neutral_sketch_constraint_sha256")
    });
    let current_neutral = sketch_hash(ir);
    let current_native = native.as_ref().map(lane_hash);
    if baseline_neutral.is_none() && baseline_native.is_none() {
        if ir.model.sketches.is_empty()
            && ir.model.sketch_entities.is_empty()
            && ir.model.sketch_constraints.is_empty()
        {
            return Ok(());
        }
        validate_source_less_constraints(ir)?;
        let generated = source_less_lanes(ir)?;
        native
            .get_or_insert_with(crate::native::SldprtNative::default)
            .feature_input_lanes
            .extend(generated);
        return Ok(());
    }
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &current_neutral);
    if !neutral_changed {
        return Ok(());
    }
    let current_constraints = constraint_hash(ir);
    if baseline_constraints.is_none_or(|hash| hash != &current_constraints) {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
            "SLDPRT native sketch relation editing is not implemented".into(),
        ));
    }
    let native_changed = match (&current_native, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    };
    if native_changed {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "conflicting neutral and native SLDPRT sketch edits".into(),
        ));
    }
    patch_line_profiles(
        ir,
        native.as_mut().ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "SLDPRT sketch write-back requires retained feature-input lanes".into(),
            )
        })?,
    )
}

fn validate_source_less_constraints(
    ir: &cadmpeg_ir::CadIr,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    for constraint in &ir.model.sketch_constraints {
        let SketchConstraintDefinition::CoincidentLoci { loci } = &constraint.definition else {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
                "source-less SLDPRT sketch constraints support only solved endpoint coincidences"
                    .into(),
            ));
        };
        if loci.len() < 2 {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "sketch constraint {} requires at least two loci",
                constraint.id.0
            )));
        }
        let sketch = ir
            .model
            .sketches
            .iter()
            .find(|sketch| sketch.id == constraint.sketch)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch constraint {} references missing sketch {}",
                    constraint.id.0, constraint.sketch.0
                ))
            })?;
        let v_axis = cross(sketch.normal, sketch.u_axis);
        let mut expected = None;
        for locus in loci {
            let (entity_id, start) = match locus {
                SketchLocus::Start(entity) => (entity, true),
                SketchLocus::End(entity) => (entity, false),
                _ => {
                    return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
                        "source-less SLDPRT sketch constraints support only solved endpoint coincidences"
                            .into(),
                    ));
                }
            };
            let entity = ir
                .model
                .sketch_entities
                .iter()
                .find(|entity| entity.id == *entity_id && entity.sketch == sketch.id)
                .ok_or_else(|| {
                    cadmpeg_ir::codec::CodecError::Malformed(format!(
                        "sketch constraint {} references entity {} outside sketch {}",
                        constraint.id.0, entity_id.0, sketch.id.0
                    ))
                })?;
            let curve = generated_sketch_curve(&entity.geometry, sketch, v_axis)?;
            let point = if start { curve.start } else { curve.end };
            if expected.is_some_and(|expected| !same_sketch_point(expected, point)) {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "source-less SLDPRT sketch constraint {} has unsolved endpoint coordinates",
                    constraint.id.0
                )));
            }
            expected = Some(point);
        }
    }
    Ok(())
}

fn source_less_lanes(
    ir: &cadmpeg_ir::CadIr,
) -> Result<Vec<FeatureInputLane>, cadmpeg_ir::codec::CodecError> {
    let mut lanes = Vec::<FeatureInputLane>::new();
    for sketch in &ir.model.sketches {
        let configuration = sketch.configuration.clone().unwrap_or_else(|| "0".into());
        let section = format!("Contents/Config-{configuration}-ResolvedFeatures");
        let lane = if let Some(lane) = lanes
            .iter_mut()
            .find(|lane| lane.configuration.as_deref() == Some(configuration.as_str()))
        {
            lane
        } else {
            lanes.push(FeatureInputLane {
                id: section,
                configuration: Some(configuration.clone()),
                native_payload: Vec::new(),
                classes: Vec::new(),
                names: Vec::new(),
                scalars: Vec::new(),
                relation_bindings: Vec::new(),
                relation_instances: Vec::new(),
                references: Vec::new(),
                sketch_entities: Vec::new(),
            });
            lanes.last_mut().expect("lane was inserted")
        };
        let sketch_ir = sketch_brep(ir, sketch)?;
        let body = crate::writer::brep_body(&sketch_ir, 0.001, false)?;
        lane.native_payload
            .extend(crate::writer::parasolid_stream_named(
                &body,
                "SCH_SW_33103_11000",
                sketch.name.as_deref().unwrap_or(&sketch.id.0),
            ));
    }
    Ok(lanes)
}

fn sketch_brep(
    source: &cadmpeg_ir::CadIr,
    sketch: &Sketch,
) -> Result<cadmpeg_ir::CadIr, cadmpeg_ir::codec::CodecError> {
    let mut ir = cadmpeg_ir::CadIr::empty(source.units.clone());
    let prefix = format!("generated:sldprt:sketch:{}", sketch.id.0);
    let body_id = BodyId(format!("{prefix}:body"));
    let region_id = RegionId(format!("{prefix}:region"));
    let shell_id = ShellId(format!("{prefix}:shell"));
    let face_id = FaceId(format!("{prefix}:face"));
    let surface_id = SurfaceId(format!("{prefix}:surface"));
    let v_axis = cross(sketch.normal, sketch.u_axis);
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: sketch.origin,
            normal: sketch.normal,
            u_axis: sketch.u_axis,
        },
        source_object: None,
    });
    let ordered_entities = source
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
        .collect::<Vec<_>>();
    let entities = ordered_entities
        .iter()
        .copied()
        .map(|entity| (entity.id.clone(), entity))
        .collect::<HashMap<_, _>>();
    let referenced = sketch
        .profiles
        .iter()
        .flatten()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<HashSet<_>>();
    if let Some(entity) = ordered_entities.iter().find(|entity| {
        !referenced.contains(&entity.id) && !matches!(entity.geometry, SketchGeometry::Point { .. })
    }) {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch writing cannot encode unprofiled curve {}",
            entity.id.0
        )));
    }
    let profiles = sketch.profiles.clone();
    let mut face_loops = Vec::new();
    let mut vertex_by_position = HashMap::<(u64, u64), VertexId>::new();
    for (profile_index, profile) in profiles.iter().enumerate() {
        if profile.is_empty() {
            continue;
        }
        let endpoints = profile
            .iter()
            .map(|entity_use| {
                let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                    cadmpeg_ir::codec::CodecError::Malformed(format!(
                        "sketch {} references missing entity {}",
                        sketch.id.0, entity_use.entity.0
                    ))
                })?;
                let generated = generated_sketch_curve(&entity.geometry, sketch, v_axis)?;
                Ok(if entity_use.reversed {
                    (generated.end, generated.start)
                } else {
                    (generated.start, generated.end)
                })
            })
            .collect::<Result<Vec<_>, cadmpeg_ir::codec::CodecError>>()?;
        if endpoints.iter().enumerate().any(|(index, (_, end))| {
            let (next_start, _) = endpoints[(index + 1) % endpoints.len()];
            !same_sketch_point(*end, next_start)
        }) {
            return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
                "source-less SLDPRT sketch profile {profile_index} is not a closed endpoint chain"
            )));
        }
        let loop_id = LoopId(format!("{prefix}:loop:{profile_index}"));
        face_loops.push(loop_id.clone());
        let mut coedge_ids = Vec::new();
        for (use_index, entity_use) in profile.iter().enumerate() {
            let entity = entities.get(&entity_use.entity).ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch {} references missing entity {}",
                    sketch.id.0, entity_use.entity.0
                ))
            })?;
            let generated = generated_sketch_curve(&entity.geometry, sketch, v_axis)?;
            let start_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                generated.start,
                sketch,
                v_axis,
            );
            let end_vertex = sketch_vertex(
                &mut ir,
                &mut vertex_by_position,
                &prefix,
                generated.end,
                sketch,
                v_axis,
            );
            let start_3d = lift_point(generated.start, sketch.origin, sketch.u_axis, v_axis);
            let end_3d = lift_point(generated.end, sketch.origin, sketch.u_axis, v_axis);
            let delta = Vector3::new(
                end_3d.x - start_3d.x,
                end_3d.y - start_3d.y,
                end_3d.z - start_3d.z,
            );
            let length = (dot(delta, delta)).sqrt();
            if length == 0.0 && matches!(entity.geometry, SketchGeometry::Line { .. }) {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "sketch entity {} has zero length",
                    entity.id.0
                )));
            }
            let curve_id = CurveId(format!("{prefix}:curve:{profile_index}:{use_index}"));
            let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{use_index}"));
            let coedge_id = CoedgeId(format!("{prefix}:coedge:{profile_index}:{use_index}"));
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: generated.curve,
                source_object: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: start_vertex,
                end: end_vertex,
                param_range: Some(generated.param_range.unwrap_or([0.0, length])),
                tolerance: None,
            });
            coedge_ids.push(coedge_id.clone());
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id,
                sense: if entity_use.reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                pcurves: Vec::new(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
        }
        let count = coedge_ids.len();
        for (index, coedge) in ir
            .model
            .coedges
            .iter_mut()
            .rev()
            .take(count)
            .rev()
            .enumerate()
        {
            coedge.next = coedge_ids[(index + 1) % count].clone();
            coedge.previous = coedge_ids[(index + count - 1) % count].clone();
        }
        ir.model.loops.push(Loop {
            id: loop_id,
            face: face_id.clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: coedge_ids,
            vertex_uses: Vec::new(),
        });
    }
    for (ordinal, entity) in ordered_entities.iter().enumerate() {
        let SketchGeometry::Point { position } = entity.geometry else {
            continue;
        };
        let point_id = PointId(format!("{prefix}:free-point:{ordinal}"));
        let vertex_id = VertexId(format!("{prefix}:free-vertex:{ordinal}"));
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: lift_point(position, sketch.origin, sketch.u_axis, v_axis),
            source_object: None,
        });
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
        });
        let edge_id = EdgeId(format!("{prefix}:point-edge:{ordinal}"));
        let loop_id = LoopId(format!("{prefix}:point-loop:{ordinal}"));
        let coedge_id = CoedgeId(format!("{prefix}:point-coedge:{ordinal}"));
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            curve: None,
            start: vertex_id.clone(),
            end: vertex_id,
            param_range: None,
            tolerance: None,
        });
        ir.model.coedges.push(Coedge {
            id: coedge_id.clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            next: coedge_id.clone(),
            previous: coedge_id.clone(),
            radial_next: coedge_id.clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
            use_curve_parameter_range: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id.clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: vec![coedge_id],
            vertex_uses: Vec::new(),
        });
        face_loops.push(loop_id);
    }
    if face_loops.is_empty() {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(format!(
            "source-less SLDPRT sketch {} has no profiles",
            sketch.id.0
        )));
    }
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: surface_id,
        sense: Sense::Forward,
        loops: face_loops,
        name: sketch.name.clone(),
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: vec![face_id],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::Sheet,
        regions: vec![region_id],
        transform: None,
        name: sketch.name.clone(),
        color: None,
        visible: None,
    });
    ir.model.finalize();
    Ok(ir)
}

struct GeneratedSketchCurve {
    curve: CurveGeometry,
    start: Point2,
    end: Point2,
    param_range: Option<[f64; 2]>,
}

fn generated_sketch_curve(
    geometry: &SketchGeometry,
    sketch: &Sketch,
    v_axis: Vector3,
) -> Result<GeneratedSketchCurve, cadmpeg_ir::codec::CodecError> {
    let lift = |point| lift_point(point, sketch.origin, sketch.u_axis, v_axis);
    let vector = |u: f64, v: f64| {
        Vector3::new(
            sketch.u_axis.x * u + v_axis.x * v,
            sketch.u_axis.y * u + v_axis.y * v,
            sketch.u_axis.z * u + v_axis.z * v,
        )
    };
    match geometry {
        SketchGeometry::Line { start, end } => {
            let origin = lift(*start);
            let target = lift(*end);
            let delta = Vector3::new(
                target.x - origin.x,
                target.y - origin.y,
                target.z - origin.z,
            );
            let length = dot(delta, delta).sqrt();
            if length == 0.0 {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(
                    "source-less SLDPRT sketch contains a zero-length line".into(),
                ));
            }
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Line {
                    origin,
                    direction: Vector3::new(
                        delta.x / length,
                        delta.y / length,
                        delta.z / length,
                    ),
                },
                start: *start,
                end: *end,
                param_range: Some([0.0, length]),
            })
        }
        SketchGeometry::Circle { center, radius } => {
            let point = offset_point(*center, Point2::new(radius.0, 0.0));
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Circle {
                    center: lift(*center),
                    axis: sketch.normal,
                    ref_direction: sketch.u_axis,
                    radius: radius.0,
                },
                start: point,
                end: point,
                param_range: Some([0.0, std::f64::consts::TAU]),
            })
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Ok(GeneratedSketchCurve {
            curve: CurveGeometry::Circle {
                center: lift(*center),
                axis: sketch.normal,
                ref_direction: sketch.u_axis,
                radius: radius.0,
            },
            start: offset_point(*center, polar(radius.0, start_angle.0)),
            end: offset_point(*center, polar(radius.0, end_angle.0)),
            param_range: Some([start_angle.0, end_angle.0]),
        }),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                        - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                    center.v
                        + major_angle.0.sin() * major_radius.0 * parameter.cos()
                        + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                )
            };
            let start = start_angle.as_ref().map_or(0.0, |angle| angle.0);
            let end = end_angle
                .as_ref()
                .map_or(std::f64::consts::TAU, |angle| angle.0);
            let full = start_angle.is_none() && end_angle.is_none();
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Ellipse {
                    center: lift(*center),
                    axis: sketch.normal,
                    major_direction: vector(major_angle.0.cos(), major_angle.0.sin()),
                    major_radius: major_radius.0,
                    minor_radius: minor_radius.0,
                },
                start: point(start),
                end: if full { point(start) } else { point(end) },
                param_range: Some([start, end]),
            })
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => {
            if *periodic || control_points.len() < 2 {
                return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
                    "source-less SLDPRT sketch writing requires a non-periodic NURBS with at least two poles".into(),
                ));
            }
            let start = control_points[0];
            let end = control_points[control_points.len() - 1];
            Ok(GeneratedSketchCurve {
                curve: CurveGeometry::Nurbs(NurbsCurve {
                    degree: *degree,
                    knots: knots.clone(),
                    control_points: control_points.iter().copied().map(lift).collect(),
                    weights: weights.clone(),
                    periodic: false,
                }),
                start,
                end,
                param_range: knots
                    .get(*degree as usize)
                    .zip(knots.get(knots.len().saturating_sub(*degree as usize + 1)))
                    .map(|(start, end)| [*start, *end]),
            })
        }
        SketchGeometry::Point { .. }
        | SketchGeometry::Text { .. }
        | SketchGeometry::ReferenceLine { .. }
        | SketchGeometry::Hyperbola { .. }
        | SketchGeometry::Parabola { .. }
        | SketchGeometry::Native { .. } => Err(
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "source-less SLDPRT sketch writing does not support point, text, or native-only profile entities".into(),
            ),
        ),
    }
}

fn sketch_vertex(
    ir: &mut cadmpeg_ir::CadIr,
    vertices: &mut HashMap<(u64, u64), VertexId>,
    prefix: &str,
    position: Point2,
    sketch: &Sketch,
    v_axis: Vector3,
) -> VertexId {
    if let Some((_, id)) = vertices.iter().find(|((u, v), _)| {
        same_sketch_point(
            Point2::new(f64::from_bits(*u), f64::from_bits(*v)),
            position,
        )
    }) {
        return id.clone();
    }
    let key = (position.u.to_bits(), position.v.to_bits());
    let ordinal = vertices.len();
    let point_id = PointId(format!("{prefix}:point:{ordinal}"));
    let vertex_id = VertexId(format!("{prefix}:vertex:{ordinal}"));
    ir.model.points.push(Point {
        id: point_id.clone(),
        position: lift_point(position, sketch.origin, sketch.u_axis, v_axis),
        source_object: None,
    });
    ir.model.vertices.push(Vertex {
        id: vertex_id.clone(),
        point: point_id,
        tolerance: None,
    });
    vertices.insert(key, vertex_id.clone());
    vertex_id
}

fn same_sketch_point(left: Point2, right: Point2) -> bool {
    (left.u - right.u).abs() <= SKETCH_POINT_TOLERANCE
        && (left.v - right.v).abs() <= SKETCH_POINT_TOLERANCE
}

fn patch_line_profiles(
    ir: &cadmpeg_ir::CadIr,
    native: &mut crate::native::SldprtNative,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let mut requested = HashMap::<(String, usize, u16), Point3>::new();
    let mut curves = Vec::new();
    for sketch in &ir.model.sketches {
        let lane_id = sketch.native_ref.as_ref().ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "SLDPRT sketch write-back requires native sketch provenance".into(),
            )
        })?;
        let v_axis = cross(sketch.normal, sketch.u_axis);
        for entity in ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
        {
            if entity.endpoint_refs.len() != 2 {
                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch entity {} lacks two endpoint references",
                    entity.id.0
                )));
            }
            match &entity.geometry {
                SketchGeometry::Point { position } => {
                    let reference = &entity.endpoint_refs[0];
                    let (stream, attr) = parse_point_ref(reference)?;
                    let point = lift_point(*position, sketch.origin, sketch.u_axis, v_axis);
                    requested.insert((lane_id.clone(), stream, attr), point);
                }
                SketchGeometry::Line { start, end } => {
                    for (reference, point) in entity.endpoint_refs.iter().zip([start, end]) {
                        let (stream, attr) = parse_point_ref(reference)?;
                        let point = lift_point(*point, sketch.origin, sketch.u_axis, v_axis);
                        let key = (lane_id.clone(), stream, attr);
                        if let Some(previous) = requested.insert(key, point) {
                            if distance(previous, point) > 1.0e-9 {
                                return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                                    "SLDPRT shared sketch point {reference} has conflicting positions"
                                )));
                            }
                        }
                    }
                }
                geometry @ (SketchGeometry::Circle { .. }
                | SketchGeometry::Arc { .. }
                | SketchGeometry::Ellipse { .. }
                | SketchGeometry::Nurbs { .. }) => {
                    let geometry_ref = entity.geometry_ref.as_deref().ok_or_else(|| {
                        cadmpeg_ir::codec::CodecError::Malformed(
                            "SLDPRT sketch curve lacks native carrier provenance".into(),
                        )
                    })?;
                    let (stream, carrier_attr) = parse_point_ref(geometry_ref)?;
                    let (_, start_attr) = parse_point_ref(&entity.endpoint_refs[0])?;
                    let (_, end_attr) = parse_point_ref(&entity.endpoint_refs[1])?;
                    if let Some(endpoints) = bounded_endpoints(geometry) {
                        for (reference, point) in entity.endpoint_refs.iter().zip(endpoints) {
                            let (point_stream, attr) = parse_point_ref(reference)?;
                            let point = lift_point(point, sketch.origin, sketch.u_axis, v_axis);
                            let key = (lane_id.clone(), point_stream, attr);
                            if let Some(previous) = requested.insert(key, point) {
                                if distance(previous, point) > 1.0e-9 {
                                    return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                                        "SLDPRT shared sketch point {reference} has conflicting positions"
                                    )));
                                }
                            }
                        }
                    }
                    curves.push(CurvePatch {
                        lane_id: lane_id.clone(),
                        stream,
                        carrier_attr,
                        start_attr,
                        end_attr,
                        geometry: geometry.clone(),
                        origin: sketch.origin,
                        u_axis: sketch.u_axis,
                        v_axis,
                    });
                }
                _ => {
                    return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
                        "SLDPRT sketch write-back does not support this curve family".into(),
                    ))
                }
            }
        }
    }
    for ((lane_id, stream_ordinal, attr), point) in requested {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.id == lane_id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch lane {lane_id} is missing"
                ))
            })?;
        patch_direct_stream_point(&mut lane.native_payload, stream_ordinal, attr, point)?;
    }
    for request in curves {
        let lane = native
            .feature_input_lanes
            .iter_mut()
            .find(|lane| lane.id == request.lane_id)
            .ok_or_else(|| {
                cadmpeg_ir::codec::CodecError::Malformed(format!(
                    "SLDPRT sketch lane {} is missing",
                    request.lane_id
                ))
            })?;
        patch_direct_curve(&mut lane.native_payload, &request)?;
    }
    Ok(())
}

fn bounded_endpoints(geometry: &SketchGeometry) -> Option<[Point2; 2]> {
    match geometry {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            offset_point(*center, polar(radius.0, start_angle.0)),
            offset_point(*center, polar(radius.0, end_angle.0)),
        ]),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle: Some(start),
            end_angle: Some(end),
        } => {
            let point = |parameter: f64| {
                Point2::new(
                    center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                        - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
                    center.v
                        + major_angle.0.sin() * major_radius.0 * parameter.cos()
                        + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
                )
            };
            Some([point(start.0), point(end.0)])
        }
        SketchGeometry::Nurbs {
            control_points,
            periodic: false,
            ..
        } if control_points.len() >= 2 => {
            Some([control_points[0], control_points[control_points.len() - 1]])
        }
        _ => None,
    }
}

struct CurvePatch {
    lane_id: String,
    stream: usize,
    carrier_attr: u16,
    start_attr: u16,
    end_attr: u16,
    geometry: SketchGeometry,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
}

fn parse_point_ref(reference: &str) -> Result<(usize, u16), cadmpeg_ir::codec::CodecError> {
    let (stream, id) = reference.split_once(':').ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed(format!(
            "invalid SLDPRT sketch endpoint reference {reference}"
        ))
    })?;
    let attr = id.rsplit('#').next().and_then(|value| value.parse().ok());
    match (stream.parse().ok(), attr) {
        (Some(stream), Some(attr)) => Ok((stream, attr)),
        _ => Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
            "invalid SLDPRT sketch endpoint reference {reference}"
        ))),
    }
}

fn lift_point(point: Point2, origin: Point3, u_axis: Vector3, v_axis: Vector3) -> Point3 {
    Point3::new(
        origin.x + point.u * u_axis.x + point.v * v_axis.x,
        origin.y + point.u * u_axis.y + point.v * v_axis.y,
        origin.z + point.u * u_axis.z + point.v * v_axis.z,
    )
}

fn distance(left: Point3, right: Point3) -> f64 {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
}

fn patch_direct_stream_point(
    payload: &mut Vec<u8>,
    stream_ordinal: usize,
    attr: u16,
    point_mm: Point3,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let xyz_m = [point_mm.x * 0.001, point_mm.y * 0.001, point_mm.z * 0.001];
    edit_stream(payload, stream_ordinal, |body| {
        if !crate::brep::patch_point(body, attr, xyz_m) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(format!(
                "SLDPRT sketch point {attr} is missing"
            )));
        }
        Ok(())
    })
}

fn patch_direct_curve(
    payload: &mut Vec<u8>,
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    edit_stream(payload, request.stream, |body| {
        patch_direct_curve_body(body, request)
    })
}

fn patch_direct_curve_body(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    if matches!(request.geometry, SketchGeometry::Nurbs { .. }) {
        return patch_direct_nurbs(body, request);
    }
    let Some(CurveGeometry::Circle {
        axis,
        ref_direction,
        ..
    }) = crate::brep::curve_by_attr(body, request.carrier_attr)
    else {
        return patch_direct_ellipse(body, request);
    };
    let (center_2d, radius, angles) = match request.geometry {
        SketchGeometry::Circle { center, radius } => (center, radius.0, None),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (center, radius.0, Some((start_angle.0, end_angle.0))),
        _ => {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch carrier family changed".into(),
            ))
        }
    };
    let center = lift_point(center_2d, request.origin, request.u_axis, request.v_axis);
    let curve = CurveGeometry::Circle {
        center,
        axis,
        ref_direction,
        radius,
    };
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch circle carrier cannot be patched".into(),
        ));
    }
    let endpoints = angles.map_or(
        [offset_point(center_2d, polar(radius, 0.0)); 2],
        |(start, end)| {
            [
                offset_point(center_2d, polar(radius, start)),
                offset_point(center_2d, polar(radius, end)),
            ]
        },
    );
    for (attr, endpoint) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(endpoints)
    {
        let point = lift_point(endpoint, request.origin, request.u_axis, request.v_axis);
        if !crate::brep::patch_point(
            body,
            attr,
            [point.x * 0.001, point.y * 0.001, point.z * 0.001],
        ) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch curve endpoint is missing".into(),
            ));
        }
    }
    Ok(())
}

fn edit_stream(
    payload: &mut Vec<u8>,
    stream_ordinal: usize,
    edit: impl FnOnce(&mut [u8]) -> Result<(), cadmpeg_ir::codec::CodecError>,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let stream = crate::parasolid::extract_streams(payload)
        .get(stream_ordinal)
        .cloned()
        .ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed("SLDPRT sketch stream is missing".into())
        })?;
    if let Some(start) = payload
        .windows(stream.len())
        .position(|candidate| candidate == stream.as_slice())
    {
        let header = crate::parasolid::stream_header(&stream).ok_or_else(|| {
            cadmpeg_ir::codec::CodecError::Malformed("invalid retained SLDPRT sketch stream".into())
        })?;
        return edit(&mut payload[start + header.body_offset..start + stream.len()]);
    }
    let (start, end) = compressed_member(payload, &stream).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed(
            "compressed retained SLDPRT sketch stream is missing".into(),
        )
    })?;
    let mut inflated = stream;
    let header = crate::parasolid::stream_header(&inflated).ok_or_else(|| {
        cadmpeg_ir::codec::CodecError::Malformed("invalid retained SLDPRT sketch stream".into())
    })?;
    edit(&mut inflated[header.body_offset..])?;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&inflated)?;
    payload.splice(start..end, encoder.finish()?);
    Ok(())
}

fn compressed_member(payload: &[u8], target: &[u8]) -> Option<(usize, usize)> {
    for start in 0..payload.len().saturating_sub(1) {
        if payload[start] != 0x78 || !matches!(payload[start + 1], 0x01 | 0x9c | 0xda) {
            continue;
        }
        let mut decoder = flate2::read::ZlibDecoder::new(&payload[start..]);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_ok() && inflated == target {
            return Some((start, start + decoder.total_in() as usize));
        }
    }
    None
}

fn patch_direct_nurbs(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let SketchGeometry::Nurbs {
        degree,
        ref knots,
        ref control_points,
        ref weights,
        periodic,
    } = request.geometry
    else {
        unreachable!();
    };
    let curve = cadmpeg_ir::geometry::NurbsCurve {
        degree,
        knots: knots.clone(),
        control_points: control_points
            .iter()
            .map(|point| lift_point(*point, request.origin, request.u_axis, request.v_axis))
            .collect(),
        weights: weights.clone(),
        periodic,
    };
    if !crate::brep::patch_nurbs_by_attr(body, request.carrier_attr, &curve) {
        return Err(cadmpeg_ir::codec::CodecError::NotImplemented(
            "SLDPRT sketch NURBS edit changes native storage shape".into(),
        ));
    }
    Ok(())
}

fn patch_direct_ellipse(
    body: &mut [u8],
    request: &CurvePatch,
) -> Result<(), cadmpeg_ir::codec::CodecError> {
    let Some(CurveGeometry::Ellipse { axis, .. }) =
        crate::brep::curve_by_attr(body, request.carrier_attr)
    else {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch analytic carrier is missing".into(),
        ));
    };
    let SketchGeometry::Ellipse {
        center,
        major_angle,
        major_radius,
        minor_radius,
        start_angle,
        end_angle,
    } = request.geometry
    else {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch carrier family changed".into(),
        ));
    };
    let center_3d = lift_point(center, request.origin, request.u_axis, request.v_axis);
    let major_direction = Vector3::new(
        request.u_axis.x * major_angle.0.cos() + request.v_axis.x * major_angle.0.sin(),
        request.u_axis.y * major_angle.0.cos() + request.v_axis.y * major_angle.0.sin(),
        request.u_axis.z * major_angle.0.cos() + request.v_axis.z * major_angle.0.sin(),
    );
    let curve = CurveGeometry::Ellipse {
        center: center_3d,
        axis,
        major_direction,
        major_radius: major_radius.0,
        minor_radius: minor_radius.0,
    };
    let (_, values) = crate::writer::curve_values(&curve, 0.001)?;
    if !crate::brep::patch_compact_values(body, request.carrier_attr, &values) {
        return Err(cadmpeg_ir::codec::CodecError::Malformed(
            "SLDPRT sketch ellipse carrier cannot be patched".into(),
        ));
    }
    let parameters = match (start_angle, end_angle) {
        (Some(start), Some(end)) => [start.0, end.0],
        (None, None) => [0.0, 0.0],
        _ => unreachable!(),
    };
    for (attr, parameter) in [request.start_attr, request.end_attr]
        .into_iter()
        .zip(parameters)
    {
        let local = Point2::new(
            center.u + major_angle.0.cos() * major_radius.0 * parameter.cos()
                - major_angle.0.sin() * minor_radius.0 * parameter.sin(),
            center.v
                + major_angle.0.sin() * major_radius.0 * parameter.cos()
                + major_angle.0.cos() * minor_radius.0 * parameter.sin(),
        );
        let point = lift_point(local, request.origin, request.u_axis, request.v_axis);
        if !crate::brep::patch_point(
            body,
            attr,
            [point.x * 0.001, point.y * 0.001, point.z * 0.001],
        ) {
            return Err(cadmpeg_ir::codec::CodecError::Malformed(
                "SLDPRT sketch ellipse endpoint is missing".into(),
            ));
        }
    }
    Ok(())
}

fn polar(radius: f64, angle: f64) -> Point2 {
    Point2::new(radius * angle.cos(), radius * angle.sin())
}

fn offset_point(origin: Point2, delta: Point2) -> Point2 {
    Point2::new(origin.u + delta.u, origin.v + delta.v)
}
