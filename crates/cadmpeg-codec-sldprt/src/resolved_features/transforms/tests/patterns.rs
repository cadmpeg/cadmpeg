//! Pattern input and line-reference binding tests.

use super::super::*;
use super::marker;
use crate::records::{
    Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
    FeatureInputLane, FeatureInputName, FeatureInputOperand, FeatureInputOperandKind,
    FeatureInputRelationFamily, FeatureInputRelationInstance,
};
use cadmpeg_ir::features::{
    DesignParameter, Feature, FeatureDefinition, FeatureId, Length, ParameterId, ParameterValue,
    PathRef, PatternKind, PatternSeed, SweepMode,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
    SketchLocus, SketchPlacement,
};
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn pattern_inputs_bind_adjacent_objects_and_line_reference_direction() {
    let native_feature = |id: &str, source_id: &str, name: &str| NativeFeature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: 0,
        name: name.into(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let mut seed_native = native_feature("seed-native", "5", "SeedFeature");
    seed_native.input_class = Some("moExtrusion_c".into());
    let mut pattern_native = native_feature("pattern-native", "10", "Pattern1");
    pattern_native.input_class = Some("moCurvePattern_c".into());
    let mut path_native = native_feature("path-native", "20", "PathSketch");
    path_native.input_class = Some("moProfileFeature_c".into());
    let next_native = native_feature("next-native", "30", "NextFeature");
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![seed_native, pattern_native, path_native, next_native],
    };
    let name = |offset: u64, object_id: u32, value: &str| FeatureInputName {
        id: format!("name-{offset}"),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        value: value.into(),
        object_id: Some(object_id),
    };
    let line_ref_offset = 120usize;
    let mut native_payload = vec![0; 400];
    native_payload[line_ref_offset + 136..line_ref_offset + 144]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    native_payload[line_ref_offset + 148..line_ref_offset + 152]
        .copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    for (index, value) in [-1.0f64, 0.0, 0.0].into_iter().enumerate() {
        let offset = line_ref_offset + 200 + index * 8;
        native_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        line_reference_direction(&native_payload, line_ref_offset as u64),
        Some(Vector3::new(-1.0, 0.0, 0.0))
    );
    assert_eq!(
        compact_line_reference_direction(
            &native_payload,
            0,
            native_payload.len(),
            &[line_ref_offset + 136],
        ),
        None
    );
    let mut three_word_payload = vec![0; 400];
    three_word_payload[line_ref_offset + 144..line_ref_offset + 156].copy_from_slice(&[
        0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff,
    ]);
    three_word_payload[line_ref_offset + 160..line_ref_offset + 164]
        .copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    for (index, value) in [0.0f64, 0.6, 0.8].into_iter().enumerate() {
        let offset = line_ref_offset + 220 + index * 8;
        three_word_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        line_reference_direction(&three_word_payload, line_ref_offset as u64),
        Some(Vector3::new(0.0, 0.6, 0.8))
    );
    let mut declared_variants = vec![0; 280];
    let addressed_handles = 32;
    declared_variants[addressed_handles..addressed_handles + 8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    declared_variants[addressed_handles + 12..addressed_handles + 16]
        .copy_from_slice(&9000u32.to_le_bytes());
    for (index, value) in [0.1f64, 0.2, 0.3, 0.4, 0.0, 0.0, -1.0]
        .into_iter()
        .enumerate()
    {
        let offset = addressed_handles + 32 + index * 8;
        declared_variants[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        declared_line_reference_directions(&declared_variants, 0, declared_variants.len()),
        vec![Vector3::new(0.0, 0.0, -1.0)]
    );
    let mut display_payload = vec![0; 512];
    let display_names = [
        FeatureInputName {
            id: "display-d3".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            object_id: Some(u32::MAX),
            value: "D3".into(),
        },
        FeatureInputName {
            id: "display-d4".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 300,
            object_id: Some(u32::MAX),
            value: "D4".into(),
        },
    ];
    for (offset, spacing, direction) in [
        (100usize, 0.027f64, [0.6f64, 0.8, 0.0]),
        (300usize, 0.039f64, [0.0f64, 0.0, 1.0]),
    ] {
        display_payload[offset + 32..offset + 40].copy_from_slice(&spacing.to_le_bytes());
        for (index, value) in direction.into_iter().enumerate() {
            let scalar = offset + 161 + index * 8;
            display_payload[scalar..scalar + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    assert_eq!(
        linear_pattern_display_directions(
            &display_payload,
            50,
            display_payload.len(),
            &display_names,
            [Some(0.027), Some(0.039)],
        ),
        vec![Vector3::new(0.6, 0.8, 0.0), Vector3::new(0.0, 0.0, 1.0)]
    );
    assert_eq!(
        linear_pattern_display_directions(
            &display_payload,
            50,
            display_payload.len(),
            &display_names,
            [Some(0.028), Some(0.039)],
        ),
        vec![Vector3::new(0.0, 0.0, 1.0)]
    );
    assert_eq!(
        linear_pattern_display_directions(
            &display_payload,
            50,
            484,
            &display_names,
            [Some(0.027), Some(0.039)],
        ),
        vec![Vector3::new(0.6, 0.8, 0.0)]
    );
    let mut compact_longer_form = vec![0; 126];
    compact_longer_form[..8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    compact_longer_form[12..16].copy_from_slice(&9000u32.to_le_bytes());
    for (index, value) in [0.1f64, 0.2, 0.3, 0.4, 0.0, 0.0, 1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = 32 + index * 8;
        compact_longer_form[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_longer_form[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    compact_longer_form[124..126].copy_from_slice(&0x8001u16.to_le_bytes());
    assert!(
        declared_line_reference_directions(&compact_longer_form, 0, compact_longer_form.len())
            .is_empty()
    );
    assert_eq!(
        compact_line_reference_direction(&compact_longer_form, 0, compact_longer_form.len(), &[]),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    let mut compact_payload = vec![0; 400];
    let handles = 64;
    compact_payload[handles..handles + 8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    compact_payload[handles + 12..handles + 16].copy_from_slice(&5000u32.to_le_bytes());
    for (index, value) in [0.58, -0.0125, 0.023, -0.29, 0.0, 0.0, 0.0, -1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = handles + 32 + index * 8;
        compact_payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    compact_payload[handles + 104..handles + 112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 0, compact_payload.len(), &[]),
        Some(Vector3::new(0.0, -1.0, 0.0))
    );
    let mut six_scalar_payload = vec![0; 160];
    six_scalar_payload[..8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    six_scalar_payload[12..16].copy_from_slice(&8000u32.to_le_bytes());
    for (index, value) in [0.2, 0.27, -0.1, 0.0, 1.0, 0.0].into_iter().enumerate() {
        let offset = 40 + index * 8;
        six_scalar_payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    six_scalar_payload[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(
        compact_line_reference_direction(&six_scalar_payload, 0, six_scalar_payload.len(), &[],),
        Some(Vector3::new(0.0, 1.0, 0.0))
    );
    let mut token_terminated_payload = vec![0; 130];
    token_terminated_payload[..8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    token_terminated_payload[12..16].copy_from_slice(&9000u32.to_le_bytes());
    for (index, value) in [1.0_f64, 0.0, 0.25, 0.855, 0.0, 0.0, 1.0, 0.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = 32 + index * 8;
        token_terminated_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    token_terminated_payload[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    token_terminated_payload[124..126].copy_from_slice(&0x82c0u16.to_le_bytes());
    assert_eq!(
        compact_line_reference_direction(
            &token_terminated_payload,
            0,
            token_terminated_payload.len(),
            &[],
        ),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    token_terminated_payload[124..126].copy_from_slice(&0x02c0u16.to_le_bytes());
    assert_eq!(
        compact_line_reference_direction(
            &token_terminated_payload,
            0,
            token_terminated_payload.len(),
            &[],
        ),
        None
    );
    let mut tagged_trailer_payload = vec![0; 144];
    tagged_trailer_payload[..8].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    tagged_trailer_payload[12..16].copy_from_slice(&9100u32.to_le_bytes());
    for (index, value) in [0.07_f64, -0.046, 0.018, 0.012, 0.0, 0.0, 1.0]
        .into_iter()
        .enumerate()
    {
        let offset = 32 + index * 8;
        tagged_trailer_payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    tagged_trailer_payload[124..126].copy_from_slice(&0x8933u16.to_le_bytes());
    tagged_trailer_payload[142..144].copy_from_slice(&[0xff; 2]);
    assert_eq!(
        compact_line_reference_direction(
            &tagged_trailer_payload,
            0,
            tagged_trailer_payload.len(),
            &[],
        ),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
    tagged_trailer_payload[124..144].fill(0);
    tagged_trailer_payload[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    tagged_trailer_payload[122..124].copy_from_slice(&0x81b3u16.to_le_bytes());
    tagged_trailer_payload[140..142].copy_from_slice(&[0xff; 2]);
    assert_eq!(
        compact_line_reference_direction(
            &tagged_trailer_payload,
            0,
            tagged_trailer_payload.len(),
            &[],
        ),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
    tagged_trailer_payload[122..124].copy_from_slice(&0x01b3u16.to_le_bytes());
    assert_eq!(
        compact_line_reference_direction(
            &tagged_trailer_payload,
            0,
            tagged_trailer_payload.len(),
            &[],
        ),
        None
    );
    tagged_trailer_payload[122..144].fill(0);
    tagged_trailer_payload[124..126].copy_from_slice(&0x8204u16.to_le_bytes());
    tagged_trailer_payload[142..144].copy_from_slice(&[0xff; 2]);
    assert_eq!(
        compact_line_reference_direction(
            &tagged_trailer_payload,
            0,
            tagged_trailer_payload.len(),
            &[],
        ),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
    let short_handles = 200;
    compact_payload[short_handles..short_handles + 8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    compact_payload[short_handles + 12..short_handles + 16].copy_from_slice(&6000u32.to_le_bytes());
    for (index, value) in [0.056, -0.0415, 0.027, 0.018, 0.0, -1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = short_handles + 24 + index * 8;
        compact_payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    compact_payload[short_handles + 80..short_handles + 88]
        .copy_from_slice(&[0xb8, 0x85, 0xad, 0x80, 0xff, 0xfe, 0xff, 0x07]);
    let eight_scalar_handles = 300;
    compact_payload[eight_scalar_handles..eight_scalar_handles + 8]
        .copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff]);
    compact_payload[eight_scalar_handles + 12..eight_scalar_handles + 16]
        .copy_from_slice(&7000u32.to_le_bytes());
    for (index, value) in [0.0, 0.988, 0.005, 0.494, 0.2215, 0.0, -1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = eight_scalar_handles + 24 + index * 8;
        compact_payload[offset..offset + 8].copy_from_slice(&f64::to_le_bytes(value));
    }
    compact_payload[eight_scalar_handles + 88..eight_scalar_handles + 96]
        .copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 200, 288, &[]),
        Some(Vector3::new(0.0, -1.0, 0.0))
    );
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 300, 400, &[]),
        Some(Vector3::new(0.0, -1.0, 0.0))
    );
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 0, compact_payload.len(), &[]),
        Some(Vector3::new(0.0, -1.0, 0.0))
    );
    compact_payload[short_handles + 56..short_handles + 80].copy_from_slice(&[
        0, 0, 0, 0, 0, 0, 0xf0, 0x3f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 0, compact_payload.len(), &[]),
        None
    );
    compact_payload[short_handles + 12..short_handles + 16].fill(0);
    compact_payload[eight_scalar_handles + 12..eight_scalar_handles + 16].fill(0);
    compact_payload[handles + 12..handles + 16].fill(0);
    assert_eq!(
        compact_line_reference_direction(&compact_payload, 0, compact_payload.len(), &[]),
        None
    );
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload,
        classes: vec![FeatureInputClass {
            id: "line-reference".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: line_ref_offset as u64,
            name: "moLineRef_w".into(),
            role: FeatureInputClassRole::Reference,
        }],
        names: vec![
            name(50, 5, "SeedFeature"),
            name(100, 10, "Pattern1"),
            name(500, 20, "PathSketch"),
            name(600, 30, "NextFeature"),
        ],
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
    let model_feature = |id: &str, native_ref: &str, definition| Feature {
        id: FeatureId(id.into()),
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
        definition,
        native_ref: Some(native_ref.into()),
    };
    let sketch = SketchId("path-sketch".into());
    let mut features = vec![
        model_feature(
            "pattern",
            "pattern-native",
            FeatureDefinition::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::CurveDriven {
                    path: None,
                    spacing: Length(5.0),
                    count: 3,
                },
            },
        ),
        model_feature(
            "path",
            "path-native",
            FeatureDefinition::Sketch { sketch: None },
        ),
        model_feature(
            "seed",
            "seed-native",
            FeatureDefinition::Native {
                kind: "Extrude".into(),
                parameters: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
        ),
    ];

    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    );

    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::CurveDriven { path: None, .. },
            ..
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
    ));
    assert_eq!(features[0].dependencies, [features[2].id.clone()]);
    features[1].definition = FeatureDefinition::Sketch {
        sketch: Some(sketch.clone()),
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&history),
        std::slice::from_ref(&lane),
    );

    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven {
                path: Some(PathRef::Sketch(ref path)),
                ..
            },
            ..
        } if path == &sketch
    ));
    assert_eq!(
        features[0].dependencies,
        [features[2].id.clone(), features[1].id.clone()]
    );
    let FeatureDefinition::Pattern { seeds, .. } = &features[0].definition else {
        panic!("expected pattern");
    };
    assert_eq!(seeds, &[PatternSeed::Feature(features[2].id.clone())]);

    let mut ambiguous_lane = lane.clone();
    ambiguous_lane.names.insert(2, name(450, 20, "PathSketch"));
    if let FeatureDefinition::Pattern {
        pattern: PatternKind::CurveDriven { path, .. },
        seeds,
        ..
    } = &mut features[0].definition
    {
        *path = None;
        seeds.clear();
    }
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&history),
        &[ambiguous_lane],
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::CurveDriven { path: None, .. },
            ..
        }
    ));

    let mut linear_history = history.clone();
    linear_history.features[1].input_class = Some("moLPattern_c".into());
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Linear {
            direction: None,
            spacing: Length(5.0),
            count: 3,
            second: None,
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&linear_history),
        std::slice::from_ref(&lane),
    );
    let FeatureDefinition::Pattern { seeds, .. } = &features[0].definition else {
        panic!("expected pattern");
    };
    assert_eq!(seeds, &[PatternSeed::Feature(features[2].id.clone())]);
    assert_eq!(features[0].dependencies, [features[2].id.clone()]);
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x, y, z }),
                ..
            },
            ..
        } if x == -1.0 && y == 0.0 && z == 0.0
    ));

    let FeatureDefinition::Pattern {
        pattern: PatternKind::Linear { direction, .. },
        ..
    } = &mut features[0].definition
    else {
        panic!("expected linear pattern");
    };
    *direction = None;
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&linear_history),
        std::slice::from_ref(&lane),
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x, y, z }),
                ..
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
            && x == -1.0 && y == 0.0 && z == 0.0
    ));

    let mut derived_history = linear_history.clone();
    derived_history.features[0].input_class = Some("moCosmeticThread_c".into());
    derived_history.features[0].ordinal = 1;
    let mut decoy = native_feature("decoy-native", "6", "Decoy");
    decoy.input_class = Some("moProfileFeature_c".into());
    decoy.ordinal = 2;
    derived_history.features[1].ordinal = 3;
    derived_history.features[2].input_class = Some("moDerivedCosmeticThread_c".into());
    derived_history.features[2].ordinal = 4;
    derived_history.features[3].ordinal = 5;
    derived_history.features.insert(1, decoy);
    let mut derived_lane = lane.clone();
    derived_lane.names = vec![
        name(50, 5, "SeedFeature"),
        name(90, 6, "Decoy"),
        name(100, 10, "Pattern1"),
        name(150, 20, "PathSketch"),
        name(600, 30, "NextFeature"),
    ];
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Linear {
            direction: None,
            spacing: Length(5.0),
            count: 3,
            second: None,
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&derived_history),
        std::slice::from_ref(&derived_lane),
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x, y, z }),
                ..
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
            && x == -1.0 && y == 0.0 && z == 0.0
    ));
    derived_history.features[2].parameters =
        BTreeMap::from([("z".into(), "3".into()), ("e".into(), "19".into())]);
    derived_lane.classes.extend([
        FeatureInputClass {
            id: "count-dimension".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 200,
            name: "moNumberDim_c".into(),
            role: FeatureInputClassRole::Dimension,
        },
        FeatureInputClass {
            id: "spacing-dimension".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 300,
            name: "ParallelPlaneDistanceDim_c".into(),
            role: FeatureInputClassRole::Dimension,
        },
    ]);
    derived_lane
        .names
        .extend([name(220, u32::MAX, "z"), name(420, u32::MAX, "e")]);
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Unresolved {
            form: Some(cadmpeg_ir::features::PatternForm::Linear),
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&derived_history),
        std::slice::from_ref(&derived_lane),
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Linear {
                direction: Some(Vector3 { x, y, z }),
                spacing: Length(19.0),
                count: 3,
                ..
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
            && x == -1.0 && y == 0.0 && z == 0.0
    ));

    let mut mirror_history = history.clone();
    mirror_history.features[1].input_class = Some("moMirrorSolid_c".into());
    mirror_history.features[2].input_class = Some("moDerivedCosmeticThread_c".into());
    let mut mirror_lane = lane.clone();
    mirror_lane.names[2].offset = 150;
    mirror_lane.native_payload.resize(700, 0);
    mirror_lane.native_payload.fill(0);
    mirror_lane.classes.clear();
    let frame = 160;
    for (relative, value) in [
        (0, 0.012_f64),
        (8, -0.025),
        (16, 0.0),
        (24, 0.0),
        (32, 1.0),
        (40, 0.0),
        (49, 1.0),
        (57, 0.0),
        (65, 0.0),
        (73, 0.0),
        (81, 0.0),
        (89, -1.0),
    ] {
        mirror_lane.native_payload[frame + relative..frame + relative + 8]
            .copy_from_slice(&value.to_le_bytes());
    }
    mirror_lane.native_payload[frame + 48] = 1;
    let seed_path = 300;
    mirror_lane.native_payload[seed_path - 12..seed_path - 8].copy_from_slice(&3u32.to_le_bytes());
    mirror_lane.native_payload[seed_path..seed_path + COMPACT_EDGE_VECTOR_MARKER.len()]
        .copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    for (index, source) in [5u32, 40].into_iter().enumerate() {
        let entry = seed_path + 18 + index * 20;
        mirror_lane.native_payload[entry..entry + 2]
            .copy_from_slice(&(0x8001 + index as u16).to_le_bytes());
        mirror_lane.native_payload[entry + 4..entry + 8].copy_from_slice(&[1, 0, 1, 0]);
        mirror_lane.native_payload[entry + 8..entry + 12].copy_from_slice(&source.to_le_bytes());
        mirror_lane.native_payload[entry + 12..entry + 16].copy_from_slice(&9000u32.to_le_bytes());
        mirror_lane.native_payload[entry + 16..entry + 20]
            .copy_from_slice(&(index as u32 + 1).to_le_bytes());
    }
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Unresolved {
            form: Some(cadmpeg_ir::features::PatternForm::Mirror),
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&mirror_history),
        std::slice::from_ref(&mirror_lane),
    );
    assert_eq!(features[0].dependencies, [features[2].id.clone()]);
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Mirror {
                plane_origin: Point3 { x, y, z },
                plane_normal: Vector3 { x: nx, y: ny, z: nz },
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
            && x == 12.0 && y == -25.0 && z == 0.0
            && nx == 0.0 && ny == 1.0 && nz == 0.0
    ));

    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Mirror {
            plane_origin: Point3::new(12.0, -25.0, 0.0),
            plane_normal: Vector3::new(0.0, 1.0, 0.0),
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&mirror_history),
        std::slice::from_ref(&mirror_lane),
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Mirror { .. },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
    ));
    assert_eq!(features[0].dependencies, [features[2].id.clone()]);

    mirror_lane.native_payload[frame..frame + 97].fill(0);
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Pattern {
        seeds: Vec::new(),
        pattern: PatternKind::Unresolved {
            form: Some(cadmpeg_ir::features::PatternForm::Mirror),
        },
    };
    bind_pattern_inputs(
        &mut features,
        std::slice::from_ref(&mirror_history),
        std::slice::from_ref(&mirror_lane),
    );
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Pattern {
            ref seeds,
            pattern: PatternKind::Unresolved {
                form: Some(cadmpeg_ir::features::PatternForm::Mirror),
            },
        } if seeds == &[PatternSeed::Feature(features[2].id.clone())]
    ));
    assert_eq!(features[0].dependencies, [features[2].id.clone()]);

    let mut sweep_history = history;
    sweep_history.features[0].input_class = Some("moProfileFeature_c".into());
    sweep_history.features[1].input_class = Some("moSweep_c".into());
    let path_sketch = SketchId("sweep-path".into());
    features[2].definition = FeatureDefinition::Sketch {
        sketch: Some(path_sketch.clone()),
    };
    features[0].dependencies.clear();
    features[0].definition = FeatureDefinition::Sweep {
        section: cadmpeg_ir::features::SweepSection::Unresolved(None),
        sections: Vec::new(),
        path: Some(PathRef::Native("curve-reference".into())),
        mode: SweepMode::Solid {
            op: cadmpeg_ir::features::BooleanOp::Join,
        },
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    };
    bind_sweep_adjacent_profiles(&mut features, &[sweep_history], std::slice::from_ref(&lane));
    assert!(matches!(
        features[0].definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(
                cadmpeg_ir::features::ProfileRef::Sketch(ref profile),
            ),
            path: Some(PathRef::Sketch(ref path)),
            ..
        } if profile == &sketch && path == &path_sketch
    ));
    assert_eq!(
        features[0].dependencies,
        [features[1].id.clone(), features[2].id.clone()]
    );
}

