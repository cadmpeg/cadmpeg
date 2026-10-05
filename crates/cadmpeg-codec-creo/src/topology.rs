// SPDX-License-Identifier: Apache-2.0
//! Native half-edge graph assembly from curve topology rows.
//!
//! [`build`] resolves successors only when a curve and face identify one
//! candidate. It emits a [`Loop`] only when traversal closes on its starting
//! half-edge.
#![deny(clippy::disallowed_methods)]

use cadmpeg_core::decode::id_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use crate::curve::CurveTopologyRow;

/// One of the two native curve suffix sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Side {
    /// The `F0`/`E0` suffix side.
    Zero,
    /// The `F1`/`E1` suffix side.
    One,
}

impl cadmpeg_core::decode::cost::DecodeCost for Side {
    const FIXED_BYTES: Option<u64> = Some(1);
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(1)
    }
}

impl Side {
    /// The opposite side of this curve.
    pub(crate) const fn flip(self) -> Self {
        match self {
            Self::Zero => Self::One,
            Self::One => Self::Zero,
        }
    }

    /// Index into the two-element face and successor arrays.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Zero => 0,
            Self::One => 1,
        }
    }
}

impl std::fmt::Display for Side {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.index().fmt(formatter)
    }
}

impl serde::Serialize for Side {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Self::Zero => 0,
            Self::One => 1,
        })
    }
}

/// A curve identifier paired with one of its two native sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HalfEdgeId {
    /// The owning curve's `crv_id` in the `crv_array` namespace.
    pub(crate) curve_id: u32,
    /// The half-edge side: `0` for the `F0`/`E0` suffix fields, `1` for
    /// `F1`/`E1`.
    pub(crate) side: Side,
}

impl cadmpeg_core::decode::cost::DecodeCost for HalfEdgeId {
    const FIXED_BYTES: Option<u64> = Some(5);
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.curve_id, &self.side),
            ctx,
            operation,
        )
    }
}

/// A native half-edge, its face, and its uniquely resolved successor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HalfEdge {
    /// This half-edge's curve and side.
    pub(crate) id: HalfEdgeId,
    /// The `srf_array` face identifier this half-edge side bounds (the
    /// corresponding `F0`/`F1` suffix field).
    pub(crate) face_id: Option<NonZeroU32>,
    /// The next half-edge on the same face, when exactly one candidate
    /// successor matched the row's `E0`/`E1` next-edge field on that face.
    /// `None` when the successor is absent or ambiguous.
    pub(crate) next: Option<HalfEdgeId>,
}

/// A closed ring of half-edges on one face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Loop {
    /// The `srf_array` face identifier this loop bounds.
    face_id: Option<NonZeroU32>,
    /// The ring of half-edges in traversal order, starting from the first
    /// half-edge encountered for this face.
    half_edges: Vec<HalfEdgeId>,
}

impl Loop {
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        face_id: Option<NonZeroU32>,
        half_edges: Vec<HalfEdgeId>,
        graph: &[HalfEdge],
    ) -> Result<Option<Self>, CodecError> {
        if half_edges.is_empty() {
            return Ok(None);
        }
        let count = cadmpeg_core::decode::u64_from_index(half_edges.len());
        let work = count
            .checked_mul(cadmpeg_core::decode::u64_from_index(graph.len()))
            .and_then(|work| {
                count
                    .checked_mul(count)
                    .and_then(|unique| work.checked_add(unique))
            })
            .ok_or_else(|| {
                ctx.refuse_codec_limit("creo closed ring validation work", u64::MAX, u64::MAX)
            })?;
        ctx.charge_work(work, "creo closed ring validation work")?;
        for (index, (id, next)) in half_edges
            .iter()
            .zip(half_edges.iter().cycle().skip(1))
            .enumerate()
        {
            if half_edges.iter().take(index).any(|previous| previous == id) {
                return Ok(None);
            }
            let mut candidates = graph.iter().filter(|edge| edge.id == *id);
            let Some(edge) = candidates.next() else {
                return Ok(None);
            };
            if candidates.next().is_some() || edge.face_id != face_id || edge.next != Some(*next) {
                return Ok(None);
            }
        }
        Ok(Some(Self {
            face_id,
            half_edges,
        }))
    }

    pub(crate) fn face_id(&self) -> Option<NonZeroU32> {
        self.face_id
    }
    pub(crate) fn half_edges(&self) -> &[HalfEdgeId] {
        &self.half_edges
    }
}

