// SPDX-License-Identifier: Apache-2.0
//! Surface comparison and candidate lifetime admission.

use super::super::{same_surface_locus, surface_geometries_equal};
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

fn surface(u0: f64, last_pole: f64, rational: bool) -> SurfaceGeometry {
    let rows = vec![
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, last_pole)],
    ];
    let poles = if rational {
        crate::test_support::with_service_context(|ctx| {
            NurbsPoleGrid::from_lanes(ctx, rows, Some(vec![vec![1.0; 2]; 2]))
        })
        .expect("admitted weights")
        .expect("paired weights")
    } else {
        NurbsPoleGrid::Polynomial { rows }
    };
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        crate::test_support::with_service_context(|ctx| {
            NurbsSurface::new(
                ctx,
                NurbsSurfaceAxis::new(1, vec![u0, u0, 1.0, 1.0], false),
                NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                poles,
                false,
            )
        })
        .expect("admitted surface")
        .expect("valid surface"),
    ))
}

#[test]
fn surface_equality_admits_variable_lanes_and_stops_at_first_difference() {
    let left = surface(0.0, 0.0, false);
    let first_knot = surface(-1.0, 0.0, false);
    assert!(
        !crate::test_support::with_work_limit(1, |ctx| surface_geometries_equal(
            ctx,
            &left,
            &first_knot
        ))
        .expect("one mismatched knot")
    );
    let last_pole = surface(0.0, 1.0, false);
    assert!(
        !crate::test_support::with_service_context(|ctx| same_surface_locus(
            ctx, &left, &last_pole
        ))
        .expect("late pole comparison")
    );
    for rational in [false, true] {
        let value = surface(0.0, 0.0, rational);
        let other = value.clone();
        let result =
            crate::test_support::with_work_refusal("catia_freeform_surface_comparison", |ctx| {
                same_surface_locus(ctx, &value, &other)
            });
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_freeform_surface_comparison")
        );
        assert!(
            crate::test_support::with_service_context(|ctx| same_surface_locus(
                ctx, &value, &other
            ))
            .expect("equal surface")
        );
    }
}

#[test]
fn empty_wire_population_builds_no_curve_index() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: cadmpeg_ir::ids::CurveId::mint("catia:test:curve#0").expect("fixture identity"),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
        ),
        source_object: None,
    });
    crate::test_support::with_work_limit(0, |ctx| {
        assert!(super::super::attach_standalone_wires(
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &[],
            &mut crate::families::FamilyEntityAdmission::new(ctx)
        )
        .expect("no wire consumer"));
    });
    assert!(ir.model.edges.is_empty());
}

#[test]
fn freeform_census_pools_are_temporary_through_model_transfer() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let stream = crate::test_support::test_b5::b5_closed_triangle_stream_with_native_vertex_chain();
    let bytes = crate::test_support::test_container::standard_catpart_from_streams(&stream, &[]);
    for operation in [
        "catia_b5_typed_edge_records",
        "catia_b5_typed_loop_records",
        "catia_b5_typed_vertex_incidence_links",
        "catia_b5_typed_class21_pcurves",
        "catia_b5_typed_parameter_incidences",
        "catia_b5_typed_vertex_incidence_rosters",
        "catia_freeform_b5_face_ids",
        "catia_freeform_b5_loop_ids",
        "catia_freeform_census_face_ids",
    ] {
        // Each populated census route must finish with a retained refusal
        // armed at its source allocation. The model transfer still succeeds.
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let output = crate::test_support::with_retained_limit(u64::MAX, |ctx| {
            let scan = crate::container::scan_bytes(ctx, bytes.as_slice())?;
            super::super::try_decode_freeform_surfaces(
                ctx,
                &scan,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("census storage is temporary")
        .expect("resolved freeform family");
        assert_eq!(output.ir.model.faces.len(), 1);
        assert_eq!(output.ir.model.loops.len(), 1);
        assert_eq!(output.ir.model.edges.len(), 3);
        assert_eq!(output.ir.model.vertices.len(), 3);
    }
}

#[test]
fn empty_resolved_population_builds_no_model_indexes() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: cadmpeg_ir::ids::CurveId::mint("catia:test:curve#0").expect("fixture identity"),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
        ),
        source_object: None,
    });
    let counts = crate::test_support::with_materialized_limit(0, |ctx| {
        super::super::append_resolved_consolidated_surface_curves(
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &[],
            &[],
            super::super::FreeformSurfacePool {
                surfaces: &[],
                surface_ids: &[],
                surface_alias_tags: &std::collections::HashMap::new(),
            },
            &mut crate::nurbs::LaneRefusals::new(),
            &mut crate::families::FamilyEntityAdmission::new(ctx),
        )
    })
    .expect("no resolved consumer indexes");
    assert_eq!(counts.standard_edges, 0);
    assert_eq!(counts.partner_supports, 0);
    assert_eq!(counts.standard_face_surfaces, 0);
    assert_eq!(counts.standard_face_pcurves, 0);
    assert_eq!(ir.model.curves.len(), 1);
    assert!(ir.model.procedural_curves.is_empty());
}

