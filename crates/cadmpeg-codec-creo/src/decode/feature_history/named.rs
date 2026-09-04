// SPDX-License-Identifier: Apache-2.0
//! Named and referenced feature definitions.

use super::super::sketch_transfer::{
    current_feature_operation, feature_recipe_effect, feature_revolution_extent,
    feature_schema_class, feature_section_sweep_semantics_conflict,
};
use super::super::uniqueness::unique_feature_profile_ref;
use super::{
    feature_reference_name, feature_revolution_axis_for_transfer,
    filled_surface_feature_definition, knit_surface_feature_definition,
    linear_extrusion_extent_and_direction, numbered_feature_name_has_family,
    preceding_features_establish_body, schema_feature_definition, section_sweep_boolean_operation,
    sweep_output_kind, sweep_solid, thicken_feature_definition,
};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BodySelection, BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, FaceSelection,
    FeatureDefinition as IrFeatureDefinition, FeatureTreeNodeRole, LinearTermination, PatternForm,
    PatternKind, ProfileRef, RevolutionConstruction,
};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::BodyKind;
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn named_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    if feature_section_sweep_semantics_conflict(scan, feature_id)
        && (matches!(kind, "Protrusion" | "Cut" | "Extrude" | "Revolve")
            || numbered_feature_name_has_family(kind, "Extrude")
            || numbered_feature_name_has_family(kind, "Revolve"))
    {
        return None;
    }
    if numbered_feature_name_has_family(kind, "Fill") {
        return Some(filled_surface_feature_definition(scan, ir, feature_id));
    }
    if numbered_feature_name_has_family(kind, "Thicken") {
        return Some(thicken_feature_definition(scan, ir, feature_id));
    }
    if numbered_feature_name_has_family(kind, "Merge") {
        return Some(knit_surface_feature_definition(scan, feature_id));
    }
    if let Some(definition) = surface_intersect_feature_definition(scan, feature_id, kind) {
        return Some(definition);
    }
    if let Some(definition) = reference_named_feature_definition(kind) {
        return Some(definition);
    }
    if matches!(kind, "Protrusion" | "Cut") {
        return Some(extrude_feature_definition_with_profile(
            scan,
            ir,
            feature_id,
            section_sweep_boolean_operation(
                feature_recipe_effect(scan, feature_id),
                kind,
                false,
                preceding_features_establish_body(ir),
            ),
        ));
    }
    let tree_node_role = match kind {
        "Annotation Feature" => Some(FeatureTreeNodeRole::Annotations),
        "Cross Section" | "Querschnitt" => Some(FeatureTreeNodeRole::CrossSections),
        "Body" | "Körper"
            if feature_reference_name(scan, feature_id).is_none()
                && feature_schema_class(scan, feature_id).is_none() =>
        {
            Some(FeatureTreeNodeRole::SolidBodies)
        }
        "Surface"
            if feature_reference_name(scan, feature_id).is_none()
                && feature_schema_class(scan, feature_id).is_none() =>
        {
            Some(FeatureTreeNodeRole::SurfaceBodies)
        }
        _ => None,
    };
    if let Some(role) = tree_node_role {
        return Some(IrFeatureDefinition::TreeNode {
            role,
            children: Vec::new(),
            active_child: None,
        });
    }
    if kind == "Mirror" {
        return Some(IrFeatureDefinition::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Mirror),
            },
        });
    }
    if kind == "Extrude" || numbered_feature_name_has_family(kind, "Extrude") {
        let output_kind = sweep_output_kind(scan, ir, "extrusion", feature_id);
        let op = section_sweep_boolean_operation(
            feature_recipe_effect(scan, feature_id),
            kind,
            output_kind.is_some(),
            preceding_features_establish_body(ir),
        );
        return Some(extrude_feature_definition_with_profile(
            scan, ir, feature_id, op,
        ));
    }
    if kind == "Revolve" || numbered_feature_name_has_family(kind, "Revolve") {
        let output_kind = sweep_output_kind(scan, ir, "revolution", feature_id);
        let op = section_sweep_boolean_operation(
            feature_recipe_effect(scan, feature_id),
            kind,
            output_kind.is_some(),
            preceding_features_establish_body(ir),
        );
        return Some(revolve_feature_definition_with_profile(
            scan, ir, feature_id, op,
        ));
    }
    let schema_class = match kind {
        "Datum Plane" | "Bezugsebene" => 923,
        "Hole" => 911,
        "Round" | "Rundung" => 913,
        "Chamfer" => 914,
        "Draft" | "Schräge" => 927,
        _ => return None,
    };
    Some(schema_feature_definition(
        scan,
        ir,
        feature_id,
        schema_class,
        kind,
    ))
}

