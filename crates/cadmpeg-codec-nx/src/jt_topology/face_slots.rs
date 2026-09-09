use cadmpeg_core::decode::alloc_filled;

#[derive(Clone)]
pub(super) struct FaceSlots {
    vertices: Vec<Option<usize>>,
    empty: usize,
}

impl FaceSlots {
    pub(super) fn new(degree: usize) -> Option<Self> {
        Some(Self {
            vertices: alloc_filled(degree, None, "nx JT face vertex slots").ok()?,
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
        let mut slots = FaceSlots::new(2).unwrap();
        assert_eq!(slots.fill(0, 7), Some(()));
        assert_eq!(slots.fill(0, 7), Some(()));
        assert_eq!(slots.fill(0, 8), None);
        assert_eq!(slots.fill(2, 8), None);
        assert_eq!(&*slots, &[Some(7), None]);
        assert_eq!(slots.empty(), 1);
        assert_eq!(slots.fill(1, 8), Some(()));
        assert_eq!(slots.empty(), 0);
    }
}
