// SPDX-License-Identifier: Apache-2.0
//! Author legacy sketch containers, points and analytic curve members.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchConstraintId, SketchEntityId, SketchGeometryDefinition,
    SketchId,
};

use super::presentation::{GeneratedDesignRegistry, GeneratedDesignType};
use super::records::{encode_design_metastream, native_lp_ascii, native_lp_utf16};

const EPS_FULL_TURN_TO_CIRCLE: f64 = 1.0e-9;

enum RelationFamily {
    Lines,
    Circular,
    Curves,
    Equal,
}

pub(super) struct SketchStream {
    pub(super) bulk: Vec<u8>,
    pub(super) meta: Vec<u8>,
    pub(super) registry: GeneratedDesignRegistry,
    pub(super) primary: Vec<crate::metastream::RecordIndexEntry>,
    pub(super) secondary: Vec<crate::metastream::RecordIndexEntry>,
    pub(super) occupied: std::collections::BTreeSet<u32>,
    pub(super) next: u32,
    pub(super) feature_scopes: Vec<(u64, cadmpeg_ir::features::FeatureId, u32)>,
    pub(super) feature_containers: std::collections::BTreeMap<cadmpeg_ir::features::FeatureId, u32>,
    pub(super) point_indices: std::collections::BTreeMap<SketchEntityId, u32>,
    pub(super) sketches: Vec<SketchId>,
    pub(super) renamed_sketches: Vec<SketchId>,
    pub(super) entities: Vec<SketchEntityId>,
    pub(super) text_entities: Vec<SketchEntityId>,
    pub(super) nurbs_entities: Vec<SketchEntityId>,
    pub(super) constraints: Vec<SketchConstraintId>,
    /// Point relations whose native kind, operand fields or repeated record
    /// indices are written as their stored mask and members derive them.
    pub(super) normalized_constraints: Vec<SketchConstraintId>,
}

/// Constraint kinds a relation state bit selects, in bit order (spec §3.1
/// sketch relations).
const POINT_RELATION_KINDS: [(u64, &str); 14] = [
    (0x1, "coincident"),
    (0x2, "collinear"),
    (0x4, "concentric"),
    (0x8, "equal_length"),
    (0x10, "parallel"),
    (0x20, "perpendicular"),
    (0x40, "horizontal"),
    (0x80, "vertical"),
    (0x100, "tangent"),
    (0x200, "curvature"),
    (0x400, "symmetry"),
    (0x800, "equal"),
    (0x1000, "midpoint"),
    (0x2000, "polygon"),
];

/// The native kind a reader projects from a four-point relation state: the
/// `+`-joined kinds of its bits. The state carries the parallel or
/// concentric bit and no bit outside the point-relation kinds.
fn point_relation_kind(mask: u64) -> Option<String> {
    let known = POINT_RELATION_KINDS
        .iter()
        .fold(0, |known, (bit, _)| known | bit);
    if mask & !known != 0 || mask & (0x10 | 0x4) == 0 {
        return None;
    }
    Some(
        POINT_RELATION_KINDS
            .iter()
            .filter(|(bit, _)| mask & bit != 0)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join("+"),
    )
}

