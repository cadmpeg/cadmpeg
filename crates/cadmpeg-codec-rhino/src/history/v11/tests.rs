// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use std::collections::HashMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::NativeNameValue;
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::refs::{EntityRef, Member, Position};
use cadmpeg_ir::design::selection::{Anchor, PickTarget, Picks};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::EditableDecodeResult;

use super::super::{HistoryRecord, HistoryValue, RecordType, Value, ANONYMOUS, HISTORY_CLASS};
use super::{project, ObjectBodies};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    anonymous_chunk, class_wrapper, crc_chunk, crc_table, fixed_attributes, long_chunk,
    minimal_document, nested_crc_chunk, point_payload, short_chunk, table, POINT_CLASS,
};
use crate::wire::Uuid;

const ARCHIVE: ArchiveVersion = ArchiveVersion::V5;

fn id(value: u8) -> Uuid {
    let mut bytes = [0; 16];
    bytes[15] = value;
    Uuid::from_canonical(bytes)
}

fn point_object(object: Uuid, point: [f64; 3]) -> Vec<u8> {
    let object_type = short_chunk(ARCHIVE, 0x8200_0071, 1);
    let mut uuid_body = POINT_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&POINT_CLASS).to_le_bytes());
    let class_uuid = long_chunk(ARCHIVE, 0x0002_fffb, &uuid_body);
    let class_data = crc_chunk(ARCHIVE, 0x0002_fffc, &point_payload(point));
    let class_end = short_chunk(ARCHIVE, 0x8002_7fff, 0);
    let class = long_chunk(
        ARCHIVE,
        0x0002_7ffa,
        &[class_uuid, class_data, class_end].concat(),
    );
    let mut attributes = fixed_attributes(0, 0, Some(true));
    attributes[1..17].copy_from_slice(&object.to_wire());
    let attributes = crc_chunk(ARCHIVE, 0x0200_8072, &attributes);
    let object_end = short_chunk(ARCHIVE, 0x8200_007f, 0);
    nested_crc_chunk(
        ARCHIVE,
        0x2000_8070 | crate::chunks::TCODE_CRC,
        &[object_type, class, attributes, object_end].concat(),
    )
}

fn versioned(minor: i32, body: &[u8]) -> Vec<u8> {
    let mut payload = 1_i32.to_le_bytes().to_vec();
    payload.extend(minor.to_le_bytes());
    payload.extend(body);
    crc_chunk(ARCHIVE, ANONYMOUS, &payload)
}

fn uuid_list(ids: &[Uuid]) -> Vec<u8> {
    let mut body = i32::try_from(ids.len())
        .expect("fixture count")
        .to_le_bytes()
        .to_vec();
    for id in ids {
        body.extend(id.to_wire());
    }
    anonymous_chunk(ARCHIVE, 0, &body)
}

fn integers_value(value_id: i32, values: &[i32]) -> Vec<u8> {
    let mut body = 2_i32.to_le_bytes().to_vec();
    body.extend(value_id.to_le_bytes());
    body.extend(i32::try_from(values.len()).expect("count").to_le_bytes());
    for value in values {
        body.extend(value.to_le_bytes());
    }
    versioned(0, &body)
}

fn history_record(
    record: Uuid,
    command: Uuid,
    descendants: &[Uuid],
    values: &[Vec<u8>],
) -> Vec<u8> {
    let mut values_body = i32::try_from(values.len())
        .expect("fixture count")
        .to_le_bytes()
        .to_vec();
    for value in values {
        values_body.extend(value);
    }
    let mut body = record.to_wire().to_vec();
    body.extend(1_i32.to_le_bytes());
    body.extend(command.to_wire());
    body.extend(uuid_list(descendants));
    body.extend(uuid_list(&[]));
    body.extend(anonymous_chunk(ARCHIVE, 0, &values_body));
    body.extend(1_i32.to_le_bytes());
    body.push(0);
    let class = class_wrapper(ARCHIVE, HISTORY_CLASS.to_wire(), &versioned(2, &body));
    crc_chunk(ARCHIVE, 0x2000_807b, &class)
}

fn units() -> Vec<u8> {
    let mut body = 100_i32.to_le_bytes().to_vec();
    body.extend(2_i32.to_le_bytes());
    body.extend(0.01_f64.to_le_bytes());
    body.extend(0.1_f64.to_le_bytes());
    body.extend(0.001_f64.to_le_bytes());
    crc_chunk(ARCHIVE, 0x2000_8031, &body)
}

fn decode(objects: &[Vec<u8>], history: &[Vec<u8>]) -> EditableDecodeResult {
    let bytes = minimal_document(
        "50",
        &[
            table(ARCHIVE, 0x1000_0014, &[]),
            table(ARCHIVE, 0x1000_0015, &[units()]),
            table(ARCHIVE, 0x1000_0013, objects),
            crc_table(ARCHIVE, 0x1000_0026, history),
        ],
    );
    EditableDecodeResult::from(
        crate::RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("history document"),
    )
}

