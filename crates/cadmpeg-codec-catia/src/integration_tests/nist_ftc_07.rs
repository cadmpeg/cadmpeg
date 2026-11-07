// SPDX-License-Identifier: Apache-2.0
//! B-rep geometry of the FTC-07 solid, compared with its AP242 model.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;

use crate::CatiaCodec;

#[test]
fn nist_ftc_07_transfers_every_solid_face_carrier() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart");
    let bytes = std::fs::read(path).expect("read FTC-07 CATPart");
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode FTC-07 CATPart");
    let model = &decoded.ir().model;
    assert_eq!(
        model
            .faces
            .iter()
            .filter(|face| face.id.as_str().starts_with("catia:standard:face#"))
            .count(),
        304
    );
    let mut nurbs = 0;
    for face in &model.faces {
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("face carrier exists");
        match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => nurbs += 1,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => {
                panic!("unresolved face carrier")
            }
            SurfaceGeometry::Solved(_) => {}
            SurfaceGeometry::Procedural { .. } => panic!("unresolved face carrier construction"),
        }
    }
    assert_eq!(nurbs, 20);
    let native = decoded.ir().native.namespace("catia").expect("native data");
    let charts: Vec<crate::native::a5_chart::CatiaConsolidatedA5Chart> = native
        .arena_as("consolidated_a5_charts")
        .expect("reload native chart records");
    assert_eq!(charts.len(), 20);
}

#[test]
fn nist_ftc_07_circular_edge_agrees_with_both_face_curves() {
    const STEP_EDGE_LENGTH_MM: f64 = 2.367_294_783_918_816_3;
    const LENGTH_TOLERANCE_MM: f64 = 2e-3;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart");
    let bytes = std::fs::read(path).expect("read FTC-07 CATPart");
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode FTC-07 CATPart");
    let model = &decoded.ir().model;
    let edge = model
        .edges
        .iter()
        .find(|edge| edge.id.as_str() == "catia:standard:edge#259")
        .expect("edge");
    let curve = model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve())
        .expect("circle");
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = &curve.geometry else {
        panic!("circular edge");
    };
    let [lower, upper] = edge.param_range().expect("trim interval").get();
    assert!(
        (circle.radius().get() * (upper - lower) - STEP_EDGE_LENGTH_MM).abs() < LENGTH_TOLERANCE_MM
    );
    let admission = cadmpeg_ir::eval::admission::EvaluationAdmission::Standard;
    let midpoint =
        cadmpeg_ir::eval::decode::curve_point(admission, &curve.geometry, lower.midpoint(upper))
            .expect("edge midpoint")
            .get();
    for (surface_id, pcurve_id) in [
        ("catia:standard:surf#10", "catia:standard:pcurve#10:0:0"),
        ("catia:standard:surf#139", "catia:standard:pcurve#139:0:5"),
    ] {
        let surface = model
            .surfaces
            .iter()
            .find(|item| item.id.as_str() == surface_id)
            .expect("face surface");
        let pcurve = model
            .pcurves
            .iter()
            .find(|item| item.id.as_str() == pcurve_id)
            .expect("face pcurve");
        let [first, last] = pcurve.parameter_range().expect("pcurve interval").get();
        let uv =
            cadmpeg_ir::eval::decode::pcurve_uv(admission, &pcurve.geometry, first.midpoint(last))
                .expect("pcurve midpoint");
        let lifted =
            cadmpeg_ir::eval::decode::surface_point(admission, &surface.geometry, uv.u, uv.v)
                .expect("lifted pcurve midpoint")
                .get();
        assert!(lifted.distance(midpoint) < LENGTH_TOLERANCE_MM);
    }
}

