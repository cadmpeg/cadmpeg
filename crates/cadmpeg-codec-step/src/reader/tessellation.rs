// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation decoding.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::transform::Transform;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::geometry::GeometryData;
use super::topology::TopologyData;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

pub(super) fn decode(
    exchange: &Exchange,
    geometry: &GeometryData,
    topology: &TopologyData,
    ir: &mut CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut admitted_meshes = u64_from_index(ir.model.tessellations.len());
    let mut coordinates = BTreeMap::new();
    for (&id, record) in exchange.records() {
        if !has_entity(record, "COORDINATES_LIST") {
            continue;
        }
        let scale = geometry.units.length([id]).get();
        if let Some(vertices) = coordinate_rows(record, scale) {
            coordinates.insert(id, vertices);
        }
    }
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let mut item_bodies = BTreeMap::<u64, BTreeSet<BodyId>>::new();
    let mut item_placements = BTreeMap::<u64, Vec<Transform>>::new();
    let mut unresolved_placements = BTreeSet::new();
    let mut declared_items = BTreeSet::new();
    let mut unresolved_containers = BTreeSet::new();
    let mut body_context_items = BTreeSet::new();
    for (&id, record) in exchange.records() {
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        let Some(items) = entity_parameter(record, kind, 0, 1).and_then(ValueExt::list) else {
            losses.push(
                StepLossCode::DecodeWarning.note(format!("{kind} #{id} has no structured items")),
            );
            continue;
        };
        let item_ids = container_item_ids(items, kind, id)?;
        declared_items.extend(item_ids.iter().copied());
        let candidates = linked_bodies(record, kind, topology);
        if candidates.is_empty() {
            unresolved_containers.insert(id);
        }
        let body_candidates = candidates.into_iter().collect::<Vec<_>>();
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
            mode: AssociationMode::BodyItems,
            active: BTreeSet::new(),
            ctx,
        };
        for item in item_ids {
            associator.visit(item, None)?;
        }
        typed.insert(id);
    }
    let mut representation_cache = BTreeMap::new();
    let product_representations = product_linked_representations(exchange);
    let product_representation_items =
        product_representation_items(exchange, &product_representations);
    for (&id, record) in exchange.records() {
        if !is_tessellated_shape_representation(record) {
            continue;
        }
        let Some(items) = representation_items(record) else {
            continue;
        };
        let bodies = super::topology::representation_bodies(
            id,
            exchange,
            topology,
            &mut representation_cache,
            &mut BTreeSet::new(),
            ctx,
        )?;
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
            mode: AssociationMode::Placements,
            active: BTreeSet::new(),
            ctx,
        };
        for item in items {
            associator.visit(item, None)?;
        }
    }
    drop(product_representation_items);
    drop(product_representations);
    drop(representation_cache);
    for (&id, record) in exchange.records() {
        if !has_entity(record, "TESSELLATED_ANNOTATION_OCCURRENCE") {
            continue;
        }
        let Some(item) = tessellated_annotation_item(record) else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "TESSELLATED_ANNOTATION_OCCURRENCE #{id} has no tessellated item"
            )));
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
            mode: AssociationMode::DetachedAnnotation,
            active: BTreeSet::new(),
            ctx,
        };
        associator.visit(item, None)?;
    }
    for id in unresolved_placements {
        losses.push(StepLossCode::TessellationPlacementUnresolved.note(format!(
            "repositioned tessellated item #{id} has no valid AXIS2_PLACEMENT_3D; unresolved placement is not applied"
        )));
    }
    for id in unresolved_containers {
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        let Some(kind) = entity_kind(record, &["TESSELLATED_SOLID", "TESSELLATED_SHELL"]) else {
            continue;
        };
        losses.push(
            StepLossCode::DecodeWarning
                .note(format!("{kind} #{id} has no decoded exact body link")),
        );
    }
    for (&item, placements) in &item_placements {
        if !item_bodies.get(&item).is_some_and(BTreeSet::is_empty) {
            continue;
        }
        if distinct_placement(placements).is_none() {
            let distinct_count = distinct_placement_count(placements);
            losses.push(StepLossCode::TessellationPlacementAmbiguous.note(format!(
                "tessellation item #{item} has {distinct_count} distinct repositioning placements; mesh retained in source coordinates"
            )));
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
            losses.push(StepLossCode::TessellationItemBodyUnresolved.note(format!(
                "tessellation item #{item} has {detail}; mesh retained as detached"
            )));
        }
    }
    for (&id, record) in exchange.records() {
        let Some(entity) = TriangulatedEntity::of(record) else {
            continue;
        };
        let kind = entity.name();
        let base_kind = entity.base_name();
        let Some(coordinate_id) =
            inherited_parameter(record, base_kind, 0).and_then(ValueExt::reference)
        else {
            losses.push(
                StepLossCode::DecodeWarning
                    .note(format!("{kind} #{id} has no COORDINATES_LIST reference")),
            );
            continue;
        };
        let Some(vertices) = coordinates.get(&coordinate_id) else {
            losses.push(
                StepLossCode::DecodeWarning
                    .note(format!("{kind} #{id} has no resolved COORDINATES_LIST")),
            );
            continue;
        };
        let offset = entity.own_parameter_offset();
        let triangles = match entity {
            TriangulatedEntity::Face | TriangulatedEntity::SurfaceSet => {
                entity_parameter(record, kind, 1, offset).and_then(triangle_rows)
            }
            TriangulatedEntity::ComplexFace | TriangulatedEntity::ComplexSurfaceSet => {
                complex_triangles(
                    entity_parameter(record, kind, 1, offset),
                    entity_parameter(record, kind, 2, offset),
                    kind,
                    id,
                )?
            }
        };
        let Some(triangles) = triangles.filter(|triangles| !triangles.is_empty()) else {
            losses.push(
                StepLossCode::DecodeWarning.note(format!("{kind} #{id} has no triangle indices")),
            );
            continue;
        };
        let pnindex = match entity_parameter(record, kind, 0, offset) {
            None | Some(Value::Omitted) => Vec::new(),
            Some(value) => {
                let Some(indices) = index_list(Some(value)) else {
                    losses.push(
                        StepLossCode::DecodeWarning
                            .note(format!("{kind} #{id} has an invalid pnindex")),
                    );
                    continue;
                };
                indices
            }
        };
        let (mut local_vertices, local_triangles, addressing) = if pnindex.is_empty() {
            if triangles.iter().flatten().any(|index| {
                *index == 0 || cadmpeg_core::decode::index_from_u32(*index) > vertices.len()
            }) {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "{kind} #{id} has an out-of-range one-based coordinate index"
                )));
                continue;
            }
            let coordinate_indices = triangles.iter().flatten().copied().collect::<BTreeSet<_>>();
            let mut local_index = BTreeMap::new();
            for (local, global) in coordinate_indices.iter().enumerate() {
                let local = u32::try_from(local).map_err(|_| {
                    CodecError::malformed(format_args!(
                        "{kind} #{id} names more distinct coordinates than a u32 index addresses"
                    ))
                })?;
                local_index.insert(*global, local);
            }
            let local_vertices = coordinate_indices
                .iter()
                .map(|index| vertices[cadmpeg_core::decode::index_from_u32(*index) - 1])
                .collect::<Vec<_>>();
            let local_triangles = triangles
                .iter()
                .map(|triangle| triangle.map(|index| local_index[&index]))
                .collect::<Vec<_>>();
            (
                local_vertices,
                local_triangles,
                CoordinateAddressing::TriangleIndices(coordinate_indices),
            )
        } else {
            if pnindex.iter().any(|index| {
                *index == 0 || cadmpeg_core::decode::index_from_u32(*index) > vertices.len()
            }) || triangles.iter().flatten().any(|index| {
                *index == 0 || cadmpeg_core::decode::index_from_u32(*index) > pnindex.len()
            }) {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "{kind} #{id} has an out-of-range one-based tessellation index"
                )));
                continue;
            }
            (
                pnindex
                    .iter()
                    .map(|index| vertices[cadmpeg_core::decode::index_from_u32(*index) - 1])
                    .collect::<Vec<_>>(),
                triangles
                    .iter()
                    .map(|triangle| triangle.map(|index| index - 1))
                    .collect::<Vec<_>>(),
                CoordinateAddressing::PnIndex,
            )
        };
        drop(triangles);
        drop(pnindex);
        let source_normals = match inherited_parameter(record, base_kind, 2) {
            None | Some(Value::Omitted) => Vec::new(),
            Some(value) => normal_rows(Some(value)).unwrap_or_else(|| {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "{kind} #{id} has invalid normal rows; normals omitted"
                )));
                Vec::new()
            }),
        };
        // AP242 permits an empty normals aggregate; the IR represents both
        // that spelling and an omitted lane as an absent normal lane.
        let mut normals = match source_normals.len() {
            0 => None,
            1 => Some(ctx.alloc_filled(
                local_vertices.len(),
                source_normals[0],
                "step_tessellation_normal_replication",
            )?),
            count if count == local_vertices.len() => Some(source_normals),
            count => match &addressing {
                CoordinateAddressing::TriangleIndices(coordinate_indices)
                    if count == vertices.len() =>
                {
                    Some(
                        coordinate_indices
                            .iter()
                            .map(|index| {
                                source_normals[cadmpeg_core::decode::index_from_u32(*index) - 1]
                            })
                            .collect::<Vec<_>>(),
                    )
                }
                CoordinateAddressing::PnIndex | CoordinateAddressing::TriangleIndices(_) => {
                    losses.push(StepLossCode::DecodeWarning.note(format!(
                        "{kind} #{id} carries {count} normals for {} coordinates",
                        local_vertices.len()
                    )));
                    None
                }
            },
        };
        if item_bodies.get(&id).is_some_and(BTreeSet::is_empty) {
            if let Some(placement) =
                distinct_placement(item_placements.get(&id).map_or(&[], Vec::as_slice))
            {
                local_vertices = local_vertices
                    .into_iter()
                    .map(|vertex| placement.apply_point(vertex.get()))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        CodecError::malformed(format!(
                            "{kind} #{id} placed tessellation vertex contains a non-finite coordinate"
                        ))
                    })?;
                if let Some(source_normals) = normals.take() {
                    match source_normals
                        .into_iter()
                        .map(|normal| {
                            placement
                                .apply_normal(normal.get())
                                .map(FiniteVector3::from)
                        })
                        .collect::<Option<Vec<_>>>()
                    {
                        Some(transformed) => normals = Some(transformed),
                        None => {
                            losses.push(StepLossCode::DecodeWarning.note(format!(
                                "{kind} #{id} normal placement could not produce finite unit normals; normals omitted"
                            )));
                        }
                    }
                }
            }
        }
        if let Some(surface_step) = complex_triangulated_face_surface(record) {
            let surface_id = ids::data(kind!("surface"), surface_step);
            if let Some(surface) = ir
                .model
                .surfaces
                .iter_mut()
                .find(|surface| surface.id.as_str() == surface_id.as_str())
            {
                if surface.source_object.is_none() {
                    surface.source_object = Some(super::step_source_association(id, None));
                }
            }
        }
        let rows = match cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
            local_vertices,
            local_triangles,
            normals,
        ) {
            Ok(rows) => rows,
            Err(error) => {
                losses.push(
                    StepLossCode::TessellationInvalidPayload.note(format!("{kind} #{id}: {error}")),
                );
                continue;
            }
        };

        let mut pending_meshes = admitted_meshes;
        let next_mesh_count = admitted_meshes
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("STEP tessellation mesh count exceeds u64"))?;
        ctx.admit_entities(
            next_mesh_count,
            &mut pending_meshes,
            "step_tessellation_mesh_entity",
        )?;
        let mesh = match Tessellation::from_parts(
            ids::tessellation(kind!("mesh"), id).into(),
            rows,
            Vec::new(),
        ) {
            Ok(mesh) => mesh,
            Err(error) => {
                losses.push(
                    StepLossCode::TessellationInvalidPayload.note(format!("{kind} #{id}: {error}")),
                );
                continue;
            }
        };
        admitted_meshes = pending_meshes;
        if !declared_items.contains(&id) {
            losses.push(StepLossCode::TessellationItemUndeclared.note(format!(
                "tessellation item #{id} is not declared by an exact body container; mesh retained as detached"
            )));
        }

        let body = (!item_bodies.get(&id).is_some_and(BTreeSet::is_empty))
            .then(|| item_bodies.get(&id))
            .flatten()
            .filter(|bodies| bodies.len() == 1)
            .and_then(|bodies| bodies.iter().next())
            .cloned();
        let source_object = if !declared_items.contains(&id)
            || item_bodies.get(&id).is_some_and(BTreeSet::is_empty)
            || item_bodies.get(&id).is_none_or(|bodies| bodies.len() != 1)
        {
            Some(super::step_source_association(id, None))
        } else {
            None
        };

        ir.model
            .tessellations
            .push(mesh.with_body(body).with_source_object(source_object));
        typed.insert(id);
        typed.insert(coordinate_id);
    }
    if !ir.model.tessellations.is_empty() {
        for (&id, record) in exchange.records() {
            if has_entity(record, "TESSELLATED_SHAPE_REPRESENTATION")
                || has_entity(record, "TESSELLATED_SOLID")
                || has_entity(record, "TESSELLATED_SHELL")
            {
                typed.insert(id);
            }
        }
    }
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

