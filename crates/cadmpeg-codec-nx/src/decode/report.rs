// SPDX-License-Identifier: Apache-2.0
//! Geometry-report losses and feature-completeness predicates.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) fn jpeg_dimensions(payload: &[u8]) -> Option<(u16, u16, u8, u8)> {
    if payload.get(..2)? != [0xff, 0xd8] {
        return None;
    }
    let mut offset = 2usize;
    while offset < payload.len() {
        while payload.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *payload.get(offset)?;
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(View::u16_be_at(payload, offset)?);
        if length < 2 {
            return None;
        }
        let segment_start = offset + 2;
        let segment_end = offset.checked_add(length)?;
        let segment = payload.get(segment_start..segment_end)?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let precision = *segment.first()?;
            let height = View::u16_be_at(segment, 1)?;
            let width = View::u16_be_at(segment, 3)?;
            let components = *segment.get(5)?;
            if width == 0
                || height == 0
                || components == 0
                || segment.len() != 6 + 3 * usize::from(components)
            {
                return None;
            }
            return Some((width, height, precision, components));
        }
        offset = segment_end;
    }
    None
}

pub(crate) fn build_geometry_report(
    scan: &Scan,
    ir: &CadIr,
    counts: &Counts,
    has_topology: bool,
    has_unresolved_sub_bodies: bool,
    tessellation_count: usize,
    model: &crate::native::NativeModel,
) -> DecodeReport {
    let has_untransferred_attribute_fields = model.has_untransferred_parasolid_attribute_fields();
    let mut losses = Vec::new();

    losses.push(LossNote {
        code: LossKind::shared(LossTaxonomy::CarrierSummary),
        severity: Severity::Info,
        message: format!(
            "Decoded {} POINT carrier(s) verbatim from Parasolid POINT records (3×f64 big-endian, \
             metres → millimetres), {} analytic surface carrier(s) ({} plane, {} cylinder, {} \
             cone, {} sphere, {} torus), and {} analytic curve carrier(s) ({} line, {} circle, {} \
             ellipse). All parameters are byte-exact at the document's millimetre scale.",
            counts.points,
            counts.surfaces(),
            counts.planes,
            counts.cylinders,
            counts.cones,
            counts.spheres,
            counts.tori,
            counts.curves(),
            counts.lines,
            counts.circles,
            counts.ellipses,
        ),
        provenance: None,
    });

    if tessellation_count != 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Decoded {tessellation_count} embedded JT display tessellation(s) with scene-node ownership, model-space coordinates, topological triangle connectivity, and corner normals when bound."
            ),
            provenance: None,
        });
    }

    if !has_topology {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::TopologyNotTransferred),
            severity: Severity::Blocking,
            message: "The B-rep topology graph (body→shell→face→loop→fin→edge→vertex) was not \
                      reconstructed because the surviving typed records did not form a complete \
                      connected ownership graph. Exact-key supported partition↔deltas replacements \
                      and deletions were applied before graph construction. Required unresolved \
                      records prevent their dependent incidence from being emitted; decoded geometry \
                      then remains unattached."
                .to_string(),
            provenance: None,
        });
    }

    if counts.intersection_rejections.total() > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::ObjectRecordsUntransferred),
            severity: Severity::Warning,
            message: format!(
                "{} surface-intersection record(s) without a complete validated CHART_s and \
                 term-endpoint witness remain opaque constructions. Support-UV values govern \
                 optional pcurve attachment and do not invalidate a witnessed 3D carrier. Each \
                 Parasolid stream is preserved verbatim as an unknown passthrough record so the \
                 unresolved source bytes remain available. Rejections: {} missing chart, {} missing \
                 start term, {} missing end term, {} endpoint mismatch.",
                counts.intersection_rejections.total(),
                counts.intersection_rejections.missing_chart,
                counts.intersection_rejections.missing_start_term,
                counts.intersection_rejections.missing_end_term,
                counts.intersection_rejections.endpoint_mismatch,
            ),
            provenance: None,
        });
    }

    if scan.count(StreamKind::Deltas) > 0 {
        let unmatched_tombstone_counts = unmatched_delta_tombstone_counts(scan);
        let unmatched_tombstones = unmatched_tombstone_counts.values().sum::<usize>();
        let unmatched_tombstone_detail = unmatched_tombstone_counts
            .iter()
            .map(|(family, count)| format!("{family} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::DecodeDiagnostic),
            severity: if unmatched_tombstones == 0 {
                Severity::Info
            } else {
                Severity::Warning
            },
            message: if unmatched_tombstones == 0 {
                format!(
                    "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                 Equal-schema deltas were paired with the preceding partition. Exact-key \
                 BODY, SHELL, FACE, LOOP, FIN, EDGE, VERTEX, REGION, POINT, LINE, CIRCLE, ELLIPSE, PLANE, CYLINDER, CONE, SPHERE, TORUS, BLEND_SURF, OFFSET_SURF, B_SURFACE, TRIMMED_CURVE, B_CURVE, and SP_CURVE full records and compact \
                 non-topology replacements and tombstones were applied using the last event for \
                 each key within each current body-sequence interval. Validated partition topology remained authoritative, including any \
                 point, curve, or surface carrier still referenced by surviving topology. Complete \
                 ENTITY_51, ENTITY_52, ENTITY_53, and ENTITY_54 records were retained for native \
                 attribute extraction. Every completely bounded full record, compact tombstone, \
                 and BODY revision envelope was retained as an individually identified native event \
                 with its source bounds and decoded identities; BODY state tails retain exact \
                 bounded bytes and digests. Complete transmit headers retain their description, \
                 schema, consecutive identities, and exact bytes. Terminal two- and \
                 four-null-reference trailers retain their exact stream boundary and bytes. \
                 Count-selected numeric tails after \
                 term-use endpoints were retained with their ordered finite binary64 values. Maximal \
                 event gaps containing only typed stream-local references, framed reference/type \
                 maps, and complete four-reference state packets, reference-marker packets, and inline schema \
                 declarations were retained in order. \
                 Spans outside those events were retained with exact inflated-stream bounds and \
                 digests. Semantic intersection and NURBS records were retained in the semantic \
                 lane. Every \
                 terminal tombstone resolved to an exact current or earlier-added key.",
                    scan.count(StreamKind::Deltas)
                )
            } else {
                format!(
                    "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                    Equal-schema deltas were paired with the preceding partition. Exact-key revisions in current body-sequence intervals were applied using the last \
                 event for each key, but {unmatched_tombstones} terminal tombstone(s) have no exact \
                 current or earlier-added key and remain unresolved: {unmatched_tombstone_detail}.",
                    scan.count(StreamKind::Deltas)
                )
            },
            provenance: None,
        });
    }

    if has_unresolved_sub_bodies {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "This part is composed of {} sub-body partition(s); its decoded feature-history \
                 Booleans do not resolve every intermediate body object to a partition image. \
                 Carriers from all sub-bodies are emitted without the unresolved composition that \
                 would remove interior/construction faces.",
                scan.count(StreamKind::Partition)
            ),
            provenance: None,
        });
    }

    append_design_intent_losses(ir, &mut losses);

    if has_untransferred_attribute_fields {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::AttributesNotTransferred),
            severity: Severity::Warning,
            message: "A referenced Parasolid attribute value was not transferred because its \
                      complete value relation did not resolve."
                .to_string(),
            provenance: None,
        });
    }

    DecodeReport {
        format: "nx".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: summary_notes(scan),
    }
}

