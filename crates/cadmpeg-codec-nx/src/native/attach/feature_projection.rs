use super::{extrude_feature_definition, feature_body_outputs};
use crate::native::history::BodyWriterHistory;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{BodyId, LoopId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{BodyKind, Face, FaceLoops, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferSpec, RadiusSpec},
        holes::{HoleForm, HoleKind, HolePlacement},
        patterns::PatternKind,
        BodySelection, BodyTrimSide, BooleanOp, CurveProjectionDirection,
        CurveProjectionDirectionState, EdgeSelection, FaceSelection, FeatureDefinition, FeatureId,
        FeatureOperation, FeatureSourceContent, FeatureTreeNodeRole, LinearTermination, PathRef,
        RibConstruction, RibDraft, SurfaceExtension, ThickenSide, TreeChildren, TrimRegion,
        UnresolvedFamily,
    },
    scalar::{FiniteReal, Length, NonZeroLength, PositiveLength},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn body_faces<'a>(ir: &'a CadIr, body_id: &BodyId) -> Option<Vec<&'a Face>> {
    let body = ir.model.bodies.iter().find(|body| body.id == *body_id)?;
    let mut faces = Vec::new();
    for region_id in &body.regions {
        let region = ir
            .model
            .regions
            .iter()
            .find(|region| region.id == *region_id && region.body == body.id)?;
        for shell_id in &region.shells {
            let shell = ir
                .model
                .shells
                .iter()
                .find(|shell| shell.id == *shell_id && shell.region == region.id)?;
            for face_id in shell.faces() {
                let face = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id && face.shell == shell.id)?;
                faces.push(face);
            }
        }
    }
    Some(faces)
}

pub(super) fn connected_solid_body_faces<'a>(
    ir: &'a CadIr,
    body_id: &BodyId,
) -> Option<Vec<&'a Face>> {
    let body = ir.model.bodies.iter().find(|body| body.id == *body_id)?;
    if body.kind != cadmpeg_ir::topology::BodyKind::Solid {
        return None;
    }
    let [region_id] = body.regions.as_slice() else {
        return None;
    };

    let region = ir
        .model
        .regions
        .iter()
        .find(|region| region.id == *region_id && region.body == body.id)?;
    let [shell_id] = region.shells.as_slice() else {
        return None;
    };

    let shell = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == *shell_id && shell.region == region.id)?;
    let mut faces = Vec::new();
    for face_id in shell.faces() {
        let face = ir
            .model
            .faces
            .iter()
            .find(|face| face.id == *face_id && face.shell == shell.id)?;
        faces.push(face);
    }
    Some(faces)
}

pub(super) fn connected_solid_body_exists(ir: &CadIr, body: &cadmpeg_ir::topology::Body) -> bool {
    let Some(body) = ir
        .model
        .bodies
        .iter()
        .find(|candidate| candidate.id == body.id)
    else {
        return false;
    };
    if body.kind != BodyKind::Solid {
        return false;
    }
    let [region_id] = body.regions.as_slice() else {
        return false;
    };

    let Some(region) = ir
        .model
        .regions
        .iter()
        .find(|region| region.id == *region_id && region.body == body.id)
    else {
        return false;
    };
    let [shell_id] = region.shells.as_slice() else {
        return false;
    };

    let Some(shell) = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == *shell_id && shell.region == region.id)
    else {
        return false;
    };
    shell.faces().iter().all(|face_id| {
        ir.model
            .faces
            .iter()
            .any(|face| face.id == *face_id && face.shell == shell.id)
    })
}

pub(super) fn selection_indices_native(indices: impl Iterator<Item = u32>) -> String {
    let mut text = String::from("nx:om-object-indices#");
    for (ordinal, index) in indices.enumerate() {
        if ordinal != 0 {
            text.push(',');
        }
        text.push_str(&index.to_string());
    }
    text
}

pub(super) fn parameter_consumer_identity(operation_label: &str) -> String {
    match operation_label.split_once("operation-label") {
        Some((prefix, suffix)) => format!("{prefix}feature{suffix}"),
        None => operation_label.to_owned(),
    }
}

pub(super) fn insert_parameter_property(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: String,
) -> Result<(), CodecError> {
    let key = cadmpeg_core::text::NonBlankString::new(std::fmt::format(key)).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed(format_args!("NX parameter property key is blank"))
    })?;
    properties.insert(key, value);
    Ok(())
}

pub(super) fn body_surface_ids(ir: &CadIr, body_id: &BodyId) -> Option<BTreeSet<SurfaceId>> {
    let faces = body_faces(ir, body_id)?;
    let mut ids = BTreeSet::new();
    for face in &faces {
        if ids.contains(&face.surface) {
            continue;
        }
        let surface = face.surface.clone();
        ids.insert(surface);
    }
    Some(ids)
}

/// Neutral operand family named by an NX rolling-ball blend operation.
#[derive(Clone, Copy)]
pub(super) enum NxBlendFamily {
    /// Edge-selected `BLEND` operation.
    Edge,
    /// Face-selected `FACE_BLEND` operation.
    Face,
}

/// Project complete owned rolling-ball carriers into their named blend family.
pub(super) fn blend_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
    family: NxBlendFamily,
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let [body] = outputs else {
        return None;
    };
    let body_surfaces = body_surface_ids(ir, body)?;
    let mut surfaces = Vec::new();
    let mut first_radius = None;
    let mut constant_radii = true;
    let mut uniform_radii = true;
    let mut variable_radii = true;
    let mut pairs = Vec::new();
    let mut complete_pairs = true;
    for procedural in &ir.model.procedural_surfaces {
        let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) else {
            continue;
        };
        if !body_surfaces.contains(owner) {
            continue;
        }
        let ProceduralSurfaceDefinition::Blend(definition_payload) = procedural.definition() else {
            continue;
        };
        let supports = definition_payload.supports();
        let radius = definition_payload.radius();
        let cross_section = definition_payload.cross_section();

        if *cross_section != BlendCrossSection::Circular {
            return None;
        }

        surfaces.push(owner.clone());
        match radius {
            BlendRadiusLaw::Constant { signed_radius } if signed_radius.get() != 0.0 => {
                let magnitude = signed_radius.get().abs();
                if first_radius.is_some_and(|first: f64| first.to_bits() != magnitude.to_bits()) {
                    uniform_radii = false;
                }
                first_radius.get_or_insert(magnitude);
                variable_radii = false;
            }
            BlendRadiusLaw::Linear { .. } | BlendRadiusLaw::Law { .. } => {
                constant_radii = false;
            }
            BlendRadiusLaw::Constant { .. } => {
                constant_radii = false;
                variable_radii = false;
            }
        }
        if matches!(family, NxBlendFamily::Face) {
            if let [Some(first), Some(second)] = supports {
                if first.surface == second.surface {
                    complete_pairs = false;
                } else {
                    pairs.push([first.surface.clone(), second.surface.clone()]);
                }
            } else {
                complete_pairs = false;
            }
        }
    }
    if surfaces.is_empty() {
        return None;
    }
    surfaces.sort_by(Ord::cmp);
    let radius = if constant_radii {
        if uniform_radii {
            first_radius
                .and_then(cadmpeg_ir::scalar::PositiveLength::new)
                .map_or(
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
                    },
                    |radius| RadiusSpec::Constant { radius },
                )
        } else {
            RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
            }
        }
    } else if variable_radii {
        RadiusSpec::Unresolved {
            form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable),
        }
    } else {
        RadiusSpec::Unresolved { form: None }
    };
    let face_blend = if matches!(family, NxBlendFamily::Face) && complete_pairs {
        if let Some(sides) = blend_support_bipartition(&pairs) {
            let first_native = format!("{body}:blend-first-support-surfaces");
            let second_native = format!("{body}:blend-second-support-surfaces");
            let (first_faces, _) = support_face_projection(ir, &sides.first, first_native);
            let (second_faces, _) = support_face_projection(ir, &sides.second, second_native);
            match (&first_faces, &second_faces) {
                (FaceSelection::Resolved { .. }, FaceSelection::Resolved { .. }) => {
                    cadmpeg_ir::features::FaceBlendOperands::new(first_faces, second_faces)
                        .ok()
                        .map(|operands| {
                            FeatureDefinition::Operation(FeatureOperation::FaceBlend {
                                operands,
                                radius: radius.clone(),
                            })
                        })
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    let unresolved_operands = cadmpeg_ir::features::FaceBlendOperands::new(
        FaceSelection::Unresolved,
        FaceSelection::Unresolved,
    )
    .ok()?;
    let unresolved = match family {
        NxBlendFamily::Edge => FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius,
                    tangency_weight: None,
                },
            ),
        }),
        NxBlendFamily::Face => FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: unresolved_operands,
            radius,
        }),
    };
    Some((face_blend.unwrap_or(unresolved), surfaces))
}

/// Split an unordered rolling-ball support graph into two deterministic face
/// sets. Face blending is symmetric, so each connected component starts with
/// its lowest surface identity on the first side. The support graph must be
/// complete bipartite: odd cycles and missing cross-pairs cannot be represented
/// by one neutral face-blend operation.
pub(super) struct BlendSides {
    pub(super) first: Vec<SurfaceId>,
    pub(super) second: Vec<SurfaceId>,
}

pub(super) fn blend_support_bipartition(pairs: &[[SurfaceId; 2]]) -> Option<BlendSides> {
    let mut adjacent = BTreeMap::<&SurfaceId, BTreeSet<&SurfaceId>>::new();
    for [first, second] in pairs {
        if first == second {
            return None;
        }
        for (from, to) in [(first, second), (second, first)] {
            let neighbors = adjacent.entry(from).or_default();
            if !neighbors.contains(to) {
                neighbors.insert(to);
            }
        }
    }
    let mut sides = BTreeMap::<&SurfaceId, bool>::new();
    let mut pending = Vec::new();
    for seed in adjacent.keys() {
        if sides.contains_key(seed) {
            continue;
        }
        sides.insert(*seed, false);
        pending.push(*seed);
        while let Some(surface) = pending.pop() {
            let side = sides[&surface];
            for neighbor in &adjacent[&surface] {
                match sides.get(neighbor) {
                    Some(neighbor_side) if *neighbor_side == side => return None,
                    Some(_) => {}
                    None => {
                        sides.insert(*neighbor, !side);
                        pending.push(*neighbor);
                    }
                }
            }
        }
    }
    let mut first = Vec::new();
    let mut second = Vec::new();
    for (&surface, &second_side) in &sides {
        let output = if second_side { &mut second } else { &mut first };

        output.push(surface.clone());
    }
    for surface in &first {
        for other in &second {
            if !adjacent[surface].contains(other) {
                return None;
            }
        }
    }
    Some(BlendSides { first, second })
}