/// How a tessellated item addresses the coordinate list it shares.
///
/// A STEP tessellated item states either a PNINDEX list, which names the
/// coordinates it uses, or no PNINDEX at all, in which case its triangle
/// indices name them directly. The two are one statement of the record, so
/// they are one value: the coordinate set exists exactly when the record
/// states no PNINDEX.
enum CoordinateAddressing {
    /// The record states a PNINDEX list.
    PnIndex,
    /// The record states no PNINDEX; these triangle indices name the
    /// coordinates it uses, in ascending order.
    TriangleIndices(BTreeSet<u32>),
}

fn complex_triangulated_face_surface(record: &RawRecord) -> Option<u64> {
    has_entity(record, "COMPLEX_TRIANGULATED_FACE")
        .then(|| inherited_parameter(record, "TESSELLATED_FACE", 3))
        .flatten()
        .and_then(ValueExt::reference)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AssociationMode {
    BodyItems,
    Placements,
    DetachedAnnotation,
}

struct TessellationItemAssociator<'a, 'ctx, 'arena> {
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
    mode: AssociationMode,
    active: BTreeSet<u64>,
    ctx: &'ctx DecodeContext<'arena>,
}

impl TessellationItemAssociator<'_, '_, '_> {
    fn visit(&mut self, id: u64, inherited_placement: Option<Transform>) -> Result<(), CodecError> {
        let _depth_guard = self.ctx.enter_nested("step_tessellation_association")?;
        if !self.active.insert(id) {
            return Ok(());
        }
        let Some(record) = self.exchange.records().get(&id) else {
            self.active.remove(&id);
            return Ok(());
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
        let placement = match (inherited_placement, local_placement) {
            (Some(parent), Some(local)) => Some(parent.compose(local).map_err(|error| {
                CodecError::malformed(format_args!(
                    "invalid STEP tessellation placement #{id}: {error}"
                ))
            })?),
            (parent, None) => parent,
            (None, local) => local,
        };
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
            if self.mode != AssociationMode::DetachedAnnotation {
                self.body_context_items.insert(id);
            }
            self.declared_items.insert(id);
            associate_bodies(self.item_bodies, id, self.bodies);
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
            if self.mode == AssociationMode::BodyItems {
                self.declared_items.insert(id);
                associate_bodies(self.item_bodies, id, self.bodies);
            }
            if self.mode != AssociationMode::DetachedAnnotation {
                self.typed.insert(id);
            }
            if !self.bodies.is_empty() && matches!(kind, "TESSELLATED_SOLID" | "TESSELLATED_SHELL")
            {
                self.unresolved_containers.remove(&id);
            }
            let item_ids = match entity_parameter(record, kind, 0, 1).and_then(ValueExt::list) {
                Some(items) => container_item_ids(items, kind, id)?,
                None => Vec::new(),
            };
            for item in item_ids {
                self.visit(item, placement)?;
            }
        } else if self.mode == AssociationMode::BodyItems {
            self.declared_items.insert(id);
            associate_bodies(self.item_bodies, id, self.bodies);
        }
        self.active.remove(&id);
        Ok(())
    }
}

