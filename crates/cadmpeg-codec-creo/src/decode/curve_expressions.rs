// SPDX-License-Identifier: Apache-2.0
//! Curve-from-equation feature transfer and assignment parameter ordering.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{
    features::{
        DesignParameter, Feature, FeatureDefinition as IrFeatureDefinition,
        FeatureId as IrFeatureId, FeatureOperation as IrFeatureOperation, FeatureSourceContent,
        ParameterId, ParameterValue,
    },
    scalar::Length,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::ContainerScan;

use super::coverage::source_section;
use super::native::annotate;

pub(super) fn curve_expression_record_id(
    ctx: &DecodeContext<'_>,
    record: &crate::curve::CurveExpressionRecord,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!(
            "creo:depdb:curve_expression#{}-{}-{}",
            if record.backup { "backup" } else { "active" },
            record.entity_id,
            record.offset
        ),
        "creo curve expression record id",
    )
}

const EPS_HELIX_BASIS_LENGTH: f64 = 1.0e-12;
const EPS_HELIX_BASIS_ORIGIN: f64 = 1.0e-12;
const EPS_HELIX_UV_EQUAL: f64 = 1.0e-9;
const EPS_HELIX_UV_ORTHO: f64 = 1.0e-9;

#[derive(Debug)]
struct CurveExpressionParameterOrder {
    ordinals: Vec<u32>,
    cyclic_edges: HashSet<(usize, usize)>,
}

fn curve_expression_helix_definition(
    record: &crate::curve::CurveExpressionRecord,
    helix: &crate::curve::CurveExpressionHelix,
) -> Option<ProceduralCurveDefinition> {
    let slots = record.local_system.as_ref()?.explicit_slots?.get();
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
    let origin = Point3::new(slots[9], slots[10], slots[11]);
    let (sin, cos) = helix.start_angle.get().sin_cos();
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
    Some(ProceduralCurveDefinition::Helix(
        cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
            [0.0, helix.revolutions.get() * std::f64::consts::TAU],
            cadmpeg_ir::geometry::HelixFrame {
                center: Point3::new(
                    origin.x + axis.x * helix.z_start.get(),
                    origin.y + axis.y * helix.z_start.get(),
                    origin.z + axis.z * helix.z_start.get(),
                ),
                major: Vector3::new(
                    major_direction.x * helix.radius.get(),
                    major_direction.y * helix.radius.get(),
                    major_direction.z * helix.radius.get(),
                ),
                minor: Vector3::new(
                    minor_direction.x * helix.radius.get(),
                    minor_direction.y * helix.radius.get(),
                    minor_direction.z * helix.radius.get(),
                ),
                pitch: Vector3::new(
                    axis.x * helix.height.get() / helix.revolutions.get(),
                    axis.y * helix.height.get() / helix.revolutions.get(),
                    axis.z * helix.height.get() / helix.revolutions.get(),
                ),
                axis,
            },
            0.0,
            None,
        )
        .ok()?,
    ))
}

fn curve_expression_helix_feature_definition(
    helix: &crate::curve::CurveExpressionHelix,
    procedural: &ProceduralCurveDefinition,
) -> Option<IrFeatureDefinition> {
    let ProceduralCurveDefinition::Helix(helix_payload) = procedural else {
        return None;
    };
    let pitch = helix_payload.pitch();
    let axis = helix_payload.axis();

    let axial_pitch = pitch.x * axis.x + pitch.y * axis.y + pitch.z * axis.z;
    let pitch = cadmpeg_ir::scalar::NonZeroLength::new(axial_pitch)?;
    Some(IrFeatureDefinition::Operation(IrFeatureOperation::Helix {
        axis_origin: *helix_payload.center(),
        axis_direction: cadmpeg_ir::features::FeatureDirection3::new(axis.get())?,
        radius: helix.radius,
        shape: cadmpeg_ir::features::HelixShape::Cylindrical { pitch },
        revolutions: helix.revolutions,
        start_angle: helix.start_angle,
        clockwise: helix.clockwise,
        segment_turns: None,
        construction_style: None,
    }))
}