pub(crate) fn append_design_intent_losses(ir: &CadIr, losses: &mut Vec<LossNote>) {
    let current_body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    // Require a non-BaseFeature writer before treating body-to-history as proven.
    let active_features = crate::native::history::active_feature_closure(ir, &current_body_ids)
        .filter(|active| {
            active.iter().any(|id| {
                ir.model.features.iter().any(|feature| {
                    feature.id == *id
                        && !matches!(&feature.definition, FeatureDefinition::BaseFeature { .. })
                })
            })
        });
    let suppression_scope = active_features.as_ref().map_or("", |_| "active ");
    let feature_in_active_scope = |feature: &Feature| {
        active_features
            .as_ref()
            .is_none_or(|active| active.contains(&feature.id))
    };
    let unresolved_suppression_count = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            feature.suppressed.is_none()
                && active_features
                    .as_ref()
                    .is_none_or(|active| active.contains(&feature.id))
        })
        .count();
    if unresolved_suppression_count != 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "Suppression state remains unresolved for {unresolved_suppression_count} NX \
                 {suppression_scope}feature history operation(s)."
            ),
            provenance: None,
        });
    }

    let active_configuration_count = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active.is_active())
        .count();
    let current_bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<BTreeSet<_>>();
    let incomplete_configuration_count = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            configuration.bodies.is_unresolved()
                || active_configuration_count != 1
                || (configuration.active.is_active()
                    && configuration.bodies.resolved().is_none_or(|bodies| {
                        bodies.len() != current_bodies.len()
                            || bodies.iter().collect::<BTreeSet<_>>() != current_bodies
                    }))
                || (configuration.active.is_active()
                    && active_configuration_state_is_incomplete(ir, configuration))
        })
        .count();
    if incomplete_configuration_count != 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "Activation, complete body membership, evaluated feature state, or evaluated \
                 parameter state remains unresolved for {incomplete_configuration_count} NX \
                 design configuration(s)."
            ),
            provenance: None,
        });
    }

    let incomplete_expression_count = incomplete_expression_parameters(ir).len();
    if incomplete_expression_count != 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "Neutral evaluation or dependency semantics remain incomplete for \
                 {incomplete_expression_count} NX expression parameter(s)."
            ),
            provenance: None,
        });
    }

    let mut native_feature_kinds = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        if let FeatureDefinition::Native { kind, .. } = &feature.definition {
            *native_feature_kinds.entry(kind.as_str()).or_default() += 1;
        }
    }
    if !native_feature_kinds.is_empty() {
        let kinds = native_feature_kinds
            .into_iter()
            .map(|(kind, count)| format!("{kind} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "NX feature-history operation(s) remain native-only because their complete neutral \
                 operation semantics are not decoded: {kinds}."
            ),
            provenance: None,
        });
    }

    let mut unresolved_feature_families = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        let family = match feature.definition {
            FeatureDefinition::DatumPlaneUnresolved => "datum plane",
            FeatureDefinition::DatumPointUnresolved => "datum point",
            FeatureDefinition::DatumCoordinateSystemUnresolved => "datum coordinate system",
            FeatureDefinition::LoftUnresolved => "loft",
            FeatureDefinition::FreeformSurfaceUnresolved => "freeform surface",
            FeatureDefinition::DraftUnresolved => "draft",
            _ => continue,
        };
        *unresolved_feature_families.entry(family).or_default() += 1;
    }
    if !unresolved_feature_families.is_empty() {
        let families = unresolved_feature_families
            .into_iter()
            .map(|(family, count)| format!("{family} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "NX feature family identities were transferred, but their neutral construction \
                 semantics remain unresolved: {families}."
            ),
            provenance: None,
        });
    }

    let mut incomplete_feature_output_families = BTreeMap::<&str, usize>::new();
    let mut incomplete_feature_construction_families = BTreeMap::<&str, usize>::new();
    let generated_body_outputs = ir
        .model
        .feature_result_topologies
        .iter()
        .filter(|state| !state.bodies.is_empty())
        .map(|state| &state.output_of)
        .collect::<BTreeSet<_>>();
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        let is_exact_empty_base = matches!(
            &feature.definition,
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Resolved { bodies, native },
            } if bodies.is_empty() && !native.trim().is_empty() && feature.outputs.is_empty()
        );
        if feature.suppressed != Some(true)
            && !is_exact_empty_base
            && !output_free_native_snapshot(feature)
            && !output_free_local_body_construction(feature)
            && !output_free_pattern_construction(feature)
            && !output_free_trim_surface_construction(feature)
        {
            if let Some(family) = feature.definition.body_output_family().filter(|_| {
                let current_outputs_are_valid = !feature.outputs.is_empty()
                    && feature.outputs.iter().collect::<BTreeSet<_>>().len()
                        == feature.outputs.len()
                    && feature
                        .outputs
                        .iter()
                        .all(|output| ir.model.bodies.iter().any(|body| body.id == *output));
                !(current_outputs_are_valid
                    || feature.outputs.is_empty() && generated_body_outputs.contains(&feature.id))
            }) {
                *incomplete_feature_output_families
                    .entry(family)
                    .or_default() += 1;
                continue;
            }
        }
        let family = match &feature.definition {
            FeatureDefinition::BaseFeature { bodies }
                if !is_exact_empty_base
                    && !output_free_native_snapshot(feature)
                    && body_selection_is_incomplete(bodies) =>
            {
                "base feature"
            }
            FeatureDefinition::Block {
                dimensions,
                placement,
                op,
            } if dimensions.is_none_or(|dimensions| {
                dimensions
                    .into_iter()
                    .any(|dimension| !positive_feature_length(dimension))
            }) || placement.is_none_or(|placement| !placement.is_proper_rigid())
                || matches!(op, BooleanOp::Unresolved) =>
            {
                "block"
            }
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } if !distance.0.is_finite()
                || reference.as_ref().is_none_or(|reference| match reference {
                    DatumPlaneReference::Feature(reference) => {
                        ir.model
                            .features
                            .iter()
                            .find(|candidate| candidate.id == *reference)
                            .is_none_or(|source| source.ordinal >= feature.ordinal)
                            || !feature.dependencies.contains(reference)
                    }
                    DatumPlaneReference::Face { face, .. } => face_selection_is_incomplete(face),
                }) =>
            {
                "datum plane"
            }
            FeatureDefinition::DatumPlane {
                origin,
                normal,
                u_axis,
            } if datum_plane_is_incomplete(*origin, *normal, *u_axis) => "datum plane",
            FeatureDefinition::DatumAxis { origin, direction }
                if !finite_feature_point(*origin) || !valid_feature_direction(*direction) =>
            {
                "datum axis"
            }
            FeatureDefinition::DatumPoint { position, .. } if !finite_feature_point(*position) => {
                "datum point"
            }
            FeatureDefinition::DatumCoordinateSystem {
                origin,
                x_axis,
                y_axis,
                z_axis,
            } if datum_coordinate_system_is_incomplete(*origin, *x_axis, *y_axis, *z_axis) => {
                "datum coordinate system"
            }
            FeatureDefinition::ExtractBody { source } if body_selection_is_incomplete(source) => {
                "extract body"
            }
            FeatureDefinition::Sketch { space, sketch }
                if !matches!(space, SketchSpace::Planar)
                    || sketch.as_ref().is_none_or(|sketch| {
                        ir.model
                            .sketches
                            .iter()
                            .find(|candidate| candidate.id == *sketch)
                            .is_none_or(|sketch| {
                                matches!(
                                    sketch.placement,
                                    cadmpeg_ir::sketches::SketchPlacement::Unresolved
                                )
                            })
                    }) =>
            {
                "sketch"
            }
            FeatureDefinition::Loft { .. } if loft_definition_is_incomplete(feature) => "loft",
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            } if path_ref_is_incomplete(source)
                || face_selection_is_incomplete(target_faces)
                || projected_curve_direction_is_incomplete(*direction)
                || bidirectional.is_none() =>
            {
                "projected curve"
            }
            FeatureDefinition::TrimSurface { .. }
                if trim_surface_definition_is_incomplete(feature) =>
            {
                "trim surface"
            }
            FeatureDefinition::ExtendSurface { .. }
                if extend_surface_definition_is_incomplete(feature) =>
            {
                "extend surface"
            }
            FeatureDefinition::Hole { .. } if hole_definition_is_incomplete(feature) => "hole",
            FeatureDefinition::Rib { .. } if rib_definition_is_incomplete(feature) => "rib",
            FeatureDefinition::Chamfer { .. } if chamfer_definition_is_incomplete(feature) => {
                "chamfer"
            }
            FeatureDefinition::Fillet { .. } if fillet_definition_is_incomplete(feature) => {
                "fillet"
            }
            FeatureDefinition::FaceBlend { .. } if face_blend_definition_is_incomplete(feature) => {
                "face blend"
            }
            FeatureDefinition::SewBodies { .. } if sew_bodies_definition_is_incomplete(feature) => {
                "sew bodies"
            }
            FeatureDefinition::TrimBodies { .. }
                if trim_bodies_definition_is_incomplete(feature) =>
            {
                "trim bodies"
            }
            FeatureDefinition::Extrude { .. } if extrude_definition_is_incomplete(feature) => {
                "extrude"
            }
            FeatureDefinition::Revolve { .. } if revolve_definition_is_incomplete(feature) => {
                "revolve"
            }
            FeatureDefinition::Sweep { .. } if sweep_definition_is_incomplete(feature) => "sweep",
            FeatureDefinition::OffsetSurface { .. }
                if offset_surface_definition_is_incomplete(feature) =>
            {
                "offset surface"
            }
            FeatureDefinition::Thicken { .. } if thicken_definition_is_incomplete(feature) => {
                "thicken"
            }
            FeatureDefinition::Draft { .. } if draft_definition_is_incomplete(feature) => "draft",
            FeatureDefinition::Pattern { seeds, pattern }
                if pattern_feature_is_incomplete(seeds, pattern, &feature.dependencies) =>
            {
                "pattern"
            }
            FeatureDefinition::SectionShape {
                first,
                second,
                approximate,
            } if body_selection_is_incomplete(first)
                || body_selection_is_incomplete(second)
                || body_selections_overlap(first, second)
                || approximate.is_none() =>
            {
                "section"
            }
            FeatureDefinition::Combine { .. } if combine_definition_is_incomplete(feature) => {
                "body combine"
            }
            FeatureDefinition::DeleteBody { .. }
                if delete_body_definition_is_incomplete(feature) =>
            {
                "delete body"
            }
            FeatureDefinition::ReplaceFace { .. }
                if replace_face_definition_is_incomplete(feature) =>
            {
                "replace face"
            }
            _ => continue,
        };
        *incomplete_feature_construction_families
            .entry(family)
            .or_default() += 1;
    }
    if !incomplete_feature_output_families.is_empty() {
        let families = incomplete_feature_output_families
            .into_iter()
            .map(|(family, count)| format!("{family} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "NX typed feature operation output lineage is missing, duplicated, or does not \
                 resolve to a transferred body: {families}."
            ),
            provenance: None,
        });
    }
    if !incomplete_feature_construction_families.is_empty() {
        let families = incomplete_feature_construction_families
            .into_iter()
            .map(|(family, count)| format!("{family} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "NX typed feature operations have incomplete neutral construction fields: \
                 {families}."
            ),
            provenance: None,
        });
    }

    let sketch_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
        .filter(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .count();
    let unresolved_sketch_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
        .filter(|feature| {
            matches!(
                feature.definition,
                FeatureDefinition::Sketch { sketch: None, .. }
            )
        })
        .count();
    if unresolved_sketch_feature_count != 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "Decoded {sketch_feature_count} NX sketch history feature(s), of which \
                 {unresolved_sketch_feature_count} have no neutral sketch graph because complete \
                 sketch placement and entity semantics are unresolved."
            ),
            provenance: None,
        });
    }

    let active_sketch_ids = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } => Some(sketch.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let sketch_in_active_scope = |sketch: &cadmpeg_ir::sketches::SketchId| {
        active_features.is_none() || active_sketch_ids.contains(sketch)
    };
    let native_sketch_entity_count = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| sketch_in_active_scope(&entity.sketch))
        .filter(|entity| {
            matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::Native { .. }
            )
        })
        .count();
    let native_sketch_constraint_count = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| sketch_in_active_scope(&constraint.sketch))
        .filter(|constraint| {
            matches!(
                constraint.definition,
                cadmpeg_ir::sketches::SketchConstraintDefinition::Native { .. }
            )
        })
        .count();
    if native_sketch_entity_count != 0 || native_sketch_constraint_count != 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "Neutral semantics remain unresolved for {native_sketch_entity_count} NX sketch \
                 geometry record(s) and {native_sketch_constraint_count} sketch constraint \
                 record(s)."
            ),
            provenance: None,
        });
    }
}

