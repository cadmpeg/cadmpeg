// SPDX-License-Identifier: Apache-2.0
//! Referenced construction carriers cannot become independent export roots.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};
use cadmpeg_ir::{geometry::SolvedCurveGeometry, schema::EntitySchema, CadIr};
use std::collections::BTreeSet;

pub(super) fn construction_supports(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<String>, ResourceLimit> {
    let mut supports = BTreeSet::new();
    let mut visit = |id: &str| -> Result<(), ResourceLimit> {
        if !supports.contains(id) {
            let id = id.to_owned();
            supports.insert(id);
        }
        Ok(())
    };
    for curve in &ir.model.curves {
        let Some(mut geometry) = curve.geometry.solved() else {
            continue;
        };
        while let SolvedCurveGeometry::Transformed(placed) = geometry {
            geometry = placed.basis();
        }
        if let SolvedCurveGeometry::Composite { segments, .. } = geometry {
            for segment in segments {
                visit(segment.curve.as_str())?;
            }
        }
    }
    for surface in &ir.model.procedural_surfaces {
        surface.visit_references(ctx, &mut visit)?;
    }
    for curve in &ir.model.procedural_curves {
        curve.visit_references(ctx, &mut visit)?;
    }
    for pcurve in &ir.model.pcurves {
        pcurve.visit_references(ctx, &mut visit)?;
    }
    Ok(supports)
}

/// Whether the carrier is an explicitly independent root: it keeps its own
/// root beside any construction that references it.
pub(super) fn is_independent(source: Option<&cadmpeg_ir::SourceObjectAssociation>) -> bool {
    source.and_then(|source| source.geometry_role)
        == Some(cadmpeg_ir::SourceGeometryRole::Independent)
}

#[cfg(test)]
mod tests {
    use super::construction_supports;
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension,
    };
    use cadmpeg_ir::geometry::pcurve::{
        LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata, PlacedPcurve,
    };
    use cadmpeg_ir::ids::PcurveId;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::transform::Transform2;

    #[test]
    fn construction_visitor_uses_the_supplied_recursion_limit() {
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut geometry = PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
        );
        for _ in 0..4 {
            geometry = PcurveGeometry::Transformed(
                PlacedPcurve::try_new(Box::new(geometry), Transform2::identity()).unwrap(),
            );
        }
        ir.model.pcurves.push(Pcurve {
            native_ref: None,
            id: PcurveId::mint("test:model:pcurve#nested").unwrap(),
            geometry,
            metadata: PcurveMetadata::default(),
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::desktop();
        policy.limits.max_recursion_depth = 1;
        let context = DecodeContext::new(&arena, &policy, DecodeDepth::Geometry);
        let limit = construction_supports(&ir, &context).expect_err("nesting exceeds limit");
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    }
}
