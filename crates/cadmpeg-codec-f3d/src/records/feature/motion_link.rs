// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::scalar::PositiveReal;
use serde::{Deserialize, Serialize};

pub(crate) const ROTATE_Z: u32 = 2;
pub(crate) const SCOPE_TYPE: &str = "29098FE8-08FC-4FD8-9D31-42CF164F73EB";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignLinkedRotations {
    pub(crate) joints: [u32; 2],
    pub(crate) angles: [PositiveReal; 2],
}

impl DesignLinkedRotations {
    pub(crate) fn ratio(&self) -> Option<PositiveReal> {
        let [first, second] = self.angles;
        PositiveReal::new(second.get() / first.get())
    }

    pub(crate) fn is_valid(&self) -> bool {
        let [first, second] = self.joints;
        first != 0 && second != 0 && first != second && self.ratio().is_some()
    }
}

rewrite_native_record!(DesignLinkedRotations, []; { joints, angles });
