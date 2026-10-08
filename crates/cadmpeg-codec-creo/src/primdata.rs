// SPDX-License-Identifier: Apache-2.0
//! Typed geometry arrays from expanded `SolidPrimdata` sections.

use crate::{psb, scalar};
use cadmpeg_core::decode::{index_from_u32, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;

/// Named scalar-array field in a primitive record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrimitiveArrayField {
    /// First primitive point.
    P1,
    /// Second primitive point.
    P2,
    /// Primitive points.
    Points,
    /// Consecutive vertex positions.
    VertexPositions,
    /// Interleaved vertex normals and positions.
    VertexNormalsAndPositions,
}

impl PrimitiveArrayField {
    /// Stored field spelling.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::P1 => "p1",
            Self::P2 => "p2",
            Self::Points => "pts",
            Self::VertexPositions => "mv_p_xyz",
            Self::VertexNormalsAndPositions => "mv_p_NxNyNzxyz",
        }
    }
}

/// One bounded, named scalar array in a primitive record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrimitiveScalarArray {
    /// Named field containing the array.
    pub(crate) field: PrimitiveArrayField,
    /// Byte offset of the named-record header in the expanded section.
    pub(crate) offset: usize,
    /// Completely decoded scalar values.
    pub(crate) values: Vec<FiniteReal>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrimitiveShadedVertex {
    pub(crate) position: FiniteVector<3>,
    pub(crate) normal: FiniteVector<3>,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrimitiveVertices {
    Unshaded(Vec<FiniteVector<3>>),
    Shaded(Vec<PrimitiveShadedVertex>),
}

/// A strip set with complete vertex lanes and exact, nonempty span coverage.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrimitiveTriangleStrip {
    pub(crate) offset: usize,
    vertices: PrimitiveVertices,
    strip_lengths: Vec<u32>,
}
impl PrimitiveTriangleStrip {
    #[cfg(test)]
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        offset: usize,
        positions: Vec<FiniteVector<3>>,
        normals: Option<Vec<FiniteVector<3>>>,
        strip_lengths: Vec<u32>,
    ) -> Result<Option<Self>, CodecError> {
        let mut total = 0usize;
        let mut lengths = strip_lengths.iter();
        while let Some(&length) = ctx.next_charged(&mut lengths, "creo primitive strip validation")? {
            if length < 3 { return Ok(None); }
            let Some(next) = usize::try_from(length).ok().and_then(|length| total.checked_add(length)) else { return Ok(None); };
            total = next;
        }
        if strip_lengths.is_empty()
            || total != positions.len()
            || normals
                .as_ref()
                .is_some_and(|normals| normals.len() != positions.len())
        {
            return Ok(None);
        }
        let vertices = match normals {
            None => PrimitiveVertices::Unshaded(positions),
            Some(normals) => {
                PrimitiveVertices::Shaded(
                    ctx.collect_retained_vec(
                        positions
                            .into_iter()
                            .zip(normals)
                            .map(|(position, normal)| PrimitiveShadedVertex { position, normal }),
                        "creo primitive shaded vertices",
                    )?,
                )
            }
        };
        Ok(Some(Self {
            offset,
            vertices,
            strip_lengths,
        }))
    }
    #[cfg(test)]
    pub(crate) fn positions(&self) -> impl ExactSizeIterator<Item = &FiniteVector<3>> {
        let count = match &self.vertices {
            PrimitiveVertices::Unshaded(rows) => rows.len(),
            PrimitiveVertices::Shaded(rows) => rows.len(),
        };
        (0..count).map(|index| match &self.vertices {
            PrimitiveVertices::Unshaded(rows) => &rows[index],
            PrimitiveVertices::Shaded(rows) => &rows[index].position,
        })
    }
    #[cfg(test)]
    pub(crate) fn normals(&self) -> Option<impl ExactSizeIterator<Item = &FiniteVector<3>>> {
        match &self.vertices {
            PrimitiveVertices::Unshaded(_) => None,
            PrimitiveVertices::Shaded(rows) => Some(rows.iter().map(|row| &row.normal)),
        }
    }
    /// Returns the complete stored vertex lane without copying its rows.
    pub(crate) fn vertices(&self) -> &PrimitiveVertices {
        &self.vertices
    }
    pub(crate) fn strip_lengths(&self) -> &[u32] {
        &self.strip_lengths
    }
}

