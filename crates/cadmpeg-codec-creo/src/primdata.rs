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
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        offset: usize,
        positions: Vec<FiniteVector<3>>,
        normals: Option<Vec<FiniteVector<3>>>,
        strip_lengths: Vec<u32>,
    ) -> Result<Option<Self>, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let mut total = 0usize;
        let mut lengths = strip_lengths.iter();
        while lengths.len() != 0 {
            let Some(length) = ctx.next_charged(&mut lengths, "creo primitive strip validation")? else {
                break;
            };
            if *length < 3 {
                return Ok(None);
            }
            let Some(next) = usize::try_from(*length).ok().and_then(|length| total.checked_add(length)) else {
                return Ok(None);
            };
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
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(positions.len()),
                    "creo primitive vertex pairing",
                )?;
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

#[derive(Debug, Clone, PartialEq)]
struct TriangleStripGeometry {
    positions: Vec<FiniteVector<3>>,
    normals: Option<Vec<FiniteVector<3>>>,
}

fn triangle_strip_geometry(
    ctx: &DecodeContext<'_>,
    arrays: &[PrimitiveScalarArray],
    vertex_count: u32,
) -> Result<TriangleStripGeometry, TriangleStripGeometryError> {
    let vertex_count =
        usize::try_from(vertex_count).map_err(|_| TriangleStripGeometryError::Missing)?;
    let mut positions = None::<Vec<FiniteVector<3>>>;
    let mut normals = None::<Vec<FiniteVector<3>>>;
    for array in arrays {
        let (candidate_positions, candidate_normals) = match array.field {
            PrimitiveArrayField::VertexPositions => {
                if array.values.len()
                    != vertex_count
                        .checked_mul(3)
                        .ok_or(TriangleStripGeometryError::Missing)?
                {
                    continue;
                }
                (
                    {
                        let mut points = Vec::new();
                        ctx.reserve_vec(&mut points, vertex_count, "creo triangle strip positions")
                            .map_err(TriangleStripGeometryError::Resource)?;
                        points.extend(
                            array
                                .values
                                .chunks_exact(3)
                                .map(|point| FiniteVector::from([point[0], point[1], point[2]])),
                        );
                        points
                    },
                    None,
                )
            }
            PrimitiveArrayField::VertexNormalsAndPositions => {
                if array.values.len()
                    != vertex_count
                        .checked_mul(6)
                        .ok_or(TriangleStripGeometryError::Missing)?
                {
                    continue;
                }
                (
                    {
                        let mut points = Vec::new();
                        ctx.reserve_vec(&mut points, vertex_count, "creo triangle strip positions")
                            .map_err(TriangleStripGeometryError::Resource)?;
                        points.extend(
                            array
                                .values
                                .chunks_exact(6)
                                .map(|tuple| FiniteVector::from([tuple[3], tuple[4], tuple[5]])),
                        );
                        points
                    },
                    Some({
                        let mut normals = Vec::new();
                        ctx.reserve_vec(&mut normals, vertex_count, "creo triangle strip normals")
                            .map_err(TriangleStripGeometryError::Resource)?;
                        normals.extend(
                            array
                                .values
                                .chunks_exact(6)
                                .map(|tuple| FiniteVector::from([tuple[0], tuple[1], tuple[2]])),
                        );
                        normals
                    }),
                )
            }
            PrimitiveArrayField::P1 | PrimitiveArrayField::P2 | PrimitiveArrayField::Points => {
                continue
            }
        };
        if positions
            .as_ref()
            .is_some_and(|selected| *selected != candidate_positions)
        {
            return Err(TriangleStripGeometryError::Conflicting);
        }
        positions.get_or_insert(candidate_positions);
        if let Some(candidate_normals) = candidate_normals {
            if normals
                .as_ref()
                .is_some_and(|selected| *selected != candidate_normals)
            {
                return Err(TriangleStripGeometryError::Conflicting);
            }
            normals.get_or_insert(candidate_normals);
        }
    }
    Ok(TriangleStripGeometry {
        positions: positions.ok_or(TriangleStripGeometryError::Missing)?,
        normals,
    })
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
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(data.len()),
        "creo primitive strip discovery",
    )?;
    for (offset, _) in data
        .windows(RECORD.len())
        .enumerate()
        .filter(|(_, window)| *window == RECORD)
    {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(data.len() - offset - RECORD.len()),
            "creo primitive strip boundary scan",
        )?;
        let end = data[offset + RECORD.len()..]
            .windows(b"\xe0\x00value(".len())
            .position(|window| window == b"\xe0\x00value(")
            .map_or(data.len(), |relative| offset + RECORD.len() + relative);
        let record = &data[offset..end];
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(record.len()),
            "creo primitive cumulative label scan",
        )?;
        let Some(accum) = record
            .windows(ACCUM.len())
            .position(|window| window == ACCUM)
            .map(|relative| relative + ACCUM.len())
        else {
            continue;
        };
        if record.get(accum) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (count, mut cursor) = psb::compact_int(record, accum + 1);
        if cadmpeg_core::decode::bounded_len(u64::from(count), 1, record.len() - cursor).is_none() {
            continue;
        }
        ctx.charge_work(u64::from(count), "creo primitive cumulative parsing")?;
        let mut cumulative = Vec::new();
        ctx.reserve_vec(
            &mut cumulative,
            index_from_u32(count),
            "creo triangle strip cumulative counts",
        )?;
        for _ in 0..count {
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
        let mut strip_lengths = Vec::new();
        ctx.reserve_vec(
            &mut strip_lengths,
            cumulative.len(),
            "creo triangle strip lengths",
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(cumulative.len()),
            "creo primitive strip length construction",
        )?;
        for current in cumulative {
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
        let arrays = scalar_arrays(ctx, record)?;
        let geometry = match triangle_strip_geometry(ctx, &arrays, vertex_count) {
            Ok(geometry) => geometry,
            Err(TriangleStripGeometryError::Missing) => continue,
            Err(TriangleStripGeometryError::Conflicting) => {
                conflicting_representation_count += 1;
                continue;
            }
            Err(TriangleStripGeometryError::Resource(error)) => return Err(error),
        };
        ctx.reserve_vec(&mut strips, 1, "creo triangle strip records")?;
        if let Some(strip) = PrimitiveTriangleStrip::new(
            ctx,
            offset,
            geometry.positions,
            geometry.normals,
            strip_lengths,
        )? {
            strips.push(strip);
        }
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
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(data.len()),
            "creo primitive scalar discovery",
        )?;
        for (offset, _) in data.windows(marker_len).enumerate().filter(|(_, window)| {
            window[0] == psb::token::NAMED_RECORD
                && window[1] == 0x06
                && window[2..2 + name.len()] == *name
                && window[marker_len - 1] == 0
        }) {
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
            ctx.charge_work(u64::from(count), "creo primitive scalar parsing")?;
            let mut values = Vec::new();
            ctx.reserve_vec(&mut values, capacity, "creo primitive scalar values")?;
            let mut cursor = psb::Cursor::at(data, start);
            while values.len() < capacity {
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
                    values,
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
    mod strip_visits;
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
        assert_eq!(
            with_collection_limit(&bytes, 2, |ctx| scalar_arrays(ctx, &bytes))
                .expect("one scalar array admitted")
                .len(),
            1
        );
        let error = with_collection_limit(&bytes, 0, |ctx| scalar_arrays(ctx, &bytes))
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
        let error = with_collection_limit(&bytes, 1, |ctx| scalar_arrays(ctx, &bytes))
            .expect_err("the result array needs admission");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo primitive scalar arrays")
        );
    }

    #[test]
    fn primitive_scalar_ordering_refuses_work_and_index_scratch_before_sorting() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let bytes: Vec<_> = (0..21).flat_map(|_| named("p1", &[], 0)).collect();
        let record_bytes =
            u64::try_from(std::mem::size_of::<PrimitiveScalarArray>()).expect("record bytes");
        let index_bytes = u64::try_from(std::mem::size_of::<usize>()).expect("index bytes");
        let scratch = 21 * 2 * index_bytes;
        // The result growth keeps sixteen old records live; stable ordering holds two index vectors.
        let peak = (16 * record_bytes).max(scratch);
        // Five scans and three result reallocations, then the stable ordering: index
        // setup, one sort of the index array by (offset, index) keys, two record moves
        // per value along the permutation, the permutation visits, and one unit per
        // value for its swap or, as here where the records arrive in order, its top-up.
        let work = 5 * u64::try_from(bytes.len()).expect("scan work")
            + (4 + 8 + 16) * record_bytes
            + 2 * 21
            + 2 * 21
            + 21 * (index_bytes + 4 * index_bytes) * 6 * 8
            + 2 * record_bytes * 21
            + 2 * 21
            + 21;
        let run = |materialized, work_limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            // Twenty-one arrays plus the stable sort's two index slots per value.
            policy.limits.max_collection_items = 21 + 2 * 21;
            policy.limits.max_materialized_bytes = materialized;
            policy.limits.max_work_units = work_limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root admitted");
            let result = scalar_arrays(&ctx, &bytes);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(resource)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(resource));
            }
            result
        };
        let admitted = run(peak, work).expect("exact work and scratch admit ordering");
        assert_eq!(admitted.len(), 21);
        assert!(admitted
            .windows(2)
            .all(|pair| pair[0].offset < pair[1].offset));
        for (dimension, materialized, work_limit, need) in [
            (ResourceDimension::MaterializedBytes, peak - 1, work, peak),
            (ResourceDimension::WorkUnits, peak, work - 1, work),
        ] {
            let error = run(materialized, work_limit).expect_err("ordering needs admission");
            let cadmpeg_core::CodecError::ResourceLimit(resource) = error else {
                panic!("ordering resource refusal expected");
            };
            assert_eq!(resource.dimension, dimension);
            assert_eq!(
                resource.operation,
                if dimension == ResourceDimension::MaterializedBytes {
                    "creo primitive scalar arrays"
                } else {
                    "creo primitive scalar array ordering"
                }
            );
            assert_eq!(resource.used + resource.additional, need);
            assert_eq!(resource.limit, need - 1);
        }
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
        assert_eq!(
            with_collection_limit(&bytes, 16, |ctx| triangle_strips(ctx, &bytes))
                .expect("one triangle strip admitted")
                .strips
                .len(),
            1
        );
        let error = with_collection_limit(&bytes, 0, |ctx| triangle_strips(ctx, &bytes))
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
        let error = with_collection_limit(&bytes, 1, |ctx| triangle_strips(ctx, &bytes))
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
        let error = with_collection_limit(&bytes, 12, |ctx| triangle_strips(ctx, &bytes))
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
        let error = with_collection_limit(&bytes, 15, |ctx| triangle_strips(ctx, &bytes))
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
        let error = with_collection_limit(&[], 3, |ctx| {
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
