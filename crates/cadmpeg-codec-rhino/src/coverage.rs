// SPDX-License-Identifier: Apache-2.0
//! Statically declared decode-coverage measures.

use cadmpeg_ir::CoverageKey;

pub(crate) const LEGACY_V1_ANNOTATIONS: CoverageKey = CoverageKey::new("legacy_v1_annotations");
pub(crate) const LEGACY_V1_BREPS: CoverageKey = CoverageKey::new("legacy_v1_breps");
pub(crate) const LEGACY_V1_CURVE_SEGMENTS: CoverageKey =
    CoverageKey::new("legacy_v1_curve_segments");
pub(crate) const LEGACY_V1_MESHES: CoverageKey = CoverageKey::new("legacy_v1_meshes");
pub(crate) const LEGACY_V1_NURBS_BREPS: CoverageKey = CoverageKey::new("legacy_v1_nurbs_breps");
pub(crate) const LEGACY_V1_NURBS_CURVES: CoverageKey = CoverageKey::new("legacy_v1_nurbs_curves");
pub(crate) const LEGACY_V1_NURBS_SURFACES: CoverageKey =
    CoverageKey::new("legacy_v1_nurbs_surfaces");
pub(crate) const LEGACY_V1_POINTS: CoverageKey = CoverageKey::new("legacy_v1_points");
