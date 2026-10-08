// SPDX-License-Identifier: Apache-2.0
//! Inventor-native validation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use serde::de::DeserializeOwned;

use cadmpeg_asm::brep::records::FaceNativeKey;
use cadmpeg_core::decode::{cost::DecodeCost, DecodeContext};
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
    PmDcEntityStyleLink, PmDcFeature, PmDcFeatureLabel, PmDcFeatureLabelPayloadWire,
    PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureTerminator, PmDcPatternFeature,
};
use crate::sketch::{
    PmDcDirection, PmDcSketch, PmDcSketchConstraint, PmDcSketchConstraintKind, PmDcSketchEntity,
    PmDcSketchEntityKind, PmDcTransform, PointTail,
};

use crate::native::protein::{
    ProteinAssetRecord, ProteinAssetRecordWire, ProteinRecord, ProteinRejectionRecord,
    ProteinRejectionRecordWire,
};
use crate::native::ufrx::{ExternalReferenceRecord, UfrxRecord};
use crate::native::{
    ActiveCarrierRecord, AssemblyOccurrenceRecord, AssemblyPlacementRecord,
    AssemblyPlacementRecordWire, DatabaseIssueRecord, DatabaseRecord, MetaSectionRecord,
    MetaTypeRecord, PmAppDefaultStyleRecord, PmAppRenderingStyleRecord,
    PmAppRenderingStyleRecordWire, PmGraphicsFaceRecord, PmGraphicsFaceRecordWire,
    PmGraphicsPrimaryColorStyleRecord, PmGraphicsStyleCollectionRecord,
    PmGraphicsStyleCollectionRecordWire, PropertyRecord, PropertySectionRecord,
    PropertySetIssueRecord, PropertySetRecord, RevisionRecord, RseRecordRecord,
    SegmentBulkIssueRecord, SegmentBulkRecord, SegmentMetaIssueRecord, SegmentMetaRecord,
    SegmentPairRecord, SegmentRegistryRecord, StorageBandRecord, StructuralIssueRecord,
    UnpairedSegmentRecord,
};
use crate::pmdc::PmDcReferenceList;
use crate::record_identity::{Located, LocatedWire, RecordPayload};
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

pub(crate) fn validate_native(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("inventor") else {
        return Ok(Vec::new());
    };
    let (actual_arenas, actual_arenas_storage) =
        ctx.with_scoped_storage("collect Inventor native arena names", || {
            ctx.collect_btree_set(
                ctx.admit_iter(namespace.arenas(), "visit Inventor native arenas")?
                    .map(|(name, _)| name.as_str()),
                "collect Inventor native arena names",
            )
        })?;
    let expected_arenas = ARENAS.iter().copied().collect::<BTreeSet<_>>();
    if !equal_btree_sets(
        ctx,
        &actual_arenas,
        &expected_arenas,
        "compare Inventor native arena names",
    )? {
        // The expected names are the fixed arena list; walking its ordered
        // set lists the missing ones in order.
        let mut missing = Vec::new();
        for arena in &expected_arenas {
            if !ctx.contains_btree_set(&actual_arenas, arena, "check missing Inventor arena")? {
                missing.push(*arena);
            }
        }
        let (unexpected, _unexpected_storage) =
            ctx.with_scoped_storage("collect unexpected Inventor arenas", || {
                ctx.try_collect_vec(
                    ctx.admit_iter(&actual_arenas, "find unexpected Inventor arenas")?
                        .copied()
                        .filter_map(|arena| {
                            match ctx.contains_btree_set(
                                &expected_arenas,
                                arena,
                                "check unexpected Inventor arena",
                            ) {
                                Ok(true) => None,
                                Ok(false) => Some(Ok(arena)),
                                Err(error) => Some(Err(error)),
                            }
                        }),
                    "collect unexpected Inventor arenas",
                )
            })?;
        drop((actual_arenas, expected_arenas, actual_arenas_storage));
        let mut findings = Vec::new();
        push_finding(
            ctx,
            &mut findings,
            Check::NativeLinks,
            format_args!(
                "Inventor native namespace has missing arenas {missing:?} and unexpected arenas {unexpected:?}"
            ),
            None,
        )?;
        return Ok(findings);
    }
    drop((actual_arenas, expected_arenas, actual_arenas_storage));
    let mut data_storage = ctx.reserve_scoped(0, "load Inventor native data")?;
    let data = match data_storage.with_storage(|| NativeData::load(ctx, namespace)) {
        Ok(data) => data,
        Err(error) => {
            let error = CodecError::from(error);
            if matches!(error, CodecError::ResourceLimit(_)) {
                return Err(error);
            }
            let mut findings = Vec::new();
            push_finding(
                ctx,
                &mut findings,
                Check::NativeLinks,
                format_args!("Inventor native arenas are invalid: {error}"),
                None,
            )?;
            return Ok(findings);
        }
    };

    let mut findings = Vec::new();
    validate_databases(ctx, &data, &mut findings)?;
    validate_segments(ctx, &data, &mut findings)?;
    validate_active_carrier(ctx, &data, &mut findings)?;
    validate_design(ctx, &data, ir, &mut findings)?;
    validate_sketches(ctx, &data, ir, &mut findings)?;
    validate_features(ctx, ir, &data, &mut findings)?;
    unique(
        ctx,
        &mut findings,
        &data.unknowns,
        |record| Ok(record.id.as_str()),
        format_args!("ASM unknown-record id"),
    )?;
    validate_properties(ctx, &data, &mut findings)?;
    validate_protein(ctx, &data, &mut findings)?;
    unique(
        ctx,
        &mut findings,
        &data.protein_assets,
        |record| Ok(record.id.as_str()),
        format_args!("Protein asset id"),
    )?;
    validate_protein_assets(ctx, &data, &mut findings)?;
    validate_protein_rejections(ctx, &data, &mut findings)?;
    validate_protein_record_coverage(ctx, &data, &mut findings)?;
    validate_ufrx(ctx, ir, &data, &mut findings)?;
    validate_assembly(ctx, ir, &data, &mut findings)?;
    validate_presentation(ctx, ir, &data, &mut findings)?;
    let mut structural_issues = data.structural_issues.iter();
    while let Some(issue) = ctx.next_charged(
        &mut structural_issues,
        "validate Inventor structural issues",
    )? {
        push_finding(
            ctx,
            &mut findings,
            Check::NativeLinks,
            format_args!("Inventor {}: {}", issue.scope, issue.detail),
            Some(ctx.copy_retained_text(&issue.id, "retain Inventor structural issue id")?),
        )?;
    }
    let mut property_issues = data.property_issues.iter();
    while let Some(issue) =
        ctx.next_charged(&mut property_issues, "validate Inventor property issues")?
    {
        push_finding(
            ctx,
            &mut findings,
            Check::NativeLinks,
            format_args!("Inventor property set {:?}: {}", issue.path, issue.detail),
            Some(ctx.copy_retained_text(&issue.id, "retain Inventor property issue id")?),
        )?;
    }
    Ok(findings)
}

fn validate_design(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let (raw, raw_storage) =
        ctx.with_scoped_storage("collect Inventor RSe design record index", || {
            ctx.collect_hash_map(
                data.records.iter().map(|record| {
                    (
                        (record.token.as_str(), record.ordinal),
                        record.type_id.as_str(),
                    )
                }),
                "collect Inventor RSe design record index",
            )
        })?;
    let resolves = |token: &str, reference: u32| -> Result<bool, CodecError> {
        if reference == 0 {
            Ok(true)
        } else {
            ctx.contains_key_hash_map(
                &raw,
                &(token, reference - 1),
                "resolve Inventor design reference",
            )
        }
    };
    unique(
        ctx,
        findings,
        &data.pm_dc_parameters,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc parameter",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_expressions,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc expression",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_units,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc unit",
    )?;
    let mut parameter_steps = data.pm_dc_parameters.iter();
    while let Some(parameter) =
        ctx.next_charged(&mut parameter_steps, "validate Inventor PmDc parameters")?
    {
        let key = (
            parameter.identity.segment_token.as_str(),
            parameter.identity.record_ordinal,
        );
        let record_matches = match ctx.get_hash_map(&raw, &key, "find Inventor PmDc parameter")? {
            Some(type_id) => *type_id == parameter.identity.type_id.as_str(),
            None => false,
        };
        if !record_matches
            || !resolves(
                parameter.identity.segment_token.as_str(),
                parameter.header.next.index(),
            )?
            || !resolves(
                parameter.identity.segment_token.as_str(),
                parameter.header.context.index(),
            )?
            || !resolves(
                parameter.identity.segment_token.as_str(),
                parameter.unit.index(),
            )?
            || !resolves(
                parameter.identity.segment_token.as_str(),
                parameter.formula.index(),
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc parameter record or reference does not resolve"),
                Some(ctx.format_retained(
                    format_args!(
                        "inventor:pmdc:parameter#{}-{}",
                        parameter.identity.segment_token, parameter.identity.record_ordinal
                    ),
                    "retain Inventor PmDc parameter identity",
                )?),
            )?;
        }
    }
    let mut expression_steps = data.pm_dc_expressions.iter();
    while let Some(expression) =
        ctx.next_charged(&mut expression_steps, "validate Inventor PmDc expressions")?
    {
        let key = (
            expression.identity.segment_token.as_str(),
            expression.identity.record_ordinal,
        );
        let record_matches = match ctx.get_hash_map(&raw, &key, "find Inventor PmDc expression")? {
            Some(type_id) => *type_id == expression.identity.type_id.as_str(),
            None => false,
        };
        let token = expression.identity.segment_token.as_str();
        let references_match = record_matches
            && resolves(token, expression.unit.index())?
            && match &expression.kind {
                PmDcExpressionKind::Value { .. } => true,
                PmDcExpressionKind::ParameterReference { operand, .. }
                | PmDcExpressionKind::Unary { operand, .. } => resolves(token, operand.index())?,
                PmDcExpressionKind::Binary { left, right, .. } => {
                    resolves(token, left.index())? && resolves(token, right.index())?
                }
            };
        if !references_match {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc expression record or reference does not resolve"),
                Some(ctx.format_retained(
                    format_args!(
                        "inventor:pmdc:expression#{}-{}",
                        expression.identity.segment_token, expression.identity.record_ordinal
                    ),
                    "retain Inventor PmDc expression identity",
                )?),
            )?;
        }
    }
    let mut unit_steps = data.pm_dc_units.iter();
    while let Some(unit) = ctx.next_charged(&mut unit_steps, "validate Inventor PmDc units")? {
        let key = (
            unit.identity.segment_token.as_str(),
            unit.identity.record_ordinal,
        );
        let record_matches = match ctx.get_hash_map(&raw, &key, "find Inventor PmDc unit")? {
            Some(type_id) => *type_id == unit.identity.type_id.as_str(),
            None => false,
        };
        let token = unit.identity.segment_token.as_str();
        // A unit definition's references are checked where they lie.
        let references_resolve = record_matches
            && match &unit.kind {
                PmDcUnitKind::Definition {
                    numerators,
                    denominators,
                    derived,
                    ..
                } => {
                    ctx.all_by(
                        numerators.references(),
                        |reference| resolves(token, reference.index()),
                        "resolve Inventor PmDc unit references",
                    )? && ctx.all_by(
                        denominators.references(),
                        |reference| resolves(token, reference.index()),
                        "resolve Inventor PmDc unit references",
                    )? && resolves(token, derived.index())?
                }
                PmDcUnitKind::Base { .. } => true,
            };
        if !references_resolve {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc unit record or reference does not resolve"),
                Some(ctx.format_retained(
                    format_args!(
                        "inventor:pmdc:unit#{}-{}",
                        unit.identity.segment_token, unit.identity.record_ordinal
                    ),
                    "retain Inventor PmDc unit identity",
                )?),
            )?;
        }
    }
    drop((raw, raw_storage));
    let (mut native_parameter_ids, mut native_parameter_ids_storage) =
        ctx.temporary_set(0, "index Inventor PmDc parameter identities")?;
    let mut identity_records = data.pm_dc_parameters.iter();
    while let Some(record) = ctx.next_charged(
        &mut identity_records,
        "index Inventor PmDc parameter identities",
    )? {
        native_parameter_ids_storage.with_storage(|| {
            let id = ctx.format_retained(
                format_args!(
                    "inventor:pmdc:parameter#{}-{}",
                    record.identity.segment_token, record.identity.record_ordinal
                ),
                "retain Inventor PmDc parameter identity",
            )?;
            ctx.insert_hash_set(
                &mut native_parameter_ids,
                id,
                "index Inventor PmDc parameter identities",
            )
        })?;
    }
    let mut neutral_parameters = ir.model.parameters.iter();
    while let Some(parameter) = ctx.next_charged(
        &mut neutral_parameters,
        "validate Inventor neutral parameters",
    )? {
        let resolves = match parameter.native_ref.as_ref() {
            Some(reference) => ctx.contains_hash_set(
                &native_parameter_ids,
                reference,
                "resolve Inventor neutral parameter source",
            )?,
            None => false,
        };
        if !resolves {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor neutral parameter does not resolve to its PmDc source record"
                ),
                Some(ctx.copy_retained_text(
                    parameter.id.as_str(),
                    "retain Inventor neutral parameter id",
                )?),
            )?;
        }
    }
    drop((native_parameter_ids, native_parameter_ids_storage));
    let mut design_issues = data.design_record_issues.iter();
    while let Some(issue) =
        ctx.next_charged(&mut design_issues, "validate Inventor design record issues")?
    {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor design record: {}", issue.detail),
            Some(ctx.format_retained(
                format_args!(
                    "inventor:pmdc:record#{}-{}",
                    issue.segment_token, issue.record_ordinal
                ),
                "retain Inventor design record identity",
            )?),
        )?;
    }
    Ok(())
}

