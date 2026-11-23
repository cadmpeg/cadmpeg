//! Loft section and guide operands of `moBlend_c` records.

use super::{
    bind_guides, enrich_history_loft_inputs, restates_record_sections, retained_guide_state,
    retained_guides, RetainedGuides,
};
use crate::loss::SldprtLossCode;
use crate::records::{
    Feature, FeatureHistory, FeatureInputLane, FeatureInputName, FeatureSource, ObjectId,
};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{LoftSection, PathRef, PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation, LoftGuidance};
use cadmpeg_ir::report::loss::{LossKind, LossNote};
use std::collections::BTreeMap;

const GENERAL_CURVE_REF: u16 = 0x809c;
const COMPONENT_PROFILE: u16 = 0x809e;
const ENTITY_GROUP: u16 = 0x8289;
const WIRE_CONTACT: u16 = 0x8286;
const LOFT: u32 = 30;
const LOFT_NAME: u64 = 10;

#[derive(Clone, Copy, PartialEq)]
enum Tail {
    Loft,
    Sweep,
    NotUnit,
    BodyFlag,
    Suffix,
}

#[derive(Clone, Copy, PartialEq)]
enum Guide {
    Group,
    SectionChild,
    AfterGap,
}

#[derive(Clone)]
struct Record {
    sections: Vec<u32>,
    wide_count: Option<u32>,
    tail: Tail,
    declared_guides: u16,
    guides: Vec<Guide>,
}

impl Record {
    fn sections(sections: &[u32]) -> Self {
        Self {
            sections: sections.to_vec(),
            wide_count: None,
            tail: Tail::Loft,
            declared_guides: 0,
            guides: Vec::new(),
        }
    }

    fn guides(self, declared: u16, bodies: usize) -> Self {
        self.guide_forms(declared, &vec![Guide::Group; bodies])
    }

    fn guide_forms(self, declared: u16, guides: &[Guide]) -> Self {
        Self {
            declared_guides: declared,
            guides: guides.to_vec(),
            ..self
        }
    }
}

fn section(payload: &mut Vec<u8>, source: u32, tail: Tail) {
    payload.extend(GENERAL_CURVE_REF.to_le_bytes());
    payload.extend(COMPONENT_PROFILE.to_le_bytes());
    let prefix = payload.len();
    payload.resize(prefix + 81, 0);
    payload[prefix..prefix + 10].copy_from_slice(&[0x2d, 0x80, 2, 0, 0, 0, 0, 0, 0, 0]);
    payload[prefix + 45..prefix + 61].fill(0xff);
    let mut record = [0; 52];
    record[0..4].copy_from_slice(&source.to_le_bytes());
    record[4..8].copy_from_slice(&0x603b_07f6u32.to_le_bytes());
    record[16..20].copy_from_slice(&0x65u32.to_le_bytes());
    record[24..28].fill(0xff);
    for handle in [32, 36, 40] {
        record[handle..handle + 4].copy_from_slice(&[0xc7, 0xcf, 0xff, 0xff]);
    }
    record[48..52].copy_from_slice(&13_000u32.to_le_bytes());
    payload.extend(record);
    if tail == Tail::Sweep {
        payload.extend([0; 12]);
        payload.extend([0xff; 4]);
        payload.extend([0; 8]);
    } else {
        payload.extend([0; 20]);
    }
    let unit: f64 = if tail == Tail::NotUnit { 2.0 } else { 1.0 };
    payload.extend(unit.to_le_bytes());
    payload.push(u8::from(tail == Tail::BodyFlag));
    payload.extend([if tail == Tail::Suffix { 0 } else { 0xff }; 8]);
    payload.extend([0; 6]);
}

fn guide(payload: &mut Vec<u8>, form: Guide) {
    if form == Guide::AfterGap {
        payload.extend([0; 8]);
    }
    payload.extend(WIRE_CONTACT.to_le_bytes());
    let body = payload.len();
    payload.resize(body + 62, 0);
    payload[body + 28..body + 32].copy_from_slice(&1u32.to_le_bytes());
    payload[body + 32..body + 36].copy_from_slice(&(-1i32).to_le_bytes());
    payload[body + 36..body + 38].copy_from_slice(&2u16.to_le_bytes());
    if form == Guide::SectionChild {
        section(payload, 11, Tail::Loft);
        return;
    }
    payload.extend(GENERAL_CURVE_REF.to_le_bytes());
    payload.extend(ENTITY_GROUP.to_le_bytes());
    payload.extend([0x2d, 0x80, 2, 0, 0, 0, 0, 0, 0, 0]);
    payload.extend([0; 20]);
    payload.extend([0xff; 8]);
    payload.extend([0; 6]);
}

