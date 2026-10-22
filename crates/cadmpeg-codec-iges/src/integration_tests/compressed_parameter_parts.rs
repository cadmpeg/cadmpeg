// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::NativeUnknownRecord;

#[test]
fn compressed_parameter_type_failure_has_one_actual_data_home_and_an_unchanged_peer() {
    let original = crate::test_support::compressed_parts::points(false, false);
    let baseline = crate::test_support::decode(original.clone());
    let first = original
        .windows(3)
        .position(|bytes| bytes == b"D1@")
        .expect("D1 source");
    let second = original
        .windows(3)
        .position(|bytes| bytes == b"D3@")
        .expect("D3 source");
    let token = original
        .windows(b"116,1.0,2.0,3.0;".len())
        .position(|bytes| bytes == b"116,1.0,2.0,3.0;")
        .expect("first Parameter source");
    let mut bytes = original;
    bytes[token..token + 3].copy_from_slice(b"110");
    let decoded = crate::test_support::decode(bytes.clone());
    let point_id = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    let vertex_id = crate::ids::vertex(&crate::ids::Stem::directory(3_u32));
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    assert_eq!(decoded.ir().model.points[0].id, point_id);
    assert_eq!(decoded.ir().model.vertices[0].id, vertex_id);
    let expected = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == point_id)
        .expect("baseline peer point");
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).expect("point"),
        serde_json::to_value(expected).expect("baseline point")
    );
    let expected = baseline
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == vertex_id)
        .expect("baseline peer vertex");
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.vertices[0]).expect("vertex"),
        serde_json::to_value(expected).expect("baseline vertex")
    );
    let native = decoded.ir().native.namespace("iges").expect("IGES native");
    let old = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("baseline native");
    for arena in ["entities", "display_attributes"] {
        assert_eq!(native.arenas()[arena].len(), 1);
        assert_eq!(native.arenas()[arena][0], old.arenas()[arena][1]);
    }
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let unknowns = native
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("typed unknowns");
    assert_eq!(unknowns.len(), 1);
    let fidelity = decoded.source_fidelity();
    let id = unknowns[0].id.as_str();
    let part = &fidelity.parts()[id];
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(part.level().as_str(), "iges.compressed_data_record");
    assert_eq!(part.offset(), u64::try_from(first).expect("D1 start"));
    assert_eq!(
        part.byte_len(),
        u64::try_from(second - first).expect("D1 length")
    );
    assert_eq!(
        fidelity
            .retained_record(id)
            .expect("one failure image")
            .data(),
        Some(&bytes[first..second])
    );
    assert!(fidelity.record_layout()[id].reads().is_empty());
    assert!(!fidelity
        .record_layout()
        .contains_key("iges:entity:directory#1"));
    let peer = &fidelity.record_layout()["iges:entity:directory#3"];
    assert_eq!(peer.offset(), u64::try_from(second).expect("D3 source"));
    assert!(peer.reads().is_empty());
    assert!(fidelity
        .parts()
        .values()
        .all(|part| part.level().as_str() != "iges.parameter_record"));
    let unreadable: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(unreadable[0].message.contains("iges:entity:directory#1"));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != crate::loss::IgesLossCode::ParameterDataQuarantined.kind()));
    let stream = part.stream();
    cadmpeg_test_support::contract::parts::check_one_home(
        cadmpeg_ir::CodecFormat::Iges,
        &decoded.ir().native,
        fidelity,
        &decoded.report().losses,
        stream,
        |requested, range| {
            if requested != stream {
                return None;
            }
            bytes.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
        },
    )
    .expect("actual compressed Data failure has one source home");
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("neutral validation")
        .is_ok());
}