fn validate_sketches(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let (raw, raw_storage) =
        ctx.with_scoped_storage("collect Inventor RSe sketch record index", || {
            ctx.collect_hash_map(
                data.records.iter().map(|record| {
                    (
                        (record.token.as_str(), record.ordinal),
                        record.type_id.as_str(),
                    )
                }),
                "collect Inventor RSe sketch record index",
            )
        })?;
    let record_is_exact = |token: &str, ordinal: u32, expected: &str| {
        let key = (token, ordinal);
        match ctx.get_hash_map(&raw, &key, "find Inventor RSe sketch record")? {
            Some(actual) => Ok::<_, CodecError>(*actual == expected),
            None => Ok(false),
        }
    };
    let reference_resolves = |token: &str, reference: u32| {
        if reference == 0 {
            return Ok(true);
        }
        ctx.contains_key_hash_map(
            &raw,
            &(token, reference - 1),
            "resolve Inventor PmDc sketch reference",
        )
    };
    unique(
        ctx,
        findings,
        &data.pm_dc_sketches,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc sketch",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_sketch_entities,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc sketch entity",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_transforms,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc transform",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_sketch_constraints,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc sketch constraint",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_directions,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc direction",
    )?;
    let mut sketch_steps = data.pm_dc_sketches.iter();
    while let Some(sketch) =
        ctx.next_charged(&mut sketch_steps, "validate Inventor PmDc sketches")?
    {
        let token = sketch.identity.segment_token.as_str();
        // Each reference source is checked where it lies, stopping at the
        // first reference that does not resolve.
        let references_match = record_is_exact(
            token,
            sketch.identity.record_ordinal,
            sketch.identity.type_id.as_str(),
        )? && reference_resolves(token, sketch.header.next.index())?
            && reference_resolves(token, sketch.header.context.index())?
            && reference_resolves(token, sketch.transform.index())?
            && reference_resolves(token, sketch.direction.index())?
            && ctx.all_by(
                sketch.entities.references(),
                |reference| reference_resolves(token, reference.index()),
                "resolve Inventor PmDc sketch entity references",
            )?
            && match &sketch.auxiliary {
                Some(list) => ctx.all_by(
                    list.references(),
                    |reference| reference_resolves(token, reference.index()),
                    "resolve Inventor PmDc sketch auxiliary references",
                )?,
                None => true,
            };
        if !references_match {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc sketch record or reference does not resolve"),
                Some(sketch.id(ctx)?),
            )?;
        }
    }
    let mut entity_steps = data.pm_dc_sketch_entities.iter();
    while let Some(entity) =
        ctx.next_charged(&mut entity_steps, "validate Inventor PmDc sketch entities")?
    {
        let token = entity.identity.segment_token.as_str();
        let header_matches = record_is_exact(
            token,
            entity.identity.record_ordinal,
            entity.identity.type_id.as_str(),
        )? && reference_resolves(token, entity.header.next.index())?
            && reference_resolves(token, entity.header.context.index())?
            && reference_resolves(token, entity.sketch.index())?;
        let list_resolves = |list: &PmDcReferenceList| {
            ctx.all_by(
                list.references(),
                |reference| reference_resolves(token, reference.index()),
                "resolve Inventor PmDc sketch entity references",
            )
        };
        let references_match = header_matches
            && match &entity.kind {
                PmDcSketchEntityKind::Point {
                    endpoint_of,
                    center_of,
                    tail,
                    ..
                } => {
                    list_resolves(endpoint_of)?
                        && list_resolves(center_of)?
                        && match tail {
                            PointTail::Absent => true,
                            PointTail::Present { associations, .. } => list_resolves(associations)?,
                        }
                }
                PmDcSketchEntityKind::Line {
                    points, auxiliary, ..
                } => {
                    list_resolves(points)?
                        && ctx.all_by(
                            auxiliary,
                            list_resolves,
                            "visit Inventor PmDc sketch entity auxiliary lists",
                        )?
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
                    list_resolves(points)?
                        && ctx.all_by(
                            auxiliary,
                            list_resolves,
                            "visit Inventor PmDc sketch entity auxiliary lists",
                        )?
                        && reference_resolves(token, center.index())?
                }
            };
        if !references_match {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc sketch-entity record or reference does not resolve"),
                Some(entity.id(ctx)?),
            )?;
        }
    }
    let mut transform_steps = data.pm_dc_transforms.iter();
    while let Some(transform) =
        ctx.next_charged(&mut transform_steps, "validate Inventor PmDc transforms")?
    {
        if !record_is_exact(
            transform.identity.segment_token.as_str(),
            transform.identity.record_ordinal,
            transform.identity.type_id.as_str(),
        )? || !reference_resolves(
            transform.identity.segment_token.as_str(),
            transform.header.next.index(),
        )? || !reference_resolves(
            transform.identity.segment_token.as_str(),
            transform.header.context.index(),
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc transform record or reference does not resolve"),
                Some(transform.id(ctx)?),
            )?;
        }
    }
    let mut constraint_steps = data.pm_dc_sketch_constraints.iter();
    while let Some(constraint) = ctx.next_charged(
        &mut constraint_steps,
        "validate Inventor PmDc sketch constraints",
    )? {
        let header = &constraint.header;
        let token = constraint.identity.segment_token.as_str();
        let references_match = record_is_exact(
            token,
            constraint.identity.record_ordinal,
            constraint.identity.type_id.as_str(),
        )? && reference_resolves(token, header.content.next.index())?
            && reference_resolves(token, header.content.context.index())?
            && reference_resolves(token, header.group.index())?
            && reference_resolves(token, header.parameter.index())?
            && ctx.all_by(
                header.scalar_map.entries(),
                |(key, _)| reference_resolves(token, key.index()),
                "resolve Inventor PmDc scalar-map entries",
            )?
            && ctx.all_by(
                header.reference_map.entries(),
                |(key, value)| {
                    Ok(reference_resolves(token, key.index())?
                        && reference_resolves(token, value.index())?)
                },
                "resolve Inventor PmDc reference-map entries",
            )?
            && match &constraint.kind {
                PmDcSketchConstraintKind::Coincident { first, second }
                | PmDcSketchConstraintKind::Parallel { first, second, .. }
                | PmDcSketchConstraintKind::Perpendicular { first, second, .. }
                | PmDcSketchConstraintKind::Tangent { first, second, .. }
                | PmDcSketchConstraintKind::EqualRadius { first, second } => {
                    reference_resolves(token, first.index())?
                        && reference_resolves(token, second.index())?
                }
                PmDcSketchConstraintKind::Horizontal { entity, .. }
                | PmDcSketchConstraintKind::Vertical { entity, .. }
                | PmDcSketchConstraintKind::Radius { entity, .. } => {
                    reference_resolves(token, entity.index())?
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
                } => {
                    reference_resolves(token, first.index())?
                        && reference_resolves(token, second.index())?
                        && reference_resolves(token, parameter.index())?
                }
                PmDcSketchConstraintKind::Diameter {
                    reference, entity, ..
                } => {
                    reference_resolves(token, reference.index())?
                        && reference_resolves(token, entity.index())?
                }
                PmDcSketchConstraintKind::CircleCenter { entity, center } => {
                    reference_resolves(token, entity.index())?
                        && reference_resolves(token, center.index())?
                }
            };
        if !references_match {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor PmDc sketch-constraint record or reference does not resolve"
                ),
                Some(constraint.id(ctx)?),
            )?;
        }
    }
    let mut direction_steps = data.pm_dc_directions.iter();
    while let Some(direction) =
        ctx.next_charged(&mut direction_steps, "validate Inventor PmDc directions")?
    {
        if !record_is_exact(
            direction.identity.segment_token.as_str(),
            direction.identity.record_ordinal,
            direction.identity.type_id.as_str(),
        )? || !reference_resolves(
            direction.identity.segment_token.as_str(),
            direction.header.next.index(),
        )? || !reference_resolves(
            direction.identity.segment_token.as_str(),
            direction.header.context.index(),
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc direction record or reference does not resolve"),
                Some(direction.id(ctx)?),
            )?;
        }
    }
    drop((raw, raw_storage));
    let (mut native_sketches, mut native_sketches_storage) =
        ctx.temporary_set(0, "index Inventor PmDc sketches")?;
    let mut sketch_index = data.pm_dc_sketches.iter();
    while let Some(record) = ctx.next_charged(&mut sketch_index, "index Inventor PmDc sketches")? {
        native_sketches_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut native_sketches,
                record.id(ctx)?,
                "index Inventor PmDc sketches",
            )
        })?;
    }
    let (mut native_entities, mut native_entities_storage) =
        ctx.temporary_set(0, "index Inventor PmDc sketch entities")?;
    let mut entity_index = data.pm_dc_sketch_entities.iter();
    while let Some(record) =
        ctx.next_charged(&mut entity_index, "index Inventor PmDc sketch entities")?
    {
        native_entities_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut native_entities,
                record.id(ctx)?,
                "index Inventor PmDc sketch entities",
            )
        })?;
    }
    let (mut native_constraints, mut native_constraints_storage) =
        ctx.temporary_set(0, "index Inventor PmDc sketch constraints")?;
    let mut constraint_index = data.pm_dc_sketch_constraints.iter();
    while let Some(record) = ctx.next_charged(
        &mut constraint_index,
        "index Inventor PmDc sketch constraints",
    )? {
        native_constraints_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut native_constraints,
                record.id(ctx)?,
                "index Inventor PmDc sketch constraints",
            )
        })?;
    }
    let mut neutral_sketches = ir.model.sketches.iter();
    while let Some(sketch) =
        ctx.next_charged(&mut neutral_sketches, "validate Inventor neutral sketches")?
    {
        let resolves = match sketch.native_ref.as_deref() {
            Some(reference) => ctx.contains_hash_set(
                &native_sketches,
                reference,
                "resolve Inventor neutral sketch source",
            )?,
            None => false,
        };
        if !resolves {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor neutral sketch does not resolve to its PmDc source record"),
                Some(
                    ctx.copy_retained_text(
                        sketch.id.as_str(),
                        "retain Inventor neutral sketch id",
                    )?,
                ),
            )?;
        }
    }
    let mut neutral_entities = ir.model.sketch_entities.iter();
    while let Some(entity) = ctx.next_charged(
        &mut neutral_entities,
        "validate Inventor neutral sketch entities",
    )? {
        let native_resolves = match entity.native_ref.as_deref() {
            Some(reference) => ctx.contains_hash_set(
                &native_entities,
                reference,
                "resolve Inventor neutral sketch entity source",
            )?,
            None => false,
        };
        let endpoint_resolves = ctx.all_by(
            &entity.endpoint_refs,
            |reference| {
                ctx.contains_hash_set(
                    &native_entities,
                    reference.as_str(),
                    "resolve Inventor neutral endpoint source",
                )
            },
            "validate Inventor neutral sketch endpoints",
        )?;
        if !native_resolves || !endpoint_resolves {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor neutral sketch entity does not resolve to its PmDc source records"
                ),
                Some(ctx.copy_retained_text(
                    entity.id().as_str(),
                    "retain Inventor neutral sketch-entity id",
                )?),
            )?;
        }
    }
    let mut neutral_constraints = ir.model.sketch_constraints.iter();
    while let Some(constraint) = ctx.next_charged(
        &mut neutral_constraints,
        "validate Inventor neutral sketch constraints",
    )? {
        let resolves = match constraint.native_ref.as_deref() {
            Some(reference) => ctx.contains_hash_set(
                &native_constraints,
                reference,
                "resolve Inventor neutral sketch constraint source",
            )?,
            None => false,
        };
        if !resolves {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor neutral sketch constraint does not resolve to its PmDc source record"
                ),
                Some(ctx.copy_retained_text(
                    constraint.id.as_str(),
                    "retain Inventor neutral sketch-constraint id",
                )?),
            )?;
        }
    }
    drop((
        native_sketches,
        native_sketches_storage,
        native_entities,
        native_entities_storage,
        native_constraints,
        native_constraints_storage,
    ));
    let mut sketch_issues = data.sketch_record_issues.iter();
    while let Some(issue) =
        ctx.next_charged(&mut sketch_issues, "validate Inventor sketch record issues")?
    {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor sketch record: {}", issue.detail),
            Some(issue.id(ctx)?),
        )?;
    }
    Ok(())
}

