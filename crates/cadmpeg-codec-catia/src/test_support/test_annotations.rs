// SPDX-License-Identifier: Apache-2.0
//! Provenance checks shared by decode and integration suites.

#![allow(clippy::unwrap_used)]
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::Annotations;

pub(crate) fn assert_every_entity_has_v1_annotation(ir: &CadIr, annotations: &Annotations) {
    let mut entity_count = 0;
    macro_rules! check {
        ($entities:expr) => {
            for entity in $entities {
                entity_count += 1;
                let provenance = &annotations.provenance[entity.id.as_str()];
                assert!(provenance.stream().starts_with("catia:"));
            }
        };
    }

    check!(&ir.geometry.bodies);
    check!(&ir.geometry.regions);
    check!(&ir.geometry.shells);
    check!(&ir.geometry.faces);
    check!(&ir.geometry.loops);
    check!(&ir.geometry.coedges);
    check!(&ir.geometry.edges);
    check!(&ir.geometry.vertices);
    check!(&ir.geometry.points);
    check!(&ir.geometry.surfaces);
    check!(&ir.geometry.curves);
    check!(&ir.geometry.pcurves);
    check!(&ir.geometry.procedural_curves);
    check!(&ir.geometry.procedural_surfaces);
    let unknowns = ir.native_unknowns("catia").unwrap();
    check!(&unknowns);
    assert_eq!(annotations.provenance.len(), entity_count);
}
