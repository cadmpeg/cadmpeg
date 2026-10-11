// SPDX-License-Identifier: Apache-2.0
//! Product scratch ownership and parent-local occurrence state.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::CadIr;

#[test]
fn standalone_body_group_buffers_release_with_each_group() {
    let mut records = String::new();
    for placement in 0..64 {
        let point = 1 + 2 * placement;
        writeln!(
            records,
            "#{point}=CARTESIAN_POINT('',({placement}.,0.,0.));"
        )
        .expect("point");
        writeln!(
            records,
            "#{}=AXIS2_PLACEMENT_3D('',#{point},$,$);",
            point + 1
        )
        .expect("placement");
    }
    for group in 0..16 {
        writeln!(records, "#{}=SHAPE_REPRESENTATION('',(),$);", 200 + group)
            .expect("representation");
        writeln!(
            records,
            "#{}=REPRESENTATION_MAP(#2,#{});",
            300 + group,
            200 + group
        )
        .expect("map");
        for placement in 0..64 {
            writeln!(
                records,
                "#{}=MAPPED_ITEM('',#{},#{});",
                1000 + 64 * group + placement,
                300 + group,
                2 + 2 * placement
            )
            .expect("mapped item");
        }
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("exchange");
    crate::test_support::with_service_context(source.as_bytes(), |_, setup| {
        let mut ir = CadIr::empty();
        let geometry =
            crate::reader::geometry::decode(&exchange, &mut ir, setup).expect("geometry");
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, setup).expect("carriers");
        let mut topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, setup)
            .expect("topology");
        let template = cadmpeg_ir::examples::unit_cube()
            .expect("body")
            .model
            .bodies
            .remove(0);
        for group in 0_u64..16 {
            let id = BodyId::from(crate::ids::data(crate::ids::kind!("body"), group));
            let mut body = template.clone();
            body.id = id.clone();
            body.transform = None;
            ir.model.bodies.push(body);
            topology.value.body_by_root.insert(200 + group, vec![id]);
        }
        let mut policy = DecodePolicy::service();
        // Sixteen groups share a scratch allowance; only one unique buffer is live.
        policy.limits.max_materialized_bytes = 480 * 1024;
        crate::test_support::with_policy_context(source.as_bytes(), &policy, |_, ctx| {
            let mut losses = Vec::new();
            let reports = std::cell::RefCell::new(
                ctx.reserve_scoped(0, "body group reports").expect("scope"),
            );
            super::super::apply_body_placements(
                &exchange,
                &geometry.value,
                &topology.value,
                &BTreeMap::new(),
                &mut ir,
                (&mut losses, &reports),
                ctx,
            )
            .expect("group storage retires before the next group");
            assert_eq!(losses.len(), 16);
            assert!(losses.iter().all(|loss| loss.code
                == crate::loss::StepLossCode::BodyConflictingMappedPlacements.kind()));
            assert!(ir.model.bodies.iter().all(|body| body.transform.is_none()));
        });
    });
}

#[test]
fn rejected_usage_names_release_with_the_rejected_usage() {
    for (parent, child) in [("$", "#1000"), ("#1000", "$")] {
        let mut records = String::from("#1000=UNUSED_REFERENCE();");
        let name = "A".repeat(1024);
        for id in 1..=128 {
            writeln!(
                records,
                "#{id}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u','{name}','',{parent},{child},$);"
            )
            .expect("usage");
        }
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("exchange");
        crate::test_support::with_service_context(source.as_bytes(), |_, setup| {
            let mut ir = CadIr::empty();
            let geometry =
                crate::reader::geometry::decode(&exchange, &mut ir, setup).expect("geometry");
            let carriers =
                crate::reader::index::CarrierIndex::from_ir(&ir, setup).expect("carriers");
            let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, setup)
                .expect("topology");
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 4096;
            policy.limits.max_retained_bytes = 0;
            crate::test_support::with_policy_context(source.as_bytes(), &policy, |_, ctx| {
                let stage = super::super::decode(
                    &exchange,
                    &geometry.value,
                    &topology.value,
                    &mut ir,
                    ctx,
                    &mut 0,
                )
                .expect("each rejected name releases its backing");
                assert!(stage.losses.is_empty());
                assert!(ir.model.product_definitions.is_empty());
                assert!(ir.model.occurrences.is_empty());
                drop(stage);
                ctx.reserve_scoped(4096, "rejected usage name release probe")
                    .expect("all scratch released");
            });
        });
    }
}

#[test]
fn child_ordinals_reset_for_each_repeated_parent_instance() {
    use cadmpeg_ir::products::OccurrenceParent;
    let decoded = crate::test_support::exchange::decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('P','parent','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('parent','',#4,#5);
#7=PRODUCT('S','subassembly','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('subassembly','',#8,#5);
#10=PRODUCT('L','leaf','',(#2));
#11=PRODUCT_DEFINITION_FORMATION('','',#10);
#12=PRODUCT_DEFINITION('leaf','',#11,#5);
#20=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u1','sub one','',#6,#9,$);
#21=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u2','sub two','',#6,#9,$);
#22=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u3','leaf one','',#9,#12,$);
#23=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u4','leaf two','',#9,#12,$);",
    );
    let occurrences = &decoded.ir().model.occurrences;
    assert_eq!(occurrences.len(), 7);
    let root = occurrences
        .iter()
        .find(|item| matches!(item.parent, OccurrenceParent::Root {}))
        .expect("root");
    assert_eq!(root.ordinal, 0);
    let children = occurrences.iter().filter(|item| matches!(&item.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &root.id)).collect::<Vec<_>>();
    assert_eq!(
        children.iter().map(|item| item.ordinal).collect::<Vec<_>>(),
        [0, 1]
    );
    for parent in children {
        let leaves = occurrences.iter().filter(|item| matches!(&item.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &parent.id)).collect::<Vec<_>>();
        assert_eq!(
            leaves.iter().map(|item| item.ordinal).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            leaves
                .iter()
                .map(|item| item.name.as_deref())
                .collect::<Vec<_>>(),
            [Some("leaf one"), Some("leaf two")]
        );
    }
}
