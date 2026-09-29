use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Clone)]
pub(super) struct FaceSlots {
    vertices: Vec<Option<usize>>,
    empty: usize,
}

impl FaceSlots {
    pub(super) fn new(ctx: &DecodeContext<'_>, degree: usize) -> Result<Self, CodecError> {
        Ok(Self {
            vertices: ctx.alloc_filled(degree, None, "nx JT face vertex slots")?,
            empty: degree,
        })
    }

    pub(super) fn fill(&mut self, slot: usize, value: usize) -> Option<()> {
        match self.vertices.get_mut(slot)? {
            target @ None => {
                *target = Some(value);
                self.empty -= 1;
            }
            Some(existing) if *existing == value => {}
            Some(_) => return None,
        }
        Some(())
    }

    pub(super) fn empty(&self) -> usize {
        self.empty
    }
}

impl std::ops::Deref for FaceSlots {
    type Target = [Option<usize>];

    fn deref(&self) -> &Self::Target {
        &self.vertices
    }
}

#[cfg(test)]
mod tests {
    use super::FaceSlots;

    #[test]
    fn repeated_and_rejected_fills_preserve_the_empty_count() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut slots = FaceSlots::new(&ctx, 2).unwrap();
        assert_eq!(slots.fill(0, 7), Some(()));
        assert_eq!(slots.fill(0, 7), Some(()));
        assert_eq!(slots.fill(0, 8), None);
        assert_eq!(slots.fill(2, 8), None);
        assert_eq!(&*slots, &[Some(7), None]);
        assert_eq!(slots.empty(), 1);
        assert_eq!(slots.fill(1, 8), Some(()));
        assert_eq!(slots.empty(), 0);
    }

    #[test]
    fn face_vertex_slots_refuse_collection_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 2;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = FaceSlots::new(&ctx, 3)
            .err()
            .expect("three slots exceed two items");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && limit.used == 0
                    && limit.additional == 3
        ));
    }
}
