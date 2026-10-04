// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    BinderCopyOnChange, BinderLifecycle, BinderOffset, BinderOffsetJoin, BinderPlacement,
    BodyRetentionMode, BodyTrimSide, BooleanKind, BooleanOp, CoilExtent, CoilSection,
    CoilSectionPlacement, CosmeticThreadExtent, CurveProjectionDirection,
    CurveProjectionDirectionState, DecalMapping, DesignConfiguration, DesignParameter,
    DimensionDisplay, FeatureCircularArc, FeatureCoordinateFrame, FeatureEllipticArc,
    FeatureImageBounds, FeatureInputTopology, FeatureLineSegment, FeatureResultMembers,
    FeatureResultTopology, FeatureTreeNodeRole, FeatureUnitPlaneFrame, FlexForm, FlexMode,
    FuzzyTolerance, GeometryImportFormat, HelicalSweepLaw, HelicalSweepTravel,
    HelixConstructionStyle, HelixShape, InnerWireTaper, NoGeneratedSection, ParameterPmi,
    PmiDimensionSubtype, PolygonSideCount, PrincipalPlane, RevolutionFuseOrder, RibDraft, RibSide,
    RuledCurveOrientation, RuledSurfaceCorner, ScaleFactors, SheetMetalBendPosition,
    SheetMetalHeightDatum, SheetMetalHemDirection, SheetMetalThicknessSide, ShellJoin, ShellMode,
    SolidSweepOperation, SurfaceContinuity, SurfaceExtension, SurfaceProjectionMode,
    SweepPathExtent, SweepTransformation, SweepTransition, ThickenSide, UnresolvedFamily, WrapMode,
};

rewrite_scalar!(BinderCopyOnChange);
rewrite_scalar!(BinderLifecycle);
rewrite_scalar!(BinderOffset);
rewrite_scalar!(BinderOffsetJoin);
rewrite_scalar!(BinderPlacement);
rewrite_scalar!(BodyRetentionMode);
rewrite_scalar!(BodyTrimSide);
rewrite_scalar!(BooleanKind);
rewrite_scalar!(BooleanOp);
rewrite_scalar!(CoilExtent);
rewrite_scalar!(CoilSection);
rewrite_scalar!(CoilSectionPlacement);
rewrite_scalar!(CosmeticThreadExtent);
rewrite_scalar!(CurveProjectionDirection);
rewrite_scalar!(CurveProjectionDirectionState);
rewrite_scalar!(DecalMapping);
rewrite_record!(DesignConfiguration, []; {id, ordinal, active, source_index, name, material, properties, parameter_overrides, bodies, parameter_values, feature_states, native_ref});
rewrite_record!(DesignParameter, []; {id, owner, ordinal, name, expression, display, value, dependencies, properties, pmi, native_ref});
rewrite_scalar!(DimensionDisplay);
rewrite_scalar!(FeatureCircularArc);
rewrite_scalar!(FeatureCoordinateFrame);
rewrite_scalar!(FeatureEllipticArc);
rewrite_scalar!(FeatureImageBounds);
rewrite_record!(FeatureInputTopology, []; {id, input_of, bodies, faces, edges, vertices, native_ref});
rewrite_scalar!(FeatureLineSegment);
rewrite_record!(FeatureResultMembers, []; {bodies, faces, edges, vertices});
rewrite_record!(FeatureResultTopology, []; {id, output_of, members, native_ref});
rewrite_scalar!(FeatureTreeNodeRole);
rewrite_scalar!(FeatureUnitPlaneFrame);
rewrite_scalar!(FlexForm);
rewrite_scalar!(FlexMode);
rewrite_scalar!(FuzzyTolerance);
rewrite_scalar!(GeometryImportFormat);
rewrite_scalar!(HelicalSweepLaw);
rewrite_scalar!(HelicalSweepTravel);
rewrite_scalar!(HelixConstructionStyle);
rewrite_scalar!(HelixShape);
rewrite_scalar!(InnerWireTaper);
rewrite_scalar!(NoGeneratedSection);
rewrite_record!(ParameterPmi, []; {subtype, precision, display_text, basic, inspection, reference_only, native_ref});
rewrite_enum!(PmiDimensionSubtype, []; {
    Linear,
    Angle,
    Diameter,
    Radial,
    Ordinate,
    Count,
    Native(field0),
});
rewrite_scalar!(PolygonSideCount);
rewrite_scalar!(PrincipalPlane);
rewrite_scalar!(RevolutionFuseOrder);
rewrite_scalar!(RibDraft);
rewrite_scalar!(RibSide);
rewrite_scalar!(RuledCurveOrientation);
rewrite_scalar!(RuledSurfaceCorner);
rewrite_scalar!(ScaleFactors);
rewrite_scalar!(SheetMetalBendPosition);
rewrite_scalar!(SheetMetalHeightDatum);
rewrite_scalar!(SheetMetalHemDirection);
rewrite_scalar!(SheetMetalThicknessSide);
rewrite_scalar!(ShellJoin);
rewrite_scalar!(ShellMode);
rewrite_scalar!(SolidSweepOperation);
rewrite_scalar!(SurfaceContinuity);
rewrite_scalar!(SurfaceExtension);
rewrite_scalar!(SurfaceProjectionMode);
rewrite_scalar!(SweepPathExtent);
rewrite_scalar!(SweepTransformation);
rewrite_scalar!(SweepTransition);
rewrite_scalar!(ThickenSide);
rewrite_scalar!(UnresolvedFamily);
rewrite_scalar!(WrapMode);

rewrite_scalar!(super::SweepCircularRegion);
