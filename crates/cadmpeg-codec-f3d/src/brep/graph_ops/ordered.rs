// SPDX-License-Identifier: Apache-2.0
//! Select graph rows in source order through admitted identity indexes.

use cadmpeg_ir::attributes::AttributeTarget;
use std::collections::HashSet;

pub(super) fn select_rows<T>(
    rows: Vec<T>,
    reachable: &HashSet<String>,
    owner: impl Fn(&T) -> &str,
) -> Vec<T> {
    let mut retained = Vec::new();
    for row in rows {
        if reachable.contains(owner(&row)) {
            retained.push(row);
        }
    }
    retained
}

fn target_selected(target: &AttributeTarget, reachable: &HashSet<String>) -> bool {
    let id = match target {
        AttributeTarget::Document => return true,
        AttributeTarget::Body(id) => id.as_str(),
        AttributeTarget::Face(id) => id.as_str(),
        AttributeTarget::Shell(id) => id.as_str(),
        AttributeTarget::Loop(id) => id.as_str(),
        AttributeTarget::Coedge(id) => id.as_str(),
        AttributeTarget::Edge(id) => id.as_str(),
        AttributeTarget::Vertex(id) => id.as_str(),
        AttributeTarget::Region(id) => id.as_str(),
        AttributeTarget::Surface(id) => id.as_str(),
        AttributeTarget::Curve(id) => id.as_str(),
        AttributeTarget::Point(id) => id.as_str(),
        AttributeTarget::Record { arena, id }
            if matches!(
                arena.as_str(),
                "bodies"
                    | "faces"
                    | "shells"
                    | "loops"
                    | "coedges"
                    | "edges"
                    | "vertices"
                    | "regions"
                    | "surfaces"
                    | "curves"
                    | "points"
            ) =>
        {
            id.as_str()
        }
        // This operation extracts a B-rep graph; owners outside that graph
        // are retained by their own document projection, not this subset.
        AttributeTarget::Feature(_)
        | AttributeTarget::Sketch(_)
        | AttributeTarget::Parameter(_)
        | AttributeTarget::ProductDefinition(_)
        | AttributeTarget::Occurrence(_)
        | AttributeTarget::Tessellation(_)
        | AttributeTarget::Pmi(_)
        | AttributeTarget::SemanticAnnotation(_)
        | AttributeTarget::Drawing(_)
        | AttributeTarget::ViewPresentation(_)
        | AttributeTarget::Record { .. } => return false,
    };
    reachable.contains(id)
}

pub(super) fn select_links<T>(
    rows: Vec<T>,
    reachable: &HashSet<String>,
    target: impl Fn(&T) -> &AttributeTarget,
) -> Vec<T> {
    let mut retained = Vec::new();
    for row in rows {
        if target_selected(target(&row), reachable) {
            retained.push(row);
        }
    }
    retained
}

#[cfg(test)]
mod tests;
