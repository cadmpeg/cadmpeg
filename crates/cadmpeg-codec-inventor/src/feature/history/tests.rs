// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};

use super::{build, HistoryNodeKind};
use crate::design::scope::{BrepSegmentName, BrepSegmentRole, PmDcModelRoot, PmDcModelRootPayload};
use crate::design::DesignInventory;
use crate::feature::tests::{reference, reference_list, segment, test_feature, test_header};
use crate::feature::{
    ClassId, FeatureInventory, PmDcFeatureLabel, PmDcFeatureLabelPayload,
    PmDcFeatureLabelPayloadWire, PmDcFeatureTerminator, PmDcFeatureTerminatorPayload,
    PmDcLinkedHeader, FEATURE_LABEL_TYPE, FEATURE_TYPE,
};
use crate::record_identity::{Located, RecordTypeId};

const EXTRUSION: ClassId = crate::feature::EXTRUSION_CLASS_ID;

fn token() -> cadmpeg_ir::ids::IdentityKey {
    segment()
}

/// A model root naming the B-rep segment `segment_name`.
fn model_root(ordinal: u32, segment_name: &str) -> PmDcModelRoot {
    Located::new(
        PmDcModelRootPayload {
            save_version_major: 19,
            header_value: 0,
            header_id: 0,
            next: reference(0),
            flags: 0x40,
            flags_extension: None,
            identifier: "0".repeat(32),
            name: String::new(),
            members: reference_list(&[]),
            values: [0; 15],
            parameter_table: reference(0),
            brep_segment: segment_name.to_owned(),
            scale: cadmpeg_ir::scalar::FiniteReal::ZERO,
            terminal_value: 0,
        },
        RecordTypeId::from_bytes(crate::design::scope::MODEL_ROOT_TYPE),
        token(),
        ordinal,
    )
}

/// The document and flat-pattern B-rep segments and their model roots.
fn design() -> DesignInventory {
    DesignInventory {
        parameters: Vec::new(),
        expressions: Vec::new(),
        units: Vec::new(),
        parameter_tables: Vec::new(),
        model_roots: vec![
            model_root(20, "MbBRepSegment"),
            model_root(0, "PmBRepSegment"),
        ],
        brep_segments: vec![
            BrepSegmentName {
                display_name: "PmBRepSegment".into(),
                role: BrepSegmentRole::Document,
            },
            BrepSegmentName {
                display_name: "MbBRepSegment".into(),
                role: BrepSegmentRole::FlatPattern,
            },
        ],
        issues: Vec::new(),
    }
}

fn label(
    record: u32,
    owner: u32,
    name: &str,
    class_id: ClassId,
    parts: &[u32],
) -> PmDcFeatureLabel {
    let one_based = parts.iter().map(|ordinal| ordinal + 1).collect::<Vec<_>>();
    Located::new(
        PmDcFeatureLabelPayload::try_from(PmDcFeatureLabelPayloadWire {
            save_version_major: 19,
            header: PmDcLinkedHeader {
                header_value: 0,
                header_id: 0,
                values: [0; 2],
                flags_extension: None,
                owner: reference(owner + 1),
                parent: reference(0),
                next: reference(0),
            },
            index: record,
            participants: reference_list(&one_based),
            name: name.to_owned(),
            name_units: None,
            class_id: class_id.into(),
        })
        .expect("valid label fixture"),
        RecordTypeId::from_bytes(FEATURE_LABEL_TYPE),
        token(),
        record,
    )
}

fn terminator(ordinal: u32) -> PmDcFeatureTerminator {
    Located::new(
        PmDcFeatureTerminatorPayload {
            save_version_major: 19,
            header: test_header(),
            state: -1,
        },
        RecordTypeId::from_bytes(crate::feature::END_OF_FEATURES_TYPE),
        token(),
        ordinal,
    )
}

fn feature(ordinal: u32, flags: u32) -> crate::feature::PmDcFeature {
    let feature = test_feature(ordinal, 0, &[]);
    let mut payload = (*feature).clone();
    payload.header.flags = flags;
    Located::new(
        payload,
        RecordTypeId::from_bytes(FEATURE_TYPE),
        token(),
        ordinal,
    )
}