pub(super) fn encode(
    target: &CadIr,
    native: &crate::native::F3dNative,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<SketchStream>, crate::writer::WriteError> {
    let mut bulk = Vec::new();
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut containers = Vec::new();
    let mut heads = Vec::new();
    let mut points = Vec::new();
    let mut companions = Vec::new();
    let mut lines = Vec::new();
    let mut circles = Vec::new();
    let mut texts = Vec::new();
    let mut text_entities = Vec::new();
    let mut nurbs = Vec::new();
    let mut nurbs_entities = Vec::new();
    let mut relation_ids = Vec::new();
    let mut constraints = Vec::new();
    let mut sketches = Vec::new();
    let mut renamed_sketches = Vec::new();
    let mut normalized_constraints = Vec::new();
    let mut entities = Vec::new();
    let mut feature_scopes = Vec::new();
    let mut feature_containers = std::collections::BTreeMap::new();
    let mut point_indices = std::collections::BTreeMap::new();
    let scope_type_needed = target
        .model
        .sketches
        .iter()
        .any(|sketch| sketch_feature(target, &sketch.id).is_some());
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
    let reserved_scopes = target
        .model
        .sketches
        .iter()
        .filter_map(|sketch| {
            let feature = sketch_feature(target, &sketch.id)?;
            let scope = native.design_parameter_scopes.iter().find(|scope| {
                feature.native_ref.as_deref() == Some(scope.id.as_str())
                    && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Sketch
            })?;
            (scope.record_index != 0 && occupied.insert(scope.record_index))
                .then_some((feature.id.clone(), scope.record_index))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut next = 1u32;
    for sketch in &target.model.sketches {
        let original_container = sketch
            .name
            .as_ref()
            .filter(|name| name.encode_utf16().count() <= 256)
            .and_then(|name| crate::records::identity::DesignEntityId::try_from(name.clone()).ok())
            .and_then(|id| u32::try_from(id.suffix()).ok())
            .filter(|id| *id != 0 && !containers_used.contains(id));
        // A legacy container stores its Design entity ID as the sketch name.
        // A name that is not an entity ID keeps the container its F3D sketch
        // identity names, and the container's generated entity ID is stored.
        let identity_container = original_container
            .is_none()
            .then(|| {
                sketch
                    .id
                    .as_str()
                    .rsplit_once('@')
                    .and_then(|(_, suffix)| suffix.parse::<u32>().ok())
            })
            .flatten()
            .filter(|id| *id != 0 && !containers_used.contains(id) && occupied.insert(*id));
        let Some(container) = original_container
            .or(identity_container)
            .or_else(|| allocate(&mut occupied, &mut next))
        else {
            break;
        };
        let name = if original_container.is_some() {
            sketch.name.clone().unwrap_or_default()
        } else {
            format!("Sketch_{container}")
        };
        let Some((origin, normal, axis)) = sketch.resolved_placement() else {
            continue;
        };
        let origin = origin.get();
        let normal = normal.get();
        let axis = axis.get();
        if [origin.x, origin.y, origin.z].into_iter().any(underflows) {
            continue;
        }
        let v_axis = normal.cross(axis);
        let frame = [
            [axis.x, v_axis.x, normal.x, origin.x / 10.0],
            [axis.y, v_axis.y, normal.y, origin.y / 10.0],
            [axis.z, v_axis.z, normal.z, origin.z / 10.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        if crate::records::sketch_placement::SketchPlacementMatrix::try_from(frame).is_err()
            || sketch.configuration.is_some()
            || sketch.visible.is_some()
        {
            continue;
        }
        let members = target
            .model
            .sketch_entities
            .iter()
            .filter(|row| row.sketch == sketch.id && !unsupported_member(row))
            .collect::<Vec<_>>();
        let mut relations = Vec::new();
        let mut relation_members = Vec::new();
        let mut reserved_members = std::collections::BTreeMap::<usize, u32>::new();
        for relation in target
            .model
            .sketch_constraints
            .iter()
            .filter(|row| row.sketch == sketch.id)
        {
            if relation.name.is_some()
                || relation.driving.is_some()
                || relation.active.is_some()
                || relation.virtual_space.is_some()
                || relation.visible.is_some()
                || relation.orientation.is_some()
                || relation.label_distance.is_some()
                || relation.label_position.is_some()
                || relation.metadata.is_some()
            {
                continue;
            }
            if let SketchConstraintDefinitionInput::Native {
                native_kind,
                native_state: Some(mask),
                native_flags: None,
                native_properties,
                entities,
                parameter: None,
                operands,
            } = relation.definition.kind()
            {
                let Some(kind_name) = point_relation_kind(*mask) else {
                    continue;
                };
                if !native_properties.is_empty() || entities.len() != 4 || operands.len() != 4 {
                    continue;
                }
                let mut normalized = native_kind.as_str() != kind_name;
                let mut requested = reserved_members.clone();
                let mut chosen = std::collections::BTreeMap::<usize, u32>::new();
                let mut indices = Vec::new();
                let mut valid = true;
                for (ordinal, (entity, operand)) in entities.iter().zip(operands).enumerate() {
                    let Some(member) = members.iter().position(|member| member.id() == entity)
                    else {
                        valid = false;
                        break;
                    };
                    if !matches!(
                        members[member].geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    ) || operand.native_ref.is_some()
                    {
                        valid = false;
                        break;
                    }
                    let field = if ordinal < 2 { "member" } else { "return" };
                    normalized |= operand.native_kind.as_str() != "point"
                        || !operand.field.as_ref().is_some_and(|value| {
                            value.name.as_str() == field && value.role.is_none()
                        });
                    // A member's first operand states its record index; a
                    // later operand of the same member names that record.
                    let index = if let Some(index) = chosen.get(&member) {
                        normalized |= operand.object_index != Some(*index);
                        *index
                    } else {
                        let Some(index) = operand.object_index.filter(|index| *index != 0) else {
                            valid = false;
                            break;
                        };
                        if requested
                            .get(&member)
                            .is_some_and(|existing| *existing != index)
                            || requested
                                .iter()
                                .any(|(other, existing)| *other != member && *existing == index)
                            || (occupied.contains(&index) && requested.get(&member) != Some(&index))
                        {
                            valid = false;
                            break;
                        }
                        chosen.insert(member, index);
                        index
                    };
                    requested.insert(member, index);
                    indices.push(member);
                }
                if !valid {
                    continue;
                }
                if normalized {
                    normalized_constraints.push(relation.id.clone());
                }
                for index in requested.values() {
                    occupied.insert(*index);
                }
                reserved_members = requested;
                relations.push(relation);
                relation_members.push((indices[..2].to_vec(), indices[2..].to_vec(), *mask));
                continue;
            }
            let (entities, mask, family): (Vec<_>, u64, RelationFamily) =
                match relation.definition.kind() {
                    SketchConstraintDefinitionInput::Horizontal { entity } => {
                        (vec![entity], 0x40, RelationFamily::Lines)
                    }
                    SketchConstraintDefinitionInput::Vertical { entity } => {
                        (vec![entity], 0x80, RelationFamily::Lines)
                    }
                    SketchConstraintDefinitionInput::Collinear { first, second } => {
                        (vec![first, second], 0x2, RelationFamily::Lines)
                    }
                    SketchConstraintDefinitionInput::Parallel { first, second } => {
                        (vec![first, second], 0x10, RelationFamily::Lines)
                    }
                    SketchConstraintDefinitionInput::Perpendicular { first, second } => {
                        (vec![first, second], 0x20, RelationFamily::Lines)
                    }
                    SketchConstraintDefinitionInput::Equal { first, second } => {
                        (vec![first, second], 0x800, RelationFamily::Equal)
                    }
                    SketchConstraintDefinitionInput::Concentric { first, second } => {
                        (vec![first, second], 0x4, RelationFamily::Circular)
                    }
                    SketchConstraintDefinitionInput::Tangent { first, second } => {
                        (vec![first, second], 0x100, RelationFamily::Curves)
                    }
                    SketchConstraintDefinitionInput::Curvature { first, second } => {
                        (vec![first, second], 0x200, RelationFamily::Curves)
                    }
                    _ => continue,
                };
            let indices = entities
                .iter()
                .map(|entity| members.iter().position(|member| member.id() == *entity))
                .collect::<Option<Vec<_>>>();
            let Some(indices) = indices else {
                continue;
            };
            if indices.len() == 2 && indices[0] == indices[1] {
                continue;
            }
            let shapes = indices
                .iter()
                .map(|index| members[*index].geometry.definition())
                .collect::<Vec<_>>();
            let line = |shape: &SketchGeometryDefinition<_, _, _, _, _, _>| {
                matches!(shape, SketchGeometryDefinition::Line { .. })
            };
            let circular = |shape: &SketchGeometryDefinition<_, _, _, _, _, _>| {
                matches!(
                    shape,
                    SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Arc { .. }
                )
            };
            let supported = match family {
                RelationFamily::Lines => shapes.iter().all(|shape| line(shape)),
                RelationFamily::Circular => shapes.iter().all(|shape| circular(shape)),
                RelationFamily::Curves => shapes.iter().all(|shape| line(shape) || circular(shape)),
                RelationFamily::Equal => {
                    shapes.iter().all(|shape| line(shape))
                        || shapes.iter().all(|shape| circular(shape))
                }
            };
            if !supported {
                continue;
            }
            relations.push(relation);
            relation_members.push((indices.clone(), indices, mask));
        }
        for (ordinal, member) in members.iter().enumerate() {
            if reserved_members.contains_key(&ordinal) {
                continue;
            }
            let index = native
                .sketch_points
                .iter()
                .find(|point| member.native_ref.as_deref() == Some(point.id.as_str()))
                .and_then(crate::records::sketch_geometry::SketchPoint::persistent_id)
                .and_then(|id| u32::try_from(id).ok())
                .filter(|id| *id != 0);
            if let Some(index) = index.filter(|index| occupied.insert(*index)) {
                reserved_members.insert(ordinal, index);
            }
        }
        let relation_indices = relations
            .iter()
            .map(|relation| {
                native
                    .sketch_relations
                    .iter()
                    .find(|record| {
                        relation.native_ref.as_deref() == Some(record.id.as_str())
                            && record.owner_reference == container
                    })
                    .map(|record| record.record_index)
                    .filter(|index| *index != 0 && occupied.insert(*index))
            })
            .collect::<Vec<_>>();
        let count = u32::try_from(members.len()).map_err(|_| {
            crate::writer::WriteError::Unwritable("F3D sketch member count exceeds u32".into())
        })?;
        let Some(head) = allocate(&mut occupied, &mut next) else {
            break;
        };
        let mut member_indices = Vec::new();
        for member in 0..members.len() {
            let Some(point) = reserved_members
                .get(&member)
                .copied()
                .or_else(|| allocate(&mut occupied, &mut next))
            else {
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
        let mut allocated_relations = Vec::new();
        for reserved in relation_indices {
            let Some(index) = reserved.or_else(|| allocate(&mut occupied, &mut next)) else {
                break;
            };
            allocated_relations.push(index);
        }
        if allocated_relations.len() != relations.len() {
            break;
        }
        if !profiles_match(
            sketch,
            &members,
            &member_indices,
            target.tolerances.linear_or_default().get(),
            ctx,
        )? {
            continue;
        }
        mark(ctx, &mut primary, container, bulk.len())?;
        header(&mut bulk, "256", container)?;
        bulk.extend_from_slice(&0u32.to_le_bytes());
        bulk.extend_from_slice(&[0; 5]);
        bulk.push(0);
        native_lp_utf16(&mut bulk, &name)?;
        bulk.extend_from_slice(&u64::MAX.to_le_bytes());
        bulk.push(1);
        bulk.extend_from_slice(&count.to_le_bytes());
        for (point, _) in &member_indices {
            reference(&mut bulk, *point);
        }
        mark(ctx, &mut secondary, container, bulk.len())?;
        header(&mut bulk, "257", container)?;
        bulk.extend_from_slice(&[0; 8]);
        reference(&mut bulk, head);
        let ordinal = u32::try_from(sketches.len())
            .ok()
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| {
                crate::writer::WriteError::Unwritable("F3D sketch ordinal exceeds u32".into())
            })?;
        bulk.extend_from_slice(&ordinal.to_le_bytes());
        bulk.extend_from_slice(&[0; 7]);
        bulk.extend_from_slice(&count.to_le_bytes());
        for (point, _) in &member_indices {
            reference(&mut bulk, *point);
        }
        mark(ctx, &mut primary, head, bulk.len())?;
        header(&mut bulk, "258", head)?;
        bulk.extend_from_slice(&[0; 11]);
        for value in frame.into_iter().flatten() {
            bulk.extend_from_slice(&value.to_le_bytes());
        }
        bulk.extend_from_slice(&[0, 1]);
        bulk.extend_from_slice(&[0; 10]);
        containers.push(u64::from(container));
        heads.push(u64::from(head));
        for (member, &(point, companion)) in members.iter().zip(&member_indices) {
            if matches!(
                member.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            ) {
                point_indices.insert(member.id().clone(), point);
            }
            if let Some(text) = neutral_text(member, point, container) {
                mark(ctx, &mut primary, point, bulk.len())?;
                super::records::encode_sketch_text(&mut bulk, &text)?;
                texts.push(u64::from(point));
                text_entities.push(member.id().clone());
                entities.push(member.id().clone());
                continue;
            }
            if let Some(geometry) = neutral_nurbs(member) {
                mark(ctx, &mut primary, point, bulk.len())?;
                let mut record = Vec::new();
                curve_header(&mut record, "265", point)?;
                let subtype = "266"
                    .to_owned()
                    .try_into()
                    .map_err(crate::writer::WriteError::Unwritable)?;
                super::records::encode_sketch_nurbs(&mut record, None, &subtype, point, &geometry)?;
                curve_tail(&mut record, container, false);
                bulk.extend_from_slice(&record);
                nurbs.push(u64::from(point));
                nurbs_entities.push(member.id().clone());
                entities.push(member.id().clone());
                continue;
            }
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
                mark(ctx, &mut primary, point, bulk.len())?;
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
                curve_tail(&mut bulk, container, member.construction);
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
                mark(ctx, &mut primary, point, bulk.len())?;
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
                curve_tail(&mut bulk, container, member.construction);
                lines.push(u64::from(point));
                entities.push(member.id().clone());
                continue;
            }
            let SketchGeometryDefinition::Point { position } = member.geometry.definition() else {
                continue;
            };
            let position = position.get();
            mark(ctx, &mut primary, point, bulk.len())?;
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
            mark(ctx, &mut primary, companion, bulk.len())?;
            header(&mut bulk, "260", companion)?;
            bulk.extend_from_slice(&[0; 10]);
            bulk.extend_from_slice(&0u32.to_le_bytes());
            bulk.push(0);
            reference(&mut bulk, point);
            points.push(u64::from(point));
            companions.push(u64::from(companion));
            entities.push(member.id().clone());
        }
        let mut relation_ordinals = std::collections::BTreeMap::<u32, u32>::new();
        for ((relation, (member, returned, mask)), index) in relations
            .iter()
            .zip(relation_members)
            .zip(allocated_relations)
        {
            let entities = member
                .into_iter()
                .map(|member| member_indices[member].0)
                .collect::<Vec<_>>();
            let count = u32::try_from(entities.len()).map_err(|_| {
                crate::writer::WriteError::Unwritable("too many F3D relation members".into())
            })?;
            let returned = returned
                .into_iter()
                .map(|member| member_indices[member].0)
                .collect::<Vec<_>>();
            let returned_count = u32::try_from(returned.len()).map_err(|_| {
                crate::writer::WriteError::Unwritable("too many F3D relation return members".into())
            })?;
            mark(ctx, &mut primary, index, bulk.len())?;
            header(&mut bulk, "263", index)?;
            bulk.extend_from_slice(&[0; 8]);
            bulk.push(1);
            bulk.extend_from_slice(&count.to_le_bytes());
            for entity in &entities {
                let ordinal = relation_ordinals.entry(*entity).or_default();
                reference(&mut bulk, *entity);
                bulk.extend_from_slice(&ordinal.to_le_bytes());
                *ordinal = ordinal.checked_add(1).ok_or_else(|| {
                    crate::writer::WriteError::Unwritable(
                        "too many relations on F3D sketch entity".into(),
                    )
                })?;
            }
            bulk.push(0);
            reference(&mut bulk, container);
            bulk.extend_from_slice(&mask.to_le_bytes());
            bulk.extend_from_slice(&returned_count.to_le_bytes());
            for entity in returned {
                reference(&mut bulk, entity);
            }
            bulk.push(0);
            relation_ids.push(u64::from(index));
            constraints.push(relation.id.clone());
        }
        if let Some(feature) = sketch_feature(target, &sketch.id) {
            let Some(scope) = reserved_scopes
                .get(&feature.id)
                .copied()
                .or_else(|| allocate(&mut occupied, &mut next))
            else {
                break;
            };
            let family_ordinal = target
                .model
                .features
                .iter()
                .filter(|candidate| {
                    candidate.ordinal < feature.ordinal
                        && matches!(
                            candidate.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(
                                cadmpeg_ir::features::FeatureOperation::Sketch { .. }
                            )
                        )
                })
                .count()
                .checked_add(1)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    crate::writer::WriteError::Unwritable(
                        "F3D sketch feature ordinal exceeds u32".into(),
                    )
                })?;
            mark(ctx, &mut primary, scope, bulk.len())?;
            header(&mut bulk, "267", scope)?;
            bulk.extend_from_slice(&[0; 10]);
            bulk.extend_from_slice(&1u32.to_le_bytes());
            reference(&mut bulk, container);
            bulk.extend_from_slice(&u32::MAX.to_le_bytes());
            native_lp_utf16(&mut bulk, "Sketch")?;
            let mut tail = [0u8; 78];
            tail[..4].copy_from_slice(&family_ordinal.to_le_bytes());
            tail[31..35].copy_from_slice(&u32::MAX.to_le_bytes());
            bulk.extend_from_slice(&tail);
            mark(ctx, &mut secondary, scope, bulk.len())?;
            header(&mut bulk, "267", scope)?;
            feature_scopes.push((feature.ordinal, feature.id.clone(), scope));
            feature_containers.insert(feature.id.clone(), container);
        }
        sketches.push(sketch.id.clone());
        containers_used.insert(container);
        if original_container.is_none() {
            renamed_sketches.push(sketch.id.clone());
        }
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
        (
            "60403D47-0C49-49B0-BDE8-1679608164A2",
            "98542EB9-A4F2-4137-A808-DBB5B3CD6159",
            3,
            "MSketch",
            relation_ids,
        ),
    ]
    .into_iter()
    .chain(
        (!texts.is_empty() || !nurbs.is_empty() || scope_type_needed).then_some((
            "E0618268-3A06-450E-9E94-7CF4C2E66802",
            "8E9D588C-28EF-43C4-B9A6-AA5430A68D4B",
            0,
            "Geometry",
            texts,
        )),
    )
    .chain((!nurbs.is_empty() || scope_type_needed).then_some((
        "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B",
        "EE518F9E-9CC3-4424-9CAB-2B92426E5312",
        3,
        "MSketch",
        nurbs,
    )))
    .chain(
        (!nurbs_entities.is_empty() || scope_type_needed).then_some((
            "EE518F9E-9CC3-4424-9CAB-2B92426E5312",
            "9FF71F46-205F-484B-A2B5-A245D0D8FE33",
            4,
            "Geometry",
            vec![],
        )),
    )
    .chain(
        scope_type_needed.then_some((
            "8DA771B7-52ED-42FC-94E2-2353FE141373",
            "2EDC2536-D95B-4469-8ACA-DC07124D9C43",
            0,
            "Fusion",
            feature_scopes
                .iter()
                .map(|(_, _, index)| u64::from(*index))
                .collect(),
        )),
    )
    .map(|(guid, base, version, module, entity_ids)| {
        Ok(GeneratedDesignType {
            type_guid: guid
                .to_owned()
                .try_into()
                .map_err(crate::writer::WriteError::Unwritable)?,
            base_type_guid: Some(
                base.to_owned()
                    .try_into()
                    .map_err(crate::writer::WriteError::Unwritable)?,
            ),
            version,
            module: module.into(),
            entity_ids,
        })
    })
    .collect::<Result<Vec<_>, crate::writer::WriteError>>()?;
    let registry = GeneratedDesignRegistry {
        types,
        body_map: None,
        browser_nodes: None,
    };
    let meta = encode_design_metastream(&registry, &primary, &secondary)?;
    debug_assert!(meta.is_some(), "generated sketch registry has fixed types");
    let Some(meta) = meta else {
        return Ok(None);
    };
    Ok(Some(SketchStream {
        bulk,
        meta,
        registry,
        primary,
        secondary,
        occupied,
        next,
        feature_scopes,
        feature_containers,
        point_indices,
        sketches,
        renamed_sketches,
        entities,
        text_entities,
        nurbs_entities,
        constraints,
        normalized_constraints,
    }))
}

fn sketch_feature<'a>(
    target: &'a CadIr,
    sketch: &SketchId,
) -> Option<&'a cadmpeg_ir::features::Feature> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SketchFeatureBinding};
    let mut matching = target.model.features.iter().filter(|feature| {
        matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(id)),
        }) if id == sketch)
    });
    let (Some(feature), None) = (matching.next(), matching.next()) else {
        return None;
    };
    (feature.suppressed != Some(true)
        && feature.dependencies.is_empty()
        && feature.source_properties.is_empty()
        && feature.source_text.is_none()
        && feature.source_content.is_empty()
        && feature
            .source_tag
            .as_deref()
            .is_none_or(|tag| tag == "Sketch")
        && feature.product_definition.is_none()
        && feature.evaluation.outputs().is_empty())
    .then_some(feature)
}

