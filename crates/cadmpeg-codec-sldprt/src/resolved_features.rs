// SPDX-License-Identifier: Apache-2.0
//! Typed views over `SolidWorks` `ResolvedFeatures` sketch records.

use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{
    FeatureInputClass, FeatureInputClassRole, FeatureInputLane, FeatureInputName,
    FeatureInputOperand, FeatureInputOperandKind, FeatureInputReference,
    FeatureInputRelationBinding, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity, SketchInputKind,
    SketchInputLink,
};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::features::{BooleanOp, FeatureDefinition};
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
                        feature_ref: None,
                        ordinal: ordinal as u32,
                        offset: offset as u64,
                        local_id: marker_local_id(&block.payload, offset),
                        kind: SketchInputKind::from_native_code(code),
                        state_value: marker_state_value(&block.payload, offset),
                        coordinates_m: marker_coordinates(&block.payload, offset),
                        links: Vec::new(),
                        link_selector: None,
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
                "sgCircleDim" => FeatureInputRelationFamily::CircleDiameter,
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
    let relative = if marker_local_links(payload, offset).is_some() {
        88
    } else if marker_coordinates(payload, offset).is_some() {
        let search_start = offset.checked_add(SKETCH_MARKER.len())?;
        let next = payload
            .get(search_start..)?
            .windows(SKETCH_MARKER.len())
            .position(|bytes| bytes == SKETCH_MARKER)?
            .checked_add(search_start)?;
        match next.checked_sub(offset)? {
            142 | 146 => 138,
            152 | 156 => 148,
            162 | 166 | 167 => 158,
            _ => return None,
        }
    } else {
        return None;
    };
    let start = offset.checked_add(relative)?;
    let end = start.checked_add(4)?;
    let id = u32::from_le_bytes(payload.get(start..end)?.try_into().ok()?);
    (id != u32::MAX).then_some(id)
}

fn marker_state_value(payload: &[u8], offset: usize) -> Option<f64> {
    let offset = offset.checked_add(48)?;
    let value = f64::from_le_bytes(payload.get(offset..offset + 8)?.try_into().ok()?);
    value.is_finite().then_some(value)
}

pub(crate) fn marker_coordinates(payload: &[u8], offset: usize) -> Option<[f64; 2]> {
    const GEOMETRY_PREFIX: [u8; 12] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xbf,
    ];
    if payload.get(offset + 5..offset + 17)? != GEOMETRY_PREFIX
        || payload.get(offset + 64..offset + 66)? != [0x1e, 0x00]
    {
        return None;
    }
    let first = f64::from_le_bytes(payload.get(offset + 66..offset + 74)?.try_into().ok()?);
    let second = f64::from_le_bytes(payload.get(offset + 74..offset + 82)?.try_into().ok()?);
    (first.is_finite() && second.is_finite()).then_some([first, second])
}

#[cfg(test)]
mod marker_tests {
    use super::{marker_coordinates, marker_local_id, marker_local_links, unique_marker_candidate};

    #[test]
    fn marker_local_id_is_the_trailing_u32() {
        let mut payload = vec![0; 92];
        payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
        payload[88..92].copy_from_slice(&37u32.to_le_bytes());
        assert_eq!(marker_local_id(&payload, 0), Some(37));
        payload[88..92].fill(0xff);
        assert_eq!(marker_local_id(&payload, 0), None);
    }

    #[test]
    fn coordinate_marker_local_id_uses_the_variant_footer() {
        let mut payload = vec![0; 142 + 5];
        payload[..5].copy_from_slice(super::SKETCH_MARKER);
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[138..142].copy_from_slice(&41u32.to_le_bytes());
        payload[142..147].copy_from_slice(super::SKETCH_MARKER);
        assert_eq!(marker_local_id(&payload, 0), Some(41));
    }

    #[test]
    fn geometry_marker_coordinates_are_selected_by_layout() {
        let mut payload = vec![0; 82];
        payload[5..13].fill(0xff);
        payload[13..17].copy_from_slice(&[0x00, 0x00, 0x80, 0xbf]);
        payload[17..21].copy_from_slice(&10u32.to_le_bytes());
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[66..74].copy_from_slice(&1.25f64.to_le_bytes());
        payload[74..82].copy_from_slice(&(-2.5f64).to_le_bytes());
        assert_eq!(marker_coordinates(&payload, 0), Some([1.25, -2.5]));
        payload[64..66].copy_from_slice(&[0x14, 0x00]);
        assert_eq!(marker_coordinates(&payload, 0), None);
        payload[64..66].copy_from_slice(&[0x1e, 0x00]);
        payload[5] = 0;
        assert_eq!(marker_coordinates(&payload, 0), None);
    }