pub(super) fn offset_surface_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let OwnedOffsetSurfaceData {
        body,
        distance,
        supports,
    } = owned_offset_surface_data(ir, outputs)?;
    let native = format!("{}:offset-support-surfaces", body.as_str());
    let (faces, senses) = support_face_projection(ir, &supports, native);
    let distance = senses
        .as_deref()
        .and_then(uniform_face_sense)
        .map(|sense| match sense {
            Sense::Forward => distance,
            Sense::Reversed => distance.negated(),
        });
    Some((
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces,
            distance: distance.map(Length::from_assigned_real),
        }),
        supports,
    ))
}

pub(super) struct OwnedOffsetSurfaceData<'a> {
    body: &'a BodyId,
    distance: FiniteReal,
    supports: Vec<SurfaceId>,
}

pub(super) fn owned_offset_surface_data<'a>(
    ir: &'a CadIr,
    outputs: &'a [BodyId],
) -> Option<OwnedOffsetSurfaceData<'a>> {
    let (body, carriers) = owned_offset_carriers(ir, outputs)?;
    let distance = carriers[0].1;
    if carriers
        .iter()
        .any(|(_, candidate)| candidate.get().to_bits() != distance.get().to_bits())
    {
        return None;
    }
    let supports = unique_carrier_supports(&carriers);
    Some(OwnedOffsetSurfaceData {
        body,
        distance,
        supports,
    })
}

pub(super) fn unique_carrier_supports(carriers: &[(&SurfaceId, FiniteReal)]) -> Vec<SurfaceId> {
    let mut supports = Vec::new();
    for &(support, _) in carriers {
        if supports.contains(support) {
            continue;
        }

        supports.push(support.clone());
    }
    supports.sort_by(Ord::cmp);
    supports
}

/// The single output body and the offset carriers it owns, each as its support
/// surface and offset distance.
type OwnedOffsetCarriers<'a> = (&'a BodyId, Vec<(&'a SurfaceId, FiniteReal)>);

pub(super) fn owned_offset_carriers<'a>(
    ir: &'a CadIr,
    outputs: &'a [BodyId],
) -> Option<OwnedOffsetCarriers<'a>> {
    let [body] = outputs else {
        return None;
    };
    let body_surfaces = body_surface_ids(ir, body)?;
    let mut carriers = Vec::new();
    for procedural in &ir.model.procedural_surfaces {
        let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) else {
            continue;
        };
        if !body_surfaces.contains(owner) {
            continue;
        }
        let ProceduralSurfaceDefinition::Offset(definition_payload) = procedural.definition()
        else {
            continue;
        };
        let support = definition_payload.support();
        let candidate = definition_payload.distance();
        carriers.push((support, candidate));
    }
    (!carriers.is_empty()).then_some((body, carriers))
}

pub(super) fn thicken_feature_definition(
    ir: &CadIr,
    outputs: &[BodyId],
) -> Option<(FeatureDefinition, Vec<SurfaceId>)> {
    let OwnedThickenSurfaceData {
        body,
        thickness,
        supports,
        direction,
    } = owned_thicken_surface_data(ir, outputs)?;
    let native = format!("{}:thicken-support-surfaces", body.as_str());
    let (faces, senses) = support_face_projection(ir, &supports, native);
    let side = match direction {
        ThickenDirection::Both => Some(ThickenSide::Both),
        ThickenDirection::Signed(distance) => senses
            .as_deref()
            .and_then(uniform_face_sense)
            .map(|sense| thicken_side(distance, sense)),
    };
    Some((
        FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces,
            thickness: Some(thickness),
            side,
        }),
        supports,
    ))
}

pub(super) enum ThickenDirection {
    Signed(NonZeroLength),
    Both,
}

pub(super) struct OwnedThickenSurfaceData<'a> {
    body: &'a BodyId,
    thickness: PositiveLength,
    supports: Vec<SurfaceId>,
    direction: ThickenDirection,
}

pub(super) fn owned_thicken_surface_data<'a>(
    ir: &'a CadIr,
    outputs: &'a [BodyId],
) -> Option<OwnedThickenSurfaceData<'a>> {
    let (body, carriers) = owned_offset_carriers(ir, outputs)?;
    let output_body = ir
        .model
        .bodies
        .iter()
        .find(|candidate| candidate.id == *body)?;
    if output_body.kind != BodyKind::Solid {
        return None;
    }
    let distance = carriers[0].1;
    if carriers
        .iter()
        .all(|(_, candidate)| candidate.get().to_bits() == distance.get().to_bits())
    {
        if let Ok(distance) = NonZeroLength::try_from(Length::from_assigned_real(distance)) {
            let supports = unique_carrier_supports(&carriers);
            return Some(OwnedThickenSurfaceData {
                body,
                thickness: distance.abs(),
                supports,
                direction: ThickenDirection::Signed(distance),
            });
        }
        return None;
    }

    let mut magnitude = None::<PositiveLength>;
    let mut positive = BTreeSet::new();
    let mut negative = BTreeSet::new();
    for &(support, distance) in &carriers {
        let Ok(distance) = NonZeroLength::try_from(Length::from_assigned_real(distance)) else {
            return None;
        };
        let candidate = distance.abs();
        if magnitude.is_some_and(|magnitude| magnitude.get().to_bits() != candidate.get().to_bits())
        {
            return None;
        }
        magnitude = Some(candidate);
        if distance.get().is_sign_positive() {
            positive.insert(support);
        } else {
            negative.insert(support);
        }
    }
    if positive.is_empty() || positive != negative {
        return None;
    }
    let magnitude = magnitude?;
    let thickness = PositiveLength::new(magnitude.get() * 2.0)?;
    let mut supports = Vec::new();
    for support in positive {
        supports.push(support.clone());
    }
    Some(OwnedThickenSurfaceData {
        body,
        thickness,
        supports,
        direction: ThickenDirection::Both,
    })
}

pub(super) fn support_face_projection(
    ir: &CadIr,
    supports: &[SurfaceId],
    native: String,
) -> (
    cadmpeg_ir::features::FaceSelection,
    std::option::Option<std::vec::Vec<cadmpeg_ir::topology::Sense>>,
) {
    let mut selected = Vec::new();
    for support in supports {
        let mut matches = ir
            .model
            .faces
            .iter()
            .filter(|face| face.surface == *support);
        let Some(face) = matches.next() else {
            return (FaceSelection::Native(native), None);
        };
        if matches.next().is_some() {
            return (FaceSelection::Native(native), None);
        }

        if selected.iter().any(|(id, _)| *id == face.id) {
            return (FaceSelection::Native(native), None);
        }

        selected.push((face.id.clone(), face.sense));
    }
    let (faces, senses): (Vec<_>, Vec<_>) = selected.into_iter().unzip();
    (FaceSelection::Resolved { faces, native }, Some(senses))
}

pub(super) fn thicken_side(distance: NonZeroLength, sense: Sense) -> ThickenSide {
    match (distance.get().is_sign_positive(), sense) {
        (true, Sense::Forward) | (false, Sense::Reversed) => ThickenSide::Forward,
        (true, Sense::Reversed) | (false, Sense::Forward) => ThickenSide::Reverse,
    }
}

pub(super) fn uniform_face_sense(senses: &[Sense]) -> Option<Sense> {
    let (first, rest) = senses.split_first()?;
    rest.iter().all(|sense| sense == first).then_some(*first)
}

pub(in crate::native) fn feature_source_content(
    payload_strings: &[&crate::native::features::FeaturePayloadString],
) -> Result<cadmpeg_ir::features::FeatureContent, CodecError> {
    let mut sorted = payload_strings.to_vec();
    sorted.sort_by_key(|first| first.source_offset);
    let content = sorted
        .into_iter()
        .map(|value| FeatureSourceContent::Text(value.value.as_str().to_owned()))
        .collect::<Vec<_>>();
    cadmpeg_ir::features::FeatureContent::try_from(content).map_err(CodecError::from)
}

pub(super) fn simple_hole_native_properties(
    properties: &mut BTreeMap<String, String>,
    operation_label: &str,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    repeated_lanes: &[crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLane],
    block_references: &[crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
    construction_groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
) {
    fn insert_property(properties: &mut BTreeMap<String, String>, key: &'static str, value: &str) {
        properties.insert(key.to_owned(), value.to_owned());
    }
    if let Some(template) = templates
        .iter()
        .find(|template| template.operation_label == operation_label)
    {
        insert_property(properties, "simple_hole_template", &template.id);
    }
    if let Some(pair) = repeated_lanes
        .iter()
        .find(|pair| pair.operation_label == operation_label)
    {
        insert_property(properties, "simple_hole_repeated_scalar_lane", &pair.id);
    }
    if let Some(references) = block_references
        .iter()
        .find(|references| references.operation_label == operation_label)
    {
        insert_property(
            properties,
            "simple_hole_repeated_scalar_lane_block_references",
            &references.id,
        );
    }
    if let Some(group) = construction_groups.iter().find(|group| {
        group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|label| label == operation_label)
    }) {
        insert_property(properties, "simple_hole_construction_group", &group.id);
    }
}

