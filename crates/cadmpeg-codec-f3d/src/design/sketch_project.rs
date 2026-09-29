// SPDX-License-Identifier: Apache-2.0
//! Project planar and spatial sketch geometry.

use crate::design::dimensions::{planar_point, sketch_normal_sign};
use crate::design::face_resolve::{placement_origin_scale, sketch_curve_is_spatial};
use crate::design::feature_project::closed_spatial_sketch_profiles;
use crate::design::geometry::closed_sketch_profiles;
use crate::ids::native_stream;
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
use std::hash::Hash;

const EPS_SPATIAL_OWNER_DEPTH: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_DESIGN_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E12: f64 = 1.0e-12;

fn insert_project_index<K: Eq + Hash, V>(
    ctx: Option<&DecodeContext<'_>>,
    index: &mut HashMap<K, V>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !index.contains_key(&key) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
            index.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
        }
    }
    // discarded-value: duplicate index keys retain the last source record.
    let _ = index.insert(key, value);
    Ok(())
}

fn insert_project_set<T: Eq + Hash>(
    ctx: Option<&DecodeContext<'_>>,
    set: &mut HashSet<T>,
    value: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !set.contains(&value) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
            set.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
        }
        set.insert(value);
    }
    Ok(())
}

fn collect_project_items<T>(
    ctx: Option<&DecodeContext<'_>>,
    items: impl IntoIterator<Item = T>,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut collected = Vec::new();
    for item in items {
        push_project_item(ctx, &mut collected, item, operation)?;
    }
    Ok(collected)
}

fn push_project_item<T>(
    ctx: Option<&DecodeContext<'_>>,
    items: &mut Vec<T>,
    item: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, operation)?;
        items.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    }
    items.push(item);
    Ok(())
}

fn copy_project_text(
    ctx: Option<&DecodeContext<'_>>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let Some(ctx) = ctx else { return Ok(value.to_owned()); };
    let bytes = ctx.copy_retained(value.as_bytes(), operation)?;
    String::from_utf8(bytes).map_err(|_| CodecError::malformed("validated sketch text is not UTF-8"))
}

fn copy_project_id<T>(
    ctx: Option<&DecodeContext<'_>>,
    value: &str,
    operation: &'static str,
) -> Result<T, CodecError>
where
    T: TryFrom<String>,
    T::Error: std::fmt::Display,
{
    T::try_from(copy_project_text(ctx, value, operation)?).map_err(CodecError::malformed)
}

fn record_spline_segment<'a>(
    ctx: Option<&DecodeContext<'_>>,
    segments: &mut HashMap<(&'a str, u32), Option<[Point3; 2]>>,
    scope: &'a str,
    record: u32,
    points: [Point3; 2],
) -> Result<(), CodecError> {
    if !segments.contains_key(&(scope, record)) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "f3d spatial sketch spline segment index")?;
            segments.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d spatial sketch spline segment allocation", 0, 1)
            })?;
        }
    }
    segments
        .entry((scope, record))
        .and_modify(|existing| {
            if *existing != Some(points) {
                *existing = None;
            }
        })
        .or_insert(Some(points));
    Ok(())
}

fn distinct_return_member_indices(
    ctx: Option<&DecodeContext<'_>>,
    members: &[crate::records::sketch_relations::SketchRelationReturnMember],
) -> Result<bool, CodecError> {
    let mut seen = HashSet::new();
    for member in members {
        let index = member.reference.record_index();
        if seen.contains(&index) {
            return Ok(false);
        }
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "f3d spatial spline member index")?;
            seen.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d spatial spline member allocation", 0, 1)
            })?;
        }
        seen.insert(index);
    }
    Ok(true)
}