fn bodies_of(ir: &CadIr, object: Uuid) -> Vec<BodyId> {
    let key = object.to_string();
    ir.geometry
        .bodies
        .iter()
        .filter(|body| body.id.as_str().contains(&key))
        .map(|body| body.id.clone())
        .collect()
}

fn design_errors(ir: &CadIr) -> Vec<String> {
    cadmpeg_ir::validate::validate_neutral(ir, Vec::new())
        .expect("validation")
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design && finding.severity == Severity::Error)
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn a_document_without_history_states_no_design() {
    let result = decode(&[point_object(id(0x40), [1.0, 2.0, 3.0])], &[]);
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.containers.is_empty());
    assert!(ir.design.features.is_empty());
    let [root] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert!(root.is_root());
    assert_eq!(root.bodies, bodies_of(ir, id(0x40)));
}

#[test]
fn history_records_are_native_members_of_one_group_holding_their_descendants() {
    let produced = id(0x40);
    let plain = id(0x41);
    let result = decode(
        &[
            point_object(produced, [1.0, 2.0, 3.0]),
            point_object(plain, [4.0, 5.0, 6.0]),
        ],
        &[history_record(
            id(1),
            id(2),
            &[produced],
            &[integers_value(9, &[2]), integers_value(100, &[1, 0])],
        )],
    );
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert_eq!(ir.model.features.len(), 1, "the v10 feature is kept");

    let [root] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert!(root.is_root());
    assert!(root.bodies.is_empty());

    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one design scope");
    };
    assert_eq!(scope.definition, root.id);
    let [Container::Unordered {
        id: group,
        kind: UnorderedKind::Group,
        members,
    }] = ir.design.containers.as_slice()
    else {
        panic!("one unordered group");
    };
    assert_eq!(scope.roots, std::slice::from_ref(group));

    let [feature] = ir.design.features.as_slice() else {
        panic!("one feature");
    };
    assert_eq!(feature.id, ir.model.features[0].id);
    assert_eq!(feature.native_ref, ir.model.features[0].native_ref);
    assert_eq!(feature.suppressed, None);
    assert!(members.contains(&Member::Feature(feature.id.clone())));
    let FeatureOperation::Native {
        kind,
        native_options,
        source_settings,
    } = &feature.definition.operation
    else {
        panic!("a native operation");
    };
    assert_eq!(kind.as_str(), format!("rhino:{}", id(2)));
    assert!(source_settings.is_empty());
    let option = |key: &str| {
        native_options
            .iter()
            .find(|(name, _)| name.as_str() == key)
            .map(|(_, value)| value.clone())
    };
    let integer = |value| {
        Some(NativeOption::Integer(
            cadmpeg_ir::design::dimension::Dimension::Literal(value),
        ))
    };
    assert_eq!(option("rhino:value_9"), integer(2));
    assert_eq!(option("rhino:value_100.0"), integer(1));
    assert_eq!(option("rhino:value_100.1"), integer(0));

    let [current] = scope.current.as_slice() else {
        panic!("one current state");
    };
    assert_eq!(
        current.at,
        Position::After(Member::Feature(feature.id.clone()))
    );
    let produced_bodies = bodies_of(ir, produced);
    assert_eq!(produced_bodies.len(), 1);
    assert_eq!(current.bodies, produced_bodies);
    assert!(bodies_of(ir, plain)
        .iter()
        .all(|body| !current.bodies.contains(body)));

    assert_eq!(design_errors(ir), Vec::<String>::new());
}

fn record(record: u8, descendants: &[u8]) -> HistoryRecord {
    record_with_antecedents(record, descendants, &[])
}

fn record_with_antecedents(record: u8, descendants: &[u8], antecedents: &[u8]) -> HistoryRecord {
    HistoryRecord {
        source_range: usize::from(record)..usize::from(record) + 1,
        id: id(record),
        version: 1,
        command_id: id(0x20),
        descendants: descendants.iter().copied().map(id).collect(),
        antecedents: antecedents.iter().copied().map(id).collect(),
        values: vec![
            HistoryValue {
                id: 3,
                value: Value::Doubles(vec![2.5, f64::NAN]),
            },
            HistoryValue {
                id: 3,
                value: Value::Strings(vec![" ".to_owned()]),
            },
            HistoryValue {
                id: 4,
                value: Value::Booleans(vec![true]),
            },
        ],
        record_type: RecordType::FeatureParameters,
        copy_on_replace: false,
    }
}

fn body(key: u8) -> BodyId {
    BodyId::mint(format!("rhino:object:body#{key}")).expect("body id")
}