pub(crate) fn output_free_native_snapshot(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.outputs.is_empty()
        && feature.name.as_deref() == Some("MASTER SNAPSHOT BODY")
        && matches!(
            &feature.definition,
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved
            }
        )
        && feature
            .source_properties
            .get("operation_record")
            .is_some_and(|record| !record.trim().is_empty())
}

/// Return whether a feature's primary body is local to the history namespace.
///
/// Offset-store and unbound object-namespace bodies are retained as native
/// feature-local identities. They do not create neutral current-body outputs;
/// the saved segment image remains the only neutral body census.
pub(crate) fn output_free_local_body_construction(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.outputs.is_empty()
        && feature
            .source_properties
            .contains_key("primary_body_reference")
        && !feature
            .source_properties
            .contains_key("primary_body_segment_use")
}

/// Return whether a pattern record is construction-only and has no neutral
/// body-output obligation.
///
/// Pattern construction records without a primary-body field describe the
/// seed and transform graph. A body-affecting pattern has at least one body
/// reference occurrence, even when the occurrence is too ambiguous to become
/// a primary writer. Keep that distinction explicit so an incomplete body
/// binding cannot be mistaken for a construction-only record.
pub(crate) fn output_free_pattern_construction(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.outputs.is_empty()
        && matches!(&feature.definition, FeatureDefinition::Pattern { .. })
        && !feature.source_properties.keys().any(|key| {
            key == "primary_body_reference"
                || key == "primary_body_object_index"
                || key == "primary_body_data_block"
                || key.starts_with("body_reference.")
                || key.starts_with("body_reference_occurrence.")
        })
}

/// Return whether a `TRIMMED_SH` record is a construction-only operation.
///
/// NX uses the typed trim-surface family for records that carry no body
/// occurrence or primary-body field. Those records have no body result to
/// bind; a body marker makes the output obligation explicit again.
pub(crate) fn output_free_trim_surface_construction(
    feature: &cadmpeg_ir::features::Feature,
) -> bool {
    feature.outputs.is_empty()
        && matches!(&feature.definition, FeatureDefinition::TrimSurface { .. })
        && !feature.source_properties.keys().any(|key| {
            key == "primary_body_reference"
                || key == "primary_body_object_index"
                || key == "primary_body_data_block"
                || key.starts_with("body_reference.")
                || key.starts_with("body_reference_occurrence.")
        })
}

