// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::target::TargetDescriptor;

pub(super) const CATALOG_WRITE_TARGETS: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: cadmpeg_core::dialect_id!("test:old"),
        aliases: &["old"],
    },
    TargetDescriptor {
        id: cadmpeg_core::dialect_id!("test:new"),
        aliases: &["new"],
    },
];
