// SPDX-License-Identifier: Apache-2.0
//! Sheet selection of spindle `TOROIDAL_SURFACE` carriers from the bounding
//! edges of their faces.

use std::f64::consts::PI;
use std::fmt::Write as _;

use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};
use cadmpeg_measure::test_support::round_trip::measured;

use crate::loss::StepLossCode;
use crate::test_support::exchange::decode_inline;

/// Allowance of a measured area against its closed form beyond the value's
/// numeric error, per unit of the expected area.
const AREA_RELATIVE: f64 = 1.0e-9;

/// The spindle torus: major radius 1, minor radius 5.
const SPINDLE: (f64, f64) = (1.0, 5.0);

/// The ring torus: major radius 5, minor radius 1.
const RING: (f64, f64) = (5.0, 1.0);

/// Half the height of the spindle bands.
const SPINDLE_BAND_HALF_HEIGHT: f64 = 2.0;

/// Half the height of the ring band.
const RING_BAND_HALF_HEIGHT: f64 = 0.5;

/// A face between `z = -height` and `z = height` on one sheet of the torus.
#[derive(Clone, Copy)]
struct Band {
    outer: bool,
    height: f64,
    /// A radial displacement of the bounding circles off their sheet.
    offset: f64,
}

impl Band {
    /// The radius of the band's bounding circles: `√(r² − z²) ± R`.
    fn radius(self, (major, minor): (f64, f64)) -> f64 {
        let reach = minor.mul_add(minor, -self.height * self.height).sqrt();
        let on_sheet = if self.outer {
            reach + major
        } else {
            reach - major
        };
        on_sheet + self.offset
    }

    /// `2π·∫ r|R + r cos v| dv` over the band's `v` range.
    fn area(self, (major, minor): (f64, f64)) -> f64 {
        let half = (self.height / minor).asin();
        let integral = if self.outer {
            2.0 * major * half + 2.0 * minor * half.sin()
        } else {
            2.0 * minor * half.sin() - 2.0 * major * half
        };
        2.0 * PI * minor * integral
    }
}

/// A sheet part whose one open shell holds `bands` on one torus, in Part 21
/// records.
fn part((major, minor): (f64, f64), bands: &[Band]) -> String {
    let mut records = format!(
        "#10=CARTESIAN_POINT('',(0.,0.,0.));\n\
         #11=DIRECTION('',(0.,0.,1.));\n\
         #12=DIRECTION('',(1.,0.,0.));\n\
         #13=AXIS2_PLACEMENT_3D('',#10,#11,#12);\n\
         #14=TOROIDAL_SURFACE('',#13,{major:?},{minor:?});\n"
    );
    let mut faces = Vec::new();
    let mut next = 100;
    for band in bands {
        let radius = band.radius((major, minor));
        let mut bounds = Vec::new();
        // The natural normal points away from the axis on the outer sheet and
        // toward it on the inner sheet; each bound keeps the band on its left.
        let (lower, upper) = if band.outer {
            (".T.", ".F.")
        } else {
            (".F.", ".T.")
        };
        for (height, forward) in [(-band.height, lower), (band.height, upper)] {
            let id = next;
            next += 10;
            write!(
                records,
                "#{id}=CARTESIAN_POINT('',(0.,0.,{height:?}));\n\
                 #{}=AXIS2_PLACEMENT_3D('',#{id},#11,#12);\n\
                 #{}=CIRCLE('',#{},{radius:?});\n\
                 #{}=CARTESIAN_POINT('',({radius:?},0.,{height:?}));\n\
                 #{}=VERTEX_POINT('',#{});\n\
                 #{}=EDGE_CURVE('',#{},#{},#{},.T.);\n\
                 #{}=ORIENTED_EDGE('',*,*,#{},{forward});\n\
                 #{}=EDGE_LOOP('',(#{}));\n\
                 #{}=FACE_BOUND('',#{},.T.);\n",
                id + 1,
                id + 2,
                id + 1,
                id + 3,
                id + 4,
                id + 3,
                id + 5,
                id + 4,
                id + 4,
                id + 2,
                id + 6,
                id + 5,
                id + 7,
                id + 6,
                id + 8,
                id + 7,
            )
            .expect("write records");
            bounds.push(format!("#{}", id + 8));
        }
        let face = next;
        next += 10;
        writeln!(
            records,
            "#{face}=ADVANCED_FACE('',({}),#14,.T.);",
            bounds.join(",")
        )
        .expect("write face");
        faces.push(format!("#{face}"));
    }
    write!(
        records,
        "#20=OPEN_SHELL('',({}));\n\
         #21=SHELL_BASED_SURFACE_MODEL('',(#20));\n\
         #30=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #31=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #32=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#30,#31)) REPRESENTATION_CONTEXT('model','3D'));\n\
         #33=SHAPE_REPRESENTATION('',(#21),#32);\n\
         #1=APPLICATION_CONTEXT('mechanical design');\n\
         #2=PRODUCT_CONTEXT('',#1,'mechanical');\n\
         #3=PRODUCT('P','Part','',(#2));\n\
         #4=PRODUCT_DEFINITION_FORMATION('','',#3);\n\
         #5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');\n\
         #6=PRODUCT_DEFINITION('design','',#4,#5);\n\
         #7=PRODUCT_DEFINITION_SHAPE('','',#6);\n\
         #8=SHAPE_DEFINITION_REPRESENTATION(#7,#33);\n",
        faces.join(",")
    )
    .expect("write part");
    records
}

