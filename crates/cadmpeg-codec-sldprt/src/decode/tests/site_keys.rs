// SPDX-License-Identifier: Apache-2.0
//! Container site-key identity tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use super::super::*;
use crate::container::{Block, CompoundStream, ContainerScan};
use crate::native::SldprtNative;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputLane, FeatureInputName, FeatureInputRelationBinding, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use cadmpeg_ir::features::{
    Angle, BodyRetentionMode, BodySelection, BooleanOp, ConfigurationFeatureState, ConfigurationId,
    DesignConfiguration, DesignParameter, EdgeSelection, FaceSelection, Feature, FeatureDefinition,
    FeatureId, FeatureSourceContent, FeatureTreeNodeRole, HoleBottom, HoleKind, HolePlacement,
    Length, ParameterId, ParameterPmi, ParameterValue, PathRef, PatternKind, PatternSeed,
    PmiDimensionSubtype, RadiusSpec, RuledSurfaceMode, SurfaceContinuity, Termination,
};
use cadmpeg_ir::ids::{BodyId, EdgeId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchConstraintId, SketchEntity, SketchEntityId, SketchGeometry,
    SketchId, SpatialSketchConstraint, SpatialSketchConstraintDefinition, SpatialSketchEntity,
    SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchId,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn site_keys_use_outer_container_identity() {
    let first = Block {
        offset: 100,
        type_id: 0,
        comp_sz: 0,
        uncomp_sz: 0,
        section: Some("Contents/Config-0-Partition".into()),
        family: "parasolid",
        payload: Vec::new(),
        ps_stream: None,
        ps_streams: Vec::new(),
        ps_stream_offsets: Vec::new(),
    };
    let second = Block {
        offset: 200,
        section: first.section.clone(),
        ..first.clone()
    };
    assert_ne!(
        super::super::BodyOrigin::Block(&first).site_key(),
        super::super::BodyOrigin::Block(&second).site_key()
    );

    let compound = CompoundStream {
        path: "Contents/Config-0-Partition".into(),
        directory_id: 300,
        start_sector: 0,
        payload: Vec::new(),
        decoded_payload: None,
        ps_streams: Vec::new(),
        ps_stream_offsets: Vec::new(),
    };
    assert_eq!(
        super::super::BodyOrigin::Compound(&compound).site_key(),
        "compound@300"
    );
}
