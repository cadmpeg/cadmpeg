// SPDX-License-Identifier: Apache-2.0
//! Geometry-report and native-relation design-loss tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use crate::container::ContainerScan;
use crate::native::SldprtNative;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputLane, FeatureInputName, FeatureInputRelationBinding, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use cadmpeg_ir::features::{
    DesignParameter, Feature, FeatureDefinition, FeatureId, FeatureTreeNodeRole, ParameterId,
    ParameterPmi, ParameterValue, PmiDimensionSubtype,
};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchId, SpatialSketchEntity,
    SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchId,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn native_planar_and_spatial_sketch_geometry_is_reported() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.sketch_entities.push(SketchEntity {
        id: SketchEntityId("planar-entity".into()),
        sketch: SketchId("planar-sketch".into()),
        construction: false,
        native_ref: Some("native:planar".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "SplineHandle".into(),
        },
    });
    ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
        id: SpatialSketchEntityId("spatial-entity".into()),
        sketch: SpatialSketchId("spatial-sketch".into()),
        construction: false,
        native_ref: Some("native:spatial".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SpatialSketchGeometry::Native {
            native_kind: "ReferenceCurve".into(),
        },
    });
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "2 sketch entity geometry record(s) retain native kinds without solved neutral geometry."
    }));
}

#[test]
fn only_sketch_owned_relation_records_without_constraints_are_counted() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("sketch-feature".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::default(),
            sketch: Some(SketchId("sketch".into())),
        },
        native_ref: Some("feature".into()),
    });
    ir.model.sketch_entities.push(SketchEntity {
        id: SketchEntityId("represented-geometry".into()),
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: Some("geometry-marker".into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "UnknownGeometry".into(),
        },
    });
    let marker = |id: &str, ordinal, kind| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal,
        offset: u64::from(ordinal),
        object_index: None,
        local_id: None,
        kind,
        state_value: None,
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let relation = FeatureInputRelationInstance {
        id: "relation-instance".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: Vec::new(),
    };
    let binding =
        |id: &str, class_ref: &str, scalar_ref: &str, ordinal| FeatureInputRelationBinding {
            id: id.into(),
            parent: "lane".into(),
            ordinal,
            offset: u64::from(ordinal),
            class_ref: class_ref.into(),
            family: FeatureInputRelationFamily::PointPointDistance,
            scalar_ref: scalar_ref.into(),
            feature_ref: Some("feature".into()),
        };
    let mut relation_marker = marker(
        "relation-marker",
        0,
        SketchInputKind::Relation(SketchRelationKind::Horizontal),
    );
    relation_marker.links.push(SketchInputLink {
        local_id: 1,
        entity_ref: "geometry-marker".into(),
    });
    let native = SldprtNative {
        feature_input_lanes: vec![FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: vec![
                binding("grouped-binding", "class", "scalar", 0),
                binding("orphan-binding", "other-class", "other-scalar", 1),
            ],
            relation_instances: vec![relation],
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: vec![
                relation_marker,
                marker(
                    "dimension-handle",
                    1,
                    SketchInputKind::Relation(SketchRelationKind::Distance),
                ),
                marker("geometry-marker", 2, SketchInputKind::Native(99)),
                marker(
                    "operandless-relation-marker",
                    3,
                    SketchInputKind::Relation(SketchRelationKind::Vertical),
                ),
            ],
        }],
        ..SldprtNative::default()
    };

    assert_eq!(unprojected_sketch_relation_records(&ir, &native), 3);

    ir.model.features[0].definition = FeatureDefinition::TreeNode {
        role: FeatureTreeNodeRole::History,
        children: Vec::new(),
        active_child: None,
    };
    assert_eq!(unprojected_sketch_relation_records(&ir, &native), 0);
}

#[test]
fn native_relation_records_have_at_most_one_neutral_owner() {
    let mut ir = CadIr::empty(Units::default());
    let entity = |id: &str, native_ref: &str| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: SketchId("sketch".into()),
        construction: false,
        native_ref: Some(native_ref.into()),
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "UnknownGeometry".into(),
        },
    };
    ir.model.sketch_entities = vec![
        entity("first", "relation-marker"),
        entity("second", "relation-marker"),
        entity("profile", "profile-stream-record"),
    ];
    let native = SldprtNative {
        feature_input_lanes: vec![FeatureInputLane {
            id: "lane".into(),
            configuration: None,
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
            sketch_entities: vec![
                SketchInputEntity {
                    id: "relation-marker".into(),
                    parent: "lane".into(),
                    feature_ref: Some("feature".into()),
                    ordinal: 0,
                    offset: 0,
                    object_index: None,
                    local_id: None,
                    kind: SketchInputKind::Relation(SketchRelationKind::Horizontal),
                    state_value: None,
                    coordinates_m: None,
                    links: vec![SketchInputLink {
                        local_id: 1,
                        entity_ref: "geometry-marker".into(),
                    }],
                    link_selector: None,
                },
                SketchInputEntity {
                    id: "geometry-marker".into(),
                    parent: "lane".into(),
                    feature_ref: Some("feature".into()),
                    ordinal: 1,
                    offset: 1,
                    object_index: None,
                    local_id: Some(1),
                    kind: SketchInputKind::Native(99),
                    state_value: None,
                    coordinates_m: None,
                    links: Vec::new(),
                    link_selector: None,
                },
            ],
        }],
        ..SldprtNative::default()
    };

    assert_eq!(multiply_projected_sketch_relation_records(&ir, &native), 1);
}

