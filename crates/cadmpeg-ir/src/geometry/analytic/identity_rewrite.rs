// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, HyperbolaCurve,
    LineCurve, ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
};

rewrite_scalar!(CircleCurve);
rewrite_scalar!(ConeSurface);
rewrite_scalar!(CylinderSurface);
rewrite_scalar!(DegenerateCurve);
rewrite_scalar!(EllipseCurve);
rewrite_scalar!(HyperbolaCurve);
rewrite_scalar!(LineCurve);
rewrite_scalar!(ParabolaCurve);
rewrite_scalar!(PlaneSurface);
rewrite_scalar!(SphereSurface);
rewrite_scalar!(TorusSurface);
