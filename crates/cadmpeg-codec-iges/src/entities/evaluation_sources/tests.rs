// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, u64_from_index, DecodeArena, DecodePolicy, ResourceDimension, ResourceLimit};
use cadmpeg_ir::geometry::{analytic::{LineCurve, PlaneSurface}, CacheContract, CompoundComponent, CompoundCurveConstruction, CompositeCurveSegment, CompositeCurveTransition, Curve, ProceduralCurve, ProceduralSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};

const COUNT: usize = 64;
const METER: &str = "test evaluation completed prefix";

fn refusal(operation: &'static str, additional: u64, run: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>) -> ResourceLimit {
    let error = cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, operation, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = (cap == u64::MAX).then(|| RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(additional)));
        let result = run(&ctx);
        if let Err(CodecError::ResourceLimit(first)) = &result {
            assert_eq!(first.operation, operation);
            assert_eq!(first.additional, additional);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == *first));
        }
        result
    });
    let CodecError::ResourceLimit(limit) = error else { panic!("expected child work refusal"); };
    limit
}

fn prefix_work(run: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>) -> u64 {
    refusal(METER, 1, |ctx| { run(ctx)?; ctx.charge_work(1, METER) }).used
}

fn curve_key(position: usize, long: bool) -> CurveId {
    CurveId::mint(if long { format!("test:model:curve#{}", "x".repeat(101)) } else { format!("test:model:curve#{position:014}") }).unwrap()
}

fn line(id: CurveId) -> Curve {
    Curve { id, geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap())), source_object: None }
}

fn comparisons(count: usize) -> u64 {
    if count == 0 { 0 } else { u64_from_index(count).min(11 * (u64::from(count.div_ceil(2).ilog(6)) + 1)) }
}

#[test]
fn evaluation_position_index_child_refuses_first_and_last_without_visiting_the_tail() {
    for target in [0, COUNT - 1] {
        let values: Vec<_> = (0..COUNT).map(|position| line(curve_key(position, position == target))).collect();
        let before = values.clone();
        let used = prefix_work(|ctx| {
            index_positions(&values[..target], &mut BTreeMap::new(), false, ctx)?;
            ctx.charge_work(1, INDEX)
        });
        let amount = u64_from_index(values[target].id.as_str().len()) * comparisons(target).max(1);
        let limit = refusal(INDEX, amount, |ctx| {
            let mut positions = BTreeMap::new();
            let result = index_positions(&values, &mut positions, false, ctx);
            assert_eq!(positions.len(), target);
            assert_eq!(values, before);
            result
        });
        assert_eq!(limit.used, used);
    }
}

#[test]
fn evaluation_curve_owner_index_child_refuses_first_and_last_without_visiting_the_tail() {
    for target in [0, COUNT - 1] {
        let mut ir = CadIr::empty();
        ir.model.curves = (0..COUNT).map(|position| line(curve_key(position, false))).collect();
        let id = ProceduralCurveId::mint("test:model:procedural-curve#owner").unwrap();
        ir.model.curves[target].geometry = CurveGeometry::Procedural { construction: id.clone(), cache: None };
        ir.model.procedural_curves.push(ProceduralCurve::new(id, compound(vec![curve_key(0, false)])));
        let before = ir.clone();
        let used = prefix_work(|ctx| {
            index_positions(&ir.model.curves, &mut BTreeMap::new(), false, ctx)?;
            let mut constructions = BTreeMap::new();
            index_positions(&ir.model.procedural_curves, &mut constructions, true, ctx)?;
            // Only the selected carrier is procedural. Earlier owner visits
            // have no child work; the new group has no existing key comparisons.
            ctx.charge_work(u64_from_index(target + 1), INDEX)?;
            ctx.contains_key_btree_map(&constructions, ir.model.procedural_curves[0].id.as_str(), INDEX)?;
            ctx.copy_retained_text(ir.model.curves[target].id.as_str(), INDEX)?;
            Ok(())
        });
        let alignment = std::mem::align_of::<String>().max(std::mem::align_of::<Vec<usize>>()).max(std::mem::align_of::<usize>());
        // The first group pays one shift and two split passes of its node bound.
        let amount = 3 * u64_from_index(11 * (std::mem::size_of::<String>() + std::mem::size_of::<Vec<usize>>()) + 16 * std::mem::size_of::<usize>() + 2 * alignment);
        let limit = refusal(INDEX, amount, |ctx| {
            let result = EvaluationSources::new(&ir, ctx).map(|_| ());
            assert_eq!(ir, before);
            result
        });
        assert_eq!(limit.used, used);
    }
}

