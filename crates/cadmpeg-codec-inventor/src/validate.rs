// SPDX-License-Identifier: Apache-2.0
//! Inventor-native validation.

use cadmpeg_ir::{CadIr, Check, Finding, Severity};

use crate::native::INVENTOR_NATIVE_VERSION;

pub(crate) fn validate_native(ir: &CadIr) -> Vec<Finding> {
    let Some(namespace) = ir.native.namespace("inventor") else {
        return Vec::new();
    };
    if namespace.version == INVENTOR_NATIVE_VERSION {
        Vec::new()
    } else {
        vec![Finding {
            check: Check::Version,
            severity: Severity::Error,
            message: format!(
                "unsupported Inventor native namespace version {}",
                namespace.version
            ),
            entity: None,
        }]
    }
}