/// One connected component of non-null `srf_array` face references.
///
/// The component is native topology only. It is not an emitted shell because
/// curve geometry, face carriers, and vertex bindings are independent layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FaceComponent {
    /// Sorted nonzero face identifiers in the connected component.
    face_ids: Vec<u32>,
    /// Sorted curve identifiers whose two sides connect component faces.
    curve_ids: Vec<u32>,
}

impl FaceComponent {
    fn new(
        ctx: &DecodeContext<'_>,
        face_ids: Vec<u32>,
        curve_ids: Vec<u32>,
    ) -> Result<Option<Self>, CodecError> {
        let work = cadmpeg_core::decode::u64_from_index(face_ids.len())
            .checked_mul(2)
            .and_then(|work| {
                work.checked_add(cadmpeg_core::decode::u64_from_index(curve_ids.len()))
            })
            .ok_or_else(|| {
                ctx.refuse_codec_limit("creo face component validation work", u64::MAX, u64::MAX)
            })?;
        ctx.charge_work(work, "creo face component validation work")?;
        if face_ids.is_empty()
            || face_ids.contains(&0)
            || face_ids.windows(2).any(|pair| pair[0] >= pair[1])
            || curve_ids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Ok(None);
        }
        Ok(Some(Self {
            face_ids,
            curve_ids,
        }))
    }
    pub(crate) fn face_ids(&self) -> &[u32] {
        &self.face_ids
    }
    pub(crate) fn curve_ids(&self) -> &[u32] {
        &self.curve_ids
    }
    #[cfg(test)]
    pub(crate) fn new_for_test(
        ctx: &DecodeContext<'_>,
        face_ids: Vec<u32>,
        curve_ids: Vec<u32>,
    ) -> Result<Option<Self>, CodecError> {
        Self::new(ctx, face_ids, curve_ids)
    }
}

/// One topological vertex represented by its incident half-edge orbit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopologicalVertex {
    /// Deterministic one-based vertex identifier.
    pub(crate) id: NonZeroU32,
    /// Sorted half-edges sharing this start vertex.
    half_edges: Vec<HalfEdgeId>,
}

impl TopologicalVertex {
    fn new(
        ctx: &DecodeContext<'_>,
        id: u32,
        half_edges: Vec<HalfEdgeId>,
    ) -> Result<Option<Self>, CodecError> {
        let Some(id) = NonZeroU32::new(id) else {
            return Ok(None);
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(half_edges.len()),
            "creo vertex orbit validation work",
        )?;
        if half_edges.is_empty() || half_edges.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Ok(None);
        }
        Ok(Some(Self { id, half_edges }))
    }
    pub(crate) fn half_edges(&self) -> &[HalfEdgeId] {
        &self.half_edges
    }
    #[cfg(test)]
    pub(crate) fn new_for_test(
        ctx: &DecodeContext<'_>,
        id: u32,
        half_edges: Vec<HalfEdgeId>,
    ) -> Result<Option<Self>, CodecError> {
        Self::new(ctx, id, half_edges)
    }
}

/// Start/end vertex binding for one oriented half-edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HalfEdgeVertexIncidence {
    /// Bound oriented half-edge.
    pub(crate) half_edge: HalfEdgeId,
    /// Vertex orbit containing this half-edge.
    pub(crate) start_vertex_id: NonZeroU32,
    /// Start vertex of the resolved successor half-edge.
    pub(crate) end_vertex_id: Option<NonZeroU32>,
}