fn sketch(ordinal: u32) -> crate::sketch::PmDcSketch {
    Located::new(
        crate::sketch::PmDcSketchPayload {
            save_version_major: 19,
            header: test_header(),
            state: 0,
            count_value: 0,
            entities: crate::pmdc::PmDcReferenceList::new(8, None, Vec::new())
                .expect("empty entity list"),
            transform: reference(0),
            direction: reference(0),
            values: [0; 2],
            auxiliary: None,
        },
        RecordTypeId::try_from("114d8790d011f8d10008cabc0663dc09".to_owned()).expect("sketch type"),
        token(),
        ordinal,
    )
}

/// Model tree: folder 1, Extrusion 2 consuming sketch 7, wrapper 3 holding
/// sketch 10 and feature 11, suppressed feature 4, end-of-part 5, feature 6.
/// Flat-pattern tree: feature 21 and end-of-part 22. Model root 0 is the
/// document model; model root 20 is the flat-pattern model.
fn inventory() -> (FeatureInventory, Vec<crate::sketch::PmDcSketch>) {
    let inventory = FeatureInventory {
        edge_inputs: crate::feature::edge_selection::EdgeInventory::default(),
        record_types: std::collections::HashMap::new(),
        features: vec![
            feature(2, 0x6200),
            feature(4, 0x6604),
            feature(6, 0x6200),
            feature(11, 0x6200),
            feature(21, 0x6200),
        ],
        pattern_features: Vec::new(),
        pattern_operands: crate::feature::pattern::Operands::default(),
        terminators: vec![terminator(5), terminator(22)],
        dimensions: Vec::new(),
        properties: Vec::new(),
        labels: vec![
            label(
                100,
                0,
                "",
                ClassId::try_from("0".repeat(32)).expect("class"),
                &[1, 2, 3, 4, 5, 6],
            ),
            label(101, 2, "Extrusion1", EXTRUSION, &[7]),
            label(
                102,
                3,
                "Face1",
                ClassId::try_from("0".repeat(32)).expect("class"),
                &[10, 11],
            ),
            label(103, 4, "Extrusion2", EXTRUSION, &[]),
            label(104, 6, "Extrusion3", EXTRUSION, &[]),
            label(105, 11, "Plate1", EXTRUSION, &[]),
            label(
                106,
                20,
                "",
                ClassId::try_from("0".repeat(32)).expect("class"),
                &[21, 22],
            ),
            label(107, 21, "Definition1", EXTRUSION, &[]),
        ],
        entity_style_links: Vec::new(),
        kernel_entity_links: Vec::new(),
        work_planes: Vec::new(),
        issues: Vec::new(),
        ..Default::default()
    };
    (inventory, vec![sketch(7), sketch(10)])
}

#[test]
fn history_follows_browser_order_and_end_of_part() {
    let (inventory, sketches) = inventory();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("history context");
    let history = build(&ctx, "generated", &inventory, &design(), &sketches).expect("history");
    assert_eq!(history.order, vec![7, 2, 10, 11, 4, 6, 21]);
    let placement = |ordinal| history.placement(ordinal).expect("placed record");
    assert_eq!(placement(7).kind, HistoryNodeKind::Sketch);
    assert_eq!(placement(2).ordinal, 2);
    assert!(!placement(4).below_end_of_part);
    assert!(placement(6).below_end_of_part);
    assert!(placement(6).model_history);
    assert!(!placement(21).model_history);
    assert!(!placement(21).below_end_of_part);
    assert_eq!(history.next_ordinal, 8);
}

