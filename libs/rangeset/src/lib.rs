#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

/// An inclusive range.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Range {
    pub start: u64,
    pub end:   u64,
}

impl Range {
    /// Get the size of this range.
    pub fn size(&self) -> u64 {
        // Add one because range is inclusive.
        self.end - self.start + 1
    }
}

/// Set of unique, inclusive ranges.
#[derive(Clone)]
#[repr(C)]
pub struct RangeSet {
    /// Static array of all available ranges. There cannot be more than 256 ranges. We need to do
    /// this because we can't import `alloc` crate and get `Vec`.
    ranges: [Range; 256],

    /// Number of ranges used in this set.
    used: u32,
}

impl RangeSet {
    /// Create a new, empty `RangeSet`.
    pub const fn new() -> Self {
        Self {
            ranges: [Range { start: 0, end: 0 }; 256],
            used:   0,
        }
    }

    /// Get all used ranges in this `RangeSet`.
    pub fn entries(&self) -> &[Range] {
        &self.ranges[..self.used as usize]
    }

    /// Insert inclusive range to `RangeSet`. This function will handle possible merges.
    pub fn insert(&mut self, mut range: Range) {
        assert!(range.start <= range.end, "Range to insert has invalid shape.");

        'merge_loop: loop {
            for idx in 0..self.used as usize {
                let current = self.ranges[idx];

                // If this entry doesn't overlap or touch the range to insert then we
                // can't do anything.
                if !overlaps(Range { start: range.start,   end: range.end.saturating_add(1) },
                             Range { start: current.start, end: current.end.saturating_add(1) }) {
                    continue;
                }

                // Make this range a combination of 2 overlaping entries.
                range.start = core::cmp::min(range.start, current.start);
                range.end   = core::cmp::max(range.end,   current.end);

                // This entry can be deleted, because `range` fully contains it now.
                self.delete_entry(idx);

                continue 'merge_loop;
            }
            
            // Stop if we haven't found any more merges in the whole set.
            break;
        }

