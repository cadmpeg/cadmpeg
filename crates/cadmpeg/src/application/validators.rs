// SPDX-License-Identifier: Apache-2.0
//! The native-validator catalog.
//!
//! Codec-owned validators run over a decoded document's native namespaces.
//! Unlike the codec registry, this is an application concern: it belongs to
//! `cadmpeg check`, not to the four questions an embedder asks of a file.

use cadmpeg_ir::{CadIr, Finding};

type NativeValidator = fn(&CadIr) -> Vec<Finding>;

/// Maps native namespace ids to codec-owned validator functions.
pub struct NativeValidatorCatalog {
    entries: Vec<(&'static str, NativeValidator)>,
}

impl NativeValidatorCatalog {
    /// Registers the four native validators shipped with the CLI.
    pub fn with_builtins() -> Self {
        Self {
            entries: vec![
                #[cfg(feature = "fcstd")]
                ("fcstd", cadmpeg_codec_freecad::validate_native),
                #[cfg(feature = "f3d")]
                ("f3d", cadmpeg_codec_f3d::validate_native),
                #[cfg(feature = "inventor")]
                ("inventor", cadmpeg_codec_inventor::validate_native),
                #[cfg(feature = "sldprt")]
                ("sldprt", cadmpeg_codec_sldprt::validate_native),
            ],
        }
    }

    /// Stable namespace ids that have a registered validator.
    #[cfg(test)]
    fn namespaces(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|(namespace, _)| *namespace)
    }

    /// Runs every validator whose namespace is present on the document.
    pub fn validate(&self, ir: &CadIr) -> Vec<Finding> {
        self.entries
            .iter()
            .filter(|(namespace, _)| ir.native.namespace(namespace).is_some())
            .flat_map(|(_, validator)| validator(ir))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::CadIr;

    #[cfg(all(
        feature = "fcstd",
        feature = "f3d",
        feature = "inventor",
        feature = "sldprt"
    ))]
    #[test]
    fn native_validator_catalog_registers_the_four_shipped_validators() {
        let catalog = NativeValidatorCatalog::with_builtins();
        let mut namespaces = catalog.namespaces().collect::<Vec<_>>();
        namespaces.sort_unstable();
        assert_eq!(namespaces, ["f3d", "fcstd", "inventor", "sldprt"]);
    }

    #[cfg(all(feature = "fcstd", feature = "f3d"))]
    #[test]
    fn native_validator_catalog_invokes_two_validators_for_two_namespaces() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALLS: AtomicUsize = AtomicUsize::new(0);

        fn counting_fcstd(ir: &CadIr) -> Vec<Finding> {
            let _ = ir;
            CALLS.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        }
        fn counting_f3d(ir: &CadIr) -> Vec<Finding> {
            let _ = ir;
            CALLS.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        }

        let catalog = NativeValidatorCatalog {
            entries: vec![("fcstd", counting_fcstd), ("f3d", counting_f3d)],
        };
        CALLS.store(0, Ordering::SeqCst);
        let mut ir = CadIr::empty(Units::default());
        let _ = ir.native.namespace_mut("fcstd");
        let _ = ir.native.namespace_mut("f3d");
        let _ = catalog.validate(&ir);
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);

        CALLS.store(0, Ordering::SeqCst);
        let mut none = CadIr::empty(Units::default());
        let _ = none.native.namespace_mut("absent");
        let _ = catalog.validate(&none);
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    }
}
