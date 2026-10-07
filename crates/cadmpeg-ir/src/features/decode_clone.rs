// SPDX-License-Identifier: Apache-2.0
//! Copy admitted feature fields under the caller decode resource policy.

use super::{
    AngularTermination, AxisAngle, BinderConstruction, BinderCopyOnChange, BinderLifecycle,
    BinderOffset, BinderPlacement, BinderSource, BinderTarget, BodyMember, BodyMembers,
    BodyRetentionMode, BodySelection, BodyTrimSide, BooleanKind, BooleanOp, CoilConstruction,
    CoilExtent, CoilPlacement, CoilResult, CoilSection, CoilSectionPlacement, CombineOperands,
    CosmeticThreadExtent, CurveProjectionDirection, DatumPlaneReference, DatumPointConstruction,
    DecalMapping, DistinctMembers, DraftAnchor, DraftPull, EdgeSelection, ExtrudeDirection,
    ExtrudeExtent, ExtrudeSide, ExtrudeStart, ExtrusionDirectionSource, FaceBlendOperands,
    FaceMaker, FaceMotion, FaceSelection, FeatureCircularArc, FeatureCoordinateFrame,
    FeatureDefinition, FeatureEllipticArc, FeatureEquationCurve, FeatureImageBounds,
    FeatureLineSegment, FeatureOperation, FeaturePolyline, FeatureTreeNodeRole,
    FeatureUnitPlaneFrame, FilledSurfaceContinuity, FilledSurfaceContinuityState, FlexMode,
    FuzzyTolerance, GeneratedBodyRef, GeneratedCurveRef, GeneratedEdgeRef, GeneratedFaceRef,
    GeneratedSweepSection, GeneratedVertexRef, GeometryImportFormat, GeometryImportPath,
    HelicalSweepConstruction, HelicalSweepLaw, HelicalSweepTravel, HelixConstructionStyle,
    HelixShape, InnerWireTaper, InsertedBodies, LinearTermination, LoftGuidance, LoftPointSection,
    LoftSection, NativeFeatureKind, NativeSelections, NoGeneratedSection, NonEmptyMembers,
    PartialRevolveConstruction, PathRef, PlanarProfileRef, PolygonSideCount, PrimitiveSolid,
    PrimitiveSolidKind, PrincipalPlane, ProfileRef, ReplaceFaceOperands, RevolutionAxis,
    RevolutionFuseOrder, RevolveConstruction, RevolveExtent, RibConstruction, RibDraft, RibSide,
    RuledCurveOrientation, RuledSurfaceCorner, RuledSurfaceMode, ScaleCenter, ScaleFactors,
    SectionOperands, SelectionMembers, SelectionReference, SewBodySelection,
    SheetMetalBendPosition, SheetMetalFlangeEdgeWidths, SheetMetalFlangeHeight,
    SheetMetalFlangeHeightTarget, SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth,
    SheetMetalHeightDatum, SheetMetalHemDirection, SheetMetalHemForm, SheetMetalThicknessSide,
    ShellJoin, ShellMode, SketchFeatureBinding, SketchPointSelection, SketchProfileBoundaryUse,
    SketchProfileLoops, SketchProfileRegion, SketchProfileRegions, SolidSweepOperation,
    SplitFacePlanes, SplitFaceTool, SurfaceBoundary, SurfaceContinuity, SurfaceExtension,
    SurfaceProjectionMode, SweepCircularRegion, SweepGuideRail, SweepOrientation, SweepPathExtent,
    SweepSection, SweepShape, SweepTransformation, SweepTransition, ThickenSide,
    ThreePointSelection, TreeChildren, TrimBodyOperands, TrimCellSelection, TrimRegion,
    UnresolvedFamily, VertexSelection, WrapMode,
};

