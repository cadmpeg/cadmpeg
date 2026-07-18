// SPDX-License-Identifier: Apache-2.0
//! STEP writer carrier dispatch table.
//!
//! Every IR geometry carrier the writer can emit is described once, here, as a
//! [`CarrierSpec`]: the IR enum variant, the STEP entity keyword it maps to, the
//! emission form (simple, interned, or the AND-combined complex instance a
//! rational carrier requires), and the ordered mandatory parameter fields.
//!
//! The table is the precedence record: for rational NURBS the STEP realization
//! is a complex instance combining several supertypes, and [`CarrierSpec::form`]
//! carries that supertype list in emission order. Non-rational carriers map to a
//! single most-specific subtype.

/// How the writer realizes a carrier in the DATA section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// One `#id = KEYWORD(params);` instance.
    Simple,
    /// A rational carrier realized as an AND-combined complex instance. The
    /// slice lists the partial supertypes in emission order; the tally keyword
    /// (`KEYWORD`) is the `*_WITH_KNOTS` member of that list.
    Complex(&'static [&'static str]),
}

/// One IR carrier's STEP realization.
#[derive(Debug, Clone, Copy)]
pub struct CarrierSpec {
    /// `surface` or `curve`: which dispatch owns this variant.
    pub kind: &'static str,
    /// IR enum variant name (`SurfaceGeometry`/`CurveGeometry`).
    pub ir_variant: &'static str,
    /// STEP entity keyword, also the writer's entity-count tally key.
    pub keyword: &'static str,
    /// Emission form.
    pub form: Form,
    /// Ordered mandatory parameter fields, excluding the leading `''` label.
    pub fields: &'static [&'static str],
}

/// Surface carrier realizations, in `SurfaceGeometry` declaration order.
pub const SURFACES: &[CarrierSpec] = &[
    CarrierSpec {
        kind: "surface",
        ir_variant: "Plane",
        keyword: "PLANE",
        form: Form::Simple,
        fields: &["position"],
    },
    CarrierSpec {
        kind: "surface",
        ir_variant: "Cylinder",
        keyword: "CYLINDRICAL_SURFACE",
        form: Form::Simple,
        fields: &["position", "radius"],
    },
    CarrierSpec {
        kind: "surface",
        ir_variant: "Cone",
        keyword: "CONICAL_SURFACE",
        form: Form::Simple,
        fields: &["position", "radius", "semi_angle"],
    },
    CarrierSpec {
        kind: "surface",
        ir_variant: "Sphere",
        keyword: "SPHERICAL_SURFACE",
        form: Form::Simple,
        fields: &["position", "radius"],
    },
    CarrierSpec {
        kind: "surface",
        ir_variant: "Torus",
        keyword: "TOROIDAL_SURFACE",
        form: Form::Simple,
        fields: &["position", "major_radius", "minor_radius"],
    },
    CarrierSpec {
        kind: "surface",
        ir_variant: "Nurbs (non-rational)",
        keyword: "B_SPLINE_SURFACE_WITH_KNOTS",
        form: Form::Simple,
        fields: &[
            "u_degree",
            "v_degree",
            "control_points_list",
            "surface_form",
            "u_closed",
            "v_closed",
            "self_intersect",
            "u_multiplicities",
            "v_multiplicities",
            "u_knots",
            "v_knots",
            "knot_spec",
        ],
    },
    CarrierSpec {
        kind: "surface",
        ir_variant: "Nurbs (rational)",
        keyword: "B_SPLINE_SURFACE_WITH_KNOTS",
        form: Form::Complex(&[
            "BOUNDED_SURFACE",
            "B_SPLINE_SURFACE",
            "B_SPLINE_SURFACE_WITH_KNOTS",
            "GEOMETRIC_REPRESENTATION_ITEM",
            "RATIONAL_B_SPLINE_SURFACE",
            "REPRESENTATION_ITEM",
            "SURFACE",
        ]),
        fields: &["weights"],
    },
];