pub(super) fn block_placement(
    ir: &CadIr,
    dimensions: [f64; 3],
    outputs: &[BodyId],
) -> Option<(BodyId, Transform)> {
    struct PlaneBand {
        normal: Vector3,
        offsets: Vec<f64>,
    }

    #[derive(Clone, Copy)]
    struct PlaneExtent {
        normal: Vector3,
        minimum: f64,
        maximum: f64,
    }

    fn canonical_normal(
        normal: cadmpeg_ir::units::UnitVector3,
        angular_tolerance: f64,
    ) -> Option<Vector3> {
        let mut normal = *normal.to_unit_length_charted().as_raw();
        let leading = [normal.x, normal.y, normal.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)?;
        if leading < 0.0 {
            normal = Vector3::new(-normal.x, -normal.y, -normal.z);
        }
        Some(normal)
    }

    fn plane_extent(band: &mut PlaneBand, linear_tolerance: f64) -> Option<PlaneExtent> {
        band.offsets.sort_by(f64::total_cmp);
        let mut first: Option<[f64; 2]> = None;
        let mut second: Option<[f64; 2]> = None;
        for &offset in &band.offsets {
            if !offset.is_finite() {
                return None;
            }
            if let Some(cluster) = second.as_mut().or(first.as_mut()) {
                if offset - cluster[0] <= linear_tolerance {
                    cluster[1] = offset;
                    continue;
                }
            }
            if first.is_none() {
                first = Some([offset, offset]);
            } else if second.is_none() {
                second = Some([offset, offset]);
            } else {
                return None;
            }
        }
        let (Some(minimum), Some(maximum)) = (first, second) else {
            return None;
        };
        if maximum[1] - minimum[0] <= linear_tolerance {
            return None;
        }
        Some(PlaneExtent {
            normal: band.normal,
            minimum: minimum[0],
            maximum: maximum[1],
        })
    }

    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    if dimensions
        .iter()
        .any(|dimension| *dimension <= linear_tolerance)
    {
        return None;
    }
    let body = match outputs {
        [body] => body,
        [] => {
            let mut unique = None;
            for candidate in &ir.model.bodies {
                if connected_solid_body_faces(ir, &candidate.id).is_some()
                    && unique.replace(&candidate.id).is_some()
                {
                    return None;
                }
            }

            unique?
        }
        _ => return None,
    };
    let faces = connected_solid_body_faces(ir, body)?;
    let mut bands = Vec::<PlaneBand>::new();
    for face in faces.iter().copied() {
        let geometry = ir
            .model
            .surfaces
            .iter()
            .rev()
            .find(|surface| surface.id == face.surface)
            .map(|surface| &surface.geometry)?;
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = geometry else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let normal = canonical_normal(*plane_surface.frame().axis(), angular_tolerance)?;
        let offset = normal.dot(Vector3::new(origin.x, origin.y, origin.z));

        let existing = bands
            .iter_mut()
            .find(|band| (1.0 - band.normal.dot(normal)).abs() <= angular_tolerance);

        if let Some(band) = existing {
            band.offsets.push(offset);
        } else {
            bands.push(PlaneBand {
                normal,
                offsets: vec![offset],
            });
        }
    }
    if bands.len() != 3
        || (0..3).any(|first| {
            (first + 1..3).any(|second| {
                bands[first].normal.dot(bands[second].normal).abs() > angular_tolerance
            })
        })
    {
        return None;
    }
    let [first, second, third] = bands.as_mut_slice() else {
        return None;
    };
    let (Some(first), Some(second), Some(third)) = (
        plane_extent(first, linear_tolerance),
        plane_extent(second, linear_tolerance),
        plane_extent(third, linear_tolerance),
    ) else {
        return None;
    };
    let mut extents = [first, second, third];
    extents.sort_by(|left, right| {
        right
            .normal
            .x
            .total_cmp(&left.normal.x)
            .then_with(|| right.normal.y.total_cmp(&left.normal.y))
            .then_with(|| right.normal.z.total_cmp(&left.normal.z))
    });
    let permutations = [
        [0usize, 1usize, 2usize],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut matched = None;
    for permutation in permutations {
        if (0..3).all(|axis| {
            let band = extents[permutation[axis]];
            ((band.maximum - band.minimum) - dimensions[axis]).abs() <= linear_tolerance
        }) && matched.replace(permutation).is_some()
        {
            return None;
        }
    }
    let permutation = matched?;
    let mut ordered = permutation.map(|index| extents[index]);
    if ordered[0]
        .normal
        .cross(ordered[1].normal)
        .dot(ordered[2].normal)
        < 0.0
    {
        let third = &mut ordered[2];
        third.normal = Vector3::new(-third.normal.x, -third.normal.y, -third.normal.z);
        (third.minimum, third.maximum) = (-third.maximum, -third.minimum);
    }
    let origin = Point3::new(
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.x)
            .sum(),
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.y)
            .sum(),
        ordered
            .iter()
            .map(|band| band.minimum * band.normal.z)
            .sum(),
    );
    let [x_axis, y_axis, z_axis] = ordered.map(|band| band.normal);
    let placement = Transform::affine([
        [x_axis.x, y_axis.x, z_axis.x, origin.x],
        [x_axis.y, y_axis.y, z_axis.y, origin.y],
        [x_axis.z, y_axis.z, z_axis.z, origin.z],
    ])?;

    Some((body.clone(), placement))
}

/// Return the complete primitive witness for an NX `SPHERE` operation.
///
/// A spherical surface inside a larger result is not enough: a Boolean or a
/// later feature can leave the same carrier in the output. The primitive
/// projection therefore accepts only one connected solid body with exactly
/// one face whose surface is a finite positive-radius sphere. With no native
/// output relation, the candidate must also be unique across the model.
pub(super) fn sphere_body_projection(
    ir: &CadIr,
    outputs: &[BodyId],
) -> Option<(
    BodyId,
    cadmpeg_ir::features::FinitePoint3,
    cadmpeg_ir::scalar::PositiveLength,
)> {
    let body = match outputs {
        [body] => body,
        [] => {
            let mut unique = None;
            for candidate in &ir.model.bodies {
                let Some(faces) = connected_solid_body_faces(ir, &candidate.id) else {
                    continue;
                };
                let [face] = &faces[..] else {
                    continue;
                };

                if !ir.model.surfaces.iter().any(|surface| {
                    surface.id == face.surface
                        && matches!(
                            surface.geometry.solved(),
                            Some(SolvedSurfaceGeometry::Sphere(_))
                        )
                }) {
                    continue;
                }
                if unique.replace(&candidate.id).is_some() {
                    return None;
                }
            }

            unique?
        }
        _ => return None,
    };
    let faces = connected_solid_body_faces(ir, body)?;
    let [face] = &faces[..] else {
        return None;
    };

    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)?;
    let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
        return None;
    };
    let center = sphere_surface.center();
    let Ok(radius) = cadmpeg_ir::scalar::PositiveLength::try_from(sphere_surface.radius()) else {
        return None;
    };

    Some((body.clone(), center, radius))
}

pub(super) struct NewBodyEvidence<'a> {
    pub(super) has_complete_projection: bool,
    pub(super) has_complete_primitive_construction: bool,
    pub(super) outputs: &'a [BodyId],
    pub(super) body_reference_count: usize,
    pub(super) provisional_feature: Option<&'a FeatureId>,
    pub(super) native_primary_body: Option<u32>,
    pub(super) offset_store_primary_body: Option<&'a str>,
    pub(super) history: &'a BodyWriterHistory,
}

pub(super) fn new_body_boolean_op(evidence: &NewBodyEvidence<'_>) -> BooleanOp {
    // A unique offset-store body field proves the operation's local writer
    // namespace, but the fallback body selected for placement is not that
    // writer. Likewise, multiple body fields have no primary role until the
    // operation-specific relation identifies one. Do not let a placement
    // fallback turn either case into a neutral body Boolean.
    if evidence.body_reference_count > 1
        && evidence.native_primary_body.is_none()
        && evidence.offset_store_primary_body.is_none()
        && !evidence.has_complete_primitive_construction
    {
        return BooleanOp::Unresolved;
    }
    if evidence.has_complete_projection
        && matches!(evidence.outputs, [_])
        && !evidence.history.has_preceding_writer(
            evidence.provisional_feature,
            evidence.native_primary_body,
            evidence.offset_store_primary_body,
            evidence.outputs,
        )
    {
        BooleanOp::NewBody
    } else {
        BooleanOp::Unresolved
    }
}

pub(super) fn body_writing_unresolved_feature_definition(
    kind: &str,
    source_properties: &BTreeMap<String, String>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let mut body_write = false;
    for key in source_properties.keys() {
        if key.len() < "body_write.".len() {
            continue;
        }

        if key.starts_with("body_write.") {
            body_write = true;
            break;
        }
    }
    if !body_write {
        return Ok(None);
    }
    Ok(match kind {
        "BREP" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Brep,
        })),
        "CONE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Cone,
        })),
        "SPHERE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Sphere,
        })),
        "BLEND" => Some(FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                },
            ),
        })),
        "FACE_BLEND" => Some(FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                FaceSelection::Unresolved,
                FaceSelection::Unresolved,
            )
            .map_err(CodecError::malformed)?,

            radius: RadiusSpec::Unresolved { form: None },
        })),
        "DELETE FACE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DeleteFace,
        })),
        "MIRROR_FACE" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MirrorFace,
        })),
        "SUBDIVISION_BODY" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::SubdivisionBody,
        })),
        "TOPOLOGY_OPTIMIZATION" => {
            Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::TopologyOptimization,
            }))
        }
        "THREADS" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Thread,
        })),
        "DETAILED_THREAD" => Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DetailedThread,
        })),
        _ => None,
    })
}

#[cfg(test)]
pub(super) fn non_boolean_feature_definition(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole_diameter: Option<Length>,
) -> FeatureDefinition {
    non_boolean_feature_definition_with_parameters(
        kind,
        payload_strings,
        block_dimensions,
        block_placement,
        HoleProjection {
            diameter: hole_diameter,
            ..HoleProjection::default()
        },
        BTreeMap::new(),
    )
    .unwrap()
}

/// Project one operation as a history node only when its bounded record has
/// no modeling relation or value lane.
pub(super) fn non_modeling_history_definition(
    kind: &str,
    object_indices: &[Option<u32>; 4],
    outputs: &[BodyId],
    body_reference_count: usize,
    body_operand_count: usize,
    payload_string_count: usize,
    source_properties: &BTreeMap<String, String>,
) -> Option<FeatureDefinition> {
    let operation_identity_only = source_properties.keys().all(|key| {
        matches!(
            key.as_str(),
            "operation_record" | "operation_terminal_frame"
        ) || (key
            .strip_prefix("object_index.")
            .is_some_and(|slot| matches!(slot, "0" | "1" | "2" | "3")))
    });
    (kind == "EXTRACT_STRING"
        && object_indices.iter().all(Option::is_none)
        && outputs.is_empty()
        && body_reference_count == 0
        && body_operand_count == 0
        && payload_string_count == 0
        && source_properties.contains_key("operation_record")
        && source_properties.contains_key("operation_terminal_frame")
        && operation_identity_only)
        .then_some(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: TreeChildren::default(),
        }))
}

