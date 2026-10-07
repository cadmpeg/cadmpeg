// SPDX-License-Identifier: Apache-2.0

use super::{
    finite_pairs, support_with_parameters, transfer_closed_wire_loops, CadIr, Curve, CurveGeometry,
    CurveId, HashMap, NonZeroUsize, Point3, ProceduralCurve, ProceduralCurveDefinition,
    ProceduralCurveId, SolvedCurveGeometry, Vector3, ZeroEntityLoopClass, ZeroEntityLoopMembers,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::AnnotationBuilder;

fn reversed_wire_fixture(ctx: &DecodeContext<'_>, procedural: bool) -> Result<usize, CodecError> {
    let first = Point3::new(0.0, 0.0, 0.0);
    let corner = Point3::new(1.0, 0.0, 0.0);
    let curve_id = CurveId::mint("catia:test:helix-curve#0".to_string()).expect("identity grammar");
    let construction_id = ProceduralCurveId::mint("catia:test:helix-construction#0".to_string())
        .expect("identity grammar");
    let definition = ProceduralCurveDefinition::Helix(
        cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
            [0.0, 1.0],
            cadmpeg_ir::geometry::HelixFrame {
                center: Point3::new(0.0, 0.0, 0.0),
                major: Vector3::new(1.0, 0.0, 0.0),
                minor: Vector3::new(0.0, 1.0, 0.0),
                pitch: Vector3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            0.2,
            None,
        )
        .expect("valid fixture construction"),
    );
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: if procedural {
            CurveGeometry::Procedural {
                construction: construction_id.clone(),
                cache: None,
            }
        } else {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    first,
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid fixture line"),
            ))
        },
        source_object: None,
    });
    if procedural {
        ir.model
            .add_procedural_curve(
                &cadmpeg_ir::document::admission::StandardAdmission,
                &curve_id,
                ProceduralCurve::new(construction_id.clone(), definition.clone()),
            )
            .expect("valid fixture construction")
            .expect("valid fixture construction");
    }
    let support_runs = vec![
        crate::families::zero_entity::records::ZeroEntitySupportRun {
            carrier_pos: 0,
            carrier_record_ordinal: 1,
            face: Some(crate::families::zero_entity::records::ZeroEntityFace {
                pos: 10,
                record_ordinal: 2,
                tag: [0x5f, 0x0c],
                allocations: vec![8],
                loops: Some(vec![
                    crate::families::zero_entity::records::ZeroEntityLoop {
                        pos: 20,
                        record_ordinal: 3,
                        tag: [0x62, 0x14],
                        members: ZeroEntityLoopMembers::try_new(
                            8,
                            1,
                            NonZeroUsize::new(2).expect("nonzero loop member count"),
                        )
                        .expect("admitted loop member run"),
                        typed_references: vec![1, 2],
                        support_record_ordinals: vec![4, 4],
                        loop_class: ZeroEntityLoopClass::Outer41,
                        forward_senses: vec![true, false],
                        oriented_model_endpoints: finite_pairs(vec![
                            [first, corner],
                            [corner, first],
                        ]),
                    },
                ]),
                terminal_control:
                    crate::families::zero_entity::records::ZeroEntityFaceControl::Control05,
            }),
            supports: vec![support_with_parameters(
                4,
                30,
                [first, corner],
                Some([1.0, 0.0]),
            )],
        },
    ];
    let support_curve_ids = HashMap::from([(4, curve_id.clone())]);
    let mut annotations = AnnotationBuilder::new();
    let counts = transfer_closed_wire_loops(
        &mut crate::families::FamilyEntityAdmission::new(ctx),
        &mut ir,
        &mut annotations,
        &support_runs,
        &support_curve_ids,
        None,
        &mut crate::nurbs::LaneRefusals::new(),
    )?;
    Ok(counts.edges)
}

fn assert_reversed_lookup_refusal(procedural: bool, operation: &'static str) {
    assert_eq!(
        crate::test_support::with_service_context(|ctx| reversed_wire_fixture(ctx, procedural))
            .expect("service wire fixture"),
        2
    );
    let result = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = reversed_wire_fixture(ctx, procedural);
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
    assert!(matches!(result,
        Err(CodecError::ResourceLimit(limit)) if limit.operation == operation));
}

#[test]
fn zero_entity_procedural_position_lookup_preserves_work_refusal() {
    assert_reversed_lookup_refusal(true, "catia_zero_wire_procedural_lookup");
}

#[test]
fn zero_entity_curve_position_lookup_preserves_work_refusal() {
    assert_reversed_lookup_refusal(false, "catia_zero_wire_curve_lookup");
}
