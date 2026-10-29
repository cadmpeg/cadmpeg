// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::{PmDcFeatureEnumFamily, PmDcKernelEntityLinkPayload};
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::test_fixtures::{content_at, parse};
use cadmpeg_ir::ids::IdentityKey;

#[test]
fn retained_features_report_unresolved_construction_controls_by_family() {
    use crate::loss::InventorLossCode;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};

    let definitions = ["Sweep", "Loft", "LoftSurface", "Sweep", "Revolve"].map(|kind| {
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: NativeFeatureKind::Other(kind.into()),
            parameters: std::collections::BTreeMap::new(),
        })
    });
    let losses = retained_control_losses(&definitions);
    assert_eq!(losses.len(), 3);
    assert_eq!(
        losses[0].code,
        InventorLossCode::FeatureSweepControlsUnresolved.kind()
    );
    assert!(losses[0]
        .message
        .starts_with("Retained 2 native Inventor sweep"));
    assert_eq!(
        losses[1].code,
        InventorLossCode::FeatureLoftControlsUnresolved.kind()
    );
    assert!(losses[1]
        .message
        .starts_with("Retained 1 native Inventor loft"));
    assert_eq!(
        losses[2].code,
        InventorLossCode::FeatureSurfaceLoftSectionsNative.kind()
    );
    assert_eq!(
        losses[2].code.local_code(),
        "feature.surface-loft-sections-native"
    );
    assert!(losses[2]
        .message
        .starts_with("Retained 1 native Inventor surface loft"));
    assert!(retained_control_losses(std::iter::empty()).is_empty());
    assert!(retained_control_losses(&definitions[4..]).is_empty());
}

fn header(version: u8) -> Vec<u8> {
    let stamps: &[i32] = match version {
        15..=22 => &[],
        23 => &[-1],
        _ => &[-1, 0],
    };
    content_at(2, version, stamps)
}

fn section_bytes(version: u8, map: [u32; 3]) -> Vec<u8> {
    let mut bytes = header(version);
    bytes.extend_from_slice(&0x8000_0004u32.to_le_bytes());
    bytes.push(0xa5);
    for value in [5u32, 6, 7] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in map {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&0x8000_0008u32.to_le_bytes());
    bytes
}

