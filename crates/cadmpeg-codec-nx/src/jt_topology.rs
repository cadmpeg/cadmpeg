// SPDX-License-Identifier: Apache-2.0
//! Topological dual-mesh reconstruction for embedded JT display models.

const MAX_TOPOLOGY_ITEMS: usize = 1_000_000;
const MAX_TOPOLOGY_SLOTS: usize = 8_000_000;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::num::NonZeroUsize;

mod face_slots;
use face_slots::FaceSlots;

/// Decoded polygon in topological-vertex visit order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Polygon {
    pub(crate) corners: Vec<(u32, Option<u32>)>,
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
    vertices: FaceSlots,
    attribute_mask: Vec<bool>,
    attributes: Vec<u32>,
}

/// One of the eight face-attribute-mask contexts a dual face consumes its mask
/// from. A face of degree `d` takes context `min(7, max(0, d - 2))`: degrees one
/// and two occur at non-manifold display seams and share context zero, degrees
/// three through nine take contexts one through seven in turn, and every higher
/// degree shares context seven.
#[derive(Clone, Copy, PartialEq, Eq)]
struct AttributeMaskContext(u8);

impl AttributeMaskContext {
    /// The context whose mask spans three lanes: a low 30-bit lane, the next
    /// 30 bits, and the upper four bits.
    const COMBINED: Self = Self(7);

    fn of(degree: NonZeroUsize) -> Option<Self> {
        Some(Self(match degree.get() {
            1 | 2 => 0,
            3..=9 => u8::try_from(degree.get()).ok()? - 2,
            _ => Self::COMBINED.0,
        }))
    }

    fn lane(self) -> usize {
        usize::from(self.0)
    }
}

/// Attribute-mask symbol lanes consumed while faces are created.
pub(crate) struct AttributeMaskLanes<'a> {
    pub(crate) small: [&'a [i32]; 8],
    pub(crate) context_7_next_30: &'a [i32],
    pub(crate) context_7_upper_4: &'a [i32],
    pub(crate) large_words: &'a [i32],
}

