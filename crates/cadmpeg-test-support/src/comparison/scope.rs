// SPDX-License-Identifier: Apache-2.0
//! Scope loss requires an actual failed root and conservation of every class row.

use std::collections::BTreeSet;

use cadmpeg_ir::document::{ArenaClass, ArenaName, ContentScope};
use serde_json::Value;

use super::{header, Allowances};

impl Allowances {
    // Called only after ZIP source correspondence proves an actual original
    // root prefix and its exact zip.archive GivenUp home, and one-home checking
    // has established the complete removed-record closure.
    pub(super) fn prove_root_scope_losses(
        &mut self,
        before: &cadmpeg_ir::CadIr,
        after: &cadmpeg_ir::CadIr,
    ) -> Result<(), String> {
        for class in before
            .scope
            .classes()
            .filter(|class| !after.scope.holds(*class))
        {
            if after.model.populated_arena(class).is_some() {
                return Err(format!("scope loss for {class} leaves a populated arena"));
            }
            before
                .visit_neutral_provenance(&mut |record| {
                    if record.class() == class && !self.removed.contains(record.id()) {
                        return Err(cadmpeg_core::CodecError::malformed(format_args!(
                            "scope loss for {class} has an unwitnessed record {}",
                            record.id()
                        )));
                    }
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
            self.lost_classes.insert(class);
        }
        Ok(())
    }
}

pub(super) fn headers(
    before: &Value,
    after: &Value,
    allowances: &Allowances,
) -> Result<(Value, Value), String> {
    let mut previous = header(before)?;
    let current = header(after)?;
    for class in &allowances.lost_classes {
        if !allowances.given_up || !scope(before)?.holds(*class) || scope(after)?.holds(*class) {
            return Err(format!("unproved scope transition for {class}"));
        }
        check_rows(before, after, *class, &allowances.removed)?;
        previous["scope"][class.to_string()] = Value::Bool(false);
    }
    Ok((previous, current))
}

fn scope(document: &Value) -> Result<ContentScope, String> {
    serde_json::from_value(
        document
            .get("scope")
            .cloned()
            .ok_or("document has no scope")?,
    )
    .map_err(|error| error.to_string())
}

fn check_rows(
    before: &Value,
    after: &Value,
    class: ArenaClass,
    removed: &BTreeSet<String>,
) -> Result<(), String> {
    for (document, previous) in [(before, true), (after, false)] {
        for (name, rows) in document
            .get("model")
            .and_then(Value::as_object)
            .ok_or("document has no model")?
        {
            let arena = ArenaName::parse(name).ok_or("unregistered neutral arena")?;
            if arena.class() != class {
                continue;
            }
            for row in rows.as_array().ok_or("neutral arena is not an array")? {
                let id = row
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("neutral record has no string id")?;
                if !previous || !removed.contains(id) {
                    return Err(format!(
                        "scope loss for {class} has an unwitnessed record {}",
                        id
                    ));
                }
            }
        }
    }
    Ok(())
}
