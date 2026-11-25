use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use crate::test_support::split_face::metadata;
use crate::test_support::{indexed_header, lp_ascii};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::report::loss::LossNote;

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";
const PRIMARY_TYPE: &str = "00000000-0000-0000-0000-000000000001";
const SECONDARY_TYPE: &str = "00000000-0000-0000-0000-000000000002";

struct Fixture {
    bytes: Vec<u8>,
    primary: Vec<(u64, u64)>,
    secondary: Vec<(u64, u64)>,
    recipe: ConstructionRecipe,
    program: Vec<i32>,
}

fn fixture(kind: ConstructionRecipeKind, fake_headers: bool) -> Fixture {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"256", 100);
    if fake_headers {
        for index in 100..=104 {
            indexed_header(&mut bytes, *b"256", index);
        }
    }
    let secondary = vec![(100, u64_from_index(bytes.len()))];
    indexed_header(&mut bytes, *b"257", 100);
    let mut primary = vec![(100, 0)];
    for index in 101..=103 {
        primary.push((u64::from(index), u64_from_index(bytes.len())));
        indexed_header(&mut bytes, *b"256", index);
    }
    let name_at = bytes.len() + 4;
    lp_ascii(
        &mut bytes,
        match kind {
            ConstructionRecipeKind::Face => "face_recipe_data",
            ConstructionRecipeKind::BoundedFace => "bounded_face_recipe_data",
            _ => panic!("face recipe fixture"),
        },
    );
    let program = if fake_headers {
        vec![0, -1, 4, 3, 0x6836_3532, 0]
    } else {
        vec![0, -1, 4]
    };
    for word in &program {
        bytes.extend_from_slice(&i32::to_le_bytes(*word));
    }
    primary.push((104, u64_from_index(bytes.len())));
    indexed_header(&mut bytes, *b"256", 104);
    Fixture {
        bytes,
        primary,
        secondary,
        recipe: ConstructionRecipe {
            id: format!("{STREAM}:construction-recipe#{name_at}"),
            byte_offset: u64_from_index(name_at),
            kind,
            design: None,
            recipe_index: 0,
            record_index: None,
        },
        program,
    }
}

fn metadata_for(fixture: &Fixture) -> Vec<u8> {
    let entities: Vec<_> = fixture.primary.iter().map(|(entity, _)| *entity).collect();
    metadata(
        &[(PRIMARY_TYPE, 1, &entities), (SECONDARY_TYPE, 1, &[])],
        &fixture.primary,
        &fixture.secondary,
    )
}

fn decode(
    fixture: &Fixture,
    meta: &[u8],
    role: Option<DesignOperandRole>,
) -> Vec<DesignFaceOperand> {
    decode_with_scanned_header(fixture, meta, role, 11).0
}

fn decode_with_scanned_header(
    fixture: &Fixture,
    meta: &[u8],
    role: Option<DesignOperandRole>,
    scanned_header_offset: u64,
) -> (Vec<DesignFaceOperand>, Vec<LossNote>) {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::SplitFace,
        50,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(if role.is_some() {
                vec![90, 100]
            } else {
                vec![100]
            });
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let groups: Vec<DesignConstructionOperandGroup> = role
        .into_iter()
        .map(|role| {
            cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
                "id": format!("{STREAM}:group#90"),
                "scope_record_index": 50,
                "scope_reference_ordinal": 0,
                "record_index": 90,
                "byte_offset": 1000,
                "class_tag": "256",
                "members": [100],
                "member_offsets": [1026],
                "frame": {
                    "member_count_offset": 1021,
                    "opaque_index": 1,
                    "opaque_index_offset": 1072,
                    "opaque_scalar": 0.0,
                    "opaque_scalar_offset": 1076,
                    "variant": false
                },
                "role": role.raw(),
                "role_offset": 1054,
                "paired_class_tag": "257",
                "paired_byte_offset": 1125
            }))
            .unwrap()
        })
        .collect();
    let scanned_header = DesignRecordHeader {
        id: format!("{STREAM}:design-record-header#{scanned_header_offset}"),
        record_index: 100,
        byte_offset: scanned_header_offset,
        class_tag: "256".to_owned().try_into().unwrap(),
    };
    let archive = crate::test_support::body_reference_tail_test::archive(&fixture.bytes, meta);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            super::super::face_operands::decode_face_operands(
                ctx,
                scan,
                &[scope],
                &groups,
                &[scanned_header],
                std::slice::from_ref(&fixture.recipe),
                &[],
            )
            .unwrap()
        })
    })
}