/// Assign a component to each vertex with two iterative depth-first passes.
fn expression_dependency_components(
    ctx: &DecodeContext<'_>,
    dependencies: &[Vec<usize>],
    reverse: &[Vec<usize>],
) -> Result<Vec<usize>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo curve-expression component scratch")?;
    let mut visited = scratch.with_storage(|| {
        ctx.alloc_filled(
            dependencies.len(),
            false,
            "creo curve-expression visited dependencies",
        )
    })?;
    let mut pending = Vec::new();
    let mut finish = Vec::new();
    for root in ctx.admit_iter(
        0..dependencies.len(),
        "creo curve-expression component roots",
    )? {
        if visited[root] {
            continue;
        }
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut pending,
                (root, false),
                "creo curve-expression pending dependency",
            )
        })?;
        while !pending.is_empty() {
            let Some((index, expanded)) = ctx.next_charged(
                &mut std::iter::from_fn(|| pending.pop()),
                "walk Creo curve-expression dependencies",
            )? else {
                break;
            };
            if expanded {
                scratch.with_storage(|| {
                    ctx.push_vec(
                        &mut finish,
                        index,
                        "creo curve-expression component finish order",
                    )
                })?;
                continue;
            }
            if visited[index] {
                continue;
            }
            visited[index] = true;
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut pending,
                    (index, true),
                    "creo curve-expression pending dependencies",
                )
            })?;
            for &dependency in ctx.admit_iter(
                &dependencies[index],
                "creo curve-expression component edges",
            )? {
                if !visited[dependency] {
                    scratch.with_storage(|| {
                        ctx.push_vec(
                            &mut pending,
                            (dependency, false),
                            "creo curve-expression pending dependencies",
                        )
                    })?;
                }
            }
        }
    }
    let mut components = ctx.alloc_filled(
        dependencies.len(),
        0usize,
        "creo curve-expression components",
    )?;
    for value in ctx.admit_iter(&mut visited, "creo curve-expression component marks reset")? {
        *value = false;
    }
    let mut component = 0;
    for root in ctx
        .admit_iter(finish, "creo curve-expression component finish traversal")?
        .rev()
    {
        if visited[root] {
            continue;
        }
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut pending,
                (root, false),
                "creo curve-expression pending dependency",
            )
        })?;
        while !pending.is_empty() {
            let Some((index, _)) = ctx.next_charged(
                &mut std::iter::from_fn(|| pending.pop()),
                "walk Creo curve-expression reverse dependencies",
            )? else {
                break;
            };
            if visited[index] {
                continue;
            }
            visited[index] = true;
            components[index] = component;
            for &consumer in
                ctx.admit_iter(&reverse[index], "creo curve-expression reverse edges")?
            {
                if !visited[consumer] {
                    scratch.with_storage(|| {
                        ctx.push_vec(
                            &mut pending,
                            (consumer, false),
                            "creo curve-expression pending dependencies",
                        )
                    })?;
                }
            }
        }
        component += 1;
    }
    Ok(components)
}

fn curve_expression_parameter_order(
    ctx: &DecodeContext<'_>,
    record: &crate::curve::CurveExpressionRecord,
    unique_assignment_indices: &BTreeMap<String, usize>,
) -> Result<Option<CurveExpressionParameterOrder>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo curve-expression ordering scratch")?;
    let mut dependencies = scratch.with_storage(|| {
        ctx.collect_indexed_vec(
            record.assignments.len(),
            "creo curve-expression dependency rows",
            |_| Ok(Vec::new()),
        )
    })?;
    let mut reverse = scratch.with_storage(|| {
        ctx.collect_indexed_vec(
            record.assignments.len(),
            "creo curve-expression reverse rows",
            |_| Ok(Vec::new()),
        )
    })?;
    for (consumer, (row, assignment)) in ctx
        .admit_iter(&mut dependencies, "creo curve-expression ordering rows")?
        .zip(&record.assignments)
        .enumerate()
    {
        let mut row_storage = ctx.reserve_scoped(0, "creo curve-expression edge index scratch")?;
        let mut seen = HashSet::new();
        for name in ctx.admit_iter(
            &assignment.dependencies,
            "creo curve-expression dependency traversal",
        )? {
            let key_parts = ctx.format_scoped(
                format_args!("{name}"),
                "creo curve-expression ordering lookup",
            )?;
            let _key_storage = key_parts.1;
            let mut key = key_parts.0;
            ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
            let Some(&index) = ctx.get_btree_map(
                unique_assignment_indices,
                &key,
                "creo curve-expression assignment lookup",
            )?
            else {
                continue;
            };
            if seen.contains(&index) {
                continue;
            }
            row_storage.with_storage(|| {
                ctx.insert_hash_set(&mut seen, index, "creo curve-expression edge index nodes")
            })?;
            scratch.with_storage(|| {
                ctx.push_vec(row, index, "creo curve-expression dependency indices")
            })?;
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut reverse[index],
                    consumer,
                    "creo curve-expression reverse indices",
                )
            })?;
        }
    }
    let components =
        scratch.with_storage(|| expression_dependency_components(ctx, &dependencies, &reverse))?;
    let mut cyclic_edges = HashSet::new();
    let mut remaining = scratch.with_storage(|| {
        ctx.alloc_filled(
            dependencies.len(),
            0usize,
            "creo curve-expression remaining dependency counts",
        )
    })?;
    let mut ready = BTreeSet::new();
    for (consumer, row) in ctx
        .admit_iter(&dependencies, "creo curve-expression cycle rows")?
        .enumerate()
    {
        for &dependency in ctx.admit_iter(row, "creo curve-expression cycle edges")? {
            if components[consumer] == components[dependency] {
                ctx.insert_hash_set(
                    &mut cyclic_edges,
                    (consumer, dependency),
                    "creo curve-expression cyclic edges",
                )?;
            } else {
                remaining[consumer] += 1;
            }
        }
        if remaining[consumer] == 0 {
            scratch.with_storage(|| {
                ctx.insert_btree_set(&mut ready, consumer, "creo curve-expression ready indices")
            })?;
        }
    }
    let mut ordinals = ctx.alloc_filled(
        dependencies.len(),
        0u32,
        "creo curve-expression parameter ordinals",
    )?;
    let mut ordinal_steps = 0..dependencies.len();
    while ordinal_steps.len() > 0 {
        let Some(ordinal) = ctx.next_charged(
            &mut ordinal_steps,
            "creo curve-expression ordinal traversal",
        )? else {
            break;
        };
        let Some(index) = ctx
            .find_by(
                &ready,
                |_| Ok(true),
                "creo curve-expression ready candidates",
            )?
            .copied()
        else {
            return Ok(None);
        };
        ctx.remove_btree_set(&mut ready, &index, "creo curve-expression ready removal")?;
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(None);
        };
        ordinals[index] = ordinal;
        for &consumer in ctx.admit_iter(&reverse[index], "creo curve-expression ready edges")? {
            if components[consumer] == components[index] {
                continue;
            }
            remaining[consumer] -= 1;
            if remaining[consumer] == 0 {
                scratch.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut ready,
                        consumer,
                        "creo curve-expression ready indices",
                    )
                })?;
            }
        }
    }
    Ok(Some(CurveExpressionParameterOrder {
        ordinals,
        cyclic_edges,
    }))
}