#[test]
fn an_object_two_records_list_is_in_neither_current_state() {
    let objects: ObjectBodies =
        HashMap::from([(id(0x40), vec![body(0x40)]), (id(0x41), vec![body(0x41)])]);
    let records = [record(1, &[0x40, 0x41, 0x41]), record(2, &[0x40])];
    let mut ir = CadIr::empty();
    project(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &records,
        &objects,
        None,
        &mut ir,
    )
    .expect("projection");
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope");
    };
    let bodies = scope
        .current
        .iter()
        .map(|current| current.bodies.clone())
        .collect::<Vec<_>>();
    assert_eq!(bodies, [vec![body(0x41)], Vec::new()]);
    assert_eq!(ir.model.product_definitions.len(), 1);
}

/// A pick's native name and its resolved entities.
type PickSummary = (String, Option<Vec<EntityRef>>);

fn selection(options: &NativeOptions, key: &str) -> (Anchor, Vec<PickSummary>) {
    let Some((_, NativeOption::Selection(selection))) =
        options.iter().find(|(name, _)| name.as_str() == key)
    else {
        panic!("selection {key}");
    };
    let Picks::Named(picks) = &selection.picks else {
        panic!("named picks");
    };
    let picks = picks
        .as_slice()
        .iter()
        .map(|pick| {
            assert_eq!(pick.target, PickTarget::Native { within: None });
            let native = pick.native.as_ref().expect("native name");
            assert_eq!(native.codec.as_str(), "rhino");
            let NativeNameValue::Text(text) = &native.value else {
                panic!("text name");
            };
            (text.as_str().to_owned(), pick.resolved.clone())
        })
        .collect();
    (selection.at.clone(), picks)
}

#[test]
fn antecedents_are_picks_anchored_after_their_producer_or_current() {
    let objects: ObjectBodies = HashMap::from([
        (id(0x40), vec![body(0x40)]),
        (id(0x41), vec![body(0x41)]),
        (id(0x50), vec![body(0x50)]),
    ]);
    let records = [
        record(1, &[0x40]),
        record_with_antecedents(2, &[0x41], &[0x40, 0x50, 0x41, 0x40]),
    ];
    let mut ir = CadIr::empty();
    project(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &records,
        &objects,
        None,
        &mut ir,
    )
    .expect("projection");
    let producer = ir.design.features[0].id.clone();
    let FeatureOperation::Native { native_options, .. } =
        &ir.design.features[1].definition.operation
    else {
        panic!("a native operation");
    };
    let keys = native_options
        .keys()
        .map(|key| key.as_str().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "rhino:antecedents".to_owned(),
            format!("rhino:antecedents.{}", id(1)),
            "rhino:value_3.0".to_owned(),
            "rhino:value_4".to_owned(),
        ]
    );
    assert_eq!(
        selection(native_options, &format!("rhino:antecedents.{}", id(1))),
        (
            Anchor::Pinned(Position::After(Member::Feature(producer))),
            vec![(
                id(0x40).to_string(),
                Some(vec![EntityRef::Body(body(0x40))])
            )],
        )
    );
    let (at, mut picks) = selection(native_options, "rhino:antecedents");
    picks.sort();
    assert_eq!(at, Anchor::Current);
    assert_eq!(
        picks,
        [
            (
                id(0x41).to_string(),
                Some(vec![EntityRef::Body(body(0x41))])
            ),
            (id(0x50).to_string(), None),
        ]
    );
}

#[test]
fn options_skip_values_no_option_can_hold() {
    let mut ir = CadIr::empty();
    project(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &[record(1, &[])],
        &ObjectBodies::new(),
        None,
        &mut ir,
    )
    .expect("projection");
    let FeatureOperation::Native { native_options, .. } =
        &ir.design.features[0].definition.operation
    else {
        panic!("a native operation");
    };
    let keys = native_options
        .keys()
        .map(|key| key.as_str().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(keys, ["rhino:value_3.0", "rhino:value_4"]);
}

#[test]
fn every_option_key_is_rhino_codec_name() {
    let records = [
        record(1, &[0x40]),
        record_with_antecedents(2, &[0x41], &[0x40, 0x50]),
    ];
    let mut ir = CadIr::empty();
    project(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &records,
        &ObjectBodies::from([(id(0x40), vec![body(0x40)])]),
        None,
        &mut ir,
    )
    .expect("projection");
    let keys = ir
        .design
        .features
        .iter()
        .flat_map(|feature| {
            let FeatureOperation::Native { native_options, .. } = &feature.definition.operation
            else {
                panic!("a native operation");
            };
            native_options.keys().map(|key| key.as_str().to_owned())
        })
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 6);
    for key in keys {
        let name = key.strip_prefix("rhino:");
        assert!(
            name.is_some_and(|name| !name.is_empty()),
            "{key:?} is not rhino:<name>"
        );
    }
}
