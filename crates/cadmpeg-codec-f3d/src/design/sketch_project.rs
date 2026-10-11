// SPDX-License-Identifier: Apache-2.0
//! Project planar and spatial sketch geometry.

use crate::design::dimensions::{planar_point, sketch_normal_sign};
use crate::design::face_resolve::placement_origin_scale;
use crate::design::feature_project::spatial_profiles::closed_spatial_sketch_profiles;
use crate::design::geometry::closed_sketch_profiles;
use crate::records::{
    sketch_geometry::{
        SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchSurface, SketchText,
    },
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchConstraintKind, SketchRelation},
};
use cadmpeg_core::decode::{index_from_u32, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

use self::spatial_owners::spatial_geometry_owners;

mod spatial_owners;

const EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_DESIGN_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E12: f64 = 1.0e-12;

fn record_spline_segment<'a>(
    ctx: &DecodeContext<'_>,
    segments: &mut HashMap<(&'a str, u32), Option<[Point3; 2]>>,
    scope: &'a str,
    record: u32,
    points: [Point3; 2],
) -> Result<(), CodecError> {
    ctx.entry_hash_map(
        segments,
        (scope, record),
        "f3d spatial sketch spline segment index",
    )?
    .and_modify(|existing| {
        if *existing != Some(points) {
            *existing = None;
        }
    })
    .or_insert(Some(points));
    Ok(())
}

fn distinct_return_member_indices(
    ctx: &DecodeContext<'_>,
    members: &[crate::records::sketch_relations::SketchRelationReturnMember],
) -> Result<bool, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d spatial spline member storage")?;
    storage.with_storage(|| {
        let mut seen = HashSet::new();
        ctx.all_by(
            members,
            |member| {
                ctx.insert_hash_set(
                    &mut seen,
                    member.reference.record_index(),
                    "f3d spatial spline member index",
                )
            },
            "f3d spatial spline member scan",
        )
    })
}

fn sketch_text_horizontal_alignment(
    code: Option<u32>,
) -> Option<cadmpeg_ir::sketches::SketchTextHorizontalAlignment> {
    code.map(|code| match code {
        1 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Left,
        2 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Right,
        3 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Center,
        code => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Native(code),
    })
}

fn sketch_text_vertical_alignment(
    code: Option<u32>,
) -> Option<cadmpeg_ir::sketches::SketchTextVerticalAlignment> {
    code.map(|code| match code {
        1 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Top,
        2 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Bottom,
        3 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Middle,
        code => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Native(code),
    })
}

fn text_frame_curve_records<'a>(
    ctx: &DecodeContext<'_>,
    relations: &'a [SketchRelation],
    curves: &'a [SketchCurveIdentity],
    texts: &'a [SketchText],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d text frame owner storage")?;
    let (curve_owners, text_owners) = storage.with_storage(|| -> Result<_, CodecError> {
        let mut curve_owners = HashMap::new();
        for curve in ctx.admit_iter(curves, "f3d text frame curve scan")? {
            let (Some(scope), Some(owner)) = (crate::ids::native_stream_charged(ctx, &curve.id, "resolve F3D sketch projection stream")?, curve.owner_reference)
            else {
                continue;
            };
            ctx.insert_hash_map(
                &mut curve_owners,
                (scope, curve.record_index),
                owner,
                "f3d text frame curve owner",
            )?;
        }
        let mut text_owners = HashMap::new();
        for text in ctx.admit_iter(texts, "f3d text frame text scan")? {
            let Some(scope) = crate::ids::native_stream_charged(ctx, &text.id, "resolve F3D sketch projection stream")? else {
                continue;
            };
            ctx.insert_hash_map(
                &mut text_owners,
                (scope, text.record_index),
                text.owner_reference,
                "f3d text frame text owner",
            )?;
        }
        Ok((curve_owners, text_owners))
    })?;
    let mut frame_curves = HashSet::new();
    for relation in ctx.admit_iter(relations, "f3d text frame relation scan")? {
        let Some(crate::records::sketch_relations::SketchPatternDefinition::TextFrame {
            text_reference,
        }) = relation.definition.pattern()
        else {
            continue;
        };
        let Some(scope) = crate::ids::native_stream_charged(ctx, &relation.id, "resolve F3D sketch projection stream")? else {
            continue;
        };
        if crate::design::relation_kinds::sole_constraint_kind(relation).is_none()
            || relation
                .members()
                .first()
                .map(|member| member.reference.record_index())
                != Some(*text_reference)
            || relation.auxiliary_references().values_array::<1>() != Some([text_reference])
            || relation.members().len() < 2
            || relation.return_members().len() != relation.members().len() - 1
            || !ctx.all_by(
                relation
                    .return_members()
                    .iter()
                    .zip(&relation.members()[1..]),
                |(returned, member)| {
                    Ok(returned.reference.record_index() == member.reference.record_index())
                },
                "f3d text frame member comparison",
            )?
            || ctx.get_hash_map(
                &text_owners,
                &(scope, *text_reference),
                "f3d text frame text owner lookup",
            )? != Some(&relation.owner_reference)
        {
            continue;
        }
        if !ctx.all_by(
            relation.return_members().iter(),
            |member| {
                Ok(ctx.get_hash_map(
                    &curve_owners,
                    &(scope, member.reference.record_index()),
                    "f3d text frame curve owner lookup",
                )? == Some(&relation.owner_reference))
            },
            "f3d text frame owner comparison",
        )? {
            continue;
        }
        for member in
            ctx.admit_iter(&relation.return_members()[..], "f3d text frame return scan")?
        {
            ctx.insert_hash_set(
                &mut frame_curves,
                (scope, member.reference.record_index()),
                "f3d text frame curve record",
            )?;
        }
    }
    Ok(frame_curves)
}

/// Project placed Design sketches and their exact planar point/curve records.
pub(crate) fn project_sketch_design(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    placements: &[DesignSketchPlacement],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    relations: &[SketchRelation],
    texts: &[SketchText],
    linear_tolerance: f64,
) -> Result<
    (
        Vec<cadmpeg_ir::sketches::Sketch>,
        Vec<cadmpeg_ir::sketches::SketchEntity>,
    ),
    cadmpeg_core::CodecError,