/// The sheet selections the decode attaches.
fn selections(result: &DecodeResult) -> Vec<bool> {
    result
        .ir()
        .geometry
        .procedural_surfaces
        .iter()
        .filter_map(|procedural| match procedural.definition() {
            ProceduralSurfaceDefinition::DegenerateTorus { select_outer } => Some(*select_outer),
            _ => None,
        })
        .collect()
}

fn sheet_losses(result: &DecodeResult) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == StepLossCode::TorusSheetUnresolved.kind())
        .count()
}

/// The decode selects `outer`, validates, and measures the band's closed-form
/// area.
fn assert_selects(band: Band) {
    let result = decode_inline(&part(SPINDLE, &[band]));
    assert_eq!(selections(&result), [band.outer]);
    assert_eq!(sheet_losses(&result), 0);
    assert!(matches!(
        result.ir().geometry.surfaces[0].geometry,
        SurfaceGeometry::Procedural { .. }
    ));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validation runs");
    assert_eq!(validation.error_count(), 0, "{:#?}", validation.findings);
    let report = measured("spindle band", result.ir()).expect("measure");
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    let area = body.area().as_ref().expect("area row");
    // A measurer without trimmed spindle sheet domains refuses the region
    // domain; every other measurer meets the closed form.
    let Some(area) = area.measured() else {
        assert!(
            format!("{area:?}").contains("region.domain-unsupported"),
            "{area:?}"
        );
        return;
    };
    let expected = band.area(SPINDLE);
    let allowed = area.numeric_error().get() + AREA_RELATIVE * expected;
    let computed = area.value().value.get();
    assert!(
        (computed - expected).abs() <= allowed,
        "computed {computed}, expected {expected}, allowed {allowed}"
    );
}

#[test]
fn a_band_on_the_lemon_sheet_selects_the_inner_sheet() {
    assert_selects(Band {
        outer: false,
        height: SPINDLE_BAND_HALF_HEIGHT,
        offset: 0.0,
    });
}

#[test]
fn a_band_on_the_apple_sheet_selects_the_outer_sheet() {
    assert_selects(Band {
        outer: true,
        height: SPINDLE_BAND_HALF_HEIGHT,
        offset: 0.0,
    });
}

#[test]
fn faces_on_both_sheets_of_one_surface_select_none_and_report_it() {
    let result = decode_inline(&part(
        SPINDLE,
        &[
            Band {
                outer: false,
                height: SPINDLE_BAND_HALF_HEIGHT,
                offset: 0.0,
            },
            Band {
                outer: true,
                height: SPINDLE_BAND_HALF_HEIGHT,
                offset: 0.0,
            },
        ],
    ));
    assert!(selections(&result).is_empty());
    assert_eq!(sheet_losses(&result), 1);
}

#[test]
fn a_ring_torus_has_no_sheet_selection_and_no_loss() {
    let result = decode_inline(&part(
        RING,
        &[Band {
            outer: true,
            height: RING_BAND_HALF_HEIGHT,
            offset: 0.0,
        }],
    ));
    assert!(selections(&result).is_empty());
    assert_eq!(sheet_losses(&result), 0);
}

/// A displacement of the bounding circles off the outer sheet, ten times the
/// default linear uncertainty and far below the distance between the sheets.
const EXPORTER_OFFSET: f64 = 1.0e-4;

/// The radius at which a band's circles lie about equally far from both
/// sheets: close to the axis, away from the poles.
const BETWEEN_SHEETS_RADIUS: f64 = 1.0e-3;

#[test]
fn circles_slightly_off_the_outer_sheet_select_the_nearer_sheet() {
    let result = decode_inline(&part(
        SPINDLE,
        &[Band {
            outer: true,
            height: SPINDLE_BAND_HALF_HEIGHT,
            offset: EXPORTER_OFFSET,
        }],
    ));
    assert_eq!(selections(&result), [true]);
    assert_eq!(sheet_losses(&result), 0);
}

#[test]
fn circles_between_the_sheets_select_none_and_report_it() {
    let on_sheet = Band {
        outer: false,
        height: SPINDLE_BAND_HALF_HEIGHT,
        offset: 0.0,
    }
    .radius(SPINDLE);
    let result = decode_inline(&part(
        SPINDLE,
        &[Band {
            outer: false,
            height: SPINDLE_BAND_HALF_HEIGHT,
            offset: BETWEEN_SHEETS_RADIUS - on_sheet,
        }],
    ));
    assert!(selections(&result).is_empty());
    assert_eq!(sheet_losses(&result), 1);
}
