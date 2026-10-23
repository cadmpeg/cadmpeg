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
        features: vec![
            feature(2, 0x6200),
            feature(4, 0x6604),
            feature(6, 0x6200),
            feature(11, 0x6200),
            feature(21, 0x6200),
        ],
        pattern_features: Vec::new(),
        terminators: vec![terminator(5), terminator(22)],
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
