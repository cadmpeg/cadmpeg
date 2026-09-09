// SPDX-License-Identifier: Apache-2.0
//! Structural and numeric validation for [`CadIr`].
//!
//! Validation checks identity and arena order, references, topology rings,
//! carrier reachability, annotations, native links, parameter
//! domains, payload integrity, tessellation, numeric bounds, and geometric
//! consistency (edge-curve endpoints and pcurve surface images against vertex
//! positions). It does not evaluate interior surface membership or solid
//! closure.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::document::{CadIr, CensusKey};
use crate::features::Feature;
use crate::geometry::{
    CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use crate::report::{Check, Finding, LossNote, Severity, ValidationReport};
use crate::source_fidelity::SourceFidelity;
use crate::topology::Coedge;

/// Frozen accept/reject IR builders for Phase 5 gate swaps.
pub mod admissibility_freeze;
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
use identity_order::{check_identity_and_order, collect_native_ids};
use pmi::check_pmi;
use presentation::check_presentation;
use products::check_products;
use referential_integrity::check_typed_references;
use semantic_annotations::check_semantic_annotations;
use sketches::check_sketches;
use spreadsheets::check_spreadsheets;
use topology::{
    check_coedge_pairing, check_references, check_shell_connectivity, check_tolerances,
    check_topology_tolerances, check_wire_topology,
};

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
    if let Err(error) = source_fidelity.validate() {
        report.findings.push(Finding {
            check: Check::PayloadIntegrity,
            severity: Severity::Error,
            message: format!("invalid source fidelity: {error}"),
            entity: None,
        });
    }
    let index = crate::index::ModelIndex::new(ir);
    let mut all_ids = index
        .identities()
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    all_ids.extend(
        source_fidelity
            .retained_records
            .iter()
            .map(|record| record.id().to_owned()),
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
    use super::validate_neutral;
    use crate::features::{
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, FaceSelection, Feature,
        FeatureDefinition, FeatureId, PrincipalPlane, SplitFaceTool,
    };
    use crate::math::{Point3, Vector3};
    use crate::sketches::{Sketch, SketchId};
    use crate::CadIr;
    use std::collections::BTreeMap;

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
                FeatureDefinition::Sketch {
                    sketch: crate::features::SketchFeatureBinding::Unresolved,
                },
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
            name: "Default".into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            bodies: crate::features::ConfigurationBodies::Resolved(
                crate::features::DistinctMembers::default(),
            ),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                feature_id,
                ConfigurationFeatureState {
                    evaluation: crate::features::ConfigurationEvaluation::Active {
                        outputs: crate::features::DistinctMembers::default(),
                    },
                    dependencies: crate::features::DistinctMembers::default(),
                    definition: FeatureDefinition::Sketch {
                        sketch: crate::features::SketchFeatureBinding::Planar(Some(sketch_id)),
                    },
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
                FeatureDefinition::DatumPrincipalPlane {
                    plane: PrincipalPlane::Front,
                },
            ),
            feature(
                second.clone(),
                1,
                Vec::new(),
                FeatureDefinition::DatumPrincipalPlane {
                    plane: PrincipalPlane::Right,
                },
            ),
            feature(
                split,
                2,
                vec![first.clone(), second.clone()],
                FeatureDefinition::SplitFace {
                    targets: FaceSelection::Unresolved,
                    tool: SplitFaceTool::Planes {
                        planes: vec![first.clone(), second.clone()].try_into().unwrap(),
                    },
                },
            ),
        ];

        let report = validate_neutral(&ir, Vec::new());
        assert!(report.findings.is_empty(), "{:?}", report.findings);
    }
}
