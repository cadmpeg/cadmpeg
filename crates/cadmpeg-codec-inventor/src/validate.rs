// SPDX-License-Identifier: Apache-2.0
//! Inventor-native validation.

use std::collections::{HashMap, HashSet};

use cadmpeg_asm::brep::records::FaceNativeKey;
use cadmpeg_ir::{CadIr, Check, Finding, NativeUnknownRecord, Severity};

use crate::design::{PmDcExpression, PmDcExpressionKind, PmDcParameter, PmDcUnit, PmDcUnitKind};
use crate::feature::{
    PmDcEntityStyleLink, PmDcFeature, PmDcFeatureLabel, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureTerminator, PmDcPatternFeature,
};
use crate::sketch::{
    PmDcDirection, PmDcSketch, PmDcSketchConstraint, PmDcSketchConstraintKind, PmDcSketchEntity,
    PmDcSketchEntityKind, PmDcTransform, PointTail,
};

use crate::native::protein::{ProteinAssetRecord, ProteinRecord, ProteinRejectionRecord};
use crate::native::ufrx::UfrxRecord;
use crate::native::{
    ActiveCarrierRecord, AssemblyOccurrenceRecord, AssemblyPlacementRecord, DatabaseIssueRecord,
    DatabaseRecord, MetaSectionRecord, MetaTypeRecord, PmAppDefaultStyleRecord,
    PmAppRenderingStyleRecord, PmGraphicsFaceRecord, PmGraphicsPrimaryColorStyleRecord,
    PmGraphicsStyleCollectionRecord, PropertyRecord, PropertySectionRecord, PropertySetIssueRecord,
    PropertySetRecord, RevisionRecord, RseRecordRecord, SegmentBulkIssueRecord, SegmentBulkRecord,
    SegmentMetaIssueRecord, SegmentMetaRecord, SegmentPairRecord, SegmentRegistryRecord,
    StorageBandRecord, StructuralIssueRecord, UnpairedSegmentRecord,
};
use crate::pmdc::PmDcReferenceList;
use crate::record_issue::RecordIssue;

const ARENAS: &[&str] = &[
    "active_carrier",
    "assembly_occurrences",
    "assembly_placements",
    "assembly_record_issues",
    "body_native_keys",
    "database_issues",
    "databases",
    "design_record_issues",
    "embedded_references",
    "external_references",
    "feature_record_issues",
    "edge_continuities",
    "edge_ownerships",
    "face_sidedness",
    "face_native_keys",
    "meta_sections",
    "meta_types",
    "mesh_surface_sentinels",
    "properties",
    "pm_app_default_styles",
    "pm_app_rendering_styles",
    "pm_dc_expressions",
    "pm_dc_feature_labels",
    "pm_dc_feature_properties",
    "pm_dc_feature_terminators",
    "pm_dc_features",
    "pm_dc_pattern_features",
    "pm_dc_entity_style_links",
    "pm_dc_parameters",
    "pm_dc_directions",
    "pm_dc_sketch_entities",
    "pm_dc_sketch_constraints",
    "pm_dc_sketches",
    "pm_dc_transforms",
    "pm_dc_units",
    "pm_graphics_faces",
    "pm_graphics_style_collections",
    "pm_graphics_primary_color_styles",
    "presentation_record_issues",
    "property_sections",
    "property_set_issues",
    "property_sets",
    "protein",
    "protein_assets",
    "protein_entries",
    "protein_rejections",
    "revisions",
    "rse_records",
    "segment_bulk",
    "segment_bulk_issues",
    "segment_meta",
    "segment_meta_issues",
    "segment_pairs",
    "segment_registry",
    "sketch_record_issues",
    "storage_bands",
    "structural_issues",
    "tolerant_coedge_parameters",
    "tolerant_edge_tails",
    "tolerant_vertex_tails",
    "transform_hints",
    "ufrx",
    "ufrx_model_states",
    "ufrx_occurrences",
    "unknowns",
    "unpaired_segments",
    "vertex_ownerships",
    "wire_topologies",
];

pub(crate) fn validate_native(ir: &CadIr) -> Vec<Finding> {
    let Some(namespace) = ir.native.namespace("inventor") else {
        return Vec::new();
    };
    let actual_arenas = namespace
        .arenas()
        .keys()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let expected_arenas = ARENAS.iter().copied().collect::<HashSet<_>>();
    if actual_arenas != expected_arenas {
        let mut missing = expected_arenas
            .difference(&actual_arenas)
            .copied()
            .collect::<Vec<_>>();
        let mut unexpected = actual_arenas
            .difference(&expected_arenas)
            .copied()
            .collect::<Vec<_>>();
        missing.sort_unstable();
        unexpected.sort_unstable();
        return vec![finding(
            Check::NativeLinks,
            format!(
                "Inventor native namespace has missing arenas {missing:?} and unexpected arenas {unexpected:?}"
            ),
            None,
        )];
    }
    let data = match NativeData::load(namespace) {
        Ok(data) => data,
        Err(error) => {
            return vec![finding(
                Check::NativeLinks,
                format!("Inventor native arenas are invalid: {error}"),
                None,
            )];
        }
    };

    let mut findings = Vec::new();
    validate_databases(&data, &mut findings);
    validate_segments(&data, &mut findings);
    validate_active_carrier(&data, &mut findings);
    validate_design(&data, ir, &mut findings);
    validate_sketches(&data, ir, &mut findings);
    validate_features(ir, &data, &mut findings);
    unique(
        &mut findings,
        data.unknowns.iter().map(|record| record.id.as_str()),
        "ASM unknown-record id",
    );
    validate_properties(&data, &mut findings);
    validate_protein(&data, &mut findings);
    unique(
        &mut findings,
        data.protein_assets.iter().map(|record| record.id.as_str()),
        "Protein asset id",
    );
    validate_protein_assets(&data, &mut findings);
    validate_protein_rejections(&data, &mut findings);
    validate_protein_record_coverage(&data, &mut findings);
    validate_ufrx(ir, &data, &mut findings);
    validate_assembly(ir, &data, &mut findings);
    validate_presentation(ir, &data, &mut findings);
    for issue in &data.structural_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor {}: {}", issue.scope, issue.detail),
            Some(issue.id.clone()),
        ));
    }
    for issue in &data.property_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor property set {:?}: {}", issue.path, issue.detail),
            Some(issue.id.clone()),
        ));
    }
    findings
}