fn name(id: &str, source: u32, offset: u64) -> FeatureInputName {
    FeatureInputName {
        id: format!("name:{id}"),
        parent: "lane#7".into(),
        ordinal: 0,
        offset,
        object_id: ObjectId::from_value(source),
        value: id.into(),
    }
}

/// A lane that holds one loft record after `pad` leading bytes, with extra
/// name records at the given offsets, shifted by `pad`.
fn lane(id: &str, record: &Record, pad: u64, extra: &[(&str, u32, u64)]) -> FeatureInputLane {
    let mut payload = vec![0; 40 + usize::try_from(pad).unwrap()];
    let count = u16::try_from(record.sections.len()).unwrap();
    payload.extend(record.wide_count.unwrap_or(u32::from(count)).to_le_bytes());
    payload.extend(count.to_le_bytes());
    for source in &record.sections {
        section(&mut payload, *source, record.tail);
    }
    payload.extend(record.declared_guides.to_le_bytes());
    for form in &record.guides {
        guide(&mut payload, *form);
    }
    payload.extend([0; 16]);
    let end = u64::try_from(payload.len()).unwrap();
    let mut names = vec![name("loft", LOFT, LOFT_NAME + pad), name("next", 40, end)];
    names.extend(
        extra
            .iter()
            .map(|(id, source, offset)| name(id, *source, offset + pad)),
    );
    FeatureInputLane {
        id: id.into(),
        configuration: None,
        native_payload: payload.into(),
        classes: Vec::new(),
        names,
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

fn native(id: &str, parent: &str, source: u32, class: &str) -> Feature {
    Feature {
        id: id.into(),
        parent: parent.into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal: source,
        name: id.into(),
        kind: "Feature".into(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn history() -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native("a", "history", 11, "moProfileFeature_c"),
            native("b", "history", 12, "moProfileFeature_c"),
            native("c", "history", 13, "moProfileFeature_c"),
            native("held", "history", 14, "moProfileFeature_c"),
            native("plane", "history", 20, "moRefPlane_c"),
            native("loft", "history", LOFT, "moBlend_c"),
            native("later", "history", 35, "moProfileFeature_c"),
            native("next", "history", 40, "moRefPlane_c"),
        ],
    }
}

fn enrich(histories: &mut [FeatureHistory], lanes: &[FeatureInputLane]) -> Vec<LossNote> {
    enrich_history_loft_inputs(histories, lanes)
}

fn apply(record: &Record, extra: &[(&str, u32, u64)]) -> (Feature, Vec<LossNote>) {
    let mut histories = [history()];
    let losses = enrich(&mut histories, &[lane("lane#7", record, 0, extra)]);
    let [history] = histories;
    let loft = history
        .features
        .into_iter()
        .find(|feature| feature.id == "loft")
        .unwrap();
    (loft, losses)
}

fn profiles(feature: &Feature) -> Option<&str> {
    feature.properties.get("Profiles").map(String::as_str)
}

fn codes(losses: &[LossNote]) -> Vec<LossKind> {
    losses.iter().map(|loss| loss.code.clone()).collect()
}

fn assert_unresolved(losses: &[LossNote], reason: &str) {
    assert_eq!(
        codes(losses),
        [SldprtLossCode::FeatureLoftSectionsUnresolved.kind()]
    );
    assert_eq!(
        losses[0].message,
        format!("loft keeps no sections: {reason}.")
    );
}

#[test]
fn sections_name_their_sketches_by_source_in_stored_order() {
    let (loft, losses) = apply(&Record::sections(&[12, 11]), &[]);
    assert_eq!(profiles(&loft), Some("12,11"));
    assert!(losses.is_empty());
}

#[test]
fn three_sections_state_their_assumed_order() {
    let (loft, losses) = apply(&Record::sections(&[11, 13, 12]), &[]);
    assert_eq!(profiles(&loft), Some("11,13,12"));
    assert_eq!(
        codes(&losses),
        [SldprtLossCode::FeatureLoftSectionOrderAssumed.kind()]
    );
}

#[test]
fn a_single_section_is_no_loft() {
    let (loft, losses) = apply(&Record::sections(&[11]), &[]);
    assert_eq!(profiles(&loft), None);
    assert_unresolved(&losses, "its section records are unreadable");
}

#[test]
fn the_two_section_counts_must_agree() {
    let record = Record {
        wide_count: Some(3),
        ..Record::sections(&[11, 12])
    };
    let (loft, losses) = apply(&record, &[]);
    assert_eq!(profiles(&loft), None);
    assert_unresolved(&losses, "its section records are unreadable");
}

#[test]
fn a_section_needs_the_loft_section_tail() {
    for tail in [Tail::Sweep, Tail::NotUnit, Tail::BodyFlag, Tail::Suffix] {
        let record = Record {
            tail,
            ..Record::sections(&[11, 12])
        };
        let (loft, losses) = apply(&record, &[]);
        assert_eq!(profiles(&loft), None);
        assert_unresolved(&losses, "its section records are unreadable");
    }
}

#[test]
fn a_sketch_held_in_place_does_not_end_the_loft() {
    let (loft, losses) = apply(&Record::sections(&[11, 12]), &[("held", 14, 20)]);
    assert_eq!(profiles(&loft), Some("11,12"));
    assert!(losses.is_empty());
}

#[test]
fn another_object_ends_the_loft() {
    for (id, source) in [("plane", 20), ("later", 35)] {
        let (loft, losses) = apply(&Record::sections(&[11, 12]), &[(id, source, 20)]);
        assert_eq!(profiles(&loft), None);
        assert_unresolved(&losses, "its section records are unreadable");
    }
}

#[test]
fn a_section_names_a_preceding_sketch_of_its_history() {
    for (source, reason) in [
        (35, "section 35 names a later feature"),
        (20, "section 20 names no sketch"),
        (99, "section 99 names no feature of its history"),
    ] {
        let (loft, losses) = apply(&Record::sections(&[11, source]), &[]);
        assert_eq!(profiles(&loft), None);
        assert_unresolved(&losses, reason);
    }
}

#[test]
fn a_source_of_another_history_leaves_the_section_unique() {
    let mut other = history();
    other.id = "other".into();
    other.features = vec![native("other-a", "other", 11, "moProfileFeature_c")];
    let mut histories = [history(), other];
    let losses = enrich(
        &mut histories,
        &[lane("lane#7", &Record::sections(&[11, 12]), 0, &[])],
    );
    assert_eq!(profiles(&histories[0].features[5]), Some("11,12"));
    assert!(losses.is_empty());
}

#[test]
fn stated_operands_are_kept() {
    for property in ["Profiles", "Guides"] {
        let mut histories = [history()];
        histories[0].features[5].properties.insert(
            cadmpeg_core::text::NonBlankString::new(property.to_owned()).unwrap(),
            "13".into(),
        );
        let losses = enrich(
            &mut histories,
            &[lane("lane#7", &Record::sections(&[11, 12]), 0, &[])],
        );
        assert_eq!(
            profiles(&histories[0].features[5]),
            (property == "Profiles").then_some("13")
        );
        assert!(losses.is_empty());
    }
}

#[test]
fn configurations_agree_on_content() {
    let record = Record::sections(&[11, 12]);
    let mut histories = [history()];
    let losses = enrich(
        &mut histories,
        &[
            lane("lane#7", &record, 0, &[]),
            lane("lane#8", &record, 8, &[]),
        ],
    );
    assert_eq!(profiles(&histories[0].features[5]), Some("11,12"));
    assert!(losses.is_empty());
    for other in [Record::sections(&[11, 13]), Record::sections(&[11])] {
        let mut histories = [history()];
        let losses = enrich(
            &mut histories,
            &[
                lane("lane#7", &record, 0, &[]),
                lane("lane#8", &other, 0, &[]),
            ],
        );
        assert_eq!(profiles(&histories[0].features[5]), None);
        assert_unresolved(
            &losses,
            "its configurations do not agree on its section records",
        );
    }
}

fn model_loft() -> cadmpeg_ir::features::Feature {
    let section =
        |id: &str| LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(id.into())));
    cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#loft").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        native_ref: Some("loft".into()),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Loft {
                sections: vec![section("a"), section("b")],
                guidance: LoftGuidance::default(),
                op: BooleanOp::Unresolved,
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                allow_multi_profile_faces: None,
            },
        )),
    }
}