#[test]
fn nist_ftc_07_transfers_all_freeform_boundary_curves() {
    const ENDPOINT_TOLERANCE_MM: f64 = 2.0e-5;
    const STEP_SAMPLE_TOLERANCE_MM: f64 = 1.0e-4;
    const PLANE_JET_EDGES: [usize; 8] = [418, 425, 436, 443, 700, 728, 753, 761];
    const STEP_BOUNDARY_MIDPOINTS_MM: [(usize, [f64; 3]); 64] = [
        (418, [-94.611_231_844, -103.516_638_909, -96.624_385_766]),
        (425, [141.074_385_766, -103.516_638_905, -69.211_231_846]),
        (436, [-94.611_234_209, -103.516_636_583, 96.624_385_806]),
        (443, [-141.074_385_769, -103.516_638_717, 69.211_232_037]),
        (471, [152.419_630_201, -0.015_815_501, 105.047_870_591]),
        (472, [152.262_395_522, 0.326_387_930, 106.232_159_232]),
        (473, [150.685_277_757, 0.326_237_963, 107.815_552_885]),
        (474, [149.497_870_591, -0.015_815_501, 107.969_630_201]),
        (475, [150.555_826_635, -0.608_282_208, 107.686_101_764]),
        (476, [152.133_136_334, -0.606_746_659, 106.102_900_043]),
        (477, [-152.419_630_201, -0.015_815_501, 105.047_870_591]),
        (478, [-152.133_136_334, -0.606_746_659, 106.102_900_043]),
        (479, [-150.555_826_635, -0.608_282_208, 107.686_101_764]),
        (480, [-149.497_870_591, -0.015_815_501, 107.969_630_201]),
        (481, [-150.685_277_757, 0.326_237_963, 107.815_552_885]),
        (482, [-152.262_395_522, 0.326_387_930, 106.232_159_232]),
        (483, [-152.419_630_201, -0.015_815_501, -105.047_870_591]),
        (484, [-152.262_395_522, 0.326_387_930, -106.232_159_232]),
        (485, [-150.685_277_757, 0.326_237_963, -107.815_552_885]),
        (486, [-149.497_870_591, -0.015_815_501, -107.969_630_201]),
        (487, [-150.555_826_635, -0.608_282_208, -107.686_101_764]),
        (488, [-152.133_136_334, -0.606_746_659, -106.102_900_043]),
        (489, [149.497_870_591, -0.015_815_501, -107.969_630_201]),
        (490, [150.682_159_232, 0.326_387_930, -107.812_395_522]),
        (491, [152.265_552_885, 0.326_237_963, -106.235_277_757]),
        (492, [152.419_630_201, -0.015_815_501, -105.047_870_591]),
        (493, [152.136_101_764, -0.608_282_208, -106.105_826_635]),
        (494, [150.552_900_043, -0.606_746_659, -107.683_136_334]),
        (499, [152.495_954_571, 0.358_107_336, -104.997_593_857]),
        (502, [152.393_000_475, -0.376_697_815, -104.946_744_674]),
        (505, [152.495_954_571, 0.358_107_336, 104.997_593_855]),
        (506, [152.393_000_475, -0.376_697_815, 104.946_744_674]),
        (507, [149.447_593_857, 0.358_107_336, 108.045_954_571]),
        (510, [149.396_744_674, -0.376_697_815, 107.943_000_475]),
        (511, [-149.396_744_674, -0.376_697_815, 107.943_000_475]),
        (514, [-149.447_593_854, 0.358_107_336, 108.045_954_571]),
        (517, [-152.393_000_475, -0.376_697_815, 104.946_744_674]),
        (518, [-152.495_954_571, 0.358_107_336, 104.997_593_857]),
        (521, [-152.495_954_571, 0.358_107_336, -104.997_593_855]),
        (522, [-152.393_000_475, -0.376_697_815, -104.946_744_674]),
        (523, [-149.447_593_857, 0.358_107_336, -108.045_954_571]),
        (526, [-149.396_744_674, -0.376_697_815, -107.943_000_475]),
        (529, [149.447_593_854, 0.358_107_336, -108.045_954_571]),
        (530, [149.396_744_674, -0.376_697_815, -107.943_000_475]),
        (544, [-139.561_823_278, -104.256_042_271, 70.771_719_768]),
        (551, [-96.171_718_273, -104.256_040_730, 95.111_823_333]),
        (554, [139.533_631_188, -105.063_604_155, -69.949_938_563]),
        (563, [95.348_251_038, -105.061_910_305, -95.083_690_320]),
        (672, [-139.533_690_250, -105.061_912_319, 69.948_253_065]),
        (675, [-95.348_251_038, -105.061_910_305, 95.083_690_320]),
        (698, [96.171_718_273, -104.256_040_730, 95.111_823_333]),
        (699, [95.348_251_038, -105.061_910_305, 95.083_690_320]),
        (700, [94.611_231_938, -103.516_638_815, 96.624_385_768]),
        (728, [-141.074_385_806, -103.516_636_589, -69.211_234_202]),
        (730, [-139.533_690_250, -105.061_912_319, -69.948_253_065]),
        (731, [-139.561_823_278, -104.256_042_271, -70.771_719_768]),
        (753, [94.611_234_209, -103.516_636_583, -96.624_385_806]),
        (754, [96.171_718_273, -104.256_040_730, -95.111_823_333]),
        (757, [-96.170_055_626, -104.254_325_444, -95.111_883_214]),
        (758, [-95.349_937_047, -105.063_602_637, -95.083_631_240]),
        (761, [141.074_385_808, -103.516_636_488, 69.211_234_306]),
        (763, [139.533_690_250, -105.061_912_319, 69.948_253_065]),
        (764, [139.561_823_278, -104.256_042_271, 70.771_719_768]),
        (809, [139.561_883_141, -104.254_327_504, -70.770_057_607]),
    ];
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart");
    let bytes = std::fs::read(path).expect("read FTC-07 CATPart");
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode FTC-07 CATPart");
    let model = &decoded.ir().model;
    let surfaces: HashMap<_, _> = model.surfaces.iter().map(|s| (&s.id, s)).collect();
    let freeform_faces: HashSet<_> = model
        .faces
        .iter()
        .filter(|face| {
            matches!(
                surfaces[&face.surface].geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
            )
        })
        .map(|face| &face.id)
        .collect();
    let freeform_loops: HashSet<_> = model
        .loops
        .iter()
        .filter(|boundary| freeform_faces.contains(&boundary.face))
        .map(|boundary| &boundary.id)
        .collect();
    let boundary_edges: HashSet<_> = model
        .coedges
        .iter()
        .filter(|coedge| freeform_loops.contains(&coedge.owner_loop))
        .map(|coedge| &coedge.edge)
        .collect();
    assert_eq!(freeform_faces.len(), 20);
    assert_eq!(boundary_edges.len(), 112);
    let curves: HashMap<_, _> = model
        .curves
        .iter()
        .map(|curve| (&curve.id, curve))
        .collect();
    let edges: HashMap<_, _> = model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect();
    let mut circle_count = 0;
    let mut nurbs_count = 0;
    let mut unresolved = Vec::new();
    for edge in model
        .edges
        .iter()
        .filter(|edge| boundary_edges.contains(&edge.id))
    {
        match edge
            .curve()
            .and_then(|id| curves.get(id))
            .and_then(|curve| curve.geometry.solved())
        {
            Some(SolvedCurveGeometry::Circle(_)) => circle_count += 1,
            Some(SolvedCurveGeometry::Nurbs(_)) => nurbs_count += 1,
            _ => unresolved.push(edge.id.as_str()),
        }
    }
    assert!(
        unresolved.is_empty(),
        "unresolved freeform boundary curves: {unresolved:?}"
    );
    assert_eq!(circle_count, 48);
    assert_eq!(nurbs_count, STEP_BOUNDARY_MIDPOINTS_MM.len());
    let points: HashMap<_, _> = model
        .points
        .iter()
        .map(|point| (&point.id, point.position().get()))
        .collect();
    let vertices: HashMap<_, _> = model
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, points[&vertex.point]))
        .collect();
    let pcurves: HashMap<_, _> = model
        .pcurves
        .iter()
        .map(|pcurve| (&pcurve.id, pcurve))
        .collect();
    let loop_faces: HashMap<_, _> = model
        .loops
        .iter()
        .map(|boundary| (&boundary.id, &boundary.face))
        .collect();
    let faces: HashMap<_, _> = model.faces.iter().map(|face| (&face.id, face)).collect();
    let mut freeform_pcurve_uses = 0;
    let mut plane_pcurve_uses = 0;
    let admission = cadmpeg_ir::eval::admission::EvaluationAdmission::Standard;
    for (ordinal, [x, y, z]) in STEP_BOUNDARY_MIDPOINTS_MM {
        let id = format!("catia:standard:edge#{ordinal}");
        let edge = edges[id.as_str()];
        assert!(boundary_edges.contains(&edge.id));
        let curve = curves[edge.curve().expect("edge curve")];
        assert!(matches!(
            curve.geometry.solved(),
            Some(SolvedCurveGeometry::Nurbs(_))
        ));
        let [first, last] = edge.param_range().expect("bounded edge curve").get();
        let start = cadmpeg_ir::eval::decode::curve_point(admission, &curve.geometry, first)
            .unwrap_or_else(|error| panic!("{id}: curve start at {first}: {error:?}"))
            .get();
        let end = cadmpeg_ir::eval::decode::curve_point(admission, &curve.geometry, last)
            .unwrap_or_else(|error| panic!("{id}: curve end at {last}: {error:?}"))
            .get();
        let vertex_start = vertices[&edge.start];
        let vertex_end = vertices[&edge.end];
        let endpoint_error = start.distance(vertex_start).max(end.distance(vertex_end));
        assert!(
            endpoint_error < ENDPOINT_TOLERANCE_MM,
            "{id}: endpoint error {endpoint_error} mm"
        );
        let midpoint =
            cadmpeg_ir::eval::decode::curve_point(admission, &curve.geometry, first.midpoint(last))
                .unwrap_or_else(|error| {
                    panic!(
                        "{id}: curve midpoint at {}: {error:?}",
                        first.midpoint(last)
                    )
                })
                .get();
        let midpoint_error = midpoint.distance(Point3::new(x, y, z));
        assert!(
            midpoint_error < STEP_SAMPLE_TOLERANCE_MM,
            "{id}: STEP midpoint error {midpoint_error} mm"
        );
        for coedge in model.coedges.iter().filter(|coedge| coedge.edge == edge.id) {
            let face = faces[loop_faces[&coedge.owner_loop]];
            let surface = surfaces[&face.surface];
            let plane_jet = PLANE_JET_EDGES.contains(&ordinal)
                && matches!(
                    surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                );
            let freeform = freeform_faces.contains(&face.id);
            if !freeform && !plane_jet {
                continue;
            }
            let [use_] = coedge.pcurves.as_slice() else {
                panic!("{id}: {} must have one source pcurve", coedge.id);
            };
            let pcurve = pcurves[&use_.pcurve];
            let [pfirst, plast] = use_.parameter_range.map_or_else(
                || {
                    pcurve
                        .parameter_range()
                        .unwrap_or_else(|| panic!("{id}: {} has no source interval", pcurve.id))
                        .get()
                },
                cadmpeg_ir::geometry::DirectedParameterRange::endpoints,
            );
            let mut lifted = Vec::new();
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let parameter = pfirst * (1.0 - fraction) + plast * fraction;
                let uv =
                    cadmpeg_ir::eval::decode::pcurve_uv(admission, &pcurve.geometry, parameter)
                        .unwrap_or_else(|error| {
                            panic!("{id}: {} at {parameter}: {error:?}", pcurve.id)
                        });
                lifted.push(
                    cadmpeg_ir::eval::decode::surface_point(
                        admission,
                        &surface.geometry,
                        uv.u,
                        uv.v,
                    )
                    .unwrap_or_else(|error| {
                        panic!("{id}: {} surface lift at {parameter}: {error:?}", pcurve.id)
                    })
                    .get(),
                );
            }
            let direct = lifted[0]
                .distance(vertex_start)
                .max(lifted[4].distance(vertex_end));
            let reverse = lifted[0]
                .distance(vertex_end)
                .max(lifted[4].distance(vertex_start));
            assert!(
                direct.min(reverse) < ENDPOINT_TOLERANCE_MM,
                "{id}: {} endpoint lift misses by {} mm",
                pcurve.id,
                direct.min(reverse)
            );
            if freeform {
                freeform_pcurve_uses += 1;
            }
            if plane_jet {
                plane_pcurve_uses += 1;
                let [cfirst, clast] = if direct <= reverse {
                    [first, last]
                } else {
                    [last, first]
                };
                for (point, fraction) in lifted.iter().zip([0.0, 0.25, 0.5, 0.75, 1.0]) {
                    let parameter = cfirst * (1.0 - fraction) + clast * fraction;
                    let expected = cadmpeg_ir::eval::decode::curve_point(
                        admission,
                        &curve.geometry,
                        parameter,
                    )
                    .expect("plane-jet curve point")
                    .get();
                    let miss = point.distance(expected);
                    assert!(
                        miss < ENDPOINT_TOLERANCE_MM,
                        "{id}: {} plane rechart misses the edge by {miss} mm",
                        pcurve.id
                    );
                }
            }
        }
    }
    assert_eq!(freeform_pcurve_uses, 72);
    assert_eq!(plane_pcurve_uses, PLANE_JET_EDGES.len());
}

