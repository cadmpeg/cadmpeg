// SPDX-License-Identifier: Apache-2.0
//! Zero-entity decode route for independently complete geometry carriers.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, PcurveGeometry,
    ProceduralCurve, ProceduralCurveDefinition, Surface, SurfaceCurveFamily,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::assemble::{annotate, link_payload_carriers, preserve_raw_payload, source_meta};
use crate::container::{self, ContainerScan};
use crate::families::FamilyOutput;

pub(crate) fn try_decode_zero_entity(scan: &ContainerScan) -> Option<FamilyOutput> {
    let surfaces = crate::families::zero_entity::records::zero_entity_surfaces(&scan.data);
    if surfaces.is_empty() {
        return None;
    }
    let support_runs = crate::families::zero_entity::records::zero_entity_support_runs(&scan.data);
    let ownership_root =
        crate::families::zero_entity::records::zero_entity_ownership_root(&scan.data);

    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut unknowns,
        &mut annotations,
        scan,
        "catia:payload:unknown#zero-entity",
    );

    let mut surface_ids_by_position = HashMap::new();
    for (index, surface) in surfaces.into_iter().enumerate() {
        let id = SurfaceId(format!("catia:zero-entity:surf#{index}"));
        annotate(
            &mut annotations,
            &id,
            "zero_entity_a9_03",
            surface.pos as u64,
            "analytic_surface",
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: surface.geometry,
            source_object: None,
        });
        surface_ids_by_position.insert(surface.pos, id);
    }

    let mut transferred_support_curves = 0usize;
    let mut transferred_parametric_surface_curves = 0usize;
    for run in support_runs {
        let Some(surface) = surface_ids_by_position.get(&run.carrier_pos).cloned() else {
            continue;
        };
        for support in run.supports {
            let curve_id = CurveId(format!(
                "catia:zero-entity:support-curve#{}",
                support.record_ordinal
            ));
            if let Some(geometry) = support.model_curve {
                annotate(
                    &mut annotations,
                    &curve_id,
                    "zero_entity_a9_03",
                    support.pos as u64,
                    "support_model_curve",
                    Exactness::Derived,
                );
                annotations.derived(&curve_id, "geometry");
                ir.model.curves.push(Curve {
                    id: curve_id,
                    geometry,
                    source_object: None,
                });
                transferred_support_curves += 1;
                continue;
            }

            let (definition, role) = if let Some(definition) = support.model_curve_construction {
                (definition, "support_model_curve_construction")
            } else {
                let Some(pcurve) = support.pcurve else {
                    continue;
                };
                let PcurveGeometry::Nurbs {
                    degree,
                    knots,
                    control_points,
                    ..
                } = &pcurve
                else {
                    continue;
                };
                let Some(parameter_range) = usize::try_from(*degree)
                    .ok()
                    .and_then(|degree| {
                        Some([*knots.get(degree)?, *knots.get(control_points.len())?])
                    })
                    .filter(|range| {
                        range.iter().all(|value| value.is_finite()) && range[0] < range[1]
                    })
                else {
                    continue;
                };
                transferred_parametric_surface_curves += 1;
                (
                    ProceduralCurveDefinition::SurfaceCurve {
                        family: SurfaceCurveFamily::Parametric,
                        context: IntcurveSupportContext {
                            sides: [
                                IntcurveSupportSide {
                                    surface: Some(surface.clone()),
                                    pcurve: Some(pcurve),
                                    pcurve_parameter_range: None,
                                },
                                IntcurveSupportSide {
                                    surface: None,
                                    pcurve: None,
                                    pcurve_parameter_range: None,
                                },
                            ],
                            parameter_range,
                            discontinuities: std::array::from_fn(|_| Vec::new()),
                        },
                        tail: None,
                    },
                    "parametric_surface_curve",
                )
            };
            let construction_id = ProceduralCurveId(format!(
                "catia:zero-entity:support-curve-construction#{}",
                support.record_ordinal
            ));
            annotate(
                &mut annotations,
                &curve_id,
                "zero_entity_a9_03",
                support.pos as u64,
                role,
                Exactness::Derived,
            );
            annotations
                .derived(&curve_id, "geometry")
                .derived(&construction_id, "curve")
                .derived(&construction_id, "definition");
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Procedural {
                    construction: construction_id.clone(),
                },
                source_object: None,
            });
            ir.model.procedural_curves.push(ProceduralCurve {
                id: construction_id,
                curve: curve_id,
                definition,
                cache_fit_tolerance: None,
            });
            transferred_support_curves += 1;
        }
    }

    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let coverage = BTreeMap::from([
        (
            "transferred_zero_entity_support_curve_count".to_string(),
            transferred_support_curves,
        ),
        (
            "transferred_zero_entity_parametric_surface_curve_count".to_string(),
            transferred_parametric_surface_curves,
        ),
    ]);
    Some(FamilyOutput {
        ir,
        report: DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            coverage,
            losses: vec![LossNote {
                code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
                category: LossCategory::Topology,
                severity: Severity::Blocking,
                message: if ownership_root.is_some() {
                    "Zero-entity loop members bind their face-local support occurrences and the terminal ownership root binds the complete face roster through one shell and body, but support-to-oriented-use, oriented-use-to-incidence, and physical endpoint bindings remain unresolved; no neutral topology was transferred."
                } else {
                    "Zero-entity loop members bind their face-local support occurrences, but support-to-oriented-use, oriented-use-to-incidence, physical endpoint, and body/shell bindings remain unresolved; no neutral topology was transferred."
                }
                .to_string(),
                provenance: None,
            }],
            notes: container::summarize(scan).notes,
        },
        annotations: annotations.build(),
        unknowns,
    })
}