fn validate_design(data: &NativeData, ir: &CadIr, findings: &mut Vec<Finding>) {
    let raw = data
        .records
        .iter()
        .map(|record| {
            (
                (record.token.as_str(), record.ordinal),
                record.type_id.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();
    let resolves = |token: &str, reference: u32| {
        reference == 0 || raw.contains_key(&(token, reference.saturating_sub(1)))
    };
    unique(
        findings,
        data.pm_dc_parameters.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc parameter",
    );
    unique(
        findings,
        data.pm_dc_expressions.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc expression",
    );
    unique(
        findings,
        data.pm_dc_units.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc unit",
    );
    for parameter in &data.pm_dc_parameters {
        let references = [
            parameter.header.next.index,
            parameter.header.context.index,
            parameter.unit.index,
            parameter.formula.index,
        ];
        if raw.get(&(
            parameter.identity.segment_token.as_str(),
            parameter.identity.record_ordinal,
        )) != Some(&parameter.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&parameter.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc parameter record or reference does not resolve".into(),
                Some(format!(
                    "inventor:pmdc:parameter#{}-{}",
                    parameter.identity.segment_token, parameter.identity.record_ordinal
                )),
            ));
        }
    }
    for expression in &data.pm_dc_expressions {
        let mut references = vec![expression.unit.index];
        match &expression.kind {
            PmDcExpressionKind::Value { .. } => {}
            PmDcExpressionKind::ParameterReference { operand, .. }
            | PmDcExpressionKind::Unary { operand, .. } => references.push(operand.index),
            PmDcExpressionKind::Binary { left, right, .. } => {
                references.push(left.index);
                references.push(right.index);
            }
        }
        if raw.get(&(
            expression.identity.segment_token.as_str(),
            expression.identity.record_ordinal,
        )) != Some(&expression.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&expression.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc expression record or reference does not resolve".into(),
                Some(format!(
                    "inventor:pmdc:expression#{}-{}",
                    expression.identity.segment_token, expression.identity.record_ordinal
                )),
            ));
        }
    }
    for unit in &data.pm_dc_units {
        let references = match &unit.kind {
            PmDcUnitKind::Definition {
                numerators,
                denominators,
                derived,
                ..
            } => numerators
                .references()
                .iter()
                .chain(denominators.references())
                .map(|reference| reference.index)
                .chain(std::iter::once(derived.index))
                .collect::<Vec<_>>(),
            PmDcUnitKind::Base { .. } => Vec::new(),
        };
        if raw.get(&(
            unit.identity.segment_token.as_str(),
            unit.identity.record_ordinal,
        )) != Some(&unit.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&unit.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc unit record or reference does not resolve".into(),
                Some(format!(
                    "inventor:pmdc:unit#{}-{}",
                    unit.identity.segment_token, unit.identity.record_ordinal
                )),
            ));
        }
    }
    let native_parameter_ids = data
        .pm_dc_parameters
        .iter()
        .map(|record| {
            format!(
                "inventor:pmdc:parameter#{}-{}",
                record.identity.segment_token, record.identity.record_ordinal
            )
        })
        .collect::<HashSet<_>>();
    for parameter in &ir.model.parameters {
        if parameter
            .native_ref
            .as_ref()
            .is_none_or(|reference| !native_parameter_ids.contains(reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral parameter does not resolve to its PmDc source record".into(),
                Some(parameter.id.as_str().to_owned()),
            ));
        }
    }
    for issue in &data.design_record_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor design record: {}", issue.detail),
            Some(format!(
                "inventor:pmdc:record#{}-{}",
                issue.segment_token, issue.record_ordinal
            )),
        ));
    }
}

fn validate_sketches(data: &NativeData, ir: &CadIr, findings: &mut Vec<Finding>) {
    let raw = data
        .records
        .iter()
        .map(|record| {
            (
                (record.token.as_str(), record.ordinal),
                record.type_id.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();
    let record_is_exact = |token: &str, ordinal: u32, type_id: &str| {
        raw.get(&(token, ordinal)).copied() == Some(type_id)
    };
    let references_resolve = |token: &str, references: &[u32]| {
        references.iter().all(|reference| {
            *reference == 0 || raw.contains_key(&(token, reference.saturating_sub(1)))
        })
    };
    unique(
        findings,
        data.pm_dc_sketches.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc sketch",
    );
    unique(
        findings,
        data.pm_dc_sketch_entities.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc sketch entity",
    );
    unique(
        findings,
        data.pm_dc_transforms.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc transform",
    );
    unique(
        findings,
        data.pm_dc_sketch_constraints.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc sketch constraint",
    );
    unique(
        findings,
        data.pm_dc_directions.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc direction",
    );
    for sketch in &data.pm_dc_sketches {
        let references = [
            sketch.header.next.index,
            sketch.header.context.index,
            sketch.transform.index,
            sketch.direction.index,
        ]
        .into_iter()
        .chain(
            sketch
                .entities
                .references()
                .iter()
                .map(|reference| reference.index),
        )
        .chain(
            sketch
                .auxiliary
                .iter()
                .flat_map(super::pmdc::PmDcReferenceList::references)
                .map(|reference| reference.index),
        )
        .collect::<Vec<_>>();
        if !record_is_exact(
            &sketch.identity.segment_token,
            sketch.identity.record_ordinal,
            &sketch.identity.type_id,
        ) || !references_resolve(&sketch.identity.segment_token, &references)
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc sketch record or reference does not resolve".into(),
                Some(sketch.id()),
            ));
        }
    }
    for entity in &data.pm_dc_sketch_entities {
        let mut references = vec![
            entity.header.next.index,
            entity.header.context.index,
            entity.sketch.index,
        ];
        let mut add_list = |list: &PmDcReferenceList| {
            references.extend(list.references().iter().map(|reference| reference.index));
        };
        match &entity.kind {
            PmDcSketchEntityKind::Point {
                endpoint_of,
                center_of,
                tail,
                ..
            } => {
                add_list(endpoint_of);
                add_list(center_of);
                if let PointTail::Present { associations, .. } = tail {
                    add_list(associations);
                }
            }
            PmDcSketchEntityKind::Line {
                points, auxiliary, ..
            } => {
                add_list(points);
                for list in auxiliary {
                    add_list(list);
                }
            }
            PmDcSketchEntityKind::Circle {
                points,
                auxiliary,
                center,
                ..
            }
            | PmDcSketchEntityKind::Ellipse {
                points,
                auxiliary,
                center,
                ..
            } => {
                add_list(points);
                for list in auxiliary {
                    add_list(list);
                }
                references.push(center.index);
            }
        }
        if !record_is_exact(
            &entity.identity.segment_token,
            entity.identity.record_ordinal,
            &entity.identity.type_id,
        ) || !references_resolve(&entity.identity.segment_token, &references)
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc sketch-entity record or reference does not resolve".into(),
                Some(entity.id()),
            ));
        }
    }
    for transform in &data.pm_dc_transforms {
        if !record_is_exact(
            &transform.identity.segment_token,
            transform.identity.record_ordinal,
            &transform.identity.type_id,
        ) || !references_resolve(
            &transform.identity.segment_token,
            &[transform.header.next.index, transform.header.context.index],
        ) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc transform record or reference does not resolve".into(),
                Some(transform.id()),
            ));
        }
    }
    for constraint in &data.pm_dc_sketch_constraints {
        let header = &constraint.header;
        let mut references = vec![
            header.content.next.index,
            header.content.context.index,
            header.group.index,
            header.parameter.index,
        ];
        for (key, _) in header.scalar_map.entries() {
            references.push(key.index);
        }
        for (key, value) in header.reference_map.entries() {
            references.extend([key.index, value.index]);
        }
        match constraint.kind {
            PmDcSketchConstraintKind::Coincident { first, second }
            | PmDcSketchConstraintKind::Parallel { first, second, .. }
            | PmDcSketchConstraintKind::Perpendicular { first, second, .. }
            | PmDcSketchConstraintKind::Tangent { first, second, .. } => {
                references.extend([first.index, second.index]);
            }
            PmDcSketchConstraintKind::Horizontal { entity, .. }
            | PmDcSketchConstraintKind::Vertical { entity, .. } => {
                references.push(entity.index);
            }
            PmDcSketchConstraintKind::HorizontalDistance {
                first,
                second,
                parameter,
                ..
            }
            | PmDcSketchConstraintKind::VerticalDistance {
                first,
                second,
                parameter,
                ..
            } => references.extend([first.index, second.index, parameter.index]),
            PmDcSketchConstraintKind::Radius { entity, .. } => {
                references.push(entity.index);
            }
            PmDcSketchConstraintKind::Diameter {
                reference, entity, ..
            } => references.extend([reference.index, entity.index]),
            PmDcSketchConstraintKind::CircleCenter { entity, center } => {
                references.extend([entity.index, center.index]);
            }
            PmDcSketchConstraintKind::EqualRadius { first, second } => {
                references.extend([first.index, second.index]);
            }
        }
        if !record_is_exact(
            &constraint.identity.segment_token,
            constraint.identity.record_ordinal,
            &constraint.identity.type_id,
        ) || !references_resolve(&constraint.identity.segment_token, &references)
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc sketch-constraint record or reference does not resolve".into(),
                Some(constraint.id()),
            ));
        }
    }
    for direction in &data.pm_dc_directions {
        if !record_is_exact(
            &direction.identity.segment_token,
            direction.identity.record_ordinal,
            &direction.identity.type_id,
        ) || !references_resolve(
            &direction.identity.segment_token,
            &[direction.header.next.index, direction.header.context.index],
        ) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc direction record or reference does not resolve".into(),
                Some(direction.id()),
            ));
        }
    }
    let native_sketches = data
        .pm_dc_sketches
        .iter()
        .map(crate::record_identity::Located::id)
        .collect::<HashSet<_>>();
    let native_entities = data
        .pm_dc_sketch_entities
        .iter()
        .map(crate::record_identity::Located::id)
        .collect::<HashSet<_>>();
    let native_constraints = data
        .pm_dc_sketch_constraints
        .iter()
        .map(crate::record_identity::Located::id)
        .collect::<HashSet<_>>();
    for sketch in &ir.model.sketches {
        if sketch
            .native_ref
            .as_deref()
            .is_none_or(|reference| !native_sketches.contains(reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral sketch does not resolve to its PmDc source record".into(),
                Some(sketch.id.as_str().to_owned()),
            ));
        }
    }
    for entity in &ir.model.sketch_entities {
        if entity
            .native_ref
            .as_deref()
            .is_none_or(|reference| !native_entities.contains(reference))
            || entity
                .endpoint_refs
                .iter()
                .any(|reference| !native_entities.contains(reference.as_str()))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral sketch entity does not resolve to its PmDc source records".into(),
                Some(entity.id().as_str().to_owned()),
            ));
        }
    }
    for constraint in &ir.model.sketch_constraints {
        if constraint
            .native_ref
            .as_deref()
            .is_none_or(|reference| !native_constraints.contains(reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral sketch constraint does not resolve to its PmDc source record"
                    .into(),
                Some(constraint.id.as_str().to_owned()),
            ));
        }
    }
    for issue in &data.sketch_record_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor sketch record: {}", issue.detail),
            Some(issue.id()),
        ));
    }
}

