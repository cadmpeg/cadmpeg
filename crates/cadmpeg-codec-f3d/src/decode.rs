// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(
    test,
    allow(clippy::default_trait_access, clippy::field_reassign_with_default)
)]
//! Assemble a `.f3d` archive into a [`CadIr`] document and [`DecodeReport`].
//!
//! [`crate::container`] scans the ZIP, reads ASM headers, finds the history
//! boundary. This module resolves Design body-to-blob bindings, frames every
//! referenced B-rep with [`cadmpeg_asm::sab`], builds topology and geometry through
//! [`crate::brep`], then
//! adds design, sketch, history, ACT, and appearance data.
//!
//! A framing failure or a stream without decoded geometry produces a
//! metadata-only document. The report marks geometry and topology as blocking,
//! and retained source data remains available for native replay.

use crate::native::{F3dNative, F3D_NATIVE_VERSION};
use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationBuilder;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossKind, LossNote, LossTaxonomy, Severity};
use cadmpeg_ir::units::{Tolerances, Units};
use cadmpeg_ir::unknown::UnknownRecord;

use crate::brep::{self, Brep};
use crate::container::{self, BrepFacts, ContainerScan};
use crate::materials;
use cadmpeg_asm::{asm_header, sab};

fn container_only_dimension_parameters(
    native: &F3dNative,
) -> std::collections::HashSet<cadmpeg_ir::features::ParameterId> {
    let container_only = crate::design::dimensions::container_only_dimension_companions(
        &native.design_dimension_locus_pairs,
        &native.design_dimension_null_locus_pairs,
        &native.design_dimension_annotation_frames,
        &native.design_dimension_locus_groups,
        &native.design_dimension_recipe_records,
    );
    native
        .design_parameter_owners
        .iter()
        .filter_map(|owner| {
            let stream = crate::ids::native_stream(&owner.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            if !container_only.contains(&(stream.to_owned(), owner.companion_record_index)) {
                return None;
            }
            let mut parameters = native.design_parameters.iter().filter(|parameter| {
                crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM)
                    == stream
                    && parameter.record_index == owner.parameter_record_index
                    && parameter.kind == crate::records::DesignParameterKind::Dimension
            });
            let parameter = parameters.next()?;
            parameters
                .next()
                .is_none()
                .then(|| crate::ids::neutral_parameter_id(parameter))
        })
        .collect()
}

fn unresolved_dimension_companion_count(native: &F3dNative, ir: &CadIr) -> usize {
    use std::collections::{HashMap, HashSet};

    let parameters = native
        .design_parameters
        .iter()
        .map(|parameter| {
            (
                (
                    crate::ids::native_stream(&parameter.id).unwrap_or(crate::ids::DEFAULT_STREAM),
                    parameter.record_index,
                ),
                parameter.kind,
            )
        })
        .collect::<HashMap<_, _>>();
    let dimension_owners = native
        .design_parameter_owners
        .iter()
        .filter_map(|owner| {
            let stream = crate::ids::native_stream(&owner.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            (parameters.get(&(stream, owner.parameter_record_index))
                == Some(&crate::records::DesignParameterKind::Dimension))
            .then_some((stream, owner.record_index))
        })
        .collect::<HashSet<_>>();
    let mut typed = HashSet::new();
    for pair in &native.design_dimension_locus_pairs {
        typed.insert((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.companion_record_index,
        ));
        typed.insert((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.governing_companion_record_index,
        ));
    }
    for frame in &native.design_dimension_annotation_frames {
        typed.insert((
            crate::ids::native_stream(&frame.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            frame.governing_companion_record_index,
        ));
    }
    for group in &native.design_dimension_locus_groups {
        typed.insert((
            crate::ids::native_stream(&group.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            group.companion_record_index,
        ));
    }
    for pair in &native.design_dimension_null_locus_pairs {
        typed.insert((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.companion_record_index,
        ));
        typed.insert((
            crate::ids::native_stream(&pair.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            pair.governing_companion_record_index,
        ));
    }
    for record in &native.design_dimension_recipe_records {
        typed.insert((
            crate::ids::native_stream(&record.id).unwrap_or(crate::ids::DEFAULT_STREAM),
            record.companion_record_index,
        ));
    }
    for constraint in &ir.model.sketch_constraints {
        if !matches!(
            constraint.definition,
            cadmpeg_ir::sketches::SketchConstraintDefinition::Native { .. }
        ) {
            if let Some(native_ref) = &constraint.native_ref {
                if let Some(companion) = native
                    .design_parameter_companions
                    .iter()
                    .find(|companion| companion.id == *native_ref)
                {
                    typed.insert((
                        crate::ids::native_stream(native_ref).unwrap_or(crate::ids::DEFAULT_STREAM),
                        companion.record_index,
                    ));
                }
            }
        }
    }
    native
        .design_parameter_companions
        .iter()
        .filter(|companion| {
            let stream =
                crate::ids::native_stream(&companion.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            companion.payload_byte_length > 0
                && dimension_owners.contains(&(stream, companion.owner_record_index))
                && !typed.contains(&(stream, companion.record_index))
        })
        .count()
}

fn report_unresolved_dimension_companions(
    report: &mut DecodeReport,
    native: &F3dNative,
    ir: &CadIr,
) {
    let count = unresolved_dimension_companion_count(native, ir);
    if count != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{count} payload-bearing Design dimension companion(s) were retained without a typed locus frame."
            ),
            provenance: None,
        });
    }
}

fn report_unresolved_configuration_rules(
    report: &mut DecodeReport,
    native: &F3dNative,
    ir: &CadIr,
) {
    let count = crate::design::configurations::unresolved_configuration_member_count(
        &native.design_configurations,
    );
    if count != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::MetadataNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{count} Design configuration JSON member(s) were retained without assigned neutral configuration semantics."
            ),
            provenance: None,
        });
    }
    let count = crate::design::configurations::unresolved_configuration_rule_count(
        &native.design_configurations,
        &ir.model.configurations,
    );
    if count != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::MetadataNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{count} nonempty Design configuration rule(s) were retained without an unambiguous neutral activation target."
            ),
            provenance: None,
        });
    }
    let count = crate::design::configurations::unresolved_configuration_parameter_override_count(
        &ir.model.configurations,
    );
    if count != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::MetadataNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{count} Design configuration parameter override(s) were retained without an unambiguous neutral parameter identity."
            ),
            provenance: None,
        });
    }
    let count = crate::design::configurations::unresolved_configuration_suppressed_feature_count(
        &ir.model.configurations,
    );
    if count != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::MetadataNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{count} Design configuration feature suppression(s) were retained without an unambiguous neutral feature identity."
            ),
            provenance: None,
        });
    }
}

fn report_unretained_act_component_links(report: &mut DecodeReport, count: usize) {
    if count != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::AssemblyComponentsExternal),
            severity: Severity::Warning,
            message: format!(
                "{count} non-root ACT component link(s) remain source-only because their product-structure role is unresolved."
            ),
            provenance: None,
        });
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DesignProjectionGaps {
    unresolved_body_bindings: usize,
    incomplete_features: usize,
    native_reference_images: usize,
    native_decals: usize,
    unprojected_feature_scopes: usize,
    unprojected_parameters: usize,
    unresolved_parameter_owners: usize,
    untyped_parameter_units: usize,
    unresolved_expression_dependencies: usize,
    unprojected_history_dependencies: usize,
    ambiguous_history_dependencies: usize,
    native_sketch_relations: usize,
    native_dimensions: usize,
    unprojected_sketch_placements: usize,
    unprojected_sketch_points: usize,
    unprojected_sketch_curves: usize,
    unprojected_sketch_surfaces: usize,
    unprojected_sketch_texts: usize,
    unprojected_sketch_relations: usize,
    unprojected_dimensions: usize,
    profile_selections: usize,
    path_selections: usize,
    face_selections: usize,
    body_selections: usize,
    partially_resolved_face_members: usize,
    native_edge_selections: usize,
    partially_resolved_edge_members: usize,
    unresolved_edge_selections: usize,
    unrepaired_lost_edge_references: usize,
}

/// Returns whether a face selection supplies the operation's neutral faces.
///
/// Native or absent selections leave the definition incomplete.
fn face_selection_is_resolved(selection: &cadmpeg_ir::features::FaceSelection) -> bool {
    use cadmpeg_ir::features::FaceSelection;

    match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => !faces.is_empty(),
        FaceSelection::Historical { faces, .. } => !faces.is_empty(),
        FaceSelection::Generated { faces, .. } => !faces.is_empty(),
        FaceSelection::HistoricalPartial {
            faces, unresolved, ..
        } => !faces.is_empty() && unresolved.is_empty(),
        FaceSelection::Unresolved | FaceSelection::Native(_) => false,
    }
}

fn edge_selection_is_resolved(selection: &cadmpeg_ir::features::EdgeSelection) -> bool {
    use cadmpeg_ir::features::EdgeSelection;

    match selection {
        EdgeSelection::All => true,
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => !edges.is_empty(),
        EdgeSelection::Historical { edges, .. } => !edges.is_empty(),
        EdgeSelection::Generated { edges, .. } => !edges.is_empty(),
        EdgeSelection::HistoricalPartial {
            edges, unresolved, ..
        } => !edges.is_empty() && unresolved.is_empty(),
        EdgeSelection::Unresolved | EdgeSelection::Native(_) => false,
    }
}

fn datum_plane_reference_is_resolved(
    reference: &cadmpeg_ir::features::DatumPlaneReference,
) -> bool {
    match reference {
        cadmpeg_ir::features::DatumPlaneReference::Feature(_) => true,
        cadmpeg_ir::features::DatumPlaneReference::Face { face, .. } => {
            face_selection_is_resolved(face)
        }
    }
}

fn datum_point_construction_is_resolved(
    construction: &cadmpeg_ir::features::DatumPointConstruction,
) -> bool {
    use cadmpeg_ir::features::{DatumPointConstruction, VertexSelection};

    match construction {
        DatumPointConstruction::CircleCenter { edge } => edge_selection_is_resolved(edge),
        DatumPointConstruction::TwoEdgeIntersection { edges } => {
            edges.iter().all(edge_selection_is_resolved)
        }
        DatumPointConstruction::ThreePlaneIntersection { planes } => {
            planes.iter().all(datum_plane_reference_is_resolved)
        }
        DatumPointConstruction::Vertex { vertex } => matches!(
            vertex,
            VertexSelection::Generated { .. } | VertexSelection::Historical { .. }
        ),
        DatumPointConstruction::EdgePlaneIntersection { edge, plane } => {
            edge_selection_is_resolved(edge) && datum_plane_reference_is_resolved(plane)
        }
        DatumPointConstruction::DistanceOnEdge { edge, fraction } => {
            edge_selection_is_resolved(edge)
                && fraction.is_finite()
                && (0.0..=1.0).contains(fraction)
        }
    }
}

fn body_selection_is_resolved(selection: &cadmpeg_ir::features::BodySelection) -> bool {
    use cadmpeg_ir::features::BodySelection;

    match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => !bodies.is_empty(),
        BodySelection::Historical { bodies, .. }
        | BodySelection::HistoricalSet { bodies, .. }
        | BodySelection::HistoricalUnorderedSet { bodies, .. } => !bodies.is_empty(),
        BodySelection::Generated { bodies, .. } => !bodies.is_empty(),
        BodySelection::Local { bodies, .. } => !bodies.is_empty(),
        BodySelection::Unresolved | BodySelection::Native(_) | BodySelection::NativeSet(_) => false,
    }
}

fn profile_ref_is_resolved(profile: &cadmpeg_ir::features::ProfileRef) -> bool {
    use cadmpeg_ir::features::ProfileRef;

    match profile {
        ProfileRef::Unresolved(_)
        | ProfileRef::Native(_)
        | ProfileRef::SketchSelection { .. }
        | ProfileRef::SpatialSketchSelection { .. } => false,
        ProfileRef::Sketch(_) | ProfileRef::Feature(_) => true,
        ProfileRef::SketchProfiles { profiles, .. }
        | ProfileRef::SpatialSketchProfiles { profiles, .. } => !profiles.is_empty(),
        ProfileRef::SketchRegions { regions, .. } => !regions.is_empty(),
        ProfileRef::SketchEntities { entities, .. } => !entities.is_empty(),
        ProfileRef::HistoricalFaces { faces, .. } => !faces.is_empty(),
        ProfileRef::Generated { curves, .. } => !curves.is_empty(),
        ProfileRef::Faces(faces) => !faces.is_empty(),
    }
}

fn termination_is_resolved(termination: &cadmpeg_ir::features::Termination) -> bool {
    use cadmpeg_ir::features::{Termination, VertexSelection};

    match termination {
        Termination::Unresolved | Termination::Angle { .. } => false,
        Termination::ToFace { face, .. }
        | Termination::OffsetFromFace { face, .. }
        | Termination::ToShape { target: face } => face_selection_is_resolved(face),
        Termination::ToVertex { vertex } => matches!(
            vertex,
            VertexSelection::Generated { .. } | VertexSelection::Historical { .. }
        ),
        Termination::Blind { .. }
        | Termination::ThroughAll
        | Termination::ThroughNext
        | Termination::ToFirst
        | Termination::ToLast => true,
    }
}

fn loft_path_is_resolved(path: &cadmpeg_ir::features::PathRef) -> bool {
    use cadmpeg_ir::features::PathRef;

    match path {
        PathRef::Unresolved(_) | PathRef::Native(_) | PathRef::SpatialSketchSelection { .. } => {
            false
        }
        PathRef::Sketch(_) => true,
        PathRef::SketchCurves { curves, .. } => !curves.is_empty(),
        PathRef::SpatialSketchCurves { curves, .. } => !curves.is_empty(),
        PathRef::Edges(edges) => !edges.is_empty(),
        PathRef::Curves(curves) => !curves.is_empty(),
        PathRef::HistoricalEdges { edges, .. } => !edges.is_empty(),
    }
}

fn feature_definition_is_incomplete(definition: &cadmpeg_ir::features::FeatureDefinition) -> bool {
    use cadmpeg_ir::features::{FeatureDefinition, PatternKind, SketchSpace};

    match definition {
        FeatureDefinition::Native { kind, .. } => !matches!(kind.as_str(), "Canvas" | "Decal"),
        FeatureDefinition::ReferenceImage { .. }
        | FeatureDefinition::DatumPrincipalPlane { .. } => false,
        FeatureDefinition::MeshImport { tessellations } => tessellations.is_empty(),
        FeatureDefinition::Decal { faces, .. } => !face_selection_is_resolved(faces),
        FeatureDefinition::CosmeticThread {
            face,
            diameter,
            extent,
        } => !face_selection_is_resolved(face) || diameter.is_none() || extent.is_none(),
        FeatureDefinition::DatumPlaneUnresolved
        | FeatureDefinition::DatumPointUnresolved
        | FeatureDefinition::DatumCoordinateSystemUnresolved
        | FeatureDefinition::LoftUnresolved
        | FeatureDefinition::FreeformSurfaceUnresolved
        | FeatureDefinition::BoundarySurfaceUnresolved
        | FeatureDefinition::DraftUnresolved => true,
        // F3D WorkPlane projection currently retains only the solved frame.
        // Its construction rule and operands are required for replay.
        FeatureDefinition::DatumPlane { .. } => true,
        FeatureDefinition::DatumThreePointPlane { points, .. } => !points.iter().all(|point| {
            matches!(
                point,
                cadmpeg_ir::features::VertexSelection::Generated { .. }
                    | cadmpeg_ir::features::VertexSelection::Historical { .. }
            )
        }),
        FeatureDefinition::DatumOffsetPlane { reference, .. } => reference
            .as_ref()
            .is_none_or(|reference| !datum_plane_reference_is_resolved(reference)),
        FeatureDefinition::Extrude {
            profile,
            start,
            extent,
            ..
        } => {
            use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeStart};

            let start_is_resolved = match start {
                ExtrudeStart::Unresolved => false,
                ExtrudeStart::FromFace { face, .. } => face_selection_is_resolved(face),
                ExtrudeStart::ProfilePlane | ExtrudeStart::OffsetProfilePlane { .. } => true,
            };
            let extent_is_resolved = match extent {
                ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                    termination_is_resolved(&side.termination)
                }
                ExtrudeExtent::TwoSided { first, second } => {
                    termination_is_resolved(&first.termination)
                        && termination_is_resolved(&second.termination)
                }
            };
            !profile_ref_is_resolved(profile) || !start_is_resolved || !extent_is_resolved
        }
        FeatureDefinition::Hole {
            profile,
            face,
            position,
            direction,
            placements,
            kind,
            diameter,
            extent,
            ..
        } => {
            use cadmpeg_ir::features::{HoleKind, HolePlacement};

            let support_is_resolved = profile.as_ref().is_some_and(profile_ref_is_resolved)
                || face.as_ref().is_some_and(face_selection_is_resolved);
            let shared_placement_is_resolved = position.is_some()
                && direction
                    .as_ref()
                    .is_some_and(|direction| direction.unit().is_some());
            let placements_are_resolved = !placements.is_empty()
                && placements.iter().all(|placement| match placement {
                    HolePlacement::Directed { direction, .. } => direction.unit().is_some(),
                    HolePlacement::Axis { axis, .. } => axis.unit().is_some(),
                });

            !support_is_resolved
                || (!shared_placement_is_resolved && !placements_are_resolved)
                || matches!(kind, HoleKind::Unresolved { .. })
                || diameter.is_none()
                || extent
                    .as_ref()
                    .is_none_or(|extent| !termination_is_resolved(extent))
        }
        FeatureDefinition::Coil {
            construction,
            result,
        } => {
            use cadmpeg_ir::features::{BooleanOp, CoilPlacement, CoilResult};

            matches!(construction.placement, CoilPlacement::Native { .. })
                || match result {
                    CoilResult::NewBody => false,
                    CoilResult::Boolean { operation, targets } => {
                        matches!(operation, BooleanOp::Unresolved | BooleanOp::NewBody)
                            || !body_selection_is_resolved(targets)
                    }
                }
        }
        // The draft angle remains available when face recipes fail, but replay
        // also requires resolved selections and the material-side convention.
        FeatureDefinition::Draft {
            faces,
            neutral_plane,
            parting_tool,
            pull_direction,
            pull_plane,
            angle,
            outward,
            ..
        } => {
            angle.is_none()
                || outward.is_none()
                || !face_selection_is_resolved(faces)
                || match parting_tool {
                    Some(parting_tool) => {
                        !face_selection_is_resolved(parting_tool)
                            || pull_direction.is_none()
                            || pull_direction.is_some_and(|direction| direction.unit().is_none())
                            || pull_plane.is_none()
                    }
                    None => !face_selection_is_resolved(neutral_plane),
                }
        }
        FeatureDefinition::Sketch { space, sketch } => {
            *space == SketchSpace::Unresolved || sketch.is_none()
        }
        FeatureDefinition::DatumPoint { construction, .. } => construction
            .as_deref()
            .is_none_or(|construction| !datum_point_construction_is_resolved(construction)),
        FeatureDefinition::SpatialSketch { sketch } => sketch.is_none(),
        FeatureDefinition::SketchBlockDefinition { sketch } => sketch.is_none(),
        FeatureDefinition::SketchBlockInstance { block, .. } => block.is_none(),
        FeatureDefinition::Form { cages } => cages.is_empty(),
        FeatureDefinition::Block {
            dimensions,
            placement,
            op,
        } => {
            dimensions.is_none()
                || placement.is_none()
                || *op == cadmpeg_ir::features::BooleanOp::Unresolved
        }
        FeatureDefinition::Primitive { op, .. } => {
            *op == cadmpeg_ir::features::BooleanOp::Unresolved
        }
        FeatureDefinition::Pattern { seeds, pattern } => {
            seeds.is_empty() || matches!(pattern, PatternKind::Unresolved { .. })
        }
        FeatureDefinition::Fillet { groups } => {
            groups.is_empty()
                || groups
                    .iter()
                    .any(|group| !edge_selection_is_resolved(&group.edges))
        }
        FeatureDefinition::DeleteFace { faces, .. } => !face_selection_is_resolved(faces),
        FeatureDefinition::OffsetSurface {
            faces, distance, ..
        } => !face_selection_is_resolved(faces) || distance.is_none(),
        FeatureDefinition::SheetMetalBaseFlange { profile, .. } => {
            !profile_ref_is_resolved(profile)
        }
        FeatureDefinition::SheetMetalEdgeFlange { edges, height, .. } => {
            !edge_selection_is_resolved(edges)
                || matches!(
                    height,
                    cadmpeg_ir::features::SheetMetalFlangeHeight::ToObject {
                        target: cadmpeg_ir::features::SheetMetalFlangeHeightTarget::Native(_),
                        ..
                    }
                )
        }
        FeatureDefinition::SheetMetalHem {
            edges,
            form,
            direction,
            ..
        } => {
            !edge_selection_is_resolved(edges)
                || matches!(
                    direction,
                    cadmpeg_ir::features::SheetMetalHemDirection::Unresolved
                )
                || matches!(
                    form,
                    cadmpeg_ir::features::SheetMetalHemForm::GapLength { .. }
                )
        }
        FeatureDefinition::SplitFace { targets, tool } => {
            !face_selection_is_resolved(targets)
                || match tool {
                    cadmpeg_ir::features::SplitFaceTool::Plane { .. }
                    | cadmpeg_ir::features::SplitFaceTool::Planes { .. } => false,
                    cadmpeg_ir::features::SplitFaceTool::Path(path) => matches!(
                        path,
                        cadmpeg_ir::features::PathRef::Native(_)
                            | cadmpeg_ir::features::PathRef::Unresolved(_)
                    ),
                }
        }
        FeatureDefinition::Loft {
            sections,
            guides,
            centerline,
            ..
        } => {
            sections.len() < 2
                || sections.iter().any(|section| match section {
                    cadmpeg_ir::features::LoftSection::Profile(profile) => {
                        !profile_ref_is_resolved(profile)
                    }
                    cadmpeg_ir::features::LoftSection::Point(
                        cadmpeg_ir::features::LoftPointSection::Native(_),
                    ) => true,
                    cadmpeg_ir::features::LoftSection::Point(
                        cadmpeg_ir::features::LoftPointSection::Point(_)
                        | cadmpeg_ir::features::LoftPointSection::Vertex(_),
                    ) => false,
                })
                || guides.iter().any(|path| !loft_path_is_resolved(path))
                || centerline
                    .as_ref()
                    .is_some_and(|path| !loft_path_is_resolved(path))
        }
        FeatureDefinition::FilledSurface {
            boundary,
            support_faces,
            continuity,
            boundary_continuities,
            merge_result,
        } => {
            use cadmpeg_ir::features::{SurfaceBoundary, SurfaceContinuity};

            let boundary_is_resolved = match boundary {
                SurfaceBoundary::Edges(edges) => edge_selection_is_resolved(edges),
                SurfaceBoundary::Path(path) => loft_path_is_resolved(path),
            };
            let continuity_is_resolved = continuity.is_some() || !boundary_continuities.is_empty();
            let support_is_required =
                continuity
                    .iter()
                    .chain(boundary_continuities)
                    .any(|continuity| {
                        matches!(
                            continuity,
                            SurfaceContinuity::Tangent | SurfaceContinuity::Curvature
                        )
                    });

            !boundary_is_resolved
                || !continuity_is_resolved
                || (support_is_required && !face_selection_is_resolved(support_faces))
                || merge_result.is_none()
        }
        FeatureDefinition::FullRoundFillet { groups } => {
            groups.is_empty()
                || groups.iter().any(|group| {
                    !face_selection_is_resolved(&group.center_faces)
                        || matches!(
                            group.side_one_faces,
                            cadmpeg_ir::features::FullRoundSideSelection::Unresolved
                        )
                        || matches!(
                            group.side_two_faces,
                            cadmpeg_ir::features::FullRoundSideSelection::Unresolved
                        )
                        || matches!(
                            group.side_one_faces,
                            cadmpeg_ir::features::FullRoundSideSelection::Explicit(ref selection)
                                if !face_selection_is_resolved(selection)
                        )
                        || matches!(
                            group.side_two_faces,
                            cadmpeg_ir::features::FullRoundSideSelection::Explicit(ref selection)
                                if !face_selection_is_resolved(selection)
                        )
                })
        }
        // A typed family is not replayable until this match states and checks
        // its complete construction invariants.
        _ => true,
    }
}