fn curve_expression_parameter_names(
    ctx: &DecodeContext<'_>,
    assignments: &[crate::curve::CurveExpressionAssignment],
) -> Result<Vec<Option<String>>, CodecError> {
    let mut counts = BTreeMap::new();
    for assignment in ctx.admit_iter(assignments, "creo curve-expression name traversal")? {
        if let Some((name, _)) = assignment.parameter_target() {
            let mut key = ctx.copy_retained_text(name, "creo curve-expression name key")?;
            ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
            match ctx.entry_btree_map(&mut counts, key, "creo curve-expression unique names")? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(1usize);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() += 1;
                }
            }
        }
    }
    let mut occurrences = BTreeMap::new();
    let mut names = Vec::new();
    for assignment in ctx.admit_iter(assignments, "creo curve-expression name traversal")? {
        let name = if let Some((name, _)) = assignment.parameter_target() {
            let mut key = ctx.copy_retained_text(name, "creo curve-expression occurrence key")?;
            ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
            if *ctx
                .get_btree_map(&counts, &key, "creo curve-expression name count lookup")?
                .ok_or_else(|| CodecError::malformed("curve expression name count is absent"))?
                == 1
            {
                Some(ctx.copy_retained_text(name, "creo curve-expression parameter name")?)
            } else {
                let occurrence = ctx
                    .entry_btree_map(&mut occurrences, key, "creo curve-expression occurrences")?
                    .or_insert(0usize);
                *occurrence += 1;
                let mut output =
                    ctx.copy_retained_text(name, "creo curve-expression parameter name")?;
                let mut digits = [0u8; 20];
                let mut value = *occurrence;
                let mut width = 0;
                loop {
                    digits[width] = u8::try_from(value % 10).map_err(|_| {
                        cadmpeg_core::CodecError::malformed(
                            "Creo occurrence decimal digit exceeds u8",
                        )
                    })?;
                    width += 1;
                    value /= 10;
                    if value == 0 {
                        break;
                    }
                }
                ctx.try_reserve_retained_text(
                    &mut output,
                    width + 1,
                    "creo curve-expression parameter suffix",
                )?;
                output.push('#');
                for &digit in digits[..width].iter().rev() {
                    output.push(char::from(b'0' + digit));
                }
                Some(output)
            }
        } else {
            None
        };
        ctx.reserve_vec(&mut names, 1, "creo curve-expression parameter name slots")?;
        names.push(name);
    }
    Ok(names)
}

#[derive(Debug)]
struct AssignmentIndices {
    by_name: BTreeMap<String, Option<usize>>,
    unique: BTreeMap<String, usize>,
}

fn curve_expression_assignment_indices(
    ctx: &DecodeContext<'_>,
    record: &crate::curve::CurveExpressionRecord,
) -> Result<AssignmentIndices, CodecError> {
    let mut by_name = BTreeMap::<String, Option<usize>>::new();
    for (ordinal, assignment) in ctx
        .admit_iter(
            &record.assignments,
            "creo curve-expression assignment traversal",
        )?
        .enumerate()
    {
        if assignment.activation == crate::curve::CurveExpressionActivation::Inactive {
            continue;
        }
        let Some((name, _)) = assignment.parameter_target() else {
            continue;
        };
        let mut key = ctx.copy_retained_text(name, "creo curve-expression assignment key")?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        match ctx.entry_btree_map(
            &mut by_name,
            key,
            "creo curve-expression assignment indices",
        )? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(ordinal));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    let mut unique = BTreeMap::new();
    for (name, index) in ctx.admit_iter(&by_name, "creo curve-expression index traversal")? {
        if let Some(index) = index {
            let key = ctx.copy_retained_text(name, "creo curve-expression unique key")?;
            ctx.insert_btree_map(
                &mut unique,
                key,
                *index,
                "creo curve-expression unique indices",
            )?;
        }
    }
    Ok(AssignmentIndices { by_name, unique })
}