/// Return each uniquely identified curve's two half-edge start vertices.
///
/// Start vertices come from the vertex-orbit relation and remain available
/// when one or both successor end relations are unresolved. Callers that need
/// a complete oriented edge must use [`edge_vertex_pairs`] instead.
pub(crate) fn edge_start_vertex_pairs(
    ctx: &DecodeContext<'_>,
    incidence: &[HalfEdgeVertexIncidence],
) -> Result<BTreeMap<u32, [NonZeroU32; 2]>, CodecError> {
    let mut by_curve = BTreeMap::<u32, [SingleSide<NonZeroU32>; 2]>::new();
    for binding in incidence {
        let sides = match ctx.entry_btree_map(
            &mut by_curve,
            binding.half_edge.curve_id,
            "creo start-vertex pair group nodes",
        )? {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert([SingleSide::Empty, SingleSide::Empty])
            }
        };
        sides[binding.half_edge.side.index()].push(binding.start_vertex_id);
    }
    let mut pairs = BTreeMap::new();
    for (curve_id, sides) in by_curve {
        if let (Some(first), Some(second)) = (sides[0].sole(), sides[1].sole()) {
            ctx.insert_btree_map(
                &mut pairs,
                curve_id,
                [*first, *second],
                "creo start-vertex pair nodes",
            )?;
        }
    }
    Ok(pairs)
}

enum SingleSide<T> {
    Empty,
    One(T),
    Many,
}

impl<T> SingleSide<T> {
    fn push(&mut self, value: T) {
        *self = match std::mem::replace(self, Self::Empty) {
            Self::Empty => Self::One(value),
            Self::One(_) | Self::Many => Self::Many,
        };
    }

    fn sole(&self) -> Option<&T> {
        match self {
            Self::One(value) => Some(value),
            Self::Empty | Self::Many => None,
        }
    }
}

/// Return every non-null face incident to a vertex orbit.
///
/// Each orbit member identifies an edge endpoint. Both half-edge sides of that
/// edge contribute face carriers at the endpoint, even when only one side is a
/// member of the outgoing orbit.
pub(crate) fn vertex_incident_faces(
    ctx: &DecodeContext<'_>,
    vertices: &[TopologicalVertex],
    edges: &[HalfEdge],
) -> Result<BTreeMap<NonZeroU32, BTreeSet<u32>>, CodecError> {
    let mut by_id = BTreeMap::new();
    for edge in edges {
        ctx.insert_btree_map(
            &mut by_id,
            edge.id,
            edge.face_id,
            "creo incident-face half-edge lookup nodes",
        )?;
    }
    let mut by_vertex = BTreeMap::new();
    for vertex in vertices {
        let mut faces = BTreeSet::new();
        for half_edge in &vertex.half_edges {
            for side in [
                *half_edge,
                HalfEdgeId {
                    curve_id: half_edge.curve_id,
                    side: half_edge.side.flip(),
                },
            ] {
                if let Some(face) = by_id.get(&side).copied().flatten().map(NonZeroU32::get) {
                    ctx.insert_btree_set(&mut faces, face, "creo incident face nodes")?;
                }
            }
        }
        ctx.insert_btree_map(
            &mut by_vertex,
            vertex.id,
            faces,
            "creo incident-face vertex nodes",
        )?;
    }
    Ok(by_vertex)
}

/// Resolve a curve's side-0 start and side-1 start as its oriented endpoint
/// pair when at least one face loop supplies the corresponding end relation.
/// Any supplied relation must agree with the opposite side's start vertex.
pub(crate) fn edge_vertex_pairs(
    ctx: &DecodeContext<'_>,
    incidence: &[HalfEdgeVertexIncidence],
) -> Result<BTreeMap<u32, [NonZeroU32; 2]>, CodecError> {
    let mut by_curve = BTreeMap::<u32, [SingleSide<&HalfEdgeVertexIncidence>; 2]>::new();
    for binding in incidence {
        let sides = match ctx.entry_btree_map(
            &mut by_curve,
            binding.half_edge.curve_id,
            "creo edge-vertex pair group nodes",
        )? {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert([SingleSide::Empty, SingleSide::Empty])
            }
        };
        sides[binding.half_edge.side.index()].push(binding);
    }
    let mut pairs = BTreeMap::new();
    for (curve_id, sides) in by_curve {
        let (Some(forward), Some(reverse)) = (sides[0].sole(), sides[1].sole()) else {
            continue;
        };
        if forward
            .end_vertex_id
            .is_some_and(|end| end != reverse.start_vertex_id)
            || reverse
                .end_vertex_id
                .is_some_and(|end| end != forward.start_vertex_id)
            || (forward.end_vertex_id.is_none() && reverse.end_vertex_id.is_none())
        {
            continue;
        }
        ctx.insert_btree_map(
            &mut pairs,
            curve_id,
            [forward.start_vertex_id, reverse.start_vertex_id],
            "creo edge-vertex pair nodes",
        )?;
    }
    Ok(pairs)
}

