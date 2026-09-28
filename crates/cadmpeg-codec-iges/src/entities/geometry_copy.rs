// SPDX-License-Identifier: Apache-2.0
//! Fallible copies of retained geometry used by derived IGES carriers.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::NonEmptyMembers;
use cadmpeg_ir::geometry::nurbs::{KnotVector, NurbsCurve, NurbsPoles3};
use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
use cadmpeg_ir::geometry::{CompositeCurveSegments, PlacedCurve, SolvedCurveGeometry};

use crate::decode_resource::{clone_optional_identity, reserve_optional_vec};

fn copy_nurbs_curve(
    curve: &NurbsCurve,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<NurbsCurve, CodecError> {
    let mut knots = reserve_optional_vec(ctx, curve.knots().len(), "iges solved curve copied knots")?;
    knots.extend_from_slice(curve.knots().as_slice());
    let knots = KnotVector::new(knots).map_err(CodecError::malformed)?;
    let poles = match curve.pole_rows() {
        NurbsPoles3::Polynomial { points } => {
            let mut copied = reserve_optional_vec(ctx, points.len(), "iges solved curve copied poles")?;
            copied.extend_from_slice(points);
            NurbsPoles3::Polynomial { points: copied }
        }
        NurbsPoles3::Rational { points } => {
            let mut copied = reserve_optional_vec(ctx, points.len(), "iges solved curve copied weighted poles")?;
            copied.extend_from_slice(points);
            NurbsPoles3::Rational { points: copied }
        }
    };
    NurbsCurve::new(curve.degree(), knots, poles, curve.periodic()).map_err(CodecError::malformed)
}

fn copy_polyline(
    curve: &PolylineCurve,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<PolylineCurve, CodecError> {
    let count = curve.point_count();
    let samples = if let Some(parameters) = curve.parameters() {
        let mut vertices = reserve_optional_vec(ctx, count, "iges solved curve copied polyline samples")?;
        vertices.extend(parameters.zip(curve.points()).map(|(parameter, point)| PolylineVertex { parameter, point }));
        PolylineSamples::Parameterized {
            vertices: NonEmptyMembers::try_from(vertices).map_err(CodecError::malformed)?,
        }
    } else {
        let mut points = reserve_optional_vec(ctx, count, "iges solved curve copied polyline samples")?;
        points.extend(curve.points());
        PolylineSamples::Unparameterized {
            points: NonEmptyMembers::try_from(points).map_err(CodecError::malformed)?,
        }
    };
    PolylineCurve::from_checked_samples(samples, curve.chordal_deflection().get())
        .map_err(CodecError::malformed)
}

pub(super) fn copy_solved_curve(
    geometry: &SolvedCurveGeometry,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<SolvedCurveGeometry, CodecError> {
    Ok(match geometry {
        SolvedCurveGeometry::Line(value) => SolvedCurveGeometry::Line(*value),
        SolvedCurveGeometry::Circle(value) => SolvedCurveGeometry::Circle(*value),
        SolvedCurveGeometry::Ellipse(value) => SolvedCurveGeometry::Ellipse(*value),
        SolvedCurveGeometry::Parabola(value) => SolvedCurveGeometry::Parabola(*value),
        SolvedCurveGeometry::Hyperbola(value) => SolvedCurveGeometry::Hyperbola(*value),
        SolvedCurveGeometry::Degenerate(value) => SolvedCurveGeometry::Degenerate(*value),
        SolvedCurveGeometry::Nurbs(value) => SolvedCurveGeometry::Nurbs(copy_nurbs_curve(value, ctx)?),
        SolvedCurveGeometry::Polyline(value) => SolvedCurveGeometry::Polyline(copy_polyline(value, ctx)?),
        SolvedCurveGeometry::Composite { segments, self_intersect } => {
            let mut copied = reserve_optional_vec(ctx, segments.len(), "iges solved curve copied composite segments")?;
            for segment in segments.iter() {
                copied.push(cadmpeg_ir::geometry::CompositeCurveSegment {
                    curve: clone_optional_identity(ctx, &segment.curve, "iges solved curve copied composite ID")?,
                    same_sense: segment.same_sense,
                    transition: segment.transition,
                });
            }
            SolvedCurveGeometry::Composite {
                segments: CompositeCurveSegments::try_from(copied).map_err(CodecError::malformed)?,
                self_intersect: *self_intersect,
            }
        }
        SolvedCurveGeometry::Transformed(value) => {
            let _nested = ctx.map(|ctx| ctx.enter_nested("iges_solved_curve_copy")).transpose()?;
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, "iges solved curve copied placement box")?;
            }
            SolvedCurveGeometry::Transformed(
                PlacedCurve::try_new(Box::new(copy_solved_curve(value.basis(), ctx)?), *value.transform())
                    .map_err(CodecError::malformed)?,
            )
        }
        SolvedCurveGeometry::Unknown { record } => SolvedCurveGeometry::Unknown {
            record: record.as_ref().map(|id| clone_optional_identity(ctx, id, "iges solved curve copied unknown ID")).transpose()?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn solved_nurbs_copy_admits_knots_and_poles() {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        ).unwrap();
        let geometry = SolvedCurveGeometry::Nurbs(curve);
        for (cap, operation) in [
            (0, "iges solved curve copied knots"),
            (4, "iges solved curve copied poles"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = copy_solved_curve(&geometry, Some(&ctx));
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
        }
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(copy_solved_curve(&geometry, Some(&ctx)).unwrap(), geometry);
    }

    #[test]
    fn solved_polyline_copy_admits_sample_lane() {
        let samples = PolylineSamples::Unparameterized {
            points: NonEmptyMembers::try_from(vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            ]).unwrap(),
        };
        let geometry = SolvedCurveGeometry::Polyline(PolylineCurve::new(samples, 0.0).unwrap());
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(copy_solved_curve(&geometry, Some(&ctx)), Err(CodecError::ResourceLimit(limit)) if limit.operation == "iges solved curve copied polyline samples"));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(copy_solved_curve(&geometry, Some(&ctx)).unwrap(), geometry);
    }

    #[test]
    fn solved_composite_copy_admits_nested_curve_id() {
        let segment = cadmpeg_ir::geometry::CompositeCurveSegment {
            curve: CurveId::mint("test:model:curve#child").unwrap(),
            same_sense: true,
            transition: cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous,
        };
        let geometry = SolvedCurveGeometry::Composite {
            segments: CompositeCurveSegments::try_from(vec![segment]).unwrap(),
            self_intersect: None,
        };
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(copy_solved_curve(&geometry, Some(&ctx)), Err(CodecError::ResourceLimit(limit)) if limit.operation == "iges solved curve copied composite ID"));
    }
}
