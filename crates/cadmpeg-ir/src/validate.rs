// SPDX-License-Identifier: Apache-2.0
//! Structural and numeric validation for [`CadIr`].
//!
//! Validation checks identity and arena order, references, topology rings,
//! carrier reachability, annotations, native links, parameter
//! domains, payload integrity, tessellation, numeric bounds, and geometric
//! consistency (edge-curve endpoints and pcurve surface images against vertex
//! positions). It does not evaluate interior surface membership or solid
//! closure.

use std::collections::{BTreeMap, HashSet};

use crate::document::{CadIr, CensusKey};
use crate::report::{
    check::{Check, Finding, ValidationReport},
    loss::LossNote,
    Severity,
};
use crate::source_fidelity::SourceFidelity;

/// Narrow admissibility predicates as documented `Check` subsets.
pub mod admit;
mod annotations_native;
mod carriers_parameterization;
mod drawings;
mod geometry_consistency;
mod geometry_payloads;
mod identity_order;
mod pmi;
mod presentation;
mod products;
mod referential_integrity;
mod semantic_annotations;
mod sketches;
mod spreadsheets;
mod topology;

use annotations_native::{check_annotations, check_native_links};
use carriers_parameterization::{check_carrier_reachability, check_parameter_domains};
use drawings::check_drawings;
use geometry_consistency::{
    check_edge_endpoint_consistency, check_pcurve_surface_consistency,
    check_procedural_support_consistency,
};
use geometry_payloads::check_tessellations;
use identity_order::check_identity_and_order;
use pmi::check_pmi;
use presentation::{check_appearances, check_presentation};
use products::check_products;
use referential_integrity::check_typed_references;
use semantic_annotations::check_semantic_annotations;
use sketches::check_sketches;
use spreadsheets::check_spreadsheets;
use topology::{
    check_coedge_pairing, check_references, check_shell_connectivity, check_tolerances,
    check_topology_tolerances, check_wire_topology,
};

/// The parameter interval a pcurve carrier is defined on, when the carrier
/// states one.
///
/// A nesting carrier states its own interval where it has one and otherwise
/// reports its basis's: a trim is the interval the trimmed carrier exists on,
/// and a placement and an offset both keep their basis's parameterization. An
/// analytic carrier has no bounded domain. The match is exhaustive, so a new
/// pcurve variant states its answer here rather than inheriting one.
///
/// Both the carrier-parameterization pass and the geometric-consistency pass
/// ask this question of the same carrier, so they ask it of one function.
///
/// The recursion is bounded by the carrier: each pcurve nesting constructor
/// refuses a chain past
/// [`MAX_GEOMETRY_NESTING`](crate::geometry::MAX_GEOMETRY_NESTING).
fn pcurve_parameter_domain(geometry: &crate::geometry::pcurve::PcurveGeometry) -> Option<[f64; 2]> {
    use crate::geometry::pcurve::PcurveGeometry;

    match geometry {
        PcurveGeometry::Nurbs { nurbs } => crate::eval::nurbs_pcurve_parameter_domain(
            nurbs.degree(),
            nurbs.knots(),
            nurbs.control_points().len(),
        )
        .map(crate::topology::IncreasingParameterInterval::endpoints),
        PcurveGeometry::PolarNurbs { nurbs } => crate::eval::nurbs_pcurve_parameter_domain(
            nurbs.degree(),
            nurbs.knots(),
            nurbs.poles().len(),
        )
        .map(crate::topology::IncreasingParameterInterval::endpoints),
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            if parameter_range.endpoints()[0] < parameter_range.endpoints()[1] {
                Some(parameter_range.endpoints())
            } else {
                pcurve_parameter_domain(trimmed_pcurve.basis())
            }
        }
        PcurveGeometry::Offset(offset_pcurve) => pcurve_parameter_domain(offset_pcurve.basis()),
        PcurveGeometry::Transformed(placed) => pcurve_parameter_domain(placed.basis()),
        PcurveGeometry::Line(_)
        | PcurveGeometry::Circle(_)
        | PcurveGeometry::Ellipse(_)
        | PcurveGeometry::Harmonic(_)
        | PcurveGeometry::Parabola(_)
        | PcurveGeometry::Hyperbola(_)
        | PcurveGeometry::Hyperbolic(_)
        | PcurveGeometry::PolarHarmonic(_)
        | PcurveGeometry::SphericalGreatCircle(_) => None,
    }
}