pub(crate) fn active_configuration_state_is_incomplete(
    ir: &CadIr,
    configuration: &cadmpeg_ir::features::DesignConfiguration,
) -> bool {
    let suppressed_features = configuration
        .suppressed_features
        .iter()
        .collect::<BTreeSet<_>>();
    if suppressed_features.len() != configuration.suppressed_features.len()
        || ir.model.features.iter().any(|feature| {
            feature
                .suppressed
                .is_none_or(|suppressed| suppressed_features.contains(&feature.id) != suppressed)
        })
    {
        return true;
    }
    let Some(bodies) = configuration.bodies.resolved() else {
        return true;
    };
    let active_features = if ir.model.features.is_empty() {
        BTreeSet::new()
    } else {
        let Some(active_features) = crate::native::history::active_feature_closure(ir, bodies)
        else {
            return true;
        };
        active_features
    };
    if configuration.feature_states.len() != active_features.len() {
        return true;
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<BTreeMap<_, _>>();
    if active_features.iter().any(|id| {
        let (Some(feature), Some(state)) = (features.get(id), configuration.feature_states.get(id))
        else {
            return true;
        };
        state.suppressed
            || state.dependencies != feature.dependencies
            || state.outputs != feature.outputs
            || state.definition != feature.definition
    }) {
        return true;
    }

    configuration.parameter_values.len() != ir.model.parameters.len()
        || ir.model.parameters.iter().any(|parameter| {
            parameter.value.as_ref().is_none_or(|value| {
                configuration.parameter_values.get(&parameter.id) != Some(value)
            })
        })
}

pub(crate) fn datum_plane_is_incomplete(origin: Point3, normal: Vector3, u_axis: Vector3) -> bool {
    !finite_feature_point(origin)
        || !valid_feature_direction(normal)
        || !valid_feature_direction(u_axis)
        || !directions_are_perpendicular(normal, u_axis)
}

pub(crate) fn datum_coordinate_system_is_incomplete(
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
) -> bool {
    if !finite_feature_point(origin)
        || !unit_feature_direction(x_axis)
        || !unit_feature_direction(y_axis)
        || !unit_feature_direction(z_axis)
        || !directions_are_perpendicular(x_axis, y_axis)
        || !directions_are_perpendicular(y_axis, z_axis)
        || !directions_are_perpendicular(z_axis, x_axis)
    {
        return true;
    }
    let handedness = x_axis.cross(y_axis).dot(z_axis);
    !handedness.is_finite() || (handedness - 1.0).abs() > 1e-9
}

pub(crate) fn projected_curve_direction_is_incomplete(direction: CurveProjectionDirection) -> bool {
    match direction {
        CurveProjectionDirection::Vector(direction) => !valid_feature_direction(direction),
        CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved) => true,
        CurveProjectionDirection::State(CurveProjectionDirectionState::TargetNormal) => false,
    }
}

pub(crate) fn unit_feature_direction(direction: Vector3) -> bool {
    valid_feature_direction(direction) && (direction.norm() - 1.0).abs() <= 1e-9
}

pub(crate) fn directions_are_perpendicular(first: Vector3, second: Vector3) -> bool {
    let scale = first.norm() * second.norm();
    scale.is_finite() && first.dot(second).abs() <= 1e-9 * scale
}

pub(crate) fn incomplete_expression_parameters(ir: &CadIr) -> BTreeSet<ParameterId> {
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.owner.clone())
        .collect::<BTreeSet<_>>();
    let mut incomplete = BTreeSet::new();
    for owner in parameter_owners {
        let parameters = ir
            .model
            .parameters
            .iter()
            .filter(|parameter| parameter.owner == owner)
            .collect::<Vec<_>>();
        let mut ids_by_name = BTreeMap::<(&str, Option<&str>), Vec<&ParameterId>>::new();
        for parameter in &parameters {
            ids_by_name
                .entry((
                    parameter.name.as_str(),
                    parameter.properties.get("unit").map(String::as_str),
                ))
                .or_default()
                .push(&parameter.id);
        }
        let expected = parameters
            .iter()
            .map(|parameter| {
                let unit = match parameter.properties.get("unit").map(String::as_str) {
                    None => None,
                    Some(unit @ ("millimeter" | "degree")) => Some(unit),
                    Some(_) => return None,
                };
                let [_] = ids_by_name
                    .get(&(parameter.name.as_str(), unit))?
                    .as_slice()
                else {
                    return None;
                };
                let mut seen = BTreeSet::new();
                let dependencies = crate::native::expression_parameter_names(&parameter.expression)
                    .into_iter()
                    .map(|name| {
                        let [dependency] = ids_by_name.get(&(name, unit))?.as_slice() else {
                            return None;
                        };
                        Some((*dependency).clone())
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(
                    dependencies
                        .into_iter()
                        .filter(|dependency| seen.insert(dependency.clone()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let indices = parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| (&parameter.id, index))
            .collect::<BTreeMap<_, _>>();
        let mut emitted = BTreeSet::new();
        let mut evaluated = BTreeMap::<ParameterId, f64>::new();
        while let Some(index) = (0..parameters.len()).find(|index| {
            !emitted.contains(index)
                && expected[*index].as_ref().is_some_and(|dependencies| {
                    dependencies.iter().all(|dependency| {
                        evaluated.contains_key(dependency)
                            && indices
                                .get(dependency)
                                .is_some_and(|index| emitted.contains(index))
                    })
                })
        }) {
            let parameter = parameters[index];
            let unit = parameter.properties.get("unit").map(String::as_str);
            let value =
                crate::native::evaluate_parameterized_expression(&parameter.expression, |name| {
                    let [dependency] = ids_by_name.get(&(name, unit))?.as_slice() else {
                        return None;
                    };
                    evaluated.get(*dependency).copied()
                });
            let stored = match (unit, parameter.value.as_ref()) {
                (Some("millimeter"), Some(cadmpeg_ir::features::ParameterValue::Length(value))) => {
                    Some(value.0)
                }
                (Some("degree"), Some(cadmpeg_ir::features::ParameterValue::Angle(value))) => {
                    Some(value.0.to_degrees())
                }
                (None, Some(cadmpeg_ir::features::ParameterValue::Real(value))) => Some(*value),
                (None, Some(cadmpeg_ir::features::ParameterValue::Integer(value))) => {
                    Some(*value as f64)
                }
                _ => None,
            };
            if let (Some(value), Some(stored)) = (value, stored) {
                let tolerance = 64.0 * f64::EPSILON * value.abs().max(stored.abs()).max(1.0);
                if value.is_finite() && stored.is_finite() && (value - stored).abs() <= tolerance {
                    evaluated.insert(parameter.id.clone(), value);
                }
            }
            emitted.insert(index);
        }
        for (index, parameter) in parameters.into_iter().enumerate() {
            if expected[index].as_ref() != Some(&parameter.dependencies)
                || !emitted.contains(&index)
                || !evaluated.contains_key(&parameter.id)
            {
                incomplete.insert(parameter.id.clone());
            }
        }
    }
    incomplete
}

pub(crate) fn trim_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::TrimSurface { faces, tool, keep } = &feature.definition else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || path_ref_is_incomplete(tool)
        || matches!(keep, TrimRegion::Unresolved)
}

pub(crate) fn extend_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::ExtendSurface {
        faces,
        distance,
        method,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || distance.is_none_or(|distance| !positive_feature_length(distance))
        || matches!(method, cadmpeg_ir::features::SurfaceExtension::Unresolved)
}

pub(crate) fn sew_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::SewBodies {
        bodies,
        gap_tolerance,
    } = &feature.definition
    else {
        return true;
    };
    body_selection_is_incomplete(bodies)
        || resolved_body_selection_len(bodies).is_some_and(|count| count < 2)
        || gap_tolerance.is_some_and(|tolerance| !positive_feature_length(tolerance))
}

pub(crate) fn combine_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Combine {
        target, tools, op, ..
    } = &feature.definition
    else {
        return true;
    };
    body_selection_is_incomplete(target)
        || body_selection_is_incomplete(tools)
        || resolved_body_selection_len(target) != Some(1)
        || body_selections_overlap(target, tools)
        || matches!(op, BooleanOp::Unresolved | BooleanOp::NewBody)
}

pub(crate) fn trim_bodies_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::TrimBodies {
        targets,
        tools,
        keep,
    } = &feature.definition
    else {
        return true;
    };
    body_selection_is_incomplete(targets)
        || body_selection_is_incomplete(tools)
        || body_selections_overlap(targets, tools)
        || matches!(keep, BodyTrimSide::Unresolved)
}

pub(crate) fn delete_body_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::DeleteBody { bodies, mode } = &feature.definition else {
        return true;
    };
    body_selection_is_incomplete(bodies) || matches!(mode, BodyRetentionMode::Unresolved)
}

