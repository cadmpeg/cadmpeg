// SPDX-License-Identifier: Apache-2.0
//! Topological dual-mesh reconstruction for embedded JT display models.

const MAX_TOPOLOGY_ITEMS: usize = 1_000_000;
const MAX_TOPOLOGY_SLOTS: usize = 8_000_000;

/// Decoded polygon in topological-vertex visit order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Polygon {
    pub(crate) vertex_indices: Vec<u32>,
    pub(crate) attribute_indices: Vec<Option<u32>>,
    pub(crate) group: i32,
    pub(crate) flags: u16,
}

#[derive(Clone)]
struct Vertex {
    faces: Vec<Option<usize>>,
    group: i32,
    flags: u16,
}

#[derive(Clone)]
struct Face {
    vertices: Vec<Option<usize>>,
    empty: usize,
    attribute_mask: Vec<bool>,
    attributes: Vec<u32>,
}

/// Attribute-mask symbol lanes consumed while faces are created.
pub(crate) struct AttributeMaskLanes<'a> {
    pub(crate) small: [&'a [i32]; 8],
    pub(crate) context_7_next_30: &'a [i32],
    pub(crate) context_7_upper_4: &'a [i32],
    pub(crate) large_words: &'a [i32],
}

struct Symbols<'a> {
    degrees: [&'a [i32]; 8],
    degree_pos: [usize; 8],
    valences: &'a [i32],
    groups: &'a [i32],
    flags: &'a [i32],
    split_faces: &'a [i32],
    split_positions: &'a [i32],
    attribute_masks: AttributeMaskLanes<'a>,
    attribute_mask_pos: [usize; 8],
    large_mask_pos: usize,
    vertex_pos: usize,
    split_pos: usize,
}

impl Symbols<'_> {
    fn vertex(&mut self) -> Option<(usize, i32, u16)> {
        let valence = usize::try_from(*self.valences.get(self.vertex_pos)?).ok()?;
        if valence == 0 {
            return None;
        }
        let group = *self.groups.get(self.vertex_pos)?;
        let flags = u16::try_from(*self.flags.get(self.vertex_pos)?).ok()?;
        self.vertex_pos += 1;
        Some((valence, group, flags))
    }

    fn degree(&mut self, context: usize) -> Option<i32> {
        let value = *self.degrees.get(context)?.get(self.degree_pos[context])?;
        self.degree_pos[context] += 1;
        Some(value)
    }

    fn split(&mut self) -> Option<(usize, usize)> {
        let face = usize::try_from(*self.split_faces.get(self.split_pos)?).ok()?;
        let position = usize::try_from(*self.split_positions.get(self.split_pos)?).ok()?;
        self.split_pos += 1;
        (face > 0).then_some((face, position))
    }

    fn attribute_mask(&mut self, degree: usize) -> Option<Vec<bool>> {
        if degree <= 64 {
            let context = degree.saturating_sub(2).min(7);
            let position = self.attribute_mask_pos[context];
            let low =
                u64::from(u32::try_from(*self.attribute_masks.small[context].get(position)?).ok()?);
            let mask = if context == 7 {
                let next = u64::from(
                    u32::try_from(*self.attribute_masks.context_7_next_30.get(position)?).ok()?,
                );
                let upper = u64::from(
                    u32::try_from(*self.attribute_masks.context_7_upper_4.get(position)?).ok()?,
                );
                if low >= 1_u64 << 30 || next >= 1_u64 << 30 || upper >= 1_u64 << 4 {
                    return None;
                }
                low | (next << 30) | (upper << 60)
            } else {
                low
            };
            if degree < 64 && mask >> degree != 0 {
                return None;
            }
            self.attribute_mask_pos[context] += 1;
            return Some((0..degree).map(|bit| mask & (1_u64 << bit) != 0).collect());
        }
        let word_count = degree.div_ceil(32);
        let end = self.large_mask_pos.checked_add(word_count)?;
        let words = self
            .attribute_masks
            .large_words
            .get(self.large_mask_pos..end)?;
        self.large_mask_pos = end;
        let mut mask = Vec::with_capacity(degree);
        for bit in 0..degree {
            let word = words[bit / 32] as u32;
            mask.push(word & (1_u32 << (bit % 32)) != 0);
        }
        if !degree.is_multiple_of(32) {
            let used = degree % 32;
            let last = *words.last()? as u32;
            if last >> used != 0 {
                return None;
            }
        }
        Some(mask)
    }

    fn exhausted(&self) -> bool {
        self.vertex_pos == self.valences.len()
            && self.vertex_pos == self.groups.len()
            && self.vertex_pos == self.flags.len()
            && self.split_pos == self.split_faces.len()
            && self.split_pos == self.split_positions.len()
            && self
                .degree_pos
                .iter()
                .zip(self.degrees)
                .all(|(&position, lane)| position == lane.len())
            && self
                .attribute_mask_pos
                .iter()
                .zip(self.attribute_masks.small)
                .all(|(&position, lane)| position == lane.len())
            && self.attribute_mask_pos[7] == self.attribute_masks.context_7_next_30.len()
            && self.attribute_mask_pos[7] == self.attribute_masks.context_7_upper_4.len()
            && self.large_mask_pos == self.attribute_masks.large_words.len()
    }
}

