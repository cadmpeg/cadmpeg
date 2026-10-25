// SPDX-License-Identifier: Apache-2.0
//! Mesh material fields keep their own getters and current source-less values.

use super::*;
use cadmpeg_ir::attributes::AttributeNumericList;
use cadmpeg_ir::source_fidelity::parts::PartState;

fn source(binding: u32, truncated: Option<usize>) -> Vec<u8> {
    let document = "<Document SchemaVersion='4'><Properties Count='2'><Property name='Material' type='Mesh::PropertyMaterial'><Material file='mesh' Vendor='pointer-note'/></Property><Property name='Alias' type='Mesh::PropertyMaterial'><Material file='mesh'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let mut bytes = binding.to_le_bytes().to_vec();
    for _ in 0..4 {
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&0x11223344_u32.to_le_bytes());
        bytes.extend_from_slice(&0x55667788_u32.to_le_bytes());
    }
    for _ in 0..2 {
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&0.25_f32.to_le_bytes());
        bytes.extend_from_slice(&0.75_f32.to_le_bytes());
    }
    if let Some(end) = truncated {
        bytes.truncate(end);
    }
    crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("mesh", &bytes),
    ])
}
fn decode(bytes: &[u8]) -> cadmpeg_ir::codec::DecodeResult {
    FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}
fn write(
    ir: &cadmpeg_ir::CadIr,
) -> (
    cadmpeg_ir::codec::DecodeResult,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let decoded = decode(&bytes);
    assert!(decoded
        .source_fidelity()
        .parts()
        .values()
        .all(|part| part.state() != &PartState::GivenUp {}));
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    (decoded, losses)
}

#[test]
fn mesh_material_attributes_use_all_seven_original_fields_and_current_source_less_values() {
    let before = decode(&source(1, None));
    assert_eq!(before.ir().model.attributes.len(), 2);
    let expected = vec![
        AttributeValue::Integer(1),
        AttributeValue::NumericList(AttributeNumericList::Color(Some(vec![
            Some(0x11223344),
            Some(0x55667788),
        ]))),
        AttributeValue::NumericList(AttributeNumericList::Color(Some(vec![
            Some(0x11223344),
            Some(0x55667788),
        ]))),
        AttributeValue::NumericList(AttributeNumericList::Color(Some(vec![
            Some(0x11223344),
            Some(0x55667788),
        ]))),
        AttributeValue::NumericList(AttributeNumericList::Color(Some(vec![
            Some(0x11223344),
            Some(0x55667788),
        ]))),
        AttributeValue::NumericList(AttributeNumericList::Float(Some(vec![
            Some(cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap()),
            Some(cadmpeg_ir::scalar::FiniteReal::new(0.75).unwrap()),
        ]))),
        AttributeValue::NumericList(AttributeNumericList::Float(Some(vec![
            Some(cadmpeg_ir::scalar::FiniteReal::new(0.25).unwrap()),
            Some(cadmpeg_ir::scalar::FiniteReal::new(0.75).unwrap()),
        ]))),
    ];
    let part = before
        .source_fidelity()
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.mesh_material_entry")
        .unwrap();
    for attribute in &before.ir().model.attributes {
        assert_eq!(attribute.values, expected);
        assert_eq!(
            before.source_fidelity().record_layout()[attribute.id.as_str()].reads()[part.stream()],
            [0..76]
        );
    }
    let mut current = before.ir().clone();
    let material = current
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "Material")
        .unwrap();
    material.values[0] = AttributeValue::Integer(2);
    material.values[1] =
        AttributeValue::NumericList(AttributeNumericList::Color(Some(vec![Some(0xabcdef01)])));
    material.values[5] =
        AttributeValue::NumericList(AttributeNumericList::Float(Some(vec![Some(
            cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
        )])));
    let expected = current
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<BTreeMap<_, _>>();
    let (after, losses) = write(&current);
    assert!(!losses.iter().any(|loss| matches!(
        loss.code.local_code(),
        "attribute.not-transferred" | "source.undecoded-omitted"
    )));
    assert_eq!(
        after
            .ir()
            .model
            .attributes
            .iter()
            .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
            .collect::<BTreeMap<_, _>>(),
        expected
    );
    let properties = after
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let material = properties
        .iter()
        .find(|property| property.name == "Material")
        .unwrap();
    assert_eq!(material.type_name, "Mesh::PropertyMaterial");
    assert_eq!(material.values()[0].attributes["Vendor"], "pointer-note");
}

#[test]
fn mesh_material_unknown_arrays_and_deleted_attributes_never_restore_old_native_bytes() {
    let before = decode(&source(1, None));
    for list in [
        AttributeNumericList::Color(None),
        AttributeNumericList::Color(Some(vec![Some(7), None])),
    ] {
        let mut current = before.ir().clone();
        let material = current
            .model
            .attributes
            .iter_mut()
            .find(|attribute| attribute.name == "Material")
            .unwrap();
        let id = material.id.to_string();
        material.values[1] = AttributeValue::NumericList(list);
        let (after, losses) = write(&current);
        assert!(!after
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Material"));
        assert!(after
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Alias"));
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code.local_code() == "source.undecoded-omitted"
                    && loss.message.contains(&id))
                .count(),
            1
        );
    }
    let mut current = before.ir().clone();
    current
        .model
        .attributes
        .retain(|attribute| attribute.name != "Material");
    let (after, _) = write(&current);
    assert!(!after
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Material"));
    assert_eq!(
        after
            .ir()
            .model
            .attributes
            .iter()
            .filter(|attribute| attribute.name == "Alias")
            .count(),
        1
    );
    let partial = decode(&source(1, Some(9)));
    for attribute in &partial.ir().model.attributes {
        assert!(
            matches!(attribute.values.as_slice(),[AttributeValue::Integer(1),AttributeValue::NumericList(AttributeNumericList::Color(Some(values))),..] if values==&vec![None,None])
        );
        assert!(attribute.values[2..].iter().all(|value| matches!(
            value,
            AttributeValue::NumericList(
                AttributeNumericList::Color(None) | AttributeNumericList::Float(None)
            )
        )));
    }
    assert_eq!(partial.ir().model.attributes.len(), 2);
}

#[test]
fn mesh_material_missing_binding_sponsors_only_its_actual_binding_failure() {
    let before = decode(&source(1, None));
    let ids = before
        .ir()
        .model
        .attributes
        .iter()
        .map(|attribute| attribute.id.to_string())
        .collect::<Vec<_>>();
    let after = decode(&source(3, None));
    assert!(after.ir().model.attributes.is_empty());
    let failed = after
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].level().as_str(), "freecad.mesh_material_binding");
    assert_eq!((failed[0].offset(), failed[0].byte_len()), (0, 4));
    let errors = after
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    for id in ids {
        assert!(errors[0].message.contains(&id), "{:?}", errors[0]);
    }
}
