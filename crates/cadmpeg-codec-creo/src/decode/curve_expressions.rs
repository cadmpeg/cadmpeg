// SPDX-License-Identifier: Apache-2.0
//! Curve-from-equation feature transfer and assignment parameter ordering.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, DesignParameter, Feature, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, FeatureSourceContent, Length, ParameterId, ParameterValue,
};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;

use super::coverage::source_section;
use super::native::annotate;

pub(crate) fn curve_expression_record_id(record: &crate::curve::CurveExpressionRecord) -> String {
    format!(
        "creo:depdb:curve_expression#{}-{}-{}",
        if record.backup { "backup" } else { "active" },
        record.entity_id,
        record.offset
    )
}

const EPS_HELIX_BASIS_LENGTH: f64 = 1e-12;
const EPS_HELIX_BASIS_ORIGIN: f64 = 1e-12;
const EPS_HELIX_UV_EQUAL: f64 = 1e-9;
const EPS_HELIX_UV_ORTHO: f64 = 1e-9;

pub(crate) fn curve_expression_helix_definition(
    record: &crate::curve::CurveExpressionRecord,
) -> Option<ProceduralCurveDefinition> {
    let helix = crate::curve::expression_helix(record)?;
    let slots = record.local_system.as_ref()?.explicit_slots?;
    let u = Vector3::new(slots[0], slots[1], slots[2]);
    let v = Vector3::new(slots[6], slots[7], slots[8]);
    let u_norm = u.norm();
    let v_norm = v.norm();
    let scale = u_norm.max(v_norm).max(1.0);
    if !u_norm.is_finite()
        || !v_norm.is_finite()
        || u_norm <= EPS_HELIX_BASIS_LENGTH
        || v_norm <= EPS_HELIX_BASIS_LENGTH
        || (u_norm - v_norm).abs() > EPS_HELIX_UV_EQUAL * scale
        || (u.x * v.x + u.y * v.y + u.z * v.z).abs() > EPS_HELIX_UV_ORTHO * u_norm * v_norm
        || slots[3..6]
            .iter()
            .any(|value| value.abs() > EPS_HELIX_BASIS_ORIGIN)
    {
        return None;
    }
    let u = Vector3::new(u.x / u_norm, u.y / u_norm, u.z / u_norm);
    let v = Vector3::new(v.x / v_norm, v.y / v_norm, v.z / v_norm);
    let axis = Vector3::new(
        u.y * v.z - u.z * v.y,
        u.z * v.x - u.x * v.z,
        u.x * v.y - u.y * v.x,
    );
    slots[9..12]
        .iter()
        .all(|value| value.is_finite())
        .then_some(())?;
    let origin = Point3::new(slots[9], slots[10], slots[11]);
    let (sin, cos) = helix.start_angle.sin_cos();
    let major_direction = Vector3::new(
        u.x * cos + v.x * sin,
        u.y * cos + v.y * sin,
        u.z * cos + v.z * sin,
    );
    let tangent_direction = Vector3::new(
        -u.x * sin + v.x * cos,
        -u.y * sin + v.y * cos,
        -u.z * sin + v.z * cos,
    );
    let minor_direction = if helix.clockwise {
        Vector3::new(
            -tangent_direction.x,
            -tangent_direction.y,
            -tangent_direction.z,
        )
    } else {
        tangent_direction
    };
    Some(ProceduralCurveDefinition::Helix {
        angle_range: [0.0, helix.revolutions * std::f64::consts::TAU],
        center: Point3::new(
            origin.x + axis.x * helix.z_start,
            origin.y + axis.y * helix.z_start,
            origin.z + axis.z * helix.z_start,
        ),
        major: Vector3::new(
            major_direction.x * helix.radius,
            major_direction.y * helix.radius,
            major_direction.z * helix.radius,
        ),
        minor: Vector3::new(
            minor_direction.x * helix.radius,
            minor_direction.y * helix.radius,
            minor_direction.z * helix.radius,
        ),
        pitch: Vector3::new(
            axis.x * helix.height / helix.revolutions,
            axis.y * helix.height / helix.revolutions,
            axis.z * helix.height / helix.revolutions,
        ),
        apex_factor: 0.0,
        axis,
    })
}

