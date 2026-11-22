// SPDX-License-Identifier: Apache-2.0
//! The user features of a generated part as the native tree writes them,
//! from the CADIR model and the history summary's feature records.

use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch as IrSketch, SketchGeometryDefinition};

use super::sketch::{Curve, Point, Sketch};
use super::tree::Plane;
use crate::resolved_features::write_prepare::{
    generated_profile_stream, generated_sketch_owner_id, generated_sketch_owner_record,
    unique_planar_sketch_owner, GENERATED_NAME,
};

/// Largest departure of a sketch normal from a default plane's normal, as
/// one minus the absolute cosine.
const PLANE_NORMAL_TOLERANCE: f64 = 1.0e-9;

/// Largest distance, in millimetres, of a sketch origin from a default plane.
const PLANE_OFFSET_TOLERANCE_MM: f64 = 1.0e-6;

/// Largest distance, in metres, between two loci written as one point record.
const POINT_MERGE_TOLERANCE_M: f64 = 1.0e-9;

/// Millimetres to metres.
const METRES_PER_MILLIMETRE: f64 = 0.001;

/// The user sketches of a generated part, or why the tree writes none of them.
#[derive(Debug)]
pub(in crate::writer) enum Sketches {
    /// Every sketch, in source order.
    Native(Vec<Sketch>),
    /// The sketches stay in the generated lane; the text names the first
    /// sketch the tree cannot write and why.
    Lane(String),
}

/// The sketches of `ir`: every planar sketch of the first configuration as a
/// native sketch, when each lies on a default plane and holds only points,
/// lines, arcs and circles that are not construction geometry, and the part
/// holds no spatial sketch.
pub(in crate::writer) fn sketches(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
) -> Result<Sketches, CodecError> {
    if ir.model.sketches.is_empty() {
        return Ok(Sketches::Native(Vec::new()));
    }
    if let Some(spatial) = ir.model.spatial_sketches.first() {
        return Ok(Sketches::Lane(format!(
            "spatial sketch {} has no native sketch form here",
            spatial.id.as_str()
        )));
    }
    let Some(native) = native else {
        return Ok(Sketches::Lane("the write holds no history summary".into()));
    };
    let mut out = Vec::new();
    for sketch in &ir.model.sketches {
        if sketch
            .configuration
            .as_deref()
            .is_some_and(|key| key != "0")
        {
            return Ok(Sketches::Lane(format!(
                "sketch {} belongs to configuration {}",
                sketch.id.as_str(),
                sketch.configuration.as_deref().unwrap_or_default()
            )));
        }
        let owner = unique_planar_sketch_owner(ir, &sketch.id)?;
        let record = generated_sketch_owner_record(native, owner, sketch.id.as_str())?;
        let feature_id = generated_sketch_owner_id(record, sketch.id.as_str())?;
        let name = if record.name.is_empty() {
            sketch.name.as_deref().unwrap_or(sketch.id.as_str())
        } else {
            record.name.as_str()
        };
        let name = GENERATED_NAME.fit(name).text.to_owned();
        let (plane, points, curves) = match geometry(ir, sketch) {
            Ok(geometry) => geometry,
            Err(reason) => {
                return Ok(Sketches::Lane(format!(
                    "sketch {}: {reason}",
                    sketch.id.as_str()
                )))
            }
        };
        out.push((
            owner.ordinal,
            Sketch {
                name,
                feature_id,
                plane,
                points,
                curves,
                body: generated_profile_stream(ir, sketch)?,
            },
        ));
    }
    out.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(Sketches::Native(
        out.into_iter().map(|(_, sketch)| sketch).collect(),
    ))
}