fn incomplete_feature_families(ir: &CadIr) -> std::collections::BTreeMap<&str, usize> {
    let mut families = std::collections::BTreeMap::new();
    for feature in &ir.model.features {
        if !feature_definition_is_incomplete(&feature.definition) {
            continue;
        }
        let family = feature.source_tag.as_deref().unwrap_or_else(|| {
            if let cadmpeg_ir::features::FeatureDefinition::Native { kind, .. } =
                &feature.definition
            {
                kind
            } else {
                "<missing source tag>"
            }
        });
        *families.entry(family).or_default() += 1;
    }
    families
}

fn design_projection_gaps(ir: &CadIr, native: &F3dNative) -> DesignProjectionGaps {
    use cadmpeg_ir::features::{
        BodySelection, EdgeSelection, ExtrudeExtent, ExtrudeStart, FaceSelection, Termination,
    };
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, ProfileRef};
    use cadmpeg_ir::sketches::SketchConstraintDefinition;
    use std::collections::{HashMap, HashSet};

    let source_lost_edge_reference_ids = native
        .lost_edge_references
        .iter()
        .map(|reference| reference.id.as_str())
        .collect::<HashSet<_>>();
    let mut complete_edge_selection_native_ids = HashSet::new();
    let projected_constraint_refs = ir
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketch_constraints
                .iter()
                .filter_map(|constraint| constraint.native_ref.as_deref()),
        )
        .collect::<HashSet<_>>();
    let projected_sketch_refs = ir
        .model
        .sketches
        .iter()
        .filter_map(|sketch| sketch.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketches
                .iter()
                .filter_map(|sketch| sketch.native_ref.as_deref()),
        )
        .collect::<HashSet<_>>();
    let projected_sketch_entity_refs = ir
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| entity.native_ref.as_deref())
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .collect::<HashSet<_>>();
    let projected_feature_refs = ir
        .model
        .features
        .iter()
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let projected_parameter_refs = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| parameter.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let projected_features = ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
        .collect::<HashMap<_, _>>();
    let mut unprojected_history_dependencies = 0;
    let mut ambiguous_history_dependencies = 0;
    let scope_history = crate::design::feature_project::ScopeHistoryGraph::new(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    for scope in &native.design_parameter_scopes {
        let Some(feature) = projected_features.get(scope.id.as_str()) else {
            continue;
        };
        let predecessor_scope = match scope_history.predecessor(scope, |candidate| {
            projected_features.contains_key(candidate.id.as_str())
        }) {
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Scope(predecessor)) => {
                predecessor
            }
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Ambiguous) | Err(_) => {
                ambiguous_history_dependencies += 1;
                continue;
            }
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::None) => {
                continue;
            }
        };
        let Some(predecessor) = projected_features.get(predecessor_scope.id.as_str()) else {
            unprojected_history_dependencies += 1;
            continue;
        };
        if predecessor.id != feature.id && !feature.dependencies.contains(&predecessor.id) {
            unprojected_history_dependencies += 1;
        }
    }
    let projected_dimension_parameters =
        ir.model
            .sketch_constraints
            .iter()
            .flat_map(|constraint| {
                crate::design::dimensions::constraint_parameters(&constraint.definition)
            })
            .chain(
                ir.model.spatial_sketch_constraints.iter().filter_map(
                    |constraint| match &constraint.definition {
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Native {
                            parameter,
                            ..
                        } => parameter.as_ref(),
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::PointDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::ParallelLineDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::RepeatedParallelLineDistance {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::LineLength {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::RepeatedLineLength {
                            parameter,
                            ..
                        }
                        | cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::ParallelLineSetDistance {
                            parameter,
                            ..
                        } => Some(parameter),
                        cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Offset {
                            parameter,
                            ..
                        } => parameter.as_ref(),
                        _ => None,
                    },
                ),
            )
            .cloned()
            .collect::<HashSet<_>>();

    let native_sketch_relation_ids = native
        .sketch_relations
        .iter()
        .map(|relation| relation.id.as_str())
        .collect::<HashSet<_>>();
    let mut native_sketch_relations = 0;
    let mut native_dimensions = 0;
    for constraint in &ir.model.sketch_constraints {
        if !matches!(
            constraint.definition,
            SketchConstraintDefinition::Native { .. }
        ) {
            continue;
        }
        if constraint
            .native_ref
            .as_deref()
            .is_some_and(|native_ref| native_sketch_relation_ids.contains(native_ref))
        {
            native_sketch_relations += 1;
        } else {
            native_dimensions += 1;
        }
    }
    for constraint in &ir.model.spatial_sketch_constraints {
        if !matches!(
            constraint.definition,
            cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::Native { .. }
        ) {
            continue;
        }
        if constraint
            .native_ref
            .as_deref()
            .is_some_and(|native_ref| native_sketch_relation_ids.contains(native_ref))
        {
            native_sketch_relations += 1;
        } else {
            native_dimensions += 1;
        }
    }

    let authored_scopes = crate::design::feature_project::authored_scope_ordinals_per_stream(
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
    )
    .ok();
    let mut gaps = DesignProjectionGaps {
        unresolved_body_bindings: native
            .design_body_bindings
            .iter()
            .filter(|binding| binding.body.is_none())
            .count(),
        unprojected_history_dependencies,
        ambiguous_history_dependencies,
        unprojected_feature_scopes: native
            .design_parameter_scopes
            .iter()
            .filter(|scope| {
                let authored = authored_scopes.as_ref().map_or_else(
                    || {
                        scope
                            .assembly_alignment
                            .as_ref()
                            .and_then(|alignment| alignment.joint_origin_scope_record_index)
                            .is_none()
                    },
                    |ordinals| {
                        let stream = crate::ids::native_stream(&scope.id)
                            .unwrap_or(crate::ids::DEFAULT_STREAM);
                        ordinals.contains_key(&(stream, scope.record_index))
                    },
                );
                authored && !projected_feature_refs.contains(scope.id.as_str())
            })
            .count(),
        unprojected_parameters: native
            .design_parameters
            .iter()
            .filter(|parameter| !projected_parameter_refs.contains(parameter.id.as_str()))
            .count(),
        unresolved_parameter_owners: native
            .design_parameters
            .iter()
            .filter(|parameter| {
                let Some(owner_record_index) = parameter.owner_record_index else {
                    return false;
                };
                let Some(stream) = crate::ids::native_stream(&parameter.id) else {
                    return true;
                };
                !native.design_parameter_owners.iter().any(|owner| {
                    crate::ids::native_stream(&owner.id) == Some(stream)
                        && owner.record_index == owner_record_index
                        && native.design_parameter_scopes.iter().any(|scope| {
                            crate::ids::native_stream(&scope.id) == Some(stream)
                                && scope.record_index == owner.scope_record_index
                        })
                })
            })
            .count(),
        untyped_parameter_units: crate::design::feature_project::untyped_parameter_unit_count(
            &native.design_parameters,
        ),
        unresolved_expression_dependencies:
            crate::design::dimensions::unresolved_parameter_expression_dependency_count(
                &native.design_parameters,
                &ir.model.parameters,
            ),
        native_sketch_relations,
        native_dimensions,
        unprojected_sketch_placements: native
            .design_sketch_placements
            .iter()
            .filter(|placement| !projected_sketch_refs.contains(placement.id.as_str()))
            .count(),
        unprojected_sketch_points: native
            .sketch_points
            .iter()
            .filter(|point| {
                point.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(point.id.as_str())
            })
            .count(),
        unprojected_sketch_curves: native
            .sketch_curve_identities
            .iter()
            .filter(|curve| {
                curve.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(curve.id.as_str())
            })
            .count(),
        unprojected_sketch_surfaces: native
            .sketch_surfaces
            .iter()
            .filter(|surface| {
                surface.owner_reference.is_some()
                    && !projected_sketch_entity_refs.contains(surface.id.as_str())
            })
            .count(),
        unprojected_sketch_texts: native
            .sketch_texts
            .iter()
            .filter(|text| !projected_sketch_entity_refs.contains(text.id.as_str()))
            .count(),
        unprojected_sketch_relations: native
            .sketch_relations
            .iter()
            .filter(|relation| !projected_constraint_refs.contains(relation.id.as_str()))
            .count(),
        unprojected_dimensions: {
            let container_only = container_only_dimension_parameters(native);
            let relation_bearing_companions = native
                .design_parameter_companions
                .iter()
                .filter(|companion| companion.payload_byte_length > 0)
                .filter_map(|companion| {
                    Some((
                        crate::ids::native_stream(&companion.id)?.to_owned(),
                        companion.record_index,
                    ))
                })
                .chain(
                    native
                        .design_dimension_locus_pairs
                        .iter()
                        .filter_map(|pair| {
                            Some((
                                crate::ids::native_stream(&pair.id)?.to_owned(),
                                pair.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    native
                        .design_dimension_null_locus_pairs
                        .iter()
                        .filter_map(|pair| {
                            Some((
                                crate::ids::native_stream(&pair.id)?.to_owned(),
                                pair.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    native
                        .design_dimension_annotation_frames
                        .iter()
                        .filter_map(|frame| {
                            Some((
                                crate::ids::native_stream(&frame.id)?.to_owned(),
                                frame.governing_companion_record_index,
                            ))
                        }),
                )
                .chain(
                    native
                        .design_dimension_locus_groups
                        .iter()
                        .filter_map(|group| {
                            Some((
                                crate::ids::native_stream(&group.id)?.to_owned(),
                                group.companion_record_index,
                            ))
                        }),
                )
                .chain(
                    native
                        .design_dimension_recipe_records
                        .iter()
                        .filter_map(|record| {
                            Some((
                                crate::ids::native_stream(&record.id)?.to_owned(),
                                record.companion_record_index,
                            ))
                        }),
                )
                .collect::<HashSet<_>>();
            let relation_bearing_parameters = native
                .design_parameter_owners
                .iter()
                .filter_map(|owner| {
                    let stream = crate::ids::native_stream(&owner.id)?;
                    relation_bearing_companions
                        .contains(&(stream.to_owned(), owner.companion_record_index))
                        .then_some((stream, owner.parameter_record_index))
                })
                .collect::<HashSet<_>>();
            native
                .design_parameters
                .iter()
                .filter(|parameter| {
                    let stream = crate::ids::native_stream(&parameter.id)
                        .unwrap_or(crate::ids::DEFAULT_STREAM);
                    parameter.kind == crate::records::DesignParameterKind::Dimension
                        && relation_bearing_parameters.contains(&(stream, parameter.record_index))
                        && !projected_dimension_parameters
                            .contains(&crate::ids::neutral_parameter_id(parameter))
                        && !container_only.contains(&crate::ids::neutral_parameter_id(parameter))
                })
                .count()
        },
        ..DesignProjectionGaps::default()
    };
    let mut edge_selection = |selection: &EdgeSelection| match selection {
        EdgeSelection::Native(_) => gaps.native_edge_selections += 1,
        EdgeSelection::Unresolved => gaps.unresolved_edge_selections += 1,
        EdgeSelection::HistoricalPartial { unresolved, .. } => {
            gaps.partially_resolved_edge_members += unresolved
                .iter()
                .filter(|id| !source_lost_edge_reference_ids.contains(id.as_str()))
                .count();
        }
        EdgeSelection::Resolved { native, .. }
        | EdgeSelection::Generated { native, .. }
        | EdgeSelection::Historical { native, .. } => {
            complete_edge_selection_native_ids.insert(native.clone());
        }
        EdgeSelection::All | EdgeSelection::Edges(_) => {}
    };
    let mut face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Native(_) | FaceSelection::Unresolved => gaps.face_selections += 1,
        FaceSelection::HistoricalPartial { unresolved, .. } => {
            gaps.partially_resolved_face_members += unresolved.len();
        }
        FaceSelection::Faces(_)
        | FaceSelection::Resolved { .. }
        | FaceSelection::Generated { .. }
        | FaceSelection::Historical { .. } => {}
    };
    let native_body_selection_count = |selection: &BodySelection| match selection {
        BodySelection::Native(_) | BodySelection::Unresolved => 1,
        BodySelection::NativeSet(members) => members.len(),
        BodySelection::Bodies(_)
        | BodySelection::Resolved { .. }
        | BodySelection::ResolvedSet { .. }
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. } => 0,
    };
    for feature in &ir.model.features {
        gaps.incomplete_features +=
            usize::from(feature_definition_is_incomplete(&feature.definition));
        gaps.native_reference_images += usize::from(matches!(
            &feature.definition,
            FeatureDefinition::Native { kind, .. } if kind == "Canvas"
        ));
        gaps.native_decals += usize::from(matches!(
            &feature.definition,
            FeatureDefinition::Native { kind, .. } if kind == "Decal"
        ));
        match &feature.definition {
            FeatureDefinition::BaseFeature { bodies }
            | FeatureDefinition::InsertBodies { bodies } => {
                gaps.body_selections += native_body_selection_count(bodies);
            }
            FeatureDefinition::Combine { target, tools, .. } => {
                gaps.body_selections +=
                    native_body_selection_count(target) + native_body_selection_count(tools);
            }
            FeatureDefinition::Coil {
                result: cadmpeg_ir::features::CoilResult::Boolean { targets, .. },
                ..
            } => gaps.body_selections += native_body_selection_count(targets),
            FeatureDefinition::Extrude {
                profile,
                start,
                extent,
                ..
            } => {
                if matches!(
                    profile,
                    ProfileRef::Native(_) | ProfileRef::SketchSelection { .. }
                ) {
                    gaps.profile_selections += 1;
                }
                if let ExtrudeStart::FromFace { face, .. } = start {
                    face_selection(face);
                }
                let sides = match extent {
                    ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                        vec![side]
                    }
                    ExtrudeExtent::TwoSided { first, second } => vec![first, second],
                };
                for side in sides {
                    if let Termination::ToFace { face, .. } = &side.termination {
                        face_selection(face);
                    }
                }
            }
            FeatureDefinition::Fillet { groups } => {
                for group in groups {
                    edge_selection(&group.edges);
                }
            }
            FeatureDefinition::FullRoundFillet { groups } => {
                for group in groups {
                    face_selection(&group.center_faces);
                    for side in [&group.side_one_faces, &group.side_two_faces] {
                        if let cadmpeg_ir::features::FullRoundSideSelection::Explicit(selection) =
                            side
                        {
                            face_selection(selection);
                        }
                    }
                }
            }
            FeatureDefinition::Chamfer { groups, .. } => {
                for group in groups {
                    edge_selection(&group.edges);
                }
            }
            FeatureDefinition::Sweep {
                section,
                sections,
                path,
                guide_rail,
                ..
            } => {
                for section in std::iter::once(section).chain(sections) {
                    if matches!(
                        section,
                        cadmpeg_ir::features::SweepSection::Unresolved(Some(_))
                    ) || section.referenced_profile().is_some_and(|profile| {
                        matches!(
                            profile,
                            ProfileRef::Native(_)
                                | ProfileRef::Unresolved(_)
                                | ProfileRef::SketchSelection { .. }
                                | ProfileRef::SpatialSketchSelection { .. }
                        )
                    }) {
                        gaps.profile_selections += 1;
                    }
                }
                if path.as_ref().is_some_and(|path| {
                    matches!(
                        path,
                        PathRef::Native(_)
                            | PathRef::Unresolved(_)
                            | PathRef::SpatialSketchSelection { .. }
                    )
                }) {
                    gaps.path_selections += 1;
                }
                if guide_rail.as_ref().is_some_and(|guide| {
                    matches!(
                        &guide.path,
                        PathRef::Native(_)
                            | PathRef::Unresolved(_)
                            | PathRef::SpatialSketchSelection { .. }
                    )
                }) {
                    gaps.path_selections += 1;
                }
            }
            FeatureDefinition::DatumPoint {
                construction: Some(construction),
                ..
            } => {
                use cadmpeg_ir::features::{DatumPlaneReference, DatumPointConstruction};

                let mut plane = |reference: &DatumPlaneReference| {
                    if let DatumPlaneReference::Face { face, .. } = reference {
                        face_selection(face);
                    }
                };
                match construction.as_ref() {
                    DatumPointConstruction::CircleCenter { edge }
                    | DatumPointConstruction::DistanceOnEdge { edge, .. } => edge_selection(edge),
                    DatumPointConstruction::TwoEdgeIntersection { edges } => {
                        edges.iter().for_each(&mut edge_selection);
                    }
                    DatumPointConstruction::ThreePlaneIntersection { planes } => {
                        planes.iter().for_each(&mut plane);
                    }
                    DatumPointConstruction::Vertex { .. } => {}
                    DatumPointConstruction::EdgePlaneIntersection {
                        edge,
                        plane: reference,
                    } => {
                        edge_selection(edge);
                        plane(reference);
                    }
                }
            }
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                ..
            } => {
                match boundary {
                    cadmpeg_ir::features::SurfaceBoundary::Edges(edges) => edge_selection(edges),
                    cadmpeg_ir::features::SurfaceBoundary::Path(path) => {
                        gaps.path_selections += usize::from(!loft_path_is_resolved(path));
                    }
                }
                face_selection(support_faces);
            }
            FeatureDefinition::Loft {
                sections,
                guides,
                centerline,
                ..
            } => {
                gaps.profile_selections += sections
                    .iter()
                    .filter(|section| {
                        matches!(
                            section,
                            cadmpeg_ir::features::LoftSection::Profile(profile)
                                if !profile_ref_is_resolved(profile)
                        )
                    })
                    .count();
                gaps.path_selections += guides
                    .iter()
                    .chain(centerline.iter())
                    .filter(|path| !loft_path_is_resolved(path))
                    .count();
            }
            FeatureDefinition::Shell {
                bodies,
                removed_faces,
                ..
            } => {
                if let Some(bodies) = bodies {
                    gaps.body_selections += native_body_selection_count(bodies);
                }
                face_selection(removed_faces);
            }
            FeatureDefinition::CosmeticThread { face, .. } => face_selection(face),
            FeatureDefinition::Decal { faces, .. } => face_selection(faces),
            FeatureDefinition::DeleteFace { faces, .. }
            | FeatureDefinition::OffsetSurface { faces, .. } => face_selection(faces),
            FeatureDefinition::SheetMetalBaseFlange { profile, .. } => {
                gaps.profile_selections += usize::from(!profile_ref_is_resolved(profile));
            }
            FeatureDefinition::SheetMetalEdgeFlange { edges, .. }
            | FeatureDefinition::SheetMetalHem { edges, .. } => edge_selection(edges),
            FeatureDefinition::MoveFace { faces, .. } => face_selection(faces),
            _ => {}
        }
    }
    let repaired_lost_edge_reference_ids = native
        .design_construction_operand_groups
        .iter()
        .filter(|group| complete_edge_selection_native_ids.contains(group.id.as_str()))
        .flat_map(|group| group.lost_edge_references.iter().map(String::as_str))
        .collect::<HashSet<_>>();
    gaps.unrepaired_lost_edge_references = native
        .lost_edge_references
        .iter()
        .filter(|reference| !repaired_lost_edge_reference_ids.contains(reference.id.as_str()))
        .count();
    gaps
}

fn report_design_projection_gaps(report: &mut DecodeReport, ir: &CadIr, native: &F3dNative) {
    let gaps = design_projection_gaps(ir, native);
    let incomplete_families = incomplete_feature_families(ir);
    let history_budget_skips = native
        .asm_histories
        .iter()
        .filter(|history| history.record_table_binding_budget_exceeded)
        .count();
    if history_budget_skips != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Error,
            message: format!(
                "{history_budget_skips} ASM history stream(s) retain no historical topology because their binding work exceeded the decoder safety budget."
            ),
            provenance: None,
        });
    }
    for error in native
        .asm_histories
        .iter()
        .flat_map(|history| &history.states)
        .flat_map(|state| &state.records)
        .filter_map(|record| record.framing_error.as_deref())
    {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Error,
            message: format!(
                "An ASM history span remains opaque because record framing failed: {error}."
            ),
            provenance: None,
        });
    }
    if gaps.unresolved_body_bindings != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::ReferenceGraphNotClosed),
            severity: Severity::Warning,
            message: format!(
                "{} Design body-map pair(s) do not resolve to a body in the named BREP blob.",
                gaps.unresolved_body_bindings
            ),
            provenance: None,
        });
    }
    if gaps.native_reference_images != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::AttributesNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{} reference-image timeline object(s) retain native Canvas records because no neutral image-plane binding was resolved.",
                gaps.native_reference_images
            ),
            provenance: None,
        });
    }
    if gaps.native_decals != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::AttributesNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{} decal timeline object(s) retain native image and mapping records because no neutral decal binding was resolved.",
                gaps.native_decals
            ),
            provenance: None,
        });
    }
    if gaps.unrepaired_lost_edge_references != 0 {
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::AttributesNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{} source parametric edge reference(s) were marked EDGE_REFERENCE_LOST and have no independent complete selection proof.",
                gaps.unrepaired_lost_edge_references
            ),
            provenance: None,
        });
    }
    let mut push = |count: usize, message: String| {
        if count != 0 {
            report.losses.push(LossNote {
                code: LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
                severity: Severity::Warning,
                message,
                provenance: None,
            });
        }
    };
    push(
        gaps.incomplete_features,
        format!(
            "{} feature scope(s) have no complete neutral feature definition: {}.",
            gaps.incomplete_features,
            incomplete_families
                .iter()
                .map(|(family, count)| format!("{family}={count}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    push(
        gaps.unprojected_feature_scopes,
        format!(
            "{} decoded feature scope(s) have no neutral construction-history feature.",
            gaps.unprojected_feature_scopes
        ),
    );
    push(
        gaps.unprojected_parameters,
        format!(
            "{} decoded Design parameter(s) have no neutral parameter.",
            gaps.unprojected_parameters
        ),
    );
    push(
        gaps.unresolved_parameter_owners,
        format!(
            "{} decoded Design parameter owner binding(s) have no recognized feature scope.",
            gaps.unresolved_parameter_owners
        ),
    );
    push(
        gaps.untyped_parameter_units,
        format!(
            "{} decoded Design parameter(s) retain unit tokens without a settled neutral quantity kind.",
            gaps.untyped_parameter_units
        ),
    );
    push(
        gaps.unresolved_expression_dependencies,
        format!(
            "{} decoded parameter expression symbol(s) name same-stream parameters without a neutral dependency edge.",
            gaps.unresolved_expression_dependencies
        ),
    );
    push(
        gaps.unprojected_history_dependencies,
        format!(
            "{} feature history-state dependency link(s) were not projected into neutral construction history.",
            gaps.unprojected_history_dependencies
        ),
    );
    push(
        gaps.ambiguous_history_dependencies,
        format!(
            "{} feature history-state dependency link(s) have multiple source scopes for the preceding state identity.",
            gaps.ambiguous_history_dependencies
        ),
    );
    push(
        gaps.native_sketch_relations,
        format!(
            "{} sketch relation(s) retain native operands because no unique neutral relation was resolved.",
            gaps.native_sketch_relations
        ),
    );
    push(
        gaps.native_dimensions,
        format!(
            "{} sketch dimension(s) retain native operands because no unique neutral dimension was resolved.",
            gaps.native_dimensions
        ),
    );
    push(
        gaps.unprojected_sketch_placements,
        format!(
            "{} decoded Sketch placement(s) have no neutral sketch.",
            gaps.unprojected_sketch_placements
        ),
    );
    push(
        gaps.unprojected_sketch_points,
        format!(
            "{} decoded sketch point(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_points
        ),
    );
    push(
        gaps.unprojected_sketch_curves,
        format!(
            "{} decoded sketch curve(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_curves
        ),
    );
    push(
        gaps.unprojected_sketch_surfaces,
        format!(
            "{} decoded sketch surface(s) have no neutral spatial sketch entity.",
            gaps.unprojected_sketch_surfaces
        ),
    );
    push(
        gaps.unprojected_sketch_texts,
        format!(
            "{} decoded sketch text record(s) have no neutral sketch entity.",
            gaps.unprojected_sketch_texts
        ),
    );
    push(
        gaps.unprojected_sketch_relations,
        format!(
            "{} decoded sketch relation(s) have no neutral constraint.",
            gaps.unprojected_sketch_relations
        ),
    );
    push(
        gaps.unprojected_dimensions,
        format!(
            "{} Design dimension parameter(s) have no parameter-backed neutral or native sketch constraint.",
            gaps.unprojected_dimensions
        ),
    );
    push(
        gaps.profile_selections,
        format!(
            "{} feature profile selection(s) retain native selection identities because no unique neutral profile was resolved.",
            gaps.profile_selections
        ),
    );
    push(
        gaps.path_selections,
        format!(
            "{} feature path selection(s) retain native selection identities because no unique neutral path was resolved.",
            gaps.path_selections
        ),
    );
    push(
        gaps.face_selections,
        format!(
            "{} feature face selection(s) retain native candidates because no unique topological face was resolved.",
            gaps.face_selections
        ),
    );
    push(
        gaps.body_selections,
        format!(
            "{} feature body selection(s) retain native identities because no unique solved body was resolved.",
            gaps.body_selections
        ),
    );
    push(
        gaps.partially_resolved_face_members,
        format!(
            "{} feature face operand(s) remain unresolved inside state-bound historical selections.",
            gaps.partially_resolved_face_members
        ),
    );
    push(
        gaps.native_edge_selections,
        format!(
            "{} edge-treatment selection(s) retain native construction recipes because no neutral historical edge selection was resolved.",
            gaps.native_edge_selections
        ),
    );
    push(
        gaps.partially_resolved_edge_members,
        format!(
            "{} edge-treatment operand(s) remain unresolved inside state-bound historical selections.",
            gaps.partially_resolved_edge_members
        ),
    );
    push(
        gaps.unresolved_edge_selections,
        format!(
            "{} edge-treatment selection(s) are unresolved because their source edge references were lost.",
            gaps.unresolved_edge_selections
        ),
    );
}

fn model_brep_candidates(
    scan: &ContainerScan,
    bindings: &[crate::records::DesignBodyBinding],
) -> Result<Vec<BrepFacts>, CodecError> {
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for binding in bindings {
        if !seen.insert(binding.blob_name.as_str()) {
            continue;
        }
        let matches = container::design_breps(scan)
            .filter(|brep| brep.name.rsplit('/').next() == Some(binding.blob_name.as_str()))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [brep] => candidates.push((**brep).clone()),
            [] => {
                return Err(CodecError::Malformed(format!(
                    "Design body map references missing BREP entry {}",
                    binding.blob_name
                )))
            }
            _ => {
                return Err(CodecError::Malformed(format!(
                    "Design body map BREP basename is ambiguous: {}",
                    binding.blob_name
                )))
            }
        }
    }
    if candidates.is_empty() {
        let legacy = if bindings.is_empty() {
            container::legacy_design_model_breps(scan)
        } else {
            None
        };
        if let Some(legacy) = legacy {
            candidates.extend(legacy.into_iter().cloned());
        } else if let Some(fallback) = container::select_fallback_brep(scan) {
            candidates.push((*fallback).clone());
        } else if container::design_breps(scan).next().is_some() {
            return Err(CodecError::Malformed(
                "Design body map is absent and BREP selection is ambiguous".to_string(),
            ));
        }
    }
    Ok(candidates)
}

/// Decode the document model from its text-encoded carriers.
///
/// The text encoding is the model carrier only when no binary stream decoded,
/// so the caller runs this after the binary candidate loop. Every `.sat` and
/// `.smt` entry that parses and produces geometry joins the merged graph;
/// with more than one contributing entry, each graph is qualified by its
/// entry basename.
fn try_decode_text_model(
    scan: &ContainerScan<'_>,
) -> Result<Option<(BrepFacts, Brep)>, CodecError> {
    let names: Vec<String> = container::text_brep_names(scan)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut parts: Vec<(BrepFacts, Brep)> = Vec::new();
    for name in &names {
        let bytes = scan.entry_bytes(name)?;
        let Ok(stream) = cadmpeg_asm::sat::parse(bytes) else {
            continue;
        };
        let decoded = brep::decode_text(&stream, bytes, name, crate::ids::ID_FORMAT);
        if decoded.asm.surfaces.is_empty()
            && decoded.asm.points.is_empty()
            && decoded.asm.faces.is_empty()
        {
            continue;
        }
        // Facts for the report and source attributes. The header carries the
        // stream's own unit; the decoded token values are already in the
        // centimetre convention.
        let mut header = stream.header.as_kernel_header();
        header.scale = Some(stream.header.scale);
        parts.push((
            BrepFacts {
                name: name.clone(),
                is_smbh: false,
                uncompressed_len: bytes.len() as u64,
                header: Some(header),
                solved_record_limit: None,
                sha256: sha256_hex(bytes),
            },
            decoded,
        ));
    }
    let qualify = parts.len() > 1;
    let mut merged: Option<(BrepFacts, Brep)> = None;
    for (facts, mut part) in parts {
        if qualify {
            let namespace = facts.name.rsplit('/').next().unwrap_or(&facts.name);
            part.qualify_ids(crate::ids::ID_FORMAT, namespace)?;
        }
        match &mut merged {
            None => merged = Some((facts, part)),
            Some((_, whole)) => whole.append(part),
        }
    }
    Ok(merged)
}

/// Assemble the complete document result from one decoded model B-rep graph.
///
/// Both encodings converge here: the binary candidate loop and the text-stream
/// decode hand over the merged graph, the facts of the primary carrier, the
/// resolved body visibilities, and the count of candidates whose decode
/// produced nothing.
fn finish_model_decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
    primary_model_brep: &BrepFacts,
    brep: Brep,
    body_visibilities: Vec<crate::records::BodyVisibility>,
    undecoded_candidates: usize,
    admitted_entities: u64,
) -> Result<DecodeResult, CodecError> {
    F3dDecodeSession::from_geometry(
        ctx,
        scan,
        primary_model_brep,
        brep,
        body_visibilities,
        undecoded_candidates,
        admitted_entities,
    )?
    .into_result()
}

