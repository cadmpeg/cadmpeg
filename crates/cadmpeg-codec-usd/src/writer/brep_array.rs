// SPDX-License-Identifier: Apache-2.0
//! Writes a body's B-rep as a `BrepArray` prim (spec §17).

use cadmpeg_ir::topology::Body;

use super::text::{quoted, real, tuple};
use super::{records, Placement, Writer, MILLIMETRES_PER_UNIT};
use crate::brep::solid::exact::{Frames, PackedExact, PcurveFrames, Source};
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
        let exported = records::export(self.ir, body, &mut self.tessellator, false)?;
        let name = body.name.clone().unwrap_or_else(|| body.id.to_string());
        if exported.unwritten.faces > 0 || exported.unwritten.edges > 0 {
            self.unwritten.breps.push(format!(
                "{name}: {} faces and {} edges have no carrier record",
                exported.unwritten.faces, exported.unwritten.edges
            ));
            return None;
        }
        let records = exported.brep.divided(MILLIMETRES_PER_UNIT);
        self.exported
            .insert(body.id.clone(), (records.clone(), exported.faces.clone()));
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
        // A traced pcurve is the form of no pcurve of the IR.
        let mut exact = records;
        for oriented in &exported.traced {
            if let Some(record) = exact.oriented_edges.get_mut(*oriented) {
                record.pcurve = None;
            }
        }
        let mut packed = Packed::default();
        if let Err(reason) = packed.push(&converted.brep, converted.trimmed, Some(&exact)) {
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
        if packed.identities.any() {
            schemas.push(crate::brep::solid::IDENTITY_API);
        }
        if packed.tolerances.any() {
            schemas.push(crate::brep::solid::TOLERANCE_API);
        }
        if packed.exact.any() {
            schemas.push(crate::brep::solid::exact::ANALYTIC_API);
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
        self.brep_paths.insert(body.id.clone(), self.out.path());
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
        if packed.identities.any() {
            let ids = &packed.identities;
            let strings =
                |values: &[String]| values.iter().map(|value| quoted(value)).collect::<Vec<_>>();
            let out = &mut self.out;
            out.array("uniform string[] cad:brep:id", &strings(&ids.breps), "");
            out.array("uniform string[] cad:face:id", &strings(&ids.faces), "");
            out.array("uniform string[] cad:edge:id", &strings(&ids.edges), "");
            out.array(
                "uniform string[] cad:vertex:id",
                &strings(&ids.vertices),
                "",
            );
            for (name, values) in [
                ("cad:brep:name", &ids.brep_names),
                ("cad:face:name", &ids.face_names),
            ] {
                if values.iter().any(|value| !value.is_empty()) {
                    out.array(&format!("uniform string[] {name}"), &strings(values), "");
                }
            }
        }
        if packed.tolerances.any() {
            let tolerances = &packed.tolerances;
            let out = &mut self.out;
            out.array(
                "uniform double[] cad:face:tolerance",
                &reals(&tolerances.faces),
                "",
            );
            out.array(
                "uniform double[] cad:edge:tolerance",
                &reals(&tolerances.edges),
                "",
            );
            out.array(
                "uniform double[] cad:vertex:tolerance",
                &reals(&tolerances.vertices),
                "",
            );
        }
        if packed.exact.any() {
            exact_arrays(&mut self.out, &packed.exact);
        }
        self.face_subsets(&brep.faces);
        self.out.close();
    }
}

