// SPDX-License-Identifier: Apache-2.0
//! Member-count-dependent `THRU_CURVE` branch states.

use super::branch_items::BranchItems;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ThruCurveBranchItems<T> {
    Standard(BranchItems<T>),
    Extended {
        members: [T; 4],
        values: [[u8; 4]; 2],
    },
}

impl<T> ThruCurveBranchItems<T> {
    pub(crate) fn from_parts(members: Vec<T>, lane: &[u8]) -> Result<Self, &'static str> {
        match Self::validate(members, lane, |lane| {
            Ok::<_, std::convert::Infallible>(lane.iter())
        }) {
            Ok(value) => value,
            Err(error) => match error {},
        }
    }

    pub(super) fn from_wire(
        ctx: &DecodeContext<'_>,
        members: Vec<T>,
        lane: &[u8],
    ) -> Result<Result<Self, &'static str>, CodecError> {
        Ok(Self::validate(members, lane, |lane| {
            ctx.admit_iter(lane, "NX thru-curve state lane validation")
        })?)
    }

    fn validate<'a, E, I: Iterator<Item = &'a u8>>(
        members: Vec<T>,
        lane: &'a [u8],
        admit: impl FnOnce(&'a [u8]) -> Result<I, E>,
    ) -> Result<Result<Self, &'static str>, E> {
        if members.len().checked_add(4) == Some(lane.len()) && admit(lane)?.all(|&byte| byte == 0) {
            return Ok(BranchItems::new(members).map(Self::Standard));
        }
        if let [0, 0, 0, 0, 1, 5, a, b, c, d, 1, 5, e, f, g, h, 0, 0] = lane {
            let Ok(members) = members.try_into() else {
                return Ok(Err("state_lane extended form requires four members"));
            };
            return Ok(Ok(Self::Extended {
                members,
                values: [[*a, *b, *c, *d], [*e, *f, *g, *h]],
            }));
        }
        Ok(Err(
            "state_lane must be the member-count-sized zero lane or the four-member extended lane",
        ))
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        match self {
            Self::Standard(members) => members.as_slice(),
            Self::Extended { members, .. } => members,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub(crate) fn declared_count(&self) -> u8 {
        match self {
            Self::Standard(members) => members.declared_count(),
            Self::Extended { .. } => 5,
        }
    }

    pub(super) fn state_lane_len(&self) -> usize {
        match self {
            Self::Standard(members) => members.len() + 4,
            Self::Extended { .. } => 18,
        }
    }

    // Names follow the ordered source slots in this fixed-width lane.
    #[allow(clippy::many_single_char_names)]
    #[cfg(test)]
    pub(crate) fn state_lane(&self) -> Vec<u8> {
        match self {
            Self::Standard(members) => [0; 258][..members.len() + 4].to_vec(),
            Self::Extended {
                values: [[a, b, c, d], [e, f, g, h]],
                ..
            } => vec![0, 0, 0, 0, 1, 5, *a, *b, *c, *d, 1, 5, *e, *f, *g, *h, 0, 0],
        }
    }

    #[cfg(test)]
    pub(super) fn map_indexed<U>(
        self,
        mut f: impl FnMut(usize, T) -> U,
    ) -> ThruCurveBranchItems<U> {
        match self {
            Self::Standard(members) => ThruCurveBranchItems::Standard(members.map_indexed(f)),
            Self::Extended { members, values } => {
                let mut index = 0;
                let members = members.map(|member| {
                    let mapped = f(index, member);
                    index += 1;
                    mapped
                });
                ThruCurveBranchItems::Extended { members, values }
            }
        }
    }

    pub(super) fn try_map_indexed_charged<U>(
        self,
        ctx: &DecodeContext<'_>,
        mut project: impl FnMut(usize, T) -> Result<U, CodecError>,
    ) -> Result<ThruCurveBranchItems<U>, CodecError> {
        match self {
            Self::Standard(members) => Ok(ThruCurveBranchItems::Standard(
                members.try_map_indexed_charged(ctx, project)?,
            )),
            Self::Extended {
                members: [first, second, third, fourth],
                values,
            } => Ok(ThruCurveBranchItems::Extended {
                members: [
                    project(0, first)?,
                    project(1, second)?,
                    project(2, third)?,
                    project(3, fourth)?,
                ],
                values,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ThruCurveBranchItems;

    #[test]
    fn state_layout_and_member_count_cannot_disagree() {
        let lane = [0, 0, 0, 0, 1, 5, 2, 3, 4, 5, 1, 5, 6, 7, 8, 9, 0, 0];
        let extended = ThruCurveBranchItems::from_parts(vec![1, 2, 3, 4], &lane).unwrap();
        assert_eq!(extended.state_lane(), lane);
        assert_eq!(extended.declared_count(), 5);
        let mapped = extended.map_indexed(|i, n| i + n);
        assert_eq!(mapped.as_slice(), [1, 3, 5, 7]);
        assert_eq!(mapped.state_lane(), lane);
        assert!(ThruCurveBranchItems::from_parts(vec![1, 2, 3], &lane).is_err());
        for index in [0, 4, 5, 10, 11, 16, 17] {
            let mut invalid = lane;
            invalid[index] ^= 1;
            assert!(ThruCurveBranchItems::from_parts(vec![1, 2, 3, 4], &invalid).is_err());
        }
        let standard = ThruCurveBranchItems::from_parts(vec![1, 2], &[0; 6]).unwrap();
        assert_eq!(standard.state_lane(), [0; 6]);
        assert_eq!(standard.declared_count(), 3);
        assert!(ThruCurveBranchItems::from_parts(vec![1, 2], &[0; 5]).is_err());
        assert!(ThruCurveBranchItems::from_parts(Vec::<u8>::new(), &[0; 4]).is_err());
    }

    #[test]
    fn thru_curve_state_iteration_refusal_propagates() {
        use cadmpeg_core::decode::ResourceDimension;
        use cadmpeg_core::CodecError;
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "NX thru-curve state lane validation",
            |ctx| ThruCurveBranchItems::from_wire(ctx, vec![1, 2], &[0; 6]),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "NX thru-curve state lane validation"));
    }
}
