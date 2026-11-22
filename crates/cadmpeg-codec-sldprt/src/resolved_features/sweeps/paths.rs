use super::super::sketch_lists::{
    curve_form, off_chord_arc, CurveForm, ListedRecord, SketchRecordLists,
};
use crate::layout::{sketch_record_head as head, wide_spatial_marker_coordinate_prefix as xyz};
use crate::records::FeatureInputLane;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SpatialSketchGeometry};

const PATH_JOIN_TOLERANCE_MM: f64 = 1.0e-7;
const MAX_SWEEP_PATH_CURVES: usize = 1024;

#[derive(Clone)]
pub(super) struct Curve<G> {
    pub(super) offset: usize,
    pub(super) native: String,
    pub(super) geometry: G,
    pub(super) start: Point3,
    pub(super) end: Point3,
}

fn planar_ends(geometry: &SketchGeometry) -> Option<(Point3, Point3)> {
    let point = |point: Point2| Point3::new(point.u, point.v, 0.0);
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            Some((point(start.get()), point(end.get())))
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let at = |angle: f64| {
                Point3::new(
                    center.get().u + radius.get() * angle.cos(),
                    center.get().v + radius.get() * angle.sin(),
                    0.0,
                )
            };
            Some((at(start_angle.get()), at(end_angle.get())))
        }
        _ => None,
    }
}

fn selected_curve(payload: &[u8], offset: usize) -> bool {
    View::u16_le_at(payload, offset + head::ROLE) == Some(1)
        && View::u16_le_at(payload, offset + head::SELECTOR)
            .is_some_and(|selector| selector & 8 == 0)
}

pub(super) fn planar(lane: &FeatureInputLane, owner: &str) -> Option<Vec<Curve<SketchGeometry>>> {
    let lists = SketchRecordLists::new(&lane.native_payload);
    let mut curves = Vec::new();
    for marker in lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(owner))
    {
        let offset = usize::try_from(marker.offset()).ok()?;
        if !selected_curve(&lane.native_payload, offset) {
            continue;
        }
        // A selected marker that is not an admitted curve may be the first or last
        // path member; ordering cannot detect its absence, so the path stays native.
        if !matches!(
            lists.record(offset),
            Some(ListedRecord::Curve { later: false, .. })
        ) {
            return None;
        }
        // The record's own geometry from the point records of its sketch, with
        // no chord test: the projection only scales, so it cannot move an arc's
        // ends off its endpoint point records.
        let geometry = off_chord_arc(&lane.native_payload, &lists, offset, &|[u, v]| {
            Some(Point2::new(u * 1000.0, v * 1000.0))
        })?
        .geometry;
        if matches!(geometry.definition(), SketchGeometryDefinition::Line { start, end } if start == end)
        {
            return None;
        }
        let (start, end) = planar_ends(&geometry)?;
        if curves.len() == MAX_SWEEP_PATH_CURVES {
            return None;
        }
        curves.push(Curve {
            offset,
            native: marker.id().to_owned(),
            geometry,
            start,
            end,
        });
    }
    ordered(curves)
}

