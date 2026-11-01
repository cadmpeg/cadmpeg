// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{PointChannel, PointCloud};

rewrite_record!(PointChannel, []; {name, values});
rewrite_record!(PointCloud, []; {id, product_definition, name, sensor, positions, normals, colors, channels, native_ref});
