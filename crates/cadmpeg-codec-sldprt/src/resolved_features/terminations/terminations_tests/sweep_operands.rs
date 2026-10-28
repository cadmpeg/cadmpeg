//! Owned sweep operand roles.

use super::super::{
    enrich_history_sweep_profiles, project_sweep_reference_paths, COMPACT_EDGE_VECTOR_MARKER,
};
use super::{Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId};
use crate::resolved_features::bindings::bind_sweep_profile_inputs;
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, PathRef, PlanarProfileRef,
    SketchFeatureBinding, SolidSweepOperation, SweepMode, SweepSection, SweepShape,
};
use cadmpeg_ir::sketches::SketchId;
use std::collections::BTreeMap;

fn native(id: &str, source: u32, class: Option<&str>) -> Feature {
    Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal: source,
        name: id.into(),
        kind: "Feature".into(),
        input_class: class.map(Into::into),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn model(id: &str, ordinal: u64, definition: FeatureDefinition) -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: FeatureId::mint(format!("test:model:feature#{id}")).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        native_ref: Some(id.into()),
        evaluation: FeatureEvaluation::from_definition(definition),
    }
}

fn class(payload: &mut [u8], offset: usize, name: &str) -> usize {
    payload[offset..offset + 4].copy_from_slice(&[0xff, 0xff, 1, 0]);
    payload[offset + 4..offset + 6]
        .copy_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
    payload[offset + 6..offset + 6 + name.len()].copy_from_slice(name.as_bytes());
    offset + 6 + name.len()
}

fn reference(payload: &mut [u8], prefix: usize, source: u32) {
    payload[prefix..prefix + 10].copy_from_slice(&[0x2b, 0x80, 2, 0, 0, 0, 0, 0, 0, 0]);
    payload[prefix + 45..prefix + 61].fill(0xff);
    let at = prefix + 81;
    payload[at..at + 4].copy_from_slice(&source.to_le_bytes());
    payload[at + 4..at + 8].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    payload[at + 16..at + 20].copy_from_slice(&0x65u32.to_le_bytes());
    payload[at + 24..at + 28].fill(0xff);
    for offset in [32, 36, 40] {
        payload[at + offset..at + offset + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    payload[at + 48..at + 52].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
}

struct Input {
    history: FeatureHistory,
    lane: FeatureInputLane,
    features: Vec<cadmpeg_ir::features::Feature>,
    direct_prefix: usize,
}

fn input(solid: bool, direct: bool, generated: bool) -> Input {
    let mut payload = vec![0; 1600];
    let general = class(&mut payload, 100, "moGeneralCurveRef_w");
    let section = class(&mut payload, general, "moCompProfile_c");
    reference(&mut payload, section, 19);
    let declaration = if direct { 600 } else { 16 };
    payload[declaration - 2..declaration].copy_from_slice(&0x8a9bu16.to_le_bytes());
    let direct_prefix = class(&mut payload, declaration, "moCompReferenceCurve_c");
    if direct {
        reference(&mut payload, direct_prefix, 11);
    }
    if generated {
        let wrapper = 1000;
        payload[wrapper..wrapper + 2].copy_from_slice(&0x8a9bu16.to_le_bytes());
        payload[wrapper + 4..wrapper + 9].copy_from_slice(&[0x2b, 0x80, 2, 0, 0]);
        let marker = wrapper + 48;
        payload[marker - 12..marker - 8].copy_from_slice(&5u32.to_le_bytes());
        payload[marker - 8..marker - 4].copy_from_slice(&[4, 2, 0, 0]);
        payload[marker..marker + 16].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
        for index in 0..4 {
            let at = marker + 18 + index * 20;
            payload[at..at + 2]
                .copy_from_slice(&(0x100 + u16::try_from(index).unwrap()).to_le_bytes());
            payload[at + 4..at + 8].copy_from_slice(&0x4050_6070u32.to_le_bytes());
            payload[at + 8..at + 12].copy_from_slice(&11u32.to_le_bytes());
            payload[at + 12..at + 16].copy_from_slice(&0x1122_3344u32.to_le_bytes());
            payload[at + 16..at + 20].copy_from_slice(&7u32.to_le_bytes());
        }
        let end = marker + 18 + 80;
        payload[end + 8..end + 12].copy_from_slice(&[0xf8, 0x2a, 0, 0]);
    }
    let class_record = |id: &str, offset, name: &str| crate::records::FeatureInputClass {
        id: id.into(),
        parent: "lane#7".into(),
        ordinal: 0,
        offset,
        name: name.into(),
    };
    let name = |id: &str, source, offset| FeatureInputName {
        id: id.into(),
        parent: "lane#7".into(),
        ordinal: 0,
        offset,
        object_id: ObjectId::from_value(source),
        value: id.into(),
    };
    let lane = FeatureInputLane {
        id: "lane#7".into(),
        configuration: None,
        native_payload: payload.into(),
        classes: vec![
            class_record("general", 100, "moGeneralCurveRef_w"),
            class_record(
                "component",
                u64::try_from(declaration).unwrap(),
                "moCompReferenceCurve_c",
            ),
        ],
        names: vec![
            name("prior", 11, 0),
            name("section", 19, 32),
            name("sweep", 31, 64),
            name("next", 99, 1500),
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
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native("prior", 11, None),
            native("section", 19, Some("moProfileFeature_c")),
            native(
                "sweep",
                31,
                Some(if solid {
                    "moSweep_c"
                } else {
                    "moSweepRefSurface_c"
                }),
            ),
            native("next", 99, None),
        ],
    };
    let features = vec![
        model(
            "prior",
            0,
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(None),
            }),
        ),
        model(
            "section",
            1,
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(
                    SketchId::mint("test:model:sketch#section").unwrap(),
                )),
            }),
        ),
        model(
            "sweep",
            2,
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape: SweepShape::sheet_sections(
                    if solid {
                        SweepMode::Solid {
                            op: SolidSweepOperation::Join,
                        }
                    } else {
                        SweepMode::Surface {}
                    },
                    SweepSection::Unresolved(None),
                    Vec::new(),
                ),
                path: None,
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
            }),
        ),
    ];
    Input {
        history,
        lane,
        features,
        direct_prefix,
    }
}

