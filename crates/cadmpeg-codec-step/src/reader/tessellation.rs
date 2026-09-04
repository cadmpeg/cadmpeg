// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation decoding.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::SourceObjectAssociation;

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
) -> StageOutcome<()> {
    let coordinates = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| {
            if !has_entity(record, "COORDINATES_LIST") {
                return None;
            }
            let scale = geometry
                .length_scales
                .get(&id)
                .copied()
                .unwrap_or(geometry.length_scale);
            super::geometry::coordinate_rows(record, scale).map(|vertices| (id, vertices))
        })
        .collect::<BTreeMap<_, _>>();
    let mut typed = HashSet::new();
    let mut warnings = Vec::new();
    let mut losses = Vec::new();
    let mut item_bodies = BTreeMap::<u64, BTreeSet<BodyId>>::new();
    let mut item_placements = BTreeMap::<u64, Vec<Transform>>::new();
    let mut unresolved_placements = BTreeSet::new();
    let mut declared_items = BTreeSet::new();
    let mut unresolved_containers = BTreeSet::new();
    let mut body_context_items = BTreeSet::new();
    for (&id, record) in &exchange.records {
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        let Some(items) = entity_parameter(record, kind, 0, 1).and_then(ValueExt::list) else {
            warnings.push(format!("{kind} #{id} has no structured items"));
            continue;
        };
        let item_ids = items
            .iter()
            .filter_map(ValueExt::reference)
            .collect::<Vec<_>>();
        declared_items.extend(item_ids.iter().copied());
        let candidates = linked_bodies(record, kind, topology);
        if candidates.is_empty() {
            unresolved_containers.insert(id);
        }
        let body_candidates = candidates.iter().cloned().collect::<Vec<_>>();
        let mut associator = TessellationItemAssociator {
            bodies: &body_candidates,
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            detached_annotation: false,
            declare_containers: true,
            claim_containers: true,
            active: BTreeSet::new(),
        };
        for item in item_ids {
            associator.visit(item, 0, None);
        }
        typed.insert(id);
    }
    let mut representation_cache = BTreeMap::new();
    let product_representations = product_linked_representations(exchange);
    let product_representation_items = product_representations
        .iter()
        .filter_map(|id| exchange.records.get(id))
        .filter_map(super::representation::items)
        .flatten()
        .collect::<BTreeSet<_>>();
    for (&id, record) in &exchange.records {
        if !is_tessellated_shape_representation(record) {
            continue;
        }
        let Some(items) = super::representation::items(record) else {
            continue;
        };
        let bodies = super::topology::representation_bodies(
            id,
            exchange,
            topology,
            &mut representation_cache,
            &mut BTreeSet::new(),
            0,
            None,
        );
        let product_linked = product_representations.contains(&id)
            || items
                .iter()
                .any(|item| product_representation_items.contains(item));
        if bodies.is_empty() && !product_linked {
            continue;
        }
        let mut associator = TessellationItemAssociator {
            bodies: &bodies,
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            detached_annotation: false,
            declare_containers: false,
            claim_containers: true,
            active: BTreeSet::new(),
        };
        for item in items {
            associator.visit(item, 0, None);
        }
    }
    for (&id, record) in &exchange.records {
        if !has_entity(record, "TESSELLATED_ANNOTATION_OCCURRENCE") {
            continue;
        }
        let Some(item) = tessellated_annotation_item(record) else {
            warnings.push(format!(
                "TESSELLATED_ANNOTATION_OCCURRENCE #{id} has no tessellated item"
            ));
            continue;
        };
        let mut associator = TessellationItemAssociator {
            bodies: &[],
            exchange,
            item_bodies: &mut item_bodies,
            declared_items: &mut declared_items,
            unresolved_containers: &mut unresolved_containers,
            typed: &mut typed,
            geometry,
            placements: &mut item_placements,
            unresolved_placements: &mut unresolved_placements,
            body_context_items: &mut body_context_items,
            detached_annotation: true,
            declare_containers: false,
            claim_containers: false,
            active: BTreeSet::new(),
        };
        associator.visit(item, 0, None);
    }
    for id in unresolved_placements {
        let message = format!(
            "repositioned tessellated item #{id} has no valid AXIS2_PLACEMENT_3D; unresolved placement is not applied"
        );
        warnings.push(message.clone());
        losses.push(StepLossCode::TessellationPlacementUnresolved.note(message));
    }
    for id in unresolved_containers {
        let Some(record) = exchange.records.get(&id) else {
            continue;
        };
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        warnings.push(format!("{kind} #{id} has no decoded exact body link"));
    }
    let unresolved_items = item_bodies
        .iter()
        .filter_map(|(&item, bodies)| bodies.is_empty().then_some(item))
        .collect::<BTreeSet<_>>();
    for (&item, placements) in &item_placements {
        if !item_bodies.get(&item).is_some_and(BTreeSet::is_empty) {
            continue;
        }
        let distinct = distinct_placements(placements);
        if distinct.len() > 1 {
            let message = format!(
                "tessellation item #{item} has {} distinct repositioning placements; mesh retained in source coordinates",
                distinct.len()
            );
            warnings.push(message.clone());
            losses.push(StepLossCode::TessellationPlacementAmbiguous.note(message));
        }
    }
    for (&item, bodies) in &item_bodies {
        if body_context_items.contains(&item) && bodies.len() != 1 {
            let detail = if bodies.is_empty() {
                "no decoded body"
            } else if bodies.len() > 1 {
                "multiple candidate bodies"
            } else {
                "an unresolved container association"
            };
            let message =
                format!("tessellation item #{item} has {detail}; mesh retained as detached");
            warnings.push(message.clone());
            losses.push(StepLossCode::TessellationItemBodyUnresolved.note(message));
        }
    }
    for (&id, record) in &exchange.records {
        let Some(kind) = entity_kind(
            record,
            &[
                "TRIANGULATED_FACE",
                "COMPLEX_TRIANGULATED_FACE",
                "TRIANGULATED_SURFACE_SET",
                "COMPLEX_TRIANGULATED_SURFACE_SET",
            ],
        ) else {
            continue;
        };
        let base_kind = if matches!(kind, "TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_FACE") {
            "TESSELLATED_FACE"
        } else {
            "TESSELLATED_SURFACE_SET"
        };
        let Some(coordinate_id) =
            inherited_parameter(record, base_kind, 0).and_then(ValueExt::reference)
        else {
            warnings.push(format!("{kind} #{id} has no COORDINATES_LIST reference"));
            continue;
        };
        let Some(vertices) = coordinates.get(&coordinate_id) else {
            warnings.push(format!("{kind} #{id} has no resolved COORDINATES_LIST"));
            continue;
        };
        let (triangles, strip_lengths) = match kind {
            "TRIANGULATED_FACE" | "TRIANGULATED_SURFACE_SET" => (
                entity_parameter(record, kind, 1, own_parameter_offset(kind))
                    .and_then(triangle_rows),
                Vec::new(),
            ),
            "COMPLEX_TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_SURFACE_SET" => complex_triangles(
                entity_parameter(record, kind, 1, own_parameter_offset(kind)),
                entity_parameter(record, kind, 2, own_parameter_offset(kind)),
            ),
            _ => unreachable!("tessellation kind was checked above"),
        };
        let Some(triangles) = triangles.filter(|triangles| !triangles.is_empty()) else {
            warnings.push(format!("{kind} #{id} has no triangle indices"));
            continue;
        };
        let pnindex = match entity_parameter(record, kind, 0, own_parameter_offset(kind)) {
            None | Some(Value::Omitted) => Vec::new(),
            Some(value) => {
                let Some(indices) = index_list(Some(value)) else {
                    warnings.push(format!("{kind} #{id} has an invalid pnindex"));
                    continue;
                };
                indices
            }
        };
        let (mut local_vertices, local_triangles, coordinate_indices) = if pnindex.is_empty() {
            if triangles
                .iter()
                .flatten()
                .any(|index| *index == 0 || *index as usize > vertices.len())
            {
                warnings.push(format!(
                    "{kind} #{id} has an out-of-range one-based coordinate index"
                ));
                continue;
            }
            let coordinate_indices = triangles.iter().flatten().copied().collect::<BTreeSet<_>>();
            let local_index = coordinate_indices
                .iter()
                .enumerate()
                .map(|(local, global)| (*global, local as u32))
                .collect::<BTreeMap<_, _>>();
            let local_vertices = coordinate_indices
                .iter()
                .map(|index| vertices[*index as usize - 1])
                .collect::<Vec<_>>();
            let local_triangles = triangles
                .iter()
                .map(|triangle| triangle.map(|index| local_index[&index]))
                .collect::<Vec<_>>();
            (local_vertices, local_triangles, Some(coordinate_indices))
        } else {
            if pnindex
                .iter()
                .any(|index| *index == 0 || *index as usize > vertices.len())
                || triangles
                    .iter()
                    .flatten()
                    .any(|index| *index == 0 || *index as usize > pnindex.len())
            {
                warnings.push(format!(
                    "{kind} #{id} has an out-of-range one-based tessellation index"
                ));
                continue;
            }
            (
                pnindex
                    .iter()
                    .map(|index| vertices[*index as usize - 1])
                    .collect(),
                triangles
                    .iter()
                    .map(|triangle| triangle.map(|index| index - 1))
                    .collect(),
                None,
            )
        };
        let source_normals =
            normal_rows(inherited_parameter(record, base_kind, 2)).unwrap_or_default();
        let mut normals = match source_normals.len() {
            0 => Vec::new(),
            1 => match alloc_filled(
                local_vertices.len(),
                source_normals[0],
                "STEP tessellation normal rows",
            ) {
                Ok(normals) => normals,
                Err(error) => {
                    warnings.push(format!(
                        "{kind} #{id} normal-row allocation refused: {error}"
                    ));
                    Vec::new()
                }
            },
            count if count == local_vertices.len() => source_normals,
            count if pnindex.is_empty() && count == vertices.len() => coordinate_indices
                .expect("coordinate indices exist without pnindex")
                .iter()
                .map(|index| source_normals[*index as usize - 1])
                .collect(),
            count => {
                warnings.push(format!(
                    "{kind} #{id} carries {count} normals for {} coordinates",
                    local_vertices.len()
                ));
                Vec::new()
            }
        };
        if item_bodies.get(&id).is_some_and(BTreeSet::is_empty) {
            let distinct = distinct_placements(item_placements.get(&id).map_or(&[], Vec::as_slice));
            if let [placement] = distinct.as_slice() {
                local_vertices = local_vertices
                    .into_iter()
                    .map(|vertex| placement.apply_point(vertex))
                    .collect();
                normals = normals
                    .into_iter()
                    .map(|normal| placement.apply_normal(normal).unwrap_or(normal))
                    .collect();
            }
        }
        if let Some(surface_step) = complex_triangulated_face_surface(record) {
            let surface_id = StepIdentity::data("surface", surface_step);
            if let Some(surface) = ir
                .model
                .surfaces
                .iter_mut()
                .find(|surface| surface.id.0 == surface_id)
            {
                surface
                    .source_object
                    .get_or_insert_with(|| SourceObjectAssociation {
                        format: crate::dialect::FORMAT.into(),
                        object_id: format!("#{id}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    });
            }
        }
        if !declared_items.contains(&id) {
            let message = format!(
                "tessellation item #{id} is not declared by an exact body container; mesh retained as detached"
            );
            warnings.push(message.clone());
            losses.push(StepLossCode::TessellationItemUndeclared.note(message));
        }
        ir.model.tessellations.push(
            Tessellation::from_decoded(
                StepIdentity::tessellation("mesh", id),
                local_vertices,
                local_triangles,
                strip_lengths,
                normals,
                Vec::new(),
                Vec::new(),
            )
            .expect("decoded STEP tessellation is valid")
            .with_body(
                (!unresolved_items.contains(&id))
                    .then(|| item_bodies.get(&id))
                    .flatten()
                    .filter(|bodies| bodies.len() == 1)
                    .and_then(|bodies| bodies.iter().next().cloned()),
            )
            .with_source_object(
                (!declared_items.contains(&id)
                    || unresolved_items.contains(&id)
                    || item_bodies.get(&id).is_none_or(|bodies| bodies.len() != 1))
                .then(|| SourceObjectAssociation {
                    format: crate::dialect::FORMAT.into(),
                    object_id: format!("#{id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            ),
        );
        typed.extend([id, coordinate_id]);
    }
    if !ir.model.tessellations.is_empty() {
        for (&id, record) in &exchange.records {
            if has_entity(record, "TESSELLATED_SHAPE_REPRESENTATION")
                || has_entity(record, "TESSELLATED_SOLID")
                || has_entity(record, "TESSELLATED_SHELL")
            {
                typed.insert(id);
            }
        }
    }
    StageOutcome {
        value: (),
        claims: typed,
        warnings,
        losses,
        notes: Vec::new(),
    }
}

fn complex_triangulated_face_surface(record: &RawRecord) -> Option<u64> {
    has_entity(record, "COMPLEX_TRIANGULATED_FACE")
        .then(|| inherited_parameter(record, "TESSELLATED_FACE", 3))
        .flatten()
        .and_then(ValueExt::reference)
}

struct TessellationItemAssociator<'a> {
    bodies: &'a [BodyId],
    exchange: &'a Exchange,
    item_bodies: &'a mut BTreeMap<u64, BTreeSet<BodyId>>,
    declared_items: &'a mut BTreeSet<u64>,
    unresolved_containers: &'a mut BTreeSet<u64>,
    typed: &'a mut HashSet<u64>,
    geometry: &'a GeometryData,
    placements: &'a mut BTreeMap<u64, Vec<Transform>>,
    unresolved_placements: &'a mut BTreeSet<u64>,
    body_context_items: &'a mut BTreeSet<u64>,
    detached_annotation: bool,
    declare_containers: bool,
    claim_containers: bool,
    active: BTreeSet<u64>,
}

impl TessellationItemAssociator<'_> {
    fn visit(&mut self, id: u64, depth: usize, inherited_placement: Option<Transform>) {
        if depth >= super::record_graph_limit(None) || !self.active.insert(id) {
            return;
        }
        let Some(record) = self.exchange.records.get(&id) else {
            self.active.remove(&id);
            return;
        };
        let local_placement = if has_entity(record, "REPOSITIONED_TESSELLATED_ITEM") {
            let placement = repositioned_placement(record, self.geometry);
            if placement.is_none() {
                self.unresolved_placements.insert(id);
            }
            placement
        } else {
            None
        };
        let placement = local_placement.map_or(inherited_placement, |local| {
            Some(inherited_placement.map_or(local, |parent| parent.compose(local)))
        });
        if entity_kind(
            record,
            &[
                "TRIANGULATED_FACE",
                "COMPLEX_TRIANGULATED_FACE",
                "TRIANGULATED_SURFACE_SET",
                "COMPLEX_TRIANGULATED_SURFACE_SET",
            ],
        )
        .is_some()
        {
            if !self.detached_annotation {
                self.body_context_items.insert(id);
            }
            self.declared_items.insert(id);
            self.item_bodies
                .entry(id)
                .or_default()
                .extend(self.bodies.iter().cloned());
            if let Some(placement) = placement {
                self.placements.entry(id).or_default().push(placement);
            }
        } else if let Some(kind) = entity_kind(
            record,
            &[
                "TESSELLATED_SOLID",
                "TESSELLATED_SHELL",
                "TESSELLATED_GEOMETRIC_SET",
            ],
        ) {
            if self.declare_containers {
                self.declared_items.insert(id);
                self.item_bodies
                    .entry(id)
                    .or_default()
                    .extend(self.bodies.iter().cloned());
            }
            if self.claim_containers {
                self.typed.insert(id);
            }
            if !self.bodies.is_empty() && matches!(kind, "TESSELLATED_SOLID" | "TESSELLATED_SHELL")
            {
                self.unresolved_containers.remove(&id);
            }
            let item_ids = entity_parameter(record, kind, 0, 1)
                .and_then(ValueExt::list)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(ValueExt::reference)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for item in item_ids {
                self.visit(item, depth + 1, placement);
            }
        } else if self.declare_containers {
            self.declared_items.insert(id);
            self.item_bodies
                .entry(id)
                .or_default()
                .extend(self.bodies.iter().cloned());
        }
        self.active.remove(&id);
    }
}

fn distinct_placements(placements: &[Transform]) -> Vec<Transform> {
    let mut distinct = Vec::new();
    for &placement in placements {
        if !distinct.contains(&placement) {
            distinct.push(placement);
        }
    }
    distinct
}

fn repositioned_placement(record: &RawRecord, geometry: &GeometryData) -> Option<Transform> {
    let placement_id = entity_parameter(record, "REPOSITIONED_TESSELLATED_ITEM", 0, 1)
        .and_then(ValueExt::reference)?;
    geometry
        .placements
        .get(&placement_id)
        .copied()
        .map(super::geometry::placement_transform)
}

fn tessellated_annotation_item(record: &RawRecord) -> Option<u64> {
    for name in ["TESSELLATED_ANNOTATION_OCCURRENCE", "STYLED_ITEM"] {
        let Some(partial) = record.partials.iter().find(|partial| partial.name == name) else {
            continue;
        };
        if let Some(item) = partial
            .parameters
            .iter()
            .rev()
            .find_map(ValueExt::reference)
        {
            return Some(item);
        }
    }
    None
}

fn product_linked_representations(exchange: &Exchange) -> BTreeSet<u64> {
    let product_shape_definitions = exchange
        .entities("PRODUCT_DEFINITION_SHAPE")
        .filter_map(|(id, record)| {
            record
                .partials
                .iter()
                .any(|partial| partial.name == "PRODUCT_DEFINITION_SHAPE")
                .then_some(id)
        })
        .collect::<BTreeSet<_>>();
    let mut linked = exchange
        .entities("SHAPE_DEFINITION_REPRESENTATION")
        .filter_map(|(_, record)| {
            let partial = record
                .partials
                .iter()
                .find(|partial| partial.name == "SHAPE_DEFINITION_REPRESENTATION")?;
            let definition = partial.parameters.first().and_then(ValueExt::reference)?;
            product_shape_definitions
                .contains(&definition)
                .then(|| partial.parameters.get(1).and_then(ValueExt::reference))?
        })
        .collect::<BTreeSet<_>>();
    if linked.is_empty() {
        return linked;
    }
    let mut relationships = BTreeMap::<u64, BTreeSet<u64>>::new();
    for record in exchange.records.values() {
        let Some(shape_relationship) = record
            .partials
            .iter()
            .find(|partial| partial.name == "SHAPE_REPRESENTATION_RELATIONSHIP")
        else {
            continue;
        };
        let (left, right) = {
            let mut references = shape_relationship
                .parameters
                .iter()
                .filter_map(ValueExt::reference);
            match (references.next(), references.next()) {
                (Some(left), Some(right)) => (left, right),
                _ => {
                    let Some(base) = record
                        .partials
                        .iter()
                        .find(|partial| partial.name == "REPRESENTATION_RELATIONSHIP")
                    else {
                        continue;
                    };
                    let mut references = base.parameters.iter().filter_map(ValueExt::reference);
                    let (Some(left), Some(right)) = (references.next(), references.next()) else {
                        continue;
                    };
                    (left, right)
                }
            }
        };
        relationships.entry(left).or_default().insert(right);
        relationships.entry(right).or_default().insert(left);
    }
    let mut pending = linked.iter().copied().collect::<Vec<_>>();
    while let Some(representation) = pending.pop() {
        for &related in relationships.get(&representation).into_iter().flatten() {
            if linked.insert(related) {
                pending.push(related);
            }
        }
    }
    linked
}

fn linked_bodies(record: &RawRecord, kind: &str, topology: &TopologyData) -> BTreeSet<BodyId> {
    let Some(link) = entity_parameter(record, kind, 1, 1).and_then(ValueExt::reference) else {
        return BTreeSet::new();
    };
    match kind {
        "TESSELLATED_SOLID" => topology
            .body_by_root
            .get(&link)
            .into_iter()
            .flatten()
            .cloned()
            .collect(),
        "TESSELLATED_SHELL" => topology
            .body_by_shell
            .get(&link)
            .cloned()
            .unwrap_or_default(),
        _ => BTreeSet::new(),
    }
}

fn index_list(value: Option<&Value>) -> Option<Vec<u32>> {
    value?
        .list()?
        .iter()
        .map(|value| u32::try_from(value.integer()?).ok())
        .collect()
}

fn has_entity(record: &RawRecord, name: &str) -> bool {
    entity_kind(record, &[name]).is_some()
}

fn is_tessellated_shape_representation(record: &RawRecord) -> bool {
    entity_kind(
        record,
        &[
            "TESSELLATED_SHAPE_REPRESENTATION",
            "TESSELLATED_SHAPE_REPRESENTATION_WITH_ACCURACY_PARAMETERS",
        ],
    )
    .is_some()
}

fn entity_kind<'a>(record: &'a RawRecord, names: &[&str]) -> Option<&'a str> {
    record
        .partials
        .iter()
        .find(|partial| names.iter().any(|name| *name == partial.name))
        .map(|partial| partial.name.as_str())
}

fn entity_parameter<'a>(
    record: &'a RawRecord,
    entity: &str,
    index: usize,
    simple_offset: usize,
) -> Option<&'a Value> {
    let partial = record
        .partials
        .iter()
        .find(|partial| partial.name == entity)?;
    let offset = if record.partials.len() == 1 {
        simple_offset
    } else {
        0
    };
    partial.parameters.get(index + offset)
}