#[test]
fn grouped_and_direct_split_face_retain_indexed_envelopes_without_registration() {
    for role in [
        None,
        Some(DesignOperandRole::ROLE_0X9),
        Some(DesignOperandRole::ROLE_0X10),
    ] {
        for kind in [
            ConstructionRecipeKind::Face,
            ConstructionRecipeKind::BoundedFace,
        ] {
            let fixture = fixture(kind, false);
            let types = [(PRIMARY_TYPE, 1, &[100, 101, 102, 103, 104][..])];
            let unregistered_member = [
                fixture.primary[0],
                fixture.primary[1],
                fixture.primary[3],
                fixture.primary[4],
            ];
            let incomplete = metadata(&types, &unregistered_member, &fixture.secondary);
            for meta in [&[][..], incomplete.as_slice()] {
                let (operands, losses) = decode_with_scanned_header(&fixture, meta, role, 0);
                let [operand] = operands.as_slice() else {
                    panic!("one indexed face operand");
                };
                assert_eq!(operand.record_index(), 100);
                assert_eq!(operand.recipe_record_index(), 103);
                assert_eq!(operand.recipe_kind, kind);
                assert_eq!(operand.recipe_program, [0, -1, 4]);
                assert_eq!(operand.next_byte_offset(), fixture.primary[4].1);
                assert_eq!(operand.group.is_some(), role.is_some());
                assert!(operand.registered_recipe_frame().is_none());
                let [loss] = losses.as_slice() else {
                    panic!("one indexed-frame assumption");
                };
                assert_eq!(
                    loss.code,
                    crate::loss::F3dLossCode::FeatureSplitFaceIndexedOperandAssumed.kind()
                );
            }
        }
    }
}

#[test]
fn grouped_and_direct_split_face_parse_complete_registered_envelopes() {
    for role in [
        None,
        Some(DesignOperandRole::ROLE_0X9),
        Some(DesignOperandRole::ROLE_0X10),
    ] {
        for kind in [
            ConstructionRecipeKind::Face,
            ConstructionRecipeKind::BoundedFace,
        ] {
            let fixture = fixture(kind, false);
            let operands = decode(&fixture, &metadata_for(&fixture), role);
            let [operand] = operands.as_slice() else {
                panic!("one registered face operand")
            };
            assert_eq!(operand.record_index(), 100);
            assert_eq!(operand.recipe_record_index(), 103);
            assert_eq!(operand.recipe_kind, kind);
            assert_eq!(operand.recipe_program, [0, -1, 4]);
            assert_eq!(operand.next_record_index, 104);
            assert_eq!(operand.next_byte_offset(), fixture.primary[4].1);
            assert_eq!(operand.group.is_some(), role.is_some());
            let draft = operands.into_iter().next().unwrap().into_draft();
            assert_eq!(draft.byte_offset, 0);
            assert_eq!(draft.paired_byte_offset, 11);
            assert_eq!(draft.recipe_record_byte_offset, 44);
        }
    }
}

#[test]
fn registered_face_offsets_ignore_fake_chain_and_terminal_headers_in_payload() {
    for role in [
        None,
        Some(DesignOperandRole::ROLE_0X9),
        Some(DesignOperandRole::ROLE_0X10),
    ] {
        let fixture = fixture(ConstructionRecipeKind::BoundedFace, true);
        let operands = decode(&fixture, &metadata_for(&fixture), role);
        let [operand] = operands.as_slice() else {
            panic!("one registered face operand")
        };
        assert_eq!(operand.recipe_program, fixture.program);
        assert_eq!(operand.next_byte_offset(), fixture.primary[4].1);
        let draft = operands.into_iter().next().unwrap().into_draft();
        assert_eq!(draft.paired_byte_offset, 66);
        assert_eq!(draft.recipe_record_byte_offset, 99);
    }
}

#[test]
fn registered_face_envelope_cannot_borrow_a_later_record_recipe() {
    let mut fixture = fixture(ConstructionRecipeKind::Face, false);
    fixture.bytes.truncate(55);
    fixture.bytes.extend_from_slice(&[0; 8]);
    fixture.primary[4].1 = u64_from_index(fixture.bytes.len());
    indexed_header(&mut fixture.bytes, *b"256", 104);
    let name_at = fixture.bytes.len() + 4;
    lp_ascii(&mut fixture.bytes, "face_recipe_data");
    for word in [0i32, -1] {
        fixture.bytes.extend_from_slice(&word.to_le_bytes());
    }
    fixture.recipe.byte_offset = u64_from_index(name_at);
    fixture.recipe.id = format!("{STREAM}:construction-recipe#{name_at}");
    fixture
        .primary
        .push((105, u64_from_index(fixture.bytes.len())));
    indexed_header(&mut fixture.bytes, *b"256", 105);
    for role in [
        None,
        Some(DesignOperandRole::ROLE_0X9),
        Some(DesignOperandRole::ROLE_0X10),
    ] {
        let (operands, losses) =
            decode_with_scanned_header(&fixture, &metadata_for(&fixture), role, 0);
        assert!(operands.is_empty());
        assert!(losses.is_empty());
    }
}

