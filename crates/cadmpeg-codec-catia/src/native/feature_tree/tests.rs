use super::CatiaCompactClassToken::{Selector, Tag};
use super::{CatiaFeatureContainer, CatiaFeatureMember, CatiaFeatureReference, CatiaSegmentValue};
use crate::native::CatiaNative;
use crate::test_support::test_feature_tree::{
    escaped, part, stream, wide, CURRENT_FEATURE, FTR_LIST, GSM_PLANE, HYBRID_BODY, MAIN_TOOL,
    MECHANICAL_PART, MECHANICAL_TOOL, PAD, REFERENCE_PLANES,
};

fn container(native: &CatiaNative, entity_id: u32) -> &CatiaFeatureContainer {
    native
        .feature_containers
        .iter()
        .find(|container| container.entity_id == entity_id)
        .expect("container is read")
}

/// The stored `_FtrList`: the segment's list, an empty list when a complete
/// walk has no such segment, or `None` when it is unread.
fn members(container: &CatiaFeatureContainer) -> Option<&[CatiaFeatureMember]> {
    let mut lists = container
        .segments
        .iter()
        .filter(|segment| segment.name == "_FtrList");
    match (lists.next(), lists.next()) {
        (Some(segment), None) => match &segment.value {
            CatiaSegmentValue::List(members) => Some(members),
            CatiaSegmentValue::Token(_) => None,
        },
        (None, None) if container.unsegmented_payload_offset.is_none() => Some(&[]),
        _ => None,
    }
}

fn token<'c>(
    container: &'c CatiaFeatureContainer,
    name: &str,
) -> Option<&'c CatiaFeatureReference> {
    container
        .segments
        .iter()
        .find_map(|segment| match &segment.value {
            CatiaSegmentValue::Token(token) if segment.name == name => Some(token),
            _ => None,
        })
}

fn member_classes(members: &[CatiaFeatureMember]) -> Vec<(u32, Option<&str>)> {
    members
        .iter()
        .map(|member| {
            (
                member.entity_id,
                member
                    .target
                    .as_ref()
                    .and_then(|target| target.class.as_ref())
                    .map(|class| class.name.as_str()),
            )
        })
        .collect()
}

