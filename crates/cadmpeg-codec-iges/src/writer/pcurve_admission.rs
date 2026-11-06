// SPDX-License-Identifier: Apache-2.0
//! Adapt parameter curves and provable isoparametric uses to IGES B-rep.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::codec::write::ExportLossCode;
use cadmpeg_ir::geometry::nurbs::NurbsPoles3;
use cadmpeg_ir::geometry::pcurve::{
    CirclePcurve, PcurveGeometry, PcurveNurbs, PcurveNurbsPoles, WeightedPole2,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::topology::{LoopBoundary, LoopRing, PcurveUse};
use cadmpeg_ir::CadIr;

pub(super) fn admit(ir: &CadIr) -> (Cow<'_, CadIr>, Vec<LossNote>) {
    let mut used = BTreeSet::new();
    let mut unknown_flags = BTreeSet::new();
    let mut isoparametric_flags = BTreeSet::new();
    let mut record_uses = |uses: &[PcurveUse]| {
        for item in uses {
            used.insert(item.pcurve.clone());
            if item.isoparametric.is_none() {
                unknown_flags.insert(item.pcurve.clone());
            }
            if item.isoparametric == Some(true) {
                isoparametric_flags.insert(item.pcurve.clone());
            }
        }
    };
    for coedge in &ir.model.coedges {
        record_uses(&coedge.pcurves);
    }
    for boundary in ir.model.loops.iter().map(|item| &item.boundary) {
        match boundary {
            LoopBoundary::Vertex { pcurves, .. } => {
                record_uses(pcurves);
            }
            LoopBoundary::Ring(ring) => {
                for vertex in ring.vertex_uses() {
                    record_uses(&vertex.pcurves);
                }
            }
        }
    }
    let mut replacements = BTreeMap::new();
    let mut reparameterized = BTreeSet::new();
    let mut flags = BTreeMap::new();
    for curve in &ir.model.pcurves {
        if unknown_flags.contains(&curve.id) {
            if let PcurveGeometry::Nurbs { nurbs } = &curve.geometry {
                let points = nurbs.control_points();
                if let Some(first) = points.first() {
                    let first = first.get();
                    if points.iter().all(|point| point.get().u == first.u)
                        || points.iter().all(|point| point.get().v == first.v)
                    {
                        flags.insert(curve.id.clone(), true);
                    }
                }
            }
        }
        if !used.contains(&curve.id) {
            continue;
        }
        let Some(range) = curve.parameter_range() else {
            continue;
        };
        let [start, end] = range.get();
        if start >= end {
            continue;
        }
        if let PcurveGeometry::Circle(circle) = &curve.geometry {
            if let Some(nurbs) = circle_nurbs(circle, [start, end])
                .filter(|_| !isoparametric_flags.contains(&curve.id))
            {
                replacements.insert(curve.id.clone(), nurbs);
                reparameterized.insert(curve.id.clone());
                flags.insert(curve.id.clone(), false);
            }
            continue;
        }
        let PcurveGeometry::Line(line) = &curve.geometry else {
            continue;
        };
        let origin = line.origin().get();
        let direction = line.direction().as_raw();
        let point = |t: f64| Point2::new(origin.u + direction.u * t, origin.v + direction.v * t);
        let Ok(nurbs) = PcurveNurbs::new(
            1,
            vec![start, start, end, end],
            PcurveNurbsPoles::Polynomial {
                points: vec![point(start), point(end)],
            },
            false,
        ) else {
            continue;
        };
        flags.insert(curve.id.clone(), direction.u == 0.0 || direction.v == 0.0);
        replacements.insert(curve.id.clone(), nurbs);
    }
    if replacements.is_empty() && flags.is_empty() {
        return (Cow::Borrowed(ir), Vec::new());
    }
    let mut document = ir.clone();
    let mut losses = Vec::new();
    for curve in &mut document.model.pcurves {
        if let Some(nurbs) = replacements.get(&curve.id) {
            curve.geometry = PcurveGeometry::Nurbs {
                nurbs: nurbs.clone(),
            };
            if reparameterized.contains(&curve.id) {
                losses.push(
                    super::IgesLossCode::WriterPcurveReparameterized.note(format!(
                        "IGES pcurve {} retains its circular locus with a NURBS parameterization",
                        curve.id
                    )),
                );
            } else {
                losses.push(super::IgesLossCode::WriterPcurveConverted.note(format!(
                    "IGES pcurve {} converts its line representation to a degree-1 NURBS",
                    curve.id
                )));
            }
        }
    }
    let mut adapt = |owner: &str, uses: &mut [PcurveUse]| {
        for item in uses {
            if item.isoparametric.is_none() {
                if let Some(isoparametric) = flags.get(&item.pcurve) {
                    item.isoparametric = Some(*isoparametric);
                    losses.push(ExportLossCode::RecordResidualUnavailable.note(format!("IGES {owner} pcurve {} requires an isoparametric flag; its geometry determines {isoparametric}", item.pcurve)));
                }
            }
        }
    };
    for coedge in &mut document.model.coedges {
        adapt(coedge.id.as_str(), &mut coedge.pcurves);
    }
    for loop_ in &mut document.model.loops {
        match &mut loop_.boundary {
            LoopBoundary::Vertex { pcurves, .. } => adapt(loop_.id.as_str(), pcurves),
            LoopBoundary::Ring(ring) => {
                let mut vertices = ring.vertex_uses().to_vec();
                for vertex in &mut vertices {
                    adapt(loop_.id.as_str(), &mut vertex.pcurves);
                }
                *ring = LoopRing::new(ring.coedges().to_vec(), vertices)
                    .expect("existing ring anchors remain valid");
            }
        }
    }
    (Cow::Owned(document), losses)
}

fn circle_nurbs(circle: &CirclePcurve, range: [f64; 2]) -> Option<PcurveNurbs> {
    let center = circle.center().get();
    let first = circle.x_axis().get();
    let second = circle.y_axis().get();
    if first.u * first.u + first.v * first.v != 1.0
        || second.u * second.u + second.v * second.v != 1.0
        || first.u * second.u + first.v * second.v != 0.0
    {
        return None;
    }
    let direction = Vector3::new(first.u, first.v, 0.0);
    let axis = direction.cross(Vector3::new(second.u, second.v, 0.0));
    let radius = PositiveLength::new(circle.radius().get())?;
    let nurbs = crate::entities::curve_conversion::circular_arc_nurbs(
        Point3::new(center.u, center.v, 0.0),
        axis,
        direction,
        radius,
        range,
    )
    .ok()??;
    let (degree, knots, poles, periodic) = nurbs.into_parts();
    let NurbsPoles3::Rational { points } = poles else {
        return None;
    };
    let points = points
        .into_iter()
        .map(|pole| {
            let point = pole.point.get();
            WeightedPole2 {
                point: Point2::new(point.x, point.y),
                weight: pole.weight,
            }
        })
        .collect();
    PcurveNurbs::new(
        degree,
        knots,
        PcurveNurbsPoles::Rational { points },
        periodic,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_ir::codec::write::target::TargetRequest;
    use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use cadmpeg_ir::geometry::pcurve::{CirclePcurve, PcurveGeometry};
    use cadmpeg_ir::math::Point2;

    use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
    use crate::IgesCodec;

    #[test]
    fn circular_parameter_curve_preserves_the_trimmed_face() {
        let entities = [
            (108, "00010000", "108,0,0,1,0,0,0,0,0,0;"),
            (100, "00010000", "100,0,0,0,0.5,0,0.5,0;"),
            (100, "00010500", "100,0,0,0,0.5,0,0.5,0;"),
            (142, "00010000", "142,0,1,5,3,3;"),
            (144, "00000000", "144,1,1,0,7;"),
        ];
        let bytes = owned_test_file(
            &entities
                .into_iter()
                .map(|(entity_type, status, parameters)| OwnedTestEntity {
                    entity_type,
                    form: 0,
                    label: String::new(),
                    status,
                    parameters: parameters.into(),
                })
                .collect::<Vec<_>>(),
        );
        let decoded = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("trimmed plane decodes");
        let mut ir = decoded.ir().clone();
        assert_eq!(ir.model.faces.len(), 1);
        assert_eq!(ir.model.pcurves.len(), 1);
        let pcurve_id = ir.model.pcurves[0].id.clone();
        ir.model.pcurves[0].geometry = PcurveGeometry::Circle(
            CirclePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
                0.5,
            )
            .expect("valid circle"),
        );
        let plan = IgesCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .expect("trimmed plane writes");
        assert!(plan.report().losses.iter().any(|loss| {
            loss.code == crate::loss::IgesLossCode::WriterPcurveReparameterized.kind()
                && loss.message.contains(pcurve_id.as_str())
        }));
        let mut output = Vec::new();
        plan.write_to(&mut output).expect("write output");
        let roundtrip = IgesCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .expect("output decodes");
        assert_eq!(roundtrip.ir().model.faces.len(), 1);
        assert_eq!(roundtrip.ir().model.pcurves.len(), 1);
        let written = &roundtrip.ir().model.pcurves[0];
        let [start, end] = written.parameter_range().expect("pcurve range").get();
        for fraction in [0.0, 0.125, 0.25, 0.5, 0.75, 1.0] {
            let point = cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &written.geometry,
                start + (end - start) * fraction,
            )
            .expect("pcurve evaluates")
            .get();
            assert!((point.u * point.u + point.v * point.v - 0.25).abs() < 1.0e-10);
        }
    }
}