fn validate_features(ir: &CadIr, data: &NativeData, findings: &mut Vec<Finding>) {
    let raw = data
        .records
        .iter()
        .map(|record| {
            (
                (record.token.as_str(), record.ordinal),
                record.type_id.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();
    let resolves = |token: &str, reference: u32| {
        reference == 0 || raw.contains_key(&(token, reference.saturating_sub(1)))
    };
    unique(
        findings,
        data.pm_dc_features.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc feature",
    );
    unique(
        findings,
        data.pm_dc_feature_terminators.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc feature terminator",
    );
    unique(
        findings,
        data.pm_dc_pattern_features.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc pattern feature",
    );
    unique(
        findings,
        data.pm_dc_feature_properties.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc feature property",
    );
    unique(
        findings,
        data.pm_dc_feature_labels.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc feature label",
    );
    for feature in &data.pm_dc_features {
        let references = [feature.header.next.index, feature.header.context.index]
            .into_iter()
            .chain(
                feature
                    .properties
                    .references()
                    .iter()
                    .map(|reference| reference.index),
            );
        if raw.get(&(
            feature.identity.segment_token.as_str(),
            feature.identity.record_ordinal,
        )) != Some(&feature.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&feature.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc feature record or reference does not resolve".into(),
                Some(feature.id()),
            ));
        }
    }
    for feature in &data.pm_dc_pattern_features {
        let references = [feature.header.next.index, feature.header.context.index]
            .into_iter()
            .chain(
                feature
                    .properties
                    .references()
                    .iter()
                    .map(|reference| reference.index),
            )
            .chain(
                feature
                    .participants
                    .references()
                    .iter()
                    .map(|reference| reference.index),
            )
            .chain(
                feature
                    .property_slots
                    .iter()
                    .map(|reference| reference.index),
            );
        if raw.get(&(
            feature.identity.segment_token.as_str(),
            feature.identity.record_ordinal,
        )) != Some(&feature.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&feature.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc pattern-feature record or reference does not resolve".into(),
                Some(feature.id()),
            ));
        }
    }
    for property in &data.pm_dc_feature_properties {
        let mut references = vec![property.header.next.index, property.header.context.index];
        match &property.kind {
            PmDcFeaturePropertyKind::References { items, .. } => {
                references.extend(items.references().iter().map(|reference| reference.index));
            }
            PmDcFeaturePropertyKind::SurfaceBody { body } => references.push(body.index),
            PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } => {
                references.push(entity_link.index);
            }
            PmDcFeaturePropertyKind::Placement {
                transform,
                point,
                value,
            } => references.extend([transform.index, point.index, value.index]),
            PmDcFeaturePropertyKind::FilletEdgeSet {
                edges,
                radius,
                selection,
                continuity,
            } => references.extend([edges.index, radius.index, selection.index, continuity.index]),
            PmDcFeaturePropertyKind::Enumeration { .. }
            | PmDcFeaturePropertyKind::WideEnumeration { .. }
            | PmDcFeaturePropertyKind::Boolean { .. }
            | PmDcFeaturePropertyKind::RdxVariable { .. }
            | PmDcFeaturePropertyKind::EdgeItem { .. } => {}
        }
        if raw.get(&(
            property.identity.segment_token.as_str(),
            property.identity.record_ordinal,
        )) != Some(&property.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&property.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc feature-property record or reference does not resolve".into(),
                Some(property.id()),
            ));
        }
    }
    for link in &data.pm_dc_entity_style_links {
        let references = [
            link.header.owner.index,
            link.header.parent.index,
            link.header.next.index,
        ];
        if raw.get(&(
            link.identity.segment_token.as_str(),
            link.identity.record_ordinal,
        )) != Some(&link.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&link.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc entity-style-link record or reference does not resolve".into(),
                Some(link.id()),
            ));
        }
    }
    for label in &data.pm_dc_feature_labels {
        let references = [
            label.header.owner.index,
            label.header.parent.index,
            label.header.next.index,
        ]
        .into_iter()
        .chain(
            label
                .participants
                .references()
                .iter()
                .map(|reference| reference.index),
        );
        if raw.get(&(
            label.identity.segment_token.as_str(),
            label.identity.record_ordinal,
        )) != Some(&label.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&label.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc feature-label record or reference does not resolve".into(),
                Some(label.id()),
            ));
        }
    }
    for terminator in &data.pm_dc_feature_terminators {
        let references = [
            terminator.header.next.index,
            terminator.header.context.index,
        ];
        if raw.get(&(
            terminator.identity.segment_token.as_str(),
            terminator.identity.record_ordinal,
        )) != Some(&terminator.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(&terminator.identity.segment_token, reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc feature-terminator record or reference does not resolve".into(),
                Some(terminator.id()),
            ));
        }
    }
    let raw_features = data
        .pm_dc_features
        .iter()
        .map(|feature| (feature.id(), feature))
        .collect::<HashMap<_, _>>();
    let labels = data
        .pm_dc_feature_labels
        .iter()
        .filter_map(|label| {
            Some((
                (
                    label.identity.segment_token.as_str(),
                    label.header.owner.index.checked_sub(1)?,
                ),
                label,
            ))
        })
        .collect::<HashMap<_, _>>();
    let properties = data
        .pm_dc_feature_properties
        .iter()
        .map(|property| (property.id(), property))
        .collect::<HashMap<_, _>>();
    let properties_by_record = data
        .pm_dc_feature_properties
        .iter()
        .map(|property| {
            (
                (
                    property.identity.segment_token.as_str(),
                    property.identity.record_ordinal,
                ),
                property,
            )
        })
        .collect::<HashMap<_, _>>();
    let results = ir
        .model
        .feature_result_topologies
        .iter()
        .map(|result| (&result.output_of, result))
        .collect::<HashMap<_, _>>();
    for feature in &ir.model.features {
        let Some(raw_feature) = feature
            .native_ref
            .as_deref()
            .and_then(|native| raw_features.get(native).copied())
        else {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral feature does not resolve to its PmDc source record".into(),
                Some(feature.id.as_str().to_owned()),
            ));
            continue;
        };
        let Some(family) =
            crate::feature::FeatureFamily::from_definition(feature.evaluation.definition())
        else {
            continue;
        };
        let expected_class = family.class_id();
        let output_slot = family.output_slot();
        if labels
            .get(&(
                raw_feature.identity.segment_token.as_str(),
                raw_feature.identity.record_ordinal,
            ))
            .is_none_or(|label| label.class_id() != expected_class)
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral feature family does not match its PmDc label".into(),
                Some(feature.id.as_str().to_owned()),
            ));
        }
        let expected_collection = raw_feature
            .properties
            .references()
            .get(output_slot)
            .and_then(|reference| reference.index.checked_sub(1))
            .and_then(|ordinal| {
                properties_by_record
                    .get(&(raw_feature.identity.segment_token.as_str(), ordinal))
                    .copied()
            });
        if expected_collection.is_none_or(|collection| {
            results
                .get(&feature.id)
                .and_then(|result| result.native_ref.as_deref())
                != Some(collection.id().as_str())
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral feature result does not match its PmDc output slot".into(),
                Some(feature.id.as_str().to_owned()),
            ));
        }
    }
    for result in &ir.model.feature_result_topologies {
        let Some(collection) = result
            .native_ref
            .as_deref()
            .and_then(|native| properties.get(native).copied())
        else {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor feature result does not resolve to its PmDc object collection".into(),
                Some(result.id.as_str().to_owned()),
            ));
            continue;
        };
        let PmDcFeaturePropertyKind::References {
            family: crate::feature::PmDcFeatureReferenceFamily::ObjectCollection,
            items,
        } = &collection.kind
        else {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor feature result native reference is not an object collection".into(),
                Some(result.id.as_str().to_owned()),
            ));
            continue;
        };
        let expected_bodies = items
            .references()
            .iter()
            .filter_map(|reference| {
                let ordinal = reference.index.checked_sub(1)?;
                properties_by_record
                    .get(&(collection.identity.segment_token.as_str(), ordinal))
                    .filter(|property| {
                        matches!(property.kind, PmDcFeaturePropertyKind::SurfaceBody { .. })
                    })
                    .map(|property| property.id())
            })
            .collect::<Vec<_>>();
        if expected_bodies.len() != items.references().len()
            || expected_bodies
                != result
                    .bodies()
                    .iter()
                    .map(cadmpeg_ir::products::NonEmptyString::as_str)
                    .collect::<Vec<_>>()
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor feature result bodies do not match its PmDc object collection".into(),
                Some(result.id.as_str().to_owned()),
            ));
        }
    }
    for issue in &data.feature_record_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor feature record: {}", issue.detail),
            Some(issue.id()),
        ));
    }
}

