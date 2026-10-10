// SPDX-License-Identifier: Apache-2.0
//! Tessellation graph contexts and stage storage.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use std::fmt::Write;

#[test]
fn layered_tessellation_dag_work_is_bounded_by_graph_size() {
    for layers in [12_u64, 24, 48] {
        let mut records = String::from(super::ONE_TRIANGLE);
        records.push_str("#3=TESSELLATED_SOLID('',(#4,#5),$);");
        for layer in 0..layers {
            let left = 4 + 2 * layer;
            let (child_left, child_right) = if layer + 1 == layers {
                (2, 2)
            } else {
                (left + 2, left + 3)
            };
            for id in [left, left + 1] {
                if child_left == child_right {
                    write!(records, "#{id}=TESSELLATED_GEOMETRIC_SET('',(#{child_left}));").unwrap();
                } else {
                    write!(records, "#{id}=TESSELLATED_GEOMETRIC_SET('',(#{child_left},#{child_right}));").unwrap();
                }
            }
        }
        let mut policy = DecodePolicy::service();
        // The allowance grows with records; path expansion grows exponentially.
        policy.limits.max_work_units = 50_000 * layers;
        let ir = super::decode_tessellation_under_policy(&records, policy).unwrap();
        assert_eq!(ir.model.tessellations.len(), 1);
        assert_eq!(ir.model.tessellations[0].triangles(), [[0, 1, 2]]);
    }
}

#[test]
fn shared_tessellation_items_keep_distinct_inherited_placements() {
    let records = format!("{}\n#3=CARTESIAN_POINT('',(1.,0.,0.));
#4=CARTESIAN_POINT('',(2.,0.,0.));
#5=AXIS2_PLACEMENT_3D('',#3,$,$);
#6=AXIS2_PLACEMENT_3D('',#4,$,$);
#7=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#5) REPRESENTATION_ITEM('') TESSELLATED_GEOMETRIC_SET((#2)) TESSELLATED_ITEM());
#8=(GEOMETRIC_REPRESENTATION_ITEM() REPOSITIONED_TESSELLATED_ITEM(#6) REPRESENTATION_ITEM('') TESSELLATED_GEOMETRIC_SET((#2)) TESSELLATED_ITEM());
#9=TESSELLATED_GEOMETRIC_SET('',(#7,#8));
#10=TESSELLATED_ANNOTATION_OCCURRENCE('',(),#9);", super::ONE_TRIANGLE);
    let decoded = crate::test_support::exchange::decode_inline(&records);
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
    assert!(decoded.report().losses.iter().any(|loss| loss.message.contains("2 distinct repositioning placements")));
    let vertices = decoded.ir().model.tessellations[0].vertices();
    super::assert_point3_close(vertices[0].get(), cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
}

#[test]
fn tessellation_claims_and_report_slots_release_without_retained_backing() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=TESSELLATED_SOLID('',(),$);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner).unwrap();
    crate::test_support::with_service_context(source, |_, owner| {
        let mut ir = CadIr::empty();
        let geometry = crate::reader::geometry::decode(&exchange, &mut ir, owner).unwrap();
        let index = crate::reader::index::CarrierIndex::from_ir(&ir, owner).unwrap();
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &index, owner).unwrap();
        let expected = "TESSELLATED_SOLID #1 has no decoded exact body link";
        for hold in [true, false] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 8192;
            policy.limits.max_retained_bytes = u64::try_from(expected.len()).unwrap();
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let stage = super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, &ctx, &mut 0).unwrap();
            assert_eq!(stage.losses.len(), 1);
            assert_eq!(stage.losses[0].message, expected);
            assert_eq!(stage.claims, [1].into());
            let held = hold.then_some(stage);
            let probe = ctx.reserve_scoped(8192, "tessellation stage lifetime probe");
            if hold {
                assert!(matches!(probe, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes && limit.used > 0));
            } else {
                probe.expect("claim nodes and report slots released");
            }
            drop(held);
        }
    });
}

#[test]
fn support_surface_index_is_admitted_at_the_first_lookup() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=COMPLEX_TRIANGULATED_FACE('',#1,3,$,#5,(1,2,3),((1,2,3)),());
#3=CARTESIAN_POINT('',(0.,0.,0.));#4=AXIS2_PLACEMENT_3D('',#3,$,$);#5=PLANE('',#4);";
    super::assert_tessellation_collection_refusal(records, "step tessellation surface index");
}