fn validate_features(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let (raw, raw_storage) =
        ctx.with_scoped_storage("collect Inventor RSe feature record index", || {
            ctx.collect_hash_map(
                data.records.iter().map(|record| {
                    (
                        (record.token.as_str(), record.ordinal),
                        record.type_id.as_str(),
                    )
                }),
                "collect Inventor RSe feature record index",
            )
        })?;
    let resolves = |token: &str, reference: u32| -> Result<bool, CodecError> {
        if reference == 0 {
            Ok(true)
        } else {
            ctx.contains_key_hash_map(
                &raw,
                &(token, reference - 1),
                "resolve Inventor feature reference",
            )
        }
    };
    unique(
        ctx,
        findings,
        &data.pm_dc_features,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc feature",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_feature_terminators,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc feature terminator",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_pattern_features,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc pattern feature",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_feature_properties,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc feature property",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_dc_feature_labels,
        |record| {
            Ok((
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ))
        },
        "Inventor PmDc feature label",
    )?;
    let mut feature_steps = data.pm_dc_features.iter();
    while let Some(feature) =
        ctx.next_charged(&mut feature_steps, "validate Inventor PmDc features")?
    {
        let key = (
            feature.identity.segment_token.as_str(),
            feature.identity.record_ordinal,
        );
        let record_matches = match ctx.get_hash_map(&raw, &key, "find Inventor PmDc feature")? {
            Some(type_id) => *type_id == feature.identity.type_id.as_str(),
            None => false,
        };
        if !record_matches
            || !resolves(
                feature.identity.segment_token.as_str(),
                feature.header.next.index(),
            )?
            || !resolves(
                feature.identity.segment_token.as_str(),
                feature.header.context.index(),
            )?
            || !ctx.all_by(
                feature.properties.references(),
                |reference| resolves(feature.identity.segment_token.as_str(), reference.index()),
                "resolve Inventor PmDc feature property references",
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc feature record or reference does not resolve"),
                Some(feature.id(ctx)?),
            )?;
        }
    }
    let mut pattern_steps = data.pm_dc_pattern_features.iter();
    while let Some(feature) = ctx.next_charged(
        &mut pattern_steps,
        "validate Inventor PmDc pattern features",
    )? {
        let key = (
            feature.identity.segment_token.as_str(),
            feature.identity.record_ordinal,
        );
        let record_matches =
            match ctx.get_hash_map(&raw, &key, "find Inventor PmDc pattern feature")? {
                Some(type_id) => *type_id == feature.identity.type_id.as_str(),
                None => false,
            };
        let token = feature.identity.segment_token.as_str();
        if !record_matches
            || !resolves(
                feature.identity.segment_token.as_str(),
                feature.header.next.index(),
            )?
            || !resolves(
                feature.identity.segment_token.as_str(),
                feature.header.context.index(),
            )?
            || !ctx.all_by(
                feature.properties.references(),
                |reference| resolves(token, reference.index()),
                "resolve Inventor PmDc pattern-feature properties",
            )?
            || !ctx.all_by(
                feature.participants.references(),
                |reference| resolves(token, reference.index()),
                "resolve Inventor PmDc pattern participants",
            )?
            || !ctx.all_by(
                &feature.property_slots,
                |reference| resolves(token, reference.index()),
                "resolve Inventor PmDc pattern property slots",
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc pattern-feature record or reference does not resolve"),
                Some(feature.id(ctx)?),
            )?;
        }
    }
    let mut property_steps = data.pm_dc_feature_properties.iter();
    while let Some(property) = ctx.next_charged(
        &mut property_steps,
        "validate Inventor PmDc feature properties",
    )? {
        let token = property.identity.segment_token.as_str();
        let key = (token, property.identity.record_ordinal);
        let record_matches =
            match ctx.get_hash_map(&raw, &key, "find Inventor PmDc feature property")? {
                Some(type_id) => *type_id == property.identity.type_id.as_str(),
                None => false,
            };
        let references_match = record_matches
            && resolves(token, property.header.next.index())?
            && resolves(token, property.header.context.index())?
            && match &property.kind {
                PmDcFeaturePropertyKind::References { items, .. } => ctx.all_by(
                    items.references(),
                    |reference| resolves(token, reference.index()),
                    "resolve Inventor PmDc feature-property references",
                )?,
                PmDcFeaturePropertyKind::SurfaceBody { body } => resolves(token, body.index())?,
                PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } => {
                    resolves(token, entity_link.index())?
                }
                PmDcFeaturePropertyKind::Placement {
                    transform,
                    point,
                    value,
                } => {
                    resolves(token, transform.index())?
                        && resolves(token, point.index())?
                        && resolves(token, value.index())?
                }
                PmDcFeaturePropertyKind::FilletEdgeSet {
                    edges,
                    radius,
                    selection,
                    continuity,
                } => {
                    resolves(token, edges.index())?
                        && resolves(token, radius.index())?
                        && resolves(token, selection.index())?
                        && resolves(token, continuity.index())?
                }
                PmDcFeaturePropertyKind::Enumeration { .. }
                | PmDcFeaturePropertyKind::WideEnumeration { .. }
                | PmDcFeaturePropertyKind::Boolean { .. }
                | PmDcFeaturePropertyKind::RdxVariable { .. }
                | PmDcFeaturePropertyKind::EdgeItem { .. } => true,
            };
        if !references_match {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc feature-property record or reference does not resolve"),
                Some(property.id(ctx)?),
            )?;
        }
    }
    let mut style_link_steps = data.pm_dc_entity_style_links.iter();
    while let Some(link) = ctx.next_charged(
        &mut style_link_steps,
        "validate Inventor entity-style links",
    )? {
        let key = (
            link.identity.segment_token.as_str(),
            link.identity.record_ordinal,
        );
        let record_matches =
            match ctx.get_hash_map(&raw, &key, "find Inventor PmDc entity-style link")? {
                Some(type_id) => *type_id == link.identity.type_id.as_str(),
                None => false,
            };
        if !record_matches
            || !resolves(
                link.identity.segment_token.as_str(),
                link.header.owner.index(),
            )?
            || !resolves(
                link.identity.segment_token.as_str(),
                link.header.parent.index(),
            )?
            || !resolves(
                link.identity.segment_token.as_str(),
                link.header.next.index(),
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor PmDc entity-style-link record or reference does not resolve"
                ),
                Some(link.id(ctx)?),
            )?;
        }
    }
    let mut label_steps = data.pm_dc_feature_labels.iter();
    while let Some(label) =
        ctx.next_charged(&mut label_steps, "validate Inventor PmDc feature labels")?
    {
        let token = label.identity.segment_token.as_str();
        let key = (token, label.identity.record_ordinal);
        let record_matches =
            match ctx.get_hash_map(&raw, &key, "find Inventor PmDc feature label")? {
                Some(type_id) => *type_id == label.identity.type_id.as_str(),
                None => false,
            };
        if !record_matches
            || !resolves(
                label.identity.segment_token.as_str(),
                label.header.owner.index(),
            )?
            || !resolves(
                label.identity.segment_token.as_str(),
                label.header.parent.index(),
            )?
            || !resolves(
                label.identity.segment_token.as_str(),
                label.header.next.index(),
            )?
            || !ctx.all_by(
                label.participants.references(),
                |reference| resolves(token, reference.index()),
                "resolve Inventor PmDc feature-label participants",
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmDc feature-label record or reference does not resolve"),
                Some(label.id(ctx)?),
            )?;
        }
    }
    let mut terminator_steps = data.pm_dc_feature_terminators.iter();
    while let Some(terminator) = ctx.next_charged(
        &mut terminator_steps,
        "validate Inventor PmDc feature terminators",
    )? {
        let key = (
            terminator.identity.segment_token.as_str(),
            terminator.identity.record_ordinal,
        );
        let record_matches =
            match ctx.get_hash_map(&raw, &key, "find Inventor PmDc feature terminator")? {
                Some(type_id) => *type_id == terminator.identity.type_id.as_str(),
                None => false,
            };
        if !record_matches
            || !resolves(
                terminator.identity.segment_token.as_str(),
                terminator.header.next.index(),
            )?
            || !resolves(
                terminator.identity.segment_token.as_str(),
                terminator.header.context.index(),
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor PmDc feature-terminator record or reference does not resolve"
                ),
                Some(terminator.id(ctx)?),
            )?;
        }
    }
    drop((raw, raw_storage));
    let mut raw_features = HashMap::new();
    let mut raw_features_storage =
        ctx.reserve_scoped(0, "index Inventor PmDc features by identity")?;
    let mut feature_index = data.pm_dc_features.iter();
    while let Some(feature) = ctx.next_charged(
        &mut feature_index,
        "index Inventor PmDc features by identity",
    )? {
        raw_features_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut raw_features,
                feature.id(ctx)?,
                feature,
                "index Inventor PmDc features by identity",
            )
        })?;
    }
    let mut labels = HashMap::new();
    let mut labels_storage = ctx.reserve_scoped(0, "index Inventor PmDc labels")?;
    let mut label_index = data.pm_dc_feature_labels.iter();
    while let Some(label) = ctx.next_charged(&mut label_index, "index Inventor PmDc labels")? {
        if let Some(ordinal) = label.header.owner.index().checked_sub(1) {
            labels_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut labels,
                    (label.identity.segment_token.as_str(), ordinal),
                    label,
                    "index Inventor PmDc labels",
                )
            })?;
        }
    }
    let mut properties = HashMap::new();
    let mut properties_storage =
        ctx.reserve_scoped(0, "index Inventor PmDc feature properties by identity")?;
    let mut properties_by_record = HashMap::new();
    let mut properties_by_record_storage =
        ctx.reserve_scoped(0, "index Inventor PmDc feature properties by record")?;
    let mut property_index = data.pm_dc_feature_properties.iter();
    while let Some(property) = ctx.next_charged(
        &mut property_index,
        "index Inventor PmDc feature properties",
    )? {
        properties_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut properties,
                property.id(ctx)?,
                property,
                "index Inventor PmDc feature properties by identity",
            )
        })?;
        properties_by_record_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut properties_by_record,
                (
                    property.identity.segment_token.as_str(),
                    property.identity.record_ordinal,
                ),
                property,
                "index Inventor PmDc feature properties by record",
            )
        })?;
    }
    let mut results = HashMap::new();
    let mut results_storage = ctx.reserve_scoped(0, "index Inventor feature results")?;
    let mut feature_results = ir.model.feature_result_topologies.iter();
    while let Some(result) =
        ctx.next_charged(&mut feature_results, "index Inventor feature results")?
    {
        results_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut results,
                &result.output_of,
                result,
                "index Inventor feature results",
            )
        })?;
    }
    let mut neutral_features = ir.model.features.iter();
    while let Some(feature) =
        ctx.next_charged(&mut neutral_features, "validate Inventor neutral features")?
    {
        let raw_feature = match feature.native_ref.as_deref() {
            Some(native) => ctx
                .get_hash_map(
                    &raw_features,
                    native,
                    "resolve Inventor neutral feature source",
                )?
                .copied(),
            None => None,
        };
        let Some(raw_feature) = raw_feature else {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor neutral feature does not resolve to its PmDc source record"),
                Some(ctx.copy_retained_text(
                    feature.id.as_str(),
                    "retain Inventor neutral feature id",
                )?),
            )?;
            continue;
        };
        let Some(family) =
            crate::feature::FeatureFamily::from_definition(feature.evaluation.definition())
        else {
            continue;
        };
        let expected_class = family.class_id();
        let output_slot = family.output_slot();
        let label_key = (
            raw_feature.identity.segment_token.as_str(),
            raw_feature.identity.record_ordinal,
        );
        let class_matches =
            match ctx.get_hash_map(&labels, &label_key, "find Inventor PmDc feature label")? {
                Some(label) => label.class_id() == expected_class,
                None => false,
            };
        if !class_matches {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor neutral feature family does not match its PmDc label"),
                Some(ctx.copy_retained_text(
                    feature.id.as_str(),
                    "retain Inventor neutral feature id",
                )?),
            )?;
        }
        let expected_collection = raw_feature
            .properties
            .references()
            .get(output_slot)
            .and_then(|reference| reference.index().checked_sub(1))
            .map(|ordinal| (raw_feature.identity.segment_token.as_str(), ordinal));
        let output_matches = if let Some(collection_key) = expected_collection {
            if let Some(collection) = ctx.get_hash_map(
                &properties_by_record,
                &collection_key,
                "find Inventor PmDc output collection",
            )? {
                match ctx.get_hash_map(
                    &results,
                    &feature.id,
                    "find Inventor neutral feature result",
                )? {
                    Some(result) => match result.native_ref.as_deref() {
                        Some(native) => match ctx.get_hash_map(
                            &properties,
                            native,
                            "resolve Inventor feature output collection",
                        )? {
                            Some(actual) => ctx.equal(
                                &(
                                    actual.identity.segment_token.as_str(),
                                    actual.identity.record_ordinal,
                                ),
                                &(
                                    collection.identity.segment_token.as_str(),
                                    collection.identity.record_ordinal,
                                ),
                                "compare Inventor feature output collection",
                            )?,
                            None => false,
                        },
                        None => false,
                    },
                    None => false,
                }
            } else {
                false
            }
        } else {
            false
        };
        if !output_matches {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor neutral feature result does not match its PmDc output slot"),
                Some(ctx.copy_retained_text(
                    feature.id.as_str(),
                    "retain Inventor neutral feature id",
                )?),
            )?;
        }
    }
    drop((
        raw_features,
        raw_features_storage,
        labels,
        labels_storage,
        results,
        results_storage,
    ));
    let mut result_topologies = ir.model.feature_result_topologies.iter();
    while let Some(result) = ctx.next_charged(
        &mut result_topologies,
        "validate Inventor feature result topologies",
    )? {
        let collection = match result.native_ref.as_deref() {
            Some(native) => ctx
                .get_hash_map(
                    &properties,
                    native,
                    "resolve Inventor feature result collection",
                )?
                .copied(),
            None => None,
        };
        let Some(collection) = collection else {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor feature result does not resolve to its PmDc object collection"
                ),
                Some(
                    ctx.copy_retained_text(
                        result.id.as_str(),
                        "retain Inventor feature-result id",
                    )?,
                ),
            )?;
            continue;
        };
        let PmDcFeaturePropertyKind::References {
            family: crate::feature::PmDcFeatureReferenceFamily::ObjectCollection,
            items,
        } = &collection.kind
        else {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor feature result native reference is not an object collection"
                ),
                Some(
                    ctx.copy_retained_text(
                        result.id.as_str(),
                        "retain Inventor feature-result id",
                    )?,
                ),
            )?;
            continue;
        };
        let body_references = items.references();
        let bodies_match = body_references.len() == result.bodies().len()
            && ctx.all_by(
                body_references.iter().zip(result.bodies()),
                |(reference, actual)| {
                    let Some(ordinal) = reference.index().checked_sub(1) else {
                        return Ok(false);
                    };
                    let Some(property) = ctx.get_hash_map(
                        &properties_by_record,
                        &(collection.identity.segment_token.as_str(), ordinal),
                        "find Inventor feature-result body",
                    )?
                    else {
                        return Ok(false);
                    };
                    if !matches!(property.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
                        return Ok(false);
                    }
                    if property.id_len() != Some(actual.as_str().len()) {
                        return Ok(false);
                    }
                    let Some(actual_property) = ctx.get_hash_map(
                        &properties,
                        actual.as_str(),
                        "resolve Inventor neutral body source",
                    )?
                    else {
                        return Ok(false);
                    };
                    ctx.equal(
                        &(
                            property.identity.segment_token.as_str(),
                            property.identity.record_ordinal,
                        ),
                        &(
                            actual_property.identity.segment_token.as_str(),
                            actual_property.identity.record_ordinal,
                        ),
                        "compare Inventor feature-result body id",
                    )
                },
                "compare Inventor feature-result bodies",
            )?;
        if !bodies_match {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor feature result bodies do not match its PmDc object collection"
                ),
                Some(
                    ctx.copy_retained_text(
                        result.id.as_str(),
                        "retain Inventor feature-result id",
                    )?,
                ),
            )?;
        }
    }
    drop((
        properties,
        properties_storage,
        properties_by_record,
        properties_by_record_storage,
    ));
    let mut feature_issues = data.feature_record_issues.iter();
    while let Some(issue) = ctx.next_charged(
        &mut feature_issues,
        "validate Inventor feature record issues",
    )? {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor feature record: {}", issue.detail),
            Some(issue.id(ctx)?),
        )?;
    }
    Ok(())
}