/// What one half-edge set states about its topological vertices.
#[derive(Debug)]
pub(crate) struct VertexOrbits {
    /// Topological vertex identities, one per admitted half-edge orbit.
    pub(crate) vertices: Vec<TopologicalVertex>,
    /// Start and end vertex binding of every half-edge an admitted orbit holds.
    pub(crate) incidence: Vec<HalfEdgeVertexIncidence>,
    /// The seed half-edge of every orbit past the one-based `u32` vertex
    /// identifier space, in traversal order. Each names an orbit this scan
    /// states no vertex for; its half-edges carry no incidence, and the decode
    /// report names the lane and the first instance.
    pub(crate) unstatable_orbits: Vec<HalfEdgeId>,
}

/// Build topological vertex orbits under `twin(previous(h))` and bind each
/// half-edge's start and end vertex.
/// Groups half-edges into start-vertex orbits.
///
/// An orbit past the one-based `u32` vertex identifier space states no vertex:
/// two orbits would otherwise carry one identifier and the incidence map would
/// bind the wrong half-edges. That orbit is named in
/// [`VertexOrbits::unstatable_orbits`] by its seed half-edge and refused at its
/// own lane. The rest of
/// the file's topology is unaffected, so it is not a whole-file refusal.
pub(crate) fn vertex_orbits(
    ctx: &DecodeContext<'_>,
    edges: &[HalfEdge],
) -> Result<VertexOrbits, CodecError> {
    let count = cadmpeg_core::decode::u64_from_index(edges.len());
    // The bound covers numeric tree comparisons and row projection per step.
    let lookup_work = 256 * (u64::from(u64::BITS - count.leading_zeros()) + 2);
    let mut by_id = BTreeMap::new();
    for edge in edges {
        ctx.charge_work(lookup_work, "creo vertex graph assembly")?;
        ctx.insert_btree_map(
            &mut by_id,
            edge.id,
            edge,
            "creo vertex-orbit half-edge lookup nodes",
        )?;
    }
    let mut predecessors = BTreeMap::<HalfEdgeId, Vec<HalfEdgeId>>::new();
    for edge in edges {
        ctx.charge_work(lookup_work, "creo vertex graph assembly")?;
        if let Some(next) = edge.next {
            let previous = ctx
                .entry_btree_map(&mut predecessors, next, "creo predecessor group nodes")?
                .or_default();
            ctx.reserve_vec(previous, 1, "creo predecessor group members")?;
            previous.push(edge.id);
        }
    }
    let mut vertex_adjacency = BTreeMap::<HalfEdgeId, BTreeSet<HalfEdgeId>>::new();
    for half_edge in by_id.keys().copied() {
        ctx.charge_work(lookup_work, "creo vertex graph adjacency")?;
        adjacency_for(ctx, &mut vertex_adjacency, half_edge)?;
        let Some(previous) = predecessors.get(&half_edge) else {
            continue;
        };
        if previous.len() != 1 {
            continue;
        }
        let twin_previous = HalfEdgeId {
            curve_id: previous[0].curve_id,
            side: previous[0].side.flip(),
        };
        if !by_id.contains_key(&twin_previous) {
            continue;
        }
        let adjacent = adjacency_for(ctx, &mut vertex_adjacency, half_edge)?;
        ctx.insert_btree_set(adjacent, twin_previous, "creo vertex adjacency links")?;
        let adjacent = adjacency_for(ctx, &mut vertex_adjacency, twin_previous)?;
        ctx.insert_btree_set(adjacent, half_edge, "creo vertex adjacency links")?;
    }
    let mut visited = BTreeSet::new();
    let mut vertices = Vec::new();
    let mut unstatable_orbits = Vec::new();
    for start in by_id.keys().copied() {
        ctx.charge_work(lookup_work, "creo vertex graph seeds")?;
        if visited.contains(&start) {
            continue;
        }
        let mut orbit = BTreeSet::new();
        let mut pending = Vec::new();
        ctx.reserve_vec(&mut pending, 1, "creo vertex orbit pending edges")?;
        pending.push(start);
        while let Some(half_edge) = pending.pop() {
            ctx.charge_work(lookup_work, "creo vertex graph traversal")?;
            if visited.contains(&half_edge) {
                continue;
            }
            ctx.insert_btree_set(&mut visited, half_edge, "creo visited vertex-orbit edges")?;
            ctx.insert_btree_set(&mut orbit, half_edge, "creo vertex orbit member nodes")?;
            for next in vertex_adjacency
                .get(&half_edge)
                .into_iter()
                .flatten()
                .copied()
            {
                ctx.charge_work(lookup_work, "creo vertex graph neighbours")?;
                if visited.contains(&next) {
                    continue;
                }
                ctx.reserve_vec(&mut pending, 1, "creo vertex orbit pending edges")?;
                pending.push(next);
            }
        }
        let Some(id) = id_from_index(vertices.len()).and_then(|position| position.checked_add(1))
        else {
            // `start` is the half-edge the orbit was grown from, so it names
            // the orbit no identifier could be stated for.
            ctx.reserve_vec(&mut unstatable_orbits, 1, "creo unstatable vertex orbits")?;
            unstatable_orbits.push(start);
            continue;
        };
        let mut half_edges = Vec::new();
        ctx.reserve_vec(&mut half_edges, orbit.len(), "creo vertex orbit half-edges")?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(orbit.len()),
            "creo vertex orbit projection",
        )?;
        half_edges.extend(orbit);
        ctx.reserve_vec(&mut vertices, 1, "creo topological vertices")?;
        let vertex = TopologicalVertex::new(ctx, id, half_edges)?
            .ok_or_else(|| CodecError::malformed("invalid derived Creo vertex orbit"))?;
        vertices.push(vertex);
    }
    let mut start_vertex = BTreeMap::new();
    for vertex in &vertices {
        for half_edge in &vertex.half_edges {
            ctx.charge_work(lookup_work, "creo vertex graph start bindings")?;
            ctx.insert_btree_map(
                &mut start_vertex,
                *half_edge,
                vertex.id,
                "creo start-vertex lookup nodes",
            )?;
        }
    }
    let mut incidence = Vec::new();
    for edge in edges {
        ctx.charge_work(lookup_work, "creo vertex graph assembly")?;
        if let Some(start_vertex_id) = start_vertex.get(&edge.id) {
            ctx.reserve_vec(&mut incidence, 1, "creo half-edge vertex incidence")?;
            incidence.push(HalfEdgeVertexIncidence {
                half_edge: edge.id,
                start_vertex_id: *start_vertex_id,
                end_vertex_id: edge.next.and_then(|next| start_vertex.get(&next).copied()),
            });
        }
    }
    Ok(VertexOrbits {
        vertices,
        incidence,
        unstatable_orbits,
    })
}

