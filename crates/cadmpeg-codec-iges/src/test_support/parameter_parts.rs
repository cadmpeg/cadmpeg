// SPDX-License-Identifier: Apache-2.0
//! Exact source-field give-up assertions shared by codec contract controls.

pub(crate) fn assert_one_parameter_field(
    result: &cadmpeg_ir::codec::DecodeResult,
    bytes: &[u8],
    logical_start: usize,
    field: &[u8],
) {
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
    let fidelity = result.source_fidelity();
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(
        part.stream(),
        format!("iges/parameter/{start}/{}", start + 81)
    );
    assert_eq!(part.offset(), u64::try_from(logical_start).unwrap());
    assert_eq!(part.byte_len(), u64::try_from(field.len()).unwrap());
    assert_eq!(part.data(), Some(field));
    let unreadable: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1, "{:#?}", result.report().losses);
    assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(unreadable[0].message.contains("iges.parameter_field"));
    assert!(result.report().losses.iter().all(|loss| loss.code
        != crate::loss::IgesLossCode::ParameterDataQuarantined.kind()
        && loss.code != crate::loss::IgesLossCode::GeometryNotProjected.kind()));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .unwrap()
        .is_ok());
}
