// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, CacheContract, CacheFirstCurveForm,
    CacheFirstCurveParameterization, ClassicLoftProfileData, CompositeCurveSegment,
    CompositeCurveSegments, CompositeCurveTransition, CompoundComponent, CompoundCurveConstruction,
    CompoundLoftConstruction, CompoundLoftDirection, CompoundLoftScale, CompoundLoftScaleMember,
    CompoundLoftTail, Curve, CurveGeometry, CurveOffsetCoordinate, CurveOffsetDistanceLaw,
    CurveOffsetLawBasis, CurveOffsetRange, DeformableCurveData, DeformableCurveSource,
    DeformableSurfaceConstruction, DeformableSurfaceData, DeformableSurfaceFrame,
    DeformableVectorFrame, DirectedParameterRange, EdgeOffsetDiscriminator, ExactSpline,
    FiniteLawFormula, FitTolerance, G2BlendConstruction, G2BlendFirstShape, G2BlendFullSupport,
    G2BlendSide, HelixCircleProfile, HelixCurveConstruction, HelixLineProfile,
    HelixPathConstruction, HelixSurfaceConstruction, HelixSurfaceProfile,
    InlineTSplineSubtransform, IntcurveSupportContext, IntcurveSupportSide, LawCurveVersionForm,
    LawExpression, LawFormula, LawSurfaceConstruction, LawSurfaceTail, LegacyCache,
    LegacyExtensionFlags, LoftBridgeToken, LoftMemberForm, LoftPath, LoftPathCurve,
    LoftProfileMember, LoftRevisionForm, LoftSection, LoftSectionEntry, LoftSubdata,
    LoftSubdataRow, LoftSubdataTable, NetSurfaceConstruction, OffsetExtension, OffsetSide,
    ParametricSurfaceCurveFlags, PlacedCurve, PlacedSurface, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, ProjectionRole,
    ProjectionTail, RecordBounds, RevisionCacheForm, RevisionCompoundLoftConstruction,
    RevisionCompoundLoftTail, RevisionG2BlendConstruction, RevisionSurfaceForm,
    RevisionSurfaceParameterization, RollingBallConstruction, RollingBallJetDerivative,
    RollingBallJetSite, RollingBallJetStation, RollingBallJetStations, RollingBallRadiusSelector,
    RollingBallSide, RollingBallSideExtension, RollingBallSupportCurve, RollingBallSupportSurface,
    RollingBallThirdSide, ScaledCompoundLoftBranch, ScaledCompoundLoftConstruction,
    ScaledCompoundLoftShape, SilhouetteKind, SkinSurfaceConstruction, SkinSurfaceLayout,
    SkinSurfaceProfile, SolvedCurveGeometry, SolvedSurfaceGeometry, SplineSurfaceParameters,
    SpringLayout, SpringPcurve, SpringSupport, SubtypeTableIndex, SupportPcurve, Surface,
    SurfaceCurveCacheFirst, SurfaceCurveFamily, SurfaceCurveTail, SurfaceGeometry,
    SweepRevisionForm, SweepSurfaceConstruction, SweepSurfaceLayout, TSplineSubtransform,
    TSplineSurfaceConstruction, TaperSurfaceKind, TolerantIntersectionConstruction,
    TolerantIntersectionParameterization, VariableBlendBareCrossSection, VariableBlendCache,
    VariableBlendConstruction, VariableBlendConvexity, VariableBlendCrossSection,
    VariableBlendInterpolationPoint, VariableBlendRadii, VariableBlendRenderMode,
    VariableBlendSupportKind, VariableBlendSurfaceSubtype, VariableBlendTerminal,
    VariableBlendValue, VariableBlendValuePayload, VectorOffsetRoles, VertexBlendBoundary,
    VertexBlendBoundaryGeometry, VertexBlendConstruction, VertexBlendTwists,
};

