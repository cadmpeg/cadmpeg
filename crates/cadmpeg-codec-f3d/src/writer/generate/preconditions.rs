// SPDX-License-Identifier: Apache-2.0
//! Source-less pre-write validators for the neutral `CadIr` and its F3D native
//! extension.

use cadmpeg_core::decode::index_from_u32;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::native::F3dNative;
use crate::records::{
    entity_header::SegmentType,
    sketch_geometry::SketchCurveGeometry,
    sketch_links::{PersistentDesignLink, PersistentSubentityTag},
};
use cadmpeg_asm::brep::records::{EvaluatedToleranceSlot, TolerantEdgeTail, TolerantVertexTail};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};

use super::reduce::{DroppedRecords, Unwritable};
pub(super) fn validate_source_less_procedural_carriers(target: &CadIr) -> Result<(), CodecError> {
    let mut surface_owners = BTreeSet::new();
    for procedural in &target.geometry.procedural_surfaces {
        let mut owners =
            target.geometry.surfaces.iter().filter(|surface| {
                surface.geometry.procedural_construction() == Some(&procedural.id)
            });
        let (Some(surface), None) = (owners.next(), owners.next()) else {
            return Err(CodecError::InvalidInput(format!(
                "procedural surface {} has no unique carrier",
                procedural.id
            )));
        };
        let owner = &surface.id;
        if !surface_owners.insert(owner) {
            return Err(CodecError::InvalidInput(format!(
                "surface {owner} has multiple procedural constructions"
            )));
        }
        if surface.geometry.solved_cache().is_some_and(|geometry| {
            !matches!(
                geometry,
                SolvedSurfaceGeometry::Nurbs(_) | SolvedSurfaceGeometry::Unknown { .. }
            )
        }) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} cannot retain its construction on analytic carrier {}",
                procedural.id, surface.id
            )));
        }
    }

    let mut curve_owners = BTreeSet::new();
    for procedural in &target.geometry.procedural_curves {
        let mut owners = target
            .geometry
            .curves
            .iter()
            .filter(|curve| curve.geometry.procedural_construction() == Some(&procedural.id));
        let (Some(curve), None) = (owners.next(), owners.next()) else {
            return Err(CodecError::InvalidInput(format!(
                "procedural curve {} has no unique carrier",
                procedural.id
            )));
        };
        let owner = &curve.id;
        if !curve_owners.insert(owner) {
            return Err(CodecError::InvalidInput(format!(
                "curve {owner} has multiple procedural constructions"
            )));
        }
        match curve.geometry.solved_cache() {
            Some(SolvedCurveGeometry::Nurbs(_)) => {}
            None if procedural.cache_fit_tolerance().is_none() => {}
            None => {
                return Err(CodecError::InvalidInput(format!(
                    "cacheless procedural curve {} carries a cache-fit tolerance",
                    procedural.id
                )));
            }
            Some(_) => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D procedural curve {} cannot retain its construction on carrier {}",
                    procedural.id, curve.id
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_source_less_topology_tolerances(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    if let Some(face) = target
        .geometry
        .faces
        .iter()
        .find(|face| face.tolerance.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize face {} tolerance losslessly",
            face.id
        )));
    }
    let tolerant = native
        .tolerant_coedge_parameters
        .iter()
        .filter(|parameters| {
            matches!(
                parameters.extension,
                cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                    target: None,
                    ..
                }
            )
        })
        .map(|parameters| parameters.coedge.as_str())
        .collect::<std::collections::HashSet<_>>();
    if let Some(coedge) = target
        .geometry
        .coedges
        .iter()
        .find(|coedge| coedge.use_curve.is_some() && !tolerant.contains(coedge.id.as_str()))
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D coedge {} use curve lacks a cache-local tolerant extension",
            coedge.id
        )));
    }
    Ok(())
}

pub(super) fn validate_source_less_auxiliary_geometry(target: &CadIr) -> Result<(), CodecError> {
    if let Some(tessellation) = target.model.tessellations.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize neutral tessellation {} losslessly",
            tessellation.id
        )));
    }
    if let Some(surface) = target
        .geometry
        .surfaces
        .iter()
        .find(|surface| surface.source_object.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize source-object association on surface {} losslessly",
            surface.id
        )));
    }
    if let Some(curve) = target
        .geometry
        .curves
        .iter()
        .find(|curve| curve.source_object.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize source-object association on curve {} losslessly",
            curve.id
        )));
    }
    Ok(())
}

pub(super) fn validate_source_less_recipes(native: &F3dNative) -> Result<(), CodecError> {
    let mut group_counts = HashMap::new();
    for recipe in &native.construction_recipes {
        let expected = group_counts
            .entry((
                recipe.kind,
                recipe
                    .design
                    .as_ref()
                    .map(|design| design.id.value.as_str()),
            ))
            .or_insert(0_u64);
        if u64::from(recipe.recipe_index) != *expected {
            return Err(CodecError::InvalidInput(format!(
                "F3D construction recipe {} has noncontiguous group index {}",
                recipe.id, recipe.recipe_index
            )));
        }
        *expected += 1;
    }
    Ok(())
}

fn source_less_design_record_type<'a>(
    native: &'a F3dNative,
    class_tag: &crate::records::references::DesignClassTag,
    record_index: u32,
    record_kind: &str,
) -> Result<&'a SegmentType, CodecError> {
    let type_ordinal = class_tag.dynamic_ordinal().ok_or_else(|| {
        CodecError::InvalidInput(format!(
            "F3D {record_kind} class tag {} is below the dynamic type range",
            class_tag.as_str()
        ))
    })?;
    let design_type = native.design_types.get(type_ordinal).ok_or_else(|| {
        CodecError::InvalidInput(format!(
            "F3D {record_kind} class tag {} is outside the Design type table",
            class_tag.as_str()
        ))
    })?;
    if !design_type
        .entities
        .values()
        .any(|registered| *registered == u64::from(record_index))
    {
        return Err(CodecError::InvalidInput(format!(
            "F3D {record_kind} {record_index} is not registered by class tag {}",
            class_tag.as_str()
        )));
    }
    Ok(design_type)
}

fn design_type_matches(design_type: &SegmentType, expected: (&str, u32, &str)) -> bool {
    design_type
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(expected.0)
        && design_type.version == expected.1
        && design_type.module == expected.2
}

/// The record indices that a sketch-module entity header names in its
/// reference list and its base-record slot.
fn sketch_header_indices(native: &F3dNative) -> BTreeSet<u32> {
    native
        .design_entity_headers
        .iter()
        .filter(|header| header.in_sketch_module())
        .flat_map(|header| {
            header.reference_values().copied().chain(
                header
                    .sketch_references()
                    .and_then(|list| list.record_reference),
            )
        })
        .collect()
}

/// The Design record headers that no sketch header or sketch relation
/// names. The source-less Design stream writes a header as the stub of a
/// record of the sketch graph, so no written record names these.
pub(super) fn unreachable_record_headers(native: &F3dNative) -> Option<Unwritable> {
    let mut reachable = sketch_header_indices(native);
    for relation in &native.sketch_relations {
        reachable.extend(relation.all_member_indices());
    }
    Unwritable::of(
        "of Design record headers that no sketch header or sketch relation names (the source-less Design stream writes the headers of the sketch graph)",
        native
            .design_record_headers
            .iter()
            .filter(|header| !reachable.contains(&header.record_index))
            .map(|header| header.id.clone())
            .collect(),
    )
}

