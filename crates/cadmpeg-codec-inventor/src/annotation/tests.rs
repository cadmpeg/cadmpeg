// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::pmi::{
    DimensionTolerance, GeometricToleranceKind, PmiAnnotation, PmiDefinition, PmiDimension,
    PmiQuantity, PmiTarget,
};

use super::names::HOLE_NAME;
use super::names::{edge_name, generated_name, AttributeName, BodyNames, EntityKind, NameFamily};
use super::records::{
    edge_selector, frame_row, naming_table, tolerance_feature, trailing_reference, Record, Segment,
    Value, ATTACHMENT, BODY, DATUM_DEFINITION, DATUM_FEATURE, DATUM_FRAME, DATUM_OWNER,
    DIAMETER_LAYOUT, DIAMETER_OWNER, EDGE_ITEM, FACE_SELECTOR, FEATURE_FACE_SELECTOR, FRAME_OWNER,
    FRAME_ROW, GEOMETRY, GEOMETRY_LINK, HOLE_NOTE_OWNER, HOLE_VALUE, INTENT, LINEAR_OWNER,
    NAMING_TABLE, PARAMETER, PLANE, SIZE_RELATION, STYLE, TOLERANCE_FEATURE, VALUE,
};
use super::{fit, losses, project_segment, tolerance, Label, Outcome, Selections};
use crate::loss::InventorLossCode;

const VALUE_TOLERANCE: f64 = 1.0e-12;
const FACE: &str = "inventor:brep:entity#10";
const OTHER_FACE: &str = "inventor:brep:entity#11";

// Record ordinals of the dimension fixture.
const PLANE_AT: u32 = 1;
const LAYOUT_AT: u32 = 2;
const VALUE_AT: u32 = 3;
const INTENT_AT: u32 = 4;
const STYLE_AT: u32 = 5;
const SECOND_STYLE_AT: u32 = 6;
const NOMINAL_AT: u32 = 7;
const REFERENCE_AT: u32 = 8;
const OWNER_AT: u32 = 9;
const RELATION_AT: u32 = 10;
const ITEM_AT: u32 = 11;
const LINK_AT: u32 = 12;
const GEOMETRY_AT: u32 = 13;
const SELECTOR_AT: u32 = 14;
const TABLE_AT: u32 = 15;
const BODY_AT: u32 = 16;
const NAME_AT: u32 = 17;
const SECOND_SELECTOR_AT: u32 = 18;
const SECOND_NAME_AT: u32 = 19;
const SECOND_RELATION_AT: u32 = 20;
const KEY: u32 = 100;
const SECOND_KEY: u32 = 101;
/// Offset of the `enabled` byte of a version-25 face selector.
const SELECTOR_ENABLED_AT: usize = 26;
/// Offset of `word_a` in a name-record prefix.
const NAME_FORM_AT: usize = 6;

const fn qref(ordinal: u32) -> u32 {
    0x8000_0000 | (ordinal + 1)
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend(value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_le_bytes());
}

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend(value.to_le_bytes());
}

fn push_text(bytes: &mut Vec<u8>, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    push_u32(bytes, u32::try_from(units.len()).expect("text length"));
    for unit in units {
        push_u16(bytes, unit);
    }
}

/// A state content header of segment major version 25.
fn state_header(source: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 3);
    push_u16(&mut bytes, 0x1234);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0x200);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, source);
    bytes
}

/// A content header of segment major version 25 with its two stamps.
fn content_header(source: u32) -> Vec<u8> {
    let mut bytes = state_header(source);
    push_i32(&mut bytes, -1);
    push_i32(&mut bytes, -1);
    bytes
}

/// An annotation header: the state header and `state = -1`.
fn annotation_header(source: u32) -> Vec<u8> {
    let mut bytes = state_header(source);
    push_i32(&mut bytes, -1);
    bytes
}

fn type_2_list(bytes: &mut Vec<u8>, references: &[u32]) {
    push_u16(bytes, 2);
    push_u16(bytes, 0x3000);
    push_u32(bytes, u32::try_from(references.len()).expect("list length"));
    if !references.is_empty() {
        push_u32(bytes, u32::try_from(references.len()).expect("list length"));
        push_u32(bytes, 0);
    }
    for reference in references {
        push_u32(bytes, *reference);
    }
}

fn diameter_owner(references: [u32; 5]) -> Vec<u8> {
    let mut bytes = annotation_header(900);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, qref(PLANE_AT));
    push_u32(&mut bytes, 1);
    for reference in references {
        push_u32(&mut bytes, reference);
    }
    bytes
}

fn linear_owner(operands: &[u32]) -> Vec<u8> {
    let mut bytes = annotation_header(901);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, qref(PLANE_AT));
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, qref(LAYOUT_AT));
    push_u32(&mut bytes, qref(VALUE_AT));
    for operand in operands {
        push_u32(&mut bytes, *operand);
    }
    bytes
}

/// A value record with the given measure, tolerance code, display, and
/// deviations, in internal units.
fn value_record(measure: u32, tolerance: u32, display: u32, upper: f64, lower: f64) -> Vec<u8> {
    let mut bytes = content_header(902);
    for word in [0, 0, 0, measure, tolerance, 0, 0, 0, 0, 0, 0, display, 0, 0] {
        push_u32(&mut bytes, word);
    }
    bytes.extend(upper.to_le_bytes());
    bytes.extend(lower.to_le_bytes());
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_text(&mut bytes, "");
    push_text(&mut bytes, "");
    bytes.extend([0; 76]);
    push_u32(&mut bytes, qref(NOMINAL_AT));
    push_u32(&mut bytes, qref(REFERENCE_AT));
    bytes
}

fn relation(intent: u32, items: &[u32]) -> Vec<u8> {
    let mut bytes = annotation_header(903);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, qref(intent));
    for item in items {
        push_u32(&mut bytes, qref(*item));
    }
    bytes
}

