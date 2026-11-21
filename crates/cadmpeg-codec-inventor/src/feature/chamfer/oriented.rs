// SPDX-License-Identifier: Apache-2.0
//! Unequal two-distance chamfers whose edge and support face join stored
//! revisions of one input body state.

use cadmpeg_core::{
    decode::{unique_index, DecodeContext},
    CodecError,
};
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferForm, ChamferSpec},
        DistinctMembers, EdgeSelection, Feature, FeatureDefinition, FeatureInputTopology,
        FeatureOperation, HistoricalEdgeGeometry,
    },
    ids::{HistoricalEdgeId, HistoricalFaceId},
    scalar::{Length, PositiveLength},
};

use super::Operands;
use crate::feature::{
    edge_selection::{geometry::EdgeGeometry, historical},
    face_selection, FeatureInventory, PmDcFeature, PmDcFeatureProperty,
};
use crate::pmdc::PmDcReference;

/// Largest generator array of a support-face selector that the join admits.
const SUPPORT_GENERATOR_LIMIT: usize = 2;

/// The joined operands of one chamfer.
struct Joined {
    edge: HistoricalEdgeId,
    face: HistoricalFaceId,
    native: String,
    distances: [PositiveLength; 2],
    geometry: EdgeGeometry,
}

/// Joins the one edge and the one support face of the chamfer. The face
/// revision that carries the first distance is the only matched owner of the
/// support selector that uses the edge revision. A selector with a repeated
/// matching attribute or an unshared generated-name family stays native,
/// because no note states those readings for a chamfer.
fn join<'a>(
    inventory: &FeatureInventory,
    source: &PmDcFeature,
    property: &impl Fn(&str, PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: &impl Fn(&PmDcFeature, usize) -> Option<Length>,
) -> Option<Joined> {
    let token = source.identity.segment_token.as_str();
    let by_reference = |reference| property(token, reference);
    let operands =
        super::operands(source, by_reference, |slot| length(source, slot), |_| None).ok()?;
    let Operands {
        edges,
        distances: Some(distances),
        support: Some(support),
        ..
    } = operands
    else {
        return None;
    };
    if !super::explicit_selection_controls(source, by_reference) {
        return None;
    }
    let [(edge_item, edge_source)] = historical::selection_sources(edges, by_reference)?[..] else {
        return None;
    };
    let edge = inventory
        .edge_inputs
        .historical
        .get(&(token.to_owned(), edge_source))
        .filter(|edge| edge.item == edge_item && edge.owners == 1)?;
    let [first, second] = edge.faces?;
    let collection = by_reference(*source.properties.references().get(1)?)?;
    let [(face_item, face_source)] =
        face_selection::selection_sources(collection, by_reference)?[..]
    else {
        return None;
    };
    if face_source != support {
        return None;
    }
    let face = inventory
        .edge_inputs
        .faces
        .get(&(token.to_owned(), face_source))
        .filter(|face| {
            face.item == face_item
                && face.state != face_selection::NAMING_STATE_MODIFIED
                && face.generators <= SUPPORT_GENERATOR_LIMIT
                && !face.repeated_attribute
                && !face.unshared_family
        })?;
    let mut uses_edge = face
        .owners
        .iter()
        .filter(|owner| **owner == first || **owner == second);
    uses_edge.next()?;
    if uses_edge.next().is_some() {
        return None;
    }
    Some(Joined {
        edge: historical::edge_id(token, edge_source).ok()?,
        face: face_selection::face_id(token, face_source).ok()?,
        native: edges.id(),
        distances,
        geometry: edge.geometry.clone(),
    })
}