pub(crate) fn expression_dependency_reaches(
    dependencies: &[Vec<usize>],
    start: usize,
    target: usize,
) -> bool {
    let mut pending = vec![start];
    let mut visited = BTreeSet::new();
    while let Some(index) = pending.pop() {
        if index == target {
            return true;
        }
        if visited.insert(index) {
            pending.extend(dependencies[index].iter().copied());
        }
    }
    false
}

pub(crate) fn curve_expression_parameter_order(
    record: &crate::curve::CurveExpressionRecord,
    unique_assignment_indices: &BTreeMap<String, usize>,
) -> (Vec<u32>, BTreeSet<(usize, usize)>) {
    let dependencies = record
        .assignments
        .iter()
        .map(|assignment| {
            let mut seen = BTreeSet::new();
            assignment
                .dependencies
                .iter()
                .filter_map(|name| {
                    unique_assignment_indices
                        .get(&crate::curve::expression_identifier_key(name))
                        .copied()
                })
                .filter(|index| seen.insert(*index))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut cyclic_edges = BTreeSet::new();
    for (consumer, dependency_indices) in dependencies.iter().enumerate() {
        for &dependency in dependency_indices {
            if expression_dependency_reaches(&dependencies, dependency, consumer) {
                cyclic_edges.insert((consumer, dependency));
            }
        }
    }
    let mut ordinals = vec![u32::MAX; dependencies.len()];
    for ordinal in 0..dependencies.len() {
        let index = (0..dependencies.len())
            .find(|&candidate| {
                ordinals[candidate] == u32::MAX
                    && dependencies[candidate].iter().all(|dependency| {
                        cyclic_edges.contains(&(candidate, *dependency))
                            || ordinals[*dependency] != u32::MAX
                    })
            })
            .expect("removing cyclic edges leaves an acyclic assignment graph");
        ordinals[index] = ordinal as u32;
    }
    (ordinals, cyclic_edges)
}

pub(crate) fn curve_expression_parameter_names(
    assignments: &[crate::curve::CurveExpressionAssignment],
) -> Vec<Option<String>> {
    let counts = assignments
        .iter()
        .fold(BTreeMap::new(), |mut counts, assignment| {
            if let Some((name, _)) = assignment.parameter_target() {
                *counts
                    .entry(crate::curve::expression_identifier_key(name))
                    .or_insert(0usize) += 1;
            }
            counts
        });
    let mut occurrences = BTreeMap::new();
    assignments
        .iter()
        .map(|assignment| {
            let (name, _) = assignment.parameter_target()?;
            let key = crate::curve::expression_identifier_key(name);
            if counts[&key] == 1 {
                return Some(name.to_owned());
            }
            let occurrence = occurrences.entry(key).or_insert(0usize);
            *occurrence += 1;
            Some(format!("{name}#{occurrence}"))
        })
        .collect()
}

pub(crate) fn transfer_curve_expression_features(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    dimension_parameters: &BTreeMap<String, ParameterId>,
) -> usize {
    let ordinal_base = ir
        .model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(0, |value| value + 1);
    let mut transferred_parameter_count = 0;
    for (expression_ordinal, record) in scan
        .curves
        .expressions
        .iter()
        .filter(|record| !record.backup)
        .enumerate()
    {
        let source_section = source_section(scan, record.offset);
        let ordinal = ordinal_base + expression_ordinal as u64;
        let feature_id = IrFeatureId(format!(
            "creo:depdb:curve_expression_feature#{}-{}",
            record.entity_id, record.offset
        ));
        let mut assignment_indices_by_name = BTreeMap::<String, Option<usize>>::new();
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            if assignment.activation == crate::curve::CurveExpressionActivation::Inactive {
                continue;
            }
            let Some((name, _)) = assignment.parameter_target() else {
                continue;
            };
            assignment_indices_by_name
                .entry(crate::curve::expression_identifier_key(name))
                .and_modify(|index| *index = None)
                .or_insert(Some(assignment_ordinal));
        }
        let unique_assignment_indices = assignment_indices_by_name
            .iter()
            .filter_map(|(name, index)| index.map(|index| (name.clone(), index)))
            .collect::<BTreeMap<_, _>>();
        let (parameter_ordinals, cyclic_edges) =
            curve_expression_parameter_order(record, &unique_assignment_indices);
        let parameter_names = curve_expression_parameter_names(&record.assignments);
        let mut emitted_assignment_indices = record
            .assignments
            .iter()
            .enumerate()
            .filter_map(|(index, assignment)| assignment.parameter_target().map(|_| index))
            .collect::<Vec<_>>();
        emitted_assignment_indices.sort_by_key(|index| parameter_ordinals[*index]);
        let emitted_ordinals = emitted_assignment_indices
            .into_iter()
            .enumerate()
            .map(|(ordinal, index)| (index, ordinal as u32))
            .collect::<BTreeMap<_, _>>();
        let mut source_content = Vec::with_capacity(emitted_ordinals.len());
        for (assignment_ordinal, assignment) in record.assignments.iter().enumerate() {
            let Some((assignment_name, declared_unit)) = assignment.parameter_target() else {
                continue;
            };
            let Some(&ordinal) = emitted_ordinals.get(&assignment_ordinal) else {
                continue;
            };
            let parameter_id = ParameterId(format!(
                "creo:depdb:curve_expression_parameter#{}-{}-{}",
                record.entity_id, record.offset, assignment_ordinal
            ));
            let mut dependencies = assignment
                .dependencies
                .iter()
                .filter_map(|name| {
                    unique_assignment_indices
                        .get(&crate::curve::expression_identifier_key(name))
                        .copied()
                })
                .filter(|dependency| !cyclic_edges.contains(&(assignment_ordinal, *dependency)))
                .scan(BTreeSet::new(), |seen, dependency| {
                    seen.insert(dependency).then_some(dependency)
                })
                .map(|dependency| {
                    ParameterId(format!(
                        "creo:depdb:curve_expression_parameter#{}-{}-{}",
                        record.entity_id, record.offset, dependency
                    ))
                })
                .collect::<Vec<_>>();
            dependencies.extend(assignment.dependencies.iter().filter_map(|name| {
                let key = crate::curve::expression_identifier_key(name);
                if assignment_indices_by_name.contains_key(&key) {
                    None
                } else {
                    dimension_parameters.get(&key).cloned()
                }
            }));
            let external_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| {
                    let key = crate::curve::expression_identifier_key(name);
                    key != "t"
                        && !assignment_indices_by_name.contains_key(&key)
                        && !dimension_parameters.contains_key(&key)
                })
                .cloned()
                .collect::<Vec<_>>();
            let ambiguous_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| {
                    matches!(
                        assignment_indices_by_name
                            .get(&crate::curve::expression_identifier_key(name)),
                        Some(None)
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            let intrinsic_dependencies = assignment
                .dependencies
                .iter()
                .filter(|name| crate::curve::expression_identifier_key(name) == "t")
                .cloned()
                .collect::<Vec<_>>();
            let mut properties = BTreeMap::new();
            if !external_dependencies.is_empty() {
                properties.insert(
                    "external_dependencies".to_string(),
                    external_dependencies.join(","),
                );
            }
            if !ambiguous_dependencies.is_empty() {
                properties.insert(
                    "ambiguous_dependencies".to_string(),
                    ambiguous_dependencies.join(","),
                );
            }
            properties.insert(
                "source_assignment_ordinal".to_string(),
                assignment_ordinal.to_string(),
            );
            properties.insert(
                "activation".to_string(),
                assignment.activation.token().to_string(),
            );
            if let Some(unit) = declared_unit {
                properties.insert("declared_unit".to_string(), unit.to_owned());
            }
            if let Some(crate::curve::CurveExpressionValue::Quantity(quantity)) = &assignment.value
            {
                properties.insert(
                    "evaluated_canonical_value".to_string(),
                    quantity.value.to_string(),
                );
                properties.insert(
                    "evaluated_dimension".to_string(),
                    format!(
                        "length:{},mass:{},time:{},angle:{},temperature:{}",
                        quantity.length_power,
                        quantity.mass_power,
                        quantity.time_power,
                        quantity.angle_power,
                        quantity.temperature_power
                    ),
                );
            }
            let parameter_name = parameter_names[assignment_ordinal]
                .as_ref()
                .expect("emitted parameter assignment has a parameter name");
            if parameter_name != assignment_name {
                properties.insert("source_name".to_string(), assignment_name.to_owned());
            }
            if !intrinsic_dependencies.is_empty() {
                properties.insert(
                    "independent_variables".to_string(),
                    intrinsic_dependencies.join(","),
                );
            }
            let cyclic_dependencies = assignment
                .dependencies
                .iter()
                .filter_map(|name| {
                    let key = crate::curve::expression_identifier_key(name);
                    unique_assignment_indices
                        .get(&key)
                        .filter(|dependency| {
                            cyclic_edges.contains(&(assignment_ordinal, **dependency))
                        })
                        .map(|_| name.clone())
                })
                .collect::<BTreeSet<_>>();
            if !cyclic_dependencies.is_empty() {
                properties.insert(
                    "cyclic_dependencies".to_string(),
                    cyclic_dependencies
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            annotate(
                annotations,
                &parameter_id.0,
                &source_section,
                assignment.offset as u64,
                "curve_expression_assignment",
                Exactness::Derived,
            );
            ir.model.parameters.push(DesignParameter {
                id: parameter_id.clone(),
                owner: Some(feature_id.clone()),
                ordinal,
                name: parameter_name.clone(),
                expression: assignment.expression.clone(),
                display: None,
                value: assignment.value.as_ref().and_then(|value| match value {
                    crate::curve::CurveExpressionValue::Number(value) => {
                        Some(ParameterValue::Real(*value))
                    }
                    crate::curve::CurveExpressionValue::Length(value) => {
                        Some(ParameterValue::Length(cadmpeg_ir::features::Length(*value)))
                    }
                    crate::curve::CurveExpressionValue::Angle(value) => Some(
                        ParameterValue::Angle(cadmpeg_ir::features::Angle(value.to_radians())),
                    ),
                    crate::curve::CurveExpressionValue::Quantity(_) => None,
                    crate::curve::CurveExpressionValue::String(value) => {
                        Some(ParameterValue::String(value.clone()))
                    }
                }),
                dependencies,
                properties,
                pmi: None,
                native_ref: Some(curve_expression_record_id(record)),
            });
            transferred_parameter_count += 1;
            source_content.push(FeatureSourceContent::Parameter(parameter_id.clone()));
        }
        annotate(
            annotations,
            &feature_id.0,
            &source_section,
            record.expression_offset as u64,
            "curve_expression_feature",
            Exactness::Derived,
        );
        let helix = crate::curve::expression_helix(record);
        let placed_helix = curve_expression_helix_definition(record);
        if let Some(procedural_definition) = placed_helix {
            let curve_id = CurveId(format!(
                "creo:depdb:curve_expression_curve#{}-{}",
                record.entity_id, record.offset
            ));
            let procedural_id = ProceduralCurveId(format!(
                "creo:depdb:curve_expression_helix#{}-{}",
                record.entity_id, record.offset
            ));
            annotate(
                annotations,
                &curve_id.0,
                &source_section,
                record.offset as u64,
                "curve_expression_carrier",
                Exactness::Unknown,
            );
            annotate(
                annotations,
                &procedural_id.0,
                &source_section,
                record.offset as u64,
                "curve_expression_helix",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Unknown { record: None },
                source_object: None,
            });
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id,
                definition: procedural_definition,
                cache_fit_tolerance: None,
            });
        }
        let definition = helix.map_or_else(
            || IrFeatureDefinition::Native {
                kind: "CurveFromEquation".to_string(),
                parameters: BTreeMap::from([
                    ("entity_id".to_string(), record.entity_id.to_string()),
                    (
                        "assignment_count".to_string(),
                        record.assignments.len().to_string(),
                    ),
                ]),
                properties: BTreeMap::new(),
            },
            |helix| IrFeatureDefinition::HelixNativeAxis {
                axis_native_ref: curve_expression_record_id(record),
                axial_rise: Length(helix.height),
                pitch: Length(helix.height / helix.revolutions),
                revolutions: helix.revolutions,
                start_angle: Angle(helix.start_angle),
                clockwise: helix.clockwise,
            },
        );
        ir.model.features.push(Feature {
            id: feature_id,
            ordinal,
            name: Some(format!("Curve Equation {}", record.entity_id)),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("crv_fr_eqn".to_string()),
            source_text: Some(
                record
                    .lines
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            source_content,
            outputs: Vec::new(),
            definition,
            native_ref: Some(curve_expression_record_id(record)),
        });
    }
    transferred_parameter_count
}

#[cfg(test)]
mod tests;