fn adjacency_for<'a>(
    ctx: &DecodeContext<'_>,
    adjacency: &'a mut BTreeMap<HalfEdgeId, BTreeSet<HalfEdgeId>>,
    id: HalfEdgeId,
) -> Result<&'a mut BTreeSet<HalfEdgeId>, CodecError> {
    Ok(
        match ctx.entry_btree_map(adjacency, id, "creo vertex adjacency nodes")? {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(BTreeSet::new()),
        },
    )
}

/// Group bounded face references connected by uniquely identified curve
/// topology rows.
///
/// A curve contributes to a component when either of its sides names a face.
pub(crate) fn face_components(
    ctx: &DecodeContext<'_>,
    rows: &[CurveTopologyRow],
) -> Result<Vec<FaceComponent>, CodecError> {
    let count = cadmpeg_core::decode::u64_from_index(rows.len());
    let lookup_work = 256 * (u64::from(u64::BITS - count.leading_zeros()) + 2);
    let rows = crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)?;
    let mut adjacency = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut face_curves = BTreeMap::<u32, BTreeSet<u32>>::new();
    for row in &rows {
        ctx.charge_work(lookup_work, "creo face graph assembly")?;
        let [left, right] = row.faces;
        for face in [left, right].into_iter().flatten().map(NonZeroU32::get) {
            face_set(ctx, &mut adjacency, face, "creo face adjacency nodes")?;
            let curves = face_set(ctx, &mut face_curves, face, "creo face curve group nodes")?;
            ctx.insert_btree_set(curves, row.id, "creo face curve member nodes")?;
        }
        if let (Some(left), Some(right)) = (left, right) {
            if left != right {
                let neighbors =
                    face_set(ctx, &mut adjacency, left.get(), "creo face adjacency nodes")?;
                ctx.insert_btree_set(neighbors, right.get(), "creo face adjacency links")?;
                let neighbors = face_set(
                    ctx,
                    &mut adjacency,
                    right.get(),
                    "creo face adjacency nodes",
                )?;
                ctx.insert_btree_set(neighbors, left.get(), "creo face adjacency links")?;
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    for start in adjacency.keys().copied() {
        ctx.charge_work(lookup_work, "creo face graph seeds")?;
        if seen.contains(&start) {
            continue;
        }
        ctx.insert_btree_set(&mut seen, start, "creo seen component faces")?;
        let mut pending = Vec::new();
        ctx.reserve_vec(&mut pending, 1, "creo pending component faces")?;
        pending.push(start);
        let mut faces = BTreeSet::new();
        let mut curves = BTreeSet::new();
        while let Some(face) = pending.pop() {
            ctx.charge_work(lookup_work, "creo face graph traversal")?;
            ctx.insert_btree_set(&mut faces, face, "creo component face nodes")?;
            for curve in face_curves.get(&face).into_iter().flatten().copied() {
                ctx.charge_work(lookup_work, "creo face graph curve memberships")?;
                ctx.insert_btree_set(&mut curves, curve, "creo component curve nodes")?;
            }
            for neighbour in adjacency.get(&face).into_iter().flatten().copied() {
                ctx.charge_work(lookup_work, "creo face graph neighbours")?;
                if !seen.contains(&neighbour) {
                    ctx.insert_btree_set(&mut seen, neighbour, "creo seen component faces")?;
                    ctx.reserve_vec(&mut pending, 1, "creo pending component faces")?;
                    pending.push(neighbour);
                }
            }
        }
        let mut face_ids = Vec::new();
        ctx.reserve_vec(&mut face_ids, faces.len(), "creo component face IDs")?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(faces.len()),
            "creo face component projection",
        )?;
        face_ids.extend(faces);
        let mut curve_ids = Vec::new();
        ctx.reserve_vec(&mut curve_ids, curves.len(), "creo component curve IDs")?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(curves.len()),
            "creo face component projection",
        )?;
        curve_ids.extend(curves);
        ctx.reserve_vec(&mut components, 1, "creo face components")?;
        let component = FaceComponent::new(ctx, face_ids, curve_ids)?
            .ok_or_else(|| CodecError::malformed("invalid derived Creo face component"))?;
        components.push(component);
    }
    Ok(components)
}