/// Replaces the native selection and unresolved dimensions of each unequal
/// two-distance chamfer whose edge and support face join, and returns the
/// input states that hold them. The support face is the face of the edge that
/// carries the first distance, in slot 2; the other face of the edge carries
/// the second, in slot 3.
pub(crate) fn project<'a>(
    ctx: &DecodeContext<'_>,
    features: &mut [Feature],
    inventory: &FeatureInventory,
    property: impl Fn(&str, PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: impl Fn(&PmDcFeature, usize) -> Option<Length>,
) -> Result<Vec<FeatureInputTopology>, CodecError> {
    if inventory.edge_inputs.historical.is_empty() || inventory.edge_inputs.faces.is_empty() {
        return Ok(Vec::new());
    }
    let sources = unique_index(
        inventory
            .features
            .iter()
            .map(|source| (source.id(), source)),
    );
    let mut states = Vec::new();
    for feature in features {
        let Some(Some(source)) = feature
            .native_ref
            .as_deref()
            .and_then(|native| sources.get(native))
        else {
            continue;
        };
        if !native_two_distances(feature, source) {
            continue;
        }
        let Some(joined) = join(inventory, source, &property, &length) else {
            continue;
        };
        let state = historical::state_id(source)?;
        let selection =
            EdgeSelection::historical(state.clone(), vec![joined.edge.clone()], joined.native)
                .map_err(CodecError::malformed)?;
        let [first, second] = joined.distances;
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
                definition
            {
                for group in groups.iter_mut() {
                    group.edges = selection.clone();
                    group.spec = ChamferSpec::TwoDistances { first, second };
                }
            }
        });
        ctx.charge_entities(3, "project Inventor oriented chamfer")?;
        states.push(FeatureInputTopology {
            id: state,
            input_of: feature.id.clone(),
            bodies: DistinctMembers::default(),
            faces: vec![joined.face.clone()]
                .try_into()
                .map_err(CodecError::malformed)?,
            edges: vec![joined.edge.clone()]
                .try_into()
                .map_err(CodecError::malformed)?,
            vertices: DistinctMembers::default(),
            edge_geometry: vec![HistoricalEdgeGeometry {
                edge: joined.edge,
                start: Some(joined.geometry.start),
                end: Some(joined.geometry.end),
                interior: joined.geometry.interior,
                tangent: Some(joined.geometry.tangent),
                faces: vec![joined.face]
                    .try_into()
                    .map_err(CodecError::malformed)?,
                supports: Vec::new(),
            }],
            native_ref: Some(source.id()),
        });
    }
    Ok(states)
}

/// Whether the feature is one chamfer group with an unresolved two-distance
/// dimension and the native oriented selection of its record.
fn native_two_distances(feature: &Feature, source: &PmDcFeature) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups,
        flip_direction: true,
    }) = feature.evaluation.definition()
    else {
        return false;
    };
    let [group] = groups.as_slice() else {
        return false;
    };
    group.spec
        == ChamferSpec::Unresolved {
            form: Some(ChamferForm::TwoDistances),
        }
        && group.edges == EdgeSelection::Native(source.id())
}

/// Whether a joined two-distance chamfer holds its support face in its input
/// state: the state lists that face once, and the sample of the edge names it
/// as the only face. Other chamfers pass.
pub(crate) fn valid(
    feature: &Feature,
    source: &PmDcFeature,
    expected: &Operands<'_>,
    states: &[FeatureInputTopology],
) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
        feature.evaluation.definition()
    else {
        return true;
    };
    let [group] = groups.as_slice() else {
        return true;
    };
    let (EdgeSelection::Historical { state, edges, .. }, ChamferSpec::TwoDistances { .. }) =
        (&group.edges, group.spec)
    else {
        return true;
    };
    if expected
        .distances
        .is_none_or(|[first, second]| first == second)
    {
        return true;
    }
    let token = source.identity.segment_token.as_str();
    let Some(face) = expected
        .support
        .and_then(|selector| face_selection::face_id(token, selector).ok())
    else {
        return false;
    };
    let mut rows = states
        .iter()
        .filter(|row| row.id == *state && row.input_of == feature.id);
    let Some(row) = rows.next() else {
        return false;
    };
    rows.next().is_none()
        && row.faces.as_slice() == [face.clone()]
        && edges.iter().all(|edge| {
            row.edge_geometry
                .iter()
                .filter(|sample| sample.edge == *edge)
                .all(|sample| sample.faces.as_slice() == [face.clone()])
        })
}

#[cfg(test)]
mod tests;