fn validate_presentation(ir: &CadIr, data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.pm_app_default_styles
            .iter()
            .map(|record| record.id.as_str()),
        "PmApp default-style id",
    );
    unique(
        findings,
        data.pm_app_rendering_styles
            .iter()
            .map(|record| record.id.as_str()),
        "PmApp rendering-style id",
    );
    unique(
        findings,
        data.pm_app_default_styles
            .iter()
            .map(|record| (record.segment_token.as_str(), record.record_ordinal)),
        "PmApp default-style record key",
    );
    unique(
        findings,
        data.pm_app_rendering_styles
            .iter()
            .map(|record| (record.segment_token.as_str(), record.record_ordinal)),
        "PmApp rendering-style record key",
    );
    unique(
        findings,
        data.pm_graphics_faces
            .iter()
            .map(|record| record.id.as_str()),
        "PmGraphics face id",
    );
    unique(
        findings,
        data.pm_graphics_faces
            .iter()
            .map(|record| (record.segment_token.as_str(), record.record_ordinal)),
        "PmGraphics face record key",
    );
    unique(
        findings,
        data.pm_graphics_style_collections
            .iter()
            .map(|record| record.id.as_str()),
        "PmGraphics style-collection id",
    );
    unique(
        findings,
        data.pm_graphics_style_collections
            .iter()
            .map(|record| (record.segment_token.as_str(), record.record_ordinal)),
        "PmGraphics style-collection record key",
    );
    unique(
        findings,
        data.pm_graphics_primary_color_styles
            .iter()
            .map(|record| record.id.as_str()),
        "PmGraphics primary-color style id",
    );
    unique(
        findings,
        data.pm_graphics_primary_color_styles
            .iter()
            .map(|record| (record.segment_token.as_str(), record.record_ordinal)),
        "PmGraphics primary-color style record key",
    );
    unique(
        findings,
        data.face_native_keys
            .iter()
            .map(cadmpeg_asm::brep::records::FaceNativeKey::id),
        "ASM face-native-key id",
    );
    unique(
        findings,
        data.face_native_keys
            .iter()
            .filter_map(|record| record.asm_face_key),
        "ASM non-null face Design key",
    );
    let neutral_faces = ir
        .model
        .faces
        .iter()
        .map(|face| &face.id)
        .collect::<HashSet<_>>();
    for record in &data.face_native_keys {
        if !neutral_faces.contains(&record.face) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor ASM face-native-key record does not resolve to a neutral face".into(),
                Some(record.id()),
            ));
        }
    }

    let raw_records = data
        .records
        .iter()
        .map(|record| {
            (
                (record.token.as_str(), record.ordinal),
                record.type_id.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();
    let raw_keys = raw_records.keys().copied().collect::<HashSet<_>>();
    let rendering_keys = data
        .pm_app_rendering_styles
        .iter()
        .map(|record| (record.segment_token.as_str(), record.record_ordinal))
        .collect::<HashSet<_>>();
    for record in &data.pm_app_default_styles {
        let key = (record.segment_token.as_str(), record.record_ordinal);
        if raw_records.get(&key) != Some(&"cdecfb11d1116b250008ebbb21eddc09") {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmApp default style does not resolve to its RSe record".into(),
                Some(record.id.clone()),
            ));
        }
        let target = record
            .rendering_style_reference
            .checked_sub(1)
            .map(|ordinal| (record.segment_token.as_str(), ordinal));
        if target.is_some_and(|target| !rendering_keys.contains(&target)) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmApp default rendering-style reference does not resolve".into(),
                Some(record.id.clone()),
            ));
        }
    }
    for record in &data.pm_app_rendering_styles {
        let key = (record.segment_token.as_str(), record.record_ordinal);
        if raw_records.get(&key) != Some(&"6fd85967d2113878600094b70b02ecb0") {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmApp rendering style does not resolve to its RSe record".into(),
                Some(record.id.clone()),
            ));
        }
    }
    for record in &data.pm_graphics_faces {
        let key = (record.segment_token.as_str(), record.record_ordinal);
        if raw_records.get(&key) != Some(&"a3e99451d2119b2860006ab72c39cdb0") {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmGraphics face does not resolve to its RSe record".into(),
                Some(record.id.clone()),
            ));
        }
        for reference in std::iter::once(record.surface.index)
            .chain(std::iter::once(record.parent.index))
            .chain(
                record
                    .edge_references
                    .references()
                    .iter()
                    .map(|reference| reference.index),
            )
            .filter(|reference| *reference != 0)
        {
            let Some(ordinal) = reference.checked_sub(1) else {
                continue;
            };
            if !raw_keys.contains(&(record.segment_token.as_str(), ordinal)) {
                findings.push(finding(
                    Check::NativeLinks,
                    format!("Inventor PmGraphics face reference {reference} does not resolve"),
                    Some(record.id.clone()),
                ));
            }
        }
        if record.styles.index != 0 {
            let target = record.styles.index - 1;
            if raw_records.get(&(record.segment_token.as_str(), target))
                != Some(&"0786eb48d2110c076000f99ac5361ab0")
            {
                findings.push(finding(
                    Check::NativeLinks,
                    "Inventor PmGraphics face style reference does not resolve to a style collection"
                        .into(),
                    Some(record.id.clone()),
                ));
            }
        }
    }
    for record in &data.pm_graphics_style_collections {
        let key = (record.segment_token.as_str(), record.record_ordinal);
        if raw_records.get(&key) != Some(&"0786eb48d2110c076000f99ac5361ab0") {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmGraphics style collection does not resolve to its RSe record".into(),
                Some(record.id.clone()),
            ));
        }
        for reference in record.style_references.references() {
            if reference.index == 0
                || !raw_keys.contains(&(
                    record.segment_token.as_str(),
                    reference.index.saturating_sub(1),
                ))
            {
                findings.push(finding(
                    Check::NativeLinks,
                    format!(
                        "Inventor PmGraphics style-collection reference {} does not resolve",
                        reference.index
                    ),
                    Some(record.id.clone()),
                ));
            }
        }
    }
    for record in &data.pm_graphics_primary_color_styles {
        let key = (record.segment_token.as_str(), record.record_ordinal);
        if raw_records.get(&key) != Some(&"0f5648afd411c78d1000d58dc04a0ab5") {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmGraphics primary-color style does not resolve to its RSe record".into(),
                Some(record.id.clone()),
            ));
        }
    }
    for issue in &data.presentation_record_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor presentation record: {}", issue.detail),
            Some(issue.id()),
        ));
    }
}