fn spatial_geometry_owners<'a>(
    ctx: Option<&DecodeContext<'_>>,
    points: &'a [SketchPoint],
    curves: &'a [SketchCurveIdentity],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let mut owners = HashSet::new();
    for curve in curves.iter().filter(|curve| sketch_curve_is_spatial(curve)) {
        let (Some(scope), Some(owner)) = (native_stream(&curve.id), curve.owner_reference) else {
            continue;
        };
        if !owners.contains(&(scope, owner)) {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "f3d spatial geometry owner")?;
                owners.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d spatial geometry owner allocation", 0, 1)
                })?;
            }
            owners.insert((scope, owner));
        }
    }
    for point in points.iter().filter(|point| point.depth().abs() > EPS_SPATIAL_OWNER_DEPTH) {
        let (Some(scope), Some(owner)) = (native_stream(&point.id), point.owner_reference) else {
            continue;
        };
        if !owners.contains(&(scope, owner)) {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "f3d spatial geometry owner")?;
                owners.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d spatial geometry owner allocation", 0, 1)
                })?;
            }
            owners.insert((scope, owner));
        }
    }
    Ok(owners)
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
    ctx: Option<&DecodeContext<'_>>,
    relations: &'a [SketchRelation],
    curves: &'a [SketchCurveIdentity],
    texts: &'a [SketchText],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let mut curve_owners = HashMap::new();
    for curve in curves {
        let (Some(scope), Some(owner)) = (native_stream(&curve.id), curve.owner_reference) else {
            continue;
        };
        let key = (scope, curve.record_index);
        if !curve_owners.contains_key(&key) {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "f3d text frame curve owner")?;
                curve_owners.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d text frame curve owner allocation", 0, 1)
                })?;
            }
        }
        curve_owners.insert(key, owner);
    }
    let mut text_owners = HashMap::new();
    for text in texts {
        let Some(scope) = native_stream(&text.id) else { continue; };
        let key = (scope, text.record_index);
        if !text_owners.contains_key(&key) {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "f3d text frame text owner")?;
                text_owners.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d text frame text owner allocation", 0, 1)
                })?;
            }
        }
        text_owners.insert(key, text.owner_reference);
    }
    let mut frame_curves = HashSet::new();
    for relation in relations {
        let Some(crate::records::sketch_relations::SketchPatternDefinition::TextFrame {
            text_reference,
        }) = relation.definition.pattern() else { continue; };
        let Some(scope) = native_stream(&relation.id) else { continue; };
        if crate::design::relation_kinds::sole_constraint_kind(relation).is_none()
            || relation.members().first().map(|member| member.reference.record_index())
                != Some(*text_reference)
            || !relation.auxiliary_references().values().copied().eq([*text_reference])
            || relation.members().len() < 2
            || !relation.return_members().iter().map(|member| member.reference.record_index())
                .eq(relation.members().iter().skip(1).map(|member| member.reference.record_index()))
            || text_owners.get(&(scope, *text_reference)) != Some(&relation.owner_reference)
        {
            continue;
        }
        if !relation.return_members().iter().all(|member| {
            curve_owners.get(&(scope, member.reference.record_index()))
                == Some(&relation.owner_reference)
        }) {
            continue;
        }
        for member in relation.return_members().iter() {
            let key = (scope, member.reference.record_index());
            if !frame_curves.contains(&key) {
                if let Some(ctx) = ctx {
                    ctx.charge_collection_items(1, "f3d text frame curve record")?;
                    frame_curves.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit("f3d text frame curve record allocation", 0, 1)
                    })?;
                }
                frame_curves.insert(key);
            }
        }
    }
    Ok(frame_curves)
}

