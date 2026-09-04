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

    check!(&ir.model.bodies);
    check!(&ir.model.regions);
    check!(&ir.model.shells);
    check!(&ir.model.faces);
    check!(&ir.model.loops);
    check!(&ir.model.coedges);
    check!(&ir.model.edges);
    check!(&ir.model.vertices);
    check!(&ir.model.points);
    check!(&ir.model.surfaces);
    check!(&ir.model.curves);
    check!(&ir.model.pcurves);
    let unknowns = ir.native_unknowns("catia").unwrap();
    check!(&unknowns);
    assert_eq!(annotations.provenance.len(), entity_count);
}