/// The record indices of the typed sketch records.
pub(super) fn typed_sketch_indices(native: &F3dNative) -> BTreeSet<u32> {
    native
        .sketch_points
        .iter()
        .map(|record| record.record_index)
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|record| record.record_index),
        )
        .chain(
            native
                .sketch_relations
                .iter()
                .map(|record| record.record_index),
        )
        .chain(native.sketch_texts.iter().map(|record| record.record_index))
        .collect()
}

/// The Design record headers that use the record index of a typed sketch
/// record. The typed record is written at that index.
pub(super) fn shadowing_record_headers(native: &F3dNative) -> Option<Unwritable> {
    let typed = typed_sketch_indices(native);
    Unwritable::of(
        "of Design record headers that use the record index of a typed sketch record (the Design stream writes the typed record at that index)",
        native
            .design_record_headers
            .iter()
            .filter(|header| typed.contains(&header.record_index))
            .map(|header| header.id.clone())
            .collect(),
    )
}

/// The sketch records whose sketch owner the document does not hold: the
/// written record names its owner, and no written sketch header holds it.
pub(super) fn unowned_sketch_records(native: &F3dNative) -> Option<Unwritable> {
    let owners = native
        .design_entity_headers
        .iter()
        .filter(|header| header.in_sketch_module())
        .map(|header| header.entity_id.suffix())
        .collect::<BTreeSet<_>>();
    let owned = |owner: Option<u32>| owner.is_some_and(|owner| owners.contains(&u64::from(owner)));
    let records = native
        .sketch_points
        .iter()
        .filter(|point| !owned(point.owner_reference))
        .map(|point| point.id.clone())
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .filter(|curve| !owned(curve.owner_reference))
                .map(|curve| curve.id.clone()),
        )
        .chain(
            native
                .sketch_relations
                .iter()
                .filter(|relation| !owned(Some(relation.owner_reference)))
                .map(|relation| relation.id.clone()),
        )
        .chain(
            native
                .sketch_texts
                .iter()
                .filter(|text| !owned(Some(text.owner_reference)))
                .map(|text| text.id.clone()),
        )
        .collect();
    Unwritable::of(
        "of sketch records whose sketch owner the document does not hold (the written record names its owner, and no sketch header holds it)",
        records,
    )
}

/// The sketch records that name the record index of a dropped sketch record
/// or record header: the written record would name a record the Design
/// stream does not hold. Each drop adds the indices of its records to
/// `dropped`, so the attempts drop every record that a chain of references
/// ties to a dropped one (W1).
pub(super) fn sketch_records_naming(
    native: &F3dNative,
    dropped: &BTreeSet<u32>,
) -> Option<Unwritable> {
    use crate::records::sketch_geometry::SketchTextLayout;

    if dropped.is_empty() {
        return None;
    }
    let names = |mut indices: Box<dyn Iterator<Item = u32> + '_>| {
        indices.any(|index| dropped.contains(&index))
    };
    let records = native
        .sketch_points
        .iter()
        .filter(|point| names(Box::new(point.companion().incident_curves.iter().copied())))
        .map(|point| point.id.clone())
        .chain(
            native
                .sketch_relations
                .iter()
                .filter(|relation| {
                    names(Box::new(
                        relation
                            .all_member_indices()
                            .chain(relation.auxiliary_references().values().copied()),
                    ))
                })
                .map(|relation| relation.id.clone()),
        )
        .chain(
            native
                .sketch_texts
                .iter()
                .filter(|text| match text.layout {
                    SketchTextLayout::TextexTag {
                        first_reference,
                        second_reference,
                        ..
                    } => names(Box::new(
                        [first_reference, second_reference].into_iter().flatten(),
                    )),
                    SketchTextLayout::TxtTag { .. } => false,
                })
                .map(|text| text.id.clone()),
        )
        .collect();
    Unwritable::of(
        "of sketch records that name a dropped sketch record (the written record would name a record the Design stream does not hold)",
        records,
    )
}

/// The sketch-curve links and body visibilities the B-rep and Design
/// streams cannot hold, the first family that holds one.
pub(super) fn unwritable_link_records(target: &CadIr, native: &F3dNative) -> Option<Unwritable> {
    let coedges = target
        .geometry
        .coedges
        .iter()
        .map(|coedge| &coedge.id)
        .collect::<HashSet<_>>();
    let mut unresolved = BTreeSet::new();
    for link in &native.sketch_curve_links {
        // Only a coedge-owned link is regenerated; a link on any other owner is
        // reported as a source-fidelity loss instead of refusing the write.
        let AttributeTarget::Coedge(coedge) = &link.target else {
            continue;
        };
        if !coedges.contains(coedge) {
            unresolved.insert(link.id.clone());
        }
    }
    if let Some(unwritable) = Unwritable::of(
        "of sketch-curve links that name a coedge the document does not hold",
        unresolved,
    ) {
        return Some(unwritable);
    }
    let bodies = target
        .geometry
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<HashSet<_>>();
    Unwritable::of(
        "of body visibilities whose body the write omits",
        native
            .body_visibilities
            .iter()
            .filter(|visibility| !bodies.contains(&visibility.body))
            .map(|visibility| visibility.id().to_owned())
            .collect(),
    )
}

/// The body visibilities that name the body or the Design entity of an
/// earlier kept visibility. The Design stream holds one visibility for each
/// body and one body for each Design entity.
pub(super) fn shared_body_visibilities(native: &F3dNative) -> Option<Unwritable> {
    let mut bodies = HashSet::new();
    let mut entities = HashSet::new();
    Unwritable::of(
        "of body visibilities that name the body or the Design entity of an earlier body visibility (the Design stream holds one visibility for each body and one body for each Design entity)",
        native
            .body_visibilities
            .iter()
            .filter(|visibility| {
                let kept = !bodies.contains(&visibility.body)
                    && !entities.contains(&visibility.entity_suffix);
                if kept {
                    bodies.insert(&visibility.body);
                    entities.insert(visibility.entity_suffix);
                }
                !kept
            })
            .map(|visibility| visibility.id().to_owned())
            .collect(),
    )
}

/// The `BodiesRoot` members that name the Design entity of a dropped body
/// visibility, where no kept visibility holds that entity: the written
/// member would name an entity the Design stream does not hold.
pub(super) fn body_members_naming(
    native: &F3dNative,
    dropped: &BTreeSet<u64>,
) -> Option<Unwritable> {
    if dropped.is_empty() {
        return None;
    }
    let held = native
        .body_visibilities
        .iter()
        .map(|visibility| visibility.entity_suffix)
        .collect::<BTreeSet<_>>();
    Unwritable::of(
        "of BodiesRoot members that name the Design entity of a dropped body visibility (the written member would name an entity the Design stream does not hold)",
        native
            .design_body_members
            .iter()
            .filter(|member| {
                dropped.contains(&member.entity_suffix) && !held.contains(&member.entity_suffix)
            })
            .map(|member| member.id().clone())
            .collect(),
    )
}

/// The sketch-curve links whose source-curve key names a dropped sketch
/// curve or the record of a dropped sketch text, and no kept sketch curve:
/// the written link would name a record the Design stream does not hold.
pub(super) fn sketch_curve_links_naming(
    native: &F3dNative,
    dropped: &DroppedRecords,
) -> Option<Unwritable> {
    if dropped.curve_identities.is_empty() && dropped.texts.is_empty() {
        return None;
    }
    let kept = native
        .sketch_curve_identities
        .iter()
        .map(|curve| (curve.primary_id.get(), curve.secondary_id))
        .collect::<BTreeSet<_>>();
    Unwritable::of(
        "of sketch-curve links that name a dropped sketch curve or sketch text (the written link would name a record the Design stream does not hold)",
        native
            .sketch_curve_links
            .iter()
            .filter(|link| {
                let key = (link.sketch_curve_id.cast_unsigned(), link.ref_b);
                !kept.contains(&key)
                    && (dropped.curve_identities.contains(&key)
                        || u32::try_from(link.ref_b)
                            .is_ok_and(|index| dropped.texts.contains(&index)))
            })
            .map(|link| link.id.clone())
            .collect(),
    )
}

