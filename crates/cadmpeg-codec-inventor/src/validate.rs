// SPDX-License-Identifier: Apache-2.0
//! Inventor-native validation.

mod mirror;

use std::collections::{BTreeMap, HashMap, HashSet};

mod base;
mod sheet_metal;

use cadmpeg_asm::brep::records::{BodyNativeKey, FaceNativeKey};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    report::{
        check::{Check, Finding},
        Severity,
    },
    CadIr, NativeUnknownRecord,
};

use crate::design::{PmDcExpression, PmDcExpressionKind, PmDcParameter, PmDcUnit, PmDcUnitKind};
use crate::feature::{
    PmDcEntityStyleLink, PmDcFeature, PmDcFeatureLabel, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureTerminator, PmDcKernelEntityLink, PmDcPatternFeature,
    PmDcWorkPlane,
};
use crate::sketch::{
    PmDcDirection, PmDcSketch, PmDcSketchConstraint, PmDcSketchConstraintKind, PmDcSketchEntity,
    PmDcSketchEntityKind, PmDcTransform, PointTail,
};

use crate::native::protein::{ProteinAssetRecord, ProteinRecord, ProteinRejectionRecord};
use crate::native::ufrx::{ExternalReferenceRecord, UfrxRecord};
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
    "pm_dc_feature_dimensions",
    "pm_dc_feature_labels",
    "pm_dc_feature_properties",
    "pm_dc_feature_terminators",
    "pm_dc_features",
    "pm_dc_kernel_entity_links",
    "pm_dc_model_roots",
    "pm_dc_parameter_tables",
    "pm_dc_pattern_features",
    "pm_dc_entity_style_links",
    "pm_dc_parameters",
    "pm_dc_directions",
    "pm_dc_sketch_entities",
    "pm_dc_sketch_constraints",
    "pm_dc_sketches",
    "pm_dc_transforms",
    "pm_dc_units",
    "pm_dc_work_planes",
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

const OPTIONAL_ARENAS: &[&str] = &[
    "pm_dc_sheet_metal_styles",
    "pm_dc_sheet_metal_flanges",
    "drawing_records",
    "drawing_metadata_references",
    "drawing_reference_tables",
    "drawing_record_issues",
];

pub(crate) fn validate_native(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("inventor") else {
        return Ok(Vec::new());
    };
    let actual_arenas = namespace
        .arenas()
        .keys()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let expected_arenas = ARENAS.iter().copied().collect::<HashSet<_>>();
    let allowed_arenas = ARENAS
        .iter()
        .chain(OPTIONAL_ARENAS)
        .copied()
        .collect::<HashSet<_>>();
    if !expected_arenas.is_subset(&actual_arenas) || !actual_arenas.is_subset(&allowed_arenas) {
        let mut missing = expected_arenas
            .difference(&actual_arenas)
            .copied()
            .collect::<Vec<_>>();
        let mut unexpected = actual_arenas
            .difference(&allowed_arenas)
            .copied()
            .collect::<Vec<_>>();
        missing.sort_unstable_by(Ord::cmp);
        unexpected.sort_unstable_by(Ord::cmp);
        return Ok(vec![finding(
            Check::NativeLinks,
            format!(
                "Inventor native namespace has missing arenas {missing:?} and unexpected arenas {unexpected:?}"
            ),
            None,
        )]);
    }
    let data = match NativeData::load(ctx, namespace) {
        Ok(data) => data,
        Err(error) => {
            let error = CodecError::from(error);
            if matches!(error, CodecError::ResourceLimit(_)) {
                return Err(error);
            }
            return Ok(vec![finding(
                Check::NativeLinks,
                format!("Inventor native arenas are invalid: {error}"),
                None,
            )]);
        }
    };

    let mut findings = Vec::new();
    validate_databases(&data, &mut findings);
    validate_segments(&data, &mut findings);
    crate::drawing::validate_native(
        ctx,
        namespace,
        &data.records,
        &data.metadata,
        &data.registry,
        &data.meta_sections,
        &mut findings,
    )?;
    validate_active_carrier(&data, &mut findings);
    validate_design(&data, ir, &mut findings);
    validate_sketches(&data, ir, &mut findings);
    validate_features(ctx, ir, &data, &mut findings)?;
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
    validate_assembly(ctx, ir, &data, &mut findings)?;
    validate_presentation(ir, &data, &mut findings);
    for issue in &data.structural_issues {
        findings.push(finding(
            Check::NativeLinks,
            format!("Inventor {}: {}", issue.scope, issue.detail),
            Some(issue.id.clone()),
        ));
    }
    for issue in &data.property_issues {
        findings.push(Finding {
            severity: Severity::Warning,
            ..finding(
                Check::NativeLinks,
                format!("Inventor property set {:?}: {}", issue.path, issue.detail),
                Some(issue.id.clone()),
            )
        });
    }
    Ok(findings)
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
    let resolves =
        |token: &str, reference: u32| reference == 0 || raw.contains_key(&(token, reference - 1));
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
            parameter.header.next.index(),
            parameter.header.context.index(),
            parameter.unit.index(),
            parameter.formula.index(),
        ];
        if raw.get(&(
            parameter.identity.segment_token.as_str(),
            parameter.identity.record_ordinal,
        )) != Some(&parameter.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(parameter.identity.segment_token.as_str(), reference))
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
        let mut references = vec![expression.unit.index()];
        match &expression.kind {
            PmDcExpressionKind::Value { .. } => {}
            PmDcExpressionKind::ParameterReference { operand, .. }
            | PmDcExpressionKind::Unary { operand, .. } => references.push(operand.index()),
            PmDcExpressionKind::Binary { left, right, .. } => {
                references.push(left.index());
                references.push(right.index());
            }
            PmDcExpressionKind::Function { arguments, .. } => {
                references.extend(
                    arguments
                        .references()
                        .iter()
                        .map(|argument| argument.index()),
                );
            }
        }
        if raw.get(&(
            expression.identity.segment_token.as_str(),
            expression.identity.record_ordinal,
        )) != Some(&expression.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(expression.identity.segment_token.as_str(), reference))
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
                .map(|reference| reference.index())
                .chain(std::iter::once(derived.index()))
                .collect::<Vec<_>>(),
            PmDcUnitKind::Base { .. } => Vec::new(),
        };
        if raw.get(&(
            unit.identity.segment_token.as_str(),
            unit.identity.record_ordinal,
        )) != Some(&unit.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(unit.identity.segment_token.as_str(), reference))
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
    validate_parameter_scopes(data, &raw, findings);
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

