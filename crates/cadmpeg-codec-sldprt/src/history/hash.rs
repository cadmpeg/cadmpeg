// SPDX-License-Identifier: Apache-2.0
//! Stable hashes of projected and native history state.

#![allow(unused_imports)]
use crate::classification::{
    classify, classify_type_token, classify_xml_element, native_object_class,
    principal_plane_with_siblings, FeatureClass, NativeClassKind,
};
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::{
    Angle, AxisAngle, BodyRetentionMode, BodySelection, BooleanOp, ChamferForm, ChamferSpec,
    ConfigurationBodies, ConfigurationId, CosmeticThreadExtent, CurveProjectionDirection,
    CurveProjectionDirectionState, DatumPlaneReference, DesignConfiguration, DesignParameter,
    DimensionDisplay, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceMotion, FaceSelection,
    FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole, FlexForm, FlexMode,
    HoleBottom, HoleForm, HoleKind, Length, ParameterId, ParameterValue, PathRef, PatternForm,
    PatternKind, PatternSeed, ProfileRef, RadiusForm, RadiusSpec, RevolutionAxis,
    RevolutionConstruction, RevolveExtent, RibConstruction, RibDraft, RibSide, RuledSurfaceMode,
    ScaleCenter, ScaleFactors, SketchSpace, SplitFaceTool, SurfaceExtension, SweepMode,
    Termination, TrimRegion, VariableRadius, VertexSelection, WrapMode,
};
use cadmpeg_ir::geometry::{Curve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

pub fn feature_hash(features: &[cadmpeg_ir::features::Feature]) -> String {
    let mut features = features.to_vec();
    features.sort_by(|left, right| left.id.cmp(&right.id));
    hash_debug(&features)
}

/// Stable hash of the native feature histories.
pub fn history_hash(histories: &[FeatureHistory]) -> String {
    hash_debug(histories)
}

/// Stable hash of neutral configurations.
pub fn configuration_hash(configurations: &[DesignConfiguration]) -> String {
    let mut configurations = configurations.to_vec();
    configurations.sort_by(|left, right| left.id.cmp(&right.id));
    hash_debug(&configurations)
}

/// Stable hash of configuration-local evaluated parameter state.
pub fn configuration_parameter_value_hash(configurations: &[DesignConfiguration]) -> String {
    let mut values = configurations
        .iter()
        .filter(|configuration| !configuration.parameter_values.is_empty())
        .map(|configuration| (&configuration.id, &configuration.parameter_values))
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(right.0));
    hash_debug(&values)
}

/// Stable hash of configuration-local evaluated feature state.
pub fn configuration_feature_state_hash(configurations: &[DesignConfiguration]) -> String {
    let mut states = configurations
        .iter()
        .filter(|configuration| !configuration.feature_states.is_empty())
        .map(|configuration| (&configuration.id, &configuration.feature_states))
        .collect::<Vec<_>>();
    states.sort_by(|left, right| left.0.cmp(right.0));
    hash_debug(&states)
}

/// Stable hash of native configuration records.
pub fn native_configuration_hash(histories: &[FeatureHistory]) -> String {
    let mut configurations = histories
        .iter()
        .flat_map(|history| history.configurations.clone())
        .collect::<Vec<_>>();
    configurations.sort_by(|left, right| left.id.cmp(&right.id));
    hash_debug(&configurations)
}

/// Stable hash of neutral feature parameters.
pub fn parameter_hash(parameters: &[DesignParameter]) -> String {
    let mut parameters = parameters.to_vec();
    parameters.sort_by(|left, right| left.id.cmp(&right.id));
    hash_debug(&parameters)
}

/// Stable hash of native feature parameters, properties, and ordering.
pub fn native_parameter_hash(histories: &[FeatureHistory]) -> String {
    let mut parameters = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| {
            (
                feature.id.clone(),
                feature.parameters.clone(),
                feature.dimension_properties.clone(),
                feature
                    .content
                    .iter()
                    .filter_map(|item| match item {
                        FeatureContent::Dimension(name) => Some(name.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    parameters.sort_by(|left, right| left.0.cmp(&right.0));
    hash_debug(&parameters)
}

pub(crate) fn hash_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> String {
    let bytes = format!("{value:?}");
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes.as_bytes()) {
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}