fn underflows(value: f64) -> bool {
    value != 0.0 && value / 10.0 == 0.0
}

pub(in crate::writer) fn neutral_nurbs(
    row: &cadmpeg_ir::sketches::SketchEntity,
) -> Option<crate::records::sketch_geometry::SketchNurbsGeometry> {
    use crate::records::sketch_geometry::{SketchNurbsGeometry, SketchNurbsPoles};
    let SketchGeometryDefinition::Nurbs { curve } = row.geometry.definition() else {
        return None;
    };
    if row.construction
        || curve.periodic()
        || curve.knots().len() > 100_000
        || curve.pole_rows().count() > 100_000
    {
        return None;
    }
    let points = curve
        .control_points()
        .into_iter()
        .map(|point| {
            let point = point.get();
            (!underflows(point.u) && !underflows(point.v))
                .then_some(cadmpeg_ir::math::Point3::new(point.u, point.v, 0.0))
        })
        .collect::<Option<Vec<_>>>()?;
    let weights = curve.pole_rows().weights().unwrap_or_default();
    let poles = SketchNurbsPoles::from_wire(points, weights).ok()?;
    SketchNurbsGeometry::from_parts(
        curve.degree(),
        0.0,
        8,
        curve.knots().as_slice().to_vec(),
        poles,
    )
    .ok()
}

