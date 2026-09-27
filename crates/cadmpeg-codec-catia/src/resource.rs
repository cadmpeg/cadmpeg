// SPDX-License-Identifier: Apache-2.0
//! Charged fallible growth for CATIA decode collections.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use cadmpeg_core::decode::{
    DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit, ScopedReservation,
};
use cadmpeg_core::CodecError;

fn allocation_failed(
    used: usize,
    capacity: usize,
    additional: usize,
    operation: &'static str,
) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::Codec(operation),
        reason: ResourceFailure::AllocationFailed,
        limit: capacity as u64,
        used: used as u64,
        additional: additional as u64,
        operation,
    })
}

pub(crate) fn push<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    values.push(value);
    Ok(())
}

pub(crate) fn push_back<T>(
    ctx: &DecodeContext<'_>,
    values: &mut VecDeque<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    values.push_back(value);
    Ok(())
}

pub(crate) fn reserve_vec<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(additional as u64, operation)?;
    values
        .try_reserve(additional)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), additional, operation))
}

pub(crate) fn copy_slice<T: Clone>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut copy = Vec::new();
    reserve_vec(ctx, &mut copy, values.len(), operation)?;
    copy.extend_from_slice(values);
    Ok(copy)
}

pub(crate) fn copy_retained_slice<T: Clone>(
    ctx: &DecodeContext<'_>,
    values: &[T],
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let Some(bytes) = values
        .len()
        .checked_mul(std::mem::size_of::<T>().max(1))
        .and_then(|bytes| u64::try_from(bytes).ok())
    else {
        return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, operation)?;
    copy_slice(ctx, values, operation)
}

pub(crate) fn copy_retained_rows<T: Clone>(
    ctx: &DecodeContext<'_>,
    rows: &[Vec<T>],
    row_operation: &'static str,
    item_operation: &'static str,
) -> Result<Vec<Vec<T>>, CodecError> {
    let Some(bytes) = rows
        .len()
        .checked_mul(std::mem::size_of::<Vec<T>>())
        .and_then(|bytes| u64::try_from(bytes).ok())
    else {
        return Err(ctx.refuse_codec_limit(row_operation, u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, row_operation)?;
    let mut copy = Vec::new();
    reserve_vec(ctx, &mut copy, rows.len(), row_operation)?;
    for row in rows {
        copy.push(copy_retained_slice(ctx, row, item_operation)?);
    }
    Ok(copy)
}

pub(crate) fn copy_knot_vector(
    ctx: &DecodeContext<'_>,
    knots: &cadmpeg_ir::geometry::nurbs::KnotVector,
    operation: &'static str,
) -> Result<cadmpeg_ir::geometry::nurbs::KnotVector, CodecError> {
    let count = u64::try_from(knots.len())
        .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    ctx.charge_collection_items(count, operation)?;
    knots
        .try_clone()
        .map_err(|_| allocation_failed(0, 0, knots.len(), operation))
}

pub(crate) fn copy_nurbs_surface(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    operation: &'static str,
) -> Result<cadmpeg_ir::geometry::nurbs::NurbsSurface, CodecError> {
    use cadmpeg_ir::geometry::nurbs::NurbsPoleGrid;

    let knots = surface.u_knots().len().checked_add(surface.v_knots().len());
    let (rows, poles, pole_bytes) = match surface.pole_grid() {
        NurbsPoleGrid::Polynomial { rows } => (
            rows.len(),
            rows.iter().try_fold(0usize, |total, row| total.checked_add(row.len())),
            std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>(),
        ),
        NurbsPoleGrid::Rational { rows } => (
            rows.len(),
            rows.iter().try_fold(0usize, |total, row| total.checked_add(row.len())),
            std::mem::size_of::<cadmpeg_ir::geometry::nurbs::WeightedPole3<cadmpeg_ir::features::FinitePoint3>>(),
        ),
    };
    let Some((count, bytes)) = knots
        .and_then(|knots| knots.checked_add(rows).zip(knots.checked_mul(size_of::<f64>())))
        .and_then(|(count, knot_bytes)| {
            poles.and_then(|poles| {
                count.checked_add(poles).zip(
                    rows.checked_mul(size_of::<Vec<usize>>())
                        .and_then(|row_bytes| poles.checked_mul(pole_bytes).and_then(|pole_bytes| knot_bytes.checked_add(row_bytes)?.checked_add(pole_bytes))),
                )
            })
        })
        .and_then(|(count, bytes)| Some((u64::try_from(count).ok()?, u64::try_from(bytes).ok()?)))
    else {
        return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
    };
    ctx.charge_collection_items(count, operation)?;
    ctx.charge_retained(bytes, operation)?;
    surface.try_clone().map_err(|_| allocation_failed(0, 0, usize::try_from(count).unwrap_or(usize::MAX), operation))
}

#[cfg(test)]
mod nurbs_copy_tests {
    use super::copy_nurbs_surface;
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::Point3;

    #[test]
    fn nurbs_surface_copy_refuses_before_nested_lanes() {
        let surface = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                ],
                None,
            ),
            false,
        )
        .expect("valid surface");
        assert_eq!(
            crate::test_support::with_service_context(|ctx| copy_nurbs_surface(ctx, &surface, "catia_nurbs_surface_copy"))
                .expect("service resource budget"),
            surface
        );
        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| copy_nurbs_surface(ctx, &surface, "catia_nurbs_surface_copy")),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_nurbs_surface_copy"
        ));
    }
}

