// SPDX-License-Identifier: Apache-2.0

use crate::geometry::procedural_surfaces::{ExactSpline, ProceduralSurfaceDefinition};
use crate::geometry::surface_payloads::ExactSurfacePayload;
use crate::geometry::{ProceduralSurface, SurfaceGeometry};
use crate::ids::ProceduralSurfaceId;

/// The unit cube with one procedural surface, the construction of the
/// cube's surfaces whose indices `users` lists.
fn cube_with_procedural(users: &[usize]) -> crate::CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    let construction = ProceduralSurfaceId::mint("test:model:procedural-surface#0").unwrap();
    for &user in users {
        ir.geometry.surfaces[user].geometry = SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        };
    }
    let definition = ProceduralSurfaceDefinition::Exact(
        ExactSurfacePayload::try_new(ExactSpline::Legacy {
            ranges: [[0.0, 1.0], [0.0, 1.0]],
            extension: 0,
            cache: None,
        })
        .unwrap(),
    );
    ir.geometry
        .procedural_surfaces
        .push(ProceduralSurface::new(construction, definition, None));
    ir
}

fn owner_findings(ir: &crate::CadIr) -> Vec<String> {
    let mut findings = Vec::new();
    super::check_procedural_owners(ir, &mut findings);
    findings
        .into_iter()
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn a_procedural_surface_is_the_construction_of_exactly_one_surface() {
    assert_eq!(
        owner_findings(&cube_with_procedural(&[0, 1])),
        ["procedural surface is the construction of 2 surfaces; it is the construction of exactly one"]
    );
    assert_eq!(
        owner_findings(&cube_with_procedural(&[])),
        ["procedural surface is the construction of 0 surfaces; it is the construction of exactly one"]
    );
}

#[test]
fn a_procedural_surface_of_one_surface_breaks_no_rule() {
    assert!(owner_findings(&cube_with_procedural(&[0])).is_empty());
}
