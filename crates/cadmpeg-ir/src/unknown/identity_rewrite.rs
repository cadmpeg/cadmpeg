// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{NativeUnknownRecord, RawRetainedBytes, UnknownRecord};

rewrite_record!(NativeUnknownRecord, []; {id, links});

rewrite_record!(UnknownRecord, []; {id, offset, retention, links});
rewrite_enum!(RawRetainedBytes, []; {Inline {data}, Digest {byte_len, sha256}});
