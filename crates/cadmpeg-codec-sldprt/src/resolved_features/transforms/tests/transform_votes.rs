//! Transform votes over indexed marker loci.

use super::super::{compatible_marker_transform_candidates, Axes, MarkerTransform, Sign};
use crate::resolved_features::grid::GridPoint;
use std::collections::{HashMap, HashSet};

#[test]
fn transform_votes_select_the_winner() {
    let loci = (0i64..64)
        .map(|i| GridPoint::from((i + 7, i * i + 11)))
        .collect::<HashSet<_>>();
    let compatible = (0i64..64)
        .map(|i| (GridPoint::from((i, i * i)), &loci))
        .collect::<HashMap<_, _>>();
    let expected = MarkerTransform {
        axes: Axes::Aligned {
            swap: false,
            u: Sign::Positive,
            v: Sign::Positive,
        },
        translation: (7, 11),
    };
    assert_eq!(
        compatible_marker_transform_candidates(&compatible),
        vec![expected]
    );
}