#[test]
fn face_recipe_ends_at_the_next_registered_header_without_an_id_delta_guess() {
    for terminal in [105u32, 900] {
        let mut fixture = fixture(ConstructionRecipeKind::Face, true);
        let boundary = usize::try_from(fixture.primary[4].1).unwrap();
        fixture.bytes.truncate(boundary);
        indexed_header(&mut fixture.bytes, *b"256", terminal);
        fixture.primary[4].0 = u64::from(terminal);
        let operands = decode(&fixture, &metadata_for(&fixture), None);
        let [operand] = operands.as_slice() else {
            panic!("registered terminal")
        };
        assert_eq!(operand.next_record_index, terminal);
        assert_eq!(operand.next_byte_offset(), u64_from_index(boundary));
        assert_eq!(operand.recipe_program, fixture.program);
    }
}

#[test]
fn face_recipe_secondary_header_is_the_registered_terminal_boundary() {
    let mut fixture = fixture(ConstructionRecipeKind::Face, false);
    let boundary = fixture.primary[4].1;
    fixture.bytes.truncate(usize::try_from(boundary).unwrap());
    indexed_header(&mut fixture.bytes, *b"257", 103);
    fixture.secondary.push((103, boundary));
    fixture.primary[4].1 = u64_from_index(fixture.bytes.len());
    indexed_header(&mut fixture.bytes, *b"256", 104);
    let operands = decode(&fixture, &metadata_for(&fixture), None);
    let [operand] = operands.as_slice() else {
        panic!("registered secondary terminal")
    };
    assert_eq!(operand.next_record_index, 103);
    assert_eq!(operand.next_byte_offset(), boundary);
    assert_eq!(operand.recipe_program, [0, -1, 4]);
}

#[test]
fn registered_face_rejects_truncated_headers_and_mismatched_recipe_fields() {
    for damage in 0..3 {
        let mut fixture = fixture(ConstructionRecipeKind::Face, false);
        match damage {
            0 => {
                fixture.bytes.pop();
            }
            1 => {
                fixture.bytes[59] = b'X';
            }
            2 => {
                fixture.bytes[55..59].copy_from_slice(&24u32.to_le_bytes());
            }
            _ => unreachable!(),
        }
        assert!(decode(&fixture, &metadata_for(&fixture), None).is_empty());
    }
}

#[test]
fn split_face_with_an_unusable_scan_requires_complete_unambiguous_registered_metadata() {
    let fixture = fixture(ConstructionRecipeKind::Face, false);
    let valid_types = [
        (PRIMARY_TYPE, 1, &[100, 101, 102, 103, 104][..]),
        (SECONDARY_TYPE, 1, &[][..]),
    ];
    let cases = [
        Vec::new(),
        metadata(&valid_types, &fixture.primary, &[]),
        metadata(&valid_types, &fixture.primary[..4], &fixture.secondary),
        metadata(&valid_types, &fixture.primary, &[(100, 1)]),
        metadata(&valid_types, &fixture.primary, &[(101, 11)]),
        metadata(
            &[
                (PRIMARY_TYPE, 1, &[100, 101, 102, 103, 104, 100]),
                (SECONDARY_TYPE, 1, &[]),
            ],
            &fixture.primary,
            &fixture.secondary,
        ),
        metadata(
            &[
                (PRIMARY_TYPE, 1, &[100, 101, 102, 103, 104]),
                (SECONDARY_TYPE, 1, &[100]),
            ],
            &fixture.primary,
            &fixture.secondary,
        ),
        metadata(
            &[
                (PRIMARY_TYPE, 1, &[100, 101, 102, 104]),
                (SECONDARY_TYPE, 1, &[]),
            ],
            &fixture.primary,
            &fixture.secondary,
        ),
        metadata(
            &valid_types,
            &[(100, 0), (102, 33), (103, 44), (104, fixture.primary[4].1)],
            &fixture.secondary,
        ),
    ];
    for meta in cases {
        for role in [
            None,
            Some(DesignOperandRole::ROLE_0X9),
            Some(DesignOperandRole::ROLE_0X10),
        ] {
            assert!(decode(&fixture, &meta, role).is_empty());
        }
    }
}
