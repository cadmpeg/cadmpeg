use super::*;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope_with_type_guid;
use crate::design::test_support::design_type;
use crate::records::entity_header::SegmentType;
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, lp_utf16, with_decode_context};

const GENERATE: &str = "CA2AE956-FD81-4EBD-A601-F93A0F335722";
const COMBINE: &str = "8136D47E-3955-4682-8BD7-B353B7B45624";
const FIRST: &str = "11111111-2222-3333-4444-555555555555";
const SECOND: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

fn reference(bytes: &mut Vec<u8>, target: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(target).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
}

fn token(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

fn fixture(
    combine: bool,
    groups: &[&str],
    method: u32,
    tag: &str,
) -> (Vec<u8>, DesignParameterScope, SegmentType) {
    let kind = if combine {
        "CombineFaceGroups"
    } else {
        "GenerateFaceGroups"
    };
    let guid = if combine { COMBINE } else { GENERATE };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, tag.as_bytes().try_into().unwrap(), 700);
    bytes.extend_from_slice(&[0; 10]);
    if combine {
        token(&mut bytes, FIRST);
    } else {
        reference(&mut bytes, 91);
        bytes.push(0);
        reference(&mut bytes, 94);
        bytes.extend_from_slice(&method.to_le_bytes());
    }
    reference(&mut bytes, 88);
    reference(&mut bytes, 89);
    bytes.extend_from_slice(&u32::try_from(groups.len()).unwrap().to_le_bytes());
    for group in groups {
        token(&mut bytes, group);
    }
    reference(&mut bytes, 72);
    bytes.push(0);
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    for target in [72, 75, 91, 94] {
        reference(&mut bytes, target);
    }
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut bytes, kind);
    let mut tail = [0; 77];
    tail[..4].copy_from_slice(&1_u32.to_le_bytes());
    tail[31..35].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&tail);
    indexed_header(&mut bytes, *b"268", 700);
    let mut scope = parse_parameter_scope_with_type_guid(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        700,
        &DesignClassTag::try_from(tag.to_owned()).unwrap(),
        0,
        Some(guid),
    )
    .unwrap()
    .unwrap();
    scope.id = "f3d:native/BulkStream.dat:design-parameter-scope#0".into();
    let info = SegmentType::try_new(
        "f3d:native/MetaStream.dat:design-type#0".into(),
        design_type(
            guid,
            Some("90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF"),
            if combine { 1 } else { 3 },
            "Fusion",
            vec![700],
        ),
    )
    .unwrap();
    (bytes, scope, info)
}

fn decode(
    bytes: &[u8],
    scope: &DesignParameterScope,
    info: Option<&SegmentType>,
) -> Option<DesignMeshFaceGroups> {
    with_decode_context(|ctx| {
        decode_prefix(
            ctx,
            bytes,
            scope,
            info,
            Some((0, usize::try_from(scope.paired_byte_offset()).unwrap())),
        )
    })
    .unwrap()
}

#[test]
fn registered_prefixes_preserve_methods_nullable_owners_and_ordered_groups() {
    for tag in ["477", "612"] {
        for (combine, groups) in [
            (false, vec![]),
            (true, vec![SECOND, FIRST]),
            (true, vec![FIRST, SECOND, FIRST]),
        ] {
            let (bytes, scope, info) = fixture(combine, &groups, 1, tag);
            let construction = decode(&bytes, &scope, Some(&info)).unwrap();
            assert_eq!(
                construction.method_code.as_ref().map(|field| field.value),
                (!combine).then_some(1)
            );
            assert_eq!(construction.source_mesh.as_ref().unwrap().value, 89);
            assert_eq!(construction.related_mesh.as_ref().unwrap().value, 88);
            assert_eq!(
                construction
                    .groups
                    .iter()
                    .map(|group| group.value.as_str())
                    .collect::<Vec<_>>(),
                groups
            );
            assert_eq!(
                construction.group_count.as_ref().unwrap().value,
                u32::try_from(groups.len()).unwrap()
            );
            let mut scope = scope;
            match scope.payload_mut() {
                crate::records::feature::scope::DesignScopePayloadMut::GenerateFaceGroups(slot)
                | crate::records::feature::scope::DesignScopePayloadMut::CombineFaceGroups(slot) => {
                    *slot = Some(construction);
                }
                _ => panic!("face-group scope"),
            }
            let restored: DesignParameterScope =
                serde_json::from_value(serde_json::to_value(&scope).unwrap()).unwrap();
            assert_eq!(restored, scope);
        }
    }
}

#[test]
fn unsupported_registrations_do_not_enter_the_prefix_grammar() {
    let (bytes, scope, info) = fixture(false, &[], 0, "477");
    for mutation in 0..4 {
        let mut changed = info.clone();
        match mutation {
            0 => changed.set_version(4),
            1 => changed.set_module("Other".into()),
            2 => changed.set_base_type_guid(crate::records::entity_header::BaseTypeGuid::Absent),
            _ => changed.set_type_guid(COMBINE.to_owned().try_into().unwrap()),
        }
        assert!(decode(&bytes, &scope, Some(&changed)).is_none());
    }
    assert!(decode(&bytes, &scope, None).is_none());
    for range in [None, Some((1, bytes.len())), Some((0, bytes.len()))] {
        assert!(
            with_decode_context(|ctx| decode_prefix(ctx, &bytes, &scope, Some(&info), range))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn partial_fields_survive_missing_members_and_an_unknown_method() {
    let (mut bytes, scope, info) = fixture(false, &[FIRST, SECOND], 19, "477");
    let original = decode(&bytes, &scope, Some(&info)).unwrap();
    let first = usize::try_from(original.groups[0].offset).unwrap();
    bytes[first + 40] = b'?';
    let partial = decode(&bytes, &scope, Some(&info)).unwrap();
    assert_eq!(partial.method_code.unwrap().value, 19);
    assert_eq!(partial.group_count.unwrap().value, 2);
    assert_eq!(partial.groups.len(), 1);
    let count_at = usize::try_from(original.group_count.unwrap().offset).unwrap();
    bytes[count_at..count_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let bounded = decode(&bytes, &scope, Some(&info)).unwrap();
    assert_eq!(bounded.group_count.unwrap().value, u32::MAX);
    assert!(bounded.groups.is_empty());
    let truncated = decode(&bytes[..count_at], &scope, Some(&info));
    assert!(truncated.is_none());
}
