// SPDX-License-Identifier: Apache-2.0
//! Arena populations under standard or live-context allocation.

use std::collections::BTreeMap;
use std::convert::Infallible;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use super::{ArenaName, CensusKey};
use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::native::view::NativeView;

pub(crate) trait CensusStorage {
    type Error;
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error>;
    fn native_key(&self, format: &str, arena: &str) -> Result<CensusKey, Self::Error>;
    fn insert(&self, counts: &mut BTreeMap<CensusKey, usize>, key: CensusKey, count: usize, operation: &'static str) -> Result<(), Self::Error>;
}

pub(crate) struct StandardStorage;

impl CensusStorage for StandardStorage {
    type Error = Infallible;
    fn work(&self, _count: usize, _operation: &'static str) -> Result<(), Self::Error> { Ok(()) }
    fn native_key(&self, format: &str, arena: &str) -> Result<CensusKey, Self::Error> {
        Ok(CensusKey::native(format, arena))
    }
    fn insert(&self, counts: &mut BTreeMap<CensusKey, usize>, key: CensusKey, count: usize, _operation: &'static str) -> Result<(), Self::Error> {
        counts.insert(key, count);
        Ok(())
    }
}

impl CensusStorage for DecodeContext<'_> {
    type Error = CodecError;
    fn work(&self, count: usize, operation: &'static str) -> Result<(), Self::Error> {
        self.charge_work(u64_from_index(count), operation)
    }
    fn native_key(&self, format: &str, arena: &str) -> Result<CensusKey, Self::Error> {
        self.format_retained(format_args!("native.{format}.{arena}"), "validation native census key").map(CensusKey::from_wire)
    }
    fn insert(&self, counts: &mut BTreeMap<CensusKey, usize>, key: CensusKey, count: usize, operation: &'static str) -> Result<(), Self::Error> {
        let work = key.as_str().len().checked_add(1)
            .and_then(|bytes| counts.len().checked_add(1).and_then(|count| bytes.checked_mul(count)))
            .ok_or_else(|| self.refuse_codec_limit("validation census key comparisons", u64::MAX - 1, u64::MAX))?;
        self.charge_work(u64_from_index(work), "validation census key comparisons")?;
        self.admit_retained_btree_record::<CensusKey, usize>(0, operation)?;
        counts.insert(key, count);
        Ok(())
    }
}

macro_rules! define_census {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        /// Count the borrowed view with one arena walk and allocation policy.
        pub(crate) fn count<S: CensusStorage>(storage: &S, view: NativeView<'_>) -> Result<BTreeMap<CensusKey, usize>, S::Error> {
            let mut counts = BTreeMap::new();
            $(storage.insert(&mut counts, CensusKey::model(ArenaName::registered(stringify!($field))), view.ir.model.$field.len(), "validation model census slots")?;)*
            storage.work(view.ir.model.surfaces.len(), "validation surface census scan")?;
            let unknown_surfaces = view.ir.model.surfaces.iter().filter(|surface| matches!(surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }))).count();
            storage.insert(&mut counts, CensusKey::surfaces_unknown_geometry(), unknown_surfaces, "validation surface census slot")?;
            view.visit(|work| storage.work(work, "validation native census scan"), |format, arena, records| {
                if records.len() != 0 {
                    let key = storage.native_key(format, arena)?;
                    storage.insert(&mut counts, key, records.len(), "validation native census slots")?;
                }
                Ok(())
            })?;
            Ok(counts)
        }
    };
}
super::arena_registry!(define_census);

#[cfg(test)]
mod tests {
    use super::{count, NativeView};
    use crate::document::CadIr;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn census_preserves_registered_and_nonempty_native_populations() {
        let mut ir = CadIr::empty();
        ir.native.namespace_mut("future").arenas_mut().insert("empty".into(), Vec::new());
        ir.native.namespace_mut("future").arenas_mut().insert("records".into(), vec![crate::native::NativeRecord::new(
            crate::ids::Identity::new("test:native:record#counted").unwrap(), serde_json::Map::new()).unwrap()]);
        let standard = ir.census();
        assert_eq!(standard["native.future.records"], 1);
        assert!(!standard.contains_key("native.future.empty"));
        for name in super::ArenaName::ALL { assert_eq!(standard[name.as_str()], 0); }
        let decoded = count(&cadmpeg_test_support::service_decode_context(), NativeView::new(&ir, None)).unwrap();
        assert_eq!(decoded, standard);
    }

    #[test]
    fn census_refuses_retained_nodes_items_and_comparison_work() {
        let ir = CadIr::empty();
        for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = count(&ctx, NativeView::new(&ir, None)) else { panic!("census must retain the caller refusal"); };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.operation, if dimension == ResourceDimension::WorkUnits { "validation census key comparisons" } else { "validation model census slots" });
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
            assert!(!ir.census().is_empty());
        }
        assert_eq!(count(&cadmpeg_test_support::service_decode_context(), NativeView::new(&ir, None)).unwrap(), ir.census());
    }
}
