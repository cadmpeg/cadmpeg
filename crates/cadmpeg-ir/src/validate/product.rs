// SPDX-License-Identifier: Apache-2.0
//! Product prototype and occurrence-tree validation.

use std::collections::{HashMap, HashSet};

use super::{Check, Finding, Severity};
use crate::document::CadIr;
use crate::product::OccurrenceParent;

pub(super) fn check_products(ir: &CadIr, findings: &mut Vec<Finding>) {
    let bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect::<HashSet<_>>();
    let products = ir
        .model
        .products
        .iter()
        .map(|product| product.id.as_str())
        .collect::<HashSet<_>>();
    let occurrences = ir
        .model
        .product_occurrences
        .iter()
        .map(|occurrence| (occurrence.id.as_str(), occurrence))
        .collect::<HashMap<_, _>>();

    for product in &ir.model.products {
        for body in &product.bodies {
            if !bodies.contains(body.as_str()) {
                missing(findings, product.id.as_str(), "body", body.as_str());
            }
        }
    }
    for occurrence in &ir.model.product_occurrences {
        if !occurrence.transform.is_finite() {
            findings.push(Finding {
                check: Check::ProductStructure,
                severity: Severity::Error,
                message: "occurrence transform contains a non-finite coefficient".into(),
                entity: Some(occurrence.id.0.clone()),
            });
        }
        if !products.contains(occurrence.product.as_str()) {
            missing(
                findings,
                occurrence.id.as_str(),
                "product",
                occurrence.product.as_str(),
            );
        }
        if let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent {
            if !occurrences.contains_key(parent.as_str()) {
                missing(
                    findings,
                    occurrence.id.as_str(),
                    "parent occurrence",
                    parent.as_str(),
                );
            }
        }
    }
    let mut parent_state = HashMap::<&str, u8>::new();
    for occurrence in &ir.model.product_occurrences {
        if parent_state.get(occurrence.id.as_str()) == Some(&2) {
            continue;
        }
        let mut path = Vec::new();
        let mut cursor = occurrence;
        loop {
            match parent_state.get(cursor.id.as_str()) {
                Some(1) => {
                    findings.push(Finding {
                        check: Check::ProductStructure,
                        severity: Severity::Error,
                        message: "occurrence parent graph contains a cycle".into(),
                        entity: Some(cursor.id.0.clone()),
                    });
                    break;
                }
                Some(2) => break,
                _ => {}
            }
            parent_state.insert(cursor.id.as_str(), 1);
            path.push(cursor.id.as_str());
            let OccurrenceParent::Occurrence { occurrence: parent } = &cursor.parent else {
                break;
            };
            let Some(next) = occurrences.get(parent.as_str()) else {
                break;
            };
            cursor = next;
        }
        for id in path {
            parent_state.insert(id, 2);
        }
    }
}

fn missing(findings: &mut Vec<Finding>, owner: &str, kind: &str, target: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: format!("references missing {kind} `{target}`"),
        entity: Some(owner.into()),
    });
}
