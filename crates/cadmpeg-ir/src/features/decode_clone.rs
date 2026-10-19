// SPDX-License-Identifier: Apache-2.0
//! Identity rewrite implementations for feature records.

use super::{
    AngularTermination, AxisAngle, BinderConstruction, BinderSource, BinderTarget, BodyMember,
    BodyMembers, BodySelection, CoilConstruction, CoilPlacement, CoilResult, CombineOperands,
    DatumPlaneReference, DatumPointConstruction, DistinctMembers, DraftAnchor, DraftPull,
    EdgeSelection, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart,
    ExtrusionDirectionSource, FaceBlendOperands, FaceMaker, FaceMotion, FaceSelection,
    FeatureDefinition, FeatureEquationCurve, FeatureOperation, FeaturePolyline,
    FilledSurfaceContinuity, FilledSurfaceContinuityState, GeneratedBodyRef, GeneratedCurveRef,
    GeneratedEdgeRef, GeneratedFaceRef, GeneratedSweepSection, GeneratedVertexRef,
    GeometryImportPath, HelicalSweepConstruction, InsertedBodies, LinearTermination, LoftGuidance,
    LoftPointSection, LoftSection, NativeFeatureKind, NativeSelections, NonEmptyMembers,
    PartialRevolveConstruction, PathRef, PlanarProfileRef, PrimitiveSolid, PrimitiveSolidKind,
    ProfileRef, ReplaceFaceOperands, RevolutionAxis, RevolveConstruction, RevolveExtent,
    RibConstruction, RuledSurfaceMode, ScaleCenter, SectionOperands, SelectionMembers,
    SelectionReference, SewBodySelection, SheetMetalFlangeEdgeWidths, SheetMetalFlangeHeight,
    SheetMetalFlangeHeightTarget, SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth,
    SheetMetalHemForm, SketchFeatureBinding, SketchPointSelection, SketchProfileBoundaryUse,
    SketchProfileLoops, SketchProfileRegion, SketchProfileRegions, SplitFacePlanes, SplitFaceTool,
    SurfaceBoundary, SweepGuideRail, SweepOrientation, SweepSection, SweepShape,
    ThreePointSelection, TreeChildren, TrimBodyOperands, TrimCellSelection, TrimRegion,
    VertexSelection,
};

rewrite_enum!(AngularTermination, []; {
    Unresolved {  },
    ThroughAll {  },
    ThroughNext {  },
    ToFirst {  },
    ToLast {  },
    ToFace { face, offset },
    ToVertex { vertex },
    OffsetFromFace { face, offset },
    ToShape { target },
    Angle { angle },
});

rewrite_record!(AxisAngle, []; { origin, direction, angle });

rewrite_enum!(BinderConstruction, []; {
    Shape { trace_support },
    SubShape { lifecycle, placement, copy_on_change, claim_children, fuse, make_face, partial_load, refine, offset, context },
});

rewrite_record!(BinderSource, []; { target, subelements });

rewrite_enum!(BinderTarget, []; {
    Feature { feature },
    External { document, object },
    Native { reference },
});

rewrite_record!(BodyMember<B>, [B]; { body, native });

rewrite_record!(BodyMembers<B>, [B]; (field0));

rewrite_enum!(BodySelection, []; {
    Unresolved,
    Bodies(field0),
    Resolved { bodies, native },
    ResolvedSet { members },
    Historical { state, bodies, native },
    HistoricalSet { state, members },
    Generated { bodies, native },
    Local { bodies, native },
    Native(field0),
    NativeSet(field0),
});

rewrite_record!(CoilConstruction, []; { placement, diameter, extent, section, section_placement, clockwise, taper });

rewrite_enum!(CoilPlacement, []; {
    Explicit { frame },
    Native { native_ref },
});

rewrite_enum!(CoilResult, []; {
    NewBody {  },
    Boolean { operation, targets },
});

rewrite_record!(CombineOperands, []; { target, tools });

rewrite_enum!(DatumPlaneReference, []; {
    Feature { feature },
    Face { face },
    ResolvedPlane { frame },
});

