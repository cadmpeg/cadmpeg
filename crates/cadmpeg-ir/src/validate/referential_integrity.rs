// SPDX-License-Identifier: Apache-2.0
//! Registry-driven validation of every typed entity reference.

use crate::document::CadIr;
use crate::index::ModelIndex;
use crate::report::{Check, Finding, Severity};
use crate::schema::EntitySchema;

pub(super) fn check_typed_references(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    macro_rules! check_arenas {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
            $(for entity in &ir.model.$field {
                let owner = entity.identity();
                entity.visit_references(&mut |reference| {
                    if !index.contains(&reference.target)
                        && !findings.iter().any(|finding| {
                            finding.check == Check::ReferentialIntegrity
                                && finding.entity.as_deref() == Some(owner)
                                && finding.message.contains(&reference.target)
                        })
                    {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!("unresolved typed reference {}", reference.target),
                            entity: Some(owner.to_owned()),
                        });
                    }
                });
            })*
        };
    }

    crate::document::arena_registry!(check_arenas);
}
