// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{CodecFormat, SourceGeometryRole, SourceObjectAssociation};

rewrite_scalar!(CodecFormat);
rewrite_record!(SourceObjectAssociation, []; {format, geometry_role, object_id, instance_path});

rewrite_scalar!(SourceGeometryRole);