fn own_parameter_offset(entity: &str) -> usize {
    match entity {
        "TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_FACE" => 5,
        "TRIANGULATED_SURFACE_SET" | "COMPLEX_TRIANGULATED_SURFACE_SET" => 4,
        _ => unreachable!("tessellation entity has no indexed subtype fields"),
    }
}

fn inherited_parameter<'a>(record: &'a RawRecord, entity: &str, index: usize) -> Option<&'a Value> {
    if record.partials.len() == 1 {
        record.parameter(index + 1)
    } else {
        entity_parameter(record, entity, index, 0)
    }
}

fn triangle_rows(value: &Value) -> Option<Vec<[u32; 3]>> {
    let rows = value.list()?;
    rows.iter()
        .map(|row| {
            let values = row.list()?;
            if values.len() != 3 {
                return None;
            }
            Some([
                u32::try_from(values[0].integer()?).ok()?,
                u32::try_from(values[1].integer()?).ok()?,
                u32::try_from(values[2].integer()?).ok()?,
            ])
        })
        .collect::<Option<Vec<_>>>()
}

fn complex_triangles(
    strips: Option<&Value>,
    fans: Option<&Value>,
) -> (Option<Vec<[u32; 3]>>, Vec<u32>) {
    let strips = index_rows(strips).unwrap_or_default();
    let fans = index_rows(fans).unwrap_or_default();
    let mut triangles = Vec::new();
    for strip in strips {
        for index in 0..strip.len().saturating_sub(2) {
            triangles.push(if index % 2 == 0 {
                [strip[index], strip[index + 1], strip[index + 2]]
            } else {
                [strip[index + 1], strip[index], strip[index + 2]]
            });
        }
    }
    for fan in fans {
        for index in 1..fan.len().saturating_sub(1) {
            triangles.push([fan[0], fan[index], fan[index + 1]]);
        }
    }
    ((!triangles.is_empty()).then_some(triangles), Vec::new())
}