/// Project placed Design sketches and their exact planar point/curve records.
pub(crate) fn project_sketch_design(
    ctx: Option<&cadmpeg_core::decode::DecodeContext<'_>>,
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

    let text_frame_curves = text_frame_curve_records(ctx, relations, curves, texts)?;
    let mut placements_by_suffix = HashMap::new();
    for placement in placements {
        let (Some(scope), Ok(owner)) = (
            native_stream(&placement.id),
            u32::try_from(placement.entity_id.suffix()),
        ) else { continue; };
        insert_project_index(
            ctx, &mut placements_by_suffix, (scope, owner), placement,
            "f3d planar sketch placement index",
        )?;
    }
    let spatial_owners = spatial_geometry_owners(ctx, points, curves)?;
    let mut sketches = Vec::new();
    for placement in placements {
        if u32::try_from(placement.entity_id.suffix()).is_ok_and(|owner| {
            native_stream(&placement.id)
                .is_some_and(|scope| spatial_owners.contains(&(scope, owner)))
        }) {
            continue;
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
        ) else { continue; };
        let name = copy_project_text(ctx, placement.entity_id.as_str(), "f3d planar sketch name")?;
        let native_ref = copy_project_text(ctx, &placement.id, "f3d planar sketch native reference")?;
        push_project_item(ctx, &mut sketches,
            Sketch {
                id: crate::design::identity::neutral_sketch_id(ctx,placement)?,
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
    sketches.sort_by(|a, b| a.id.cmp(&b.id));

    let mut entities = Vec::new();
    for point in points {
        let Some(owner) = point.owner_reference else { continue; };
        let Some(scope) = native_stream(&point.id) else { continue; };
        if spatial_owners.contains(&(scope, owner)) { continue; }
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else { continue; };
        let sketch = crate::design::identity::neutral_sketch_id(ctx,placement)?;
        let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: point.coordinates(),
        }) else { continue; };
        let native_ref = copy_project_text(ctx, &point.id, "f3d planar sketch point native reference")?;
        push_project_item(ctx, &mut entities,
            SketchEntity::new(
                    point.persistent_id().map_or_else(
                        || crate::design::identity::neutral_sketch_record_id(ctx,&sketch, point.record_index),
                        |persistent_id| crate::design::identity::neutral_sketch_point_id(ctx,&sketch, persistent_id),
                    )?,
                    sketch,
                    geometry,
            ).with_native_ref(Some(native_ref)),
            "f3d planar sketch point entity",
        )?;
    }
    for curve in curves {
        let Some(owner) = curve.owner_reference else {
            continue;
        };
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        if spatial_owners.contains(&(scope, owner)) {
            continue;
        }
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
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
                    && geometry.poles().point_count() > index_from_u32(geometry.degree())
                    && geometry.poles().points().all(planar_point) =>
            {
                let poles = geometry.poles();
                let planar_poles = collect_project_items(
                    ctx,
                    poles.points().map(|point| Point2::new(point.x, point.y)),
                    "f3d planar sketch nurbs poles",
                )?;
                let weights = poles.weights().next().is_some()
                    .then(|| collect_project_items(
                        ctx, poles.weights(), "f3d planar sketch nurbs weights",
                    ))
                    .transpose()?;
                SketchGeometry::nurbs(cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                    geometry.degree(),
                    geometry.knots(),
                    planar_poles,
                    weights,
                    false,
                )?)
            }
            _ => continue,
        };
        let sketch = crate::design::identity::neutral_sketch_id(ctx,placement)?;
        let native_ref = copy_project_text(ctx, &curve.id, "f3d planar sketch curve native reference")?;
        push_project_item(ctx, &mut entities,
            SketchEntity::new(
                crate::design::identity::neutral_sketch_curve_id(ctx,&sketch, curve.primary_id.get(), curve.secondary_id)?,
                sketch,
                geometry,
            )
            .with_construction(text_frame_curves.contains(&(scope, curve.record_index)))
            .with_native_ref(Some(native_ref)),
            "f3d planar sketch curve entity",
        )?;
    }
    for text in texts {
        let Some(scope) = native_stream(&text.id) else { continue; };
        let Some(placement) = placements_by_suffix.get(&(scope, text.owner_reference)) else { continue; };
        let sketch = crate::design::identity::neutral_sketch_id(ctx,placement)?;
        let Some(text_value) = cadmpeg_core::text::NonBlankString::new(
            copy_project_text(ctx, &text.text, "f3d planar sketch text")?,
        ) else { continue; };
        let Some(font_family) = cadmpeg_core::text::NonBlankString::new(
            copy_project_text(ctx, &text.font_family, "f3d planar sketch font family")?,
        ) else { continue; };
        let Ok(font_weight) = text.font_weight.try_into() else { continue; };
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
        }) else { continue; };
        let native_ref = copy_project_text(ctx, &text.id, "f3d planar sketch text native reference")?;
        push_project_item(ctx, &mut entities,
            SketchEntity::new(
                text.persistent_id.map_or_else(
                    || crate::design::identity::neutral_sketch_record_id(ctx,&sketch, text.record_index),
                    |persistent_id| crate::design::identity::neutral_sketch_text_id(ctx,&sketch, persistent_id),
                )?,
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
            "f3d planar sketch text entity",
        )?;
    }
    entities.sort_by(|a, b| a.id().cmp(b.id()));
    for sketch in &mut sketches {
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
    ctx: Option<&DecodeContext<'_>>,
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

    let mut placements_by_suffix = HashMap::new();
    for placement in placements {
        let (Some(scope), Ok(owner)) = (
            native_stream(&placement.id),
            u32::try_from(placement.entity_id.suffix()),
        ) else { continue; };
        insert_project_index(
            ctx, &mut placements_by_suffix, (scope, owner), placement,
            "f3d spatial sketch placement index",
        )?;
    }
    let mut spatial_owners = spatial_geometry_owners(ctx, points, curves)?;
    for surface in surfaces {
        let (Some(scope), Some(owner)) = (native_stream(&surface.id), surface.owner_reference) else {
            continue;
        };
        if !spatial_owners.contains(&(scope, owner)) {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "f3d spatial surface owner")?;
                spatial_owners.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d spatial surface owner allocation", 0, 1)
                })?;
            }
            spatial_owners.insert((scope, owner));
        }
    }
    let mut curves_by_record = HashMap::new();
    for curve in curves {
        let Some(scope) = native_stream(&curve.id) else { continue; };
        insert_project_index(
            ctx, &mut curves_by_record, (scope, curve.record_index), curve,
            "f3d spatial sketch curve index",
        )?;
    }
    let mut spline_segments = HashMap::new();
    for relation in relations {
        // Only the second reference run of a relation record is in semantic
        // order: the control polygon ends with the spline there, and the
        // interleaved first run orders its members by nothing a reader can use.
        let members = relation.return_members();
        if crate::design::relation_kinds::unknown_constraint_bits(relation.definition.state()) != 0
            || crate::records::sketch_relations::constraint_kinds_iter(relation.definition.state())
                .ne([SketchConstraintKind::SplineGroup])
            || members.len() < 2
            || !distinct_return_member_indices(ctx, members)?
        {
            continue;
        }
        let Some(scope) = native_stream(&relation.id) else {
            continue;
        };
        let Some(curve) = members
            .last()
            .and_then(|member| curves_by_record.get(&(scope, member.reference.record_index())))
        else {
            continue;
        };
        let Some(SketchCurveGeometry::Nurbs { geometry, .. }) = curve.geometry.as_ref() else {
            continue;
        };
        let poles = geometry.poles();
        if curve.owner_reference != Some(relation.owner_reference)
            || poles.point_count() != members.len()
        {
            continue;
        }
        let mut segments = Vec::new();
        let mut complete = true;
        for (member, (first, second)) in members[..members.len() - 1]
            .iter()
            .zip(poles.points().zip(poles.points().skip(1)))
        {
            let candidate = (|| {
                let record = member.reference.record_index();
                let member = curves_by_record.get(&(scope, record))?;
                if member.owner_reference != Some(relation.owner_reference) {
                    return None;
                }
                match member.geometry.as_ref() {
                    None => Some((record, [*first, *second])),
                    Some(SketchCurveGeometry::Line { start, end, .. })
                        if start.as_raw() == first && end.as_raw() == second =>
                    {
                        Some((record, [*first, *second]))
                    }
                    _ => None,
                }
            })();
            let Some(candidate) = candidate else { complete = false; break; };
            push_project_item(ctx, &mut segments, candidate, "f3d spatial spline segment candidates")?;
        }
        if !complete { continue; }
        for (record, points) in segments {
            record_spline_segment(ctx, &mut spline_segments, scope, record, points)?;
        }
    }
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
    for curve in curves {
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        let Some(owner) = curve.owner_reference else {
            continue;
        };
        if !spatial_owners.contains(&(scope, owner)) {
            continue;
        }
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
            continue;
        };
        let geometry =
            if let Some([start, end]) = spline_segments
                .get(&(scope, curve.record_index))
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
                        let transformed_poles = collect_project_items(
                            ctx,
                            poles.points().map(|point| transform_point(placement, point)),
                            "f3d spatial sketch nurbs poles",
                        )?;
                        let weights = poles.weights().next().is_some()
                            .then(|| collect_project_items(
                                ctx, poles.weights(), "f3d spatial sketch nurbs weights",
                            ))
                            .transpose()?;
                        let curve3d = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                            geometry.degree(),
                            geometry.knots(),
                            transformed_poles,
                            weights,
                            false,
                        )?;
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
        let sketch = crate::design::identity::neutral_spatial_sketch_id(ctx,placement)?;
        let native_ref = copy_project_text(ctx, &curve.id, "f3d spatial sketch curve native reference")?;
        push_project_item(ctx, &mut entities,
            SpatialSketchEntity::new(
                crate::design::identity::neutral_spatial_sketch_curve_id(ctx,
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
    for point in points {
        let Some(scope) = native_stream(&point.id) else { continue; };
        let Some(owner) = point.owner_reference else { continue; };
        if !spatial_owners.contains(&(scope, owner)) {
            continue;
        }
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else { continue; };
        let sketch = crate::design::identity::neutral_spatial_sketch_id(ctx,placement)?;
        let depth = point.depth();
        let Ok(geometry) = SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
            position: transform_point(
                placement,
                &Point3::new(point.coordinates().u, point.coordinates().v, depth),
            ),
        }) else { continue; };
        let native_ref = copy_project_text(ctx, &point.id, "f3d spatial sketch point native reference")?;
        push_project_item(ctx, &mut entities,
            SpatialSketchEntity::new(
                point.persistent_id().map_or_else(
                    || crate::design::identity::neutral_spatial_sketch_record_id(ctx,&sketch, point.record_index),
                    |persistent_id| crate::design::identity::neutral_spatial_sketch_point_id(ctx,&sketch, persistent_id),
                )?,
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
            "f3d spatial sketch point entity",
        )?;
    }
    for surface in surfaces {
        let Some(scope) = native_stream(&surface.id) else {
            continue;
        };
        let Some(owner) = surface.owner_reference else {
            continue;
        };
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
            continue;
        };
        let sketch = crate::design::identity::neutral_spatial_sketch_id(ctx,placement)?;
        let entity_id = crate::design::identity::neutral_spatial_sketch_surface_id(ctx,&sketch, surface.persistent_id.get())?;
        let u_knots = collect_project_items(
            ctx,
            surface.geometry.u_knots.iter().copied().map(cadmpeg_ir::scalar::FiniteReal::get),
            "f3d spatial sketch surface u knots",
        )?;
        let v_knots = collect_project_items(
            ctx,
            surface.geometry.v_knots.iter().copied().map(cadmpeg_ir::scalar::FiniteReal::get),
            "f3d spatial sketch surface v knots",
        )?;
        let mut control_points = Vec::new();
        for row in &surface.geometry.control_points {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "f3d spatial sketch surface control rows")?;
                control_points.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d spatial sketch surface control row allocation", 0, 1)
                })?;
            }
            let points = collect_project_items(
                ctx,
                row.iter().map(|point| transform_point(placement, &point.get())),
                "f3d spatial sketch surface control points",
            )?;
            control_points.push(points);
        }
        let native_ref = copy_project_text(ctx, &surface.id, "f3d spatial sketch surface native reference")?;
        push_project_item(ctx, &mut entities,
            SpatialSketchEntity::new(
                entity_id,
                sketch,
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::NurbsSurface {
                    surface: cadmpeg_ir::geometry::nurbs::BsplineSurface::new(
                        surface.geometry.u_degree.get(),
                        surface.geometry.v_degree.get(),
                        u_knots,
                        v_knots,
                        control_points,
                    )
                    .map_err(|error| {
                        crate::design::text::malformed_design(ctx, format_args!(
                            "F3D spatial sketch surface {} is invalid: {error}",
                            surface.id
                        ))
                    })?,
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            )
            .with_native_ref(Some(native_ref)),
            "f3d spatial sketch surface entity",
        )?;
    }
    entities.sort_by(|a, b| a.id().cmp(b.id()));
    let mut spatial_ids = HashSet::<cadmpeg_ir::sketches::SpatialSketchId>::new();
    for entity in &entities {
        if !spatial_ids.contains(&entity.sketch) {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "f3d spatial sketch id index")?;
                spatial_ids.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d spatial sketch id index allocation", 0, 1)
                })?;
            }
            spatial_ids.insert(copy_project_id(ctx, entity.sketch.as_str(),
                "f3d spatial sketch id index copy")?);
        }
    }
    let mut sketches = Vec::new();
    for placement in placements {
        let id = crate::design::identity::neutral_spatial_sketch_id(ctx,placement)?;
        if !spatial_ids.contains(&id) { continue; }
        let profiles = closed_spatial_sketch_profiles(ctx, &id, &entities, linear_tolerance)?;
        let name = copy_project_text(ctx, placement.entity_id.as_str(), "f3d spatial sketch name")?;
        let native_ref = copy_project_text(ctx, &placement.id, "f3d spatial sketch native reference")?;
        push_project_item(ctx, &mut sketches,
            SpatialSketch {
                profiles,
                id,
                name: Some(name),
                configuration: None,
                visible: placement.visibility.as_ref().map(|visibility| visibility.visible),
                native_ref: Some(native_ref),
            },
            "f3d spatial sketch",
        )?;
    }
    sketches.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((sketches, entities))
}