    #[test]
    fn local_links_require_the_reference_trailer() {
        let mut payload = vec![0; 80];
        payload[64..66].copy_from_slice(&37u16.to_le_bytes());
        payload[66..68].copy_from_slice(&39u16.to_le_bytes());
        payload[68..70].copy_from_slice(&1u16.to_le_bytes());
        payload[72..80].copy_from_slice(&(-1.0f64).to_le_bytes());
        assert_eq!(marker_local_links(&payload, 0), Some(([37, 39], 1)));
        payload[70] = 1;
        assert_eq!(marker_local_links(&payload, 0), None);
        payload[70] = 0;
        payload[72..80].copy_from_slice(&0.0f64.to_le_bytes());
        assert_eq!(marker_local_links(&payload, 0), None);
    }

    #[test]
    fn coordinate_namespace_disambiguates_reused_local_id() {
        let candidates = vec![("relation".into(), false), ("geometry".into(), true)];
        assert_eq!(unique_marker_candidate(&candidates), Some("geometry"));
        let ambiguous = vec![("first".into(), true), ("second".into(), true)];
        assert_eq!(unique_marker_candidate(&ambiguous), None);
    }
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
        for entity in &mut lane.sketch_entities {
            entity.feature_ref = None;
            entity.links.clear();
            entity.link_selector = None;
        }
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
            for entity in lane
                .sketch_entities
                .iter_mut()
                .filter(|entity| entity.offset > start && entity.offset < end)
            {
                entity.feature_ref = Some(feature_id.to_string());
            }
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
                            | FeatureInputOperandKind::E1
                            | FeatureInputOperandKind::Native(
                                0x837b | 0x8386 | 0x83fe | 0x8dcb | 0x8dda | 0xbc7c | 0xbc87
                            )
                    ) {
                        continue;
                    }
                    let mut matches = entities
                        .iter()
                        .filter(|entity| entity.local_id == Some(u32::from(operand.entity_index)))
                        .filter(|entity| operand_accepts_marker(operand.kind, entity.kind));
                    let Some(entity) = matches.next() else {
                        continue;
                    };
                    if matches.next().is_none() {
                        operand.entity_ref = Some(entity.id.clone());
                    }
                }
            }
        }
        let mut marker_ids = HashMap::<(String, u32), Vec<(String, bool)>>::new();
        for entity in &lane.sketch_entities {
            if let (Some(feature), Some(local_id)) = (&entity.feature_ref, entity.local_id) {
                marker_ids
                    .entry((feature.clone(), local_id))
                    .or_default()
                    .push((entity.id.clone(), entity.coordinates_m.is_some()));
            }
        }
        for entity in &mut lane.sketch_entities {
            let Ok(offset) = usize::try_from(entity.offset) else {
                continue;
            };
            let Some((local_ids, selector)) = marker_local_links(&lane.native_payload, offset)
            else {
                continue;
            };
            let Some(owner) = &entity.feature_ref else {
                continue;
            };
            let links = local_ids
                .into_iter()
                .filter_map(|local_id| {
                    let entity_ref = unique_marker_candidate(
                        marker_ids.get(&(owner.clone(), u32::from(local_id)))?,
                    )?;
                    Some(SketchInputLink {
                        local_id,
                        entity_ref: entity_ref.to_string(),
                    })
                })
                .collect::<Vec<_>>();
            if !links.is_empty() {
                entity.links = links;
                entity.link_selector = Some(selector);
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

fn unique_marker_candidate(candidates: &[(String, bool)]) -> Option<&str> {
    let mut coordinate = candidates
        .iter()
        .filter(|(_, coordinate)| *coordinate)
        .map(|(id, _)| id.as_str());
    if let Some(first) = coordinate.next() {
        return coordinate.next().is_none().then_some(first);
    }
    let [(id, _)] = candidates else {
        return None;
    };
    Some(id)
}

fn operand_accepts_marker(kind: FeatureInputOperandKind, marker: SketchInputKind) -> bool {
    match kind {
        FeatureInputOperandKind::Native(0x837b | 0xbc7c) => {
            matches!(
                marker,
                SketchInputKind::Point | SketchInputKind::ConstrainedPoint
            )
        }
        FeatureInputOperandKind::Native(0x8386 | 0x83fe | 0xbc87) => {
            marker == SketchInputKind::LineOrCircle
        }
        _ => true,
    }
}

fn marker_local_links(payload: &[u8], offset: usize) -> Option<([u16; 2], u16)> {
    if payload.get(offset + 70..offset + 72)? != [0, 0]
        || payload.get(offset + 72..offset + 80)? != (-1.0f64).to_le_bytes()
    {
        return None;
    }
    Some((
        [
            u16::from_le_bytes(payload.get(offset + 64..offset + 66)?.try_into().ok()?),
            u16::from_le_bytes(payload.get(offset + 66..offset + 68)?.try_into().ok()?),
        ],
        u16::from_le_bytes(payload.get(offset + 68..offset + 70)?.try_into().ok()?),
    ))
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
        let Some((_, family, class_ref)) = declarations
            .iter()
            .filter(|(offset, family, _)| {
                *offset < scalar.offset && relation_signature(*family, &scalar.operands)
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
    operands: &[FeatureInputOperand],
) -> bool {
    use FeatureInputOperandKind::{Native, D6, E1};
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    if family == CircleDiameter {
        return matches!(operands, [operand] if operand.kind == Native(0x83fe));
    }
    let [first, second] = operands else {
        return false;
    };
    match family {
        PointPointDistance => {
            (first.kind == D6 && second.kind == D6)
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
            first.kind == Native(0x8dcb) && second.kind == Native(0x8dcb)
        }
        Angle => first.kind == Native(0x8dda) && second.kind == Native(0x8dda),
        CircleDiameter => unreachable!("handled as a unary relation"),
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
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        feature.input_class = None;
    }
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

    let mut classes_by_type = HashMap::<String, Vec<String>>::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        if let Some(class) = &feature.input_class {
            classes_by_type
                .entry(feature.kind.clone())
                .or_default()
                .push(class.clone());
        }
    }
    for classes in classes_by_type.values_mut() {
        classes.sort();
        classes.dedup();
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        if let Some([class]) = classes_by_type.get(&feature.kind).map(Vec::as_slice) {
            feature.input_class = Some(class.clone());
        }
    }

    let direct_name_offsets = lanes
        .iter()
        .flat_map(|lane| {
            lane.classes
                .iter()
                .map(|class| (lane.id.as_str(), class.offset + 6 + class.name.len() as u64))
        })
        .collect::<HashSet<_>>();
    let mut classes_by_token = HashMap::<(&str, u16), Vec<String>>::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        let (Some(class), Some(object_id)) = (
            &feature.input_class,
            feature
                .source_id
                .as_deref()
                .and_then(|value| value.parse::<u32>().ok()),
        ) else {
            continue;
        };
        for lane in lanes {
            for name in lane.names.iter().filter(|name| {
                name.object_id == Some(object_id)
                    && !direct_name_offsets.contains(&(lane.id.as_str(), name.offset))
            }) {
                let Ok(offset) = usize::try_from(name.offset) else {
                    continue;
                };
                if let Some(token) = repeated_class_token(&lane.native_payload, offset) {
                    classes_by_token
                        .entry((lane.id.as_str(), token))
                        .or_default()
                        .push(class.clone());
                }
            }
        }
    }
    for classes in classes_by_token.values_mut() {
        classes.sort();
        classes.dedup();
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
        .filter(|feature| feature.input_class.is_none())
    {
        let Some(object_id) = feature
            .source_id
            .as_deref()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let mut candidates = Vec::new();
        for lane in lanes {
            for name in lane.names.iter().filter(|name| {
                name.object_id == Some(object_id)
                    && !direct_name_offsets.contains(&(lane.id.as_str(), name.offset))
            }) {
                let Ok(offset) = usize::try_from(name.offset) else {
                    continue;
                };
                let Some(token) = repeated_class_token(&lane.native_payload, offset) else {
                    continue;
                };
                if let Some([class]) = classes_by_token
                    .get(&(lane.id.as_str(), token))
                    .map(Vec::as_slice)
                {
                    candidates.push(class.clone());
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        if let [class] = candidates.as_slice() {
            feature.input_class = Some(class.clone());
        }
    }
}

fn repeated_class_token(payload: &[u8], name_offset: usize) -> Option<u16> {
    let start = name_offset.checked_sub(2)?;
    Some(u16::from_le_bytes(
        payload.get(start..name_offset)?.try_into().ok()?,
    ))
}

fn feature_operation_code(lane: &FeatureInputLane, name: &FeatureInputName) -> Option<u32> {
    let name_offset = usize::try_from(name.offset).ok()?;
    let direct_class = lane
        .classes
        .iter()
        .find(|class| class.offset + 6 + class.name.len() as u64 == name.offset);
    let code_offset = if let Some(class) = direct_class {
        let class_offset = usize::try_from(class.offset).ok()?;
        [8usize, 4].into_iter().find_map(|padding| {
            let code_offset = class_offset.checked_sub(4 + padding)?;
            lane.native_payload
                .get(code_offset + 4..class_offset)?
                .iter()
                .all(|byte| *byte == 0)
                .then_some(code_offset)
        })?
    } else {
        [8usize, 4].into_iter().find_map(|padding| {
            let code_offset = name_offset.checked_sub(6 + padding)?;
            lane.native_payload
                .get(code_offset + 4..name_offset - 2)?
                .iter()
                .all(|byte| *byte == 0)
                .then_some(code_offset)
        })?
    };
    Some(u32::from_le_bytes(
        lane.native_payload
            .get(code_offset..code_offset + 4)?
            .try_into()
            .ok()?,
    ))
}

/// Project the feature-input operation discriminator onto typed extrusions.
pub(crate) fn bind_extrusion_operations(
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) {
    let history_features = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    for feature in features {
        let FeatureDefinition::Extrude { op, .. } = &mut feature.definition else {
            continue;
        };
        if *op != BooleanOp::Unresolved {
            continue;
        }
        let Some(history) = feature
            .native_ref
            .as_deref()
            .and_then(|native| history_features.get(native).copied())
        else {
            continue;
        };
        let mut operations = lanes.iter().filter_map(|lane| {
            let name = feature_object_name(history, lane)?;
            match (
                history.input_class.as_deref(),
                feature_operation_code(lane, name)?,
            ) {
                (Some("moExtrusion_c"), 1) | (_, 3) => Some(BooleanOp::Join),
                (_, 11) => Some(BooleanOp::Cut),
                _ => None,
            }
        });
        let Some(first) = operations.next() else {
            continue;
        };
        if operations.all(|operation| operation == first) {
            *op = first;
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
                neutral_owners.get(&parameter.owner).copied() == Some(native_feature.id.as_str())
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
    sketch_entities: &[SketchEntity],
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
    let loci_by_marker = profile_loci_by_marker(features, sketch_entities, lanes);
    let markers_by_id = lanes
        .iter()
        .flat_map(|lane| &lane.sketch_entities)
        .map(|marker| (marker.id.as_str(), marker))
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
                FeatureInputRelationFamily::CircleDiameter => "sgCircleDim",
            };
            let mut entities = relation
                .operands
                .iter()
                .filter_map(|operand| operand.entity_ref.as_deref())
                .flat_map(|marker| {
                    marker_entities(marker, &markers_by_id, &loci_by_marker).into_iter()
                })
                .collect::<Vec<_>>();
            entities.sort_by(|left, right| left.0.cmp(&right.0));
            entities.dedup();
            let definition = typed_relation_definition(
                relation,
                parameter.clone(),
                &markers_by_id,
                &loci_by_marker,
            )
            .unwrap_or_else(|| SketchConstraintDefinition::Native {
                native_kind: native_kind.into(),
                entities,
                parameter,
                operands: relation
                    .operands
                    .iter()
                    .map(|operand| SketchNativeOperand {
                        native_kind: operand_kind_name(operand.kind),
                        object_index: u32::from(operand.entity_index),
                        native_ref: operand.entity_ref.clone(),
                    })
                    .collect(),
            });
            constraints.push(SketchConstraint {
                id: SketchConstraintId(format!(
                    "sldprt:model:sketch-constraint#relation:{lane_key}:{}",
                    relation.offset
                )),
                sketch: (*sketch).clone(),
                definition,
                native_ref: Some(relation.id.clone()),
            });
        }
    }
}

fn typed_relation_definition(
    relation: &FeatureInputRelationInstance,
    parameter: Option<cadmpeg_ir::features::ParameterId>,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchConstraintDefinition> {
    use FeatureInputRelationFamily::{
        Angle, CircleDiameter, LineLineDistance, PointLineDistance, PointPointDistance,
        PointPointHorizontalDistance, PointPointVerticalDistance,
    };
    let parameter = parameter?;
    let marker = |index: usize| relation.operands.get(index)?.entity_ref.as_deref();
    match relation.family {
        PointPointDistance => Some(SketchConstraintDefinition::DistanceLoci {
            first: marker_point_locus(marker(0)?, markers_by_id, loci_by_marker)?,
            second: marker_point_locus(marker(1)?, markers_by_id, loci_by_marker)?,
            parameter,
        }),
        PointPointHorizontalDistance => Some(SketchConstraintDefinition::HorizontalDistance {
            first: marker_point_locus(marker(0)?, markers_by_id, loci_by_marker)?,
            second: marker_point_locus(marker(1)?, markers_by_id, loci_by_marker)?,
            parameter,
        }),
        PointPointVerticalDistance => Some(SketchConstraintDefinition::VerticalDistance {
            first: marker_point_locus(marker(0)?, markers_by_id, loci_by_marker)?,
            second: marker_point_locus(marker(1)?, markers_by_id, loci_by_marker)?,
            parameter,
        }),
        PointLineDistance => Some(SketchConstraintDefinition::DistanceLoci {
            first: marker_point_locus(marker(0)?, markers_by_id, loci_by_marker)?,
            second: SketchLocus::Entity(single_marker_entity(
                marker(1)?,
                markers_by_id,
                loci_by_marker,
            )?),
            parameter,
        }),
        LineLineDistance => Some(SketchConstraintDefinition::Distance {
            entities: vec![
                single_marker_entity(marker(0)?, markers_by_id, loci_by_marker)?,
                single_marker_entity(marker(1)?, markers_by_id, loci_by_marker)?,
            ],
            parameter,
        }),
        Angle => Some(SketchConstraintDefinition::Angle {
            first: single_marker_entity(marker(0)?, markers_by_id, loci_by_marker)?,
            second: single_marker_entity(marker(1)?, markers_by_id, loci_by_marker)?,
            parameter,
        }),
        CircleDiameter => Some(SketchConstraintDefinition::Diameter {
            entity: single_marker_entity(marker(0)?, markers_by_id, loci_by_marker)?,
            parameter,
        }),
    }
}

fn marker_point_locus(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchLocus> {
    if let Some(loci) = loci_by_marker.get(marker_id) {
        return loci.first().cloned();
    }
    let marker = markers_by_id.get(marker_id)?;
    let mut linked = marker
        .links
        .iter()
        .filter_map(|link| loci_by_marker.get(&link.entity_ref));
    let loci = linked.next()?;
    if linked.next().is_some() {
        return None;
    }
    loci.first().cloned()
}

fn single_marker_entity(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Option<SketchEntityId> {
    let entities = marker_entities(marker_id, markers_by_id, loci_by_marker);
    let [entity] = entities.as_slice() else {
        return None;
    };
    Some(entity.clone())
}

fn profile_loci_by_marker(
    features: &[cadmpeg_ir::features::Feature],
    sketch_entities: &[SketchEntity],
    lanes: &[FeatureInputLane],
) -> HashMap<String, Vec<SketchLocus>> {
    const NATIVE_TO_IR: f64 = 1000.0;
    const QUANTUM: f64 = 1.0e-8;

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
    let mut profile_loci = HashMap::<&SketchId, Vec<(Point2, SketchLocus)>>::new();
    for entity in sketch_entities {
        for (point, locus) in sketch_entity_loci(entity) {
            profile_loci
                .entry(&entity.sketch)
                .or_default()
                .push((point, locus));
        }
    }
    let mut result = HashMap::new();
    for lane in lanes {
        let mut markers_by_feature = HashMap::<&str, Vec<&SketchInputEntity>>::new();
        for marker in &lane.sketch_entities {
            let Some(feature) = marker.feature_ref.as_deref() else {
                continue;
            };
            if marker.coordinates_m.is_some() && sketches_by_feature.contains_key(feature) {
                markers_by_feature.entry(feature).or_default().push(marker);
            }
        }
        for (feature, markers) in markers_by_feature {
            let Some(sketch) = sketches_by_feature.get(feature) else {
                continue;
            };
            let Some(loci) = profile_loci.get(sketch) else {
                continue;
            };
            let marker_points = markers
                .iter()
                .filter_map(|marker| marker.coordinates_m)
                .map(|[u, v]| quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM))
                .collect::<HashSet<_>>();
            let locus_points = loci
                .iter()
                .map(|(point, _)| quantize(*point, QUANTUM))
                .collect::<HashSet<_>>();
            let Some(transform) = unique_marker_transform(&marker_points, &locus_points) else {
                continue;
            };
            let loci_by_point = loci.iter().fold(
                HashMap::<(i64, i64), Vec<SketchLocus>>::new(),
                |mut by_point, (point, locus)| {
                    by_point
                        .entry(quantize(*point, QUANTUM))
                        .or_default()
                        .push(locus.clone());
                    by_point
                },
            );
            for marker in markers {
                let Some([u, v]) = marker.coordinates_m else {
                    continue;
                };
                let point = quantize(Point2::new(u * NATIVE_TO_IR, v * NATIVE_TO_IR), QUANTUM);
                let Some(translated) = transform.apply(point) else {
                    continue;
                };
                let Some(marker_loci) = loci_by_point.get(&translated) else {
                    continue;
                };
                let mut marker_loci = marker_loci.clone();
                marker_loci.sort_by(|left, right| locus_key(left).cmp(&locus_key(right)));
                marker_loci.dedup();
                result.insert(marker.id.clone(), marker_loci);
            }
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MarkerTransform {
    swap: bool,
    u_sign: i8,
    v_sign: i8,
    translation: (i64, i64),
}

impl MarkerTransform {
    fn apply_axes(self, point: (i64, i64)) -> Option<(i64, i64)> {
        let (u, v) = if self.swap { (point.1, point.0) } else { point };
        Some((
            i64::try_from(i128::from(u) * i128::from(self.u_sign)).ok()?,
            i64::try_from(i128::from(v) * i128::from(self.v_sign)).ok()?,
        ))
    }

    fn apply(self, point: (i64, i64)) -> Option<(i64, i64)> {
        let point = self.apply_axes(point)?;
        Some((
            point.0.checked_add(self.translation.0)?,
            point.1.checked_add(self.translation.1)?,
        ))
    }
}

fn unique_marker_transform(
    marker_points: &HashSet<(i64, i64)>,
    locus_points: &HashSet<(i64, i64)>,
) -> Option<MarkerTransform> {
    let identity = MarkerTransform {
        swap: false,
        u_sign: 1,
        v_sign: 1,
        translation: (0, 0),
    };
    if let Some(transform) = unique_transform_translation(identity, marker_points, locus_points) {
        return Some(transform);
    }
    let mut scored = Vec::new();
    for swap in [false, true] {
        for u_sign in [-1, 1] {
            for v_sign in [-1, 1] {
                if !swap && u_sign == 1 && v_sign == 1 {
                    continue;
                }
                let transform = MarkerTransform {
                    swap,
                    u_sign,
                    v_sign,
                    translation: (0, 0),
                };
                let transformed = marker_points
                    .iter()
                    .filter_map(|point| transform.apply_axes(*point))
                    .collect::<HashSet<_>>();
                let mut translations = HashMap::<(i64, i64), usize>::new();
                for marker in &transformed {
                    for locus in locus_points {
                        let Some(translation) = locus
                            .0
                            .checked_sub(marker.0)
                            .zip(locus.1.checked_sub(marker.1))
                        else {
                            continue;
                        };
                        *translations.entry(translation).or_default() += 1;
                    }
                }
                scored.extend(translations.into_iter().map(|(translation, count)| {
                    (
                        MarkerTransform {
                            translation,
                            ..transform
                        },
                        count,
                    )
                }));
            }
        }
    }
    let maximum = scored
        .iter()
        .map(|(_, count)| *count)
        .max()
        .filter(|count| *count >= 2)?;
    let mut candidates = scored
        .into_iter()
        .filter_map(|(transform, count)| (count == maximum).then_some(transform));
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

fn unique_transform_translation(
    transform: MarkerTransform,
    marker_points: &HashSet<(i64, i64)>,
    locus_points: &HashSet<(i64, i64)>,
) -> Option<MarkerTransform> {
    let transformed = marker_points
        .iter()
        .filter_map(|point| transform.apply_axes(*point))
        .collect::<HashSet<_>>();
    let mut translations = HashMap::<(i64, i64), usize>::new();
    for marker in &transformed {
        for locus in locus_points {
            let Some(translation) = locus
                .0
                .checked_sub(marker.0)
                .zip(locus.1.checked_sub(marker.1))
            else {
                continue;
            };
            *translations.entry(translation).or_default() += 1;
        }
    }
    let maximum = translations
        .values()
        .copied()
        .max()
        .filter(|count| *count >= 2)?;
    let mut candidates = translations
        .into_iter()
        .filter_map(|(translation, count)| (count == maximum).then_some(translation));
    let translation = candidates.next()?;
    candidates.next().is_none().then_some(MarkerTransform {
        translation,
        ..transform
    })
}

fn quantize(point: Point2, quantum: f64) -> (i64, i64) {
    (
        (point.u / quantum).round() as i64,
        (point.v / quantum).round() as i64,
    )
}

fn sketch_entity_loci(entity: &SketchEntity) -> Vec<(Point2, SketchLocus)> {
    let locus = |point, locus| (point, locus);
    match &entity.geometry {
        SketchGeometry::Point { position } => {
            vec![locus(*position, SketchLocus::Entity(entity.id.clone()))]
        }
        SketchGeometry::Line { start, end } => vec![
            locus(*start, SketchLocus::Start(entity.id.clone())),
            locus(*end, SketchLocus::End(entity.id.clone())),
        ],
        SketchGeometry::Circle { center, .. } | SketchGeometry::Ellipse { center, .. } => {
            vec![locus(*center, SketchLocus::Center(entity.id.clone()))]
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => vec![
            locus(*center, SketchLocus::Center(entity.id.clone())),
            locus(
                Point2::new(
                    center.u + radius.0 * start_angle.0.cos(),
                    center.v + radius.0 * start_angle.0.sin(),
                ),
                SketchLocus::Start(entity.id.clone()),
            ),
            locus(
                Point2::new(
                    center.u + radius.0 * end_angle.0.cos(),
                    center.v + radius.0 * end_angle.0.sin(),
                ),
                SketchLocus::End(entity.id.clone()),
            ),
        ],
        SketchGeometry::Nurbs { control_points, .. } if !control_points.is_empty() => vec![
            locus(control_points[0], SketchLocus::Start(entity.id.clone())),
            locus(
                control_points[control_points.len() - 1],
                SketchLocus::End(entity.id.clone()),
            ),
        ],
        SketchGeometry::Nurbs { .. } | SketchGeometry::Native { .. } => Vec::new(),
    }
}

fn locus_key(locus: &SketchLocus) -> (&str, u8) {
    match locus {
        SketchLocus::Entity(entity) => (&entity.0, 0),
        SketchLocus::Start(entity) => (&entity.0, 1),
        SketchLocus::End(entity) => (&entity.0, 2),
        SketchLocus::Center(entity) => (&entity.0, 3),
    }
}

fn locus_entity(locus: &SketchLocus) -> SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity.clone(),
    }
}

fn marker_entities(
    marker_id: &str,
    markers_by_id: &HashMap<&str, &SketchInputEntity>,
    loci_by_marker: &HashMap<String, Vec<SketchLocus>>,
) -> Vec<SketchEntityId> {
    if let Some(loci) = loci_by_marker.get(marker_id) {
        return loci.iter().map(locus_entity).collect();
    }
    let Some(marker) = markers_by_id.get(marker_id) else {
        return Vec::new();
    };
    let mut linked = marker
        .links
        .iter()
        .filter_map(|link| loci_by_marker.get(&link.entity_ref))
        .map(|loci| loci.iter().map(locus_entity).collect::<HashSet<_>>())
        .filter(|entities| !entities.is_empty());
    let Some(mut entities) = linked.next() else {
        return Vec::new();
    };
    for candidates in linked {
        entities.retain(|entity| candidates.contains(entity));
    }
    entities.into_iter().collect()
}

#[cfg(test)]
mod profile_join_tests {
    use super::{
        marker_entities, profile_loci_by_marker, typed_relation_definition, unique_marker_transform,
    };
    use crate::records::{
        FeatureInputLane, FeatureInputOperand, FeatureInputOperandKind, FeatureInputRelationFamily,
        FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchInputLink,
    };
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, ParameterId, SketchSpace};
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};
    use std::collections::{BTreeMap, HashMap};

    fn marker(id: &str, coordinates_m: Option<[f64; 2]>) -> SketchInputEntity {
        SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature-native".into()),
            ordinal: 0,
            offset: 0,
            local_id: None,
            kind: SketchInputKind::Point,
            state_value: None,
            coordinates_m,
            links: Vec::new(),
            link_selector: None,
        }
    }

    #[test]
    fn unique_translation_joins_linked_endpoints_to_one_profile_entity() {
        let sketch = SketchId("sketch".into());
        let first = SketchEntityId("first".into());
        let second = SketchEntityId("second".into());
        let entities = vec![
            SketchEntity {
                id: first.clone(),
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(10.0, 20.0),
                    end: Point2::new(20.0, 20.0),
                },
            },
            SketchEntity {
                id: second,
                sketch: sketch.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: Point2::new(20.0, 20.0),
                    end: Point2::new(20.0, 30.0),
                },
            },
        ];
        let feature = Feature {
            id: FeatureId("feature".into()),
            ordinal: 0,
            name: None,
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Sketch {
                space: SketchSpace::Planar,
                sketch: Some(sketch),
            },
            native_ref: Some("feature-native".into()),
        };
        let mut reference = marker("reference", None);
        reference.links = vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: "marker-a".into(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: "marker-b".into(),
            },
        ];
        reference.link_selector = Some(0);
        let lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![
                marker("marker-a", Some([0.0, 0.0])),
                marker("marker-b", Some([0.01, 0.0])),
                marker("marker-c", Some([0.01, 0.01])),
                reference,
            ],
        };

        let joins = profile_loci_by_marker(&[feature], &entities, std::slice::from_ref(&lane));
        assert!(joins.contains_key("marker-a"));
        assert!(joins.contains_key("marker-b"));
        assert!(joins.contains_key("marker-c"));
        let markers = lane
            .sketch_entities
            .iter()
            .map(|marker| (marker.id.as_str(), marker))
            .collect::<HashMap<_, _>>();
        assert_eq!(marker_entities("reference", &markers, &joins), vec![first]);
        let relation = FeatureInputRelationInstance {
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalar_refs: Vec::new(),
            parameter_scalar_ref: None,
            display_scalar_ref: None,
            operands: ["marker-a", "marker-c"]
                .into_iter()
                .enumerate()
                .map(|(index, marker)| FeatureInputOperand {
                    offset: index as u64,
                    reference_ref: format!("reference-{index}"),
                    kind: FeatureInputOperandKind::D6,
                    entity_index: index as u16,
                    entity_ref: Some(marker.into()),
                })
                .collect(),
        };
        assert!(matches!(
            typed_relation_definition(
                &relation,
                Some(ParameterId("distance".into())),
                &markers,
                &joins,
            ),
            Some(cadmpeg_ir::sketches::SketchConstraintDefinition::DistanceLoci {
                parameter,
                ..
            }) if parameter.0 == "distance"
        ));
    }

    #[test]
    fn unique_axis_swap_maps_marker_coordinates_to_profile_loci() {
        let markers = [(0, 0), (2, 1), (7, 4), (3, 9)].into_iter().collect();
        let loci = [(0, 0), (1, 2), (4, 7), (9, 3)].into_iter().collect();
        let transform = unique_marker_transform(&markers, &loci).expect("unique transform");
        assert!(transform.swap);
        assert_eq!(transform.u_sign, 1);
        assert_eq!(transform.v_sign, 1);
        assert!(markers
            .into_iter()
            .all(|point| loci.contains(&transform.apply(point).unwrap())));
    }

    #[test]
    fn symmetric_axis_swaps_remain_unbound() {
        let markers = [(0, 0), (48, 0), (48, 24), (0, 24)].into_iter().collect();
        let loci = [(0, 0), (24, 0), (24, 48), (0, 48)].into_iter().collect();
        assert_eq!(unique_marker_transform(&markers, &loci), None);
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
                pcurve: None,
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
            coedges: coedge_ids,
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
            pcurve: None,
        });
        ir.model.loops.push(Loop {
            id: loop_id.clone(),
            face: face_id.clone(),
            coedges: vec![coedge_id],
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
        SketchGeometry::Point { .. } | SketchGeometry::Native { .. } => Err(
            cadmpeg_ir::codec::CodecError::NotImplemented(
                "source-less SLDPRT sketch writing does not support point or native-only profile entities".into(),
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