/// Complete triangle strips and conflicts found in one primitive-data stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrimitiveTriangleStripScan {
    /// Triangle-strip records whose cumulative counts and geometry agree.
    pub(crate) strips: Vec<PrimitiveTriangleStrip>,
    /// Records with complete position or normal representations that disagree.
    pub(crate) conflicting_representation_count: usize,
}

#[derive(Debug)]
enum TriangleStripGeometryError {
    Missing,
    Conflicting,
    Resource(CodecError),
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
struct TriangleStripGeometry {
    positions: Vec<FiniteVector<3>>,
    normals: Option<Vec<FiniteVector<3>>>,
}

impl From<CodecError> for TriangleStripGeometryError {
    fn from(error: CodecError) -> Self { Self::Resource(error) }
}

type VertexLane<'a> = (&'a [FiniteReal], usize, usize);

fn vertex_lanes_equal(ctx: &DecodeContext<'_>, left: VertexLane<'_>, right: VertexLane<'_>) -> Result<bool, CodecError> {
    let (left, left_width, left_start) = left;
    let (right, right_width, right_start) = right;
    ctx.all_by(left.chunks_exact(left_width).zip(right.chunks_exact(right_width)), |(left, right)| {
        Ok(left[left_start..left_start + 3] == right[right_start..right_start + 3])
    }, "creo triangle strip representation comparison")
}

fn project_vertex_lane(ctx: &DecodeContext<'_>, lane: VertexLane<'_>, operation: &'static str) -> Result<Vec<FiniteVector<3>>, CodecError> {
    let (values, width, start) = lane;
    let mut points = ctx.collection_vec(values.len() / width, operation)?;
    for tuple in ctx.admit_iter(0..values.len() / width, operation)? {
        let offset = tuple * width + start;
        points.push(FiniteVector::from([values[offset], values[offset + 1], values[offset + 2]]));
    }
    Ok(points)
}

fn select_triangle_strip_lanes<'a>(
    ctx: &DecodeContext<'_>, arrays: &'a [PrimitiveScalarArray], vertex_count: u32,
) -> Result<(VertexLane<'a>, Option<VertexLane<'a>>), TriangleStripGeometryError> {
    let vertex_count = usize::try_from(vertex_count).map_err(|_| TriangleStripGeometryError::Missing)?;
    let mut positions = None;
    let mut normals = None;
    let mut candidates = arrays.iter();
    while let Some(array) = ctx.next_charged(&mut candidates, "creo triangle strip array rows")? {
        let (width, position_start) = match array.field {
            PrimitiveArrayField::VertexPositions => (3, 0),
            PrimitiveArrayField::VertexNormalsAndPositions => (6, 3),
            PrimitiveArrayField::P1 | PrimitiveArrayField::P2 | PrimitiveArrayField::Points => continue,
        };
        if array.values.len() != vertex_count.checked_mul(width).ok_or(TriangleStripGeometryError::Missing)? { continue; }
        let candidate = (array.values.as_slice(), width, position_start);
        if let Some(selected) = positions {
            if !vertex_lanes_equal(ctx, selected, candidate)? { return Err(TriangleStripGeometryError::Conflicting); }
        } else { positions = Some(candidate); }
        if width == 6 {
            let candidate = (array.values.as_slice(), width, 0);
            if let Some(selected) = normals {
                if !vertex_lanes_equal(ctx, selected, candidate)? { return Err(TriangleStripGeometryError::Conflicting); }
            } else { normals = Some(candidate); }
        }
    }
    let positions = positions.ok_or(TriangleStripGeometryError::Missing)?;
    Ok((positions, normals))
}

#[cfg(test)]
fn triangle_strip_geometry(
    ctx: &DecodeContext<'_>, arrays: &[PrimitiveScalarArray], vertex_count: u32,
) -> Result<TriangleStripGeometry, TriangleStripGeometryError> {
    let (positions, normals) = select_triangle_strip_lanes(ctx, arrays, vertex_count)?;
    Ok(TriangleStripGeometry {
        positions: project_vertex_lane(ctx, positions, "creo triangle strip positions")?,
        normals: normals.map(|lane| project_vertex_lane(ctx, lane, "creo triangle strip normals")).transpose()?,
    })
}

