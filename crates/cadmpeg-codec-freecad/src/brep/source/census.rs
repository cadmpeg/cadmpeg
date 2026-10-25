// SPDX-License-Identifier: Apache-2.0
//! Independent bounded source getters for the native family census.
use super::*;

#[derive(Clone, Copy)]
pub(in crate::brep) enum Lane {
    Curve2d,
    Curve,
    Surface,
    Shape,
    Polygon3d,
    IndexedPolygon,
    Triangulation,
}

#[derive(Default, Clone)]
pub(super) struct Delta {
    curves_2d: BTreeMap<String, u64>,
    curves_3d: BTreeMap<String, u64>,
    surfaces: BTreeMap<String, u64>,
    topology: BTreeMap<String, u64>,
    polygons_3d: u64,
    polygons_on_triangulations: u64,
    triangulations: u64,
}
impl Delta {
    pub(super) fn read(marks: &[(View<'_>, Lane)], binary: bool) -> Result<Self, CodecError> {
        let mut delta = Self::default();
        for (view, lane) in marks {
            let mut source = *view;
            let text;
            let value = if binary {
                if matches!(
                    lane,
                    Lane::Polygon3d | Lane::IndexedPolygon | Lane::Triangulation
                ) {
                    i64::from(source.req_i32_le()?)
                } else {
                    i64::from(source.req_u8()?)
                }
            } else {
                text = std::str::from_utf8(source.req_take(view.window().len())?)
                    .map_err(|_| CodecError::malformed("census discriminator is not UTF-8"))?;
                if matches!(lane, Lane::Shape) {
                    let kind = parse_shape_kind(text)?;
                    let name = match kind {
                        TextShapeKind::Compound => "compound",
                        TextShapeKind::CompSolid => "compsolid",
                        TextShapeKind::Solid => "solid",
                        TextShapeKind::Shell => "shell",
                        TextShapeKind::Face => "face",
                        TextShapeKind::Wire => "wire",
                        TextShapeKind::Edge => "edge",
                        TextShapeKind::Vertex => "vertex",
                    };
                    increment(&mut delta.topology, name);
                    continue;
                }
                text.parse::<i64>()
                    .map_err(|_| CodecError::malformed("invalid census family discriminator"))?
            };
            let curve_family = |value| match value {
                1 => Some("line"),
                2 => Some("circle"),
                3 => Some("ellipse"),
                4 => Some("parabola"),
                5 => Some("hyperbola"),
                6 | 7 => Some("nurbs"),
                8 => Some("trimmed"),
                9 => Some("offset"),
                _ => None,
            };
            match lane {
                Lane::Curve2d | Lane::Curve => {
                    let family = curve_family(value)
                        .ok_or_else(|| CodecError::malformed("invalid census curve family"))?;
                    increment(
                        if matches!(lane, Lane::Curve2d) {
                            &mut delta.curves_2d
                        } else {
                            &mut delta.curves_3d
                        },
                        family,
                    );
                }
                Lane::Surface => {
                    let family = match value {
                        1 => "plane",
                        2 => "cylinder",
                        3 => "cone",
                        4 => "sphere",
                        5 => "torus",
                        6 => "extrusion",
                        7 => "revolution",
                        8 | 9 => "nurbs",
                        10 => "trimmed",
                        11 => "offset",
                        _ => return Err(CodecError::malformed("invalid census surface family")),
                    };
                    increment(&mut delta.surfaces, family);
                }
                Lane::Shape => {
                    let family = match value {
                        0 => "compound",
                        1 => "compsolid",
                        2 => "solid",
                        3 => "shell",
                        4 => "face",
                        5 => "wire",
                        6 => "edge",
                        7 => "vertex",
                        _ => return Err(CodecError::malformed("invalid census shape family")),
                    };
                    increment(&mut delta.topology, family);
                }
                Lane::Polygon3d | Lane::IndexedPolygon | Lane::Triangulation => {
                    if !(0..=1_000_000).contains(&value) {
                        return Err(CodecError::malformed("invalid census node count"));
                    }
                    // This source count admits one actual carrier in its census.
                    match lane {
                        Lane::Polygon3d => delta.polygons_3d += 1,
                        Lane::IndexedPolygon => delta.polygons_on_triangulations += 1,
                        _ => delta.triangulations += 1,
                    }
                }
            }
        }
        Ok(delta)
    }
    pub(super) fn apply(&mut self, other: Self) {
        fn extend(target: &mut BTreeMap<String, u64>, source: BTreeMap<String, u64>) {
            for (key, count) in source {
                *target.entry(key).or_default() += count;
            }
        }
        extend(&mut self.curves_2d, other.curves_2d);
        extend(&mut self.curves_3d, other.curves_3d);
        extend(&mut self.surfaces, other.surfaces);
        extend(&mut self.topology, other.topology);
        self.polygons_3d += other.polygons_3d;
        self.polygons_on_triangulations += other.polygons_on_triangulations;
        self.triangulations += other.triangulations;
    }
    pub(super) fn finish(
        self,
        id: &str,
        binary: bool,
        version: u8,
    ) -> crate::native::CarrierCensusRecord {
        crate::native::CarrierCensusRecord {
            id: census_id(id),
            payload: id.to_owned(),
            form: if binary {
                crate::native::CarrierCensusForm::Binary
            } else {
                crate::native::CarrierCensusForm::Text
            },
            topology_version: version,
            curves_2d: self.curves_2d,
            curves_3d: self.curves_3d,
            surfaces: self.surfaces,
            topology: self.topology,
            polygons_3d: self.polygons_3d,
            polygons_on_triangulations: self.polygons_on_triangulations,
            triangulations: self.triangulations,
        }
    }
}

/// Empty geometry-free tables have no census fields and open no fake producer.
pub(super) fn capture(
    ctx: &DecodeContext<'_>,
    owner: &cadmpeg_core::decode::RecordOwner<'_>,
    extent: View<'_>,
    marks: &[(View<'_>, Lane)],
    binary: bool,
) -> Result<Delta, CodecError> {
    if marks.is_empty() {
        return Ok(Delta::default());
    }
    ctx.record_fragment(owner, extent, || Delta::read(marks, binary))
}