/// Permutation-invariant hole properties derived from one complete body partition.
#[derive(Default)]
pub(super) struct HoleProjection {
    pub(super) placements: Vec<HolePlacement>,
    pub(super) diameter: Option<Length>,
    pub(super) extent: Option<LinearTermination>,
    pub(super) counterbore: Option<CounterboreDimensions>,
    pub(super) chamfer: Option<HoleKind>,
    pub(super) grouped_simple_through: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CounterboreDimensions {
    pub(super) diameter: cadmpeg_ir::scalar::PositiveLength,
    pub(super) depth: cadmpeg_ir::scalar::PositiveLength,
}

pub(super) fn non_boolean_feature_definition_with_parameters(
    kind: &str,
    payload_strings: &[&str],
    block_dimensions: Option<[f64; 3]>,
    block_placement: Option<Transform>,
    hole: HoleProjection,
    native_parameters: BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) -> Result<FeatureDefinition, CodecError> {
    let hole_template = unique_simple_hole_template(payload_strings);
    if matches!(kind, "BLEND" | "FACE_BLEND") {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: kind.into(),
            parameters: native_parameters,
        }));
    }
    if let ("BLOCK", Some([Some(length), Some(width), Some(height)])) = (
        kind,
        block_dimensions.map(|dimensions| dimensions.map(cadmpeg_ir::scalar::PositiveLength::new)),
    ) {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions: Some([length, width, height]),
            placement: block_placement.and_then(cadmpeg_ir::features::FeatureRigidPlacement::new),
            op: BooleanOp::Unresolved,
        }));
    }
    if let Some(op) = match kind {
        "UNITE" => Some(cadmpeg_ir::features::BooleanKind::Join),
        "SUBTRACT" => Some(cadmpeg_ir::features::BooleanKind::Cut),
        "INTERSECT" => Some(cadmpeg_ir::features::BooleanKind::Intersect),
        _ => None,
    } {
        return Ok(FeatureDefinition::Operation(FeatureOperation::Combine {
            operands: cadmpeg_ir::features::CombineOperands::new(
                BodySelection::Unresolved,
                BodySelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            op,
            keep_tools: false,
        }));
    }
    Ok(match kind {
        "DATUM_PLANE" | "EXTRACT_DATUM_PLANE" => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPlane,
            })
        }
        "DATUM_AXIS" | "EXTRACT_DATUM_AXIS" => {
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumAxis,
            })
        }
        "BRIDGE_CURVE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::BridgeCurve,
        }),
        "POINT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPoint,
        }),
        "DATUM_CSYS" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumCoordinateSystem,
        }),
        "BLOCK" => FeatureDefinition::Operation(FeatureOperation::Block {
            dimensions: None,
            placement: None,
            op: BooleanOp::Unresolved,
        }),
        "SKETCH" => FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
        }),
        "EXTRACT_BODY" => FeatureDefinition::Operation(FeatureOperation::ExtractBody {
            source: BodySelection::Unresolved,
        }),
        "MASTER SNAPSHOT BODY" => FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Unresolved,
        }),
        "SKIN" | "THRU_CURVE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Loft,
        }),
        "THRU_CURVE_MESH" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::ThroughCurveMesh,
        }),
        "Studio Surface" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::FreeformSurface,
        }),
        "SWP104" => FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::unresolved(None),
            path: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            scale: None,
            allow_multi_profile_faces: None,
        }),
        "DRAFT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Draft,
        }),
        "CPROJ" | "CPROJ_CMB" => FeatureDefinition::Operation(FeatureOperation::ProjectedCurve {
            source: PathRef::Unresolved("nx:unresolved".into()),
            target_faces: FaceSelection::Unresolved,
            direction: CurveProjectionDirection::State(CurveProjectionDirectionState::Unresolved),
            bidirectional: None,
        }),
        "TRIMMED_SH" => FeatureDefinition::Operation(FeatureOperation::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: PathRef::Unresolved("nx:unresolved".into()),
            keep: TrimRegion::Unresolved,
        }),
        "EXTRACT_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::ExtractFace,
        }),
        "COPY_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::CopyFace,
        }),
        "LINKED_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::LinkedFace,
        }),
        "FILL_HOLE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::FillHole,
        }),
        "MOVE_FACE" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MoveFace,
        }),
        "MOVE_OBJECT" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MoveObject,
        }),
        "CYLINDER" => FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Cylinder,
        }),
        "SYMBOLIC_THREAD" => symbolic_thread_feature_definition(),
        "EXTEND_SHEET" => FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        }),
        "SIMPLE HOLE" | "CBORE_HOLE" | "CSUNK_HOLE" => {
            let measured_chamfer = hole.chamfer;
            let (template_kind, template_exit_kind, template_extent) = hole_template.map_or(
                (
                    if matches!(kind, "CBORE_HOLE" | "CSUNK_HOLE") {
                        HoleKind::Unresolved(None)
                    } else {
                        HoleKind::Simple
                    },
                    None,
                    None,
                ),
                |(form, extent, start_treatment, end_treatment)| {
                    let kind = match start_treatment {
                        crate::native::features::holes::SimpleHoleEndTreatment::Chamfer => {
                            HoleKind::Unresolved(Some(HoleForm::Chamfer))
                        }
                        crate::native::features::holes::SimpleHoleEndTreatment::None => {
                            match form {
                                crate::native::features::holes::SimpleHoleForm::Simple => {
                                    HoleKind::Simple
                                }
                                crate::native::features::holes::SimpleHoleForm::Counterbored => {
                                    HoleKind::Unresolved(Some(HoleForm::Counterbore))
                                }
                                crate::native::features::holes::SimpleHoleForm::Countersunk => {
                                    HoleKind::Unresolved(Some(HoleForm::Countersink))
                                }
                            }
                        }
                    };
                    let exit_kind = match end_treatment {
                        crate::native::features::holes::SimpleHoleEndTreatment::Chamfer => {
                            Some(HoleKind::Unresolved(Some(HoleForm::Chamfer)))
                        }
                        crate::native::features::holes::SimpleHoleEndTreatment::None => None,
                    };
                    let extent = match extent {
                        crate::native::features::holes::SimpleHoleExtent::Through => {
                            Some(cadmpeg_ir::features::LinearTermination::ThroughAll {})
                        }
                        crate::native::features::holes::SimpleHoleExtent::Blind => None,
                    };
                    (kind, exit_kind, extent)
                },
            );
            let template_kind = match (
                hole.counterbore,
                matches!(
                    &template_kind,
                    HoleKind::Unresolved(Some(HoleForm::Counterbore))
                        | HoleKind::PartialCounterbore(..)
                ),
            ) {
                (Some(dimensions), true) => HoleKind::Counterbore {
                    diameter: dimensions.diameter,
                    depth: dimensions.depth,
                },
                _ => template_kind,
            };
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                direction: None,
                placements: Some(hole.placements).filter(|placements| !placements.is_empty()),
                shape: cadmpeg_ir::features::holes::HoleShape::new(
                    cadmpeg_ir::features::holes::HoleConstruction::Form {
                        kind: match (measured_chamfer, hole_template) {
                            (
                                Some(chamfer),
                                Some((
                                    crate::native::features::holes::SimpleHoleForm::Simple,
                                    crate::native::features::holes::SimpleHoleExtent::Through,
                                    crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                    crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                )),
                            ) => chamfer,
                            _ => template_kind,
                        },
                        specification: None,
                    },
                    match (measured_chamfer, hole_template) {
                        (
                            Some(chamfer),
                            Some((
                                crate::native::features::holes::SimpleHoleForm::Simple,
                                crate::native::features::holes::SimpleHoleExtent::Through,
                                crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                                crate::native::features::holes::SimpleHoleEndTreatment::Chamfer,
                            )),
                        ) => Some(chamfer),
                        _ => template_exit_kind,
                    },
                    hole.diameter.and_then(|diameter| {
                        cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()
                    }),
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,

                extent: hole.extent.or(template_extent),
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            })
        }
        "HOLE PACKAGE" => FeatureDefinition::Operation(FeatureOperation::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            direction: None,
            placements: Some(hole.placements).filter(|placements| !placements.is_empty()),
            shape: cadmpeg_ir::features::holes::HoleShape::new(
                cadmpeg_ir::features::holes::HoleConstruction::Form {
                    kind: if hole.grouped_simple_through {
                        hole.chamfer.unwrap_or(HoleKind::Simple)
                    } else {
                        HoleKind::Unresolved(None)
                    },
                    specification: None,
                },
                hole.grouped_simple_through
                    .then_some(hole.chamfer)
                    .flatten(),
                hole.diameter.and_then(|diameter| {
                    cadmpeg_ir::scalar::PositiveLength::try_from(diameter).ok()
                }),
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            extent: hole
                .grouped_simple_through
                .then_some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        }),
        "RIB" => FeatureDefinition::Operation(FeatureOperation::Rib {
            construction: RibConstruction {
                profile: None,
                direction: None,
                thickness: None,
                side: None,
                draft: RibDraft::Unresolved,
            },
            op: BooleanOp::Unresolved,
        }),
        "SHELL" => shell_feature_definition(),
        "ENLARGE" => enlarge_feature_definition(),
        "CHAMFER" => FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Unresolved { form: None },
                },
            ),
            flip_direction: false,
        }),
        "BLEND" => FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                },
            ),
        }),
        "FACE_BLEND" => FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                FaceSelection::Unresolved,
                FaceSelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            radius: RadiusSpec::Unresolved { form: None },
        }),
        "SEW" => FeatureDefinition::Operation(FeatureOperation::SewBodies {
            bodies: BodySelection::Unresolved
                .try_into()
                .map_err(CodecError::malformed)?,
            gap_tolerance: None,
        }),
        "TRIM BODY" => FeatureDefinition::Operation(FeatureOperation::TrimBodies {
            operands: cadmpeg_ir::features::TrimBodyOperands::new(
                BodySelection::Unresolved,
                BodySelection::Unresolved,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,

            keep: BodyTrimSide::Unresolved,
        }),
        "EXTRUDE" => extrude_feature_definition(None, None, BooleanOp::Unresolved, &[]),
        "OFFSET" => FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
        }),
        "THICKEN_SHEET" => FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        }),
        "Pattern Feature"
        | "Pattern Geometry"
        | "Geometry Instance"
        | "Multi Instance Output"
        | "IDENTICAL INSTANCE OUTPUT"
        | "Instance Feature" => FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::UNRESOLVED,
        }),
        "ASSOCIATIVE_INTERSECTION" | "Intersection Curve" => {
            FeatureDefinition::Operation(FeatureOperation::SectionShape {
                operands: cadmpeg_ir::features::SectionOperands::new(
                    BodySelection::Unresolved,
                    BodySelection::Unresolved,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,

                approximate: None,
            })
        }
        _ => FeatureDefinition::Operation(FeatureOperation::Native {
            kind: kind.into(),
            parameters: native_parameters,
        }),
    })
}

/// Project a BREP operation as direct stored geometry when its result bodies
/// are resolved. A BREP record carries boundary representation rather than a
/// replayable parametric construction; without a closed result-body relation,
/// retaining the native definition preserves the unresolved history edge.
pub(super) fn brep_feature_definition(outputs: &[BodyId]) -> Option<FeatureDefinition> {
    for (index, body) in outputs.iter().enumerate() {
        if outputs[..index].contains(body) {
            return None;
        }
    }
    (!outputs.is_empty()).then_some(FeatureDefinition::Operation(
        FeatureOperation::StoredGeometry {},
    ))
}

/// Preserve a SHELL operation as a typed neutral family while its construction roles remain
/// unresolved. The operation label identifies the family, but does not assign bodies, opening
/// faces, thickness, side, offset mode, corner join, or intersection policy.
pub(super) fn shell_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: None,
        removed_faces: FaceSelection::Unresolved,
        thickness: None,
        outward: None,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    })
}

/// Preserve an ENLARGE operation as a typed surface-extension family while its selected faces,
/// extension law, and extent remain unresolved.
pub(super) fn enlarge_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
        faces: FaceSelection::Unresolved,
        distance: None,
        method: SurfaceExtension::Unresolved,
    })
}

