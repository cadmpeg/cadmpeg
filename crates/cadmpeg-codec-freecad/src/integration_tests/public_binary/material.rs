// SPDX-License-Identifier: Apache-2.0
//! The public v3 carrier states eight fixed materials before its string tail.
use super::*;
use crate::native::entry_contents::EntryContents;

#[test]
fn public_shape_appearance_keeps_eight_fixed_materials_then_twenty_four_strings() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
    ));
    let decoded = decode(bytes, cadmpeg_core::decode::DecodeDepth::Full);
    let (carriers, boundaries) = source::inventory(bytes, &decoded);
    let (index, carrier) = carriers
        .iter()
        .enumerate()
        .find(|(_, carrier)| carrier.member == "ShapeAppearance")
        .unwrap();
    assert!(matches!(carrier.kind, BinaryKind::Material { version: 3 }));
    let raw = source::member(bytes, &carrier.member);
    assert_eq!(raw.len(), 292);
    assert_eq!(u32::from_le_bytes(raw[..4].try_into().unwrap()), 8);
    let material_ranges = boundaries
        .iter()
        .filter(|part| part.carrier == index && part.level == "freecad.material_list_material")
        .map(|part| part.offset..part.offset + part.length)
        .collect::<Vec<_>>();
    assert_eq!(
        material_ranges,
        (0..8)
            .map(|index| 4 + index * 24..4 + (index + 1) * 24)
            .collect::<Vec<_>>()
    );
    let mut at = 196;
    let mut strings = Vec::new();
    for _ in 0..24 {
        let start = at;
        let length =
            usize::try_from(u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())).unwrap();
        at += 4 + length;
        assert!(std::str::from_utf8(&raw[start + 4..at]).is_ok());
        strings.push(u64::try_from(start).unwrap()..u64::try_from(at).unwrap());
    }
    assert_eq!(at, 292);
    assert_eq!(
        boundaries
            .iter()
            .filter(|part| part.carrier == index && part.level == "freecad.material_list_string")
            .map(|part| part.offset..part.offset + part.length)
            .collect::<Vec<_>>(),
        strings
    );
    let entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.name() == carrier.member)
        .unwrap();
    let materials = entry
        .contents()
        .iter()
        .find_map(|contents| match contents {
            EntryContents::Material { version: 3, values } => Some(values),
            _ => None,
        })
        .unwrap();
    assert_eq!(materials.len(), 8);
    assert_eq!(
        materials.iter().filter(|slot| slot.fixed.is_some()).count(),
        8
    );
    assert_eq!(
        materials
            .iter()
            .flat_map(|slot| [&slot.image, &slot.image_path, &slot.uuid])
            .filter(|value| value.is_some())
            .count(),
        24
    );
    let appearances = decoded
        .ir()
        .model
        .appearances
        .iter()
        .filter(|appearance| appearance.native_ref.as_deref() == Some(entry.id()))
        .collect::<Vec<_>>();
    assert_eq!(appearances.len(), 8);
    let appearance_ids = appearances
        .iter()
        .map(|appearance| appearance.id.as_str())
        .collect::<BTreeSet<_>>();
    let maps = decoded.ir().model.attributes.iter().filter(|attribute|
        attribute.native_ref.as_deref() == Some(entry.id()) && attribute.name == "ShapeAppearance" &&
        matches!(&attribute.target, cadmpeg_ir::attributes::AttributeTarget::Record { id, .. } if appearance_ids.contains(id.as_str()))
    ).collect::<Vec<_>>();
    assert_eq!(maps.len(), 8);
    assert!(maps.iter().all(|attribute| matches!(attribute.values.as_slice(), [cadmpeg_ir::attributes::AttributeValue::Map(values)] if values.len() == 3)));
    assert!(decoded
        .source_fidelity()
        .parts()
        .values()
        .all(|part| part.state() != &(PartState::GivenUp {})));
    exact(&oracle::expected(&decoded, carrier, &raw), decoded.ir());
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    crate::fidelity::b1::assert_classified_spans(&decoded);
}