pub(super) fn spatial(
    lane: &FeatureInputLane,
    owner: &str,
) -> Option<Vec<Curve<SpatialSketchGeometry>>> {
    let payload = &lane.native_payload;
    let lists = SketchRecordLists::new(payload);
    let points = spatial_primary_points(lane, owner);
    let mut curves = Vec::new();
    for marker in lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(owner))
    {
        let offset = usize::try_from(marker.offset()).ok()?;
        if !selected_curve(payload, offset) {
            continue;
        }
        let sketch = match lists.record(offset) {
            Some(ListedRecord::Curve { sketch, later }) => {
                let first = sketch.point(0)?;
                let owner_of_points = lane
                    .sketch_entities
                    .iter()
                    .find(|marker| usize::try_from(marker.offset()).ok() == Some(first))
                    .and_then(|marker| marker.feature_ref.as_deref());
                if owner_of_points.is_some_and(|value| value != owner) {
                    None
                } else if later {
                    return None;
                } else {
                    Some(sketch)
                }
            }
            Some(_) => continue,
            None => None,
        };
        let form = curve_form(payload, offset);
        let CurveForm::Line { start, end } = (match (form, sketch) {
            // An unlisted primary point record is read through the point roster; any
            // other unlisted selected record may be a path member, so the path stays native.
            (None, None) if View::u16_le_at(payload, offset + head::SELECTOR)? & 15 == 4 => {
                continue;
            }
            (None, None) => return None,
            (Some(form), _) => form,
            (None, Some(_)) => return None,
        }) else {
            return None;
        };
        let coordinate = |index: usize| {
            let Some(sketch) = sketch else {
                return points
                    .as_ref()?
                    .get(index.checked_sub(1)?)
                    .and_then(|(_, point)| *point);
            };
            let point = sketch.point(index)?;
            let fields = point.checked_add(xyz::COORDINATE_TAG)?;
            if !matches!(View::u16_le_at(payload, fields)?, 0x000e | 0x000f) {
                return None;
            }
            point3_at(payload, point + xyz::COORDINATES)
        };
        let start = coordinate(start)?;
        let end = coordinate(end)?;
        let geometry = SpatialSketchGeometry::try_line_from_parts(
            cadmpeg_ir::features::geometry::FinitePoint3::new(start)?,
            cadmpeg_ir::features::geometry::FinitePoint3::new(end)?,
        )
        .ok()?;
        if curves.len() == MAX_SWEEP_PATH_CURVES {
            return None;
        }
        curves.push(Curve {
            offset,
            native: marker.id().to_owned(),
            geometry,
            start,
            end,
        });
    }
    ordered(curves)
}

fn spatial_primary_points(
    lane: &FeatureInputLane,
    owner: &str,
) -> Option<Vec<(usize, Option<Point3>)>> {
    let payload = &lane.native_payload;
    let mut points = Vec::new();
    for marker in lane
        .sketch_entities
        .iter()
        .filter(|marker| marker.feature_ref.as_deref() == Some(owner))
    {
        let offset = usize::try_from(marker.offset()).ok()?;
        if !selected_curve(payload, offset)
            || View::u16_le_at(payload, offset + head::SELECTOR)? & 15 != 4
            || curve_form(payload, offset).is_some()
        {
            continue;
        }
        if View::u16_le_at(payload, offset + head::OBJECT_POINTER)? != 0
            || payload.get(offset + head::ZERO_PAD..offset + xyz::COORDINATE_TAG)? != [0; 8]
            || !matches!(
                View::u16_le_at(payload, offset + xyz::COORDINATE_TAG)?,
                0x000e | 0x000f
            )
        {
            return None;
        }
        points.push((offset, point3_at(payload, offset + xyz::COORDINATES)));
    }
    points.sort_unstable_by_key(|(offset, _)| *offset);
    Some(points)
}

fn point3_at(payload: &[u8], offset: usize) -> Option<Point3> {
    let point = Point3::new(
        View::f64_le_at(payload, offset)? * 1000.0,
        View::f64_le_at(payload, offset + 8)? * 1000.0,
        View::f64_le_at(payload, offset + 16)? * 1000.0,
    );
    cadmpeg_ir::features::geometry::FinitePoint3::new(point)
        .map(cadmpeg_ir::features::geometry::FinitePoint3::get)
}

fn coincident(left: Point3, right: Point3) -> bool {
    (left.x - right.x).abs() <= PATH_JOIN_TOLERANCE_MM
        && (left.y - right.y).abs() <= PATH_JOIN_TOLERANCE_MM
        && (left.z - right.z).abs() <= PATH_JOIN_TOLERANCE_MM
}

fn ordered<G>(mut curves: Vec<Curve<G>>) -> Option<Vec<Curve<G>>> {
    let mut starts = curves
        .iter()
        .enumerate()
        .filter(|(_, curve)| {
            !curves
                .iter()
                .any(|other| coincident(other.end, curve.start))
        })
        .map(|(index, _)| index);
    let first = starts.next()?;
    if starts.next().is_some() {
        return None;
    }
    let mut result = Vec::with_capacity(curves.len());
    result.push(curves.remove(first));
    while !curves.is_empty() {
        let end = result.last()?.end;
        let mut next = curves
            .iter()
            .enumerate()
            .filter(|(_, curve)| coincident(end, curve.start))
            .map(|(index, _)| index);
        let index = next.next()?;
        if next.next().is_some() {
            return None;
        }
        result.push(curves.remove(index));
    }
    Some(result)
}

#[cfg(test)]
mod tests;