/// The lengths of the arenas that the stamp of a regenerated design extends.
#[derive(Clone, Copy)]
pub(super) struct StampMark {
    design_links: usize,
    subentity_tags: usize,
    sketch_curve_links: usize,
}

impl StampMark {
    pub(super) fn of(native: &F3dNative) -> Self {
        Self {
            design_links: native.persistent_design_links.len(),
            subentity_tags: native.persistent_subentity_tags.len(),
            sketch_curve_links: native.sketch_curve_links.len(),
        }
    }
}

/// The records before `mark` on an entity that a record after `mark` names.
/// The regenerated Design stream names these entities, and the records
/// before the mark name records of the source Design stream (W1, W2).
pub(super) fn replaced_by_stamp(native: &F3dNative, mark: StampMark) -> Option<Unwritable> {
    fn replaced<T>(
        rows: &[T],
        mark: usize,
        target: impl Fn(&T) -> &AttributeTarget,
        id: impl Fn(&T) -> &str,
        out: &mut BTreeSet<String>,
    ) {
        let (source, stamped) = rows.split_at(mark);
        let named = stamped.iter().map(&target).collect::<BTreeSet<_>>();
        out.extend(
            source
                .iter()
                .filter(|row| named.contains(target(row)))
                .map(|row| id(row).to_owned()),
        );
    }
    let mut records = BTreeSet::new();
    replaced(
        &native.persistent_design_links,
        mark.design_links,
        |link| &link.target,
        |link| &link.id,
        &mut records,
    );
    replaced(
        &native.persistent_subentity_tags,
        mark.subentity_tags,
        |tag| &tag.target,
        |tag| &tag.id,
        &mut records,
    );
    replaced(
        &native.sketch_curve_links,
        mark.sketch_curve_links,
        |link| &link.target,
        |link| &link.id,
        &mut records,
    );
    Unwritable::of(
        "on an entity that the regenerated Design stream names (the B-rep stream holds the regenerated name, and these records name records of the source Design stream)",
        records,
    )
}

/// The records of the groups that hold more than one record, as the ids
/// of the groups whose records differ and the ids of each record after the
/// first in a group of equal records. The writer writes the first record of
/// each entity.
fn shared<K, T: Clone + PartialEq>(
    groups: BTreeMap<K, Vec<&T>>,
    id: impl Fn(&T) -> String,
    renumbered: impl Fn(&T, &T) -> T,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut differing = BTreeSet::new();
    let mut repeated = BTreeSet::new();
    for records in groups.into_values() {
        let Some((first, rest)) = records.split_first() else {
            continue;
        };
        if rest
            .iter()
            .all(|record| renumbered(first, record) == **record)
        {
            repeated.extend(rest.iter().map(|record| id(record)));
        } else if !rest.is_empty() {
            differing.extend(records.iter().map(|record| id(record)));
        }
    }
    (differing, repeated)
}

/// Whether the decoder reads `range`, moved onto the knot domain of `curve`,
/// as the use-curve range `use_range`.
pub(super) fn reads_as_use_curve_range(
    range: cadmpeg_ir::units::FiniteVector<2>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    use_range: cadmpeg_ir::topology::ParameterInterval,
) -> bool {
    cadmpeg_ir::topology::ParameterInterval::from_finite_endpoints(range)
        .map(|range| cadmpeg_asm::brep::emit::clamp_to_knot_domain(range, curve))
        .is_ok_and(|range| range.endpoints() == use_range.endpoints())
}

/// The tolerant-coedge records the B-rep stream cannot hold as they stand,
/// in groups by reason. The coedge record holds one extension, written
/// inline, whose embedded curve is the coedge's NURBS use curve. The decoder
/// reads the use-curve range from the record range moved onto the curve's
/// knot domain. A record whose moved range is the use-curve range is written
/// as stored, and any other record is written with the use-curve range where
/// the decoder reads that range back (W2).
pub(super) fn unwritable_tolerant_coedges(
    target: &CadIr,
    native: &F3dNative,
) -> Option<Unwritable> {
    use cadmpeg_asm::brep::records::{TolerantCoedgeExtension, TolerantCoedgeParameters};

    let coedges = target
        .geometry
        .coedges
        .iter()
        .map(|coedge| (&coedge.id, coedge))
        .collect::<HashMap<_, _>>();
    let curves = target
        .geometry
        .curves
        .iter()
        .map(|curve| (&curve.id, curve))
        .collect::<HashMap<_, _>>();
    let mut missing = BTreeSet::new();
    let mut by_coedge = BTreeMap::<_, Vec<&TolerantCoedgeParameters>>::new();
    let mut unwritable_curve = BTreeSet::new();
    let mut other_range = BTreeSet::new();
    let mut relocated = BTreeSet::new();
    for parameters in &native.tolerant_coedge_parameters {
        let id = parameters.id();
        let Some(coedge) = coedges.get(&parameters.coedge) else {
            missing.insert(id);
            continue;
        };
        by_coedge
            .entry(&parameters.coedge)
            .or_default()
            .push(parameters);
        match &parameters.extension {
            TolerantCoedgeExtension::None {} | TolerantCoedgeExtension::Empty { target: None } => {}
            TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                parameter_range,
                ..
            } => {
                let curve = coedge.use_curve.as_ref().and_then(|use_curve| {
                    match curves.get(&use_curve.curve)?.geometry.solved() {
                        Some(SolvedCurveGeometry::Nurbs(curve)) => Some((use_curve, curve)),
                        _ => None,
                    }
                });
                let Some((use_curve, curve)) = curve else {
                    unwritable_curve.insert(id);
                    continue;
                };
                let use_range = use_curve.parameter_range;
                let stored = parameter_range.unwrap_or(parameters.parameter_range);
                let regenerated = cadmpeg_ir::units::FiniteVector::new(use_range.endpoints());
                if !reads_as_use_curve_range(stored, curve, use_range)
                    && !regenerated
                        .is_some_and(|range| reads_as_use_curve_range(range, curve, use_range))
                {
                    other_range.insert(id);
                }
            }
            TolerantCoedgeExtension::Empty { target: Some(_) }
            | TolerantCoedgeExtension::Reference { .. }
            | TolerantCoedgeExtension::EmbeddedCurve {
                target: Some(_), ..
            } => {
                relocated.insert(id);
            }
        }
    }
    let (differing, repeated) = shared(by_coedge, TolerantCoedgeParameters::id, |first, record| {
        TolerantCoedgeParameters {
            source_namespace: record.source_namespace.clone(),
            record_index: record.record_index,
            ..first.clone()
        }
    });
    [
        (
            "of tolerant-coedge records that name a coedge the document does not hold",
            missing,
        ),
        (
            "of tolerant-coedge records equal to an earlier record of their coedge (the coedge record holds one extension, which the earlier record writes)",
            repeated,
        ),
        (
            "of tolerant-coedge records that share their coedge with another record (the coedge record holds one extension, and the records do not state which one it is)",
            differing,
        ),
        (
            "of tolerant-coedge records whose coedge has no NURBS use curve for the embedded curve",
            unwritable_curve,
        ),
        (
            "of tolerant-coedge records whose parameter range and use-curve range both read back as another range (the decoder moves the range onto the knot domain of the curve)",
            other_range,
        ),
        (
            "of tolerant-coedge records whose extension names another record (the written coedge record holds its extension inline)",
            relocated,
        ),
    ]
    .into_iter()
    .find_map(|(reason, records)| Unwritable::of(reason, records))
}

