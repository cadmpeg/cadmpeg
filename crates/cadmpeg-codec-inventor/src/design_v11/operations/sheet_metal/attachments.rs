// SPDX-License-Identifier: Apache-2.0
//! Complete edge operands for attached sheet metal.

use cadmpeg_ir::design::selection::{NativeOwner, OrderedSelection, Picks};
use cadmpeg_ir::features::{EdgeSelection, Feature};

use super::super::{selections, Projected};

pub(super) fn edges(value: &EdgeSelection, features: &[Feature]) -> Projected<OrderedSelection> {
    let (items, native) = match value {
        EdgeSelection::Historical {
            state,
            edges,
            native,
        } => {
            let items = edges
                .iter()
                .map(|edge| {
                    let key = serde_json::to_string(&(state, edge))
                        .map_err(|_| "the historical attachment key is invalid")?;
                    selections::named(&key, None)
                })
                .collect::<Projected<Vec<_>>>()?;
            (items, native.as_str())
        }
        EdgeSelection::Generated { edges, native } => {
            let items = edges
                .iter()
                .map(|edge| {
                    if !features.iter().any(|feature| feature.id == edge.feature) {
                        return Err("the attachment producer is absent");
                    }
                    selections::named(
                        edge.local_id.as_str(),
                        Some(NativeOwner::Feature(edge.feature.clone())),
                    )
                })
                .collect::<Projected<Vec<_>>>()?;
            (items, native.as_str())
        }
        _ => return Err("the attachment edges are unresolved"),
    };
    let mut selection = selections::selection(items)?;
    if !matches!(&selection.picks, Picks::Named(items) if !items.as_slice().is_empty()) {
        return Err("the attachment edges are empty");
    }
    selection.native = Some(
        vec![native.to_owned()]
            .try_into()
            .map_err(|_| "the attachment source is blank")?,
    );
    Ok(selection)
}