fn geometry_link() -> Vec<u8> {
    let mut bytes = annotation_header(904);
    push_u32(&mut bytes, qref(GEOMETRY_AT));
    push_u32(&mut bytes, qref(PLANE_AT));
    push_u32(&mut bytes, qref(ITEM_AT));
    bytes
}

/// A face selector in the face-naming grammar of section 13.
fn face_selector(source: u32, state: i32, key: u32) -> Vec<u8> {
    let mut bytes = state_header(source);
    push_i32(&mut bytes, state);
    bytes.push(1);
    bytes.push(5);
    push_u32(&mut bytes, qref(BODY_AT));
    push_u32(&mut bytes, 77);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 77);
    push_u16(&mut bytes, 8);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 1);
    push_u16(&mut bytes, 1);
    push_u16(&mut bytes, 0);
    push_u32(&mut bytes, 210);
    push_u16(&mut bytes, 8);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 0);
    bytes.push(0);
    push_u32(&mut bytes, qref(TABLE_AT));
    push_u32(&mut bytes, key);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, qref(TABLE_AT));
    push_u32(&mut bytes, 55);
    push_u32(&mut bytes, qref(GEOMETRY_AT));
    bytes
}

fn naming_table_record(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0x1e);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 999);
    push_u32(
        &mut bytes,
        u32::try_from(entries.len()).expect("entry count"),
    );
    for (id, ordinal) in entries {
        push_u32(&mut bytes, *id);
        push_u32(&mut bytes, qref(*ordinal));
    }
    bytes
}

fn name_prefix(form: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 0x1234);
    push_u32(&mut bytes, form);
    push_u32(&mut bytes, 0x3000_0002);
    bytes.extend([0; 24]);
    push_u32(&mut bytes, 0xb4);
    push_u32(&mut bytes, 0);
    bytes
}

fn hole_name(pairs: &[(u32, i32)], first: u32, role: u16) -> Vec<u8> {
    let mut bytes = name_prefix(5);
    push_u16(&mut bytes, 2);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, u32::try_from(pairs.len()).expect("pair count"));
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0x1000_0000);
    for (source, value) in pairs {
        push_u32(&mut bytes, *source);
        push_i32(&mut bytes, *value);
    }
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, first);
    push_u16(&mut bytes, role);
    bytes
}

fn edge_name_record(form: u32, list: &[u32], faces: &[u32]) -> Vec<u8> {
    let mut bytes = name_prefix(form);
    for value in [qref(TABLE_AT), 1894, qref(TABLE_AT), 1894] {
        push_u32(&mut bytes, value);
    }
    bytes.push(1);
    type_2_list(&mut bytes, list);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, u32::try_from(faces.len()).expect("face count"));
    for face in faces {
        push_u32(&mut bytes, qref(TABLE_AT));
        push_u32(&mut bytes, *face);
    }
    bytes
}

/// Records of one synthesized segment.
#[derive(Default)]
struct Fixture {
    records: Vec<(u32, [u8; 16], Vec<u8>)>,
    parameters: HashMap<u32, f64>,
}

impl Fixture {
    fn put(&mut self, ordinal: u32, type_id: [u8; 16], payload: Vec<u8>) -> &mut Self {
        self.records.retain(|(existing, ..)| *existing != ordinal);
        self.records.push((ordinal, type_id, payload));
        self
    }

    /// A diameter dimension whose intent joins one face selector.
    fn dimension(tolerance: u32, display: u32, upper: f64, lower: f64) -> Self {
        let mut fixture = Self::default();
        fixture
            .put(PLANE_AT, PLANE, Vec::new())
            .put(LAYOUT_AT, DIAMETER_LAYOUT, Vec::new())
            .put(
                VALUE_AT,
                VALUE,
                value_record(1, tolerance, display, upper, lower),
            )
            .put(INTENT_AT, INTENT, Vec::new())
            .put(STYLE_AT, STYLE, Vec::new())
            .put(SECOND_STYLE_AT, STYLE, Vec::new())
            .put(NOMINAL_AT, PARAMETER, Vec::new())
            .put(REFERENCE_AT, PARAMETER, Vec::new())
            .put(
                OWNER_AT,
                DIAMETER_OWNER,
                diameter_owner([
                    qref(LAYOUT_AT),
                    qref(VALUE_AT),
                    qref(INTENT_AT),
                    qref(STYLE_AT),
                    qref(SECOND_STYLE_AT),
                ]),
            )
            .put(RELATION_AT, SIZE_RELATION, relation(INTENT_AT, &[ITEM_AT]))
            .put(ITEM_AT, EDGE_ITEM, Vec::new())
            .put(LINK_AT, GEOMETRY_LINK, geometry_link())
            .put(GEOMETRY_AT, GEOMETRY, Vec::new())
            .put(SELECTOR_AT, FACE_SELECTOR, face_selector(3809, 2, KEY))
            .put(
                TABLE_AT,
                NAMING_TABLE,
                naming_table_record(&[(KEY, NAME_AT), (SECOND_KEY, SECOND_NAME_AT)]),
            )
            .put(BODY_AT, BODY, state_header(77))
            .put(NAME_AT, HOLE_NAME, hole_name(&[(945, -1)], 1, 7))
            .put(SECOND_NAME_AT, HOLE_NAME, hole_name(&[(818, -1)], 1, 7));
        fixture.parameters.insert(NOMINAL_AT, 3.5);
        fixture
    }

    fn segment(&self) -> Segment<'_> {
        Segment {
            token: "segment".to_owned(),
            version: 25,
            records: self
                .records
                .iter()
                .map(|(ordinal, type_id, payload)| {
                    (
                        *ordinal,
                        Record {
                            type_id: *type_id,
                            payload: View::over_retained(payload),
                        },
                    )
                })
                .collect(),
        }
    }
}

fn hole_attribute(entity: &str, source: i64) -> (Vec<(i64, i64)>, AttributeName<'_>) {
    (
        vec![(source, -1)],
        AttributeName {
            kind: EntityKind::Face,
            entity,
            family: NameFamily::Hole,
            tail: vec![1, 7],
        },
    )
}

