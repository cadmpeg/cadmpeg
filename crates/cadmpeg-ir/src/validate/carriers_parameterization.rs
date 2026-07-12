// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for carriers parameterization.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;

pub(super) fn check_carrier_reachability(ir: &CadIr, findings: &mut Vec<Finding>) {
    let mut surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.0.as_str())
        .collect::<HashSet<_>>();
    let mut curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.as_ref().map(|id| id.0.as_str()))
        .collect::<HashSet<_>>();
    surfaces.extend(
        ir.model
            .surfaces
            .iter()
            .filter(|surface| surface.source_object.is_some())
            .map(|surface| surface.id.0.as_str()),
    );
    curves.extend(
        ir.model
            .curves
            .iter()
            .filter(|curve| curve.source_object.is_some())
            .map(|curve| curve.id.0.as_str()),
    );
    let pcurves = ir
        .model
        .coedges
        .iter()
        .filter_map(|coedge| coedge.pcurve.as_ref().map(|id| id.0.as_str()))
        .collect::<HashSet<_>>();
    let points = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.0.as_str())
        .collect::<HashSet<_>>();

    for procedural in &ir.model.procedural_surfaces {
        surfaces.insert(&procedural.surface.0);
        match &procedural.definition {
            ProceduralSurfaceDefinition::Exact { .. } => {}
            ProceduralSurfaceDefinition::Compound { components, .. } => {
                surfaces.extend(components.iter().map(|component| component.0.as_str()));
            }
            ProceduralSurfaceDefinition::Taper {
                support, reference, ..
            } => {
                surfaces.insert(&support.0);
                curves.insert(&reference.0);
            }
            ProceduralSurfaceDefinition::Loft { sections, .. } => {
                for entry in sections.iter().flat_map(|section| &section.entries) {
                    curves.insert(&entry.path.curve.0);
                    curves.extend(entry.path.auxiliaries.iter().map(|curve| curve.0.as_str()));
                    for member in &entry.profile {
                        curves.insert(&member.curve.0);
                        surfaces.insert(&member.data.surface.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::G2Blend { construction } => {
                for side in [&construction.first, &construction.second] {
                    surfaces.insert(&side.surface.0);
                    curves.insert(&side.curve.0);
                }
                surfaces.insert(&construction.second_exact_surface.0);
                curves.insert(&construction.center_curve.0);
                if let crate::geometry::G2BlendFirstShape::Full {
                    surface: Some(surface),
                    ..
                } = &construction.first_shape
                {
                    surfaces.insert(&surface.0);
                }
            }
            ProceduralSurfaceDefinition::Extrusion { directrix, .. }
            | ProceduralSurfaceDefinition::Revolution { directrix, .. } => {
                curves.insert(&directrix.0);
            }
            ProceduralSurfaceDefinition::Sweep { profile, spine } => {
                curves.extend([profile.0.as_str(), spine.0.as_str()]);
            }
            ProceduralSurfaceDefinition::Offset { support, .. } => {
                surfaces.insert(&support.0);
            }
            ProceduralSurfaceDefinition::Ruled { first, second } => {
                curves.extend([first.0.as_str(), second.0.as_str()]);
            }
            ProceduralSurfaceDefinition::Sum { first, second, .. } => {
                curves.extend([first.0.as_str(), second.0.as_str()]);
            }
            ProceduralSurfaceDefinition::Blend {
                supports, spine, ..
            } => {
                for support in supports.iter().flatten() {
                    surfaces.insert(&support.surface.0);
                }
                if let Some(spine) = spine {
                    curves.insert(&spine.0);
                }
            }
            ProceduralSurfaceDefinition::Unknown { .. } => {}
        }
    }
    for procedural in &ir.model.procedural_curves {
        curves.insert(&procedural.curve.0);
        match &procedural.definition {
            ProceduralCurveDefinition::Exact | ProceduralCurveDefinition::Helix { .. } => {}
            ProceduralCurveDefinition::Compound { components, .. } => {
                curves.extend(components.iter().map(|component| component.0.as_str()));
            }
            ProceduralCurveDefinition::Intersection { context } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } => {
                for side in context.sides.iter().chain(std::iter::once(third)) {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Silhouette {
                context,
                cast_surface,
                ..
            } => {
                surfaces.insert(&cast_surface.0);
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset { context, base, .. } => {
                curves.insert(&base.0);
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Spring { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Deformable { bend, data, .. } => {
                curves.insert(&bend.0);
                if let crate::geometry::DeformableCurveData::Surface { surface } = data {
                    surfaces.insert(&surface.0);
                }
            }
            ProceduralCurveDefinition::Projection {
                context, source, ..
            } => {
                curves.insert(&source.0);
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Offset {
                source, support, ..
            } => {
                curves.insert(&source.0);
                if let Some(support) = support {
                    surfaces.insert(&support.0);
                }
            }
            ProceduralCurveDefinition::TwoSidedOffset { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.0.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset { source, .. } => {
                curves.insert(&source.0);
            }
            ProceduralCurveDefinition::Subset { source, .. } => {
                curves.insert(&source.0);
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    surfaces.insert(&surface.0);
                }
            }
            ProceduralCurveDefinition::Unknown { .. } => {}
        }
    }
    for link in ir.unknowns.iter().flat_map(|record| &record.links) {
        surfaces.insert(link);
        curves.insert(link);
    }

    for (kind, id) in ir
        .model
        .surfaces
        .iter()
        .filter(|entity| !surfaces.contains(entity.id.0.as_str()))
        .map(|entity| ("surface", entity.id.0.as_str()))
        .chain(
            ir.model
                .curves
                .iter()
                .filter(|entity| !curves.contains(entity.id.0.as_str()))
                .map(|entity| ("curve", entity.id.0.as_str())),
        )
        .chain(
            ir.model
                .pcurves
                .iter()
                .filter(|entity| !pcurves.contains(entity.id.0.as_str()))
                .map(|entity| ("pcurve", entity.id.0.as_str())),
        )
        .chain(
            ir.model
                .points
                .iter()
                .filter(|entity| !points.contains(entity.id.0.as_str()))
                .map(|entity| ("point", entity.id.0.as_str())),
        )
    {
        findings.push(Finding {
            check: Check::CarrierReachability,
            severity: Severity::Error,
            message: format!("orphan {kind} carrier"),
            entity: Some(id.into()),
        });
    }
}

pub(super) fn check_parameter_domains(ir: &CadIr, findings: &mut Vec<Finding>) {
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.0.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    for edge in &ir.model.edges {
        let Some([start, end]) = edge.param_range else {
            continue;
        };
        let mut valid = start.is_finite() && end.is_finite() && start <= end;
        if let Some(curve) = edge.curve.as_ref().and_then(|id| curves.get(id.0.as_str())) {
            let tau = std::f64::consts::TAU;
            match curve {
                CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
                    // Canonical periodic domain: the start angle wrapped into
                    // one turn, the sweep at most a full turn. An arc crossing
                    // the seam ends past `τ`.
                    valid &= (0.0..tau).contains(&start) && end - start <= tau;
                }
                CurveGeometry::Nurbs(nurbs) => {
                    if let (Some(first), Some(last)) = (nurbs.knots.first(), nurbs.knots.last()) {
                        valid &= start >= *first && end <= *last;
                    }
                }
                _ => {}
            }
        }
        if !valid {
            findings.push(Finding {
                check: Check::ParameterDomain,
                severity: Severity::Error,
                message: "edge parameter range is outside its canonical carrier domain".into(),
                entity: Some(edge.id.0.clone()),
            });
        }
    }
}