fn curve_expression_emitted_ordinals(
    ctx: &DecodeContext<'_>,
    record: &crate::curve::CurveExpressionRecord,
    parameter_ordinals: &[u32],
) -> Result<BTreeMap<usize, u32>, CodecError> {
    let mut indices_storage = ctx.reserve_scoped(0, "creo curve-expression emitted scratch")?;
    let mut indices = Vec::new();
    for (index, assignment) in ctx
        .admit_iter(&record.assignments, "creo emitted assignment traversal")?
        .enumerate()
    {
        if assignment.parameter_target().is_some() {
            indices_storage.with_storage(|| {
                ctx.push_vec(&mut indices, index, "creo curve-expression emitted indices")
            })?;
        }
    }
    ctx.stable_sort_by_key(
        indices.as_mut_slice(),
        |value| parameter_ordinals[*value],
        Ord::cmp,
        "creo curve expression emitted ordinals indices ordering",
    )?;
    let mut emitted = BTreeMap::new();
    for (ordinal, index) in ctx
        .admit_iter(indices, "creo curve-expression emitted index traversal")?
        .enumerate()
    {
        let ordinal = u32::try_from(ordinal)
            .map_err(|_| CodecError::malformed("curve expression parameter ordinal exceeds u32"))?;
        ctx.insert_btree_map(
            &mut emitted,
            index,
            ordinal,
            "creo curve-expression emitted ordinals",
        )?;
    }
    Ok(emitted)
}

fn joined_dependency_names(
    ctx: &DecodeContext<'_>,
    names: &[String],
    include: impl Fn(&str) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Option<String>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo curve-expression joined names scratch")?;
    let mut selected = Vec::new();
    for name in ctx.admit_iter(names, operation)? {
        if include(name)? {
            storage.with_storage(|| ctx.push_vec(&mut selected, name.as_str(), operation))?;
        }
    }
    if selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(ctx.join_retained(&selected, ",", operation)?))
}

fn insert_curve_expression_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    name: &'static str,
    value: String,
) -> Result<(), CodecError> {
    let key = ctx.copy_retained_text(name, "creo curve-expression property key")?;
    ctx.insert_btree_map(
        properties,
        key,
        value,
        "creo curve-expression property nodes",
    )?;
    Ok(())
}

fn join_cyclic_dependency_names(
    ctx: &DecodeContext<'_>,
    names: &[&str],
) -> Result<String, CodecError> {
    ctx.join_retained(names, ",", "creo curve-expression cyclic dependency text")
}

fn curve_expression_source_text(
    ctx: &DecodeContext<'_>,
    lines: &[crate::curve::CurveExpressionLine],
) -> Result<String, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo curve-expression source text scratch")?;
    let parts = storage.with_storage(|| {
        ctx.collect_indexed_vec(
            lines.len(),
            "creo curve-expression source text parts",
            |index| Ok(lines[index].text.as_str()),
        )
    })?;
    ctx.join_retained(&parts, "\n", "creo curve-expression feature source text")
}