rewrite_enum!(DatumPointConstruction, []; {
    CircleCenter { edge },
    TwoEdgeIntersection { edges },
    ThreePlaneIntersection { planes },
    Vertex { vertex },
    SketchPoint { point },
    EdgePlaneIntersection { edge, plane },
    DistanceOnEdge { edge, fraction },
});

rewrite_record!(DistinctMembers<T>, [T]; (field0));

rewrite_enum!(DraftAnchor, []; {
    NeutralPlane { plane, pull },
    PartingLine { tool, pull },
});

rewrite_record!(DraftPull, []; { direction, plane });

rewrite_enum!(EdgeSelection, []; {
    Unresolved,
    All,
    Edges(field0),
    Resolved { edges, native },
    Historical { state, edges, native },
    HistoricalPartial { state, edges, unresolved, native },
    Generated { edges, native },
    Native(field0),
});

rewrite_enum!(ExtrudeDirection, []; {
    Unresolved {  },
    ProfileNormal {  },
    ReversedProfileNormal {  },
    Explicit { vector, source },
});

rewrite_enum!(ExtrudeExtent, []; {
    OneSided { side },
    TwoSided { first, second },
    Symmetric { side },
});

rewrite_record!(ExtrudeSide, []; { termination, draft });

rewrite_enum!(ExtrudeStart, []; {
    Unresolved {  },
    ProfilePlane {  },
    OffsetProfilePlane { offset },
    FromFace { face, offset },
});

rewrite_enum!(ExtrusionDirectionSource, []; {
    Custom {  },
    Edge { reference },
    ProfileNormal {  },
});

rewrite_record!(FaceBlendOperands, []; { first_faces, second_faces });

rewrite_enum!(FaceMaker, []; {
    Simple,
    Cheese,
    Extrusion,
    Bullseye,
    Unified,
    Other(field0),
});

rewrite_enum!(FaceMotion, []; {
    Offset { distance },
    Translate { direction, distance },
    Rotate { axis_origin, axis_dir, angle },
});

rewrite_enum!(FaceSelection, []; {
    Unresolved,
    Faces(field0),
    Resolved { faces, native },
    Historical { state, faces, native },
    HistoricalPartial { state, faces, unresolved, native },
    Generated { faces, native },
    Native(field0),
});

rewrite_enum!(FeatureDefinition, []; {
    PostProcess { operation, refine, fuzzy_tolerance },
    Operation(field0),
});

rewrite_record!(FeatureEquationCurve, []; { parameter, x_expression, y_expression, z_expression, domain });