#[test]
fn compact_line_reference_scalar_counts_follow_their_trailers() {
    const HANDLES: [u8; 8] = [0xc7, 0xcf, 0xff, 0xff, 0xc7, 0xcf, 0xff, 0xff];
    let write_scalars = |payload: &mut [u8], start: usize, values: &[f64]| {
        for (index, value) in values.iter().enumerate() {
            let offset = start + index * 8;
            payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
    };

    let mut shifted_nine = vec![0; 136];
    shifted_nine[..8].copy_from_slice(&HANDLES);
    shifted_nine[12..16].copy_from_slice(&8000u32.to_le_bytes());
    write_scalars(
        &mut shifted_nine,
        24,
        &[0.13, 0.01, -0.02, 0.05, 0.0, 0.0, -1.0, 0.0, 0.0],
    );
    shifted_nine[96..104].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    shifted_nine[116..118].copy_from_slice(&0x81a5u16.to_le_bytes());
    shifted_nine[134..136].fill(0xff);
    assert_eq!(
        compact_line_reference_direction(&shifted_nine, 0, shifted_nine.len(), &[]),
        Some(Vector3::new(-1.0, 0.0, 0.0))
    );

    let mut shifted_seven = vec![0; 136];
    shifted_seven[..8].copy_from_slice(&HANDLES);
    shifted_seven[12..16].copy_from_slice(&8000u32.to_le_bytes());
    write_scalars(
        &mut shifted_seven,
        24,
        &[0.01, 0.005, 0.022, 0.031, 1.0, 0.0, 0.0],
    );
    shifted_seven[116..118].copy_from_slice(&0x85deu16.to_le_bytes());
    shifted_seven[134..136].fill(0xff);
    assert_eq!(
        compact_line_reference_direction(&shifted_seven, 0, shifted_seven.len(), &[]),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    shifted_seven[80..136].fill(0);
    shifted_seven[80..88].copy_from_slice(&[120, 0, 0, 0, 10, 0, 0, 0]);
    assert_eq!(
        compact_line_reference_direction(&shifted_seven, 0, shifted_seven.len(), &[]),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );

    let mut unshifted_seven = vec![0; 96];
    unshifted_seven[..8].copy_from_slice(&HANDLES);
    unshifted_seven[12..16].copy_from_slice(&9000u32.to_le_bytes());
    write_scalars(
        &mut unshifted_seven,
        32,
        &[0.06, 0.03, 0.076, -0.03, 0.0, 0.0, 1.0],
    );
    unshifted_seven[88..96].fill(1);
    assert_eq!(
        compact_line_reference_direction(&unshifted_seven, 0, unshifted_seven.len(), &[]),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );

    let mut addressless = vec![0; 84];
    addressless[..8].copy_from_slice(&HANDLES);
    write_scalars(
        &mut addressless,
        24,
        &[0.04, 0.01, 0.0, 0.0, 0.0, 0.0, -1.0],
    );
    assert_eq!(
        compact_line_reference_direction(&addressless, 0, addressless.len(), &[]),
        Some(Vector3::new(0.0, 0.0, -1.0))
    );
    addressless.truncate(80);
    addressless.extend([0, 0, 0, 0, 0xd8, 0x81]);
    assert_eq!(
        compact_line_reference_direction(&addressless, 0, addressless.len(), &[]),
        Some(Vector3::new(0.0, 0.0, -1.0))
    );

    let mut addressless_unshifted = vec![0; 136];
    addressless_unshifted[..8].copy_from_slice(&HANDLES);
    write_scalars(
        &mut addressless_unshifted,
        32,
        &[0.065, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    );
    addressless_unshifted[104..112].copy_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(
        compact_line_reference_direction(
            &addressless_unshifted,
            0,
            addressless_unshifted.len(),
            &[],
        ),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
}

#[test]
fn e1_line_distance_indices_address_coordinate_point_pairs() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
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
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let coordinates = [
        [0.002, -0.007],
        [0.018, -0.007],
        [0.002, -0.002],
        [0.002, -0.018],
        [0.018, -0.018],
        [0.018, -0.002],
        [0.002, -0.013],
        [0.018, -0.013],
        [0.002, -0.018],
        [0.018, -0.018],
        [0.018, -0.002],
        [0.002, -0.002],
    ];
    let markers = coordinates
        .into_iter()
        .enumerate()
        .map(|(index, coordinates)| {
            let mut point = marker(&format!("point-{index}"), Some(coordinates));
            point.offset = index as u64;
            point
        })
        .collect::<Vec<_>>();
    let mut entities = markers
        .iter()
        .take(3)
        .map(|marker| {
            let [u, v] = marker.coordinates_m.unwrap();
            SketchEntity::new(
                SketchEntityId(format!("bound-{}", marker.id)),
                sketch.clone(),
                SketchGeometry::Point {
                    position: Point2::new(u * 1000.0, v * 1000.0),
                },
            )
            .with_native_ref(Some(marker.id.clone()))
        })
        .collect::<Vec<_>>();
    let operand = |offset: u64, index: u16| FeatureInputOperand {
        offset,
        reference_ref: format!("reference-{offset}"),
        kind: FeatureInputOperandKind::E1,
        entity_index: index,
        entity_ref: None,
    };
    let relation = |id: &str, offset: u64, first: u16, second: u16, scalar: &str| {
        FeatureInputRelationInstance {
            id: id.into(),
            parent: "lane".into(),
            ordinal: offset as u32,
            offset,
            family: FeatureInputRelationFamily::LineLineDistance,
            class_ref: "class".into(),
            feature_ref: "feature-native".into(),
            scalar_refs: vec![scalar.into()],
            parameter_scalar_ref: Some(scalar.into()),
            display_scalar_ref: None,
            operands: vec![operand(offset + 1, first), operand(offset + 2, second)],
        }
    };
    let lane = FeatureInputLane {
        id: "lane#test".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![
            relation("lower-distance", 100, 4, 3, "lower-scalar"),
            relation("upper-distance", 200, 5, 0, "upper-scalar"),
        ],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    };
    let parameter = |id: &str, scalar: &str| DesignParameter {
        id: ParameterId(id.into()),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: id.into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(scalar.into()),
    };
    let parameters = vec![
        parameter("lower", "lower-scalar"),
        parameter("upper", "upper-scalar"),
    ];
    let mut constraints = Vec::new();
    project_relation_bindings(
        &mut constraints,
        &[],
        std::slice::from_ref(&feature),
        &entities,
        &parameters,
        std::slice::from_ref(&lane),
    );
    assert_eq!(constraints.len(), 2);
    assert!(constraints.iter().all(|constraint| matches!(
        &constraint.definition,
        SketchConstraintDefinition::Native { .. }
    )));

    project_relation_solved_line_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        &parameters,
        std::slice::from_ref(&lane),
    );

    let solver_lines = entities
        .iter()
        .filter(|entity| entity.id().0.contains("#solver-line:"))
        .collect::<Vec<_>>();
    assert_eq!(solver_lines.len(), 4);
    assert_eq!(
        solver_lines
            .iter()
            .filter_map(|entity| entity.geometry_ref.as_deref())
            .collect::<HashSet<_>>(),
        [
            "feature-native:solver-line:0",
            "feature-native:solver-line:3",
            "feature-native:solver-line:4",
            "feature-native:solver-line:5",
        ]
        .into_iter()
        .collect()
    );
    project_relation_bindings(
        &mut constraints,
        &[],
        std::slice::from_ref(&feature),
        &entities,
        &parameters,
        std::slice::from_ref(&lane),
    );
    assert_eq!(constraints.len(), 2);
    assert!(constraints.iter().all(|constraint| matches!(
        &constraint.definition,
        SketchConstraintDefinition::Distance { entities, .. } if entities.len() == 2
    )));
    project_relation_bindings(
        &mut constraints,
        &[],
        std::slice::from_ref(&feature),
        &entities,
        &parameters,
        std::slice::from_ref(&lane),
    );
    assert_eq!(constraints.len(), 2);
}

#[test]
fn roster_point_line_distance_materializes_one_solver_line() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
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
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let coordinates = [
        [0.006, -0.005],
        [0.036, -0.082],
        [0.036, -0.005],
        [0.0, -0.005],
        [0.036, -0.005],
        [0.036, -0.15],
        [0.036, -0.162],
        [-0.014, -0.162],
    ];
    let markers = coordinates
        .into_iter()
        .enumerate()
        .map(|(index, coordinates_m)| {
            let mut marker = marker(&format!("point-{index}"), Some(coordinates_m));
            marker.offset = index as u64;
            marker
        })
        .collect::<Vec<_>>();
    let mut entities = coordinates
        .into_iter()
        .take(3)
        .enumerate()
        .map(|(index, [u, v])| {
            SketchEntity::new(
                SketchEntityId(format!("bound-point-{index}")),
                sketch.clone(),
                SketchGeometry::Point {
                    position: Point2::new(u * 1000.0, v * 1000.0),
                },
            )
            .with_construction(true)
            .with_native_ref(Some(format!("point-{index}")))
        })
        .collect::<Vec<_>>();
    let relation = FeatureInputRelationInstance {
        id: "relation-point-line".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 100,
        family: FeatureInputRelationFamily::PointLineDistance,
        class_ref: "class".into(),
        feature_ref: "feature-native".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: vec![
            FeatureInputOperand {
                offset: 101,
                reference_ref: "point-reference".into(),
                kind: FeatureInputOperandKind::Native(0x81dd),
                entity_index: 0,
                entity_ref: Some("point-0".into()),
            },
            FeatureInputOperand {
                offset: 102,
                reference_ref: "line-reference".into(),
                kind: FeatureInputOperandKind::Native(0x81e7),
                entity_index: 2,
                entity_ref: None,
            },
        ],
    };
    let lane = FeatureInputLane {
        id: "lane#test".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![relation],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    };
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "30mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(30.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };
    project_relation_solved_line_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&lane),
    );
    let solver_line = entities
        .iter()
        .find(|entity| entity.geometry_ref.as_deref() == Some("feature-native:solver-line:2"))
        .expect("point-line solver line");
    assert!(matches!(
        solver_line.geometry,
        SketchGeometry::Line { start, end }
            if start == Point2::new(36.0, -5.0) && end == Point2::new(36.0, -150.0)
    ));
    let solver_line_id = solver_line.id().clone();

    let mut constraints = Vec::new();
    project_relation_bindings(
        &mut constraints,
        &[],
        std::slice::from_ref(&feature),
        &entities,
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&lane),
    );
    let [constraint] = constraints.as_slice() else {
        panic!("one point-line constraint");
    };
    let SketchConstraintDefinition::DistanceLoci {
        first,
        second,
        parameter: parameter_ref,
    } = &constraint.definition
    else {
        panic!("typed point-line constraint");
    };
    assert_eq!(
        first,
        &SketchLocus::Entity(SketchEntityId("bound-point-0".into()))
    );
    assert_eq!(second, &SketchLocus::Entity(solver_line_id));
    assert_eq!(parameter_ref, &parameter.id);
}