> {
    use cadmpeg_ir::scalar::{Angle, Length};
    use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry, SketchGeometryDefinition};

    let mut lookup_storage = ctx.reserve_scoped(0, "f3d planar sketch lookup storage")?;
    let (text_frame_curves, placements_by_suffix, spatial_owners) =
        lookup_storage.with_storage(|| -> Result<_, CodecError> {
            let text_frame_curves = text_frame_curve_records(ctx, relations, curves, texts)?;
            let mut placements_by_suffix = HashMap::new();
            for placement in ctx.admit_iter(placements, "f3d sketch projection placements scan")? {
                let (Some(scope), Ok(owner)) = (
                    crate::ids::native_stream_charged(ctx, &placement.id, "resolve F3D sketch projection stream")?,
                    u32::try_from(placement.entity_id.suffix()),
                ) else {
                    continue;
                };
                ctx.insert_hash_map(
                    &mut placements_by_suffix,
                    (scope, owner),
                    placement,
                    "f3d planar sketch placement index",
                )
                .map(|_| ())?;
            }
            let spatial_owners = spatial_geometry_owners(ctx, points, curves)?;
            Ok((text_frame_curves, placements_by_suffix, spatial_owners))
        })?;
    let mut sketches = Vec::new();
    for placement in ctx.admit_iter(placements, "f3d sketch projection placements scan")? {
        if let (Ok(owner), Some(scope)) = (
            u32::try_from(placement.entity_id.suffix()),
            crate::ids::native_stream_charged(ctx, &placement.id, "resolve F3D sketch projection stream")?,
        ) {
            if ctx.contains_hash_set(
                &spatial_owners,
                &(scope, owner),
                "f3d planar spatial owner lookup",
            )? {
                continue;
            }
        }
        let Ok(resolved) = cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(
                placement.transform()[0][3] * placement_origin_scale(placement),
                placement.transform()[1][3] * placement_origin_scale(placement),
                placement.transform()[2][3] * placement_origin_scale(placement),
            ),
            Vector3::new(
                placement.transform()[0][2],
                placement.transform()[1][2],
                placement.transform()[2][2],
            ),
            Vector3::new(
                placement.transform()[0][0],
                placement.transform()[1][0],
                placement.transform()[2][0],
            ),
        ) else {
            continue;
        };
        let name =
            ctx.copy_retained_text(placement.entity_id.as_str(), "f3d planar sketch name")?;
        let native_ref =
            ctx.copy_retained_text(&placement.id, "f3d planar sketch native reference")?;
        ctx.push_vec(
            &mut sketches,
            Sketch {
                id: crate::design::identity::neutral_sketch_id(ctx, placement)?,
                name: Some(name),
                configuration: None,
                visible: placement
                    .visibility
                    .as_ref()
                    .map(|visibility| visibility.visible),
                placement: resolved,
                profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
                native_ref: Some(native_ref),
            },
            "f3d planar sketch",
        )?;
    }
    ctx.stable_sort_by(
        &mut sketches[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design sketch_project 1",
    )?;

    let mut entities = Vec::new();
    for point in ctx.admit_iter(points, "f3d sketch projection points scan")? {
        let Some(owner) = point.owner_reference else {
            continue;
        };
        let Some(scope) = crate::ids::native_stream_charged(ctx, &point.id, "resolve F3D sketch projection stream")? else {
            continue;
        };
        if ctx.contains_hash_set(
            &spatial_owners,
            &(scope, owner),
            "f3d sketch spatial owner lookup",
        )? {
            continue;
        }
        let Some(placement) = ctx.get_hash_map(
            &placements_by_suffix,
            &(scope, owner),
            "f3d sketch placement lookup",
        )?
        else {
            continue;
        };
        let sketch = crate::design::identity::neutral_sketch_id(ctx, placement)?;
        let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: point.coordinates(),
        }) else {
            continue;
        };
        let native_ref =
            ctx.copy_retained_text(&point.id, "f3d planar sketch point native reference")?;
        ctx.push_vec(
            &mut entities,
            SketchEntity::new(
                point.persistent_id().map_or_else(
                    || {
                        crate::design::identity::neutral_sketch_record_id(
                            ctx,
                            &sketch,
                            point.record_index,
                        )
                    },
                    |persistent_id| {
                        crate::design::identity::neutral_sketch_point_id(
                            ctx,
                            &sketch,
                            persistent_id,
                        )
                    },
                )?,
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
            "f3d planar sketch point entity",
        )?;
    }
    for curve in ctx.admit_iter(curves, "f3d sketch projection curves scan")? {
        let Some(owner) = curve.owner_reference else {
            continue;
        };
        let Some(scope) = crate::ids::native_stream_charged(ctx, &curve.id, "resolve F3D sketch projection stream")? else {
            continue;
        };
        if ctx.contains_hash_set(
            &spatial_owners,
            &(scope, owner),
            "f3d sketch spatial owner lookup",
        )? {
            continue;
        }
        let Some(placement) = ctx.get_hash_map(
            &placements_by_suffix,
            &(scope, owner),
            "f3d sketch placement lookup",
        )?
        else {
            continue;
        };
        let Some(source) = curve.geometry.as_ref() else {
            continue;
        };
        let geometry = match source {
            SketchCurveGeometry::Line {
                start, end, normal, ..
            } if planar_point(start.as_raw())
                && planar_point(end.as_raw())
                && normal.as_raw().z != 0.0 =>
            {
                let start = start.as_raw();
                let end = end.as_raw();
                let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start.x, start.y),
                    end: Point2::new(end.x, end.y),
                }) else {
                    continue;
                };
                geometry
            }
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } if planar_point(center.as_raw())
                && reference_direction.as_raw().z.abs()
                    <= EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9 =>
            {
                let Some(orientation) = sketch_normal_sign(normal.as_raw()) else {
                    continue;
                };
                let phase = reference_direction
                    .as_raw()
                    .y
                    .atan2(reference_direction.as_raw().x);
                let start_angle = phase + orientation * start_angle.get();
                let end_angle = phase + orientation * end_angle.get();
                let radius = Length::from(*radius);
                let definition = if (end_angle - start_angle).abs()
                    >= std::f64::consts::TAU - EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9
                {
                    SketchGeometryDefinition::Circle {
                        center: Point2::new(center.get().x, center.get().y),
                        radius,
                    }
                } else {
                    let (Some(start_angle), Some(end_angle)) =
                        (Angle::new(start_angle), Angle::new(end_angle))
                    else {
                        continue;
                    };
                    SketchGeometryDefinition::Arc {
                        center: Point2::new(center.get().x, center.get().y),
                        radius,
                        start_angle,
                        end_angle,
                    }
                };
                let Ok(geometry) = SketchGeometry::try_from(definition) else {
                    continue;
                };
                geometry
            }
            SketchCurveGeometry::Nurbs { geometry, .. }
                if geometry.degree() != 0
                    && geometry.poles().point_count() > index_from_u32(geometry.degree()) =>
            {
                let poles = geometry.poles();
                let planar_poles = ctx.collect_vec(
                    poles.points().map(|point| Point2::new(point.x, point.y)),
                    "f3d planar sketch nurbs poles",
                )?;
                let weights = poles
                    .weights()
                    .next()
                    .is_some()
                    .then(|| ctx.collect_vec(poles.weights(), "f3d planar sketch nurbs weights"))
                    .transpose()?;
                SketchGeometry::nurbs(cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                    ctx,
                    geometry.degree(),
                    geometry.knots_copy(ctx)?,
                    planar_poles,
                    weights,
                    false,
                )??)
            }
            _ => continue,
        };
        let sketch = crate::design::identity::neutral_sketch_id(ctx, placement)?;
        let native_ref =
            ctx.copy_retained_text(&curve.id, "f3d planar sketch curve native reference")?;
        ctx.push_vec(
            &mut entities,
            SketchEntity::new(
                crate::design::identity::neutral_sketch_curve_id(
                    ctx,
                    &sketch,
                    curve.primary_id.get(),
                    curve.secondary_id,
                )?,
                sketch,
                geometry,
            )
            .with_construction(ctx.contains_hash_set(
                &text_frame_curves,
                &(scope, curve.record_index),
                "f3d sketch text frame membership",
            )?)
            .with_native_ref(Some(native_ref)),
            "f3d planar sketch curve entity",
        )?;
    }
    for text in ctx.admit_iter(texts, "f3d sketch projection texts scan")? {
        let Some(scope) = crate::ids::native_stream_charged(ctx, &text.id, "resolve F3D sketch projection stream")? else {
            continue;
        };
        let Some(placement) = ctx.get_hash_map(
            &placements_by_suffix,
            &(scope, text.owner_reference),
            "f3d sketch text placement lookup",
        )?
        else {
            continue;
        };
        let sketch = crate::design::identity::neutral_sketch_id(ctx, placement)?;
        let Some(text_value) = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            ctx.copy_retained_text(&text.text, "f3d planar sketch text")?,
            "validate nonblank text",
        )?
        else {
            continue;
        };
        let Some(font_family) = cadmpeg_core::text::NonBlankString::for_decode(
            ctx,
            ctx.copy_retained_text(&text.font_family, "f3d planar sketch font family")?,
            "validate nonblank text",
        )?
        else {
            continue;
        };
        let Ok(font_weight) = text.font_weight.try_into() else {
            continue;
        };
        let Ok(geometry) = SketchGeometry::from_parts(SketchGeometryDefinition::Text {
            text: text_value,
            font_family,
            font_weight,
            height: text.height,
            width_factor: text
                .width_factor()
                .and_then(|factor| cadmpeg_ir::scalar::PositiveReal::try_from(factor).ok()),
            placement: text.placement(),
            horizontal_alignment: sketch_text_horizontal_alignment(
                text.alignment().map(|alignment| alignment.horizontal),
            ),
            vertical_alignment: sketch_text_vertical_alignment(
                text.alignment().map(|alignment| alignment.vertical),
            ),
        }) else {
            continue;
        };
        let native_ref =
            ctx.copy_retained_text(&text.id, "f3d planar sketch text native reference")?;
        ctx.push_vec(
            &mut entities,
            SketchEntity::new(
                text.persistent_id.map_or_else(
                    || {
                        crate::design::identity::neutral_sketch_record_id(
                            ctx,
                            &sketch,
                            text.record_index,
                        )
                    },
                    |persistent_id| {
                        crate::design::identity::neutral_sketch_text_id(ctx, &sketch, persistent_id)
                    },
                )?,
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
            "f3d planar sketch text entity",
        )?;
    }
    ctx.stable_sort_by(
        &mut entities[..],
        |value| value.id(),
        Ord::cmp,
        "sort f3d design sketch_project 2",
    )?;
    for sketch in ctx.admit_iter(&mut sketches, "f3d planar sketch profile scan")? {
        let inferred = closed_sketch_profiles(ctx, &sketch.id, &entities, linear_tolerance)?;
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(inferred) else {
            continue;
        };
        sketch.profiles = profiles;
    }
    Ok((sketches, entities))
}