/// Optional geometry index after a successful B-rep transfer.
///
/// Absence means the document is assembled from Design / mesh / presentation
/// content alone; product decoding still runs through the same session path.
struct GeometryIndex {
    primary_model_brep_name: String,
    annotation_records: Vec<cadmpeg_asm::brep::AnnotationRecord>,
    mesh_projection: MeshProjection,
}

/// Private decode accumulator for one `.f3d` document.
///
/// Geometry presence is an optional index on this session. History, parameters,
/// sketches, dimensions, body bindings, configurations, materials, products,
/// native storage, annotations, and the report all flow through one finalizer
/// so a missing BREP does not fork unrelated product decoding.
struct F3dDecodeSession<'a> {
    ctx: &'a DecodeContext<'a>,
    scan: &'a ContainerScan<'a>,
    geometry: Option<GeometryIndex>,
    /// Geometry-path materials, decoded before the shared design graph runs.
    geometry_materials: Option<materials::DecodedMaterials>,
    native: F3dNative,
    ir: CadIr,
    report: DecodeReport,
    unknowns: Vec<UnknownRecord>,
    /// No-BREP finalize inputs retained across product decode.
    deferred_xref: Option<Result<Option<crate::xref::XrefTable>, CodecError>>,
    deferred_non_root_act: Option<usize>,
    deferred_has_appearance: Option<bool>,
    admitted_entities: u64,
}

impl<'a> F3dDecodeSession<'a> {
    fn from_geometry(
        ctx: &'a DecodeContext<'a>,
        scan: &'a ContainerScan<'a>,
        primary_model_brep: &BrepFacts,
        brep: Brep,
        body_visibilities: Vec<crate::records::BodyVisibility>,
        undecoded_candidates: usize,
        mut admitted_entities: u64,
    ) -> Result<Self, CodecError> {
        let mut report = build_geometry_report(scan, &brep);
        if undecoded_candidates != 0 {
            report.losses.push(LossNote {
                code: LossKind::shared(LossTaxonomy::GeometryNotTransferred),
                severity: Severity::Warning,
                message: format!(
                    "{undecoded_candidates} Design-referenced BREP blob(s) could not be decoded."
                ),
                provenance: None,
            });
        }
        let design_body_bindings = crate::design::decode::body::decode_design_body_bindings(
            scan,
            Some(&primary_model_brep.name),
            &brep.asm.body_native_keys,
        )?;
        let geometry_materials =
            materials::decode_with_body_bindings(ctx, scan, &design_body_bindings)?;
        let (mut ir, mut native, asm_remainder) =
            build_geometry_ir(ctx, scan, primary_model_brep, brep)?;
        // ASM transfer already charged its delta; keep the running counter in
        // sync so a later admit_entities call cannot double-count those bodies.
        admitted_entities = admitted_entities.max(ir.model.entity_count() as u64);
        let AsmTransferRemainder {
            body_keys: _,
            face_keys: _,
            unknowns,
            stats: _,
            annotation_records,
        } = asm_remainder;
        let (subds, subd_losses) = crate::tsm::decode(scan)?;
        ir.model.subds = subds;
        let mesh_projection = project_mesh_bodies(scan, &mut ir, &mut native, &mut report)?;
        report.losses.extend(subd_losses);
        native.body_visibilities = body_visibilities;
        native.design_body_bindings = design_body_bindings;
        ctx.admit_entities(
            ir.model.entity_count() as u64,
            &mut admitted_entities,
            "admit F3D geometry entities",
        )?;
        Ok(Self {
            ctx,
            scan,
            geometry: Some(GeometryIndex {
                primary_model_brep_name: primary_model_brep.name.clone(),
                annotation_records,
                mesh_projection,
            }),
            geometry_materials: Some(geometry_materials),
            native,
            ir,
            report,
            unknowns,
            deferred_xref: None,
            deferred_non_root_act: None,
            deferred_has_appearance: None,
            admitted_entities,
        })
    }

    fn from_metadata(
        ctx: &'a DecodeContext<'a>,
        scan: &'a ContainerScan<'a>,
        admitted_entities: u64,
    ) -> Self {
        let (ir, unknowns) = build_metadata_ir(scan);
        Self {
            ctx,
            scan,
            geometry: None,
            geometry_materials: None,
            native: F3dNative::default(),
            ir,
            report: build_container_report(scan, false),
            unknowns,
            deferred_xref: None,
            deferred_non_root_act: None,
            deferred_has_appearance: None,
            admitted_entities,
        }
    }

    fn admit_model_entities(&mut self, operation: &'static str) -> Result<(), CodecError> {
        self.ctx.admit_entities(
            self.ir.model.entity_count() as u64,
            &mut self.admitted_entities,
            operation,
        )
    }

    /// Decode design graph, products, annotations, and the report.
    fn into_result(mut self) -> Result<DecodeResult, CodecError> {
        self.admit_model_entities("admit F3D geometry entities")?;
        self.decode_design_graph()?;
        self.decode_products()?;
        self.finalize()
    }

    fn decode_design_graph(&mut self) -> Result<(), CodecError> {
        let scan = self.scan;
        let ctx = self.ctx;
        for history_brep in container::history_breps(scan) {
            if let Some(history) = decode_asm_history(ctx, scan, history_brep)? {
                self.native.asm_histories.push(history);
            }
        }
        self.native.construction_recipes = crate::design::decode::parameters::decode_recipes(scan)?;
        self.native.persistent_references =
            crate::design::decode::sketch::decode_persistent_references(scan)?;
        self.native.lost_edge_references =
            crate::design::decode::sketch::decode_lost_edge_references(scan)?;
        self.native.design_material_assignments =
            crate::materials::decode_design_assignments(scan)?;
        self.native.design_types = crate::design::decode::meta::decode_types(scan)?;
        self.native.design_parameters = crate::design::decode::parameters::decode_parameters(scan)?;
        self.native.design_entity_headers =
            crate::design::decode::sketch::decode_entity_headers(scan)?;
        self.native.design_record_headers = crate::design::decode::sketch::decode_record_headers(
            scan,
            &self.native.design_entity_headers,
        )?;
        self.native.sketch_relations = crate::design::decode::sketch::decode_sketch_relations(
            scan,
            &self.native.design_record_headers,
        )?;
        extend_related_design_records(scan, &mut self.native)?;
        self.native.sketch_points = crate::design::decode::sketch::decode_sketch_points(scan)?;
        self.native.sketch_texts = crate::design::decode::sketch::decode_sketch_texts(scan)?;
        self.native.sketch_curve_identities =
            crate::design::decode::sketch::decode_sketch_curve_identities(scan)?;
        self.native.sketch_surfaces = crate::design::decode::sketch::decode_sketch_surfaces(scan)?;
        crate::design::decode::sketch::bind_sketch_graph(
            &self.native.design_entity_headers,
            &mut self.native.sketch_points,
            &mut self.native.sketch_curve_identities,
            &mut self.native.sketch_surfaces,
            &mut self.native.sketch_relations,
        )?;
        crate::design::decode::operands::bind_extrude_selection_geometry(
            &mut self.native.design_extrude_selection_members,
            &self.native.design_extrude_selection_groups,
            &self.native.design_parameter_scopes,
            &self.native.sketch_points,
            &self.native.sketch_curve_identities,
        );
        let dimension_inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
            scan,
            parameters: &self.native.design_parameters,
            owners: &self.native.design_parameter_owners,
            companions: &self.native.design_parameter_companions,
            scopes: &self.native.design_parameter_scopes,
            headers: &self.native.design_record_headers,
            points: &self.native.sketch_points,
            curves: &self.native.sketch_curve_identities,
        };
        self.native.design_dimension_locus_pairs =
            crate::design::decode::dimension_frames::decode_dimension_locus_pairs(
                &dimension_inputs,
            )?;
        self.native.design_dimension_annotation_frames =
            crate::design::decode::dimension_frames::decode_dimension_annotation_frames(
                &dimension_inputs,
                &self.native.design_entity_headers,
            )?;
        self.native.design_dimension_locus_groups =
            crate::design::decode::dimension_frames::decode_dimension_locus_groups(
                &dimension_inputs,
                &self.native.design_entity_headers,
            )?;
        self.native.design_dimension_null_locus_pairs =
            crate::design::decode::dimension_frames::decode_dimension_null_locus_pairs(
                &dimension_inputs,
                &self.native.design_dimension_locus_pairs,
                &self.native.design_dimension_locus_groups,
            )?;
        crate::design::dimensions::remove_dimension_frame_relations(
            &mut self.native.sketch_relations,
            &self.native.design_dimension_locus_pairs,
            &self.native.design_dimension_locus_groups,
            &self.native.design_dimension_null_locus_pairs,
        );
        crate::design::dimensions::bind_dimension_loci(
            &self.native.design_sketch_placements,
            &self.native.design_parameter_owners,
            &self.native.design_dimension_locus_pairs,
            &self.native.design_dimension_locus_groups,
            &self.native.design_dimension_annotation_frames,
            &self.native.design_dimension_null_locus_pairs,
            &mut self.native.sketch_points,
            &mut self.native.sketch_curve_identities,
        )?;
        self.native.design_body_members = crate::design::decode::body::decode_body_members(scan)?;
        if self.geometry.is_none() {
            self.native.design_body_bindings =
                crate::design::decode::body::decode_design_body_bindings(
                    scan,
                    container::select_fallback_brep(scan).map(|entry| entry.name.as_str()),
                    &self.native.body_native_keys,
                )?;
        }
        self.native.design_body_bounds = crate::design::decode::body::decode_body_bounds(
            scan,
            &self.native.design_entity_headers,
        )?;
        crate::design::decode::body::bind_body_bounds(
            &mut self.native.design_body_bounds,
            &self.native.design_body_bindings,
        );
        self.native.design_configurations =
            crate::design::configurations::decode_configurations(scan)?;
        self.ir.model.configurations = crate::design::configurations::project_configurations(
            &self.native.design_configurations,
        )?;
        (self.ir.model.features, self.ir.model.parameters) =
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                &crate::design::feature_project::ProjectInputs {
                    native: &self.native.design_parameters,
                    owners: &self.native.design_parameter_owners,
                    scopes: &self.native.design_parameter_scopes,
                    timelines: &self.native.design_feature_timelines,
                    construction_groups: &self.native.design_construction_operand_groups,
                    fillet_radius_groups: &self.native.design_fillet_radius_groups,
                    edge_operands: &self.native.design_edge_operands,
                    edge_identity_operands: &self.native.design_edge_identity_operands,
                    entity_selection_operands: &self.native.design_entity_selection_operands,
                    curve_identities: &self.native.sketch_curve_identities,
                    face_operands: &self.native.design_face_operands,
                    body_recipe_operands: &self.native.design_body_recipe_operands,
                    placements: &self.native.design_sketch_placements,
                    body_bindings: &self.native.design_body_bindings,
                    histories: &self.native.asm_histories,
                },
            )?;
        if let Some(geometry) = &self.geometry {
            bind_mesh_feature_definitions(
                &mut self.ir.model.features,
                &self.native.design_parameter_scopes,
                &geometry.mesh_projection,
            );
        }
        crate::design::feature_project::bind_form_cages(
            scan,
            &self.native.design_parameter_scopes,
            &mut self.ir.model.features,
            &self.ir.model.subds,
        )?;
        let canvas_assets = crate::design::decode::canvas::project_canvas_images(
            scan,
            &self.native.design_parameter_scopes,
            &self.native.design_canvas_images,
            &mut self.ir.model.features,
        )?;
        extend_unique_assets(&mut self.ir.model.assets, canvas_assets)?;
        let decal_assets = crate::design::decode::decal::project_decal_images(
            scan,
            &self.native.design_parameter_scopes,
            &self.native.design_decal_images,
            &self.native.design_construction_operand_groups,
            &self.native.design_body_recipe_operands,
            &mut self.ir.model.features,
        )?;
        extend_unique_assets(&mut self.ir.model.assets, decal_assets)?;
        crate::design::configurations::bind_configuration_parameter_overrides(
            &mut self.ir.model.configurations,
            &self.ir.model.parameters,
        );
        crate::design::configurations::bind_configuration_suppressed_features(
            &mut self.ir.model.configurations,
            &self.ir.model.features,
        );
        self.ir.model.feature_input_topologies = crate::history::project_feature_input_topologies(
            &self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.asm_histories,
            &self.native.design_edge_operands,
        );
        crate::history::bind_feature_outputs(
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.asm_histories,
            &self.ir.model.bodies,
        );
        crate::history::bind_sweep_result_modes(&mut self.ir.model.features, &self.ir.model.bodies);
        crate::history::bind_feature_body_selections(
            &mut self.ir.model.features,
            &crate::history::FeatureBodySelectionInputs {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                body_recipe_operands: &self.native.design_body_recipe_operands,
                construction_recipes: &self.native.construction_recipes,
                persistent_design_links: &self.native.persistent_design_links,
                histories: &self.native.asm_histories,
                bodies: &self.ir.model.bodies,
                regions: &self.ir.model.regions,
                shells: &self.ir.model.shells,
            },
        );
        crate::history::bind_feature_face_selections(
            &mut self.ir.model.features,
            &mut self.ir.model.feature_input_topologies,
            &self.native.design_parameter_scopes,
            &self.native.design_construction_operand_groups,
            &self.native.design_face_operands,
            &self.native.design_entity_selection_operands,
            &self.native.design_body_recipe_operands,
            &self.native.asm_histories,
        );
        crate::history::bind_feature_path_selections(
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_construction_operand_groups,
            &self.native.design_entity_selection_operands,
        );
        crate::design::feature_project::bind_revolve_face_axes(
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_construction_operand_groups,
            &self.native.design_entity_selection_operands,
            &self.native.design_face_operands,
            &self.ir.model.faces,
            &self.ir.model.surfaces,
        );
        (self.ir.model.sketches, self.ir.model.sketch_entities) =
            crate::design::sketch_project::project_sketch_design(
                &self.native.design_sketch_placements,
                &self.native.sketch_points,
                &self.native.sketch_curve_identities,
                &self.native.sketch_texts,
                self.ir.tolerances.linear,
            );
        (
            self.ir.model.spatial_sketches,
            self.ir.model.spatial_sketch_entities,
        ) = crate::design::sketch_project::project_spatial_sketch_design(
            &self.native.design_sketch_placements,
            &self.native.sketch_points,
            &self.native.sketch_curve_identities,
            &self.native.sketch_surfaces,
            &self.native.sketch_relations,
            self.ir.tolerances.linear,
        );
        let arrangement_budget =
            ctx.work_budget(crate::design::geometry::MAX_ARRANGEMENT_WALK_WORK as u64);
        crate::design::profile_select::bind_sweep_sketch_selections(
            &mut self.ir.model.features,
            &crate::design::profile_select::SketchCurveSelectionResolution {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
            },
        );
        crate::design::profile_select::bind_split_face_sketch_selections(
            &mut self.ir.model.features,
            &crate::design::profile_select::SketchCurveSelectionResolution {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
            },
        );
        crate::design::profile_select::bind_loft_sketch_selections(
            scan,
            &self.native.design_construction_operand_groups,
            &self.native.design_record_headers,
            &crate::design::profile_select::LoftSketchResolution {
                entities: &self.native.design_entity_headers,
                entity_selection_operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
                linear_tolerance: self.ir.tolerances.linear,
                angular_tolerance: self.ir.tolerances.angular,
            },
            &mut self.ir.model.features,
        )?;
        crate::design::feature_project::bind_sketch_feature_geometry(
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_sketch_placements,
            &self.ir.model.sketches,
            &self.ir.model.spatial_sketches,
        );
        self.ir.model.spatial_sketch_constraints =
            crate::design::sketch_project::project_spatial_sketch_constraints(
                &self.native.design_sketch_placements,
                &self.native.sketch_relations,
                &self.native.sketch_points,
                &self.native.sketch_curve_identities,
                &self.native.sketch_surfaces,
                &self.ir.model.spatial_sketch_entities,
            );
        crate::design::profile_select::bind_extrude_profile_selections(
            &mut self.ir.model.features,
            &self.native.design_parameter_scopes,
            &self.native.design_extrude_selection_groups,
            &self.native.design_extrude_selection_members,
            &self.ir.model.sketches,
            &crate::design::profile_select::SketchCurveSelectionResolution {
                scopes: &self.native.design_parameter_scopes,
                groups: &self.native.design_construction_operand_groups,
                operands: &self.native.design_entity_selection_operands,
                placements: &self.native.design_sketch_placements,
                curve_identities: &self.native.sketch_curve_identities,
                sketches: &self.ir.model.sketches,
                sketch_entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_sketch_entities: &self.ir.model.spatial_sketch_entities,
            },
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &self.ir.model.sketch_entities,
                spatial_sketches: &self.ir.model.spatial_sketches,
                spatial_entities: &self.ir.model.spatial_sketch_entities,
                histories: &self.native.asm_histories,
                linear_tolerance: self.ir.tolerances.linear,
                angular_tolerance: self.ir.tolerances.angular,
                arrangement_budget: &arrangement_budget,
            },
        );
        if self.geometry.is_some() {
            crate::history::discard_projection_caches(&mut self.native.asm_histories);
        }
        crate::design::face_resolve::bind_extrude_start_planes(
            &mut self.ir.model.features,
            &self.ir.model.sketches,
            &mut crate::design::face_resolve::ExtrudeStartPlaneResolution {
                faces: &self.ir.model.faces,
                surfaces: &self.ir.model.surfaces,
                groups: &self.native.design_construction_operand_groups,
                operands: &mut self.native.design_face_operands,
                linear_tolerance: self.ir.tolerances.linear,
                angular_tolerance: self.ir.tolerances.angular,
            },
        );
        self.ir.model.sketch_constraints = crate::design::constraints::project_sketch_constraints(
            &self.native.design_sketch_placements,
            &self.native.design_parameters,
            &self.native.sketch_points,
            &self.native.sketch_curve_identities,
            &self.native.sketch_texts,
            &self.native.sketch_relations,
            &self.ir.model.sketch_entities,
        );
        let constraint_inputs = crate::design::dimensions::DimensionConstraintInputs {
            placements: &self.native.design_sketch_placements,
            parameters: &self.native.design_parameters,
            owners: &self.native.design_parameter_owners,
            pairs: &self.native.design_dimension_locus_pairs,
            groups: &self.native.design_dimension_locus_groups,
            annotation_frames: &self.native.design_dimension_annotation_frames,
            null_pairs: &self.native.design_dimension_null_locus_pairs,
            companions: &self.native.design_parameter_companions,
            recipe_records: &self.native.design_dimension_recipe_records,
            points: &self.native.sketch_points,
            curves: &self.native.sketch_curve_identities,
            entities: &self.ir.model.sketch_entities,
        };
        self.ir.model.sketch_constraints.extend(
            crate::design::dimensions::project_dimension_constraints(
                &constraint_inputs,
                &self.ir.model.spatial_sketches,
                self.ir.tolerances.linear,
            ),
        );
        self.ir.model.spatial_sketch_constraints.extend(
            crate::design::dimensions::project_spatial_dimension_constraints(
                &constraint_inputs,
                &self.ir.model.spatial_sketches,
                &self.ir.model.spatial_sketch_entities,
                self.ir.tolerances.linear,
            ),
        );
        crate::design::dimensions::bind_offset_dimension_parameters(
            &mut self.ir.model.sketch_constraints,
            &self.native.design_parameters,
        );
        self.ir
            .model
            .sketch_constraints
            .sort_by_key(|constraint| constraint.id.clone());
        self.ir
            .model
            .spatial_sketch_constraints
            .sort_by_key(|constraint| constraint.id.clone());
        Ok(())
    }

    fn decode_products(&mut self) -> Result<(), CodecError> {
        let scan = self.scan;
        let act = crate::act::decode(scan)?;
        let non_root_act_component_links = act.non_root_component_links;
        self.native.act_entities = act.entities;
        self.native.act_guids = act.guids;
        self.native.act_registry_channels = act.registry_channels;
        self.native.act_root_components = act.root_components;
        self.native.act_table_references = act.table_references;

        if self.geometry.is_some() {
            report_unretained_act_component_links(&mut self.report, non_root_act_component_links);
            report_unresolved_dimension_companions(&mut self.report, &self.native, &self.ir);
            report_unresolved_configuration_rules(&mut self.report, &self.native, &self.ir);
            let materials = self
                .geometry_materials
                .take()
                .expect("geometry-path materials");
            self.ir.model.appearances = materials.appearances;
            self.ir.model.appearance_bindings = materials.bindings;
            resolve_face_appearance_bindings(&mut self.ir, &materials.face_assignments)?;
            apply_appearance_base_colors(&mut self.ir);
            self.ir
                .model
                .appearance_bindings
                .sort_by(|a, b| a.id.cmp(&b.id));
            reconcile_appearance_loss(
                &mut self.report,
                &self.ir,
                materials.has_topology_assignments,
            );
            annotate_docstruct(&mut self.ir, scan);
            match crate::xref::decode(scan) {
                Ok(Some(table)) => {
                    self.ir.model.occurrences = crate::xref::project_occurrences(&table);
                    crate::xref::bind_component_insert_features(
                        &mut self.ir.model.features,
                        &self.native.design_parameter_scopes,
                        &table,
                    );
                    self.native.xref_designs = table.designs;
                    self.native.xref_references = table.references;
                }
                Ok(None) => {}
                Err(error) => self.report.losses.push(xref_parse_loss(&error)),
            }
        } else {
            let decoded_materials = materials::decode(self.ctx, scan)?;
            self.deferred_has_appearance = Some(decoded_materials.has_topology_assignments);
            self.ir.model.appearances = decoded_materials.appearances;
            self.ir.model.appearance_bindings = decoded_materials.bindings;
            annotate_docstruct(&mut self.ir, scan);
            let xref_table = crate::xref::decode(scan);
            if let Ok(Some(table)) = &xref_table {
                self.ir.model.occurrences = crate::xref::project_occurrences(table);
                crate::xref::bind_component_insert_features(
                    &mut self.ir.model.features,
                    &self.native.design_parameter_scopes,
                    table,
                );
                self.native.xref_designs.clone_from(&table.designs);
                self.native.xref_references.clone_from(&table.references);
            }
            self.deferred_xref = Some(xref_table);
            self.deferred_non_root_act = Some(non_root_act_component_links);
        }

        let (components, occurrences) = crate::design::components::project_local_components(
            &self.native.design_parameter_scopes,
            &self.native.design_component_occurrences,
        );
        self.ir.model.product_definitions.extend(components);
        self.ir.model.occurrences.extend(occurrences);
        let unresolved_component_inserts =
            crate::design::components::project_unresolved_component_insert_occurrences(
                &mut self.ir.model.features,
                &self.native.design_parameter_scopes,
                self.ir.model.occurrences.len(),
            );
        self.ir
            .model
            .occurrences
            .extend(unresolved_component_inserts);
        self.ir.model.assembly_joints = crate::design::assembly::project_assembly_joints(
            &self.native.design_parameter_scopes,
            &self.native.design_component_occurrences,
            &self.ir.model.features,
        );
        Ok(())
    }

    fn finalize(mut self) -> Result<DecodeResult, CodecError> {
        let scan = self.scan;
        let ctx = self.ctx;
        if self.geometry.is_none() {
            report_unretained_act_component_links(
                &mut self.report,
                self.deferred_non_root_act.take().unwrap_or(0),
            );
            let has_appearance = self.deferred_has_appearance.take().unwrap_or(false);
            reconcile_appearance_loss(&mut self.report, &self.ir, has_appearance);
            let mesh_projection =
                project_mesh_bodies(scan, &mut self.ir, &mut self.native, &mut self.report)?;
            bind_mesh_feature_definitions(
                &mut self.ir.model.features,
                &self.native.design_parameter_scopes,
                &mesh_projection,
            );
            report_design_projection_gaps(&mut self.report, &self.ir, &self.native);
            self.admit_model_entities("admit F3D entities")?;
            self.native.store(self.ir.native.namespace_mut("f3d"))?;
            let annotations =
                populate_annotations(&self.ir, scan, &self.native, None, &self.unknowns);
            let source_image = preserve_source_image(scan);
            if mesh_projection.count > 0 {
                apply_mesh_body_classification(&mut self.report, scan, mesh_projection.count);
            } else {
                apply_bodyless_design_classification(
                    &mut self.report,
                    container::design_breps(scan).count(),
                    container::text_brep_names(scan).len(),
                    self.native.design_body_bindings.len() + self.native.design_body_members.len(),
                    self.ir.model.sketch_entities.len()
                        + self.ir.model.spatial_sketch_entities.len(),
                    self.native.design_canvas_images.len(),
                );
            }
            report_unresolved_dimension_companions(&mut self.report, &self.native, &self.ir);
            match self.deferred_xref.take() {
                Some(Ok(Some(table))) => {
                    apply_assembly_classification(&mut self.report, scan, &table);
                }
                Some(Ok(None)) => {}
                Some(Err(error)) => self.report.losses.push(xref_parse_loss(&error)),
                None => {}
            }
            let mut admitted_entities = self.admitted_entities;
            return decode_result(
                ctx,
                self.ir,
                self.report,
                annotations,
                self.unknowns,
                source_image,
                &mut admitted_entities,
            );
        }

        report_design_projection_gaps(&mut self.report, &self.ir, &self.native);
        self.admit_model_entities("admit F3D entities")?;
        self.native.store(self.ir.native.namespace_mut("f3d"))?;
        let geometry = self.geometry.take().expect("geometry");
        let annotations = populate_annotations(
            &self.ir,
            scan,
            &self.native,
            Some((
                &geometry.primary_model_brep_name,
                &geometry.annotation_records,
            )),
            &self.unknowns,
        );
        let source_image = preserve_source_image(scan);
        let mut admitted_entities = self.admitted_entities;
        decode_result(
            ctx,
            self.ir,
            self.report,
            annotations,
            self.unknowns,
            source_image,
            &mut admitted_entities,
        )
    }
}