fn distinct_placement_count(placements: &[Transform]) -> usize {
    placements
        .iter()
        .enumerate()
        .filter(|(index, placement)| !placements[..*index].contains(placement))
        .count()
}

fn distinct_placement(placements: &[Transform]) -> Option<Transform> {
    let first = *placements.first()?;
    placements
        .iter()
        .all(|placement| *placement == first)
        .then_some(first)
}

fn repositioned_placement(record: &RawRecord, geometry: &GeometryData) -> Option<Transform> {
    let placement_id = entity_parameter(record, "REPOSITIONED_TESSELLATED_ITEM", 0, 1)
        .and_then(ValueExt::reference)?;
    geometry
        .placements
        .get(&placement_id)
        .copied()
        .and_then(super::geometry::placement_transform)
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

fn associate_bodies(
    associations: &mut BTreeMap<u64, BTreeSet<BodyId>>,
    item: u64,
    bodies: &[BodyId],
) {
    associations
        .entry(item)
        .or_default()
        .extend(bodies.iter().cloned());
}

fn product_linked_representations(exchange: &Exchange) -> BTreeSet<u64> {
    let mut product_shape_definitions = BTreeSet::new();
    for (id, record) in exchange.entities("PRODUCT_DEFINITION_SHAPE") {
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "PRODUCT_DEFINITION_SHAPE")
        {
            product_shape_definitions.insert(id);
        }
    }
    let mut linked = BTreeSet::new();
    for (_, record) in exchange.entities("SHAPE_DEFINITION_REPRESENTATION") {
        let representation = (|| {
            let partial = record
                .partials
                .iter()
                .find(|partial| partial.name == "SHAPE_DEFINITION_REPRESENTATION")?;
            let definition = partial.parameters.first().and_then(ValueExt::reference)?;
            product_shape_definitions
                .contains(&definition)
                .then(|| partial.parameters.get(1).and_then(ValueExt::reference))?
        })();
        if let Some(representation) = representation {
            linked.insert(representation);
        }
    }
    if linked.is_empty() {
        return linked;
    }
    let mut relationships = BTreeMap::<u64, BTreeSet<u64>>::new();
    for record in exchange.records().values() {
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

fn product_representation_items(
    exchange: &Exchange,
    representations: &BTreeSet<u64>,
) -> BTreeSet<u64> {
    representations
        .iter()
        .filter_map(|id| exchange.records().get(id))
        .filter_map(representation_items)
        .flatten()
        .collect()
}

fn representation_items(record: &RawRecord) -> Option<Vec<u64>> {
    let values = super::representation::item_values(record)?;
    Some(values.iter().filter_map(ValueExt::reference).collect())
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
    value
        .and_then(ValueExt::list)?
        .iter()
        .map(|value| u32::try_from(value.integer()?).ok())
        .collect()
}

fn container_item_ids(items: &[Value], kind: &str, id: u64) -> Result<Vec<u64>, CodecError> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            item.reference().ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{kind} #{id} item {} is not a reference",
                    index + 1
                ))
            })
        })
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