/// Preserve a `SYMBOLIC_THREAD` operation as a cosmetic-thread family while its cylindrical face,
/// nominal diameter, and axial extent remain unresolved.
pub(super) fn symbolic_thread_feature_definition() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
        face: FaceSelection::Unresolved,
        diameter: None,
        extent: None,
    })
}

pub(super) fn native_feature_parameters(
    uses: &[&crate::native::features::FeatureParameterUse],
    expressions: &[crate::native::om::ParameterFormula],
) -> BTreeMap<String, String> {
    let mut parameters = BTreeMap::new();
    for parameter_use in uses {
        let Some(expression) = expressions
            .iter()
            .rev()
            .find(|expression| expression.id == parameter_use.expression)
        else {
            return BTreeMap::new();
        };
        if parameters.contains_key(expression.name.as_str()) {
            return BTreeMap::new();
        }
        parameters.insert(
            expression.name.as_str().to_owned(),
            expression.expression.clone(),
        );
    }
    parameters
}

/// Resolve explicit hole outputs only from the proven segment-body namespace.
/// Offset-store body fields remain absent so a complete unique-solid topology
/// witness can apply the documented fallback.
pub(super) fn primary_hole_outputs(
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    body_references: &BTreeMap<&str, u32>,
    body_bindings: &[crate::native::segments::SegmentBodyBinding],
    bodies_by_object_index: &BTreeMap<u32, Vec<BodyId>>,
) -> BTreeMap<String, Vec<BodyId>> {
    let mut outputs = BTreeMap::new();
    for template in templates {
        let Some(object_index) = body_references.get(template.operation_label.as_str()) else {
            continue;
        };
        let bodies = feature_body_outputs(*object_index, body_bindings, bodies_by_object_index);
        outputs.insert(template.operation_label.clone(), bodies);
    }
    outputs
}

pub(super) fn simple_hole_operations(
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
    operation_positions: &BTreeMap<&str, usize>,
) -> Option<Vec<String>> {
    let mut ordered_templates = Vec::new();
    for template in templates {
        if template.form != crate::native::features::holes::SimpleHoleForm::Simple
            || template.extent != crate::native::features::holes::SimpleHoleExtent::Through
        {
            continue;
        }

        if templates
            .iter()
            .filter(|candidate| candidate.operation_label == template.operation_label)
            .count()
            != 1
            || !operation_positions.contains_key(template.operation_label.as_str())
        {
            return None;
        }
        ordered_templates.push(template);
    }
    if ordered_templates.is_empty() {
        return None;
    }
    ordered_templates.sort_by(|first, second| {
        operation_positions
            .get(first.operation_label.as_str())
            .cmp(&operation_positions.get(second.operation_label.as_str()))
            .then_with(|| first.operation_label.cmp(&second.operation_label))
    });
    let mut selected_group = None;
    for group in groups {
        let same_operations = group.members.iter().all(|member| {
            ordered_templates
                .iter()
                .any(|template| template.operation_label == member.operation_label)
        }) && ordered_templates.iter().all(|template| {
            group
                .members
                .iter()
                .any(|member| member.operation_label == template.operation_label)
        });
        if same_operations && selected_group.replace(group).is_some() {
            return None;
        }
    }
    let mut operations = Vec::new();
    if let Some(group) = selected_group {
        if group
            .members
            .iter()
            .any(|member| !operation_positions.contains_key(member.operation_label.as_str()))
            || group.members.windows(2).any(|pair| {
                operation_positions[pair[0].operation_label.as_str()]
                    >= operation_positions[pair[1].operation_label.as_str()]
            })
        {
            return None;
        }
        for member in group.members.iter() {
            operations.push(member.operation_label.clone());
        }
    } else {
        for template in ordered_templates {
            operations.push(template.operation_label.clone());
        }
    }
    Some(operations)
}

/// Select uniquely typed hole operations in feature-history order.
pub(super) fn selected_hole_operations(
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
    accepts: impl Fn(&crate::native::features::holes::FeatureSimpleHoleTemplate) -> bool,
) -> Option<Vec<String>> {
    let mut operations = Vec::new();
    for template in templates {
        if !accepts(template) {
            continue;
        }

        if templates
            .iter()
            .filter(|candidate| candidate.operation_label == template.operation_label)
            .count()
            == 1
        {
            operations.push(template.operation_label.clone());
        }
    }
    if operations.is_empty() || !hole_operations_are_unique(&operations) {
        return None;
    }

    if operations
        .iter()
        .any(|operation| !operation_positions.contains_key(operation.as_str()))
    {
        return None;
    }
    operations.sort_by(|first, second| {
        operation_positions
            .get(first.as_str())
            .cmp(&operation_positions.get(second.as_str()))
            .then_with(|| first.cmp(second))
    });
    Some(operations)
}

/// Return simple blind-hole operations in feature-history order. A blind
/// operation with competing typed templates is not assignable to one body
/// witness and remains native-only.
pub(super) fn blind_hole_operations(
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
) -> Option<Vec<String>> {
    selected_hole_operations(templates, operation_positions, |template| {
        template.form == crate::native::features::holes::SimpleHoleForm::Simple
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Blind
    })
}

/// Return counterbored through-hole operations in feature-history order.
/// Counterbore construction groups are not inferred from the scalar lanes:
/// each operation must have its own unambiguous body and topology witness.
pub(super) fn counterbore_operations(
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    operation_positions: &BTreeMap<&str, usize>,
) -> Option<Vec<String>> {
    selected_hole_operations(templates, operation_positions, |template| {
        template.form == crate::native::features::holes::SimpleHoleForm::Counterbored
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Through
            && template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::None
            && template.end_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::None
    })
}

#[derive(Default)]
pub(super) struct HolePackageProjection {
    pub(super) internal_operations: BTreeSet<String>,
    pub(super) outputs: BTreeMap<String, Vec<BodyId>>,
    pub(super) diameters: BTreeMap<String, Length>,
    pub(super) chamfers: BTreeMap<String, HoleKind>,
    pub(super) placements: BTreeMap<String, Vec<HolePlacement>>,
}

#[derive(Clone, Copy)]
pub(super) struct HolePackageSources<'a> {
    pub(super) outputs: &'a BTreeMap<String, Vec<BodyId>>,
    pub(super) diameters: &'a BTreeMap<String, Length>,
    pub(super) chamfers: &'a BTreeMap<String, HoleKind>,
}

pub(super) fn hole_package_projection(
    ir: &CadIr,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    groups: &[crate::native::features::holes::FeatureSimpleHoleConstructionGroup],
    uses: &[crate::native::features::holes::FeatureHolePackageConstructionGroupUse],
    sources: HolePackageSources<'_>,
) -> HolePackageProjection {
    let HolePackageSources {
        outputs,
        diameters,
        chamfers,
    } = sources;
    let mut projection = HolePackageProjection::default();
    for use_ in uses {
        if uses
            .iter()
            .filter(|candidate| candidate.operation_label == use_.operation_label)
            .count()
            != 1
            || uses
                .iter()
                .filter(|candidate| {
                    candidate.simple_hole_construction_group == use_.simple_hole_construction_group
                })
                .count()
                != 1
        {
            continue;
        }

        let Some(group) = groups
            .iter()
            .find(|group| group.id == use_.simple_hole_construction_group)
        else {
            continue;
        };
        if group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|operation| projection.internal_operations.contains(operation))
        {
            continue;
        }
        let mut requests_chamfer = true;
        let mut requests_no_treatment = true;
        let mut complete_templates = true;
        for member in group.members.iter() {
            let mut matches = templates
                .iter()
                .filter(|template| template.operation_label == member.operation_label);
            let Some(template) = matches.next() else {
                complete_templates = false;
                break;
            };
            if matches.next().is_some()
                || template.form != crate::native::features::holes::SimpleHoleForm::Simple
                || template.extent != crate::native::features::holes::SimpleHoleExtent::Through
            {
                complete_templates = false;
                break;
            }
            requests_chamfer &= template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer
                && template.end_treatment
                    == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer;
            requests_no_treatment &= template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::None
                && template.end_treatment
                    == crate::native::features::holes::SimpleHoleEndTreatment::None;
        }
        if !complete_templates {
            continue;
        }
        let Some(body) = group
            .members
            .first()
            .and_then(|member| outputs.get(&member.operation_label))
            .and_then(|bodies| bodies.as_slice().first().filter(|_| bodies.len() == 1))
        else {
            continue;
        };

        if group.members.iter().any(|member| !matches!(outputs.get(&member.operation_label).map(Vec::as_slice), Some([candidate]) if candidate == body))
        {
            continue;
        }
        let Some(diameter) = group
            .members
            .first()
            .map(|member| &member.operation_label)
            .and_then(|operation| diameters.get(operation))
            .copied()
        else {
            continue;
        };
        if group
            .members
            .iter()
            .map(|member| &member.operation_label)
            .any(|operation| diameters.get(operation).copied() != Some(diameter))
        {
            continue;
        }
        if !requests_chamfer && !requests_no_treatment {
            continue;
        }
        let chamfer = if requests_chamfer {
            let Some(chamfer) = group
                .members
                .first()
                .map(|member| &member.operation_label)
                .and_then(|operation| chamfers.get(operation))
                .copied()
            else {
                continue;
            };
            if group
                .members
                .iter()
                .map(|member| &member.operation_label)
                .any(|operation| chamfers.get(operation).copied() != Some(chamfer))
            {
                continue;
            }
            Some(chamfer)
        } else {
            None
        };
        for member in group.members.iter() {
            if projection
                .internal_operations
                .contains(&member.operation_label)
            {
                continue;
            }
            projection
                .internal_operations
                .insert(member.operation_label.clone());
        }
        insert_hole_output_body(&mut projection.outputs, &use_.operation_label, body);
        projection
            .diameters
            .insert(use_.operation_label.clone(), diameter);
        if let Some(chamfer) = chamfer {
            projection
                .chamfers
                .insert(use_.operation_label.clone(), chamfer);
        }
        let placements = hole_axis_placements_for_body(ir, body);
        if placements.len() == group.members.len() {
            projection
                .placements
                .insert(use_.operation_label.clone(), placements);
        }
    }
    projection
}

pub(super) struct HoleBodyProjection {
    pub(super) outputs: BTreeMap<String, Vec<BodyId>>,
    pub(super) diameters: BTreeMap<String, Length>,
    pub(super) blind_depths: BTreeMap<String, cadmpeg_ir::scalar::NonZeroLength>,
    pub(super) counterbores: BTreeMap<String, CounterboreDimensions>,
}

pub(super) fn hole_operations_are_unique(operations: &[String]) -> bool {
    operations
        .iter()
        .enumerate()
        .all(|(index, operation)| !operations[..index].contains(operation))
}

pub(super) fn insert_hole_output_body(
    outputs: &mut BTreeMap<String, Vec<BodyId>>,
    operation: &str,
    body: &BodyId,
) {
    outputs.insert(operation.to_owned(), vec![body.clone()]);
}

