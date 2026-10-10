// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;

fn compound(ctx: &DecodeContext<'_>, count: usize) -> Result<(), CodecError> {
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap()
    .unwrap();
    let mut carriers = Carriers::default();
    carriers.curve_geo.insert(
        7,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
    );
    carriers.procedural_curve_defs.insert(
        7,
        super::super::super::ProceduralCurveSource::Cached {
            construction: Box::new(ProceduralCurveConstruction::Compound(
                crate::nurbs::proc_curve::CompoundDefinition {
                    parameters: vec![0.0, 1.0],
                    components: (0..count)
                        .map(|_| cadmpeg_ir::geometry::CompoundComponent {
                            parameter: 0.0,
                            component: curve.clone(),
                        })
                        .collect(),
                },
            )),
            cache_fit_tolerance: None,
            parsed_domain: Some([0.0, 1.0]),
        },
    );
    emit_carrier_curve(
        ctx,
        &mut AsmBrep::default(),
        7,
        &mut carriers,
        &HashSet::new(),
        &HashSet::new(),
        crate::asm_format!("sat"),
    )
}

#[test]
fn compound_collector_charges_only_the_visited_member() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "ASM emitted identity copy",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            compound(&ctx, 4096)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("identity copy refusal");
    };
    assert_eq!(limit.operation, "ASM emitted identity copy");
    assert_eq!(limit.used, 1);
    assert_eq!(
        limit.additional,
        u64::try_from("sat:brep:procedural_curve#7:component:0".len()).unwrap()
    );
}

#[test]
fn compound_collector_refuses_before_the_first_callback() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "ASM compound curve components",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            compound(&ctx, 4096)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("member refusal");
    };
    assert_eq!(limit.operation, "ASM compound curve components");
    assert_eq!((limit.used, limit.additional), (0, 1));
}

fn shell_member_boundary(count: usize, vertices: bool) -> u64 {
    let records = [crate::test_support::sab::record(
        0,
        "shell".into(),
        vec![
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(1),
        ]
        .into(),
        0,
        0,
    )];
    let by_index = std::collections::HashMap::from([(0, &records[0])]);
    let members = (2..count + 2)
        .map(|index| i64::try_from(index).unwrap())
        .collect();
    let mut wire = WireShellTopology::default();
    let operation = if vertices {
        wire.free_vertices_by_shell.insert(0, members);
        "ASM shell free vertices"
    } else {
        wire.wire_edges_by_shell.insert(0, members);
        "ASM shell wire edges"
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            emit_containers(
                &ctx,
                &mut AsmBrep::default(),
                ContainerInputs {
                    records: &records,
                    by_index: &by_index,
                    reach: &Reachable::default(),
                    wire: &wire,
                    stream: "",
                    header_scale: 1.0,
                    format: crate::asm_format!("sat"),
                },
            )
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("shell member refusal");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 1);
    limit.used
}

#[test]
fn shell_wire_collector_refuses_without_paying_unvisited_sources() {
    assert_eq!(
        shell_member_boundary(1, false),
        shell_member_boundary(4096, false)
    );
}

#[test]
fn shell_vertex_collector_refuses_without_paying_unvisited_sources() {
    assert_eq!(
        shell_member_boundary(1, true),
        shell_member_boundary(4096, true)
    );
}