        self.add_entry(range);
    }

    /// Remove inclusive range from `RangeSet`. This function will handle possible cutoffs and
    /// splits.
    pub fn remove(&mut self, range: Range) {
        assert!(range.start <= range.end, "Range to remove has invalid shape.");

        'subtract_loop: loop {
            for idx in 0..self.used as usize {
                let current = self.ranges[idx];

                // If this entry doesn't overlap the range to remove then we can't do anything.
                if !overlaps(range, current) {
                    continue;
                }
                
                // If this entry is entirely contained by the range to remove, then we
                // can just delete it.
                if contains(range, current) {
                    self.delete_entry(idx);

                    continue 'subtract_loop;
                }

                // There is only partial overlap.

                if range.start <= current.start {
                    // current:     XXXXXXX
                    // range:    YYYYYY
                    self.ranges[idx].start = range.end.saturating_add(1);
                } else if range.end >= current.end {
                    // current:  XXXXXXX
                    // range:       YYYYYYY
                    self.ranges[idx].end = range.start.saturating_sub(1);
                } else {
                    // current:  XXXXXXXXXXXXXXX
                    // range:        YYYYYY
                    
                    // Create right split entry.
                    self.ranges[idx].start = range.end.saturating_add(1);

                    // Create left split entry.
                    self.add_entry(Range {
                        start: current.start,
                        end:   range.start.saturating_sub(1),
                    });

                    continue 'subtract_loop;
                }
            }

            // Stop if we haven't found any more subtracts in the whole set.
            break;
        }
    }

    pub fn allocate(&mut self, size: u64, align: u64) -> Option<usize> {
        self.allocate_limited(size, align, None)
    }

    pub fn allocate_limited(&mut self, size: u64, align: u64, max_address: Option<u64>)
        -> Option<usize> 
    {
        // Zero-sized allocations are not allowed.
        if size == 0 {
            return None;
        }

        // Make sure that alignment is power of two.
        if align.count_ones() != 1 {
            return None;
        }

        // Calculate alignment mask, this can be done because alignment is always power of two.
        let align_mask = align - 1;

        let max_address = max_address.unwrap_or(usize::MAX as u64);
        let max_address = max_address.min(usize::MAX as u64);

        let mut allocation: Option<(u64, u32, u64, u64)> = None;

        // Try to find the best region for new allocation.
        for idx in 0..self.used as usize {
            let current = self.ranges[idx];

            // Calculate the amount of bytes required for front padding to satifsy
            // alignment requirements.
            let padding = (align - (current.start & align_mask)) & align_mask;

            // Calculate the actual end of allocation acounting for alignment.
            let actual_end = current.start.checked_add(size - 1)?.checked_add(padding)?;

            // Make sure that the allocation will fit in this region.
            if actual_end > current.end {
                continue;
            }

            // Make sure that this memory can be accessed by the processor in it's current state.
            // This library will be used by both 32 bit and 64 bit code.
            if actual_end > max_address {
                continue;
            }

            // Get the power of 2 of this region alignment.
            let region_align_power = current.start.trailing_zeros();

            // Check if this region is better than current best.
            let replace = if let Some(allocation) = allocation {
                if allocation.0 == padding {
                    // If both regions waste the same amount of space, pick one with
                    // smaller alignment.
                    allocation.1 > region_align_power
                } else {
                    // Choose region which wastes less space.
                    allocation.0 > padding
                }
            } else {
                true
            };

            // Current region is better then the previous best, replace it.
            if replace {
                allocation = Some((padding, region_align_power, current.start, actual_end));
            }
        }

        allocation.map(|(padding, _, start, end)| {
            // We found good region to allocate and it should be removed from the set.
            // Although padding space is not used, we will remove it too to avoid too big
            // fragmentation in the set.
            self.remove(Range { start, end });

            // Align address before returning it.
            (start + padding) as usize
        })
    }

    /// Push an entry to the `RangeSet`.
    fn add_entry(&mut self, range: Range) {
        // Make sure that static storage is enough to hold this many ranges.
        assert!((self.used as usize) < self.ranges.len(), "Out of space in `RangeSet`.");

        // Insert range to the end of the list.
        self.ranges[self.used as usize] = range;
        self.used += 1;
    }

    /// Delete an entry from the `RangeSet`.
    fn delete_entry(&mut self, to_delete: usize) {
        assert!(to_delete < self.used as usize, "Index to delete is out of bounds.");

        // Copy entry to delete to the end of the list.
        for idx in to_delete..self.used as usize - 1 {
            self.ranges.swap(idx, idx + 1);
        }

        // Free the last entry.
        self.used -= 1;
    }
}

/// Check if `a` overlaps `b`.
fn overlaps(a: Range, b: Range) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// Check if `a` contains `b`.
fn contains(a: Range, b: Range) -> bool {
    b.start >= a.start && b.end <= a.end
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::println;

    fn print_rs(rs: &RangeSet) {
        for entry in rs.entries() {
            println!("{:x} - {:x}", entry.start, entry.end);
        }
    }

    #[test]
    fn insert_remove_test() {
        let mut rs = RangeSet::new();

        rs.insert(Range { start: 0x1000, end: 0x1fff });

        print_rs(&rs);
        println!();

        rs.insert(Range { start: 0x2000, end: 0x3fff });
        rs.insert(Range { start: 0x8000, end: 0x9fff });

        print_rs(&rs);
        println!();

        rs.remove(Range { start: 0x7000, end: 0x10000 });

        print_rs(&rs);
        println!();

        rs.insert(Range { start: 0x8000, end: 0x9fff });
        rs.remove(Range { start: 0x7000, end: 0x8000 });

        print_rs(&rs);
        println!();

        rs.remove(Range { start: 0x9000, end: 0x10000 });

        print_rs(&rs);
        println!();

        rs.remove(Range { start: 0x8070, end: 0x8200 });

        print_rs(&rs);
        println!();

        rs.insert(Range { start: 0x800, end: 0x10000 });

        print_rs(&rs);
        println!();

        panic!("Done!");
    }

    #[test]
    fn allocate_test() {
        let mut rs = RangeSet::new();

        rs.insert(Range { start: 0x1000, end: 0x4000 });
        rs.insert(Range { start: 0x80000, end: 0x400000 });
        rs.insert(Range { start: 0x9999111100000, end: 0x9f99111100012 });

        println!("{:x?}", rs.allocate(0x1000, 0x100));
        print_rs(&rs);

        panic!("Done!");
    }
}