fn apply(input: &mut Input) {
    enrich_history_sweep_profiles(
        std::slice::from_mut(&mut input.history),
        std::slice::from_ref(&input.lane),
    );
    let profile = input.history.features[2].properties.get("Profile").cloned();
    if let Some(profile) = profile {
        input.features[2].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            {
                shape.set_referenced_profile(PlanarProfileRef::Native(profile));
            }
        });
    }
    project_sweep_reference_paths(
        &mut input.features,
        std::slice::from_ref(&input.history),
        std::slice::from_ref(&input.lane),
    );
    bind_sweep_profile_inputs(&mut input.features);
}

#[test]
fn explicit_section_and_path_have_independent_roles() {
    let mut input = input(false, true, false);
    let raw = input.lane.native_payload.clone();
    apply(&mut input);
    let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path, .. }) =
        input.features[2].evaluation.definition()
    else {
        panic!("sweep");
    };
    assert!(
        matches!(shape.referenced_profile(), Some(PlanarProfileRef::Sketch(id)) if id.as_str() == "test:model:sketch#section")
    );
    assert!(
        matches!(path, Some(PathRef::Native(id)) if id == "sldprt:feature-input:component-reference-curve:7:600")
    );
    assert_eq!(input.features[2].dependencies.len(), 2);
    assert_eq!(input.lane.native_payload, raw);
    assert!(!input.history.features[2].properties.contains_key("Path"));
}

#[test]
fn generated_path_keeps_distinct_instances_in_native_bytes() {
    let mut input = input(false, false, true);
    let raw = input.lane.native_payload.clone();
    apply(&mut input);
    let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path, .. }) =
        input.features[2].evaluation.definition()
    else {
        panic!("sweep");
    };
    assert!(matches!(
        shape.referenced_profile(),
        Some(PlanarProfileRef::Sketch(_))
    ));
    assert!(
        matches!(path, Some(PathRef::Native(id)) if id == "sldprt:feature-input:component-reference-curve:7:1000")
    );
    assert_eq!(input.lane.native_payload, raw);
    for index in 0..4 {
        let at = 1048 + 18 + index * 20;
        assert_eq!(
            &input.lane.native_payload[at..at + 2],
            &(0x100 + u16::try_from(index).unwrap()).to_le_bytes()
        );
    }
}

#[test]
fn direct_and_generated_conflict_does_not_choose_a_path() {
    let mut input = input(false, true, true);
    apply(&mut input);
    assert!(matches!(
        input.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep { path: None, .. })
    ));
    assert!(!input.features[2]
        .dependencies
        .contains(&input.features[0].id));
}

#[test]
fn unsupported_solid_child_retains_identity_without_producer_binding() {
    for class in ["moSweep_c", "moSweepCut_c"] {
        let mut input = input(true, true, false);
        input.history.features[2].input_class = Some(class.into());
        if class == "moSweepCut_c" {
            input.features[2].evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) =
                    definition
                {
                    *shape = SweepShape::sheet_sections(
                        SweepMode::Solid {
                            op: SolidSweepOperation::Cut,
                        },
                        SweepSection::Unresolved(None),
                        Vec::new(),
                    );
                }
            });
        }
        let prefix = input.direct_prefix;
        input.lane.native_payload[prefix..prefix + 2].copy_from_slice(&0x0d9bu16.to_le_bytes());
        apply(&mut input);
        assert!(
            matches!(input.features[2].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { path: Some(PathRef::Native(id)), .. }) if id == "sldprt:feature-input:component-reference-curve:7:600")
        );
        assert!(!input.features[2]
            .dependencies
            .contains(&input.features[0].id));
    }
}