/// Project non-planar Design sketch curves into model-space spatial sketches.
pub(crate) fn project_spatial_sketch_design(
    ctx: &DecodeContext<'_>,
    placements: &[DesignSketchPlacement],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    surfaces: &[SketchSurface],
    relations: &[SketchRelation],
    linear_tolerance: f64,
) -> Result<
    (
        Vec<cadmpeg_ir::sketches::SpatialSketch>,
        Vec<cadmpeg_ir::sketches::SpatialSketchEntity>,
    ),
    cadmpeg_core::CodecError,
> {
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::{
        SpatialSketch, SpatialSketchEntity, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    };

    let mut lookup_storage = ctx.reserve_scoped(0, "f3d spatial sketch lookup storage")?;
    let (placements_by_suffix, spatial_owners, spline_segments) =
        lookup_storage.with_storage(|| -> Result<_, CodecError> {
            let mut placements_by_suffix = HashMap::new();
            for placement in ctx.admit_iter(placements, "f3d sketch projection placements scan")? {
                let (Some(scope), Ok(owner)) = (
                    crate::ids::native_stream_charged(ctx, &placement.id, "resolve F3D sketch projection stream")?,
                    u32::try_from(placement.entity_id.suffix()),
                ) else {
                    continue;
                };
                ctx.insert_hash_map(
                    &mut placements_by_suffix,
                    (scope, owner),
                    placement,
                    "f3d spatial sketch placement index",
                )
                .map(|_| ())?;
            }
            let mut spatial_owners = spatial_geometry_owners(ctx, points, curves)?;
            for surface in ctx.admit_iter(surfaces, "f3d sketch projection surfaces scan")? {
                let (Some(scope), Some(owner)) =
                    (crate::ids::native_stream_charged(ctx, &surface.id, "resolve F3D sketch projection stream")?, surface.owner_reference)
                else {
                    continue;
                };
                ctx.insert_hash_set(
                    &mut spatial_owners,
                    (scope, owner),
                    "f3d spatial surface owner",
                )?;
            }
            let mut curve_storage = ctx.reserve_scoped(0, "f3d spatial spline curve storage")?;
            let curves_by_record = curve_storage.with_storage(|| -> Result<_, CodecError> {
                let mut curves_by_record = HashMap::new();
                for curve in ctx.admit_iter(curves, "f3d sketch projection curves scan")? {
                    let Some(scope) = crate::ids::native_stream_charged(ctx, &curve.id, "resolve F3D sketch projection stream")? else {
                        continue;
                    };
                    ctx.insert_hash_map(
                        &mut curves_by_record,
                        (scope, curve.record_index),
                        curve,
                        "f3d spatial sketch curve index",
                    )
                    .map(|_| ())?;
                }
                Ok(curves_by_record)
            })?;
            let mut spline_segments = HashMap::new();
            for relation in ctx.admit_iter(relations, "f3d sketch projection relations scan")? {
                // Only the second reference run of a relation record is in semantic
                // order: the control polygon ends with the spline there, and the
                // interleaved first run orders its members by nothing a reader can use.
                let members = relation.return_members();
                if crate::design::relation_kinds::unknown_constraint_bits(
                    relation.definition.state(),
                ) != 0
                    || crate::records::sketch_relations::constraint_kinds_iter(
                        relation.definition.state(),
                    )
                    .ne([SketchConstraintKind::SplineGroup])
                    || members.len() < 2
                    || !distinct_return_member_indices(ctx, members)?
                {
                    continue;
                }
                let Some(scope) = crate::ids::native_stream_charged(ctx, &relation.id, "resolve F3D sketch projection stream")? else {
                    continue;
                };
                let Some(curve) = (match members.last() {
                    Some(member) => ctx.get_hash_map(
                        &curves_by_record,
                        &(scope, member.reference.record_index()),
                        "f3d spatial spline curve lookup",
                    )?,
                    None => None,
                }) else {
                    continue;
                };
                let Some(SketchCurveGeometry::Nurbs { geometry, .. }) = curve.geometry.as_ref()
                else {
                    continue;
                };
                let poles = geometry.poles();
                if curve.owner_reference != Some(relation.owner_reference)
                    || poles.point_count() != members.len()
                {
                    continue;
                }
                let mut segment_storage =
                    ctx.reserve_scoped(0, "f3d spatial spline candidate storage")?;
                let mut segments = Vec::new();
                let mut complete = true;
                let mut candidates = members[..members.len() - 1]
                    .iter()
                    .zip(poles.points().zip(poles.points().skip(1)));
                while let Some((member, (first, second))) =
                    ctx.next_charged(&mut candidates, "f3d spatial spline candidate scan")?
                {
                    let candidate = (|| -> Result<Option<_>, CodecError> {
                        let record = member.reference.record_index();
                        let Some(member) = ctx.get_hash_map(
                            &curves_by_record,
                            &(scope, record),
                            "f3d spatial spline member lookup",
                        )?
                        else {
                            return Ok(None);
                        };
                        if member.owner_reference != Some(relation.owner_reference) {
                            return Ok(None);
                        }
                        Ok(match member.geometry.as_ref() {
                            None => Some((record, [*first, *second])),
                            Some(SketchCurveGeometry::Line { start, end, .. })
                                if start.as_raw() == first && end.as_raw() == second =>
                            {
                                Some((record, [*first, *second]))
                            }
                            _ => None,
                        })
                    })()?;
                    let Some(candidate) = candidate else {
                        complete = false;
                        break;
                    };
                    segment_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut segments,
                            candidate,
                            "f3d spatial spline segment candidates",
                        )
                    })?;
                }
                if !complete {
                    continue;
                }
                for (record, points) in
                    ctx.admit_iter(segments, "f3d spatial spline accepted scan")?
                {
                    record_spline_segment(ctx, &mut spline_segments, scope, record, points)?;
                }
            }
            Ok((placements_by_suffix, spatial_owners, spline_segments))
        })?;
    let transform_point = |placement: &DesignSketchPlacement, point: &Point3| {
        let origin_scale = placement_origin_scale(placement);
        Point3::new(
            placement.transform()[0][0] * point.x
                + placement.transform()[0][1] * point.y
                + placement.transform()[0][2] * point.z
                + placement.transform()[0][3] * origin_scale,
            placement.transform()[1][0] * point.x
                + placement.transform()[1][1] * point.y
                + placement.transform()[1][2] * point.z
                + placement.transform()[1][3] * origin_scale,
            placement.transform()[2][0] * point.x
                + placement.transform()[2][1] * point.y
                + placement.transform()[2][2] * point.z
                + placement.transform()[2][3] * origin_scale,
        )
    };
    let transform_vector = |placement: &DesignSketchPlacement, vector: &Vector3| {
        Vector3::new(
            placement.transform()[0][0] * vector.x
                + placement.transform()[0][1] * vector.y
                + placement.transform()[0][2] * vector.z,
            placement.transform()[1][0] * vector.x
                + placement.transform()[1][1] * vector.y
                + placement.transform()[1][2] * vector.z,
            placement.transform()[2][0] * vector.x
                + placement.transform()[2][1] * vector.y
                + placement.transform()[2][2] * vector.z,
        )
    };

    let mut entities = Vec::new();
    for curve in ctx.admit_iter(curves, "f3d sketch projection curves scan")? {
        let Some(scope) = crate::ids::native_stream_charged(ctx, &curve.id, "resolve F3D sketch projection stream")? else {
            continue;
        };
        let Some(owner) = curve.owner_reference else {
            continue;
        };
        if !ctx.contains_hash_set(
            &spatial_owners,
            &(scope, owner),
            "f3d sketch spatial owner lookup",
        )? {
            continue;
        }
        let Some(placement) = ctx.get_hash_map(
            &placements_by_suffix,
            &(scope, owner),
            "f3d sketch placement lookup",
        )?
        else {
            continue;
        };
        let geometry =
            if let Some([start, end]) = ctx
                .get_hash_map(
                    &spline_segments,
                    &(scope, curve.record_index),
                    "f3d spatial spline segment lookup",
                )?
                .copied()
                .flatten()
            {
                let Ok(geometry) =
                    SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                        start: transform_point(placement, &start),
                        end: transform_point(placement, &end),
                    })
                else {
                    continue;
                };
                geometry
            } else {
                let Some(source) = curve.geometry.as_ref() else {
                    continue;
                };
                match source {
                    SketchCurveGeometry::Line { start, end, .. } => {
                        let Ok(geometry) = SpatialSketchGeometry::try_from(
                            SpatialSketchGeometryDefinition::Line {
                                start: transform_point(placement, start.as_raw()),
                                end: transform_point(placement, end.as_raw()),
                            },
                        ) else {
                            continue;
                        };
                        geometry
                    }
                    SketchCurveGeometry::Arc {
                        center,
                        normal,
                        reference_direction,
                        radius,
                        start_angle,
                        end_angle,
                    } => {
                        let center = transform_point(placement, center.as_raw());
                        let normal = transform_vector(placement, normal.as_raw());
                        let reference_direction =
                            transform_vector(placement, reference_direction.as_raw());
                        let radius = Length::from(*radius);
                        let definition = if (end_angle.get() - start_angle.get()).abs()
                            >= std::f64::consts::TAU
                                - EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_DESIGN_E9
                        {
                            SpatialSketchGeometryDefinition::Circle {
                                center,
                                normal,
                                reference_direction,
                                radius,
                            }
                        } else {
                            SpatialSketchGeometryDefinition::Arc {
                                center,
                                normal,
                                reference_direction,
                                radius,
                                start_angle: *start_angle,
                                end_angle: *end_angle,
                            }
                        };
                        let Ok(geometry) = SpatialSketchGeometry::try_from(definition) else {
                            continue;
                        };
                        geometry
                    }
                    SketchCurveGeometry::Nurbs { geometry, .. } => {
                        let poles = geometry.poles();
                        let transformed_poles = ctx.collect_vec(
                            poles
                                .points()
                                .map(|point| transform_point(placement, point)),
                            "f3d spatial sketch nurbs poles",
                        )?;
                        let weights = poles
                            .weights()
                            .next()
                            .is_some()
                            .then(|| {
                                ctx.collect_vec(poles.weights(), "f3d spatial sketch nurbs weights")
                            })
                            .transpose()?;
                        let curve3d = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                            ctx,
                            geometry.degree(),
                            geometry.knots_copy(ctx)?,
                            transformed_poles,
                            weights,
                            false,
                        )??;
                        let Ok(curve3d) = curve3d.try_into() else {
                            continue;
                        };
                        let Ok(geometry) = SpatialSketchGeometry::try_from(
                            SpatialSketchGeometryDefinition::Nurbs { curve: curve3d },
                        ) else {
                            continue;
                        };
                        geometry
                    }
                }
            };
        let sketch = crate::design::identity::neutral_spatial_sketch_id(ctx, placement)?;
        let native_ref =
            ctx.copy_retained_text(&curve.id, "f3d spatial sketch curve native reference")?;
        ctx.push_vec(
            &mut entities,
            SpatialSketchEntity::new(
                crate::design::identity::neutral_spatial_sketch_curve_id(
                    ctx,
                    &sketch,
                    curve.primary_id.get(),
                    curve.secondary_id,
                )?,
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
            "f3d spatial sketch curve entity",
        )?;
    }
    for point in ctx.admit_iter(points, "f3d sketch projection points scan")? {
        let Some(scope) = crate::ids::native_stream_charged(ctx, &point.id, "resolve F3D sketch projection stream")? else {
            continue;
        };
        let Some(owner) = point.owner_reference else {
            continue;
        };
        if !ctx.contains_hash_set(
            &spatial_owners,
            &(scope, owner),
            "f3d sketch spatial owner lookup",
        )? {
            continue;
        }
        let Some(placement) = ctx.get_hash_map(
            &placements_by_suffix,
            &(scope, owner),
            "f3d sketch placement lookup",
        )?
        else {
            continue;
        };
        let sketch = crate::design::identity::neutral_spatial_sketch_id(ctx, placement)?;
        let depth = point.depth();
        let Ok(geometry) =
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: transform_point(
                    placement,
                    &Point3::new(point.coordinates().u, point.coordinates().v, depth),
                ),
            })
        else {
            continue;
        };
        let native_ref =
            ctx.copy_retained_text(&point.id, "f3d spatial sketch point native reference")?;
        ctx.push_vec(
            &mut entities,
            SpatialSketchEntity::new(
                point.persistent_id().map_or_else(
                    || {
                        crate::design::identity::neutral_spatial_sketch_record_id(
                            ctx,
                            &sketch,
                            point.record_index,
                        )
                    },
                    |persistent_id| {
                        crate::design::identity::neutral_spatial_sketch_point_id(
                            ctx,
                            &sketch,
                            persistent_id,
                        )
                    },
                )?,
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
            "f3d spatial sketch point entity",
        )?;
    }
    for surface in ctx.admit_iter(surfaces, "f3d sketch projection surfaces scan")? {
        let Some(scope) = crate::ids::native_stream_charged(ctx, &surface.id, "resolve F3D sketch projection stream")? else {
            continue;
        };
        let Some(owner) = surface.owner_reference else {
            continue;
        };
        let Some(placement) = ctx.get_hash_map(
            &placements_by_suffix,
            &(scope, owner),
            "f3d sketch placement lookup",
        )?
        else {
            continue;
        };
        let sketch = crate::design::identity::neutral_spatial_sketch_id(ctx, placement)?;
        let entity_id = crate::design::identity::neutral_spatial_sketch_surface_id(
            ctx,
            &sketch,
            surface.persistent_id.get(),
        )?;
        let u_knots = ctx.collect_vec(
            surface
                .geometry
                .u_knots
                .iter()
                .copied()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            "f3d spatial sketch surface u knots",
        )?;
        let v_knots = ctx.collect_vec(
            surface
                .geometry
                .v_knots
                .iter()
                .copied()
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            "f3d spatial sketch surface v knots",
        )?;
        let mut control_points = Vec::new();
        for row in ctx.admit_iter(
            &surface.geometry.control_points,
            "f3d spatial surface row scan",
        )? {
            {
                ctx.reserve_vec(
                    &mut control_points,
                    1,
                    "f3d spatial sketch surface control rows",
                )?;
            }
            let points = ctx.collect_vec(
                row.iter()
                    .map(|point| transform_point(placement, &point.get())),
                "f3d spatial sketch surface control points",
            )?;
            control_points.push(points);
        }
        let native_ref =
            ctx.copy_retained_text(&surface.id, "f3d spatial sketch surface native reference")?;
        ctx.push_vec(
            &mut entities,
            SpatialSketchEntity::new(
                entity_id,
                sketch,
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::NurbsSurface {
                    surface: cadmpeg_ir::geometry::nurbs::BsplineSurface::new(
                        ctx,
                        surface.geometry.u_degree.get(),
                        surface.geometry.v_degree.get(),
                        u_knots,
                        v_knots,
                        control_points,
                    )?
                    .map_err(|error| {
                        crate::design::text::malformed_design(
                            ctx,
                            format_args!(
                                "F3D spatial sketch surface {} is invalid: {error}",
                                surface.id
                            ),
                        )
                    })?,
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            )
            .with_native_ref(Some(native_ref)),
            "f3d spatial sketch surface entity",
        )?;
    }
    ctx.stable_sort_by(
        &mut entities[..],
        |value| value.id(),
        Ord::cmp,
        "sort f3d design sketch_project 3",
    )?;
    let mut id_storage = ctx.reserve_scoped(0, "f3d spatial sketch id storage")?;
    let spatial_ids = id_storage.with_storage(|| -> Result<_, CodecError> {
        let mut ids = HashSet::new();
        for entity in ctx.admit_iter(&entities, "f3d spatial sketch entity scan")? {
            ctx.insert_hash_set(&mut ids, &entity.sketch, "f3d spatial sketch id index")?;
        }
        Ok(ids)
    })?;
    let mut sketches = Vec::new();
    for placement in ctx.admit_iter(placements, "f3d sketch projection placements scan")? {
        let (id, candidate_storage) = ctx.with_scoped_storage("f3d spatial sketch candidate identity storage", || {
            crate::design::identity::neutral_spatial_sketch_id(ctx, placement)
        })?;
        if !ctx.contains_hash_set(&spatial_ids, &id, "f3d spatial sketch id membership")? {
            continue;
        }
        let id = candidate_storage.commit_value(id)?;
        let profiles = closed_spatial_sketch_profiles(ctx, &id, &entities, linear_tolerance)?;
        let name =
            ctx.copy_retained_text(placement.entity_id.as_str(), "f3d spatial sketch name")?;
        let native_ref =
            ctx.copy_retained_text(&placement.id, "f3d spatial sketch native reference")?;
        ctx.push_vec(
            &mut sketches,
            SpatialSketch {
                profiles,
                id,
                name: Some(name),
                configuration: None,
                visible: placement
                    .visibility
                    .as_ref()
                    .map(|visibility| visibility.visible),
                native_ref: Some(native_ref),
            },
            "f3d spatial sketch",
        )?;
    }
    ctx.stable_sort_by(
        &mut sketches[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design sketch_project 4",
    )?;
    Ok((sketches, entities))
}

/// Project exact aggregate relations owned by model-space spatial sketches.
pub(crate) fn project_spatial_sketch_constraints(
    ctx: &DecodeContext<'_>,
    placements: &[DesignSketchPlacement],
    relations: &[SketchRelation],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    surfaces: &[SketchSurface],
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Result<Vec<cadmpeg_ir::sketches::SpatialSketchConstraint>, CodecError> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraint, SpatialSketchConstraintDefinitionInput as Definition,
        SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    };

    let mut lookup_storage = ctx.reserve_scoped(0, "f3d spatial constraint lookup storage")?;
    let (sketches, projected) = lookup_storage.with_storage(|| -> Result<_, CodecError> {
        let mut sketches = HashMap::new();
        {
            let mut membership_storage =
                ctx.reserve_scoped(0, "f3d spatial constraint membership storage")?;
            let admitted = membership_storage.with_storage(|| -> Result<_, CodecError> {
                let mut admitted = HashSet::new();
                for entity in ctx.admit_iter(entities, "f3d spatial constraint membership scan")? {
                    ctx.insert_hash_set(
                        &mut admitted,
                        &entity.sketch,
                        "f3d spatial constraint membership index",
                    )?;
                }
                Ok(admitted)
            })?;
            for placement in ctx.admit_iter(placements, "f3d spatial constraint placement scan")? {
                let mut identity_storage =
                    ctx.reserve_scoped(0, "f3d spatial constraint query identity storage")?;
                let id = identity_storage.with_storage(|| {
                    crate::design::identity::neutral_spatial_sketch_id(ctx, placement)
                })?;
                let Some(sketch) = ctx.get_hash_set(
                    &admitted,
                    &id,
                    "f3d spatial constraint sketch membership work",
                )?.copied() else {
                    continue;
                };
                let (Some(scope), Ok(owner)) = (
                    crate::ids::native_stream_charged(ctx, &placement.id, "f3d spatial constraint native stream")?,
                    u32::try_from(placement.entity_id.suffix()),
                ) else {
                    continue;
                };
                ctx.insert_hash_map(
                    &mut sketches,
                    (scope, owner),
                    (sketch, placement),
                    "f3d spatial constraint sketch index",
                )?;
            }
        }
        let mut record_storage = ctx.reserve_scoped(0, "f3d spatial constraint record storage")?;
        let record_indices = record_storage.with_storage(|| -> Result<_, CodecError> {
            let mut record_indices = HashMap::new();
            let mut records = curves
                .iter()
                .map(|curve| (curve.id.as_str(), curve.record_index))
                .chain(
                    points
                        .iter()
                        .map(|point| (point.id.as_str(), point.record_index)),
                )
                .chain(
                    surfaces
                        .iter()
                        .map(|surface| (surface.id.as_str(), surface.record_index)),
                );
            while let Some((native_ref, record_index)) =
                ctx.next_charged(&mut records, "f3d spatial constraint record scan")?
            {
                ctx.insert_hash_map(
                    &mut record_indices,
                    native_ref,
                    record_index,
                    "f3d spatial constraint native record index",
                )
                .map(|_| ())?;
            }
            Ok(record_indices)
        })?;
        let mut projected = HashMap::new();
        for entity in ctx.admit_iter(entities, "f3d spatial constraint entity scan")? {
            let Some(native_ref) = entity.native_ref.as_deref() else {
                continue;
            };
            let Some(scope) = crate::ids::native_stream_charged(ctx, native_ref, "f3d spatial constraint native stream")? else {
                continue;
            };
            let Some(record) = ctx.get_hash_map(
                &record_indices,
                native_ref,
                "f3d spatial constraint native record lookup",
            )?
            else {
                continue;
            };
            let (key, value) = ((scope, *record), entity);
            ctx.insert_hash_map(
                &mut projected,
                key,
                value,
                "f3d spatial constraint entity index",
            )
            .map(|_| ())?;
        }
        Ok((sketches, projected))
    })?;
    let mut constraints = Vec::new();
    for relation in ctx.admit_iter(relations, "f3d sketch projection relations scan")? {
        let mut output_storage = ctx.reserve_scoped(0, "f3d spatial constraint output storage")?;
        let Some(constraint) = output_storage.with_storage(
            || -> Result<Option<SpatialSketchConstraint>, CodecError> {
                let Some(sole_kind) = crate::design::relation_kinds::sole_constraint_kind(relation)
                else {
                    return Ok(None);
                };
                let Some(scope) = crate::ids::native_stream_charged(ctx, &relation.id, "f3d spatial constraint native stream")? else {
                    return Ok(None);
                };
                let Some((sketch, placement)) = ctx.get_hash_map(
                    &sketches,
                    &(scope, relation.owner_reference),
                    "f3d spatial constraint sketch lookup",
                )?
                else {
                    return Ok(None);
                };
                // The second relation run is the semantic member order. The first
                // run interleaves per-member relation ordinals and has no role
                // order, so it cannot define a neutral spatial constraint.
                let mut entity_storage =
                    ctx.reserve_scoped(0, "f3d spatial constraint member storage")?;
                let semantic_entities =
                    entity_storage.with_storage(|| -> Result<_, CodecError> {
                        let mut semantic_entities = Vec::new();
                        let mut returns = relation.return_members().iter();
                        while let Some(member) =
                            ctx.next_charged(&mut returns, "f3d spatial constraint return scan")?
                        {
                            let Some(entity) = ctx
                                .get_hash_map(
                                    &projected,
                                    &(scope, member.reference.record_index()),
                                    "f3d spatial constraint entity lookup",
                                )?
                                .copied()
                            else {
                                return Ok(None);
                            };
                            ctx.push_vec(
                                &mut semantic_entities,
                                entity,
                                "f3d spatial constraint semantic entity",
                            )?;
                        }
                        Ok(Some(semantic_entities))
                    })?;
                let Some(semantic_entities) = semantic_entities else {
                    return Ok(None);
                };
                let mut distinct_storage =
                    ctx.reserve_scoped(0, "f3d spatial constraint distinct storage")?;
                let unique = distinct_storage.with_storage(|| {
                    let mut distinct = HashSet::new();
                    ctx.all_by(
                        &semantic_entities,
                        |entity| {
                            ctx.insert_hash_set(
                                &mut distinct,
                                entity.id(),
                                "f3d spatial constraint distinct member",
                            )
                        },
                        "f3d spatial constraint distinct scan",
                    )
                })?;
                if !unique {
                    return Ok(None);
                }
                let definition = match sole_kind {
                    SketchConstraintKind::Coincident => {
                        let [first, second] = semantic_entities.as_slice() else {
                            return Ok(None);
                        };
                        let point_on_surface =
                            match (first.geometry.definition(), second.geometry.definition()) {
                                (
                                    SpatialSketchGeometryDefinition::Point { .. },
                                    SpatialSketchGeometryDefinition::NurbsSurface { .. },
                                ) => Some((first, second)),
                                (
                                    SpatialSketchGeometryDefinition::NurbsSurface { .. },
                                    SpatialSketchGeometryDefinition::Point { .. },
                                ) => Some((second, first)),
                                _ => None,
                            };
                        if let Some((point, surface)) = point_on_surface {
                            Definition::PointOnSurface {
                                point: (point.id()).try_clone_for_decode(
                                    ctx,
                                    "f3d spatial constraint operand id",
                                )?,
                                surface: (surface.id()).try_clone_for_decode(
                                    ctx,
                                    "f3d spatial constraint operand id",
                                )?,
                            }
                        } else {
                            let (
                                SpatialSketchGeometryDefinition::Point {
                                    position: first_position,
                                },
                                SpatialSketchGeometryDefinition::Point {
                                    position: second_position,
                                },
                            ) = (first.geometry.definition(), second.geometry.definition())
                            else {
                                return Ok(None);
                            };
                            let scale = 1.0
                                + first_position
                                    .x
                                    .abs()
                                    .max(first_position.y.abs())
                                    .max(first_position.z.abs())
                                    .max(second_position.x.abs())
                                    .max(second_position.y.abs())
                                    .max(second_position.z.abs());
                            if (first_position.x - second_position.x).abs()
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                                || (first_position.y - second_position.y).abs()
                                    > scale
                                        * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                                || (first_position.z - second_position.z).abs()
                                    > scale
                                        * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                            {
                                return Ok(None);
                            }
                            Definition::Coincident {
                                first: (first.id()).try_clone_for_decode(
                                    ctx,
                                    "f3d spatial constraint operand id",
                                )?,
                                second: (second.id()).try_clone_for_decode(
                                    ctx,
                                    "f3d spatial constraint operand id",
                                )?,
                            }
                        }
                    }
                    SketchConstraintKind::SplineGroup if semantic_entities.len() >= 2 => {
                        let mut members = Vec::new();
                        for entity in ctx
                            .admit_iter(&semantic_entities, "f3d spatial constraint member scan")?
                        {
                            let id = entity
                                .id()
                                .try_clone_for_decode(ctx, "f3d spatial constraint member id")?;
                            ctx.push_vec(&mut members, id, "f3d spatial constraint member")?;
                        }
                        Definition::SplineGroup { entities: members }
                    }
                    SketchConstraintKind::Tangent => {
                        let [first, second] = semantic_entities.as_slice() else {
                            return Ok(None);
                        };
                        let curve = |geometry: &SpatialSketchGeometry| {
                            matches!(
                                (geometry).definition(),
                                SpatialSketchGeometryDefinition::Line { .. }
                                    | SpatialSketchGeometryDefinition::Circle { .. }
                                    | SpatialSketchGeometryDefinition::Arc { .. }
                                    | SpatialSketchGeometryDefinition::Nurbs { .. }
                            )
                        };
                        if !curve(&first.geometry) || !curve(&second.geometry) {
                            return Ok(None);
                        }
                        Definition::Tangent {
                            first: (first.id())
                                .try_clone_for_decode(ctx, "f3d spatial constraint operand id")?,
                            second: (second.id())
                                .try_clone_for_decode(ctx, "f3d spatial constraint operand id")?,
                        }
                    }
                    SketchConstraintKind::Midpoint => {
                        let [first, second] = semantic_entities.as_slice() else {
                            return Ok(None);
                        };
                        let (point, line, position, start, end) =
                            match (first.geometry.definition(), second.geometry.definition()) {
                                (
                                    SpatialSketchGeometryDefinition::Point { position },
                                    SpatialSketchGeometryDefinition::Line { start, end },
                                ) => (first, second, position, start, end),
                                (
                                    SpatialSketchGeometryDefinition::Line { start, end },
                                    SpatialSketchGeometryDefinition::Point { position },
                                ) => (second, first, position, start, end),
                                _ => return Ok(None),
                            };
                        let midpoint = Point3::new(
                            (start.x + end.x) * 0.5,
                            (start.y + end.y) * 0.5,
                            (start.z + end.z) * 0.5,
                        );
                        let scale =
                            1.0 + midpoint.x.abs().max(midpoint.y.abs()).max(midpoint.z.abs());
                        if (position.x - midpoint.x).abs()
                            > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                            || (position.y - midpoint.y).abs()
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                            || (position.z - midpoint.z).abs()
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                        {
                            return Ok(None);
                        }
                        Definition::Midpoint {
                            point: (point.id())
                                .try_clone_for_decode(ctx, "f3d spatial constraint operand id")?,
                            entity: (line.id())
                                .try_clone_for_decode(ctx, "f3d spatial constraint operand id")?,
                        }
                    }
                    kind @ (SketchConstraintKind::Horizontal | SketchConstraintKind::Vertical) => {
                        let [entity] = semantic_entities.as_slice() else {
                            return Ok(None);
                        };
                        let SpatialSketchGeometryDefinition::Line { start, end } =
                            *entity.geometry.definition()
                        else {
                            return Ok(None);
                        };
                        let column = usize::from(kind == SketchConstraintKind::Vertical);
                        let direction = Vector3::new(
                            placement.transform()[0][column],
                            placement.transform()[1][column],
                            placement.transform()[2][column],
                        );
                        let line = end.vector_from(start.get());
                        let cross = line.cross(direction);
                        if line.norm() <= EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E12
                            || cross.norm()
                                > EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                                    * line.norm()
                        {
                            return Ok(None);
                        }
                        Definition::ParallelToDirection {
                            entity: (entity.id())
                                .try_clone_for_decode(ctx, "f3d spatial constraint operand id")?,
                            direction,
                        }
                    }
                    _ => return Ok(None),
                };
                let Ok(definition) =
                    cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(definition)
                else {
                    return Ok(None);
                };
                let sketch =
                    (sketch).try_clone_for_decode(ctx, "f3d spatial constraint sketch id")?;
                let native_ref = ctx
                    .copy_retained_text(&relation.id, "f3d spatial constraint native reference")?;
                Ok(Some(SpatialSketchConstraint {
                    id: crate::design::identity::neutral_sketch_constraint_id(
                        ctx,
                        &relation.id,
                        relation.record_index,
                    )?,
                    sketch,
                    definition,
                    native_ref: Some(native_ref),
                }))
            },
        )?
        else {
            continue;
        };
        output_storage.commit()?;
        ctx.push_vec(
            &mut constraints,
            constraint,
            "f3d spatial constraint output",
        )?;
    }
    ctx.stable_sort_by(
        &mut constraints[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design sketch_project 5",
    )?;
    Ok(constraints)
}

#[cfg(test)]
mod tests;