rewrite_enum!(BlendCrossSection, []; {
    Circular,
    Conic,
    Polynomial,
});
rewrite_enum!(BlendRadiusLaw, []; {
    Constant {signed_radius},
    Linear {start, end},
    Law {curve},
});
rewrite_record!(BlendSupport, []; {surface, reversed});
rewrite_enum!(CacheContract<F>, [F]; {
    Legacy {cache},
    Revision {form},
});
rewrite_record!(CacheFirstCurveForm<R>, [R]; {revision, cache, support_bounds, solved_range, extension});
rewrite_record!(CacheFirstCurveParameterization<R>, [R]; {interval, closed_form});
rewrite_record!(ClassicLoftProfileData<R, V>, [R, V]; {surface, pcurve, first_flag, asm_extension, subdata, direction});
rewrite_record!(CompositeCurveSegment, []; {curve, same_sense, transition});
rewrite_record!(CompositeCurveSegments, []; (field0));
rewrite_scalar!(CompositeCurveTransition);
rewrite_record!(CompoundComponent<T, R>, [T, R]; {parameter, component});
rewrite_record!(CompoundCurveConstruction, []; {parameters, components, cache});
rewrite_record!(CompoundLoftConstruction<R, V>, [R, V]; {scales, flags, tail});
rewrite_enum!(CompoundLoftDirection<V>, [V]; {
    Vector {value},
    Curve {curve, selector},
});
rewrite_record!(CompoundLoftScale<R, V>, [R, V]; {members, path, auxiliaries, tail});
rewrite_record!(CompoundLoftScaleMember<R, V>, [R, V]; {type_code, curve, data});
rewrite_enum!(CompoundLoftTail<R, V>, [R, V]; {
    Six {flags, scale, selector, direction, parameter_range, curve},
    Seven {first_flag, first_scale, second_flag, second_scale, selector, direction, trailing_flags},
    Zero {flags, direction, trailing_flags},
});
rewrite_record!(Curve, []; {id, geometry, parameter_range, source_object});
rewrite_enum!(CurveGeometry, []; {
    Procedural {construction, cache},
    Solved(field0),
});
rewrite_scalar!(CurveOffsetCoordinate);
rewrite_enum!(CurveOffsetDistanceLaw<R>, [R]; {
    Linear {basis, distances, control_range},
    Coordinate {function, coordinate, basis, function_parameter_offset, function_parameter_scale},
});
rewrite_scalar!(CurveOffsetLawBasis);
rewrite_enum!(CurveOffsetRange<R>, [R]; {
    Uniform {parameter_range},
    Variable {parameter_range, distance_law},
});
rewrite_enum!(DeformableCurveData<R, V, P>, [R, V, P]; {
    VectorField {vectors, parameter_pairs},
    Mode3 {leading_vectors, leading_parameter, leading_flags, trailing_point, trailing_vectors, frame_parameter, frame_flags, parameters, trailing_flags, trailing_parameter, trailing_value},
});
rewrite_enum!(DeformableCurveSource, []; {
    Curve {curve},
    NativeReference {flag, index},
});
rewrite_record!(DeformableSurfaceConstruction<R, V, P>, [R, V, P]; {support, data, cache, discontinuities, discontinuity_flag});
rewrite_enum!(DeformableSurfaceData<R, V, P>, [R, V, P]; {
    Full {leading_vectors, leading_parameter, leading_flags, selector, surface, native_id, flag, first_parameter, version_value, second_parameter, curve, frames, trailing_value},
    SurfaceCurve {surface, native_id, flag, first_parameter, selector, second_parameter, curve, vectors, frame_parameter, flags, parameter_triples},
    Plain {frame, parameter_triples},
    Guided {frame, selector, guide_parameter},
    Minimal {vectors, selector},
    RevisionMode3 {leading_vectors, leading_parameter, leading_flags, trailing_point, trailing_vectors, frame_parameter, frame_flags, parameters, trailing_flags, trailing_parameter, trailing_value},
});
rewrite_record!(DeformableSurfaceFrame<R, V, P>, [R, V, P]; {leading_vectors, leading_parameter, leading_flags, secondary_vectors, secondary_parameter, secondary_flags, point, trailing_flags});
rewrite_record!(DeformableVectorFrame<R, V>, [R, V]; {vectors, parameter, flags});
rewrite_scalar!(DirectedParameterRange);
rewrite_scalar!(EdgeOffsetDiscriminator);
rewrite_enum!(ExactSpline<R>, [R]; {
    Legacy {ranges, extension, cache},
    Revision {intervals, extension, form},
});
rewrite_record!(FiniteLawFormula, []; (field0));
rewrite_scalar!(FitTolerance);
rewrite_record!(G2BlendConstruction<R, V>, [R, V]; {first, singularity, first_shape, second, second_exact_surface, center_curve, center_parameters, center_flag, parameter_ranges, trailing_parameters, discontinuities});
rewrite_enum!(G2BlendFirstShape<R>, [R]; {
    Full {support},
    None {coefficients, tolerance, extension, pcurve},
});
rewrite_record!(G2BlendFullSupport, []; {surface, tolerance});
rewrite_record!(G2BlendSide<V>, [V]; {label, surface, curve, pcurves, direction});
rewrite_scalar!(HelixCircleProfile);
rewrite_scalar!(HelixCurveConstruction);
rewrite_scalar!(HelixLineProfile);
rewrite_scalar!(HelixPathConstruction);
rewrite_scalar!(HelixSurfaceConstruction);
rewrite_scalar!(HelixSurfaceProfile);
rewrite_record!(InlineTSplineSubtransform, []; {program, separator, values});
rewrite_record!(IntcurveSupportContext, []; {sides, parameter_range, discontinuities});
rewrite_record!(IntcurveSupportSide, []; {surface, pcurve});
rewrite_record!(LawCurveVersionForm, []; {stamp, post_enum, parameter_range});
rewrite_enum!(LawExpression<R, V, P>, [R, V, P]; {
    Null {},
    Text {value},
    Integer {value},
    Double {value},
    Point {value},
    Vector {value},
    Transform {scalars, enums},
    TransformVec {vectors, scale, flags},
    Edge {curve, parameters},
    Spline {native_id, knots, controls, point},
    Algebraic {operator, operands},
});
rewrite_enum!(LawFormula<R, V, P>, [R, V, P]; {
    Null {},
    Named {name, variables},
});
rewrite_record!(LawSurfaceConstruction<R, V, P>, [R, V, P]; {parameter_ranges, primary, additional, tail, discontinuities});
rewrite_enum!(LawSurfaceTail<R>, [R]; {
    Full {cache},
    Summary {parameters, fit_tolerance, closures, singularities},
    None {parameter_ranges, closures, singularities},
    Historical {},
    Optimal {},
});
rewrite_scalar!(LegacyCache);
rewrite_scalar!(LegacyExtensionFlags);
rewrite_enum!(LoftBridgeToken<R>, [R]; {
    Boolean(field0),
    Integer(field0),
    Double(field0),
    Text(field0),
    Enum(field0),
});
rewrite_enum!(LoftMemberForm<R, V>, [R, V]; {
    Support {type_code, surface, support_bounds, pcurve, first_flag, asm_extension, subdata, direction},
    PcurvePair {pcurve, secondary_pcurve, asm_extension, subdata, direction},
});
rewrite_record!(LoftPath<R>, [R]; {path, auxiliaries, flag});
rewrite_record!(LoftPathCurve<R>, [R]; {id, endpoints});
rewrite_record!(LoftProfileMember<R, V>, [R, V]; {profile, form});
rewrite_record!(LoftRevisionForm<R>, [R]; {revision, flags, ints, cache, discontinuities, tail_flag});
rewrite_record!(LoftSection<R, V>, [R, V]; {entries});
rewrite_record!(LoftSectionEntry<R, V>, [R, V]; {parameter, profile, path});
rewrite_enum!(LoftSubdata<R>, [R]; {
    Type211 {dimensions, row},
    Table(field0),
});
rewrite_record!(LoftSubdataRow<R>, [R]; {parameters, columns, extra});
rewrite_record!(LoftSubdataTable<R>, [R]; {type_code, rows, row_count, column_count});
rewrite_record!(NetSurfaceConstruction<R, V, P>, [R, V, P]; {sections, frame_parameters, flag, directions, formulas, discontinuities, discontinuity_flag});
rewrite_enum!(OffsetExtension<R>, [R]; {
    Legacy {flags, cache},
    Revision {form},
});
rewrite_enum!(OffsetSide<V>, [V]; {
    PlaneNormal {normal},
    Direction {direction, support},
});
rewrite_scalar!(ParametricSurfaceCurveFlags);
rewrite_record!(PlacedCurve, []; {basis, transform, depth});
rewrite_record!(PlacedSurface, []; {basis, transform, depth});
rewrite_record!(ProceduralCurve, []; {id, definition});
rewrite_enum!(ProceduralCurveDefinition, []; {
    Exact {cache},
    Law {context, version, extension, primary, additional, cache},
    Compound(field0),
    Helix(field0),
    Intersection {context, discontinuity_flag, cache},
    TolerantIntersection {construction, parameterization, cache},
    ThreeSurfaceIntersection(field0),
    SurfaceCurve {family},
    Silhouette(field0),
    SurfaceOffset(field0),
    Spring(field0),
    Deformable(field0),
    Projection(field0),
    Offset(field0),
    SpatialOffset(field0),
    TwoSidedOffset(field0),
    VectorOffset(field0),
    Subset(field0),
    Replica {source, transform},
    BlendSpine {blend_surface},
    Unknown {native_kind, record, cache},
});
rewrite_record!(ProceduralSurface, []; {id, definition, record_bounds});
rewrite_enum!(ProceduralSurfaceDefinition, []; {
    Exact(field0),
    Compound(field0),
    SubSurface(field0),
    Taper(field0),
    Loft(field0),
    CompoundLoft(field0),
    RevisionCompoundLoft {construction},
    ScaledCompoundLoft(field0),
    Skin(field0),
    Law(field0),
    Net(field0),
    G2Blend(field0),
    RevisionG2Blend {construction},
    VariableBlend(field0),
    VertexBlend(field0),
    Extrusion(field0),
    LinearSweep(field0),
    Revolution(field0),
    AxisRevolution(field0),
    Sum(field0),
    Sweep(field0),
    TSpline {construction},
    Helix {construction},
    Deformable(field0),
    Offset(field0),
    Subset(field0),
    Replica {source, transform},
    ParallelOffset(field0),
    DegenerateTorus {select_outer},
    CurveBounded {support, boundaries, boundary_pcurves, implicit_outer},
    Ruled {first, second, cache},
    Blend(field0),
    RollingBallJet(field0),
    Unknown {record, cache},
});
rewrite_scalar!(ProjectionRole);
rewrite_enum!(ProjectionTail<R>, [R]; {
    EarlyClose {flag},
    Ranged {flag, parameter_range, role},
});
rewrite_scalar!(RecordBounds);
rewrite_enum!(RevisionCacheForm<P>, [P]; {
    SolvedCache {fit_tolerance},
    Parameterization(field0),
});
rewrite_record!(RevisionCompoundLoftConstruction, []; {revision, cache, discontinuities, tail_flag, base_profile, base_path, entries, flags, kind_flags, direction, tail});
rewrite_enum!(RevisionCompoundLoftTail<T, S>, [T, S]; {
    Unbounded {},
    LowerBound {lower},
    UpperBound {upper},
    Curve {interval, curve},
});
rewrite_record!(RevisionG2BlendConstruction, []; {revision, leading_parameters, sides, center, center_range, radii, radius_selector, u_range, v_range, shape_prefix, shape_parameter, shape_length, shape_tail, cache, discontinuities, tail_flag, tail_extensions});
rewrite_record!(RevisionSurfaceForm<F, R>, [F: Default, R]; {revision, support_bounds, reference_endpoints, second_endpoints, flags, cache, discontinuities, tail_flag, trailing_flags});
rewrite_record!(RevisionSurfaceParameterization<R>, [R]; {u_interval, v_interval, u_closure, v_closure, u_singularity, v_singularity});
rewrite_record!(RollingBallConstruction<R, V, P>, [R, V, P]; {revision, sides, slice, slice_range, offsets, radius_selector, u_range, v_range, shape_prefix, parameters, tail, cache, discontinuities, tail_flag, third, tail_extensions});
rewrite_record!(RollingBallJetDerivative<R, V>, [R, V]; {first_limit, second_limit, center, angle});
rewrite_record!(RollingBallJetSite<R, V, P>, [R, V, P]; {first_limit, second_limit, center, angle, first_derivative, second_derivative});
rewrite_record!(RollingBallJetStation<R, V, P>, [R, V, P]; {knot, multiplicity, site});
rewrite_record!(RollingBallJetStations, []; {degree, stations});
rewrite_enum!(RollingBallRadiusSelector<T>, [T]; {
    None {},
    Value {value},
});
rewrite_record!(RollingBallSide<S, C, P, R, L>, [S, C, P, R, L]; {support_kind, surface, curve, pcurve, location, secondary_pcurve, extension});
rewrite_record!(RollingBallSideExtension<P>, [P]; {value, pcurve});
rewrite_record!(RollingBallSupportCurve<C, R>, [C, R]; {curve, parameter_range});
rewrite_record!(RollingBallSupportSurface<S, R>, [S, R]; {surface, parameter_ranges});
rewrite_record!(RollingBallThirdSide<V>, [V]; {label, surface, curve, pcurve, direction, secondary_pcurve, extension, tertiary_pcurve, flag});
rewrite_enum!(ScaledCompoundLoftBranch<R, V>, [R, V]; {
    ExtendedVector {first_scale, second_scale, selector, direction},
    ExtendedCurve {scale, flag, singularity, curve},
    Direct {flag, direction},
});
rewrite_record!(ScaledCompoundLoftConstruction<R, V>, [R, V]; {singularity, shape, discontinuities, discontinuity_flag, scales, flags, selector, branch, trailing_flags, tail_kind, tail_directions, tail_singularity, tail_curve});
rewrite_enum!(ScaledCompoundLoftShape<R>, [R]; {
    Full {},
    None {parameter_ranges, parameters},
});
rewrite_enum!(SilhouetteKind, []; {
    Standard {},
    Parametric {},
    Taper {draft_factor},
});
rewrite_record!(SkinSurfaceConstruction<R, V, P>, [R, V, P]; {surface_boolean, surface_normal, surface_direction, count, parameter, layout, direction, trailing_parameter, formula, parameter_curve, discontinuities, discontinuity_flag});
rewrite_enum!(SkinSurfaceLayout<R, V>, [R, V]; {
    Profiles {profiles, path, tail},
    Compact {inner_count, curve, subdata, first_tail, secondary_curve, second_tail},
});
rewrite_record!(SkinSurfaceProfile<R, V>, [R, V]; {type_code, curve, data});
rewrite_enum!(SolvedCurveGeometry, []; {
    Line(field0),
    Circle(field0),
    Ellipse(field0),
    Parabola(field0),
    Hyperbola(field0),
    Degenerate(field0),
    Composite {segments, self_intersect},
    Nurbs(field0),
    Polyline(field0),
    Transformed(field0),
    Unknown {record},
});
rewrite_enum!(SolvedSurfaceGeometry, []; {
    Plane(field0),
    Cylinder(field0),
    Cone(field0),
    Sphere(field0),
    Torus(field0),
    Nurbs(field0),
    Polygonal(field0),
    Transformed(field0),
    Unknown {record},
});
rewrite_enum!(SplineSurfaceParameters<R>, [R]; {
    OrderedRanges {ranges},
    RevisionRanges {intervals},
});
rewrite_enum!(SpringLayout<R, I>, [R, I]; {
    ContextFirst {supports, first_pcurve, second_pcurve, parameter_range, discontinuities, discontinuity_flag, cache},
    CacheFirst {context, form},
});
rewrite_enum!(SpringPcurve<R>, [R]; {
    Pcurve(field0),
    Range(field0),
});
rewrite_enum!(SpringSupport<R>, [R]; {
    Surface(field0),
    Ranges(field0),
});
rewrite_scalar!(SubtypeTableIndex);
rewrite_record!(SupportPcurve, []; {geometry, parameter_range});
rewrite_record!(Surface, []; {id, geometry, source_object});
rewrite_record!(SurfaceCurveCacheFirst<F>, [F]; {form, flags});
rewrite_enum!(SurfaceCurveFamily, []; {
    Blend {context, tail},
    SurfaceConstrained {context, tail},
    Parametric {context, tail},
    Skin {context, tail},
});
rewrite_record!(SurfaceCurveTail, []; {extension, revision, cache, support_bounds, solved_range});
rewrite_enum!(SurfaceGeometry, []; {
    Procedural {construction, cache},
    Solved(field0),
});
rewrite_record!(SweepRevisionForm<R>, [R]; {revision, primary_flag, profile_endpoints, path_endpoints, cache});
rewrite_record!(SweepSurfaceConstruction<R, V, P>, [R, V, P]; {primary_kind, cache, layout, discontinuities, discontinuity_flag});
rewrite_enum!(SweepSurfaceLayout<R, V, P>, [R, V, P]; {
    ProfileFirst {secondary_kind, directions, origin, parameters, formulas},
    ExplicitFormula {mode, profile_range, profile_frame, origin, directions, trajectory_flag, path_range, path_parameter, formula_flag, formula, trailing_flag},
    ExplicitGuide {mode, profile_range, profile_frame, origin, directions, trajectory_flag, path_range, path_parameter, guide_flags, guide_curve, guide_range, guide_modes, guide_parameters, trailing_flags},
    ExplicitSurface {mode, profile_range, profile_frame, origin, directions, trajectory_flag, path_range, path_parameter, singularity, support_surface, auxiliary_curve, support_flag, legacy_flag},
    LawDriven {mode, profile_range, profile_frame, origin, directions, first_law, first_mode, first_range, law_direction, path_mode, path_flag, path_range, path_parameter, second_law_flag, second_law, formula_mode, formula, trailing_flag},
});
rewrite_enum!(TSplineSubtransform, []; {
    Inline(field0),
    Resolved {index, transform},
});
rewrite_record!(TSplineSurfaceConstruction, []; {parameter_ranges, type_code, subtransform, trailing_value, discontinuities, discontinuity_flag, cache});
rewrite_enum!(TaperSurfaceKind<R, V>, [R, V]; {
    Standard {},
    Orthogonal {sense},
    Edge {draft},
    Shadow {draft, sine, cosine},
    Ruled {draft, sine, cosine, factor},
    Swept {draft, sine, cosine},
});
rewrite_record!(TolerantIntersectionConstruction, []; {supports, endpoints, tolerance});
rewrite_record!(TolerantIntersectionParameterization, []; {pcurves, parameter_range});
rewrite_scalar!(VariableBlendBareCrossSection);
rewrite_enum!(VariableBlendCache<R>, [R]; {
    Current {shape_prefix, fit_tolerance},
    Stale {},
    Parameterization {shape_prefix, parameterization},
});
rewrite_record!(VariableBlendConstruction<R, V, P>, [R, V, P]; {subtype, revision, sides, slice, slice_range, offsets, radii, cross_section, u_range, v_lower, shape_parameter, shape_length, shape_tail, cache, discontinuities, tail_flag, tail_extensions, secondary_curve, convexity, render_mode, post_range, post_curve, post_pcurve});
rewrite_scalar!(VariableBlendConvexity);
rewrite_enum!(VariableBlendCrossSection<R, V, P>, [R, V, P]; {
    Circular {},
    Thumbweights {parameters},
    RoundedChamfer {radius},
    G2Round {parameters},
    UnclassifiedBare {selector},
});
rewrite_record!(VariableBlendInterpolationPoint<R, V, P>, [R, V, P]; {parameter, radius, tangents, location, normal});
rewrite_enum!(VariableBlendRadii<R, V, P>, [R, V, P]; {
    Single {value},
    Two {first, second},
});
rewrite_scalar!(VariableBlendRenderMode);
rewrite_scalar!(VariableBlendSupportKind);
rewrite_scalar!(VariableBlendSurfaceSubtype);
rewrite_enum!(VariableBlendTerminal<R>, [R]; {
    Double(field0),
    Text(field0),
});
rewrite_record!(VariableBlendValue<R, V, P>, [R, V, P]; {modern_flag, calibrated, payload});
rewrite_enum!(VariableBlendValuePayload<R, V, P>, [R, V, P]; {
    TwoEnds {discriminator, parameters, radii},
    FixedWidth {discriminator, parameters, width},
    EdgeOffset {discriminator, scalars, lengths},
    Functional {discriminator, parameter, radius, function, terminal},
    Constant {discriminator, parameters, radius, variable_chamfer, chamfer_type, nested},
    Interpolated {discriminator, parameter, radius, function, enum_count, enum_tagged, points},
});
rewrite_scalar!(VectorOffsetRoles);
rewrite_record!(VertexBlendBoundary<R, V, P>, [R, V, P]; {boundary_type, magic, u_smoothing, v_smoothing, fullness, geometry});
rewrite_enum!(VertexBlendBoundaryGeometry<R, V, P>, [R, V, P]; {
    Circle {curve, curve_endpoints, twists, parameters, sense},
    Degenerate {location, normals},
    Pcurve {surface, support_bounds, pcurve, sense, fit_tolerance},
    Plane {normal, parameters, curve, curve_endpoints},
});
rewrite_record!(VertexBlendConstruction<R, V, P>, [R, V, P]; {revision, boundaries, grid_size, fit_tolerance});
rewrite_enum!(VertexBlendTwists<P>, [P]; {
    None {},
    One {twist},
    Two {twists},
});

impl<
        const CAPACITY: usize,
        R: crate::schema::rewrite::typed::RewriteIdentities,
        V: crate::schema::rewrite::typed::RewriteIdentities,
    > crate::schema::rewrite::typed::RewriteIdentities
    for super::CompoundLoftScales<CAPACITY, R, V>
{
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.0.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        self.0.rewrite_identities(ctx, map).map(Self)
    }
}
