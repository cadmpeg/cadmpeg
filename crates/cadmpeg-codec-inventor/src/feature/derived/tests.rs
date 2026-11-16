// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::{parse_component, parse_source, Property};
use crate::feature::{PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload};
use crate::layout::pm_dc_derived_component_fields as fields;
use crate::test_support::test_fixtures::{content, content_at, push_u32};

fn component() -> Vec<u8> {
    let mut bytes = content(4);
    push_u32(&mut bytes, u32::MAX);
    for value in [1, 2, 3, 4] {
        push_u32(&mut bytes, 0x8000_0000 | value);
    }
    push_u32(&mut bytes, fields::MARKER_VALUE);
    push_u32(&mut bytes, fields::COUNT_VALUE);
    push_u32(&mut bytes, 0x8000_0005);
    bytes
}

fn parsed(
    bytes: &[u8],
    source: bool,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).expect("context");
    if source {
        parse_source(&ctx, root, version)
    } else {
        parse_component(&ctx, root, version)
    }
}

#[test]
fn component_has_four_operands_empty_list_and_terminal_reference() {
    let bytes = component();
    let record = parsed(&bytes, false, 19).expect("component");
    let PmDcFeaturePropertyKind::Derived {
        definition:
            Property::Component {
                state,
                references,
                terminal,
            },
    } = record.kind
    else {
        panic!("component form");
    };
    assert_eq!(state, -1);
    assert_eq!(
        references,
        [1, 2, 3, 4].map(|index| crate::pmdc::PmDcReference::from_packed(0x8000_0000 | index))
    );
    assert_eq!(
        terminal,
        crate::pmdc::PmDcReference::from_packed(0x8000_0005)
    );
    assert_eq!(
        bytes.len(),
        crate::layout::pm_dc_content_header::LEN + fields::LEN
    );
}

#[test]
fn component_rejects_nonempty_lists_wrong_markers_tails_and_truncation() {
    let bytes = component();
    for field in [fields::MARKER, fields::COUNT] {
        let mut changed = bytes.clone();
        let offset = crate::layout::pm_dc_content_header::LEN + field;
        changed[offset..offset + 4].copy_from_slice(&1_u32.to_le_bytes());
        assert!(parsed(&changed, false, 19).is_err());
    }
    for len in 0..bytes.len() {
        assert!(parsed(&bytes[..len], false, 19).is_err());
    }
    let mut tail = bytes;
    tail.push(0);
    assert!(parsed(&tail, false, 19).is_err());
    assert!(parsed(&tail, false, 18).is_err());
    assert!(parsed(&tail, false, 31).is_err());
}

#[test]
fn source_ends_after_one_component_reference() {
    let mut bytes = content(5);
    push_u32(&mut bytes, 0x8000_0006);
    let record = parsed(&bytes, true, 19).expect("source");
    assert!(matches!(record.kind, PmDcFeaturePropertyKind::Derived {
        definition: Property::Source { component }
    } if component == crate::pmdc::PmDcReference::from_packed(0x8000_0006)));
    for len in 0..bytes.len() {
        assert!(parsed(&bytes[..len], true, 19).is_err());
    }
    bytes.push(0);
    assert!(parsed(&bytes, true, 19).is_err());
    assert!(parsed(&bytes, true, 31).is_err());
}

#[test]
fn property_wire_keeps_all_source_references() {
    let parsed = parsed(&component(), false, 19).expect("component");
    let wire = serde_json::to_vec(&parsed).expect("serialize");
    let read: PmDcFeaturePropertyPayload = serde_json::from_slice(&wire).expect("deserialize");
    assert_eq!(read, parsed);
    let PmDcFeaturePropertyKind::Derived { definition } = &read.kind else {
        panic!("derived form");
    };
    assert_eq!(definition.references().len(), 5);
}

#[test]
fn unsupported_component_versions_lengths_and_lists_remain_untyped() {
    let bytes = component();
    for version in [18, 20, 22, 24, 26, 28, 32] {
        assert!(super::parser(
            super::COMPONENT_TYPE,
            cadmpeg_core::decode::View::over_retained(&bytes),
            version
        )
        .is_none());
    }
    assert!(super::parser(
        super::COMPONENT_TYPE,
        cadmpeg_core::decode::View::over_retained(&bytes),
        19
    )
    .is_some());
    for len in 0..bytes.len() {
        assert!(super::parser(
            super::COMPONENT_TYPE,
            cadmpeg_core::decode::View::over_retained(&bytes[..len]),
            19
        )
        .is_none());
    }
    let mut nonempty = bytes;
    let offset = crate::layout::pm_dc_content_header::LEN + fields::COUNT;
    nonempty[offset..offset + 4].copy_from_slice(&1_u32.to_le_bytes());
    assert!(super::parser(
        super::COMPONENT_TYPE,
        cadmpeg_core::decode::View::over_retained(&nonempty),
        19
    )
    .is_none());
}

#[test]
fn native_component_wire_cannot_add_an_unchecked_list_count() {
    let value = parsed(&component(), false, 19).expect("component");
    let mut wire = serde_json::to_value(value).expect("serialize");
    wire["kind"]["definition"]["count"] = 1.into();
    assert!(serde_json::from_value::<PmDcFeaturePropertyPayload>(wire).is_err());
}

