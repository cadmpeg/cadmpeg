// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use std::collections::HashMap;
use std::io::Cursor;

fn curve_key(ir: &CadIr, value: &cadmpeg_ir::geometry::Curve) -> String {
    curve_layout_key(
        ir,
        value,
        &ir.model
            .curves
            .iter()
            .map(|row| row.id.to_string())
            .collect(),
    )
}
fn surface_key(ir: &CadIr, value: &cadmpeg_ir::geometry::Surface) -> String {
    surface_layout_key(
        ir,
        value,
        &ir.model
            .surfaces
            .iter()
            .map(|row| row.id.to_string())
            .collect(),
    )
}

fn placed() -> (CadIr, usize, usize, usize, usize) {
    let decoded = crate::FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::GEOMETRY),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let curve = ir
        .model
        .curves
        .iter()
        .position(|value| {
            value.native_ref.is_some()
                && value.geometry.solved().is_some_and(|geometry| {
                    !matches!(geometry, SolvedCurveGeometry::Transformed(_))
                })
        })
        .unwrap();
    let surface = ir
        .model
        .surfaces
        .iter()
        .position(|value| {
            value.native_ref.is_some()
                && value.geometry.solved().is_some_and(|geometry| {
                    !matches!(geometry, SolvedSurfaceGeometry::Transformed(_))
                })
        })
        .unwrap();
    let transform = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    let mut curve_clone = ir.model.curves[curve].clone();
    curve_clone.id =
        cadmpeg_ir::ids::CurveId::mint("author:model:curve#located-with-arbitrary-label").unwrap();
    curve_clone.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedCurve::try_new(
            Box::new(curve_clone.geometry.solved().unwrap().clone()),
            transform,
        )
        .unwrap(),
    ));
    let placed_curve = ir.model.curves.len();
    ir.model.curves.push(curve_clone);
    let mut surface_clone = ir.model.surfaces[surface].clone();
    surface_clone.id =
        cadmpeg_ir::ids::SurfaceId::mint("author:model:surface#another-arbitrary-label").unwrap();
    surface_clone.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(
            Box::new(surface_clone.geometry.solved().unwrap().clone()),
            transform,
        )
        .unwrap(),
    ));
    let placed_surface = ir.model.surfaces.len();
    ir.model.surfaces.push(surface_clone);
    (ir, curve, placed_curve, surface, placed_surface)
}

struct Rename<'a> {
    map: &'a HashMap<String, String>,
}
impl EntityRewrite for Rename<'_> {
    type Error = cadmpeg_core::CodecError;
    fn rewrite<T: RewriteIdentities>(&mut self, value: T) -> Result<T, Self::Error> {
        cadmpeg_ir::schema::rewrite::identities(
            &cadmpeg_test_support::service_decode_context(),
            value,
            |id| Ok(self.map.get(id).cloned().unwrap_or_else(|| id.to_owned())),
        )
    }
}

#[test]
fn actual_native_home_and_current_basis_survive_arbitrary_neutral_identity_rewrites() {
    let (mut ir, curve, placed_curve, surface, placed_surface) = placed();
    let mappings = [
        (
            ir.model.curves[curve].id.to_string(),
            "rewrite:model:curve#base".to_owned(),
        ),
        (
            ir.model.curves[placed_curve].id.to_string(),
            "rewrite:model:curve#placed".to_owned(),
        ),
        (
            ir.model.surfaces[surface].id.to_string(),
            "rewrite:model:surface#base".to_owned(),
        ),
        (
            ir.model.surfaces[placed_surface].id.to_string(),
            "rewrite:model:surface#placed".to_owned(),
        ),
    ]
    .into_iter()
    .collect();
    let mut model = Model::default();
    model
        .extend_rewritten(ir.model, &mut Rename { map: &mappings })
        .unwrap();
    ir.model = model;
    assert_eq!(
        curve_key(&ir, &ir.model.curves[placed_curve]),
        ir.model.curves[curve].id.to_string()
    );
    assert_eq!(
        surface_key(&ir, &ir.model.surfaces[placed_surface]),
        ir.model.surfaces[surface].id.to_string()
    );
    let (_, transform) = located_curve(&ir, &ir.model.curves[placed_curve]).unwrap();
    assert_ne!(transform, cadmpeg_ir::transform::Transform::identity());
    let (_, transform) = located_surface(&ir, &ir.model.surfaces[placed_surface]).unwrap();
    assert_ne!(transform, cadmpeg_ir::transform::Transform::identity());
}