pub(crate) fn hole_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Hole {
        profile,
        profile_filter,
        face,
        position,
        direction,
        placements,
        kind,
        exit_kind,
        diameter,
        extent,
        bottom,
        taper_angle,
        specification,
        ..
    } = &feature.definition
    else {
        return true;
    };
    hole_feature_is_incomplete(
        profile.as_ref(),
        face.as_ref(),
        (*position, *direction),
        placements,
        (kind, exit_kind.as_ref()),
        *diameter,
        extent.as_ref(),
    ) || hole_auxiliary_semantics_are_incomplete(
        profile_filter.as_ref(),
        bottom.as_ref(),
        *taper_angle,
        specification.as_deref(),
    ) || extent
        .as_ref()
        .is_some_and(|extent| termination_dependency_is_incomplete(extent, &feature.dependencies))
        || profile
            .as_ref()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, &feature.dependencies))
}

pub(crate) fn chamfer_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Chamfer { groups, .. } = &feature.definition else {
        return true;
    };
    groups.is_empty()
        || groups.iter().any(|group| {
            edge_selection_is_incomplete(&group.edges) || chamfer_spec_is_incomplete(&group.spec)
        })
}

pub(crate) fn fillet_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Fillet { groups } = &feature.definition else {
        return true;
    };
    groups.is_empty()
        || groups.iter().any(|group| {
            edge_selection_is_incomplete(&group.edges)
                || radius_spec_is_incomplete(&group.radius)
                || group
                    .tangency_weight
                    .is_some_and(|weight| !weight.is_finite())
        })
}

pub(crate) fn face_blend_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::FaceBlend {
        first_faces,
        second_faces,
        radius,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(first_faces)
        || face_selection_is_incomplete(second_faces)
        || face_selections_overlap(first_faces, second_faces)
        || radius_spec_is_incomplete(radius)
}

pub(crate) fn offset_surface_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::OffsetSurface { faces, distance } = &feature.definition else {
        return true;
    };
    face_selection_is_incomplete(faces) || distance.is_none_or(|distance| !distance.0.is_finite())
}

pub(crate) fn thicken_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Thicken {
        faces,
        thickness,
        side,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || thickness.is_none_or(|thickness| !positive_feature_length(thickness))
        || side.is_none()
}

pub(crate) fn draft_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Draft {
        faces,
        neutral_plane,
        parting_tool,
        pull_direction,
        pull_plane,
        angle,
        outward,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(faces)
        || parting_tool.as_ref().map_or_else(
            || face_selection_is_incomplete(neutral_plane),
            face_selection_is_incomplete,
        )
        || pull_direction.is_none_or(|direction| !valid_feature_direction(direction))
        || pull_plane
            .as_ref()
            .is_some_and(|plane| plane.as_str().is_empty())
        || angle.is_none_or(|angle| !valid_draft_angle(angle))
        || outward.is_none()
}

pub(crate) fn replace_face_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::ReplaceFace {
        targets,
        replacements,
    } = &feature.definition
    else {
        return true;
    };
    face_selection_is_incomplete(targets)
        || face_selection_is_incomplete(replacements)
        || face_selections_overlap(targets, replacements)
}

pub(crate) fn loft_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Loft {
        sections,
        centerline,
        guides,
        op,
        max_degree,
        ..
    } = &feature.definition
    else {
        return true;
    };
    sections.len() < 2
        || sections.iter().any(loft_section_is_incomplete)
        || sections.iter().any(|section| {
            matches!(
                section,
                LoftSection::Profile(profile)
                    if profile_dependency_is_incomplete(profile, &feature.dependencies)
            )
        })
        || centerline.as_ref().is_some_and(path_ref_is_incomplete)
        || guides.iter().any(path_ref_is_incomplete)
        || (centerline.is_some() && !guides.is_empty())
        || max_degree.is_some_and(|degree| degree == 0)
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn extrude_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Extrude {
        profile,
        direction,
        start,
        extent,
        op,
        solid,
        direction_source,
        face_maker,
        ..
    } = &feature.definition
    else {
        return true;
    };
    profile_ref_is_incomplete(profile)
        || profile_dependency_is_incomplete(profile, &feature.dependencies)
        || matches!(
            direction,
            cadmpeg_ir::features::ExtrudeDirection::Unresolved
        )
        || matches!(
            direction,
            cadmpeg_ir::features::ExtrudeDirection::Explicit(direction)
                if !valid_feature_direction(*direction)
        )
        || extrude_start_is_incomplete(start)
        || extrude_extent_is_incomplete(extent, &feature.dependencies)
        || matches!(op, BooleanOp::Unresolved)
        || solid.is_none()
        || direction_source.as_ref().is_some_and(|source| {
            matches!(
                source,
                cadmpeg_ir::features::ExtrusionDirectionSource::Edge { reference }
                    if path_ref_is_incomplete(reference)
            )
        })
        || face_maker
            .as_ref()
            .is_some_and(|maker| maker.class.trim().is_empty())
}

pub(crate) fn revolve_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Revolve { construction, op } = &feature.definition else {
        return true;
    };
    revolve_feature_is_incomplete(construction, *op, &feature.dependencies)
}

pub(crate) fn rib_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Rib { construction, op } = &feature.definition else {
        return true;
    };
    rib_feature_is_incomplete(construction, *op)
        || construction
            .profile
            .as_ref()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, &feature.dependencies))
}

pub(crate) fn sweep_definition_is_incomplete(feature: &Feature) -> bool {
    let FeatureDefinition::Sweep {
        section,
        sections,
        path,
        mode,
        orientation,
        transition,
        transformation,
        twist,
        scale,
        ..
    } = &feature.definition
    else {
        return true;
    };
    matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_))
        || section
            .referenced_profile()
            .is_some_and(profile_ref_is_incomplete)
        || section
            .referenced_profile()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, &feature.dependencies))
        || sections.iter().any(|section| {
            matches!(section, cadmpeg_ir::features::SweepSection::Unresolved(_))
                || section
                    .referenced_profile()
                    .is_some_and(profile_ref_is_incomplete)
        })
        || sections.iter().any(|section| {
            section.referenced_profile().is_some_and(|profile| {
                profile_dependency_is_incomplete(profile, &feature.dependencies)
            })
        })
        || path.as_ref().is_none_or(path_ref_is_incomplete)
        || sweep_mode_is_incomplete(*mode)
        || orientation
            .as_ref()
            .is_none_or(sweep_orientation_is_incomplete)
        || transition.is_none()
        || transformation.is_none()
        || twist.is_some_and(|twist| !twist.0.is_finite())
        || scale.is_some_and(|scale| !scale.is_finite() || scale <= 0.0)
}

pub(crate) fn hole_feature_is_incomplete(
    profile: Option<&ProfileRef>,
    face: Option<&FaceSelection>,
    authored_axis: (Option<Point3>, Option<Vector3>),
    placements: &[cadmpeg_ir::features::HolePlacement],
    treatments: (&HoleKind, Option<&HoleKind>),
    diameter: Option<Length>,
    extent: Option<&Termination>,
) -> bool {
    let (position, direction) = authored_axis;
    let (kind, exit_kind) = treatments;
    let profile_incomplete = profile.is_some_and(profile_ref_is_incomplete);
    let face_incomplete = face.is_some_and(face_selection_is_incomplete);
    let finite_point =
        |point: Point3| point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
    let finite_direction = |vector: Vector3| {
        vector.x.is_finite()
            && vector.y.is_finite()
            && vector.z.is_finite()
            && vector.norm() > 1e-12
    };
    let axis_is_direction_invariant = matches!(extent, Some(Termination::ThroughAll))
        && exit_kind.is_none_or(|exit| exit == kind);
    let placements_complete = !placements.is_empty()
        && !placements
            .iter()
            .enumerate()
            .any(|(index, placement)| placements[index + 1..].contains(placement))
        && placements.iter().all(|placement| match placement {
            cadmpeg_ir::features::HolePlacement::Directed {
                position,
                direction,
            } => finite_point(*position) && finite_direction(*direction),
            cadmpeg_ir::features::HolePlacement::Axis { origin, axis } => {
                axis_is_direction_invariant && finite_point(*origin) && finite_direction(*axis)
            }
        });
    let placements_incomplete = !placements.is_empty() && !placements_complete;
    let authored_axis_incomplete = position.is_some_and(|point| !finite_point(point))
        || direction.is_some_and(|vector| !finite_direction(vector));
    let location_unresolved =
        !placements_complete && position.is_none() && profile.is_none_or(profile_ref_is_incomplete);
    let orientation_unresolved = !placements_complete
        && direction.is_none()
        && face.is_none_or(face_selection_is_incomplete);
    profile_incomplete
        || face_incomplete
        || authored_axis_incomplete
        || placements_incomplete
        || location_unresolved
        || orientation_unresolved
        || hole_kind_is_incomplete(kind, diameter)
        || exit_kind.is_some_and(|kind| hole_kind_is_incomplete(kind, diameter))
        || diameter.is_none_or(|diameter| !positive_feature_length(diameter))
        || extent.is_none_or(termination_is_incomplete)
}