fn brep_identity_namespace(entry: &str) -> Option<&str> {
    entry.rsplit('/').next()?.strip_prefix("BREP.")
}

/// Decode a `.f3d` reader into a document and its loss report.
pub fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(ctx, root)?;
    let mut admitted_entities = 0_u64;
    ctx.admit_entities(
        scan.entries.len() as u64,
        &mut admitted_entities,
        "admit F3D archive entries",
    )?;

    if crate::f3z::is_f3z(&scan) {
        return crate::f3z::decode(ctx, &scan);
    }

    if ctx.container_only() {
        let (mut ir, unknowns) = build_metadata_ir(&scan);
        annotate_docstruct(&mut ir, &scan);
        let annotations = populate_annotations(&ir, &scan, &F3dNative::default(), None, &unknowns);
        let source_image = preserve_source_image(&scan);
        let mut report = build_container_report(&scan, true);
        if let Ok(Some(table)) = crate::xref::decode(&scan) {
            apply_assembly_classification(&mut report, &scan, &table);
        }
        return decode_result(
            ctx,
            ir,
            report,
            annotations,
            unknowns,
            source_image,
            &mut admitted_entities,
        );
    }

    let unbound_body_bindings =
        crate::design::decode::body::decode_design_body_bindings(&scan, None, &[])?;
    let model_breps = model_brep_candidates(&scan, &unbound_body_bindings)?;

    // Every Design body-map pair names its owning BREP blob. Decode the
    // complete referenced set; a document-level model is not confined to one
    // arbitrary `.smbh` entry.
    if let Some(primary_model_brep) = model_breps.first().cloned() {
        let qualify_ids = model_breps.len() > 1;
        let mut brep = Brep::default();
        let mut body_visibilities = Vec::new();
        let mut decoded_brep_count = 0usize;
        let all_body_visibility = crate::design::decode::body::decode_all_body_visibility(&scan)?;
        let mut selected_body_keys =
            std::collections::HashMap::<String, std::collections::HashSet<u64>>::new();
        for binding in &unbound_body_bindings {
            selected_body_keys
                .entry(binding.blob_name.clone())
                .or_default()
                .insert(binding.asm_body_key);
        }
        for candidate in &model_breps {
            let Some(mut part) = try_decode_brep(&scan, candidate)? else {
                continue;
            };
            let blob_name = candidate.name.rsplit('/').next().unwrap_or(&candidate.name);
            if let Some(keys) = selected_body_keys.get(blob_name) {
                part.retain_body_keys(keys)?;
            }
            let mut body_selectors = match selected_body_keys.get(blob_name) {
                Some(keys) => part.body_selectors_for(keys)?,
                None => part.body_selectors(),
            };
            for body in &mut part.asm.bodies {
                if let Some(visibility) = body_selectors.get(&body.id).and_then(|selector| {
                    all_body_visibility.get(&(blob_name.to_owned(), *selector))
                }) {
                    body.visible = Some(visibility.visible);
                }
            }
            if qualify_ids {
                let namespace = brep_identity_namespace(&candidate.name).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "BREP entry has no stable blob identity: {}",
                        candidate.name
                    ))
                })?;
                part.qualify_ids(crate::ids::ID_FORMAT, namespace)?;
                body_selectors = match selected_body_keys.get(blob_name) {
                    Some(keys) => part.body_selectors_for(keys)?,
                    None => part.body_selectors(),
                };
            }
            for body in &part.asm.bodies {
                if let Some((body_selector, visibility)) =
                    body_selectors.get(&body.id).and_then(|selector| {
                        all_body_visibility
                            .get(&(blob_name.to_owned(), *selector))
                            .map(|visibility| (*selector, visibility))
                    })
                {
                    body_visibilities.push(crate::records::BodyVisibility {
                        id: crate::ids::native_scoped_id(
                            &candidate.name,
                            "body-visibility",
                            body_selector,
                        ),
                        body: body.id.clone(),
                        stream: visibility.stream.clone(),
                        byte_offset: visibility.byte_offset,
                        asm_body_key_offset: visibility.asm_body_key_offset,
                        asm_body_key: body_selector,
                        entity_suffix: visibility.entity_suffix,
                        visible: visibility.visible,
                    });
                }
            }
            brep.append(part);
            decoded_brep_count += 1;
        }
        if decoded_brep_count != 0 {
            // Re-find primary in model_breps after move — keep the cloned primary.
            return finish_model_decode(
                ctx,
                &scan,
                &primary_model_brep,
                brep,
                body_visibilities,
                model_breps.len() - decoded_brep_count,
                admitted_entities,
            );
        }
    }

    // No binary stream decoded: the model may be carried only in the text
    // encoding.
    if let Some((text_facts, text_brep)) = try_decode_text_model(&scan)? {
        return finish_model_decode(
            ctx,
            &scan,
            &text_facts,
            text_brep,
            Vec::new(),
            0,
            admitted_entities,
        );
    }

    // No decodable SAB stream: use container metadata through the shared session.
    F3dDecodeSession::from_metadata(ctx, &scan, admitted_entities).into_result()
}

/// Projected mesh geometry and the Design records that own it.
struct MeshProjection {
    /// Number of joined mesh-body containers.
    count: usize,
    /// Tessellation identities grouped by their Design stream and feature scope record.
    tessellations_by_scope: std::collections::HashMap<(String, u32), Vec<String>>,
}

fn extend_unique_assets(
    assets: &mut Vec<cadmpeg_ir::assets::Asset>,
    incoming: Vec<cadmpeg_ir::assets::Asset>,
) -> Result<(), CodecError> {
    for asset in incoming {
        match assets.iter().find(|existing| existing.id == asset.id) {
            Some(existing) if existing != &asset => {
                return Err(CodecError::Malformed(format!(
                    "F3D embedded asset {} has conflicting projections",
                    asset.id.0
                )))
            }
            Some(_) => {}
            None => assets.push(asset),
        }
    }
    Ok(())
}

/// Project each mesh body's container geometry into the tessellation arena.
///
/// A mesh body carries no B-rep topology: its geometry is a triangle list, and
/// the neutral tessellation arena is where a standalone triangle list belongs.
/// Returns the number of bodies projected and reports every mesh-geometry
/// container that no body claimed.
fn project_mesh_bodies(
    scan: &ContainerScan,
    ir: &mut CadIr,
    native: &mut F3dNative,
    report: &mut DecodeReport,
) -> Result<MeshProjection, CodecError> {
    use crate::design::decode::mesh::MeshContainerOutcome;

    let decoded = crate::design::decode::mesh::decode_mesh_bodies(scan)?;
    native.design_mesh_features = decoded.features;
    let mut texture_assets = Vec::new();
    for texture in native
        .design_mesh_features
        .iter()
        .flat_map(|feature| &feature.textures)
    {
        if ir
            .model
            .assets
            .iter()
            .chain(&texture_assets)
            .any(|asset: &cadmpeg_ir::assets::Asset| asset.id == texture.asset)
        {
            continue;
        }
        let media_type = std::path::Path::new(&texture.filename)
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
                "jpg" | "jpeg" => Some("image/jpeg"),
                "png" => Some("image/png"),
                _ => None,
            })
            .map(str::to_owned);
        texture_assets.push(cadmpeg_ir::assets::Asset {
            id: texture.asset.clone(),
            name: Some(texture.filename.clone()),
            media_type,
            content: cadmpeg_ir::assets::AssetContent::Embedded {
                data: scan.entry_bytes(&texture.archive_entry_name)?.to_vec(),
            },
            native_ref: Some(crate::ids::native_scope(&texture.archive_entry_name)),
        });
    }
    extend_unique_assets(&mut ir.model.assets, texture_assets)?;
    let mut texture_tables = std::collections::HashMap::new();
    for feature in &native.design_mesh_features {
        let texture_table = feature
            .textures
            .iter()
            .enumerate()
            .map(|(ordinal, texture)| {
                if usize::try_from(texture.ordinal) != Ok(ordinal) {
                    return Err(CodecError::Malformed(
                        "F3D mesh texture ordinals do not match flags-map order".into(),
                    ));
                }
                Ok((texture.resource_guid.clone(), texture.asset.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for body in &feature.bodies {
            if let Some(tessellation_id) = &body.tessellation_id {
                if texture_tables
                    .insert(tessellation_id.clone(), texture_table.clone())
                    .is_some()
                {
                    return Err(CodecError::Malformed(
                        "F3D mesh tessellation belongs to more than one texture table".into(),
                    ));
                }
            }
        }
    }
    let mut bodies = Vec::new();
    for outcome in decoded.outcomes {
        match outcome {
            MeshContainerOutcome::Joined(body) => bodies.push(body),
            MeshContainerOutcome::Unjoined { entry_name } => report.losses.push(LossNote {
                code: LossKind::shared(LossTaxonomy::AssetNotTransferred),
                severity: Severity::Warning,
                message: format!(
                    "mesh geometry container `{entry_name}` decoded but has no complete Design body join"
                ),
                provenance: None,
            }),
            MeshContainerOutcome::Failed { entry_name, error } => report.losses.push(LossNote {
                code: LossKind::shared(LossTaxonomy::DecodeDiagnostic),
                severity: Severity::Error,
                message: format!("mesh geometry container `{entry_name}` was not decoded: {error}"),
                provenance: None,
            }),
            MeshContainerOutcome::Missing { entry_name } => report.losses.push(LossNote {
                code: LossKind::shared(LossTaxonomy::AssetNotTransferred),
                severity: Severity::Warning,
                message: format!(
                    "Design mesh body names `{entry_name}`, but no unique geometry container joined it"
                ),
                provenance: None,
            }),
        }
    }
    let mut unresolved = std::collections::BTreeMap::new();
    let mut projection = MeshProjection {
        count: bodies.len(),
        tessellations_by_scope: std::collections::HashMap::new(),
    };
    for mut body in bodies {
        let id = body.id.clone();
        let texture_table = texture_tables.remove(&id).ok_or_else(|| {
            CodecError::Malformed("F3D joined mesh body has no owning texture table".into())
        })?;
        let texture_assignments = mesh_texture_assignments(
            body.texture_ids.as_deref(),
            &texture_table,
            body.triangles.len(),
        )?;
        let triangle_groups = std::mem::take(&mut body.triangle_groups)
            .into_iter()
            .map(
                |group| cadmpeg_ir::tessellation::TessellationTriangleGroup {
                    source_id: Some(group.source_id),
                    triangles: group.triangles,
                },
            )
            .collect();
        let channels = mesh_attribute_channels(
            &body.attributes,
            body.vertices.len(),
            &body.triangles,
            &mut unresolved,
        );
        ir.model
            .tessellations
            .push(cadmpeg_ir::tessellation::Tessellation {
                id,
                body: None,
                faces: Vec::new(),
                // The container stores no chordal deflection: a mesh body's
                // triangles are its geometry, not an approximation of a surface.
                chordal_deflection: None,
                source_object: None,
                vertices: body.vertices,
                triangles: body.triangles,
                feature_edges: body.feature_edges,
                strip_lengths: Vec::new(),
                normals: Vec::new(),
                corner_normals: body.corner_normals,
                triangle_groups,
                texture_assignments,
                channels,
            });
    }
    if !texture_tables.is_empty() {
        return Err(CodecError::Malformed(
            "F3D mesh texture table has no joined tessellation body".into(),
        ));
    }
    for feature in &native.design_mesh_features {
        let tessellations = feature
            .bodies
            .iter()
            .filter_map(|body| body.tessellation_id.clone())
            .collect::<Vec<_>>();
        if tessellations.is_empty() {
            continue;
        }
        let stream = crate::ids::native_stream(&feature.id)
            .unwrap_or(crate::ids::DEFAULT_STREAM)
            .to_owned();
        if projection
            .tessellations_by_scope
            .insert((stream, feature.scope_record.record_index), tessellations)
            .is_some()
        {
            return Err(CodecError::Malformed(
                "F3D Design mesh feature scope is not unique".into(),
            ));
        }
    }
    report_unresolved_mesh_attributes(report, &unresolved);
    Ok(projection)
}

/// Resolve one-based `tid` values through a Design texture table. Zero leaves
/// the triangle untextured.
fn mesh_texture_assignments(
    texture_ids: Option<&[u32]>,
    textures: &[(String, cadmpeg_ir::assets::AssetId)],
    triangle_count: usize,
) -> Result<Vec<cadmpeg_ir::tessellation::TessellationTextureAssignment>, CodecError> {
    let Some(texture_ids) = texture_ids else {
        return Ok(Vec::new());
    };
    if texture_ids.len() != triangle_count {
        return Err(CodecError::Malformed(
            "F3D mesh texture-id count differs from the triangle count".into(),
        ));
    }
    let mut triangles = vec![Vec::new(); textures.len()];
    for (triangle, texture_id) in texture_ids.iter().enumerate() {
        if *texture_id == 0 {
            continue;
        }
        let index = texture_id
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < textures.len())
            .ok_or_else(|| {
                CodecError::Malformed(
                    "F3D mesh triangle texture id names no Design texture resource".into(),
                )
            })?;
        triangles[index].push(u32::try_from(triangle).map_err(|_| {
            CodecError::Malformed("F3D mesh triangle ordinal is out of range".into())
        })?);
    }
    Ok(textures
        .iter()
        .cloned()
        .zip(triangles)
        .filter(|(_, triangles)| !triangles.is_empty())
        .map(|((source_id, texture), triangles)| {
            cadmpeg_ir::tessellation::TessellationTextureAssignment {
                source_id: Some(source_id),
                texture,
                triangles,
            }
        })
        .collect())
}

/// Replace the native definition of each mesh-import scope with its exact
/// tessellation identities.
fn bind_mesh_feature_definitions(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::DesignParameterScope],
    projection: &MeshProjection,
) {
    for feature in features {
        if feature.source_tag.as_deref() != Some("Base Mesh Feature") {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| scope.id == native_ref) else {
            continue;
        };
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(tessellations) = projection
            .tessellations_by_scope
            .get(&(stream.to_owned(), scope.record_index))
        else {
            continue;
        };
        if tessellations.is_empty() {
            continue;
        }
        feature.definition = cadmpeg_ir::features::FeatureDefinition::MeshImport {
            tessellations: tessellations.clone(),
        };
    }
}

/// Project channels with a settled element layout and count unresolved channels
/// by domain.
///
/// An indexed channel stores one default value per vertex followed by values
/// selected at the explicit corner positions in its index stream. The IR keeps
/// the value table and expands those selections into one selector per triangle
/// corner.
fn mesh_attribute_channels(
    attributes: &[crate::paramesh::MeshAttribute],
    vertices: usize,
    triangles: &[[u32; 3]],
    unresolved: &mut std::collections::BTreeMap<crate::paramesh::MeshAttributeDomain, usize>,
) -> Vec<cadmpeg_ir::tessellation::TessellationChannel> {
    use crate::paramesh::MeshAttributeDomain;

    let mut channels = Vec::new();
    for attribute in attributes {
        match (attribute.domain, attribute.item_size, attribute.count()) {
            (MeshAttributeDomain::Vertex, Some(item_size), Some(count)) => {
                channels.push(cadmpeg_ir::tessellation::TessellationChannel {
                    domain: cadmpeg_ir::tessellation::TessellationChannelDomain::default(),
                    item_size,
                    kind: attribute.role,
                    flags: attribute.element_code,
                    count,
                    data: attribute.values.clone(),
                    indices: Vec::new(),
                });
            }
            (MeshAttributeDomain::Corner, Some(item_size), Some(count)) => {
                let Some(selectors) = attribute.corner_selectors(vertices, triangles) else {
                    *unresolved.entry(MeshAttributeDomain::Corner).or_default() += 1;
                    continue;
                };
                channels.push(cadmpeg_ir::tessellation::TessellationChannel {
                    domain: cadmpeg_ir::tessellation::TessellationChannelDomain::Corner,
                    item_size,
                    kind: attribute.role,
                    flags: attribute.element_code,
                    count,
                    data: attribute.values.clone(),
                    indices: selectors,
                });
            }
            (MeshAttributeDomain::Triangle, Some(item_size), Some(count))
                if usize::try_from(count) == Ok(triangles.len()) && item_size == 4 =>
            {
                let Some(indices) = (0..triangles.len())
                    .map(|index| u32::try_from(index).ok())
                    .collect::<Option<Vec<_>>>()
                else {
                    *unresolved.entry(MeshAttributeDomain::Triangle).or_default() += 1;
                    continue;
                };
                channels.push(cadmpeg_ir::tessellation::TessellationChannel {
                    domain: cadmpeg_ir::tessellation::TessellationChannelDomain::Triangle,
                    item_size,
                    kind: attribute.role,
                    flags: attribute.element_code,
                    count,
                    data: attribute.values.clone(),
                    indices,
                });
            }
            (domain, _, _) => *unresolved.entry(domain).or_default() += 1,
        }
    }
    channels
}

/// Report mesh attribute channels that the projector left unresolved, grouped by
/// domain.
fn report_unresolved_mesh_attributes(
    report: &mut DecodeReport,
    unresolved: &std::collections::BTreeMap<crate::paramesh::MeshAttributeDomain, usize>,
) {
    use crate::paramesh::MeshAttributeDomain;

    for (domain, count) in unresolved {
        let (addressing, reason) = match domain {
            MeshAttributeDomain::Corner => (
                "triangle corners",
                "their indexed value table or corner selector stream has no settled layout",
            ),
            MeshAttributeDomain::Triangle => (
                "triangles",
                "their element code or value count has no settled layout",
            ),
            MeshAttributeDomain::Vertex => (
                "vertices",
                "their element code does not settle a stored element layout of one value per \
                 vertex",
            ),
        };
        report.losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::AttributesNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{count} mesh attribute channel(s) addressing {addressing} were not transferred: \
                 {reason}."
            ),
            provenance: None,
        });
    }
}

/// Record the `Properties.dat` docstruct declaration on the source metadata.
fn annotate_docstruct(ir: &mut CadIr, scan: &ContainerScan) {
    let Some(docstruct) = crate::xref::docstruct(scan) else {
        return;
    };
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("docstruct_type".into(), docstruct.doc_type);
        source
            .attributes
            .insert("docstruct_subtype".into(), docstruct.subtype);
    }
}

/// A warning for a present but unparseable `RedirectionsStream.dat`.
fn xref_parse_loss(error: &CodecError) -> LossNote {
    LossNote {
        code: LossKind::shared(LossTaxonomy::MetadataNotTransferred),
        severity: Severity::Warning,
        message: format!("external-reference table was not decoded: {error}"),
        provenance: None,
    }
}

