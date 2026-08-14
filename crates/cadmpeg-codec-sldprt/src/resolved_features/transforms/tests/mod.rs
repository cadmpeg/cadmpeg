//! Tests for the `transforms` module.
#![allow(unused_imports)]

use super::*;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputEdgeSelection, FeatureInputLane, FeatureInputName, FeatureInputOperand,
    FeatureInputOperandKind, FeatureInputReference, FeatureInputRelationFamily,
    FeatureInputRelationInstance, FeatureInputScalar, FeatureInputScalarRole, SketchInputEntity,
    SketchInputKind, SketchInputLink, SketchRelationKind,
};
use crate::resolved_features::relation_geometry::declared_entity_handle_circular_marker;
use cadmpeg_ir::annotations::{Annotations, ExactnessNote, StreamProvenance};
use cadmpeg_ir::features::{
    Angle, BooleanOp, DesignParameter, DimensionDisplay, EdgeSelection, ExtrudeExtent, ExtrudeSide,
    Feature, FeatureDefinition, FeatureId, Length, ParameterId, ParameterValue, PathRef,
    PatternKind, PatternSeed, ProfileRef, RadiusSpec, SweepMode, Termination,
};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus, SketchNativeOperand,
    SketchPlacement,
};
use std::collections::{BTreeMap, HashMap, HashSet};

fn marker(id: &str, coordinates_m: Option<[f64; 2]>) -> SketchInputEntity {
    SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature-native".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    }
}

mod frames;
mod join;
mod patterns;
mod profile;
mod relation_geometry;
mod relation_links;
mod relation_operands;
mod selection;