pub(crate) fn hole_kind_is_incomplete(kind: &HoleKind, bore_diameter: Option<Length>) -> bool {
    let valid_angle = |angle: cadmpeg_ir::features::Angle| {
        angle.0.is_finite() && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
    };
    let treatment_diameter_is_incomplete = |diameter: Length| {
        !positive_feature_length(diameter) || bore_diameter.is_none_or(|bore| diameter.0 <= bore.0)
    };
    match kind {
        HoleKind::Unresolved { .. } => true,
        HoleKind::Simple => false,
        HoleKind::Chamfer { diameter, angle } | HoleKind::Countersink { diameter, angle } => {
            treatment_diameter_is_incomplete(*diameter) || !valid_angle(*angle)
        }
        HoleKind::SimpleDrilled { drill_point_angle } => !valid_angle(*drill_point_angle),
        HoleKind::Counterbore { diameter, depth } => {
            treatment_diameter_is_incomplete(*diameter) || !positive_feature_length(*depth)
        }
        HoleKind::CounterboreDrilled {
            diameter,
            depth,
            drill_point_angle,
        } => {
            treatment_diameter_is_incomplete(*diameter)
                || !positive_feature_length(*depth)
                || !valid_angle(*drill_point_angle)
        }
        HoleKind::Threaded {
            major_diameter,
            thread_depth,
            pitch,
            drill_point_angle,
        } => {
            !positive_feature_length(*major_diameter)
                || !positive_feature_length(*thread_depth)
                || pitch.is_some_and(|pitch| !positive_feature_length(pitch))
                || !valid_angle(*drill_point_angle)
                || bore_diameter.is_none_or(|diameter| major_diameter.0 <= diameter.0)
        }
        HoleKind::Counterdrill {
            diameter,
            entry_diameter,
            depth,
            angle,
        } => {
            treatment_diameter_is_incomplete(*diameter)
                || entry_diameter
                    .is_some_and(|entry| !positive_feature_length(entry) || entry.0 <= diameter.0)
                || !positive_feature_length(*depth)
                || !valid_angle(*angle)
        }
    }
}

pub(crate) fn hole_auxiliary_semantics_are_incomplete(
    profile_filter: Option<&cadmpeg_ir::features::HoleProfileFilter>,
    bottom: Option<&cadmpeg_ir::features::HoleBottom>,
    taper_angle: Option<cadmpeg_ir::features::Angle>,
    specification: Option<&cadmpeg_ir::features::HoleSpecification>,
) -> bool {
    let valid_angle = |angle: cadmpeg_ir::features::Angle| {
        angle.0.is_finite() && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
    };
    profile_filter.is_some_and(|filter| !filter.points && !filter.circles && !filter.arcs)
        || bottom.is_some_and(|bottom| {
            matches!(
                bottom,
                cadmpeg_ir::features::HoleBottom::Angled { included_angle, .. }
                    if !valid_angle(*included_angle)
            )
        })
        || taper_angle.is_some_and(|angle| !valid_angle(angle))
        || specification.is_some_and(|specification| {
            specification.standard.trim().is_empty()
                || specification
                    .pitch
                    .is_some_and(|pitch| !positive_feature_length(pitch))
                || specification
                    .major_diameter
                    .is_some_and(|diameter| !positive_feature_length(diameter))
                || specification
                    .clearance
                    .is_some_and(|clearance| !clearance.0.is_finite())
                || matches!(
                    specification.depth,
                    cadmpeg_ir::features::HoleThreadDepth::Blind { depth }
                        if !positive_feature_length(depth)
                )
        })
}

pub(crate) fn chamfer_spec_is_incomplete(spec: &ChamferSpec) -> bool {
    match spec {
        ChamferSpec::Unresolved { .. } => true,
        ChamferSpec::Distance { distance } => !positive_feature_length(*distance),
        ChamferSpec::TwoDistances { first, second } => {
            !positive_feature_length(*first) || !positive_feature_length(*second)
        }
        ChamferSpec::DistanceAngle { distance, angle } => {
            !positive_feature_length(*distance)
                || !angle.0.is_finite()
                || angle.0 <= 0.0
                || angle.0 >= std::f64::consts::PI
        }
    }
}

pub(crate) fn extrude_extent_is_incomplete(
    extent: &ExtrudeExtent,
    dependencies: &[FeatureId],
) -> bool {
    let side_is_incomplete = |side: &cadmpeg_ir::features::ExtrudeSide| {
        termination_is_incomplete(&side.termination)
            || termination_dependency_is_incomplete(&side.termination, dependencies)
            || side.draft.is_some_and(|angle| {
                !angle.0.is_finite() || angle.0.abs() >= std::f64::consts::FRAC_PI_2
            })
            || side.offset.is_some_and(|offset| !offset.0.is_finite())
    };
    match extent {
        ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
            side_is_incomplete(side)
        }
        ExtrudeExtent::TwoSided { first, second } => {
            side_is_incomplete(first) || side_is_incomplete(second)
        }
    }
}

pub(crate) fn extrude_start_is_incomplete(start: &ExtrudeStart) -> bool {
    match start {
        ExtrudeStart::Unresolved => true,
        ExtrudeStart::FromFace { face, offset } => {
            face_selection_is_incomplete(face) || offset.is_some_and(|offset| !offset.0.is_finite())
        }
        ExtrudeStart::OffsetProfilePlane { offset } => !offset.0.is_finite(),
        ExtrudeStart::ProfilePlane => false,
    }
}