fn validate_parameter_scopes(
    data: &NativeData,
    raw: &HashMap<(&str, u32), &str>,
    findings: &mut Vec<Finding>,
) {
    let resolves = |token: &str, reference: u32, type_id: Option<&str>| {
        reference == 0
            || raw
                .get(&(token, reference - 1))
                .is_some_and(|raw_type| type_id.is_none_or(|expected| *raw_type == expected))
    };
    let table_type = crate::pmdc::type_id_string(crate::design::scope::PARAMETER_TABLE_TYPE);
    let root_type = crate::pmdc::type_id_string(crate::design::scope::MODEL_ROOT_TYPE);
    unique(
        findings,
        data.pm_dc_parameter_tables.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc parameter table",
    );
    unique(
        findings,
        data.pm_dc_model_roots.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc model root",
    );
    for table in &data.pm_dc_parameter_tables {
        let token = table.identity.segment_token.as_str();
        let exact = raw.get(&(token, table.identity.record_ordinal)).copied()
            == Some(table.identity.type_id.as_str())
            && table.identity.type_id.as_str() == table_type;
        let references_resolve = resolves(token, table.next.index(), None)
            && resolves(token, table.context.index(), Some(&root_type))
            && table
                .members
                .references()
                .iter()
                .chain(table.names.iter().map(|named| &named.reference))
                .all(|reference| resolves(token, reference.index(), None));
        if !exact || !references_resolve {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc parameter table record or reference does not resolve".into(),
                Some(table.id()),
            ));
        }
    }
    for root in &data.pm_dc_model_roots {
        let token = root.identity.segment_token.as_str();
        let exact = raw.get(&(token, root.identity.record_ordinal)).copied()
            == Some(root.identity.type_id.as_str())
            && root.identity.type_id.as_str() == root_type;
        let references_resolve = resolves(token, root.next.index(), None)
            && resolves(token, root.parameter_table.index(), Some(&table_type))
            && root
                .members
                .references()
                .iter()
                .all(|reference| resolves(token, reference.index(), None));
        if !exact || !references_resolve {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc model root record or reference does not resolve".into(),
                Some(root.id()),
            ));
        }
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
        references
            .iter()
            .all(|reference| *reference == 0 || raw.contains_key(&(token, reference - 1)))
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
            sketch.header.next.index(),
            sketch.header.context.index(),
            sketch.transform.index(),
            sketch.direction.index(),
        ]
        .into_iter()
        .chain(
            sketch
                .entities
                .references()
                .iter()
                .map(|reference| reference.index()),
        )
        .chain(
            sketch
                .auxiliary
                .iter()
                .flat_map(super::pmdc::PmDcReferenceList::references)
                .map(|reference| reference.index()),
        )
        .collect::<Vec<_>>();
        if !record_is_exact(
            sketch.identity.segment_token.as_str(),
            sketch.identity.record_ordinal,
            sketch.identity.type_id.as_str(),
        ) || !references_resolve(sketch.identity.segment_token.as_str(), &references)
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc sketch record or reference does not resolve".into(),
                Some(sketch.id()),
            ));
        }
    }
    let entities = data
        .pm_dc_sketch_entities
        .iter()
        .map(|entity| {
            (
                (
                    entity.identity.segment_token.as_str(),
                    entity.identity.record_ordinal,
                ),
                entity,
            )
        })
        .collect::<HashMap<_, _>>();
    for entity in &data.pm_dc_sketch_entities {
        if !entity.tail_matches_version() {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor point tail form does not match save_version_major".into(),
                Some(entity.id()),
            ));
        }
        let mut references = vec![
            entity.header.next.index(),
            entity.header.context.index(),
            entity.sketch.index(),
        ];
        let mut add_list = |list: &PmDcReferenceList| {
            references.extend(list.references().iter().map(|reference| reference.index()));
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
                match tail {
                    PointTail::Absent => {}
                    PointTail::Present {
                        associations,
                        links,
                        ..
                    } => {
                        add_list(associations);
                        if let Some(links) = links {
                            add_list(links);
                        }
                    }
                    PointTail::Paired(paired) => {
                        let token = entity.identity.segment_token.as_str();
                        if !paired.operands().iter().all(|reference| {
                            reference.index().checked_sub(1).is_some_and(|ordinal| {
                                entities.get(&(token, ordinal)).is_some_and(|operand| {
                                    let expected_type =
                                        crate::pmdc::type_id_string(operand.kind.record_type_id());
                                    record_is_exact(token, ordinal, &expected_type)
                                        && operand.identity.type_id.as_str() == expected_type
                                })
                            })
                        }) {
                            findings.push(finding(
                                Check::NativeLinks,
                                "Inventor point paired operand does not resolve to a sketch entity"
                                    .into(),
                                Some(entity.id()),
                            ));
                        }
                        references
                            .extend(paired.operands().iter().map(|reference| reference.index()));
                    }
                }
            }
            PmDcSketchEntityKind::Spline { data } => {
                add_list(&data.points);
                for list in &data.auxiliary {
                    add_list(list);
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
                references.push(center.index());
            }
            PmDcSketchEntityKind::Polygon { reference, .. } => references.push(reference.index()),
            PmDcSketchEntityKind::BlockInstance {
                definition, group, ..
            } => references.extend([definition.index(), group.index()]),
            PmDcSketchEntityKind::BlockMember { instances, member } => {
                add_list(instances);
                references.push(member.index());
            }
        }
        if !record_is_exact(
            entity.identity.segment_token.as_str(),
            entity.identity.record_ordinal,
            entity.identity.type_id.as_str(),
        ) || !references_resolve(entity.identity.segment_token.as_str(), &references)
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
            transform.identity.segment_token.as_str(),
            transform.identity.record_ordinal,
            transform.identity.type_id.as_str(),
        ) || !references_resolve(
            transform.identity.segment_token.as_str(),
            &[
                transform.header.next.index(),
                transform.header.context.index(),
            ],
        ) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc transform record or reference does not resolve".into(),
                Some(transform.id()),
            ));
        }
    }
    for constraint in &data.pm_dc_sketch_constraints {
        let references = sketch_constraint_references(constraint);
        if !record_is_exact(
            constraint.identity.segment_token.as_str(),
            constraint.identity.record_ordinal,
            constraint.identity.type_id.as_str(),
        ) || !references_resolve(constraint.identity.segment_token.as_str(), &references)
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
            direction.identity.segment_token.as_str(),
            direction.identity.record_ordinal,
            direction.identity.type_id.as_str(),
        ) || !references_resolve(
            direction.identity.segment_token.as_str(),
            &[
                direction.header.next.index(),
                direction.header.context.index(),
            ],
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

fn validate_features(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mirror_outputs = mirror::validate_mirrors(ctx, ir, data, findings)?;
    sheet_metal::records(data, findings);
    sheet_metal::definitions(ir, data, findings);
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
    let resolves =
        |token: &str, reference: u32| reference == 0 || raw.contains_key(&(token, reference - 1));
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
        let references = [feature.header.next.index(), feature.header.context.index()]
            .into_iter()
            .chain(
                feature
                    .properties
                    .references()
                    .iter()
                    .map(|reference| reference.index()),
            );
        if raw.get(&(
            feature.identity.segment_token.as_str(),
            feature.identity.record_ordinal,
        )) != Some(&feature.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(feature.identity.segment_token.as_str(), reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc feature record or reference does not resolve".into(),
                Some(feature.id()),
            ));
        }
    }
    for feature in &data.pm_dc_pattern_features {
        let references = [feature.header.next.index(), feature.header.context.index()]
            .into_iter()
            .chain(
                feature
                    .properties
                    .references()
                    .iter()
                    .map(|reference| reference.index()),
            )
            .chain(
                feature
                    .participants
                    .references()
                    .iter()
                    .map(|reference| reference.index()),
            )
            .chain(
                feature
                    .property_slots
                    .iter()
                    .map(|reference| reference.index()),
            );
        if raw.get(&(
            feature.identity.segment_token.as_str(),
            feature.identity.record_ordinal,
        )) != Some(&feature.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(feature.identity.segment_token.as_str(), reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc pattern-feature record or reference does not resolve".into(),
                Some(feature.id()),
            ));
        }
    }
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
    let selection_list_type = crate::pmdc::type_id_string(crate::feature::SELECTION_LIST_TYPE);
    let kernel_links =
        cadmpeg_core::decode::unique_index(data.pm_dc_kernel_entity_links.iter().map(|link| {
            (
                (
                    link.identity.segment_token.as_str(),
                    link.identity.record_ordinal,
                ),
                link,
            )
        }));
    unique(
        findings,
        data.pm_dc_feature_dimensions.iter().map(|record| {
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            )
        }),
        "Inventor PmDc feature dimension",
    );
    for dimension in &data.pm_dc_feature_dimensions {
        let token = dimension.identity.segment_token.as_str();
        if raw.get(&(token, dimension.identity.record_ordinal))
            != Some(&dimension.identity.type_id.as_str())
            || dimension.identity.type_id.as_str()
                != crate::pmdc::type_id_string(crate::feature::revolve::DIMENSION_TYPE)
            || !resolves(token, dimension.parameter.index())
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor feature dimension record or reference does not resolve".into(),
                Some(dimension.id()),
            ));
        }
    }
    for property in &data.pm_dc_feature_properties {
        let mut references = vec![
            property.header.next.index(),
            property.header.context.index(),
        ];
        match &property.kind {
            PmDcFeaturePropertyKind::SheetMetal { definition } => {
                references.extend(
                    definition
                        .references()
                        .iter()
                        .map(|reference| reference.index()),
                );
            }
            PmDcFeaturePropertyKind::References { items, .. } => {
                references.extend(items.references().iter().map(|reference| reference.index()));
            }
            PmDcFeaturePropertyKind::SurfaceBody { body } => {
                if body.qualified() {
                    references.push(body.index());
                }
            }
            PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } => {
                references.push(entity_link.index());
            }
            PmDcFeaturePropertyKind::LoftSection {
                entity_link,
                condition,
                parameters,
                option,
                ..
            } => references.extend([
                entity_link.index(),
                condition.index(),
                parameters[0].index(),
                parameters[1].index(),
                option.index(),
            ]),
            PmDcFeaturePropertyKind::Placement {
                transform,
                point,
                value,
            } => references.extend([transform.index(), point.index(), value.index()]),
            PmDcFeaturePropertyKind::FilletEdgeSet {
                edges,
                radius,
                selection,
                continuity,
                option,
            } => references.extend(
                [
                    edges.index(),
                    radius.index(),
                    selection.index(),
                    continuity.index(),
                ]
                .into_iter()
                .chain(option.map(crate::pmdc::PmDcReference::index)),
            ),
            PmDcFeaturePropertyKind::Mirror { .. }
            | PmDcFeaturePropertyKind::Enumeration { .. }
            | PmDcFeaturePropertyKind::WideEnumeration { .. }
            | PmDcFeaturePropertyKind::Boolean { .. }
            | PmDcFeaturePropertyKind::RdxVariable { .. }
            | PmDcFeaturePropertyKind::EdgeItem { .. }
            | PmDcFeaturePropertyKind::FaceItem { .. }
            | PmDcFeaturePropertyKind::TerminationFlag { .. } => {}
            PmDcFeaturePropertyKind::WorkAxis {
                references: axis, ..
            } => {
                references.extend(axis.references().iter().map(|reference| reference.index()));
            }
            PmDcFeaturePropertyKind::PlaneSurface {
                references: surface,
                ..
            } => {
                references.extend(surface.iter().map(|reference| reference.index()));
            }
        }
        if raw.get(&(
            property.identity.segment_token.as_str(),
            property.identity.record_ordinal,
        )) != Some(&property.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(property.identity.segment_token.as_str(), reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc feature-property record or reference does not resolve".into(),
                Some(property.id()),
            ));
        }
        if !face_property_is_admitted(property, |ordinal| {
            properties_by_record
                .get(&(property.identity.segment_token.as_str(), ordinal))
                .copied()
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc face selection has an invalid record type, list, or member".into(),
                Some(property.id()),
            ));
        }
        if !crate::feature::sheet_metal::valid_references(property, |ordinal| {
            raw.get(&(property.identity.segment_token.as_str(), ordinal))
                .copied()
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor sheet-metal property has an invalid record or operand type".into(),
                Some(property.id()),
            ));
        }
        if !crate::feature::loft_sweep::valid_references(
            property,
            |ordinal| {
                raw.get(&(property.identity.segment_token.as_str(), ordinal))
                    .copied()
            },
            |ordinal| {
                kernel_links
                    .get(&(property.identity.segment_token.as_str(), ordinal))
                    .and_then(Option::as_ref)
                    .copied()
            },
        ) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor loft section control or reference type is invalid".into(),
                Some(property.id()),
            ));
        }
        if let Some(message) = fillet_continuity_type_error(&property.kind, |ordinal| {
            raw.get(&(property.identity.segment_token.as_str(), ordinal))
                .copied()
        }) {
            findings.push(finding(
                Check::NativeLinks,
                message.into(),
                Some(property.id()),
            ));
        }
        if !fillet_option_is_admitted(&property.kind, |ordinal| {
            raw.get(&(property.identity.segment_token.as_str(), ordinal))
                .copied()
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc fillet edge-set option is not a Boolean record".into(),
                Some(property.id()),
            ));
        }
        if matches!(
            property.kind,
            PmDcFeaturePropertyKind::References {
                family: crate::feature::PmDcFeatureReferenceFamily::SelectionList,
                ..
            }
        ) != (property.identity.type_id.as_str() == selection_list_type)
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor selection list has the wrong PmDc record type".into(),
                Some(property.id()),
            ));
        }
    }
    for link in &data.pm_dc_entity_style_links {
        let references = [
            link.header.owner.index(),
            link.header.parent.index(),
            link.header.next.index(),
        ];
        if raw.get(&(
            link.identity.segment_token.as_str(),
            link.identity.record_ordinal,
        )) != Some(&link.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(link.identity.segment_token.as_str(), reference))
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
            label.header.owner.index(),
            label.header.parent.index(),
            label.header.next.index(),
        ]
        .into_iter()
        .chain(
            label
                .participants
                .references()
                .iter()
                .map(|reference| reference.index()),
        );
        if raw.get(&(
            label.identity.segment_token.as_str(),
            label.identity.record_ordinal,
        )) != Some(&label.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(label.identity.segment_token.as_str(), reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc feature-label record or reference does not resolve".into(),
                Some(label.id()),
            ));
        }
    }
    for link in &data.pm_dc_kernel_entity_links {
        if raw.get(&(
            link.identity.segment_token.as_str(),
            link.identity.record_ordinal,
        )) != Some(&link.identity.type_id.as_str())
            || !resolves(link.identity.segment_token.as_str(), link.body.index())
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc kernel entity link record or reference does not resolve".into(),
                Some(link.id()),
            ));
        }
    }
    for plane in &data.pm_dc_work_planes {
        let references = [plane.header.next.index(), plane.header.context.index()];
        if raw.get(&(
            plane.identity.segment_token.as_str(),
            plane.identity.record_ordinal,
        )) != Some(&plane.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(plane.identity.segment_token.as_str(), reference))
        {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmDc work plane record or reference does not resolve".into(),
                Some(plane.id()),
            ));
        }
    }
    for terminator in &data.pm_dc_feature_terminators {
        let references = [
            terminator.header.next.index(),
            terminator.header.context.index(),
        ];
        if raw.get(&(
            terminator.identity.segment_token.as_str(),
            terminator.identity.record_ordinal,
        )) != Some(&terminator.identity.type_id.as_str())
            || references
                .into_iter()
                .any(|reference| !resolves(terminator.identity.segment_token.as_str(), reference))
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
                    label.header.owner.index().checked_sub(1)?,
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
    for source in &data.pm_dc_features {
        let token = source.identity.segment_token.as_str();
        let family = labels
            .get(&(token, source.identity.record_ordinal))
            .and_then(|label| {
                crate::feature::loft_sweep::slots::Family::from_class_id(label.class_id())
            });
        if family.is_some_and(|family| {
            !family.valid_slots(source, |ordinal| raw.get(&(token, ordinal)).copied())
        }) {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor loft or sweep property slot has an invalid record type".into(),
                Some(source.id()),
            ));
        }
    }
    let parameters_by_record = data
        .pm_dc_parameters
        .iter()
        .map(|parameter| {
            (
                (
                    parameter.identity.segment_token.as_str(),
                    parameter.identity.record_ordinal,
                ),
                parameter,
            )
        })
        .collect::<HashMap<_, _>>();
    let results = ir
        .model
        .feature_result_topologies
        .iter()
        .map(|result| (&result.output_of, result))
        .collect::<HashMap<_, _>>();
    let model_roots = data
        .pm_dc_model_roots
        .iter()
        .map(crate::record_identity::Located::id)
        .collect::<HashSet<_>>();
    let history_records = data
        .pm_dc_pattern_features
        .iter()
        .map(PmDcPatternFeature::id)
        .chain(data.pm_dc_sketches.iter().map(PmDcSketch::id))
        .collect::<HashSet<_>>();
    let thicken_record_types =
        cadmpeg_core::decode::unique_index(data.records.iter().map(|record| {
            (
                (record.token.as_str(), record.ordinal),
                record.type_id.as_str(),
            )
        }));
    let feature_properties =
        cadmpeg_core::decode::unique_index(data.pm_dc_feature_properties.iter().map(|property| {
            (
                (
                    property.identity.segment_token.as_str(),
                    property.identity.record_ordinal,
                ),
                property,
            )
        }));
    let feature_parameters =
        cadmpeg_core::decode::unique_index(data.pm_dc_parameters.iter().map(|parameter| {
            (
                (
                    parameter.identity.segment_token.as_str(),
                    parameter.identity.record_ordinal,
                ),
                parameter,
            )
        }));
    let parameter_values = cadmpeg_core::decode::unique_index(
        ir.model
            .parameters
            .iter()
            .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter))),
    );
    let revolve_dimensions =
        cadmpeg_core::decode::unique_index(data.pm_dc_feature_dimensions.iter().map(|record| {
            (
                (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                ),
                record,
            )
        }));
    for feature in &ir.model.features {
        if is_flat_pattern_feature(feature)
            && feature
                .native_ref
                .as_ref()
                .is_some_and(|native| model_roots.contains(native))
        {
            continue;
        }
        let native = feature.native_ref.as_deref();
        let Some(raw_feature) = native.and_then(|native| raw_features.get(native).copied()) else {
            if native.is_some_and(|native| history_records.contains(native))
                && crate::feature::FeatureFamily::from_definition(feature.evaluation.definition())
                    .is_none()
                && !matches!(
                    feature.evaluation.definition(),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::CosmeticThread { .. }
                    )
                )
            {
                continue;
            }
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral feature does not resolve to its PmDc source record".into(),
                Some(feature.id.as_str().to_owned()),
            ));
            continue;
        };
        if let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::CosmeticThread {
                face,
                diameter,
                extent,
            },
        ) = feature.evaluation.definition()
        {
            let token = raw_feature.identity.segment_token.as_str();
            let collection = crate::feature::thread_face_collection(
                raw_feature,
                |reference| {
                    properties_by_record
                        .get(&(token, reference.checked_sub(1)?))
                        .copied()
                },
                |reference| {
                    reference
                        .checked_sub(1)
                        .is_some_and(|ordinal| parameters_by_record.contains_key(&(token, ordinal)))
                },
            );
            let label_matches = labels
                .get(&(token, raw_feature.identity.record_ordinal))
                .is_some_and(|label| label.class_id() == crate::feature::THREAD_CLASS_ID);
            let selection_matches = collection.is_some_and(|collection| {
                matches!(face, cadmpeg_ir::features::FaceSelection::Native(native)
                    if *native == collection.id())
            });
            if !label_matches
                || !selection_matches
                || diameter.is_some()
                || extent.is_some()
                || results.contains_key(&feature.id)
            {
                findings.push(finding(
                    Check::NativeLinks,
                    "Inventor thread annotation does not match its PmDc face selection".into(),
                    Some(feature.id.as_str().to_owned()),
                ));
            }
            continue;
        }
        let definition = feature.evaluation.definition();
        if crate::feature::FeatureFamily::from_definition(definition).is_none() {
            continue;
        }
        if let cadmpeg_ir::features::FeatureDefinition::Operation(
            actual @ cadmpeg_ir::features::FeatureOperation::Thicken { .. },
        ) = feature.evaluation.definition()
        {
            let token = raw_feature.identity.segment_token.as_str();
            let expected = crate::feature::thicken_operation(
                raw_feature,
                |slot| {
                    let ordinal = raw_feature
                        .properties
                        .references()
                        .get(slot)?
                        .record_ordinal()?;
                    feature_properties.get(&(token, ordinal))?.as_ref().copied()
                },
                |slot| {
                    let ordinal = raw_feature
                        .properties
                        .references()
                        .get(slot)?
                        .record_ordinal()?;
                    let parameter = feature_parameters.get(&(token, ordinal))?.as_ref()?;
                    let native = parameter.id();
                    let value = parameter_values
                        .get(native.as_str())?
                        .as_ref()?
                        .value
                        .as_ref()?;
                    Some((*parameter, value))
                },
                |ordinal| {
                    thicken_record_types
                        .get(&(token, ordinal))?
                        .as_ref()
                        .copied()
                },
            );
            if expected.as_ref() != Some(actual) {
                findings.push(finding(
                    Check::NativeLinks,
                    "Inventor neutral thicken operands do not match their PmDc slots".into(),
                    Some(feature.id.as_str().to_owned()),
                ));
            }
        }
        if let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Revolve { construction, op },
        ) = definition
        {
            let token = raw_feature.identity.segment_token.as_str();
            let expected = crate::feature::revolve::operands(
                raw_feature,
                |slot| {
                    let ordinal = raw_feature
                        .properties
                        .references()
                        .get(slot)?
                        .record_ordinal()?;
                    feature_properties.get(&(token, ordinal))?.as_ref().copied()
                },
                |slot| {
                    let ordinal = raw_feature
                        .properties
                        .references()
                        .get(slot)?
                        .record_ordinal()?;
                    let raw = feature_parameters.get(&(token, ordinal))?.as_ref()?;
                    let native = raw.id();
                    match parameter_values
                        .get(native.as_str())?
                        .as_ref()?
                        .value
                        .as_ref()?
                    {
                        cadmpeg_ir::features::ParameterValue::Angle(angle) => Some(*angle),
                        _ => None,
                    }
                },
                |ordinal| revolve_dimensions.get(&(token, ordinal))?.as_ref().copied(),
            );
            if !expected.is_ok_and(|expected| {
                crate::feature::revolve::matches(&expected, construction, *op)
            }) {
                findings.push(finding(
                    Check::NativeLinks,
                    "Inventor neutral revolve operands do not match their PmDc slots".into(),
                    Some(feature.id.as_str().to_owned()),
                ));
            }
        }
        // The label class selects the record layout; the neutral operation
        // must be the one that layout projects.
        let Some(family) = labels
            .get(&(
                raw_feature.identity.segment_token.as_str(),
                raw_feature.identity.record_ordinal,
            ))
            .and_then(|label| crate::feature::FeatureFamily::from_class_id(label.class_id()))
            .filter(|family| family.admits_definition(definition))
        else {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor neutral feature family does not match its PmDc label".into(),
                Some(feature.id.as_str().to_owned()),
            ));
            continue;
        };
        if matches!(family, crate::feature::FeatureFamily::Base) {
            if !base::valid(
                ir,
                data,
                feature,
                raw_feature,
                results.get(&feature.id).copied(),
            ) {
                findings.push(finding(
                    Check::NativeLinks,
                    "Inventor Base result does not match its stored source and current model body"
                        .into(),
                    Some(feature.id.as_str().to_owned()),
                ));
            }
            continue;
        }
        let output_slot = family.output_slot(raw_feature);
        let expected_collection = raw_feature
            .properties
            .references()
            .get(output_slot)
            .and_then(|reference| reference.index().checked_sub(1))
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
        if mirror_outputs.contains(&result.output_of)
            || ir.model.features.iter().any(|feature| {
                feature.id == result.output_of
                    && matches!(
                        crate::feature::FeatureFamily::from_definition(
                            feature.evaluation.definition()
                        ),
                        Some(crate::feature::FeatureFamily::Base)
                    )
            })
        {
            continue;
        }
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
        if matches!(collection.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
            // A plate result is its one surface-body record.
            if result
                .bodies()
                .iter()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                .ne([collection.id().as_str()])
            {
                findings.push(finding(
                    Check::NativeLinks,
                    "Inventor feature result body does not match its PmDc surface body".into(),
                    Some(result.id.as_str().to_owned()),
                ));
            }
            continue;
        }
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
                let ordinal = reference.index().checked_sub(1)?;
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
                    .map(cadmpeg_core::text::NonBlankString::as_str)
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
    Ok(())
}

