// SPDX-License-Identifier: Apache-2.0
//! The caller's strictness: which entries of a completed report refuse the
//! operation (CADIR v11 §3.4, Refusals).
//!
//! Decode, write and measurement take the same [`Strictness`]. The sealed
//! wrappers apply it to the completed report; no backend refuses on a report
//! entry. The default refuses nothing.

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;

use super::loss::{LossKind, LossNote};
use super::Severity;

/// The report entries on which a caller refuses an operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strictness {
    /// Refuse an entry whose code pins a strict floor its severity reaches.
    /// A decode at `Container` depth is exempt.
    pub pinned: bool,
    /// Refuse an entry at or above this severity.
    pub floor: Option<Severity>,
    /// Refuse an entry with one of these codes, spelled as [`code_name`]
    /// spells it, whatever its severity.
    pub codes: BTreeSet<NonBlankString>,
}

impl Strictness {
    /// Whether this strictness refuses `entry`. `container` states a decode at
    /// `Container` depth, whose losses are by request; it exempts `pinned`.
    #[must_use]
    pub fn refuses(&self, entry: &LossNote, container: bool) -> bool {
        self.floor.is_some_and(|floor| entry.severity >= floor)
            || (!self.codes.is_empty()
                && NonBlankString::new(code_name(&entry.code))
                    .is_some_and(|name| self.codes.contains(&name)))
            || (self.pinned
                && !container
                && entry
                    .code
                    .strict_floor()
                    .is_some_and(|floor| entry.severity >= floor))
    }

    /// The index of the first entry this strictness refuses.
    #[must_use]
    pub fn first_refused(&self, entries: &[LossNote], container: bool) -> Option<usize> {
        entries
            .iter()
            .position(|entry| self.refuses(entry, container))
    }

    /// Adds a code name to [`Self::codes`].
    #[must_use]
    pub fn with_code(mut self, name: &str) -> Self {
        if let Some(name) = NonBlankString::new(name) {
            self.codes.insert(name);
        }
        self
    }
}

/// Shared codes that §3.2 spells without a namespace, whichever codec
/// charges them.
const BARE_SHARED_CODES: [&str; 5] = [
    "record.unreadable",
    "sidecar.stale",
    "geometry.vertex-tolerance-widened",
    "target.source-dialect-displaced",
    "target.request-displaced",
];

/// Namespaces whose codes §3.2 spells `namespace:code`.
const COLON_NAMESPACES: [&str; 2] = ["export", "measure"];

/// The name of a loss code, as §3.2 spells a shared code and as its codec's
/// format specification spells a codec's code. [`Strictness::codes`] holds
/// these names.
///
/// - an `export` or `measure` code is `export:code` or `measure:code`;
/// - a shared code §3.2 spells bare (`record.unreadable`, `sidecar.stale`,
///   `geometry.vertex-tolerance-widened`, `target.source-dialect-displaced`,
///   `target.request-displaced`) is that code, whichever namespace holds it;
/// - every other code is `namespace/code`: `decode/depth.not-delivered`,
///   `read/spelling.normalized`, `iges/global.metadata-field-unusable`, and
///   `shared/<taxonomy>` for a code of the shared taxonomy.
#[must_use]
pub fn code_name(kind: &LossKind) -> String {
    let namespace = kind.namespace();
    let code = kind.local_code();
    if COLON_NAMESPACES.contains(&namespace) {
        format!("{namespace}:{code}")
    } else if BARE_SHARED_CODES.contains(&code) {
        code.to_owned()
    } else {
        format!("{namespace}/{code}")
    }
}

#[cfg(test)]
mod tests;