rewrite_enum!(FeatureOperation, []; {
    TreeNode { role, children },
    BaseFeature { bodies },
    MeshImport { tessellations },
    InsertBodies { bodies },
    InsertComponent { occurrence },
    AssemblyJoint { joint },
    Form { cages },
    CosmeticThread { face, diameter, extent },
    ReferenceImage { asset, visible, mirror_u, mirror_v, frame, bounds, opacity },
    Decal { asset, faces, mapping, opacity },
    DatumPrincipalPlane { plane },
    DatumPlane { frame },
    DatumThreePointPlane { frame, points },
    DatumOffsetPlane { reference, distance },
    DatumAxis { origin, direction },
    DatumPoint { position, construction },
    PointGeometry { position },
    LineSegment { segment },
    CircularArc { arc },
    EllipticArc { arc },
    Polyline { chain },
    RegularPolygonCurve { sides, circumradius },
    PlanarPatch { length, width },
    FaceFromShapes { sources, face_maker },
    DatumCoordinateSystem { frame },
    Block { dimensions, placement, op },
    EquationCurve { curve },
    ProjectedCurve { source, target_faces, direction, bidirectional },
    ProjectOnSurface { sources, support_face, direction, mode, height, offset },
    CompositeCurve { segments, closed },
    Helix { axis_origin, axis_direction, radius, shape, revolutions, start_angle, clockwise, segment_turns, construction_style },
    HelixNativeAxis { axis_native_ref, axial_rise, pitch, revolutions, start_angle, clockwise },
    Coil { construction, result },
    Sphere { center, radius, op },
    Torus { center, axis, major_radius, minor_radius, op },
    Wrap { profile, face, mode },
    Sketch { sketch },
    SpatialSketch { sketch },
    SketchBlockDefinition { sketch },
    SketchBlockInstance { block, placement },
    StoredGeometry {  },
    ExtractBody { source },
    DerivedGeometry { source },
    ImportedGeometry { path, format },
    Primitive { solid, op },
    Revolve { construction, op },
    Sweep { shape, path, orientation, transition, transformation, path_tangent, linearize, twist, path_extent, guide_rail, taper, scale, allow_multi_profile_faces },
    HelicalSweep { construction, op },
    Binder { sources, construction },
    Rib { construction, op },
    SheetMetalBaseFlange { profile, thickness, side },
    SheetMetalEdgeFlange { edges, height, angle, height_datum, bend_position, width, bend_radius },
    SheetMetalHem { edges, form, direction, bend_radius },
    Fillet { groups },
    FullRoundFillet { groups },
    FaceBlend { operands, radius },
    Chamfer { groups, flip_direction },
    Shell { bodies, removed_faces, thickness, outward, mode, join, resolve_intersections, allow_self_intersections },
    OffsetShape { source, distance, mode, join, resolve_intersections, allow_self_intersections, fill, planar },
    Compound { members },
    RefineShape { source },
    ReverseShape { source },
    RuledBetweenCurves { first, second, orientation },
    SectionShape { operands, approximate },
    MirrorShape { source, plane_origin, plane_normal, plane_reference },
    Thicken { faces, thickness, side },
    OffsetSurface { faces, distance },
    KnitSurface { faces, merge_entities, create_solid, gap_tolerance },
    SewBodies { bodies, gap_tolerance },
    FilledSurface { boundary, support_faces, continuity, merge_result },
    TrimSurface { faces, tool, keep },
    ExtendSurface { faces, distance, method },
    RuledSurface { edges, support_faces, mode, angle, alternate_face, corner },
    Draft { faces, anchor, angle, outward },
    Combine { operands, op, keep_tools },
    BoundaryFill { tools, cells },
    CutWithSurface { targets, tools, reverse },
    TrimBodies { operands, keep },
    SplitBody { targets, tools },
    SplitFace { targets, tool },
    DeleteBody { bodies, mode },
    DeleteFace { faces, heal },
    ReplaceFace { operands },
    MoveFace { faces, motion },
    MoveBody { bodies, translation, rotation, copies },
    Dome { faces, height, elliptical, reverse },
    Flex { axis, mode },
    Scale { bodies, center, factors },
    Hole { profile, profile_filter, face, direction, placements, shape, extent, bottom, taper_angle, allow_multi_profile_faces },
    Pattern { seeds, pattern },
    Unresolved { family },
    Native { kind, parameters },
    Extrude { profile, direction, start, extent, op, solid, face_maker, inner_wire_taper, length_along_profile_normal, allow_multi_profile_faces },
    Loft { sections, guidance, op, closed, solid, ruled, linearize, max_degree, allow_multi_profile_faces },
});

rewrite_record!(FeaturePolyline, []; { points, closed });

rewrite_record!(FilledSurfaceContinuity, []; { conditions });

rewrite_record!(FilledSurfaceContinuityState, []; (field0));

rewrite_record!(GeneratedBodyRef, []; { feature, local_id });

rewrite_record!(GeneratedCurveRef, []; { feature, local_id });

rewrite_record!(GeneratedEdgeRef, []; { feature, local_id });

rewrite_record!(GeneratedFaceRef, []; { feature, local_id });

rewrite_enum!(GeneratedSweepSection, []; {
    CircularRegion { region },
});

rewrite_record!(GeneratedVertexRef, []; { feature, local_id });

rewrite_record!(GeometryImportPath, []; (field0));