/// Classify a mesh-body document.
///
/// Mesh bodies use tessellation as their geometry carrier. The report marks
/// geometry as transferred and records vertex precision.
fn apply_mesh_body_classification(report: &mut DecodeReport, scan: &ContainerScan, bodies: usize) {
    if container::design_breps(scan).next().is_some() {
        return;
    }
    report.losses.retain(|loss| {
        !matches!(
            loss.code.taxonomy(),
            LossTaxonomy::GeometryNotTransferred
                | LossTaxonomy::TopologyNotTransferred
                | LossTaxonomy::MissingGeometryStream
        )
    });
    report.geometry_transferred = true;
    report.losses.push(LossNote {
        code: LossKind::shared(LossTaxonomy::MeshVertexPrecision),
        severity: Severity::Warning,
        message: format!(
            "{bodies} mesh body geometry container(s) store vertex coordinates at f32 precision"
        ),
        provenance: None,
    });
}

/// Classify a bodyless design whose transferred content requires no BREP.
///
/// The container has zero BREP streams and the Design segment has zero bodies.
/// Sketch entities can supply the complete geometry. Reference-image timeline
/// objects are presentation content and require no geometry carrier.
pub(crate) fn apply_bodyless_design_classification(
    report: &mut DecodeReport,
    brep_streams: usize,
    text_brep_streams: usize,
    declared_bodies: usize,
    sketch_entities: usize,
    reference_images: usize,
) {
    if brep_streams != 0
        || text_brep_streams != 0
        || declared_bodies != 0
        || (sketch_entities == 0 && reference_images == 0)
    {
        return;
    }
    report.losses.retain(|loss| {
        !matches!(
            loss.code.taxonomy(),
            LossTaxonomy::GeometryNotTransferred
                | LossTaxonomy::TopologyNotTransferred
                | LossTaxonomy::MissingGeometryStream
        )
    });
    report.geometry_transferred = true;
    let message = match (sketch_entities, reference_images) {
        (0, reference_images) => format!(
            "presentation-only design: the document declares no body, and its {reference_images} reference-image timeline object(s) require no BREP geometry"
        ),
        (sketch_entities, 0) => format!(
            "sketch-only design: the document declares no body, and its {sketch_entities} sketch entity(s) are its complete geometry"
        ),
        (sketch_entities, reference_images) => format!(
            "bodyless design: the document declares no body; its {sketch_entities} sketch entity(s) are its complete geometry, and its {reference_images} reference-image timeline object(s) require no BREP geometry"
        ),
    };
    report.losses.push(LossNote {
        code: LossKind::shared(LossTaxonomy::CarrierSummary),
        severity: Severity::Info,
        message,
        provenance: None,
    });
}

/// Reclassify a BREP-less assembly document: its model is the placement of
/// its XREF targets, so producing no geometry is not a loss
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
fn apply_assembly_classification(
    report: &mut DecodeReport,
    scan: &ContainerScan,
    table: &crate::xref::XrefTable,
) {
    if !crate::xref::is_assembly(scan, Some(table)) {
        return;
    }
    report.losses.retain(|loss| {
        !(loss.severity >= Severity::Error
            && matches!(
                loss.code.category(),
                LossCategory::Geometry | LossCategory::Topology
            ))
    });
    report.losses.push(LossNote {
        code: LossKind::shared(LossTaxonomy::AssemblyComponentsExternal),
        severity: Severity::Info,
        message: format!(
            "assembly document: geometry is defined by {} external reference(s); decode the \
             containing .f3z archive to resolve them",
            table.references.len()
        ),
        provenance: None,
    });
    for reference in &table.references {
        let property_note = if reference.neutron_data.is_empty()
            || reference.neutron_data == reference.neutron_role
        {
            format!("neutronRole {}", reference.neutron_role)
        } else {
            format!(
                "neutronRole {}, neutronData {}",
                reference.neutron_role, reference.neutron_data
            )
        };
        let note = match crate::xref::design_for(table, reference) {
            Some(design) => format!(
                "xref {}: {} -> {} (lineage {}, version {}, {})",
                reference.ordinal,
                design.display_name,
                design.target_file_name,
                design.lineage_urn,
                design.version_urn,
                property_note
            ),
            None => format!(
                "xref {}: -> {} ({})",
                reference.ordinal, reference.relative_path, property_note
            ),
        };
        report.notes.push(note);
    }
}

fn decode_result(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
    source_image: UnknownRecord,
    admitted_entities: &mut u64,
) -> Result<DecodeResult, CodecError> {
    // ASM transfer already charged its delta; admit any remaining neutral entities
    // (sketches, appearances, products) before finalizing.
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit F3D entities",
    )?;
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "f3d", unknowns)?;
    source_fidelity.retain_unknown_records("f3d", [source_image]);
    ir.finalize();
    // Stamped last, over the finalized document, so the write path can ask
    // whether anything moved since this decode. See `document_local_sha256`.
    let hash = document_local_sha256(&ir);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.into(),
            hash,
        );
    }
    Ok(DecodeResult::new(ir, report, source_fidelity))
}

fn preserve_source_image(scan: &ContainerScan) -> UnknownRecord {
    let id = crate::ids::FILE_SOURCE_IMAGE_ID;
    UnknownRecord {
        id: UnknownId(id.into()),
        offset: 0,
        byte_len: scan.source_image.len() as u64,
        sha256: sha256_hex(scan.source_image),
        data: Some(scan.source_image.to_vec()),
        links: Vec::new(),
    }
}

/// Machine-local `document_local_sha256` for the F3D write-path edit oracle.
///
/// See [`cadmpeg_ir::hash::document_local_sha256`].
pub(crate) fn document_local_sha256(ir: &CadIr) -> String {
    cadmpeg_ir::hash::document_local_sha256(ir, "f3d", crate::ids::FILE_SOURCE_IMAGE_ID)
}

