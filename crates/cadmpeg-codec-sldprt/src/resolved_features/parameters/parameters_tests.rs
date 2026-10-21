//! Tests for the `parameters` module.

use super::{
    enrich_history_parameters, native_scalar_matches_discrete_parameter,
    scalar_unit_from_feature_parameter, sync_feature_scalars, DeclaredParameters, ScalarOwners,
    ScalarUnit,
};
use crate::records::FeatureSource;
use crate::records::ObjectId;
use crate::records::{
    FeatureContent, FeatureHistory, FeatureInputClass, FeatureInputLane, FeatureInputName,
    FeatureInputScalar, FeatureInputScalarRole,
};
use std::collections::BTreeMap;

#[test]
fn native_scalar_must_match_an_existing_discrete_parameter() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        ordinal: 0,
        name: "Pattern".into(),
        kind: "Pattern".into(),
        input_class: Some("moLPattern_c".into()),
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    assert!(native_scalar_matches_discrete_parameter(
        &feature, "D1", "15", 15.0
    ));
    assert!(!native_scalar_matches_discrete_parameter(
        &feature,
        "D1",
        "15",
        8.371_160_993_642_741e298
    ));
}

#[test]
fn fillet_display_placeholder_establishes_length_unit() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: None,
        ordinal: 0,
        name: "Fillet".into(),
        kind: "Fillet".into(),
        input_class: Some("Fillet_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([(cadmpeg_core::nonblank_literal!("D1"), "R0".into())]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "D1"),
        Some(super::ScalarUnit::Length)
    );
    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "missing"),
        None
    );
    let mut numeric = feature;
    numeric
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "0".into());
    assert_eq!(scalar_unit_from_feature_parameter(&numeric, "D1"), None);

    let mut cosmetic_thread = numeric.clone();
    cosmetic_thread.kind = "CosmeticThread".into();
    cosmetic_thread.input_class = Some("CosmeticThread_c".into());
    cosmetic_thread.parameters.clear();
    cosmetic_thread
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D2"), "<MOD-DIAM>6".into());
    assert_eq!(
        scalar_unit_from_feature_parameter(&cosmetic_thread, "D2"),
        None
    );

    let mut variable = numeric;
    variable.kind = "VarFillet".into();
    variable.input_class = Some("VarFillet_c".into());
    variable
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D0"), "R0".into());
    variable
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D01"), "R0".into());
    assert_eq!(
        scalar_unit_from_feature_parameter(&variable, "D0"),
        Some(super::ScalarUnit::Length)
    );
    assert_eq!(
        scalar_unit_from_feature_parameter(&variable, "D01"),
        Some(super::ScalarUnit::Length)
    );
    assert_eq!(scalar_unit_from_feature_parameter(&variable, "D1"), None);
}

#[test]
fn thin_cut_native_dimensions_are_lengths() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Extrusion".into(),
        tree_parent: None,
        source_id: None,
        ordinal: 0,
        name: "Thin cut".into(),
        kind: "Cut-Extrude-Thin".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::from([
            (cadmpeg_core::nonblank_literal!("D5"), "0.3".into()),
            (cadmpeg_core::nonblank_literal!("D6"), "0.1".into()),
            (cadmpeg_core::nonblank_literal!("D7"), "0.2".into()),
        ]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };

    for name in ["D5", "D6", "D7"] {
        assert_eq!(
            scalar_unit_from_feature_parameter(&feature, name),
            Some(ScalarUnit::Length)
        );
    }
    assert_eq!(scalar_unit_from_feature_parameter(&feature, "D8"), None);
}

#[test]
fn sketch_source_dimension_establishes_scalar_unit() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        ordinal: 0,
        name: "Sketch".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([
            (cadmpeg_core::nonblank_literal!("depth"), "0.75".into()),
            (cadmpeg_core::nonblank_literal!("angle"), "90°".into()),
            (cadmpeg_core::nonblank_literal!("unowned"), "1".into()),
        ]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![
            FeatureContent::Dimension("depth".into()),
            FeatureContent::Dimension("angle".into()),
        ],
    };

    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "depth"),
        Some(ScalarUnit::Length)
    );
    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "angle"),
        Some(ScalarUnit::Angle)
    );
    assert_eq!(
        scalar_unit_from_feature_parameter(&feature, "unowned"),
        None
    );
}