#[test]
fn stale_native_home_never_overrides_edited_bases_or_ambiguous_counterparts() {
    let (mut ir, curve, placed_curve, surface, placed_surface) = placed();
    let mut divergent = ir.clone();
    divergent.model.curves[placed_curve].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
            cadmpeg_ir::geometry::PlacedCurve::try_new(
                Box::new(
                    ir.model.curves[placed_curve]
                        .geometry
                        .solved()
                        .unwrap()
                        .clone(),
                ),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .unwrap(),
        ));
    divergent.model.surfaces[placed_surface].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
            cadmpeg_ir::geometry::PlacedSurface::try_new(
                Box::new(
                    ir.model.surfaces[placed_surface]
                        .geometry
                        .solved()
                        .unwrap()
                        .clone(),
                ),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .unwrap(),
        ));
    assert_eq!(
        curve_key(&divergent, &divergent.model.curves[placed_curve]),
        divergent.model.curves[placed_curve].id.to_string()
    );
    assert_eq!(
        surface_key(&divergent, &divergent.model.surfaces[placed_surface]),
        divergent.model.surfaces[placed_surface].id.to_string()
    );
    let mut duplicate_curve = ir.model.curves[curve].clone();
    duplicate_curve.id =
        cadmpeg_ir::ids::CurveId::mint("author:model:curve#ambiguous-base").unwrap();
    ir.model.curves.push(duplicate_curve);
    let mut duplicate_surface = ir.model.surfaces[surface].clone();
    duplicate_surface.id =
        cadmpeg_ir::ids::SurfaceId::mint("author:model:surface#ambiguous-base").unwrap();
    ir.model.surfaces.push(duplicate_surface);
    assert_eq!(
        curve_key(&ir, &ir.model.curves[placed_curve]),
        ir.model.curves[curve].id.to_string()
    );
    assert_eq!(
        surface_key(&ir, &ir.model.surfaces[placed_surface]),
        ir.model.surfaces[surface].id.to_string()
    );
    assert_eq!(
        curve_key(&ir, &ir.model.curves[curve]),
        ir.model.curves[curve].id.to_string()
    );
    assert_eq!(
        curve_key(&ir, ir.model.curves.last().unwrap()),
        "author:model:curve#ambiguous-base"
    );
    assert_eq!(
        surface_key(&ir, &ir.model.surfaces[surface]),
        ir.model.surfaces[surface].id.to_string()
    );
    assert_eq!(
        surface_key(&ir, ir.model.surfaces.last().unwrap()),
        "author:model:surface#ambiguous-base"
    );
    ir.model.curves.truncate(placed_curve + 1);
    ir.model.surfaces.truncate(placed_surface + 1);
    // Editing each base to the placed geometry leaves the clone's inline basis
    // unchanged; the shared native home cannot authorize stale deduplication.
    ir.model.curves[curve].geometry = ir.model.curves[placed_curve].geometry.clone();
    ir.model.surfaces[surface].geometry = ir.model.surfaces[placed_surface].geometry.clone();
    assert_eq!(
        curve_key(&ir, &ir.model.curves[placed_curve]),
        ir.model.curves[placed_curve].id.to_string()
    );
    assert_eq!(
        surface_key(&ir, &ir.model.surfaces[placed_surface]),
        ir.model.surfaces[placed_surface].id.to_string()
    );
}

#[test]
fn equal_inline_bases_keep_distinct_current_base_and_placed_identities() {
    let (mut ir, curve, placed_curve, surface, placed_surface) = placed();
    let base_curve = ir.model.curves[curve].clone();
    let placed_curve = ir.model.curves[placed_curve].clone();
    let mut second_base_curve = base_curve.clone();
    second_base_curve.id =
        cadmpeg_ir::ids::CurveId::mint("author:model:curve#second-base").unwrap();
    let mut second_placed_curve = placed_curve.clone();
    second_placed_curve.id =
        cadmpeg_ir::ids::CurveId::mint("author:model:curve#second-use").unwrap();
    ir.model.curves = vec![
        base_curve,
        placed_curve,
        second_base_curve,
        second_placed_curve,
    ];
    let base_surface = ir.model.surfaces[surface].clone();
    let placed_surface = ir.model.surfaces[placed_surface].clone();
    let mut second_base_surface = base_surface.clone();
    second_base_surface.id =
        cadmpeg_ir::ids::SurfaceId::mint("author:model:surface#second-base").unwrap();
    let mut second_placed_surface = placed_surface.clone();
    second_placed_surface.id =
        cadmpeg_ir::ids::SurfaceId::mint("author:model:surface#second-use").unwrap();
    ir.model.surfaces = vec![
        base_surface,
        placed_surface,
        second_base_surface,
        second_placed_surface,
    ];
    for index in [0, 2] {
        assert_eq!(
            curve_key(&ir, &ir.model.curves[index]),
            ir.model.curves[index].id.to_string()
        );
        assert_eq!(
            surface_key(&ir, &ir.model.surfaces[index]),
            ir.model.surfaces[index].id.to_string()
        );
        assert_eq!(
            curve_key(&ir, &ir.model.curves[index + 1]),
            ir.model.curves[index].id.to_string()
        );
        assert_eq!(
            surface_key(&ir, &ir.model.surfaces[index + 1]),
            ir.model.surfaces[index].id.to_string()
        );
    }
}