pub(crate) fn reserve_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<T>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), count, operation))
}

pub(crate) fn reserve_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(count as u64, operation)?;
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), count, operation))
}

pub(crate) fn insert_set<T: Eq + Hash>(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<T>,
    value: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if values.contains(&value) {
        return Ok(false);
    }
    ctx.charge_collection_items(1, operation)?;
    values
        .try_reserve(1)
        .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    Ok(values.insert(value))
}

pub(crate) fn insert_map<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<Option<V>, CodecError> {
    if !values.contains_key(&key) {
        ctx.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    }
    Ok(values.insert(key, value))
}

pub(crate) fn admit_map_entry<K: Eq + Hash, V>(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<K, V>,
    key: &K,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains_key(key) {
        ctx.charge_collection_items(1, operation)?;
        values
            .try_reserve(1)
            .map_err(|_| allocation_failed(values.len(), values.capacity(), 1, operation))?;
    }
    Ok(())
}

fn temporary_bytes<T>(
    ctx: &DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<u64, CodecError> {
    let item_bytes = std::mem::size_of::<T>().max(1);
    let Some(bytes) = item_bytes
        .checked_add(32)
        .and_then(|size| size.checked_mul(count))
        .and_then(|size| u64::try_from(size).ok())
    else {
        return Err(ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX));
    };
    Ok(bytes)
}

pub(crate) fn temporary_set<'a, T: Eq + Hash>(
    ctx: &'a DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(HashSet<T>, ScopedReservation<'a>), CodecError> {
    let reservation =
        ctx.reserve_scoped(temporary_bytes::<T>(ctx, count, operation)?, operation)?;
    let mut values = HashSet::new();
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(0, values.capacity(), count, operation))?;
    Ok((values, reservation))
}

pub(crate) fn temporary_queue<'a, T>(
    ctx: &'a DecodeContext<'_>,
    count: usize,
    operation: &'static str,
) -> Result<(VecDeque<T>, ScopedReservation<'a>), CodecError> {
    let reservation =
        ctx.reserve_scoped(temporary_bytes::<T>(ctx, count, operation)?, operation)?;
    let mut values = VecDeque::new();
    values
        .try_reserve(count)
        .map_err(|_| allocation_failed(0, values.capacity(), count, operation))?;
    Ok((values, reservation))
}

#[cfg(test)]
mod tests {
    use super::admit_map_entry;
    use std::collections::HashMap;

    #[test]
    fn zero_entity_source_cache_refuses_before_vacant_map_entry() {
        let mut limited_map = HashMap::<u32, u32>::new();
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            admit_map_entry(
                ctx,
                &mut limited_map,
                &7,
                "catia_zero_wire_source_geometries",
            )
        });
        assert!(matches!(
            limited,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert!(limited_map.is_empty());

        crate::test_support::with_service_context(|ctx| {
            let mut admitted_map = HashMap::<u32, u32>::new();
            admit_map_entry(
                ctx,
                &mut admitted_map,
                &7,
                "catia_zero_wire_source_geometries",
            )
            .expect("service map entry budget");
            admitted_map.insert(7, 11);
            admit_map_entry(
                ctx,
                &mut admitted_map,
                &7,
                "catia_zero_wire_source_geometries",
            )
            .expect("existing entry needs no allocation");
            assert_eq!(admitted_map.get(&7), Some(&11));
        });
    }
}