fn curve_expression_properties(
    ctx: &DecodeContext<'_>,
    assignment: &crate::curve::CurveExpressionAssignment,
    assignment_ordinal: usize,
    parameter: (&str, &ParameterId),
    indices: (&BTreeMap<String, Option<usize>>, &BTreeMap<String, usize>),
    dimension_parameters: &BTreeMap<String, ParameterId>,
    cyclic_edges: &HashSet<(usize, usize)>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let (assignment_indices_by_name, unique_assignment_indices) = indices;

    let (parameter_name, parameter_id) = parameter;

    let Some((assignment_name, declared_unit)) = assignment.parameter_target() else {
        return Err(CodecError::malformed(
            "curve expression assignment has no parameter target",
        ));
    };
    let external_dependencies = joined_dependency_names(
        ctx,
        &assignment.dependencies,
        |name| {
            let key_parts = ctx.format_scoped(
                format_args!("{name}"),
                "creo curve-expression external lookup",
            )?;
            let _reservation = key_parts.1;
            let mut key = key_parts.0;
            ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
            Ok(key != "t"
                && !ctx.contains_key_btree_map(
                    assignment_indices_by_name,
                    &key,
                    "creo curve-expression assignment lookup",
                )?
                && !ctx.contains_key_btree_map(
                    dimension_parameters,
                    &key,
                    "creo curve-expression dimension lookup",
                )?)
        },
        "creo curve-expression external dependency text",
    )?;
    let ambiguous_dependencies = joined_dependency_names(
        ctx,
        &assignment.dependencies,
        |name| {
            let key_parts = ctx.format_scoped(
                format_args!("{name}"),
                "creo curve-expression ambiguous lookup",
            )?;
            let _reservation = key_parts.1;
            let mut key = key_parts.0;
            ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
            Ok(matches!(
                ctx.get_btree_map(
                    assignment_indices_by_name,
                    &key,
                    "creo curve-expression assignment lookup"
                )?,
                Some(None)
            ))
        },
        "creo curve-expression ambiguous dependency text",
    )?;
    let intrinsic_dependencies = joined_dependency_names(
        ctx,
        &assignment.dependencies,
        |name| Ok(name.eq_ignore_ascii_case("t")),
        "creo curve-expression intrinsic dependency text",
    )?;
    let mut properties = BTreeMap::new();
    if let Some(value) = external_dependencies {
        insert_curve_expression_property(ctx, &mut properties, "external_dependencies", value)?;
    }
    if let Some(value) = ambiguous_dependencies {
        insert_curve_expression_property(ctx, &mut properties, "ambiguous_dependencies", value)?;
    }
    insert_curve_expression_property(
        ctx,
        &mut properties,
        "source_assignment_ordinal",
        ctx.format_retained(
            format_args!("{assignment_ordinal}"),
            "creo curve-expression source ordinal value",
        )?,
    )?;
    insert_curve_expression_property(
        ctx,
        &mut properties,
        "activation",
        ctx.copy_retained_text(
            assignment.activation.token(),
            "creo curve-expression activation value",
        )?,
    )?;
    if let Some(unit) = declared_unit {
        let unit = ctx.copy_retained_text(unit, "creo curve-expression declared unit")?;
        insert_curve_expression_property(ctx, &mut properties, "declared_unit", unit)?;
    }
    if let Some(crate::curve::CurveExpressionValue::Quantity(quantity)) = &assignment.value {
        insert_curve_expression_property(
            ctx,
            &mut properties,
            "evaluated_canonical_value",
            ctx.format_retained(
                format_args!("{}", quantity.value()),
                "creo curve-expression canonical value",
            )?,
        )?;
        insert_curve_expression_property(
            ctx,
            &mut properties,
            "evaluated_dimension",
            ctx.format_retained(
                format_args!(
                    "length:{},mass:{},time:{},angle:{},temperature:{}",
                    quantity.powers()[0],
                    quantity.powers()[1],
                    quantity.powers()[2],
                    quantity.powers()[3],
                    quantity.powers()[4]
                ),
                "creo curve-expression dimension value",
            )?,
        )?;
    }
    if !ctx.equal(
        parameter_name,
        assignment_name,
        "creo curve-expression source name comparison",
    )? {
        let source_name =
            ctx.copy_retained_text(assignment_name, "creo curve-expression source name")?;
        insert_curve_expression_property(ctx, &mut properties, "source_name", source_name)?;
    }
    if let Some(value) = intrinsic_dependencies {
        insert_curve_expression_property(ctx, &mut properties, "independent_variables", value)?;
    }
    let mut cyclic_storage = ctx.reserve_scoped(0, "creo curve-expression cyclic names scratch")?;
    let mut cyclic_dependencies = Vec::new();
    for name in ctx.admit_iter(
        &assignment.dependencies,
        "creo curve-expression dependency traversal",
    )? {
        let key_parts = ctx.format_scoped(
            format_args!("{name}"),
            "creo curve-expression cyclic lookup",
        )?;
        let _reservation = key_parts.1;
        let mut key = key_parts.0;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        if ctx
            .get_btree_map(
                unique_assignment_indices,
                &key,
                "creo curve-expression assignment lookup",
            )?
            .is_some_and(|dependency| cyclic_edges.contains(&(assignment_ordinal, *dependency)))
        {
            cyclic_storage.with_storage(|| {
                ctx.reserve_vec(
                    &mut cyclic_dependencies,
                    1,
                    "creo curve-expression cyclic dependency names",
                )
            })?;
            cyclic_dependencies.push(name.as_str());
        }
    }
    ctx.sort_unstable_by(
        &mut cyclic_dependencies,
        |value| value,
        Ord::cmp,
        "creo curve-expression cyclic dependency name sort",
    )?;
    ctx.dedup_vec(
        &mut cyclic_dependencies,
        "creo curve-expression cyclic name deduplication",
    )?;
    if !cyclic_dependencies.is_empty() {
        let value = join_cyclic_dependency_names(ctx, &cyclic_dependencies)?;
        insert_curve_expression_property(ctx, &mut properties, "cyclic_dependencies", value)?;
    }
    cadmpeg_core::text::named_entries_for_decode(ctx, parameter_id.as_str(), properties)
        .map_err(Into::into)
}

fn native_curve_expression_definition(
    ctx: &DecodeContext<'_>,
    entity_id: u32,
    assignment_count: usize,
) -> Result<IrFeatureDefinition, CodecError> {
    let mut parameters = BTreeMap::new();
    let kind = ctx
        .copy_retained_text("CurveFromEquation", "creo curve-expression native kind")?
        .into();
    let entity_value = ctx.format_retained(
        format_args!("{entity_id}"),
        "creo curve-expression native entity value",
    )?;
    let assignment_value = ctx.format_retained(
        format_args!("{assignment_count}"),
        "creo curve-expression native assignment count",
    )?;
    let entity_key = cadmpeg_core::text::NonBlankString::for_decode(
        ctx,
        ctx.copy_retained_text("entity_id", "creo curve-expression native entity key")?,
        "validate nonblank text",
    )?
    .ok_or_else(|| CodecError::malformed("native entity key is blank"))?;
    ctx.insert_btree_map(
        &mut parameters,
        entity_key,
        entity_value,
        "creo curve-expression native parameters",
    )?;
    let assignment_key = cadmpeg_core::text::NonBlankString::for_decode(
        ctx,
        ctx.copy_retained_text(
            "assignment_count",
            "creo curve-expression native assignment key",
        )?,
        "validate nonblank text",
    )?
    .ok_or_else(|| CodecError::malformed("native assignment key is blank"))?;
    ctx.insert_btree_map(
        &mut parameters,
        assignment_key,
        assignment_value,
        "creo curve-expression native parameters",
    )?;
    Ok(IrFeatureDefinition::Operation(IrFeatureOperation::Native {
        kind,
        parameters,
    }))
}

fn curve_expression_feature_labels(
    ctx: &DecodeContext<'_>,
    entity_id: u32,
) -> Result<(String, String), CodecError> {
    Ok((
        ctx.format_retained(
            format_args!("Curve Equation {entity_id}"),
            "creo curve-expression feature name",
        )?,
        ctx.copy_retained_text("crv_fr_eqn", "creo curve-expression feature source tag")?,
    ))
}