/// The tolerant vertex and edge tails the B-rep stream cannot hold as they
/// stand, in groups by reason. The written vertex and edge records hold the
/// tolerance of the neutral vertex or edge, and one tail each.
pub(super) fn unwritable_tolerant_tails(target: &CadIr, native: &F3dNative) -> Option<Unwritable> {
    let vertices = target
        .geometry
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, vertex))
        .collect::<HashMap<_, _>>();
    let edges = target
        .geometry
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let mut vertex_absent = BTreeSet::new();
    let mut by_vertex = BTreeMap::<_, Vec<&TolerantVertexTail>>::new();
    let mut slot_disagrees = BTreeSet::new();
    for tail in &native.tolerant_vertex_tails {
        let Some(vertex) = vertices.get(&tail.vertex) else {
            vertex_absent.insert(tail.id());
            continue;
        };
        by_vertex.entry(&tail.vertex).or_default().push(tail);
        if vertex.tolerance.is_some()
            != matches!(
                tail.evaluated_slot,
                EvaluatedToleranceSlot::Evaluated { .. }
            )
        {
            slot_disagrees.insert(tail.id());
        }
    }
    let mut edge_absent = BTreeSet::new();
    let mut by_edge = BTreeMap::<_, Vec<&TolerantEdgeTail>>::new();
    let mut no_tolerance = BTreeSet::new();
    for tail in &native.tolerant_edge_tails {
        let Some(edge) = edges.get(&tail.edge) else {
            edge_absent.insert(tail.id());
            continue;
        };
        by_edge.entry(&tail.edge).or_default().push(tail);
        if edge.tolerance.is_none() {
            no_tolerance.insert(tail.id());
        }
    }
    let (vertex_differing, vertex_repeated) =
        shared(by_vertex, TolerantVertexTail::id, |first, record| {
            TolerantVertexTail {
                source_namespace: record.source_namespace.clone(),
                record_index: record.record_index,
                ..first.clone()
            }
        });
    let (edge_differing, edge_repeated) = shared(by_edge, TolerantEdgeTail::id, |first, record| {
        TolerantEdgeTail {
            source_namespace: record.source_namespace.clone(),
            record_index: record.record_index,
            ..first.clone()
        }
    });
    [
        (
            "of tolerant-vertex records that name a vertex the document does not hold",
            vertex_absent,
        ),
        (
            "of tolerant-vertex records equal to an earlier record of their vertex (the vertex record holds one tolerance tail, which the earlier record writes)",
            vertex_repeated,
        ),
        (
            "of tolerant-vertex records that share their vertex with another record (the vertex record holds one tolerance tail, and the records do not state which one it is)",
            vertex_differing,
        ),
        (
            "of tolerant-vertex records whose evaluated slot does not agree with the tolerance of their vertex (the written vertex record holds the vertex tolerance)",
            slot_disagrees,
        ),
        (
            "of tolerant-edge records that name an edge the document does not hold",
            edge_absent,
        ),
        (
            "of tolerant-edge records equal to an earlier record of their edge (the edge record holds one tolerance tail, which the earlier record writes)",
            edge_repeated,
        ),
        (
            "of tolerant-edge records that share their edge with another record (the edge record holds one tolerance tail, and the records do not state which one it is)",
            edge_differing,
        ),
        (
            "of tolerant-edge records on an edge with no tolerance (the written edge record holds the edge tolerance)",
            no_tolerance,
        ),
    ]
    .into_iter()
    .find_map(|(reason, records)| Unwritable::of(reason, records))
}

/// The sketch records the source-less Design stream cannot write as they
/// stand, the first group by reason that holds one. The stream writes a
/// sketch point in the current version-11 form with its companion, a sketch
/// curve in the current form of its geometry, and a relation or text that a
/// sketch header names.
pub(super) fn unwritable_sketch_records(native: &F3dNative) -> Option<Unwritable> {
    let registered = |class_tag, record_index| {
        source_less_design_record_type(native, class_tag, record_index, "sketch record").ok()
    };
    let mut unregistered = BTreeSet::new();
    let mut other_form = BTreeSet::new();
    let mut no_identity = BTreeSet::new();
    let mut owner_types = BTreeSet::new();
    let mut companions = BTreeSet::new();
    let mut missing_curves = BTreeSet::new();
    let mut unreachable = BTreeSet::new();
    let mut other_owner = BTreeSet::new();
    let typed = native
        .sketch_points
        .iter()
        .map(|record| record.record_index)
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|record| record.record_index),
        )
        .chain(
            native
                .sketch_relations
                .iter()
                .map(|record| record.record_index),
        )
        .chain(native.sketch_texts.iter().map(|record| record.record_index))
        .collect::<BTreeSet<_>>();
    let curve_indices = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.record_index)
        .collect::<BTreeSet<_>>();
    let registrations = |index: u32, matches: &dyn Fn(&SegmentType) -> bool| {
        native
            .design_types
            .iter()
            .filter(|design_type| {
                design_type
                    .entities
                    .values()
                    .any(|registered| *registered == u64::from(index))
                    && matches(design_type)
            })
            .count()
    };
    let mut paired = BTreeMap::<u32, usize>::new();
    for point in &native.sketch_points {
        *paired.entry(point.paired_reference).or_default() += 1;
    }
    let mut geometry_owners = BTreeMap::new();
    for point in &native.sketch_points {
        let id = point.id.clone();
        let Some(point_type) = registered(&point.class_tag, point.record_index) else {
            unregistered.insert(id);
            continue;
        };
        if !design_type_matches(
            point_type,
            crate::design::decode::sketch::CURRENT_SKETCH_POINT_TYPE,
        ) || point.record_form().class_version() != point_type.version
            || !matches!(
                point.record_form(),
                crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
                    derived_type: false,
                    ..
                }
            )
        {
            other_form.insert(id);
            continue;
        }
        if point
            .persistent_id()
            .is_none_or(|persistent_id| persistent_id == 0)
        {
            no_identity.insert(id);
            continue;
        }
        if let Some(owner) = point.owner_reference {
            geometry_owners.insert(point.record_index, owner);
            let containers = registrations(owner, &|design_type: &SegmentType| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID)
            });
            if containers != 1 {
                owner_types.insert(id);
                continue;
            }
        }
        let companion = point.paired_reference;
        if companion == point.record_index
            || typed.contains(&companion)
            || paired.get(&companion) != Some(&1)
            || registrations(companion, &|design_type: &SegmentType| {
                design_type_matches(
                    design_type,
                    crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE,
                )
            }) != 1
        {
            companions.insert(id);
            continue;
        }
        if point
            .companion()
            .incident_curves
            .iter()
            .any(|curve| !curve_indices.contains(curve))
        {
            missing_curves.insert(id);
        }
    }
    for curve in &native.sketch_curve_identities {
        let id = curve.id.clone();
        let Some(curve_type) = registered(&curve.class_tag, curve.record_index) else {
            unregistered.insert(id);
            continue;
        };
        let expected = match curve.geometry.as_ref() {
            Some(SketchCurveGeometry::Line { .. }) => {
                Some(crate::design::decode::sketch::CURRENT_SKETCH_LINE_TYPE)
            }
            Some(SketchCurveGeometry::Arc { .. }) => {
                Some(crate::design::decode::sketch::CURRENT_SKETCH_CIRCULAR_TYPE)
            }
            Some(SketchCurveGeometry::Nurbs { .. }) => {
                Some(crate::design::decode::sketch::CURRENT_SKETCH_NURBS_TYPE)
            }
            None => None,
        };
        if !expected.is_some_and(|expected| design_type_matches(curve_type, expected)) {
            other_form.insert(id);
            continue;
        }
        if let Some(owner) = curve.owner_reference {
            geometry_owners.insert(curve.record_index, owner);
        }
    }
    let roots = sketch_header_indices(native);
    for relation in &native.sketch_relations {
        let id = relation.id.clone();
        if registered(&relation.class_tag, relation.record_index).is_none() {
            unregistered.insert(id);
        } else if !roots.contains(&relation.record_index) {
            unreachable.insert(id);
        } else if relation.all_member_indices().any(|member| {
            geometry_owners
                .get(&member)
                .is_some_and(|owner| *owner != relation.owner_reference)
        }) {
            other_owner.insert(id);
        }
    }
    for text in &native.sketch_texts {
        let id = text.id.clone();
        if registered(&text.class_tag, text.record_index).is_none() {
            unregistered.insert(id);
        } else if !roots.contains(&text.record_index) {
            unreachable.insert(id);
        }
    }
    [
        (
            "of sketch records that the Design type registration of their class tag does not name",
            unregistered,
        ),
        (
            "of sketch records in a form other than the current record form of their kind (the source-less Design stream writes the current form)",
            other_form,
        ),
        (
            "of sketch points with no persistent identity (the written point record stores one)",
            no_identity,
        ),
        (
            "of sketch points whose sketch owner does not have one sketch-container type registration",
            owner_types,
        ),
        (
            "of sketch points whose companion record is another record, is shared, or does not have one current companion type registration (the written point holds one companion of its own)",
            companions,
        ),
        (
            "of sketch points whose companion names a sketch curve the document does not hold",
            missing_curves,
        ),
        (
            "of sketch relations and texts that no sketch header names (the source-less Design stream writes the records a sketch header names)",
            unreachable,
        ),
        (
            "of sketch relations whose sketch owner differs from the owner of a member record",
            other_owner,
        ),
    ]
    .into_iter()
    .find_map(|(reason, records)| Unwritable::of(reason, records))
}