/// Curve carrier realizations, in `CurveGeometry` declaration order.
pub const CURVES: &[CarrierSpec] = &[
    CarrierSpec {
        kind: "curve",
        ir_variant: "Line",
        keyword: "LINE",
        form: Form::Simple,
        fields: &["pnt", "dir"],
    },
    CarrierSpec {
        kind: "curve",
        ir_variant: "Circle",
        keyword: "CIRCLE",
        form: Form::Simple,
        fields: &["position", "radius"],
    },
    CarrierSpec {
        kind: "curve",
        ir_variant: "Ellipse",
        keyword: "ELLIPSE",
        form: Form::Simple,
        fields: &["position", "semi_axis_1", "semi_axis_2"],
    },
    CarrierSpec {
        kind: "curve",
        ir_variant: "Parabola",
        keyword: "PARABOLA",
        form: Form::Simple,
        fields: &["position", "focal_dist"],
    },
    CarrierSpec {
        kind: "curve",
        ir_variant: "Hyperbola",
        keyword: "HYPERBOLA",
        form: Form::Simple,
        fields: &["position", "semi_axis", "imaginary_semi_axis"],
    },
    CarrierSpec {
        kind: "curve",
        ir_variant: "Degenerate",
        keyword: "POLYLINE",
        form: Form::Simple,
        fields: &["points"],
    },
    CarrierSpec {
        kind: "curve",
        ir_variant: "Nurbs (non-rational)",
        keyword: "B_SPLINE_CURVE_WITH_KNOTS",
        form: Form::Simple,
        fields: &[
            "degree",
            "control_points_list",
            "curve_form",
            "closed_curve",
            "self_intersect",
            "knot_multiplicities",
            "knots",
            "knot_spec",
        ],
    },
    CarrierSpec {
        kind: "curve",
        ir_variant: "Nurbs (rational)",
        keyword: "B_SPLINE_CURVE_WITH_KNOTS",
        form: Form::Complex(&[
            "BOUNDED_CURVE",
            "B_SPLINE_CURVE",
            "B_SPLINE_CURVE_WITH_KNOTS",
            "CURVE",
            "GEOMETRIC_REPRESENTATION_ITEM",
            "RATIONAL_B_SPLINE_CURVE",
            "REPRESENTATION_ITEM",
        ]),
        fields: &["weights"],
    },
];

/// Supporting entities the carrier dispatch emits for geometry it references.
///
/// These are not carrier subtypes; they are the shared placement and coordinate
/// vocabulary every carrier builds on. Listed so the reference doc and fuzz
/// dictionary cover the writer's full keyword surface.
pub const SUPPORT: &[CarrierSpec] = &[
    CarrierSpec {
        kind: "support",
        ir_variant: "Point3",
        keyword: "CARTESIAN_POINT",
        form: Form::Simple,
        fields: &["coordinates"],
    },
    CarrierSpec {
        kind: "support",
        ir_variant: "Vector3 (unit)",
        keyword: "DIRECTION",
        form: Form::Simple,
        fields: &["direction_ratios"],
    },
    CarrierSpec {
        kind: "support",
        ir_variant: "Vector3 (with magnitude)",
        keyword: "VECTOR",
        form: Form::Simple,
        fields: &["orientation", "magnitude"],
    },
    CarrierSpec {
        kind: "support",
        ir_variant: "Placement",
        keyword: "AXIS2_PLACEMENT_3D",
        form: Form::Simple,
        fields: &["location", "axis", "ref_direction"],
    },
];

/// Every carrier and supporting spec in a stable order.
pub fn all() -> impl Iterator<Item = &'static CarrierSpec> {
    SURFACES.iter().chain(CURVES).chain(SUPPORT)
}

/// Render the writer reference doc: a deterministic Markdown table generated
/// from [`all`]. The `docs/formats/step_writer_entities.md` file is this output
/// verbatim; the checked test regenerates and diffs it.
pub fn render_reference() -> String {
    let mut out = String::new();
    out.push_str("<!-- Generated from crates/cadmpeg-step/src/entity_table.rs. ");
    out.push_str("Do not edit by hand; run `cargo test -p cadmpeg-step`. -->\n\n");
    out.push_str("# STEP writer carrier dispatch\n\n");
    out.push_str(
        "The writer maps each IR geometry carrier to its most-specific STEP \
entity subtype. Rational NURBS carriers realize as AND-combined complex \
instances; the supertype list records the emission order.\n\n",
    );

    render_section(&mut out, "Surfaces", SURFACES);
    render_section(&mut out, "Curves", CURVES);
    render_section(&mut out, "Supporting entities", SUPPORT);
    out
}