fn validate_presentation(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    unique(
        ctx,
        findings,
        &data.pm_app_default_styles,
        |record| Ok(record.id.as_str()),
        "PmApp default-style id",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_app_rendering_styles,
        |record| Ok(record.id.as_str()),
        "PmApp rendering-style id",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_app_default_styles,
        |record| Ok((record.segment_token.as_str(), record.record_ordinal)),
        "PmApp default-style record key",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_app_rendering_styles,
        |record| Ok((record.segment_token.as_str(), record.record_ordinal)),
        "PmApp rendering-style record key",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_graphics_faces,
        |record| Ok(record.id.as_str()),
        "PmGraphics face id",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_graphics_faces,
        |record| Ok((record.segment_token.as_str(), record.record_ordinal)),
        "PmGraphics face record key",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_graphics_style_collections,
        |record| Ok(record.id()),
        "PmGraphics style-collection id",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_graphics_style_collections,
        |record| Ok((record.segment_token(), record.record_ordinal())),
        "PmGraphics style-collection record key",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_graphics_primary_color_styles,
        |record| Ok(record.id.as_str()),
        "PmGraphics primary-color style id",
    )?;
    unique(
        ctx,
        findings,
        &data.pm_graphics_primary_color_styles,
        |record| Ok((record.segment_token.as_str(), record.record_ordinal)),
        "PmGraphics primary-color style record key",
    )?;
    unique(
        ctx,
        findings,
        &data.face_native_keys,
        |record| Ok((record.source_namespace.as_str(), record.record_index)),
        "ASM face-native-key id",
    )?;

    let (mut face_keys, mut face_keys_storage) =
        ctx.temporary_set(0, "check ASM face Design keys")?;
    let mut face_key_steps = data.face_native_keys.iter();
    while let Some(record) = ctx.next_charged(&mut face_key_steps, "check ASM face Design keys")? {
        if let Some(key) = record.asm_face_key {
            let inserted = face_keys_storage.with_storage(|| {
                ctx.insert_hash_set(&mut face_keys, key, "check ASM face Design keys")
            })?;
            if !inserted {
                push_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    format_args!("Inventor native data repeats a ASM non-null face Design key"),
                    None,
                )?;
            }
        }
    }
    drop((face_keys, face_keys_storage));
    let (neutral_faces, neutral_faces_storage) =
        ctx.with_scoped_storage("index Inventor neutral face ids", || {
            ctx.collect_hash_set(
                ir.model.faces.iter().map(|face| face.id.as_str()),
                "index Inventor neutral face ids",
            )
        })?;
    let mut face_records = data.face_native_keys.iter();
    while let Some(record) = ctx.next_charged(&mut face_records, "validate ASM face-native keys")? {
        if !ctx.contains_hash_set(
            &neutral_faces,
            record.face.as_str(),
            "resolve ASM face-native key",
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor ASM face-native-key record does not resolve to a neutral face"
                ),
                Some(ctx.format_retained(
                    format_args!(
                        "{}:face-native-key#{}",
                        record.source_namespace.as_str(),
                        record.record_index
                    ),
                    "retain ASM face-native-key id",
                )?),
            )?;
        }
    }

    drop((neutral_faces, neutral_faces_storage));
    let (raw_records, raw_records_storage) =
        ctx.with_scoped_storage("collect Inventor RSe presentation record index", || {
            ctx.collect_hash_map(
                data.records.iter().map(|record| {
                    (
                        (record.token.as_str(), record.ordinal),
                        record.type_id.as_str(),
                    )
                }),
                "collect Inventor RSe presentation record index",
            )
        })?;
    let (rendering_keys, rendering_keys_storage) =
        ctx.with_scoped_storage("collect Inventor rendering-style keys", || {
            ctx.collect_hash_set(
                data.pm_app_rendering_styles
                    .iter()
                    .map(|record| (record.segment_token.as_str(), record.record_ordinal)),
                "collect Inventor rendering-style keys",
            )
        })?;
    let mut default_styles = data.pm_app_default_styles.iter();
    while let Some(record) =
        ctx.next_charged(&mut default_styles, "validate Inventor default styles")?
    {
        let key = (record.segment_token.as_str(), record.record_ordinal);
        let record_matches = match ctx.get_hash_map(
            &raw_records,
            &key,
            "find Inventor PmApp default style record",
        )? {
            Some(type_id) => *type_id == "cdecfb11d1116b250008ebbb21eddc09",
            None => false,
        };
        if !record_matches {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmApp default style does not resolve to its RSe record"),
                Some(ctx.copy_retained_text(&record.id, "retain Inventor PmApp default-style id")?),
            )?;
        }
        if let Some(ordinal) = record.rendering_style_reference.checked_sub(1) {
            if !ctx.contains_hash_set(
                &rendering_keys,
                &(record.segment_token.as_str(), ordinal),
                "resolve Inventor PmApp rendering-style reference",
            )? {
                push_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    format_args!(
                        "Inventor PmApp default rendering-style reference does not resolve"
                    ),
                    Some(ctx.copy_retained_text(
                        &record.id,
                        "retain Inventor PmApp default-style id",
                    )?),
                )?;
            }
        }
    }
    drop((rendering_keys, rendering_keys_storage));
    let mut rendering_styles = data.pm_app_rendering_styles.iter();
    while let Some(record) =
        ctx.next_charged(&mut rendering_styles, "validate Inventor rendering styles")?
    {
        let key = (record.segment_token.as_str(), record.record_ordinal);
        let record_matches = match ctx.get_hash_map(
            &raw_records,
            &key,
            "find Inventor PmApp rendering style record",
        )? {
            Some(type_id) => *type_id == "6fd85967d2113878600094b70b02ecb0",
            None => false,
        };
        if !record_matches {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmApp rendering style does not resolve to its RSe record"),
                Some(
                    ctx.copy_retained_text(&record.id, "retain Inventor PmApp rendering-style id")?,
                ),
            )?;
        }
    }
    let mut graphics_faces = data.pm_graphics_faces.iter();
    while let Some(record) =
        ctx.next_charged(&mut graphics_faces, "validate Inventor PmGraphics faces")?
    {
        let token = record.segment_token.as_str();
        let key = (token, record.record_ordinal);
        let record_matches =
            match ctx.get_hash_map(&raw_records, &key, "find Inventor PmGraphics face record")? {
                Some(type_id) => *type_id == "a3e99451d2119b2860006ab72c39cdb0",
                None => false,
            };
        if !record_matches {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor PmGraphics face does not resolve to its RSe record"),
                Some(ctx.copy_retained_text(&record.id, "retain Inventor PmGraphics face id")?),
            )?;
        }
        {
            let mut validate_reference = |reference| -> Result<(), CodecError> {
                if reference != 0 {
                    let resolves = ctx.contains_key_hash_map(
                        &raw_records,
                        &(token, reference - 1),
                        "resolve Inventor PmGraphics face reference",
                    )?;
                    if !resolves {
                        push_finding(
                            ctx,
                            findings,
                            Check::NativeLinks,
                            format_args!(
                                "Inventor PmGraphics face reference {reference} does not resolve"
                            ),
                            Some(ctx.copy_retained_text(
                                &record.id,
                                "retain Inventor PmGraphics face id",
                            )?),
                        )?;
                    }
                }
                Ok(())
            };
            for reference in [record.surface.index(), record.parent.index()] {
                validate_reference(reference)?;
            }
            let mut edge_references = record.edge_references.references().iter();
            while let Some(reference) = ctx.next_charged(
                &mut edge_references,
                "visit Inventor PmGraphics edge references",
            )? {
                validate_reference(reference.index())?;
            }
        }
        if record.styles.index() != 0 {
            let style_key = (token, record.styles.index() - 1);
            let style_matches = match ctx.get_hash_map(
                &raw_records,
                &style_key,
                "find Inventor PmGraphics style collection",
            )? {
                Some(type_id) => *type_id == "0786eb48d2110c076000f99ac5361ab0",
                None => false,
            };
            if !style_matches {
                push_finding(ctx, findings, Check::NativeLinks, format_args!("Inventor PmGraphics face style reference does not resolve to a style collection"), Some(ctx.copy_retained_text(&record.id, "retain Inventor PmGraphics face id")?))?;
            }
        }
    }
    let mut style_collections = data.pm_graphics_style_collections.iter();
    while let Some(record) = ctx.next_charged(
        &mut style_collections,
        "validate Inventor PmGraphics style collections",
    )? {
        let key = (record.segment_token(), record.record_ordinal());
        let record_matches = match ctx.get_hash_map(
            &raw_records,
            &key,
            "find Inventor PmGraphics style collection",
        )? {
            Some(type_id) => *type_id == "0786eb48d2110c076000f99ac5361ab0",
            None => false,
        };
        if !record_matches {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor PmGraphics style collection does not resolve to its RSe record"
                ),
                Some(ctx.copy_retained_text(
                    record.id(),
                    "retain Inventor PmGraphics style-collection id",
                )?),
            )?;
        }
        let mut style_references = record.style_references.references().iter();
        while let Some(reference) = ctx.next_charged(
            &mut style_references,
            "validate Inventor PmGraphics style references",
        )? {
            let resolves = reference.index() != 0
                && ctx.contains_key_hash_map(
                    &raw_records,
                    &(record.segment_token(), reference.index() - 1),
                    "resolve Inventor PmGraphics style reference",
                )?;
            if !resolves {
                push_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    format_args!(
                        "Inventor PmGraphics style-collection reference {} does not resolve",
                        reference.index()
                    ),
                    Some(ctx.copy_retained_text(
                        record.id(),
                        "retain Inventor PmGraphics style-collection id",
                    )?),
                )?;
            }
        }
    }
    let mut primary_color_styles = data.pm_graphics_primary_color_styles.iter();
    while let Some(record) = ctx.next_charged(
        &mut primary_color_styles,
        "validate Inventor primary-color styles",
    )? {
        let key = (record.segment_token.as_str(), record.record_ordinal);
        let record_matches = match ctx.get_hash_map(
            &raw_records,
            &key,
            "find Inventor primary-color style record",
        )? {
            Some(type_id) => *type_id == "0f5648afd411c78d1000d58dc04a0ab5",
            None => false,
        };
        if !record_matches {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor PmGraphics primary-color style does not resolve to its RSe record"
                ),
                Some(ctx.copy_retained_text(&record.id, "retain Inventor primary-color style id")?),
            )?;
        }
    }
    drop((raw_records, raw_records_storage));
    let mut presentation_issues = data.presentation_record_issues.iter();
    while let Some(issue) = ctx.next_charged(
        &mut presentation_issues,
        "validate Inventor presentation record issues",
    )? {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor presentation record: {}", issue.detail),
            Some(issue.id(ctx)?),
        )?;
    }
    Ok(())
}