fn body_names<'m>() -> BodyNames<'m> {
    let mut names = BodyNames::default();
    for (pairs, attribute) in [hole_attribute(FACE, 945), hole_attribute(OTHER_FACE, 818)] {
        names.by_pairs.entry(pairs).or_default().push(attribute);
    }
    names
}

fn run(fixture: &Fixture, owner: u32) -> (Vec<PmiAnnotation>, Outcome) {
    let labels = [Label {
        owner,
        name: Some("Dimension".to_owned()),
        participants: Vec::new(),
    }];
    run_with(fixture, &labels, &Selections::default())
}

fn run_with(
    fixture: &Fixture,
    labels: &[Label],
    selections: &Selections,
) -> (Vec<PmiAnnotation>, Outcome) {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let segment = fixture.segment();
    let mut outcome = Outcome::default();
    let annotations = project_segment(
        &ctx,
        &segment,
        labels,
        (&fixture.parameters, selections),
        &body_names(),
        &mut outcome,
    )
    .expect("projection");
    (annotations, outcome)
}

fn has_note(outcome: &Outcome, code: InventorLossCode) -> bool {
    losses(outcome)
        .iter()
        .any(|note| note.code.local_code() == code.code())
}

fn dimension_of(annotation: &PmiAnnotation) -> &PmiDimension {
    match &annotation.definition {
        PmiDefinition::Dimension(dimension) => dimension,
        other => panic!("expected a dimension, got {other:?}"),
    }
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < VALUE_TOLERANCE
}

#[test]
fn diameter_transfers_symmetric_tolerance_basic_modifier_and_face() {
    let fixture = Fixture::dimension(6, 1, 0.015, -0.015);
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    let [annotation] = annotations.as_slice() else {
        panic!("one annotation, got {annotations:?}");
    };
    let dimension = dimension_of(annotation);
    assert!(close(
        dimension.nominal().expect("nominal").value.get(),
        35.0
    ));
    let Some(DimensionTolerance::PlusMinus { lower, upper }) = dimension.tolerance() else {
        panic!("plus-minus tolerance");
    };
    assert!(close(lower.value.get(), -0.15) && close(upper.value.get(), 0.15));
    assert_eq!(dimension.modifiers(), ["theoretical"]);
    assert_eq!(
        annotation.targets,
        [PmiTarget::Face {
            face: cadmpeg_ir::ids::FaceId::mint(FACE).expect("face id")
        }]
    );
    assert!(losses(&outcome).is_empty());
}

#[test]
fn display_two_marks_a_reference_dimension() {
    let fixture = Fixture::dimension(0, 2, 0.0, 0.0);
    let (annotations, _) = run(&fixture, OWNER_AT);
    assert_eq!(dimension_of(&annotations[0]).modifiers(), ["auxiliary"]);
}

#[test]
fn measure_of_another_owner_kind_stays_native() {
    let mut fixture = Fixture::dimension(6, 0, 0.015, -0.015);
    fixture.put(VALUE_AT, VALUE, value_record(2, 6, 0, 0.015, -0.015));
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(annotations.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
}

#[test]
fn reversed_limits_keep_the_dimension_without_tolerance() {
    let fixture = Fixture::dimension(3, 0, -0.02, 0.02);
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    let dimension = dimension_of(&annotations[0]);
    assert!(dimension.tolerance().is_none());
    assert!(close(
        dimension.nominal().expect("nominal").value.get(),
        35.0
    ));
    assert!(has_note(&outcome, InventorLossCode::PmiToleranceUnresolved));
}

#[test]
fn unknown_tolerance_code_keeps_the_dimension_without_tolerance() {
    let fixture = Fixture::dimension(5, 0, 0.02, 0.0);
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(dimension_of(&annotations[0]).tolerance().is_none());
    assert!(has_note(&outcome, InventorLossCode::PmiToleranceUnresolved));
}

#[test]
fn state_zero_face_selector_gives_no_target() {
    let mut fixture = Fixture::dimension(6, 0, 0.015, -0.015);
    fixture.put(SELECTOR_AT, FACE_SELECTOR, face_selector(3809, 0, KEY));
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(annotations[0].targets.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiTargetUnresolved));
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
}

#[test]
fn selectors_of_one_item_that_disagree_give_no_target() {
    let mut fixture = Fixture::dimension(6, 0, 0.015, -0.015);
    fixture.put(
        SECOND_SELECTOR_AT,
        FACE_SELECTOR,
        face_selector(3810, 2, SECOND_KEY),
    );
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(annotations[0].targets.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiTargetUnresolved));
}

#[test]
fn malformed_second_selector_withholds_the_item_target() {
    let mut fixture = Fixture::dimension(6, 0, 0.015, -0.015);
    let mut malformed = face_selector(3810, 2, KEY);
    malformed[SELECTOR_ENABLED_AT] = 0;
    fixture.put(SECOND_SELECTOR_AT, FACE_SELECTOR, malformed);
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(annotations[0].targets.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiTargetUnresolved));
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
}

#[test]
fn malformed_relation_is_charged_against_its_intent() {
    let mut fixture = Fixture::dimension(6, 0, 0.015, -0.015);
    let mut malformed = relation(INTENT_AT, &[ITEM_AT]);
    malformed.push(0);
    fixture.put(SECOND_RELATION_AT, SIZE_RELATION, malformed);
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert_eq!(annotations[0].targets.len(), 1);
    assert!(has_note(&outcome, InventorLossCode::PmiTargetUnresolved));
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
}