fn guidance(feature: &cadmpeg_ir::features::Feature) -> LoftGuidance {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Loft { guidance, .. }) => guidance.clone(),
        _ => panic!("the feature is a loft"),
    }
}

fn bind(record: &Record, histories: &[FeatureHistory]) -> (LoftGuidance, Vec<LossNote>) {
    let lanes = [lane("lane#7", record, 0, &[])];
    bind_lanes(&lanes, histories)
}

fn bind_lanes(
    lanes: &[FeatureInputLane],
    histories: &[FeatureHistory],
) -> (LoftGuidance, Vec<LossNote>) {
    let mut features = [model_loft()];
    let mut losses = Vec::new();
    bind_guides(&mut features, &[], histories, lanes, &mut losses);
    (guidance(&features[0]), losses)
}

fn assert_guides_unresolved(guidance: &LoftGuidance, losses: &[LossNote], reason: &str) {
    assert_eq!(*guidance, LoftGuidance::default());
    assert_eq!(
        codes(losses),
        [SldprtLossCode::FeatureLoftGuidesUnresolved.kind()]
    );
    assert_eq!(
        losses[0].message,
        format!("loft keeps no guide curves: {reason}.")
    );
}

fn assert_guides(guidance: &LoftGuidance, losses: &[LossNote], count: usize) {
    assert_eq!(
        *guidance,
        LoftGuidance::Guides(retained_guides(LOFT, count))
    );
    assert_eq!(
        codes(losses),
        [SldprtLossCode::FeatureLoftGuidesRetained.kind()]
    );
}

