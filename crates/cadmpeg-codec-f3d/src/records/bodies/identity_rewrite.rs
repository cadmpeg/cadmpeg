// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{BodyVisibility, DesignBodyBinding};

rewrite_native_record!(BodyVisibility, []; {id, body, stream, byte_offset, asm_body_key_offset, asm_body_key, entity_suffix, visible});
rewrite_native_record!(DesignBodyBinding, []; {id, stream, pair_count, pair_ordinal, asm_body_key, asm_body_key_offset, entity_suffix, blob_name, blob_name_offset, body});