pub(in crate::writer) fn neutral_text(
    row: &cadmpeg_ir::sketches::SketchEntity,
    record_index: u32,
    owner_reference: u32,
) -> Option<crate::records::sketch_geometry::SketchText> {
    use crate::records::sketch_geometry::{SketchText, SketchTextAlignment, SketchTextLayout};
    use cadmpeg_ir::sketches::{
        SketchTextHorizontalAlignment as H, SketchTextVerticalAlignment as V,
    };
    if row.construction {
        return None;
    }
    let SketchGeometryDefinition::Text {
        text,
        font_family,
        font_weight,
        height,
        width_factor,
        placement: Some(placement),
        horizontal_alignment: Some(horizontal),
        vertical_alignment: Some(vertical),
    } = row.geometry.definition()
    else {
        return None;
    };
    let horizontal = match horizontal {
        H::Left => 1,
        H::Right => 2,
        H::Center => 3,
        H::Native(value) if !matches!(*value, 1..=3) => *value,
        H::Native(_) => return None,
    };
    let vertical = match vertical {
        V::Top => 1,
        V::Bottom => 2,
        V::Middle => 3,
        V::Native(value) if !matches!(*value, 1..=3) => *value,
        V::Native(_) => return None,
    };
    let text = SketchText {
        id: format!("f3d:generated:sketch-text#{record_index}"),
        record_index,
        owner_reference,
        class_tag: "264".to_owned().try_into().ok()?,
        class_version: 0,
        byte_offset: None,
        entity_genesis: None,
        persistent_id: Some(u64::from(record_index)),
        base_id: None,
        text: text.as_str().to_owned(),
        font_family: font_family.as_str().to_owned(),
        font_weight: i32::from(*font_weight),
        height: *height,
        color: cadmpeg_ir::topology::Color::new(0.0, 0.0, 0.0, 0.0)?,
        layout: SketchTextLayout::TextexTag {
            width_factor: cadmpeg_ir::scalar::NonNegativeReal::new(
                width_factor.map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
            )?,
            alignment: Some(SketchTextAlignment {
                horizontal,
                vertical,
            }),
            first_reference: None,
            second_reference: None,
            placement: Some(*placement),
        },
        raw_bytes: Vec::new().into(),
    };
    super::records::sketch_text_supported(&text).then_some(text)
}

