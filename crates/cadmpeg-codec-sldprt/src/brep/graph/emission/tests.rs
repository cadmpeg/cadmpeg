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
        crate::brep::index::view::CarrierView::Retained(
            &crate::brep::index::CarrierIndex::default(),
        ),
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

#[test]
fn analytic_graph_skips_procedural_and_appearance_index_work() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let bytes = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    let mut options = DecodeOptions::default();
    options.policy.limits.max_work_units = u64::MAX;
    for operation in [
        "index Parasolid surface face edges",
        "select Parasolid face surface carriers",
        "index Parasolid face owners",
        "index Parasolid body attributes",
    ] {
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let decoded = crate::SldprtCodec
            .decode(&mut std::io::Cursor::new(&bytes), &options)
            .unwrap();
        drop(probe);
        assert_eq!(decoded.ir().model.faces.len(), 1);
        assert_eq!(decoded.ir().model.bodies.len(), 1);
    }
}

fn rolling_ball_record(attr: u16, selector: u8, references: [u16; 3], values: [f64; 4]) -> Vec<u8> {
    use crate::test_support::parasolid::{be16, be32, bef64};
    let mut bytes = vec![0, 0x38];
    be16(&mut bytes, attr);
    be32(&mut bytes, 0);
    bytes.extend([0; 10]);
    bytes.extend([0x2b, selector]);
    for reference in references {
        be16(&mut bytes, reference);
    }
    for value in values {
        bef64(&mut bytes, value);
    }
    bytes
}

fn offset_blend_graph(
    visible_supports: bool,
    winning_offset: bool,
    pair_intersection: u16,
    support_chain: bool,
) -> (
    crate::brep::index::CarrierIndex,
    crate::brep::topology::Tables,
) {
    use crate::test_support::parasolid::{
        bridge, coedge, edge_use, line_carrier, loop_head, offset_surface_carrier, plane_carrier,
        triangle_body,
    };
    let mut body = triangle_body();
    // This graph fixture supplies no body ownership; the face bridges own it.
    let ownership = body
        .windows(3)
        .position(|bytes| bytes == [0, 0x0c, 0xff])
        .unwrap();
    body.truncate(ownership);
    if support_chain {
        body.extend(offset_surface_carrier(181, 182, 0.001));
        body.extend(plane_carrier(182, [0.; 3], [0., 0., 1.], [1., 0., 0.]));
    } else {
        body.extend(plane_carrier(181, [0.; 3], [0., 0., 1.], [1., 0., 0.]));
    }
    if winning_offset {
        body.extend(offset_surface_carrier(180, 181, 0.001));
    }
    if visible_supports || support_chain {
        body.extend(rolling_ball_record(
            180,
            b'E',
            [190, 181, 184],
            [0.002, 0.002, 1., 1.],
        ));
        body.extend(rolling_ball_record(
            190,
            b'R',
            [181, 182, pair_intersection],
            [0., 0., 1., 1.],
        ));
        body.extend(line_carrier(183, [0.; 3], [1., 0., 0.]));
        if !support_chain {
            body.extend(plane_carrier(182, [0.; 3], [0., 0., 1.], [1., 0., 0.]));
        }
        if visible_supports {
            for (face, lp, head, edges, reversed, surface) in [
                (8, 21, 110, [40, 41, 42], true, 181),
                (9, 22, 120, [70, 71, 72], false, 182),
            ] {
                body.extend(bridge(face, lp, surface));
                body.extend(loop_head(lp, head, face));
                for (index, edge) in edges.into_iter().enumerate() {
                    let index = u16::try_from(index).unwrap();
                    body.extend(coedge(
                        head + index,
                        lp,
                        head + (index + 1) % 3,
                        50 + index,
                        0,
                        edge,
                        reversed,
                    ));
                    if !reversed {
                        body.extend(edge_use(edge, 0));
                    }
                }
            }
        }
    } else {
        body.extend(rolling_ball_record(
            180,
            b'R',
            [181, 182, 184],
            [0.002, 0.002, 1., 1.],
        ));
    }
    let setup = cadmpeg_test_support::service_decode_context();
    let carriers = crate::brep::index::scan_carriers(&setup, &body).unwrap();
    let view = crate::brep::index::view::CarrierView::Retained(&carriers);
    assert!(view.blend(&setup, 180).unwrap().is_some());
    assert_eq!(view.offset(&setup, 180).unwrap().is_some(), winning_offset);
    if visible_supports || support_chain {
        assert!(view.blend_support_pair(&setup, 190).unwrap().is_some());
        assert!(view.curve(&setup, 183).unwrap().is_some());
    }
    let mut tables = crate::brep::topology::scan(&setup, &body).unwrap();
    let mut current = tables.bridges().get(&10).unwrap().clone();
    current.refs[4] = 180;
    tables.insert_bridge(&setup, current).unwrap();
    (carriers, tables)
}