/// The sketch's plane and its point and curve records, or why it has none.
fn geometry(
    ir: &cadmpeg_ir::CadIr,
    sketch: &IrSketch,
) -> Result<(Plane, Vec<Point>, Vec<Curve>), String> {
    let (origin, normal, u_axis) = sketch
        .resolved_placement()
        .map(|(origin, normal, u_axis)| (origin.get(), unit(normal.get()), unit(u_axis.get())))
        .ok_or("the sketch has no resolved placement")?;
    let v_axis = normal.cross(u_axis);
    let (plane, flipped) = [Plane::Front, Plane::Top, Plane::Right]
        .into_iter()
        .find_map(|plane| {
            let cosine = dot(normal, plane.axes()[2]);
            (1.0 - cosine.abs() <= PLANE_NORMAL_TOLERANCE).then_some((plane, cosine < 0.0))
        })
        .ok_or("the sketch plane is no default plane")?;
    let [plane_u, plane_v, plane_n] = plane.axes();
    if dot_point(origin, plane_n).abs() > PLANE_OFFSET_TOLERANCE_MM {
        return Err("the sketch plane is offset from its default plane".into());
    }
    let to_plane = |point: Point2| -> [f64; 2] {
        let model = Point3::new(
            origin.x + point.u * u_axis.x + point.v * v_axis.x,
            origin.y + point.u * u_axis.y + point.v * v_axis.y,
            origin.z + point.u * u_axis.z + point.v * v_axis.z,
        );
        [
            dot_point(model, plane_u) * METRES_PER_MILLIMETRE,
            dot_point(model, plane_v) * METRES_PER_MILLIMETRE,
        ]
    };
    let mut points = Vec::<Point>::new();
    let mut point = |at: [f64; 2], kind: u32| -> usize {
        if let Some(index) = points.iter().position(|candidate| {
            (candidate.at[0] - at[0]).abs() <= POINT_MERGE_TOLERANCE_M
                && (candidate.at[1] - at[1]).abs() <= POINT_MERGE_TOLERANCE_M
        }) {
            return index;
        }
        points.push(Point { at, kind });
        points.len() - 1
    };
    let mut lines = Vec::new();
    let mut arcs = Vec::new();
    for entity in ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
    {
        if entity.construction {
            return Err(format!(
                "entity {} is construction geometry",
                entity.id().as_str()
            ));
        }
        match entity.geometry.definition() {
            SketchGeometryDefinition::Point { position } => {
                point(to_plane(position.get()), 0);
            }
            SketchGeometryDefinition::Line { start, end } => {
                let start = point(to_plane(start.get()), 1);
                let end = point(to_plane(end.get()), 1);
                lines.push(Curve::Line { start, end });
            }
            SketchGeometryDefinition::Circle { center, radius } => {
                let centre = to_plane(center.get());
                let radius = radius.get() * METRES_PER_MILLIMETRE;
                let on = point([centre[0] + radius, centre[1]], 1);
                let centre = point(centre, 1);
                arcs.push(Curve::Circle { on, centre });
            }
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let center = center.get();
                let radius = radius.get();
                let at = |angle: f64| {
                    to_plane(Point2::new(
                        center.u + radius * angle.cos(),
                        center.v + radius * angle.sin(),
                    ))
                };
                let (first, second) = (at(start_angle.get()), at(end_angle.get()));
                let (first, second) = if flipped {
                    (second, first)
                } else {
                    (first, second)
                };
                let start = point(first, 2);
                let end = point(second, 2);
                let centre = point(to_plane(center), 2);
                arcs.push(Curve::Arc { start, end, centre });
            }
            _ => {
                return Err(format!(
                    "entity {} has no native curve record here",
                    entity.id().as_str()
                ))
            }
        }
    }
    lines.extend(arcs);
    Ok((plane, points, lines))
}

fn unit(vector: Vector3) -> Vector3 {
    let length = vector.norm();
    Vector3::new(vector.x / length, vector.y / length, vector.z / length)
}

fn dot(vector: Vector3, axis: [f64; 3]) -> f64 {
    vector.x * axis[0] + vector.y * axis[1] + vector.z * axis[2]
}

fn dot_point(point: Point3, axis: [f64; 3]) -> f64 {
    point.x * axis[0] + point.y * axis[1] + point.z * axis[2]
}