fn validate_active_carrier(data: &NativeData, findings: &mut Vec<Finding>) {
    let carrier = &data.active_carrier;
    if let ActiveCarrierRecord::Selected {
        id,
        segment_token,
        record_ordinal,
        ..
    } = carrier
    {
        let resolves = data.records.iter().any(|record| {
            record.token.as_str() == segment_token
                && record.ordinal == *record_ordinal
                && record.type_id == "5c5945f6d5113313100060a6bba647b5"
        });
        if !resolves {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor active carrier does not resolve to its typed RSe record".into(),
                Some(id.clone()),
            ));
        }
    }
}

struct NativeData {
    storage_bands: Vec<StorageBandRecord>,
    databases: Vec<DatabaseRecord>,
    database_issues: Vec<DatabaseIssueRecord>,
    registry: Vec<SegmentRegistryRecord>,
    revisions: Vec<RevisionRecord>,
    pairs: Vec<SegmentPairRecord>,
    metadata: Vec<SegmentMetaRecord>,
    meta_sections: Vec<MetaSectionRecord>,
    meta_types: Vec<MetaTypeRecord>,
    metadata_issues: Vec<SegmentMetaIssueRecord>,
    bulk: Vec<SegmentBulkRecord>,
    records: Vec<RseRecordRecord>,
    bulk_issues: Vec<SegmentBulkIssueRecord>,
    unpaired: Vec<UnpairedSegmentRecord>,
    structural_issues: Vec<StructuralIssueRecord>,
    property_sets: Vec<PropertySetRecord>,
    property_sections: Vec<PropertySectionRecord>,
    properties: Vec<PropertyRecord>,
    property_issues: Vec<PropertySetIssueRecord>,
    protein: ProteinRecord,
    protein_assets: Vec<ProteinAssetRecord>,
    protein_rejections: Vec<ProteinRejectionRecord>,
    ufrx: UfrxRecord,
    assembly_occurrences: Vec<AssemblyOccurrenceRecord>,
    assembly_placements: Vec<AssemblyPlacementRecord>,
    assembly_record_issues: Vec<RecordIssue>,
    pm_app_default_styles: Vec<PmAppDefaultStyleRecord>,
    pm_app_rendering_styles: Vec<PmAppRenderingStyleRecord>,
    pm_graphics_faces: Vec<PmGraphicsFaceRecord>,
    pm_graphics_style_collections: Vec<PmGraphicsStyleCollectionRecord>,
    pm_graphics_primary_color_styles: Vec<PmGraphicsPrimaryColorStyleRecord>,
    face_native_keys: Vec<FaceNativeKey>,
    presentation_record_issues: Vec<RecordIssue>,
    pm_dc_parameters: Vec<PmDcParameter>,
    pm_dc_expressions: Vec<PmDcExpression>,
    pm_dc_units: Vec<PmDcUnit>,
    design_record_issues: Vec<RecordIssue>,
    pm_dc_sketches: Vec<PmDcSketch>,
    pm_dc_sketch_entities: Vec<PmDcSketchEntity>,
    pm_dc_sketch_constraints: Vec<PmDcSketchConstraint>,
    pm_dc_transforms: Vec<PmDcTransform>,
    pm_dc_directions: Vec<PmDcDirection>,
    sketch_record_issues: Vec<RecordIssue>,
    pm_dc_features: Vec<PmDcFeature>,
    pm_dc_pattern_features: Vec<PmDcPatternFeature>,
    pm_dc_feature_terminators: Vec<PmDcFeatureTerminator>,
    pm_dc_feature_properties: Vec<PmDcFeatureProperty>,
    pm_dc_feature_labels: Vec<PmDcFeatureLabel>,
    pm_dc_entity_style_links: Vec<PmDcEntityStyleLink>,
    feature_record_issues: Vec<RecordIssue>,
    active_carrier: ActiveCarrierRecord,
    unknowns: Vec<NativeUnknownRecord>,
}