fn face_property_is_admitted<'a>(
    property: &PmDcFeatureProperty,
    property_at: impl Fn(u32) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    let face_item_type = "ccb59f50d1114f6b0008a1ba32a3dc09";
    let face_collection_type = "f1b2bc24d01121dd0008d0bc0663dc09";
    let valid_item = |item: &PmDcFeatureProperty| {
        item.identity.type_id.as_str() == face_item_type
            && matches!(&item.kind, PmDcFeaturePropertyKind::FaceItem { index_references }
                if index_references.marker() == 2)
    };
    match &property.kind {
        PmDcFeaturePropertyKind::FaceItem { .. } => valid_item(property),
        PmDcFeaturePropertyKind::References {
            family: crate::feature::PmDcFeatureReferenceFamily::FaceCollection,
            items,
        } => {
            property.identity.type_id.as_str() == face_collection_type
                && items.marker() == 2
                && items.references().iter().all(|reference| {
                    reference
                        .record_ordinal()
                        .and_then(&property_at)
                        .is_some_and(valid_item)
                })
        }
        _ => {
            property.identity.type_id.as_str() != face_item_type
                && property.identity.type_id.as_str() != face_collection_type
        }
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
            .map(PmGraphicsStyleCollectionRecord::id),
        "PmGraphics style-collection id",
    );
    unique(
        findings,
        data.pm_graphics_style_collections
            .iter()
            .map(|record| (record.segment_token(), record.record_ordinal())),
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
        for reference in std::iter::once(record.surface.index())
            .chain(std::iter::once(record.parent.index()))
            .chain(
                record
                    .edge_references
                    .references()
                    .iter()
                    .map(|reference| reference.index()),
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
        if record.styles.index() != 0 {
            let target = record.styles.index() - 1;
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
        let key = (record.segment_token(), record.record_ordinal());
        if raw_records.get(&key) != Some(&"0786eb48d2110c076000f99ac5361ab0") {
            findings.push(finding(
                Check::NativeLinks,
                "Inventor PmGraphics style collection does not resolve to its RSe record".into(),
                Some(record.id().to_owned()),
            ));
        }
        for reference in record.style_references.references() {
            if reference.index() == 0
                || !raw_keys.contains(&(record.segment_token(), reference.index() - 1))
            {
                findings.push(finding(
                    Check::NativeLinks,
                    format!(
                        "Inventor PmGraphics style-collection reference {} does not resolve",
                        reference.index()
                    ),
                    Some(record.id().to_owned()),
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
    body_native_keys: Vec<BodyNativeKey>,
    presentation_record_issues: Vec<RecordIssue>,
    pm_dc_parameters: Vec<PmDcParameter>,
    pm_dc_expressions: Vec<PmDcExpression>,
    pm_dc_units: Vec<PmDcUnit>,
    pm_dc_parameter_tables: Vec<crate::design::scope::PmDcParameterTable>,
    pm_dc_model_roots: Vec<crate::design::scope::PmDcModelRoot>,
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
    pm_dc_sheet_metal_styles: Vec<crate::feature::sheet_metal::style::Style>,
    pm_dc_sheet_metal_flanges: Vec<crate::feature::sheet_metal::flange::Flange>,
    pm_dc_feature_dimensions: Vec<crate::feature::revolve::Dimension>,
    pm_dc_feature_labels: Vec<PmDcFeatureLabel>,
    pm_dc_entity_style_links: Vec<PmDcEntityStyleLink>,
    pm_dc_kernel_entity_links: Vec<PmDcKernelEntityLink>,
    pm_dc_work_planes: Vec<PmDcWorkPlane>,
    feature_record_issues: Vec<RecordIssue>,
    active_carrier: ActiveCarrierRecord,
    unknowns: Vec<NativeUnknownRecord>,
}

impl NativeData {
    fn load(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::native::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::native::NativeConvertError> {
        Ok(Self {
            storage_bands: namespace.arena_as_for_decode(ctx, "storage_bands")?,
            databases: namespace.arena_as_for_decode(ctx, "databases")?,
            database_issues: namespace.arena_as_for_decode(ctx, "database_issues")?,
            registry: namespace.arena_as_for_decode(ctx, "segment_registry")?,
            revisions: namespace.arena_as_for_decode(ctx, "revisions")?,
            pairs: namespace.arena_as_for_decode(ctx, "segment_pairs")?,
            metadata: namespace.arena_as_for_decode(ctx, "segment_meta")?,
            meta_sections: namespace.arena_as_for_decode(ctx, "meta_sections")?,
            meta_types: namespace.arena_as_for_decode(ctx, "meta_types")?,
            metadata_issues: namespace.arena_as_for_decode(ctx, "segment_meta_issues")?,
            bulk: namespace.arena_as_for_decode(ctx, "segment_bulk")?,
            records: namespace.arena_as_for_decode(ctx, "rse_records")?,
            bulk_issues: namespace.arena_as_for_decode(ctx, "segment_bulk_issues")?,
            unpaired: namespace.arena_as_for_decode(ctx, "unpaired_segments")?,
            structural_issues: namespace.arena_as_for_decode(ctx, "structural_issues")?,
            property_sets: namespace.arena_as_for_decode(ctx, "property_sets")?,
            property_sections: namespace.arena_as_for_decode(ctx, "property_sections")?,
            properties: namespace.arena_as_for_decode(ctx, "properties")?,
            property_issues: namespace.arena_as_for_decode(ctx, "property_set_issues")?,
            protein: ProteinRecord::read(ctx, namespace)?,
            protein_assets: namespace.arena_as_for_decode(ctx, "protein_assets")?,
            protein_rejections: namespace.arena_as_for_decode(ctx, "protein_rejections")?,
            ufrx: UfrxRecord::read(ctx, namespace)?,
            assembly_occurrences: namespace.arena_as_for_decode(ctx, "assembly_occurrences")?,
            assembly_placements: namespace.arena_as_for_decode(ctx, "assembly_placements")?,
            assembly_record_issues: namespace.arena_as_for_decode(ctx, "assembly_record_issues")?,
            pm_app_default_styles: namespace.arena_as_for_decode(ctx, "pm_app_default_styles")?,
            pm_app_rendering_styles: namespace
                .arena_as_for_decode(ctx, "pm_app_rendering_styles")?,
            pm_graphics_faces: namespace.arena_as_for_decode(ctx, "pm_graphics_faces")?,
            pm_graphics_style_collections: namespace
                .arena_as_for_decode(ctx, "pm_graphics_style_collections")?,
            pm_graphics_primary_color_styles: namespace
                .arena_as_for_decode(ctx, "pm_graphics_primary_color_styles")?,
            face_native_keys: namespace.arena_as_for_decode(ctx, "face_native_keys")?,
            body_native_keys: namespace.arena_as_for_decode(ctx, "body_native_keys")?,
            presentation_record_issues: namespace
                .arena_as_for_decode(ctx, "presentation_record_issues")?,
            pm_dc_parameters: namespace.arena_as_for_decode(ctx, "pm_dc_parameters")?,
            pm_dc_expressions: namespace.arena_as_for_decode(ctx, "pm_dc_expressions")?,
            pm_dc_units: namespace.arena_as_for_decode(ctx, "pm_dc_units")?,
            pm_dc_parameter_tables: namespace.arena_as_for_decode(ctx, "pm_dc_parameter_tables")?,
            pm_dc_model_roots: namespace.arena_as_for_decode(ctx, "pm_dc_model_roots")?,
            design_record_issues: namespace.arena_as_for_decode(ctx, "design_record_issues")?,
            pm_dc_sketches: namespace.arena_as_for_decode(ctx, "pm_dc_sketches")?,
            pm_dc_sketch_entities: namespace.arena_as_for_decode(ctx, "pm_dc_sketch_entities")?,
            pm_dc_sketch_constraints: namespace
                .arena_as_for_decode(ctx, "pm_dc_sketch_constraints")?,
            pm_dc_transforms: namespace.arena_as_for_decode(ctx, "pm_dc_transforms")?,
            pm_dc_directions: namespace.arena_as_for_decode(ctx, "pm_dc_directions")?,
            sketch_record_issues: namespace.arena_as_for_decode(ctx, "sketch_record_issues")?,
            pm_dc_features: namespace.arena_as_for_decode(ctx, "pm_dc_features")?,
            pm_dc_pattern_features: namespace.arena_as_for_decode(ctx, "pm_dc_pattern_features")?,
            pm_dc_feature_terminators: namespace
                .arena_as_for_decode(ctx, "pm_dc_feature_terminators")?,
            pm_dc_feature_properties: namespace
                .arena_as_for_decode(ctx, "pm_dc_feature_properties")?,
            pm_dc_sheet_metal_styles: namespace
                .arena_as_for_decode(ctx, "pm_dc_sheet_metal_styles")?,
            pm_dc_sheet_metal_flanges: namespace
                .arena_as_for_decode(ctx, "pm_dc_sheet_metal_flanges")?,
            pm_dc_feature_dimensions: namespace
                .arena_as_for_decode(ctx, "pm_dc_feature_dimensions")?,
            pm_dc_feature_labels: namespace.arena_as_for_decode(ctx, "pm_dc_feature_labels")?,
            pm_dc_entity_style_links: namespace
                .arena_as_for_decode(ctx, "pm_dc_entity_style_links")?,
            pm_dc_kernel_entity_links: namespace
                .arena_as_for_decode(ctx, "pm_dc_kernel_entity_links")?,
            pm_dc_work_planes: namespace.arena_as_for_decode(ctx, "pm_dc_work_planes")?,
            feature_record_issues: namespace.arena_as_for_decode(ctx, "feature_record_issues")?,
            active_carrier: ActiveCarrierRecord::read(namespace)?,
            unknowns: namespace.arena_as_for_decode(ctx, "unknowns")?,
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
        .collect::<BTreeMap<_, _>>();
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
            .fold(HashMap::<&str, HashSet<u32>>::new(), |mut types, record| {
                types
                    .entry(record.token.as_str())
                    .or_default()
                    .insert(record.index);
                types
            });
    for (token, meta) in metadata_by_token {
        let expected_sections = (1_u8..=11).collect::<HashSet<_>>();
        if sections_by_token.get(token) != Some(&expected_sections)
            || cadmpeg_core::decode::u64_from_index(
                types_by_token.get(token).map_or(0, HashSet::len),
            ) != meta.type_count
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
                    u32::from(record.type_index()),
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
        findings.push(Finding {
            severity: Severity::Warning,
            ..finding(
                Check::NativeLinks,
                format!("Inventor Protein stream is malformed: {detail}"),
                Some(id.clone()),
            )
        });
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
    let mut positions = BTreeMap::<&str, HashSet<u64>>::new();
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
            .map(ExternalReferenceRecord::ordinal),
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
        findings.push(Finding {
            severity: Severity::Warning,
            ..finding(
                Check::NativeLinks,
                format!("Inventor UFRxDoc stream is malformed: {detail}"),
                Some(id.clone()),
            )
        });
    }
    let model_state_ordinals = data
        .ufrx
        .model_states()
        .iter()
        .map(|state| state.ordinal)
        .collect::<HashSet<_>>();
    if model_state_ordinals.len() != data.ufrx.model_states().len()
        || !u32::try_from(data.ufrx.model_states().len())
            .is_ok_and(|count| model_state_ordinals == (0..count).collect::<HashSet<_>>())
    {
        findings.push(finding(
            Check::NativeLinks,
            "Inventor UFRxDoc model-state ordinals are not contiguous".into(),
            None,
        ));
    }
    if let UfrxRecord::ParsedPrefix(payload) = record {
        if let Some(representation) = payload.representation.as_ref() {
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
                    Some(payload.id.clone()),
                ));
            }
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
    let assembly_ids = data
        .assembly_occurrences
        .iter()
        .map(|occurrence| occurrence.occurrence_id)
        .collect::<HashSet<_>>();
    let mut actual_counts = HashMap::<u32, u64>::new();
    let assembly_document = is_assembly_document(ir);
    for occurrence in data.ufrx.occurrences() {
        let reference = crate::native::ufrx::resolve_occurrence_reference(
            occurrence,
            data.ufrx.external_references(),
        );
        if let Some(reference) = reference {
            *actual_counts.entry(reference.reference_id).or_default() += 1;
        }
        if reference.is_none()
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
                Some(reference.id().clone()),
            ));
        }
    }
}

fn validate_assembly(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
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
        if declared != cadmpeg_core::decode::u64_from_index(data.assembly_occurrences.len()) {
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
        ctx,
        data.ufrx.occurrences(),
        data.ufrx.external_references(),
        &data.assembly_occurrences,
        &data.assembly_placements,
    )?;
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
    Ok(())
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

/// Whether a neutral feature is the flat-pattern model feature of a model root.
fn is_flat_pattern_feature(feature: &cadmpeg_ir::features::Feature) -> bool {
    matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native { kind, .. }
        ) if kind.as_str() == crate::design::scope::FLAT_PATTERN_KIND
    )
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
        related: Vec::new(),
        check,
        severity: Severity::Error,
        message,
        entity,
    }
}

/// Record references a sketch constraint stores, in its header, maps, and kind.
pub(crate) fn sketch_constraint_references(constraint: &PmDcSketchConstraint) -> Vec<u32> {
    let header = &constraint.header;
    let mut references = vec![
        header.content.next.index(),
        header.content.context.index(),
        header.group.index(),
        header
            .parameter
            .map_or(0, crate::pmdc::PmDcReference::index),
    ];
    for (key, _) in header.scalar_map.entries() {
        references.push(key.index());
    }
    // A tangent solver hint stores an unqualified code, not a reference, as its value.
    let hint_codes = matches!(constraint.kind, PmDcSketchConstraintKind::Tangent { .. });
    for (key, value) in header.reference_map.entries() {
        references.push(key.index());
        if !hint_codes {
            references.push(value.index());
        }
    }
    match constraint.kind {
        PmDcSketchConstraintKind::Coincident { first, second }
        | PmDcSketchConstraintKind::Parallel { first, second, .. }
        | PmDcSketchConstraintKind::Perpendicular { first, second, .. }
        | PmDcSketchConstraintKind::Tangent { first, second, .. } => {
            references.extend([first.index(), second.index()]);
        }
        PmDcSketchConstraintKind::Horizontal { entity, .. }
        | PmDcSketchConstraintKind::Vertical { entity, .. } => {
            references.push(entity.index());
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
        } => references.extend([first.index(), second.index(), parameter.index()]),
        PmDcSketchConstraintKind::Radius { entity, .. } => {
            references.push(entity.index());
        }
        PmDcSketchConstraintKind::Diameter {
            reference, entity, ..
        } => references.extend([reference.index(), entity.index()]),
        PmDcSketchConstraintKind::CircleCenter { entity, center } => {
            references.extend([entity.index(), center.index()]);
        }
        PmDcSketchConstraintKind::EqualRadius { first, second }
        | PmDcSketchConstraintKind::EqualLength { first, second }
        | PmDcSketchConstraintKind::Distance { first, second, .. }
        | PmDcSketchConstraintKind::Angle { first, second, .. }
        | PmDcSketchConstraintKind::HorizontalAlignment { first, second }
        | PmDcSketchConstraintKind::VerticalAlignment { first, second }
        | PmDcSketchConstraintKind::Collinear { first, second, .. } => {
            references.extend([first.index(), second.index()]);
        }
        PmDcSketchConstraintKind::SplinePoint { spline, point } => {
            references.extend([spline.index(), point.index()]);
        }
        PmDcSketchConstraintKind::Midpoint { entity, point, .. } => {
            references.extend([entity.index(), point.index()]);
        }
        PmDcSketchConstraintKind::Fix { entity } => references.push(entity.index()),
        PmDcSketchConstraintKind::Symmetric {
            first,
            second,
            axis,
        } => references.extend([first.index(), second.index(), axis.index()]),
        PmDcSketchConstraintKind::EqualDistance { first, second } => {
            references.extend(first.iter().chain(&second).map(|entity| entity.index()));
        }
        PmDcSketchConstraintKind::BlockGroup { ref members } => {
            references.extend(members.references().iter().map(|member| member.index()));
        }
        PmDcSketchConstraintKind::PolygonRelation {
            first,
            second,
            reference,
            polygon,
            ..
        } => references.extend([
            first.index(),
            second.index(),
            reference.index(),
            polygon[0].index(),
            polygon[1].index(),
        ]),
    }
    references
}

/// Whether a fillet edge set's fifth reference is null or selects a Boolean
/// record. `type_at` gives the type identifier of the record at an ordinal. A
/// reference that selects no record is reported by the reference check, so it
/// is admitted here.
pub(crate) fn fillet_option_is_admitted<'a>(
    kind: &PmDcFeaturePropertyKind,
    type_at: impl Fn(u32) -> Option<&'a str>,
) -> bool {
    let PmDcFeaturePropertyKind::FilletEdgeSet {
        option: Some(option),
        ..
    } = kind
    else {
        return true;
    };
    option.index() == 0
        || type_at(option.index() - 1).is_none_or(|type_id| {
            type_id == crate::pmdc::type_id_string(crate::feature::BOOLEAN_TYPE)
        })
}

fn fillet_continuity_type_error<'a>(
    kind: &PmDcFeaturePropertyKind,
    type_at: impl Fn(u32) -> Option<&'a str>,
) -> Option<&'static str> {
    let PmDcFeaturePropertyKind::FilletEdgeSet { continuity, .. } = kind else {
        return None;
    };
    let boolean_type = crate::pmdc::type_id_string(crate::feature::BOOLEAN_TYPE);
    continuity
        .record_ordinal()
        .and_then(type_at)
        .is_some_and(|type_id| type_id != boolean_type)
        .then_some("Inventor PmDc fillet continuity is not a Boolean record")
}

#[cfg(test)]
mod tests;