#[test]
fn nist_ftc_07_transfers_both_planar_sheet_bodies() {
    use cadmpeg_ir::topology::BodyKind;

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart");
    let bytes = std::fs::read(path).expect("read FTC-07 CATPart");
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode FTC-07 CATPart");
    let model = &decoded.ir().model;
    let solid_faces: HashSet<_> = model
        .faces
        .iter()
        .filter(|face| face.id.as_str().starts_with("catia:standard:face#"))
        .map(|face| &face.id)
        .collect();
    let colored_solid_faces: HashSet<_> = model
        .appearance_bindings
        .iter()
        .filter_map(|binding| match &binding.target {
            cadmpeg_ir::appearance::AppearanceTarget::Face(face) if solid_faces.contains(face) => {
                Some(face)
            }
            _ => None,
        })
        .collect();
    assert_eq!(solid_faces.len(), 304);
    assert_eq!(colored_solid_faces, solid_faces);
    let sheets: Vec<_> = model
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::Sheet)
        .collect();
    assert_eq!(sheets.len(), 2);
    let mut areas = Vec::new();
    for body in sheets {
        let [region_id] = body.regions.as_slice() else {
            panic!("one region per sheet")
        };
        let region = model
            .regions
            .iter()
            .find(|region| &region.id == region_id)
            .expect("sheet region");
        let [shell_id] = region.shells.as_slice() else {
            panic!("one shell per sheet")
        };
        let shell = model
            .shells
            .iter()
            .find(|shell| &shell.id == shell_id)
            .expect("sheet shell");
        let [face_id] = shell.faces() else {
            panic!("one face per sheet")
        };
        let face = model
            .faces
            .iter()
            .find(|face| &face.id == face_id)
            .expect("sheet face");
        let surface = model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .expect("sheet surface");
        assert!(matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ));
        let boundaries: Vec<_> = model
            .loops
            .iter()
            .filter(|boundary| boundary.face == face.id)
            .collect();
        let [boundary] = boundaries.as_slice() else {
            panic!("one boundary per sheet")
        };
        let coedges: Vec<_> = model
            .coedges
            .iter()
            .filter(|coedge| coedge.owner_loop == boundary.id)
            .collect();
        assert_eq!(coedges.len(), 3);
        let mut vertex_ids = HashSet::new();
        for coedge in coedges {
            let edge = model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .expect("sheet edge");
            vertex_ids.insert(&edge.start);
            vertex_ids.insert(&edge.end);
        }
        assert_eq!(vertex_ids.len(), 3);
        let points: Vec<_> = vertex_ids
            .into_iter()
            .map(|id| {
                let vertex = model
                    .vertices
                    .iter()
                    .find(|vertex| &vertex.id == id)
                    .expect("sheet vertex");
                model
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
                    .expect("sheet point")
                    .position()
                    .get()
            })
            .collect();
        for point in &points {
            assert!((point.y - 12.7).abs() < 1.0e-6);
        }
        let a = points[1].vector_from(points[0]);
        let b = points[2].vector_from(points[0]);
        areas.push(a.cross(b).norm() * 0.5);
    }
    areas.sort_by(f64::total_cmp);
    for (actual, expected) in areas
        .into_iter()
        .zip([7.796_162_084_652_766, 7.796_162_524_684_708])
    {
        assert!(
            (actual - expected).abs() < 1.0e-5,
            "sheet area {actual} vs STEP {expected}"
        );
    }
}