#[test]
fn every_intent_precedes_every_style() {
    let mut fixture = Fixture::dimension(6, 0, 0.015, -0.015);
    fixture.put(
        OWNER_AT,
        LINEAR_OWNER,
        linear_owner(&[qref(STYLE_AT), qref(INTENT_AT)]),
    );
    fixture.put(VALUE_AT, VALUE, value_record(0, 0, 0, 0.0, 0.0));
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(annotations.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
    fixture.put(
        OWNER_AT,
        LINEAR_OWNER,
        linear_owner(&[qref(INTENT_AT), qref(STYLE_AT)]),
    );
    let (annotations, _) = run(&fixture, OWNER_AT);
    assert_eq!(annotations.len(), 1);
}

#[test]
fn naming_table_refuses_a_repeated_identifier() {
    let bytes = naming_table_record(&[(7, 1), (7, 2)]);
    assert!(naming_table(View::over_retained(&bytes)).is_err());
    let bytes = naming_table_record(&[(7, 1), (8, 2)]);
    let table = naming_table(View::over_retained(&bytes)).expect("table");
    assert_eq!(table.get(&8), Some(&2));
}

#[test]
fn generated_name_refuses_zero_pairs_and_the_edge_form() {
    assert!(generated_name(NameFamily::Hole, View::over_retained(&hole_name(&[], 1, 7))).is_err());
    let mut edge_form = hole_name(&[(945, -1)], 1, 7);
    edge_form[NAME_FORM_AT] = 1;
    assert!(generated_name(NameFamily::Hole, View::over_retained(&edge_form)).is_err());
    let name = generated_name(
        NameFamily::Hole,
        View::over_retained(&hole_name(&[(1417, 5), (1344, -1)], 1, 7)),
    )
    .expect("name");
    assert_eq!(name.pairs, vec![(1417, 5), (1344, -1)]);
    assert_eq!(NameFamily::Hole.record_tail(&name.tail), Some(vec![1, 7]));
}

#[test]
fn edge_name_reads_the_empty_list_form_with_two_faces() {
    let faces = edge_name(View::over_retained(&edge_name_record(1, &[], &[17, 859])));
    assert_eq!(faces.expect("edge name"), Some([17, 859]));
    let listed = edge_name(View::over_retained(&edge_name_record(
        1,
        &[qref(3)],
        &[17, 859],
    )));
    assert_eq!(listed.expect("edge name"), None);
    let three = edge_name(View::over_retained(&edge_name_record(
        1,
        &[],
        &[17, 859, 4],
    )));
    assert_eq!(three.expect("edge name"), None);
    assert!(edge_name(View::over_retained(&edge_name_record(5, &[], &[17, 859]))).is_err());
}

#[test]
fn shared_edge_requires_exactly_one_common_edge() {
    let mut names = BodyNames::default();
    names
        .face_edges
        .insert(FACE, HashSet::from(["edge#1", "edge#2"]));
    names
        .face_edges
        .insert(OTHER_FACE, HashSet::from(["edge#2", "edge#3"]));
    names
        .face_edges
        .insert("third", HashSet::from(["edge#1", "edge#2"]));
    assert_eq!(names.shared_edge(FACE, OTHER_FACE), Some("edge#2"));
    assert_eq!(names.shared_edge(FACE, "third"), None);
}

#[test]
fn tolerance_forms_scale_internal_centimetres() {
    let value = |tolerance, upper, lower, hole_fit: &str| Value {
        measure: 1,
        tolerance,
        display: 0,
        upper,
        lower,
        hole_fit: hole_fit.to_owned(),
        shaft_fit: String::new(),
        nominal_parameter: 0,
    };
    let bounds = |tolerance: Option<&DimensionTolerance>| match tolerance {
        Some(
            DimensionTolerance::PlusMinus { lower, upper }
            | DimensionTolerance::Limits { lower, upper }
            | DimensionTolerance::PlusMinusFit { lower, upper, .. },
        ) => Some((lower.value.get(), upper.value.get())),
        _ => None,
    };
    let length = PmiQuantity::Length;
    let deviation = tolerance(&value(7, 0.01, -0.005, ""), 2.0, 10.0, length);
    assert!(bounds(deviation.bounds.as_ref())
        .is_some_and(|(lower, upper)| close(lower, -0.05) && close(upper, 0.1)));
    let limits = tolerance(&value(3, 0.02, -0.02, ""), 3.5, 10.0, length);
    assert!(bounds(limits.bounds.as_ref())
        .is_some_and(|(lower, upper)| close(lower, 34.8) && close(upper, 35.2)));
    let fitted = tolerance(&value(2, 0.0012, 0.0004, "G6"), 0.35, 10.0, length);
    assert!(matches!(
        fitted.bounds,
        Some(DimensionTolerance::PlusMinusFit { .. })
    ));
    let maximum = tolerance(&value(4, 0.0, 0.0, ""), 0.02, 10.0, length);
    assert!(maximum.resolved && maximum.bounds.is_none());
    assert_eq!(maximum.modifier, Some("maximum"));
    let unknown = tolerance(&value(5, 0.0, 0.0, ""), 1.0, 10.0, length);
    assert!(!unknown.resolved);
}

#[test]
fn fit_classes_split_letters_and_grade() {
    let hole = fit("G6", "").expect("hole class");
    assert_eq!(
        (
            hole.form_variance.as_str(),
            hole.zone_variance.as_str(),
            hole.grade.as_str()
        ),
        ("G", "hole", "6")
    );
    assert_eq!(fit("", "h7").expect("shaft class").zone_variance, "shaft");
    assert!(fit("H7", "h7").is_none());
    assert!(fit("g6", "").is_none());
}

// Record ordinals of the frame fixture.
const FRAME_OWNER_AT: u32 = 30;
const ROW_AT: u32 = 31;
const MAGNITUDE_AT: u32 = 32;
const ZERO_AT: u32 = 33;
const DATUM_FEATURE_AT: u32 = 34;
const DATUM_OWNER_AT: u32 = 35;
const SECOND_DATUM_FEATURE_AT: u32 = 36;
const SECOND_DATUM_OWNER_AT: u32 = 37;
const DATUM_FRAME_AT: u32 = 40;

fn tolerance_owner(source: u32, trailing: Option<u32>) -> Vec<u8> {
    let mut bytes = annotation_header(source);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, qref(PLANE_AT));
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    bytes.extend([0; 12]);
    if let Some(trailing) = trailing {
        push_u32(&mut bytes, trailing);
    }
    bytes
}