impl NativeData {
    fn load(
        namespace: &cadmpeg_ir::native::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::native::NativeConvertError> {
        Ok(Self {
            storage_bands: namespace.arena_as("storage_bands")?,
            databases: namespace.arena_as("databases")?,
            database_issues: namespace.arena_as("database_issues")?,
            registry: namespace.arena_as("segment_registry")?,
            revisions: namespace.arena_as("revisions")?,
            pairs: namespace.arena_as("segment_pairs")?,
            metadata: namespace.arena_as("segment_meta")?,
            meta_sections: namespace.arena_as("meta_sections")?,
            meta_types: namespace.arena_as("meta_types")?,
            metadata_issues: namespace.arena_as("segment_meta_issues")?,
            bulk: namespace.arena_as("segment_bulk")?,
            records: namespace.arena_as("rse_records")?,
            bulk_issues: namespace.arena_as("segment_bulk_issues")?,
            unpaired: namespace.arena_as("unpaired_segments")?,
            structural_issues: namespace.arena_as("structural_issues")?,
            property_sets: namespace.arena_as("property_sets")?,
            property_sections: namespace.arena_as("property_sections")?,
            properties: namespace.arena_as("properties")?,
            property_issues: namespace.arena_as("property_set_issues")?,
            protein: ProteinRecord::read(namespace)?,
            protein_assets: namespace.arena_as("protein_assets")?,
            protein_rejections: namespace.arena_as("protein_rejections")?,
            ufrx: UfrxRecord::read(namespace)?,
            assembly_occurrences: namespace.arena_as("assembly_occurrences")?,
            assembly_placements: namespace.arena_as("assembly_placements")?,
            assembly_record_issues: namespace.arena_as("assembly_record_issues")?,
            pm_app_default_styles: namespace.arena_as("pm_app_default_styles")?,
            pm_app_rendering_styles: namespace.arena_as("pm_app_rendering_styles")?,
            pm_graphics_faces: namespace.arena_as("pm_graphics_faces")?,
            pm_graphics_style_collections: namespace.arena_as("pm_graphics_style_collections")?,
            pm_graphics_primary_color_styles: namespace
                .arena_as("pm_graphics_primary_color_styles")?,
            face_native_keys: namespace.arena_as("face_native_keys")?,
            presentation_record_issues: namespace.arena_as("presentation_record_issues")?,
            pm_dc_parameters: namespace.arena_as("pm_dc_parameters")?,
            pm_dc_expressions: namespace.arena_as("pm_dc_expressions")?,
            pm_dc_units: namespace.arena_as("pm_dc_units")?,
            design_record_issues: namespace.arena_as("design_record_issues")?,
            pm_dc_sketches: namespace.arena_as("pm_dc_sketches")?,
            pm_dc_sketch_entities: namespace.arena_as("pm_dc_sketch_entities")?,
            pm_dc_sketch_constraints: namespace.arena_as("pm_dc_sketch_constraints")?,
            pm_dc_transforms: namespace.arena_as("pm_dc_transforms")?,
            pm_dc_directions: namespace.arena_as("pm_dc_directions")?,
            sketch_record_issues: namespace.arena_as("sketch_record_issues")?,
            pm_dc_features: namespace.arena_as("pm_dc_features")?,
            pm_dc_pattern_features: namespace.arena_as("pm_dc_pattern_features")?,
            pm_dc_feature_terminators: namespace.arena_as("pm_dc_feature_terminators")?,
            pm_dc_feature_properties: namespace.arena_as("pm_dc_feature_properties")?,
            pm_dc_feature_labels: namespace.arena_as("pm_dc_feature_labels")?,
            pm_dc_entity_style_links: namespace.arena_as("pm_dc_entity_style_links")?,
            feature_record_issues: namespace.arena_as("feature_record_issues")?,
            active_carrier: ActiveCarrierRecord::read(namespace)?,
            unknowns: namespace.arena_as("unknowns")?,
        })
    }
}

fn validate_databases(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.storage_bands.iter().map(|record| record.band),
        "storage band",
    );
    unique(
        findings,
        data.storage_bands
            .iter()
            .map(|record| record.database_directory_id),
        "database directory id",
    );
    let storage = data
        .storage_bands
        .iter()
        .map(|record| record.band)
        .collect::<HashSet<_>>();
    let states = data
        .databases
        .iter()
        .map(|record| record.band)
        .chain(data.database_issues.iter().map(|record| record.band))
        .collect::<Vec<_>>();
    unique(findings, states.iter().copied(), "database state band");
    if storage != states.into_iter().collect() {
        findings.push(finding(
            Check::NativeLinks,
            "Inventor database states do not cover the storage bands exactly".into(),
            None,
        ));
    }
    for issue in &data.database_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor database band {} is unavailable: {}",
                issue.band, issue.detail
            ),
            Some(issue.id.clone()),
        ));
    }
}