pub(super) fn hole_body_projection(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Option<HoleBodyProjection> {
    if operations.is_empty() || !hole_operations_are_unique(operations) {
        return None;
    }
    let operations_by_body = hole_operations_by_body(ir, operations, outputs)?;

    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let body_faces = connected_solid_body_faces(ir, &body)?;
        let bores = cylindrical_face_witnesses(ir, &body_faces)?;
        let radius = bores.first().map(|bore| bore.radius)?;
        if bores.len() != operations.len()
            || bores
                .iter()
                .any(|bore| bore.radius.to_bits() != radius.to_bits())
        {
            return None;
        }
        for operation in operations {
            insert_hole_output_body(&mut projected_outputs, &operation, &body);
            let diameter = Length::new(radius * 2.0)?;
            diameters.insert(operation, diameter);
        }
    }
    Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths: BTreeMap::new(),
        counterbores: BTreeMap::new(),
    })
}

pub(super) fn counterbore_body_projection(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<Option<HoleBodyProjection>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(operations) {
        return Ok(None);
    }
    let Some(operations_by_body) = hole_operations_by_body(ir, operations, outputs) else {
        return Ok(None);
    };
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut counterbores = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            // A counterbore pair has no serialized operation-to-pair relation
            // once multiple operations share one result body. Do not assign
            // geometry to history order.
            return Ok(None);
        };
        let Some(body_faces) = connected_solid_body_faces(ir, &body) else {
            return Ok(None);
        };
        let Some(witnesses) = counterbore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(None);
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(None);
        };
        insert_hole_output_body(&mut projected_outputs, operation, &body);
        let Some(diameter) = Length::new(witness.bore_radius * 2.0) else {
            return Ok(None);
        };
        diameters.insert(operation.clone(), diameter);
        let (Some(diameter), Some(depth)) = (
            cadmpeg_ir::scalar::PositiveLength::new(witness.counterbore_radius * 2.0),
            cadmpeg_ir::scalar::PositiveLength::new(witness.depth),
        ) else {
            return Ok(None);
        };
        counterbores.insert(operation.clone(), CounterboreDimensions { diameter, depth });
    }
    Ok(Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths: BTreeMap::new(),
        counterbores,
    }))
}

pub(super) fn blind_hole_body_projection(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Option<HoleBodyProjection> {
    if operations.is_empty() || !hole_operations_are_unique(operations) {
        return None;
    }
    let operations_by_body = hole_operations_by_body(ir, operations, outputs)?;
    let mut projected_outputs = BTreeMap::new();
    let mut diameters = BTreeMap::new();
    let mut blind_depths = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return None;
        };
        let body_faces = connected_solid_body_faces(ir, &body)?;
        let witnesses = blind_bore_cylinders(ir, &body_faces)?;
        let [witness] = witnesses.as_slice() else {
            return None;
        };
        insert_hole_output_body(&mut projected_outputs, operation, &body);
        let diameter = Length::new(witness.bore_radius * 2.0)?;
        diameters.insert(operation.clone(), diameter);
        let depth = cadmpeg_ir::scalar::NonZeroLength::new(witness.depth)?;
        blind_depths.insert(operation.clone(), depth);
    }
    Some(HoleBodyProjection {
        outputs: projected_outputs,
        diameters,
        blind_depths,
        counterbores: BTreeMap::new(),
    })
}

/// Derive one complete unoriented placement when one operation owns exactly
/// one through bore. The closest point to the model origin is invariant under
/// axial shifts of the serialized cylinder origin. Canonical axis sign makes
/// serialization deterministic but carries no drilling-direction semantics.
pub(super) fn hole_axis_placements_for_operations(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, HolePlacement> {
    if operations.is_empty() || !hole_operations_are_unique(operations) {
        return BTreeMap::new();
    }
    let Some(operations_by_body) = hole_operations_by_body(ir, operations, outputs) else {
        return BTreeMap::new();
    };

    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            continue;
        };
        let mut body_placements = hole_axis_placements_for_body(ir, &body);
        if body_placements.len() != 1 {
            continue;
        }
        placements.insert(operation.clone(), body_placements.remove(0));
    }
    placements
}

pub(super) fn counterbore_axis_placements_for_operations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HolePlacement>, CodecError> {
    if operations.is_empty() || !hole_operations_are_unique(operations) {
        return Ok(BTreeMap::new());
    }
    let Some(operations_by_body) = hole_operations_by_body(ir, operations, outputs) else {
        return Ok(BTreeMap::new());
    };
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let Some(body_faces) = connected_solid_body_faces(ir, &body) else {
            return Ok(BTreeMap::new());
        };
        let Some(witnesses) = counterbore_cylinders(ctx, ir, &body_faces)? else {
            return Ok(BTreeMap::new());
        };
        let [witness] = witnesses.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let (Some(point), Some(direction)) = (
            cadmpeg_ir::features::FinitePoint3::new(witness.line_origin),
            cadmpeg_ir::features::FeatureDirection3::new(witness.axis),
        ) else {
            return Ok(BTreeMap::new());
        };
        placements.insert(
            operation.clone(),
            HolePlacement::Axis {
                origin: point,
                axis: direction,
            },
        );
    }
    Ok(placements)
}

pub(super) fn blind_hole_axis_placements_for_operations(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> BTreeMap<String, HolePlacement> {
    if operations.is_empty() || !hole_operations_are_unique(operations) {
        return BTreeMap::new();
    }
    let Some(operations_by_body) = hole_operations_by_body(ir, operations, outputs) else {
        return BTreeMap::new();
    };
    let mut placements = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let [operation] = operations.as_slice() else {
            return BTreeMap::new();
        };
        let Some(body_faces) = connected_solid_body_faces(ir, &body) else {
            return BTreeMap::new();
        };
        let Some(witnesses) = blind_bore_cylinders(ir, &body_faces) else {
            return BTreeMap::new();
        };
        let [witness] = witnesses.as_slice() else {
            return BTreeMap::new();
        };
        let (Some(point), Some(direction)) = (
            cadmpeg_ir::features::FinitePoint3::new(witness.position),
            cadmpeg_ir::features::FeatureDirection3::new(witness.direction),
        ) else {
            return BTreeMap::new();
        };
        placements.insert(
            operation.clone(),
            HolePlacement::Directed {
                position: point,
                direction,
            },
        );
    }
    placements
}

pub(super) fn hole_axis_placements_for_body(ir: &CadIr, body: &BodyId) -> Vec<HolePlacement> {
    let Some(body_faces) = connected_solid_body_faces(ir, body) else {
        return Vec::new();
    };
    let Some(bores) = cylindrical_face_witnesses(ir, &body_faces) else {
        return Vec::new();
    };
    let angular_tolerance = ir.tolerances.angular.get();
    let mut placements = Vec::new();
    for bore in bores {
        let origin = bore.line_origin;
        let axis = bore.axis;
        let Some(mut axis) = FiniteVector3::new(axis).and_then(FiniteVector3::unit_nonzero) else {
            return Vec::new();
        };
        let Some(leading) = [axis.x, axis.y, axis.z]
            .into_iter()
            .find(|component| component.abs() > angular_tolerance)
        else {
            return Vec::new();
        };
        if leading < 0.0 {
            axis = Vector3::new(-axis.x, -axis.y, -axis.z);
        }
        let axial_offset = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
        let origin = Point3::new(
            origin.x - axial_offset * axis.x,
            origin.y - axial_offset * axis.y,
            origin.z - axial_offset * axis.z,
        );
        let (Some(origin), Some(axis)) = (
            cadmpeg_ir::features::FinitePoint3::new(origin),
            cadmpeg_ir::features::FeatureDirection3::new(axis),
        ) else {
            return Vec::new();
        };

        placements.push(HolePlacement::Axis { origin, axis });
    }
    placements.sort_by_key(hole_placement_key);
    placements
}

pub(super) fn hole_placement_key(placement: &HolePlacement) -> [u64; 6] {
    let HolePlacement::Axis { origin, axis } = placement else {
        return [0; 6];
    };
    [
        origin.x.to_bits(),
        origin.y.to_bits(),
        origin.z.to_bits(),
        axis.x.to_bits(),
        axis.y.to_bits(),
        axis.z.to_bits(),
    ]
}

#[derive(Clone, Debug)]
pub(super) struct CylindricalFaceWitness {
    line_origin: Point3,
    axis: Vector3,
    radius: f64,
    stations: [f64; 2],
    loop_ids: [LoopId; 2],
}

#[derive(Clone, Copy)]
pub(super) struct CounterboreCylinderWitness {
    line_origin: Point3,
    axis: Vector3,
    bore_radius: f64,
    counterbore_radius: f64,
    depth: f64,
}

#[derive(Clone, Copy)]
pub(super) struct BlindBoreCylinderWitness {
    position: Point3,
    direction: Vector3,
    bore_radius: f64,
    depth: f64,
}

pub(super) fn canonical_axis(
    axis: cadmpeg_ir::units::UnitVector3,
    angular_tolerance: f64,
) -> Option<Vector3> {
    let mut axis = *axis.to_unit_length_charted().as_raw();
    let leading = [axis.x, axis.y, axis.z]
        .into_iter()
        .find(|component| component.abs() > angular_tolerance)?;
    if leading < 0.0 {
        axis = Vector3::new(-axis.x, -axis.y, -axis.z);
    }
    Some(axis)
}

pub(super) fn face_two_loops(face: &Face) -> Option<[&LoopId; 2]> {
    match &face.loops {
        FaceLoops::Unspecified { loops } => {
            let [first, second] = loops.as_slice() else {
                return None;
            };
            Some([first, second])
        }
        FaceLoops::Classified { outer, inner } => {
            let [second] = inner.as_slice() else {
                return None;
            };
            Some([outer, second])
        }
    }
}

pub(super) fn face_one_loop(face: &Face) -> Option<&LoopId> {
    match &face.loops {
        FaceLoops::Unspecified { loops } => {
            let [only] = loops.as_slice() else {
                return None;
            };
            Some(only)
        }
        FaceLoops::Classified { outer, inner } if inner.is_empty() => Some(outer),
        FaceLoops::Classified { .. } => None,
    }
}