fn triangle_strip_vertices(
    ctx: &DecodeContext<'_>, positions: VertexLane<'_>, normals: Option<VertexLane<'_>>,
) -> Result<PrimitiveVertices, CodecError> {
    let Some(normals) = normals else {
        return Ok(PrimitiveVertices::Unshaded(project_vertex_lane(ctx, positions, "creo triangle strip positions")?));
    };
    let count = positions.0.len() / positions.1;
    let mut vertices = ctx.collection_vec(count, "creo primitive shaded vertices")?;
    for index in ctx.admit_iter(0..count, "creo primitive shaded vertex projection")? {
        let position = index * positions.1 + positions.2;
        let normal = index * normals.1 + normals.2;
        vertices.push(PrimitiveShadedVertex {
            position: FiniteVector::from([positions.0[position], positions.0[position + 1], positions.0[position + 2]]),
            normal: FiniteVector::from([normals.0[normal], normals.0[normal + 1], normals.0[normal + 2]]),
        });
    }
    Ok(PrimitiveVertices::Shaded(vertices))
}

/// Decode named triangle-strip primitives and representation conflicts.
pub(crate) fn triangle_strips(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<PrimitiveTriangleStripScan, CodecError> {
    const RECORD: &[u8] = b"value(prim_tristripsetwithatt)\0";
    const ACCUM: &[u8] = b"\xe0\x01p_accum_set_size\0";
    let mut strips = Vec::new();
    let mut conflicting_representation_count = 0usize;
    for offset in ctx.find_bytes_iter(data, RECORD, "creo primitive strip discovery")? {
        let start = offset + RECORD.len();
        let end = ctx.find_map(data.get(start..).unwrap_or_default().windows(b"\xe0\x00value(".len()).enumerate(), |(offset, bytes)| Ok((bytes == b"\xe0\x00value(").then_some(start + offset)), "creo primitive strip boundary scan")?.unwrap_or(data.len());
        let record = &data[offset..end];
        let Some(accum) = ctx.find_map(record.windows(ACCUM.len()).enumerate(), |(offset, bytes)| Ok((bytes == ACCUM).then_some(offset)), "creo primitive cumulative label scan")?.map(|relative| relative + ACCUM.len()) else { continue; };
        if record.get(accum) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (count, mut cursor) = psb::compact_int(record, accum + 1);
        if cadmpeg_core::decode::bounded_len(u64::from(count), 1, record.len() - cursor).is_none() {
            continue;
        }
        let (mut cumulative, _cumulative_scope) = ctx.temporary_vec(index_from_u32(count), "creo triangle strip cumulative counts")?;
        let mut slots = 0..count;
        while ctx.next_charged(&mut slots, "creo primitive cumulative parsing")?.is_some() {
            let (value, next) = psb::compact_int(record, cursor);
            if next == cursor {
                cumulative.clear();
                break;
            }
            cumulative.push(value);
            cursor = next;
        }
        let Some(vertex_count) = cumulative.last().copied() else {
            continue;
        };
        let mut previous = 0;
        let (mut strip_lengths, _lengths_scope) = ctx.temporary_vec(
            cumulative.len(), "creo triangle strip lengths",
        )?;
        let mut counts = cumulative.into_iter();
        while let Some(current) = ctx.next_charged(&mut counts, "creo primitive strip length construction")? {
            let Some(length) = current.checked_sub(previous).filter(|length| *length >= 3) else {
                strip_lengths.clear();
                break;
            };
            strip_lengths.push(length);
            previous = current;
        }
        if strip_lengths.is_empty() {
            continue;
        }
        let mut arrays_scope = ctx.reserve_scoped(0, "creo primitive geometry arrays")?;
        let arrays = arrays_scope.with_storage(|| scalar_arrays(ctx, record))?;
        let (positions, normals) = match select_triangle_strip_lanes(ctx, &arrays, vertex_count) {
            Ok(geometry) => geometry,
            Err(TriangleStripGeometryError::Missing) => continue,
            Err(TriangleStripGeometryError::Conflicting) => {
                conflicting_representation_count += 1;
                continue;
            }
            Err(TriangleStripGeometryError::Resource(error)) => return Err(error),
        };
        ctx.reserve_vec(&mut strips, 1, "creo triangle strip records")?;
        let vertices = triangle_strip_vertices(ctx, positions, normals)?;
        let strip_lengths = ctx.collect_retained_vec(strip_lengths.iter().copied(), "creo triangle strip retained lengths")?;
        strips.push(PrimitiveTriangleStrip { offset, vertices, strip_lengths });
    }
    Ok(PrimitiveTriangleStripScan {
        strips,
        conflicting_representation_count,
    })
}

/// Decode model-space scalar arrays from an expanded primitive-data section.
///
/// Primitive coordinates use a float32 lane distinct from the float64 lanes
/// in analytic geometry records. `00` is zero, `00 28 00` is a three-slot
/// positive-Y unit vector, and signed four-byte values replace the IEEE-754
/// high byte with a compact exponent byte. Only complete arrays whose declared
/// count is satisfied are returned.
pub(crate) fn scalar_arrays(
    ctx: &DecodeContext<'_>,
    data: &[u8],
) -> Result<Vec<PrimitiveScalarArray>, CodecError> {
    const FIELDS: [PrimitiveArrayField; 5] = [
        PrimitiveArrayField::P1,
        PrimitiveArrayField::P2,
        PrimitiveArrayField::Points,
        PrimitiveArrayField::VertexPositions,
        PrimitiveArrayField::VertexNormalsAndPositions,
    ];
    let mut arrays = Vec::new();
    for field in FIELDS {
        let name = field.as_str().as_bytes();
        let marker_len = name.len() + 3;
        let windows = data.windows(marker_len);
        for (offset, window) in ctx.admit_iter(0..windows.len(), "creo primitive scalar discovery")?.map(|offset| (offset, &data[offset..offset + marker_len])) {
            if !(window[0] == psb::token::NAMED_RECORD && window[1] == 0x06 && window[2..2 + name.len()] == *name && window[marker_len - 1] == 0) { continue; }
            let opener = offset + marker_len;
            if data.get(opener) != Some(&psb::token::ARRAY_OPEN) {
                continue;
            }
            let (count, start) = psb::compact_int(data, opener + 1);
            if start == opener + 1 {
                continue;
            }
            let Some(capacity) =
                cadmpeg_core::decode::bounded_len(u64::from(count), 1, data.len() - start)
            else {
                continue;
            };
            let (mut values, _values_scope) = ctx.temporary_vec(capacity, "creo primitive scalar values")?;
            let mut cursor = psb::Cursor::at(data, start);
            let mut steps = 0..capacity;
            while values.len() < capacity && ctx.next_charged(&mut steps, "creo primitive scalar parsing")?.is_some() {
                if capacity - values.len() >= 3 && cursor.take_slice_if(&[0x00, 0x28, 0x00]) {
                    values.extend([FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ZERO]);
                    continue;
                }
                let Some(value) = cursor.take_with(primitive_scalar) else {
                    break;
                };
                let Some(value) = FiniteReal::new(value) else {
                    break;
                };
                values.push(value);
            }
            if values.len() == capacity {
                ctx.reserve_vec(&mut arrays, 1, "creo primitive scalar arrays")?;
                arrays.push(PrimitiveScalarArray {
                    field,
                    offset,
                    values: ctx.collect_retained_vec(values.iter().copied(), "creo primitive retained scalar values")?,
                });
            }
        }
    }
    ctx.stable_sort_by(
        &mut arrays,
        |value| &value.offset,
        Ord::cmp,
        "creo primitive scalar array ordering",
    )?;
    Ok(arrays)
}

fn primitive_scalar(data: &[u8], offset: usize) -> Option<(f64, usize)> {
    match data.get(offset..)? {
        [0x00, ..] => Some((0.0, offset + 1)),
        [head @ (0x36..=0x3d | 0x46..=0x4d), b1, b2, b3, ..] => {
            let ieee_high = if *head >= 0x46 {
                head.checked_sub(7)?
            } else {
                head.checked_add(0x89)?
            };
            // Compact exponent byte is remapped into the IEEE high byte; the
            // four-byte argument is assembled, not a contiguous in-order window.
            let value = f64::from(scalar::be_f32([ieee_high, *b1, *b2, *b3]));
            Some((value, offset + 4))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        scalar_arrays, triangle_strip_geometry, triangle_strips, PrimitiveArrayField,
        PrimitiveScalarArray, TriangleStripGeometry, TriangleStripGeometryError,
    };
    use cadmpeg_ir::scalar::FiniteReal;
    use cadmpeg_ir::units::FiniteVector;

    fn with_context<T>(
        bytes: &[u8],
        f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
    ) -> T {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .expect("the primitive fixture fits the root limit");
        f(&ctx)
    }

    fn with_collection_limit<T>(
        bytes: &[u8],
        limit: u64,
        f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
    ) -> Result<T, cadmpeg_core::CodecError> {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
            .expect("the primitive fixture fits the root limit");
        f(&ctx)
    }

    #[test]
    fn primitive_miss_searches_refuse_work() {
        crate::test_support::assert_work_boundaries(&["creo primitive scalar discovery"], |ctx| {
            scalar_arrays(ctx, &[0; 64])
        });
        crate::test_support::assert_work_boundaries(&["creo primitive strip discovery"], |ctx| {
            triangle_strips(ctx, &[0; 64])
        });
    }

    #[test]
    fn primitive_scalar_values_refuse_before_declared_count_growth() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = named("p1", &[0], 1);
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, Some("creo primitive scalar values"),
            |limit| with_collection_limit(&bytes, limit, |ctx| scalar_arrays(ctx, &bytes)),
        );
        let admitted = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, None,
            |limit| with_collection_limit(&bytes, limit, |ctx| scalar_arrays(ctx, &bytes)),
        );
        assert_eq!(
            with_collection_limit(&bytes, admitted, |ctx| scalar_arrays(ctx, &bytes))
                .expect("one scalar array admitted")
                .len(),
            1
        );
        let error = with_collection_limit(&bytes, limit, |ctx| scalar_arrays(ctx, &bytes))
            .expect_err("the scalar needs admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo primitive scalar values")
        );
    }

    #[test]
    fn primitive_scalar_array_refuses_before_result_growth() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = named("p1", &[0], 1);
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, Some("creo primitive scalar arrays"),
            |limit| with_collection_limit(&bytes, limit, |ctx| scalar_arrays(ctx, &bytes)),
        );
        let error = with_collection_limit(&bytes, limit, |ctx| scalar_arrays(ctx, &bytes))
            .expect_err("the result array needs admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo primitive scalar arrays")
        );
    }

    #[test]
    fn primitive_scalar_ordering_refuses_work_and_index_scratch_before_sorting() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes: Vec<_> = (0..21).flat_map(|_| named("p1", &[], 0)).collect();
        let admitted = crate::decode::with_test_decode_ctx(|ctx| scalar_arrays(ctx, &bytes))
            .expect("service admits ordering");
        assert_eq!(admitted.len(), 21);
        assert!(admitted.windows(2).all(|pair| pair[0].offset < pair[1].offset));
        for (dimension, operation) in [
            (ResourceDimension::MaterializedBytes, "creo primitive scalar arrays"),
            (ResourceDimension::WorkUnits, "creo primitive scalar array ordering"),
        ] {
            let error = crate::test_support::last_refusal_at(
                &bytes, dimension, operation, |ctx| scalar_arrays(ctx, &bytes),
            );
            let cadmpeg_core::CodecError::ResourceLimit(resource) = error else {
                panic!("ordering resource refusal expected");
            };
            assert_eq!(resource.dimension, dimension);
            assert_eq!(resource.operation, operation);
            assert_eq!(resource.limit + 1, resource.used + resource.additional);
        }
    }

    #[test]
    fn triangle_strip_comparison_stops_at_first_difference() {
        use cadmpeg_core::decode::ResourceDimension;
        let compare = |count: usize| {
            let left = finite_values(vec![0.0; 3 * count]);
            let mut right = left.clone();
            right[0] = FiniteReal::ONE;
            let error = crate::test_support::last_refusal_at(
                &[], ResourceDimension::WorkUnits, "creo triangle strip representation comparison",
                |ctx| super::vertex_lanes_equal(ctx, (&left, 3, 0), (&right, 3, 0)),
            );
            let cadmpeg_core::CodecError::ResourceLimit(resource) = error else {
                panic!("comparison work refusal");
            };
            assert!(!crate::decode::with_test_decode_ctx(|ctx|
                super::vertex_lanes_equal(ctx, (&left, 3, 0), (&right, 3, 0))
            ).expect("comparison"));
            (resource.used, resource.additional)
        };
        assert_eq!(compare(1), compare(64));
    }

    fn minimal_strip() -> Vec<u8> {
        let mut bytes =
            b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\x01\x03".to_vec();
        bytes.extend(named("mv_p_xyz", &[0; 9], 9));
        bytes
    }

    #[test]
    fn triangle_strip_cumulative_counts_refuse_before_growth() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = minimal_strip();
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, Some("creo triangle strip cumulative counts"),
            |limit| with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes)),
        );
        let admitted = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, None,
            |limit| with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes)),
        );
        assert_eq!(
            with_collection_limit(&bytes, admitted, |ctx| triangle_strips(ctx, &bytes))
                .expect("one triangle strip admitted")
                .strips
                .len(),
            1
        );
        let error = with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes))
            .expect_err("cumulative count needs admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo triangle strip cumulative counts")
        );
    }

    #[test]
    fn triangle_strip_lengths_refuse_before_growth() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = minimal_strip();
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, Some("creo triangle strip lengths"),
            |limit| with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes)),
        );
        let error = with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes))
            .expect_err("strip length needs admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo triangle strip lengths")
        );
    }

    #[test]
    fn triangle_strip_positions_refuse_before_growth() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = minimal_strip();
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, Some("creo triangle strip positions"),
            |limit| with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes)),
        );
        let error = with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes))
            .expect_err("strip positions need admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo triangle strip positions")
        );
    }

    #[test]
    fn triangle_strip_records_refuse_before_result_growth() {
        use cadmpeg_core::decode::ResourceDimension;
        let bytes = minimal_strip();
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, Some("creo triangle strip records"),
            |limit| with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes)),
        );
        let error = with_collection_limit(&bytes, limit, |ctx| triangle_strips(ctx, &bytes))
            .expect_err("strip record needs admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo triangle strip records")
        );
    }

    #[test]
    fn triangle_strip_normals_refuse_before_growth() {
        use cadmpeg_core::decode::ResourceDimension;
        let arrays = [PrimitiveScalarArray {
            field: PrimitiveArrayField::VertexNormalsAndPositions,
            offset: 0,
            values: finite_values(vec![0.0; 18]),
        }];
        let run = |limit| with_collection_limit(&[], limit, |ctx| {
            triangle_strip_geometry(ctx, &arrays, 3).map_err(|error| match error {
                TriangleStripGeometryError::Resource(error) => error,
                other => panic!("unexpected geometry refusal: {other:?}"),
            })
        });
        let limit = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems, Some("creo triangle strip normals"), run,
        );
        let error = with_collection_limit(&[], limit, |ctx| {
            triangle_strip_geometry(ctx, &arrays, 3).map_err(|error| match error {
                TriangleStripGeometryError::Resource(error) => error,
                other => panic!("unexpected geometry refusal: {other:?}"),
            })
        })
        .expect_err("normals need admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo triangle strip normals")
        );
    }

    fn finite_values(values: Vec<f64>) -> Vec<FiniteReal> {
        values
            .into_iter()
            .map(|value| FiniteReal::new(value).expect("finite test scalar"))
            .collect()
    }

    fn finite_points(points: Vec<[f64; 3]>) -> Vec<FiniteVector<3>> {
        points
            .into_iter()
            .map(|point| FiniteVector::new(point).expect("finite test point"))
            .collect()
    }

    fn named(name: &str, values: &[u8], count: u8) -> Vec<u8> {
        let mut bytes = vec![0xe0, 0x06];
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&[0, 0xf8, count]);
        bytes.extend_from_slice(values);
        bytes
    }

    #[test]
    fn decodes_complete_primitive_float_array() {
        let bytes = named(
            "p1",
            &[0x00, 0x48, 0xa6, 0x66, 0x66, 0x38, 0x86, 0x66, 0x66],
            3,
        );
        let arrays = with_context(&bytes, |ctx| scalar_arrays(ctx, &bytes))
            .expect("the primitive fixture fits service limits");
        assert_eq!(arrays.len(), 1);
        assert_eq!(arrays[0].values[0].get(), 0.0);
        assert!((arrays[0].values[1].get() - 20.8).abs() < 1.0e-5);
        assert!((arrays[0].values[2].get() + 16.8).abs() < 1.0e-5);
    }

    #[test]
    fn rejects_truncated_declared_array() {
        let bytes = named("pts", &[0x48, 0xa6, 0x66, 0x66], 2);
        assert!(with_context(&bytes, |ctx| scalar_arrays(ctx, &bytes))
            .expect("the primitive fixture fits service limits")
            .is_empty());
    }

    #[test]
    fn decodes_interleaved_normal_position_array() {
        let tuple = [
            0x00, 0x28, 0x00, 0x38, 0xa6, 0x66, 0x66, 0x48, 0x93, 0x33, 0x33, 0x38, 0x86, 0x66,
            0x66,
        ];
        let bytes = named("mv_p_NxNyNzxyz", &tuple, 6);
        let arrays = with_context(&bytes, |ctx| scalar_arrays(ctx, &bytes))
            .expect("the primitive fixture fits service limits");
        assert_eq!(arrays.len(), 1);
        assert_eq!(arrays[0].values.len(), 6);
    }

    #[test]
    fn decodes_position_only_array() {
        let bytes = named(
            "mv_p_xyz",
            &[
                0x48, 0x21, 0x96, 0xec, 0x3a, 0xa2, 0xe2, 0xc4, 0x48, 0x2a, 0xbb, 0x34,
            ],
            3,
        );
        let arrays = with_context(&bytes, |ctx| scalar_arrays(ctx, &bytes))
            .expect("the primitive fixture fits service limits");
        assert_eq!(arrays.len(), 1);
        assert_eq!(arrays[0].field.as_str(), "mv_p_xyz");
        assert_eq!(arrays[0].values.len(), 3);
    }

    #[test]
    fn decodes_named_triangle_strip() {
        let mut bytes =
            b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\x01\x03".to_vec();
        bytes.extend(named(
            "mv_p_xyz",
            &[
                0x00, 0x00, 0x00, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
            9,
        ));
        let scan = with_context(&bytes, |ctx| triangle_strips(ctx, &bytes))
            .expect("the primitive fixture fits service limits");
        assert_eq!(scan.conflicting_representation_count, 0);
        let strips = scan.strips;
        assert_eq!(strips.len(), 1);
        assert_eq!(strips[0].positions().len(), 3);
        assert!(strips[0].normals().is_none());
        assert_eq!(strips[0].strip_lengths, [3]);
    }

    #[test]
    fn decodes_interleaved_triangle_strip_positions_and_normals() {
        let mut bytes =
            b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\x01\x03".to_vec();
        let tuple = [
            0x00, 0x28, 0x00, 0x00, 0x00, 0x00, // normal, position 0
            0x00, 0x28, 0x00, 0x46, 0x80, 0x00, 0x00, 0x00, 0x00, // normal, position x
            0x00, 0x28, 0x00, 0x00, 0x46, 0x80, 0x00, 0x00, 0x00, // normal, position y
        ];
        bytes.extend(named("mv_p_NxNyNzxyz", &tuple, 18));

        let scan = with_context(&bytes, |ctx| triangle_strips(ctx, &bytes))
            .expect("the primitive fixture fits service limits");
        assert_eq!(scan.conflicting_representation_count, 0);
        let strips = scan.strips;
        assert_eq!(strips.len(), 1);
        assert_eq!(
            strips[0]
                .positions()
                .map(|position| position.get())
                .collect::<Vec<_>>(),
            [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(
            strips[0]
                .normals()
                .map(|normals| normals.copied().collect::<Vec<_>>()),
            Some(finite_points(vec![[0.0, 1.0, 0.0]; 3]))
        );
        assert_eq!(strips[0].strip_lengths, [3]);
    }

    #[test]
    fn agreeing_triangle_strip_representations_select_normals_independent_of_order() {
        let xyz = PrimitiveScalarArray {
            field: PrimitiveArrayField::VertexPositions,
            offset: 10,
            values: finite_values(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
        };
        let normal_xyz = PrimitiveScalarArray {
            field: PrimitiveArrayField::VertexNormalsAndPositions,
            offset: 20,
            values: finite_values(vec![
                0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                1.0, 0.0,
            ]),
        };

        let expected = TriangleStripGeometry {
            positions: finite_points(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
            normals: Some(finite_points(vec![[0.0, 0.0, 1.0]; 3])),
        };
        assert_eq!(
            with_context(&[], |ctx| triangle_strip_geometry(
                ctx,
                &[xyz.clone(), normal_xyz.clone()],
                3
            ))
            .expect("the primitive fixture fits service limits"),
            expected.clone()
        );
        assert_eq!(
            with_context(&[], |ctx| triangle_strip_geometry(
                ctx,
                &[normal_xyz, xyz],
                3
            ))
            .expect("the primitive fixture fits service limits"),
            expected
        );
    }

    #[test]
    fn conflicting_triangle_strip_representations_are_withheld() {
        let xyz = PrimitiveScalarArray {
            field: PrimitiveArrayField::VertexPositions,
            offset: 10,
            values: finite_values(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
        };
        let conflicting_xyz = PrimitiveScalarArray {
            field: PrimitiveArrayField::VertexNormalsAndPositions,
            offset: 20,
            values: finite_values(vec![
                0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                1.0, 0.0,
            ]),
        };

        assert!(matches!(
            with_context(&[], |ctx| triangle_strip_geometry(
                ctx,
                &[xyz, conflicting_xyz],
                3
            )),
            Err(TriangleStripGeometryError::Conflicting)
        ));

        let mut bytes =
            b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\x01\x03".to_vec();
        bytes.extend(named(
            "mv_p_xyz",
            &[
                0x00, 0x00, 0x00, 0x46, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46, 0x80, 0x00, 0x00,
                0x00,
            ],
            9,
        ));
        bytes.extend(named(
            "mv_p_NxNyNzxyz",
            &[
                0x00, 0x28, 0x00, 0x00, 0x00, 0x00, // normal, position 0
                0x00, 0x28, 0x00, 0x47, 0x00, 0x00, 0x00, 0x00,
                0x00, // normal, position x = 2
                0x00, 0x28, 0x00, 0x00, 0x46, 0x80, 0x00, 0x00, 0x00,
            ],
            18,
        ));
        let arrays = with_context(&bytes, |ctx| scalar_arrays(ctx, &bytes))
            .expect("the primitive fixture fits service limits");
        assert_eq!(
            arrays
                .iter()
                .map(|array| array.field.as_str())
                .collect::<Vec<_>>(),
            ["mv_p_xyz", "mv_p_NxNyNzxyz"]
        );
        assert!(matches!(
            with_context(&bytes, |ctx| triangle_strip_geometry(ctx, &arrays, 3)),
            Err(TriangleStripGeometryError::Conflicting)
        ));
        let scan = with_context(&bytes, |ctx| triangle_strips(ctx, &bytes))
            .expect("the primitive fixture fits service limits");
        assert!(scan.strips.is_empty());
        assert_eq!(scan.conflicting_representation_count, 1);
    }
    #[test]
    fn primitive_impossible_counts_do_not_allocate() {
        let scalar = b"\xe0\x06p1\0\xf8\xbf\xff";
        assert!(
            with_collection_limit(scalar, 0, |ctx| scalar_arrays(ctx, scalar))
                .expect("truncated candidate")
                .is_empty()
        );
        let strip = b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8\xbf\xff";
        assert!(
            with_collection_limit(strip, 0, |ctx| triangle_strips(ctx, strip))
                .expect("truncated strip")
                .strips
                .is_empty()
        );
    }

    #[test]
    fn checked_primitive_strips_reject_invalid_lanes_and_spans() {
        crate::decode::with_test_decode_ctx(|ctx| {
            let positions = || finite_points(vec![[0.0; 3]; 3]);
            for spans in [vec![], vec![2], vec![4], vec![3, 3]] {
                assert!(
                    super::PrimitiveTriangleStrip::new(ctx, 0, positions(), None, spans)
                        .expect("service")
                        .is_none()
                );
            }
            assert!(super::PrimitiveTriangleStrip::new(
                ctx,
                0,
                positions(),
                Some(finite_points(vec![[0.0; 3]; 2])),
                vec![3]
            )
            .expect("service")
            .is_none());
            let shaded = super::PrimitiveTriangleStrip::new(
                ctx,
                0,
                positions(),
                Some(finite_points(vec![[0.0, 1.0, 0.0]; 3])),
                vec![3],
            )
            .expect("service")
            .expect("paired strip");
            assert_eq!(shaded.positions().len(), 3);
            assert_eq!(shaded.normals().expect("shaded").len(), 3);
        });
    }
}
