// SPDX-License-Identifier: Apache-2.0
//! eDrawings dialect identity: which registry row a package is, and how it
//! was admitted.
//!
//! The discriminant is the stream version that the `eModel` comment line
//! declares (docs/formats/edrawings.md section 2). [`classify`] pins one
//! registry row per known version. A version outside the table, and an
//! `eModel` member whose comment line does not read, is `edrawings:unknown`,
//! admitted on the residual path and charged
//! [`EdrawingsLossCode::SourceDialectUnverified`]. The residual path walks the
//! stream with the rules of the oldest admitted dialect.

use cadmpeg_core::dialect::{DialectId, DialectMatch};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::EdrawingsLossCode;
use crate::stream::StreamVersion;

include!("dialect/registry_ids.rs");

/// Key of the stream version in [`DialectMatch::declared`].
const DECLARED_HSF_VERSION: &str = "hsf_version";
/// The stream version of `edrawings:hsf-19-10`.
const VERSION_19_10: &str = "19.10";

/// The version of `edrawings:hsf-18-00`.
const VERSION_18_00: &str = "18.00";
/// The version of `edrawings:hsf-19-00`.
const VERSION_19_00: &str = "19.00";

/// The record rules a stream is walked with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grammar {
    /// Streams re-saved by the eDrawings viewer (`edrawings:hsf-19-10`).
    Resaved,
    /// Streams written directly by `SolidWorks` (`edrawings:hsf-18-00`).
    SolidWorks,
    /// Streams written by `SolidWorks` with version 19.00
    /// (`edrawings:hsf-19-00`): the layout and tree of `SolidWorks`, the
    /// image object header of `Resaved`.
    SolidWorksLate,
}

impl Grammar {
    /// Whether the header block, the `V` forms, the texture record `t`, and
    /// the bare line style run follow the `SolidWorks` layout.
    pub(crate) const fn solidworks_layout(self) -> bool {
        matches!(self, Self::SolidWorks | Self::SolidWorksLate)
    }

    /// Whether an image object has the one byte header.
    pub(crate) const fn short_image_header(self) -> bool {
        matches!(self, Self::SolidWorks)
    }
}

/// A completed classification whose recovery state cannot drift from its match.
pub(crate) struct DialectClassification(State, Grammar);

enum State {
    /// A known stream version and the admitted identity it produced.
    Admitted(DialectMatch),
    /// No known stream version: the residual identity.
    Residual(DialectMatch),
}

/// Classifies a package from the stream version of its `eModel` member, when
/// the comment line read. The single construction path for a
/// [`DialectMatch`] in this codec.
pub(crate) fn classify(version: Option<&StreamVersion>) -> DialectClassification {
    match version {
        Some(version) if version.text() == VERSION_19_10 => DialectClassification(
            State::Admitted(
                DialectMatch::admitted(EDRAWINGS_HSF_19_10).with_declared_entry(
                    cadmpeg_core::nonblank_const!(DECLARED_HSF_VERSION),
                    version.text(),
                ),
            ),
            Grammar::Resaved,
        ),
        Some(version) if version.text() == VERSION_18_00 => DialectClassification(
            State::Admitted(
                DialectMatch::admitted(EDRAWINGS_HSF_18_00).with_declared_entry(
                    cadmpeg_core::nonblank_const!(DECLARED_HSF_VERSION),
                    version.text(),
                ),
            ),
            Grammar::SolidWorks,
        ),
        Some(version) if version.text() == VERSION_19_00 => DialectClassification(
            State::Admitted(
                DialectMatch::admitted(EDRAWINGS_HSF_19_00).with_declared_entry(
                    cadmpeg_core::nonblank_const!(DECLARED_HSF_VERSION),
                    version.text(),
                ),
            ),
            Grammar::SolidWorksLate,
        ),
        Some(version) => DialectClassification(
            State::Residual(
                DialectMatch::residual(EDRAWINGS_UNKNOWN).with_declared_entry(
                    cadmpeg_core::nonblank_const!(DECLARED_HSF_VERSION),
                    version.text(),
                ),
            ),
            Grammar::Resaved,
        ),
        None => DialectClassification(
            State::Residual(DialectMatch::residual(EDRAWINGS_UNKNOWN)),
            Grammar::Resaved,
        ),
    }
}

impl DialectClassification {
    /// The record rules of the stream.
    pub(crate) const fn grammar(&self) -> Grammar {
        self.1
    }

    pub(crate) fn into_matched(self) -> DialectMatch {
        match self.0 {
            State::Admitted(matched) | State::Residual(matched) => matched,
        }
    }

    /// The loss charged exactly for the residual state.
    pub(crate) fn loss(&self) -> Option<LossNote> {
        match self.0 {
            State::Admitted(_) => None,
            State::Residual(_) => Some(EdrawingsLossCode::SourceDialectUnverified.note(
                "the eModel member declares no known stream version; the package was read \
                 without a declared stream grammar",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::View;
    use cadmpeg_core::dialect::Admission;

    use super::{
        classify, Grammar, EDRAWINGS_HSF_18_00, EDRAWINGS_HSF_19_00, EDRAWINGS_HSF_19_10,
        EDRAWINGS_UNKNOWN,
    };
    use crate::loss::EdrawingsLossCode;
    use crate::stream::read_wrapper;
    use crate::test_support::emodel_with;

    fn version(text: &str) -> crate::stream::StreamVersion {
        let member = emodel_with(text, &[], &[], &[]);
        read_wrapper(View::over_retained(&member)).unwrap().version
    }

    #[test]
    fn the_observed_version_is_admitted_with_its_declared_version() {
        let version = version("19.10");
        let classification = classify(Some(&version));
        assert!(classification.loss().is_none());
        let matched = classification.into_matched();
        assert_eq!(matched.dialect(), &EDRAWINGS_HSF_19_10);
        assert_eq!(matched.admission(), &Admission::Admitted);
        assert_eq!(matched.declared().values().collect::<Vec<_>>(), ["19.10"]);
    }

    #[test]
    fn the_solidworks_version_is_admitted_with_its_grammar() {
        let version = version("18.00");
        let classification = classify(Some(&version));
        assert!(classification.loss().is_none());
        assert_eq!(classification.grammar(), Grammar::SolidWorks);
        let matched = classification.into_matched();
        assert_eq!(matched.dialect(), &EDRAWINGS_HSF_18_00);
        assert_eq!(matched.admission(), &Admission::Admitted);
        assert_eq!(matched.declared().values().collect::<Vec<_>>(), ["18.00"]);
    }

    #[test]
    fn the_late_solidworks_version_is_admitted_with_its_grammar() {
        let version = version("19.00");
        let classification = classify(Some(&version));
        assert!(classification.loss().is_none());
        assert_eq!(classification.grammar(), Grammar::SolidWorksLate);
        let matched = classification.into_matched();
        assert_eq!(matched.dialect(), &EDRAWINGS_HSF_19_00);
        assert_eq!(matched.admission(), &Admission::Admitted);
        assert_eq!(matched.declared().values().collect::<Vec<_>>(), ["19.00"]);
    }

    #[test]
    fn another_version_or_no_version_is_residual_and_charged() {
        let other = version("20.00");
        for classification in [classify(Some(&other)), classify(None)] {
            assert_eq!(
                classification.loss().map(|loss| loss.code),
                Some(EdrawingsLossCode::SourceDialectUnverified.kind())
            );
            let matched = classification.into_matched();
            assert_eq!(matched.dialect(), &EDRAWINGS_UNKNOWN);
            assert_eq!(matched.admission(), &Admission::Residual);
        }
    }
}
