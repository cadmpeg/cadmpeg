// SPDX-License-Identifier: Apache-2.0
//! Typed native-reference access over the complete neutral arena registry.

use crate::document::{ArenaName, Model};
use crate::schema::EntitySchema;
use cadmpeg_core::decode::ArenaClass;

/// Visit every neutral record through its declared native-reference accessor.
///
/// The callback receives the arena, compiled Rust owner type, record identity,
/// and optional native reference. Labels and other string fields are not links.
pub fn visit_native_references<'a>(
    model: &'a Model,
    visitor: &mut impl FnMut(&'static str, &'static str, &'a str, Option<&'a str>),
) {
    macro_rules! visit {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
            for record in &model.$field {
                visitor(stringify!($field), std::any::type_name_of_val(record),
                    record.identity(), record.native_reference());
            }
        )*};
    }
    crate::document::arena_registry!(visit);
    for topology in &model.feature_input_topologies {
        for operand in &topology.operands {
            let native_ref = &operand.native_ref;
            visitor(
                "feature_input_topologies",
                std::any::type_name_of_val(operand),
                operand.id.as_str(),
                Some(native_ref.as_str()),
            );
        }
    }
}

/// First neutral record linked to a native owner.
///
/// A neutral record links the owner when its `native_ref` names the owner.
/// Model-class arenas precede design-class arenas, then registry order, then
/// ascending record id selects the first qualifier.
pub fn linking_record(model: &Model, owner: &str) -> Option<(ArenaName, String)> {
    let mut hits: Vec<(ArenaName, String)> = Vec::new();
    visit_native_references(model, &mut |arena, _, id, native_ref| {
        if native_ref == Some(owner) {
            if let Some(arena) = ArenaName::parse(arena) {
                let id = if arena.as_str() == "feature_input_topologies" {
                    model
                        .feature_input_topologies
                        .iter()
                        .find_map(|topology| {
                            topology
                                .operands
                                .iter()
                                .any(|operand| operand.id.as_str() == id)
                                .then_some(topology.id.as_str())
                        })
                        .unwrap_or(id)
                } else {
                    id
                };
                hits.push((arena, id.to_owned()));
            }
        }
    });
    hits.sort_by(|left, right| {
        let class_rank = |arena: &ArenaName| match arena.class() {
            ArenaClass::Model => 0,
            ArenaClass::Design => 1,
        };
        class_rank(&left.0)
            .cmp(&class_rank(&right.0))
            .then(
                ArenaName::ALL
                    .iter()
                    .position(|arena| arena == &left.0)
                    .cmp(&ArenaName::ALL.iter().position(|arena| arena == &right.0)),
            )
            .then(left.1.cmp(&right.1))
    });
    hits.into_iter().next()
}

#[cfg(test)]
mod tests;