fn list_bytes(version: u8, members: &[u32]) -> Vec<u8> {
    let mut bytes = header(version);
    bytes.extend_from_slice(&0x3000_0002u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(members.len()).expect("count").to_le_bytes());
    if !members.is_empty() {
        for value in [2u32, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for member in members {
        bytes.extend_from_slice(&member.to_le_bytes());
    }
    bytes
}

fn located(payload: PmDcFeaturePropertyPayload, type_id: [u8; 16]) -> PmDcFeatureProperty {
    Located::new(
        payload,
        RecordTypeId::from_bytes(type_id),
        IdentityKey::try_new("synthetic").expect("segment"),
        0,
    )
}

fn edited<T: Clone>(record: &Located<T>, edit: impl FnOnce(&mut T)) -> Located<T> {
    let mut payload = (**record).clone();
    edit(&mut payload);
    Located::new(
        payload,
        record.identity.type_id.clone(),
        record.identity.segment_token.clone(),
        record.identity.record_ordinal,
    )
}

fn section() -> PmDcFeatureProperty {
    parse(&section_bytes(16, [0, 0x3000_0007, 0]), |ctx, source| {
        located(
            parse_section(ctx, source, 16).expect("section"),
            SECTION_TYPE,
        )
    })
}

fn kernel_link() -> PmDcKernelEntityLink {
    Located::new(
        PmDcKernelEntityLinkPayload {
            save_version_major: 16,
            header_value: 0,
            header_id: 0,
            selection_source: 2,
            state: 4,
            body: PmDcReference::from_packed(9),
            entity_id: 1,
            entity_type: 38,
        },
        RecordTypeId::from_bytes(crate::feature::KERNEL_ENTITY_LINK_TYPE),
        IdentityKey::try_new("synthetic").expect("segment"),
        3,
    )
}

fn type_at(ordinal: u32) -> Option<&'static str> {
    match ordinal {
        1 | 2 => Some("e850314bd111d641000880ba32a3dc09"),
        3 => Some("21750fccd1112780e38619962259017a"),
        4 => Some("6c1c6c3c9a43e3947318e2bbd0c39ff8"),
        5 | 6 => Some("264d8790d011f8d10008cabc0663dc09"),
        7 => Some("284d8790d011f8d10008cabc0663dc09"),
        8 => Some("5c5945f6d5113313100060a6bba647b5"),
        _ => None,
    }
}

#[test]
fn section_dispatch_preserves_scalars_and_versioned_headers() {
    let parser = crate::feature::feature_property_parser(SECTION_TYPE).expect("section parser");
    for version in [15, 16, 17, 22, 23, 25, 28, 29, 30, 31] {
        let bytes = section_bytes(version, [0, 0x3000_0007, 0]);
        let parsed = parse(&bytes, |ctx, source| {
            parser(ctx, source, version).expect("section")
        });
        assert_eq!(parsed.save_version_major, version);
        assert_eq!(parsed.header.source_index, 2);
        assert_eq!(parsed.header.flags_extension, (version >= 28).then_some(0));
        assert_eq!(
            parsed.header.stamps.len(),
            match version {
                ..=22 => 0,
                23 => 1,
                _ => 2,
            }
        );
        assert!(matches!(parsed.kind, PmDcFeaturePropertyKind::LoftSection {
            entity_link, value: 0xa5, condition, parameters, map_value: 0,
            source_value: u32::MAX, option,
        } if entity_link.index() == 4 && entity_link.qualified()
            && condition.index() == 5
            && parameters.map(PmDcReference::index) == [6, 7]
            && option.index() == 8 && option.qualified()));
        for length in 0..bytes.len() {
            parse(&bytes[..length], |ctx, source| {
                assert!(parser(ctx, source, version).is_err());
            });
        }
        let mut trailing = bytes;
        trailing.push(0);
        parse(&trailing, |ctx, source| {
            assert!(parser(ctx, source, version).is_err());
        });
    }
    for map in [
        [1, 0x3000_0007, 0],
        [0, 0x3000_0002, 0],
        [0, 0x3000_0007, 1],
    ] {
        parse(&section_bytes(16, map), |ctx, source| {
            assert!(parser(ctx, source, 16).is_err());
        });
    }
}

#[test]
fn section_list_dispatch_keeps_order_and_bounds_the_count() {
    let parser = crate::feature::feature_property_parser(SECTION_LIST_TYPE).expect("list parser");
    for version in [16, 23, 25, 28, 31] {
        let bytes = list_bytes(version, &[0x8000_0003, 2]);
        let parsed = parse(&bytes, |ctx, source| {
            parser(ctx, source, version).expect("list")
        });
        assert!(matches!(parsed.kind, PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::LoftSections, items,
        } if items.references().iter().map(|reference| (reference.index(), reference.qualified())).collect::<Vec<_>>()
            == [(3, true), (2, false)]));
        for length in 0..bytes.len() {
            parse(&bytes[..length], |ctx, source| {
                assert!(parser(ctx, source, version).is_err());
            });
        }
        let mut trailing = bytes;
        trailing.push(0);
        parse(&trailing, |ctx, source| {
            assert!(parser(ctx, source, version).is_err());
        });
        parse(&list_bytes(version, &[]), |ctx, source| {
            assert!(
                matches!(parser(ctx, source, version).expect("empty list").kind,
                PmDcFeaturePropertyKind::References { items, .. } if items.references().is_empty())
            );
        });
    }
    let mut excessive = header(16);
    for value in [0x3000_0002u32, u32::MAX, 0, 0] {
        excessive.extend_from_slice(&value.to_le_bytes());
    }
    parse(&excessive, |ctx, source| {
        assert!(parser(ctx, source, 16).is_err());
    });
    let mut wrong_marker = header(16);
    for value in [0x3000_0007u32, 0] {
        wrong_marker.extend_from_slice(&value.to_le_bytes());
    }
    parse(&wrong_marker, |ctx, source| {
        assert!(parser(ctx, source, 16).is_err());
    });
}

#[test]
fn control_dispatch_preserves_type_and_value_without_assigning_a_mode() {
    for type_id in [
        SECTION_CONDITION_TYPE,
        LOFT_MODE_TYPE,
        SWEEP_MODE_TYPE,
        SWEEP_ORIENTATION_TYPE,
        SWEEP_SCALING_TYPE,
    ] {
        let parser = crate::feature::feature_property_parser(type_id).expect("control parser");
        let mut bytes = header(31);
        bytes.extend_from_slice(&i16::MIN.to_le_bytes());
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        parse(&bytes, |ctx, source| {
            assert!(matches!(
                parser(ctx, source, 31).expect("control").kind,
                PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::Auxiliary,
                    type_value: i16::MIN,
                    value: u16::MAX,
                }
            ));
        });
        for length in 0..bytes.len() {
            parse(&bytes[..length], |ctx, source| {
                assert!(parser(ctx, source, 31).is_err());
            });
        }
        bytes.push(0);
        parse(&bytes, |ctx, source| {
            assert!(parser(ctx, source, 31).is_err());
        });
    }
}

