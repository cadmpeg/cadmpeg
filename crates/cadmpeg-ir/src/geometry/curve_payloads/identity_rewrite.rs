// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{DeformableCurveConstruction, OffsetCurveConstruction, ProjectionCurvePayload, SilhouetteCurveConstruction, SpatialOffsetCurveConstruction, SpringCurvePayload, SubsetCurveConstruction, SurfaceOffsetCurveConstruction, ThreeSurfaceIntersectionCurvePayload, TwoSidedOffsetCurveConstruction, VectorOffsetCurveConstruction};

rewrite_record!(DeformableCurveConstruction, []; {context, cache_first, source, source_parameter_range, data});
rewrite_record!(OffsetCurveConstruction, []; {source, distance, side, range});
rewrite_record!(ProjectionCurvePayload, []; {context, discontinuity_flag, source, tail});
rewrite_record!(SilhouetteCurveConstruction, []; {context, silhouette, cast_surface, light_direction});
rewrite_record!(SpatialOffsetCurveConstruction, []; {source, distance, reference_direction, self_intersect});
rewrite_record!(SpringCurvePayload, []; {layout, context, direction});
rewrite_record!(SubsetCurveConstruction, []; {source, parameter_range, sense, cache});
rewrite_record!(SurfaceOffsetCurveConstruction, []; {context, discontinuity_flag, base_u_range, base_v_range, base, base_range, base_endpoints, cache, distance, shift, scale});
rewrite_record!(ThreeSurfaceIntersectionCurvePayload, []; {context, selector, third});
rewrite_record!(TwoSidedOffsetCurveConstruction, []; {context, discontinuity_flag, offsets, cache});
rewrite_record!(VectorOffsetCurveConstruction, []; {source, parameter_range, offset, roles, cache});