#[test]
fn nist_ftc_07_plane_semicircle_follows_the_edge_interval() {
    const TOLERANCE_MM: f64 = 2e-3;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/catia/nist_ftc_07_asme1_rd_ct5290_ra.CATPart");
    let bytes = std::fs::read(path).expect("read FTC-07 CATPart");
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode FTC-07 CATPart");
    let model = &decoded.ir().model;
    let edge = model
        .edges
        .iter()
        .find(|e| e.id.as_str() == "catia:standard:edge#28")
        .expect("edge");
    let curve = model
        .curves
        .iter()
        .find(|c| Some(&c.id) == edge.curve())
        .expect("edge curve");
    let surface = model
        .surfaces
        .iter()
        .find(|s| s.id.as_str() == "catia:standard:surf#4")
        .expect("plane");
    let pcurve = model
        .pcurves
        .iter()
        .find(|p| p.id.as_str() == "catia:standard:pcurve#4:0:0")
        .expect("pcurve");
    let [a, b] = edge.param_range().expect("edge interval").get();
    let [u, v] = pcurve.parameter_range().expect("pcurve interval").get();
    let admission = cadmpeg_ir::eval::admission::EvaluationAdmission::Standard;
    let expected = cadmpeg_ir::eval::decode::curve_point(admission, &curve.geometry, a.midpoint(b))
        .expect("edge midpoint")
        .get();
    let uv = cadmpeg_ir::eval::decode::pcurve_uv(admission, &pcurve.geometry, u.midpoint(v))
        .expect("pcurve midpoint");
    let actual = cadmpeg_ir::eval::decode::surface_point(admission, &surface.geometry, uv.u, uv.v)
        .expect("surface midpoint")
        .get();
    assert!(actual.distance(expected) < TOLERANCE_MM);
}
