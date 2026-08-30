// SPDX-License-Identifier: Apache-2.0
//! The native-validator catalog.
//!
//! Codec-owned validators run over a decoded document's native namespaces.
//! Unlike the codec registry, this is an application concern: it belongs to
//! `cadmpeg check`, not to the four questions an embedder asks of a file.

use cadmpeg_ir::{
    validate_neutral, validate_neutral_with_source_fidelity, CadIr, Finding, SourceFidelity,
    ValidationReport,
};

type NativeValidator = fn(&CadIr) -> Vec<Finding>;

pub(crate) fn validate_ir(
    validators: &NativeValidatorCatalog,
    ir: &CadIr,
    source_fidelity: Option<&SourceFidelity>,
    losses: Vec<cadmpeg_ir::LossNote>,
) -> ValidationReport {
    let mut report = match source_fidelity {
        Some(source_fidelity) => validate_neutral_with_source_fidelity(ir, source_fidelity, losses),
        None => validate_neutral(ir, losses),
    };
    report.findings.extend(validators.validate(ir));
    report
}

/// Maps native namespace ids to codec-owned validator functions.
pub struct NativeValidatorCatalog {
    entries: Vec<(&'static str, NativeValidator)>,
}

impl NativeValidatorCatalog {
    /// Registers the four native validators shipped with the CLI.
    pub fn with_builtins() -> Self {
        Self {
            entries: cadmpeg_registry::native_validators().collect(),
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
    use cadmpeg_registry::InputCatalog;

    #[test]
    fn native_validator_and_input_catalog_registrations_agree() {
        const VALIDATED_FORMATS: [&str; 4] = ["fcstd", "f3d", "inventor", "sldprt"];

        let mut validators = NativeValidatorCatalog::with_builtins()
            .namespaces()
            .collect::<Vec<_>>();
        validators.sort_unstable();
        let mut inputs = InputCatalog::with_builtins()
            .descriptors()
            .map(cadmpeg_registry::InputDescriptor::format_id)
            .filter(|id| VALIDATED_FORMATS.contains(id))
            .collect::<Vec<_>>();
        inputs.sort_unstable();

        assert_eq!(validators, inputs);
    }

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