/// One triangulated tessellation entity, parsed from the record's partial names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriangulatedEntity {
    Face,
    ComplexFace,
    SurfaceSet,
    ComplexSurfaceSet,
}

impl TriangulatedEntity {
    fn of(record: &RawRecord) -> Option<Self> {
        record.partials.iter().find_map(|partial| {
            Some(match partial.name.as_str() {
                "TRIANGULATED_FACE" => Self::Face,
                "COMPLEX_TRIANGULATED_FACE" => Self::ComplexFace,
                "TRIANGULATED_SURFACE_SET" => Self::SurfaceSet,
                "COMPLEX_TRIANGULATED_SURFACE_SET" => Self::ComplexSurfaceSet,
                _ => return None,
            })
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Face => "TRIANGULATED_FACE",
            Self::ComplexFace => "COMPLEX_TRIANGULATED_FACE",
            Self::SurfaceSet => "TRIANGULATED_SURFACE_SET",
            Self::ComplexSurfaceSet => "COMPLEX_TRIANGULATED_SURFACE_SET",
        }
    }

    fn base_name(self) -> &'static str {
        match self {
            Self::Face | Self::ComplexFace => "TESSELLATED_FACE",
            Self::SurfaceSet | Self::ComplexSurfaceSet => "TESSELLATED_SURFACE_SET",
        }
    }

