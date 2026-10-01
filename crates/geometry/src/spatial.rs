//! Spatial index over bounding boxes (R*-tree via `rstar`).
//!
//! Pointer hit-testing and snapping query only nearby candidates instead of
//! scanning every entity on each pointer move.

use std::collections::BTreeMap;

use rstar::{AABB, PointDistance, RTree, RTreeObject};

use crate::{Aabb, Point};

#[derive(Debug, Clone, PartialEq)]
struct Item<K> {
    key: K,
    bbox: Aabb,
}

impl<K> RTreeObject for Item<K> {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners([self.bbox.min.x, self.bbox.min.y], [self.bbox.max.x, self.bbox.max.y])
    }
}

impl<K> PointDistance for Item<K> {
    fn distance_2(&self, p: &[f64; 2]) -> f64 {
        let d = self.bbox.distance_to_point(Point::new(p[0], p[1]));
        d * d
    }
}

/// Bounding-box index keyed by `K`.
#[derive(Debug, Clone)]
pub struct SpatialIndex<K: Ord + Copy> {
    tree: RTree<Item<K>>,
    boxes: BTreeMap<K, Aabb>,
}

impl<K: Ord + Copy> Default for SpatialIndex<K> {
    fn default() -> Self {
        Self { tree: RTree::new(), boxes: BTreeMap::new() }
    }
}

fn usable(b: Aabb) -> bool {
    !b.is_empty() && b.min.is_finite() && b.max.is_finite()
}

impl<K: Ord + Copy + PartialEq> SpatialIndex<K> {
    /// Empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bulk-load (much faster than repeated inserts). Empty boxes are skipped.
    #[must_use]
    pub fn bulk_load(items: Vec<(K, Aabb)>) -> Self {
        let mut boxes = BTreeMap::new();
        let mut list = Vec::with_capacity(items.len());
        for (k, b) in items {
            if usable(b) {
                boxes.insert(k, b);
            }
        }
        for (k, b) in &boxes {
            list.push(Item { key: *k, bbox: *b });
        }
        Self { tree: RTree::bulk_load(list), boxes }
    }

    /// Number of indexed keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// Whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// Insert or replace the box of `key`. Empty boxes remove the key.
    pub fn upsert(&mut self, key: K, bbox: Aabb) {
        self.remove(key);
        if usable(bbox) {
            self.tree.insert(Item { key, bbox });
            self.boxes.insert(key, bbox);
        }
    }

    /// Remove `key` if present.
    pub fn remove(&mut self, key: K) {
        if let Some(b) = self.boxes.remove(&key) {
            self.tree.remove(&Item { key, bbox: b });
        }
    }

    /// Stored box of `key`.
    #[must_use]
    pub fn get(&self, key: K) -> Option<Aabb> {
        self.boxes.get(&key).copied()
    }

    /// Keys whose boxes intersect `r`, sorted.
    #[must_use]
    pub fn query_rect(&self, r: Aabb) -> Vec<K> {
        if r.is_empty() {
            return Vec::new();
        }
        let env = AABB::from_corners([r.min.x, r.min.y], [r.max.x, r.max.y]);
        let mut out: Vec<K> = self.tree.locate_in_envelope_intersecting(env).map(|i| i.key).collect();
        out.sort();
        out
    }

    /// Keys whose boxes lie within `radius` of `p`, nearest first.
    #[must_use]
    pub fn query_point(&self, p: Point, radius: f64) -> Vec<K> {
        if !p.is_finite() || radius.is_nan() || radius < 0.0 {
            return Vec::new();
        }
        self.tree
            .nearest_neighbor_iter_with_distance_2([p.x, p.y])
            .take_while(|(_, d2)| *d2 <= radius * radius)
            .map(|(i, _)| i.key)
            .collect()
    }

    /// Union of all boxes.
    #[must_use]
    pub fn extent(&self) -> Aabb {
        self.boxes.values().fold(Aabb::EMPTY, |a, b| a.union(*b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(x: f64, y: f64) -> Aabb {
        Aabb::from_corners(Point::new(x, y), Point::new(x + 1.0, y + 1.0))
    }

    #[test]
    fn queries_match_bruteforce() {
        let items: Vec<(u32, Aabb)> =
            (0..400).map(|i| (i, bx(f64::from(i % 20) * 3.0, f64::from(i / 20) * 3.0))).collect();
        let mut idx = SpatialIndex::bulk_load(items.clone());
        let q = Aabb::from_corners(Point::new(10.0, 10.0), Point::new(20.5, 14.0));
        let mut brute: Vec<u32> = items.iter().filter(|(_, b)| b.intersects(q)).map(|(k, _)| *k).collect();
        brute.sort();
        assert_eq!(idx.query_rect(q), brute);

        // Moving key 5 far away removes it from its old neighbourhood.
        let old = idx.get(5).unwrap();
        assert!(idx.query_rect(old).contains(&5));
        idx.upsert(5, bx(1000.0, 1000.0));
        assert!(!idx.query_rect(old).contains(&5));
        assert_eq!(idx.query_point(Point::new(1000.5, 1000.5), 0.0), vec![5]);
        idx.remove(5);
        assert!(idx.query_point(Point::new(1000.5, 1000.5), 0.0).is_empty());
        assert_eq!(idx.len(), 399);
    }

    #[test]
    fn point_query_sorted_by_distance() {
        let idx = SpatialIndex::bulk_load(vec![(1u8, bx(0.0, 0.0)), (2, bx(5.0, 0.0)), (3, bx(50.0, 0.0))]);
        assert_eq!(idx.query_point(Point::new(3.0, 0.5), 3.0), vec![1, 2]);
        assert!(idx.query_point(Point::new(f64::NAN, 0.0), 3.0).is_empty());
    }

    #[test]
    fn empty_boxes_are_not_indexed() {
        let mut idx = SpatialIndex::new();
        idx.upsert(1u8, Aabb::EMPTY);
        assert!(idx.is_empty());
    }
}