fn curve_expression_parameter_dependencies(
    ctx: &DecodeContext<'_>,
    record: &crate::curve::CurveExpressionRecord,
    assignment_ordinal: usize,
    assignment_indices_by_name: &BTreeMap<String, Option<usize>>,
    unique_assignment_indices: &BTreeMap<String, usize>,
    cyclic_edges: &HashSet<(usize, usize)>,
    dimension_parameters: &BTreeMap<String, ParameterId>,
) -> Result<Vec<ParameterId>, CodecError> {
    let assignment = &record.assignments[assignment_ordinal];
    let mut scratch = ctx.reserve_scoped(0, "creo curve-expression dependency scratch")?;
    let mut seen = HashSet::new();
    let mut assignment_ids = None::<HashSet<String>>;
    let mut dimension_ids = HashSet::<&str>::new();
    let mut dependencies = Vec::new();
    let mut dimension_dependencies = Vec::new();
    for name in ctx.admit_iter(
        &assignment.dependencies,
        "creo curve-expression dependency traversal",
    )? {
        let key_parts = ctx.format_scoped(
            format_args!("{name}"),
            "creo curve-expression dependency key",
        )?;
        let _key_reservation = key_parts.1;
        let mut key = key_parts.0;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        if let Some(&dependency) = ctx.get_btree_map(
            unique_assignment_indices,
            &key,
            "creo curve-expression assignment lookup",
        )? {
            if cyclic_edges.contains(&(assignment_ordinal, dependency))
                || seen.contains(&dependency)
            {
                continue;
            }
            scratch.with_storage(|| {
                ctx.insert_hash_set(
                    &mut seen,
                    dependency,
                    "creo curve-expression seen dependencies",
                )
            })?;
            ctx.reserve_vec(
                &mut dependencies,
                1,
                "creo curve-expression parameter dependencies",
            )?;
            let parameter = crate::identity::compose_checked::<ParameterId>(
                ctx,
                &crate::identity::DEPDB_CURVE_EXPRESSION_PARAMETER,
                format_args!("{}-{}-{dependency}", record.entity_id, record.offset),
                "creo curve-expression dependency identity",
            )?;
            if let Some(ids) = &mut assignment_ids {
                scratch.with_storage(|| {
                    ctx.insert_hash_set(
                        ids,
                        ctx.copy_retained_text(
                            parameter.as_str(),
                            "creo curve-expression dependency identity key",
                        )?,
                        "creo curve-expression dependency identity nodes",
                    )
                })?;
            }
            dependencies.push(parameter);
        }
        if ctx.contains_key_btree_map(
            assignment_indices_by_name,
            &key,
            "creo curve-expression assignment lookup",
        )? {
            continue;
        }
        let Some(parameter) = ctx.get_btree_map(
            dimension_parameters,
            &key,
            "creo curve-expression dimension lookup",
        )?
        else {
            continue;
        };
        let assignment_ids = match &mut assignment_ids {
            Some(ids) => ids,
            slot @ None => {
                let mut ids = HashSet::new();
                for dependency in ctx.admit_iter(
                    &dependencies,
                    "creo curve-expression dependency identity index traversal",
                )? {
                    scratch.with_storage(|| {
                        ctx.insert_hash_set(
                            &mut ids,
                            ctx.copy_retained_text(
                                dependency.as_str(),
                                "creo curve-expression dependency identity key",
                            )?,
                            "creo curve-expression dependency identity nodes",
                        )
                    })?;
                }
                slot.insert(ids)
            }
        };
        if ctx.contains_hash_set(
            assignment_ids,
            parameter.as_str(),
            "creo curve-expression dependency identity lookup",
        )? || ctx.contains_hash_set(
            &dimension_ids,
            parameter.as_str(),
            "creo curve-expression dimension identity lookup",
        )? {
            continue;
        }
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut dimension_ids,
                parameter.as_str(),
                "creo curve-expression dimension identity nodes",
            )
        })?;
        scratch.with_storage(|| {
            ctx.reserve_vec(
                &mut dimension_dependencies,
                1,
                "creo curve-expression dimension candidates",
            )
        })?;
        dimension_dependencies.push(parameter);
    }
    for parameter in ctx.admit_iter(
        dimension_dependencies,
        "creo curve-expression dimension traversal",
    )? {
        ctx.reserve_vec(
            &mut dependencies,
            1,
            "creo curve-expression dimension dependencies",
        )?;
        let copied = ctx.copy_retained_text(
            parameter.as_str(),
            "creo curve-expression dimension parameter id",
        )?;
        let copied = ParameterId::try_from(copied).map_err(|_| {
            CodecError::malformed("curve expression dimension parameter id is invalid")
        })?;
        dependencies.push(copied);
    }
    Ok(dependencies)
}

