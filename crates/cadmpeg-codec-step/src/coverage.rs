// SPDX-License-Identifier: Apache-2.0
//! Statically declared decode-coverage measures.

use cadmpeg_ir::report::decode::CoverageKey;

pub(crate) const DATA_SECTIONS: CoverageKey = CoverageKey::new("data_sections");
pub(crate) const ENTITY_INSTANCES: CoverageKey = CoverageKey::new("entity_instances");
pub(crate) const BYTES_STRUCTURAL: CoverageKey = CoverageKey::new("bytes_structural");
pub(crate) const BYTES_TYPED: CoverageKey = CoverageKey::new("bytes_typed");
pub(crate) const BYTES_NAMED_OPAQUE: CoverageKey = CoverageKey::new("bytes_named_opaque");
pub(crate) const BYTES_UNCLASSIFIED: CoverageKey = CoverageKey::new("bytes_unclassified");
