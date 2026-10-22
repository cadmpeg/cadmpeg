// SPDX-License-Identifier: Apache-2.0
use super::{assert_valid, decode};
use crate::chunks::ArchiveVersion;

fn unit_binding_document(unit: Option<i32>) -> (Vec<u8>, Vec<u8>) {
    let archive = ArchiveVersion::V8;
    let object = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &crate::test_support::test_dump::point_payload([1.0, 2.0, 3.0]),
    );
    let settings = unit
        .map(|value| vec![crate::test_support::test_dump::units_record(archive, value)])
        .unwrap_or_default();
    let bytes = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &settings),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0013,
                std::slice::from_ref(&object),
            ),
        ],
    );
    (bytes, object)
}

#[test]
fn full_document_unit_binding_transfers_model_units_and_reports_an_undeclared_unit() {
    use crate::loss::RhinoLossCode::{DocumentLengthUnitUndeclared, DocumentLengthUnitUnset};
    for (unit, label, expected) in [
        (Some(2), "millimeters", None),
        (Some(0), "native", Some(DocumentLengthUnitUndeclared)),
        (Some(255), "unavailable", Some(DocumentLengthUnitUnset)),
        (None, "millimeters", None),
    ] {
        let (bytes, object) = unit_binding_document(unit);
        let result = decode(bytes);
        let points = &result.ir().model.points;
        assert_eq!(points.len(), 1, "{label}");
        assert_eq!(
            points[0].position().get(),
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            "{label}"
        );
        let retained = result
            .source_fidelity()
            .retained_record("rhino:object:record#000000")
            .unwrap_or_else(|| panic!("{label} object source was not retained"));
        assert_eq!(retained.data(), Some(object.as_slice()), "{label}");
        let unit_losses = result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                [DocumentLengthUnitUndeclared, DocumentLengthUnitUnset]
                    .iter()
                    .any(|code| loss.code == code.kind())
            })
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            unit_losses,
            expected
                .map(|code| code.kind())
                .into_iter()
                .collect::<Vec<_>>(),
            "{label}"
        );
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("no physical millimetre binding")));
        assert_valid(&result);
    }
}