pub(crate) fn revolve_feature_is_incomplete(
    construction: &RevolutionConstruction,
    op: BooleanOp,
    dependencies: &[FeatureId],
) -> bool {
    construction
        .profile
        .as_ref()
        .is_none_or(profile_ref_is_incomplete)
        || construction
            .profile
            .as_ref()
            .is_some_and(|profile| profile_dependency_is_incomplete(profile, dependencies))
        || construction.axis.is_none_or(|axis| {
            !finite_feature_point(axis.origin) || !unit_feature_direction(axis.direction)
        })
        || construction.extent.as_ref().is_none_or(|extent| {
            let side_is_incomplete = |termination: &Termination| {
                termination_is_incomplete(termination)
                    || termination_dependency_is_incomplete(termination, dependencies)
            };
            match extent {
                RevolveExtent::OneSided { termination }
                | RevolveExtent::Symmetric { termination } => side_is_incomplete(termination),
                RevolveExtent::TwoSided { first, second } => {
                    side_is_incomplete(first) || side_is_incomplete(second)
                }
            }
        })
        || construction
            .axis_reference
            .as_ref()
            .is_some_and(path_ref_is_incomplete)
        || construction.solid.is_none()
        || construction
            .face_maker_class
            .as_ref()
            .is_some_and(|class| class.trim().is_empty())
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn termination_is_incomplete(termination: &Termination) -> bool {
    match termination {
        Termination::Unresolved => true,
        Termination::ToFace { face, offset } => {
            face_selection_is_incomplete(face) || offset.is_some_and(|offset| !offset.0.is_finite())
        }
        Termination::ToVertex { vertex } => match vertex {
            VertexSelection::Generated { vertex, native } => {
                native.trim().is_empty() || vertex.local_id.trim().is_empty()
            }
            VertexSelection::Historical {
                state,
                vertex,
                native,
            } => {
                state.0.trim().is_empty() || vertex.0.trim().is_empty() || native.trim().is_empty()
            }
            VertexSelection::Unresolved | VertexSelection::Native(_) => true,
        },
        Termination::OffsetFromFace { face, offset } => {
            face_selection_is_incomplete(face) || !positive_feature_length(*offset)
        }
        Termination::ToShape { target } => face_selection_is_incomplete(target),
        Termination::Blind { length } => !length.0.is_finite() || length.0 == 0.0,
        Termination::Angle { angle } => !angle.0.is_finite() || angle.0 <= 0.0,
        Termination::ThroughAll
        | Termination::ThroughNext
        | Termination::ToFirst
        | Termination::ToLast => false,
    }
}

pub(crate) fn termination_dependency_is_incomplete(
    termination: &Termination,
    dependencies: &[FeatureId],
) -> bool {
    matches!(
        termination,
        Termination::ToVertex {
            vertex: VertexSelection::Generated { vertex, .. },
        } if !dependencies.contains(&vertex.feature)
    )
}

pub(crate) fn rib_feature_is_incomplete(construction: &RibConstruction, op: BooleanOp) -> bool {
    construction
        .profile
        .as_ref()
        .is_none_or(profile_ref_is_incomplete)
        || construction
            .direction
            .is_none_or(|direction| !valid_feature_direction(direction))
        || construction
            .thickness
            .is_none_or(|thickness| !positive_feature_length(thickness))
        || construction.side.is_none()
        || matches!(construction.draft, RibDraft::Unresolved)
        || matches!(construction.draft, RibDraft::Angle(angle) if !valid_draft_angle(angle))
        || matches!(op, BooleanOp::Unresolved)
}

pub(crate) fn sweep_mode_is_incomplete(mode: SweepMode) -> bool {
    match mode {
        SweepMode::Unresolved
        | SweepMode::Solid {
            op: BooleanOp::Unresolved,
        } => true,
        SweepMode::Solid { .. } | SweepMode::Surface => false,
    }
}

pub(crate) fn sweep_orientation_is_incomplete(orientation: &SweepOrientation) -> bool {
    match orientation {
        SweepOrientation::Auxiliary { path, .. } => path_ref_is_incomplete(path),
        SweepOrientation::GuideSurface { faces } => face_selection_is_incomplete(faces),
        SweepOrientation::Binormal { direction } => !valid_feature_direction(*direction),
        SweepOrientation::CorrectedFrenet | SweepOrientation::Fixed | SweepOrientation::Frenet => {
            false
        }
    }
}

pub(crate) fn pattern_is_incomplete(pattern: &PatternKind) -> bool {
    match pattern {
        PatternKind::Unresolved { .. } => true,
        PatternKind::Linear {
            direction,
            spacing,
            count,
            second,
        } => {
            direction.is_none_or(|direction| !valid_feature_direction(direction))
                || !positive_feature_length(*spacing)
                || *count < 2
                || second.as_ref().is_some_and(|second| {
                    !valid_feature_direction(second.direction)
                        || !positive_feature_length(second.spacing)
                        || second.count == 0
                })
        }
        PatternKind::LinearOffsets { direction, offsets } => {
            direction.is_none_or(|direction| !valid_feature_direction(direction))
                || offsets.len() < 2
                || !valid_increasing_locations(offsets.iter().map(|offset| offset.0))
        }
        PatternKind::Circular {
            axis_origin,
            axis_dir,
            angle,
            count,
        } => {
            !finite_feature_point(*axis_origin)
                || !valid_feature_direction(*axis_dir)
                || !angle.0.is_finite()
                || angle.0 <= 0.0
                || *count < 2
        }
        PatternKind::CircularAngles {
            axis_origin,
            axis_dir,
            angles,
        } => {
            !finite_feature_point(*axis_origin)
                || !valid_feature_direction(*axis_dir)
                || angles.len() < 2
                || !valid_increasing_locations(angles.iter().map(|angle| angle.0))
        }
        PatternKind::Mirror {
            plane_origin,
            plane_normal,
        } => !finite_feature_point(*plane_origin) || !valid_feature_direction(*plane_normal),
        PatternKind::MirrorReference { .. } => true,
        PatternKind::CurveDriven {
            path,
            spacing,
            count,
        } => {
            path.as_ref().is_none_or(path_ref_is_incomplete)
                || !positive_feature_length(*spacing)
                || *count < 2
        }
        PatternKind::Scale {
            center,
            final_factor,
            count,
        } => {
            matches!(center, cadmpeg_ir::features::PatternScaleCenter::Native(_))
                || matches!(
                    center,
                    cadmpeg_ir::features::PatternScaleCenter::Point(point)
                        if !finite_feature_point(*point)
                )
                || !final_factor.is_finite()
                || *final_factor <= 0.0
                || *count < 2
        }
        PatternKind::Composite { stages } => {
            stages.is_empty()
                || stages.iter().enumerate().any(|(index, stage)| {
                    stage.combination
                        != if index == 0 {
                            cadmpeg_ir::features::PatternStageCombination::Initialize
                        } else if matches!(*stage.pattern, PatternKind::Scale { .. }) {
                            cadmpeg_ir::features::PatternStageCombination::AlignedSlices
                        } else {
                            cadmpeg_ir::features::PatternStageCombination::CartesianProduct
                        }
                        || matches!(*stage.pattern, PatternKind::Composite { .. })
                        || pattern_is_incomplete(&stage.pattern)
                })
                || pattern_composition_is_incomplete(stages)
        }
    }
}

pub(crate) fn pattern_feature_is_incomplete(
    seeds: &[cadmpeg_ir::features::PatternSeed],
    pattern: &PatternKind,
    dependencies: &[cadmpeg_ir::features::FeatureId],
) -> bool {
    seeds.is_empty()
        || seeds.iter().any(|seed| match seed {
            cadmpeg_ir::features::PatternSeed::Feature(feature) => !dependencies.contains(feature),
            cadmpeg_ir::features::PatternSeed::Faces(faces) => face_selection_is_incomplete(faces),
            cadmpeg_ir::features::PatternSeed::Bodies(bodies) => {
                body_selection_is_incomplete(bodies)
            }
            cadmpeg_ir::features::PatternSeed::Occurrences(occurrences) => occurrences.is_empty(),
        })
        || seeds
            .iter()
            .enumerate()
            .any(|(index, seed)| seeds[..index].contains(seed))
        || pattern_is_incomplete(pattern)
}

pub(crate) fn radius_spec_is_incomplete(radius: &RadiusSpec) -> bool {
    match radius {
        RadiusSpec::Unresolved { .. } => true,
        RadiusSpec::Constant { radius } => !positive_feature_length(*radius),
        RadiusSpec::Chordal { chord_length } => !positive_feature_length(*chord_length),
        RadiusSpec::Asymmetric {
            offset_one,
            offset_two,
        } => !positive_feature_length(*offset_one) || !positive_feature_length(*offset_two),
        RadiusSpec::Variable { points } => {
            points.len() < 2
                || points.iter().any(|point| {
                    !point.parameter.is_finite()
                        || !(0.0..=1.0).contains(&point.parameter)
                        || !point.radius.0.is_finite()
                        || point.radius.0 < 0.0
                })
                || !points.iter().any(|point| point.radius.0 > 0.0)
                || points
                    .windows(2)
                    .any(|pair| pair[0].parameter >= pair[1].parameter)
        }
    }
}

pub(crate) fn positive_feature_length(length: Length) -> bool {
    length.0.is_finite() && length.0 > 0.0
}

pub(crate) fn valid_draft_angle(angle: cadmpeg_ir::features::Angle) -> bool {
    angle.0.is_finite() && angle.0.abs() < std::f64::consts::FRAC_PI_2
}

pub(crate) fn valid_feature_direction(direction: Vector3) -> bool {
    direction.norm().is_finite() && direction.norm() > 0.0
}

pub(crate) fn finite_feature_point(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}

pub(crate) fn valid_increasing_locations(locations: impl Iterator<Item = f64>) -> bool {
    let mut locations = locations;
    let Some(first) = locations.next() else {
        return false;
    };
    first == 0.0
        && locations
            .try_fold(first, |previous, location| {
                (location.is_finite() && location > previous).then_some(location)
            })
            .is_some()
}

pub(crate) fn pattern_composition_is_incomplete(
    stages: &[cadmpeg_ir::features::PatternStage],
) -> bool {
    let mut occurrences = None;
    stages.iter().enumerate().any(|(index, stage)| {
        let Some(stage_count) = pattern_occurrence_count(&stage.pattern) else {
            return false;
        };
        if stage_count == 0 {
            return true;
        }
        if index == 0 {
            occurrences = Some(stage_count);
            return false;
        }
        match stage.combination {
            cadmpeg_ir::features::PatternStageCombination::CartesianProduct => {
                if let Some(count) = occurrences {
                    occurrences = count.checked_mul(stage_count);
                    occurrences.is_none()
                } else {
                    false
                }
            }
            cadmpeg_ir::features::PatternStageCombination::AlignedSlices => {
                occurrences.is_some_and(|count| count % stage_count != 0)
            }
            cadmpeg_ir::features::PatternStageCombination::Initialize => true,
        }
    })
}

pub(crate) fn pattern_occurrence_count(pattern: &PatternKind) -> Option<usize> {
    match pattern {
        PatternKind::Linear { count, .. }
        | PatternKind::Circular { count, .. }
        | PatternKind::CurveDriven { count, .. }
        | PatternKind::Scale { count, .. } => usize::try_from(*count).ok(),
        PatternKind::LinearOffsets { offsets, .. } => Some(offsets.len()),
        PatternKind::CircularAngles { angles, .. } => Some(angles.len()),
        PatternKind::Mirror { .. } | PatternKind::MirrorReference { .. } => Some(2),
        PatternKind::Composite { stages } => {
            stages
                .iter()
                .try_fold(None::<usize>, |occurrences, stage| {
                    let stage_count = pattern_occurrence_count(&stage.pattern)?;
                    match stage.combination {
                        cadmpeg_ir::features::PatternStageCombination::Initialize => {
                            occurrences.is_none().then_some(Some(stage_count))
                        }
                        cadmpeg_ir::features::PatternStageCombination::CartesianProduct => {
                            Some(Some(occurrences?.checked_mul(stage_count)?))
                        }
                        cadmpeg_ir::features::PatternStageCombination::AlignedSlices => {
                            let occurrences = occurrences?;
                            (occurrences % stage_count == 0).then_some(Some(occurrences))
                        }
                    }
                })?
        }
        PatternKind::Unresolved { .. } => None,
    }
}

pub(crate) fn body_selection_is_incomplete(selection: &BodySelection) -> bool {
    match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => selection_ids_are_incomplete(bodies),
        BodySelection::Local { bodies, native } => {
            native.trim().is_empty()
                || selection_ids_are_incomplete(bodies)
                || bodies.iter().any(|body| body.trim().is_empty())
        }
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => true,
    }
}