#[test]
fn guide_curves_keep_native_references() {
    let record = Record::sections(&[11, 12]).guides(2, 2);
    let lanes = [lane("lane#7", &record, 0, &[])];
    let mut enriched = [history()];
    assert!(enrich(&mut enriched, &lanes).is_empty());
    for histories in [enriched, [history()]] {
        let (guidance, losses) = bind_lanes(&lanes, &histories);
        assert_guides(&guidance, &losses, 2);
    }
    assert_eq!(
        retained_guides(LOFT, 2),
        [
            PathRef::Native("sldprt:feature-input:loft-guide:30:0/2".into()),
            PathRef::Native("sldprt:feature-input:loft-guide:30:1/2".into()),
        ]
    );
}

#[test]
fn unreadable_guide_curves_keep_the_sections() {
    for (declared, bodies) in [(2, 1), (2, 0)] {
        let record = Record::sections(&[11, 12]).guides(declared, bodies);
        let (loft, losses) = apply(&record, &[]);
        assert_eq!(profiles(&loft), Some("11,12"));
        assert!(losses.is_empty());
        let (guidance, losses) = bind(&record, &[history()]);
        assert_guides_unresolved(&guidance, &losses, "its guide-curve records are unreadable");
    }
}

#[test]
fn a_guide_shaped_record_after_the_counted_guides_is_kept_apart() {
    for (declared, bodies) in [(0, 1), (1, 2)] {
        let record = Record::sections(&[11, 12]).guides(declared, bodies);
        let (loft, losses) = apply(&record, &[]);
        assert_eq!(profiles(&loft), Some("11,12"));
        assert!(losses.is_empty());
        let (guidance, losses) = bind(&record, &[history()]);
        if declared == 0 {
            assert_eq!(guidance, LoftGuidance::default());
            assert!(losses.is_empty());
        } else {
            assert_guides(&guidance, &losses, 1);
        }
    }
}

#[test]
fn guide_curves_are_consecutive() {
    let record = Record::sections(&[11, 12]).guide_forms(2, &[Guide::Group, Guide::AfterGap]);
    let (guidance, losses) = bind(&record, &[history()]);
    assert_guides_unresolved(&guidance, &losses, "its guide-curve records are unreadable");
}

#[test]
fn a_section_shaped_child_is_no_guide_curve() {
    let record = Record::sections(&[11, 12]).guide_forms(1, &[Guide::SectionChild]);
    let (guidance, losses) = bind(&record, &[history()]);
    assert_guides_unresolved(&guidance, &losses, "its guide-curve records are unreadable");
}

#[test]
fn a_name_after_the_first_section_ends_the_loft() {
    let record = Record::sections(&[11, 12]).guides(2, 2);
    let lanes = [lane("lane#7", &record, 0, &[("held", 14, 420)])];
    let (guidance, losses) = bind_lanes(&lanes, &[history()]);
    assert_guides_unresolved(&guidance, &losses, "its guide-curve records are unreadable");
}