/// Record an error finding of `check` against one entity.
fn error_finding(findings: &mut Vec<Finding>, check: Check, entity: &str, message: &str) {
    findings.push(Finding {
        check,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}

/// Count the records represented by the IR arenas without running validation.
///
/// Prefer [`CadIr::census`](crate::CadIr::census); this alias remains for
/// existing `cadmpeg_ir::entity_census` call sites.
pub fn entity_census(ir: &CadIr) -> BTreeMap<CensusKey, usize> {
    crate::document::entity_census(ir)
}

/// Validate `ir` and copy `losses` into the returned report unchanged.
fn validate_model(ir: &CadIr, losses: Vec<LossNote>) -> ValidationReport {
    let index = crate::index::ModelIndex::new(ir);
    validate_model_with_index(ir, losses, &index)
}

fn validate_model_with_index(
    ir: &CadIr,
    losses: Vec<LossNote>,
    ids: &crate::index::ModelIndex<'_>,
) -> ValidationReport {
    let mut findings = Vec::new();

    // The identity walk enumerates every entity id in the product document;
    // native links resolve against that set.
    check_identity_and_order(ir, &mut findings);
    check_tolerances(ir, &mut findings);
    check_references(ir, ids, &mut findings);
    check_pmi(ir, &mut findings);
    check_coedge_pairing(ir, &mut findings);
    check_shell_connectivity(ir, &mut findings);
    check_wire_topology(ir, &mut findings);
    check_carrier_reachability(ir, &mut findings);
    check_native_links(ir, ids, &mut findings);
    check_parameter_domains(ir, &mut findings);
    check_edge_endpoint_consistency(ir, &mut findings);
    check_pcurve_surface_consistency(ir, &mut findings);
    check_procedural_support_consistency(ir, &mut findings);
    check_topology_tolerances(ir, &mut findings);
    check_tessellations(ir, &mut findings);
    check_sketches(ir, &mut findings);
    check_spreadsheets(ir, &mut findings);
    check_products(ir, &mut findings);
    check_presentation(ir, ids, &mut findings);
    check_appearances(ir, &mut findings);
    check_drawings(ir, ids, &mut findings);
    check_semantic_annotations(ir, ids, &mut findings);
    check_typed_references(ir, ids, &mut findings);

    ValidationReport {
        entity_counts: entity_census(ir),
        findings,
        losses,
    }
}

/// Validates a model while treating staged retained-record identities as native entities.
pub fn validate_neutral_with_additional_native_identities<'a>(
    ir: &'a CadIr,
    additional: impl IntoIterator<Item = &'a str>,
    losses: Vec<LossNote>,
) -> ValidationReport {
    let index = crate::index::ModelIndex::with_additional_native_identities(ir, additional);
    validate_model_with_index(ir, losses, &index)
}

/// Validate one neutral product model.
pub fn validate_neutral(ir: &CadIr, losses: Vec<LossNote>) -> ValidationReport {
    validate_model(ir, losses)
}

/// Validate one neutral product model together with borrowed annotations.
pub fn validate_neutral_with_annotations(
    ir: &CadIr,
    annotations: &crate::annotations::Annotations,
    losses: Vec<LossNote>,
) -> ValidationReport {
    let mut report = validate_model(ir, losses);
    let all_ids = crate::index::ModelIndex::new(ir)
        .identities()
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    check_annotations(ir, annotations, &all_ids, &mut report.findings);
    report
}

/// Validate a neutral product model together with its decode-time source sidecar.
pub fn validate_neutral_with_source_fidelity(
    ir: &CadIr,
    source_fidelity: &SourceFidelity,
    losses: Vec<LossNote>,
) -> ValidationReport {
    let mut report = validate_model(ir, losses);
    let index = crate::index::ModelIndex::new(ir);
    let mut all_ids = index
        .identities()
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    all_ids.extend(
        source_fidelity
            .retained_records()
            .keys()
            .map(|id| id.as_str().to_owned()),
    );
    check_annotations(
        ir,
        &source_fidelity.annotations,
        &all_ids,
        &mut report.findings,
    );
    report
}

#[cfg(test)]
mod tests {
    use super::{pcurve_parameter_domain, validate_neutral};
    use crate::features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, FaceSelection, Feature,
        FeatureDefinition, FeatureId, FeatureOperation, PrincipalPlane, SplitFaceTool,
    };
    use crate::geometry::pcurve::PcurveGeometry;
    use crate::math::{Point3, Vector3};
    use crate::sketches::{Sketch, SketchId};
    use crate::CadIr;
    use std::collections::BTreeMap;

    fn nurbs_pcurve_leaf() -> PcurveGeometry {
        PcurveGeometry::Nurbs {
            nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    crate::math::Point2::new(0.0, 0.0),
                    crate::math::Point2::new(1.0, 1.0),
                ],
                None,
                false,
            )
            .unwrap(),
        }
    }

    fn placed_pcurve(placements: usize) -> Result<PcurveGeometry, &'static str> {
        let mut geometry = nurbs_pcurve_leaf();
        for _ in 0..placements {
            geometry = PcurveGeometry::Transformed(crate::geometry::pcurve::PlacedPcurve::try_new(
                Box::new(geometry),
                crate::transform::Transform2::identity(),
            )?);
        }
        Ok(geometry)
    }

    #[test]
    fn pcurve_parameter_domain_stops_at_the_admitted_nesting_depth() {
        let accepted =
            placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING).expect("admitted nesting");
        assert!(pcurve_parameter_domain(&accepted).is_some());

        // The walk has no depth gate because the carrier one placement deeper
        // cannot be built: `PlacedPcurve::try_new` refuses it.
        assert_eq!(
            placed_pcurve(crate::geometry::MAX_GEOMETRY_NESTING + 1),
            Err("PlacedPcurve.basis nests past the admitted inline basis depth")
        );
    }

    #[test]
    fn configuration_feature_sketch_resolves_against_model_sketches() {
        let mut ir = CadIr::empty();
        let feature_id = FeatureId::mint("test:model:feature#sketch").expect("identity grammar");
        let sketch_id = SketchId::mint("test:model:sketch#sketch").unwrap();
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: crate::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),

            evaluation: crate::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: crate::features::SketchFeatureBinding::Unresolved,
                }),
            ),
            native_ref: None,
        });
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: crate::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: crate::sketches::SketchProfiles::default(),
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId::mint("test:model:configuration#default")
                .expect("identity grammar"),
            ordinal: 0,
            active: true,
            source_index: None,
            name: Some("Default".to_string()),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            bodies: Some(crate::features::DistinctMembers::default()),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                feature_id,
                ConfigurationFeatureState {
                    evaluation: crate::features::ConfigurationEvaluation::Active {
                        outputs: crate::features::DistinctMembers::default(),
                    },
                    dependencies: crate::features::DistinctMembers::default(),
                    definition: FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: crate::features::SketchFeatureBinding::Planar(Some(sketch_id)),
                    }),
                },
            )]),
            native_ref: None,
        });

        let report = validate_neutral(&ir, Vec::new());

        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }

    #[test]
    fn split_face_plane_sets_require_two_unique_plane_dependencies() {
        let feature = |id: FeatureId,
                       ordinal,
                       dependencies: Vec<FeatureId>,
                       definition: FeatureDefinition| Feature {
            id,
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies: (dependencies).try_into().unwrap(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: crate::features::FeatureContent::default(),

            evaluation: crate::features::FeatureEvaluation::from_definition(definition),
            native_ref: None,
        };
        let first = FeatureId::mint("test:model:feature#plane-a").expect("identity grammar");
        let second = FeatureId::mint("test:model:feature#plane-b").expect("identity grammar");
        let split = FeatureId::mint("test:model:feature#split").expect("identity grammar");
        let mut ir = CadIr::empty();
        ir.model.features = vec![
            feature(
                first.clone(),
                0,
                Vec::new(),
                FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                    plane: PrincipalPlane::Front,
                }),
            ),
            feature(
                second.clone(),
                1,
                Vec::new(),
                FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane {
                    plane: PrincipalPlane::Right,
                }),
            ),
            feature(
                split,
                2,
                vec![first.clone(), second.clone()],
                FeatureDefinition::Operation(FeatureOperation::SplitFace {
                    targets: FaceSelection::Unresolved,
                    tool: SplitFaceTool::Planes {
                        planes: vec![first.clone(), second.clone()].try_into().unwrap(),
                    },
                }),
            ),
        ];

        let report = validate_neutral(&ir, Vec::new());
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }
}
