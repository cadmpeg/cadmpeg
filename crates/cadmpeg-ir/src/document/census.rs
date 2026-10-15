// SPDX-License-Identifier: Apache-2.0
//! Arena populations under standard or live-context allocation.

use std::collections::BTreeMap;
use std::convert::Infallible;

use super::{ArenaName, CensusKey};
use crate::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use crate::native::view::NativeView;

macro_rules! define_census {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        /// Count the borrowed view with one arena walk.
        pub(crate) fn count(view: NativeView<'_>) -> BTreeMap<CensusKey, usize> {
            let mut counts = BTreeMap::new();
            $(counts.insert(CensusKey::model(ArenaName::registered(stringify!($field))), view.ir.model.$field.len());)*
            let unknown_surfaces = view.ir.model.surfaces.iter().filter(|surface| matches!(surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }))).count();
            counts.insert(CensusKey::surfaces_unknown_geometry(), unknown_surfaces);
            let Ok(()) = view.visit::<Infallible>(|format, arena, records| {
                if records.len() != 0 {
                    counts.insert(CensusKey::native(format, arena), records.len());
                }
                Ok(())
            });
            counts
        }
    };
}
super::arena_registry!(define_census);

#[cfg(test)]
mod tests {
    use super::{count, NativeView};
    use crate::document::CadIr;

    #[test]
    fn census_preserves_registered_and_nonempty_native_populations() {
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut("future")
            .arenas_mut()
            .insert("empty".into(), Vec::new());
        ir.native.namespace_mut("future").arenas_mut().insert(
            "records".into(),
            vec![crate::native::NativeRecord::new(
                crate::ids::Identity::new("test:native:record#counted").unwrap(),
                serde_json::Map::new(),
            )
            .unwrap()],
        );
        let standard = ir.census();
        assert_eq!(standard["native.future.records"], 1);
        assert!(!standard.contains_key("native.future.empty"));
        for name in super::ArenaName::ALL {
            assert_eq!(standard[name.as_str()], 0);
        }
        let decoded = count(NativeView::new(&ir, None));
        assert_eq!(decoded, standard);
    }
}