#[test]
fn version_23_component_suffix_is_zero_and_source_header_has_one_stamp() {
    let mut bytes = component();
    push_u32(
        &mut bytes,
        crate::layout::pm_dc_derived_component_suffix::STATE_VALUE,
    );
    assert!(parsed(&bytes, false, 23).is_ok());
    assert!(super::parser(
        super::COMPONENT_TYPE,
        cadmpeg_core::decode::View::over_retained(&bytes),
        23
    )
    .is_some());
    assert!(parsed(&bytes, false, 19).is_err());
    let suffix = bytes.len() - crate::layout::pm_dc_derived_component_suffix::LEN;
    for value in [1_u32, u32::MAX] {
        bytes[suffix..].copy_from_slice(&value.to_le_bytes());
        assert!(parsed(&bytes, false, 23).is_err());
        assert!(super::parser(
            super::COMPONENT_TYPE,
            cadmpeg_core::decode::View::over_retained(&bytes),
            23
        )
        .is_none());
    }
    let mut source = content(5);
    push_u32(&mut source, u32::MAX);
    push_u32(&mut source, 0x8000_0006);
    let result = parsed(&source, true, 23).expect("version-23 source");
    assert_eq!(result.header.stamps, [-1]);
    assert!(
        matches!(result.kind, PmDcFeaturePropertyKind::Derived { definition: Property::Source { component } } if component == crate::pmdc::PmDcReference::from_packed(0x8000_0006))
    );
    assert!(super::parser(
        super::SOURCE_TYPE,
        cadmpeg_core::decode::View::over_retained(&source),
        23
    )
    .is_some());
    assert!(super::parser(
        super::SOURCE_TYPE,
        cadmpeg_core::decode::View::over_retained(&source),
        19
    )
    .is_none());
    for len in 0..source.len() {
        assert!(parsed(&source[..len], true, 23).is_err());
    }
}

fn view(bytes: &[u8]) -> cadmpeg_core::decode::View<'_> {
    cadmpeg_core::decode::View::over_retained(bytes)
}

fn source_at(version: u8, stamps: &[i32]) -> Vec<u8> {
    let mut bytes = content_at(5, version, stamps);
    push_u32(&mut bytes, 0x8000_0006);
    bytes
}

fn component_at(version: u8, suffix: u32) -> Vec<u8> {
    let mut bytes = content_at(4, version, &[]);
    push_u32(&mut bytes, u32::MAX);
    for value in [1, 2, 3, 4] {
        push_u32(&mut bytes, 0x8000_0000 | value);
    }
    push_u32(&mut bytes, fields::MARKER_VALUE);
    push_u32(&mut bytes, fields::COUNT_VALUE);
    push_u32(&mut bytes, 0x8000_0005);
    push_u32(&mut bytes, suffix);
    bytes
}

#[test]
fn versions_25_and_31_source_has_two_stamps_and_component_has_suffix() {
    for (version, source_len, component_len) in [(25_u8, 34_usize, 58_usize), (31, 38, 62)] {
        let source = source_at(version, &[-1, 7]);
        assert_eq!(source.len(), source_len);
        let record = parsed(&source, true, version).expect("source");
        assert_eq!(record.header.stamps, [-1, 7]);
        assert_eq!(record.header.flags_extension.is_some(), version == 31);
        assert!(super::parser(super::SOURCE_TYPE, view(&source), version).is_some());

        let component = component_at(version, 0);
        assert_eq!(component.len(), component_len);
        let record = parsed(&component, false, version).expect("component");
        assert!(record.header.stamps.is_empty());
        assert!(super::parser(super::COMPONENT_TYPE, view(&component), version).is_some());
    }
}

#[test]
fn versions_25_and_31_refuse_wrong_stamp_count_length_suffix_and_version() {
    for version in [25_u8, 31] {
        for stamps in [&[][..], &[-1][..], &[-1, 7, 8][..]] {
            let source = source_at(version, stamps);
            assert!(super::parser(super::SOURCE_TYPE, view(&source), version).is_none());
            assert!(parsed(&source, true, version).is_err());
        }
        let source = source_at(version, &[-1, 7]);
        assert!(super::parser(super::SOURCE_TYPE, view(&source), version ^ 2).is_none());
        for len in 0..source.len() {
            assert!(parsed(&source[..len], true, version).is_err());
        }
        let mut long = source;
        long.push(0);
        assert!(parsed(&long, true, version).is_err());
        assert!(super::parser(super::SOURCE_TYPE, view(&long), version).is_none());

        let component = component_at(version, 0);
        for len in 0..component.len() {
            assert!(
                super::parser(super::COMPONENT_TYPE, view(&component[..len]), version).is_none()
            );
            assert!(parsed(&component[..len], false, version).is_err());
        }
        let no_suffix = &component[..component.len() - 4];
        assert!(super::parser(super::COMPONENT_TYPE, view(no_suffix), version).is_none());
        for suffix in [1, u32::MAX] {
            let bad = component_at(version, suffix);
            assert!(super::parser(super::COMPONENT_TYPE, view(&bad), version).is_none());
            assert!(parsed(&bad, false, version).is_err());
        }
    }
    let v25_source = source_at(25, &[-1, 7]);
    assert!(super::parser(super::SOURCE_TYPE, view(&v25_source), 31).is_none());
    let v31_component = component_at(31, 0);
    assert!(super::parser(super::COMPONENT_TYPE, view(&v31_component), 25).is_none());
}