pub(super) fn transfer_curve_expression_features(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    dimension_parameters: &BTreeMap<String, ParameterId>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let ordinal_base = ctx
        .admit_iter(
            &ir.model.features,
            "creo curve-expression feature ordinal traversal",
        )?
        .map(|feature| feature.ordinal)
        .max()
        .map_or(0, |value| value + 1);
    let mut transferred_parameter_count = 0;
    for (expression_ordinal, record) in ctx
        .admit_iter(
            &scan.curves.expressions,
            "creo curve-expression record traversal",
        )?
        .filter(|record| !record.backup)
        .enumerate()
    {
        let mut scratch = ctx.reserve_scoped(0, "creo curve-expression transfer scratch")?;
        let source_section = scratch.with_storage(|| source_section(ctx, scan, record.offset))?;
        let ordinal = ordinal_base + cadmpeg_core::decode::u64_from_index(expression_ordinal);
        let feature_id = crate::identity::compose_checked::<IrFeatureId>(
            ctx,
            &crate::identity::DEPDB_CURVE_EXPRESSION_FEATURE,
            format_args!("{}-{}", record.entity_id, record.offset),
            "creo curve-expression feature identity",
        )?;
        let AssignmentIndices {
            by_name: assignment_indices_by_name,
            unique: unique_assignment_indices,
        } = scratch.with_storage(|| curve_expression_assignment_indices(ctx, record))?;
        let Some(CurveExpressionParameterOrder {
            ordinals: parameter_ordinals,
            cyclic_edges,
        }) = scratch.with_storage(|| {
            curve_expression_parameter_order(ctx, record, &unique_assignment_indices)
        })?
        else {
            continue;
        };
        let parameter_names =
            scratch.with_storage(|| curve_expression_parameter_names(ctx, &record.assignments))?;
        let emitted_ordinals = scratch
            .with_storage(|| curve_expression_emitted_ordinals(ctx, record, &parameter_ordinals))?;
        let mut source_content = Vec::new();
        ctx.reserve_vec(
            &mut source_content,
            emitted_ordinals.len(),
            "creo curve-expression source content",
        )?;
        let parameter_start = ir.model.parameters.len();
        for (assignment_ordinal, assignment) in ctx
            .admit_iter(
                &record.assignments,
                "creo curve-expression assignment traversal",
            )?
            .enumerate()
        {
            let Some((_assignment_name, _declared_unit)) = assignment.parameter_target() else {
                continue;
            };
            let Some(&ordinal) = ctx.get_btree_map(
                &emitted_ordinals,
                &assignment_ordinal,
                "creo curve-expression emitted lookup",
            )?
            else {
                continue;
            };
            let Some(parameter_name) = parameter_names
                .get(assignment_ordinal)
                .and_then(Option::as_ref)
            else {
                return Err(cadmpeg_core::CodecError::malformed(ctx.format_retained(
                    format_args!(
                        "curve expression record {} assignment {} has no parameter name",
                        record.entity_id, assignment_ordinal
                    ),
                    "creo curve-expression missing name error",
                )?));
            };
            let parameter_id = crate::identity::compose_checked::<ParameterId>(
                ctx,
                &crate::identity::DEPDB_CURVE_EXPRESSION_PARAMETER,
                format_args!(
                    "{}-{}-{assignment_ordinal}",
                    record.entity_id, record.offset
                ),
                "creo curve-expression parameter identity",
            )?;
            let dependencies = curve_expression_parameter_dependencies(
                ctx,
                record,
                assignment_ordinal,
                &assignment_indices_by_name,
                &unique_assignment_indices,
                &cyclic_edges,
                dimension_parameters,
            )?;
            annotate(
                ctx,
                annotations,
                parameter_id.as_str(),
                &source_section,
                cadmpeg_core::decode::u64_from_index(assignment.offset),
                "curve_expression_assignment",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model parameters")?;
            let value = match assignment.value.as_ref() {
                Some(crate::curve::CurveExpressionValue::String(value)) => {
                    Some(ParameterValue::String(ctx.copy_retained_text(
                        value,
                        "creo curve-expression parameter string value",
                    )?))
                }
                other => other.and_then(|value| match value {
                    crate::curve::CurveExpressionValue::Number(value) => {
                        Some(ParameterValue::Real(*value))
                    }
                    crate::curve::CurveExpressionValue::Length(value) => Some(
                        ParameterValue::Length(cadmpeg_ir::scalar::Length::new(value.get())?),
                    ),
                    crate::curve::CurveExpressionValue::Angle(value) => {
                        Some(ParameterValue::Angle(cadmpeg_ir::scalar::Angle::new(
                            value.get().to_radians(),
                        )?))
                    }
                    crate::curve::CurveExpressionValue::Quantity(_)
                    | crate::curve::CurveExpressionValue::String(_) => None,
                }),
            };
            source_carriers.admit_parameter(
                ctx,
                ir,
                DesignParameter {
                    id: crate::identity::copy_checked_id(
                        ctx,
                        parameter_id.as_str(),
                        "creo curve-expression IR parameter ID copy",
                    )?,
                    owner: Some(crate::identity::copy_checked_id(
                        ctx,
                        feature_id.as_str(),
                        "creo curve-expression owner ID copy",
                    )?),
                    ordinal,
                    name: ctx.copy_retained_text(
                        parameter_name,
                        "creo curve-expression IR parameter name",
                    )?,
                    expression: ctx.copy_retained_text(
                        &assignment.expression,
                        "creo curve-expression IR expression",
                    )?,
                    display: None,
                    value,
                    dependencies: cadmpeg_ir::features::DistinctMembers::try_from(
                        dependencies,
                        ctx,
                    )
                    .map_err(cadmpeg_core::CodecError::from)?,
                    properties: BTreeMap::new(),
                    pmi: None,
                    native_ref: Some(curve_expression_record_id(ctx, record)?),
                },
            )?;
            transferred_parameter_count += 1;
            source_content.push(FeatureSourceContent::Parameter(
                crate::identity::copy_checked_id(
                    ctx,
                    parameter_id.as_str(),
                    "creo curve-expression source parameter ID copy",
                )?,
            ));
        }
        let mut parameter_index = parameter_start;
        for (assignment_ordinal, assignment) in ctx
            .admit_iter(
                &record.assignments,
                "creo curve-expression assignment traversal",
            )?
            .enumerate()
        {
            if assignment.parameter_target().is_none()
                || !ctx.contains_key_btree_map(
                    &emitted_ordinals,
                    &assignment_ordinal,
                    "creo curve-expression emitted lookup",
                )?
            {
                continue;
            }
            let parameter = ir
                .model
                .parameters
                .get_mut(parameter_index)
                .ok_or_else(|| {
                    CodecError::malformed("curve-expression parameter sequence is incomplete")
                })?;
            parameter.properties = curve_expression_properties(
                ctx,
                assignment,
                assignment_ordinal,
                (&parameter.name, &parameter.id),
                (&assignment_indices_by_name, &unique_assignment_indices),
                dimension_parameters,
                &cyclic_edges,
            )?;
            parameter_index += 1;
        }
        let helix = crate::curve::expression_helix(ctx, record)?;
        let placed_helix = helix
            .as_ref()
            .and_then(|helix| curve_expression_helix_definition(record, helix));
        let neutral_helix =
            helix
                .as_ref()
                .zip(placed_helix.as_ref())
                .and_then(|(helix, procedural)| {
                    curve_expression_helix_feature_definition(helix, procedural)
                });
        if let Some(procedural_definition) = placed_helix {
            let curve_id = crate::identity::compose_checked::<CurveId>(
                ctx,
                &crate::identity::DEPDB_CURVE_EXPRESSION_CURVE,
                format_args!("{}-{}", record.entity_id, record.offset),
                "creo curve-expression curve identity",
            )?;
            let procedural_id = crate::identity::compose_checked::<ProceduralCurveId>(
                ctx,
                &crate::identity::DEPDB_CURVE_EXPRESSION_HELIX,
                format_args!("{}-{}", record.entity_id, record.offset),
                "creo curve-expression procedural identity",
            )?;
            annotate(
                ctx,
                annotations,
                curve_id.as_str(),
                &source_section,
                cadmpeg_core::decode::u64_from_index(record.offset),
                "curve_expression_carrier",
                Exactness::Unknown,
            )?;
            annotate(
                ctx,
                annotations,
                procedural_id.as_str(),
                &source_section,
                cadmpeg_core::decode::u64_from_index(record.offset),
                "curve_expression_helix",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: crate::identity::copy_checked_id(
                        ctx,
                        curve_id.as_str(),
                        "creo curve-expression IR curve ID copy",
                    )?,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                    source_object: None,
                },
            )?;
            source_carriers.admit_procedural_curve(
                ctx,
                ir,
                &curve_id,
                ProceduralCurve::new(procedural_id, procedural_definition),
            )?;
        }
        let axis_definition = if let Some(definition) = neutral_helix {
            Some(definition)
        } else if let Some(helix) = helix {
            let axis_id = curve_expression_record_id(ctx, record)?;
            let axis_id = cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                axis_id,
                "validate nonblank text",
            )?;
            match (
                axis_id,
                Length::new(helix.height.get()),
                Length::new(helix.height.get() / helix.revolutions.get()),
            ) {
                (Some(axis_native_ref), Some(axial_rise), Some(pitch)) => Some(
                    IrFeatureDefinition::Operation(IrFeatureOperation::HelixNativeAxis {
                        axis_native_ref,
                        axial_rise,
                        pitch,
                        revolutions: helix.revolutions,
                        start_angle: helix.start_angle,
                        clockwise: helix.clockwise,
                    }),
                ),
                _ => None,
            }
        } else {
            None
        };
        let definition = if let Some(definition) = axis_definition {
            definition
        } else {
            native_curve_expression_definition(ctx, record.entity_id, record.assignments.len())?
        };
        annotate(
            ctx,
            annotations,
            feature_id.as_str(),
            &source_section,
            cadmpeg_core::decode::u64_from_index(record.expression_offset),
            "curve_expression_feature",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model features")?;
        let (name, source_tag) = curve_expression_feature_labels(ctx, record.entity_id)?;
        source_carriers.admit_feature(
            ctx,
            ir,
            Feature {
                id: feature_id,
                ordinal,
                name: Some(name),
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: Some(source_tag),
                source_text: Some(curve_expression_source_text(ctx, &record.lines)?),
                source_content: cadmpeg_ir::features::FeatureContent::new(
                    source_content,
                    ctx,
                    "validate Creo feature source content",
                )
                .map_err(cadmpeg_core::CodecError::from)?,

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
                native_ref: Some(curve_expression_record_id(ctx, record)?),
            },
        )?;
    }
    Ok(transferred_parameter_count)
}

#[cfg(test)]
mod tests;