#[test]
fn self_and_future_sources_do_not_bind_a_producer() {
    for source in [31u32, 41] {
        let mut input = input(false, true, false);
        input.history.features.push(native("future", 41, None));
        let at = input.direct_prefix + 81;
        input.lane.native_payload[at..at + 4].copy_from_slice(&source.to_le_bytes());
        apply(&mut input);
        assert!(!input.features[2]
            .dependencies
            .contains(&input.features[0].id));
        assert!(!input.features[2]
            .dependencies
            .contains(&input.features[2].id));
        assert!(
            matches!(input.features[2].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { path: Some(PathRef::Native(id)), .. }) if id == "sldprt:feature-input:component-reference-curve:7:600")
        );
    }
}

#[test]
fn configuration_native_paths_do_not_become_a_global_choice() {
    let mut input = input(false, false, true);
    let mut other = input.lane.clone();
    other.id = "lane#8".into();
    for name in &mut other.names {
        name.parent = other.id.clone();
    }
    for class in &mut other.classes {
        class.parent = other.id.clone();
    }
    let mut missing = other.clone();
    missing.names.retain(|name| name.value != "sweep");
    let mut history = input.history.clone();
    enrich_history_sweep_profiles(
        std::slice::from_mut(&mut history),
        &[input.lane.clone(), missing],
    );
    assert!(!history.features[2].properties.contains_key("Profile"));
    project_sweep_reference_paths(
        &mut input.features,
        std::slice::from_ref(&input.history),
        &[input.lane.clone(), other],
    );
    assert!(matches!(
        input.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep { path: None, .. })
    ));
    project_sweep_reference_paths(
        &mut input.features,
        std::slice::from_ref(&input.history),
        std::slice::from_ref(&input.lane),
    );
    assert!(matches!(
        input.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sweep {
            path: Some(PathRef::Native(_)),
            ..
        })
    ));
}

#[test]
fn duplicate_producer_identity_retains_only_the_record() {
    let mut input = input(false, true, false);
    input.history.features.push(native("duplicate", 11, None));
    apply(&mut input);
    assert!(!input.features[2]
        .dependencies
        .contains(&input.features[0].id));
    assert!(
        matches!(input.features[2].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { path: Some(PathRef::Native(id)), .. }) if id == "sldprt:feature-input:component-reference-curve:7:600")
    );
}

#[test]
fn duplicate_general_records_do_not_select_a_neighboring_section() {
    let mut input = input(false, true, false);
    let parent = class(&mut input.lane.native_payload, 350, "moGeneralCurveRef_w");
    let prefix = class(&mut input.lane.native_payload, parent, "moCompProfile_c");
    reference(&mut input.lane.native_payload, prefix, 19);
    let mut extra = input.lane.classes[0].clone();
    extra.id = "second-general".into();
    extra.offset = 350;
    input.lane.classes.push(extra);
    apply(&mut input);
    assert!(
        matches!(input.features[2].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) if shape.section_is_unresolved())
    );
    assert!(!input.features[2]
        .dependencies
        .contains(&input.features[1].id));
}

#[test]
fn a_reference_cannot_read_its_suffix_from_the_next_owner() {
    let mut input = input(false, true, false);
    input.lane.names[3].offset = u64::try_from(input.direct_prefix + 90).unwrap();
    apply(&mut input);
    assert!(!input.features[2]
        .dependencies
        .contains(&input.features[0].id));
    assert!(
        matches!(input.features[2].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { path: Some(PathRef::Native(id)), .. }) if id == "sldprt:feature-input:component-reference-curve:7:600")
    );
}

#[test]
fn authored_operands_survive_unrelated_represented_sketches() {
    let mut input = input(false, true, false);
    input.history.features[2].properties.insert(
        cadmpeg_core::nonblank_literal!("Profile"),
        "authored-section".into(),
    );
    input.features[2].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Sweep { path, .. }) = definition {
            *path = Some(PathRef::Native("authored-path".into()));
        }
    });
    apply(&mut input);
    assert!(
        matches!(input.features[2].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { shape, path: Some(PathRef::Native(path)), .. })
        if matches!(shape.referenced_profile(), Some(PlanarProfileRef::Native(section)) if section == "authored-section") && path == "authored-path")
    );
    assert!(input.features[2].dependencies.is_empty());
}