fn validate_active_carrier(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let carrier = &data.active_carrier;
    if let ActiveCarrierRecord::Selected {
        id,
        segment_token,
        record_ordinal,
        ..
    } = carrier
    {
        let resolves = ctx.any_by(
            &data.records,
            |record| {
                if ctx.equal(
                    record.token.as_str(),
                    segment_token,
                    "compare Inventor active-carrier segment token",
                )? && record.ordinal == *record_ordinal
                {
                    Ok(record.type_id.as_str() == "5c5945f6d5113313100060a6bba647b5")
                } else {
                    Ok(false)
                }
            },
            "resolve Inventor active carrier",
        )?;
        if !resolves {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor active carrier does not resolve to its typed RSe record"),
                Some(ctx.copy_retained_text(id, "retain Inventor active-carrier id")?),
            )?;
        }
    }
    Ok(())
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

fn read_contextual_arena<W, T>(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
    name: &str,
    operation: &'static str,
    mut convert: impl FnMut(W, &DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<Vec<T>, cadmpeg_ir::native::NativeConvertError>
where
    W: DeserializeOwned,
{
    let records = ctx
        .get_btree_map(namespace.arenas(), name, operation)?
        .map_or(&[][..], Vec::as_slice);
    let records = ctx
        .admit_iter(records, operation)
        .map_err(CodecError::ResourceLimit)?;
    let wires = namespace.arena_iter_as_for_decode::<W>(ctx, name);
    ctx.try_collect_vec(
        records.zip(wires).map(|(record, wire)| {
            let wire = wire?;
            match convert(wire, ctx) {
                Ok(value) => Ok(value),
                Err(CodecError::Malformed(message)) => {
                    let source = cadmpeg_ir::native::NativeConvertError::ReadRecordMessage {
                        id: record.identity_for_decode(ctx, operation)?,
                        message,
                    };
                    let arena = ctx.copy_retained_text(name, "retain native arena error name")?;
                    Err(cadmpeg_ir::native::NativeConvertError::Arena {
                        arena,
                        source: Box::new(source),
                    })
                }
                Err(error) => Err(cadmpeg_ir::native::NativeConvertError::Resource(error)),
            }
        }),
        operation,
    )
}

fn read_contextual_located_arena<T>(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
    name: &str,
    operation: &'static str,
) -> Result<Vec<Located<T>>, cadmpeg_ir::native::NativeConvertError>
where
    T: DeserializeOwned + RecordPayload,
{
    read_contextual_arena::<LocatedWire<T>, _>(
        ctx,
        namespace,
        name,
        operation,
        LocatedWire::into_record,
    )
}

fn read_contextual_located_wire_arena<W, T>(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
    name: &str,
    operation: &'static str,
    mut convert: impl FnMut(W, &DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<Vec<Located<T>>, cadmpeg_ir::native::NativeConvertError>
where
    W: DeserializeOwned,
    T: RecordPayload,
{
    read_contextual_arena::<LocatedWire<W>, _>(ctx, namespace, name, operation, |wire, ctx| {
        let LocatedWire {
            id,
            type_id,
            segment_token,
            record_ordinal,
            value,
        } = wire;
        LocatedWire {
            id,
            type_id,
            segment_token,
            record_ordinal,
            value: convert(value, ctx)?,
        }
        .into_record(ctx)
    })
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
            protein_assets: read_contextual_arena::<ProteinAssetRecordWire, _>(
                ctx,
                namespace,
                "protein_assets",
                "convert Inventor Protein assets",
                ProteinAssetRecordWire::into_record,
            )?,
            protein_rejections: read_contextual_arena::<ProteinRejectionRecordWire, _>(
                ctx,
                namespace,
                "protein_rejections",
                "convert Inventor Protein rejections",
                ProteinRejectionRecordWire::into_record,
            )?,
            ufrx: UfrxRecord::read(ctx, namespace)?,
            assembly_occurrences: namespace.arena_as_for_decode(ctx, "assembly_occurrences")?,
            assembly_placements: read_contextual_arena::<AssemblyPlacementRecordWire, _>(
                ctx,
                namespace,
                "assembly_placements",
                "convert Inventor assembly placements",
                |wire, _ctx| wire.into_record().map_err(|failure| failure.error),
            )?,
            assembly_record_issues: read_contextual_arena::<crate::record_issue::RecordIssueWire, _>(
                ctx,
                namespace,
                "assembly_record_issues",
                "convert Inventor assembly record issues",
                crate::record_issue::RecordIssueWire::into_record,
            )?,
            pm_app_default_styles: namespace.arena_as_for_decode(ctx, "pm_app_default_styles")?,
            pm_app_rendering_styles: read_contextual_arena::<PmAppRenderingStyleRecordWire, _>(
                ctx,
                namespace,
                "pm_app_rendering_styles",
                "convert Inventor PmApp rendering styles",
                |wire, _ctx| wire.into_record(),
            )?,
            pm_graphics_faces: read_contextual_arena::<PmGraphicsFaceRecordWire, _>(
                ctx,
                namespace,
                "pm_graphics_faces",
                "convert Inventor PmGraphics faces",
                PmGraphicsFaceRecordWire::into_record,
            )?,
            pm_graphics_style_collections: read_contextual_arena::<
                PmGraphicsStyleCollectionRecordWire,
                _,
            >(
                ctx,
                namespace,
                "pm_graphics_style_collections",
                "convert Inventor PmGraphics style collections",
                PmGraphicsStyleCollectionRecordWire::into_record,
            )?,
            pm_graphics_primary_color_styles: namespace
                .arena_as_for_decode(ctx, "pm_graphics_primary_color_styles")?,
            face_native_keys: namespace.arena_as_for_decode(ctx, "face_native_keys")?,
            presentation_record_issues: read_contextual_arena::<
                crate::record_issue::RecordIssueWire,
                _,
            >(
                ctx,
                namespace,
                "presentation_record_issues",
                "convert Inventor presentation record issues",
                crate::record_issue::RecordIssueWire::into_record,
            )?,
            pm_dc_parameters: read_contextual_located_arena::<crate::design::PmDcParameterPayload>(
                ctx,
                namespace,
                "pm_dc_parameters",
                "convert Inventor PmDc parameters",
            )?,
            pm_dc_expressions: read_contextual_located_arena::<crate::design::PmDcExpressionPayload>(
                ctx,
                namespace,
                "pm_dc_expressions",
                "convert Inventor PmDc expressions",
            )?,
            pm_dc_units: read_contextual_located_arena::<crate::design::PmDcUnitPayload>(
                ctx,
                namespace,
                "pm_dc_units",
                "convert Inventor PmDc units",
            )?,
            design_record_issues: read_contextual_arena::<crate::record_issue::RecordIssueWire, _>(
                ctx,
                namespace,
                "design_record_issues",
                "convert Inventor design record issues",
                crate::record_issue::RecordIssueWire::into_record,
            )?,
            pm_dc_sketches: read_contextual_located_arena::<crate::sketch::PmDcSketchPayload>(
                ctx,
                namespace,
                "pm_dc_sketches",
                "convert Inventor PmDc sketches",
            )?,
            pm_dc_sketch_entities: read_contextual_located_arena::<
                crate::sketch::PmDcSketchEntityPayload,
            >(
                ctx,
                namespace,
                "pm_dc_sketch_entities",
                "convert Inventor PmDc sketch entities",
            )?,
            pm_dc_sketch_constraints: read_contextual_located_arena::<
                crate::sketch::PmDcSketchConstraintPayload,
            >(
                ctx,
                namespace,
                "pm_dc_sketch_constraints",
                "convert Inventor PmDc sketch constraints",
            )?,
            pm_dc_transforms: read_contextual_located_wire_arena::<
                crate::sketch::PmDcTransformPayloadWire,
                crate::sketch::PmDcTransformPayload,
            >(
                ctx,
                namespace,
                "pm_dc_transforms",
                "convert Inventor PmDc transforms",
                |wire, _ctx| wire.into_payload(),
            )?,
            pm_dc_directions: read_contextual_located_arena::<crate::sketch::PmDcDirectionPayload>(
                ctx,
                namespace,
                "pm_dc_directions",
                "convert Inventor PmDc directions",
            )?,
            sketch_record_issues: read_contextual_arena::<crate::record_issue::RecordIssueWire, _>(
                ctx,
                namespace,
                "sketch_record_issues",
                "convert Inventor sketch record issues",
                crate::record_issue::RecordIssueWire::into_record,
            )?,
            pm_dc_features: read_contextual_located_arena::<crate::feature::PmDcFeaturePayload>(
                ctx,
                namespace,
                "pm_dc_features",
                "convert Inventor PmDc features",
            )?,
            pm_dc_pattern_features: read_contextual_located_arena::<
                crate::feature::PmDcPatternFeaturePayload,
            >(
                ctx,
                namespace,
                "pm_dc_pattern_features",
                "convert Inventor PmDc pattern features",
            )?,
            pm_dc_feature_terminators: read_contextual_located_arena::<
                crate::feature::PmDcFeatureTerminatorPayload,
            >(
                ctx,
                namespace,
                "pm_dc_feature_terminators",
                "convert Inventor PmDc feature terminators",
            )?,
            pm_dc_feature_properties: read_contextual_located_arena::<
                crate::feature::PmDcFeaturePropertyPayload,
            >(
                ctx,
                namespace,
                "pm_dc_feature_properties",
                "convert Inventor PmDc feature properties",
            )?,
            pm_dc_feature_labels: read_contextual_located_wire_arena::<
                PmDcFeatureLabelPayloadWire,
                crate::feature::PmDcFeatureLabelPayload,
            >(
                ctx,
                namespace,
                "pm_dc_feature_labels",
                "convert Inventor PmDc feature labels",
                super::feature::PmDcFeatureLabelPayloadWire::into_record,
            )?,
            pm_dc_entity_style_links: read_contextual_located_arena::<
                crate::feature::PmDcEntityStyleLinkPayload,
            >(
                ctx,
                namespace,
                "pm_dc_entity_style_links",
                "convert Inventor PmDc entity-style links",
            )?,
            feature_record_issues: read_contextual_arena::<crate::record_issue::RecordIssueWire, _>(
                ctx,
                namespace,
                "feature_record_issues",
                "convert Inventor feature record issues",
                crate::record_issue::RecordIssueWire::into_record,
            )?,
            active_carrier: ActiveCarrierRecord::read(ctx, namespace)?,
            unknowns: namespace.arena_as_for_decode(ctx, "unknowns")?,
        })
    }
}

fn validate_databases(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    unique(
        ctx,
        findings,
        &data.storage_bands,
        |record| Ok(record.band),
        "storage band",
    )?;
    unique(
        ctx,
        findings,
        &data.storage_bands,
        |record| Ok(record.database_directory_id),
        "database directory id",
    )?;
    let (storage, storage_storage) =
        ctx.with_scoped_storage("collect Inventor storage bands", || {
            ctx.collect_btree_set(
                data.storage_bands.iter().map(|record| record.band),
                "collect Inventor storage bands",
            )
        })?;
    let mut states_storage = ctx.reserve_scoped(0, "index Inventor database state bands")?;
    let mut states = BTreeSet::new();
    {
        let mut index_band = |band| -> Result<(), CodecError> {
            let inserted = states_storage.with_storage(|| {
                ctx.insert_btree_set(&mut states, band, "index Inventor database state bands")
            })?;
            if !inserted {
                push_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    format_args!("Inventor native data repeats a database state band"),
                    None,
                )?;
            }
            Ok(())
        };
        let mut database_steps = data.databases.iter();
        while let Some(record) =
            ctx.next_charged(&mut database_steps, "index Inventor database bands")?
        {
            index_band(record.band)?;
        }
        let mut issue_steps = data.database_issues.iter();
        while let Some(issue) =
            ctx.next_charged(&mut issue_steps, "index Inventor database issue bands")?
        {
            index_band(issue.band)?;
        }
    }
    let states_match = equal_btree_sets(
        ctx,
        &storage,
        &states,
        "compare Inventor database state bands",
    )?;
    drop((storage, storage_storage, states, states_storage));
    if !states_match {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor database states do not cover the storage bands exactly"),
            None,
        )?;
    }
    let mut database_issues = data.database_issues.iter();
    while let Some(issue) =
        ctx.next_charged(&mut database_issues, "validate Inventor database issues")?
    {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!(
                "Inventor database band {} is unavailable: {}",
                issue.band, issue.detail
            ),
            Some(ctx.copy_retained_text(&issue.id, "retain Inventor database issue id")?),
        )?;
    }
    Ok(())
}

