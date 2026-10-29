// SPDX-License-Identifier: Apache-2.0
//! Typed native-reference access over the complete neutral arena registry.

use crate::document::Model;
use crate::schema::EntitySchema;

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
}

#[cfg(test)]
mod tests;
