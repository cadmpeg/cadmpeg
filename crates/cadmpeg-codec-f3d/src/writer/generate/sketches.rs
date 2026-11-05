// SPDX-License-Identifier: Apache-2.0
//! Author legacy sketch containers, points and analytic curve members.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{SketchEntityId, SketchGeometryDefinition, SketchId};

use super::presentation::{GeneratedDesignRegistry, GeneratedDesignType};
use super::records::{encode_design_metastream, native_lp_ascii, native_lp_utf16};

const EPS_FULL_TURN_TO_CIRCLE: f64 = 1.0e-9;

pub(super) struct SketchStream {
    pub(super) bulk: Vec<u8>,
    pub(super) meta: Vec<u8>,
    pub(super) sketches: Vec<SketchId>,
    pub(super) entities: Vec<SketchEntityId>,
}

pub(super) fn encode(target: &CadIr) -> Result<Option<SketchStream>, CodecError> {
    let mut bulk = Vec::new();
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut containers = Vec::new();
    let mut heads = Vec::new();
    let mut points = Vec::new();
    let mut companions = Vec::new();
    let mut lines = Vec::new();
    let mut circles = Vec::new();
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut containers_used = std::collections::BTreeSet::new();
    let mut occupied = target
        .model
        .sketches
        .iter()
        .filter_map(|sketch| {
            let name = sketch.name.as_ref()?;
            let id = crate::records::identity::DesignEntityId::try_from(name.clone()).ok()?;
            u32::try_from(id.suffix()).ok().filter(|id| *id != 0)
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut next = 1u32;
    for sketch in &target.model.sketches {
        let Some(name) = sketch
            .name
            .as_ref()
            .filter(|name| name.encode_utf16().count() <= 256)
        else {
            continue;
        };
        let Some(container) = crate::records::identity::DesignEntityId::try_from(name.clone())
            .ok()
            .and_then(|id| u32::try_from(id.suffix()).ok())
            .filter(|id| *id != 0)
        else {
            continue;
        };
        if containers_used.contains(&container) {
            continue;
        }
        let Some((origin, normal, axis)) = sketch.resolved_placement() else {
            continue;
        };
        let origin = origin.get();
        let normal = normal.get();
        let axis = axis.get();
        let v_axis = normal.cross(axis);
        let frame = [
            [axis.x, v_axis.x, normal.x, origin.x / 10.0],
            [axis.y, v_axis.y, normal.y, origin.y / 10.0],
            [axis.z, v_axis.z, normal.z, origin.z / 10.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        if crate::records::sketch_placement::SketchPlacementMatrix::try_from(frame).is_err()
            || sketch.native_ref.is_some()
            || sketch.configuration.is_some()
            || sketch.visible.is_some()
            || target
                .model
                .sketch_constraints
                .iter()
                .any(|row| row.sketch == sketch.id)
        {
            continue;
        }
        let members = target
            .model
            .sketch_entities
            .iter()
            .filter(|row| row.sketch == sketch.id)
            .collect::<Vec<_>>();
        if members.iter().any(|row| {
            row.construction
                || row.native_ref.is_some()
                || row.geometry_ref.is_some()
                || !row.endpoint_refs.is_empty()
                || match row.geometry.definition() {
                    SketchGeometryDefinition::Point { .. }
                    | SketchGeometryDefinition::Circle { .. } => false,
                    SketchGeometryDefinition::Line { start, end } => {
                        let start = start.get();
                        let end = end.get();
                        let length = (end.u - start.u).hypot(end.v - start.v);
                        !length.is_finite() || length <= f64::EPSILON
                    }
                    SketchGeometryDefinition::Arc {
                        center,
                        radius,
                        start_angle,
                        end_angle,
                    } => {
                        let sweep = (end_angle.get() - start_angle.get()).abs();
                        let center = center.get();
                        !sweep.is_finite()
                            || sweep >= std::f64::consts::TAU - EPS_FULL_TURN_TO_CIRCLE
                            || [start_angle.get(), end_angle.get()]
                                .into_iter()
                                .any(|angle| {
                                    !(center.u + radius.get() * angle.cos()).is_finite()
                                        || !(center.v + radius.get() * angle.sin()).is_finite()
                                })
                    }
                    _ => true,
                }
        }) {
            continue;
        }
        let count = u32::try_from(members.len())
            .map_err(|_| CodecError::InvalidInput("F3D sketch member count exceeds u32".into()))?;
        let Some(head) = allocate(&mut occupied, &mut next) else {
            break;
        };
        let mut member_indices = Vec::new();
        for _ in &members {
            let Some(point) = allocate(&mut occupied, &mut next) else {
                break;
            };
            let Some(companion) = allocate(&mut occupied, &mut next) else {
                break;
            };
            member_indices.push((point, companion));
        }
        if member_indices.len() != members.len() {
            break;
        }
        if !profiles_match(
            sketch,
            &members,
            &member_indices,
            target.tolerances.linear.get(),
        ) {
            continue;
        }
        mark(&mut primary, container, bulk.len())?;
        header(&mut bulk, "256", container)?;
        bulk.extend_from_slice(&0u32.to_le_bytes());
        bulk.extend_from_slice(&[0; 5]);
        bulk.push(0);
        native_lp_utf16(&mut bulk, name)?;
        bulk.extend_from_slice(&u64::MAX.to_le_bytes());
        bulk.push(1);
        bulk.extend_from_slice(&count.to_le_bytes());
        for (point, _) in &member_indices {
            reference(&mut bulk, *point);
        }
        mark(&mut secondary, container, bulk.len())?;
        header(&mut bulk, "257", container)?;
        bulk.extend_from_slice(&[0; 8]);
        reference(&mut bulk, head);
        let ordinal = u32::try_from(sketches.len())
            .ok()
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| CodecError::InvalidInput("F3D sketch ordinal exceeds u32".into()))?;
        bulk.extend_from_slice(&ordinal.to_le_bytes());
        bulk.extend_from_slice(&[0; 7]);
        bulk.extend_from_slice(&count.to_le_bytes());
        for (point, _) in &member_indices {
            reference(&mut bulk, *point);
        }
        mark(&mut primary, head, bulk.len())?;
        header(&mut bulk, "258", head)?;
        bulk.extend_from_slice(&[0; 11]);
        for value in frame.into_iter().flatten() {
            bulk.extend_from_slice(&value.to_le_bytes());
        }
        bulk.extend_from_slice(&[0, 1]);
        bulk.extend_from_slice(&[0; 10]);
        containers.push(u64::from(container));
        heads.push(u64::from(head));
        for (member, (point, companion)) in members.iter().zip(member_indices) {
            let circular = match member.geometry.definition() {
                SketchGeometryDefinition::Circle { center, radius } => {
                    Some((*center, *radius, 0.0, std::f64::consts::TAU))
                }
                SketchGeometryDefinition::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } => Some((*center, *radius, start_angle.get(), end_angle.get())),
                _ => None,
            };
            if let Some((center, radius, start_angle, end_angle)) = circular {
                let center = center.get();
                mark(&mut primary, point, bulk.len())?;
                curve_header(&mut bulk, "262", point)?;
                for value in [
                    center.u / 10.0,
                    center.v / 10.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    0.0,
                    0.0,
                    radius.get() / 10.0,
                    start_angle,
                    end_angle,
                ] {
                    bulk.extend_from_slice(&value.to_le_bytes());
                }
                reference(&mut bulk, container);
                circles.push(u64::from(point));
                entities.push(member.id().clone());
                continue;
            }
            if let SketchGeometryDefinition::Line { start, end } = member.geometry.definition() {
                let start = start.get();
                let end = end.get();
                let dx = end.u - start.u;
                let dy = end.v - start.v;
                let length = dx.hypot(dy);
                mark(&mut primary, point, bulk.len())?;
                curve_header(&mut bulk, "261", point)?;
                for value in [
                    start.u / 10.0,
                    start.v / 10.0,
                    0.0,
                    dx / 10.0,
                    dy / 10.0,
                    0.0,
                    dx / length,
                    dy / length,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                ] {
                    bulk.extend_from_slice(&value.to_le_bytes());
                }
                reference(&mut bulk, container);
                lines.push(u64::from(point));
                entities.push(member.id().clone());
                continue;
            }
            let SketchGeometryDefinition::Point { position } = member.geometry.definition() else {
                continue;
            };
            let position = position.get();
            mark(&mut primary, point, bulk.len())?;
            header(&mut bulk, "259", point)?;
            bulk.extend_from_slice(&[0; 10]);
            reference(&mut bulk, companion);
            bulk.push(0);
            bulk.extend_from_slice(&(position.u / 10.0).to_le_bytes());
            bulk.extend_from_slice(&(position.v / 10.0).to_le_bytes());
            bulk.extend_from_slice(&[0; 20]);
            bulk.extend_from_slice(&1f32.to_le_bytes());
            bulk.extend_from_slice(&[0; 12]);
            bulk.extend_from_slice(&1f32.to_le_bytes());
            bulk.extend_from_slice(&1f32.to_le_bytes());
            bulk.extend_from_slice(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
            reference(&mut bulk, companion);
            reference(&mut bulk, container);
            mark(&mut primary, companion, bulk.len())?;
            header(&mut bulk, "260", companion)?;
            bulk.extend_from_slice(&[0; 10]);
            bulk.extend_from_slice(&0u32.to_le_bytes());
            bulk.push(0);
            reference(&mut bulk, point);
            points.push(u64::from(point));
            companions.push(u64::from(companion));
            entities.push(member.id().clone());
        }
        sketches.push(sketch.id.clone());
        containers_used.insert(container);
    }
    if sketches.is_empty() {
        return Ok(None);
    }
    let types = [
        (
            "44A64366-4BD3-4B24-881A-F94C206E8F2D",
            "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9",
            16,
            "MSketch",
            containers,
        ),
        (
            "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9",
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "Geometry",
            vec![],
        ),
        (
            "F47A46FB-DA27-4AFA-8A72-1B38FA596E23",
            "63D6AD42-466A-40B7-9CCD-EB21BC500EF6",
            0,
            "Geometry",
            heads,
        ),
        (
            "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB",
            "8E9D588C-28EF-43C4-B9A6-AA5430A68D4B",
            0,
            "Geometry",
            points,
        ),
        (
            "362B7EC3-0F09-47C8-A3BE-DC066715CDAE",
            "63D6AD42-466A-40B7-9CCD-EB21BC500EF6",
            0,
            "Geometry",
            companions,
        ),
        (
            "DCA267ED-D615-4934-B64F-AD805E8003E2",
            "9FF71F46-205F-484B-A2B5-A245D0D8FE33",
            2,
            "Geometry",
            lines,
        ),
        (
            "F0130424-8B7E-4092-93C9-1CA807482534",
            "9FF71F46-205F-484B-A2B5-A245D0D8FE33",
            0,
            "Geometry",
            circles,
        ),
    ]
    .into_iter()
    .map(|(guid, base, version, module, entity_ids)| {
        Ok(GeneratedDesignType {
            type_guid: guid.to_owned().try_into().map_err(CodecError::Malformed)?,
            base_type_guid: Some(base.to_owned().try_into().map_err(CodecError::Malformed)?),
            version,
            module: module.into(),
            entity_ids,
        })
    })
    .collect::<Result<Vec<_>, CodecError>>()?;
    let registry = GeneratedDesignRegistry {
        types,
        body_map: None,
        browser_nodes: None,
    };
    let meta = encode_design_metastream(&registry, &primary, &secondary)?
        .ok_or_else(|| CodecError::Malformed("missing generated sketch metadata".into()))?;
    Ok(Some(SketchStream {
        bulk,
        meta,
        sketches,
        entities,
    }))
}

fn mark(
    rows: &mut Vec<crate::metastream::RecordIndexEntry>,
    id: u32,
    offset: usize,
) -> Result<(), CodecError> {
    rows.push(crate::metastream::RecordIndexEntry {
        entity_id: u64::from(id),
        bulk_offset: u64::try_from(offset)
            .map_err(|_| CodecError::InvalidInput("F3D sketch stream exceeds u64".into()))?,
    });
    Ok(())
}
fn header(out: &mut Vec<u8>, tag: &str, id: u32) -> Result<(), CodecError> {
    native_lp_ascii(out, tag)?;
    out.extend_from_slice(&id.to_le_bytes());
    Ok(())
}
fn reference(out: &mut Vec<u8>, id: u32) {
    out.push(1);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}

fn curve_header(out: &mut Vec<u8>, tag: &str, id: u32) -> Result<(), CodecError> {
    header(out, tag, id)?;
    out.extend_from_slice(&[0; 9]);
    out.push(1);
    out.extend_from_slice(&2u32.to_le_bytes());
    for (name, value) in [("crv_primary_id", u64::from(id)), ("crv_secondary_id", 0)] {
        native_lp_ascii(out, name)?;
        native_lp_ascii(out, "IntrinsicMetaTypeuint64")?;
        out.extend_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn allocate(occupied: &mut std::collections::BTreeSet<u32>, next: &mut u32) -> Option<u32> {
    while *next != 0 && occupied.contains(next) {
        *next = next.checked_add(1).unwrap_or(0);
    }
    let id = *next;
    if id == 0 {
        return None;
    }
    occupied.insert(id);
    *next = next.checked_add(1).unwrap_or(0);
    Some(id)
}

fn profiles_match(
    sketch: &cadmpeg_ir::sketches::Sketch,
    members: &[&cadmpeg_ir::sketches::SketchEntity],
    indices: &[(u32, u32)],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchEntityUse;
    let mut endpoints = Vec::new();
    let mut edges = Vec::new();
    let mut profiles = Vec::new();
    for (member, (index, _)) in members.iter().zip(indices) {
        let rank = crate::ids::neutral_sketch_curve_id(&sketch.id, u64::from(*index), 0);
        let pair = match member.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => Some([start.get(), end.get()]),
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let center = center.get();
                let position = |angle: f64| {
                    cadmpeg_ir::math::Point2::new(
                        center.u + radius.get() * angle.cos(),
                        center.v + radius.get() * angle.sin(),
                    )
                };
                Some([position(start_angle.get()), position(end_angle.get())])
            }
            SketchGeometryDefinition::Circle { .. } => {
                profiles.push((
                    rank.clone(),
                    vec![SketchEntityUse {
                        entity: member.id().clone(),
                        reversed: false,
                    }],
                ));
                None
            }
            _ => None,
        };
        if let Some(pair) = pair {
            endpoints.extend(pair);
            edges.push((rank, member.id().clone()));
        }
    }
    let mut parents = (0..endpoints.len()).collect::<Vec<_>>();
    let mut cells = std::collections::HashMap::<(i64, i64), Vec<usize>>::new();
    for (i, point) in endpoints.iter().copied().enumerate() {
        let Some(u) = cadmpeg_core::convert::truncate_f64_to_i64((point.u / tolerance).floor())
        else {
            return sketch.profiles.is_empty();
        };
        let Some(v) = cadmpeg_core::convert::truncate_f64_to_i64((point.v / tolerance).floor())
        else {
            return sketch.profiles.is_empty();
        };
        for du in -1..=1 {
            for dv in -1..=1 {
                let Some(u) = u.checked_add(du) else {
                    continue;
                };
                let Some(v) = v.checked_add(dv) else {
                    continue;
                };
                for &j in cells.get(&(u, v)).into_iter().flatten() {
                    if (point.u - endpoints[j].u).hypot(point.v - endpoints[j].v) <= tolerance {
                        join(&mut parents, i, j);
                    }
                }
            }
        }
        cells.entry((u, v)).or_default().push(i);
    }
    let nodes = (0..edges.len())
        .map(|i| [root(&parents, i * 2), root(&parents, i * 2 + 1)])
        .collect::<Vec<_>>();
    let mut adjacency = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    let mut pairs = std::collections::BTreeSet::new();
    for (edge, [a, b]) in nodes.iter().copied().enumerate() {
        if !pairs.insert((a.min(b), a.max(b))) {
            return false;
        }
        adjacency.entry(a).or_default().push(edge);
        adjacency.entry(b).or_default().push(edge);
    }
    if adjacency.values().any(|incident| incident.len() > 2) {
        return false;
    }
    let mut visited = (0..edges.len()).map(|_| false).collect::<Vec<_>>();
    let mut order = (0..edges.len()).collect::<Vec<_>>();
    order.sort_by(|a, b| edges[*a].0.cmp(&edges[*b].0));
    for first in order {
        if visited[first] {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = vec![first];
        while let Some(edge) = pending.pop() {
            if visited[edge] {
                continue;
            }
            visited[edge] = true;
            component.push(edge);
            for node in nodes[edge] {
                pending.extend(adjacency[&node].iter().copied());
            }
        }
        if component
            .iter()
            .flat_map(|edge| nodes[*edge])
            .any(|node| adjacency[&node].len() != 2)
        {
            continue;
        }
        let start = nodes[first][0];
        let mut current = nodes[first][1];
        let mut profile = vec![SketchEntityUse {
            entity: edges[first].1.clone(),
            reversed: false,
        }];
        let mut used = std::collections::BTreeSet::from([first]);
        while current != start {
            let Some(next) = adjacency[&current]
                .iter()
                .copied()
                .find(|edge| !used.contains(edge))
            else {
                return false;
            };
            let reversed = nodes[next][1] == current;
            current = if reversed {
                nodes[next][0]
            } else {
                nodes[next][1]
            };
            used.insert(next);
            profile.push(SketchEntityUse {
                entity: edges[next].1.clone(),
                reversed,
            });
        }
        if used.len() != component.len() {
            return false;
        }
        profiles.push((edges[first].0.clone(), profile));
    }
    profiles.sort_by(|a, b| a.0.cmp(&b.0));
    sketch.profiles.as_slice()
        == profiles
            .into_iter()
            .map(|(_, profile)| profile)
            .collect::<Vec<_>>()
}

fn root(parents: &[usize], mut index: usize) -> usize {
    while parents[index] != index {
        index = parents[index];
    }
    index
}

fn join(parents: &mut [usize], a: usize, b: usize) {
    let a = root(parents, a);
    let b = root(parents, b);
    parents[a] = b;
}
