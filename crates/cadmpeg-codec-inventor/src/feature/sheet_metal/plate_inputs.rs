// SPDX-License-Identifier: Apache-2.0
//! Attached-plate edge selections that resolve in the history state before
//! the plate.
//!
//! A selector whose matched owners differ in geometry across the whole
//! construction history selects different edge revisions in different states.
//! The plate consumes the revision that is live in the state before the plate
//! (section 13.10).

use std::collections::{HashMap, HashSet};

use cadmpeg_asm::history::{
    historical_record_archive, materialize_record_table, walk_entity_versions,
};
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;

use super::{contour::attachment::selectors_by_token, operands};
use crate::feature::edge_selection::historical::{self, SelectedEdge};
use crate::feature::{history_states, FeatureFamily, FeatureInventory};
use crate::kernel::{ActiveCarrierState, KernelFamily};

const MAX_PLATE_INPUT_WORK: usize = 1 << 21;

/// The plates whose selected sources lack a stored input revision.
struct Wanted<'a> {
    plate: &'a crate::feature::PmDcFeature,
    sources: Vec<u32>,
}

fn wanted(inventory: &FeatureInventory) -> Vec<Wanted<'_>> {
    let properties = cadmpeg_core::decode::unique_index(inventory.properties.iter().map(|p| {
        (
            (p.identity.segment_token.as_str(), p.identity.record_ordinal),
            p,
        )
    }));
    inventory
        .features
        .iter()
        .filter(|plate| {
            let token = plate.identity.segment_token.as_str();
            let mut labels = inventory.labels.iter().filter(|label| {
                label.identity.segment_token.as_str() == token
                    && label.header.owner.record_ordinal() == Some(plate.identity.record_ordinal)
            });
            matches!(
                labels
                    .next()
                    .and_then(|label| FeatureFamily::from_class_id(label.class_id())),
                Some(FeatureFamily::Plate)
            )
        })
        .filter_map(|plate| {
            let token = plate.identity.segment_token.as_str();
            let sources = operands::selection_sources(plate, |reference| {
                properties
                    .get(&(token, reference.record_ordinal()?))?
                    .as_ref()
                    .copied()
            })?;
            let missing = sources
                .into_iter()
                .filter(|source| {
                    let key = (token.to_owned(), *source);
                    !inventory.edge_inputs.historical.contains_key(&key)
                        && !matches!(inventory.edge_inputs.names.get(&key), Some(Some(_)))
                })
                .collect::<Vec<_>>();
            (!missing.is_empty()).then_some(Wanted {
                plate,
                sources: missing,
            })
        })
        .collect()
}

type Key = (String, u32);

/// Record the edge that a plate resolves for `key`. Two plates that resolve
/// one source to different geometry leave it ambiguous.
fn keep(
    resolved: &mut HashMap<Key, SelectedEdge>,
    ambiguous: &mut HashSet<Key>,
    key: Key,
    edge: SelectedEdge,
) {
    match resolved.get(&key) {
        Some(previous) if previous.geometry != edge.geometry => {
            ambiguous.insert(key);
        }
        Some(_) => {}
        None => {
            resolved.insert(key, edge);
        }
    }
}