pub(super) fn validate_source_less_sketch_graph(native: &F3dNative) -> Result<(), CodecError> {
    let sketch_owners = native
        .design_entity_headers
        .iter()
        .filter(|header| header.in_sketch_module())
        .map(|header| header.entity_id.suffix())
        .collect::<BTreeSet<_>>();
    let root_indices = sketch_header_indices(native);
    let mut typed_indices = BTreeMap::<u32, &str>::new();
    let mut typed_records = Vec::new();
    for (record_index, id, class_tag) in native
        .sketch_points
        .iter()
        .map(|record| (record.record_index, record.id.as_str(), &record.class_tag))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|record| (record.record_index, record.id.as_str(), &record.class_tag)),
        )
        .chain(
            native
                .sketch_relations
                .iter()
                .map(|record| (record.record_index, record.id.as_str(), &record.class_tag)),
        )
        .chain(
            native
                .sketch_texts
                .iter()
                .map(|record| (record.record_index, record.id.as_str(), &record.class_tag)),
        )
    {
        if let Some(before) = typed_indices.insert(record_index, id) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch records {before} and {id} share record index {record_index}"
            )));
        }
        typed_records.push((record_index, class_tag));
    }
    for (record_index, class_tag) in typed_records {
        source_less_design_record_type(native, class_tag, record_index, "sketch record")?;
    }
    let mut geometry_owners = BTreeMap::new();
    let mut point_companions = BTreeSet::new();
    let curve_indices = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.record_index)
        .collect::<BTreeSet<_>>();
    for point in &native.sketch_points {
        let point_type = source_less_design_record_type(
            native,
            &point.class_tag,
            point.record_index,
            "sketch point",
        )?;
        if !design_type_matches(
            point_type,
            crate::design::decode::sketch::CURRENT_SKETCH_POINT_TYPE,
        ) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch point {} requires the current point record type",
                point.id
            )));
        }
        if point.record_form().class_version() != point_type.version
            || !matches!(
                point.record_form(),
                crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
                    derived_type: false,
                    ..
                }
            )
        {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch point {} requires the version-11 member sequence",
                point.id
            )));
        }
        if point
            .persistent_id()
            .is_none_or(|persistent_id| persistent_id == 0)
        {
            return Err(CodecError::InvalidInput(format!(
                "source-less F3D sketch point {} has no persistent identity",
                point.id
            )));
        }
        let owner_reference = point.owner_reference.ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D sketch point {} has no direct sketch owner",
                point.id
            ))
        })?;
        if !sketch_owners.contains(&u64::from(owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} references missing sketch owner {owner_reference}",
                point.id
            )));
        }
        let owner_type_count = native
            .design_types
            .iter()
            .filter(|design_type| {
                design_type
                    .entities
                    .values()
                    .any(|registered| *registered == u64::from(owner_reference))
                    && design_type.type_guid.as_str().eq_ignore_ascii_case(
                        crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID,
                    )
            })
            .count();
        if owner_type_count != 1 {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} owner {owner_reference} does not have one sketch-container type registration",
                point.id
            )));
        }
        if point.paired_reference == point.record_index
            || typed_indices.contains_key(&point.paired_reference)
            || !point_companions.insert(point.paired_reference)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} has a conflicting companion record {}",
                point.id, point.paired_reference
            )));
        }
        let companion_type = native
            .design_types
            .iter()
            .filter(|design_type| {
                design_type
                    .entities
                    .values()
                    .any(|registered| *registered == u64::from(point.paired_reference))
                    && design_type_matches(
                        design_type,
                        crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE,
                    )
            })
            .count();
        if companion_type != 1 {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} companion {} does not have one current companion type registration",
                point.id, point.paired_reference
            )));
        }
        let companion = point.companion();
        for curve in companion.incident_curves {
            if !curve_indices.contains(curve) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D sketch point {} companion references missing curve {curve}",
                    point.id
                )));
            }
        }
        geometry_owners.insert(point.record_index, owner_reference);
    }
    for curve in &native.sketch_curve_identities {
        let curve_type = source_less_design_record_type(
            native,
            &curve.class_tag,
            curve.record_index,
            "sketch curve",
        )?;
        let expected_type = match curve.geometry.as_ref() {
            Some(SketchCurveGeometry::Line { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_LINE_TYPE
            }
            Some(SketchCurveGeometry::Arc { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_CIRCULAR_TYPE
            }
            Some(SketchCurveGeometry::Nurbs { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_NURBS_TYPE
            }
            None => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D sketch curve {} has no writable geometry",
                    curve.id
                )));
            }
        };
        if !design_type_matches(curve_type, expected_type) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch curve {} requires its current geometry record type",
                curve.id
            )));
        }
        let owner_reference = curve.owner_reference.ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D sketch curve {} has no direct sketch owner",
                curve.id
            ))
        })?;
        if !sketch_owners.contains(&u64::from(owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch curve {} references missing sketch owner {owner_reference}",
                curve.id
            )));
        }
        geometry_owners.insert(curve.record_index, owner_reference);
    }
    for relation in &native.sketch_relations {
        if !root_indices.contains(&relation.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch relation {} is not reachable from a sketch header",
                relation.id
            )));
        }
        if !sketch_owners.contains(&u64::from(relation.owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch relation {} references missing sketch owner {}",
                relation.id, relation.owner_reference
            )));
        }
        for member in relation.all_member_indices() {
            if geometry_owners
                .get(&member)
                .is_some_and(|owner| *owner != relation.owner_reference)
            {
                return Err(CodecError::InvalidInput(format!(
                    "F3D sketch relation {} owner disagrees with geometry record {member}",
                    relation.id
                )));
            }
        }
    }
    for text in &native.sketch_texts {
        if !root_indices.contains(&text.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch text {} is not reachable from a sketch header",
                text.id
            )));
        }
        if !sketch_owners.contains(&u64::from(text.owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch text {} references missing sketch owner {}",
                text.id, text.owner_reference
            )));
        }
    }
    let mut explicit_headers = BTreeSet::new();
    for header in &native.design_record_headers {
        if !explicit_headers.insert(header.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "multiple F3D Design record headers use index {}",
                header.record_index
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_source_less_design_ownership(native: &F3dNative) -> Result<(), CodecError> {
    let mut types_by_guid = BTreeMap::new();
    let mut entity_types = BTreeMap::new();
    let mut entity_modules = BTreeMap::new();
    for design_type in &native.design_types {
        if types_by_guid
            .insert(design_type.type_guid.as_str(), design_type)
            .is_some()
        {
            return Err(CodecError::InvalidInput(format!(
                "duplicate F3D Design type GUID: {}",
                design_type.type_guid.as_str()
            )));
        }
        for entity_id in design_type.entities.values() {
            if let Some(before) = entity_types.insert(*entity_id, design_type.type_guid.as_str()) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D Design entity {entity_id} is registered by both type {before} and type {}",
                    design_type.type_guid.as_str()
                )));
            }
            entity_modules.insert(*entity_id, design_type.module.clone());
        }
    }
    // A base type need not be registered by the same segment, so an unresolved
    // base GUID is legal; a resolved chain must still terminate.
    for design_type in &native.design_types {
        if design_type
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            == Some(design_type.type_guid.as_str())
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design type {} is its own base type",
                design_type.id()
            )));
        }
        let mut ancestors = BTreeSet::new();
        let mut cursor = design_type;
        while let Some(base) = cursor
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            .and_then(|base| types_by_guid.get(base))
        {
            if !ancestors.insert(base.type_guid.as_str()) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D Design type hierarchy contains a cycle at {}",
                    base.type_guid.as_str()
                )));
            }
            cursor = base;
        }
    }
    for header in &native.design_entity_headers {
        let owned_module = entity_modules.get(&header.entity_id.suffix()).cloned();
        if header.module() != owned_module.as_deref() {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design header {} module conflicts with MetaStream ownership",
                header.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_source_less_history_graph(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let Some(namespace) = target.native.namespace("f3d") else {
        return Ok(());
    };
    let stored_count = |arena: &str| namespace.arenas().get(arena).map_or(0, Vec::len);
    for arena in [
        "asm_histories",
        "asm_delta_states",
        "asm_bulletin_boards",
        "asm_entity_changes",
        "asm_history_records",
    ] {
        if let Some(records) = namespace.arenas().get(arena) {
            let unique = records
                .iter()
                .map(cadmpeg_ir::NativeRecord::id)
                .collect::<BTreeSet<_>>();
            if unique.len() != records.len() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D {arena} contains duplicate record ids"
                )));
            }
        }
    }
    let states = native
        .asm_histories
        .iter()
        .flat_map(crate::history_records::AsmHistory::states)
        .collect::<Vec<_>>();
    let boards = states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .collect::<Vec<_>>();
    let changes = boards.iter().flat_map(|board| &board.changes).count();
    let records = states.iter().flat_map(|state| &state.records).count();
    let reconstructed = [
        ("asm_histories", native.asm_histories.len()),
        ("asm_delta_states", states.len()),
        ("asm_bulletin_boards", boards.len()),
        ("asm_entity_changes", changes),
        ("asm_history_records", records),
    ];
    if reconstructed
        .iter()
        .any(|(arena, count)| stored_count(arena) != *count)
    {
        return Err(CodecError::InvalidInput(
            "F3D ASM history graph contains orphaned or ambiguously parented records".into(),
        ));
    }
    for state in states {
        for record in &state.records {
            if record.raw_bytes.is_empty() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D ASM history record {} has an empty native payload",
                    record.id
                )));
            }
        }
    }
    if let Some(history) = native
        .asm_histories
        .iter()
        .find(|history| history.binding_budget_exceeded)
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize ASM history {} whose bulletin boards and records were not retained",
            history.id
        )));
    }
    for history in &native.asm_histories {
        match history.preamble {
            Some(preamble)
                if history
                    .states()
                    .first()
                    .is_some_and(|state| state.state_id == preamble.stream_size)
                    && preamble.history_entry_count >= 0 => {}
            Some(_) => {
                return Err(CodecError::InvalidInput(format!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
            None => {}
        }
    }
    if native
        .asm_histories
        .iter()
        .any(|history| !crate::history::graph_is_coherent(history))
    {
        return Err(CodecError::InvalidInput(
            "F3D ASM history graph is not a coherent doubly linked state chain".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_source_less_design_links(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    if let Some(sentinel) = native.mesh_surface_sentinels.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize mesh-surface sentinel {} without its retained ASM record",
            sentinel.id()
        )));
    }
    let bodies = target
        .geometry
        .bodies
        .iter()
        .enumerate()
        .map(|(ordinal, body)| (&body.id, (ordinal, body)))
        .collect::<HashMap<_, _>>();
    let faces = target
        .geometry
        .faces
        .iter()
        .map(|face| (&face.id, face))
        .collect::<HashMap<_, _>>();
    let edges = target
        .geometry
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let mut groups: BTreeMap<AttributeTarget, Vec<&PersistentDesignLink>> = BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if bodies.contains_key(id) => {
                Some(link.target.clone())
            }
            _ => None,
        };
        let Some(target_key) = target_key else {
            return Err(CodecError::InvalidInput(format!(
                "F3D persistent design link {} has an unsupported or missing target",
                link.id
            )));
        };

        groups.entry(target_key).or_default().push(link);
    }
    let mut subentity_groups: BTreeMap<(u8, String), Vec<&PersistentSubentityTag>> =
        BTreeMap::new();
    for tag in &native.persistent_subentity_tags {
        let target_key = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if faces.contains_key(id) => {
                Some((2, id.as_str().to_owned()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edges.contains_key(id) => {
                Some((1, id.as_str().to_owned()))
            }
            _ => None,
        };
        let Some(target_key) = target_key else {
            return Err(CodecError::InvalidInput(format!(
                "F3D persistent subentity tag {} has an unsupported or missing target",
                tag.id
            )));
        };

        subentity_groups.entry(target_key).or_default().push(tag);
    }
    for (target, mut tags) in subentity_groups {
        tags.sort_by_key(|tag| tag.ordinal);
        for (ordinal, tag) in tags.iter().enumerate() {
            if index_from_u32(tag.ordinal) != ordinal {
                return Err(CodecError::InvalidInput(format!(
                    "F3D persistent subentity tags for {target:?} require contiguous ordinals"
                )));
            }
        }
    }
    for (target, mut links) in groups {
        links.sort_by_key(|link| link.ordinal);
        for (ordinal, link) in links.iter().enumerate() {
            if index_from_u32(link.ordinal) != ordinal {
                return Err(CodecError::InvalidInput(format!(
                    "F3D persistent design links for {target:?} require contiguous ordinals"
                )));
            }
        }
    }

    let vertices = target
        .geometry
        .vertices
        .iter()
        .map(|vertex| (&vertex.id, vertex))
        .collect::<HashMap<_, _>>();
    let shells = target
        .geometry
        .shells
        .iter()
        .map(|shell| (&shell.id, shell))
        .collect::<HashMap<_, _>>();
    macro_rules! validate_unique_targets {
        ($items:expr, $field:ident, $valid:expr, $label:literal) => {
            validate_unique_targets!($items, $field, $valid, $label, [id()], |item, _target| {});
        };
        ($items:expr, $field:ident, $valid:expr, $label:literal,
         [$($id:tt)+], |$item:ident, $target:pat_param| $validate:block) => {{
            let mut seen = BTreeSet::new();
            for $item in $items {
                let $target = $valid.get(&$item.$field).copied().ok_or_else(|| {
                    CodecError::InvalidInput(format!(
                        "F3D {} metadata {} targets missing entity {}",
                        $label, $item.$($id)+, $item.$field
                    ))
                })?;
                if !seen.insert(&$item.$field) {
                    return Err(CodecError::InvalidInput(format!(
                        "multiple F3D {} records target {}",
                        $label, $item.$field
                    )));
                }
                $validate
            }
        }};
    }
    validate_unique_targets!(&native.body_native_keys, body, bodies, "body-native-key");
    validate_unique_targets!(
        &native.transform_hints,
        body,
        bodies,
        "transform-hint",
        [id()],
        |hints, (_, body)| {
            if body.transform.is_none() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D transform hints {} target a body without a transform",
                    hints.id()
                )));
            }
        }
    );
    validate_unique_targets!(&native.edge_continuities, edge, edges, "edge-continuity");
    validate_unique_targets!(&native.edge_ownerships, edge, edges, "edge-ownership");
    validate_unique_targets!(
        &native.vertex_ownerships,
        vertex,
        vertices,
        "vertex-ownership"
    );
    validate_unique_targets!(&native.face_sidedness, face, faces, "face-sidedness");
    let mut wire_record_indices = BTreeSet::new();
    for wire in &native.wire_topologies {
        let shell = shells.get(&wire.shell).copied().ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D wire-topology metadata {} targets missing entity {}",
                wire.id(),
                wire.shell
            ))
        })?;
        if !wire_record_indices.insert(wire.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "multiple F3D wire-topology records use native index {}",
                wire.record_index
            )));
        }
        let member_form_is_valid = match &wire.members {
            cadmpeg_asm::brep::records::WireMembers::Edges(edges) => {
                !edges.is_empty()
                    && edges
                        .iter()
                        .all(|edge| shell.wire_edges().any(|member| member == edge))
            }
            cadmpeg_asm::brep::records::WireMembers::Vertex(vertex) => {
                shell.free_vertices().any(|member| member == vertex)
            }
        };
        if !member_form_is_valid {
            return Err(CodecError::InvalidInput(format!(
                "F3D wire metadata {} has invalid edge-ring or isolated-vertex membership",
                wire.id()
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_source_less_body_kinds(
    geometry: &cadmpeg_ir::document::Topology,
) -> Result<(), CodecError> {
    for (body, derived) in geometry.bodies.iter().zip(derived_body_kinds(geometry)) {
        if body.kind != derived {
            return Err(CodecError::InvalidInput(format!(
                "body {} declares {:?} but its incidence graph is {:?}",
                body.id, body.kind, derived
            )));
        }
    }
    Ok(())
}

/// The kind each body's incidence graph derives, in body order: the kind a
/// decoder reads back from the written body.
pub(super) fn derived_body_kinds(
    geometry: &cadmpeg_ir::document::Topology,
) -> Vec<cadmpeg_ir::topology::BodyKind> {
    let edges = geometry
        .edges
        .iter()
        .map(|edge| (&edge.id, edge))
        .collect::<HashMap<_, _>>();
    let curves = geometry
        .curves
        .iter()
        .map(|curve| (&curve.id, curve))
        .collect::<HashMap<_, _>>();
    // A curveless edge is written with a null curve reference, so a decoder
    // reads it back as collapsed when both endpoints name one vertex.
    let collapsed = |edge: &cadmpeg_ir::ids::EdgeId| {
        edges.get(edge).is_some_and(|edge| {
            edge.ends
                .as_ref()
                .is_some_and(|ends| ends.start == ends.end)
                && edge.curve().is_none_or(|curve| {
                    curves.get(curve).is_some_and(|curve| {
                        matches!(
                            curve.geometry,
                            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_))
                        )
                    })
                })
        })
    };
    let derive = |body: &cadmpeg_ir::topology::Body| {
        let shell_ids = geometry
            .regions
            .iter()
            .filter(|region| region.body == body.id)
            .flat_map(cadmpeg_ir::topology::Region::stated_shells)
            .collect::<BTreeSet<_>>();
        let face_ids = geometry
            .faces
            .iter()
            .filter(|face| shell_ids.contains(&face.shell))
            .map(|face| &face.id)
            .collect::<BTreeSet<_>>();
        let has_wires = geometry
            .shells
            .iter()
            .filter(|shell| shell_ids.contains(&shell.id))
            .any(|shell| {
                shell.wire_edges().next().is_some() || shell.free_vertices().next().is_some()
            });
        let loop_ids = geometry
            .faces
            .iter()
            .filter(|face| face_ids.contains(&face.id))
            .flat_map(|face| face.loops.stated())
            .collect::<BTreeSet<_>>();
        let coedge_ids = geometry
            .loops
            .iter()
            .filter(|loop_| loop_ids.contains(&loop_.id))
            .flat_map(cadmpeg_ir::topology::Loop::coedges)
            .collect::<BTreeSet<_>>();
        if face_ids.is_empty() {
            cadmpeg_ir::topology::BodyKind::Wire
        } else if has_wires {
            cadmpeg_ir::topology::BodyKind::General
        } else {
            cadmpeg_asm::brep::geometry::faced_body_kind(
                geometry
                    .coedges
                    .iter()
                    .filter(|coedge| coedge_ids.contains(&coedge.id))
                    .map(|coedge| (&coedge.edge, coedge.sense)),
                collapsed,
            )
        }
    };
    geometry.bodies.iter().map(derive).collect()
}

/// Resolved wire-ownership ordinals of one body.
#[derive(Clone, Copy)]
pub(super) struct WireBodyOwnership {
    /// Ordinal of the body's first region in `geometry.regions`.
    pub(super) first_region: usize,
    /// Ordinal of that region's first shell in `geometry.shells`.
    pub(super) first_shell: usize,
}

/// Resolved wire-ownership ordinals of one region.
#[derive(Clone, Copy)]
pub(super) struct WireRegionOwnership {
    /// Ordinal of the owning body in `geometry.bodies`.
    pub(super) body: usize,
    /// Ordinal of the next region of the same body in `geometry.regions`.
    pub(super) next_region: Option<usize>,
    /// Ordinal of the region's first shell in `geometry.shells`.
    pub(super) first_shell: usize,
}

/// Resolved wire-ownership ordinals of one shell.
#[derive(Clone, Copy)]
pub(super) struct WireShellOwnership {
    /// Ordinal of the owning region in `geometry.regions`.
    pub(super) region: usize,
    /// Ordinal of the next shell of the same region in `geometry.shells`.
    pub(super) next_shell: Option<usize>,
}

/// Proof that [`validate_source_less_wire_ownership`] ran against the borrowed
/// `CadIr`. The private fields keep construction inside this module, so the wire
/// encoder reads resolved ordinals instead of re-searching the model and
/// asserting that every lookup succeeds.
pub(in crate::writer::generate) struct WireOwnershipValidated {
    bodies: Vec<WireBodyOwnership>,
    regions: Vec<WireRegionOwnership>,
    shells: Vec<WireShellOwnership>,
}

impl WireOwnershipValidated {
    /// Per-body ordinals in `geometry.bodies` order.
    pub(super) fn bodies(&self) -> &[WireBodyOwnership] {
        &self.bodies
    }

    /// Per-region ordinals in `geometry.regions` order.
    pub(super) fn regions(&self) -> &[WireRegionOwnership] {
        &self.regions
    }

    /// Per-shell ordinals in `geometry.shells` order.
    pub(super) fn shells(&self) -> &[WireShellOwnership] {
        &self.shells
    }
}

pub(super) fn validate_source_less_wire_ownership(
    target: &CadIr,
) -> Result<WireOwnershipValidated, CodecError> {
    let geometry = &target.geometry;
    let inconsistent =
        || CodecError::Malformed("source-less F3D wire ownership is inconsistent".into());
    let region_ordinal = |id: &cadmpeg_ir::ids::RegionId| {
        geometry.regions.iter().position(|region| region.id == *id)
    };
    let shell_ordinal =
        |id: &cadmpeg_ir::ids::ShellId| geometry.shells.iter().position(|shell| shell.id == *id);
    let lists = geometry.child_lists();
    let complete_shells = |region| lists.complete_shells(region).ok_or_else(inconsistent);

    let mut regions = Vec::new();
    for region in &geometry.regions {
        let body = geometry
            .bodies
            .iter()
            .position(|body| body.id == region.body)
            .ok_or_else(inconsistent)?;
        let siblings = lists
            .complete_body_regions(&region.body)
            .ok_or_else(inconsistent)?
            .iter()
            .map(|sibling| &sibling.id)
            .collect::<Vec<_>>();
        let position = siblings
            .iter()
            .position(|id| **id == region.id)
            .ok_or_else(inconsistent)?;
        let next_region = siblings
            .get(position + 1)
            .map(|id| region_ordinal(id).ok_or_else(inconsistent))
            .transpose()?;
        let region_shells = complete_shells(region)?;
        let first_shell = region_shells
            .first()
            .and_then(&shell_ordinal)
            .ok_or_else(inconsistent)?;
        if region_shells.iter().any(|id| {
            !geometry
                .shells
                .iter()
                .any(|shell| shell.id == *id && shell.region == region.id)
        }) {
            return Err(inconsistent());
        }
        regions.push(WireRegionOwnership {
            body,
            next_region,
            first_shell,
        });
    }

    let mut bodies = Vec::new();
    for body in &geometry.bodies {
        let first_region = lists
            .complete_body_regions(&body.id)
            .ok_or_else(inconsistent)?
            .first()
            .and_then(|region| region_ordinal(&region.id))
            .ok_or_else(inconsistent)?;
        bodies.push(WireBodyOwnership {
            first_region,
            first_shell: regions[first_region].first_shell,
        });
    }

    let mut shells = Vec::new();
    for shell in &geometry.shells {
        let (region, owner, position) = geometry
            .regions
            .iter()
            .enumerate()
            .find_map(|(ordinal, region)| {
                if region.id != shell.region {
                    return None;
                }
                region
                    .stated_shells()
                    .position(|id| *id == shell.id)
                    .map(|position| (ordinal, region, position))
            })
            .ok_or_else(inconsistent)?;
        let next_shell = complete_shells(owner)?
            .get(position + 1)
            .map(|id| shell_ordinal(id).ok_or_else(inconsistent))
            .transpose()?;
        shells.push(WireShellOwnership { region, next_shell });
    }

    Ok(WireOwnershipValidated {
        bodies,
        regions,
        shells,
    })
}

#[cfg(test)]
mod tests {
    use super::{unwritable_tolerant_tails, F3dNative};
    use cadmpeg_asm::brep::records::identity::NativeRecordNamespace;
    use cadmpeg_asm::brep::records::{EvaluatedToleranceSlot, TolerantVertexTail};

    #[test]
    fn an_unset_tolerant_vertex_tail_on_a_vertex_with_a_tolerance_is_unwritable() {
        let mut target = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
        target.geometry.vertices[0].tolerance =
            Some(cadmpeg_ir::scalar::PositiveLength::new(0.025).unwrap());
        let native = F3dNative {
            tolerant_vertex_tails: vec![TolerantVertexTail {
                source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
                record_index: 0,
                vertex: target.geometry.vertices[0].id.clone(),
                leading_tolerances: [
                    cadmpeg_ir::scalar::FiniteReal::new(-1.0).unwrap(),
                    cadmpeg_ir::scalar::FiniteReal::new(-1.0).unwrap(),
                ],
                evaluated_slot: EvaluatedToleranceSlot::Unset { trailing: Some(0) },
            }],
            ..F3dNative::default()
        };
        let id = native.tolerant_vertex_tails[0].id();
        let unwritable =
            unwritable_tolerant_tails(&target, &native).expect("the tail is unwritable");
        assert_eq!(unwritable.records, std::collections::BTreeSet::from([id]));
    }

    fn unset_tail(
        vertex: &cadmpeg_ir::ids::VertexId,
        record_index: u32,
        leading: f64,
    ) -> TolerantVertexTail {
        TolerantVertexTail {
            source_namespace: NativeRecordNamespace::new(crate::ids::ID_FORMAT),
            record_index,
            vertex: vertex.clone(),
            leading_tolerances: [
                cadmpeg_ir::scalar::FiniteReal::new(leading).unwrap(),
                cadmpeg_ir::scalar::FiniteReal::new(-1.0).unwrap(),
            ],
            evaluated_slot: EvaluatedToleranceSlot::Unset { trailing: Some(0) },
        }
    }

    #[test]
    fn a_tolerant_vertex_tail_equal_to_an_earlier_tail_of_its_vertex_is_dropped_alone() {
        let target = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
        let vertex = &target.geometry.vertices[0].id;
        let native = F3dNative {
            tolerant_vertex_tails: vec![unset_tail(vertex, 0, -1.0), unset_tail(vertex, 1, -1.0)],
            ..F3dNative::default()
        };
        let unwritable =
            unwritable_tolerant_tails(&target, &native).expect("the repeated tail is unwritable");
        assert_eq!(
            unwritable.records,
            std::collections::BTreeSet::from([native.tolerant_vertex_tails[1].id()])
        );
    }

    #[test]
    fn tolerant_vertex_tails_that_differ_on_one_vertex_are_all_dropped() {
        let target = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
        let vertex = &target.geometry.vertices[0].id;
        let native = F3dNative {
            tolerant_vertex_tails: vec![unset_tail(vertex, 0, -1.0), unset_tail(vertex, 1, 0.5)],
            ..F3dNative::default()
        };
        let unwritable = unwritable_tolerant_tails(&target, &native)
            .expect("the differing tails are unwritable");
        assert_eq!(
            unwritable.records,
            native
                .tolerant_vertex_tails
                .iter()
                .map(TolerantVertexTail::id)
                .collect()
        );
    }
}