rewrite_record!(HelicalSweepConstruction, []; { profile, axis_origin, axis_direction, law, pitch, travel, turns, cone_angle, left_handed, reversed, tolerance, allow_multi_profile_faces });

rewrite_enum!(InsertedBodies, []; {
    Native(field0),
    Resolved { native },
});

rewrite_enum!(LinearTermination, []; {
    Unresolved {  },
    Blind { length },
    ThroughAll {  },
    ThroughNext {  },
    ToFirst {  },
    ToLast {  },
    ToFace { face, offset },
    ToVertex { vertex },
    OffsetFromFace { face, offset },
    ToShape { target },
});

rewrite_enum!(LoftGuidance, []; {
    Guides(field0),
    Centerline(field0),
});

rewrite_enum!(LoftPointSection, []; {
    Native(field0),
    Point(field0),
    Vertex(field0),
});

rewrite_enum!(LoftSection, []; {
    Profile(field0),
    Point(field0),
});

rewrite_enum!(NativeFeatureKind, []; {
    Canvas,
    Decal,
    Draft,
    Fillet,
    Chamfer,
    Extrude,
    DeleteFace,
    SurfaceDeleteFace,
    Other(field0),
});

rewrite_record!(NativeSelections, []; (field0));

rewrite_record!(NonEmptyMembers<T>, [T]; (field0));

rewrite_enum!(PartialRevolveConstruction, []; {
    Profile { axis, extent, solid, face_maker, fuse_order, allow_multi_profile_faces },
    Axis { profile, extent, solid, face_maker, fuse_order, allow_multi_profile_faces },
    Extent { profile, axis, solid, face_maker, fuse_order, allow_multi_profile_faces },
});

rewrite_enum!(PathRef, []; {
    Unresolved(field0),
    Native(field0),
    Sketch(field0),
    SketchCurves { sketch, curves },
    SpatialSketchSelection { sketch, selections },
    SpatialSketchCurves { sketch, curves },
    Edges(field0),
    Curves(field0),
    HistoricalEdges { state, edges, native },
});

rewrite_enum!(PlanarProfileRef, []; {
    Unresolved(field0),
    Native(field0),
    Sketch(field0),
    SketchProfiles { sketch, profiles },
    SketchRegions { sketch, regions },
    SketchEntities { sketch, entities },
    SketchSelection { sketch, selections },
    HistoricalFaces { state, faces, native },
    Feature(field0),
    Generated { curves, native },
    Faces(field0),
});

rewrite_record!(PrimitiveSolid, []; (field0));

rewrite_enum!(PrimitiveSolidKind, []; {
    Box { length, width, height },
    Cylinder { radius, height, angle },
    Cone { radius1, radius2, height, angle },
    Sphere { radius, latitude1, latitude2, longitude },
    Ellipsoid { x_radius, y_radius, z_radius, latitude1, latitude2, longitude },
    Torus { major_radius, minor_radius, latitude1, latitude2, longitude },
    Prism { sides, circumradius, height },
    Wedge { xmin, ymin, zmin, x2min, z2min, xmax, ymax, zmax, x2max, z2max },
});

rewrite_enum!(ProfileRef, []; {
    SpatialSketchProfiles { sketch, profiles },
    SpatialSketchSelection { sketch, selections },
    Planar(field0),
});

rewrite_record!(ReplaceFaceOperands, []; { targets, replacements });

rewrite_record!(RevolutionAxis, []; { origin, direction, reference });

rewrite_enum!(RevolveConstruction, []; {
    Unresolved(field0),
    Resolved { profile, axis, extent, solid, face_maker, fuse_order, allow_multi_profile_faces },
});

rewrite_enum!(RevolveExtent, []; {
    OneSided { termination },
    TwoSided { first, second },
    Symmetric { termination },
});

rewrite_record!(RibConstruction, []; { profile, direction, thickness, side, draft });

rewrite_enum!(RuledSurfaceMode, []; {
    Normal { distance },
    Tangent { distance },
    Direction { direction, distance },
});

