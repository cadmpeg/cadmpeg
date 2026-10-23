// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.pvz` package decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`PvzLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text.
//!
//! [`PvzLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate: it fixes the loss category and severity from
//! the code, and it leaves only the per-instance message to the caller.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};
use cadmpeg_ir::report::Severity;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("pvz");

/// A stable, machine-readable identifier for one `.pvz` transfer loss.
///
/// The string form (via [`PvzLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PvzLossCode {
    /// The structure and geometry members were located but not decoded.
    PackageContentUndecoded,
    /// The structure member carries no signature of a declared dialect row.
    SourceDialectUnverified,
    /// The ZIP central directory was unreadable and members were recovered
    /// from local headers.
    ContainerDirectoryRecovered,
    /// Exact-stream records with a frame and no settled meaning were kept
    /// by tag and offset only.
    ExactRecordUninterpreted,
    /// A face carrier has no neutral surface form.
    ExactSurfaceUntyped,
    /// An analytic face carrier was written as its embedded B-spline.
    ExactSurfaceAsBspline,
    /// An edge curve has no neutral curve form; the edge has no carrier.
    ExactCurveUntyped,
    /// A coedge pcurve has no neutral pcurve form or domain.
    ExactPcurveUntransferred,
    /// Secondary pcurves of analytic faces lie in the embedded B-spline
    /// domain and were not written onto the analytic carrier.
    ExactSecondaryPcurveOmitted,
    /// Coedge pcurve images miss their edge-curve ends by more than the
    /// endpoint tolerance; the face tolerance admits the measured residual.
    ExactEndpointMismatch,
    /// A loop does not close, or a loop junction joins no vertex.
    ExactLoopOpen,
    /// A face side came from a gauge, not from an outward test.
    ExactFaceSideGauge,
}

impl PvzLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [PvzLossCode] = &[
        Self::PackageContentUndecoded,
        Self::SourceDialectUnverified,
        Self::ContainerDirectoryRecovered,
        Self::ExactRecordUninterpreted,
        Self::ExactSurfaceUntyped,
        Self::ExactSurfaceAsBspline,
        Self::ExactCurveUntyped,
        Self::ExactPcurveUntransferred,
        Self::ExactSecondaryPcurveOmitted,
        Self::ExactEndpointMismatch,
        Self::ExactLoopOpen,
        Self::ExactFaceSideGauge,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::PackageContentUndecoded => "package.content-undecoded",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::ContainerDirectoryRecovered => "container.directory-recovered",
            Self::ExactRecordUninterpreted => "exact.record-uninterpreted",
            Self::ExactSurfaceUntyped => "exact.surface-untyped",
            Self::ExactSurfaceAsBspline => "exact.surface-as-bspline",
            Self::ExactCurveUntyped => "exact.curve-untyped",
            Self::ExactPcurveUntransferred => "exact.pcurve-untransferred",
            Self::ExactSecondaryPcurveOmitted => "exact.secondary-pcurve-omitted",
            Self::ExactEndpointMismatch => "exact.endpoint-mismatch",
            Self::ExactLoopOpen => "exact.loop-open",
            Self::ExactFaceSideGauge => "exact.face-side-gauge",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::PackageContentUndecoded => Severity::Blocking,
            Self::ExactSurfaceUntyped => Severity::Error,
            Self::SourceDialectUnverified
            | Self::ContainerDirectoryRecovered
            | Self::ExactRecordUninterpreted
            | Self::ExactSurfaceAsBspline
            | Self::ExactCurveUntyped
            | Self::ExactPcurveUntransferred
            | Self::ExactSecondaryPcurveOmitted
            | Self::ExactEndpointMismatch
            | Self::ExactLoopOpen
            | Self::ExactFaceSideGauge => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::PackageContentUndecoded => LossTaxonomy::GeometryNotTransferred,
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::ContainerDirectoryRecovered => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ExactRecordUninterpreted => LossTaxonomy::RecordNotTyped,
            Self::ExactSurfaceUntyped | Self::ExactCurveUntyped => {
                LossTaxonomy::GeometryNotTransferred
            }
            Self::ExactSurfaceAsBspline | Self::ExactEndpointMismatch => {
                LossTaxonomy::DecodeDiagnostic
            }
            Self::ExactPcurveUntransferred | Self::ExactSecondaryPcurveOmitted => {
                LossTaxonomy::PcurveOmitted
            }
            Self::ExactLoopOpen => LossTaxonomy::SourceTopologyInvalid,
            Self::ExactFaceSideGauge => LossTaxonomy::TopologyGaugeSubstituted,
        }
    }

    /// The message of a note that counts `count` instances of this code.
    pub(crate) fn tally_message(self, count: usize) -> String {
        let what = match self {
            Self::PackageContentUndecoded => "packages left undecoded",
            Self::SourceDialectUnverified => "unverified dialect declarations",
            Self::ContainerDirectoryRecovered => "recovered ZIP directories",
            Self::ExactRecordUninterpreted => {
                "exact-stream records kept by tag and offset only (tag 0x0a and undefined tags)"
            }
            Self::ExactSurfaceUntyped => "faces whose carrier has no neutral surface form",
            Self::ExactSurfaceAsBspline => {
                "analytic faces written as their embedded B-spline carrier"
            }
            Self::ExactCurveUntyped => {
                "edges without a neutral curve (composite or undefined curve types)"
            }
            Self::ExactPcurveUntransferred => "coedge pcurves not transferred",
            Self::ExactSecondaryPcurveOmitted => {
                "secondary pcurves in the embedded B-spline domain not written onto analytic faces"
            }
            Self::ExactEndpointMismatch => {
                "faces whose pcurve images miss the edge ends by more than 2e-5 m"
            }
            Self::ExactLoopOpen => "open loops or loop junctions that join no vertex",
            Self::ExactFaceSideGauge => {
                "faces whose side is a propagation gauge, or side conflicts between faces"
            }
        };
        format!("{count} {what}")
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(NAMESPACE, self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `pvz/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::PvzLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = PvzLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "package.content-undecoded",
                "source.dialect-unverified",
                "container.directory-recovered",
                "exact.record-uninterpreted",
                "exact.surface-untyped",
                "exact.surface-as-bspline",
                "exact.curve-untyped",
                "exact.pcurve-untransferred",
                "exact.secondary-pcurve-omitted",
                "exact.endpoint-mismatch",
                "exact.loop-open",
                "exact.face-side-gauge",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in PvzLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in PvzLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