    fn own_parameter_offset(self) -> usize {
        match self {
            Self::Face | Self::ComplexFace => 5,
            Self::SurfaceSet | Self::ComplexSurfaceSet => 4,
        }
    }
}

fn inherited_parameter<'a>(record: &'a RawRecord, entity: &str, index: usize) -> Option<&'a Value> {
    if record.partials.len() == 1 {
        record.parameter(index + 1)
    } else {
        entity_parameter(record, entity, index, 0)
    }
}

fn coordinate_rows(record: &RawRecord, scale: f64) -> Option<Vec<FinitePoint3>> {
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::list)
        .find_map(|rows| {
            rows.iter()
                .map(|row| {
                    let values = row.list()?;
                    if values.len() != 3 {
                        return None;
                    }
                    let point = Point3::new(
                        values[0].number()? * scale,
                        values[1].number()? * scale,
                        values[2].number()? * scale,
                    );
                    FinitePoint3::new(point)
                })
                .collect::<Option<Vec<_>>>()
                .filter(|vertices| !vertices.is_empty())
        })
}

fn triangle_rows(value: &Value) -> Option<Vec<[u32; 3]>> {
    value
        .list()?
        .iter()
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
        .collect()
}

fn complex_triangles(
    strips: Option<&Value>,
    fans: Option<&Value>,
    kind: &str,
    id: u64,
) -> Result<Option<Vec<[u32; 3]>>, CodecError> {
    let strips = index_rows(strips, kind, id, "strip")?;
    let fans = index_rows(fans, kind, id, "fan")?;
    let mut triangles = Vec::new();
    for strip in strips {
        for index in 0..strip.len() - 2 {
            triangles.push(if index % 2 == 0 {
                [strip[index], strip[index + 1], strip[index + 2]]
            } else {
                [strip[index + 1], strip[index], strip[index + 2]]
            });
        }
    }
    for fan in fans {
        for index in 1..fan.len() - 1 {
            triangles.push([fan[0], fan[index], fan[index + 1]]);
        }
    }
    Ok((!triangles.is_empty()).then_some(triangles))
}

fn index_rows(
    value: Option<&Value>,
    kind: &str,
    id: u64,
    lane: &str,
) -> Result<Vec<Vec<u32>>, CodecError> {
    let Some(rows) = value.and_then(ValueExt::list) else {
        return Ok(Vec::new());
    };
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let invalid = || {
                CodecError::malformed(format_args!(
                    "{kind} #{id} {lane} row {} is invalid",
                    row_index + 1
                ))
            };
            let Some(values) = row.list().filter(|values| values.len() >= 3) else {
                return Err(invalid());
            };
            values
                .iter()
                .map(|value| {
                    u32::try_from(value.integer().ok_or_else(invalid)?).map_err(|_| invalid())
                })
                .collect()
        })
        .collect()
}

fn normal_rows(value: Option<&Value>) -> Option<Vec<FiniteVector3>> {
    value
        .and_then(ValueExt::list)?
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
            super::geometry::normalize(normal).map(FiniteVector3::from)
        })
        .collect()
}
#[cfg(test)]
pub(crate) mod tests;