clone_enum_for_decode!(AngularTermination; {
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

clone_record_for_decode!(AxisAngle; { origin, direction, angle });

clone_enum_for_decode!(BinderConstruction; {
    Shape { trace_support },
    SubShape { lifecycle, placement, copy_on_change, claim_children, fuse, make_face, partial_load, refine, offset, context },
});

clone_copy_for_decode!(BinderCopyOnChange);

clone_copy_for_decode!(BinderLifecycle);

clone_copy_for_decode!(BinderOffset);

clone_copy_for_decode!(BinderPlacement);

clone_record_for_decode!(BinderSource; { target, subelements });

clone_enum_for_decode!(BinderTarget; {
    Feature { feature },
    External { document, object },
    Native { reference },
});

clone_record_for_decode!(BodyMember<B>, [B]; { body, native });

clone_record_for_decode!(BodyMembers<B>, [B]; (field0));

clone_copy_for_decode!(BodyRetentionMode);

clone_enum_for_decode!(BodySelection; {
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

clone_copy_for_decode!(BodyTrimSide);

clone_copy_for_decode!(BooleanKind);

clone_copy_for_decode!(BooleanOp);

clone_record_for_decode!(CoilConstruction; { placement, diameter, extent, section, section_placement, clockwise, taper });

clone_copy_for_decode!(CoilExtent);

clone_enum_for_decode!(CoilPlacement; {
    Explicit { frame },
    Native { native_ref },
});

clone_enum_for_decode!(CoilResult; {
    NewBody {  },
    Boolean { operation, targets },
});

clone_copy_for_decode!(CoilSection);

clone_copy_for_decode!(CoilSectionPlacement);

clone_record_for_decode!(CombineOperands; { target, tools });

clone_copy_for_decode!(CosmeticThreadExtent);

clone_copy_for_decode!(CurveProjectionDirection);

clone_enum_for_decode!(DatumPlaneReference; {
    Feature { feature },
    Face { face },
    ResolvedPlane { frame },
});

clone_enum_for_decode!(DatumPointConstruction; {
    CircleCenter { edge },
    TwoEdgeIntersection { edges },
    ThreePlaneIntersection { planes },
    Vertex { vertex },
    SketchPoint { point },
    EdgePlaneIntersection { edge, plane },
    DistanceOnEdge { edge, fraction },
});

clone_copy_for_decode!(DecalMapping);

clone_record_for_decode!(DistinctMembers<T>, [T]; (field0));

clone_enum_for_decode!(DraftAnchor; {
    NeutralPlane { plane, pull },
    PartingLine { tool, pull },
});

clone_record_for_decode!(DraftPull; { direction, plane });

clone_enum_for_decode!(EdgeSelection; {
    Unresolved,
    All,
    Edges(field0),
    Resolved { edges, native },
    Historical { state, edges, native },
    HistoricalPartial { state, edges, unresolved, native },
    Generated { edges, native },
    Native(field0),
});

clone_enum_for_decode!(ExtrudeDirection; {
    Unresolved {  },
    ProfileNormal {  },
    ReversedProfileNormal {  },
    Explicit { vector, source },
});

clone_enum_for_decode!(ExtrudeExtent; {
    OneSided { side },
    TwoSided { first, second },
    Symmetric { side },
});

clone_record_for_decode!(ExtrudeSide; { termination, draft });

clone_enum_for_decode!(ExtrudeStart; {
    Unresolved {  },
    ProfilePlane {  },
    OffsetProfilePlane { offset },
    FromFace { face, offset },
});

clone_enum_for_decode!(ExtrusionDirectionSource; {
    Custom {  },
    Edge { reference },
    ProfileNormal {  },
});

clone_record_for_decode!(FaceBlendOperands; { first_faces, second_faces });

clone_enum_for_decode!(FaceMaker; {
    Simple,
    Cheese,
    Extrusion,
    Bullseye,
    Unified,
    Other(field0),
});

clone_enum_for_decode!(FaceMotion; {
    Offset { distance },
    Translate { direction, distance },
    Rotate { axis_origin, axis_dir, angle },
});

clone_enum_for_decode!(FaceSelection; {
    Unresolved,
    Faces(field0),
    Resolved { faces, native },
    Historical { state, faces, native },
    HistoricalPartial { state, faces, unresolved, native },
    Generated { faces, native },
    Native(field0),
});

clone_copy_for_decode!(FeatureCircularArc);

clone_copy_for_decode!(FeatureCoordinateFrame);

clone_enum_for_decode!(FeatureDefinition; {
    PostProcess { operation, refine, fuzzy_tolerance },
    Operation(field0),
});

clone_copy_for_decode!(FeatureEllipticArc);

clone_record_for_decode!(FeatureEquationCurve; { parameter, x_expression, y_expression, z_expression, domain });

clone_copy_for_decode!(FeatureImageBounds);

clone_copy_for_decode!(FeatureLineSegment);

clone_enum_for_decode!(FeatureOperation; {
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

clone_record_for_decode!(FeaturePolyline; { points, closed });

clone_copy_for_decode!(FeatureTreeNodeRole);

clone_copy_for_decode!(FeatureUnitPlaneFrame);

clone_record_for_decode!(FilledSurfaceContinuity; { conditions });

clone_record_for_decode!(FilledSurfaceContinuityState; (field0));

clone_copy_for_decode!(FlexMode);

clone_copy_for_decode!(FuzzyTolerance);

clone_record_for_decode!(GeneratedBodyRef; { feature, local_id });

clone_record_for_decode!(GeneratedCurveRef; { feature, local_id });

clone_record_for_decode!(GeneratedEdgeRef; { feature, local_id });

clone_record_for_decode!(GeneratedFaceRef; { feature, local_id });

clone_enum_for_decode!(GeneratedSweepSection; {
    CircularRegion { region },
});

clone_record_for_decode!(GeneratedVertexRef; { feature, local_id });

clone_copy_for_decode!(GeometryImportFormat);

clone_record_for_decode!(GeometryImportPath; (field0));

clone_record_for_decode!(HelicalSweepConstruction; { profile, axis_origin, axis_direction, law, pitch, travel, turns, cone_angle, left_handed, reversed, tolerance, allow_multi_profile_faces });

clone_copy_for_decode!(HelicalSweepLaw);

clone_copy_for_decode!(HelicalSweepTravel);

clone_copy_for_decode!(HelixConstructionStyle);

clone_copy_for_decode!(HelixShape);

clone_copy_for_decode!(InnerWireTaper);

clone_enum_for_decode!(InsertedBodies; {
    Native(field0),
    Resolved { native },
});

clone_enum_for_decode!(LinearTermination; {
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

clone_enum_for_decode!(LoftGuidance; {
    Guides(field0),
    Centerline(field0),
});

clone_enum_for_decode!(LoftPointSection; {
    Native(field0),
    Point(field0),
    Vertex(field0),
});

clone_enum_for_decode!(LoftSection; {
    Profile(field0),
    Point(field0),
});

clone_enum_for_decode!(NativeFeatureKind; {
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

clone_record_for_decode!(NativeSelections; (field0));

clone_copy_for_decode!(NoGeneratedSection);

clone_record_for_decode!(NonEmptyMembers<T>, [T]; (field0));

clone_enum_for_decode!(PartialRevolveConstruction; {
    Profile { axis, extent, solid, face_maker, fuse_order, allow_multi_profile_faces },
    Axis { profile, extent, solid, face_maker, fuse_order, allow_multi_profile_faces },
    Extent { profile, axis, solid, face_maker, fuse_order, allow_multi_profile_faces },
});

clone_enum_for_decode!(PathRef; {
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

clone_enum_for_decode!(PlanarProfileRef; {
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

clone_copy_for_decode!(PolygonSideCount);

clone_record_for_decode!(PrimitiveSolid; (field0));

clone_enum_for_decode!(PrimitiveSolidKind; {
    Box { length, width, height },
    Cylinder { radius, height, angle },
    Cone { radius1, radius2, height, angle },
    Sphere { radius, latitude1, latitude2, longitude },
    Ellipsoid { x_radius, y_radius, z_radius, latitude1, latitude2, longitude },
    Torus { major_radius, minor_radius, latitude1, latitude2, longitude },
    Prism { sides, circumradius, height },
    Wedge { xmin, ymin, zmin, x2min, z2min, xmax, ymax, zmax, x2max, z2max },
});

clone_copy_for_decode!(PrincipalPlane);

clone_enum_for_decode!(ProfileRef; {
    SpatialSketchProfiles { sketch, profiles },
    SpatialSketchSelection { sketch, selections },
    Planar(field0),
});

clone_record_for_decode!(ReplaceFaceOperands; { targets, replacements });

clone_record_for_decode!(RevolutionAxis; { origin, direction, reference });

clone_copy_for_decode!(RevolutionFuseOrder);

clone_enum_for_decode!(RevolveConstruction; {
    Unresolved(field0),
    Resolved { profile, axis, extent, solid, face_maker, fuse_order, allow_multi_profile_faces },
});

clone_enum_for_decode!(RevolveExtent; {
    OneSided { termination },
    TwoSided { first, second },
    Symmetric { termination },
});

clone_record_for_decode!(RibConstruction; { profile, direction, thickness, side, draft });

clone_copy_for_decode!(RibDraft);

clone_copy_for_decode!(RibSide);

clone_copy_for_decode!(RuledCurveOrientation);

clone_copy_for_decode!(RuledSurfaceCorner);

clone_enum_for_decode!(RuledSurfaceMode; {
    Normal { distance },
    Tangent { distance },
    Direction { direction, distance },
});

clone_enum_for_decode!(ScaleCenter; {
    Centroid,
    ModelOrigin,
    Point(field0),
    Native(field0),
});

clone_copy_for_decode!(ScaleFactors);

clone_record_for_decode!(SectionOperands; { first, second });

clone_record_for_decode!(SelectionMembers<T>, [T]; (field0));

clone_record_for_decode!(SelectionReference; (field0));

clone_record_for_decode!(SewBodySelection; (field0));

clone_copy_for_decode!(SheetMetalBendPosition);

clone_record_for_decode!(SheetMetalFlangeEdgeWidths; (field0));

clone_enum_for_decode!(SheetMetalFlangeHeight; {
    Distance(field0),
    ToObject { target, offset },
});

clone_enum_for_decode!(SheetMetalFlangeHeightTarget; {
    Feature(field0),
    Native(field0),
});

clone_record_for_decode!(SheetMetalFlangeTwoSidedWidth; { first, second });

clone_enum_for_decode!(SheetMetalFlangeWidth; {
    FullEdge,
    Symmetric { width },
    TwoSides { first, second },
    TwoSidesPerEdge { widths },
});

clone_copy_for_decode!(SheetMetalHeightDatum);

clone_copy_for_decode!(SheetMetalHemDirection);

clone_enum_for_decode!(SheetMetalHemForm; {
    Flat { length },
    Open { gap, length },
    GapLength { gap, length },
    Rolled { radius, angle },
    Teardrop { gap, length, radius },
});

clone_copy_for_decode!(SheetMetalThicknessSide);

clone_copy_for_decode!(ShellJoin);

clone_copy_for_decode!(ShellMode);

clone_enum_for_decode!(SketchFeatureBinding; {
    Unresolved,
    Planar(field0),
});

clone_enum_for_decode!(SketchPointSelection; {
    Unresolved,
    Planar { sketch, point, native },
    Spatial { sketch, point, native },
    Native(field0),
});

clone_record_for_decode!(SketchProfileBoundaryUse; { entity, parameter_range, reversed });

clone_record_for_decode!(SketchProfileLoops; { outer, holes });

clone_enum_for_decode!(SketchProfileRegion; {
    Loops { loops },
    Trimmed { outer_boundary, hole_boundaries },
});

clone_record_for_decode!(SketchProfileRegions; (field0));

clone_copy_for_decode!(SolidSweepOperation);

clone_record_for_decode!(SplitFacePlanes; (field0));

clone_enum_for_decode!(SplitFaceTool; {
    Path(field0),
    Plane { plane },
    Planes { planes },
});

clone_enum_for_decode!(SurfaceBoundary; {
    Edges(field0),
    Path(field0),
});

clone_copy_for_decode!(SurfaceContinuity);

clone_copy_for_decode!(SurfaceExtension);

clone_copy_for_decode!(SurfaceProjectionMode);

clone_copy_for_decode!(SweepCircularRegion);

clone_record_for_decode!(SweepGuideRail; { path, extent });

clone_enum_for_decode!(SweepOrientation; {
    CorrectedFrenet {  },
    Fixed {  },
    Frenet {  },
    Auxiliary { path, tangent, curvilinear },
    GuideSurface { faces },
    Binormal { direction },
});

clone_copy_for_decode!(SweepPathExtent);

clone_enum_for_decode!(SweepSection<G>, [G]; {
    Unresolved(field0),
    Profile(field0),
    Generated(field0),
});

clone_enum_for_decode!(SweepShape; {
    Unresolved { section, sections },
    Solid { op, section, sections },
    Surface { section, sections },
});

clone_copy_for_decode!(SweepTransformation);

clone_copy_for_decode!(SweepTransition);

clone_copy_for_decode!(ThickenSide);

clone_record_for_decode!(ThreePointSelection; (field0));

clone_record_for_decode!(TreeChildren; { children, active_child });

clone_record_for_decode!(TrimBodyOperands; { targets, tools });

clone_record_for_decode!(TrimCellSelection; { removed, total });

clone_enum_for_decode!(TrimRegion; {
    Unresolved,
    Inside,
    Outside,
    Cells(field0),
});

clone_copy_for_decode!(UnresolvedFamily);

clone_enum_for_decode!(VertexSelection; {
    Unresolved,
    Generated { vertex, native },
    Historical { state, vertex, native },
    Native(field0),
});

clone_copy_for_decode!(WrapMode);
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(super) trait CloneForDecode: Sized {
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError>;
}

impl CloneForDecode for String {
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.copy_retained_text(self, operation)
    }
}

impl<T: CloneForDecode> CloneForDecode for Vec<T> {
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut copied = ctx.collection_vec(self.len(), operation)?;
        for member in ctx.admit_iter(self, operation)? {
            copied.push(member.try_clone_for_decode(ctx, operation)?);
        }
        Ok(copied)
    }
}

impl<T: CloneForDecode> CloneForDecode for Option<T> {
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        self.as_ref()
            .map(|value| value.try_clone_for_decode(ctx, operation))
            .transpose()
    }
}

impl<T: CloneForDecode> CloneForDecode for Box<T> {
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        copy_box(self.as_ref(), ctx, operation)
    }
}

impl<T: CloneForDecode> CloneForDecode for [T; 2] {
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok([
            self[0].try_clone_for_decode(ctx, operation)?,
            self[1].try_clone_for_decode(ctx, operation)?,
        ])
    }
}

impl<T: CloneForDecode> CloneForDecode for [T; 3] {
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok([
            self[0].try_clone_for_decode(ctx, operation)?,
            self[1].try_clone_for_decode(ctx, operation)?,
            self[2].try_clone_for_decode(ctx, operation)?,
        ])
    }
}

impl<K: CloneForDecode + Ord + cadmpeg_core::decode::cost::DecodeCost, V: CloneForDecode>
    CloneForDecode for BTreeMap<K, V>
{
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut copied = BTreeMap::new();
        for (key, value) in ctx.admit_iter(self, operation)? {
            let key = key.try_clone_for_decode(ctx, operation)?;
            let value = value.try_clone_for_decode(ctx, operation)?;
            ctx.insert_btree_map(&mut copied, key, value, operation)?;
        }
        Ok(copied)
    }
}

impl CloneForDecode for cadmpeg_core::text::NonBlankString {
    fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        cadmpeg_core::text::NonBlankString::try_clone_for_decode(self, ctx, operation)
    }
}

macro_rules! clone_id_for_decode {
    ($type:ty) => {
        impl CloneForDecode for $type {
            fn try_clone_for_decode(
                &self,
                ctx: &DecodeContext<'_>,
                operation: &'static str,
            ) -> Result<Self, CodecError> {
                <$type>::try_clone_for_decode(self, ctx, operation)
            }
        }
    };
}

clone_copy_for_decode!(bool);
clone_copy_for_decode!(u32);
clone_copy_for_decode!(u64);
clone_copy_for_decode!(f64);
clone_copy_for_decode!(std::num::NonZeroU32);
clone_copy_for_decode!(crate::scalar::Angle);
clone_copy_for_decode!(crate::scalar::FiniteReal);
clone_copy_for_decode!(crate::scalar::Fraction);
clone_copy_for_decode!(crate::scalar::InteriorAngle);
clone_copy_for_decode!(crate::scalar::Length);
clone_copy_for_decode!(crate::scalar::NonNegativeLength);
clone_copy_for_decode!(crate::scalar::NonZeroLength);
clone_copy_for_decode!(crate::scalar::PositiveAngle);
clone_copy_for_decode!(crate::scalar::PositiveLength);
clone_copy_for_decode!(crate::scalar::PositiveReal);
clone_copy_for_decode!(crate::scalar::SlopeAngle);
clone_copy_for_decode!(crate::features::FinitePoint3);
clone_copy_for_decode!(crate::features::FiniteVector3);
clone_copy_for_decode!(crate::features::FeatureDirection3);
clone_copy_for_decode!(crate::features::FeatureDatumPlaneFrame);
clone_copy_for_decode!(crate::features::FeatureRigidPlacement);
clone_copy_for_decode!(crate::features::FeatureSupportPlaneFrame);
clone_copy_for_decode!(crate::transform::Transform);
clone_copy_for_decode!(crate::units::UnitVector3);
clone_copy_for_decode!(crate::geometry::DirectedParameterRange);
clone_copy_for_decode!(crate::topology::IncreasingParameterInterval);

clone_id_for_decode!(crate::assets::AssetId);
clone_id_for_decode!(crate::features::FeatureId);
clone_id_for_decode!(crate::ids::BodyId);
clone_id_for_decode!(crate::ids::CurveId);
clone_id_for_decode!(crate::ids::EdgeId);
clone_id_for_decode!(crate::ids::FaceId);
clone_id_for_decode!(crate::ids::FeatureInputTopologyId);
clone_id_for_decode!(crate::ids::HistoricalBodyId);
clone_id_for_decode!(crate::ids::HistoricalEdgeId);
clone_id_for_decode!(crate::ids::HistoricalFaceId);
clone_id_for_decode!(crate::ids::HistoricalVertexId);
clone_id_for_decode!(crate::ids::OccurrenceId);
clone_id_for_decode!(crate::ids::SubdId);
clone_id_for_decode!(crate::ids::VertexId);
clone_id_for_decode!(crate::products::JointId);
clone_id_for_decode!(crate::sketches::SketchEntityId);
clone_id_for_decode!(crate::sketches::SketchId);
clone_id_for_decode!(crate::sketches::SpatialSketchEntityId);
clone_id_for_decode!(crate::sketches::SpatialSketchId);

#[cfg(test)]
mod tests;

clone_enum_for_decode!(super::ConfigurationEvaluation; { Suppressed {}, Active { outputs } });
clone_record_for_decode!(super::ConfigurationFeatureState; { evaluation, dependencies, definition });
clone_record_for_decode!(super::FeatureEvaluation; { definition, outputs });
clone_record_for_decode!(super::Feature; { id, ordinal, name, suppressed, dependencies, source_properties, source_tag, source_text, source_content, evaluation, native_ref });
clone_record_for_decode!(super::FeatureContent; (field0));
clone_enum_for_decode!(super::FeatureSourceContent; { Text(value), Parameter(value), Feature(value) });
clone_enum_for_decode!(super::ParameterValue; { Length(value), Angle(value), Real(value), Integer(value), Boolean(value), String(value) });
clone_id_for_decode!(super::ParameterId);
clone_copy_for_decode!(i64);

/// Copy a boxed field after admitting its retained allocation.
pub(super) fn copy_box<T: CloneForDecode>(
    value: &T,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Box<T>, CodecError> {
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<T>()),
        operation,
    )?;
    ctx.charge_collection_items(1, operation)?;
    Ok(Box::new(CloneForDecode::try_clone_for_decode(
        value, ctx, operation,
    )?))
}