fn validate_segments(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.registry.iter().map(|record| record.ordinal),
        "registry ordinal",
    );
    unique(
        findings,
        data.registry
            .iter()
            .map(|record| record.segment_id.as_str()),
        "registry segment id",
    );
    unique(
        findings,
        data.revisions.iter().map(|record| record.ordinal),
        "revision ordinal",
    );
    unique(
        findings,
        data.pairs.iter().map(|record| record.token.as_str()),
        "segment token",
    );
    unique(
        findings,
        data.pairs.iter().map(|record| record.metadata_directory_id),
        "metadata directory id",
    );
    unique(
        findings,
        data.pairs.iter().map(|record| record.bulk_directory_id),
        "bulk directory id",
    );
    let registry_ids = data
        .registry
        .iter()
        .map(|record| record.segment_id.as_str())
        .collect::<HashSet<_>>();
    for meta in &data.metadata {
        if !registry_ids.contains(meta.segment_id.as_str()) {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "Inventor segment metadata {} has no registry identity",
                    meta.id
                ),
                Some(meta.id.clone()),
            ));
        }
    }
    let pair_tokens = data
        .pairs
        .iter()
        .map(|record| record.token.as_str())
        .collect::<HashSet<_>>();
    validate_segment_states(
        findings,
        &pair_tokens,
        data.metadata.iter().map(|record| record.token.as_str()),
        data.metadata_issues
            .iter()
            .map(|record| record.token.as_str()),
        "metadata",
    );
    validate_segment_states(
        findings,
        &pair_tokens,
        data.bulk.iter().map(|record| record.token.as_str()),
        data.bulk_issues.iter().map(|record| record.token.as_str()),
        "bulk",
    );
    let metadata_by_token = data
        .metadata
        .iter()
        .map(|record| (record.token.as_str(), record))
        .collect::<HashMap<_, _>>();
    unique(
        findings,
        data.meta_sections
            .iter()
            .map(|record| (record.token.as_str(), record.number)),
        "metadata section number",
    );
    unique(
        findings,
        data.meta_types
            .iter()
            .map(|record| (record.token.as_str(), record.index)),
        "metadata type index",
    );
    let sections_by_token = data.meta_sections.iter().fold(
        HashMap::<&str, HashSet<u8>>::new(),
        |mut sections, record| {
            sections
                .entry(record.token.as_str())
                .or_default()
                .insert(u8::from(record.number));
            sections
        },
    );
    let types_by_token =
        data.meta_types
            .iter()
            .fold(HashMap::<&str, HashSet<u8>>::new(), |mut types, record| {
                types
                    .entry(record.token.as_str())
                    .or_default()
                    .insert(record.index);
                types
            });
    for (token, meta) in metadata_by_token {
        let expected_sections = (1_u8..=11).collect::<HashSet<_>>();
        if sections_by_token.get(token) != Some(&expected_sections)
            || types_by_token.get(token).map_or(0, HashSet::len) as u64 != meta.type_count
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor metadata tables do not match their segment summary".into(),
                Some(meta.id.clone()),
            ));
        }
    }
    let type_keys = data
        .meta_types
        .iter()
        .map(|record| (record.token.as_str(), record.index, record.type_id.as_str()))
        .collect::<HashSet<_>>();
    let record_counts =
        data.records
            .iter()
            .fold(HashMap::<&str, u64>::new(), |mut counts, record| {
                *counts.entry(record.token.as_str()).or_default() += 1;
                if !type_keys.contains(&(
                    record.token.as_str(),
                    record.type_index(),
                    record.type_id.as_str(),
                )) {
                    findings.push(finding(
                        Check::NativeLinks,
                        "Inventor RSe record type does not resolve in its metadata table".into(),
                        Some(record.id.clone()),
                    ));
                }
                counts
            });
    unique(
        findings,
        data.records
            .iter()
            .map(|record| (record.token.as_str(), record.ordinal)),
        "segment record ordinal",
    );
    for bulk in &data.bulk {
        match &bulk.records {
            crate::native::SegmentBulkFrame::Framed { record_count, .. } => {
                if *record_count != record_counts.get(bulk.token.as_str()).copied().unwrap_or(0) {
                    findings.push(finding(
                        Check::NativeLinks,
                        "Inventor bulk record summary does not match its record arena".into(),
                        Some(bulk.id.clone()),
                    ));
                }
            }
            crate::native::SegmentBulkFrame::Unavailable { detail } => {
                findings.push(finding(
                    Check::NativeLinks,
                    format!("Inventor bulk records are unavailable: {detail}"),
                    Some(bulk.id.clone()),
                ));
            }
        }
    }
    let expanded_lengths = data
        .bulk
        .iter()
        .map(|bulk| (bulk.token.as_str(), bulk.expanded_len))
        .collect::<HashMap<_, _>>();
    for record in &data.records {
        let end = record.payload_offset.checked_add(record.payload_len());
        if end.is_none_or(|end| {
            end > expanded_lengths
                .get(record.token.as_str())
                .copied()
                .unwrap_or(0)
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor RSe record payload range exceeds its bulk stream".into(),
                Some(record.id.clone()),
            ));
        }
    }
    for record in &data.unpaired {
        if pair_tokens.contains(record.token.as_str()) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor segment is both paired and unpaired".into(),
                Some(record.id.clone()),
            ));
        }
    }
}

fn validate_segment_states<'a>(
    findings: &mut Vec<Finding>,
    pairs: &HashSet<&'a str>,
    parsed: impl IntoIterator<Item = &'a str>,
    issues: impl IntoIterator<Item = &'a str>,
    member: &str,
) {
    let states = parsed.into_iter().chain(issues).collect::<Vec<_>>();
    unique(
        findings,
        states.iter().copied(),
        &format!("segment {member} state"),
    );
    if states.into_iter().collect::<HashSet<_>>() != *pairs {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor segment {member} states do not cover paired segments exactly"),
            None,
        ));
    }
}

fn validate_properties(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.property_sets.iter().map(|record| record.path.as_str()),
        "property-set path",
    );
    let mut sections_by_set = HashMap::<&str, u64>::new();
    let mut properties_by_section = HashMap::<(&str, u32), u64>::new();
    for section in &data.property_sections {
        *sections_by_set.entry(&section.set_path).or_default() += 1;
    }
    for property in &data.properties {
        *properties_by_section
            .entry((&property.set_path, property.section_ordinal))
            .or_default() += 1;
    }
    for set in &data.property_sets {
        if sections_by_set.get(set.path.as_str()).copied().unwrap_or(0) != set.section_count {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor property-set section count does not match its section arena".into(),
                Some(set.id.clone()),
            ));
        }
    }
    let set_paths = data
        .property_sets
        .iter()
        .map(|record| record.path.as_str())
        .collect::<HashSet<_>>();
    for section in &data.property_sections {
        if !set_paths.contains(section.set_path.as_str())
            || properties_by_section
                .get(&(section.set_path.as_str(), section.ordinal))
                .copied()
                .unwrap_or(0)
                != section.property_count
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor property section does not match its set or property arena".into(),
                Some(section.id.clone()),
            ));
        }
    }
}

fn validate_protein(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.protein.entries().iter().map(|record| record.ordinal),
        "Protein entry ordinal",
    );
    let record = &data.protein;
    if let ProteinRecord::Malformed { id, detail, .. } = record {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor Protein stream is malformed: {detail}"),
            Some(id.clone()),
        ));
    }
}

fn validate_protein_assets(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.protein_assets
            .iter()
            .map(|record| (record.entry_name.as_str(), record.ordinal())),
        "Protein decoded-record position",
    );
    let entry_names = data
        .protein
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<HashSet<_>>();
    for asset in &data.protein_assets {
        if !entry_names.contains(asset.entry_name.as_str()) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor Protein asset position is inconsistent or does not resolve to a package entry"
                    .into(),
                Some(asset.id.clone()),
            ));
        }
    }
}

fn validate_protein_rejections(data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.protein_rejections
            .iter()
            .map(|record| record.id.as_str()),
        "Protein rejection id",
    );
    unique(
        findings,
        data.protein_rejections
            .iter()
            .map(|record| (record.entry_name.as_str(), record.ordinal)),
        "Protein rejected-record position",
    );
    let entry_names = data
        .protein
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<HashSet<_>>();
    let accepted_positions = data
        .protein_assets
        .iter()
        .map(|record| (record.entry_name.as_str(), record.ordinal()))
        .collect::<HashSet<_>>();
    for rejection in &data.protein_rejections {
        if !entry_names.contains(rejection.entry_name.as_str())
            || accepted_positions.contains(&(rejection.entry_name.as_str(), rejection.ordinal))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor Protein rejection position overlaps an asset or does not resolve to a package entry and detail"
                    .into(),
                Some(rejection.id.clone()),
            ));
        }
    }
}

