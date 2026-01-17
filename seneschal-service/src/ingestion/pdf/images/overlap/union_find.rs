//! Union-Find data structure for grouping overlapping items.
//!
//! This module provides an efficient disjoint-set data structure with path
//! compression and union by rank for grouping images that overlap with each
//! other or share overlapping content regions.

/// Union-Find data structure for grouping overlapping items.
///
/// Uses path compression and union by rank for efficient operations.
pub(super) struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    /// Create a new Union-Find structure with `size` elements.
    ///
    /// Initially, each element is in its own group.
    pub fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    /// Find the root of the group containing element `x`.
    ///
    /// Uses path compression to flatten the tree structure.
    pub fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]); // Path compression
        }
        self.parent[x]
    }

    /// Union the groups containing elements `x` and `y`.
    ///
    /// Uses union by rank to keep the tree balanced.
    pub fn union(&mut self, x: usize, y: usize) {
        let root_x = self.find(x);
        let root_y = self.find(y);

        if root_x != root_y {
            // Union by rank
            match self.rank[root_x].cmp(&self.rank[root_y]) {
                std::cmp::Ordering::Less => self.parent[root_x] = root_y,
                std::cmp::Ordering::Greater => self.parent[root_y] = root_x,
                std::cmp::Ordering::Equal => {
                    self.parent[root_y] = root_x;
                    self.rank[root_x] += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_union_find() {
        let mut uf = UnionFind::new(5);

        // Initially all separate
        assert_ne!(uf.find(0), uf.find(1));

        // Union 0 and 1
        uf.union(0, 1);
        assert_eq!(uf.find(0), uf.find(1));

        // Union 2 and 3
        uf.union(2, 3);
        assert_eq!(uf.find(2), uf.find(3));

        // 0,1 and 2,3 still separate
        assert_ne!(uf.find(0), uf.find(2));

        // Union the groups via 1 and 2
        uf.union(1, 2);
        assert_eq!(uf.find(0), uf.find(3));
    }
}
