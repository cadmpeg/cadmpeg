// SPDX-License-Identifier: Apache-2.0
//! Writes a body's B-rep as a `BrepArray` prim (spec §17).

use cadmpeg_ir::topology::Body;

use super::text::{quoted, real, tuple};
use super::{records, Placement, Writer, MILLIMETRES_PER_UNIT};
use crate::brep::solid::{
    Curves, Packed, EDGE_CURVE_API, SURFACE_API, TRIM_CURVE_API, VERTEX_POINT_API,
};

/// The arrays of a `BrepArray` prim and the IR face of each of its faces.
pub(super) struct BrepPrim {
    pub(super) packed: Packed,
    pub(super) faces: Vec<cadmpeg_ir::ids::FaceId>,
}

/// The `BrepArray` prim name of a body written with fallback prims.
pub(super) const PRIM_NAME: &str = "Brep";

/// `value` with a negative zero written as zero.
fn unsigned_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn reals(values: &[f64]) -> Vec<String> {
    values
        .iter()
        .map(|value| real(unsigned_zero(*value)))
        .collect()
}

fn tuples<const N: usize>(values: &[[f64; N]]) -> Vec<String> {
    values
        .iter()
        .map(|value| tuple(&value.map(unsigned_zero)))
        .collect()
}

fn counts(values: &[u32]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

fn tokens(values: impl Iterator<Item = &'static str>) -> Vec<String> {
    values.map(quoted).collect()
}

/// The binary32 value nearest `value` that does not lie on the inner side
/// of it: below for a lower bound, above for an upper bound.
fn outward(value: f64, upper: bool) -> f32 {
    let rounded = crate::text::value::round_f32(value);
    let inside = if upper {
        f64::from(rounded) < value
    } else {
        f64::from(rounded) > value
    };
    if !inside {
        return if rounded == 0.0 { 0.0 } else { rounded };
    }
    let bits = rounded.to_bits();
    let away = (rounded >= 0.0) == upper;
    let step = if away { bits + 1 } else { bits - 1 };
    if rounded == 0.0 {
        return if upper {
            f32::from_bits(1)
        } else {
            -f32::from_bits(1)
        };
    }
    f32::from_bits(step)
}

impl Writer<'_> {
    /// The `BrepArray` arrays of `body`, in metres, or `None` when a face or
    /// edge of the body has no NURBS form.
    pub(super) fn brep_packed(&mut self, body: &Body) -> Option<Box<BrepPrim>> {
        let exported = records::export(self.ir, &self.index, body, &mut self.tessellator, false)?;
        let name = body.name.clone().unwrap_or_else(|| body.id.to_string());
        if exported.unwritten.faces > 0 || exported.unwritten.edges > 0 {
            self.unwritten.breps.push(format!(
                "{name}: {} faces and {} edges have no carrier record",
                exported.unwritten.faces, exported.unwritten.edges
            ));
            return None;
        }
        let records = exported.brep.divided(MILLIMETRES_PER_UNIT);
        let converted = match crate::brep::convert::nurbs_only(&records) {
            Ok(converted) => converted,
            Err(reason) => {
                self.unwritten.breps.push(format!("{name}: {reason}"));
                return None;
            }
        };
        if !converted.trimmed {
            self.unwritten.untrimmed_breps.push(name.clone());
        }
        let mut packed = Packed::default();
        if let Err(reason) = packed.push(&converted.brep, converted.trimmed) {
            self.unwritten.breps.push(format!("{name}: {reason}"));
            return None;
        }
        Some(Box::new(BrepPrim {
            packed,
            faces: exported.faces,
        }))
    }

    /// Writes `packed` as a `BrepArray` prim named `name`.
    pub(super) fn brep_array_prim(
        &mut self,
        brep: &BrepPrim,
        name: &str,
        body: &Body,
        placement: &Placement<'_>,
    ) {
        let packed = &brep.packed;
        let mut schemas = vec![VERTEX_POINT_API, EDGE_CURVE_API, SURFACE_API];
        if packed.trim_curves.is_some() {
            schemas.push(TRIM_CURVE_API);
        }
        let mut placement = placement.clone();
        placement.metadata.push(format!(
            "prepend apiSchemas = [{}]",
            schemas
                .iter()
                .map(|schema| quoted(schema))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let binding = self.binding_path(&body.id.to_string());
        self.geometry_head(
            crate::brep::solid::PRIM_TYPE,
            name,
            &placement,
            binding.as_deref(),
        );
        if let Some([min, max]) = packed.bounds() {
            let low = min.map(|value| outward(value, false));
            let high = max.map(|value| outward(value, true));
            self.extent(Some((low, high)));
        }
        self.color_primvars(Some(body));
        let out = &mut self.out;
        out.array(
            "uniform double[] brep:intersectTol3d",
            &reals(&packed.intersect_tol),
            "",
        );
        out.array("uniform double3[] brep:extent", &tuples(&packed.extent), "");
        out.array(
            "uniform uint[] brep:regionCount",
            &counts(&packed.region_count),
            "",
        );
        out.array(
            "uniform uint[] region:shellCount",
            &counts(&packed.region_shell_count),
            "",
        );
        out.array(
            "uniform token[] region:type",
            &tokens(packed.region_solid.iter().map(|solid| {
                if *solid {
                    "solidRegion"
                } else {
                    "voidRegion"
                }
            })),
            "",
        );
        out.array(
            "uniform uint[] shell:faceuseCount",
            &counts(&packed.shell_faceuse_count),
            "",
        );
        out.array(
            "uniform uint[] shell:wireEdgeCount",
            &counts(&packed.shell_wire_edge_count),
            "",
        );
        out.array(
            "uniform token[] shell:pointType",
            &tokens(
                packed
                    .shell_point
                    .iter()
                    .map(|point| if *point { "BrepPointAPI" } else { "none" }),
            ),
            "",
        );
        out.array(
            "uniform uint[] faceuse:faceIndex",
            &counts(&packed.faceuse_face),
            "",
        );
        out.array(
            "uniform token[] faceuse:orientationType",
            &tokens(
                packed
                    .faceuse_same
                    .iter()
                    .map(|same| if *same { "same" } else { "opposite" }),
            ),
            "",
        );
        out.array(
            "uniform uint[] face:loopCount",
            &counts(&packed.face_loop_count),
            "",
        );
        out.array(
            "uniform token[] face:surfaceType",
            &tokens(packed.face_loop_count.iter().map(|_| SURFACE_API)),
            "",
        );
        out.array(
            "uniform token[] face:trimType",
            &tokens(packed.face_loop_count.iter().map(|_| "general")),
            "",
        );
        out.array(
            "uniform double2[] face:range",
            &tuples(&packed.face_range),
            "",
        );
        out.array(
            "uniform uint[] loop:edgeuseCount",
            &counts(&packed.loop_edgeuse_count),
            "",
        );
        out.array(
            "uniform uint[] loop:vertexIndex",
            &counts(&packed.loop_vertex),
            "",
        );
        out.array(
            "uniform uint[] edgeuse:edgeIndex",
            &counts(&packed.edgeuse_edge),
            "",
        );
        out.array(
            "uniform token[] edgeuse:orientationType",
            &tokens(
                packed
                    .edgeuse_same
                    .iter()
                    .map(|same| if *same { "same" } else { "opposite" }),
            ),
            "",
        );
        out.array(
            "uniform uint[] edgeuse:nextRadialEUIndex",
            &counts(&packed.edgeuse_next_radial),
            "",
        );
        out.array(
            "uniform token[] edgeuse:thisRadialEntryType",
            &tokens(packed.edgeuse_top_entry.iter().map(|top| {
                if *top {
                    "topEntry"
                } else {
                    "bottomEntry"
                }
            })),
            "",
        );
        out.array(
            "uniform token[] edge:curveType",
            &tokens(packed.edge_vertices.iter().map(|_| "BrepCurve3dNurbAPI")),
            "",
        );
        out.array(
            "uniform double[] edge:range",
            &reals(&packed.edge_range),
            "",
        );
        out.array(
            "uniform int2[] edge:vertexIndices",
            &packed
                .edge_vertices
                .iter()
                .map(|[start, end]| format!("({start}, {end})"))
                .collect::<Vec<_>>(),
            "",
        );
        out.array("uniform token[] wireEdge:curveType", &[], "");
        out.array("uniform double[] wireEdge:range", &[], "");
        out.array("uniform int2[] wireEdge:vertexIndices", &[], "");
        out.array(
            "uniform token[] vertex:pointType",
            &tokens(packed.vertices.iter().map(|_| "BrepPointAPI")),
            "",
        );
        out.array(
            "uniform point3d[] brep:vertexPoint:point:position",
            &tuples(&packed.vertices),
            "",
        );
        curves(
            out,
            "brep:edge3dNurb:curve3d:nurb",
            &packed.edge_curves,
            "point3d",
        );
        let surfaces = &packed.surfaces;
        let prefix = "brep:surface:nurb";
        out.array(
            &format!("uniform point3d[] {prefix}:controlVertices"),
            &tuples(&surfaces.control_vertices),
            "",
        );
        out.array(
            &format!("uniform uint[] {prefix}:uVertexCount"),
            &counts(&surfaces.u_vertex_count),
            "",
        );
        out.array(
            &format!("uniform uint[] {prefix}:vVertexCount"),
            &counts(&surfaces.v_vertex_count),
            "",
        );
        out.array(
            &format!("uniform uint[] {prefix}:uOrder"),
            &counts(&surfaces.u_order),
            "",
        );
        out.array(
            &format!("uniform uint[] {prefix}:vOrder"),
            &counts(&surfaces.v_order),
            "",
        );
        out.array(
            &format!("uniform double[] {prefix}:uKnots"),
            &reals(&surfaces.u_knots),
            "",
        );
        out.array(
            &format!("uniform double[] {prefix}:vKnots"),
            &reals(&surfaces.v_knots),
            "",
        );
        out.array(
            &format!("uniform double[] {prefix}:weights"),
            &reals(&surfaces.weights),
            "",
        );
        if let Some(trims) = &packed.trim_curves {
            curves(out, "brep:curveUv:nurb", trims, "double2");
        }
        self.face_subsets(&brep.faces);
        self.out.close();
    }
}

impl Writer<'_> {
    /// Writes a `GeomSubset` of the faces of each material that an
    /// appearance binding binds to faces (spec §17.4).
    fn face_subsets(&mut self, faces: &[cadmpeg_ir::ids::FaceId]) {
        let mut groups: std::collections::BTreeMap<String, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (index, face) in faces.iter().enumerate() {
            if let Some(path) = self.binding_path(&face.to_string()) {
                groups.entry(path).or_default().push(index);
            }
        }
        let mut names = super::Names::default();
        for (path, indices) in groups {
            let name = names.unique(path.rsplit('/').next().unwrap_or("Faces"));
            self.prim_head(
                "def GeomSubset",
                &name,
                &["prepend apiSchemas = [\"MaterialBindingAPI\"]".to_owned()],
            );
            self.out.open();
            self.out.line("uniform token elementType = \"face\"");
            self.out.array(
                "int[] indices",
                &indices.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "",
            );
            self.out.line(&format!("rel material:binding = <{path}>"));
            self.out.close();
        }
    }
}

/// Writes packed NURBS curves under `prefix`, with control vertices of
/// `point_type`.
fn curves<const N: usize>(
    out: &mut super::Emitter,
    prefix: &str,
    curves: &Curves<N>,
    point_type: &str,
) {
    out.array(
        &format!("uniform {point_type}[] {prefix}:controlVertices"),
        &tuples(&curves.control_vertices),
        "",
    );
    out.array(
        &format!("uniform uint[] {prefix}:vertexCount"),
        &counts(&curves.vertex_count),
        "",
    );
    out.array(
        &format!("uniform uint[] {prefix}:order"),
        &counts(&curves.order),
        "",
    );
    out.array(
        &format!("uniform double[] {prefix}:knots"),
        &reals(&curves.knots),
        "",
    );
    out.array(
        &format!("uniform double[] {prefix}:weights"),
        &reals(&curves.weights),
        "",
    );
}

#[cfg(test)]
mod tests;