fn validate_protein_record_coverage(data: &NativeData, findings: &mut Vec<Finding>) {
    let mut positions = HashMap::<&str, HashSet<u64>>::new();
    for (entry_name, ordinal) in data
        .protein_assets
        .iter()
        .map(|record| (record.entry_name.as_str(), record.ordinal()))
        .chain(
            data.protein_rejections
                .iter()
                .map(|record| (record.entry_name.as_str(), record.ordinal)),
        )
    {
        positions.entry(entry_name).or_default().insert(ordinal);
    }
    for (entry_name, ordinals) in positions {
        let contiguous = ordinals
            .iter()
            .copied()
            .max()
            .and_then(|maximum| maximum.checked_add(1))
            .and_then(|count| usize::try_from(count).ok())
            == Some(ordinals.len());
        if !contiguous {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "Inventor Protein logical-record positions are not contiguous for {entry_name:?}"
                ),
                None,
            ));
        }
    }
}

fn validate_ufrx(ir: &CadIr, data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.ufrx
            .external_references()
            .iter()
            .map(|record| record.ordinal),
        "external reference ordinal",
    );
    unique(
        findings,
        data.ufrx.model_states().iter().map(|record| record.ordinal),
        "UFRxDoc model-state ordinal",
    );
    unique(
        findings,
        data.ufrx
            .external_references()
            .iter()
            .map(|record| record.reference_id),
        "external reference id",
    );
    let record = &data.ufrx;
    if let UfrxRecord::Malformed { id, detail, .. } = record {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor UFRxDoc stream is malformed: {detail}"),
            Some(id.clone()),
        ));
    }
    let model_state_ordinals = data
        .ufrx
        .model_states()
        .iter()
        .map(|state| state.ordinal)
        .collect::<HashSet<_>>();
    if model_state_ordinals.len() != data.ufrx.model_states().len()
        || model_state_ordinals
            != (0..data.ufrx.model_states().len() as u32).collect::<HashSet<_>>()
    {
        findings.push(finding(
            Check::NativeLinks,
            "Inventor UFRxDoc model-state ordinals are not contiguous".into(),
            None,
        ));
    }
    if let UfrxRecord::ParsedPrefix {
        id,
        representation: Some(representation),
        ..
    } = record
    {
        let representation_pair_present = representation.active_representation.is_some();
        let expected_pair = document_kind(ir).and_then(|kind| match kind {
            "assembly" => Some(true),
            "part" => Some(false),
            _ => None,
        });
        if expected_pair.is_some_and(|expected| representation_pair_present != expected) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor UFRxDoc representation state is inconsistent".into(),
                Some(id.clone()),
            ));
        }
    }
    unique(
        findings,
        data.ufrx
            .embedded_references()
            .iter()
            .map(|record| record.ordinal),
        "embedded reference ordinal",
    );
    unique(
        findings,
        data.ufrx
            .occurrences()
            .iter()
            .map(|occurrence| occurrence.occurrence_id),
        "UFRxDoc occurrence id",
    );
    let reference_ids = data
        .ufrx
        .external_references()
        .iter()
        .map(|reference| reference.reference_id)
        .collect::<HashSet<_>>();
    let assembly_ids = data
        .assembly_occurrences
        .iter()
        .map(|occurrence| occurrence.occurrence_id)
        .collect::<HashSet<_>>();
    let mut actual_counts = HashMap::<u32, u64>::new();
    let assembly_document = is_assembly_document(ir);
    for occurrence in data.ufrx.occurrences() {
        *actual_counts
            .entry(occurrence.file_reference_id)
            .or_default() += 1;
        if !reference_ids.contains(&occurrence.file_reference_id)
            || (assembly_document && !assembly_ids.contains(&occurrence.occurrence_id))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor UFRxDoc occurrence does not resolve to its file and assembly records"
                    .into(),
                Some(occurrence.id.clone()),
            ));
        }
    }
    for reference in data.ufrx.external_references() {
        if actual_counts
            .get(&reference.reference_id)
            .copied()
            .unwrap_or_default()
            != u64::from(reference.occurrence_count)
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor external-reference occurrence count does not match its typed records"
                    .into(),
                Some(reference.id.clone()),
            ));
        }
    }
}

fn validate_assembly(ir: &CadIr, data: &NativeData, findings: &mut Vec<Finding>) {
    unique(
        findings,
        data.assembly_occurrences
            .iter()
            .map(|record| record.occurrence_id),
        "assembly occurrence id",
    );
    unique(
        findings,
        data.assembly_placements
            .iter()
            .map(|record| record.occurrence_id),
        "assembly placement occurrence id",
    );
    let occurrence_ids = data
        .assembly_occurrences
        .iter()
        .map(|record| record.occurrence_id)
        .collect::<HashSet<_>>();
    for placement in &data.assembly_placements {
        if !occurrence_ids.contains(&placement.occurrence_id) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor assembly placement does not resolve to a finite occurrence".into(),
                Some(placement.id.clone()),
            ));
        }
    }
    if is_assembly_document(ir) && !data.ufrx.external_references().is_empty() {
        let declared = data
            .ufrx
            .external_references()
            .iter()
            .map(|reference| u64::from(reference.occurrence_count))
            .sum::<u64>();
        if declared != data.assembly_occurrences.len() as u64 {
            findings.push(finding(
                Check::NativeLinks,
                format!(
                    "Inventor external references declare {declared} occurrences, but the typed assembly table contains {}",
                    data.assembly_occurrences.len()
                ),
                None,
            ));
        }
    }
    for issue in &data.assembly_record_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!(
                "Inventor assembly record {}:{} is unavailable: {}",
                issue.segment_token, issue.record_ordinal, issue.detail
            ),
            Some(issue.id()),
        ));
    }
    let mut projected = crate::assembly::project_occurrences(
        data.ufrx.occurrences(),
        data.ufrx.external_references(),
        &data.assembly_occurrences,
        &data.assembly_placements,
    );
    projected
        .occurrences
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    if ir.model.occurrences != projected.occurrences {
        findings.push(finding(
            Check::NativeLinks,
            "Inventor neutral occurrences do not match the typed assembly records".into(),
            None,
        ));
    }
}

fn is_assembly_document(ir: &CadIr) -> bool {
    document_kind(ir) == Some("assembly")
}

fn document_kind(ir: &CadIr) -> Option<&str> {
    ir.source
        .as_ref()
        .and_then(|source| source.attributes.get("document_kind"))
        .map(String::as_str)
}

fn unique<T: Eq + std::hash::Hash>(
    findings: &mut Vec<Finding>,
    values: impl IntoIterator<Item = T>,
    field: &str,
) {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            findings.push(finding(
                Check::NativeLinks,
                format!("Inventor native data repeats a {field}"),
                None,
            ));
        }
    }
}

fn finding(check: Check, message: String, entity: Option<String>) -> Finding {
    Finding {
        check,
        severity: Severity::Error,
        message,
        entity,
    }
}