fn frame_row_record(characteristic: u32, datums: [u32; 3], zone: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 3);
    push_u16(&mut bytes, 0x32f0);
    push_u32(&mut bytes, characteristic);
    push_u32(&mut bytes, qref(MAGNITUDE_AT));
    for word in [zone, 0, 0] {
        push_u32(&mut bytes, word);
    }
    push_u32(&mut bytes, qref(ZERO_AT));
    push_u32(&mut bytes, qref(ZERO_AT));
    for datum in datums {
        push_u32(&mut bytes, datum);
    }
    bytes.extend([0; 36]);
    bytes
}

fn datum_feature_record(owner: u32, label: &str) -> Vec<u8> {
    let mut bytes = content_header(905);
    type_2_list(&mut bytes, &[qref(owner)]);
    type_2_list(&mut bytes, &[]);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_text(&mut bytes, label);
    bytes
}

fn frame_fixture(characteristic: u32, zone: u32) -> Fixture {
    let mut fixture = Fixture::default();
    fixture
        .put(PLANE_AT, PLANE, Vec::new())
        .put(MAGNITUDE_AT, PARAMETER, Vec::new())
        .put(ZERO_AT, PARAMETER, Vec::new())
        .put(
            FRAME_OWNER_AT,
            FRAME_OWNER,
            tolerance_owner(906, Some(ROW_AT + 1)),
        )
        .put(
            ROW_AT,
            FRAME_ROW,
            frame_row_record(
                characteristic,
                [qref(DATUM_FEATURE_AT), qref(SECOND_DATUM_FEATURE_AT), 0],
                zone,
            ),
        )
        .put(
            DATUM_FEATURE_AT,
            DATUM_FEATURE,
            datum_feature_record(DATUM_OWNER_AT, "A"),
        )
        .put(DATUM_OWNER_AT, DATUM_OWNER, tolerance_owner(907, None))
        .put(
            SECOND_DATUM_FEATURE_AT,
            DATUM_FEATURE,
            datum_feature_record(SECOND_DATUM_OWNER_AT, "B"),
        )
        .put(
            SECOND_DATUM_OWNER_AT,
            DATUM_OWNER,
            tolerance_owner(908, None),
        );
    fixture.parameters.insert(MAGNITUDE_AT, 0.075);
    fixture.parameters.insert(ZERO_AT, 0.0);
    fixture
}