fn validate_segments(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    const EXPECTED_SECTIONS: [u8; 11] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

    unique(
        ctx,
        findings,
        &data.registry,
        |record| Ok(record.ordinal),
        "registry ordinal",
    )?;
    unique(
        ctx,
        findings,
        &data.registry,
        |record| Ok(record.segment_id.as_str()),
        "registry segment id",
    )?;
    unique(
        ctx,
        findings,
        &data.revisions,
        |record| Ok(record.ordinal),
        "revision ordinal",
    )?;
    unique(
        ctx,
        findings,
        &data.pairs,
        |record| Ok(record.token.as_str()),
        "segment token",
    )?;
    unique(
        ctx,
        findings,
        &data.pairs,
        |record| Ok(record.metadata_directory_id),
        "metadata directory id",
    )?;
    unique(
        ctx,
        findings,
        &data.pairs,
        |record| Ok(record.bulk_directory_id),
        "bulk directory id",
    )?;
    let (registry_ids, registry_ids_storage) =
        ctx.with_scoped_storage("collect Inventor segment registry ids", || {
            ctx.collect_hash_set(
                data.registry
                    .iter()
                    .map(|record| record.segment_id.as_str()),
                "collect Inventor segment registry ids",
            )
        })?;
    let mut metadata_records = data.metadata.iter();
    while let Some(meta) =
        ctx.next_charged(&mut metadata_records, "validate Inventor segment metadata")?
    {
        if !ctx.contains_hash_set(
            &registry_ids,
            meta.segment_id.as_str(),
            "resolve Inventor segment registry identity",
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor segment metadata {} has no registry identity",
                    meta.id
                ),
                Some(ctx.copy_retained_text(&meta.id, "retain Inventor segment metadata id")?),
            )?;
        }
    }
    drop((registry_ids, registry_ids_storage));
    let (pair_tokens, _pair_tokens_storage) =
        ctx.with_scoped_storage("collect Inventor paired segment tokens", || {
            ctx.collect_btree_set(
                data.pairs.iter().map(|record| record.token.as_str()),
                "collect Inventor paired segment tokens",
            )
        })?;
    validate_segment_states(
        ctx,
        findings,
        &pair_tokens,
        (data.metadata.as_slice(), |record| record.token.as_str()),
        (data.metadata_issues.as_slice(), |record| {
            record.token.as_str()
        }),
        "metadata",
    )?;
    validate_segment_states(
        ctx,
        findings,
        &pair_tokens,
        (data.bulk.as_slice(), |record| record.token.as_str()),
        (data.bulk_issues.as_slice(), |record| record.token.as_str()),
        "bulk",
    )?;
    let (metadata_by_token, metadata_by_token_storage) =
        ctx.with_scoped_storage("collect Inventor segment metadata index", || {
            let mut metadata_by_token = BTreeMap::new();
            let mut metadata_steps = data.metadata.iter();
            while let Some(record) = ctx.next_charged(
                &mut metadata_steps,
                "index Inventor segment metadata by token",
            )? {
                ctx.insert_btree_map(
                    &mut metadata_by_token,
                    record.token.as_str(),
                    record,
                    "collect Inventor segment metadata index",
                )?;
            }
            Ok::<_, CodecError>(metadata_by_token)
        })?;
    unique(
        ctx,
        findings,
        &data.meta_sections,
        |record| Ok((record.token.as_str(), record.number)),
        "metadata section number",
    )?;
    unique(
        ctx,
        findings,
        &data.meta_types,
        |record| Ok((record.token.as_str(), record.index)),
        "metadata type index",
    )?;
    let mut sections_by_token = HashMap::<&str, BTreeSet<u8>>::new();
    let mut sections_by_token_storage =
        ctx.reserve_scoped(0, "index Inventor metadata sections")?;
    let mut metadata_sections = data.meta_sections.iter();
    while let Some(record) =
        ctx.next_charged(&mut metadata_sections, "index Inventor metadata sections")?
    {
        let token = record.token.as_str();
        sections_by_token_storage.with_storage(|| {
            if let Some(sections) = ctx.get_mut_hash_map(
                &mut sections_by_token,
                &token,
                "find Inventor metadata section group",
            )? {
                ctx.insert_btree_set(
                    sections,
                    u8::from(record.number),
                    "index Inventor metadata sections",
                )?;
            } else {
                let mut sections = BTreeSet::new();
                ctx.insert_btree_set(
                    &mut sections,
                    u8::from(record.number),
                    "index Inventor metadata sections",
                )?;
                ctx.insert_hash_map(
                    &mut sections_by_token,
                    token,
                    sections,
                    "create Inventor metadata section group",
                )?;
            }
            Ok::<(), CodecError>(())
        })?;
    }
    let mut types_by_token = HashMap::<&str, HashSet<u8>>::new();
    let mut types_by_token_storage = ctx.reserve_scoped(0, "index Inventor metadata types")?;
    let mut metadata_types = data.meta_types.iter();
    while let Some(record) =
        ctx.next_charged(&mut metadata_types, "index Inventor metadata types")?
    {
        let token = record.token.as_str();
        types_by_token_storage.with_storage(|| {
            if let Some(types) = ctx.get_mut_hash_map(
                &mut types_by_token,
                &token,
                "find Inventor metadata type group",
            )? {
                ctx.insert_hash_set(types, record.index, "index Inventor metadata types")?;
            } else {
                let mut types = HashSet::new();
                ctx.insert_hash_set(&mut types, record.index, "index Inventor metadata types")?;
                ctx.insert_hash_map(
                    &mut types_by_token,
                    token,
                    types,
                    "create Inventor metadata type group",
                )?;
            }
            Ok::<(), CodecError>(())
        })?;
    }
    for (token, meta) in ctx.admit_iter(&metadata_by_token, "validate Inventor metadata indexes")? {
        let actual_sections =
            ctx.get_hash_map(&sections_by_token, token, "find Inventor metadata sections")?;
        let actual_types =
            ctx.get_hash_map(&types_by_token, token, "find Inventor metadata types")?;
        let sections_match = actual_sections.is_some_and(|actual| {
            actual.len() == EXPECTED_SECTIONS.len()
                && EXPECTED_SECTIONS
                    .iter()
                    .all(|number| actual.contains(number))
        });
        let type_count = cadmpeg_core::decode::u64_from_index(actual_types.map_or(0, HashSet::len));
        if !sections_match || type_count != meta.type_count {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor metadata tables do not match their segment summary"),
                Some(ctx.copy_retained_text(&meta.id, "retain Inventor metadata id")?),
            )?;
        }
    }
    drop((
        metadata_by_token,
        metadata_by_token_storage,
        sections_by_token,
        sections_by_token_storage,
        types_by_token,
        types_by_token_storage,
    ));
    let (type_keys, type_keys_storage) =
        ctx.with_scoped_storage("collect Inventor RSe type keys", || {
            ctx.collect_hash_set(
                data.meta_types
                    .iter()
                    .map(|record| (record.token.as_str(), record.index, record.type_id.as_str())),
                "collect Inventor RSe type keys",
            )
        })?;
    let mut record_counts = HashMap::<&str, u64>::new();
    let mut record_counts_storage = ctx.reserve_scoped(0, "count Inventor RSe records")?;
    let mut rse_records = data.records.iter();
    while let Some(record) =
        ctx.next_charged(&mut rse_records, "validate Inventor RSe record metadata")?
    {
        record_counts_storage.with_storage(|| {
            increment_hash_count(
                ctx,
                &mut record_counts,
                record.token.as_str(),
                "count Inventor RSe records",
            )
        })?;
        if !ctx.contains_hash_set(
            &type_keys,
            &(
                record.token.as_str(),
                record.type_index(),
                record.type_id.as_str(),
            ),
            "resolve Inventor RSe record type",
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor RSe record type does not resolve in its metadata table"),
                Some(ctx.copy_retained_text(&record.id, "retain Inventor RSe record id")?),
            )?;
        }
    }
    drop((type_keys, type_keys_storage));
    unique(
        ctx,
        findings,
        &data.records,
        |record| Ok((record.token.as_str(), record.ordinal)),
        "segment record ordinal",
    )?;
    let mut bulk_records = data.bulk.iter();
    while let Some(bulk) = ctx.next_charged(
        &mut bulk_records,
        "validate Inventor bulk segment summaries",
    )? {
        match &bulk.records {
            crate::native::SegmentBulkFrame::Framed { record_count, .. } => {
                let count = ctx
                    .get_hash_map(
                        &record_counts,
                        bulk.token.as_str(),
                        "find Inventor RSe record count",
                    )?
                    .copied()
                    .unwrap_or(0);
                if *record_count != count {
                    push_finding(
                        ctx,
                        findings,
                        Check::NativeLinks,
                        format_args!(
                            "Inventor bulk record summary does not match its record arena"
                        ),
                        Some(ctx.copy_retained_text(&bulk.id, "retain Inventor bulk id")?),
                    )?;
                }
            }
            crate::native::SegmentBulkFrame::Unavailable { detail } => {
                push_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    format_args!("Inventor bulk records are unavailable: {detail}"),
                    Some(ctx.copy_retained_text(&bulk.id, "retain Inventor bulk id")?),
                )?;
            }
        }
    }
    drop((record_counts, record_counts_storage));
    let (expanded_lengths, expanded_lengths_storage) =
        ctx.with_scoped_storage("collect Inventor expanded bulk lengths", || {
            ctx.collect_hash_map(
                data.bulk
                    .iter()
                    .map(|bulk| (bulk.token.as_str(), bulk.expanded_len)),
                "collect Inventor expanded bulk lengths",
            )
        })?;
    let mut payload_ranges = data.records.iter();
    while let Some(record) =
        ctx.next_charged(&mut payload_ranges, "validate Inventor RSe payload ranges")?
    {
        let end = record.payload_offset.checked_add(record.payload_len());
        let expanded_len = ctx
            .get_hash_map(
                &expanded_lengths,
                record.token.as_str(),
                "find Inventor expanded bulk length",
            )?
            .copied()
            .unwrap_or(0);
        if end.is_none_or(|end| end > expanded_len) {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor RSe record payload range exceeds its bulk stream"),
                Some(ctx.copy_retained_text(&record.id, "retain Inventor RSe record id")?),
            )?;
        }
    }
    drop((expanded_lengths, expanded_lengths_storage));
    let mut unpaired_records = data.unpaired.iter();
    while let Some(record) =
        ctx.next_charged(&mut unpaired_records, "validate Inventor unpaired segments")?
    {
        if ctx.contains_btree_set(
            &pair_tokens,
            record.token.as_str(),
            "check Inventor paired segment",
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor segment is both paired and unpaired"),
                Some(ctx.copy_retained_text(&record.id, "retain Inventor unpaired segment id")?),
            )?;
        }
    }
    Ok(())
}

