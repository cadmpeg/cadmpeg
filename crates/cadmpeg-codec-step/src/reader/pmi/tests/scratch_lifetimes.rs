// SPDX-License-Identifier: Apache-2.0
//! Per-record scratch is released before the next record.

use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::CadIr;

fn decode_with_scratch_cap(records: &str, cap: u64) -> CadIr {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("exchange");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
    let index = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology =
        crate::reader::topology::decode(&exchange, &mut ir, &index, &setup).expect("topology");
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, ctx)
            .expect("only live scratch fits");
    });
    ir
}

#[test]
fn geometric_usage_releases_each_discarded_annotation_set() {
    let mut records =
        String::from("#1=SHAPE_ASPECT('','',#99,.T.);#2=DIMENSIONAL_SIZE(#1,'');#99=ITEM();");
    for id in 100..356 {
        write!(
            records,
            "#{id}=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1,$,#99);"
        )
        .expect("usage record");
    }
    // Long-lived maps and one single-entry set fit in 8 KiB. Keeping 256
    // discarded sets would add 256 * (11*8 + 16*8 + 2*8) = 59,392 bytes.
    let ir = decode_with_scratch_cap(&records, 8192);
    assert_eq!(ir.model.pmi.len(), 1);
    assert_eq!(ir.model.pmi[0].targets.len(), 1);
}

#[test]
fn dimensions_release_each_aspect_deduplication_set() {
    let count = 256;
    let mut records = String::from("#1=SHAPE_ASPECT('','',#99,.T.);#99=ITEM();");
    for id in 100..100 + count {
        write!(records, "#{id}=DIMENSIONAL_SIZE(#1,'');").expect("dimension record");
    }
    // The live annotation, claim and aspect-group trees fit in 200 bytes
    // per dimension plus 10 KiB. Dead singleton sets would add 59,392 bytes.
    let ir = decode_with_scratch_cap(&records, 200 * count + 10_000);
    assert_eq!(
        ir.model.pmi.len(),
        usize::try_from(count).expect("dimension count")
    );
    assert!(ir
        .model
        .pmi
        .iter()
        .all(|annotation| annotation.targets.len() == 1));
}

fn tree_node_bytes<K, V>() -> usize {
    use std::mem::{align_of, size_of};
    let alignment = align_of::<K>()
        .max(align_of::<V>())
        .max(align_of::<usize>());
    11 * (size_of::<K>() + size_of::<V>()) + 16 * size_of::<usize>() + 2 * alignment
}

#[test]
fn geometric_usage_releases_consumed_indices_before_claim_growth() {
    use super::super::annotations::{AnnotationDraft, AnnotationIndex, Annotations};
    use cadmpeg_ir::pmi::{DimensionKind, PmiDefinition, PmiDimension, PmiTarget};
    use std::collections::{BTreeMap, BTreeSet};
    use std::mem::size_of;

    let source = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHAPE_ASPECT('','',#99,.T.);#2=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1,$,#99);#99=CARTESIAN_POINT('point',(0.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("usage exchange");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology =
        crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup).expect("topology");
    let points = super::super::point_sources(&ir, &setup).expect("point sources");
    let point = points[&99][0].clone();
    let curves = BTreeMap::new();
    let aspects = BTreeSet::from([1]);
    // Live storage: annotation index; aspect map and member set; target index
    // and its string member; four target-vector slots and two identity copies.
    // One final node holds either consumed indices or the new claim, not both.
    let cap = tree_node_bytes::<u64, AnnotationIndex>()
        + tree_node_bytes::<u64, BTreeSet<AnnotationIndex>>()
        + tree_node_bytes::<AnnotationIndex, ()>()
        + tree_node_bytes::<usize, super::super::TargetIndex>()
        + tree_node_bytes::<String, ()>()
        + 4 * size_of::<PmiTarget>()
        + 2 * point.as_str().len()
        + tree_node_bytes::<u64, ()>();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(cap).expect("live byte bound");
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut annotations = Annotations::new(ctx).expect("annotations");
        annotations
            .push(
                &mut ir,
                1,
                AnnotationDraft {
                    name: None,
                    targets: Vec::new(),
                    visible: None,
                    definition: PmiDefinition::Dimension(
                        PmiDimension::new(DimensionKind::Size, None, None).expect("dimension"),
                    ),
                },
            )
            .expect("annotation index");
        let mut typed = BTreeSet::new();
        let mut claims = ctx.reserve_scoped(0, "test usage claims").expect("claims");
        super::super::resolve_geometric_item_usages(
            &exchange,
            &topology.value,
            super::super::GeometrySources {
                points: &points,
                curves: &curves,
            },
            (&aspects, &annotations),
            &mut ir,
            (&mut typed, &mut claims),
            ctx,
        )
        .expect("consumed indices release before claim storage grows");
        assert_eq!(typed, BTreeSet::from([2]));
        assert_eq!(
            ir.model.pmi[0].targets,
            [PmiTarget::Point {
                point: point.clone()
            }]
        );
    });
}
