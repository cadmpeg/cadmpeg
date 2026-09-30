// SPDX-License-Identifier: Apache-2.0

use super::project_configuration_sketch_states;
use crate::history::tests::{design_configuration, feature_input_lane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::sampled::PolygonalSurface;
use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::{CodecFormat, SourceObjectAssociation};

fn carrier_model() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let polygonal = PolygonalSurface::new(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        vec![[0, 1, 2]], 0.01,
    ).unwrap();
    let mut placed = SolvedSurfaceGeometry::Polygonal(polygonal);
    for _ in 0..2 {
        placed = SolvedSurfaceGeometry::Transformed(PlacedSurface::try_new(
            Box::new(placed), cadmpeg_ir::transform::Transform::identity(),
        ).unwrap());
    }
    ir.model.surfaces.push(Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("synthetic:test:id#polygonal").unwrap(),
        geometry: SurfaceGeometry::Solved(placed),
        source_object: Some(SourceObjectAssociation {
            format: CodecFormat::Sldprt,
            object_id: cadmpeg_core::text::NonBlankString::new("carrier").unwrap(),
            name: Some("Retained surface".into()), color: None, visible: Some(true),
            layer: Some("Layer".into()), instance_path: vec!["Outer".into(), "Inner".into()],
        }),
    });
    for rational in [false, true] {
        let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
        let poles = vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        ];
        let weights = rational.then(|| vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
        let geometry = NurbsSurface::from_lanes(axis(), axis(), NurbsSurfaceLanes::new(poles, weights), false).unwrap();
        ir.model.surfaces.push(Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!("synthetic:test:id#nurbs-{rational}")).unwrap(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
            source_object: None,
        });
    }
    ir.model.surfaces.push(Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("synthetic:test:id#procedural").unwrap(),
        geometry: SurfaceGeometry::Procedural {
            construction: cadmpeg_ir::ids::ProceduralSurfaceId::mint("synthetic:test:id#construction").unwrap(),
            cache: Some(SolvedSurfaceGeometry::Unknown {
                record: Some(cadmpeg_ir::ids::UnknownId::mint("synthetic:test:id#record").unwrap()),
            }),
        },
        source_object: None,
    });
    let mut configuration = design_configuration("carriers", 0, Some(0), None);
    configuration.bodies = None;
    ir.model.configurations.push(configuration);
    ir
}

fn run(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = carrier_model();
    let expected = ir.clone();
    let losses = project_configuration_sketch_states(
        &ctx, &mut ir, &[], &[feature_input_lane("lane", Some("0"))],
        &mut cadmpeg_ir::Annotations::default(),
    )?;
    assert!(losses.is_empty());
    assert_eq!(ir, expected);
    Ok(())
}

fn set_limit(policy: &mut DecodePolicy, dimension: ResourceDimension, limit: u64) {
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        _ => panic!("unexpected test dimension"),
    }
}

fn assert_projection_refusal(dimension: ResourceDimension) {
    let mut policy = DecodePolicy::service();
    run(&policy).unwrap();
    let mut lower = 0;
    let mut upper = 1;
    loop {
        set_limit(&mut policy, dimension, upper);
        match run(&policy) {
            Ok(()) => break,
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, dimension);
                upper = upper.checked_mul(2).unwrap();
            }
            Err(error) => panic!("unexpected route error: {error}"),
        }
    }
    while lower < upper {
        let midpoint = lower + (upper - lower) / 2;
        set_limit(&mut policy, dimension, midpoint);
        match run(&policy) {
            Ok(()) => upper = midpoint,
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, dimension);
                lower = midpoint + 1;
            }
            Err(error) => panic!("unexpected route error: {error}"),
        }
    }
    assert!(upper > 0);
    set_limit(&mut policy, dimension, upper);
    run(&policy).unwrap();
    set_limit(&mut policy, dimension, upper - 1);
    assert!(matches!(run(&policy), Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension));
}

#[test]
fn configuration_sketch_projection_refuses_carrier_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems);
}

#[test]
fn configuration_sketch_projection_refuses_carrier_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes);
}

#[test]
fn configuration_sketch_projection_refuses_carrier_nesting_limit() {
    assert_projection_refusal(ResourceDimension::RecursionDepth);
}

#[test]
fn configuration_sketch_projection_refuses_carrier_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits);
}
