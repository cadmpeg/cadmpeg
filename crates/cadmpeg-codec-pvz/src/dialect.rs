// SPDX-License-Identifier: Apache-2.0
//! `.pvz` dialect identity: which registry row a package is, and how it was
//! admitted.
//!
//! The discriminant is the signature of the package's one `.pvs` structure
//! member. [`id`] pins one registry row per [`Generation`], and the match is
//! exhaustive, so a new generation cannot be added without a row. A structure
//! member with no known signature is `pvz:unknown`, admitted on the residual
//! path and charged [`PvzLossCode::SourceDialectUnverified`].
//!
//! The `.ol` signature is a property of each geometry member, not of the
//! package. Inspection reports it per member.

use cadmpeg_core::decode::View;
use cadmpeg_core::dialect::{DialectId, DialectMatch};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::PvzLossCode;
use crate::pvs::{self, Generation};

include!("dialect/registry_ids.rs");

/// Key of the structure-member signature in [`DialectMatch::declared`].
const DECLARED_PVS_SIGNATURE: &str = "pvs_signature";

/// A completed classification whose recovery state cannot drift from its match.
pub(crate) struct DialectClassification(State);

enum State {
    /// A known structure signature and the admitted identity it produced.
    Admitted(DialectMatch),
    /// No known structure signature: the residual identity.
    Residual(DialectMatch),
}

/// The registry row of one structure-member generation.
const fn id(generation: Generation) -> DialectId {
    match generation {
        Generation::Pvs0301 => PVZ_PVS_0301,
        Generation::Pvs0302 => PVZ_PVS_0302,
        Generation::Pvs0303 => PVZ_PVS_0303,
        Generation::Pvs0304 => PVZ_PVS_0304,
        Generation::Pvs0305 => PVZ_PVS_0305,
    }
}

/// Classifies a package from its opened structure member. The single
/// construction path for a [`DialectMatch`] in this codec.
pub(crate) fn classify(structure: View<'_>) -> DialectClassification {
    match pvs::generation(structure) {
        Some(generation) => DialectClassification(State::Admitted(
            DialectMatch::admitted(id(generation)).with_declared_entry(
                cadmpeg_core::nonblank_const!(DECLARED_PVS_SIGNATURE),
                generation.signature(),
            ),
        )),
        None => DialectClassification(State::Residual(DialectMatch::residual(PVZ_UNKNOWN))),
    }
}

impl DialectClassification {
    pub(crate) fn into_matched(self) -> DialectMatch {
        match self.0 {
            State::Admitted(matched) | State::Residual(matched) => matched,
        }
    }

    /// The loss charged exactly for the residual state.
    pub(crate) fn loss(&self) -> Option<LossNote> {
        match self.0 {
            State::Admitted(_) => None,
            State::Residual(_) => Some(PvzLossCode::SourceDialectUnverified.note(
                "the .pvs structure member opens with no known signature; the package was \
                 read without a declared structure grammar",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::View;
    use cadmpeg_core::dialect::Admission;

    use super::{
        classify, PVZ_PVS_0301, PVZ_PVS_0302, PVZ_PVS_0303, PVZ_PVS_0304, PVZ_PVS_0305, PVZ_UNKNOWN,
    };
    use crate::loss::PvzLossCode;

    #[test]
    fn known_signatures_are_admitted_with_their_declared_signature() {
        for (dialect, signature) in [
            (PVZ_PVS_0301, "PV-PVS0301"),
            (PVZ_PVS_0302, "PV-PVS0302"),
            (PVZ_PVS_0303, "PV-PVS0303"),
            (PVZ_PVS_0304, "PV-PVS0304"),
            (PVZ_PVS_0305, "PV-PVS0305"),
        ] {
            let mut member = signature.as_bytes().to_vec();
            member.extend_from_slice(b"\0 body");
            let classification = classify(View::over_retained(&member));
            assert!(classification.loss().is_none());
            let matched = classification.into_matched();
            assert_eq!(matched.dialect(), &dialect);
            assert_eq!(matched.admission(), &Admission::Admitted);
            assert_eq!(matched.declared().values().collect::<Vec<_>>(), [signature]);
        }
    }

    #[test]
    fn an_unknown_signature_is_residual_and_charged() {
        let classification = classify(View::over_retained(b"PV-PVS9999 body"));
        assert_eq!(
            classification.loss().map(|loss| loss.code),
            Some(PvzLossCode::SourceDialectUnverified.kind())
        );
        let matched = classification.into_matched();
        assert_eq!(matched.dialect(), &PVZ_UNKNOWN);
        assert_eq!(matched.admission(), &Admission::Residual);
    }
}
