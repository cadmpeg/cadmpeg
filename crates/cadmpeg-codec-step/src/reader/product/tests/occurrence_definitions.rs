// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use crate::test_support::exchange::decode_inline;

#[test]
fn standalone_occurrence_definition_has_no_placement() {
    let decoded = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('PART','Part','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('part','',#4,#5);
#7=PRODUCT_DEFINITION_OCCURRENCE('instance',$,$,#6,$);",
    );
    let occurrences = &decoded.ir().model.occurrences;
    assert_eq!(occurrences.len(), 1);
    assert!(occurrences[0].transform.is_none());
}

#[test]
fn occurrence_definitions_preserve_reused_prototype_paths() {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};

    let decoded = decode_inline(
        "#1=APPLICATION_CONTEXT('mechanical design');
#2=PRODUCT_CONTEXT('',#1,'mechanical');
#3=PRODUCT('ROOT','Root','',(#2));
#4=PRODUCT_DEFINITION_FORMATION('','',#3);
#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');
#6=PRODUCT_DEFINITION('root','',#4,#5);
#7=PRODUCT('PART','Part','',(#2));
#8=PRODUCT_DEFINITION_FORMATION('','',#7);
#9=PRODUCT_DEFINITION('part','',#8,#5);
#11=PRODUCT_DEFINITION_OCCURRENCE('first',$,$,#9,$);
#12=PRODUCT_DEFINITION_OCCURRENCE('second',$,$,#9,$);
#13=NEXT_ASSEMBLY_USAGE_OCCURRENCE('first','First','',#6,#11,$);
#14=NEXT_ASSEMBLY_USAGE_OCCURRENCE('second','Second','',#11,#12,$);",
    );
    let occurrences = &decoded.ir().model.occurrences;
    let first = occurrences
        .iter()
        .find(|item| item.name.as_deref() == Some("First"))
        .unwrap();
    let second = occurrences
        .iter()
        .find(|item| item.name.as_deref() == Some("Second"))
        .unwrap();
    assert!(
        matches!(&second.parent, OccurrenceParent::Occurrence { occurrence } if occurrence == &first.id)
    );
    assert!(
        matches!((&first.prototype, &second.prototype), (PrototypeReference::Local { definition: a }, PrototypeReference::Local { definition: b }) if a == b)
    );
    assert!(first.transform.is_none());
    assert!(second.transform.is_none());
}
