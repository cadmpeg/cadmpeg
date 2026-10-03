// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    AxisRevolutionSurfaceConstruction, BlendSurfacePayload, CompoundLoftSurfacePayload,
    CompoundSurfacePayload, DeformableSurfacePayload, ExactSurfacePayload,
    ExtrusionSurfaceConstruction, G2BlendSurfacePayload, LawSurfacePayload,
    LinearSweepSurfaceConstruction, LoftSurfacePayload, NetSurfacePayload,
    OffsetSurfaceConstruction, OrderedOptionalRange, ParallelOffsetSurfaceConstruction,
    RevolutionSurfaceConstruction, ScaledCompoundLoftSurfacePayload, SkinSurfacePayload,
    SubSurfaceConstruction, SubsetSurfaceConstruction, SumSurfaceConstruction, SweepSurfacePayload,
    TaperSurfaceConstruction, VariableBlendSurfacePayload, VertexBlendSurfacePayload,
};

rewrite_record!(AxisRevolutionSurfaceConstruction, []; {directrix, axis_origin, axis_direction});
rewrite_record!(BlendSurfacePayload, []; {supports, spine, radius, cross_section, cache, native_ranges});
rewrite_record!(CompoundLoftSurfacePayload, []; {construction, cache});
rewrite_record!(CompoundSurfacePayload, []; {components, cache});
rewrite_record!(DeformableSurfacePayload, []; {construction});
rewrite_record!(ExactSurfacePayload, []; {spline});
rewrite_record!(ExtrusionSurfaceConstruction, []; {directrix, parameter_interval, direction, native_position, cache});
rewrite_record!(G2BlendSurfacePayload, []; {construction, cache});
rewrite_record!(LawSurfacePayload, []; {construction});
rewrite_record!(LinearSweepSurfaceConstruction, []; {directrix, direction});
rewrite_record!(LoftSurfacePayload, []; {sections, parameters, closures, singularities, mode, bridge, cache});
rewrite_record!(NetSurfacePayload, []; {construction, cache});
rewrite_record!(OffsetSurfaceConstruction, []; {support, distance, u_sense, v_sense, linear_support_extension, extension});
rewrite_scalar!(OrderedOptionalRange);
rewrite_record!(ParallelOffsetSurfaceConstruction, []; {support, distance, self_intersect});
rewrite_record!(RevolutionSurfaceConstruction, []; {directrix, axis_origin, axis_direction, angular_interval, angular_parameter_interval, parameter_interval, transposed, cache});
rewrite_record!(ScaledCompoundLoftSurfacePayload, []; {construction, cache});
rewrite_record!(SkinSurfacePayload, []; {construction, cache});
rewrite_record!(SubSurfaceConstruction, []; {support, parameter_ranges});
rewrite_record!(SubsetSurfaceConstruction, []; {support, parameter_ranges, u_sense, v_sense, cache});
rewrite_record!(SumSurfaceConstruction, []; {first, second, basepoint, cache});
rewrite_record!(SweepSurfacePayload, []; {profile, spine, native});
rewrite_record!(TaperSurfaceConstruction, []; {support, reference, pcurve, parameter, taper, cache});
rewrite_record!(VariableBlendSurfacePayload, []; {construction, slice_range});
rewrite_record!(VertexBlendSurfacePayload, []; {construction});
