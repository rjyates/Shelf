//! Offset mapping for slugify mutations (ST `@storyteller-platform/mapping`).
//!
//! `invert` + `append_mapping` shipped (ST Mapping.invert / appendMapping).
//! Markup wrap uses sequential sentence spans (ST markupChapter pos walk),
//! not plain.find.

#[derive(Debug, Clone, Default)]
pub struct Mapping {
    starts: Vec<i32>,
    old_sizes: Vec<i32>,
    new_sizes: Vec<i32>,
    dirty: bool,
    compiled_starts: Vec<i32>,
    compiled_offsets: Vec<i32>,
    compiled_old_sizes: Vec<i32>,
    compiled_new_sizes: Vec<i32>,
    total_offset: i32,
    /// Cached invert() result (ST `inverted`); cleared on insert_map.
    inverted: Option<Box<Mapping>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapAssoc {
    Start,
    End,
}

impl Mapping {
    pub fn new() -> Self {
        // ST Mapping starts dirty=true so an empty invert()/cursor compiles cleanly.
        Self {
            dirty: true,
            ..Self::default()
        }
    }

    /// Mutation at `start` in the original: `old_size` → `new_size`.
    pub fn insert_map(&mut self, start: i32, old_size: i32, new_size: i32) {
        let diff = new_size - old_size;
        if diff == 0 {
            return;
        }
        self.dirty = true;
        self.inverted = None;

        let last = self.starts.last().copied();
        if last.is_none_or(|l| l < start) {
            self.starts.push(start);
            self.old_sizes.push(old_size);
            self.new_sizes.push(new_size);
            return;
        }

        let i = lower_bound(start, &self.starts);
        if self.starts.get(i).copied() == Some(start) {
            if self.old_sizes[i] == new_size {
                self.starts.remove(i);
                self.old_sizes.remove(i);
                self.new_sizes.remove(i);
                return;
            }
            self.new_sizes[i] = new_size;
            return;
        }

        self.starts.insert(i, start);
        self.old_sizes.insert(i, old_size);
        self.new_sizes.insert(i, new_size);
    }

    fn compile(&mut self) {
        if !self.dirty {
            return;
        }
        let n = self.starts.len();
        self.compiled_offsets = Vec::with_capacity(n);
        let mut total = 0i32;
        let mut prev_end = -1i32;
        for i in 0..n {
            let start = self.starts[i];
            let old_size = self.old_sizes[i];
            let new_size = self.new_sizes[i];
            if start < prev_end {
                panic!("Mapping overlapping entries at {start} (prev end {prev_end})");
            }
            prev_end = start + old_size;
            self.compiled_offsets.push(total);
            total += new_size - old_size;
        }
        self.compiled_starts = self.starts.clone();
        self.compiled_old_sizes = self.old_sizes.clone();
        self.compiled_new_sizes = self.new_sizes.clone();
        self.total_offset = total;
        self.dirty = false;
    }

    pub fn cursor(&mut self) -> MappingCursor<'_> {
        self.compile();
        MappingCursor {
            starts: &self.compiled_starts,
            offsets: &self.compiled_offsets,
            old_sizes: &self.compiled_old_sizes,
            new_sizes: &self.compiled_new_sizes,
            total_offset: self.total_offset,
            cursor: 0,
        }
    }

    /// Storyteller `Mapping.invert` — map positions from after mutations back
    /// to the original content (swaps old/new sizes at compiled output positions).
    pub fn invert(&mut self) -> &Mapping {
        if self.inverted.is_none() {
            self.compile();
            let mut inverted = Mapping::new();
            for i in 0..self.starts.len() {
                let position = self.compiled_starts[i];
                let offset = self.compiled_offsets[i];
                let old_size = self.compiled_old_sizes[i];
                let new_size = self.compiled_new_sizes[i];
                let inverted_pos = position + offset;
                if inverted.starts.last().copied() == Some(inverted_pos) {
                    let last = inverted.starts.len() - 1;
                    let existing_old = inverted.old_sizes[last];
                    let existing_new = inverted.new_sizes[last];
                    // Stack inverted deletions that resolve to the same position.
                    if existing_old == 0 && new_size == 0 {
                        inverted.old_sizes[last] = 0;
                        inverted.new_sizes[last] = old_size + existing_new;
                        continue;
                    }
                }
                inverted.starts.push(inverted_pos);
                inverted.old_sizes.push(new_size);
                inverted.new_sizes.push(old_size);
            }
            self.inverted = Some(Box::new(inverted));
        }
        self.inverted.as_ref().expect("populated above")
    }