pub(in super::super) fn named_or_referenced_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    named_feature_definition(scan, ir, feature_id, kind).or_else(|| {
        if kind == "Native Feature"
            && current_feature_operation(&scan.features.operations, feature_id)
                .is_some_and(|operation| operation.display_state_conflict)
        {
            return None;
        }
        feature_reference_name(scan, feature_id)
            .filter(|reference_name| *reference_name != kind)
            .and_then(|reference_name| {
                named_feature_definition(scan, ir, feature_id, reference_name)
            })
    })
}

pub(in super::super) fn extrude_feature_definition_with_profile(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    op: BooleanOp,
) -> IrFeatureDefinition {
    let profile = unique_feature_profile_ref(scan, ir, feature_id)
        .unwrap_or_else(|| ProfileRef::Unresolved(format!("creo:model:feature#{feature_id}")));
    let output_kind = sweep_output_kind(scan, ir, "extrusion", feature_id);
    let op = if op == BooleanOp::Unresolved && output_kind == Some(BodyKind::Sheet) {
        BooleanOp::NewBody
    } else {
        op
    };
    let (direction, extent) = linear_extrusion_extent_and_direction(scan, ir, feature_id).map_or(
        (ExtrudeDirection::ProfileNormal, unresolved_extrude_extent()),
        |(extent, direction)| {
            (
                ExtrudeDirection::Explicit(Vector3::new(direction[0], direction[1], direction[2])),
                extent,
            )
        },
    );
    IrFeatureDefinition::Extrude {
        profile,
        direction,
        start: cadmpeg_ir::features::ExtrudeStart::default(),
        extent,
        op,
        direction_source: None,
        solid: sweep_solid(output_kind),
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }
}

pub(in super::super) fn revolve_feature_definition_with_profile(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    op: BooleanOp,
) -> IrFeatureDefinition {
    let extent = feature_revolution_extent(scan, feature_id);
    let output_kind = sweep_output_kind(scan, ir, "revolution", feature_id);
    IrFeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile: unique_feature_profile_ref(scan, ir, feature_id),
            axis: feature_revolution_axis_for_transfer(scan, ir, feature_id, extent.as_ref()),
            extent,
            axis_reference: None,
            solid: sweep_solid(output_kind),
            face_maker_class: None,
            fuse_order: None,
            allow_multi_profile_faces: None,
        },
        op,
    }
}

pub(in super::super) fn unresolved_extrude_extent() -> ExtrudeExtent {
    ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: LinearTermination::Unresolved,
            draft: None,
            offset: None,
        },
    }
}

pub(in super::super) fn surface_intersect_feature_definition(
    scan: &ContainerScan,
    feature_id: u32,
    kind: &str,
) -> Option<IrFeatureDefinition> {
    numbered_feature_name_has_family(kind, "Intersect").then_some(())?;
    let mut surface_tables = scan.features.entity_tables.iter().filter(|table| {
        table.feature_id == Some(feature_id)
            && table.table_class_id == 29
            && !table.surface_ids.is_empty()
            && table
                .surface_ids
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                == table.surface_ids.len()
            && table.surface_ids.iter().all(|surface_id| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id)
                    .is_some_and(|surface| surface.feature_id == feature_id)
            })
    });
    surface_tables.next()?;
    surface_tables.next().is_none().then_some(())?;
    Some(IrFeatureDefinition::SectionShape {
        first: BodySelection::Unresolved,
        second: BodySelection::Unresolved,
        approximate: None,
    })
}

pub(in super::super) fn reference_named_feature_definition(
    kind: &str,
) -> Option<IrFeatureDefinition> {
    if numbered_feature_name_has_family(kind, "Boundary Blend") {
        return Some(IrFeatureDefinition::BoundarySurfaceUnresolved);
    }
    if numbered_feature_name_has_family(kind, "Thicken") {
        return Some(IrFeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        });
    }
    if numbered_feature_name_has_family(kind, "Merge") {
        return Some(IrFeatureDefinition::KnitSurface {
            faces: FaceSelection::Unresolved,
            merge_entities: Some(true),
            create_solid: Some(false),
            gap_tolerance: None,
        });
    }
    None
}

pub(in super::super) fn retain_native_feature_parameters(
    source_properties: &mut BTreeMap<String, String>,
    definition: &IrFeatureDefinition,
    parameters: &BTreeMap<String, String>,
) {
    if matches!(definition, IrFeatureDefinition::Native { .. }) {
        return;
    }
    for (name, value) in parameters {
        source_properties.insert(format!("native_parameter.{name}"), value.clone());
    }
}