pub(super) fn circular_loop_geometry(
    ir: &CadIr,
    loop_id: &LoopId,
    linear_tolerance: f64,
    angular_tolerance: f64,
) -> Option<(Point3, Vector3, f64)> {
    if !ir
        .model
        .coedges
        .iter()
        .any(|coedge| &coedge.owner_loop == loop_id)
    {
        return None;
    }
    let mut witness: Option<(Point3, Vector3, f64)> = None;
    for coedge in ir
        .model
        .coedges
        .iter()
        .filter(|coedge| &coedge.owner_loop == loop_id)
    {
        let curve_id = ir
            .model
            .edges
            .iter()
            .rev()
            .find(|edge| edge.id == coedge.edge)
            .and_then(|edge| edge.curve())?;

        let curve = ir
            .model
            .curves
            .iter()
            .rev()
            .find(|curve| &curve.id == curve_id)?;
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = &curve.geometry
        else {
            return None;
        };
        let center = circle_curve.center().get();
        let axis = circle_curve.frame().axis();
        let radius = circle_curve.radius().get();
        let axis = canonical_axis(*axis, angular_tolerance)?;
        if let Some((previous_center, previous_axis, previous_radius)) = witness {
            if (radius - previous_radius).abs() > linear_tolerance
                || (1.0 - axis.dot(previous_axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - previous_center.x,
                    center.y - previous_center.y,
                    center.z - previous_center.z,
                )
                .norm()
                    > linear_tolerance
            {
                return None;
            }
        }
        witness = Some((center, axis, radius));
    }
    witness
}

pub(super) fn same_loop_edges(ir: &CadIr, first: &LoopId, second: &LoopId) -> bool {
    let first_has_edges = ir
        .model
        .coedges
        .iter()
        .any(|coedge| &coedge.owner_loop == first);
    let second_has_edges = ir
        .model
        .coedges
        .iter()
        .any(|coedge| &coedge.owner_loop == second);
    if first_has_edges != second_has_edges {
        return false;
    }
    let first_in_second = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| &coedge.owner_loop == first)
        .all(|coedge| {
            ir.model
                .coedges
                .iter()
                .any(|other| &other.owner_loop == second && other.edge == coedge.edge)
        });
    let second_in_first = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| &coedge.owner_loop == second)
        .all(|coedge| {
            ir.model
                .coedges
                .iter()
                .any(|other| &other.owner_loop == first && other.edge == coedge.edge)
        });
    first_in_second && second_in_first
}