#[test]
fn plain_frame_transfers_with_ordered_datum_system() {
    let fixture = frame_fixture(9, 0);
    let (annotations, outcome) = run(&fixture, FRAME_OWNER_AT);
    let labels: Vec<_> = annotations
        .iter()
        .filter_map(|annotation| match &annotation.definition {
            PmiDefinition::Datum { identification } => {
                Some((annotation.id.clone(), identification.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 2);
    let system = annotations
        .iter()
        .find_map(|annotation| match &annotation.definition {
            PmiDefinition::DatumSystem { references } => Some(references),
            _ => None,
        })
        .expect("datum system");
    let order: Vec<_> = system
        .as_slice()
        .iter()
        .map(|reference| {
            let label = labels
                .iter()
                .find(|(id, _)| *id == reference.datum)
                .map(|(_, label)| *label);
            (reference.precedence.get(), label)
        })
        .collect();
    assert_eq!(order, [(1, Some("A")), (2, Some("B"))]);
    let (kind, magnitude) = annotations
        .iter()
        .find_map(|annotation| match &annotation.definition {
            PmiDefinition::GeometricTolerance {
                tolerance,
                magnitude,
                ..
            } => Some((tolerance.clone(), magnitude.get().value.get())),
            _ => None,
        })
        .expect("geometric tolerance");
    assert_eq!(kind, GeometricToleranceKind::Position);
    assert!(close(magnitude, 0.75));
    assert_eq!(outcome.frames, 1);
}

#[test]
fn frame_with_zone_modifiers_stays_native() {
    let fixture = frame_fixture(9, 23);
    let (annotations, outcome) = run(&fixture, FRAME_OWNER_AT);
    assert!(!annotations.iter().any(|annotation| matches!(
        annotation.definition,
        PmiDefinition::GeometricTolerance { .. }
    )));
    assert!(has_note(&outcome, InventorLossCode::PmiFrameNative));
}

#[test]
fn frame_with_unestablished_characteristic_stays_native() {
    let fixture = frame_fixture(4, 0);
    let (_, outcome) = run(&fixture, FRAME_OWNER_AT);
    assert_eq!(outcome.frames, 0);
    assert!(has_note(&outcome, InventorLossCode::PmiFrameNative));
}

#[test]
fn version_28_frame_reference_withholds_the_datum_system() {
    let mut row = frame_row_record(9, [0, 0, 0], 0);
    row.extend([0; 16]);
    push_u32(&mut row, qref(DATUM_FRAME_AT));
    let mut fixture = frame_fixture(9, 0);
    fixture.put(DATUM_FRAME_AT, DATUM_FRAME, Vec::new());
    let mut segment = fixture.segment();
    segment.version = 28;
    let frame = frame_row(&segment, View::over_retained(&row)).expect("version-28 row");
    assert_eq!(frame.frame_reference, Some(DATUM_FRAME_AT));
    assert!(frame.datums.is_empty());
    drop(segment);
    fixture.put(ROW_AT, FRAME_ROW, row);
    let mut owner = annotation_header_v28(906);
    push_u32(&mut owner, 0);
    push_u32(&mut owner, qref(PLANE_AT));
    push_u32(&mut owner, 1);
    push_u32(&mut owner, 0);
    owner.extend([0; 12]);
    push_u32(&mut owner, ROW_AT + 1);
    fixture.put(FRAME_OWNER_AT, FRAME_OWNER, owner);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut segment = fixture.segment();
    segment.version = 28;
    let labels = [Label {
        owner: FRAME_OWNER_AT,
        name: None,
        participants: Vec::new(),
    }];
    let mut outcome = Outcome::default();
    let annotations = project_segment(
        &ctx,
        &segment,
        &labels,
        (&fixture.parameters, &Selections::default()),
        &body_names(),
        &mut outcome,
    )
    .expect("projection");
    let datum_system = annotations
        .iter()
        .find_map(|annotation| match &annotation.definition {
            PmiDefinition::GeometricTolerance { datum_system, .. } => Some(datum_system.clone()),
            _ => None,
        })
        .expect("geometric tolerance");
    assert_eq!(datum_system, None);
    assert!(has_note(
        &outcome,
        InventorLossCode::PmiDatumSystemUnresolved
    ));
}

/// An annotation header of segment major version 28.
fn annotation_header_v28(source: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 3);
    push_u16(&mut bytes, 0x1234);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0x200);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, source);
    push_i32(&mut bytes, -1);
    bytes
}

#[test]
fn frame_row_length_follows_its_version() {
    let fixture = frame_fixture(9, 0);
    let segment = fixture.segment();
    let mut long = frame_row_record(9, [0, 0, 0], 0);
    long.extend([0; 20]);
    assert!(frame_row(&segment, View::over_retained(&long)).is_err());
}

#[test]
fn frame_row_reference_is_unqualified() {
    let unqualified = tolerance_owner(906, Some(ROW_AT + 1));
    assert_eq!(
        trailing_reference(View::over_retained(&unqualified)),
        Some(ROW_AT)
    );
    let qualified = tolerance_owner(906, Some(qref(ROW_AT)));
    assert_eq!(trailing_reference(View::over_retained(&qualified)), None);
}

fn hole_value(flags: [u32; 4], text: &str) -> Vec<u8> {
    let mut bytes = value_record(1, 0, 0, 0.0, 0.0);
    for _ in 0..4 {
        push_u32(&mut bytes, qref(REFERENCE_AT));
    }
    for flag in flags {
        push_u32(&mut bytes, flag);
    }
    push_text(&mut bytes, text);
    push_u32(&mut bytes, 4);
    bytes.extend([0; 12]);
    bytes.extend(0.0_f64.to_le_bytes());
    bytes.extend((-0.02_f64).to_le_bytes());
    bytes
}

fn hole_fixture(flags: [u32; 4], text: &str) -> Fixture {
    let mut fixture = Fixture::dimension(0, 0, 0.0, 0.0);
    let mut owner = annotation_header(909);
    for word in [
        0,
        qref(PLANE_AT),
        1,
        0,
        0,
        qref(LAYOUT_AT),
        qref(VALUE_AT),
        qref(INTENT_AT),
        qref(STYLE_AT),
        qref(SECOND_STYLE_AT),
    ] {
        push_u32(&mut owner, word);
    }
    push_u16(&mut owner, 0x0401);
    push_u32(&mut owner, 0);
    fixture.put(OWNER_AT, HOLE_NOTE_OWNER, owner).put(
        VALUE_AT,
        HOLE_VALUE,
        hole_value(flags, text),
    );
    fixture
}

const PLAIN_FLAGS: [u32; 4] = [1, 0, 0x0100_0000, 0];

#[test]
fn plain_hole_note_transfers_its_diameter_deviations() {
    let fixture = hole_fixture(PLAIN_FLAGS, "\\F2;n\\F1;<HDIA> THRU");
    let (annotations, _) = run(&fixture, OWNER_AT);
    let dimension = dimension_of(&annotations[0]);
    let Some(DimensionTolerance::PlusMinus { lower, upper }) = dimension.tolerance() else {
        panic!("deviation tolerance");
    };
    assert!(close(lower.value.get(), -0.2) && close(upper.value.get(), 0.0));
}

#[test]
fn hole_note_outside_the_plain_form_stays_native() {
    let fixture = hole_fixture([1, 0, 0, 0], "\\F2;n\\F1;<HDIA> THRU");
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(annotations.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
    let fixture = hole_fixture(PLAIN_FLAGS, "<THDCD> THRU");
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(annotations.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
}

#[test]
fn every_loss_variant_has_its_note() {
    for (outcome, code) in [
        (
            Outcome {
                unresolved_targets: 1,
                ..Outcome::default()
            },
            InventorLossCode::PmiTargetUnresolved,
        ),
        (
            Outcome {
                tolerance_unresolved: 1,
                ..Outcome::default()
            },
            InventorLossCode::PmiToleranceUnresolved,
        ),
        (
            Outcome {
                frames_native: 1,
                ..Outcome::default()
            },
            InventorLossCode::PmiFrameNative,
        ),
        (
            Outcome {
                datum_systems_unresolved: 1,
                ..Outcome::default()
            },
            InventorLossCode::PmiDatumSystemUnresolved,
        ),
        (
            Outcome {
                malformed: 1,
                ..Outcome::default()
            },
            InventorLossCode::PmiRecordMalformed,
        ),
    ] {
        let notes = losses(&outcome);
        assert_eq!(notes.len(), 1, "{code:?}");
        assert_eq!(notes[0].code.local_code(), code.code());
    }
}

#[test]
fn face_selector_topology_must_match_its_body_source_index() {
    let mut fixture = Fixture::dimension(6, 0, 0.015, -0.015);
    fixture.put(BODY_AT, BODY, state_header(78));
    let (annotations, outcome) = run(&fixture, OWNER_AT);
    assert!(annotations[0].targets.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiTargetUnresolved));
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
}

/// An edge selector in the edge-naming grammar of section 13 that names
/// topology `77` of the body.
fn edge_selector_record() -> Vec<u8> {
    let mut bytes = state_header(3811);
    push_i32(&mut bytes, -1);
    bytes.push(1);
    bytes.push(5);
    push_u32(&mut bytes, qref(BODY_AT));
    push_u32(&mut bytes, 77);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 77);
    for _ in 0..2 {
        push_u16(&mut bytes, 8);
        push_u16(&mut bytes, 0x3000);
        push_u32(&mut bytes, 0);
    }
    bytes.push(0);
    push_u32(&mut bytes, qref(ITEM_AT));
    push_u32(&mut bytes, qref(TABLE_AT));
    push_u32(&mut bytes, KEY);
    bytes.push(0);
    bytes
}

#[test]
fn edge_selector_topology_must_match_its_body_source_index() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let payload = edge_selector_record();
    let mut fixture = Fixture::dimension(6, 0, 0.015, -0.015);
    let selector = edge_selector(&ctx, &fixture.segment(), View::over_retained(&payload))
        .expect("edge selector");
    assert_eq!(
        (selector.source, selector.item, selector.key),
        (3811, ITEM_AT, KEY)
    );
    fixture.put(BODY_AT, BODY, state_header(78));
    assert!(edge_selector(&ctx, &fixture.segment(), View::over_retained(&payload)).is_err());
}

