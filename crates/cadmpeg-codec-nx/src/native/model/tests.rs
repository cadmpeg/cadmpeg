use super::{partition_bodies, terminal_feature_body_ids, without_images_before_partition};
use crate::native::segments::{SegmentBodyBinding, SegmentBodyLineageStatus};
use crate::parasolid::StreamKind;
use cadmpeg_ir::ids::BodyId;
use std::collections::BTreeSet;

#[test]
fn a_partition_body_replaces_cached_images_only_when_it_is_selected() {
    let binding = |ordinal: u32, stream_kind| SegmentBodyBinding {
        id: format!("binding#{ordinal}"),
        stream_link: format!("link#{ordinal}"),
        stream_ordinal: ordinal,
        stream_kind,
        body_object_index: 10,
        body_alias_object_index: 20,
        stream_role: 0,
        source_offset: 0,
    };
    let bodies = |ids: &[&str]| {
        ids.iter()
            .map(|id| BodyId::mint(*id).unwrap())
            .collect::<BTreeSet<_>>()
    };
    let bindings = [
        binding(0, StreamKind::Plain),
        binding(1, StreamKind::Partition),
    ];

    assert_eq!(
        without_images_before_partition(bodies(&["nx:s0:body#0", "nx:s1:body#0"]), &bindings),
        bodies(&["nx:s1:body#0"])
    );
    assert_eq!(
        without_images_before_partition(bodies(&["nx:s0:body#0"]), &bindings),
        bodies(&["nx:s0:body#0"])
    );
}

#[test]
fn a_part_without_segment_bindings_keeps_only_its_partition_bodies() {
    let bodies = |ids: &[&str]| {
        ids.iter()
            .map(|id| BodyId::mint(*id).unwrap())
            .collect::<BTreeSet<_>>()
    };
    let kinds = [
        StreamKind::Partition,
        StreamKind::Deltas,
        StreamKind::Plain,
        StreamKind::Plain,
    ];

    assert_eq!(
        partition_bodies(
            &bodies(&["nx:s0:body#3", "nx:s2:body#2", "nx:s3:body#2"]),
            &kinds
        ),
        bodies(&["nx:s0:body#3"])
    );
    assert!(partition_bodies(&bodies(&["nx:s2:body#2", "nx:s3:body#2"]), &kinds).is_empty());
}

#[test]
fn terminal_body_selection_joins_statuses_by_binding_identity() {
    ({
        let bindings = [0, 1].map(|ordinal| SegmentBodyBinding {
            id: format!("binding#{ordinal}"),
            stream_link: format!("link#{ordinal}"),
            stream_ordinal: ordinal,
            stream_kind: StreamKind::Partition,
            body_object_index: 10 + ordinal,
            body_alias_object_index: 20 + ordinal,
            stream_role: 0,
            source_offset: u64::from(ordinal),
        });
        let status = |binding: &SegmentBodyBinding, terminal| SegmentBodyLineageStatus {
            id: format!("status#{}", binding.stream_ordinal),
            segment_body_binding: binding.id.clone(),
            body_object_index: binding.body_object_index,
            body_alias_object_index: binding.body_alias_object_index,
            terminal,
            source_offset: binding.source_offset,
        };
        let emitted = ["nx:s0:body#0", "nx:s1:body#0"]
            .map(|id| BodyId::mint(id).unwrap())
            .into_iter()
            .collect::<BTreeSet<_>>();
        let statuses = [status(&bindings[1], true), status(&bindings[0], false)];
        let selected = BTreeSet::from([BodyId::mint("nx:s1:body#0").unwrap()]);
        assert_eq!(
            terminal_feature_body_ids(&emitted, &bindings, &statuses),
            Some(selected)
        );

        let mut mismatched = statuses.clone();
        mismatched[1].segment_body_binding = "missing".into();
        assert!(terminal_feature_body_ids(&emitted, &bindings, &mismatched).is_none());
        let duplicate = [statuses[0].clone(), statuses[0].clone()];
        assert!(terminal_feature_body_ids(&emitted, &bindings, &duplicate).is_none());
        assert!(terminal_feature_body_ids(&emitted, &bindings, &statuses[..1]).is_none());
        let mut extra = statuses.to_vec();
        let mut unmatched = statuses[0].clone();
        unmatched.segment_body_binding = "extra".into();
        extra.push(unmatched);
        assert!(terminal_feature_body_ids(&emitted, &bindings, &extra).is_none());
        let duplicate_bindings = [bindings[0].clone(), bindings[0].clone()];
        assert!(terminal_feature_body_ids(&emitted, &duplicate_bindings, &statuses).is_none());
    });
}
