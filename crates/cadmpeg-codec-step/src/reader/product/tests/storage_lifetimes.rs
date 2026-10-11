// SPDX-License-Identifier: Apache-2.0
//! Lifetimes of temporary body placement groups.

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
        writeln!(records, "#{point}=CARTESIAN_POINT('',({placement}.,0.,0.));").expect("point");
        writeln!(records, "#{}=AXIS2_PLACEMENT_3D('',#{point},$,$);", point + 1).expect("placement");
    }
    for group in 0..16 {
        writeln!(records, "#{}=SHAPE_REPRESENTATION('',(),$);", 200 + group).expect("representation");
        writeln!(records, "#{}=REPRESENTATION_MAP(#2,#{});", 300 + group, 200 + group).expect("map");
        for placement in 0..64 {
            writeln!(records, "#{}=MAPPED_ITEM('',#{},#{});", 1000 + 64 * group + placement, 300 + group, 2 + 2 * placement).expect("mapped item");
        }
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("exchange");
    crate::test_support::with_service_context(source.as_bytes(), |_, setup| {
        let mut ir = CadIr::empty();
        let geometry = crate::reader::geometry::decode(&exchange, &mut ir, setup).expect("geometry");
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, setup).expect("carriers");
        let mut topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, setup).expect("topology");
        let template = cadmpeg_ir::examples::unit_cube().expect("body").model.bodies.remove(0);
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
            let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "body group reports").expect("scope"));
            super::super::apply_body_placements(&exchange, &geometry.value, &topology.value, &BTreeMap::new(), &mut ir, (&mut losses, &reports), ctx).expect("group storage retires before the next group");
            assert_eq!(losses.len(), 16);
            assert!(losses.iter().all(|loss| loss.code == crate::loss::StepLossCode::BodyConflictingMappedPlacements.kind()));
            assert!(ir.model.bodies.iter().all(|body| body.transform.is_none()));
        });
    });
}
