// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::test_fixtures::parse as with_view;

fn fixture(prefix: usize) -> Vec<u8> {
    let mut bytes = vec![0xab; prefix];
    for value in 1_u32..=10 {
        bytes.extend_from_slice(&(value | 0x8000_0000).to_le_bytes());
    }
    for value in [71_u32, 81, 91, 14, 0x8000_000f] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn style_prefix_versions_preserve_operands_qualifiers_and_opaque_fields() {
    for (version, prefix) in [(20, 26), (23, 26), (29, 30), (31, 30)] {
        let mut bytes = fixture(prefix);
        let required = bytes.len();
        bytes.extend_from_slice(&[0x4f, 0x63]);
        let value = with_view(&bytes, |ctx, view| parse(ctx, view, version)).expect("style");
        assert_eq!(value.save_version_major, version);
        assert_eq!(value.prefix.as_ref(), vec![0xab; prefix]);
        assert_eq!(value.suffix.as_ref(), [0x4f, 0x63]);
        assert_eq!(value.thickness.index(), 1);
        assert!(value.thickness.qualified());
        assert_eq!(value.bend_radius.index(), 2);
        assert_eq!(value.bend_relief_form.index(), 3);
        assert_eq!(value.bend_relief_width.index(), 4);
        assert_eq!(value.bend_relief_depth.index(), 5);
        assert_eq!(value.corner_relief_form.index(), 6);
        assert_eq!(value.corner_relief_size.index(), 7);
        assert_eq!(value.seam_gap.index(), 8);
        assert_eq!(value.minimum_remnant.index(), 9);
        assert_eq!(value.option.index(), 10);
        assert_eq!(value.values, [71, 81, 91]);
        assert_eq!(value.transition_form.index(), 14);
        assert!(!value.transition_form.qualified());
        assert_eq!(value.transition_radius.index(), 15);
        assert!(value.transition_radius.qualified());
        for end in 0..required {
            assert!(with_view(&bytes[..end], |ctx, view| parse(ctx, view, version)).is_err());
        }
        let wire = serde_json::to_value(&value).expect("serialize");
        assert_eq!(
            serde_json::from_value::<StylePayload>(wire).expect("deserialize"),
            value
        );
    }
}

#[test]
fn style_prefix_rejects_other_version_grammars() {
    for version in [15, 19, 21, 22, 25, 28, 30, 32] {
        assert!(prefix_len(version).is_none());
        assert!(with_view(&fixture(30), |ctx, view| parse(ctx, view, version)).is_err());
    }
}

#[test]
fn style_native_links_require_the_exact_operand_types() {
    use crate::record_identity::{Located, RecordTypeId};
    use cadmpeg_ir::ids::IdentityKey;
    let payload = with_view(&fixture(30), |ctx, view| parse(ctx, view, 29)).expect("style");
    let style = Located::new(
        payload,
        RecordTypeId::from_bytes(TYPE),
        IdentityKey::try_new("fixture").expect("token"),
        100,
    );
    let parameter = crate::pmdc::type_id_string(super::super::PARAMETER_TYPE);
    let relief = crate::pmdc::type_id_string(super::super::RELIEF_FORM_TYPE);
    let transition = crate::pmdc::type_id_string(super::super::TRANSITION_FORM_TYPE);
    let boolean = crate::pmdc::type_id_string(crate::feature::BOOLEAN_TYPE);
    let own = crate::pmdc::type_id_string(TYPE);
    let type_at = |ordinal| match ordinal {
        100 => Some(own.as_str()),
        2 | 5 => Some(relief.as_str()),
        9 => Some(boolean.as_str()),
        13 => Some(transition.as_str()),
        0 | 1 | 3 | 4 | 6 | 7 | 8 | 14 => Some(parameter.as_str()),
        _ => None,
    };
    assert!(valid(&style, type_at));
    assert!(!valid(&style, |ordinal| if ordinal == 13 {
        Some(relief.as_str())
    } else {
        type_at(ordinal)
    }));
    assert!(!valid(&style, |ordinal| if ordinal == 3 {
        None
    } else {
        type_at(ordinal)
    }));
}