#[test]
fn compact_definition_tag_selects_the_class_entry() {
    let native = CatiaNative::decode(&stream(&part()));
    let classes = native
        .entity_records
        .iter()
        .map(|entity| {
            let class = entity.compact_class.as_ref().expect("compact class");
            (
                entity.entity_id,
                class.token,
                class.ordinal,
                class.name.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        classes,
        [
            (2, Tag(MECHANICAL_PART), 4, "MechanicalPart"),
            (3, Tag(HYBRID_BODY), 11, "HybridBody"),
            (4, Tag(GSM_PLANE), 13, "GSMPlane"),
            (5, Tag(PAD), 12, "Pad"),
            (6, Selector(13), 12, "Pad"),
            (7, Tag(MECHANICAL_TOOL), 10, "MechanicalTool"),
        ]
    );
    assert!(native.entity_records.iter().all(|entity| entity
        .compact_class
        .as_ref()
        .is_some_and(|class| class.offset == 2)));
}

#[test]
fn compact_definition_rejects_other_prefix_shapes() {
    let records = vec![
        (2, vec![0x00, 0x01, MECHANICAL_PART, 0x39], vec![0xfe]),
        (3, vec![0x00, 0x00, MECHANICAL_PART], vec![0xfe]),
        (4, vec![0x00, 0x00, MECHANICAL_PART, 0x39, 0x00], vec![0xfe]),
        (5, vec![0x01, 0x00, MECHANICAL_PART, 0x39], vec![0xfe]),
        (6, vec![0x00, 0x00, 0x33, 0x39], vec![0xfe]),
        (7, vec![0x00, 0x00, 0x43, 0x39], vec![0xfe]),
        (8, wide(0x00, 0), vec![0xfe]),
        (9, wide(0x00, 16), vec![0xfe]),
    ];
    let native = CatiaNative::decode(&stream(&records));
    assert_eq!(native.entity_records.len(), 8);
    assert!(native
        .entity_records
        .iter()
        .all(|entity| entity.compact_class.is_none()));
    assert!(native.feature_containers.is_empty());
}

#[test]
fn part_root_lists_its_members_in_stored_order() {
    let native = CatiaNative::decode(&stream(&part()));
    let root = container(&native, 2);
    assert_eq!(root.class.name, "MechanicalPart");
    assert_eq!(root.unsegmented_payload_offset, None);
    assert_eq!(
        root.segments
            .iter()
            .map(|segment| (segment.payload_offset, segment.tag, segment.name.as_str()))
            .collect::<Vec<_>>(),
        [
            (0, FTR_LIST, "_FtrList"),
            (18, CURRENT_FEATURE, "CurrentFeature"),
            (20, MAIN_TOOL, "MainTool"),
            (22, REFERENCE_PLANES, "_ReferencePlanes"),
        ]
    );
    let members = members(root).expect("root members");
    assert_eq!(
        member_classes(members),
        [
            (4, Some("GSMPlane")),
            (3, Some("HybridBody")),
            (7, Some("MechanicalTool")),
        ]
    );
    assert_eq!(
        members
            .iter()
            .map(|member| (member.payload_offset, member.persistent_id))
            .collect::<Vec<_>>(),
        [(3, 0x1000), (9, 0x1001), (15, 0x7f)]
    );
}

#[test]
fn part_root_stores_its_in_work_object_and_main_body() {
    let native = CatiaNative::decode(&stream(&part()));
    let root = container(&native, 2);
    let current = token(root, "CurrentFeature").expect("in-work object");
    assert_eq!((current.payload_offset, current.entity_id), (19, 5));
    assert_eq!(
        current
            .target
            .as_ref()
            .and_then(|target| target.class.as_ref())
            .map(|class| class.name.as_str()),
        Some("Pad")
    );
    let main = token(root, "MainTool").expect("main body");
    assert_eq!((main.payload_offset, main.entity_id), (21, 3));
}

#[test]
fn part_root_lists_its_reference_planes_outside_its_sets() {
    let native = CatiaNative::decode(&stream(&part()));
    let root = container(&native, 2);
    let planes = root
        .segments
        .iter()
        .find_map(|segment| match &segment.value {
            CatiaSegmentValue::List(items) if segment.name == "_ReferencePlanes" => Some(items),
            _ => None,
        })
        .expect("reference planes");
    assert_eq!(
        planes
            .iter()
            .map(|plane| (plane.payload_offset, plane.entity_id, plane.persistent_id))
            .collect::<Vec<_>>(),
        [(25, 4, 0)]
    );
    assert_eq!(member_classes(planes), [(4, Some("GSMPlane"))]);
}

#[test]
fn bodies_list_their_features_and_an_empty_body_lists_none() {
    let native = CatiaNative::decode(&stream(&part()));
    let body = container(&native, 3);
    assert_eq!(body.class.name, "HybridBody");
    assert_eq!(
        member_classes(members(body).expect("body members")),
        [(5, Some("Pad")), (6, Some("Pad"))]
    );
    let empty = container(&native, 7);
    assert_eq!(empty.segments, []);
    assert_eq!(empty.unsegmented_payload_offset, None);
    assert_eq!(members(empty), Some(&[][..]));
    assert_eq!(
        native
            .feature_containers
            .iter()
            .map(|container| container.entity_id)
            .collect::<Vec<_>>(),
        [2, 3, 7]
    );
}

#[test]
fn escaped_count_and_entity_tokens_store_the_value_itself() {
    let mut records = part();
    let mut body = vec![FTR_LIST];
    body.extend(escaped(2));
    body.push(0x81);
    body.extend(escaped(5));
    body.extend(escaped(0x2000));
    body.push(0x87);
    body.extend(escaped(0x2001));
    body.push(0xfe);
    records[1].2 = body;
    let native = CatiaNative::decode(&stream(&records));
    let body = container(&native, 3);
    let members = members(body).expect("body members");
    assert_eq!(
        member_classes(members),
        [(5, Some("Pad")), (6, Some("Pad"))]
    );
    assert_eq!(members[0].payload_offset, 7);
}

#[test]
fn unresolved_member_keeps_its_stored_identity() {
    let mut records = part();
    let mut body = vec![FTR_LIST, 0x82, 0x81, 0xa0];
    body.extend(escaped(0x2000));
    body.push(0xfe);
    records[1].2 = body;
    let native = CatiaNative::decode(&stream(&records));
    let members = members(container(&native, 3)).expect("body members");
    assert_eq!(members.len(), 1);
    assert_eq!(
        (members[0].entity_id, members[0].persistent_id),
        (31, 0x2000)
    );
    assert_eq!(members[0].target, None);
}

#[test]
fn an_unsegmented_payload_before_the_list_leaves_members_unread() {
    let mut records = part();
    records[1].2 = vec![CURRENT_FEATURE, 0x86, 0x10, 0x86, 0xfe];
    let native = CatiaNative::decode(&stream(&records));
    let body = container(&native, 3);
    assert_eq!(body.segments.len(), 1);
    assert_eq!(body.unsegmented_payload_offset, Some(2));
    assert_eq!(members(body), None);
}

#[test]
fn a_short_list_is_a_single_token_and_not_members() {
    let mut records = part();
    records[1].2 = vec![FTR_LIST, 0x84, 0x81, 0x86, 0xfe];
    let native = CatiaNative::decode(&stream(&records));
    let body = container(&native, 3);
    assert_eq!(members(body), None);
    assert_eq!(body.unsegmented_payload_offset, Some(2));
}

#[test]
fn feature_containers_round_trip_through_the_native_wire() {
    let native = CatiaNative::decode(&stream(&part()));
    let wire = cadmpeg_core::serde_erase::json::to_value(&native.feature_containers)
        .expect("serialize containers");
    let back = cadmpeg_core::serde_erase::json::from_value::<Vec<CatiaFeatureContainer>>(wire)
        .expect("containers read back");
    assert_eq!(back, native.feature_containers);
    let entities = cadmpeg_core::serde_erase::json::to_value(&native.entity_records)
        .expect("serialize entities");
    let back = cadmpeg_core::serde_erase::json::from_value::<
        Vec<crate::native::entity_record::CatiaEntityRecord>,
    >(entities)
    .expect("entities read back");
    assert_eq!(back, native.entity_records);
}
