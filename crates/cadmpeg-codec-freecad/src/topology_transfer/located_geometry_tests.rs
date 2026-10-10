// SPDX-License-Identifier: Apache-2.0

use super::{Builder, Tables};
use crate::brep::{ShapePayload, ShapePayloadRecord, TextTShapes};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    analytic::PlaneSurface, ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

#[test]
fn located_surfaces_copy_only_their_own_construction_and_reuse_placements() {
    let payload = ShapePayloadRecord {
        id: "fcstd:native:shape#synthetic".into(),
        property: "fcstd:native:property#synthetic".into(),
        entry: "fcstd:native:entry#synthetic".into(),
        payload: ShapePayload::Empty,
    };
    let tshapes = TextTShapes::default();
    let tables = Tables {
        locations: &[],
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        tshapes: &tshapes,
        triangulations: &[],
        roots: &[],
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut builder = Builder::new(
        &ctx,
        &payload,
        tables,
        cadmpeg_core::text::NonBlankString::new("synthetic").unwrap(),
    )
    .unwrap();
    let mut ir = CadIr::empty();
    let base = SurfaceId::mint(crate::native::model_id("surface", &payload.id, "1")).unwrap();
    let solved = SurfaceId::mint(crate::native::model_id("surface", &payload.id, "2")).unwrap();
    for id in [&base, &solved] {
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
    }
    ir.model
        .add_procedural_surface(
            &ctx,
            &base,
            ProceduralSurface::new(
                ProceduralSurfaceId::mint("fcstd:model:surface#synthetic:construction").unwrap(),
                ProceduralSurfaceDefinition::Replica {
                    source: solved.clone(),
                    transform: Transform::identity(),
                },
                None,
            ),
        )
        .unwrap()
        .unwrap();
    let transform = Transform::affine([
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();

    assert_eq!(
        builder
            .located_surface(&mut ir, 1, Transform::identity())
            .unwrap(),
        base
    );
    let located_solved = builder.located_surface(&mut ir, 2, transform).unwrap();
    assert_eq!(ir.model.procedural_surfaces.len(), 1);
    let located = builder.located_surface(&mut ir, 1, transform).unwrap();
    assert_eq!(ir.model.procedural_surfaces.len(), 2);
    assert_eq!(
        builder.located_surface(&mut ir, 1, transform).unwrap(),
        located
    );
    assert_eq!(ir.model.surfaces.len(), 4);
    assert_eq!(ir.model.procedural_surfaces.len(), 2);
    let replica = &ir.model.procedural_surfaces[1];
    assert_eq!(
        ir.model.procedural_surface_owner(&replica.id),
        Some(&located)
    );
    assert!(
        matches!(replica.definition(), ProceduralSurfaceDefinition::Replica {
        source, transform: placement,
    } if source == &base && *placement == transform)
    );
    assert!(ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == located_solved)
        .unwrap()
        .geometry
        .procedural_construction()
        .is_none());
}