#[test]
fn history_visits_a_repeated_member_once() {
    let (mut inventory, sketches) = inventory();
    inventory.labels[0] = label(
        100,
        0,
        "",
        ClassId::try_from("0".repeat(32)).expect("class"),
        &[2, 2, 3, 3],
    );
    inventory.labels[2] = label(
        102,
        3,
        "Face1",
        ClassId::try_from("0".repeat(32)).expect("class"),
        &[3, 11],
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("history context");
    let history = build(&ctx, "generated", &inventory, &design(), &sketches).expect("history");
    assert_eq!(history.order, vec![7, 2, 11, 21]);
}

#[test]
fn history_keeps_distinct_flat_pattern_model_roots() {
    let (mut inventory, sketches) = inventory();
    let mut design = design();
    design.model_roots.push(model_root(30, "MbBRepSegment"));
    inventory.features.push(feature(31, 0));
    inventory.labels.push(label(130, 30, "", EXTRUSION, &[31]));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("history context");
    let history = build(&ctx, "generated", &inventory, &design, &sketches).expect("history");
    let first = history.placement(21).expect("first flat-pattern feature");
    let second = history.placement(31).expect("second flat-pattern feature");
    assert!(!first.model_history);
    assert!(!second.model_history);
    assert_eq!(first.model_root, 20);
    assert_eq!(second.model_root, 30);
}

#[test]
fn projection_orders_history_and_marks_suppression() {
    let (inventory, sketches) = inventory();
    let design = design();
    let sketch_inventory = crate::sketch::SketchInventory {
        sketches,
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let projection = crate::feature::project(
        &ctx,
        &inventory,
        &design,
        &sketch_inventory,
        &[],
        &crate::feature::ProjectedSketches {
            sketches: &[],
            entities: &[],
            profile_bodies: &[],
            result_model: None,
        },
    )
    .expect("feature projection");
    let rows = projection
        .features
        .iter()
        .map(|feature| {
            (
                feature.ordinal,
                feature.name.clone(),
                feature.suppressed,
                feature.dependencies.as_slice().len(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec![
            (1, None, Some(false), 0),
            (2, Some("Extrusion1".into()), Some(false), 1),
            (3, None, Some(false), 0),
            (4, Some("Plate1".into()), Some(false), 0),
            (5, Some("Extrusion2".into()), Some(true), 0),
            (6, Some("Extrusion3".into()), Some(true), 0),
            (7, Some("Definition1".into()), Some(false), 0),
        ]
    );
    assert!(matches!(
        projection.features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: NativeFeatureKind::Extrude,
            ..
        })
    ));
    assert!(matches!(
        projection.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
    ));
    assert_eq!(
        projection.features[1].dependencies.as_slice()[0],
        projection.features[0].id
    );
    assert_eq!(
        projection.features[4]
            .source_properties
            .get("suppressed")
            .map(String::as_str),
        Some("true")
    );
    assert_eq!(
        projection.features[5]
            .source_properties
            .get("end_of_part")
            .map(String::as_str),
        Some("below")
    );
    assert_eq!(
        projection.features[6]
            .source_properties
            .get("history")
            .map(String::as_str),
        Some("flat_pattern")
    );
    assert_eq!(projection.unresolved_features, 5);
    assert_eq!(projection.unresolved_states, 0);
}

/// Root order: work plane 42, origin folder 41 holding work planes 40 and 44,
/// then the base tree. Work plane 43 has no browser position.
fn work_plane_inventory() -> (FeatureInventory, Vec<crate::sketch::PmDcSketch>) {
    use crate::feature::work_plane::tests::work_plane;
    let (mut inventory, sketches) = inventory();
    let folder = ClassId::try_from("0".repeat(32)).expect("class");
    inventory.labels[0] = label(100, 0, "", folder, &[42, 41, 1, 2, 3, 4, 5, 6]);
    inventory
        .labels
        .push(label(140, 41, "Origin", folder, &[40, 44]));
    inventory
        .labels
        .push(label(141, 40, "XY Plane", folder, &[]));
    inventory
        .labels
        .push(label(142, 42, "Work Plane1", folder, &[]));
    inventory.labels.push(label(143, 44, "Skewed", folder, &[]));
    inventory.work_planes = vec![
        work_plane(40, [0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        work_plane(42, [0.0, 0.0, 1.5], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        work_plane(43, [0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        work_plane(44, [0.0; 3], [1.0, 0.0, 0.0], [0.6, 0.8, 0.0]),
    ];
    (inventory, sketches)
}

#[test]
fn history_places_browser_work_planes() {
    let (inventory, sketches) = work_plane_inventory();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("history context");
    let history = build(&ctx, "generated", &inventory, &design(), &sketches).expect("history");
    assert_eq!(history.order, vec![42, 40, 44, 7, 2, 10, 11, 4, 6, 21]);
    for ordinal in [40, 42, 44] {
        assert_eq!(
            history.placement(ordinal).expect("placed work plane").kind,
            HistoryNodeKind::WorkPlane
        );
    }
    assert!(history.placement(43).is_none());
}

fn project_work_planes(
    inventory: &FeatureInventory,
    sketches: Vec<crate::sketch::PmDcSketch>,
) -> crate::feature::FeatureProjection {
    let sketch_inventory = crate::sketch::SketchInventory {
        sketches,
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    crate::feature::project(
        &ctx,
        inventory,
        &design(),
        &sketch_inventory,
        &[],
        &crate::feature::ProjectedSketches {
            sketches: &[],
            entities: &[],
            profile_bodies: &[],
            result_model: None,
        },
    )
    .expect("feature projection")
}

#[test]
fn work_planes_do_not_count_as_typed_feature_records() {
    let (mut inventory, sketches) = work_plane_inventory();
    inventory.features.truncate(1);
    let projection = project_work_planes(&inventory, sketches);
    assert_eq!(projection.unresolved_features, 1);
    assert_eq!(
        projection
            .features
            .iter()
            .filter(|feature| feature.source_tag.as_deref() == Some("work_plane"))
            .count(),
        2
    );
}

#[test]
fn projection_writes_browser_work_planes_in_history_order() {
    let (inventory, sketches) = work_plane_inventory();
    let design = design();
    let sketch_inventory = crate::sketch::SketchInventory {
        sketches,
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    let projection = crate::feature::project(
        &ctx,
        &inventory,
        &design,
        &sketch_inventory,
        &[],
        &crate::feature::ProjectedSketches {
            sketches: &[],
            entities: &[],
            profile_bodies: &[],
            result_model: None,
        },
    )
    .expect("feature projection");
    let rows = projection
        .features
        .iter()
        .take(4)
        .map(|feature| {
            (
                feature.ordinal,
                feature.name.clone(),
                feature.source_tag.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec![
            (1, Some("Work Plane1".into()), Some("work_plane".into())),
            (2, Some("XY Plane".into()), Some("work_plane".into())),
            (3, Some("Skewed".into()), Some("native".into())),
            (4, None, Some("sketch".into())),
        ]
    );
    let FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) =
        projection.features[0].evaluation.definition()
    else {
        panic!("work plane projects as a datum plane");
    };
    assert_eq!(
        frame.origin().get(),
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 15.0)
    );
    assert!(projection
        .features
        .iter()
        .all(|feature| feature.native_ref != Some(inventory.work_planes[2].id())));
    assert_eq!(projection.features.len(), 10);
    assert_eq!(projection.unresolved_features, 5);
}

#[test]
fn a_work_plane_built_from_a_nested_work_plane_follows_it() {
    use crate::feature::work_plane::tests::work_plane;
    let (mut inventory, sketches) = work_plane_inventory();
    let folder = ClassId::try_from("0".repeat(32)).expect("class");
    let position = inventory
        .labels
        .iter()
        .position(|label| label.name() == Some("Work Plane1"))
        .expect("work-plane label");
    inventory.labels[position] = label(142, 42, "Work Plane1", folder, &[45, 40]);
    inventory
        .labels
        .push(label(145, 45, "Work Plane0", folder, &[]));
    inventory.work_planes.push(work_plane(
        45,
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("history context");
    let history = build(&ctx, "generated", &inventory, &design(), &sketches).expect("history");
    assert_eq!(history.order[..4], [45, 40, 42, 44]);
    assert_eq!(
        history.placement(45).expect("nested work plane").kind,
        HistoryNodeKind::WorkPlane
    );
    let projection = project_work_planes(&inventory, sketches);
    assert_eq!(projection.features[0].name.as_deref(), Some("Work Plane0"));
}