/// Resolve the edge selections of attached plates in the state before each
/// plate, for the sources whose whole-history resolution is ambiguous.
///
/// A source resolves when exactly one matched owner is live in that state.
/// Two plates that resolve one source to different geometry leave it
/// unresolved.
pub(in crate::feature) fn read(
    ctx: &DecodeContext<'_>,
    document: &crate::rse::RseInventory<'_>,
    inventory: &mut FeatureInventory,
) -> Result<(), CodecError> {
    let plates = wanted(inventory);
    if plates.is_empty() {
        return Ok(());
    }
    let ActiveCarrierState::Selected(carrier) = &document.active_carrier else {
        return Ok(());
    };
    if carrier.family != KernelFamily::Asm || !historical::supported_schema(carrier.schema) {
        return Ok(());
    }
    let Ok(header) = carrier.header.as_ref() else {
        return Ok(());
    };
    let selectors = selectors_by_token(ctx, document)?;
    if selectors.len() != 1 {
        return Ok(());
    }
    let bytes = carrier.bytes.window();
    let history_states::Framing::Framed(framed) =
        history_states::frame_chain(ctx, bytes, header.width)?
    else {
        return Ok(());
    };
    let history_states::Prepared::Ready(chain) =
        history_states::prepare(ctx, &framed, bytes, header.width)?
    else {
        return Ok(());
    };
    let history_states::Reading::Read(index) =
        history_states::index_chain(&chain, carrier.schema, history_states::MAX_STATE_SCAN_WORK)
    else {
        return Ok(());
    };
    let history_states::Chain {
        views,
        frames,
        archived_ids,
        active_count,
    } = *chain;
    let Some(archive) = historical_record_archive(&views, frames) else {
        return Ok(());
    };
    let work = WorkBudget::new(MAX_PLATE_INPUT_WORK);
    let mut failure = None;
    let mut bounded = false;
    let mut visited = 0;
    let mut resolved = HashMap::<Key, SelectedEdge>::new();
    let mut ambiguous = HashSet::<Key>::new();
    let closed = walk_entity_versions(&views, active_count, &archived_ids, |_, versions| {
        let ordinal = views
            .len()
            .checked_sub(visited + 1)
            .and_then(|ordinal| u32::try_from(ordinal).ok());
        visited += 1;
        let Some(ordinal) = ordinal else {
            return;
        };
        if failure.is_some() {
            return;
        }
        let before = plates
            .iter()
            .filter(|wanted| {
                index
                    .first
                    .get(&wanted.plate.header.source_index)
                    .is_some_and(|after| after.checked_sub(1) == Some(ordinal))
            })
            .collect::<Vec<_>>();
        if before.is_empty() {
            return;
        }
        if !work.charge_by(work_units(versions.len())) {
            bounded = true;
            return;
        }
        let Some(table) = materialize_record_table(versions, |revision| archive.get(&revision))
        else {
            return;
        };
        for wanted in before {
            let token = wanted.plate.identity.segment_token.as_str();
            let Some(stored) = selectors.get(token) else {
                continue;
            };
            let mut selected = match historical::selected(ctx, &table, stored, carrier.schema) {
                Ok(selected) => selected,
                Err(error @ CodecError::ResourceLimit(_)) => {
                    failure = Some(error);
                    return;
                }
                Err(_) => continue,
            };
            for source in &wanted.sources {
                let key = (token.to_owned(), *source);
                let Some(edge) = selected.remove(source).filter(|edge| edge.owners == 1) else {
                    continue;
                };
                keep(&mut resolved, &mut ambiguous, key, edge);
            }
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if bounded || !closed {
        return Ok(());
    }
    for (key, edge) in resolved {
        if ambiguous.contains(&key) {
            continue;
        }
        let Some(generators) = selectors
            .get(key.0.as_str())
            .and_then(|stored| stored.iter().find(|selector| selector.source == key.1))
            .map(|selector| selector.generators.clone())
        else {
            continue;
        };
        ctx.charge_entities(1, "retain Inventor plate input edge")?;
        inventory
            .edge_inputs
            .historical_generators
            .insert(key.clone(), generators);
        inventory.edge_inputs.historical.insert(key, edge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::edge_selection::geometry::EdgeGeometry;
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::units::UnitVector3;

    fn edge(end: f64) -> SelectedEdge {
        let point = |x| FinitePoint3::new(Point3::new(x, 0.0, 0.0)).expect("point");
        SelectedEdge {
            owner: 7,
            owners: 1,
            generators: 2,
            item: 3,
            geometry: EdgeGeometry {
                start: point(0.0),
                end: point(end),
                interior: point(end / 2.0),
                tangent: UnitVector3::X_AXIS,
            },
            faces: None,
        }
    }

    #[test]
    fn plates_that_resolve_one_source_to_different_geometry_leave_it_ambiguous() {
        let mut resolved = HashMap::new();
        let mut ambiguous = HashSet::new();
        let key = ("fixture".to_owned(), 61);
        keep(&mut resolved, &mut ambiguous, key.clone(), edge(2.0));
        keep(&mut resolved, &mut ambiguous, key.clone(), edge(2.0));
        assert!(ambiguous.is_empty());
        assert_eq!(resolved.len(), 1);
        keep(&mut resolved, &mut ambiguous, key.clone(), edge(3.0));
        assert!(ambiguous.contains(&key));
    }
}
