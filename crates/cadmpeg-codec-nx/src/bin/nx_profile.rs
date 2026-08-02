// SPDX-License-Identifier: Apache-2.0
//! Generate deterministic, conservative NX capability-gate evidence.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use cadmpeg_codec_nx::NxCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
use cadmpeg_ir::report::LossCategory;
use cadmpeg_ir::{CadIr, Severity};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct Profile {
    #[serde(rename = "profile_version")]
    version: u32,
    format: &'static str,
    fixtures: Vec<FixtureEvidence>,
    totals: EntityCounts,
    loss_codes: BTreeMap<String, usize>,
    gates: Vec<Gate>,
    highest_passing_gate: Option<String>,
}

#[derive(Debug, Serialize)]
struct FixtureEvidence {
    filename: String,
    deterministic: bool,
    native_namespace_version: Option<u32>,
    entities: EntityCounts,
    losses: BTreeMap<LossCategory, usize>,
    loss_codes: BTreeMap<String, usize>,
    validation_errors: usize,
    all_bodies_colored: bool,
    all_faces_colored: bool,
}

#[derive(Debug, Default, Serialize)]
struct EntityCounts {
    bodies: usize,
    faces: usize,
    edges: usize,
    vertices: usize,
    features: usize,
    sketches: usize,
    sketch_entities: usize,
    sketch_constraints: usize,
    parameters: usize,
    configurations: usize,
}

impl EntityCounts {
    fn from_ir(ir: &CadIr) -> Self {
        Self {
            bodies: ir.model.bodies.len(),
            faces: ir.model.faces.len(),
            edges: ir.model.edges.len(),
            vertices: ir.model.vertices.len(),
            features: ir.model.features.len(),
            sketches: ir.model.sketches.len(),
            sketch_entities: ir.model.sketch_entities.len(),
            sketch_constraints: ir.model.sketch_constraints.len(),
            parameters: ir.model.parameters.len(),
            configurations: ir.model.configurations.len(),
        }
    }

    fn add(&mut self, other: &Self) {
        self.bodies += other.bodies;
        self.faces += other.faces;
        self.edges += other.edges;
        self.vertices += other.vertices;
        self.features += other.features;
        self.sketches += other.sketches;
        self.sketch_entities += other.sketch_entities;
        self.sketch_constraints += other.sketch_constraints;
        self.parameters += other.parameters;
        self.configurations += other.configurations;
    }
}

#[derive(Debug, Serialize)]
struct Gate {
    level: String,
    passed: bool,
    assertions: Vec<Assertion>,
}

#[derive(Debug, Serialize)]
struct Assertion {
    id: &'static str,
    passed: bool,
    observed: String,
    required: &'static str,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let fixture_directory = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: nx-profile FIXTURE_DIRECTORY OUTPUT_JSON")?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: nx-profile FIXTURE_DIRECTORY OUTPUT_JSON")?;
    if arguments.next().is_some() {
        return Err("usage: nx-profile FIXTURE_DIRECTORY OUTPUT_JSON".into());
    }