fn unsupported_member(row: &cadmpeg_ir::sketches::SketchEntity) -> bool {
    (row.construction
        && matches!(
            row.geometry.definition(),
            SketchGeometryDefinition::Point { .. }
        ))
        || row.geometry_ref.is_some()
        || !row.endpoint_refs.is_empty()
        || match row.geometry.definition() {
            SketchGeometryDefinition::Point { position } => {
                let position = position.get();
                underflows(position.u) || underflows(position.v)
            }
            SketchGeometryDefinition::Circle { center, radius } => {
                let center = center.get();
                underflows(center.u) || underflows(center.v) || underflows(radius.get())
            }
            SketchGeometryDefinition::Line { start, end } => {
                let start = start.get();
                let end = end.get();
                let length = (end.u - start.u).hypot(end.v - start.v);
                !length.is_finite()
                    || length <= f64::EPSILON
                    || [
                        start.u,
                        start.v,
                        end.u,
                        end.v,
                        end.u - start.u,
                        end.v - start.v,
                    ]
                    .into_iter()
                    .any(underflows)
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
                    || underflows(center.u)
                    || underflows(center.v)
                    || underflows(radius.get())
                    || sweep >= std::f64::consts::TAU - EPS_FULL_TURN_TO_CIRCLE
                    || [start_angle.get(), end_angle.get()]
                        .into_iter()
                        .any(|angle| {
                            !(center.u + radius.get() * angle.cos()).is_finite()
                                || !(center.v + radius.get() * angle.sin()).is_finite()
                        })
            }
            SketchGeometryDefinition::Text { .. } => neutral_text(row, 1, 1).is_none(),
            SketchGeometryDefinition::Nurbs { .. } => neutral_nurbs(row).is_none(),
            _ => true,
        }
}