fn validate_segment_states<P, I, F, G>(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    pairs: &BTreeSet<&str>,
    parsed: (&[P], F),
    issues: (&[I], G),
    member: &str,
) -> Result<(), CodecError>
where
    F: Fn(&P) -> &str,
    G: Fn(&I) -> &str,
{
    let (parsed, parsed_token) = parsed;
    let (issues, issue_token) = issues;
    let mut states_storage = ctx.reserve_scoped(0, "index Inventor uniqueness keys")?;
    let mut states = BTreeSet::new();
    {
        let mut index_token = |token| -> Result<(), CodecError> {
            let inserted = states_storage.with_storage(|| {
                ctx.insert_btree_set(&mut states, token, "index Inventor uniqueness keys")
            })?;
            if !inserted {
                push_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    format_args!("Inventor native data repeats a segment {member} state"),
                    None,
                )?;
            }
            Ok(())
        };
        let mut parsed_steps = parsed.iter();
        while let Some(record) =
            ctx.next_charged(&mut parsed_steps, "validate Inventor uniqueness source")?
        {
            index_token(parsed_token(record))?;
        }
        let mut issue_steps = issues.iter();
        while let Some(issue) =
            ctx.next_charged(&mut issue_steps, "validate Inventor uniqueness source")?
        {
            index_token(issue_token(issue))?;
        }
    }
    if !equal_btree_sets(ctx, &states, pairs, "compare Inventor segment states")? {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor segment {member} states do not cover paired segments exactly"),
            None,
        )?;
    }
    Ok(())
}

fn validate_properties(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    unique(
        ctx,
        findings,
        &data.property_sets,
        |record| Ok(record.path.as_str()),
        "property-set path",
    )?;
    let mut sections_by_set = HashMap::<&str, u64>::new();
    let mut sections_by_set_storage = ctx.reserve_scoped(0, "count Inventor property sections")?;
    let mut properties_by_section = HashMap::<(&str, u32), u64>::new();
    let mut properties_by_section_storage =
        ctx.reserve_scoped(0, "count Inventor properties by section")?;
    let mut property_sections = data.property_sections.iter();
    while let Some(section) =
        ctx.next_charged(&mut property_sections, "count Inventor property sections")?
    {
        sections_by_set_storage.with_storage(|| {
            increment_hash_count(
                ctx,
                &mut sections_by_set,
                section.set_path.as_str(),
                "count Inventor property sections",
            )
        })?;
    }
    let mut properties = data.properties.iter();
    while let Some(property) =
        ctx.next_charged(&mut properties, "count Inventor properties by section")?
    {
        properties_by_section_storage.with_storage(|| {
            increment_hash_count(
                ctx,
                &mut properties_by_section,
                (property.set_path.as_str(), property.section_ordinal),
                "count Inventor properties by section",
            )
        })?;
    }
    let mut property_sets = data.property_sets.iter();
    while let Some(set) = ctx.next_charged(&mut property_sets, "validate Inventor property sets")? {
        let section_count = ctx
            .get_hash_map(
                &sections_by_set,
                set.path.as_str(),
                "find Inventor property-set section count",
            )?
            .copied()
            .unwrap_or(0);
        if section_count != set.section_count {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor property-set section count does not match its section arena"
                ),
                Some(ctx.copy_retained_text(&set.id, "retain Inventor property-set id")?),
            )?;
        }
    }
    drop((sections_by_set, sections_by_set_storage));
    let (set_paths, _set_paths_storage) =
        ctx.with_scoped_storage("collect Inventor property-set paths", || {
            ctx.collect_hash_set(
                data.property_sets.iter().map(|record| record.path.as_str()),
                "collect Inventor property-set paths",
            )
        })?;
    let mut section_references = data.property_sections.iter();
    while let Some(section) = ctx.next_charged(
        &mut section_references,
        "validate Inventor property sections",
    )? {
        let set_exists = ctx.contains_hash_set(
            &set_paths,
            section.set_path.as_str(),
            "resolve Inventor property-set path",
        )?;
        let property_count = ctx
            .get_hash_map(
                &properties_by_section,
                &(section.set_path.as_str(), section.ordinal),
                "find Inventor property count",
            )?
            .copied()
            .unwrap_or(0);
        if !set_exists || property_count != section.property_count {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor property section does not match its set or property arena"),
                Some(ctx.copy_retained_text(&section.id, "retain Inventor property-section id")?),
            )?;
        }
    }
    Ok(())
}

fn validate_protein(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    unique(
        ctx,
        findings,
        data.protein.entries(),
        |record| Ok(record.ordinal),
        "Protein entry ordinal",
    )?;
    let record = &data.protein;
    if let ProteinRecord::Malformed { id, detail, .. } = record {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor Protein stream is malformed: {detail}"),
            Some(ctx.copy_retained_text(id, "retain Inventor Protein id")?),
        )?;
    }
    Ok(())
}

fn validate_protein_assets(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    unique(
        ctx,
        findings,
        &data.protein_assets,
        |record| Ok((record.entry_name.as_str(), record.ordinal())),
        "Protein decoded-record position",
    )?;
    let (entry_names, _entry_names_storage) =
        ctx.with_scoped_storage("collect Inventor Protein entry names", || {
            ctx.collect_hash_set(
                data.protein
                    .entries()
                    .iter()
                    .map(|entry| entry.name.as_str()),
                "collect Inventor Protein entry names",
            )
        })?;
    let mut protein_assets = data.protein_assets.iter();
    while let Some(asset) =
        ctx.next_charged(&mut protein_assets, "validate Inventor Protein assets")?
    {
        if !ctx.contains_hash_set(
            &entry_names,
            asset.entry_name.as_str(),
            "resolve Inventor Protein asset entry",
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor Protein asset position is inconsistent or does not resolve to a package entry"),
                Some(ctx.copy_retained_text(&asset.id, "retain Inventor Protein asset id")?),
            )?;
        }
    }
    Ok(())
}

fn validate_protein_rejections(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    unique(
        ctx,
        findings,
        &data.protein_rejections,
        |record| Ok(record.id.as_str()),
        "Protein rejection id",
    )?;
    unique(
        ctx,
        findings,
        &data.protein_rejections,
        |record| Ok((record.entry_name.as_str(), record.ordinal)),
        "Protein rejected-record position",
    )?;
    let (entry_names, _entry_names_storage) =
        ctx.with_scoped_storage("collect Inventor Protein entry names", || {
            ctx.collect_hash_set(
                data.protein
                    .entries()
                    .iter()
                    .map(|entry| entry.name.as_str()),
                "collect Inventor Protein entry names",
            )
        })?;
    let (accepted_positions, _accepted_positions_storage) =
        ctx.with_scoped_storage("collect accepted Inventor Protein positions", || {
            ctx.collect_hash_set(
                data.protein_assets
                    .iter()
                    .map(|record| (record.entry_name.as_str(), record.ordinal())),
                "collect accepted Inventor Protein positions",
            )
        })?;
    let mut protein_rejections = data.protein_rejections.iter();
    while let Some(rejection) = ctx.next_charged(
        &mut protein_rejections,
        "validate Inventor Protein rejections",
    )? {
        let entry_missing = !ctx.contains_hash_set(
            &entry_names,
            rejection.entry_name.as_str(),
            "resolve Inventor Protein rejection entry",
        )?;
        if entry_missing
            || ctx.contains_hash_set(
                &accepted_positions,
                &(rejection.entry_name.as_str(), rejection.ordinal),
                "check Inventor Protein rejection overlap",
            )?
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor Protein rejection position overlaps an asset or does not resolve to a package entry and detail"),
                Some(ctx.copy_retained_text(&rejection.id, "retain Inventor Protein rejection id")?),
            )?;
        }
    }
    Ok(())
}

fn validate_protein_record_coverage(
    ctx: &DecodeContext<'_>,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut positions = BTreeMap::<&str, (BTreeSet<u64>, u64)>::new();
    let mut positions_storage = ctx.reserve_scoped(0, "index Inventor Protein position groups")?;
    let mut asset_positions = data.protein_assets.iter();
    while let Some(asset) = ctx.next_charged(
        &mut asset_positions,
        "group Inventor Protein asset positions",
    )? {
        positions_storage.with_storage(|| {
            match ctx.get_mut_btree_map(
                &mut positions,
                &asset.entry_name.as_str(),
                "find Inventor Protein position group",
            )? {
                Some((ordinals, maximum)) => {
                    ctx.insert_btree_set(
                        ordinals,
                        asset.ordinal(),
                        "collect Inventor Protein positions",
                    )?;
                    *maximum = (*maximum).max(asset.ordinal());
                }
                None => {
                    let mut ordinals = BTreeSet::new();
                    ctx.insert_btree_set(
                        &mut ordinals,
                        asset.ordinal(),
                        "collect Inventor Protein positions",
                    )?;
                    ctx.insert_btree_map(
                        &mut positions,
                        asset.entry_name.as_str(),
                        (ordinals, asset.ordinal()),
                        "index Inventor Protein position groups",
                    )?;
                }
            }
            Ok::<(), CodecError>(())
        })?;
    }
    let mut rejection_positions = data.protein_rejections.iter();
    while let Some(rejection) = ctx.next_charged(
        &mut rejection_positions,
        "group Inventor Protein rejection positions",
    )? {
        positions_storage.with_storage(|| {
            match ctx.get_mut_btree_map(
                &mut positions,
                &rejection.entry_name.as_str(),
                "find Inventor Protein position group",
            )? {
                Some((ordinals, maximum)) => {
                    ctx.insert_btree_set(
                        ordinals,
                        rejection.ordinal,
                        "collect Inventor Protein positions",
                    )?;
                    *maximum = (*maximum).max(rejection.ordinal);
                }
                None => {
                    let mut ordinals = BTreeSet::new();
                    ctx.insert_btree_set(
                        &mut ordinals,
                        rejection.ordinal,
                        "collect Inventor Protein positions",
                    )?;
                    ctx.insert_btree_map(
                        &mut positions,
                        rejection.entry_name.as_str(),
                        (ordinals, rejection.ordinal),
                        "index Inventor Protein position groups",
                    )?;
                }
            }
            Ok::<(), CodecError>(())
        })?;
    }
    for (entry_name, (ordinals, maximum)) in
        ctx.admit_iter(&positions, "validate Inventor Protein position groups")?
    {
        let contiguous = maximum
            .checked_add(1)
            .and_then(|count| usize::try_from(count).ok())
            == Some(ordinals.len());
        if !contiguous {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor Protein logical-record positions are not contiguous for {entry_name:?}"
                ),
                None,
            )?;
        }
    }
    Ok(())
}