    let mut paths = fs::read_dir(fixture_directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("prt"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() {
        return Err("fixture directory contains no .prt files".into());
    }

    let mut fixtures = Vec::new();
    let mut totals = EntityCounts::default();
    let mut total_loss_codes = BTreeMap::new();
    for path in paths {
        let bytes = fs::read(&path)?;
        let first = NxCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())?;
        let second = NxCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())?;
        let entities = EntityCounts::from_ir(&first.ir);
        totals.add(&entities);
        let mut losses = BTreeMap::new();
        let mut loss_codes = BTreeMap::new();
        for loss in &first.report.losses {
            if loss.severity >= Severity::Warning {
                *losses.entry(loss.code.category()).or_insert(0) += 1;
                *loss_codes
                    .entry(loss.code.as_str().to_string())
                    .or_insert(0) += 1;
                *total_loss_codes
                    .entry(loss.code.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }
        let validation_errors = cadmpeg_ir::validate(&first.ir, Vec::new())
            .findings
            .iter()
            .filter(|finding| finding.severity >= Severity::Error)
            .count();
        fixtures.push(FixtureEvidence {
            filename: path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("fixture filename is not UTF-8")?
                .to_string(),
            deterministic: first.ir.to_canonical_json()? == second.ir.to_canonical_json()?,
            native_namespace_version: first
                .ir
                .native
                .namespace("nx")
                .map(|namespace| namespace.version),
            all_bodies_colored: !first.ir.model.bodies.is_empty()
                && first
                    .ir
                    .model
                    .bodies
                    .iter()
                    .all(|body| body.color.is_some()),
            all_faces_colored: !first.ir.model.faces.is_empty()
                && first.ir.model.faces.iter().all(|face| face.color.is_some()),
            entities,
            losses,
            loss_codes,
            validation_errors,
        });
    }

    let gates = capability_gates(&fixtures);
    let highest_passing_gate = gates
        .iter()
        .take_while(|gate| gate.passed)
        .last()
        .map(|gate| gate.level.clone());
    let profile = Profile {
        version: 2,
        format: "nx",
        fixtures,
        totals,
        loss_codes: total_loss_codes,
        gates,
        highest_passing_gate,
    };
    let mut json = serde_json::to_string_pretty(&profile)?;
    json.push('\n');
    fs::write(output, json)?;
    Ok(())
}

fn capability_gates(fixtures: &[FixtureEvidence]) -> Vec<Gate> {
    let deterministic = fixtures
        .iter()
        .filter(|fixture| fixture.deterministic)
        .count();
    let valid_topology = fixtures
        .iter()
        .filter(|fixture| {
            fixture.validation_errors == 0
                && fixture
                    .losses
                    .get(&LossCategory::Topology)
                    .copied()
                    .unwrap_or(0)
                    == 0
        })
        .count();
    let without = |category| {
        fixtures
            .iter()
            .filter(|fixture| fixture.losses.get(&category).copied().unwrap_or(0) == 0)
            .count()
    };
    let body_colors = fixtures
        .iter()
        .filter(|fixture| fixture.entities.bodies == 0 || fixture.all_bodies_colored)
        .count();
    let face_colors = fixtures
        .iter()
        .filter(|fixture| fixture.entities.faces == 0 || fixture.all_faces_colored)
        .count();
    let total = fixtures.len();
    let assertion = |id, count, required| Assertion {
        id,
        passed: count == total,
        observed: format!("{count}/{total} fixtures"),
        required,
    };
    let rows = vec![
        (
            "L0",
            vec![assertion(
                "deterministic_decode",
                deterministic,
                "every fixture decodes deterministically",
            )],
        ),
        (
            "L1",
            vec![assertion(
                "closed_container_navigation",
                without(LossCategory::Other),
                "no fixture reports a container or unsupported-content loss",
            )],
        ),
        (
            "L2",
            vec![assertion(
                "complete_geometry",
                without(LossCategory::Geometry),
                "no fixture reports a geometry loss",
            )],
        ),
        (
            "L3",
            vec![assertion(
                "valid_connected_topology",
                valid_topology,
                "every fixture validates and reports no topology loss",
            )],
        ),
        (
            "L4",
            vec![assertion(
                "complete_design_records",
                without(LossCategory::DesignIntent),
                "no fixture reports a design-intent loss",
            )],
        ),
        (
            "L5",
            vec![
                assertion(
                    "body_appearance",
                    body_colors,
                    "every decoded body has a color",
                ),
                assertion(
                    "face_appearance",
                    face_colors,
                    "every decoded face has a color",
                ),
                assertion(
                    "complete_attributes",
                    without(LossCategory::Attribute),
                    "no fixture reports an attribute loss",
                ),
                assertion(
                    "complete_materials",
                    without(LossCategory::Material),
                    "no fixture reports a material loss",
                ),
            ],
        ),
        (
            "L6",
            vec![assertion(
                "design_domain_loss_empty",
                without(LossCategory::DesignIntent),
                "no fixture reports a design-intent loss",
            )],
        ),
    ];
    let mut lower_passed = true;
    rows.into_iter()
        .map(|(level, assertions)| {
            let passed = lower_passed && assertions.iter().all(|assertion| assertion.passed);
            lower_passed = passed;
            Gate {
                level: level.to_string(),
                passed,
                assertions,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> FixtureEvidence {
        FixtureEvidence {
            filename: "fixture.prt".to_string(),
            deterministic: true,
            native_namespace_version: Some(181),
            entities: EntityCounts::default(),
            losses: BTreeMap::new(),
            loss_codes: BTreeMap::new(),
            validation_errors: 0,
            all_bodies_colored: false,
            all_faces_colored: false,
        }
    }

    #[test]
    fn gates_are_cumulative() {
        let mut evidence = fixture();
        evidence.losses.insert(LossCategory::Geometry, 1);
        let gates = capability_gates(&[evidence]);

        assert!(gates[0].passed);
        assert!(gates[1].passed);
        assert!(!gates[2].passed);
        assert!(gates[3..].iter().all(|gate| !gate.passed));
    }

    #[test]
    fn topology_gate_requires_validation_and_loss_closure_in_each_fixture() {
        let mut invalid = fixture();
        invalid.validation_errors = 1;
        let mut lossy = fixture();
        lossy.losses.insert(LossCategory::Topology, 1);

        let gates = capability_gates(&[invalid, lossy]);
        let assertion = &gates[3].assertions[0];
        assert_eq!(assertion.observed, "0/2 fixtures");
        assert!(!gates[3].passed);
    }
}