fn mark(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut Vec<crate::metastream::RecordIndexEntry>,
    id: u32,
    offset: usize,
) -> Result<(), crate::writer::WriteError> {
    ctx.charge_entities(1, "F3D sketch record")?;
    rows.push(crate::metastream::RecordIndexEntry {
        entity_id: u64::from(id),
        bulk_offset: cadmpeg_core::decode::u64_from_index(offset),
    });
    Ok(())
}
fn header(out: &mut Vec<u8>, tag: &str, id: u32) -> Result<(), crate::writer::WriteError> {
    native_lp_ascii(out, tag)?;
    out.extend_from_slice(&id.to_le_bytes());
    Ok(())
}
fn reference(out: &mut Vec<u8>, id: u32) {
    out.push(1);
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}

fn curve_tail(out: &mut Vec<u8>, owner: u32, construction: bool) {
    reference(out, owner);
    if construction {
        out.extend_from_slice(&[0; 9]);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.push(1);
        reference(out, owner);
    }
}

fn curve_header(out: &mut Vec<u8>, tag: &str, id: u32) -> Result<(), crate::writer::WriteError> {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<bool, crate::writer::WriteError> {
    use cadmpeg_ir::sketches::SketchEntityUse;
    let mut endpoints = Vec::new();
    let mut edges = Vec::new();
    let mut profiles = Vec::new();
    for (member, (index, _)) in members.iter().zip(indices) {
        if member.construction {
            continue;
        }
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
            SketchGeometryDefinition::Nurbs { curve } => {
                let Some(pair) = nurbs_endpoints(curve, ctx)? else {
                    return Ok(false);
                };
                Some(pair)
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
            return Ok(sketch.profiles.is_empty());
        };
        let Some(v) = cadmpeg_core::convert::truncate_f64_to_i64((point.v / tolerance).floor())
        else {
            return Ok(sketch.profiles.is_empty());
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
            return Ok(false);
        }
        adjacency.entry(a).or_default().push(edge);
        adjacency.entry(b).or_default().push(edge);
    }
    if adjacency.values().any(|incident| incident.len() > 2) {
        return Ok(false);
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
                return Ok(false);
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
            return Ok(false);
        }
        profiles.push((edges[first].0.clone(), profile));
    }
    profiles.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(sketch.profiles.as_slice()
        == profiles
            .into_iter()
            .map(|(_, profile)| profile)
            .collect::<Vec<_>>())
}

fn nurbs_endpoints(
    curve: &cadmpeg_ir::geometry::pcurve::PcurveNurbs,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<[cadmpeg_ir::math::Point2; 2]>, crate::writer::WriteError> {
    use cadmpeg_ir::geometry::pcurve::evaluator::PcurveEvaluatorLanes;
    let lanes = PcurveEvaluatorLanes::new(curve.pole_rows());
    let Ok(degree) = usize::try_from(curve.degree()) else {
        return Ok(None);
    };
    let knots = curve.knots().as_slice();
    let (Some(&start), Some(&end)) = (knots.get(degree), knots.get(lanes.points().len())) else {
        return Ok(None);
    };
    let eval = |t| {
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
            ctx,
            curve.degree(),
            knots,
            lanes.points(),
            lanes.weights(),
            t,
        ))
    };
    let Some(start) = eval(start)? else {
        return Ok(None);
    };
    let Some(end) = eval(end)? else {
        return Ok(None);
    };
    Ok(Some([start.get(), end.get()]))
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

#[cfg(test)]
mod tests;