/// Split-face symbol lanes consumed while reconstructing connectivity.
#[derive(Clone, Copy)]
pub(crate) struct SplitLanes<'a> {
    pub(crate) faces: &'a [i32],
    pub(crate) positions: &'a [i32],
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

    fn attribute_mask(
        &mut self,
        ctx: &DecodeContext<'_>,
        degree: NonZeroUsize,
    ) -> Result<Option<Vec<bool>>, CodecError> {
        let Some(context) = AttributeMaskContext::of(degree) else {
            return Ok(None);
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(degree.get()),
            "form JT face attribute mask",
        )?;
        let lane = context.lane();
        let degree = degree.get();
        if degree <= 64 {
            let position = self.attribute_mask_pos[lane];
            let Some(mask) = (|| -> Option<u64> {
                let low = u64::from(
                    u32::try_from(*self.attribute_masks.small[lane].get(position)?).ok()?,
                );
                let mask = if context == AttributeMaskContext::COMBINED {
                    let next = u64::from(
                        u32::try_from(*self.attribute_masks.context_7_next_30.get(position)?)
                            .ok()?,
                    );
                    let upper = u64::from(
                        u32::try_from(*self.attribute_masks.context_7_upper_4.get(position)?)
                            .ok()?,
                    );
                    if low >= 1_u64 << 30 || next >= 1_u64 << 30 || upper >= 1_u64 << 4 {
                        return None;
                    }
                    low | (next << 30) | (upper << 60)
                } else {
                    low
                };
                (degree == 64 || mask >> degree == 0).then_some(mask)
            })() else {
                return Ok(None);
            };
            self.attribute_mask_pos[lane] += 1;
            let mut result = ctx.alloc_filled(degree, false, "nx JT face attribute mask")?;
            for (bit, target) in result.iter_mut().enumerate() {
                *target = mask & (1_u64 << bit) != 0;
            }
            return Ok(Some(result));
        }
        let word_count = degree.div_ceil(32);
        let Some(end) = self.large_mask_pos.checked_add(word_count) else {
            return Ok(None);
        };
        let Some(words) = self
            .attribute_masks
            .large_words
            .get(self.large_mask_pos..end)
        else {
            return Ok(None);
        };
        self.large_mask_pos = end;
        if !degree.is_multiple_of(32) {
            let used = degree % 32;
            let Some(last) = words.last() else {
                return Ok(None);
            };
            let last = last.cast_unsigned();
            if last >> used != 0 {
                return Ok(None);
            }
        }
        let mut mask = ctx.alloc_filled(degree, false, "nx JT high-degree face attribute mask")?;
        for (bit, target) in mask.iter_mut().enumerate() {
            let word = words[bit / 32].cast_unsigned();
            *target = word & (1_u32 << (bit % 32)) != 0;
        }
        Ok(Some(mask))
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
    fn new_vertex(&mut self, ctx: &DecodeContext<'_>) -> Result<Option<usize>, CodecError> {
        let Some((valence, group, flags)) = self.symbols.vertex() else {
            return Ok(None);
        };
        let Some(slot_count) = self.slot_count.checked_add(valence) else {
            return Ok(None);
        };
        self.slot_count = slot_count;
        if self.slot_count > MAX_TOPOLOGY_SLOTS {
            return Ok(None);
        }
        let faces = ctx.alloc_filled(valence, None, "nx JT vertex face slots")?;
        ctx.charge_collection_items(1, "nx JT topology vertices")?;
        let index = self.vertices.len();
        ctx.reserve_capacity(&mut self.vertices, 1, "nx JT topology vertices")?;
        self.vertices.push(Vertex {
            faces,
            group,
            flags,
        });
        Ok(Some(index))
    }

    fn face_context(
        &self,
        ctx: &DecodeContext<'_>,
        vertex: usize,
    ) -> Result<Option<usize>, CodecError> {
        let decoded: Option<Result<_, CodecError>> = (|| {
            let vertex = self.vertices.get(vertex)?;
            propagate_resource!(ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(vertex.faces.len()),
                "scan JT vertex face context"
            ));
            let (known, total) = vertex.faces.iter().flatten().try_fold(
                (0usize, 0usize),
                |(known, sum), &face| {
                    Some((
                        known.checked_add(1)?,
                        sum.checked_add(self.faces.get(face)?.vertices.len())?,
                    ))
                },
            )?;
            Some(Ok(match vertex.faces.len() {
                3 if total < known * 6 => 0,
                3 if total == known * 6 => 1,
                3 => 2,
                4 if total < known * 4 => 3,
                4 if total == known * 4 => 4,
                4 => 5,
                5 => 6,
                _ => 7,
            }))
        })();
        decoded.transpose()
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
        self.faces.get_mut(face)?.vertices.fill(slot, vertex)
    }

    fn add_vertex_to_face(
        &mut self,
        ctx: &DecodeContext<'_>,
        vertex: usize,
        vertex_face_slot: usize,
        face: usize,
        face_slot: usize,
    ) -> Result<Option<()>, CodecError> {
        let decoded: Option<Result<_, CodecError>> = (|| {
            let degree = self.faces.get(face)?.vertices.len();
            if degree == 0 || face_slot >= degree {
                return None;
            }
            self.set_face_vertex(face, face_slot, vertex)?;
            let valence = self.vertices.get(vertex)?.faces.len();
            let clockwise = (face_slot + degree - 1) % degree;
            let counterclockwise = (face_slot + 1) % degree;
            if let Some(neighbor) = self.faces[face].vertices[clockwise] {
                propagate_resource!(ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(self.vertices.get(neighbor)?.faces.len()),
                    "scan JT neighboring face ring"
                ));
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
                propagate_resource!(ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(self.vertices.get(neighbor)?.faces.len()),
                    "scan JT neighboring face ring"
                ));
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
            Some(Ok(()))
        })();
        decoded.transpose()
    }

    fn activate_face(
        &mut self,
        ctx: &DecodeContext<'_>,
        vertex: usize,
        slot: usize,
    ) -> Result<Option<usize>, CodecError> {
        let Some(context) = self.face_context(ctx, vertex)? else {
            return Ok(None);
        };
        let Some(degree) = self.symbols.degree(context) else {
            return Ok(None);
        };
        if degree != 0 {
            let Some(degree) = usize::try_from(degree).ok().and_then(NonZeroUsize::new) else {
                return Ok(None);
            };
            if degree.get() > MAX_TOPOLOGY_ITEMS {
                return Ok(None);
            }
            let Some(slot_count) = self.slot_count.checked_add(degree.get()) else {
                return Ok(None);
            };
            self.slot_count = slot_count;
            if self.slot_count > MAX_TOPOLOGY_SLOTS {
                return Ok(None);
            }
            let face = self.faces.len();
            let Some(attribute_mask) = self.symbols.attribute_mask(ctx, degree)? else {
                return Ok(None);
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(attribute_mask.len()),
                "count JT face attributes",
            )?;
            let Some(face_attribute_count) =
                u32::try_from(attribute_mask.iter().filter(|&&bit| bit).count()).ok()
            else {
                return Ok(None);
            };
            let Some(attribute_end) = self.attribute_count.checked_add(face_attribute_count) else {
                return Ok(None);
            };
            let Some(attribute_len) = usize::try_from(face_attribute_count).ok() else {
                return Ok(None);
            };
            let mut attributes = ctx.collection_vec(attribute_len, "nx JT face attributes")?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(attribute_len),
                "form JT face attributes",
            )?;
            attributes.extend(self.attribute_count..attribute_end);
            let vertices = FaceSlots::new(ctx, degree.get())?;
            ctx.reserve_vec(&mut self.faces, 1, "nx JT topology faces")?;
            ctx.reserve_vec(&mut self.removed, 1, "nx JT removed faces")?;
            ctx.reserve_vec(&mut self.active, 1, "nx JT active faces")?;
            self.faces.push(Face {
                vertices,
                attribute_mask,
                attributes,
            });
            self.attribute_count = attribute_end;
            self.removed.push(false);
            if self.set_vertex_face(vertex, slot, face).is_none()
                || self.set_face_vertex(face, 0, vertex).is_none()
            {
                return Ok(None);
            }
            self.active.push(face);
            return Ok(Some(face));
        }
        let Some((offset, face_slot)) = self.symbols.split() else {
            return Ok(None);
        };
        let Some(active_index) = self.active.len().checked_sub(offset) else {
            return Ok(None);
        };
        let Some(&face) = self.active.get(active_index) else {
            return Ok(None);
        };
        if self.set_vertex_face(vertex, slot, face).is_none()
            || self
                .add_vertex_to_face(ctx, vertex, slot, face, face_slot)?
                .is_none()
        {
            return Ok(None);
        }
        Ok(Some(face))
    }

    fn activate_vertex(
        &mut self,
        ctx: &DecodeContext<'_>,
        face: usize,
        face_slot: usize,
    ) -> Result<Option<usize>, CodecError> {
        let Some(vertex) = self.new_vertex(ctx)? else {
            return Ok(None);
        };
        if self.set_vertex_face(vertex, 0, face).is_none()
            || self
                .add_vertex_to_face(ctx, vertex, 0, face, face_slot)?
                .is_none()
        {
            return Ok(None);
        }
        Ok(Some(vertex))
    }

    fn complete_vertex(
        &mut self,
        ctx: &DecodeContext<'_>,
        vertex: usize,
        vertex_slot_on_face: usize,
    ) -> Result<Option<()>, CodecError> {
        let Some(vertex_faces) = self.vertices.get(vertex).map(|vertex| &vertex.faces) else {
            return Ok(None);
        };
        let valence = vertex_faces.len();
        let Some(&Some(mut previous_face)) = vertex_faces.first() else {
            return Ok(None);
        };
        let mut previous_slot = vertex_slot_on_face;
        let mut slot = 1usize;
        while slot < valence {
            ctx.charge_work(1, "walk JT vertex ring")?;
            let Some(next_face) = self.vertices[vertex].faces[slot] else {
                break;
            };
            let Some(degree) = self
                .faces
                .get(previous_face)
                .map(|face| face.vertices.len())
            else {
                return Ok(None);
            };
            previous_slot = (previous_slot + degree - 1) % degree;
            let Some(neighbor) = self.faces[previous_face].vertices[previous_slot] else {
                break;
            };
            let Some(next) = self.faces.get(next_face) else {
                return Ok(None);
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(next.vertices.len()),
                "scan JT next face ring",
            )?;
            let Some(found) = self
                .faces
                .get(next_face)
                .and_then(|face| face.vertices.iter().position(|&v| v == Some(neighbor)))
            else {
                return Ok(None);
            };
            let next_degree = self.faces[next_face].vertices.len();
            let next_slot = (found + next_degree - 1) % next_degree;
            if self
                .add_vertex_to_face(ctx, vertex, slot, next_face, next_slot)?
                .is_none()
            {
                return Ok(None);
            }
            previous_face = next_face;
            previous_slot = next_slot;
            slot += 1;
        }
        if slot == valence {
            return Ok(Some(()));
        }
        let first_unresolved = slot;
        let Some(first_face) = self.vertices[vertex].faces[0] else {
            return Ok(None);
        };
        previous_face = first_face;
        previous_slot = vertex_slot_on_face;
        slot = valence - 1;
        while slot >= first_unresolved {
            ctx.charge_work(1, "walk JT vertex ring")?;
            let Some(next_face) = self.vertices[vertex].faces[slot] else {
                break;
            };
            let Some(degree) = self
                .faces
                .get(previous_face)
                .map(|face| face.vertices.len())
            else {
                return Ok(None);
            };
            previous_slot = (previous_slot + 1) % degree;
            let Some(neighbor) = self.faces[previous_face].vertices[previous_slot] else {
                break;
            };
            let Some(next) = self.faces.get(next_face) else {
                return Ok(None);
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(next.vertices.len()),
                "scan JT next face ring",
            )?;
            let Some(found) = self
                .faces
                .get(next_face)
                .and_then(|face| face.vertices.iter().position(|&v| v == Some(neighbor)))
            else {
                return Ok(None);
            };
            let next_slot = (found + 1) % self.faces[next_face].vertices.len();
            if self
                .add_vertex_to_face(ctx, vertex, slot, next_face, next_slot)?
                .is_none()
            {
                return Ok(None);
            }
            previous_face = next_face;
            previous_slot = next_slot;
            if slot == first_unresolved {
                return Ok(Some(()));
            }
            slot -= 1;
        }
        for unresolved in first_unresolved..=slot {
            if self.activate_face(ctx, vertex, unresolved)?.is_none() {
                return Ok(None);
            }
        }
        Ok(Some(()))
    }

    fn next_active_face(&mut self, ctx: &DecodeContext<'_>) -> Result<Option<usize>, CodecError> {
        // At most sixteen removals shift the active lane, plus the suffix walk.
        let work = self.active.len().checked_mul(17).ok_or_else(|| {
            ctx.refuse_codec_limit("select JT active face", u64::MAX - 1, u64::MAX)
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(work),
            "select JT active face",
        )?;
        while self.active.last().is_some_and(|&face| self.removed[face]) {
            self.active.pop();
        }
        let mut best: Option<usize> = None;
        let mut index = self.active.len();
        while index > 0 {
            let Some(distance) = self.active.len().checked_sub(index) else {
                return Ok(best);
            };
            if distance >= 16 {
                break;
            }
            index -= 1;
            let face = self.active[index];
            if self.removed[face] {
                self.active.remove(index);
            } else if best.is_none_or(|current| {
                self.faces[face].vertices.empty() < self.faces[current].vertices.empty()
            }) {
                best = Some(face);
            }
        }
        Ok(best)
    }

    fn run(mut self, ctx: &DecodeContext<'_>) -> Result<Option<Vec<Polygon>>, CodecError> {
        while self.symbols.vertex_pos < self.symbols.valences.len() {
            let Some(seed) = self.new_vertex(ctx)? else {
                return Ok(None);
            };
            for slot in 0..self.vertices[seed].faces.len() {
                if self.activate_face(ctx, seed, slot)?.is_none() {
                    return Ok(None);
                }
            }
            while let Some(face) = self.next_active_face(ctx)? {
                loop {
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(self.faces[face].vertices.len()),
                        "scan JT unfilled face slots",
                    )?;
                    let Some(slot) = self.faces[face].vertices.iter().position(Option::is_none)
                    else {
                        break;
                    };
                    let Some(vertex) = self.activate_vertex(ctx, face, slot)? else {
                        return Ok(None);
                    };
                    if self.complete_vertex(ctx, vertex, slot)?.is_none() {
                        return Ok(None);
                    }
                }
                self.removed[face] = true;
            }
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(self.slot_count),
            "validate JT reconstructed rings",
        )?;
        if !self.symbols.exhausted()
            || self.faces.iter().any(|face| face.vertices.empty() != 0)
            || self
                .vertices
                .iter()
                .any(|vertex| vertex.faces.iter().any(Option::is_none))
        {
            return Ok(None);
        }
        let mut polygons = ctx.collection_vec(self.vertices.len(), "nx JT output polygons")?;
        for (vertex_index, vertex) in self.vertices.into_iter().enumerate() {
            let mut corners = ctx.collection_vec(vertex.faces.len(), "nx JT polygon corners")?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(vertex.faces.len()),
                "form JT polygon corners",
            )?;
            for face_index in vertex.faces {
                let Some(face_index) = face_index else {
                    return Ok(None);
                };
                let Some(face) = self.faces.get(face_index) else {
                    return Ok(None);
                };
                let attribute = if face.attributes.is_empty() {
                    None
                } else {
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(face.vertices.len()),
                        "scan JT polygon attribute ring",
                    )?;
                    let Some(vertex_slot) = face
                        .vertices
                        .iter()
                        .position(|&candidate| candidate == Some(vertex_index))
                    else {
                        return Ok(None);
                    };
                    let mut attribute_slot = face.attributes.len() - 1;
                    ctx.charge_work(
                        cadmpeg_core::decode::u64_from_index(vertex_slot + 1),
                        "scan JT polygon attribute mask",
                    )?;
                    for slot in 0..=vertex_slot {
                        if face.attribute_mask[slot] {
                            attribute_slot = (attribute_slot + 1) % face.attributes.len();
                        }
                    }
                    Some(face.attributes[attribute_slot])
                };
                let Ok(face_index) = u32::try_from(face_index) else {
                    return Ok(None);
                };
                corners.push((face_index, attribute));
            }
            polygons.push(Polygon {
                corners,
                group: vertex.group,
                flags: vertex.flags,
            });
        }
        Ok(Some(polygons))
    }
}

/// Reconstruct polygon connectivity from the JT topological dual-mesh lanes.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    degrees: [&[i32]; 8],
    valences: &[i32],
    groups: &[i32],
    flags: &[i32],
    split: SplitLanes<'_>,
    attribute_masks: AttributeMaskLanes<'_>,
) -> Result<Option<Vec<Polygon>>, CodecError> {
    if valences.len() > MAX_TOPOLOGY_ITEMS
        || groups.len() != valences.len()
        || flags.len() != valences.len()
    {
        return Ok(None);
    }
    Decoder {
        symbols: Symbols {
            degrees,
            degree_pos: [0; 8],
            valences,
            groups,
            flags,
            split_faces: split.faces,
            split_positions: split.positions,
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
    .run(ctx)
}

#[cfg(test)]
mod tests;
