// SPDX-License-Identifier: Apache-2.0
//! The `read/` loss vocabulary of a CADIR read (§3.2, §3.3 Reading CADIR).

use crate::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

const NAMESPACE: LossNamespace<'static> = crate::loss_namespace!("read");

/// A stable identifier for one repair of a CADIR read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadLossCode {
    /// A spelling with one reading is stated in its canonical form.
    SpellingNormalized,
    /// A value with one reading is stated as that value.
    ValueNormalized,
    /// An id or key is stated twice with different values; no copy is kept.
    RecordRepeated,
    /// A member the reader does not know is not stated.
    MemberIgnored,
}

impl ReadLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::SpellingNormalized => "spelling.normalized",
            Self::ValueNormalized => "value.normalized",
            Self::RecordRepeated => "record.repeated",
            Self::MemberIgnored => "member.ignored",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::SpellingNormalized => Severity::Info,
            Self::ValueNormalized | Self::MemberIgnored => Severity::Warning,
            Self::RecordRepeated => Severity::Error,
        }
    }

    pub(crate) fn kind(self) -> LossKind {
        NamespacedLossKind::new(NAMESPACE, self.code(), LossTaxonomy::DecodeDiagnostic)
            .with_strict_floor(Some(Severity::Info))
            .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}
