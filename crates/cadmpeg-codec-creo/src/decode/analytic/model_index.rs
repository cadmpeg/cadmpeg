// SPDX-License-Identifier: Apache-2.0
//! Unique model surface positions in the native geometry namespaces.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{Curve, Surface};

const SURFACE_PREFIXES: [&str; 3] = [
    "creo:visibgeom:surface#",
    "creo:novisgeom:surface#",
    "creo:actdatums:surface#",
];

pub(super) struct SurfaceIndex<'ctx> {
    rows: HashMap<(usize, u32), Option<usize>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx> SurfaceIndex<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, surfaces: &[Surface]) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo model surface index storage")?;
        let mut rows = HashMap::new();
        for (position, surface) in ctx.admit_iter(surfaces, "creo model surface index rows")?.enumerate() {
            for (namespace, prefix) in SURFACE_PREFIXES.into_iter().enumerate() {
                let Some(suffix) = surface.id.as_str().strip_prefix(prefix) else { continue; };
                // A native u32 identity has at most ten digits. Validate its canonical
                // spelling before inserting it in the integer-keyed index.
                if suffix.len() > 10 { continue; }
                let Ok(id) = suffix.parse::<u32>() else { continue; };
                if !crate::identity::matches_numbered_identity(surface.id.as_str(), prefix, id) { continue; }
                storage.with_storage(|| -> Result<(), CodecError> {
                    match ctx.entry_hash_map(&mut rows, (namespace, id), "creo model surface index entries")? {
                        std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(position)); }
                        std::collections::hash_map::Entry::Occupied(mut entry) => { *entry.get_mut() = None; }
                    }
                    Ok(())
                })?;
            }
        }
        Ok(Self { rows, _storage: storage })
    }

    pub(super) fn entry(&self, namespace: usize, id: u32) -> Option<Option<usize>> {
        self.rows.get(&(namespace, id)).copied()
    }

    pub(super) fn unique(&self, namespace: usize, id: u32) -> Option<usize> {
        self.rows.get(&(namespace, id)).copied().flatten()
    }

    /// Visible geometry has priority over non-visible geometry and active datums.
    /// A duplicate in the first present namespace prevents a fallback.
    pub(super) fn preferred(&self, id: u32) -> Option<usize> {
        for namespace in 0..SURFACE_PREFIXES.len() {
            if let Some(position) = self.rows.get(&(namespace, id)) { return *position; }
        }
        None
    }
}

/// Unique visible-geometry curve positions. Integer keys keep probes fixed work.
pub(super) struct CurveIndex<'ctx> {
    rows: HashMap<u32, Option<usize>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'ctx> CurveIndex<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, curves: &[Curve]) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "creo model curve index storage")?;
        let mut rows = HashMap::new();
        const PREFIX: &str = "creo:visibgeom:curve#";
        for (position, curve) in ctx.admit_iter(curves, "creo model curve index rows")?.enumerate() {
            let Some(suffix) = curve.id.as_str().strip_prefix(PREFIX) else { continue; };
            if suffix.len() > 10 { continue; }
            let Ok(id) = suffix.parse::<u32>() else { continue; };
            if !crate::identity::matches_numbered_identity(curve.id.as_str(), PREFIX, id) { continue; }
            storage.with_storage(|| -> Result<(), CodecError> {
                match ctx.entry_hash_map(&mut rows, id, "creo model curve index entries")? {
                    std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(position)); }
                    std::collections::hash_map::Entry::Occupied(mut entry) => { *entry.get_mut() = None; }
                }
                Ok(())
            })?;
        }
        Ok(Self { rows, _storage: storage })
    }

    pub(super) fn contains(&self, id: u32) -> bool { self.rows.contains_key(&id) }

    pub(super) fn unique(&self, id: u32) -> Option<usize> {
        self.rows.get(&id).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::{CurveIndex, SurfaceIndex};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::{CurveId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn surface(id: &str) -> Surface {
        Surface { id: SurfaceId::mint(id.to_owned()).expect("fixture identity"), geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }), source_object: None }
    }

    fn curve(id: &str) -> Curve {
        Curve { id: CurveId::mint(id.to_owned()).expect("fixture identity"), geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(cadmpeg_ir::geometry::analytic::LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).expect("fixture line"))), source_object: None }
    }

    #[test]
    fn model_surface_index_preserves_namespace_priority_and_duplicates() {
        let surfaces = [surface("creo:novisgeom:surface#7"), surface("creo:actdatums:surface#7"), surface("creo:visibgeom:surface#7"), surface("creo:visibgeom:surface#8"), surface("creo:visibgeom:surface#8"), surface("creo:novisgeom:surface#8")];
        crate::decode::with_test_decode_ctx(|ctx| {
            let index = SurfaceIndex::new(ctx, &surfaces)?;
            assert_eq!(index.preferred(7), Some(2));
            assert_eq!(index.unique(1, 7), Some(0));
            assert_eq!(index.unique(2, 7), Some(1));
            assert_eq!(index.entry(0, 8), Some(None));
            assert_eq!(index.preferred(8), None);
            assert_eq!(index.preferred(9), None);
            Ok::<_, cadmpeg_core::CodecError>(())
        }).expect("service index");
    }

    #[test]
    fn model_indexes_reject_noncanonical_numeric_spellings() {
        let surfaces = [surface("creo:visibgeom:surface#07"), surface("creo:visibgeom:surface#4294967296"), surface("creo:visibgeom:surface#7")];
        let curves = [curve("creo:visibgeom:curve#07"), curve("creo:visibgeom:curve#4294967296"), curve("creo:visibgeom:curve#7"), curve("creo:visibgeom:curve#8"), curve("creo:visibgeom:curve#8")];
        crate::decode::with_test_decode_ctx(|ctx| {
            let index = SurfaceIndex::new(ctx, &surfaces)?;
            assert_eq!(index.preferred(7), Some(2));
            let index = CurveIndex::new(ctx, &curves)?;
            assert_eq!(index.unique(7), Some(2));
            assert_eq!(index.unique(8), None);
            assert_eq!(index.unique(9), None);
            Ok::<_, cadmpeg_core::CodecError>(())
        }).expect("service index");
    }

    #[test]
    fn model_indexes_propagate_row_admission_refusal() {
        let surfaces = [surface("creo:visibgeom:surface#7")];
        let curves = [curve("creo:visibgeom:curve#7")];
        crate::test_support::assert_work_boundaries(&["creo model surface index rows"], |ctx| SurfaceIndex::new(ctx, &surfaces).map(|index| index.preferred(7)));
        crate::test_support::assert_work_boundaries(&["creo model curve index rows"], |ctx| CurveIndex::new(ctx, &curves).map(|index| index.unique(7)));
    }
    #[test]
    fn model_surface_index_releases_temporary_storage_between_builds() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let surfaces = [surface("creo:visibgeom:surface#7")];
        let limit = crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = limit;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            SurfaceIndex::new(&ctx, &surfaces).map(|index| index.preferred(7))
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = limit;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        for _ in 0..2 {
            let index = SurfaceIndex::new(&ctx, &surfaces).expect("temporary index fits again");
            assert_eq!(index.preferred(7), Some(0));
        }
    }

}
