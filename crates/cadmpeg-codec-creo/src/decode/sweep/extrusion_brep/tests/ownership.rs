// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn extrusion_side_rows_move_into_ordered_coedge_quartets() {
    let ir = crate::test_support::assert_retained_boundaries(
        &[
            "creo extrusion generated identities",
            "creo extrusion entity ID copies",
        ],
        |ctx| {
            let (scan, mut ir) = super::admitted_extrusion_fixture();
            let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
            assert_eq!(
                super::super::transfer_resolved_extrusion_breps(
                    ctx,
                    &scan,
                    &mut ir,
                    &mut AnnotationBuilder::new(),
                    &mut diagnostics,
                    &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
                )?,
                1
            );
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
        let expected_senses = [
            Sense::Forward,
            Sense::Forward,
            Sense::Reversed,
            Sense::Reversed,
        ];
        let face = &ir.model.faces[index];
        assert_eq!(
            face.id.as_str(),
            format!("creo:feature:extrusion#7:face:0:side:{index}")
        );
        for use_index in 0..4 {
            let coedge = &ir.model.coedges[8 + 4 * index + use_index];
            assert_eq!(coedge.id.as_str(), expected_coedges[use_index]);
            assert_eq!(coedge.edge.as_str(), expected_edges[use_index]);
            assert_eq!(coedge.sense, expected_senses[use_index]);
            assert_eq!(coedge.owner_loop, ir.model.loops[index + 2].id);
            let radial = ir
                .model
                .coedges
                .iter()
                .find(|other| other.id == coedge.radial_next)
                .expect("radial peer exists");
            assert_eq!(radial.radial_next, coedge.id);
            assert_eq!(radial.edge, coedge.edge);
            assert_eq!(coedge.pcurves.len(), 1);
        }
    }
}

#[test]
fn duplicate_extrusion_body_identity_needs_no_retained_storage() {
    let (scan, mut ir) = super::admitted_extrusion_fixture();
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        id: cadmpeg_ir::ids::BodyId::mint("creo:feature:extrusion#7:body").expect("body ID"),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let expected = ir.clone();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::super::transfer_resolved_extrusion_breps(
            &ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut crate::decode::surfaces::brep::BrepTransferDiagnostics::default(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default()
        )
        .expect("duplicate body identity remains scoped"),
        0
    );
    assert_eq!(ir, expected);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn extrusion_probes_multiple_nurbs_sides_before_brep_admission() {
    let (scan, mut ir) = super::admitted_extrusion_fixture();
    for entity in &mut ir.model.sketch_entities {
        let cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } =
            entity.geometry.definition()
        else {
            panic!("line fixture");
        };
        let curve = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                cadmpeg_ir::math::Point2::new(start.u, start.v),
                cadmpeg_ir::math::Point2::new(end.u, end.v),
            ],
            None,
            false,
        )
        .expect("fixture admission")
        .expect("line NURBS");
        entity.geometry = cadmpeg_ir::sketches::SketchGeometry::nurbs(curve);
    }
    let run = |limit| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_materialized_bytes = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("root");
        let mut output = ir.clone();
        let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
        let count = super::super::transfer_resolved_extrusion_breps(
            &ctx,
            &scan,
            &mut output,
            &mut AnnotationBuilder::new(),
            &mut diagnostics,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )?;
        assert_eq!(count, 1);
        assert!(diagnostics.rejected_extrusion_bodies.is_empty());
        Ok::<_, cadmpeg_core::CodecError>(output)
    };
    let limit = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        None,
        run,
    );
    let output = run(limit).expect("all reached materialized boundaries admitted");
    assert_eq!(output.model.bodies.len(), 1);
    assert_eq!(output.model.faces.len(), 6);
    assert_eq!(
        output
            .model
            .surfaces
            .iter()
            .filter(|surface| matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                    cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(_)
                )
            ))
            .count(),
        4
    );
}
