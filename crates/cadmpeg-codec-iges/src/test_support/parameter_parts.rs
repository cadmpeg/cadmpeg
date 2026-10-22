// SPDX-License-Identifier: Apache-2.0
//! Exact source-field give-up assertions shared by codec contract controls.

pub(crate) fn assert_one_parameter_field(
    result: &cadmpeg_ir::codec::DecodeResult,
    bytes: &[u8],
    logical_start: usize,
    field: &[u8],
) {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    use cadmpeg_ir::NativeUnknownRecord;

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, input) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("fixture input has a genuine root View");
    let source = ctx
        .capture_part_extent(input)
        .expect("fixture root source stream");
    let source_stream = source.stream();
    let start = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .unwrap()
        * 81;
    assert_eq!(
        &bytes[start + logical_start..start + logical_start + field.len()],
        field
    );
    let native = result.ir().native.namespace("iges").unwrap();
    assert!(native
        .arenas()
        .get("quarantined_parameter_records")
        .is_none_or(Vec::is_empty));
    let unknowns = native.arena_as::<NativeUnknownRecord>("unknowns").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0]
            .links
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        ["iges:entity:directory#1"]
    );
    let fidelity = result.source_fidelity();
    let part = &fidelity.parts()[&unknowns[0].id];
    assert_eq!(part.level().as_str(), "iges.parameter_field");
    assert_eq!(
        part.stream(),
        format!("iges/parameter/{source_stream}/{start}/{}", start + 81)
    );
    assert_eq!(part.offset(), u64::try_from(logical_start).unwrap());
    assert_eq!(part.byte_len(), u64::try_from(field.len()).unwrap());
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(
        part.framing_bytes(),
        &[u64::try_from(logical_start + field.len() - 1).unwrap()
            ..u64::try_from(logical_start + field.len()).unwrap()]
    );
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(field)
    );
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp {}))
            .count(),
        1
    );
    let unreadable: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1, "{:#?}", result.report().losses);
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(unreadable[0].message.contains("iges.parameter_field"));
    assert!(unreadable[0]
        .message
        .contains("record=iges:entity:directory#1"));
    assert!(result.report().losses.iter().all(|loss| loss.code
        != crate::loss::IgesLossCode::ParameterLiteralUnusable.kind()
        && loss.code != crate::loss::IgesLossCode::ParameterDataQuarantined.kind()
        && loss.code != crate::loss::IgesLossCode::GeometryNotProjected.kind()));
    let owner = &fidelity.record_layout()["iges:entity:directory#1"];
    for (stream, ranges) in owner.reads() {
        let field_start = if stream == part.stream() {
            Some(u64::try_from(logical_start).unwrap())
        } else if stream == source_stream {
            Some(u64::try_from(start + logical_start).unwrap())
        } else {
            None
        };
        if let Some(field_start) = field_start {
            let content_end = field_start + u64::try_from(field.len() - 1).unwrap();
            assert!(ranges
                .iter()
                .all(|range| range.end <= field_start || range.start >= content_end));
        }
    }
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .unwrap()
        .is_ok());
}
