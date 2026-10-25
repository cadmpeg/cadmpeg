// SPDX-License-Identifier: Apache-2.0

use super::{Family, PmDcFeature};
use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList};

fn feature(references: &[u32], version: u8) -> PmDcFeature {
    let references = references
        .iter()
        .copied()
        .map(PmDcReference::from_packed)
        .collect::<Vec<_>>();
    let metadata = (!references.is_empty())
        .then(|| PmDcListMetadata::U32([u32::try_from(references.len()).expect("slot count"), 0]));
    serde_json::from_value(serde_json::json!({
        "id": "inventor:pmdc:feature#synthetic-0",
        "segment_token": "synthetic", "record_ordinal": 0,
        "type_id": "914d8790d011f8d10008cabc0663dc09",
        "save_version_major": version,
        "header": {
            "header_value": 0, "header_id": 0,
            "next": PmDcReference::from_packed(0), "flags": 0,
            "context": PmDcReference::from_packed(0), "source_index": 1
        },
        "state": 0, "outline_value": 0,
        "properties": PmDcReferenceList::new(2, metadata, references).expect("slots"),
        "value": 0
    }))
    .expect("feature")
}

#[test]
fn slot_validation_checks_each_non_null_operand_type() {
    let loft = [
        (0, "509e2f31d1110344000881ba32a3dc09"),
        (1, "28be9a72d111440900084eba32a3dc09"),
        (2, "284d8790d011f8d10008cabc0663dc09"),
        (6, "474d8790d011f8d10008cabc0663dc09"),
        (7, "509e2f31d1110344000881ba32a3dc09"),
        (9, "284d8790d011f8d10008cabc0663dc09"),
        (10, "02e2e3e0d111480900084eba32a3dc09"),
        (13, "ae70680ed14a1e86d76248b03c2a96e1"),
    ];
    let sweep = [
        (0, "91739422d11107cf000835bd0663dc09"),
        (1, "3b2477a4d1118f96000826bd0663dc09"),
        (2, "28be9a72d111440900084eba32a3dc09"),
        (3, "264d8790d011f8d10008cabc0663dc09"),
        (4, "297d6392d1113cb9000831bd0663dc09"),
        (7, "71f23ed8d2115094a00049803603c8c9"),
        (8, "b7849ca26d4811e38b95b3a06051b440"),
        (9, "ed2ef031904b9b04d79918b91f1619c8"),
        (10, "e211b05c3e46583e928a0cbf3a085834"),
        (15, "ae70680ed14a1e86d76248b03c2a96e1"),
    ];
    for (family, rules) in [
        (Family::SolidLoft, loft.as_slice()),
        (Family::SurfaceLoft, loft.as_slice()),
        (Family::Sweep, sweep.as_slice()),
    ] {
        for version in [15, 16, 17, 18, 19, 20, 21, 22, 23, 25, 28, 29, 30, 31] {
            for (slot, type_id) in rules {
                for qualifier in [0, 0x8000_0000] {
                    let mut slots = [0; 20];
                    slots[*slot] = qualifier | 2;
                    let source = feature(&slots, version);
                    assert!(family
                        .valid_slots(&source, |ordinal| { (ordinal == 1).then_some(*type_id) }));
                    assert!(!family.valid_slots(&source, |_| None));
                    assert!(!family
                        .valid_slots(&source, |_| { Some("00000000000000000000000000000000") }));
                }
            }
        }
    }
}

#[test]
fn sweep_path_accepts_both_profile_selection_record_types() {
    let source = feature(&[0, 1], 29);
    for type_id in [
        "3b2477a4d1118f96000826bd0663dc09",
        "473f20fcd11101cf000835bd0663dc09",
    ] {
        assert!(Family::Sweep.valid_slots(&source, |_| Some(type_id)));
    }
}

#[test]
fn slot_validation_preserves_null_and_untyped_slots() {
    for family in [Family::SolidLoft, Family::SurfaceLoft, Family::Sweep] {
        assert!(family.valid_slots(&feature(&[], 19), |_| None));
        assert!(family.valid_slots(&feature(&[0; 20], 19), |_| None));
        let mut slots = [0; 21];
        slots[20] = 1;
        assert!(family.valid_slots(&feature(&slots, 19), |_| None));
    }
}

#[test]
fn slot_validation_requires_a_type_two_property_list() {
    let mut wire = serde_json::to_value(feature(&[], 19)).expect("feature wire");
    wire["properties"]["marker"] = serde_json::json!(8);
    let source = serde_json::from_value(wire).expect("type eight list");
    for family in [Family::SolidLoft, Family::SurfaceLoft, Family::Sweep] {
        assert!(!family.valid_slots(&source, |_| None));
    }
}

#[test]
fn class_identity_selects_the_slot_family() {
    for (class, family, name) in [
        ("05daba90d111386100080bbd0663dc09", Family::Sweep, "Sweep"),
        (
            "e0ef542fd111340900084eba32a3dc09",
            Family::SolidLoft,
            "Loft",
        ),
        (
            "808b00afd41140881000fca5bba647b5",
            Family::SurfaceLoft,
            "LoftSurface",
        ),
    ] {
        let class = crate::feature::ClassId::try_from(class.to_owned()).expect("class");
        assert_eq!(Family::from_class_id(class), Some(family));
        assert_eq!(family.native_name(), name);
        assert_eq!(
            crate::feature::native_kind(Some(class)),
            cadmpeg_ir::features::NativeFeatureKind::Other(name.into())
        );
    }
    let class = crate::feature::ClassId::try_from("00000000000000000000000000000000".to_owned())
        .expect("class");
    assert_eq!(Family::from_class_id(class), None);
}
