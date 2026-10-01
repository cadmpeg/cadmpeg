// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{NativeUnknownRecord};

rewrite_record!(NativeUnknownRecord, []; {id, links});