#[test]
fn evaluation_surface_construction_index_child_refuses_first_and_last_without_visiting_the_tail() {
    for target in [0, COUNT - 1] {
        let mut ir = CadIr::empty();
        ir.model.procedural_surfaces = (0..COUNT).map(|position| ProceduralSurface::new(
            ProceduralSurfaceId::mint(format!("test:model:procedural-surface#{}", if position == target { "x".repeat(101) } else { format!("{position:014}") })).unwrap(),
            ProceduralSurfaceDefinition::Unknown { record: None, cache: None }, None,
        )).collect();
        let before = ir.clone();
        let used = prefix_work(|ctx| {
            index_positions(&ir.model.procedural_surfaces[..target], &mut BTreeMap::new(), true, ctx)?;
            ctx.charge_work(1, INDEX)
        });
        let amount = u64_from_index(ir.model.procedural_surfaces[target].id.as_str().len()) * comparisons(target).max(1);
        let limit = refusal(INDEX, amount, |ctx| {
            let mut sources = EvaluationSources::default();
            let result = sources.extend_surfaces(&ir, ctx);
            assert_eq!(sources.surface_constructions.len(), target);
            assert_eq!((sources.indexed_surfaces, sources.indexed_procedural_surfaces), (0, 0));
            assert_eq!(ir, before);
            result
        });
        assert_eq!(limit.used, used);
    }
}

#[test]
fn evaluation_surface_index_child_refuses_first_and_last_without_visiting_the_tail() {
    for target in [0, COUNT - 1] {
        let mut ir = CadIr::empty();
        ir.model.surfaces = (0..COUNT).map(|position| Surface {
            id: SurfaceId::mint(format!("test:model:surface#{}", if position == target { "x".repeat(101) } else { format!("{position:014}") })).unwrap(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)).unwrap())), source_object: None,
        }).collect();
        let before = ir.clone();
        let used = prefix_work(|ctx| {
            index_positions(&ir.model.surfaces[..target], &mut BTreeMap::new(), false, ctx)?;
            ctx.charge_work(1, INDEX)
        });
        let amount = u64_from_index(ir.model.surfaces[target].id.as_str().len()) * comparisons(target).max(1);
        let limit = refusal(INDEX, amount, |ctx| {
            let mut sources = EvaluationSources::default();
            let result = sources.extend_surfaces(&ir, ctx);
            assert_eq!(sources.surfaces.len(), target);
            assert_eq!((sources.indexed_surfaces, sources.indexed_procedural_surfaces), (0, 0));
            assert_eq!(ir, before);
            result
        });
        assert_eq!(limit.used, used);
    }
}

#[test]
fn evaluation_composite_dependency_child_refuses_first_and_last_without_visiting_the_tail() {
    for target in [0, COUNT - 1] {
        let keys: Vec<_> = (0..COUNT).map(|position| curve_key(position, position == target)).collect();
        let geometry = SolvedCurveGeometry::Composite {
            segments: keys.iter().map(|id| CompositeCurveSegment { curve: id.clone(), same_sense: true, transition: CompositeCurveTransition::Continuous }).collect::<Vec<_>>().try_into().unwrap(),
            self_intersect: Some(false),
        };
        let before = geometry.clone();
        let sources = EvaluationSources { curves: BTreeMap::from([(keys[target].as_str().to_owned(), 0)]), ..EvaluationSources::default() };
        let used = u64_from_index(target + 1 + keys[..target].iter().map(|id| id.as_str().len()).sum::<usize>());
        let limit = refusal(LOOKUP, u64_from_index(keys[target].as_str().len()), |ctx| {
            let mut queue = Vec::new();
            let result = sources.curve_dependencies(&geometry, &mut queue, ctx);
            assert!(queue.is_empty());
            assert_eq!(geometry, before);
            result
        });
        assert_eq!(limit.used, used);
    }
}

fn compound(keys: Vec<CurveId>) -> ProceduralCurveDefinition {
    ProceduralCurveDefinition::Compound(CompoundCurveConstruction::try_new(Vec::new(), keys.into_iter().map(|id| CompoundComponent { parameter: 0.0, component: id }).collect(), None).unwrap())
}

