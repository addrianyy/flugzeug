#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

/// Inclusive range.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Range {
    pub start: u64,
    pub end:   u64,
}

impl Range {
    pub fn size(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// Set of inclusive ranges.
#[derive(Clone)]
#[repr(C)]
pub struct RangeSet {
    ranges: [Range; 256],
    used:   u32,
}

impl RangeSet {
    pub const fn new() -> Self {
        Self {
            ranges: [Range { start: 0, end: 0 }; 256],
            used:   0,
        }
    }

    pub fn entries(&self) -> &[Range] {
        &self.ranges[..self.used as usize]
    }

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

    fn add_entry(&mut self, range: Range) {
        assert!((self.used as usize) < self.ranges.len(), "Out of space in `RangeSet`.");

        self.ranges[self.used as usize] = range;
        self.used += 1;
    }

    fn delete_entry(&mut self, to_delete: usize) {
        assert!(to_delete < self.used as usize, "Index to delete is out of bounds.");

        // Copy entry to delete to the end of the list.
        for idx in to_delete..self.used as usize - 1 {
            self.ranges.swap(idx, idx + 1);
        }

        // Free last entry.
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
    fn test() {
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

}