pub(super) fn cylindrical_face_witnesses(
    ir: &CadIr,
    body_faces: &[&Face],
) -> Option<Vec<CylindricalFaceWitness>> {
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut witnesses = Vec::new();

    for face in body_faces
        .iter()
        .copied()
        .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
    {
        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .rev()
            .find(|surface| surface.id == face.surface)
        else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
            &surface.geometry
        else {
            continue;
        };
        let origin = cylinder_surface.origin().get();
        let axis = cylinder_surface.frame().axis();
        let radius = cylinder_surface.radius().get();
        let axis = canonical_axis(*axis, angular_tolerance)?;
        let axial_offset = Vector3::new(origin.x, origin.y, origin.z).dot(axis);
        let line_origin = Point3::new(
            origin.x - axial_offset * axis.x,
            origin.y - axial_offset * axis.y,
            origin.z - axial_offset * axis.z,
        );
        let [first_loop, second_loop] = face_two_loops(face)?;
        let mut stations = [0.0; 2];
        for (ordinal, loop_id) in [first_loop, second_loop].into_iter().enumerate() {
            let (center, circle_axis, circle_radius) =
                circular_loop_geometry(ir, loop_id, linear_tolerance, angular_tolerance)?;
            if (circle_radius - radius).abs() > linear_tolerance
                || (1.0 - axis.dot(circle_axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                )
                .cross(axis)
                .norm()
                    > linear_tolerance
            {
                return None;
            }
            let station = Vector3::new(center.x, center.y, center.z).dot(axis);
            if !station.is_finite() {
                return None;
            }
            stations[ordinal] = station;
        }
        let [first, second] = stations.as_slice() else {
            return None;
        };
        if (first - second).abs() <= linear_tolerance {
            return None;
        }

        witnesses.push(CylindricalFaceWitness {
            line_origin,
            axis,
            radius,
            stations: [*first, *second],
            loop_ids: [first_loop.clone(), second_loop.clone()],
        });
    }
    Some(witnesses)
}

pub(super) fn plane_annulus_witness(
    ir: &CadIr,
    body_faces: &[&Face],
    small: &CylindricalFaceWitness,
    small_station_ordinal: usize,
    large: &CylindricalFaceWitness,
    large_station_ordinal: usize,
) -> bool {
    let line_origin = small.line_origin;
    let axis = small.axis;
    let station = small.stations[small_station_ordinal];
    let inner_radius = small.radius;
    let outer_radius = large.radius;
    let inner_loop = &small.loop_ids[small_station_ordinal];
    let outer_loop = &large.loop_ids[large_station_ordinal];
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut matches = 0;

    for face in body_faces {
        if face.loops.len() != 2 {
            continue;
        }

        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .rev()
            .find(|surface| surface.id == face.surface)
        else {
            continue;
        };
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
            &surface.geometry
        else {
            continue;
        };
        let origin = plane_surface.origin().get();
        let normal = plane_surface.frame().axis();
        let Some(normal) = canonical_axis(*normal, angular_tolerance) else {
            continue;
        };
        if (1.0 - normal.dot(axis).abs()) > angular_tolerance
            || (Vector3::new(
                origin.x - line_origin.x,
                origin.y - line_origin.y,
                origin.z - line_origin.z,
            )
            .dot(axis)
                - station)
                .abs()
                > linear_tolerance
        {
            continue;
        }
        let Some([first_loop, second_loop]) = face_two_loops(face) else {
            continue;
        };
        let mut boundaries = [(0.0, first_loop), (0.0, second_loop)];
        let mut valid = true;
        for (ordinal, loop_id) in [first_loop, second_loop].into_iter().enumerate() {
            let Some((center, circle_axis, radius)) =
                circular_loop_geometry(ir, loop_id, linear_tolerance, angular_tolerance)
            else {
                valid = false;
                break;
            };
            if (1.0 - circle_axis.dot(normal).abs()) > angular_tolerance
                || (Vector3::new(
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                )
                .dot(normal))
                .abs()
                    > linear_tolerance
                || Vector3::new(
                    center.x - line_origin.x,
                    center.y - line_origin.y,
                    center.z - line_origin.z,
                )
                .cross(axis)
                .norm()
                    > linear_tolerance
                || (Vector3::new(center.x, center.y, center.z).dot(axis) - station).abs()
                    > linear_tolerance
            {
                valid = false;
                break;
            }
            boundaries[ordinal] = (radius, loop_id);
        }
        if !valid {
            continue;
        }
        boundaries.sort_by(|(first, _), (second, _)| first.total_cmp(second));
        let [(inner, inner_boundary), (outer, outer_boundary)] = boundaries.as_slice() else {
            continue;
        };
        if (inner - inner_radius).abs() <= linear_tolerance
            && (outer - outer_radius).abs() <= linear_tolerance
            && same_loop_edges(ir, inner_boundary, inner_loop)
            && same_loop_edges(ir, outer_boundary, outer_loop)
        {
            matches += 1;
        }
    }
    matches == 1
}

pub(super) fn counterbore_cylinders(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    body_faces: &[&Face],
) -> Result<Option<Vec<CounterboreCylinderWitness>>, CodecError> {
    let Some(cylinders) = cylindrical_face_witnesses(ir, body_faces) else {
        return Ok(None);
    };
    if cylinders.is_empty() || cylinders.len() % 2 != 0 {
        return Ok(None);
    }
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut candidates = ctx.alloc_filled(
        cylinders.len(),
        Vec::<(usize, CounterboreCylinderWitness)>::new(),
        "nx counterbore cylinder candidates",
    )?;
    for (first_index, first) in cylinders.iter().enumerate() {
        for (second_index, second) in cylinders.iter().enumerate().skip(first_index + 1) {
            let (small, large) = if first.radius < second.radius {
                (first, second)
            } else {
                (second, first)
            };
            if large.radius - small.radius <= linear_tolerance
                || (1.0 - small.axis.dot(large.axis).abs()) > angular_tolerance
                || Vector3::new(
                    large.line_origin.x - small.line_origin.x,
                    large.line_origin.y - small.line_origin.y,
                    large.line_origin.z - small.line_origin.z,
                )
                .cross(small.axis)
                .norm()
                    > linear_tolerance
            {
                continue;
            }
            let mut common = None;
            let mut multiple_common = false;
            for (small_ordinal, small_station) in small.stations.iter().enumerate() {
                for (large_ordinal, large_station) in large.stations.iter().enumerate() {
                    if (small_station - large_station).abs() <= linear_tolerance
                        && common
                            .replace((small_ordinal, large_ordinal, *small_station))
                            .is_some()
                    {
                        multiple_common = true;
                    }
                }
            }
            let Some((small_shared, large_shared, shared_station)) = common else {
                continue;
            };
            if multiple_common {
                continue;
            }
            let small_other = small.stations[1 - small_shared];
            let large_other = large.stations[1 - large_shared];
            let depth = (large_other - shared_station).abs();
            if depth <= linear_tolerance
                || (small_other - shared_station).abs() <= linear_tolerance
                || !plane_annulus_witness(ir, body_faces, small, small_shared, large, large_shared)
            {
                continue;
            }
            let witness = CounterboreCylinderWitness {
                line_origin: small.line_origin,
                axis: small.axis,
                bore_radius: small.radius,
                counterbore_radius: large.radius,
                depth,
            };

            candidates[first_index].push((second_index, witness));
            candidates[second_index].push((first_index, witness));
        }
    }
    if candidates.iter().any(|candidates| candidates.len() != 1) {
        return Ok(None);
    }
    let mut witnesses = Vec::new();
    let mut used = ctx.alloc_filled(
        cylinders.len(),
        false,
        "nx counterbore cylinder assignments",
    )?;
    for first_index in 0..cylinders.len() {
        if used[first_index] {
            continue;
        }
        let (second_index, witness) = candidates[first_index][0];
        if used[second_index]
            || candidates[second_index][0].0 != first_index
            || first_index == second_index
        {
            return Ok(None);
        }
        used[first_index] = true;
        used[second_index] = true;
        witnesses.push(witness);
    }
    Ok(Some(witnesses))
}

/// Identify one blind bore from its unique planar termination. The cylinder
/// boundary and cap loop must share the exact edge identities; a radius or
/// station match alone is not a topology relation.
pub(super) fn blind_bore_cylinders(
    ir: &CadIr,
    body_faces: &[&Face],
) -> Option<Vec<BlindBoreCylinderWitness>> {
    let cylinders = cylindrical_face_witnesses(ir, body_faces)?;
    let [cylinder] = cylinders.as_slice() else {
        return None;
    };
    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut cap_station = None;
    let mut cap_count = 0usize;
    for (station_ordinal, station) in cylinder.stations.iter().enumerate() {
        let cylinder_loop = &cylinder.loop_ids[station_ordinal];

        if !ir
            .model
            .coedges
            .iter()
            .any(|coedge| &coedge.owner_loop == cylinder_loop)
        {
            return None;
        }
        for face in body_faces {
            let Some(cap_loop) = face_one_loop(face) else {
                continue;
            };
            if !same_loop_edges(ir, cap_loop, cylinder_loop) {
                continue;
            }

            let Some(surface) = ir
                .model
                .surfaces
                .iter()
                .rev()
                .find(|surface| surface.id == face.surface)
            else {
                continue;
            };
            let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
                &surface.geometry
            else {
                continue;
            };
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis();
            let Some(normal) = canonical_axis(*normal, angular_tolerance) else {
                continue;
            };
            let Some((center, circle_axis, circle_radius)) =
                circular_loop_geometry(ir, cap_loop, linear_tolerance, angular_tolerance)
            else {
                continue;
            };
            if (circle_radius - cylinder.radius).abs() > linear_tolerance
                || (1.0 - circle_axis.dot(normal).abs()) > angular_tolerance
                || (1.0 - normal.dot(cylinder.axis).abs()) > angular_tolerance
                || Vector3::new(
                    center.x - cylinder.line_origin.x,
                    center.y - cylinder.line_origin.y,
                    center.z - cylinder.line_origin.z,
                )
                .cross(cylinder.axis)
                .norm()
                    > linear_tolerance
                || (Vector3::new(center.x, center.y, center.z).dot(cylinder.axis) - *station).abs()
                    > linear_tolerance
                || (Vector3::new(origin.x, origin.y, origin.z).dot(cylinder.axis) - *station).abs()
                    > linear_tolerance
            {
                continue;
            }
            cap_count += 1;
            cap_station = Some((station_ordinal, *station));
        }
    }
    let (cap_ordinal, cap_station) = cap_station.filter(|_| cap_count == 1)?;
    let entry_ordinal = 1 - cap_ordinal;
    let entry_station = cylinder.stations[entry_ordinal];
    let depth = (cap_station - entry_station).abs();
    if !depth.is_finite() || depth <= linear_tolerance {
        return None;
    }
    let position = Point3::new(
        cylinder.line_origin.x + entry_station * cylinder.axis.x,
        cylinder.line_origin.y + entry_station * cylinder.axis.y,
        cylinder.line_origin.z + entry_station * cylinder.axis.z,
    );
    let direction = if cap_station > entry_station {
        cylinder.axis
    } else {
        Vector3::new(-cylinder.axis.x, -cylinder.axis.y, -cylinder.axis.z)
    };
    Some(vec![BlindBoreCylinderWitness {
        position,
        direction,
        bore_radius: cylinder.radius,
        depth,
    }])
}

/// Resolve hole operations to their explicit output bodies, or to the one
/// connected solid when NX omits every operation-output relation. An output
/// entry with no body is an explicit unresolved relation and blocks fallback.
pub(super) fn hole_operations_by_body(
    ir: &CadIr,
    operations: &[String],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Option<BTreeMap<BodyId, Vec<String>>> {
    let related = operations
        .iter()
        .filter(|operation| outputs.contains_key(*operation))
        .count();
    if related != 0 && related != operations.len() {
        return None;
    }
    if related == operations.len() {
        let mut operations_by_body = BTreeMap::<BodyId, Vec<String>>::new();
        for operation in operations {
            let Some([body]) = outputs.get(operation).map(Vec::as_slice) else {
                return None;
            };
            if let Some(group) = operations_by_body.get_mut(body) {
                group.push(operation.clone());
            } else {
                let body_key = body.clone();
                operations_by_body.insert(body_key, vec![operation.clone()]);
            }
        }
        return Some(operations_by_body);
    }

    let mut selected = None;
    for body in &ir.model.bodies {
        if connected_solid_body_exists(ir, body) && selected.replace(body).is_some() {
            return None;
        }
    }
    let body = selected?;

    Some(BTreeMap::from([(body.id.clone(), operations.to_vec())]))
}

/// Derive identical entry and exit chamfer treatments only when every simple
/// through-hole bore has exactly two coaxial conical faces and every cone is
/// bounded by the bore circle and one equal larger circle.
pub(super) fn simple_hole_chamfers(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    templates: &[crate::native::features::holes::FeatureSimpleHoleTemplate],
    outputs: &BTreeMap<String, Vec<BodyId>>,
) -> Result<BTreeMap<String, HoleKind>, CodecError> {
    let mut operations = Vec::new();
    for template in templates {
        if !(template.form == crate::native::features::holes::SimpleHoleForm::Simple
            && template.extent == crate::native::features::holes::SimpleHoleExtent::Through
            && template.start_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer
            && template.end_treatment
                == crate::native::features::holes::SimpleHoleEndTreatment::Chamfer)
        {
            continue;
        }

        if templates
            .iter()
            .filter(|candidate| candidate.operation_label == template.operation_label)
            .count()
            != 1
        {
            continue;
        }
        operations.push(template.operation_label.clone());
    }
    if operations.is_empty() {
        return Ok(BTreeMap::new());
    }
    operations.sort_by(Ord::cmp);
    let Some(operations_by_body) = hole_operations_by_body(ir, &operations, outputs) else {
        return Ok(BTreeMap::new());
    };

    let linear_tolerance = ir.tolerances.linear.get();
    let angular_tolerance = ir.tolerances.angular.get();
    let mut treatments = BTreeMap::new();
    for (body, operations) in operations_by_body {
        let Some(body_faces) = connected_solid_body_faces(ir, &body) else {
            return Ok(BTreeMap::new());
        };
        let Some(bores) = cylindrical_face_witnesses(ir, &body_faces) else {
            return Ok(BTreeMap::new());
        };
        let [first_bore, ..] = bores.as_slice() else {
            return Ok(BTreeMap::new());
        };
        let bore_radius = first_bore.radius;
        if bores.len() != operations.len()
            || bores
                .iter()
                .any(|bore| bore.radius.to_bits() != bore_radius.to_bits())
        {
            return Ok(BTreeMap::new());
        }
        let mut cone_counts =
            ctx.alloc_filled(bores.len(), 0usize, "nx simple-hole chamfer cone counts")?;
        let mut outer_radii = Vec::new();
        let mut included_angles = Vec::new();
        for face in body_faces
            .iter()
            .filter(|face| face.sense == Sense::Reversed && face.loops.len() == 2)
        {
            let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))) = ir
                .model
                .surfaces
                .iter()
                .rev()
                .find(|surface| surface.id == face.surface)
                .map(|surface| &surface.geometry)
            else {
                continue;
            };
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let half_angle = cone_surface.half_angle().get();
            if half_angle <= 0.0 || half_angle >= std::f64::consts::FRAC_PI_2 {
                return Ok(BTreeMap::new());
            }

            let mut matching_bore = None;
            let mut multiple_bores = false;
            for (ordinal, bore) in bores.iter().enumerate() {
                let bore_origin = bore.line_origin;
                let bore_axis = bore.axis;
                let dot = axis.dot(bore_axis);
                if (1.0 - dot.abs()) > angular_tolerance {
                    continue;
                }
                let delta = Vector3::new(
                    origin.x - bore_origin.x,
                    origin.y - bore_origin.y,
                    origin.z - bore_origin.z,
                );
                if delta.cross(bore_axis).norm() <= linear_tolerance
                    && matching_bore.replace(ordinal).is_some()
                {
                    multiple_bores = true;
                }
            }
            let Some(bore_ordinal) = matching_bore else {
                return Ok(BTreeMap::new());
            };
            if multiple_bores {
                return Ok(BTreeMap::new());
            }
            cone_counts[bore_ordinal] += 1;

            let mut radii = [None, None];
            let Some(loops) = face_two_loops(face) else {
                return Ok(BTreeMap::new());
            };
            let mut radius_count = 0;
            for loop_id in loops {
                for coedge in ir
                    .model
                    .coedges
                    .iter()
                    .filter(|coedge| coedge.owner_loop == *loop_id)
                {
                    let Some(curve_id) = ir
                        .model
                        .edges
                        .iter()
                        .rev()
                        .find(|edge| edge.id == coedge.edge)
                        .and_then(|edge| edge.curve())
                    else {
                        continue;
                    };

                    let Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))) = ir
                        .model
                        .curves
                        .iter()
                        .rev()
                        .find(|curve| curve.id == *curve_id)
                        .map(|curve| &curve.geometry)
                    else {
                        continue;
                    };
                    let radius = circle_curve.radius().get();
                    if radius_count == radii.len() {
                        return Ok(BTreeMap::new());
                    }
                    radii[radius_count] = Some(radius);
                    radius_count += 1;
                }
            }
            let [Some(mut inner), Some(mut outer)] = radii else {
                return Ok(BTreeMap::new());
            };
            if inner.total_cmp(&outer).is_gt() {
                std::mem::swap(&mut inner, &mut outer);
            }
            if inner.to_bits() != bore_radius.to_bits() || outer <= inner {
                return Ok(BTreeMap::new());
            }

            outer_radii.push(outer);
            included_angles.push(half_angle * 2.0);
        }
        if cone_counts.iter().any(|count| *count != 2)
            || outer_radii.len() != bores.len() * 2
            || included_angles.len() != outer_radii.len()
        {
            return Ok(BTreeMap::new());
        }
        outer_radii.sort_by(f64::total_cmp);
        included_angles.sort_by(f64::total_cmp);
        let (Some(&widest), Some(&narrowest), Some(&largest), Some(&smallest)) = (
            outer_radii.last(),
            outer_radii.first(),
            included_angles.last(),
            included_angles.first(),
        ) else {
            return Ok(BTreeMap::new());
        };
        if widest - narrowest > linear_tolerance || largest - smallest > angular_tolerance {
            return Ok(BTreeMap::new());
        }
        let (Some(radius_count), Some(angle_count)) = (
            cadmpeg_core::convert::f64_from_index(outer_radii.len()),
            cadmpeg_core::convert::f64_from_index(included_angles.len()),
        ) else {
            return Ok(BTreeMap::new());
        };
        let (Some(diameter), Some(angle)) = (
            cadmpeg_ir::scalar::PositiveLength::new(
                2.0 * outer_radii.iter().sum::<f64>() / radius_count,
            ),
            cadmpeg_ir::scalar::InteriorAngle::new(
                included_angles.iter().sum::<f64>() / angle_count,
            ),
        ) else {
            return Ok(BTreeMap::new());
        };
        let treatment = HoleKind::Chamfer { diameter, angle };
        for operation in operations {
            treatments.insert(operation, treatment);
        }
    }
    Ok(treatments)
}

pub(super) fn unique_simple_hole_template(
    payload_strings: &[&str],
) -> Option<(
    crate::native::features::holes::SimpleHoleForm,
    crate::native::features::holes::SimpleHoleExtent,
    crate::native::features::holes::SimpleHoleEndTreatment,
    crate::native::features::holes::SimpleHoleEndTreatment,
)> {
    let mut candidates = payload_strings
        .iter()
        .copied()
        .filter(|value| value.starts_with("Hole_"));
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    crate::native::features::holes::parse_simple_hole_template(candidate)
}