#[test]
fn explicit_sketch_dimension_scalar_preserves_display_outside_object_range() {
    let feature = crate::records::Feature {
        id: "feature".into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(1738),
        ordinal: 0,
        name: "Sketch".into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("D1"),
            "<MOD-DIAM>0.281".into(),
        )]),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: vec![FeatureContent::Dimension("D1".into())],
    };
    let mut later_feature = feature.clone();
    later_feature.id = "later-feature".into();
    later_feature.source_id = FeatureSource::from_value(2000);
    later_feature.name = "Later".into();
    later_feature.parameters.clear();
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature, later_feature],
    }];
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 136].into(),
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                sequence: 0,
                feature_ref: None,
                id: "feature-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: Some(0),
                object_id: ObjectId::from_value(1738),
                value: "Sketch".into(),
            },
            FeatureInputName {
                sequence: 64,
                feature_ref: None,
                id: "later-feature-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: Some(64),
                object_id: ObjectId::from_value(2000),
                value: "Later".into(),
            },
            FeatureInputName {
                sequence: 100,
                feature_ref: None,
                id: "d1-name".into(),
                parent: "lane".into(),
                ordinal: 2,
                offset: Some(100),
                object_id: None,
                value: "D1".into(),
            },
        ],
        scalars: vec![FeatureInputScalar {
            sequence: 128,
            id: "scalar".into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal: 0,
            offset: Some(128),
            object_id: 1739,
            name: "d1-name".into(),
            value: cadmpeg_ir::scalar::FiniteReal::new(0.007_137_4).expect("finite test scalar"),
            role: FeatureInputScalarRole::Driving,
            operands: Vec::new(),
        }],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    enrich_history_parameters(&mut histories, [&lane], true);
    assert_eq!(
        histories[0].features[0].parameters["D1"],
        "<MOD-DIAM>7.1374"
    );
    // A semantic native load has no source offsets. Two unavailable offsets
    // are not evidence that this scalar has the value-only wire framing.
    let mut semantic_lane = lane.clone();
    for name in &mut semantic_lane.names {
        name.offset = None;
    }
    semantic_lane.scalars[0].offset = None;
    histories[0].features[0].dimension_properties.insert(
        "D1".to_owned(),
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("Value"),
            "<MOD-DIAM>25".into(),
        )]),
    );
    enrich_history_parameters(&mut histories, [&semantic_lane], true);
    assert_eq!(
        histories[0].features[0].dimension_properties["D1"]["Value"],
        "<MOD-DIAM>7.1374"
    );
    assert_eq!(
        crate::history::parameters::source_parameter_value(&histories[0].features[0], "D1"),
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(7.1374).unwrap()
        ))
    );
    semantic_lane.classes.push(FeatureInputClass {
        id: "length-class".into(),
        parent: "lane".into(),
        ordinal: 0,
        sequence: 99,
        offset: None,
        feature_ref: None,
        name: "moLengthParameter_c".into(),
    });
    assert!(super::scalar_quantity_matches(
        &semantic_lane,
        &semantic_lane.scalars[0],
        &cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(7.1374).unwrap()
        ),
    ));
    assert!(!super::scalar_quantity_matches(
        &semantic_lane,
        &semantic_lane.scalars[0],
        &cadmpeg_ir::features::ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap()
        ),
    ));
    histories[0].features[0]
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "<MOD-DIAM>8".into());
    sync_feature_scalars(
        &cadmpeg_ir::document::Model::default(),
        &histories,
        std::slice::from_mut(&mut lane),
    )
    .expect("explicit scalar owner is writable");
    assert_eq!(lane.scalars[0].value.get(), 0.008);
    assert_eq!(&lane.native_payload[128..136], &0.008f64.to_le_bytes());

    // An unmatched native configuration does not inherit the global cache.
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut model = cadmpeg_ir::document::Model::default();
    model.features = crate::history::project::project_features(&ctx, &histories).unwrap();
    model.parameters = crate::history::parameters::project_parameters(&ctx, &histories).unwrap();
    assert_eq!(model.parameters.len(), 1);
    model.parameters[0].value = Some(cadmpeg_ir::features::ParameterValue::Length(
        cadmpeg_ir::scalar::Length::new(99.0).unwrap(),
    ));
    lane.configuration = Some("99".into());
    sync_feature_scalars(&model, &histories, std::slice::from_mut(&mut lane)).unwrap();
    assert_eq!(lane.scalars[0].value.get(), 0.008);
    assert_eq!(&lane.native_payload[128..136], &0.008f64.to_le_bytes());
    assert_eq!(
        model.parameters[0].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(99.0).unwrap(),
        ))
    );
}

