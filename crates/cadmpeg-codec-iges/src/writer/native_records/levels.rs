// SPDX-License-Identifier: Apache-2.0
//! Type 406 Form 1 definition levels rebuilt from typed records.

use super::{count, entity, integer, Entity, Fields};

pub(super) fn encode(fields: &mut Fields<'_>, type_code: u32, form: i64) -> Result<Entity, String> {
    let levels = fields.array("levels")?;
    let mut values = vec![count(&fields.values, "declared_count", levels.len())?];
    for level in &levels {
        values.push(integer(Some(level))?);
    }
    Ok(entity(type_code, form, values))
}