fn validate_ufrx(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    unique(
        ctx,
        findings,
        data.ufrx.external_references(),
        |record| Ok(ExternalReferenceRecord::ordinal(record)),
        format_args!("external reference ordinal"),
    )?;
    let model_states = data.ufrx.model_states();
    let (mut model_state_ordinals, mut model_state_ordinals_storage) =
        ctx.temporary_set(0, "index Inventor uniqueness keys")?;
    unique_into(
        ctx,
        &mut model_state_ordinals_storage,
        findings,
        &mut model_state_ordinals,
        model_states,
        |record| Ok(record.ordinal),
        format_args!("UFRxDoc model-state ordinal"),
    )?;
    unique(
        ctx,
        findings,
        data.ufrx.external_references(),
        |record| Ok(record.reference_id),
        format_args!("external reference id"),
    )?;
    let record = &data.ufrx;
    if let UfrxRecord::Malformed { id, detail, .. } = record {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor UFRxDoc stream is malformed: {detail}"),
            Some(ctx.copy_retained_text(id, "retain Inventor UFRxDoc issue id")?),
        )?;
    }
    let model_states_contiguous = model_state_ordinals.len() == model_states.len()
        && if let Ok(count) = u32::try_from(model_states.len()) {
            ctx.all_by(
                model_states,
                |state| Ok(state.ordinal < count),
                "validate Inventor UFRxDoc model-state ordinals",
            )?
        } else {
            false
        };
    drop((model_state_ordinals, model_state_ordinals_storage));
    if !model_states_contiguous {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor UFRxDoc model-state ordinals are not contiguous"),
            None,
        )?;
    }
    if let UfrxRecord::ParsedPrefix(payload) = record {
        if let Some(representation) = payload.representation.as_ref() {
            let representation_pair_present = representation.active_representation.is_some();
            let expected_pair = match document_kind(ctx, ir)? {
                Some("assembly") => Some(true),
                Some("part") => Some(false),
                _ => None,
            };
            if expected_pair.is_some_and(|expected| representation_pair_present != expected) {
                push_finding(
                    ctx,
                    findings,
                    Check::NativeLinks,
                    format_args!("Inventor UFRxDoc representation state is inconsistent"),
                    Some(ctx.copy_retained_text(
                        &payload.id,
                        "retain Inventor UFRxDoc representation id",
                    )?),
                )?;
            }
        }
    }
    unique(
        ctx,
        findings,
        data.ufrx.embedded_references(),
        |record| Ok(record.ordinal),
        format_args!("embedded reference ordinal"),
    )?;
    unique(
        ctx,
        findings,
        data.ufrx.occurrences(),
        |occurrence| Ok(occurrence.occurrence_id),
        format_args!("UFRxDoc occurrence id"),
    )?;
    let (reference_ids, reference_ids_storage) =
        ctx.with_scoped_storage("collect Inventor UFRxDoc external reference ids", || {
            ctx.collect_hash_set(
                data.ufrx
                    .external_references()
                    .iter()
                    .map(|reference| reference.reference_id),
                "collect Inventor UFRxDoc external reference ids",
            )
        })?;
    let (assembly_ids, assembly_ids_storage) = ctx.with_scoped_storage(
        "collect Inventor assembly occurrence ids for UFRxDoc",
        || {
            ctx.collect_hash_set(
                data.assembly_occurrences
                    .iter()
                    .map(|occurrence| occurrence.occurrence_id),
                "collect Inventor assembly occurrence ids for UFRxDoc",
            )
        },
    )?;
    let mut actual_counts = HashMap::<u32, u64>::new();
    let mut actual_counts_storage =
        ctx.reserve_scoped(0, "count Inventor UFRxDoc file references")?;
    let assembly_document = is_assembly_document(ctx, ir)?;
    let mut occurrences = data.ufrx.occurrences().iter();
    while let Some(occurrence) =
        ctx.next_charged(&mut occurrences, "validate Inventor UFRxDoc occurrences")?
    {
        actual_counts_storage.with_storage(|| {
            increment_hash_count(
                ctx,
                &mut actual_counts,
                occurrence.file_reference_id,
                "count Inventor UFRxDoc file references",
            )
        })?;
        if !ctx.contains_hash_set(
            &reference_ids,
            &occurrence.file_reference_id,
            "resolve Inventor UFRxDoc external reference",
        )? || (assembly_document
            && !ctx.contains_hash_set(
                &assembly_ids,
                &occurrence.occurrence_id,
                "resolve Inventor UFRxDoc assembly occurrence",
            )?)
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor UFRxDoc occurrence does not resolve to its file and assembly records"
                ),
                Some(
                    ctx.copy_retained_text(
                        &occurrence.id,
                        "retain Inventor UFRxDoc occurrence id",
                    )?,
                ),
            )?;
        }
    }
    drop((
        reference_ids,
        reference_ids_storage,
        assembly_ids,
        assembly_ids_storage,
    ));
    let mut external_references = data.ufrx.external_references().iter();
    while let Some(reference) = ctx.next_charged(
        &mut external_references,
        "validate Inventor UFRxDoc external reference counts",
    )? {
        if ctx
            .get_hash_map(
                &actual_counts,
                &reference.reference_id,
                "read Inventor UFRxDoc file reference count",
            )?
            .copied()
            .unwrap_or_default()
            != u64::from(reference.occurrence_count)
        {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor external-reference occurrence count does not match its typed records"
                ),
                Some(ctx.copy_retained_text(
                    reference.id(),
                    "retain Inventor UFRxDoc external reference id",
                )?),
            )?;
        }
    }
    Ok(())
}

fn validate_assembly(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    data: &NativeData,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    unique(
        ctx,
        findings,
        &data.assembly_occurrences,
        |record| Ok(record.occurrence_id),
        format_args!("assembly occurrence id"),
    )?;
    unique(
        ctx,
        findings,
        &data.assembly_placements,
        |record| Ok(record.occurrence_id),
        format_args!("assembly placement occurrence id"),
    )?;
    let (occurrence_ids, occurrence_ids_storage) =
        ctx.with_scoped_storage("collect Inventor assembly occurrence ids", || {
            ctx.collect_hash_set(
                data.assembly_occurrences
                    .iter()
                    .map(|record| record.occurrence_id),
                "collect Inventor assembly occurrence ids",
            )
        })?;
    let mut placements = data.assembly_placements.iter();
    while let Some(placement) =
        ctx.next_charged(&mut placements, "validate Inventor assembly placements")?
    {
        if !ctx.contains_hash_set(
            &occurrence_ids,
            &placement.occurrence_id,
            "resolve Inventor assembly placement occurrence",
        )? {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor assembly placement does not resolve to a finite occurrence"),
                Some(
                    ctx.copy_retained_text(&placement.id, "retain Inventor assembly placement id")?,
                ),
            )?;
        }
    }
    drop((occurrence_ids, occurrence_ids_storage));
    if is_assembly_document(ctx, ir)? && !data.ufrx.external_references().is_empty() {
        let mut declared = 0_u64;
        let mut reference_counts = data.ufrx.external_references().iter();
        while let Some(reference) = ctx.next_charged(
            &mut reference_counts,
            "sum Inventor external reference occurrence counts",
        )? {
            declared = declared
                .checked_add(u64::from(reference.occurrence_count))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "sum Inventor external reference occurrence counts",
                        u64::MAX,
                        u64::MAX,
                    )
                })?;
        }
        if declared != cadmpeg_core::decode::u64_from_index(data.assembly_occurrences.len()) {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!(
                    "Inventor external references declare {declared} occurrences, but the typed assembly table contains {}",
                    data.assembly_occurrences.len()
                ),
                None,
            )?;
        }
    }
    let mut assembly_issues = data.assembly_record_issues.iter();
    while let Some(issue) = ctx.next_charged(
        &mut assembly_issues,
        "validate Inventor assembly record issues",
    )? {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!(
                "Inventor assembly record {}:{} is unavailable: {}",
                issue.segment_token, issue.record_ordinal, issue.detail
            ),
            Some(issue.id(ctx)?),
        )?;
    }
    let (mut projected, _projected_storage) =
        ctx.with_scoped_storage("project Inventor occurrences for validation", || {
            crate::assembly::project_occurrences(
                ctx,
                data.ufrx.occurrences(),
                data.ufrx.external_references(),
                &data.assembly_occurrences,
                &data.assembly_placements,
            )
        })?;
    ctx.stable_sort_by(
        &mut projected.occurrences,
        |value| value.id.as_str(),
        Ord::cmp,
        "Inventor projected occurrence sort",
    )?;
    if !ctx.equal(
        &ir.model.occurrences,
        &projected.occurrences,
        "compare Inventor neutral and typed occurrences",
    )? {
        push_finding(
            ctx,
            findings,
            Check::NativeLinks,
            format_args!("Inventor neutral occurrences do not match the typed assembly records"),
            None,
        )?;
    }
    Ok(())
}

fn is_assembly_document(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<bool, CodecError> {
    Ok(document_kind(ctx, ir)? == Some("assembly"))
}

fn document_kind<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
) -> Result<Option<&'ir str>, CodecError> {
    let Some(source) = ir.source.as_ref() else {
        return Ok(None);
    };
    Ok(ctx
        .get_btree_map(
            &source.attributes,
            "document_kind",
            "read Inventor source document kind",
        )?
        .map(String::as_str))
}

fn unique<'values, T, K, F, L>(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    values: &'values [T],
    key: F,
    field: L,
) -> Result<(), CodecError>
where
    K: 'values + Eq + std::hash::Hash + DecodeCost,
    F: FnMut(&'values T) -> Result<K, CodecError>,
    L: fmt::Display,
{
    let (mut seen, mut storage) = ctx.temporary_set(0, "index Inventor uniqueness keys")?;
    unique_into(ctx, &mut storage, findings, &mut seen, values, key, field)
}

fn unique_into<'values, T, K, F, L>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    findings: &mut Vec<Finding>,
    seen: &mut HashSet<K>,
    values: &'values [T],
    mut key: F,
    field: L,
) -> Result<(), CodecError>
where
    K: 'values + Eq + std::hash::Hash + DecodeCost,
    F: FnMut(&'values T) -> Result<K, CodecError>,
    L: fmt::Display,
{
    let mut values = values.iter();
    while let Some(value) = ctx.next_charged(&mut values, "validate Inventor uniqueness source")? {
        let inserted = storage.with_storage(|| {
            ctx.insert_hash_set(seen, key(value)?, "index Inventor uniqueness keys")
        })?;
        if !inserted {
            push_finding(
                ctx,
                findings,
                Check::NativeLinks,
                format_args!("Inventor native data repeats a {field}"),
                None,
            )?;
        }
    }
    Ok(())
}

fn equal_btree_sets<T: Ord + DecodeCost>(
    ctx: &DecodeContext<'_>,
    left: &BTreeSet<T>,
    right: &BTreeSet<T>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    Ok(left.len() == right.len() && ctx.is_subset_btree_set(left, right, operation)?)
}

fn increment_hash_count<K: Eq + std::hash::Hash + DecodeCost>(
    ctx: &DecodeContext<'_>,
    counts: &mut HashMap<K, u64>,
    key: K,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(count) = ctx.get_mut_hash_map(counts, &key, operation)? {
        *count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    } else {
        ctx.insert_hash_map(counts, key, 1, operation)?;
    }
    Ok(())
}

fn push_finding(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    check: Check,
    message: fmt::Arguments<'_>,
    entity: Option<String>,
) -> Result<(), CodecError> {
    let message = ctx.format_retained(message, "retain Inventor validation finding message")?;
    ctx.push_vec(
        findings,
        Finding {
            check,
            severity: Severity::Error,
            message,
            entity,
        },
        "collect Inventor validation findings",
    )
}

#[cfg(test)]
mod tests;