fn render_section(out: &mut String, title: &str, specs: &[CarrierSpec]) {
    use std::fmt::Write as _;

    let _ = writeln!(out, "## {title}\n");
    out.push_str("| IR variant | STEP entity | Form | Mandatory fields |\n");
    out.push_str("|---|---|---|---|\n");
    for spec in specs {
        let form = match spec.form {
            Form::Simple => "simple".to_string(),
            Form::Complex(supertypes) => format!("complex: {}", supertypes.join(" ")),
        };
        let _ = writeln!(
            out,
            "| {} | `{}` | {} | {} |",
            spec.ir_variant,
            spec.keyword,
            form,
            spec.fields.join(", "),
        );
    }
    out.push('\n');
}

/// Render the `step_writer` fuzz dictionary: one quoted token per distinct STEP
/// keyword the writer can emit, sorted for a stable diff. libFuzzer dictionary
/// syntax is `"token"` per line with a leading comment block.
pub fn render_dictionary() -> String {
    use std::fmt::Write as _;

    let mut keywords: Vec<&'static str> = Vec::new();
    for spec in all() {
        keywords.push(spec.keyword);
        if let Form::Complex(supertypes) = spec.form {
            keywords.extend_from_slice(supertypes);
        }
    }
    keywords.sort_unstable();
    keywords.dedup();

    let mut out = String::new();
    out.push_str("# Generated from crates/cadmpeg-step/src/entity_table.rs.\n");
    out.push_str("# Do not edit by hand; run `cargo test -p cadmpeg-step`.\n");
    out.push_str("# STEP entity keywords the writer emits, for structure-aware fuzzing.\n");
    for kw in keywords {
        let _ = writeln!(out, "\"{kw}\"");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{curve, surface};
    use crate::writer::Emitter;
    use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
    }

    fn check_generated(relative: &str, expected: &str) {
        let path = workspace_root().join(relative);
        if std::env::var_os("CADMPEG_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().expect("artifact has a parent"))
                .expect("create artifact directory");
            std::fs::write(&path, expected).expect("write blessed artifact");
            return;
        }
        let actual = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}; run with CADMPEG_BLESS=1", path.display()));
        assert_eq!(
            actual,
            expected,
            "{} is stale; run `CADMPEG_BLESS=1 cargo test -p cadmpeg-step`",
            path.display()
        );
    }

    #[test]
    fn reference_doc_matches_table() {
        check_generated("docs/formats/step_writer_entities.md", &render_reference());
    }

    #[test]
    fn fuzz_dictionary_matches_table() {
        check_generated(
            "crates/cadmpeg-fuzz/dictionaries/step_writer.dict",
            &render_dictionary(),
        );
    }

    fn p() -> Point3 {
        Point3::new(0.0, 0.0, 0.0)
    }

    fn v() -> Vector3 {
        Vector3::new(0.0, 0.0, 1.0)
    }

    fn nurbs_curve(rational: bool) -> NurbsCurve {
        NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![p(), p()],
            weights: rational.then(|| vec![1.0, 1.0]),
            periodic: false,
        }
    }

    fn nurbs_surface(rational: bool) -> NurbsSurface {
        NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![p(), p(), p(), p()],
            weights: rational.then(|| vec![1.0, 1.0, 1.0, 1.0]),
            u_periodic: false,
            v_periodic: false,
        }
    }

    fn emits_keyword(build: impl FnOnce(&mut Emitter)) -> Vec<String> {
        let mut e = Emitter::new();
        build(&mut e);
        e.counts().into_keys().collect()
    }

    #[test]
    fn every_carrier_keyword_is_emitted() {
        let cases: Vec<(&str, Vec<String>)> = vec![
            (
                "Plane",
                emits_keyword(|e| {
                    surface(
                        e,
                        &SurfaceGeometry::Plane {
                            origin: p(),
                            normal: v(),
                            u_axis: Vector3::new(1.0, 0.0, 0.0),
                        },
                    );
                }),
            ),
            (
                "Cylinder",
                emits_keyword(|e| {
                    surface(
                        e,
                        &SurfaceGeometry::Cylinder {
                            origin: p(),
                            axis: v(),
                            ref_direction: Vector3::new(1.0, 0.0, 0.0),
                            radius: 1.0,
                        },
                    );
                }),
            ),
            (
                "Cone",
                emits_keyword(|e| {
                    surface(
                        e,
                        &SurfaceGeometry::Cone {
                            origin: p(),
                            axis: v(),
                            ref_direction: Vector3::new(1.0, 0.0, 0.0),
                            radius: 1.0,
                            ratio: 0.5,
                            half_angle: 0.5,
                        },
                    );
                }),
            ),
            (
                "Sphere",
                emits_keyword(|e| {
                    surface(
                        e,
                        &SurfaceGeometry::Sphere {
                            center: p(),
                            axis: v(),
                            ref_direction: Vector3::new(1.0, 0.0, 0.0),
                            radius: 1.0,
                        },
                    );
                }),
            ),
            (
                "Torus",
                emits_keyword(|e| {
                    surface(
                        e,
                        &SurfaceGeometry::Torus {
                            center: p(),
                            axis: v(),
                            ref_direction: Vector3::new(1.0, 0.0, 0.0),
                            major_radius: 2.0,
                            minor_radius: 1.0,
                        },
                    );
                }),
            ),
            (
                "surface Nurbs (non-rational)",
                emits_keyword(|e| {
                    surface(e, &SurfaceGeometry::Nurbs(nurbs_surface(false)));
                }),
            ),
            (
                "surface Nurbs (rational)",
                emits_keyword(|e| {
                    surface(e, &SurfaceGeometry::Nurbs(nurbs_surface(true)));
                }),
            ),
            (
                "Line",
                emits_keyword(|e| {
                    curve(
                        e,
                        &CurveGeometry::Line {
                            origin: p(),
                            direction: v(),
                        },
                    );
                }),
            ),
            (
                "Circle",
                emits_keyword(|e| {
                    curve(
                        e,
                        &CurveGeometry::Circle {
                            center: p(),
                            axis: v(),
                            ref_direction: Vector3::new(1.0, 0.0, 0.0),
                            radius: 1.0,
                        },
                    );
                }),
            ),
            (
                "Ellipse",
                emits_keyword(|e| {
                    curve(
                        e,
                        &CurveGeometry::Ellipse {
                            center: p(),
                            axis: v(),
                            major_direction: Vector3::new(1.0, 0.0, 0.0),
                            major_radius: 2.0,
                            minor_radius: 1.0,
                        },
                    );
                }),
            ),
            (
                "Parabola",
                emits_keyword(|e| {
                    curve(
                        e,
                        &CurveGeometry::Parabola {
                            vertex: p(),
                            axis: v(),
                            major_direction: Vector3::new(1.0, 0.0, 0.0),
                            focal_distance: 1.0,
                        },
                    );
                }),
            ),
            (
                "Hyperbola",
                emits_keyword(|e| {
                    curve(
                        e,
                        &CurveGeometry::Hyperbola {
                            center: p(),
                            axis: v(),
                            major_direction: Vector3::new(1.0, 0.0, 0.0),
                            major_radius: 2.0,
                            minor_radius: 1.0,
                        },
                    );
                }),
            ),
            (
                "Degenerate",
                emits_keyword(|e| {
                    curve(e, &CurveGeometry::Degenerate { point: p() });
                }),
            ),
            (
                "curve Nurbs (non-rational)",
                emits_keyword(|e| {
                    curve(e, &CurveGeometry::Nurbs(nurbs_curve(false)));
                }),
            ),
            (
                "curve Nurbs (rational)",
                emits_keyword(|e| {
                    curve(e, &CurveGeometry::Nurbs(nurbs_curve(true)));
                }),
            ),
        ];

        for spec in SURFACES.iter().chain(CURVES) {
            let label = format!("{} {}", spec.kind, spec.ir_variant);
            let short = spec.ir_variant;
            let emitted = cases
                .iter()
                .find(|(name, _)| *name == label || *name == short)
                .unwrap_or_else(|| panic!("no writer case for {label}"));
            assert!(
                emitted.1.iter().any(|kw| kw == spec.keyword),
                "carrier {label} does not emit tally keyword {}; emitted {:?}",
                spec.keyword,
                emitted.1
            );
        }
    }
}