fn index_rows(value: Option<&Value>) -> Option<Vec<Vec<u32>>> {
    Some(
        value?
            .list()?
            .iter()
            .filter_map(|row| {
                let indices = row
                    .list()?
                    .iter()
                    .map(|value| u32::try_from(value.integer()?).ok())
                    .collect::<Option<Vec<_>>>()?;
                (indices.len() >= 3).then_some(indices)
            })
            .collect(),
    )
}

fn normal_rows(value: Option<&Value>) -> Option<Vec<Vector3>> {
    value?
        .list()?
        .iter()
        .map(|row| {
            let values = row.list()?;
            if values.len() != 3 {
                return None;
            }
            let normal = Vector3::new(
                values[0].number()?,
                values[1].number()?,
                values[2].number()?,
            );
            let length = normal.norm();
            (length.is_finite() && length > 0.0)
                .then(|| Vector3::new(normal.x / length, normal.y / length, normal.z / length))
        })
        .collect()
}
trait RecordExt {
    fn parameter(&self, index: usize) -> Option<&Value>;
}
impl RecordExt for RawRecord {
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials
            .first()
            .and_then(|partial| partial.parameters.get(index))
    }
}
trait ValueExt {
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn number(&self) -> Option<f64>;
    fn integer(&self) -> Option<i64>;
}
impl ValueExt for Value {
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
    fn list(&self) -> Option<&[Value]> {
        if let Value::List(values) = self {
            Some(values)
        } else {
            None
        }
    }
    fn number(&self) -> Option<f64> {
        match self {
            Value::Real(value) => Some(*value),
            Value::Integer(value) => Some(*value as f64),
            _ => None,
        }
    }
    fn integer(&self) -> Option<i64> {
        if let Value::Integer(value) = self {
            Some(*value)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