rewrite_enum!(ScaleCenter, []; {
    Centroid,
    ModelOrigin,
    Point(field0),
    Native(field0),
});

rewrite_record!(SectionOperands, []; { first, second });

rewrite_record!(SelectionMembers<T>, [T]; (field0));

rewrite_record!(SelectionReference, []; (field0));

rewrite_record!(SewBodySelection, []; (field0));

rewrite_record!(SheetMetalFlangeEdgeWidths, []; (field0));

rewrite_enum!(SheetMetalFlangeHeight, []; {
    Distance(field0),
    ToObject { target, offset },
});

rewrite_enum!(SheetMetalFlangeHeightTarget, []; {
    Feature(field0),
    Native(field0),
});

rewrite_record!(SheetMetalFlangeTwoSidedWidth, []; { first, second });

rewrite_enum!(SheetMetalFlangeWidth, []; {
    FullEdge,
    Symmetric { width },
    TwoSides { first, second },
    TwoSidesPerEdge { widths },
});

rewrite_enum!(SheetMetalHemForm, []; {
    Flat { length },
    Open { gap, length },
    GapLength { gap, length },
    Rolled { radius, angle },
    Teardrop { gap, length, radius },
});

rewrite_enum!(SketchFeatureBinding, []; {
    Unresolved,
    Planar(field0),
});

rewrite_enum!(SketchPointSelection, []; {
    Unresolved,
    Planar { sketch, point, native },
    Spatial { sketch, point, native },
    Native(field0),
});

rewrite_record!(SketchProfileBoundaryUse, []; { entity, parameter_range, reversed });

rewrite_record!(SketchProfileLoops, []; { outer, holes });

rewrite_enum!(SketchProfileRegion, []; {
    Loops { loops },
    Trimmed { outer_boundary, hole_boundaries },
});

rewrite_record!(SketchProfileRegions, []; (field0));

rewrite_record!(SplitFacePlanes, []; (field0));

rewrite_enum!(SplitFaceTool, []; {
    Path(field0),
    Plane { plane },
    Planes { planes },
});

rewrite_enum!(SurfaceBoundary, []; {
    Edges(field0),
    Path(field0),
});

rewrite_record!(SweepGuideRail, []; { path, extent });

rewrite_enum!(SweepOrientation, []; {
    CorrectedFrenet {  },
    Fixed {  },
    Frenet {  },
    Auxiliary { path, tangent, curvilinear },
    GuideSurface { faces },
    Binormal { direction },
});

rewrite_enum!(SweepSection<G>, [G]; {
    Unresolved(field0),
    Profile(field0),
    Generated(field0),
});

rewrite_enum!(SweepShape, []; {
    Unresolved { section, sections },
    Solid { op, section, sections },
    Surface { section, sections },
});

rewrite_record!(ThreePointSelection, []; (field0));

rewrite_record!(TreeChildren, []; { children, active_child });

rewrite_record!(TrimBodyOperands, []; { targets, tools });

rewrite_record!(TrimCellSelection, []; { removed, total });

rewrite_enum!(TrimRegion, []; {
    Unresolved,
    Inside,
    Outside,
    Cells(field0),
});

rewrite_enum!(VertexSelection, []; {
    Unresolved,
    Generated { vertex, native },
    Historical { state, vertex, native },
    Native(field0),
});

rewrite_enum!(super::ConfigurationEvaluation, []; { Suppressed {}, Active { outputs } });
rewrite_record!(super::ConfigurationFeatureState, []; { evaluation, dependencies, definition });
rewrite_record!(super::FeatureEvaluation, []; { definition, outputs });
rewrite_record!(super::Feature, []; { id, ordinal, name, suppressed, dependencies, source_properties, source_tag, source_text, source_content, evaluation, native_ref });
rewrite_record!(super::FeatureContent, []; (field0));
rewrite_enum!(super::FeatureSourceContent, []; { Text(value), Parameter(value), Feature(value) });
rewrite_enum!(super::ParameterValue, []; { Length(value), Angle(value), Real(value), Integer(value), Boolean(value), String(value) });
