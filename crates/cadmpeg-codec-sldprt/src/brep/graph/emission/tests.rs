// SPDX-License-Identifier: Apache-2.0

use crate::brep::topology::Bridge;
use cadmpeg_ir::topology::Sense;

#[test]
fn ambiguous_face_owner_stats_survive_when_all_uses_are_withheld() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let bridge = |attr, surface, offset| Bridge {
        attr,
        sequence: 0,
        refs: [0, 0, 0, 0, surface],
        sense: Sense::Forward,
        owner: Some(700),
        offset,
    };
    let mut tables = crate::brep::topology::Tables::default();
    tables
        .insert_bridge(&ctx, bridge(10, 100, 20))
        .expect("bridge");
    tables
        .insert_bridge(&ctx, bridge(11, 200, 10))
        .expect("bridge");
    let mut entity_facts = crate::brep::entity::FactsSource::empty(&ctx).expect("entity facts");
    entity_facts.entity_count = 1;
    let decoded = crate::brep::graph::emission::decode_graph(
        &ctx,
        &crate::brep::index::view::CarrierView::Retained(&crate::brep::index::CarrierIndex::default()),
        &tables,
        entity_facts,
        &crate::brep::typed::Facts::default(),
        &cadmpeg_ir::stream_name!("empty"),
    )
    .expect("valid exactness fields");

    assert!(decoded.faces.is_empty());
    assert_eq!(decoded.stats.ambiguous_face_owners, 1);
}

#[test]
fn topology_pruning_retains_a_procedural_blend_spine() {
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, Curve, CurveGeometry, ProceduralSurface,
        ProceduralSurfaceDefinition, SolvedCurveGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId};

    let spine = CurveId::mint("test:model:entity#spine").expect("identity grammar");
    let mut brep = crate::brep::graph::Brep {
        curves: vec![Curve {
            id: spine.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        }],
        procedural_surfaces: vec![ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:entity#blend").expect("identity grammar"),
            ProceduralSurfaceDefinition::Blend(
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    [None, None],
                    Some(spine.clone()),
                    BlendRadiusLaw::constant(0.5).unwrap(),
                    BlendCrossSection::Circular,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .unwrap(),
            ),
            None,
        )],
        ..Default::default()
    };

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    crate::brep::graph::emission::prune_rejected_topology(&ctx, &mut brep).expect("prune topology");
    assert_eq!(brep.curves.first().map(|curve| &curve.id), Some(&spine));
}