#[test]
fn section_validation_checks_reference_types_and_selection_source() {
    let section = section();
    let link = kernel_link();
    assert!(valid_references(&section, type_at, |_| Some(&link)));
    assert!(!valid_references(&section, type_at, |_| None));
    for ordinal in 3..=8 {
        assert!(!valid_references(
            &section,
            |candidate| {
                if candidate == ordinal {
                    None
                } else {
                    type_at(candidate)
                }
            },
            |_| Some(&link)
        ));
        assert!(!valid_references(
            &section,
            |candidate| {
                if candidate == ordinal {
                    Some("00000000000000000000000000000000")
                } else {
                    type_at(candidate)
                }
            },
            |_| Some(&link)
        ));
    }
    for field in 0..5 {
        let mut payload = (*section).clone();
        let PmDcFeaturePropertyKind::LoftSection {
            entity_link,
            condition,
            parameters,
            option,
            ..
        } = &mut payload.kind
        else {
            panic!("section");
        };
        *match field {
            0 => entity_link,
            1 => condition,
            2 => &mut parameters[0],
            3 => &mut parameters[1],
            _ => option,
        } = PmDcReference::from_packed(0);
        let invalid = located(payload, SECTION_TYPE);
        assert_eq!(
            valid_references(&invalid, type_at, |_| Some(&link)),
            field == 4
        );
    }
    let wrong_source = edited(&link, |payload| payload.selection_source = 3);
    assert!(!valid_references(&section, type_at, |_| Some(
        &wrong_source
    )));
    let wrong_body = edited(&link, |payload| {
        payload.body = PmDcReference::from_packed(0);
    });
    assert!(!valid_references(&section, type_at, |_| Some(&wrong_body)));
    let mut wrong_identity = link.clone();
    wrong_identity.identity.record_ordinal = 4;
    assert!(!valid_references(&section, type_at, |_| Some(
        &wrong_identity
    )));
    wrong_identity = link.clone();
    wrong_identity.identity.segment_token = IdentityKey::try_new("other").expect("segment");
    assert!(!valid_references(&section, type_at, |_| Some(
        &wrong_identity
    )));
    let wrong_map = edited(&section, |payload| {
        if let PmDcFeaturePropertyKind::LoftSection { map_value, .. } = &mut payload.kind {
            *map_value = 1;
        }
    });
    assert!(!valid_references(&wrong_map, type_at, |_| Some(&link)));
    let mut wrong_type = section;
    wrong_type.identity.type_id = RecordTypeId::from_bytes(SECTION_LIST_TYPE);
    assert!(!valid_references(&wrong_type, type_at, |_| Some(&link)));
}

#[test]
fn section_list_validation_rejects_null_missing_and_wrong_type_members() {
    for (members, valid) in [
        (&[3u32, 2][..], true),
        (&[0][..], false),
        (&[4][..], false),
        (&[10][..], false),
    ] {
        let list = parse(&list_bytes(16, members), |ctx, source| {
            located(
                parse_sections(ctx, source, 16).expect("list"),
                SECTION_LIST_TYPE,
            )
        });
        assert_eq!(valid_references(&list, type_at, |_| None), valid);
    }
}

#[test]
fn record_type_validation_rejects_wrong_payload_kinds() {
    for type_id in [
        SECTION_TYPE,
        SECTION_CONDITION_TYPE,
        LOFT_MODE_TYPE,
        SWEEP_MODE_TYPE,
        SWEEP_ORIENTATION_TYPE,
        SWEEP_SCALING_TYPE,
    ] {
        let wrong_kind = parse(&list_bytes(16, &[]), |ctx, source| {
            located(parse_sections(ctx, source, 16).expect("list"), type_id)
        });
        assert!(!valid_references(&wrong_kind, type_at, |_| None));
    }
    let mut wrong_kind = section();
    wrong_kind.identity.type_id = RecordTypeId::from_bytes(SECTION_LIST_TYPE);
    assert!(!valid_references(&wrong_kind, type_at, |_| None));
}

#[test]
fn surface_loft_class_keeps_the_native_kind_and_refuses_solid_admission() {
    use cadmpeg_ir::features::{
        BooleanOp, FeatureDefinition, FeatureOperation, LoftGuidance, LoftSection,
        PlanarProfileRef, ProfileRef,
    };
    let class = crate::feature::ClassId::try_from("808b00afd41140881000fca5bba647b5".to_owned())
        .expect("surface loft class");
    assert_eq!(
        slots::Family::from_class_id(class),
        Some(slots::Family::SurfaceLoft)
    );
    assert_eq!(
        crate::feature::native_kind(Some(class)).as_str(),
        "LoftSurface"
    );
    assert!(crate::feature::FeatureFamily::from_class_id(class).is_none());
    let definition = FeatureDefinition::Operation(FeatureOperation::Loft {
        sections: ["first", "last"]
            .map(|selection| {
                LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::native(
                    selection.into(),
                )))
            })
            .to_vec(),
        guidance: LoftGuidance::default(),
        op: BooleanOp::NewBody,
        closed: false,
        solid: false,
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    });
    assert!(crate::feature::FeatureFamily::from_definition(&definition).is_none());
    assert!(!crate::feature::FeatureFamily::Loft.admits_definition(&definition));
    assert!(!controls::matches_definition(&definition));
}