#[test]
fn parameter_indexes_match_the_class_name_and_scalar_scans() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let mut lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
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
    };
    let features = ["feature-0", "feature-1", "feature-2"];
    for _ in 0..256 {
        lane.classes = (0..next(6))
            .map(|ordinal| FeatureInputClass {
                feature_ref: None,
                id: format!("class-{ordinal}"),
                parent: "lane".into(),
                ordinal: u32::try_from(ordinal).unwrap(),
                sequence: u32::try_from(ordinal).unwrap(),
                offset: Some(next(40)),
                name: "moLengthParameter_c".into(),
            })
            .collect();
        lane.names = (0..next(8))
            .map(|ordinal| FeatureInputName {
                feature_ref: None,
                id: format!("name-{ordinal}"),
                parent: "lane".into(),
                ordinal: u32::try_from(ordinal).unwrap(),
                sequence: u32::try_from(ordinal).unwrap(),
                offset: Some(next(40)),
                object_id: None,
                value: "D1".into(),
            })
            .collect();
        lane.scalars = (0..next(10))
            .map(|ordinal| FeatureInputScalar {
                id: format!("scalar-{ordinal}"),
                parent: "lane".into(),
                feature_ref: (next(3) == 0)
                    .then(|| features[usize::try_from(next(3)).unwrap()].to_owned()),
                ordinal: u32::try_from(ordinal).unwrap(),
                sequence: u32::try_from(ordinal).unwrap(),
                offset: Some(next(40)),
                object_id: 0,
                name: "name".into(),
                value: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                role: FeatureInputScalarRole::Native,
                operands: Vec::new(),
            })
            .collect();

        for class in &mut lane.classes {
            class.sequence = u32::try_from(class.offset.expect("fixture source position")).unwrap();
        }
        for name in &mut lane.names {
            name.sequence = u32::try_from(name.offset.expect("fixture source position")).unwrap();
        }
        for scalar in &mut lane.scalars {
            scalar.sequence =
                u32::try_from(scalar.offset.expect("fixture source position")).unwrap();
        }

        let declared = DeclaredParameters::new(&lane);
        for name in &lane.names {
            let expected = lane
                .classes
                .iter()
                .filter(|class| class.offset < name.offset)
                .max_by_key(|class| class.offset)
                .filter(|class| {
                    !lane.names.iter().any(|intervening| {
                        intervening.offset > class.offset && intervening.offset < name.offset
                    })
                });
            assert_eq!(
                declared.class_of(name).map(std::ptr::from_ref),
                expected.map(std::ptr::from_ref)
            );
        }
        let owners = ScalarOwners::new(&lane.scalars);
        for feature in features {
            let start = u32::try_from(next(40)).unwrap();
            let end = (next(4) != 0).then(|| start + u32::try_from(next(20)).unwrap());
            let expected = lane
                .scalars
                .iter()
                .enumerate()
                .filter(|(_, scalar)| {
                    scalar.feature_ref.as_deref() == Some(feature)
                        || (scalar.feature_ref.is_none()
                            && scalar.sequence > start
                            && end.is_none_or(|end| scalar.sequence < end))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            assert_eq!(owners.owned(feature, start, end), expected);
        }
    }
}
