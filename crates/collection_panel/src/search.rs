use suffix::SuffixTable;

/// A substring index built once with the collection tree. Binary search finds
/// the matching suffix interval without reading every request's search text.
pub(super) struct SearchIndex {
    suffixes: SuffixTable<'static, 'static>,
    rows_by_suffix: Vec<u32>,
    row_ends: Vec<usize>,
}

impl SearchIndex {
    pub fn new(documents: impl IntoIterator<Item = String>) -> Self {
        let mut text = String::new();
        let mut rows_by_offset = Vec::new();
        let mut row_ends = Vec::new();

        for (row, document) in documents.into_iter().enumerate() {
            text.push_str(&document.to_lowercase());
            row_ends.push(text.len());
            text.push('\0');
            rows_by_offset.resize(
                text.len(),
                u32::try_from(row).expect("too many search rows"),
            );
        }

        text.shrink_to_fit();
        row_ends.shrink_to_fit();
        let suffixes = SuffixTable::new(text);
        let rows_by_suffix = suffixes
            .table()
            .iter()
            .map(|&offset| rows_by_offset[offset as usize])
            .collect();

        Self {
            suffixes,
            rows_by_suffix,
            row_ends,
        }
    }

    /// Returns unique row IDs in tree order. Work after lookup depends on the
    /// number of matching occurrences, rather than the number of tree items.
    pub fn matching_rows(&self, query: &str) -> Vec<usize> {
        if query.is_empty() {
            return Vec::new();
        }

        let query = query.to_lowercase();
        let table = self.suffixes.table();
        let text = self.suffixes.text().as_bytes();
        let compare = |&offset: &u32| {
            let suffix = &text[offset as usize..];
            suffix[..suffix.len().min(query.len())].cmp(query.as_bytes())
        };
        let start = table.partition_point(|offset| compare(offset).is_lt());
        let count = table[start..].partition_point(|offset| compare(offset).is_eq());
        let check_boundary = query.as_bytes().contains(&0);

        // The row IDs share the suffix order, so matching occurrences are read
        // contiguously instead of probing an owner array at scattered offsets.
        let matches = (start..start + count).filter_map(|index| {
            let row = self.rows_by_suffix[index] as usize;

            // Even a query containing NUL must not match across document boundaries.
            (!check_boundary
                || query.len() <= self.row_ends[row].saturating_sub(table[index] as usize))
            .then_some(row)
        });

        // Common short queries can match most rows, often several times each.
        // A bitmap avoids sorting that large occurrence list. Its word count is
        // bounded by the number of matches; sparse queries never scan it.
        let word_count = self.row_ends.len().div_ceil(64);
        if count > word_count {
            let mut bits = vec![0_u64; word_count];
            for row in matches {
                bits[row / 64] |= 1 << (row % 64);
            }

            let mut rows = Vec::new();
            for (word, mut bits) in bits.into_iter().enumerate() {
                while bits != 0 {
                    rows.push(word * 64 + bits.trailing_zeros() as usize);
                    bits &= bits - 1;
                }
            }

            return rows;
        }

        let mut rows: Vec<_> = matches.collect();
        rows.sort_unstable();
        rows.dedup();

        rows
    }

    #[cfg(test)]
    pub fn document(&self, row: usize) -> &str {
        let start = if row == 0 {
            0
        } else {
            self.row_ends[row - 1] + 1
        };

        &self.suffixes.text()[start..self.row_ends[row]]
    }

    #[cfg(test)]
    pub fn memory_bytes(&self) -> usize {
        self.suffixes.text().len()
            + std::mem::size_of_val(self.suffixes.table())
            + self.rows_by_suffix.capacity() * std::mem::size_of::<u32>()
            + self.row_ends.capacity() * std::mem::size_of::<usize>()
    }
}