#[test]
fn winning_offset_skips_discarded_blend_edge_index() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (carriers, tables) = offset_blend_graph(true, true, 183, false);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "index Parasolid surface face edges",
        None,
    );
    let decoded = super::decode_graph(
        &ctx,
        crate::brep::index::view::CarrierView::Retained(&carriers),
        &tables,
        crate::brep::entity::FactsSource::empty(&ctx).unwrap(),
        &crate::brep::typed::Facts::default(),
        &cadmpeg_ir::stream_name!("offset-blend"),
    )
    .unwrap();
    drop(probe);
    assert_eq!(decoded.faces.len(), 3);
    assert_eq!(decoded.surfaces.len(), 3);
    assert_eq!(decoded.procedural_surfaces.len(), 1);
    assert!(matches!(
        decoded.procedural_surfaces[0].definition(),
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset(_)
    ));
    ctx.finish_session().unwrap();
}

#[test]
fn winning_offset_preserves_losing_blend_hidden_supports() {
    let (carriers, tables) = offset_blend_graph(false, true, 183, false);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let decoded = super::decode_graph(
        &ctx,
        crate::brep::index::view::CarrierView::Retained(&carriers),
        &tables,
        crate::brep::entity::FactsSource::empty(&ctx).unwrap(),
        &crate::brep::typed::Facts::default(),
        &cadmpeg_ir::stream_name!("offset-blend"),
    )
    .unwrap();
    assert_eq!(decoded.faces.len(), 1);
    assert_eq!(decoded.surfaces.len(), 3);
    assert_eq!(decoded.procedural_surfaces.len(), 1);
    assert_eq!(decoded.stats.unknown_procedural_supports, 1);
    let hidden = decoded
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str().ends_with("#182"))
        .unwrap();
    assert_eq!(
        decoded
            .annotations
            .exactness()
            .get(hidden.id.as_str())
            .unwrap()
            .entity(),
        cadmpeg_ir::Exactness::Unknown
    );
    ctx.finish_session().unwrap();
}

#[test]
fn chosen_blend_reuses_current_face_edge_set() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (carriers, tables) = offset_blend_graph(true, false, 183, false);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "select Parasolid blend face edges",
        None,
    );
    let decoded = super::decode_graph(
        &ctx,
        crate::brep::index::view::CarrierView::Retained(&carriers),
        &tables,
        crate::brep::entity::FactsSource::empty(&ctx).unwrap(),
        &crate::brep::typed::Facts::default(),
        &cadmpeg_ir::stream_name!("offset-blend"),
    )
    .unwrap();
    drop(probe);
    assert_eq!(decoded.faces.len(), 3);
    assert_eq!(decoded.procedural_surfaces.len(), 1);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend(blend) =
        decoded.procedural_surfaces[0].definition()
    else {
        panic!("selected Pair support emits a blend");
    };
    for support in blend.supports() {
        assert_eq!(
            support.as_ref().unwrap().surface,
            crate::brep::graph::id_surf(8)
        );
    }
    ctx.finish_session().unwrap();
}

