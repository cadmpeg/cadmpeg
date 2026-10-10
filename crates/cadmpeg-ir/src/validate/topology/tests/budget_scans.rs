// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::geometry::*;
use crate::geometry::surface_payloads::{CompoundLoftSurfacePayload, LawSurfacePayload};
use crate::index::{ModelIndex, StandardIndex};

fn work_before_first_finding(ir: &crate::CadIr) -> u64 {
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let ids = ModelIndex::build(ir, StandardIndex);
        let mut findings = Vec::new();
        super::super::check_references(&ctx, ir, &ids, &mut findings)
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, "validation finding message", run,
    );
    let CodecError::ResourceLimit(limit) = error else { panic!("first child finding refusal"); };
    limit.used
}

fn surface_fixture(definition: ProceduralSurfaceDefinition) -> crate::CadIr {
    let mut ir = crate::CadIr::empty();
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        "test:model:surface-construction#prefix".try_into().unwrap(), definition, None,
    ));
    ir
}

#[test]
fn topology_law_scans_do_not_precharge_unvisited_variables_or_formulas() {
    for additional in [false, true] {
        let fixture = |suffix: usize| {
            let mut variables = vec![LawExpression::Edge {
                curve: LoftPathCurve { id: "test:model:curve#missing".try_into().unwrap(), endpoints: None },
                parameters: [0.0, 1.0],
            }];
            variables.extend((0..suffix).map(|_| LawExpression::Null {}));
            let formula = LawFormula::Named { name: "fixture".try_into().unwrap(), variables };
            let (primary, additional) = if additional {
                let mut additional = vec![formula];
                additional.extend((0..suffix).map(|_| LawFormula::Null {}));
                (LawFormula::Null {}, additional)
            } else { (formula, Vec::new()) };
            surface_fixture(ProceduralSurfaceDefinition::Law(LawSurfacePayload::try_new(Box::new(LawSurfaceConstruction {
                parameter_ranges: None, primary, additional, tail: LawSurfaceTail::Historical {},
                discontinuities: std::array::from_fn(|_| Vec::new()),
            })).unwrap()))
        };
        // Both inputs reach one variable, and at most one additional formula, before the same child refuses.
        assert_eq!(work_before_first_finding(&fixture(0)), work_before_first_finding(&fixture(128)));
    }
}

#[test]
fn topology_scale_scans_do_not_precharge_unvisited_auxiliaries_or_members() {
    for members in [false, true] {
        let fixture = |suffix: usize| {
            let curve: crate::ids::CurveId = "test:model:curve#path".try_into().unwrap();
            let missing: crate::ids::CurveId = "test:model:curve#missing".try_into().unwrap();
            let auxiliaries = if members { Vec::new() } else { vec![missing.clone(); suffix + 1] };
            let members = if members { (0..=suffix).map(|_| CompoundLoftScaleMember {
                type_code: 0, curve: missing.clone(), data: ClassicLoftProfileData {
                    surface: "test:model:surface#missing".try_into().unwrap(), pcurve: None,
                    first_flag: false, asm_extension: 0,
                    subdata: LoftSubdata::Type211 { dimensions: [0, 0], row: [0.0, 0.0] }, direction: None,
                },
            }).collect() } else { Vec::new() };
            let scale = CompoundLoftScale { path: curve.clone(), auxiliaries, members, tail: [0, 0] };
            let mut ir = surface_fixture(ProceduralSurfaceDefinition::CompoundLoft(CompoundLoftSurfacePayload::try_new(
                CompoundLoftConstruction {
                    scales: CompoundLoftScales::try_new(vec![scale]).unwrap(), flags: [false; 2],
                    tail: CompoundLoftTail::Zero {
                        flags: [false; 2], direction: CompoundLoftDirection::Vector { value: crate::math::Vector3::new(1.0, 0.0, 0.0) },
                        trailing_flags: [false; 2],
                    },
                }, None,
            ).unwrap()));
            ir.model.curves.push(Curve { id: curve, source_object: None,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            });
            ir
        };
        // The first auxiliary or member reaches the same finding; the suffix is never visited.
        assert_eq!(work_before_first_finding(&fixture(0)), work_before_first_finding(&fixture(128)));
    }
}