#[test]
fn direct_feature_input_operations_require_unique_history_bindings() {
    let class_name = "moExtrusion_c";
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: vec![FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10,
            name: class_name.into(),
            role: FeatureInputClassRole::Feature,
        }],
        names: vec![FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10 + 6 + class_name.len() as u64,
            object_id: Some(42),
            value: "Boss".into(),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let mut native = SldprtNative {
        feature_input_lanes: vec![lane.clone()],
        ..SldprtNative::default()
    };
    assert_eq!(unbound_feature_input_operation_objects(&native), 1);

    native.feature_histories.push(FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![NativeFeature {
            id: "feature".into(),
            parent: "history".into(),
            xml_tag: "Extrusion".into(),
            tree_parent: None,
            source_id: Some("42".into()),
            parent_source_id: None,
            ordinal: 0,
            name: "Boss".into(),
            kind: "Extrusion".into(),
            input_class: Some(class_name.into()),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    });
    assert_eq!(unbound_feature_input_operation_objects(&native), 0);
    native.feature_histories[0].features[0].input_class = None;
    assert_eq!(unbound_feature_input_operation_objects(&native), 0);
    native.feature_histories[0].features[0].xml_tag = "Sketch".into();
    native.feature_histories[0].features[0].kind = "Sketch".into();
    native.feature_histories[0].features[0].name = "Profile".into();
    lane.classes[0].name = "moProfileFeature_c".into();
    lane.names[0].offset = 10 + 6 + "moProfileFeature_c".len() as u64;
    lane.names[0].value = "Profile".into();
    native.feature_input_lanes = vec![lane.clone()];
    assert_eq!(unbound_feature_input_operation_objects(&native), 0);
    native.feature_histories[0].features[0].xml_tag = "Extrusion".into();
    native.feature_histories[0].features[0].kind = "Extrusion".into();
    native.feature_histories[0].features[0].name = "Boss".into();
    native.feature_histories[0].features[0].input_class = Some(class_name.into());
    lane.classes[0].name = class_name.into();
    lane.names[0].offset = 10 + 6 + class_name.len() as u64;
    lane.names[0].value = "Boss".into();
    native.feature_input_lanes = vec![lane.clone()];
    native.feature_histories[0].features[0].input_class = Some("moSweep_c".into());
    assert_eq!(unbound_feature_input_operation_objects(&native), 1);
    native.feature_histories[0].features[0].input_class = Some(class_name.into());
    native.feature_histories[0].features[0].source_id = None;
    assert_eq!(unbound_feature_input_operation_objects(&native), 0);
    let mut duplicate = native.feature_histories[0].features[0].clone();
    duplicate.id = "duplicate-feature".into();
    native.feature_histories[0].features.push(duplicate);
    assert_eq!(unbound_feature_input_operation_objects(&native), 1);

    lane.names[0].offset += 1;
    native.feature_input_lanes = vec![lane];
    assert_eq!(unbound_feature_input_operation_objects(&native), 0);
}

#[test]
fn native_dimension_subtypes_are_reported() {
    let mut ir = CadIr::empty(Units::default());
    let owner = FeatureId("owner".into());
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: Some("Feature".into()),
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(owner),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Real(1.0)),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: Some(ParameterPmi {
            subtype: PmiDimensionSubtype::Native("Ordinate".into()),
            precision: 3,
            display_text: None,
            basic: false,
            inspection: false,
            reference_only: false,
            native_ref: "native:pmi".into(),
        }),
        native_ref: None,
    });
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "0 semantic dimension record(s) are not bound to parameters; 1 parameter dimension(s) retain native subtypes."
    }));
}

#[test]
fn geometry_report_surfaces_ambiguous_pcurve_loss() {
    let scan = ContainerScan {
        source_image: &[],
        version: 0,
        blocks: Vec::new(),
        directory: Vec::new(),
        cache_cells: Vec::new(),
        compound_streams: Vec::new(),
    };
    let mut decoded = Brep::default();
    decoded.stats.ambiguous_pcurve_parameters = 2;

    let report = super::super::build_geometry_report(&scan, &decoded);
    assert!(report.losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::GeometryPcurveAmbiguous.kind()
            && loss.message.contains("2 pcurve(s)")
    }));
}