/// Project exact aggregate relations owned by model-space spatial sketches.
pub(crate) fn project_spatial_sketch_constraints(
    ctx: Option<&DecodeContext<'_>>,
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

    let mut sketches = HashMap::new();
    for placement in placements {
        let id = crate::design::identity::neutral_spatial_sketch_id(ctx,placement)?;
        if let Some(ctx) = ctx {
            let work = u64::try_from(entities.len()).map_err(|_| {
                ctx.refuse_codec_limit("f3d spatial constraint sketch membership work", 0, 1)
            })?;
            ctx.charge_work(work, "f3d spatial constraint sketch membership work")?;
        }
        if !entities.iter().any(|entity| entity.sketch == id) {
            continue;
        }
        let Some((key, value)) = (|| {
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_id.suffix()).ok()?,
                ),
                (id, placement),
            ))
        })() else { continue; };
        insert_project_index(ctx, &mut sketches, key, value, "f3d spatial constraint sketch index")?;
    }
    let mut record_indices = HashMap::new();
    for (native_ref, record_index) in curves.iter().map(|curve| (curve.id.as_str(), curve.record_index))
        .chain(points.iter().map(|point| (point.id.as_str(), point.record_index)))
        .chain(surfaces.iter().map(|surface| (surface.id.as_str(), surface.record_index)))
    {
        insert_project_index(
            ctx, &mut record_indices, native_ref, record_index,
            "f3d spatial constraint native record index",
        )?;
    }
    let mut projected = HashMap::new();
    for entity in entities {
        let Some((key, value)) = (|| {
            let native_ref = entity.native_ref.as_deref()?;
            Some((
                (native_stream(native_ref)?, *record_indices.get(native_ref)?),
                entity,
            ))
        })() else { continue; };
        insert_project_index(ctx, &mut projected, key, value, "f3d spatial constraint entity index")?;
    }
    let mut constraints = Vec::new();
    for relation in relations {
        let Some(constraint) = (|| -> Result<Option<SpatialSketchConstraint>, CodecError> {
            let Some(sole_kind) = crate::design::relation_kinds::sole_constraint_kind(relation) else { return Ok(None); };
            let Some(scope) = native_stream(&relation.id) else { return Ok(None); };
            let Some((sketch, placement)) = sketches.get(&(scope, relation.owner_reference)) else {
                return Ok(None);
            };
            // The second relation run is the semantic member order. The first
            // run interleaves per-member relation ordinals and has no role
            // order, so it cannot define a neutral spatial constraint.
            let mut semantic_entities = Vec::new();
            for member in relation.return_members().iter() {
                let Some(entity) = projected.get(&(scope, member.reference.record_index())).copied() else {
                    return Ok(None);
                };
                push_project_item(ctx, &mut semantic_entities, entity,
                    "f3d spatial constraint semantic entity")?;
            }
            let mut members = Vec::new();
            for entity in &semantic_entities {
                let id = copy_project_id(ctx, entity.id().as_str(),
                    "f3d spatial constraint member id")?;
                push_project_item(ctx, &mut members, id, "f3d spatial constraint member")?;
            }
            let mut distinct = HashSet::new();
            for member in &members {
                insert_project_set(ctx, &mut distinct, member,
                    "f3d spatial constraint distinct member")?;
            }
            if distinct.len() != members.len() {
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
                            point: copy_project_id(ctx, point.id().as_str(), "f3d spatial constraint operand id")?,
                            surface: copy_project_id(ctx, surface.id().as_str(), "f3d spatial constraint operand id")?,
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
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                            || (first_position.z - second_position.z).abs()
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                        {
                            return Ok(None);
                        }
                        Definition::Coincident {
                            first: copy_project_id(ctx, first.id().as_str(), "f3d spatial constraint operand id")?,
                            second: copy_project_id(ctx, second.id().as_str(), "f3d spatial constraint operand id")?,
                        }
                    }
                }
                SketchConstraintKind::SplineGroup if members.len() >= 2 => {
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
                        first: copy_project_id(ctx, first.id().as_str(), "f3d spatial constraint operand id")?,
                        second: copy_project_id(ctx, second.id().as_str(), "f3d spatial constraint operand id")?,
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
                    let scale = 1.0 + midpoint.x.abs().max(midpoint.y.abs()).max(midpoint.z.abs());
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
                        point: copy_project_id(ctx, point.id().as_str(), "f3d spatial constraint operand id")?,
                        entity: copy_project_id(ctx, line.id().as_str(), "f3d spatial constraint operand id")?,
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
                            > EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9 * line.norm()
                    {
                        return Ok(None);
                    }
                    Definition::ParallelToDirection {
                        entity: copy_project_id(ctx, entity.id().as_str(), "f3d spatial constraint operand id")?,
                        direction,
                    }
                }
                _ => return Ok(None),
            };
            let Ok(definition) = cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(
                definition,
            ) else { return Ok(None); };
            let sketch = copy_project_id(ctx, sketch.as_str(), "f3d spatial constraint sketch id")?;
            let native_ref = copy_project_text(ctx, &relation.id, "f3d spatial constraint native reference")?;
            Ok(Some(SpatialSketchConstraint {
                id: crate::design::identity::neutral_sketch_constraint_id(ctx,&relation.id, relation.record_index)?,
                sketch,
                definition,
                native_ref: Some(native_ref),
            }))
        })()? else { continue; };
        push_project_item(ctx, &mut constraints, constraint,
            "f3d spatial constraint output")?;
    }
    constraints.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(constraints)
}

#[cfg(test)]
mod tests;