#[test]
fn a_name_without_an_object_identifier_ends_the_loft() {
    let mut lane = lane("lane#7", &Record::sections(&[11, 12]), 0, &[]);
    lane.names.push(FeatureInputName {
        object_id: None,
        ..name("unnamed", 0, 20)
    });
    let mut histories = [history()];
    let losses = enrich(&mut histories, &[lane]);
    assert_eq!(profiles(&histories[0].features[5]), None);
    assert_unresolved(&losses, "its section records are unreadable");
}

#[test]
fn configurations_that_differ_on_guides_keep_the_sections() {
    let lanes = [
        lane("lane#7", &Record::sections(&[11, 12]).guides(2, 2), 0, &[]),
        lane("lane#8", &Record::sections(&[11, 12]).guides(1, 1), 0, &[]),
    ];
    let mut histories = [history()];
    assert!(enrich(&mut histories, &lanes).is_empty());
    assert_eq!(profiles(&histories[0].features[5]), Some("11,12"));
    let (guidance, losses) = bind_lanes(&lanes, &histories);
    assert_guides_unresolved(
        &guidance,
        &losses,
        "its configurations do not agree on its guide-curve records",
    );
}

#[test]
fn guide_curves_follow_the_stated_sections() {
    let record = Record::sections(&[11, 12]).guides(2, 2);
    for (stated, bound) in [("11, 12", true), ("12,11", false)] {
        let mut histories = [history()];
        histories[0].features[5]
            .properties
            .insert(cadmpeg_core::nonblank_literal!("Profiles"), stated.into());
        let (guidance, losses) = bind(&record, &histories);
        if bound {
            assert_guides(&guidance, &losses, 2);
        } else {
            assert_guides_unresolved(
                &guidance,
                &losses,
                "its stated sections differ from its section records",
            );
        }
    }
}

#[test]
fn unreadable_records_of_stated_sections_leave_no_guides() {
    let mut histories = [history()];
    histories[0].features[5]
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Profiles"), "11,12".into());
    let (guidance, losses) = bind(&Record::sections(&[11]), &histories);
    assert_guides_unresolved(&guidance, &losses, "its section records are unreadable");
}

#[test]
fn a_stated_feature_with_the_same_definition_carries_the_loss() {
    for record in [
        Record::sections(&[11, 12]).guides(2, 2),
        Record::sections(&[11, 12]).guides(2, 1),
    ] {
        let lanes = [lane("lane#7", &record, 0, &[])];
        let histories = [history()];
        let mut document = [model_loft()];
        let mut document_losses = Vec::new();
        bind_guides(&mut document, &[], &histories, &lanes, &mut document_losses);
        assert_eq!(document_losses.len(), 1);

        let mut features = [model_loft()];
        let mut losses = Vec::new();
        bind_guides(&mut features, &document, &histories, &lanes, &mut losses);
        assert_eq!(guidance(&features[0]), guidance(&document[0]));
        assert!(losses.is_empty());

        let mut other = model_loft();
        other.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Loft { op, .. }) = definition {
                *op = BooleanOp::Join;
            }
        });
        let mut features = [model_loft()];
        let mut losses = Vec::new();
        bind_guides(&mut features, &[other], &histories, &lanes, &mut losses);
        assert_eq!(losses, document_losses);
    }
}

#[test]
fn the_encoder_keeps_only_unchanged_retained_guides() {
    let histories = [history()];
    let record = &histories[0].features[5];
    let retained = retained_guides(LOFT, 2);
    assert_eq!(
        retained_guide_state(Some(record), &[]),
        RetainedGuides::Absent
    );
    assert_eq!(
        retained_guide_state(Some(record), &retained),
        RetainedGuides::Unchanged
    );
    for changed in [
        retained[..1].to_vec(),
        retained_guides(LOFT + 1, 2),
        vec![retained[1].clone(), retained[0].clone()],
        vec![retained[0].clone(), PathRef::Native("other".into())],
    ] {
        assert_eq!(
            retained_guide_state(Some(record), &changed),
            RetainedGuides::Changed
        );
    }
    assert_eq!(
        retained_guide_state(None, &retained),
        RetainedGuides::Changed
    );
}

#[test]
fn the_encoder_writes_only_changed_sections() {
    let mut histories = [history()];
    assert!(restates_record_sections(
        Some(&histories[0].features[5]),
        Some("11,12"),
        "11,12"
    ));
    assert!(!restates_record_sections(
        Some(&histories[0].features[5]),
        Some("11,12"),
        "12,11"
    ));
    histories[0].features[5]
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Profiles"), "11,12".into());
    assert!(!restates_record_sections(
        Some(&histories[0].features[5]),
        Some("11,12"),
        "11,12"
    ));
}
