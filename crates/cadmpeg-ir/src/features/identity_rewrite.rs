// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{CosmeticThreadExtent, PrincipalPlane, UnresolvedFamily};
use crate::features::binders::{
    BinderCopyOnChange, BinderLifecycle, BinderOffset, BinderOffsetJoin, BinderPlacement,
};
use crate::features::datums::DecalMapping;
use crate::features::direct_edit::{FlexForm, FlexMode, ScaleFactors, ThickenSide};
use crate::features::evaluation::{
    EdgeSupport, FeatureInputTopology, FeatureResultMembers, FeatureResultTopology,
    HistoricalBodyWriter, HistoricalEdgeCarrier, HistoricalEdgeGeometry, HistoricalFaceGeometry,
    HistoricalFaceOwner,
    HistoricalFaceSurface,
};
use crate::features::geometry::{
    FeatureCircularArc, FeatureCoordinateFrame, FeatureEllipticArc, FeatureImageBounds,
    FeatureLineSegment, FeatureUnitPlaneFrame,
};
use crate::features::operation_settings::{
    BodyRetentionMode, BodyTrimSide, BooleanKind, BooleanOp, FuzzyTolerance, GeometryImportFormat,
    RuledCurveOrientation, ShellJoin, ShellMode,
};
use crate::features::parameters::{
    DesignConfiguration, DesignParameter, DimensionDisplay, ParameterPmi, PmiDimensionSubtype,
    PolygonSideCount,
};
use crate::features::revolve::{RevolutionFuseOrder, RibDraft, RibSide};
use crate::features::sheet_metal_forms::{
    SheetMetalBendPosition, SheetMetalHeightDatum, SheetMetalHemBendPosition,
    SheetMetalHemDirection, SheetMetalThicknessSide,
};
use crate::features::surface_inputs::{
    CurveProjectionDirection, CurveProjectionDirectionState, FilledSurfaceBoundaryCondition,
    RuledSurfaceCorner, SurfaceContinuity, SurfaceExtension, SurfaceProjectionMode, WrapMode,
};
use crate::features::sweeps::{
    CoilExtent, CoilSection, CoilSectionPlacement, HelicalSweepLaw, HelicalSweepTravel,
    HelixConstructionStyle, HelixShape, InnerWireTaper, NoGeneratedSection, SolidSweepOperation,
    SweepPathExtent, SweepTransformation, SweepTransition,
};
use crate::features::tree::FeatureTreeNodeRole;

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
rewrite_record!(DesignParameter, []; {id, product_definition, owner, dependencies, pmi, native_ref}; literal {ordinal, name, expression, display, value, properties});
rewrite_scalar!(DimensionDisplay);
rewrite_scalar!(FeatureCircularArc);
rewrite_scalar!(FeatureCoordinateFrame);
rewrite_scalar!(FeatureEllipticArc);
rewrite_scalar!(FeatureImageBounds);
rewrite_record!(FeatureInputTopology, []; {id, input_of, bodies, faces, edges, vertices, edge_geometry, face_owners, face_geometry, body_writers, native_ref});
rewrite_record!(HistoricalFaceOwner, []; {face, body});
rewrite_record!(HistoricalBodyWriter, []; {body, writer});
rewrite_record!(HistoricalFaceGeometry, []; {face, surface, reversed});
rewrite_scalar!(HistoricalFaceSurface);
rewrite_record!(EdgeSupport, []; {face, normal, with_edge});
rewrite_scalar!(HistoricalEdgeCarrier);
rewrite_record!(HistoricalEdgeGeometry, []; {edge, interior, tangent, start, end, faces, carrier, supports});
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
rewrite_record!(ParameterPmi, []; {native_ref}; literal {subtype, precision, display_text, basic, inspection, reference_only});
rewrite_scalar!(PmiDimensionSubtype);
rewrite_scalar!(PolygonSideCount);
rewrite_scalar!(PrincipalPlane);
rewrite_scalar!(RevolutionFuseOrder);
rewrite_scalar!(RibDraft);
rewrite_scalar!(RibSide);
rewrite_scalar!(RuledCurveOrientation);
rewrite_scalar!(RuledSurfaceCorner);
rewrite_scalar!(ScaleFactors);
rewrite_scalar!(SheetMetalBendPosition);
rewrite_scalar!(crate::features::sheet_metal_forms::SheetMetalCornerEffect);
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
rewrite_scalar!(SweepPathExtent);
rewrite_scalar!(SweepTransformation);
rewrite_scalar!(SweepTransition);
rewrite_scalar!(ThickenSide);
rewrite_scalar!(UnresolvedFamily);
rewrite_scalar!(WrapMode);

rewrite_scalar!(crate::features::sweeps::SweepCircularRegion);

rewrite_scalar!(FilledSurfaceBoundaryCondition);

rewrite_scalar!(crate::features::sheet_metal_forms::SheetMetalReliefForm);
rewrite_scalar!(crate::features::sheet_metal_forms::SheetMetalTransitionForm);
rewrite_record!(crate::features::sheet_metal_forms::SheetMetalRelief, []; {form, width, depth, minimum_remnant, seam_gap});
rewrite_record!(crate::features::sheet_metal_forms::SheetMetalTransition, []; {form, radius});

rewrite_scalar!(super::sheet_metal::SheetMetalContourThicknessSide);