#[test]
fn ineligible_losing_blend_skips_publication_and_topology_selection() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (carriers, tables) = offset_blend_graph(false, true, 199, true);
    let setup = cadmpeg_test_support::service_decode_context();
    assert!(crate::brep::index::view::CarrierView::Retained(&carriers)
        .curve(&setup, 199)
        .unwrap()
        .is_none());
    for operation in [
        "test published Parasolid blend supports",
        "index Parasolid surface face edges",
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            operation,
            None,
        );
        let decoded = super::decode_graph(
            &ctx,
            crate::brep::index::view::CarrierView::Retained(&carriers),
            &tables,
            crate::brep::entity::FactsSource::empty(&ctx).unwrap(),
            &crate::brep::typed::Facts::default(),
            &cadmpeg_ir::stream_name!("ineligible-pair"),
        )
        .unwrap();
        drop(probe);
        assert_eq!(decoded.faces.len(), 1);
        assert_eq!(decoded.surfaces.len(), 3);
        assert_eq!(decoded.procedural_surfaces.len(), 2);
        assert!(decoded.procedural_surfaces.iter().all(|surface| matches!(
            surface.definition(),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset(_)
        )));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn ambiguous_owner_selection_skips_discarded_ring_walks() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let setup = cadmpeg_test_support::service_decode_context();
    let mut tables = crate::brep::topology::Tables::default();
    for (attr, surface) in [(10, 100), (11, 200), (12, 100)] {
        tables
            .insert_bridge(
                &setup,
                Bridge {
                    attr,
                    sequence: 0,
                    refs: [0, 0, 20, 0, surface],
                    sense: Sense::Forward,
                    owner: Some(700),
                    offset: 0,
                },
            )
            .unwrap();
    }
    for (dimension, operation) in [
        (ResourceDimension::WorkUnits, "walk Parasolid face loops"),
        (ResourceDimension::WorkUnits, "sort Parasolid face owners"),
        (
            ResourceDimension::MaterializedBytes,
            "collect Parasolid face uses",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe =
            cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(dimension, operation, None);
        let decoded = super::decode_graph(
            &ctx,
            crate::brep::index::view::CarrierView::Retained(
                &crate::brep::index::CarrierIndex::default(),
            ),
            &tables,
            crate::brep::entity::FactsSource::empty(&ctx).unwrap(),
            &crate::brep::typed::Facts::default(),
            &cadmpeg_ir::stream_name!("ambiguous"),
        )
        .unwrap();
        drop(probe);
        assert!(decoded.faces.is_empty());
        assert_eq!(decoded.stats.ambiguous_face_owners, 1);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn empty_face_graph_releases_metadata_and_annotation_stream() {
    use crate::test_support::parasolid::{
        entity51, entity53_color, face_color_definition, FACE_COLOR_DEFINITION_ID,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut bytes = face_color_definition();
    bytes.extend(entity51(1, 700, FACE_COLOR_DEFINITION_ID, &[]));
    bytes.extend(entity53_color(900, [0.2, 0.4, 0.6]));
    let setup = cadmpeg_test_support::service_decode_context();
    let mut tables = crate::brep::topology::Tables::default();
    tables
        .insert_bridge(
            &setup,
            Bridge {
                attr: 10,
                sequence: 1,
                refs: [0, 0, 20, 0, 100],
                sense: Sense::Forward,
                owner: None,
                offset: 0,
            },
        )
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut facts = crate::brep::entity::scan_metadata(&ctx, &bytes, false).unwrap();
    assert_eq!(facts.face_colors_mut().len(), 1);
    let decoded = super::decode_graph(
        &ctx,
        crate::brep::index::view::CarrierView::Retained(
            &crate::brep::index::CarrierIndex::default(),
        ),
        &tables,
        facts,
        &crate::brep::typed::Facts::default(),
        &cadmpeg_ir::stream_name!("empty-face"),
    )
    .unwrap();
    assert!(decoded.faces.is_empty());
    assert!(decoded.face_colors.is_empty());
    assert!(decoded.face_bridge_sequences.is_empty());
    assert!(decoded.annotations.provenance.is_empty());
    assert!(decoded.losses.is_empty());
    drop(
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "reuse empty graph metadata storage",
        )
        .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn exhausted_fact_shells_retire_before_orientation() {
    use crate::test_support::parasolid::{
        entity51, face_color_definition, triangle_body, FACE_COLOR_DEFINITION_ID,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let setup = cadmpeg_test_support::service_decode_context();
    let body = triangle_body();
    let tables = crate::brep::topology::scan(&setup, &body).unwrap();
    let carriers = crate::brep::index::scan_carriers(&setup, &body).unwrap();
    let mut metadata = face_color_definition();
    metadata.extend(entity51(1, 700, FACE_COLOR_DEFINITION_ID, &[]));
    let boundary = |with_facts| {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "index initial Parasolid face senses",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let facts = if with_facts {
                    let mut facts = crate::brep::entity::scan_metadata(&ctx, &metadata, false)?;
                    assert_eq!(facts.face_color_versions_mut().len(), 1);
                    assert!(facts.face_colors_mut().is_empty());
                    facts
                } else {
                    crate::brep::entity::FactsSource::empty(&ctx)?
                };
                super::decode_graph(
                    &ctx,
                    crate::brep::index::view::CarrierView::Retained(&carriers),
                    &tables,
                    facts,
                    &crate::brep::typed::Facts::default(),
                    &cadmpeg_ir::stream_name!("fact-shell"),
                )
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("orientation boundary");
        };
        limit
    };
    let empty = boundary(false);
    let consumed = boundary(true);
    assert_eq!(consumed.used, empty.used);
    assert_eq!(consumed.additional, empty.additional);
}

#[test]
fn equivalent_owner_bridges_keep_the_first_bridge_and_one_ring() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let setup = cadmpeg_test_support::service_decode_context();
    let body = crate::test_support::parasolid::triangle_body();
    let mut tables = crate::brep::topology::scan(&setup, &body).unwrap();
    let carriers = crate::brep::index::scan_carriers(&setup, &body).unwrap();
    let mut first = tables.bridges()[&10].clone();
    first.owner = Some(700);
    let mut preferred = first.clone();
    preferred.attr = 11;
    preferred.offset = 0;
    tables.insert_bridge(&setup, first).unwrap();
    tables.insert_bridge(&setup, preferred).unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let decoded = super::decode_graph(
        &ctx,
        crate::brep::index::view::CarrierView::Retained(&carriers),
        &tables,
        crate::brep::entity::FactsSource::empty(&ctx).unwrap(),
        &crate::brep::typed::Facts::default(),
        &cadmpeg_ir::stream_name!("equivalent-owner"),
    )
    .unwrap();
    assert_eq!(decoded.faces.len(), 1);
    assert_eq!(decoded.faces[0].id, crate::brep::graph::id_face(11));
    assert_eq!(decoded.loops.len(), 1);
    assert_eq!(decoded.loops[0].face, decoded.faces[0].id);
    assert_eq!(decoded.coedges.len(), 3);
    assert_eq!(decoded.edges.len(), 3);
    assert_eq!(decoded.stats.ambiguous_face_owners, 0);
    ctx.finish_session().unwrap();
}

#[test]
fn consumed_owner_groups_retire_before_orientation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let setup = cadmpeg_test_support::service_decode_context();
    let body = crate::test_support::parasolid::triangle_body();
    let mut owned_tables = crate::brep::topology::scan(&setup, &body).unwrap();
    let unowned_tables = crate::brep::topology::scan(&setup, &body).unwrap();
    let carriers = crate::brep::index::scan_carriers(&setup, &body).unwrap();
    let mut bridge = owned_tables.bridges()[&10].clone();
    bridge.owner = Some(700);
    owned_tables.insert_bridge(&setup, bridge).unwrap();
    let boundary = |tables: &crate::brep::topology::Tables| {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "index initial Parasolid face senses",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                super::decode_graph(
                    &ctx,
                    crate::brep::index::view::CarrierView::Retained(&carriers),
                    tables,
                    crate::brep::entity::FactsSource::empty(&ctx).unwrap(),
                    &crate::brep::typed::Facts::default(),
                    &cadmpeg_ir::stream_name!("owner-stage"),
                )
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("orientation boundary");
        };
        limit
    };
    let unowned = boundary(&unowned_tables);
    let owned = boundary(&owned_tables);
    assert_eq!(owned.used, unowned.used);
    assert_eq!(owned.additional, unowned.additional);
}
