// SPDX-License-Identifier: Apache-2.0
//! Statically declared decode-coverage measures.

use cadmpeg_ir::CoverageKey;

pub(crate) const UNKNOWN_RECORDS: CoverageKey = CoverageKey::new("unknown_records");
pub(crate) const UNKNOWN_SURFACE_FACES: CoverageKey = CoverageKey::new("unknown_surface_faces");
