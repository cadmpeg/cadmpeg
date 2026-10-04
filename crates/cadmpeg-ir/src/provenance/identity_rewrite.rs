// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{CodecFormat, SourceObjectAssociation};

rewrite_scalar!(CodecFormat);
rewrite_record!(SourceObjectAssociation, []; {format, object_id, name, color, visible, layer, instance_path});