pub(crate) fn body_selections_overlap(first: &BodySelection, second: &BodySelection) -> bool {
    match (first, second) {
        (
            BodySelection::Local { bodies: first, .. },
            BodySelection::Local { bodies: second, .. },
        ) => first.iter().any(|body| second.contains(body)),
        _ => explicit_body_ids(first).is_some_and(|first| {
            explicit_body_ids(second)
                .is_some_and(|second| first.iter().any(|body| second.contains(body)))
        }),
    }
}

pub(crate) fn explicit_body_ids(selection: &BodySelection) -> Option<&[BodyId]> {
    match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => Some(bodies),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => None,
    }
}

pub(crate) fn resolved_body_selection_len(selection: &BodySelection) -> Option<usize> {
    match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => Some(bodies.len()),
        BodySelection::Local { bodies, .. } => Some(bodies.len()),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => None,
    }
}

pub(crate) fn face_selection_is_incomplete(selection: &FaceSelection) -> bool {
    match selection {
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => true,
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
            selection_ids_are_incomplete(faces)
        }
    }
}

pub(crate) fn face_selections_overlap(first: &FaceSelection, second: &FaceSelection) -> bool {
    let first = match first {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces,
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => return false,
    };
    let second = match second {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces,
        FaceSelection::Unresolved
        | FaceSelection::Generated { .. }
        | FaceSelection::Native(_)
        | FaceSelection::Historical { .. }
        | FaceSelection::HistoricalPartial { .. } => return false,
    };
    first.iter().any(|face| second.contains(face))
}

pub(crate) fn edge_selection_is_incomplete(selection: &EdgeSelection) -> bool {
    match selection {
        EdgeSelection::Unresolved
        | EdgeSelection::Generated { .. }
        | EdgeSelection::Native(_)
        | EdgeSelection::Historical { .. }
        | EdgeSelection::HistoricalPartial { .. } => true,
        EdgeSelection::All => false,
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            selection_ids_are_incomplete(edges)
        }
    }
}

pub(crate) fn profile_ref_is_incomplete(profile: &ProfileRef) -> bool {
    match profile {
        ProfileRef::Unresolved(_)
        | ProfileRef::Native(_)
        | ProfileRef::SketchSelection { .. }
        | ProfileRef::SpatialSketchSelection { .. } => true,
        ProfileRef::Sketch(_) => false,
        ProfileRef::SketchEntities { entities, .. } => selection_ids_are_incomplete(entities),
        ProfileRef::SketchProfiles { profiles, .. }
        | ProfileRef::SpatialSketchProfiles { profiles, .. } => {
            selection_ids_are_incomplete(profiles)
        }
        ProfileRef::SketchRegions { regions, .. } => {
            regions.is_empty()
                || regions
                    .iter()
                    .enumerate()
                    .any(|(index, region)| regions[..index].contains(region))
        }
        ProfileRef::HistoricalFaces { faces, .. } => selection_ids_are_incomplete(faces),
        ProfileRef::Generated { curves, native } => {
            native.trim().is_empty()
                || curves.is_empty()
                || curves.iter().enumerate().any(|(index, curve)| {
                    curve.local_id.trim().is_empty() || curves[..index].contains(curve)
                })
        }
        ProfileRef::Feature(_) => false,
        ProfileRef::Faces(faces) => selection_ids_are_incomplete(faces),
    }
}

pub(crate) fn profile_dependency_is_incomplete(
    profile: &ProfileRef,
    dependencies: &[FeatureId],
) -> bool {
    match profile {
        ProfileRef::Feature(feature) => !dependencies.contains(feature),
        ProfileRef::Generated { curves, .. } => curves
            .iter()
            .any(|curve| !dependencies.contains(&curve.feature)),
        _ => false,
    }
}

pub(crate) fn loft_section_is_incomplete(section: &LoftSection) -> bool {
    match section {
        LoftSection::Profile(profile) => profile_ref_is_incomplete(profile),
        LoftSection::Point(LoftPointSection::Native(_)) => true,
        LoftSection::Point(LoftPointSection::Point(point)) => !finite_feature_point(*point),
        LoftSection::Point(LoftPointSection::Vertex(vertex)) => vertex.0.trim().is_empty(),
    }
}

pub(crate) fn selection_ids_are_incomplete<T: Ord>(ids: &[T]) -> bool {
    ids.is_empty() || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
}

pub(crate) fn path_ref_is_incomplete(path: &PathRef) -> bool {
    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. } => {
            true
        }
        PathRef::HistoricalEdges { edges, .. } => selection_ids_are_incomplete(edges),
        PathRef::Sketch(_) => false,
        PathRef::SketchCurves { curves, .. } => selection_ids_are_incomplete(curves),
        PathRef::SpatialSketchCurves { curves, .. } => selection_ids_are_incomplete(curves),
        PathRef::Edges(edges) => selection_ids_are_incomplete(edges),
        PathRef::Curves(curves) => selection_ids_are_incomplete(curves),
    }
}
