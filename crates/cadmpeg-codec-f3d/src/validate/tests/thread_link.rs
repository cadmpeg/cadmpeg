// SPDX-License-Identifier: Apache-2.0

use super::super::{thread_class_pair_holds, thread_designation_offset_holds};
use crate::records::feature::thread::DesignThreadForm;
use crate::records::references::DesignClassTag;

#[test]
fn standard_compact_admits_any_class_pair_and_the_two_designation_offsets() {
    let tag = |text: &str| DesignClassTag::try_from(text.to_owned()).unwrap();
    for (class, paired) in [("901", "902"), ("334", "262"), ("414", "263")] {
        assert!(thread_class_pair_holds(
            DesignThreadForm::StandardCompact,
            &tag(class),
            &tag(paired)
        ));
    }
    // The legacy forms stay tied to their class pair.
    assert!(!thread_class_pair_holds(
        DesignThreadForm::StandardLegacy,
        &tag("901"),
        &tag("902")
    ));
    assert!(DesignThreadForm::StandardCompact.has_standard_reference_table());
    for offset in [38, 42] {
        assert!(thread_designation_offset_holds(1000 + offset, 1000));
    }
    for offset in [0, 37, 39, 41, 43] {
        assert!(!thread_designation_offset_holds(1000 + offset, 1000));
    }
    assert!(!thread_designation_offset_holds(10, 1000));
}