struct Decoder<'a> {
    symbols: Symbols<'a>,
    vertices: Vec<Vertex>,
    faces: Vec<Face>,
    active: Vec<usize>,
    removed: Vec<bool>,
    slot_count: usize,
    attribute_count: u32,
}

impl Decoder<'_> {
    fn new_vertex(&mut self) -> Option<usize> {
        let (valence, group, flags) = self.symbols.vertex()?;
        self.slot_count = self.slot_count.checked_add(valence)?;
        if self.slot_count > MAX_TOPOLOGY_SLOTS {
            return None;
        }
        let index = self.vertices.len();
        self.vertices.push(Vertex {
            faces: vec![None; valence],
            group,
            flags,
        });
        Some(index)
    }

    fn face_context(&self, vertex: usize) -> Option<usize> {
        let vertex = self.vertices.get(vertex)?;
        let known = vertex.faces.iter().flatten().count();
        let total = vertex
            .faces
            .iter()
            .flatten()
            .try_fold(0usize, |sum, &face| {
                sum.checked_add(self.faces.get(face)?.vertices.len())
            })?;
        Some(match vertex.faces.len() {
            3 if total < known * 6 => 0,
            3 if total == known * 6 => 1,
            3 => 2,
            4 if total < known * 4 => 3,
            4 if total == known * 4 => 4,
            4 => 5,
            5 => 6,
            _ => 7,
        })
    }

    fn set_vertex_face(&mut self, vertex: usize, slot: usize, face: usize) -> Option<()> {
        let target = self.vertices.get_mut(vertex)?.faces.get_mut(slot)?;
        if target.is_some_and(|existing| existing != face) {
            return None;
        }
        *target = Some(face);
        Some(())
    }

    fn set_face_vertex(&mut self, face: usize, slot: usize, vertex: usize) -> Option<()> {
        let face = self.faces.get_mut(face)?;
        let target = face.vertices.get_mut(slot)?;
        if target.is_some_and(|existing| existing != vertex) {
            return None;
        }
        if target.is_none() {
            face.empty = face.empty.checked_sub(1)?;
        }
        *target = Some(vertex);
        Some(())
    }

    fn add_vertex_to_face(
        &mut self,
        vertex: usize,
        vertex_face_slot: usize,
        face: usize,
        face_slot: usize,
    ) -> Option<()> {
        let degree = self.faces.get(face)?.vertices.len();
        if degree == 0 || face_slot >= degree {
            return None;
        }
        self.set_face_vertex(face, face_slot, vertex)?;
        let valence = self.vertices.get(vertex)?.faces.len();
        let clockwise = (face_slot + degree - 1) % degree;
        let counterclockwise = (face_slot + 1) % degree;
        if let Some(neighbor) = self.faces[face].vertices[clockwise] {
            let shared = self
                .vertices
                .get(neighbor)?
                .faces
                .iter()
                .position(|&v| v == Some(face))?;
            let slot = (vertex_face_slot + 1) % valence;
            if self.vertices[vertex].faces[slot].is_none() {
                let adjacent = (shared + self.vertices[neighbor].faces.len() - 1)
                    % self.vertices[neighbor].faces.len();
                if let Some(adjacent_face) = self.vertices[neighbor].faces[adjacent] {
                    self.set_vertex_face(vertex, slot, adjacent_face)?;
                }
            }
        }
        if let Some(neighbor) = self.faces[face].vertices[counterclockwise] {
            let shared = self
                .vertices
                .get(neighbor)?
                .faces
                .iter()
                .position(|&v| v == Some(face))?;
            let slot = (vertex_face_slot + valence - 1) % valence;
            if self.vertices[vertex].faces[slot].is_none() {
                let adjacent = (shared + 1) % self.vertices[neighbor].faces.len();
                if let Some(adjacent_face) = self.vertices[neighbor].faces[adjacent] {
                    self.set_vertex_face(vertex, slot, adjacent_face)?;
                }
            }
        }
        Some(())
    }

    fn activate_face(&mut self, vertex: usize, slot: usize) -> Option<usize> {
        let context = self.face_context(vertex)?;
        let degree = self.symbols.degree(context)?;
        if degree != 0 {
            let degree = usize::try_from(degree).ok()?;
            if degree == 0 || degree > MAX_TOPOLOGY_ITEMS {
                return None;
            }
            self.slot_count = self.slot_count.checked_add(degree)?;
            if self.slot_count > MAX_TOPOLOGY_SLOTS {
                return None;
            }
            let face = self.faces.len();
            let attribute_mask = self.symbols.attribute_mask(degree)?;
            let face_attribute_count =
                u32::try_from(attribute_mask.iter().filter(|&&bit| bit).count()).ok()?;
            let attribute_end = self.attribute_count.checked_add(face_attribute_count)?;
            self.faces.push(Face {
                vertices: vec![None; degree],
                empty: degree,
                attribute_mask,
                attributes: (self.attribute_count..attribute_end).collect(),
            });
            self.attribute_count = attribute_end;
            self.removed.push(false);
            self.set_vertex_face(vertex, slot, face)?;
            self.set_face_vertex(face, 0, vertex)?;
            self.active.push(face);
            return Some(face);
        }
        let (offset, face_slot) = self.symbols.split()?;
        let active_index = self.active.len().checked_sub(offset)?;
        let face = *self.active.get(active_index)?;
        self.set_vertex_face(vertex, slot, face)?;
        self.add_vertex_to_face(vertex, slot, face, face_slot)?;
        Some(face)
    }

    fn activate_vertex(&mut self, face: usize, face_slot: usize) -> Option<usize> {
        let vertex = self.new_vertex()?;
        self.set_vertex_face(vertex, 0, face)?;
        self.add_vertex_to_face(vertex, 0, face, face_slot)?;
        Some(vertex)
    }

    fn complete_vertex(&mut self, vertex: usize, vertex_slot_on_face: usize) -> Option<()> {
        let valence = self.vertices.get(vertex)?.faces.len();
        let mut previous_face = self.vertices[vertex].faces[0]?;
        let mut previous_slot = vertex_slot_on_face;
        let mut slot = 1usize;
        while slot < valence {
            let Some(next_face) = self.vertices[vertex].faces[slot] else {
                break;
            };
            let degree = self.faces.get(previous_face)?.vertices.len();
            previous_slot = (previous_slot + degree - 1) % degree;
            let Some(neighbor) = self.faces[previous_face].vertices[previous_slot] else {
                break;
            };
            let found = self
                .faces
                .get(next_face)?
                .vertices
                .iter()
                .position(|&v| v == Some(neighbor))?;
            let next_degree = self.faces[next_face].vertices.len();
            let next_slot = (found + next_degree - 1) % next_degree;
            self.add_vertex_to_face(vertex, slot, next_face, next_slot)?;
            previous_face = next_face;
            previous_slot = next_slot;
            slot += 1;
        }
        if slot == valence {
            return Some(());
        }
        let first_unresolved = slot;
        previous_face = self.vertices[vertex].faces[0]?;
        previous_slot = vertex_slot_on_face;
        slot = valence - 1;
        while slot >= first_unresolved {
            let Some(next_face) = self.vertices[vertex].faces[slot] else {
                break;
            };
            previous_slot = (previous_slot + 1) % self.faces.get(previous_face)?.vertices.len();
            let Some(neighbor) = self.faces[previous_face].vertices[previous_slot] else {
                break;
            };
            let found = self
                .faces
                .get(next_face)?
                .vertices
                .iter()
                .position(|&v| v == Some(neighbor))?;
            let next_slot = (found + 1) % self.faces[next_face].vertices.len();
            self.add_vertex_to_face(vertex, slot, next_face, next_slot)?;
            previous_face = next_face;
            previous_slot = next_slot;
            if slot == first_unresolved {
                return Some(());
            }
            slot -= 1;
        }
        for unresolved in first_unresolved..=slot {
            self.activate_face(vertex, unresolved)?;
        }
        Some(())
    }

    fn next_active_face(&mut self) -> Option<usize> {
        while self.active.last().is_some_and(|&face| self.removed[face]) {
            self.active.pop();
        }
        let mut best: Option<usize> = None;
        let mut index = self.active.len();
        while index > self.active.len().saturating_sub(16) {
            index -= 1;
            let face = self.active[index];
            if self.removed[face] {
                self.active.remove(index);
            } else if best.is_none_or(|current| self.faces[face].empty < self.faces[current].empty)
            {
                best = Some(face);
            }
        }
        best
    }

    fn run(mut self) -> Option<Vec<Polygon>> {
        while self.symbols.vertex_pos < self.symbols.valences.len() {
            let seed = self.new_vertex()?;
            for slot in 0..self.vertices[seed].faces.len() {
                self.activate_face(seed, slot)?;
            }
            while let Some(face) = self.next_active_face() {
                while let Some(slot) = self.faces[face].vertices.iter().position(Option::is_none) {
                    let vertex = self.activate_vertex(face, slot)?;
                    self.complete_vertex(vertex, slot)?;
                }
                self.removed[face] = true;
            }
        }
        if !self.symbols.exhausted()
            || self.faces.iter().any(|face| face.empty != 0)
            || self
                .vertices
                .iter()
                .any(|vertex| vertex.faces.iter().any(Option::is_none))
        {
            return None;
        }
        self.vertices
            .into_iter()
            .enumerate()
            .map(|(vertex_index, vertex)| {
                let attribute_indices = vertex
                    .faces
                    .iter()
                    .map(|&face| {
                        let face = &self.faces[face?];
                        if face.attributes.is_empty() {
                            return Some(None);
                        }
                        let vertex_slot = face
                            .vertices
                            .iter()
                            .position(|&candidate| candidate == Some(vertex_index))?;
                        let mut attribute_slot = face.attributes.len() - 1;
                        for slot in 0..=vertex_slot {
                            if face.attribute_mask[slot] {
                                attribute_slot = (attribute_slot + 1) % face.attributes.len();
                            }
                        }
                        Some(Some(face.attributes[attribute_slot]))
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(Polygon {
                    vertex_indices: vertex
                        .faces
                        .into_iter()
                        .map(|face| u32::try_from(face?).ok())
                        .collect::<Option<Vec<_>>>()?,
                    attribute_indices,
                    group: vertex.group,
                    flags: vertex.flags,
                })
            })
            .collect()
    }
}

/// Reconstruct polygon connectivity from the JT topological dual-mesh lanes.
pub(crate) fn decode(
    degrees: [&[i32]; 8],
    valences: &[i32],
    groups: &[i32],
    flags: &[i32],
    split_faces: &[i32],
    split_positions: &[i32],
    attribute_masks: AttributeMaskLanes<'_>,
) -> Option<Vec<Polygon>> {
    if valences.len() > MAX_TOPOLOGY_ITEMS
        || groups.len() != valences.len()
        || flags.len() != valences.len()
    {
        return None;
    }
    Decoder {
        symbols: Symbols {
            degrees,
            degree_pos: [0; 8],
            valences,
            groups,
            flags,
            split_faces,
            split_positions,
            attribute_masks,
            attribute_mask_pos: [0; 8],
            large_mask_pos: 0,
            vertex_pos: 0,
            split_pos: 0,
        },
        vertices: Vec::new(),
        faces: Vec::new(),
        active: Vec::new(),
        removed: Vec::new(),
        slot_count: 0,
        attribute_count: 0,
    }
    .run()
}
