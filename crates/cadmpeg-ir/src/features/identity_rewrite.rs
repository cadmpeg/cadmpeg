// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{
    BinderCopyOnChange, BinderLifecycle, BinderOffset, BinderOffsetJoin, BinderPlacement,
    BodyRetentionMode, BodyTrimSide, BooleanKind, BooleanOp, CoilExtent, CoilSection,
    CoilSectionPlacement, CosmeticThreadExtent, CurveProjectionDirection,
    CurveProjectionDirectionState, DecalMapping, DesignConfiguration, DesignParameter,
    DimensionDisplay, FeatureCircularArc, FeatureCoordinateFrame, FeatureEllipticArc,
    FeatureImageBounds, FeatureInputTopology, FeatureLineSegment, FeatureResultMembers,
    FeatureResultTopology, FeatureTreeNodeRole, FeatureUnitPlaneFrame,
    FilledSurfaceBoundaryCondition, FlexForm, FlexMode, FuzzyTolerance, GeometryImportFormat,
    HelicalSweepLaw, HelicalSweepTravel, HelixConstructionStyle, HelixShape, InnerWireTaper,
    NoGeneratedSection, PolygonSideCount, PrincipalPlane,
    RevolutionFuseOrder, RibDraft, RibSide, RuledCurveOrientation, RuledSurfaceCorner,
    ScaleFactors, SheetMetalBendPosition, SheetMetalHeightDatum, SheetMetalHemBendPosition,
    SheetMetalHemDirection, SheetMetalThicknessSide, ShellJoin, ShellMode, SolidSweepOperation,
    SurfaceContinuity, SurfaceExtension, SurfaceProjectionMode, SweepPathExtent,
    SweepTransformation, SweepTransition, ThickenSide, UnresolvedFamily, WrapMode,
};

rewrite_scalar!(BinderCopyOnChange);
rewrite_scalar!(BinderLifecycle);
rewrite_record!(BinderOffset, []; { distance, join, fill, open_result, intersection });
rewrite_scalar!(BinderOffsetJoin);
rewrite_scalar!(BinderPlacement);
rewrite_scalar!(BodyRetentionMode);
rewrite_scalar!(BodyTrimSide);
rewrite_scalar!(BooleanKind);
rewrite_scalar!(BooleanOp);
rewrite_enum!(CoilExtent, []; {
    RevolutionsHeight { revolutions, height },
    RevolutionsPitch { revolutions, pitch },
    HeightPitch { height, pitch },
    Spiral { revolutions, radial_pitch },
});
rewrite_enum!(CoilSection, []; {
    Circular { diameter },
    Square { size },
    ExternalTriangle { size },
    InternalTriangle { size },
});
rewrite_scalar!(CoilSectionPlacement);
rewrite_enum!(CosmeticThreadExtent, []; {
    Blind { length },
    Through {  },
});
rewrite_scalar!(CurveProjectionDirection);
rewrite_scalar!(CurveProjectionDirectionState);
rewrite_scalar!(DecalMapping);
rewrite_record!(DesignConfiguration, []; {id, ordinal, active, source_index, name, material, properties, parameter_overrides, bodies, parameter_values, feature_states, native_ref});
rewrite_record!(DesignParameter, []; {id, owner, ordinal, name, expression, driving, display, value, properties, pmi, native_ref});
rewrite_scalar!(DimensionDisplay);
rewrite_record!(FeatureCircularArc, []; {center, normal, radius, angles});
rewrite_scalar!(FeatureCoordinateFrame);
rewrite_record!(FeatureEllipticArc, []; {center, normal, major_axis, radii, angles});
rewrite_scalar!(FeatureImageBounds);
rewrite_record!(FeatureInputTopology, []; {id, input_of, bodies, faces, edges, vertices, native_ref});
rewrite_scalar!(FeatureLineSegment);
rewrite_record!(FeatureResultMembers, []; {bodies, faces, edges, vertices});
rewrite_record!(FeatureResultTopology, []; {id, output_of, members, native_ref});
rewrite_scalar!(FeatureTreeNodeRole);
rewrite_scalar!(FeatureUnitPlaneFrame);
rewrite_scalar!(FlexForm);
rewrite_enum!(FlexMode, []; {
    Unresolved { form },
    Bending { angle },
    Twisting { angle },
    Tapering { factor },
    Stretching { distance },
});
rewrite_enum!(FuzzyTolerance, []; { KernelDefault, Automatic, Explicit(value) });
rewrite_scalar!(GeometryImportFormat);
rewrite_scalar!(HelicalSweepLaw);
rewrite_enum!(HelicalSweepTravel, []; {
    Axial { height },
    Radial { radial_growth },
    Conical { height, radial_growth },
});
rewrite_scalar!(HelixConstructionStyle);
rewrite_enum!(HelixShape, []; {
    Cylindrical { law },
    Conical { law, cone_angle },
    Spiral { radial_growth, revolutions },
});
rewrite_scalar!(InnerWireTaper);
rewrite_scalar!(NoGeneratedSection);
rewrite_scalar!(PolygonSideCount);
rewrite_scalar!(PrincipalPlane);
rewrite_scalar!(RevolutionFuseOrder);
rewrite_enum!(RibDraft, []; { Unresolved, None, Angle(field0) });
rewrite_scalar!(RibSide);
rewrite_scalar!(RuledCurveOrientation);
rewrite_scalar!(RuledSurfaceCorner);
rewrite_enum!(ScaleFactors, []; {
    Unresolved {  },
    Uniform { factor },
    PerAxis { factors },
});
rewrite_scalar!(SheetMetalBendPosition);
rewrite_scalar!(SheetMetalHeightDatum);
rewrite_scalar!(SheetMetalHemBendPosition);
rewrite_scalar!(SheetMetalHemDirection);
rewrite_scalar!(SheetMetalThicknessSide);
rewrite_scalar!(ShellJoin);
rewrite_scalar!(ShellMode);
rewrite_scalar!(SolidSweepOperation);
rewrite_scalar!(SurfaceContinuity);
rewrite_scalar!(SurfaceExtension);
rewrite_scalar!(SurfaceProjectionMode);
rewrite_record!(SweepPathExtent, []; { along_fraction, against_fraction });
rewrite_scalar!(SweepTransformation);
rewrite_scalar!(SweepTransition);
rewrite_scalar!(ThickenSide);
rewrite_scalar!(UnresolvedFamily);
rewrite_enum!(WrapMode, []; {
    Emboss { depth },
    Deboss { depth },
    Scribe,
});

rewrite_record!(super::SweepCircularRegion, []; { outer_radius, wall_thickness });

rewrite_scalar!(FilledSurfaceBoundaryCondition);