fn populate_annotations(
    ir: &CadIr,
    scan: &ContainerScan,
    native: &F3dNative,
    brep: Option<(&str, &[cadmpeg_asm::brep::AnnotationRecord])>,
    unknowns: &[UnknownRecord],
) -> cadmpeg_ir::Annotations {
    use std::collections::{HashMap, HashSet};

    let mut annotations = AnnotationBuilder::new();
    if let Some((stream_name, records)) = brep {
        let stream = annotations.stream(crate::ids::native_scope(stream_name));
        for record in records {
            annotations
                .note(&record.id, stream, record.offset)
                .tag(&record.tag);
            for field in &record.derived_fields {
                annotations.derived(&record.id, *field);
            }
        }
    }

    let mut constraints_by_native = HashMap::new();
    for constraint in &ir.model.sketch_constraints {
        if let Some(native_ref) = constraint.native_ref.as_deref() {
            constraints_by_native
                .entry(native_ref)
                .or_insert(constraint.id.0.as_str());
        }
    }
    let mut entities_by_native = HashMap::new();
    for entity in &ir.model.sketch_entities {
        if let Some(native_ref) = entity.native_ref.as_deref() {
            entities_by_native
                .entry(native_ref)
                .or_insert(entity.id.0.as_str());
        }
    }
    let planar_sketches = ir
        .model
        .sketches
        .iter()
        .map(|sketch| sketch.id.0.as_str())
        .collect::<HashSet<_>>();
    let spatial_sketches = ir
        .model
        .spatial_sketches
        .iter()
        .map(|sketch| sketch.id.0.as_str())
        .collect::<HashSet<_>>();

    let native_stream = annotations.stream("f3d:native");
    let mut note = |id: &str, tag: &str| {
        let offset = trailing_offset(id);
        annotations.note(id, native_stream, offset).tag(tag);
    };
    {
        for entity in &native.construction_recipes {
            note(&entity.id, "construction_recipe");
        }
        for entity in &native.persistent_references {
            note(&entity.id, "persistent_reference");
        }
        for entity in &native.lost_edge_references {
            note(&entity.id, "EDGE_REFERENCE_LOST");
        }
        for entity in &native.design_types {
            note(&entity.id, "design_type");
        }
        for entity in &native.design_parameters {
            note(&entity.id, "design_parameter");
        }
        for entity in &native.design_parameter_companions {
            note(&entity.id, "design_parameter_companion");
        }
        for entity in &native.design_dimension_locus_pairs {
            note(&entity.id, "design_dimension_locus_pair");
            if let Some(projected) = constraints_by_native.get(entity.id.as_str()) {
                note(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_annotation_frames {
            note(&entity.id, "design_dimension_annotation_frame");
            if let Some(projected) = constraints_by_native.get(entity.id.as_str()) {
                note(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_locus_groups {
            note(&entity.id, "design_dimension_locus_group");
            if let Some(projected) = constraints_by_native.get(entity.id.as_str()) {
                note(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_dimension_null_locus_pairs {
            note(&entity.id, "design_dimension_null_locus_pair");
            if let Some(projected) = constraints_by_native.get(entity.id.as_str()) {
                note(projected, "sketch_constraint");
            }
        }
        for entity in &native.design_parameter_owners {
            note(&entity.id, "design_parameter_owner");
        }
        for entity in &native.design_parameter_scopes {
            note(&entity.id, "design_parameter_scope");
        }
        for entity in &native.design_edge_operands {
            note(&entity.id, "design_edge_operand");
        }
        for entity in &native.design_face_operands {
            note(&entity.id, "design_face_operand");
        }
        for entity in &native.design_sketch_placements {
            note(&entity.id, "design_sketch_placement");
            let planar = crate::ids::neutral_sketch_id(entity);
            if planar_sketches.contains(planar.0.as_str()) {
                note(&planar.0, "sketch");
            }
            let spatial = crate::ids::neutral_spatial_sketch_id(entity);
            if spatial_sketches.contains(spatial.0.as_str()) {
                note(&spatial.0, "spatial_sketch");
            }
        }
        for entity in &native.design_entity_headers {
            note(&entity.id, "design_entity_header");
        }
        for entity in &native.design_record_headers {
            note(&entity.id, "design_record_header");
        }
        for entity in &native.design_body_members {
            note(&entity.id, "BodiesRoot");
        }
        for entity in &native.design_material_assignments {
            note(&entity.id, "material_assignment");
        }
        for entity in &native.sketch_relations {
            note(&entity.id, "sketch_relation");
            if constraints_by_native.contains_key(entity.id.as_str()) {
                note(
                    &crate::ids::neutral_sketch_constraint_id(&entity.id, entity.record_index).0,
                    "sketch_constraint",
                );
            }
        }
        for entity in &native.sketch_points {
            note(&entity.id, "sketch_point");
            if let Some(projected) = entities_by_native.get(entity.id.as_str()) {
                note(projected, "sketch_entity");
            }
        }
        for entity in &native.sketch_curve_identities {
            note(&entity.id, "sketch_curve");
            if let Some(projected) = entities_by_native.get(entity.id.as_str()) {
                note(projected, "sketch_entity");
            }
        }
        for entity in &native.sketch_surfaces {
            note(&entity.id, "sketch_surface");
        }
        for entity in &native.sketch_curve_links {
            note(&entity.id, "sketch_curve_link");
        }
        for entity in &native.persistent_design_links {
            note(&entity.id, "persistent_design_link");
        }
        for entity in &native.persistent_subentity_tags {
            note(&entity.id, "persistent_subentity_tag");
        }
        for entity in &native.act_entities {
            note(&entity.id, "ACTEntity");
        }
        for entity in &native.act_guids {
            note(&entity.id, "ACTGuid");
        }
        for entity in &native.act_registry_channels {
            note(&entity.id, "ACTRegistryChannel");
        }
        for entity in &native.act_root_components {
            note(&entity.id, "ACTRootComponent");
        }
        for entity in &native.act_table_references {
            note(&entity.id, "ACTTableReference");
        }
        for history in &native.asm_histories {
            note(&history.id, "history_stream");
            for state in &history.states {
                note(&state.id, "delta_state");
                for board in &state.bulletin_boards {
                    note(&board.id, "BulletinBoard");
                    for change in &board.changes {
                        note(&change.id, "entity_change");
                    }
                }
                for record in &state.records {
                    note(&record.id, &record.name);
                }
            }
        }
    }

    let appearance_stream = scan
        .entries
        .iter()
        .find(|entry| scan.is_design_asset_entry(entry, container::role::PROTEIN))
        .map(|entry| annotations.stream(crate::ids::native_scope(&entry.name)));
    if let Some(stream) = appearance_stream {
        for appearance in &ir.model.appearances {
            annotations
                .note(&appearance.id.0, stream, 0)
                .tag(appearance.schema.as_deref().unwrap_or("appearance"));
        }
    }
    for binding in &ir.model.appearance_bindings {
        annotations
            .note(&binding.id, native_stream, 0)
            .tag("appearance_binding");
    }
    if brep.is_none() {
        if let Some(fallback) = container::select_fallback_brep(scan) {
            let stream = annotations.stream(crate::ids::native_scope(&fallback.name));
            for unknown in unknowns {
                annotations
                    .note(&unknown.id.0, stream, unknown.offset)
                    .tag("opaque_brep");
            }
        }
    }
    annotations.build()
}

fn trailing_offset(id: &str) -> u64 {
    id.rsplit(':')
        .find_map(|part| part.parse::<u64>().ok())
        .unwrap_or(0)
}

fn decode_asm_history(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    history_brep: &BrepFacts,
) -> Result<Option<crate::history_records::AsmHistory>, CodecError> {
    let width = history_brep
        .header
        .as_ref()
        .map_or(8, |header| usize::from(header.width));
    let bytes = scan.entry_bytes(&history_brep.name)?;
    Ok(crate::history::decode(
        bytes,
        &history_brep.name,
        width,
        &ctx.policy().limits,
    ))
}

fn extend_related_design_records(
    scan: &ContainerScan,
    native: &mut F3dNative,
) -> Result<(), CodecError> {
    let indices = native
        .sketch_relations
        .iter()
        .flat_map(|relation| {
            let scope = crate::ids::native_stream(&relation.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            relation
                .members
                .iter()
                .chain(&relation.return_members)
                .map(move |record_index| (scope.clone(), *record_index))
        })
        .chain(native.design_parameters.iter().filter_map(|parameter| {
            Some((
                crate::ids::native_stream(&parameter.id)?.to_owned(),
                parameter.owner_record_index?,
            ))
        }))
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|scope| {
                    !existing.contains(&(scope.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_parameter_owners = crate::design::decode::parameters::decode_parameter_owners(
        scan,
        &native.design_parameters,
        &native.design_record_headers,
    )?;
    let indices = native
        .design_parameter_owners
        .iter()
        .flat_map(|owner| {
            let scope = crate::ids::native_stream(&owner.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            [
                owner.scope_record_index,
                owner.parameter_record_index,
                owner.companion_record_index,
            ]
            .map(|record_index| (scope.clone(), record_index))
        })
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|scope| {
                    !existing.contains(&(scope.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_parameter_companions =
        crate::design::decode::parameters::decode_parameter_companions(
            scan,
            &native.design_parameter_owners,
            &native.design_record_headers,
        )?;
    native.design_component_occurrences =
        crate::design::decode::components::decode_component_occurrences(scan)?;
    native.design_parameter_scopes = crate::design::decode::scopes::decode_parameter_scopes(
        scan,
        &native.design_entity_headers,
        &native.design_types,
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_component_occurrences,
        &native.construction_recipes,
    )?;
    native.design_feature_timelines = crate::design::decode::meta::decode_feature_timelines(scan)?;
    native.design_canvas_images =
        crate::design::decode::canvas::decode_canvas_images(scan, &native.design_parameter_scopes)?;
    native.design_decal_images =
        crate::design::decode::decal::decode_decal_images(scan, &native.design_parameter_scopes)?;
    crate::design::decode::operands::disambiguate_fixed_fillet_parameters(
        &mut native.design_parameter_scopes,
        &native.design_parameter_owners,
    );
    let mut existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    for scope in &native.design_parameter_scopes {
        let Some(stream) = crate::ids::native_stream(&scope.id) else {
            continue;
        };
        if existing.insert((stream.to_owned(), scope.record_index)) {
            native
                .design_record_headers
                .push(crate::records::DesignRecordHeader {
                    id: format!("{stream}:design-record-header#{}", scope.byte_offset),
                    record_index: scope.record_index,
                    class_tag: scope.class_tag.clone(),
                    byte_offset: scope.byte_offset,
                });
        }
        if let Some(operation) = &scope.copy_paste_bodies_operation {
            if existing.insert((stream.to_owned(), operation.relation_record_index)) {
                native
                    .design_record_headers
                    .push(crate::records::DesignRecordHeader {
                        id: format!(
                            "{stream}:design-record-header#{}",
                            operation.relation_byte_offset
                        ),
                        record_index: operation.relation_record_index,
                        class_tag: operation.relation_class_tag.clone(),
                        byte_offset: operation.relation_byte_offset,
                    });
            }
        }
    }
    let indices = native
        .design_parameter_scopes
        .iter()
        .flat_map(|scope| {
            let stream = crate::ids::native_stream(&scope.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            scope
                .reference_members
                .iter()
                .map(move |record_index| (stream.clone(), *record_index))
        })
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    crate::design::decode::operands::bind_sketch_profiles(
        scan,
        &mut native.design_parameter_scopes,
        &native.design_record_headers,
        &native.design_entity_headers,
    )?;
    native.design_construction_operand_groups =
        crate::design::decode::operands::decode_construction_operand_groups(
            scan,
            &mut native.design_parameter_scopes,
            &native.design_record_headers,
        )?;
    crate::design::decode::scopes::bind_mirror_constructions(
        scan,
        &mut native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.design_parameter_owners,
    )?;
    native.design_extrude_selection_groups =
        crate::design::decode::operands::decode_extrude_selection_groups(
            scan,
            &native.design_parameter_scopes,
            &native.design_record_headers,
        )?;
    let mut indices = native
        .design_extrude_selection_groups
        .iter()
        .flat_map(|group| {
            let stream = crate::ids::native_stream(&group.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            group
                .members
                .iter()
                .map(move |record_index| (stream.clone(), *record_index))
        })
        .collect::<Vec<_>>();
    indices.extend(
        native
            .design_construction_operand_groups
            .iter()
            .flat_map(|group| {
                let stream = crate::ids::native_stream(&group.id)
                    .unwrap_or(crate::ids::DEFAULT_STREAM)
                    .to_owned();
                group
                    .members
                    .iter()
                    .copied()
                    .chain(
                        group
                            .frame
                            .trailing_record_indices
                            .iter()
                            .flat_map(|record_index| {
                                std::iter::once(*record_index)
                                    .chain(record_index.checked_add(1))
                                    .chain(record_index.checked_add(2))
                                    .chain(record_index.checked_add(3))
                            }),
                    )
                    .chain(
                        group
                            .frame
                            .auxiliary_record_indices
                            .iter()
                            .flat_map(|record_index| {
                                std::iter::once(*record_index)
                                    .chain(record_index.checked_add(1))
                                    .chain(record_index.checked_add(2))
                            }),
                    )
                    .map(move |record_index| (stream.clone(), record_index))
            }),
    );
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    crate::design::decode::operands::bind_construction_operand_trailing_records(
        scan,
        &mut native.design_construction_operand_groups,
        &native.design_record_headers,
    )?;
    crate::design::decode::operands::bind_construction_operand_paths(
        scan,
        &mut native.design_construction_operand_groups,
        &native.design_record_headers,
    )?;
    native.design_construction_operand_identities =
        crate::design::decode::operands::decode_construction_operand_identities(
            scan,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    let scopes = native
        .design_parameter_scopes
        .iter()
        .filter_map(|scope| {
            Some((
                (
                    crate::ids::native_stream(&scope.id)?.to_owned(),
                    scope.record_index,
                ),
                scope.kind.as_str(),
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let identified_groups = native
        .design_construction_operand_identities
        .iter()
        .filter_map(|identity| {
            Some((
                crate::ids::native_stream(&identity.id)?.to_owned(),
                identity.group_record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_edge_identity_operands =
        crate::design::decode::operands::decode_edge_identity_operands(
            scan,
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    let identity_member_groups = native
        .design_edge_identity_operands
        .iter()
        .filter_map(|operand| {
            Some((
                crate::ids::native_stream(&operand.id)?.to_owned(),
                operand.group_record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_construction_operand_groups.retain(|group| {
        let Some(stream) = crate::ids::native_stream(&group.id) else {
            return true;
        };
        let kind = scopes
            .get(&(stream.to_owned(), group.scope_record_index))
            .copied();
        crate::design::decode::operands::construction_operand_group_is_retained(
            kind,
            identified_groups.contains(&(stream.to_owned(), group.record_index))
                || identity_member_groups.contains(&(stream.to_owned(), group.record_index)),
        )
    });
    native.design_fillet_radius_groups =
        crate::design::decode::operands::decode_fillet_radius_groups(
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_parameter_owners,
            &native.design_parameters,
        );
    crate::design::decode::operands::bind_lost_edge_groups(
        &mut native.design_construction_operand_groups,
        &native.design_construction_operand_identities,
        &native.lost_edge_references,
    )?;
    let indices = native
        .design_construction_operand_identities
        .iter()
        .flat_map(|identity| {
            let stream = crate::ids::native_stream(&identity.id)
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            identity
                .wrapper_record_indices
                .iter()
                .copied()
                .chain(std::iter::once(identity.following_record_index))
                .map(move |record_index| (stream.clone(), record_index))
        })
        .collect::<Vec<_>>();
    let existing = native
        .design_record_headers
        .iter()
        .filter_map(|record| {
            Some((
                crate::ids::native_stream(&record.id)?.to_owned(),
                record.record_index,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    native.design_record_headers.extend(
        crate::design::decode::sketch::decode_related_record_headers(scan, &indices)?
            .into_iter()
            .filter(|record| {
                crate::ids::native_stream(&record.id).is_none_or(|stream| {
                    !existing.contains(&(stream.to_owned(), record.record_index))
                })
            }),
    );
    native
        .design_record_headers
        .sort_by_key(|record| record.id.clone());
    native.design_extrude_selection_members =
        crate::design::decode::operands::decode_extrude_selection_members(
            scan,
            &native.design_extrude_selection_groups,
            &native.design_record_headers,
        )?;
    native.design_entity_selection_operands =
        crate::design::decode::operands::decode_entity_selection_operands(
            scan,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
        )?;
    crate::history::bind_entity_selection_history(
        &mut native.design_entity_selection_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    crate::history::bind_mirror_selection_planes(
        &mut native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_entity_selection_operands,
        &native.design_construction_operand_identities,
        &native.asm_histories,
    );
    native.design_body_recipe_operands =
        crate::design::decode::operands::decode_body_recipe_operands(
            scan,
            &native.design_parameter_scopes,
            &native.design_construction_operand_groups,
            &native.design_record_headers,
            &native.construction_recipes,
        )?;
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        &mut native.design_body_recipe_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
        &native.design_parameter_scopes,
    );
    crate::history::bind_body_recipe_operand_history_candidates(
        &mut native.design_body_recipe_operands,
        &native.construction_recipes,
        &native.design_parameter_scopes,
        &native.asm_histories,
    );
    crate::design::decode::operands::bind_extrude_selection_identities(
        &mut native.design_extrude_selection_members,
        &native.design_construction_operand_identities,
    );
    crate::history::bind_extrude_selection_history(
        &mut native.design_extrude_selection_members,
        &native.asm_histories,
    );
    let scope_histories = crate::history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    crate::history::bind_circular_pattern_axes(
        &mut native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    );
    crate::history::bind_edge_identity_history(
        &mut native.design_edge_identity_operands,
        &native.design_construction_operand_identities,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    );
    native.design_edge_operands = crate::design::decode::operands::decode_edge_operands(
        scan,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.construction_recipes,
    )?;
    crate::design::decode::operands::bind_edge_operand_candidates(
        &mut native.design_edge_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
    );
    crate::history::bind_edge_operand_history_candidates(
        &mut native.design_edge_operands,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    );
    crate::design::decode::operands::bind_work_point_input_carriers(
        scan,
        &mut native.design_parameter_scopes,
        &native.design_record_headers,
        &native.construction_recipes,
        &native.design_edge_operands,
    )?;
    crate::design::decode::operands::bind_work_plane_constructions(
        scan,
        &mut native.design_parameter_scopes,
        &native.design_record_headers,
        &native.construction_recipes,
        &native.design_parameter_owners,
        &native.design_parameters,
    )?;
    crate::design::decode::operands::bind_vertex_recipe_candidates(
        &mut native.design_parameter_scopes,
        &native.persistent_subentity_tags,
    );
    crate::history::bind_vertex_recipe_history(
        &mut native.design_parameter_scopes,
        &native.design_feature_timelines,
        &native.asm_histories,
    )?;
    native.design_face_operands = crate::design::decode::operands::decode_face_operands(
        scan,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.design_record_headers,
        &native.construction_recipes,
    )?;
    crate::design::decode::operands::bind_face_operand_candidates(
        &mut native.design_face_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
    );
    crate::history::bind_face_operand_history_candidates(
        &mut native.design_face_operands,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.asm_histories,
    );
    crate::history::bind_edge_identity_bounded_face_rules(
        &mut native.design_edge_identity_operands,
        &native.design_face_operands,
    );
    native.design_sketch_placements = crate::design::decode::sketch::decode_sketch_placements(
        scan,
        &native.design_parameter_scopes,
        &native.design_entity_headers,
    )?;
    let stream_lengths: std::collections::HashMap<String, usize> = scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, container::role::BULKSTREAM))
        .map(|entry| {
            scan.entry_bytes(&entry.name)
                .map(|bytes| (crate::ids::native_scope(&entry.name), bytes.len()))
        })
        .collect::<Result<_, _>>()?;
    crate::design::decode::parameters::bind_parameter_companion_payloads(
        &mut native.design_parameter_companions,
        &native.design_parameters,
        &native.design_parameter_owners,
        &native.design_parameter_scopes,
        &native.design_entity_headers,
        &native.design_record_headers,
        &native.construction_recipes,
        &stream_lengths,
    );
    native.design_dimension_recipe_records =
        crate::design::decode::dimension_frames::decode_dimension_recipe_records(
            scan,
            &native.design_parameters,
            &native.design_parameter_owners,
            &native.design_parameter_companions,
            &native.construction_recipes,
        )?;
    crate::design::decode::dimension_frames::bind_dimension_recipe_reference_candidates(
        &mut native.design_dimension_recipe_records,
        &native.persistent_subentity_tags,
    );
    crate::design::decode::dimension_frames::bind_dimension_recipe_edge_operands(
        &mut native.design_dimension_recipe_records,
        &native.design_edge_operands,
    );
    Ok(())
}

/// Frame and decode one Design-selected or explicit fallback BREP SAB stream.
///
/// The function returns `None` for an invalid header or a framed stream with no
/// geometry. The caller then builds the container-metadata IR.
fn try_decode_brep(
    scan: &ContainerScan,
    brep_entry: &BrepFacts,
) -> Result<Option<Brep>, CodecError> {
    let width = brep_entry.header.as_ref().map_or(0, |h| h.width);
    if width != 4 && width != 8 {
        return Ok(None);
    }

    let bytes = scan.entry_bytes(&brep_entry.name)?;
    let Some(start) = asm_header::record_stream_start(bytes) else {
        return Ok(None);
    };
    // A stream without a delta-state boundary is history-less: its final
    // `End-of-ASM-data` record ends at EOF without the `0x11` terminator, so
    // it needs the EOF-tolerant framer used for the history partition.
    let framed = match brep_entry.solved_record_limit {
        Some(limit) => sab::frame(bytes, start, limit, usize::from(width)),
        None => sab::frame_history(bytes, start, bytes.len(), usize::from(width)),
    };
    let records = match framed {
        Ok(r) if !r.is_empty() => r,
        _ => return Ok(None),
    };

    let decoded = brep::decode(&records, bytes, &brep_entry.name, crate::ids::ID_FORMAT);
    if decoded.asm.surfaces.is_empty()
        && decoded.asm.points.is_empty()
        && decoded.asm.faces.is_empty()
    {
        return Ok(None);
    }
    Ok(Some(decoded))
}

/// Assemble the IR document from the decoded B-rep graph.
fn build_geometry_ir(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    primary_model_brep: &BrepFacts,
    brep: Brep,
) -> Result<(CadIr, F3dNative, AsmTransferRemainder), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let (source, tolerances) = source_and_tolerances(scan, primary_model_brep);
    ir.source = Some(source);
    ir.tolerances = tolerances;
    let Brep {
        asm,
        sketch_curve_links,
        persistent_design_links,
        persistent_subentity_tags,
        creation_timestamps,
    } = brep;
    let remainder = transfer_into_ir(ctx, &mut ir, "f3d", F3D_NATIVE_VERSION, asm)?;
    let mut native = F3dNative::load(
        ir.native
            .namespace("f3d")
            .expect("ASM transfer creates the requested native namespace"),
    )?;
    native.sketch_curve_links = sketch_curve_links;
    native.persistent_design_links = persistent_design_links;
    native.persistent_subentity_tags = persistent_subentity_tags;
    native.creation_timestamps = creation_timestamps;
    Ok((ir, native, remainder))
}

/// Source metadata attributes and kernel tolerances from the primary model BREP header.
fn source_and_tolerances(
    scan: &ContainerScan,
    primary_model_brep: &BrepFacts,
) -> (SourceMeta, Tolerances) {
    let mut attributes = std::collections::BTreeMap::new();
    if let Some(folder) = scan.design_asset_folder() {
        attributes.insert("asset_folder".to_string(), folder.to_owned());
    }
    attributes.insert(
        "zip_entry_count".to_string(),
        scan.entries.len().to_string(),
    );
    attributes.insert("active_brep".to_string(), primary_model_brep.name.clone());
    attributes.insert(
        "active_brep_sha256".to_string(),
        primary_model_brep.sha256.clone(),
    );
    if let Some(off) = primary_model_brep.solved_record_limit {
        attributes.insert("solved_record_len".to_string(), off.to_string());
    }
    if let Some(unit) = crate::design::decode::units::decode_document_length_unit(scan) {
        attributes.insert("modeling_length_unit".to_string(), unit);
    }

    let mut tolerances = Tolerances::default();
    if let Some(h) = &primary_model_brep.header {
        if let Some(pf) = &h.product_family {
            attributes.insert("product_family".to_string(), pf.clone());
        }
        if let Some(pv) = &h.product_version {
            attributes.insert("product_version".to_string(), pv.clone());
        }
        if let Some(sd) = &h.save_date {
            attributes.insert("save_date".to_string(), sd.clone());
        }
        if let (Some(resabs), Some(resnor)) = (h.linear, h.angular) {
            tolerances = Tolerances {
                linear: resabs,
                angular: resnor,
            };
        }
    }

    (
        SourceMeta {
            format: "f3d".to_string(),
            attributes,
        },
        tolerances,
    )
}

/// Loss report for a successful geometry decode.
fn format_kind_counts(counts: &std::collections::BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn build_geometry_report(scan: &ContainerScan, decoded: &Brep) -> DecodeReport {
    let s = &decoded.asm.stats;
    let mut losses = Vec::new();

    if s.nurbs_surfaces > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "{} spline surface record(s) were decoded into NURBS carriers from their inline \
                 cached B-spline block.",
                s.nurbs_surfaces
            ),
            provenance: None,
        });
    }
    if s.nurbs_curves > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "{} procedural curve record(s) were decoded into NURBS carriers from their inline \
                 cached 3D B-spline block.",
                s.nurbs_curves
            ),
            provenance: None,
        });
    }
    if s.missing_face_surfaces > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::ReferenceGraphNotClosed),
            severity: Severity::Warning,
            message: format!(
                "{} face(s) were omitted because their required surface reference was null or dangling. Reference conditions: {}.",
                s.missing_face_surfaces,
                format_kind_counts(&s.missing_face_surface_kinds)
            ),
            provenance: None,
        });
    }
    if s.unknown_surface_faces > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{} face(s) rest on spline/procedural surfaces whose shape was not decoded into a \
                 typed carrier (no inline cached B-spline block: the cache is reached through a \
                 subtype reference, or the record is a procedural form this codec does not \
                 evaluate); the face, its loops, and trims are emitted with an unknown-geometry \
                 surface linking to the preserved record bytes. Topology is transferred; the \
                 underlying surface shape is not. Native kinds: {}.",
                s.unknown_surface_faces,
                format_kind_counts(&s.unknown_surface_kinds)
            ),
            provenance: None,
        });
    }
    if s.mesh_surface_faces > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "{} face(s) use zero-payload mesh_surface sentinels. Their exact surfaces are absent by definition; the emitted unknown surface preserves that distinction from tessellation attributes.",
                s.mesh_surface_faces
            ),
            provenance: None,
        });
    }
    if s.procedural_curve_edges > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::ProceduralReduced),
            severity: Severity::Warning,
            message: format!(
                "{} edge(s) reference a procedural intcurve/spline 3D curve with no decodable inline \
                 B-spline cache; the edge was emitted with its vertices and parameter range but no \
                 attributed curve carrier. Native kinds: {}.",
                s.procedural_curve_edges,
                format_kind_counts(&s.procedural_curve_kinds)
            ),
            provenance: None,
        });
    }
    if s.undecoded_pcurve_refs > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::ReferenceGraphNotClosed),
            severity: Severity::Warning,
            message: format!(
                "{} coedge(s) carry an explicit UV pcurve reference with no decodable 2D \
                 carrier on the face surface's parameterization; those coedges were emitted \
                 without a pcurve. Native kinds: {}.",
                s.undecoded_pcurve_refs,
                format_kind_counts(&s.undecoded_pcurve_kinds)
            ),
            provenance: None,
        });
    }
    if s.partial_procedural_supports > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::ProceduralReduced),
            severity: Severity::Warning,
            message: format!(
                "{} rolling-ball blend definition(s) retain their signed radius and solved cache, but only one of two native supports resolved.",
                s.partial_procedural_supports
            ),
            provenance: None,
        });
    }
    if s.other_records > 0 {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{} solved-record application/refinement record(s) were not transferred: {}.",
                s.other_records,
                s.other_record_kinds
                    .iter()
                    .map(|(name, count)| format!("{name}={count}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            provenance: None,
        });
    }
    losses.push(LossNote {
        code: LossKind::shared(LossTaxonomy::MaterialNotTransferred),
        severity: Severity::Warning,
        message: "Materials/appearances (.protein assets, ACT/design assignments) were not \
                  transferred."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "f3d".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: container::summarize(scan)
            .notes
            .into_iter()
            .filter(|note| !note.starts_with("container-level inspection only"))
            .collect(),
    }
}

fn build_metadata_ir(scan: &ContainerScan) -> (CadIr, Vec<UnknownRecord>) {
    let mut ir = CadIr::empty(Units::default());
    let mut unknowns = Vec::new();

    let mut attributes = std::collections::BTreeMap::new();
    if let Some(folder) = scan.design_asset_folder() {
        attributes.insert("asset_folder".to_string(), folder.to_owned());
    }
    attributes.insert(
        "zip_entry_count".to_string(),
        scan.entries.len().to_string(),
    );
    if let Some(unit) = crate::design::decode::units::decode_document_length_unit(scan) {
        attributes.insert("modeling_length_unit".to_string(), unit);
    }

    if let Some(brep) = container::select_fallback_brep(scan) {
        attributes.insert("active_brep".to_string(), brep.name.clone());
        attributes.insert("active_brep_sha256".to_string(), brep.sha256.clone());
        if let Some(off) = brep.solved_record_limit {
            attributes.insert("solved_record_len".to_string(), off.to_string());
        }
        if let Some(h) = &brep.header {
            if let Some(pf) = &h.product_family {
                attributes.insert("product_family".to_string(), pf.clone());
            }
            if let Some(pv) = &h.product_version {
                attributes.insert("product_version".to_string(), pv.clone());
            }
            if let Some(sd) = &h.save_date {
                attributes.insert("save_date".to_string(), sd.clone());
            }
            if let (Some(resabs), Some(resnor)) = (h.linear, h.angular) {
                ir.tolerances = Tolerances {
                    linear: resabs,
                    angular: resnor,
                };
            }
        }

        unknowns.push(UnknownRecord {
            id: UnknownId(crate::ids::native_scoped_id(&brep.name, "unknown", 0)),
            offset: 0,
            byte_len: brep.uncompressed_len,
            sha256: brep.sha256.clone(),
            data: None,
            links: Vec::new(),
        });
    }

    ir.source = Some(SourceMeta {
        format: "f3d".to_string(),
        attributes,
    });
    (ir, unknowns)
}

/// Build geometry and topology loss notes from the container state.
///
/// The report names the BREP carrier state. A failed binary decode gets a
/// decode-failure note. Each remaining state gets its own loss description.
fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let brep_count = container::design_breps(scan).count();
    let selected = container::select_fallback_brep(scan);
    let text_breps = container::text_brep_names(scan);

    let (geometry, topology) = match (brep_count, selected) {
        // The text carrier is present but its decode produced no geometry.
        (0, _) if !text_breps.is_empty() => (
            format!(
                "ASM BREP geometry was not transferred: the document's only geometry carrier is \
                 the text-encoded ASM stream(s) `{}`, and their decode produced no surfaces, \
                 curves, or points.",
                text_breps.join("`, `")
            ),
            format!(
                "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not \
                 built from the text-encoded carrier(s) `{}`.",
                text_breps.join("`, `")
            ),
        ),
        (0, _) => (
            "ASM BREP geometry was not transferred: the container declares no ASM BREP stream, so \
             no surfaces, curves, or points were produced."
                .to_string(),
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built: \
             the container declares no ASM BREP stream."
                .to_string(),
        ),
        (_, Some(brep)) => (
            format!(
                "ASM BREP geometry was not transferred: the selected stream `{}` is not a \
                 decodable BinaryFile4/BinaryFile8 SAB (or its framing failed). {brep_count} BREP \
                 stream(s) were located, but no surfaces, curves, or points were produced.",
                brep.name
            ),
            format!(
                "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not \
                 built for the selected stream `{}`.",
                brep.name
            ),
        ),
        (_, None) => (
            format!(
                "ASM BREP geometry was not transferred: {brep_count} BREP stream(s) were located, \
                 but none of them is the document's geometry stream. The Design body map that \
                 binds a body to its blob was not read, so the selection is ambiguous."
            ),
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built: \
             no BREP stream was selected."
                .to_string(),
        ),
    };

    let mut losses = vec![
        LossNote {
            code: LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Blocking,
            message: geometry,
            provenance: None,
        },
        LossNote {
            code: LossKind::shared(LossTaxonomy::TopologyNotTransferred),
            severity: Severity::Blocking,
            message: topology,
            provenance: None,
        },
        LossNote {
            code: LossKind::shared(LossTaxonomy::MaterialNotTransferred),
            severity: Severity::Warning,
            message: "Materials/appearances (.protein assets, ACT/design assignments) were not \
                      transferred."
                .to_string(),
            provenance: None,
        },
    ];

    // An absent carrier and an unselectable carrier produce different findings.
    // Full decode rejects an ambiguous selection before it builds this report.
    if selected.is_none() {
        losses.push(LossNote {
            code: LossKind::shared(LossTaxonomy::MissingGeometryStream),
            severity: Severity::Error,
            message: if brep_count == 0 && !text_breps.is_empty() {
                format!(
                    "{} ASM BREP stream(s) are present in the text encoding (.sat/.smt) and \
                     produced no geometry; no binary stream (.smb/.smbh) was found",
                    text_breps.len()
                )
            } else if brep_count == 0 {
                "no ASM BREP stream (.smb/.smbh) was found in the container".to_string()
            } else {
                format!(
                    "{brep_count} ASM BREP stream(s) are present, but none of them was selected as \
                     the document's geometry stream"
                )
            },
            provenance: None,
        });
    }

    DecodeReport {
        format: "f3d".to_string(),
        container_only,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        // `summarize` contains advice for container inspection. Full decode has
        // already run the design and body-binding passes, so drop that advice.
        notes: summary
            .notes
            .into_iter()
            .filter(|note| container_only || !note.starts_with("container-level inspection only"))
            .collect(),
    }
}

/// Resolve the appearance loss note against the appearances in the IR.
///
/// The report adds this note before appearance decoding. This function removes
/// it when the IR carries a complete document-local catalog and keeps it when a
/// serialized assignment failed to resolve.
pub(crate) fn reconcile_appearance_loss(
    report: &mut DecodeReport,
    ir: &CadIr,
    has_topology_assignments: bool,
) {
    if ir.model.appearances.is_empty() {
        return;
    }
    if has_topology_assignments && ir.model.appearance_bindings.is_empty() {
        if let Some(loss) = report
            .losses
            .iter_mut()
            .find(|loss| loss.code.category() == LossCategory::Material)
        {
            loss.message = format!(
                "{} Protein appearance asset(s) were decoded, but no topology assignment was resolved.",
                ir.model.appearances.len()
            );
        }
        return;
    }
    report
        .losses
        .retain(|loss| loss.code.category() != LossCategory::Material);
}

/// Join per-face appearance assignments to BREP faces through the face GUID
/// carried by each face's `NEUTRON_Material_attrib_def` attribute
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
pub(crate) fn resolve_face_appearance_bindings(
    ir: &mut CadIr,
    face_assignments: &[materials::FaceAppearanceAssignment],
) -> Result<(), CodecError> {
    use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use std::collections::btree_map::Entry;

    if face_assignments.is_empty() {
        return Ok(());
    }

    let mut assignments_by_guid = std::collections::BTreeMap::new();
    for assignment in face_assignments {
        match assignments_by_guid.entry(assignment.face_guid.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(assignment.clone());
            }
            Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                if !materials::visual_tokens_match(&existing.visual_guid, &assignment.visual_guid) {
                    return Err(CodecError::Malformed(format!(
                        "F3D face material GUID {} carries conflicting visual tokens",
                        assignment.face_guid
                    )));
                }
                match (existing.color, assignment.color) {
                    (Some(left), Some(right)) if left != right => {
                        return Err(CodecError::Malformed(format!(
                            "F3D face material GUID {} carries conflicting neutral colors",
                            assignment.face_guid
                        )));
                    }
                    (None, Some(color)) => existing.color = Some(color),
                    _ => {}
                }
            }
        }
    }

    let mut faces_by_guid =
        std::collections::BTreeMap::<String, Vec<cadmpeg_ir::ids::FaceId>>::new();
    let mut guid_by_face = std::collections::BTreeMap::<cadmpeg_ir::ids::FaceId, String>::new();
    for attribute in &ir.model.attributes {
        let AttributeTarget::Face(face) = &attribute.target else {
            continue;
        };
        let strings: Vec<&str> = attribute
            .values
            .iter()
            .filter_map(|value| match value {
                AttributeValue::String(value) => Some(value.as_str()),
                _ => None,
            })
            .collect();
        let material_name_count = strings
            .iter()
            .filter(|value| **value == "NEUTRON_Material_attrib_def")
            .count();
        if material_name_count == 0 {
            continue;
        }
        if material_name_count != 1 {
            return Err(CodecError::Malformed(
                "F3D face material attribute repeats its attribute-definition name".into(),
            ));
        }
        let mut face_guids = strings.iter().copied().filter(|value| {
            crate::bytes::is_guid_hyphenated(value)
                && value.bytes().all(|byte| !byte.is_ascii_uppercase())
        });
        let Some(face_guid) = face_guids.next() else {
            return Err(CodecError::Malformed(
                "F3D face material attribute does not carry exactly one lower-case face GUID"
                    .into(),
            ));
        };
        if face_guids.next().is_some() {
            return Err(CodecError::Malformed(
                "F3D face material attribute does not carry exactly one lower-case face GUID"
                    .into(),
            ));
        }
        if let Some(previous) = guid_by_face.insert(face.clone(), face_guid.to_owned()) {
            if previous != face_guid {
                return Err(CodecError::Malformed(format!(
                    "F3D face {face} carries multiple material GUIDs"
                )));
            }
        }
        faces_by_guid
            .entry(face_guid.to_owned())
            .or_default()
            .push(face.clone());
    }
    for faces in faces_by_guid.values_mut() {
        faces.sort();
        faces.dedup();
    }
    let mut bound_targets = ir
        .model
        .appearance_bindings
        .iter()
        .map(|binding| (binding.target.clone(), binding.appearance.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let face_indices = ir
        .model
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| (face.id.clone(), index))
        .collect::<std::collections::HashMap<_, _>>();
    for assignment in assignments_by_guid.values() {
        let Some(faces) = faces_by_guid.get(assignment.face_guid.as_str()) else {
            continue;
        };
        let appearance = materials::appearance_for_visual_token(
            &ir.model.appearances,
            &assignment.visual_guid,
            None,
        )?
        .map(|appearance| appearance.id.clone());
        for face in faces {
            if let Some(color) = assignment.color {
                if let Some(index) = face_indices.get(face).copied() {
                    let target = &mut ir.model.faces[index];
                    if target.color.is_none() {
                        target.color = Some(color);
                    }
                }
            }
            let Some(appearance) = appearance.as_ref() else {
                continue;
            };
            let target = AppearanceTarget::Face(face.clone());
            match bound_targets.entry(target.clone()) {
                std::collections::hash_map::Entry::Occupied(entry) => {
                    if entry.get() != appearance {
                        return Err(CodecError::Malformed(format!(
                            "F3D face {face} carries conflicting appearance assignments"
                        )));
                    }
                    continue;
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(appearance.clone());
                }
            }
            ir.model.appearance_bindings.push(AppearanceBinding {
                // The face id completes the key: one appearance attribute GUID
                // reaches every face carrying it, so the assignment pair alone
                // repeats across those faces.
                id: crate::ids::neutral_face_appearance_binding_id(
                    &assignment.face_guid,
                    &assignment.visual_guid,
                    face,
                ),
                target,
                appearance: appearance.clone(),
                source_entity_id: None,
                object_type: None,
                channels: std::collections::BTreeMap::new(),
            });
        }
    }
    Ok(())
}

/// Fill absent explicit topology colors from uniquely bound appearance assets.
/// Native RGB/truecolor attributes remain authoritative on the same target.
fn apply_appearance_base_colors(ir: &mut CadIr) {
    use cadmpeg_ir::appearance::AppearanceTarget;

    let colors = ir
        .model
        .appearances
        .iter()
        .filter_map(|appearance| Some((appearance.id.clone(), appearance.base_color?)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut targets = std::collections::HashMap::new();
    let mut ambiguous = std::collections::HashSet::new();
    for binding in &ir.model.appearance_bindings {
        let Some(color) = colors.get(&binding.appearance).copied() else {
            continue;
        };
        if targets.insert(binding.target.clone(), color).is_some() {
            ambiguous.insert(binding.target.clone());
        }
    }
    for body in &mut ir.model.bodies {
        let target = AppearanceTarget::Body(body.id.clone());
        if body.color.is_none() && !ambiguous.contains(&target) {
            body.color = targets.get(&target).copied();
        }
    }
    for face in &mut ir.model.faces {
        let target = AppearanceTarget::Face(face.id.clone());
        if face.color.is_none() && !ambiguous.contains(&target) {
            face.color = targets.get(&target).copied();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_appearance_base_colors, bind_mesh_feature_definitions,
        container_only_dimension_parameters, design_projection_gaps, face_selection_is_resolved,
        feature_definition_is_incomplete, incomplete_feature_families, mesh_attribute_channels,
        mesh_texture_assignments, unresolved_dimension_companion_count, DesignProjectionGaps,
        MeshProjection,
    };
    use crate::native::F3dNative;
    use crate::records::{
        DesignBodyBinding, DesignDimensionLocusPair, DesignDimensionNullLocusPair,
        DesignDimensionRecipeRecord, DesignFeatureTimeline, DesignParameter,
        DesignParameterCompanion, DesignParameterKind, DesignParameterOwner, DesignParameterScope,
        DesignSketchPlacement, LostEdgeReference, SketchCurveIdentity, SketchPoint, SketchRelation,
    };

    #[test]
    fn mesh_feature_binds_tessellations_in_design_body_order() {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let scope_id = "f3d:Design/BulkStream.dat:design-parameter-scope#10";
        let mut scope = DesignParameterScope::empty(scope_id, "Base Mesh Feature", 10);
        // The feature's owning entity reference is distinct from its scope index.
        scope.reference_members = vec![221];
        let mut features = vec![Feature {
            id: FeatureId("feature:mesh-import".into()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: Some("Base Mesh Feature".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "Base Mesh Feature".into(),
                parameters: std::collections::BTreeMap::new(),
                properties: std::collections::BTreeMap::new(),
            },
            native_ref: Some(scope_id.into()),
        }];
        let projection = MeshProjection {
            count: 2,
            tessellations_by_scope: std::collections::HashMap::from([(
                ("f3d:Design/BulkStream.dat".into(), 10),
                vec!["tessellation:z-body".into(), "tessellation:a-body".into()],
            )]),
        };

        bind_mesh_feature_definitions(&mut features, &[scope], &projection);

        assert_eq!(
            features[0].definition,
            FeatureDefinition::MeshImport {
                tessellations: vec!["tessellation:z-body".into(), "tessellation:a-body".into(),],
            }
        );
        assert!(!feature_definition_is_incomplete(&features[0].definition));
    }

    #[test]
    fn mesh_texture_ids_resolve_through_design_table_order() {
        use cadmpeg_ir::assets::AssetId;
        use cadmpeg_ir::tessellation::TessellationTextureAssignment;

        let first = AssetId("asset:first".into());
        let second = AssetId("asset:second".into());
        let textures = [
            ("resource:first".into(), first.clone()),
            ("resource:second".into(), second.clone()),
            ("resource:third".into(), first.clone()),
        ];
        assert_eq!(
            mesh_texture_assignments(Some(&[0, 2, 1, 3, 2]), &textures, 5)
                .expect("texture assignments"),
            vec![
                TessellationTextureAssignment {
                    source_id: Some("resource:first".into()),
                    texture: first,
                    triangles: vec![2],
                },
                TessellationTextureAssignment {
                    source_id: Some("resource:second".into()),
                    texture: second,
                    triangles: vec![1, 4],
                },
                TessellationTextureAssignment {
                    source_id: Some("resource:third".into()),
                    texture: AssetId("asset:first".into()),
                    triangles: vec![3],
                },
            ]
        );
        assert!(matches!(
            mesh_texture_assignments(
                Some(&[2]),
                &[("resource:only".into(), AssetId("asset:only".into()))],
                1,
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn indexed_mesh_channels_project_default_and_override_selectors() {
        let attribute = crate::paramesh::MeshAttribute {
            role: 4,
            resource_guid: None,
            authored_name: None,
            groups: Vec::new(),
            element_code: 4,
            domain: crate::paramesh::MeshAttributeDomain::Corner,
            item_size: Some(1),
            values: vec![10, 11, 12, 13, 14],
            indices: Some(vec![0, 2]),
            triangle_values: None,
        };
        let mut unresolved = std::collections::BTreeMap::new();
        let channels = mesh_attribute_channels(&[attribute], 3, &[[0, 1, 2]], &mut unresolved);

        assert!(unresolved.is_empty());
        assert_eq!(channels.len(), 1);
        assert_eq!(
            channels[0].domain,
            cadmpeg_ir::tessellation::TessellationChannelDomain::Corner
        );
        assert_eq!(channels[0].count, 5);
        assert_eq!(channels[0].indices, [3, 1, 4]);
        assert_eq!(channels[0].data, [10, 11, 12, 13, 14]);
    }

    #[test]
    fn presentation_timeline_objects_are_not_incomplete_modeling_features() {
        let native = |kind: &str| cadmpeg_ir::features::FeatureDefinition::Native {
            kind: kind.into(),
            parameters: std::collections::BTreeMap::new(),
            properties: std::collections::BTreeMap::new(),
        };

        assert!(!feature_definition_is_incomplete(&native("Canvas")));
        assert!(!feature_definition_is_incomplete(&native("Decal")));
        assert!(feature_definition_is_incomplete(&native("Fillet")));
    }

    #[test]
    fn full_round_fillet_with_automatic_sides_is_complete() {
        use cadmpeg_ir::features::{
            FaceSelection, Feature, FeatureDefinition, FeatureId, FullRoundFilletGroup,
            FullRoundSideSelection,
        };

        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.features.push(Feature {
            id: FeatureId("feature:full-round".into()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: Some("Fillet".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::FullRoundFillet {
                groups: vec![FullRoundFilletGroup {
                    center_faces: FaceSelection::Resolved {
                        faces: vec!["face:center".into()],
                        native: "native:center-group".into(),
                    },
                    side_one_faces: FullRoundSideSelection::Automatic,
                    side_two_faces: FullRoundSideSelection::Automatic,
                }],
            },
            native_ref: None,
        });

        assert!(!feature_definition_is_incomplete(
            &ir.model.features[0].definition
        ));
        assert_eq!(
            design_projection_gaps(&ir, &F3dNative::default()).incomplete_features,
            0
        );
    }

    #[test]
    fn extrude_completeness_requires_resolved_profile_start_and_termination() {
        let extrude = |profile: serde_json::Value,
                       start: serde_json::Value,
                       termination: serde_json::Value| {
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(serde_json::json!({
                "definition": "extrude",
                "profile": profile,
                "start": start,
                "extent": {
                    "kind": "one_sided",
                    "side": {"termination": termination}
                },
                "solid": true,
                "op": "new_body"
            }))
            .expect("Extrude definition")
        };
        let sketch_profile = serde_json::json!({
            "kind": "sketch_profiles",
            "value": {"sketch": "sketch:1", "profiles": [0]}
        });
        let profile_start = serde_json::json!({"kind": "profile_plane"});
        let blind = serde_json::json!({"kind": "blind", "length": 10.0});

        assert!(!feature_definition_is_incomplete(&extrude(
            sketch_profile.clone(),
            profile_start.clone(),
            blind.clone(),
        )));
        assert!(feature_definition_is_incomplete(&extrude(
            serde_json::json!({"kind": "native", "value": "native:profile"}),
            profile_start.clone(),
            blind.clone(),
        )));
        assert!(feature_definition_is_incomplete(&extrude(
            sketch_profile.clone(),
            serde_json::json!({"kind": "unresolved"}),
            blind,
        )));
        assert!(feature_definition_is_incomplete(&extrude(
            sketch_profile,
            profile_start,
            serde_json::json!({"kind": "to_face", "face": {"kind": "unresolved"}}),
        )));
    }

    #[test]
    fn hole_completeness_requires_support_placement_size_and_extent() {
        let complete: cadmpeg_ir::features::FeatureDefinition =
            serde_json::from_value(serde_json::json!({
                "definition": "hole",
                "face": {"kind": "faces", "value": ["face:support"]},
                "position": {"x": 1.0, "y": 2.0, "z": 3.0},
                "direction": {"x": 0.0, "y": 0.0, "z": -1.0},
                "kind": {"kind": "simple_drilled", "drill_point_angle": 2.0},
                "diameter": 5.0,
                "extent": {"kind": "blind", "length": 10.0}
            }))
            .expect("complete Hole definition");
        assert!(!feature_definition_is_incomplete(&complete));

        let mut missing_placement = complete.clone();
        let cadmpeg_ir::features::FeatureDefinition::Hole {
            position,
            direction,
            ..
        } = &mut missing_placement
        else {
            panic!("Hole definition");
        };
        *position = None;
        *direction = None;
        assert!(feature_definition_is_incomplete(&missing_placement));

        let mut native_support = complete.clone();
        let cadmpeg_ir::features::FeatureDefinition::Hole { face, .. } = &mut native_support else {
            panic!("Hole definition");
        };
        *face = Some(cadmpeg_ir::features::FaceSelection::Native(
            "native:support".into(),
        ));
        assert!(feature_definition_is_incomplete(&native_support));

        let mut missing_extent = complete;
        let cadmpeg_ir::features::FeatureDefinition::Hole { extent, .. } = &mut missing_extent
        else {
            panic!("Hole definition");
        };
        *extent = None;
        assert!(feature_definition_is_incomplete(&missing_extent));
    }

    #[test]
    fn face_selection_resolution_accepts_complete_generated_and_partial_members() {
        use cadmpeg_ir::features::{FaceSelection, FeatureId, GeneratedFaceRef};
        use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalFaceId};

        assert!(face_selection_is_resolved(&FaceSelection::Generated {
            faces: vec![GeneratedFaceRef {
                feature: FeatureId("feature:source".into()),
                local_id: "face:1".into(),
            }],
            native: "native:generated-face".into(),
        }));
        assert!(face_selection_is_resolved(
            &FaceSelection::HistoricalPartial {
                state: FeatureInputTopologyId("state:1".into()),
                faces: vec![HistoricalFaceId("face:1".into())],
                unresolved: Vec::new(),
                native: "native:historical-face".into(),
            }
        ));
        assert!(!face_selection_is_resolved(
            &FaceSelection::HistoricalPartial {
                state: FeatureInputTopologyId("state:1".into()),
                faces: vec![HistoricalFaceId("face:1".into())],
                unresolved: vec!["native:missing-face".into()],
                native: "native:historical-face".into(),
            }
        ));
    }

    #[test]
    fn filled_surface_completeness_requires_boundary_conditions_support_and_merge() {
        use cadmpeg_ir::features::{
            FaceSelection, FeatureDefinition, PathRef, SurfaceBoundary, SurfaceContinuity,
        };
        use cadmpeg_ir::ids::{EdgeId, FaceId};

        let surface = |support_faces, continuity, merge_result| FeatureDefinition::FilledSurface {
            boundary: SurfaceBoundary::Path(PathRef::Edges(vec![EdgeId("edge:1".into())])),
            support_faces,
            continuity: Some(continuity),
            boundary_continuities: Vec::new(),
            merge_result,
        };

        assert!(!feature_definition_is_incomplete(&surface(
            FaceSelection::Faces(Vec::new()),
            SurfaceContinuity::Contact,
            Some(false),
        )));
        assert!(feature_definition_is_incomplete(&surface(
            FaceSelection::Faces(Vec::new()),
            SurfaceContinuity::Contact,
            None,
        )));
        assert!(feature_definition_is_incomplete(&surface(
            FaceSelection::Faces(Vec::new()),
            SurfaceContinuity::Tangent,
            Some(true),
        )));
        assert!(!feature_definition_is_incomplete(&surface(
            FaceSelection::Faces(vec![FaceId("face:support".into())]),
            SurfaceContinuity::Curvature,
            Some(true),
        )));
    }

    #[test]
    fn sheet_metal_completeness_requires_neutral_profiles_and_edges() {
        let definition = |value| {
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
                .expect("sheet-metal definition")
        };
        let base_flange = |profile| {
            definition(serde_json::json!({
                "definition": "sheet_metal_base_flange",
                "profile": profile,
                "thickness": 2.5,
                "side": "forward"
            }))
        };
        let edge_flange = |edges| {
            definition(serde_json::json!({
                "definition": "sheet_metal_edge_flange",
                "edges": edges,
                "height": {"kind": "distance", "value": 25.0},
                "angle": std::f64::consts::FRAC_PI_2,
                "height_datum": "outer_faces",
                "bend_position": "inside",
                "width": {"kind": "full_edge"},
                "bend_radius": 2.5
            }))
        };
        let hem = |edges| {
            definition(serde_json::json!({
                "definition": "sheet_metal_hem",
                "edges": edges,
                "form": {"kind": "flat", "value": {"length": 10.0}},
                "direction": "forward",
                "bend_radius": 2.5
            }))
        };

        assert!(!feature_definition_is_incomplete(&base_flange(
            serde_json::json!({"kind": "sketch", "value": "sketch:1"}),
        )));
        assert!(feature_definition_is_incomplete(&base_flange(
            serde_json::json!({"kind": "native", "value": "native:profile"}),
        )));
        assert!(!feature_definition_is_incomplete(&edge_flange(
            serde_json::json!({"kind": "edges", "value": ["edge:1"]}),
        )));
        assert!(feature_definition_is_incomplete(&edge_flange(
            serde_json::json!({"kind": "native", "value": "native:edges"}),
        )));
        assert!(!feature_definition_is_incomplete(&hem(
            serde_json::json!({"kind": "edges", "value": ["edge:1"]}),
        )));
        assert!(feature_definition_is_incomplete(&hem(
            serde_json::json!({"kind": "native", "value": "native:edges"}),
        )));
    }

    #[test]
    fn selected_face_and_edge_features_require_neutral_operands() {
        let definition = |value| {
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
                .expect("selected feature definition")
        };
        let fillet = |edges| {
            definition(serde_json::json!({
                "definition": "fillet",
                "groups": [{
                    "edges": edges,
                    "radius": {"kind": "constant", "radius": 5.0},
                    "tangency_weight": 1.0
                }]
            }))
        };

        assert!(!feature_definition_is_incomplete(&fillet(
            serde_json::json!({"kind": "edges", "value": ["edge:1"]}),
        )));
        assert!(feature_definition_is_incomplete(&fillet(
            serde_json::json!({"kind": "native", "value": "native:edges"}),
        )));
        assert!(!feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "delete_face",
                "faces": {"kind": "faces", "value": ["face:1"]},
                "heal": true
            }),
        )));
        assert!(feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "delete_face",
                "faces": {"kind": "native", "value": "native:faces"},
                "heal": true
            }),
        )));
        assert!(!feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "offset_surface",
                "faces": {"kind": "faces", "value": ["face:1"]},
                "distance": 2.0
            }),
        )));
        assert!(feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "offset_surface",
                "faces": {"kind": "faces", "value": ["face:1"]}
            }),
        )));
    }

    #[test]
    fn form_and_primitive_completeness_requires_construction_payloads() {
        let definition = |value| {
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
                .expect("construction definition")
        };

        assert!(!feature_definition_is_incomplete(&definition(
            serde_json::json!({"definition": "form", "cages": ["subd:1"]}),
        )));
        assert!(feature_definition_is_incomplete(&definition(
            serde_json::json!({"definition": "form", "cages": []}),
        )));
        assert!(!feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "block",
                "dimensions": [30.0, 40.0, 20.0],
                "placement": [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0]
                ],
                "op": "join"
            }),
        )));
        assert!(feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "block",
                "dimensions": [30.0, 40.0, 20.0],
                "op": "join"
            }),
        )));
        assert!(!feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "primitive",
                "solid": {
                    "kind": "cylinder",
                    "radius": 15.0,
                    "height": 7.0,
                    "angle": std::f64::consts::TAU
                },
                "op": "join"
            }),
        )));
        assert!(feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "primitive",
                "solid": {
                    "kind": "cylinder",
                    "radius": 15.0,
                    "height": 7.0,
                    "angle": std::f64::consts::TAU
                },
                "op": "unresolved"
            }),
        )));
    }

    #[test]
    fn datum_point_completeness_requires_a_resolved_construction_rule() {
        let definition = |construction: Option<serde_json::Value>| {
            let mut value = serde_json::json!({
                "definition": "datum_point",
                "position": {"x": 1.0, "y": 2.0, "z": 3.0}
            });
            if let Some(construction) = construction {
                value
                    .as_object_mut()
                    .expect("DatumPoint object")
                    .insert("construction".into(), construction);
            }
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
                .expect("DatumPoint definition")
        };

        assert!(!feature_definition_is_incomplete(&definition(Some(
            serde_json::json!({
                "kind": "circle_center",
                "edge": {"kind": "edges", "value": ["edge:1"]}
            }),
        ))));
        assert!(feature_definition_is_incomplete(&definition(None)));
        assert!(feature_definition_is_incomplete(&definition(Some(
            serde_json::json!({
                "kind": "distance_on_edge",
                "edge": {"kind": "native", "value": "native:edge"},
                "fraction": 0.5
            }),
        ))));
    }

    #[test]
    fn datum_plane_completeness_distinguishes_frames_from_construction_rules() {
        let definition = |value| {
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
                .expect("datum-plane definition")
        };

        assert!(feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "datum_plane",
                "origin": {"x": 0.0, "y": 0.0, "z": 5.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
            }),
        )));
        let three_point = |point: serde_json::Value| {
            serde_json::json!({
                "definition": "datum_three_point_plane",
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
                "points": [point.clone(), point.clone(), point]
            })
        };
        assert!(!feature_definition_is_incomplete(&definition(three_point(
            serde_json::json!({
                "kind": "historical",
                "value": {
                    "state": "state:1",
                    "vertex": "vertex:1",
                    "native": "native:1"
                }
            }),
        ))));
        assert!(feature_definition_is_incomplete(&definition(three_point(
            serde_json::json!({"kind": "native", "value": "native:1"}),
        ))));
        assert!(!feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "datum_principal_plane",
                "plane": "top"
            }),
        )));
        assert!(!feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "datum_offset_plane",
                "reference": "feature:plane",
                "distance": 5.0
            }),
        )));
        assert!(feature_definition_is_incomplete(&definition(
            serde_json::json!({
                "definition": "datum_offset_plane",
                "distance": 5.0
            }),
        )));
    }

    #[test]
    fn coil_completeness_requires_neutral_placement_and_boolean_targets() {
        use cadmpeg_ir::features::{
            Angle, BodySelection, BooleanOp, CoilConstruction, CoilExtent, CoilPlacement,
            CoilResult, CoilSection, CoilSectionPlacement, FeatureDefinition, Length,
        };
        use cadmpeg_ir::ids::BodyId;
        use cadmpeg_ir::math::{Point3, Vector3};

        let construction = CoilConstruction {
            placement: CoilPlacement::Explicit {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                radial: Vector3::new(1.0, 0.0, 0.0),
            },
            diameter: Length(10.0),
            extent: CoilExtent::RevolutionsHeight {
                revolutions: 2.0,
                height: Length(5.0),
            },
            section: CoilSection::Circular {
                diameter: Length(1.0),
            },
            section_placement: CoilSectionPlacement::Center,
            clockwise: false,
            taper: Angle(0.0),
        };
        let definition = |construction, result| FeatureDefinition::Coil {
            construction,
            result,
        };

        assert!(!feature_definition_is_incomplete(&definition(
            construction.clone(),
            CoilResult::NewBody,
        )));

        let mut native_placement = construction.clone();
        native_placement.placement = CoilPlacement::Native {
            native_ref: "native:placement".into(),
        };
        assert!(feature_definition_is_incomplete(&definition(
            native_placement,
            CoilResult::NewBody,
        )));

        let native_target = definition(
            construction.clone(),
            CoilResult::Boolean {
                operation: BooleanOp::Join,
                targets: BodySelection::Native("native:target".into()),
            },
        );
        assert!(feature_definition_is_incomplete(&native_target));

        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.features.push(cadmpeg_ir::features::Feature {
            id: cadmpeg_ir::features::FeatureId("feature:coil".into()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: Some("CoilPrimitive".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: native_target,
            native_ref: None,
        });
        let gaps = design_projection_gaps(&ir, &F3dNative::default());
        assert_eq!(gaps.incomplete_features, 1);
        assert_eq!(gaps.body_selections, 1);

        assert!(!feature_definition_is_incomplete(&definition(
            construction,
            CoilResult::Boolean {
                operation: BooleanOp::Cut,
                targets: BodySelection::Bodies(vec![BodyId("body:1".into())]),
            },
        )));
    }

    #[test]
    fn draft_completeness_requires_material_side() {
        let complete: cadmpeg_ir::features::FeatureDefinition =
            serde_json::from_value(serde_json::json!({
                "definition": "draft",
                "faces": {"kind": "faces", "value": ["face:drafted"]},
                "neutral_plane": {"kind": "faces", "value": ["face:neutral"]},
                "pull_direction": null,
                "angle": 0.1,
                "outward": true
            }))
            .expect("complete neutral-plane Draft");
        assert!(!feature_definition_is_incomplete(&complete));

        let mut incomplete = complete;
        let cadmpeg_ir::features::FeatureDefinition::Draft { outward, .. } = &mut incomplete else {
            panic!("Draft definition");
        };
        *outward = None;
        assert!(feature_definition_is_incomplete(&incomplete));
    }

    #[test]
    fn loft_completeness_and_gap_counts_require_resolved_sections_and_paths() {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let resolved: FeatureDefinition = serde_json::from_value(serde_json::json!({
            "definition": "loft",
            "sections": [
                {
                    "kind": "spatial_sketch_profiles",
                    "value": {"sketch": "spatial-sketch", "profiles": [2, 3]}
                },
                {
                    "kind": "spatial_sketch_profiles",
                    "value": {"sketch": "spatial-sketch", "profiles": [1, 4]}
                }
            ],
            "guides": [{
                "kind": "spatial_sketch_curves",
                "value": {"sketch": "spatial-sketch", "curves": ["curve"]}
            }],
            "op": "join"
        }))
        .expect("resolved Loft definition");
        assert!(!feature_definition_is_incomplete(&resolved));

        let unresolved: FeatureDefinition = serde_json::from_value(serde_json::json!({
            "definition": "loft",
            "sections": [
                {"kind": "native", "value": "native:profile"},
                {
                    "kind": "spatial_sketch_profiles",
                    "value": {"sketch": "spatial-sketch", "profiles": [1, 4]}
                }
            ],
            "guides": [{"kind": "native", "value": "native:guide"}],
            "op": "join"
        }))
        .expect("unresolved Loft definition");
        assert!(feature_definition_is_incomplete(&unresolved));

        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.features.push(Feature {
            id: FeatureId("feature:loft".into()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: Some("Loft".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: unresolved,
            native_ref: None,
        });

        let gaps = design_projection_gaps(&ir, &F3dNative::default());
        assert_eq!(gaps.incomplete_features, 1);
        assert_eq!(gaps.profile_selections, 1);
        assert_eq!(gaps.path_selections, 1);
    }

    #[test]
    fn incomplete_feature_families_are_counted_by_source_operation() {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        let feature = |id: &str, source_tag: Option<&str>, kind: &str| Feature {
            id: FeatureId(id.into()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: source_tag.map(str::to_owned),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: kind.into(),
                parameters: std::collections::BTreeMap::new(),
                properties: std::collections::BTreeMap::new(),
            },
            native_ref: None,
        };
        ir.model
            .features
            .push(feature("feature:1", Some("EdgeFlange"), "native-a"));
        ir.model
            .features
            .push(feature("feature:2", Some("EdgeFlange"), "native-b"));
        ir.model.features.push(feature("feature:3", None, "Hem"));
        ir.model
            .features
            .push(feature("feature:4", Some("Canvas"), "Canvas"));

        assert_eq!(
            incomplete_feature_families(&ir),
            std::collections::BTreeMap::from([("EdgeFlange", 2), ("Hem", 1)])
        );
    }

    #[test]
    fn design_projection_gaps_count_unresolved_body_map_pairs() {
        let ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        let mut native = F3dNative::default();
        native.design_body_bindings.push(DesignBodyBinding {
            id: "f3d:design:body-binding#0".into(),
            stream: "Design/BulkStream.dat".into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 0,
            asm_body_key_offset: 0,
            entity_suffix: 1,
            entity_suffix_offset: 8,
            blob_name: "BREP.snapshot.smb".into(),
            blob_name_offset: 16,
            body: None,
        });

        assert_eq!(
            design_projection_gaps(&ir, &native).unresolved_body_bindings,
            1
        );
    }

    #[test]
    fn design_projection_gaps_count_cosmetic_thread_faces() {
        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        for (ordinal, face) in [
            serde_json::json!({"kind": "native", "value": "native:thread-face"}),
            serde_json::json!({"kind": "unresolved"}),
            serde_json::json!({
                "kind": "historical",
                "value": {
                    "state": "feature-input",
                    "faces": ["historical:face"],
                    "native": "native:thread-group"
                }
            }),
        ]
        .into_iter()
        .enumerate()
        {
            ir.model.features.push(
                serde_json::from_value(serde_json::json!({
                    "id": format!("thread-{ordinal}"),
                    "ordinal": ordinal,
                    "definition": {
                        "definition": "cosmetic_thread",
                        "face": face,
                        "diameter": 2.5
                    }
                }))
                .expect("CosmeticThread feature"),
            );
        }

        assert_eq!(
            design_projection_gaps(&ir, &F3dNative::default()).face_selections,
            2
        );
    }

    #[test]
    fn design_projection_gaps_count_each_retained_selection_family() {
        use cadmpeg_ir::math::{Point2, Point3, Vector3};
        use cadmpeg_ir::sketches::{
            Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
            SketchEntityId, SketchGeometry, SketchId,
        };

        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId("constraint".into()),
            sketch: SketchId("sketch".into()),
            definition: SketchConstraintDefinition::Native {
                native_kind: "dimension".into(),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: Vec::new(),
                parameter: None,
                operands: Vec::new(),
            },
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: Some("native:sketch-relation".into()),
        });
        let mut native_dimension = ir.model.sketch_constraints[0].clone();
        native_dimension.id = SketchConstraintId("dimension".into());
        native_dimension.native_ref = Some("native:dimension-companion".into());
        ir.model.sketch_constraints.push(native_dimension);
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "extrude",
                "ordinal": 0,
                "definition": {
                    "definition": "extrude",
                    "profile": {
                        "kind": "sketch_selection",
                        "value": {"sketch": "sketch", "selections": ["native:profile"]}
                    },
                    "start": {"kind": "profile_plane"},
                    "extent": {
                        "kind": "one_sided",
                        "side": {
                            "termination": {
                                "kind": "to_face",
                                "face": {"kind": "native", "value": "native:face"}
                            }
                        }
                    },
                    "op": "cut"
                }
            }))
            .expect("Extrude feature"),
        );
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "sweep",
                "ordinal": 1,
                "definition": {
                    "definition": "sweep",
                    "section": {
                        "kind": "profile",
                        "value": {"kind": "native", "value": "native:sweep-profile"}
                    },
                    "path": {"kind": "native", "value": "native:sweep-path"},
                    "mode": {"mode": "solid", "op": "cut"}
                }
            }))
            .expect("Sweep feature"),
        );
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "fillet",
                "ordinal": 2,
                "definition": {
                    "definition": "fillet",
                    "groups": [
                        {
                            "edges": {"kind": "native", "value": "native:edges"},
                            "radius": {"kind": "constant", "radius": 1.0}
                        },
                        {
                            "edges": {"kind": "unresolved"},
                            "radius": {"kind": "constant", "radius": 2.0}
                        },
                        {
                            "edges": {
                                "kind": "historical_partial",
                                "value": {
                                    "state": "history-input",
                                    "edges": [],
                                    "unresolved": [
                                        "native:edge-operand#1",
                                        "f3d:test:lost-edge-reference#2"
                                    ],
                                    "native": "native:partial-edges"
                                }
                            },
                            "radius": {"kind": "constant", "radius": 3.0}
                        }
                    ]
                }
            }))
            .expect("Fillet feature"),
        );
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "suppressed-fillet",
                "ordinal": 2,
                "suppressed": true,
                "definition": {
                    "definition": "fillet",
                    "groups": [{
                        "edges": {"kind": "native", "value": "native:suppressed-edges"},
                        "radius": {"kind": "constant", "radius": 3.0}
                    }]
                }
            }))
            .expect("suppressed Fillet feature"),
        );
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "native-feature",
                "ordinal": 3,
                "definition": {
                    "definition": "native",
                    "kind": "unsupported",
                    "parameters": {},
                    "properties": {}
                }
            }))
            .expect("native feature"),
        );
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": "unresolved-pattern",
                "ordinal": 4,
                "definition": {
                    "definition": "pattern",
                    "seeds": [],
                    "pattern": {
                        "kind": "unresolved",
                        "form": "circular"
                    }
                }
            }))
            .expect("unresolved pattern feature"),
        );

        let mut native = F3dNative::default();
        native.design_sketch_placements.push(DesignSketchPlacement {
            member_run_head: false,
            id: "native:sketch-placement".into(),
            scope_record_index: Some(10),
            entity_id: "Sketch_1".into(),
            entity_suffix: 1,
            visibility: None,
            byte_offset: 0,
            class_tag: "000".into(),
            record_index: 10,
            frame_length: 1,
            transform: [[0.0; 4]; 4],
            transform_offset: None,
            paired_class_tag: "001".into(),
            paired_byte_offset: 1,
        });
        native.sketch_points.push(SketchPoint {
            id: "native:sketch-point".into(),
            record_index: 11,
            owner_reference: Some(1),
            class_tag: "000".into(),
            byte_offset: 0,
            coordinate_offset: 0,
            entity_genesis: None,
            record_form: crate::records::SketchPointRecordForm::default(),
            persistent_id: Some(1),
            paired_reference: 0,
            flags: [0; 8],
            coordinates: Point2::new(0.0, 0.0),
            depth: 0.0,
            closure: None,
            companion: None,
        });
        native.sketch_curve_identities.push(SketchCurveIdentity {
            id: "native:sketch-curve".into(),
            record_index: 12,
            owner_reference: Some(1),
            class_tag: "000".into(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: 1,
            secondary_id: 2,
            geometry: None,
        });
        native.lost_edge_references.push(LostEdgeReference {
            id: "f3d:test:lost-edge-reference#2".into(),
            record_byte_offset: 0,
            class_tag_offset: 0,
            class_tag: "000".into(),
            record_index: 0,
            record_index_offset: 0,
            byte_offset: 0,
            next_byte_offset: 1,
            next_class_tag: "001".into(),
            next_record_index: 1,
        });
        native.sketch_relations.push(SketchRelation {
            id: "native:sketch-relation".into(),
            record_index: 1,
            class_tag: "000".into(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: "0_1".into(),
            auxiliary_references: Vec::new(),
            auxiliary_reference_offsets: Vec::new(),
            rectangular_counted_reference_count: None,
            members: Vec::new(),
            resolved_members: Vec::new(),
            member_offsets: Vec::new(),
            owner_reference_offset: 0,
            state: 0,
            constraint_kinds: Vec::new(),
            unknown_constraint_bits: 0,
            member_relation_ordinals: Vec::new(),
            entity_genesis: None,
            pattern: None,
            return_members: Vec::new(),
            resolved_return_members: Vec::new(),
            return_member_offsets: Vec::new(),
            raw_bytes: Vec::new(),
        });
        native.design_parameters.push(DesignParameter {
            id: "f3d:test:design-parameter#2".into(),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index: 2,
            family_discriminator: Some(0),
            family_discriminator_offset: Some(0),
            source_ordinal: 2,
            owner_record_index: Some(3),
            expression: "1 mm".into(),
            expression_offset: 0,
            source_kind: "Linear Dimension-2".into(),
            source_kind_offset: 0,
            kind: DesignParameterKind::Dimension,
            unit: Some("native-unit".into()),
            unit_offset: Some(0),
            name: "d2".into(),
            name_offset: 0,
            evaluated_value: 0.1,
            evaluated_value_offset: 0,
        });
        native.design_parameter_scopes.push(DesignParameterScope {
            id: "native:unprojected-scope".into(),
            byte_offset: 0,
            class_tag: "000".into(),
            record_index: 3,
            frame_length: 1,
            kind: "Unsupported".into(),
            kind_offset: 0,
            extrude_prologue: None,
            coil_operation: None,
            coil_operation_offset: None,
            coil_extent: None,
            coil_extent_offset: None,
            coil_section: None,
            coil_section_offset: None,
            coil_section_placement: None,
            coil_section_placement_offset: None,
            coil_clockwise: None,
            coil_clockwise_offset: None,
            coil_placement: None,
            coil_transform: None,
            feature_ordinal: 1,
            feature_ordinal_offset: 0,
            history_state_id: None,
            history_state_id_offset: 0,
            previous_history_state_id: None,
            previous_history_state_id_offset: 0,
            reference_count_offset: 0,
            reference_members: Vec::new(),
            reference_member_offsets: Vec::new(),
            solid_primitive: None,
            direct_face_operation: None,
            move_operation: None,
            scale_operation: None,
            surface_stitch_operation: None,
            surface_extend_operation: None,
            surface_offset_operation: None,
            ruled_surface_operation: None,
            surface_patch_boundaries: Vec::new(),
            base_flange_operation: None,
            edge_flange_operation: None,
            hem_operation: None,
            fixed_extrude_parameters: None,
            fixed_fillet_parameters: None,
            fixed_chamfer_parameters: None,
            path_feature_construction: None,
            combine_operation: None,
            thread_construction: None,
            draft_operation: None,
            copy_paste_bodies_operation: None,
            base_feature_construction: None,
            work_plane_transform: None,
            work_plane_transform_offset: None,
            work_plane_reference: None,
            work_plane_reference_offset: None,
            work_plane_construction: None,
            work_axis_construction: None,
            joint_origin_transform: None,
            joint_origin_transform_offset: None,
            joint_origin_reference: None,
            joint_origin_reference_offset: None,
            work_point_construction: None,
            unclosed_construction_operand_groups: Vec::new(),
            hole_construction: None,
            extrude_profile: None,
            sweep_profile: None,
            circular_pattern_construction: None,
            rectangular_pattern_construction: None,
            assembly_alignment: None,
            component_insert_construction: None,
            copy_paste_component_operation: None,
            mirror_construction: None,
            base_flange_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "001".into(),
            paired_byte_offset: 1,
        });
        assert_eq!(
            design_projection_gaps(&ir, &native),
            DesignProjectionGaps {
                unresolved_body_bindings: 0,
                incomplete_features: 6,
                native_reference_images: 0,
                native_decals: 0,
                unprojected_feature_scopes: 1,
                unprojected_parameters: 1,
                unresolved_parameter_owners: 1,
                untyped_parameter_units: 1,
                unresolved_expression_dependencies: 0,
                unprojected_history_dependencies: 0,
                ambiguous_history_dependencies: 0,
                native_sketch_relations: 1,
                native_dimensions: 1,
                unprojected_sketch_placements: 1,
                unprojected_sketch_points: 1,
                unprojected_sketch_curves: 1,
                unprojected_sketch_surfaces: 0,
                unprojected_sketch_texts: 0,
                unprojected_sketch_relations: 0,
                unprojected_dimensions: 0,
                profile_selections: 2,
                path_selections: 1,
                face_selections: 1,
                body_selections: 0,
                partially_resolved_face_members: 0,
                native_edge_selections: 2,
                partially_resolved_edge_members: 1,
                unresolved_edge_selections: 1,
                unrepaired_lost_edge_references: 1,
            }
        );

        native.design_construction_operand_groups.push(
            serde_json::from_value(serde_json::json!({
                "id": "native:partial-edges",
                "scope_record_index": 1,
                "scope_reference_ordinal": 0,
                "record_index": 2,
                "byte_offset": 0,
                "class_tag": "300",
                "members": [3],
                "lost_edge_references": ["f3d:test:lost-edge-reference#2"],
                "member_offsets": [0],
                "frame": {
                    "member_count_offset": 0,
                    "opaque_index": 1,
                    "opaque_index_offset": 0,
                    "opaque_scalar": 1.0,
                    "opaque_scalar_offset": 0,
                    "variant": false
                },
                "role": 0x10_0000_0000u64,
                "role_offset": 0,
                "paired_class_tag": "258",
                "paired_byte_offset": 0
            }))
            .expect("lost-reference construction group"),
        );
        let cadmpeg_ir::features::FeatureDefinition::Fillet { groups } =
            &mut ir.model.features[2].definition
        else {
            unreachable!();
        };
        groups[2].edges = cadmpeg_ir::features::EdgeSelection::Historical {
            state: cadmpeg_ir::ids::FeatureInputTopologyId("history-input".into()),
            edges: vec![cadmpeg_ir::ids::HistoricalEdgeId("history-edge".into())],
            native: "native:partial-edges".into(),
        };
        assert_eq!(
            design_projection_gaps(&ir, &native).unrepaired_lost_edge_references,
            0
        );

        native.sketch_points[0].owner_reference = None;
        native.sketch_curve_identities[0].owner_reference = None;
        let ownerless = design_projection_gaps(&ir, &native);
        assert_eq!(ownerless.unprojected_sketch_points, 0);
        assert_eq!(ownerless.unprojected_sketch_curves, 0);
        native.sketch_points[0].owner_reference = Some(1);
        native.sketch_curve_identities[0].owner_reference = Some(1);

        ir.model.sketches.push(Sketch {
            id: SketchId("sketch".into()),
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            profiles: Vec::new(),
            native_ref: Some("native:sketch-placement".into()),
        });
        for (id, native_ref) in [
            ("point", "native:sketch-point"),
            ("curve", "native:sketch-curve"),
        ] {
            ir.model.sketch_entities.push(SketchEntity {
                id: SketchEntityId(id.into()),
                sketch: SketchId("sketch".into()),
                construction: false,
                native_ref: Some(native_ref.into()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Point {
                    position: Point2::new(0.0, 0.0),
                },
            });
        }
        let gaps = design_projection_gaps(&ir, &native);
        assert_eq!(gaps.unprojected_sketch_placements, 0);
        assert_eq!(gaps.unprojected_sketch_points, 0);
        assert_eq!(gaps.unprojected_sketch_curves, 0);

        ir.model.parameters.push(
            serde_json::from_value(serde_json::json!({
                "id": "parameter-2",
                "ordinal": 2,
                "name": "d2",
                "expression": "1 mm",
                "native_ref": "f3d:test:design-parameter#2"
            }))
            .expect("Design parameter"),
        );
        assert_eq!(
            design_projection_gaps(&ir, &native).unprojected_parameters,
            0
        );
    }

    #[test]
    fn design_projection_gaps_require_unique_scope_state_dependencies() {
        let scope = |record_index, current, previous| DesignParameterScope {
            id: format!("f3d:native:scope#{record_index}"),
            byte_offset: u64::from(record_index),
            class_tag: "000".into(),
            record_index,
            frame_length: 1,
            kind: "Unsupported".into(),
            kind_offset: 0,
            extrude_prologue: None,
            coil_operation: None,
            coil_operation_offset: None,
            coil_extent: None,
            coil_extent_offset: None,
            coil_section: None,
            coil_section_offset: None,
            coil_section_placement: None,
            coil_section_placement_offset: None,
            coil_clockwise: None,
            coil_clockwise_offset: None,
            coil_placement: None,
            coil_transform: None,
            feature_ordinal: record_index,
            feature_ordinal_offset: 0,
            history_state_id: current,
            history_state_id_offset: 0,
            previous_history_state_id: previous,
            previous_history_state_id_offset: 0,
            reference_count_offset: 0,
            reference_members: Vec::new(),
            reference_member_offsets: Vec::new(),
            solid_primitive: None,
            direct_face_operation: None,
            move_operation: None,
            scale_operation: None,
            surface_stitch_operation: None,
            surface_extend_operation: None,
            surface_offset_operation: None,
            ruled_surface_operation: None,
            surface_patch_boundaries: Vec::new(),
            base_flange_operation: None,
            edge_flange_operation: None,
            hem_operation: None,
            fixed_extrude_parameters: None,
            fixed_fillet_parameters: None,
            fixed_chamfer_parameters: None,
            path_feature_construction: None,
            combine_operation: None,
            thread_construction: None,
            draft_operation: None,
            copy_paste_bodies_operation: None,
            base_feature_construction: None,
            work_plane_transform: None,
            work_plane_transform_offset: None,
            work_plane_reference: None,
            work_plane_reference_offset: None,
            work_plane_construction: None,
            work_axis_construction: None,
            joint_origin_transform: None,
            joint_origin_transform_offset: None,
            joint_origin_reference: None,
            joint_origin_reference_offset: None,
            work_point_construction: None,
            unclosed_construction_operand_groups: Vec::new(),
            hole_construction: None,
            extrude_profile: None,
            sweep_profile: None,
            circular_pattern_construction: None,
            rectangular_pattern_construction: None,
            assembly_alignment: None,
            component_insert_construction: None,
            copy_paste_component_operation: None,
            mirror_construction: None,
            base_flange_profile: None,
            entity_id: None,
            entity_suffix: None,
            entity_reference_offset: None,
            paired_class_tag: "001".into(),
            paired_byte_offset: u64::from(record_index) + 1,
        };
        let mut native = F3dNative::default();
        native.design_parameter_scopes = vec![
            scope(1, Some(10), None),
            scope(2, Some(11), Some(10)),
            scope(3, Some(20), None),
            scope(4, Some(20), None),
            scope(5, Some(21), Some(20)),
        ];
        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.features = native
            .design_parameter_scopes
            .iter()
            .map(|scope| {
                serde_json::from_value(serde_json::json!({
                    "id": format!("feature-{}", scope.record_index),
                    "ordinal": scope.record_index,
                    "definition": {
                        "definition": "native",
                        "kind": "Unsupported",
                        "parameters": {},
                        "properties": {}
                    },
                    "native_ref": scope.id
                }))
                .expect("native feature")
            })
            .collect();

        let gaps = design_projection_gaps(&ir, &native);
        assert_eq!(gaps.unprojected_feature_scopes, 0);
        assert_eq!(gaps.unprojected_history_dependencies, 1);
        assert_eq!(gaps.ambiguous_history_dependencies, 1);

        let predecessor = ir.model.features[0].id.clone();
        ir.model.features[1].dependencies.push(predecessor);
        let gaps = design_projection_gaps(&ir, &native);
        assert_eq!(gaps.unprojected_history_dependencies, 0);
        assert_eq!(gaps.ambiguous_history_dependencies, 1);
    }

    #[test]
    fn design_projection_gaps_accept_a_dependency_collapsed_through_an_internal_scope() {
        let stream = "f3d:Design/BulkStream.dat";
        let mut predecessor = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#100"),
            "Extrude",
            100,
        );
        predecessor.history_state_id = Some(7);
        let mut internal = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#150"),
            "Base Feature",
            150,
        );
        internal.history_state_id = Some(8);
        internal.previous_history_state_id = Some(7);
        let mut successor = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#200"),
            "Fillet",
            200,
        );
        successor.history_state_id = Some(9);
        successor.previous_history_state_id = Some(8);
        let scopes = vec![successor, internal, predecessor];
        let timeline = DesignFeatureTimeline {
            id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
            byte_offset: 0,
            class_tag: "256".into(),
            record_index: 1,
            source_ordinal: 0,
            frame_length: 0,
            context_record_index: 2,
            context_record_index_offset: 0,
            item_count_offset: 0,
            item_record_indices: vec![100, 200],
            item_record_index_offsets: vec![0, 0],
        };
        let (features, _) =
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                &crate::design::feature_project::ProjectInputs {
                    native: &[],
                    owners: &[],
                    scopes: &scopes,
                    timelines: std::slice::from_ref(&timeline),
                    construction_groups: &[],
                    fillet_radius_groups: &[],
                    edge_operands: &[],
                    edge_identity_operands: &[],
                    entity_selection_operands: &[],
                    curve_identities: &[],
                    face_operands: &[],
                    body_recipe_operands: &[],
                    placements: &[],
                    body_bindings: &[],
                    histories: &[],
                },
            )
            .expect("timeline projection through one internal scope");
        let mut native = F3dNative::default();
        native.design_parameter_scopes = scopes;
        native.design_feature_timelines = vec![timeline];
        let mut ir = cadmpeg_ir::document::CadIr::empty(Default::default());
        ir.model.features = features;

        let gaps = design_projection_gaps(&ir, &native);
        assert_eq!(gaps.unprojected_feature_scopes, 0);
        assert_eq!(gaps.unprojected_history_dependencies, 0);
        assert_eq!(gaps.ambiguous_history_dependencies, 0);
    }

    #[test]
    fn payload_bearing_dimension_companion_uses_the_governing_dimension_frame() {
        let stream = "f3d:test/BulkStream.dat";
        let mut ir = cadmpeg_ir::examples::unit_cube();
        let mut native = F3dNative::default();
        native.design_parameters.push(DesignParameter {
            id: format!("{stream}:design-parameter#10"),
            byte_offset: 0,
            class_tag: "305".into(),
            record_index: 10,
            family_discriminator: Some(0),
            family_discriminator_offset: Some(22),
            source_ordinal: 0,
            owner_record_index: Some(20),
            expression: "5 mm".into(),
            expression_offset: 40,
            source_kind: "Linear Dimension-2".into(),
            source_kind_offset: 60,
            kind: DesignParameterKind::Dimension,
            unit: Some("mm".into()),
            unit_offset: Some(90),
            name: "d1".into(),
            name_offset: 100,
            evaluated_value: 0.5,
            evaluated_value_offset: 110,
        });
        native.design_parameter_owners.push(DesignParameterOwner {
            id: format!("{stream}:design-parameter-owner#20"),
            byte_offset: 120,
            frame_length: 104,
            class_tag: "292".into(),
            record_index: 20,
            scope_record_index: 1,
            local_ordinal: 0,
            evaluated_value: 0.5,
            evaluated_value_offset: 160,
            parameter_record_index: 10,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 30,
        });
        native
            .design_parameter_companions
            .push(DesignParameterCompanion {
                id: format!("{stream}:design-parameter-companion#30"),
                byte_offset: 220,
                class_tag: "408".into(),
                record_index: 30,
                owner_record_index: 20,
                timestamp_micros: 1,
                timestamp_micros_offset: 262,
                payload_byte_offset: 278,
                payload_byte_length: 100,
                owned_recipe_ids: Vec::new(),
            });
        assert_eq!(unresolved_dimension_companion_count(&native, &ir), 1);
        ir.model.sketch_constraints.push(
            serde_json::from_value(serde_json::json!({
                "id": "f3d:model:sketch-constraint#dimension",
                "sketch": "f3d:model:sketch#1",
                "definition": {
                    "kind": "distance",
                    "entities": [],
                    "parameter": "f3d:model:parameter#1"
                },
                "native_ref": format!("{stream}:design-parameter-companion#30")
            }))
            .expect("neutral dimension constraint"),
        );
        assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
        ir.model.sketch_constraints.clear();

        let mut recipe_backed = native.clone();
        recipe_backed
            .design_dimension_recipe_records
            .push(DesignDimensionRecipeRecord {
                id: format!("{stream}:design-dimension-recipe-record#31"),
                companion_record_index: 30,
                recipe_ordinal: 0,
                recipe_id: format!("{stream}:construction-recipe#31"),
                recipe_kind: crate::records::ConstructionRecipeKind::Edge,
                byte_offset: 278,
                class_tag: "423".into(),
                record_index: 31,
                frame_length: 100,
                prefix_offset: 300,
                prefix_bytes: Vec::new(),
                references: Vec::new(),
                program_offset: 320,
                program: vec![-1],
                matching_edge_operand_ids: Vec::new(),
            });
        assert_eq!(unresolved_dimension_companion_count(&recipe_backed, &ir), 0);

        native
            .design_dimension_locus_pairs
            .push(DesignDimensionLocusPair {
                id: format!("{stream}:design-dimension-locus-pair#278"),
                companion_record_index: 99,
                governing_companion_record_index: 30,
                byte_offset: 278,
                class_tag: "423".into(),
                record_index: 31,
                frame_length: 100,
                opaque_index: 0,
                opaque_index_offset: 300,
                first_geometry_record_index: 40,
                first_geometry_reference_offset: 305,
                first_role: 1,
                first_role_offset: 315,
                second_geometry_record_index: 41,
                second_geometry_reference_offset: 320,
                second_role: 2,
                second_role_offset: 330,
                paired_class_tag: "259".into(),
                paired_byte_offset: 378,
            });
        assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
        assert!(container_only_dimension_parameters(&native).is_empty());
        native.design_dimension_locus_pairs[0].companion_record_index = 30;
        native.design_dimension_locus_pairs[0].governing_companion_record_index = 99;
        assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
        assert_eq!(container_only_dimension_parameters(&native).len(), 1);

        native.design_dimension_locus_pairs.clear();
        native
            .design_dimension_null_locus_pairs
            .push(DesignDimensionNullLocusPair {
                id: format!("{stream}:design-dimension-null-locus-pair#278"),
                companion_record_index: 99,
                governing_companion_record_index: 30,
                byte_offset: 278,
                class_tag: "423".into(),
                record_index: 31,
                frame_length: 100,
                null_reference_offset: 300,
                null_role: 14,
                null_role_offset: 305,
                geometry_record_index: 40,
                geometry_reference_offset: 310,
                geometry_role: 3,
                geometry_role_offset: 320,
                paired_class_tag: "259".into(),
                paired_byte_offset: 378,
            });
        assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
        native.design_dimension_null_locus_pairs[0].companion_record_index = 30;
        native.design_dimension_null_locus_pairs[0].governing_companion_record_index = 99;
        assert_eq!(unresolved_dimension_companion_count(&native, &ir), 0);
    }

    #[test]
    fn appearance_base_colors_fill_only_uncolored_unambiguous_targets() {
        use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
        use cadmpeg_ir::ids::AppearanceId;
        use cadmpeg_ir::topology::Color;

        let mut ir = cadmpeg_ir::examples::unit_cube();
        let body = ir.model.bodies[0].id.clone();
        let first_face = ir.model.faces[0].id.clone();
        let second_face = ir.model.faces[1].id.clone();
        let direct = Color {
            r: 0.9,
            g: 0.8,
            b: 0.7,
            a: 1.0,
        };
        let material = Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        };
        ir.model.bodies[0].color = Some(direct);
        ir.model.appearances.push(Appearance {
            id: AppearanceId("f3d:appearance#material".into()),
            name: None,
            asset_guid: None,
            library_id: None,
            textures: Vec::new(),
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(material),
            properties: Default::default(),
        });
        let binding = |id: &str, target| AppearanceBinding {
            id: id.into(),
            target,
            appearance: AppearanceId("f3d:appearance#material".into()),
            source_entity_id: None,
            object_type: None,
            channels: Default::default(),
        };
        ir.model.appearance_bindings = vec![
            binding("body", AppearanceTarget::Body(body)),
            binding("face", AppearanceTarget::Face(first_face)),
            binding("ambiguous-a", AppearanceTarget::Face(second_face.clone())),
            binding("ambiguous-b", AppearanceTarget::Face(second_face)),
        ];

        apply_appearance_base_colors(&mut ir);
        assert_eq!(ir.model.bodies[0].color, Some(direct));
        assert_eq!(ir.model.faces[0].color, Some(material));
        assert_eq!(ir.model.faces[1].color, None);
    }
}
