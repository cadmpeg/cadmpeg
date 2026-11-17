// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::test_fixtures::{content_at, parse as with_view};

#[test]
fn flange_children_follow_the_state_header_without_stamps() {
    for version in [19, 20, 23, 29, 31] {
        let mut bytes = content_at(73, version, &[]);
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
        for value in [3_u32, 1, 0, 0x8000_0005, 6, 0x8000_0007, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let flange = with_view(&bytes, |ctx, view| parse(ctx, view, version)).expect("flange");
        assert_eq!(flange.header.source_index, 73);
        assert!(flange.header.stamps.is_empty());
        assert_eq!(flange.state, -1);
        assert_eq!(flange.value, 0);
        assert_eq!(
            flange
                .children
                .references()
                .iter()
                .map(|r| (r.index(), r.qualified()))
                .collect::<Vec<_>>(),
            [(5, true), (6, false), (7, true)]
        );
        for end in 0..bytes.len() {
            assert!(with_view(&bytes[..end], |ctx, view| parse(ctx, view, version)).is_err());
        }
        bytes.push(0);
        assert!(with_view(&bytes, |ctx, view| parse(ctx, view, version)).is_err());
    }
}

fn lookup(table: &[Option<String>], ordinal: u32) -> Option<&str> {
    table
        .get(usize::try_from(ordinal).expect("ordinal"))
        .and_then(Option::as_deref)
}

#[test]
fn contour_flange_parent_has_the_flange_layout_and_its_own_kind() {
    let mut bytes = content_at(73, 31, &[]);
    bytes.extend_from_slice(&(-1_i32).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
    for value in [3_u32, 3, 16, 0x8000_0005, 0x8000_0006, 0x8000_0007, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let payload = with_view(&bytes, |ctx, view| parse(ctx, view, 31)).expect("contour flange");
    let located = |type_id| {
        Located::new(
            payload.clone(),
            crate::record_identity::RecordTypeId::from_bytes(type_id),
            cadmpeg_ir::ids::IdentityKey::try_new("synthetic").expect("key"),
            2,
        )
    };
    let contour = located(CONTOUR_TYPE);
    let edge = located(TYPE);
    assert_eq!(FlangeKind::of(&contour), Some(FlangeKind::Contour));
    assert_eq!(FlangeKind::of(&edge), Some(FlangeKind::Edge));
    assert_eq!(
        FlangeKind::of(&located([0; 16])),
        None,
        "an unrelated type has no flange kind"
    );
    let feature = crate::pmdc::type_id_string(super::super::super::FEATURE_TYPE);
    let contour_type = crate::pmdc::type_id_string(CONTOUR_TYPE);
    let edge_type = crate::pmdc::type_id_string(TYPE);
    let types = |own: &str, ordinal: u32| match ordinal {
        2 => Some(own.to_owned()),
        4..=6 => Some(feature.clone()),
        _ => None,
    };
    let table = |own: &str| (0..8).map(|o| types(own, o)).collect::<Vec<_>>();
    let contour_table = table(&contour_type);
    assert!(valid(&contour, |o| lookup(&contour_table, o)));
    let edge_table = table(&edge_type);
    assert!(!valid(&contour, |o| lookup(&edge_table, o)));
    assert!(valid(&edge, |o| lookup(&edge_table, o)));
}