fn face_set<'a>(
    ctx: &DecodeContext<'_>,
    groups: &'a mut BTreeMap<u32, BTreeSet<u32>>,
    id: u32,
    operation: &'static str,
) -> Result<&'a mut BTreeSet<u32>, CodecError> {
    Ok(match ctx.entry_btree_map(groups, id, operation)? {
        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::btree_map::Entry::Vacant(entry) => entry.insert(BTreeSet::new()),
    })
}

/// Select the model body count using the settled metadata precedence.
pub(crate) fn selected_body_count(
    declared_body_count: Option<u32>,
    first_quilt_ptr: Option<u32>,
    face_component_count: usize,
) -> Option<usize> {
    if let Some(count) = declared_body_count.filter(|count| *count > 0) {
        return usize::try_from(count).ok();
    }
    if first_quilt_ptr == Some(0) {
        return Some(1);
    }
    if face_component_count <= 1 {
        return Some(face_component_count);
    }
    (declared_body_count.is_none() && first_quilt_ptr.is_none()).then_some(face_component_count)
}

/// Build half-edges and closed loops from uniquely identified curve topology
/// rows. Repeated curve identifiers define no derived topology because a
/// half-edge identity cannot distinguish their sides.
///
/// Ambiguous or missing successors remain `None` and cannot form loops.
pub(crate) fn build(
    ctx: &DecodeContext<'_>,
    rows: &[CurveTopologyRow],
) -> Result<(Vec<HalfEdge>, Vec<Loop>), CodecError> {
    let rows = crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)?;
    let mut face_sides: BTreeMap<Option<NonZeroU32>, Vec<HalfEdgeId>> = BTreeMap::new();
    for row in &rows {
        for side in [Side::Zero, Side::One] {
            let sides = ctx
                .entry_btree_map(
                    &mut face_sides,
                    row.faces[side.index()],
                    "creo face-side group nodes",
                )?
                .or_default();
            ctx.reserve_vec(sides, 1, "creo face-side group members")?;
            sides.push(HalfEdgeId {
                curve_id: row.id,
                side,
            });
        }
    }
    let mut edges = Vec::new();
    for row in rows {
        ctx.reserve_vec(&mut edges, 2, "creo topology half-edges")?;
        for side in [Side::Zero, Side::One] {
            let face_id = row.faces[side.index()];
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(face_sides.get(&face_id).map_or(0, Vec::len)),
                "creo topology successor scan",
            )?;
            let mut candidates = face_sides
                .get(&face_id)
                .into_iter()
                .flatten()
                .filter(|id| id.curve_id == row.next_edges[side.index()])
                .copied();
            let next = match (candidates.next(), candidates.next()) {
                (Some(candidate), None) => Some(candidate),
                _ => None,
            };
            edges.push(HalfEdge {
                id: HalfEdgeId {
                    curve_id: row.id,
                    side,
                },
                face_id,
                next,
            });
        }
    }
    ctx.stable_sort_by(
        edges.as_mut_slice(),
        |value| &value.id,
        Ord::cmp,
        "creo build edges ordering",
    )?;
    let by_id = |id: HalfEdgeId| {
        edges
            .binary_search_by_key(&id, |edge| edge.id)
            .ok()
            .map(|index| &edges[index])
    };
    let mut consumed = BTreeSet::new();
    let mut open = BTreeSet::new();
    let mut loops = Vec::new();
    for edge in &edges {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(consumed.len()),
            "creo topology consumed lookup",
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(open.len()),
            "creo topology open lookup",
        )?;
        if consumed.contains(&edge.id) || open.contains(&edge.id) {
            continue;
        }
        let mut ring = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = edge.id;
        let mut open_ended = false;
        loop {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(seen.len()),
                "creo topology ring visited lookup",
            )?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(open.len()),
                "creo topology open tail lookup",
            )?;
            if open.contains(&current) {
                open_ended = true;
                break;
            }
            if seen.contains(&current) {
                if current == edge.id {
                    for id in ring.iter().copied() {
                        ctx.charge_work(
                            cadmpeg_core::decode::u64_from_index(consumed.len()),
                            "creo topology consumed membership",
                        )?;
                        ctx.insert_btree_set(
                            &mut consumed,
                            id,
                            "creo consumed topology half-edges",
                        )?;
                    }
                    ctx.reserve_vec(&mut loops, 1, "creo topology loops")?;
                    let closed = Loop::new(ctx, edge.face_id, std::mem::take(&mut ring), &edges)?
                        .ok_or_else(|| {
                        CodecError::malformed("invalid derived Creo closed ring")
                    })?;
                    loops.push(closed);
                }
                break;
            }
            ctx.insert_btree_set(&mut seen, current, "creo topology ring visit nodes")?;
            ctx.reserve_vec(&mut ring, 1, "creo topology ring half-edges")?;
            ring.push(current);
            ctx.charge_work(
                2 * u64::from(usize::BITS - edges.len().leading_zeros()),
                "creo topology ring successor lookup",
            )?;
            let Some(next) = by_id(current).and_then(|entry| entry.next) else {
                open_ended = true;
                break;
            };
            if by_id(next).is_none_or(|entry| entry.face_id != edge.face_id) {
                open_ended = true;
                break;
            }
            current = next;
        }
        if open_ended {
            for id in ring {
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(open.len()),
                    "creo topology open membership",
                )?;
                ctx.insert_btree_set(&mut open, id, "creo topology open half-edges")?;
            }
        }
    }
    Ok((edges, loops))
}

#[cfg(test)]
mod tests;
