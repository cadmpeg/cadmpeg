// SPDX-License-Identifier: Apache-2.0
//! A `TOROIDAL_SURFACE` with a negative major radius.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::surface_partials_solved;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;

use crate::StepCodec;

const MAJOR: f64 = -3.0;
const MINOR: f64 = 5.0;
const EPS_POINT: f64 = 1.0e-9;

/// One face on the torus about `z` with major radius `MAJOR` and minor
/// radius `MINOR`, bounded by its circle of radius 3 at height 5.
fn source(same_sense: &str) -> String {
    format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('lemon'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;DATA;\
#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
#2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));\
#3=CARTESIAN_POINT('',(0.,0.,0.));#4=DIRECTION('',(0.,0.,1.));#5=DIRECTION('',(1.,0.,0.));\
#6=AXIS2_PLACEMENT_3D('',#3,#4,#5);#7=TOROIDAL_SURFACE('',#6,{MAJOR:?},{MINOR:?});\
#8=CARTESIAN_POINT('',(0.,0.,5.));#9=AXIS2_PLACEMENT_3D('',#8,#4,#5);#10=CIRCLE('',#9,3.);\
#11=CARTESIAN_POINT('',(3.,0.,5.));#12=VERTEX_POINT('',#11);\
#13=EDGE_CURVE('',#12,#12,#10,.T.);#14=ORIENTED_EDGE('',*,*,#13,.T.);#15=EDGE_LOOP('',(#14));\
#16=FACE_OUTER_BOUND('',#15,.T.);#17=ADVANCED_FACE('',(#16),#7,{same_sense});\
#18=OPEN_SHELL('',(#17));#19=SHELL_BASED_SURFACE_MODEL('',(#18));\
#20=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#19),#2);ENDSEC;END-ISO-10303-21;"
    )
}

/// The source surface at `(u, v)` and its `Su × Sv`.
fn source_normal(u: f64, v: f64) -> (Point3, Vector3) {
    let ring = MAJOR + MINOR * v.cos();
    let point = Point3::new(ring * u.cos(), ring * u.sin(), MINOR * v.sin());
    let du = Vector3::new(-ring * u.sin(), ring * u.cos(), 0.0);
    let dv = Vector3::new(
        -MINOR * v.sin() * u.cos(),
        -MINOR * v.sin() * u.sin(),
        MINOR * v.cos(),
    );
    (point, du.cross(dv))
}

#[test]
fn a_negative_major_radius_states_the_inner_sheet_with_the_opposite_normal() {
    for (same_sense, sign) in [(".T.", 1.0), (".F.", -1.0)] {
        let result = StepCodec::default()
            .decode(
                &mut Cursor::new(source(same_sense).into_bytes()),
                &DecodeOptions::default(),
            )
            .expect("decode the lemon torus");
        let model = &result.ir().geometry;
        let [face] = model.faces.as_slice() else {
            panic!("one face: {:#?}", result.report().losses);
        };
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("the face's surface");
        let Some(SolvedSurfaceGeometry::Torus(torus)) = surface.geometry.solved() else {
            panic!("a torus carrier");
        };
        assert_eq!(torus.major_radius().get(), MAJOR.abs());
        assert!(model.procedural_surfaces.iter().any(|procedural| {
            matches!(
                procedural.definition(),
                ProceduralSurfaceDefinition::DegenerateTorus {
                    select_outer: false
                }
            )
        }));
        let face_sign = if face.sense == Sense::Forward {
            1.0
        } else {
            -1.0
        };
        for (u, v) in [(0.3, 1.2), (2.0, -0.7), (4.5, 2.5)] {
            let (point, normal) = source_normal(u, v);
            let partials = surface_partials_solved(
                EvaluationAdmission::Standard,
                surface.geometry.solved().expect("solved"),
                u + std::f64::consts::PI,
                std::f64::consts::PI - v,
            )
            .expect("partials");
            let decoded_point = partials.point.get();
            assert!(
                decoded_point.distance(point) < EPS_POINT,
                "{decoded_point:?} {point:?}"
            );
            let decoded_normal = partials.du.get().cross(partials.dv.get());
            assert!(
                (decoded_normal.scale(face_sign)).dot(normal.scale(sign)) > 0.0,
                "{same_sense} at ({u}, {v})"
            );
        }
    }
}