    /// Cursor over [`Self::invert`] (ST `invert().cursor()`).
    pub fn invert_cursor(&mut self) -> MappingCursor<'_> {
        let _ = self.invert();
        self.inverted
            .as_mut()
            .expect("invert just populated")
            .cursor()
    }

    fn entries(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        self.starts
            .iter()
            .copied()
            .zip(self.old_sizes.iter().copied())
            .zip(self.new_sizes.iter().copied())
            .map(|((start, old_size), new_size)| (start, old_size, new_size))
    }

    /// Storyteller `appendMapping` — rebase `other`'s mutations (in this
    /// mapping's output space) into the original space and merge.
    pub fn append_mapping(&mut self, other: &Mapping) {
        let mut rebased_starts = Vec::new();
        let mut rebased_old_sizes = Vec::new();
        let mut rebased_deltas = Vec::new();
        {
            let _ = self.invert();
            let mut inverted = self
                .inverted
                .as_mut()
                .expect("invert just populated")
                .cursor();
            for (start, old_size, new_size) in other.entries() {
                let orig_start = inverted.map(start, MapAssoc::Start);
                let orig_end = inverted.map(start + old_size, MapAssoc::End);
                rebased_starts.push(orig_start);
                rebased_old_sizes.push((orig_end - orig_start).max(0));
                rebased_deltas.push(new_size - old_size);
            }
        }

        let existing_starts = self.starts.clone();
        let existing_old = self.old_sizes.clone();
        let existing_new = self.new_sizes.clone();

        let mut starts = Vec::new();
        let mut old_sizes = Vec::new();
        let mut new_sizes = Vec::new();
        let mut i = 0usize;
        let mut j = 0usize;
        while i < existing_starts.len() || j < rebased_starts.len() {
            let take_existing = j >= rebased_starts.len()
                || (i < existing_starts.len() && existing_starts[i] < rebased_starts[j]);
            if take_existing {
                starts.push(existing_starts[i]);
                old_sizes.push(existing_old[i]);
                new_sizes.push(existing_new[i]);
                i += 1;
                continue;
            }
            let mut start = rebased_starts[j];
            let mut end = start + rebased_old_sizes[j];
            let mut delta = rebased_deltas[j];
            j += 1;
            while !starts.is_empty() {
                let prev_start = *starts.last().expect("non-empty");
                let prev_old = *old_sizes.last().expect("non-empty");
                let overlaps =
                    prev_start + prev_old > start || (prev_old == 0 && prev_start == start);
                if !overlaps {
                    break;
                }
                start = start.min(prev_start);
                end = end.max(prev_start + prev_old);
                let prev_new = new_sizes.pop().expect("paired");
                let prev_o = old_sizes.pop().expect("paired");
                starts.pop();
                delta += prev_new - prev_o;
            }
            while i < existing_starts.len()
                && (existing_starts[i] < end || (existing_starts[i] == end && existing_old[i] == 0))
            {
                end = end.max(existing_starts[i] + existing_old[i]);
                delta += existing_new[i] - existing_old[i];
                i += 1;
            }
            let old_size = end - start;
            starts.push(start);
            old_sizes.push(old_size);
            new_sizes.push(old_size + delta);
        }
        self.starts = starts;
        self.old_sizes = old_sizes;
        self.new_sizes = new_sizes;
        self.dirty = true;
        self.inverted = None;
    }
}

pub struct MappingCursor<'a> {
    starts: &'a [i32],
    offsets: &'a [i32],
    old_sizes: &'a [i32],
    new_sizes: &'a [i32],
    total_offset: i32,
    cursor: usize,
}

impl MappingCursor<'_> {
    pub fn map(&mut self, position: i32, assoc: MapAssoc) -> i32 {
        let n = self.starts.len();
        let mut i = self.cursor;

        if i < n && self.starts[i] < position {
            while i < n && self.starts[i] < position {
                i += 1;
            }
        } else if i > 0 && self.starts[i - 1] >= position {
            i = lower_bound(position, self.starts);
        }
        self.cursor = i;

        if i > 0
            && self.old_sizes[i - 1] > self.new_sizes[i - 1]
            && position < self.starts[i - 1] + self.old_sizes[i - 1]
        {
            let clamped = self.starts[i - 1] + self.offsets[i - 1];
            return clamped
                + if assoc == MapAssoc::Start {
                    0
                } else {
                    self.new_sizes[i - 1]
                };
        }

        if i == n {
            return position + self.total_offset;
        }

        let start = self.starts[i];
        let offset = self.offsets[i];
        let insertion = self.old_sizes[i] == 0;

        if !insertion || position < start {
            return position + offset;
        }

        position
            + offset
            + if assoc == MapAssoc::Start {
                0
            } else {
                self.new_sizes[i]
            }
    }
}

fn lower_bound(position: i32, starts: &[i32]) -> usize {
    let mut low = 0usize;
    let mut high = starts.len();
    while low < high {
        let mid = (low + high) / 2;
        if starts[mid] < position {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_map_expand_and_cursor() {
        let mut m = Mapping::new();
        m.insert_map(0, 1, 3);
        m.insert_map(2, 1, 4);
        let mut c = m.cursor();
        assert_eq!(c.map(0, MapAssoc::Start), 0);
        assert_eq!(c.map(1, MapAssoc::Start), 3);
        assert_eq!(c.map(2, MapAssoc::Start), 4);
    }

    #[test]
    fn invert_maps_expanded_positions_back() {
        // "a" → "aaa" at 0: original 0..1 becomes 0..3.
        let mut m = Mapping::new();
        m.insert_map(0, 1, 3);
        assert_eq!(m.cursor().map(0, MapAssoc::Start), 0);
        assert_eq!(m.cursor().map(1, MapAssoc::Start), 3);

        let mut c = m.invert_cursor();
        // New-space start → original 0; past expansion → original 1.
        assert_eq!(c.map(0, MapAssoc::Start), 0);
        assert_eq!(c.map(3, MapAssoc::Start), 1);
    }

    #[test]
    fn append_mapping_rebases_and_composes() {
        // Stage 1: expand "a" → "aaa" at 0. Stage 2 (output space): delete
        // the three expanded chars → empty. Composed: original 0..1 → 0.
        let mut base = Mapping::new();
        base.insert_map(0, 1, 3);
        let mut stage2 = Mapping::new();
        stage2.insert_map(0, 3, 0);
        base.append_mapping(&stage2);
        let mut c = base.cursor();
        assert_eq!(c.map(0, MapAssoc::Start), 0);
        assert_eq!(c.map(1, MapAssoc::Start), 0);
    }
}