#[test]
fn point_line_projection_uses_the_resolved_point_when_marker_frames_are_ambiguous() {
    let sketch_id = SketchId("sketch".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let feature = Feature {
        id: FeatureId("feature".into()),
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
            sketch: Some(sketch_id.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let line = |id: &str, start: [f64; 2], end: [f64; 2]| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch_id.clone(),
            SketchGeometry::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            },
        )
    };
    let mut entities = vec![
        line("profile-bottom", [0.0, -20.0], [70.0, -20.0]),
        line("profile-top", [0.0, 0.0], [70.0, 0.0]),
        line("profile-left", [0.0, -20.0], [0.0, 0.0]),
        line("profile-right", [70.0, 0.0], [70.0, -20.0]),
        SketchEntity::new(
            SketchEntityId("resolved-point".into()),
            sketch_id.clone(),
            SketchGeometry::Point {
                position: Point2::new(85.0, -10.0),
            },
        )
        .with_construction(true)
        .with_native_ref(Some("point-4".into())),
    ];
    let coordinates_m = [
        [0.0, 0.0],
        [0.07, 0.0],
        [0.0, -0.02],
        [0.0, 0.0],
        [0.015, -0.01],
    ];
    let markers = coordinates_m
        .into_iter()
        .enumerate()
        .map(|(index, coordinates_m)| {
            let mut marker = marker(&format!("point-{index}"), Some(coordinates_m));
            marker.offset = index as u64;
            marker
        })
        .collect::<Vec<_>>();
    let relation = FeatureInputRelationInstance {
        id: "relation-point-line-ambiguous".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 100,
        family: FeatureInputRelationFamily::PointLineDistance,
        class_ref: "class".into(),
        feature_ref: "feature-native".into(),
        scalar_refs: vec!["scalar".into()],
        parameter_scalar_ref: Some("scalar".into()),
        display_scalar_ref: None,
        operands: vec![
            FeatureInputOperand {
                offset: 101,
                reference_ref: "point-reference".into(),
                kind: FeatureInputOperandKind::Native(0x81dd),
                entity_index: 4,
                entity_ref: Some("point-4".into()),
            },
            FeatureInputOperand {
                offset: 102,
                reference_ref: "line-reference".into(),
                kind: FeatureInputOperandKind::Native(0x81e7),
                entity_index: 1,
                entity_ref: None,
            },
        ],
    };
    let lane = FeatureInputLane {
        id: "lane#test".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![relation],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers,
    };
    let parameter = DesignParameter {
        id: ParameterId("parameter".into()),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "15mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(15.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some("scalar".into()),
    };
    let transforms =
        crate::resolved_features::relation_loci::marker_transform_candidates_by_feature(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&sketch),
            &entities,
            std::slice::from_ref(&lane),
        );
    assert!(transforms["feature-native"].len() > 1);
    project_relation_solved_line_geometry(
        &mut entities,
        std::slice::from_ref(&sketch),
        std::slice::from_ref(&feature),
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&lane),
    );
    let solver_line = entities
        .iter()
        .find(|entity| entity.geometry_ref.as_deref() == Some("feature-native:solver-line:1"))
        .expect("resolved point selects one solver line");
    assert!(matches!(
        solver_line.geometry,
        SketchGeometry::Line { start, end }
            if start == Point2::new(70.0, -20.0) && end == Point2::new(70.0, 0.0)
    ));
    let mut constraints = Vec::new();
    project_relation_bindings(
        &mut constraints,
        std::slice::from_ref(&sketch),
        std::slice::from_ref(&feature),
        &entities,
        std::slice::from_ref(&parameter),
        std::slice::from_ref(&lane),
    );
    let [constraint] = constraints.as_slice() else {
        panic!("one point-line constraint");
    };
    assert!(matches!(
        &constraint.definition,
        SketchConstraintDefinition::DistanceLoci { first, second, .. }
            if first == &SketchLocus::Entity(SketchEntityId("resolved-point".into()))
                && second == &SketchLocus::Entity(solver_line.id().clone())
    ));
}

