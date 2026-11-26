// SPDX-License-Identifier: Apache-2.0
//! A document whose current state holds many bodies and whose evaluations
//! each restate a few records of it: a stored history of one design.

use std::time::Instant;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::design::evaluation::StateBase;
use crate::design::feature::OperationFamily;
use crate::design::refs::{Evaluated, Member};
use crate::document::{CadIr, Topology};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use crate::validate::design::fixture::{
    after, design_findings, document, evaluation, feature, id, stored, unresolved,
};

/// The cube's records with `:cube:` in each identity replaced by
/// `:cube-<copy>:`.
fn cube_copy(cube: &Topology, copy: usize) -> Topology {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let mut map =
        IdentityMap::new(|id: &str| Ok(id.replacen(":cube:", &format!(":cube-{copy}:"), 1)));
    let copied = cube.clone().rewrite_identities(&ctx, &mut map).unwrap();
    map.finish().unwrap();
    ctx.finish_session().unwrap();
    copied
}

/// The baseline document with `bodies` cubes in its current state after the
/// fillet, and `evaluations` stored states after features placed before
/// it: the last on the current state, each other on the next. Evaluation `k`
/// restates one point of cube `k % bodies`.
pub(super) fn history(bodies: usize, evaluations: usize) -> CadIr {
    let mut ir = document();
    let cube = std::mem::take(&mut ir.geometry);
    for copy in 0..bodies {
        let mut copied = cube_copy(&cube, copy);
        let geometry = &mut ir.geometry;
        geometry.bodies.append(&mut copied.bodies);
        geometry.regions.append(&mut copied.regions);
        geometry.shells.append(&mut copied.shells);
        geometry.faces.append(&mut copied.faces);
        geometry.loops.append(&mut copied.loops);
        geometry.coedges.append(&mut copied.coedges);
        geometry.edges.append(&mut copied.edges);
        geometry.vertices.append(&mut copied.vertices);
        geometry.points.append(&mut copied.points);
        geometry.surfaces.append(&mut copied.surfaces);
        geometry.curves.append(&mut copied.curves);
        geometry.pcurves.append(&mut copied.pcurves);
    }
    ir.geometry.finalize();
    let all: Vec<_> = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    let part = id::<crate::ids::ProductDefinitionId>("definition", "part");
    for definition in &mut ir.model.product_definitions {
        if definition.id == part {
            definition.bodies.clone_from(&all);
        }
    }
    ir.design.scopes[0].current[0].bodies = all;
    let design = &mut ir.design;
    let keys: Vec<String> = (0..evaluations).map(|k| format!("step-{k}")).collect();
    let Some(crate::design::container::Container::Ordered { members, .. }) =
        design.containers.first_mut()
    else {
        panic!("the baseline's first container is its timeline");
    };
    let fillet = members.pop().unwrap();
    members.extend(keys.iter().map(|key| Member::Feature(feature(key))));
    members.push(fillet);
    design.features.extend(
        keys.iter()
            .map(|key| unresolved(key, OperationFamily::Extrude)),
    );
    design.evaluations.clear();
    for (k, key) in keys.iter().enumerate() {
        let base = if k + 1 == evaluations {
            StateBase::Current(after("fillet"))
        } else {
            StateBase::Evaluation(id("evaluation", &keys[k + 1]))
        };
        let mut content = stored(base);
        let point = ir.geometry.points[(k % bodies) * cube.points.len()].clone();
        content.delta.records.points.push(point);
        design.evaluations.push(evaluation(
            key,
            "base",
            Evaluated::State(after(key)),
            Some(content),
        ));
    }
    ir
}

#[test]
fn a_stored_history_of_many_bodies_is_valid() {
    let findings = design_findings(&history(4, 6));
    assert!(findings.is_empty(), "{findings:#?}");
}

/// Prints the design-validation time of stored histories of growing size.
#[test]
#[ignore = "timing probe: cargo test -p cadmpeg-ir --lib -- --ignored --nocapture stored_history_cost"]
fn stored_history_cost() {
    for bodies in [16, 64] {
        for evaluations in [16, 64] {
            let ir = history(bodies, evaluations);
            let started = Instant::now();
            let findings = design_findings(&ir);
            let elapsed = started.elapsed();
            assert!(findings.is_empty(), "{findings:#?}");
            eprintln!(
                "bodies {bodies:>3} evaluations {evaluations:>3} records {:>6}: {:>8.1} ms",
                record_count(&ir.geometry),
                elapsed.as_secs_f64() * 1e3
            );
        }
    }
}

fn record_count(topology: &Topology) -> usize {
    super::super::records(topology).len()
}
