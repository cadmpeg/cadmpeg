// SPDX-License-Identifier: Apache-2.0
//! The report entries on which a caller refuses a completed operation
//! (§3.4, Refusals).

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;

use super::loss::LossNote;
use super::Severity;

/// The report entries a caller refuses. The default refuses nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strictness {
    /// Refuses an entry whose code pins a strict floor its severity reaches.
    pub pinned: bool,
    /// Refuses an entry whose severity is at least this.
    pub floor: Option<Severity>,
    /// Refuses an entry whose code is one of these, spelled as §3.2 or the
    /// codec's format specification spells it.
    pub codes: BTreeSet<NonBlankString>,
}

impl Strictness {
    /// Whether this strictness refuses `entry`.
    #[must_use]
    pub fn refuses(&self, entry: &LossNote) -> bool {
        if self.floor.is_some_and(|floor| entry.severity >= floor) {
            return true;
        }
        if self.pinned
            && entry
                .code
                .strict_floor()
                .is_some_and(|floor| entry.severity >= floor)
        {
            return true;
        }
        if self.codes.is_empty() {
            return false;
        }
        let namespace = entry.code.namespace();
        let local = entry.code.local_code();
        [
            format!("{namespace}/{local}"),
            format!("{namespace}:{local}"),
            local.to_owned(),
        ]
        .iter()
        .any(|spelling| self.codes.contains(spelling.as_str()))
    }

    /// The index of the first entry of `report` this strictness refuses.
    #[must_use]
    pub fn first_refused(&self, report: &[LossNote]) -> Option<usize> {
        report.iter().position(|entry| self.refuses(entry))
    }
}

#[cfg(test)]
mod tests {
    use super::Strictness;
    use crate::report::loss::{LossNote, LossTaxonomy, NamespacedLossKind};
    use crate::report::Severity;

    fn entry(severity: Severity, floor: Option<Severity>) -> LossNote {
        let kind = NamespacedLossKind::new(
            crate::loss_namespace!("read"),
            "spelling.normalized",
            LossTaxonomy::DecodeDiagnostic,
        )
        .with_strict_floor(floor);
        LossNote::new(kind, "message").with_severity(severity)
    }

    #[test]
    fn the_default_refuses_nothing_and_each_rule_refuses_its_entries() {
        let report = [
            entry(Severity::Info, None),
            entry(Severity::Warning, Some(Severity::Info)),
        ];
        assert_eq!(Strictness::default().first_refused(&report), None);
        let floor = Strictness {
            floor: Some(Severity::Warning),
            ..Strictness::default()
        };
        assert_eq!(floor.first_refused(&report), Some(1));
        let pinned = Strictness {
            pinned: true,
            ..Strictness::default()
        };
        assert_eq!(pinned.first_refused(&report), Some(1));
        for spelling in ["read/spelling.normalized", "spelling.normalized"] {
            let codes = Strictness {
                codes: [cadmpeg_core::text::NonBlankString::new(spelling).expect("code")].into(),
                ..Strictness::default()
            };
            assert_eq!(codes.first_refused(&report), Some(0), "{spelling}");
        }
    }
}