const FEATURE_AT: u32 = 50;
const FEATURE_LABEL_AT: u32 = 51;
const PARAMETER_TABLE_AT: u32 = 52;
const ATTACHMENT_AT: u32 = 53;
const COLLECTION_AT: u32 = 54;
const FACE_ITEM_AT: u32 = 55;
const FEATURE_SELECTOR_AT: u32 = 56;
const MEMBER_AT: u32 = 57;
const SECOND_FEATURE_AT: u32 = 58;
const DEFINITION_AT: u32 = 59;
const SECOND_ATTACHMENT_AT: u32 = 60;
const FEATURE_SOURCE: u32 = 4000;

/// The fields of a tolerance feature of section 13.11.
#[derive(Clone)]
struct FeatureRecord {
    flags: u32,
    next: u32,
    context: u32,
    members: Vec<u32>,
    collection: u32,
    definition: u32,
}

impl FeatureRecord {
    fn new(members: &[u32], collection: Option<u32>) -> Self {
        Self {
            flags: 0x200,
            next: qref(FEATURE_LABEL_AT),
            context: qref(PARAMETER_TABLE_AT),
            members: members.iter().map(|member| qref(*member)).collect(),
            collection: collection.map_or(0, qref),
            definition: 0,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 3);
        push_u16(&mut bytes, 0x1234);
        push_u32(&mut bytes, self.next);
        push_u32(&mut bytes, self.flags);
        push_u32(&mut bytes, self.context);
        push_u32(&mut bytes, 905);
        push_i32(&mut bytes, -1);
        type_2_list(&mut bytes, &self.members);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_text(&mut bytes, "Feature");
        push_u32(&mut bytes, self.collection);
        push_u32(&mut bytes, self.definition);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        bytes
    }
}

/// A face selector of a tolerance feature whose face item is `FACE_ITEM_AT`.
fn feature_face_selector() -> Vec<u8> {
    let mut bytes = face_selector(FEATURE_SOURCE, 2, KEY);
    let item_at = bytes.len() - 4;
    bytes[item_at..].copy_from_slice(&qref(FACE_ITEM_AT).to_le_bytes());
    bytes
}

/// The frame of `frame_fixture`, the records that a tolerance feature at
/// `FEATURE_AT` references, and the naming records of its face selector.
fn feature_fixture(top: &FeatureRecord) -> Fixture {
    let mut fixture = frame_fixture(9, 0);
    fixture
        .put(FEATURE_AT, TOLERANCE_FEATURE, top.bytes())
        .put(
            FEATURE_LABEL_AT,
            crate::feature::FEATURE_LABEL_TYPE,
            Vec::new(),
        )
        .put(
            PARAMETER_TABLE_AT,
            crate::design::scope::PARAMETER_TABLE_TYPE,
            Vec::new(),
        )
        .put(ATTACHMENT_AT, ATTACHMENT, Vec::new())
        .put(
            COLLECTION_AT,
            crate::feature::FACE_COLLECTION_TYPE,
            Vec::new(),
        )
        .put(FACE_ITEM_AT, crate::feature::FACE_ITEM_TYPE, Vec::new())
        .put(
            FEATURE_SELECTOR_AT,
            FEATURE_FACE_SELECTOR,
            feature_face_selector(),
        )
        .put(DEFINITION_AT, DATUM_DEFINITION, Vec::new())
        .put(
            TABLE_AT,
            NAMING_TABLE,
            naming_table_record(&[(KEY, NAME_AT)]),
        )
        .put(BODY_AT, BODY, state_header(77))
        .put(NAME_AT, HOLE_NAME, hole_name(&[(945, -1)], 1, 7));
    fixture
}

fn feature_selections() -> Selections {
    Selections {
        collections: HashMap::from([(COLLECTION_AT, vec![FACE_ITEM_AT])]),
        face_items: HashMap::from([(FACE_ITEM_AT, vec![FEATURE_SOURCE])]),
    }
}

/// The frame label and one label per tolerance feature that lists the frame
/// owner as a participant.
fn feature_labels(features: &[u32]) -> Vec<Label> {
    let mut labels = vec![Label {
        owner: FRAME_OWNER_AT,
        name: Some("Frame".to_owned()),
        participants: Vec::new(),
    }];
    for feature in features {
        labels.push(Label {
            owner: *feature,
            name: Some("Feature".to_owned()),
            participants: vec![FRAME_OWNER_AT],
        });
    }
    labels
}

/// The targets of the frame and the outcome of projecting `fixture` with
/// the labels of `features`.
fn run_features(fixture: &Fixture, features: &[u32]) -> (Vec<PmiTarget>, Outcome) {
    let (annotations, outcome) =
        run_with(fixture, &feature_labels(features), &feature_selections());
    let targets = annotations
        .into_iter()
        .find(|annotation| {
            matches!(
                annotation.definition,
                PmiDefinition::GeometricTolerance { .. }
            )
        })
        .expect("geometric tolerance")
        .targets;
    (targets, outcome)
}

fn face_target() -> PmiTarget {
    PmiTarget::Face {
        face: cadmpeg_ir::ids::FaceId::mint(FACE).expect("face id"),
    }
}