fn graph() -> (CadIr, EvaluationSources) {
    let mut ir = CadIr::empty();
    let root = curve_key(0, false);
    ir.model.curves = vec![line(root.clone()), line(curve_key(1, true))];
    let construction = ProceduralSurfaceId::mint("test:model:procedural-surface#sweep").unwrap();
    let surface = SurfaceId::mint("test:model:surface#sweep").unwrap();
    ir.model.surfaces.push(Surface { id: surface.clone(), geometry: SurfaceGeometry::Procedural { construction: construction.clone(), cache: None }, source_object: None });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(construction.clone(), ProceduralSurfaceDefinition::Extrusion(
        cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(root.clone(), None, Vector3::new(0.0, 0.0, 1.0), None, CacheContract::legacy()).unwrap(),
    ), None));
    let sources = EvaluationSources {
        surfaces: BTreeMap::from([(surface.as_str().to_owned(), 0)]),
        surface_owners: BTreeMap::from([(surface.as_str().to_owned(), 0)]),
        surface_constructions: BTreeMap::from([(construction.as_str().to_owned(), 0)]),
        curves: BTreeMap::from([(root.as_str().to_owned(), 0), (curve_key(1, true).as_str().to_owned(), 1)]),
        ..EvaluationSources::default()
    };
    (ir, sources)
}

#[test]
fn evaluation_curve_owner_traversal_child_refuses_first_and_last_without_visiting_the_tail() {
    for target in [0, COUNT - 1] {
        let (mut ir, mut sources) = graph();
        let root = curve_key(0, false);
        let short = ProceduralCurveId::mint("test:model:procedural-curve#shared").unwrap();
        let long = ProceduralCurveId::mint(format!("test:model:procedural-curve#{}", "x".repeat(101))).unwrap();
        ir.model.curves = (0..COUNT).map(|position| Curve {
            id: root.clone(),
            geometry: CurveGeometry::Procedural { construction: if position == target { long.clone() } else { short.clone() }, cache: None },
            source_object: None,
        }).collect();
        ir.model.curves.push(line(root.clone()));
        sources.curves = BTreeMap::from([(root.as_str().to_owned(), COUNT)]);
        ir.model.procedural_curves = [short.clone(), long.clone()].into_iter().map(|id| ProceduralCurve::new(id, compound(vec![curve_key(99, false)]))).collect();
        sources.curve_constructions = BTreeMap::from([(short.as_str().to_owned(), 0), (long.as_str().to_owned(), 1)]);
        let before = ir.clone();
        let owners: Vec<_> = (0..COUNT).collect();
        sources.curve_owners.insert(root.as_str().to_owned(), owners[..target + 1].to_vec());
        let run = |sources: &EvaluationSources, ctx: &DecodeContext<'_>| {
            let result = sources.surface_model(&ir, ir.model.surfaces[0].id.as_str(), ctx).map(|_| ());
            assert_eq!(ir, before);
            result
        };
        let amount = 2 * u64_from_index(long.as_str().len());
        let prefix = refusal(LOOKUP, amount, |ctx| run(&sources, ctx));
        sources.curve_owners.insert(root.as_str().to_owned(), owners);
        let full = refusal(LOOKUP, amount, |ctx| run(&sources, ctx));
        assert_eq!((full.used, full.additional), (prefix.used, prefix.additional));
    }
}

#[test]
fn evaluation_compound_dependency_child_refuses_first_and_last_without_visiting_the_tail() {
    for target in [0, COUNT - 1] {
        let (mut ir, mut sources) = graph();
        let construction = ProceduralCurveId::mint("test:model:procedural-curve#compound").unwrap();
        let keys: Vec<_> = (0..COUNT).map(|position| curve_key(position + 1, position == target)).collect();
        ir.model.curves[0].geometry = CurveGeometry::Procedural { construction: construction.clone(), cache: None };
        sources.curve_owners.insert(ir.model.curves[0].id.as_str().to_owned(), vec![0]);
        sources.curve_constructions.insert(construction.as_str().to_owned(), 0);
        let amount = 2 * u64_from_index(keys[target].as_str().len());
        let run = |keys: &[CurveId]| {
            let mut source = ir.clone();
            source.model.procedural_curves.push(ProceduralCurve::new(construction.clone(), compound(keys.to_vec())));
            let before = source.clone();
            refusal(LOOKUP, amount, |ctx| {
                let result = sources.surface_model(&source, source.model.surfaces[0].id.as_str(), ctx).map(|_| ());
                assert_eq!(source, before);
                result
            })
        };
        let prefix = run(&keys[..target + 1]);
        let full = run(&keys);
        assert_eq!((full.used, full.additional), (prefix.used, prefix.additional));
    }
}
