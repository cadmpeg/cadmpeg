// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn extrusion_side_rows_move_into_ordered_coedge_quartets() {
    let ir = crate::test_support::assert_retained_boundaries(
        &["creo extrusion generated identities", "creo extrusion entity ID copies"],
        |ctx| {
            let (scan, mut ir) = super::admitted_extrusion_fixture();
            let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
            assert_eq!(super::super::transfer_resolved_extrusion_breps(
                ctx, &scan, &mut ir, &mut AnnotationBuilder::new(), &mut diagnostics,
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )?, 1);
            assert!(diagnostics.rejected_extrusion_bodies.is_empty());
            Ok(ir)
        },
    );
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.faces.len(), 6);
    assert_eq!(ir.model.loops.len(), 6);
    assert_eq!(ir.model.coedges.len(), 24);
    for index in 0..4 {
        let next = (index + 1) % 4;
        let expected_coedges = [
            format!("creo:feature:extrusion#7:coedge:0:{index}:side-bottom"),
            format!("creo:feature:extrusion#7:coedge:0:{next}:side-vertical-out"),
            format!("creo:feature:extrusion#7:coedge:0:{index}:side-top"),
            format!("creo:feature:extrusion#7:coedge:0:{index}:side-vertical-in"),
        ];
        let expected_edges = [
            format!("creo:feature:extrusion#7:edge:0:{index}:bottom"),
            format!("creo:feature:extrusion#7:edge:0:{next}:vertical"),
            format!("creo:feature:extrusion#7:edge:0:{index}:top"),
            format!("creo:feature:extrusion#7:edge:0:{index}:vertical"),
        ];
        let expected_senses = [Sense::Forward, Sense::Forward, Sense::Reversed, Sense::Reversed];
        let face = &ir.model.faces[index];
        assert_eq!(face.id.as_str(), format!("creo:feature:extrusion#7:face:0:side:{index}"));
        for use_index in 0..4 {
            let coedge = &ir.model.coedges[8 + 4 * index + use_index];
            assert_eq!(coedge.id.as_str(), expected_coedges[use_index]);
            assert_eq!(coedge.edge.as_str(), expected_edges[use_index]);
            assert_eq!(coedge.sense, expected_senses[use_index]);
            assert_eq!(coedge.owner_loop, ir.model.loops[index + 2].id);
            let radial = ir.model.coedges.iter().find(|other| other.id == coedge.radial_next)
                .expect("radial peer exists");
            assert_eq!(radial.radial_next, coedge.id);
            assert_eq!(radial.edge, coedge.edge);
            assert_eq!(coedge.pcurves.len(), 1);
        }
    }
}