/// Writes the arrays of `CadBrepAnalyticAPI` (spec §18.5).
fn exact_arrays(out: &mut super::Emitter, exact: &PackedExact) {
    let words =
        |values: &mut dyn Iterator<Item = &str>| -> Vec<String> { values.map(quoted).collect() };
    let forms = |values: &[crate::brep::Form]| -> Vec<String> {
        values.iter().map(|form| quoted(form.token())).collect()
    };
    if !exact.surfaces.kinds.is_empty() {
        frames(out, "cad:surface:analytic", &exact.surfaces);
        sources(out, "cad:surface:analytic:profile", &exact.surface_profiles);
    }
    if !exact.curves.kinds.is_empty() {
        frames(out, "cad:curve3d:analytic", &exact.curves);
    }
    if !exact.nurbs_curve_forms.is_empty() {
        curves(out, "cad:curve3d:nurb", &exact.nurbs_curves, "point3d");
        out.array(
            "uniform token[] cad:curve3d:nurb:form",
            &forms(&exact.nurbs_curve_forms),
            "",
        );
    }
    if !exact.pcurves.kinds.is_empty() {
        analytic_pcurves(out, &exact.pcurves);
    }
    if !exact.nurbs_pcurve_forms.is_empty() {
        curves(out, "cad:curveUv:nurb", &exact.nurbs_pcurves, "double2");
        out.array(
            "uniform token[] cad:curveUv:nurb:form",
            &forms(&exact.nurbs_pcurve_forms),
            "",
        );
    }
    sources(out, "cad:face:surface", &exact.face_surfaces);
    sources(out, "cad:edge:curve", &exact.edge_curves);
    out.array(
        "uniform double2[] cad:edge:range",
        &tuples(&exact.edge_ranges),
        "",
    );
    out.array(
        "uniform token[] cad:edge:sense",
        &words(
            &mut exact
                .edge_same
                .iter()
                .map(|same| if *same { "same" } else { "opposite" }),
        ),
        "",
    );
    out.array(
        "uniform token[] cad:loop:direction",
        &words(
            &mut exact
                .loop_directions
                .iter()
                .map(|direction| direction.token()),
        ),
        "",
    );
    sources(out, "cad:edgeuse:pcurve", &exact.edgeuse_pcurves);
}

/// Writes the `{name}Type` and `{name}Index` arrays of `values`.
fn sources(out: &mut super::Emitter, name: &str, values: &[Source]) {
    out.array(
        &format!("uniform token[] {name}Type"),
        &values
            .iter()
            .map(|source| quoted(source.token()))
            .collect::<Vec<_>>(),
        "",
    );
    out.array(
        &format!("uniform int[] {name}Index"),
        &values
            .iter()
            .map(|source| source.index().to_string())
            .collect::<Vec<_>>(),
        "",
    );
}

/// Writes the frame-positioned analytic records `frames` under `prefix`.
fn frames(out: &mut super::Emitter, prefix: &str, frames: &Frames) {
    out.array(
        &format!("uniform token[] {prefix}:type"),
        &frames
            .kinds
            .iter()
            .map(|kind| quoted(kind))
            .collect::<Vec<_>>(),
        "",
    );
    out.array(
        &format!("uniform point3d[] {prefix}:position"),
        &tuples(&frames.position),
        "",
    );
    out.array(
        &format!("uniform vector3d[] {prefix}:axis"),
        &tuples(&frames.axis),
        "",
    );
    out.array(
        &format!("uniform vector3d[] {prefix}:refDirection"),
        &tuples(&frames.ref_direction),
        "",
    );
    out.array(
        &format!("uniform int[] {prefix}:paramCounts"),
        &counts(&frames.param_counts),
        "",
    );
    out.array(
        &format!("uniform double[] {prefix}:params"),
        &reals(&frames.params),
        "",
    );
}

/// Writes the analytic pcurve catalog of `CadBrepAnalyticAPI`.
fn analytic_pcurves(out: &mut super::Emitter, pcurves: &PcurveFrames) {
    out.array(
        "uniform token[] cad:curveUv:analytic:type",
        &pcurves
            .kinds
            .iter()
            .map(|kind| quoted(kind))
            .collect::<Vec<_>>(),
        "",
    );
    out.array(
        "uniform double2[] cad:curveUv:analytic:position",
        &tuples(&pcurves.position),
        "",
    );
    out.array(
        "uniform double2[] cad:curveUv:analytic:refDirection",
        &tuples(&pcurves.ref_direction),
        "",
    );
    out.array(
        "uniform int[] cad:curveUv:analytic:paramCounts",
        &counts(&pcurves.param_counts),
        "",
    );
    out.array(
        "uniform double[] cad:curveUv:analytic:params",
        &reals(&pcurves.params),
        "",
    );
    out.array(
        "uniform double2[] cad:curveUv:analytic:range",
        &tuples(&pcurves.range),
        "",
    );
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