#[test]
fn freeform_fallback_carrier_slots_and_tags_are_temporary() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let bytes = crate::test_support::test_container::outer_body_catpart(
        &crate::test_support::test_b2::b2_sphere_stream(),
    );
    for operation in [
        "catia_freeform_surface_carriers",
        "catia_freeform_surface_source_tag",
    ] {
        let _probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let output = crate::test_support::with_retained_limit(u64::MAX, |ctx| {
            let scan = crate::container::scan_bytes(ctx, bytes.as_slice())?;
            super::super::try_decode_freeform_surfaces(
                ctx,
                &scan,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("fallback pool is temporary")
        .expect("fallback family");
        assert_eq!(output.ir.model.surfaces.len(), 1);
        assert!(matches!(output.ir.model.surfaces[0].geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere))
                if sphere.radius().get() == 5.0));
        assert!(output.ir.model.surfaces[0].source_object.is_some());
    }
}

#[test]
fn freeform_surface_pool_source_tags_are_temporary_through_annotation() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let bytes = crate::test_support::test_a5a8::a5_surface_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let _probe = RefusalProbe::arm(
        ResourceDimension::RetainedBytes,
        "catia_freeform_surface_source_tag",
        None,
    );
    let ir = crate::test_support::with_retained_limit(u64::MAX, |ctx| {
        let mut ir = cadmpeg_ir::CadIr::empty();
        super::super::append_freeform_surface_pools(
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &bytes,
            &records,
            &std::collections::HashMap::new(),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut crate::families::FamilyEntityAdmission::new(ctx),
        )?;
        Ok::<_, cadmpeg_core::CodecError>(ir)
    })
    .expect("annotation source tag is temporary");
    assert_eq!(ir.model.surfaces.len(), 1);
    assert!(ir.model.surfaces[0].source_object.is_some());
}

#[test]
fn freeform_surface_source_formats_the_final_scoped_tag() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let bytes = crate::test_support::test_a5a8::a5_surface_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let mut surface = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces_from_records(
            ctx,
            &bytes,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("source surface")
    .pop()
    .expect("one source surface");
    for (identity, tags) in [
        (
            None,
            [
                "frame_offset:0000000000",
                "freeform:frame_offset:0000000000",
            ],
        ),
        (
            Some(0x1234_5678),
            ["object_id:12345678", "freeform:object_id:12345678"],
        ),
    ] {
        surface.identity = identity;
        for (prefix, expected) in ["", "freeform:"].into_iter().zip(tags) {
            let _probe = RefusalProbe::arm(
                ResourceDimension::RetainedBytes,
                "catia_freeform_surface_source_tag",
                None,
            );
            crate::test_support::with_service_context(|ctx| {
                let (_, tag) = super::super::freeform_surface_source(ctx, &surface, prefix)
                    .expect("scoped final tag");
                assert_eq!(tag.0, expected);
            });
        }
    }
}

#[test]
fn incomplete_resolved_blocks_build_no_model_indexes() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let stream = crate::test_support::test_b5::b5_closed_triangle_stream_with_native_vertex_chain();
    let file = crate::test_support::test_container::standard_catpart_from_streams(&stream, &[]);
    let original = crate::test_support::with_service_context(|ctx| {
        let scan = crate::container::scan_bytes(ctx, file.as_slice()).expect("fixture container");
        super::super::try_decode_freeform_surfaces(
            ctx,
            &scan,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("source transfer")
        .expect("populated model")
        .ir
    });
    let bytes = crate::test_support::test_b2::b2_edge_block_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    crate::test_support::with_service_context(|ctx| {
        let blocks =
            crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("resolved blocks");
        assert_eq!(blocks.len(), 1);
        assert!(
            crate::families::consolidated::records::consolidated_topology_edge_runs_from_records(
                ctx, &bytes, &records
            )
            .expect("topology inventory")
            .is_empty()
        );
    });
    for operation in [
        "catia_model_surface_positions",
        "catia_freeform_points_index",
        "catia_freeform_vertices_index",
        "catia_zero_wire_curve_positions",
        "catia_freeform_faces_index",
        "catia_freeform_loops_index",
        "catia_freeform_coedges_index",
        "catia_freeform_edges_visits",
    ] {
        let mut ir = original.clone();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let counts = crate::test_support::with_work_limit(u64::MAX, |ctx| {
            super::super::append_resolved_consolidated_surface_curves(
                &mut ir,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
                &bytes,
                &records,
                super::super::FreeformSurfacePool {
                    surfaces: &[],
                    surface_ids: &[],
                    surface_alias_tags: &std::collections::HashMap::new(),
                },
                &mut crate::nurbs::LaneRefusals::new(),
                &mut crate::families::FamilyEntityAdmission::new(ctx),
            )
        })
        .expect("no complete-run transfer consumer");
        assert_eq!(counts.standard_edges, 0);
        assert_eq!(counts.partner_supports, 0);
        assert_eq!(counts.standard_face_surfaces, 0);
        assert_eq!(counts.standard_face_pcurves, 0);
        assert_eq!(ir.model, original.model);
    }
}