#[test]
fn reused_point_handle_gets_one_solved_locus_per_dimension_relation() {
    let sketch = SketchId("sketch".into());
    let feature = Feature {
        id: FeatureId("feature".into()),
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
            sketch: Some(sketch.clone()),
        },
        native_ref: Some("feature-native".into()),
    };
    let point = |id: &str, marker: Option<&str>, u: f64| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, 0.0),
            },
        )
        .with_native_ref(marker.map(str::to_owned))
    };
    let mut entities = vec![
        point("origin", Some("known-a"), 0.0),
        point("middle", Some("known-b"), 5.0),
        point("far", None, 12.0),
    ];
    let known_a = marker("known-a", Some([0.0, 0.0]));
    let known_b = marker("known-b", Some([0.005, 0.0]));
    let missing = marker("missing", None);
    let operand = |index: usize, marker: &str| FeatureInputOperand {
        offset: index as u64,
        reference_ref: format!("reference-{index}"),
        kind: FeatureInputOperandKind::D6,
        entity_index: index as u16,
        entity_ref: Some(marker.into()),
    };
    let relation =
        |id: &str, offset: u64, family: FeatureInputRelationFamily, known: &str, scalar: &str| {
            FeatureInputRelationInstance {
                id: id.into(),
                parent: "lane".into(),
                ordinal: 0,
                offset,
                family,
                class_ref: "class".into(),
                feature_ref: "feature-native".into(),
                scalar_refs: vec![scalar.into()],
                parameter_scalar_ref: Some(scalar.into()),
                display_scalar_ref: None,
                operands: vec![operand(0, known), operand(1, "missing")],
            }
        };
    let relations = vec![
        relation(
            "relation-a",
            10,
            FeatureInputRelationFamily::PointPointDistance,
            "known-a",
            "scalar-a",
        ),
        relation(
            "relation-b",
            20,
            FeatureInputRelationFamily::PointPointDistance,
            "known-b",
            "scalar-b",
        ),
        relation(
            "relation-c",
            30,
            FeatureInputRelationFamily::PointPointHorizontalDistance,
            "known-b",
            "scalar-c",
        ),
    ];
    let lane = FeatureInputLane {
        id: "lane#test".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: relations.clone(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: vec![known_a, known_b, missing],
    };
    let parameter = |id: &str, scalar: &str, distance: f64| DesignParameter {
        id: ParameterId(id.into()),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: id.into(),
        expression: format!("{distance}mm"),
        display: None,
        value: Some(ParameterValue::Length(Length(distance))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(scalar.into()),
    };
    let parameters = vec![
        parameter("distance-a", "scalar-a", 5.0),
        parameter("distance-b", "scalar-b", 7.0),
        parameter("distance-c", "scalar-c", 7.0),
    ];

    project_relation_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        std::slice::from_ref(&lane),
    );
    project_relation_solved_point_geometry(
        &mut entities,
        &[],
        std::slice::from_ref(&feature),
        &parameters,
        std::slice::from_ref(&lane),
    );

    let solved = entities
        .iter()
        .filter(|entity| entity.id().0.contains("dimension-point:"))
        .collect::<Vec<_>>();
    assert_eq!(solved.len(), 3);
    assert!(matches!(
        solved[0].geometry,
        SketchGeometry::Point { position } if position == Point2::new(5.0, 0.0)
    ));
    assert!(matches!(
        solved[1].geometry,
        SketchGeometry::Point { position } if position == Point2::new(12.0, 0.0)
    ));
    assert!(matches!(
        solved[2].geometry,
        SketchGeometry::Point { position } if position == Point2::new(12.0, 0.0)
    ));
    assert_ne!(solved[0].geometry_ref, solved[1].geometry_ref);
    assert_ne!(solved[1].geometry_ref, solved[2].geometry_ref);

    let markers = lane
        .sketch_entities
        .iter()
        .map(|marker| (marker.id.as_str(), marker))
        .collect::<HashMap<_, _>>();
    let loci = profile_loci_by_marker(
        std::slice::from_ref(&feature),
        &[],
        &entities,
        std::slice::from_ref(&lane),
    );
    for (index, relation) in relations.iter().enumerate() {
        let definition = typed_relation_definition(
            relation,
            Some(&parameters[index]),
            &sketch,
            &entities,
            &markers,
            &loci,
        );
        let second = match definition {
            Some(
                SketchConstraintDefinition::DistanceLoci { second, .. }
                | SketchConstraintDefinition::HorizontalDistance { second, .. },
            ) => second,
            other => panic!("unexpected relation definition: {other:?}"),
        };
        assert_eq!(second, SketchLocus::Entity(solved[index].id().clone()));
    }
}