/// The outcome of a frame whose single tolerance feature targets `FACE`.
fn resolved_feature_outcome() -> Outcome {
    let fixture = feature_fixture(&FeatureRecord::new(&[ATTACHMENT_AT], Some(COLLECTION_AT)));
    let (targets, outcome) = run_features(&fixture, &[FEATURE_AT]);
    assert_eq!(targets, [face_target()]);
    outcome
}

#[test]
fn tolerance_feature_faces_target_an_owner_without_intent_targets() {
    let outcome = resolved_feature_outcome();
    assert_eq!(outcome.malformed, 0);
}

#[test]
fn member_tolerance_feature_faces_target_the_owner() {
    let mut fixture = feature_fixture(&FeatureRecord::new(&[ATTACHMENT_AT, MEMBER_AT], None));
    fixture.put(
        MEMBER_AT,
        TOLERANCE_FEATURE,
        FeatureRecord::new(&[], Some(COLLECTION_AT)).bytes(),
    );
    let (targets, outcome) = run_features(&fixture, &[FEATURE_AT]);
    assert_eq!(targets, [face_target()]);
    assert_eq!(outcome.malformed, 0);
}

#[test]
fn member_tolerance_feature_reached_from_its_own_label_targets_its_faces() {
    let mut fixture = feature_fixture(&FeatureRecord::new(&[ATTACHMENT_AT, MEMBER_AT], None));
    fixture.put(
        MEMBER_AT,
        TOLERANCE_FEATURE,
        FeatureRecord::new(&[], Some(COLLECTION_AT)).bytes(),
    );
    let (targets, outcome) = run_features(&fixture, &[MEMBER_AT]);
    assert_eq!(targets, [face_target()]);
    assert_eq!(outcome.malformed, 0);
}

#[test]
fn top_tolerance_feature_without_attachments_gives_no_target() {
    let fixture = feature_fixture(&FeatureRecord::new(&[], Some(COLLECTION_AT)));
    let (targets, outcome) = run_features(&fixture, &[FEATURE_AT]);
    assert!(targets.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
}

#[test]
fn top_tolerance_feature_with_two_attachments_targets_its_faces() {
    let mut fixture = feature_fixture(&FeatureRecord::new(
        &[ATTACHMENT_AT, SECOND_ATTACHMENT_AT],
        Some(COLLECTION_AT),
    ));
    fixture.put(SECOND_ATTACHMENT_AT, ATTACHMENT, Vec::new());
    let (targets, outcome) = run_features(&fixture, &[FEATURE_AT]);
    assert_eq!(targets, [face_target()]);
    assert_eq!(outcome.malformed, 0);
}

#[test]
fn member_tolerance_feature_with_an_attachment_gives_no_target() {
    let mut fixture = feature_fixture(&FeatureRecord::new(&[ATTACHMENT_AT, MEMBER_AT], None));
    fixture.put(
        MEMBER_AT,
        TOLERANCE_FEATURE,
        FeatureRecord::new(&[ATTACHMENT_AT], Some(COLLECTION_AT)).bytes(),
    );
    let (targets, outcome) = run_features(&fixture, &[FEATURE_AT]);
    assert!(targets.is_empty());
    assert!(has_note(&outcome, InventorLossCode::PmiRecordMalformed));
}

#[test]
fn owner_listed_by_two_tolerance_features_gives_no_target() {
    let top = FeatureRecord::new(&[ATTACHMENT_AT], Some(COLLECTION_AT));
    let mut fixture = feature_fixture(&top);
    fixture.put(SECOND_FEATURE_AT, TOLERANCE_FEATURE, top.bytes());
    let (targets, outcome) = run_features(&fixture, &[FEATURE_AT, SECOND_FEATURE_AT]);
    assert!(targets.is_empty());
    assert_eq!(
        outcome.unresolved_targets,
        resolved_feature_outcome().unresolved_targets + 1
    );
    assert!(has_note(&outcome, InventorLossCode::PmiTargetUnresolved));
}

#[test]
fn tolerance_feature_refuses_each_departure_from_its_grammar() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let base = FeatureRecord {
        definition: qref(DEFINITION_AT),
        ..FeatureRecord::new(&[ATTACHMENT_AT, MEMBER_AT], Some(COLLECTION_AT))
    };
    let mut fixture = feature_fixture(&base);
    fixture.put(
        MEMBER_AT,
        TOLERANCE_FEATURE,
        FeatureRecord::new(&[], None).bytes(),
    );
    let segment = fixture.segment();
    let parse = |bytes: &[u8]| tolerance_feature(&ctx, &segment, View::over_retained(bytes));
    let parsed = parse(&base.bytes()).expect("tolerance feature");
    assert_eq!(parsed.members, [MEMBER_AT]);
    assert_eq!(parsed.attachments, 1);
    assert_eq!(parsed.face_collection, Some(COLLECTION_AT));
    let departures = [
        (
            "flags",
            FeatureRecord {
                flags: 0x201,
                ..base.clone()
            },
        ),
        (
            "next type",
            FeatureRecord {
                next: qref(PARAMETER_TABLE_AT),
                ..base.clone()
            },
        ),
        (
            "next unqualified",
            FeatureRecord {
                next: FEATURE_LABEL_AT + 1,
                ..base.clone()
            },
        ),
        (
            "context type",
            FeatureRecord {
                context: qref(FEATURE_LABEL_AT),
                ..base.clone()
            },
        ),
        (
            "member type",
            FeatureRecord {
                members: vec![qref(COLLECTION_AT)],
                ..base.clone()
            },
        ),
        (
            "collection type",
            FeatureRecord {
                collection: qref(FACE_ITEM_AT),
                ..base.clone()
            },
        ),
        (
            "definition type",
            FeatureRecord {
                definition: qref(COLLECTION_AT),
                ..base.clone()
            },
        ),
    ];
    for (departure, record) in departures {
        assert!(parse(&record.bytes()).is_err(), "{departure}");
    }
    let bytes = base.bytes();
    assert!(
        parse(&bytes[..bytes.len() - 1]).is_err(),
        "truncated controls"
    );
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(parse(&trailing).is_err(), "trailing bytes");
}
