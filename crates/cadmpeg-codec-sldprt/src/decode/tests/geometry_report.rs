// SPDX-License-Identifier: Apache-2.0
//! Geometry-report and native-relation design-loss tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use crate::container::ContainerScan;
use crate::native::SldprtNative;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputLane,
    FeatureInputName, FeatureInputRelationBinding, FeatureInputRelationFamily,
    FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchInputLink,
    SketchRelationKind,
};
use cadmpeg_ir::features::{
    DesignParameter, Feature, FeatureDefinition, FeatureId, FeatureTreeNodeRole, ParameterId,
    ParameterPmi, ParameterValue, PmiDimensionSubtype,
};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchId, SpatialSketchEntity,
    SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchGeometryDefinition, SpatialSketchId,
};
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

#[test]
fn native_planar_and_spatial_sketch_geometry_is_reported() {
    let mut ir = CadIr::empty();
    ir.model.sketch_entities.push(
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#planar-entity").unwrap(),
            SketchId::mint("synthetic:test:id#planar-sketch").unwrap(),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("SplineHandle")
                    .expect("nonempty source identity"),
            ),
        )
        .with_native_ref(Some("native:planar".into())),
    );
    ir.model.spatial_sketch_entities.push(
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint("synthetic:test:id#spatial-entity").unwrap(),
            SpatialSketchId::mint("synthetic:test:id#spatial-sketch").unwrap(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Native {
                native_kind: cadmpeg_ir::products::NonEmptyString::new("ReferenceCurve")
                    .expect("nonempty source identity"),
            })
            .unwrap(),
        )
        .with_native_ref(Some("native:spatial".into())),
    );
    let mut report = super::empty_report(true);

    append_design_losses(&ir, &mut report);

    assert!(report.losses.iter().any(|loss| {
        loss.message
            == "2 sketch entity geometry record(s) retain native kinds without solved neutral geometry."
    }));
}

#[test]
fn only_sketch_owned_relation_records_without_constraints_are_counted() {
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#sketch-feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                    SketchId::mint("synthetic:test:id#sketch").unwrap(),
                )),
            },
        ),
        native_ref: Some("feature".into()),
    });
    ir.model.sketch_entities.push(
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#represented-geometry").unwrap(),
            SketchId::mint("synthetic:test:id#sketch").unwrap(),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("UnknownGeometry")
                    .expect("nonempty source identity"),
            ),
        )
        .with_native_ref(Some("geometry-marker".into())),
    );
    let marker = |id: &str, ordinal, kind| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, ordinal, u64::from(ordinal), kind);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.object_index = None;
        constructed_marker.local_id = None;
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = None;
        constructed_marker.links = None;
        constructed_marker
    };
    let relation = FeatureInputRelationInstance {
        id: "relation-instance".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        family: FeatureInputRelationFamily::PointPointDistance,
        class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec!["scalar".into()],
            Some("scalar".into()),
            None,
        )
        .unwrap(),
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
    relation_marker.links = crate::records::SketchInputLinks::new(
        0,
        relation_marker
            .links()
            .iter()
            .cloned()
            .chain(std::iter::once(SketchInputLink {
                local_id: 1,
                entity_ref: "geometry-marker".into(),
            }))
            .collect(),
    );
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
                marker("geometry-marker", 2, SketchInputKind::from_native_code(99)),
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

    ir.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: cadmpeg_ir::features::TreeChildren::default(),
        })
        .unwrap();
    assert_eq!(unprojected_sketch_relation_records(&ir, &native), 0);
}

#[test]
fn native_relation_records_have_at_most_one_neutral_owner() {
    let mut ir = CadIr::empty();
    let entity = |id: &str, native_ref: &str| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("synthetic:test:id#sketch").unwrap(),
            SketchGeometry::native(
                cadmpeg_ir::products::NonEmptyString::new("UnknownGeometry")
                    .expect("nonempty source identity"),
            ),
        )
        .with_native_ref(Some(native_ref.into()))
    };
    ir.model.sketch_entities = vec![
        entity("synthetic:test:id#first", "relation-marker"),
        entity("synthetic:test:id#second", "relation-marker"),
        entity("synthetic:test:id#profile", "profile-stream-record"),
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
                {
                    let marker_id: String = "relation-marker".into();
                    let marker_parent: String = "lane".into();
                    let mut constructed_marker = SketchInputEntity::new(
                        marker_id,
                        marker_parent,
                        0,
                        0,
                        SketchInputKind::Relation(SketchRelationKind::Horizontal),
                    );
                    constructed_marker.feature_ref = Some("feature".into());
                    constructed_marker.object_index = None;
                    constructed_marker.local_id = None;
                    constructed_marker.state_value = None;
                    constructed_marker.coordinates_m = None;
                    constructed_marker.links = crate::records::SketchInputLinks::new(
                        0,
                        vec![SketchInputLink {
                            local_id: 1,
                            entity_ref: "geometry-marker".into(),
                        }],
                    );
                    constructed_marker
                },
                {
                    let marker_id: String = "geometry-marker".into();
                    let marker_parent: String = "lane".into();
                    let mut constructed_marker = SketchInputEntity::new(
                        marker_id,
                        marker_parent,
                        1,
                        1,
                        SketchInputKind::from_native_code(99),
                    );
                    constructed_marker.feature_ref = Some("feature".into());
                    constructed_marker.object_index = None;
                    constructed_marker.local_id = Some(1);
                    constructed_marker.state_value = None;
                    constructed_marker.coordinates_m = None;
                    constructed_marker.links = None;
                    constructed_marker
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
    let mut ir = CadIr::empty();
    let owner = FeatureId::mint("synthetic:test:id#owner").expect("identity grammar");
    ir.model.features.push(Feature {
        id: owner.clone(),
        ordinal: 0,
        name: Some("Feature".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            },
        ),
        native_ref: None,
    });
    ir.model.parameters.push(DesignParameter {
        id: ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"),
        owner: Some(owner),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Real(
            cadmpeg_ir::features::FiniteReal::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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
        solidworks: crate::container::SolidWorksEnvelopeScan::default(),
    };
    let mut decoded = Brep::default();
    decoded.stats.ambiguous_pcurve_parameters = 2;

    let classification = crate::dialect::classify_layers(&scan);
    let report = super::super::build_geometry_report(&scan, &decoded, &classification);
    assert!(report.losses.iter().any(|loss| {
        loss.code == crate::loss::SldprtLossCode::GeometryPcurveAmbiguous.kind()
            && loss.message.contains("2 pcurve(s)")
    }));
}
