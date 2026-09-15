// SPDX-License-Identifier: Apache-2.0
//! Feature-history reference, projection, and write-prepare tests.
#![allow(clippy::unwrap_used)]

use super::*;
use crate::records::FeatureSource;
use crate::test_support::*;
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Face;
use cadmpeg_ir::{
    features::{
        AngularTermination, BooleanOp, ChamferSpec, ConfigurationId, CosmeticThreadExtent,
        DatumPlaneReference, DesignConfiguration, DesignParameter, EdgeSelection, ExtrudeExtent,
        ExtrudeSide, FaceSelection, FeatureDefinition, FeatureId, FeatureSourceContent,
        FeatureTreeNodeRole, HoleBottom, HoleKind, LinearTermination, ParameterId, ParameterValue,
        PathRef, ProfileRef, RevolveExtent, RibConstruction, SplitFaceTool,
    },
    scalar::Length,
};
use std::collections::HashSet;

#[test]
fn history_from_nameless_block_keeps_annotation_owner() {
    let payload = br#"<Keywords Name="Part"><Configuration Name="Default"/><Feature id="1" Name="Boss"/></Keywords>"#;
    let mut source = outer_header();
    source.extend(make_block(0x43, "", payload));
    let scan = crate::container::scan_bytes(&source);
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut losses = Vec::new();
    let histories = super::histories(&scan, &mut annotations, &mut losses);

    assert_eq!(histories.len(), 1);
    assert!(!histories[0].features.is_empty());
    assert!(annotations
        .provenance
        .values()
        .any(|provenance| provenance.stream() == "block@8"));
}

fn feature(id: &str, source_id: Option<&str>, ordinal: u32) -> Feature {
    Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: source_id
            .map(|source_id| FeatureSource::try_from(source_id).expect("test feature source id")),
        ordinal,
        name: id.to_string(),
        kind: "Custom".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn feature_input_lane(id: &str, configuration: Option<&str>) -> crate::records::FeatureInputLane {
    crate::records::FeatureInputLane {
        id: id.into(),
        configuration: configuration.map(str::to_string),
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn design_configuration(
    id: &str,
    ordinal: u32,
    source_index: Option<u32>,
    native_ref: Option<&str>,
) -> DesignConfiguration {
    DesignConfiguration {
        id: ConfigurationId::mint(format!("synthetic:test:id#{id}")).expect("identity grammar"),
        ordinal,
        active: false,
        source_index,
        name: Some(id.to_string()),
        material: None,
        properties: BTreeMap::new(),
        bodies: Some(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: native_ref.map(str::to_string),
    }
}

fn native_configuration(id: &str, ordinal: u32, source_index: Option<u32>) -> Configuration {
    Configuration {
        id: id.into(),
        parent: "history".into(),
        ordinal,
        source_index,
        name: id.to_string(),
        material: None,
        properties: BTreeMap::new(),
    }
}

fn with_configuration_id(mut configuration: DesignConfiguration, id: u32) -> DesignConfiguration {
    configuration
        .properties
        .insert(cadmpeg_core::nonblank_literal!("id"), id.to_string());
    configuration
}

fn native_with_configuration_id(mut configuration: Configuration, id: u32) -> Configuration {
    configuration
        .properties
        .insert(cadmpeg_core::nonblank_literal!("id"), id.to_string());
    configuration
}

fn native_with_configuration_lanes(
    configurations: Vec<Configuration>,
    lanes: Vec<crate::records::FeatureInputLane>,
) -> crate::native::SldprtNative {
    crate::native::SldprtNative {
        feature_histories: vec![FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations,
            features: Vec::new(),
        }],
        feature_input_lanes: lanes,
        ..crate::native::SldprtNative::default()
    }
}

mod configuration;
mod equations;
mod extrusion_profile;
mod feature_operations;
mod feature_projection;
mod offset_planes;
mod parameters;
mod sketch_bind;
mod sketch_relations;
mod split_and_identity;
mod tree_binding;
