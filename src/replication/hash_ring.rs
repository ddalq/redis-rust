//! Consistent Hash Ring for Partitioned Sharding
//!
//! Implements Anna KVS-style consistent hashing with virtual nodes.
//! Keys are mapped to N replicas based on their position on the ring.

use super::lattice::ReplicaId;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

/// Virtual node on the consistent hash ring
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VirtualNode {
    /// The physical node this virtual node belongs to
    pub physical_node: ReplicaId,
    /// Index of this virtual node (0..virtual_nodes_per_physical)
    pub virtual_index: u32,
}

impl VirtualNode {
    pub fn new(physical_node: ReplicaId, virtual_index: u32) -> Self {
        VirtualNode {
            physical_node,
            virtual_index,
        }
    }
}

/// Consistent hash ring for key-to-node mapping
#[derive(Debug, Clone)]
pub struct HashRing {
    /// Sorted list of (ring_position, virtual_node)
    ring: Vec<(u64, VirtualNode)>,
    /// Number of virtual nodes per physical node
    virtual_nodes_per_physical: u32,
    /// Replication factor (number of replicas per key)
    replication_factor: usize,
    /// Physical nodes in the cluster
    physical_nodes: Vec<ReplicaId>,
    /// Ring version (incremented on membership changes)
    version: u64,
}

impl HashRing {
    /// Create a new hash ring with the given nodes and configuration
    pub fn new(
        nodes: Vec<ReplicaId>,
        virtual_nodes_per_physical: u32,
        replication_factor: usize,
    ) -> Self {
        let mut ring = HashRing {
            ring: Vec::new(),
            virtual_nodes_per_physical,
            replication_factor,
            physical_nodes: Vec::new(),
            version: 0,
        };

        for node in nodes {
            ring.add_node(node);
        }

        ring
    }

    /// Create a hash ring with default settings (150 virtual nodes, RF=3)
    pub fn with_defaults(nodes: Vec<ReplicaId>) -> Self {
        Self::new(nodes, 150, 3)
    }

    /// Hash a key to a position on the ring
    fn hash_key(key: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    /// Hash a virtual node to a position on the ring
    fn hash_virtual_node(node: ReplicaId, virtual_index: u32) -> u64 {
        let mut hasher = DefaultHasher::new();
        node.0.hash(&mut hasher);
        virtual_index.hash(&mut hasher);
        hasher.finish()
    }

    /// Add a physical node to the ring
    pub fn add_node(&mut self, node: ReplicaId) {
        if self.physical_nodes.contains(&node) {
            return; // Already in ring
        }

        self.physical_nodes.push(node);

        // Add virtual nodes
        for i in 0..self.virtual_nodes_per_physical {
            let vnode = VirtualNode::new(node, i);
            let position = Self::hash_virtual_node(node, i);
            self.ring.push((position, vnode));
        }

        // Sort by position
        self.ring.sort_by_key(|(pos, _)| *pos);
        self.version += 1;
    }

    /// Remove a physical node from the ring
    pub fn remove_node(&mut self, node: ReplicaId) {
        self.physical_nodes.retain(|n| *n != node);
        self.ring.retain(|(_, vnode)| vnode.physical_node != node);
        self.version += 1;
    }

    /// Get the N nodes responsible for this key (in preference order)
    pub fn get_replicas(&self, key: &str) -> Vec<ReplicaId> {
        self.get_replicas_with_rf(key, self.replication_factor)
    }

    /// Get nodes responsible for this key with custom replication factor
    ///
    /// This allows hot keys to have higher RF than normal keys.
    /// The RF is capped at the number of physical nodes.
    pub fn get_replicas_with_rf(&self, key: &str, rf: usize) -> Vec<ReplicaId> {
        if self.ring.is_empty() {
            return vec![];
        }

        let key_pos = Self::hash_key(key);
        let n = rf.min(self.physical_nodes.len());

        // Binary search for first position >= key_pos
        let start_idx = match self.ring.binary_search_by_key(&key_pos, |(pos, _)| *pos) {
            Ok(i) => i,
            Err(i) => i % self.ring.len(),
        };

        // Walk clockwise collecting unique physical nodes
        let mut replicas = Vec::with_capacity(n);
        let mut seen = HashSet::new();
        let mut idx = start_idx;
        let ring_len = self.ring.len();

        while replicas.len() < n && seen.len() < self.physical_nodes.len() {
            let (_, vnode) = &self.ring[idx % ring_len];
            if !seen.contains(&vnode.physical_node) {
                seen.insert(vnode.physical_node);
                replicas.push(vnode.physical_node);
            }
            idx += 1;
            if idx - start_idx >= ring_len {
                break; // Wrapped around
            }
        }

        replicas
    }

    /// Check if this node should store the key with custom RF
    pub fn is_responsible_with_rf(&self, key: &str, node: ReplicaId, rf: usize) -> bool {
        self.get_replicas_with_rf(key, rf).contains(&node)
    }

    /// Check if this node should store the key
    pub fn is_responsible(&self, key: &str, node: ReplicaId) -> bool {
        self.get_replicas(key).contains(&node)
    }

    /// Get the primary (first) replica for a key
    pub fn get_primary(&self, key: &str) -> Option<ReplicaId> {
        self.get_replicas(key).into_iter().next()
    }

    /// Get all nodes that need deltas for a key (excluding sender)
    pub fn get_gossip_targets(&self, key: &str, sender: ReplicaId) -> Vec<ReplicaId> {
        self.get_replicas(key)
            .into_iter()
            .filter(|r| *r != sender)
            .collect()
    }

    /// Get the current ring version
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Get the number of physical nodes
    pub fn node_count(&self) -> usize {
        self.physical_nodes.len()
    }

    /// Get all physical nodes
    pub fn nodes(&self) -> &[ReplicaId] {
        &self.physical_nodes
    }

    /// Get the replication factor
    pub fn replication_factor(&self) -> usize {
        self.replication_factor
    }

    /// Check if a node is in the ring
    pub fn contains_node(&self, node: ReplicaId) -> bool {
        self.physical_nodes.contains(&node)
    }

    /// Get statistics about key distribution (for debugging)
    pub fn get_distribution_stats(&self, sample_keys: &[&str]) -> DistributionStats {
        let mut node_counts: std::collections::HashMap<ReplicaId, usize> =
            std::collections::HashMap::new();

        for key in sample_keys {
            for replica in self.get_replicas(key) {
                *node_counts.entry(replica).or_insert(0) += 1;
            }
        }

        let counts: Vec<usize> = node_counts.values().cloned().collect();
        let total: usize = counts.iter().sum();
        let mean = if counts.is_empty() {
            0.0
        } else {
            total as f64 / counts.len() as f64
        };

        let variance = if counts.len() > 1 {
            counts
                .iter()
                .map(|&c| (c as f64 - mean).powi(2))
                .sum::<f64>()
                / counts.len() as f64
        } else {
            0.0
        };

        DistributionStats {
            total_assignments: total,
            min_per_node: counts.iter().cloned().min().unwrap_or(0),
            max_per_node: counts.iter().cloned().max().unwrap_or(0),
            mean_per_node: mean,
            std_dev: variance.sqrt(),
        }
    }
}

/// Statistics about key distribution across nodes
#[derive(Debug, Clone)]
pub struct DistributionStats {
    pub total_assignments: usize,
    pub min_per_node: usize,
    pub max_per_node: usize,
    pub mean_per_node: f64,
    pub std_dev: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_ring_basic() {
        let nodes = vec![ReplicaId::new(1), ReplicaId::new(2), ReplicaId::new(3)];
        let ring = HashRing::new(nodes, 10, 3);

        assert_eq!(ring.node_count(), 3);
        assert_eq!(ring.replication_factor(), 3);
    }

    #[test]
    fn test_get_replicas() {
        let nodes = vec![
            ReplicaId::new(1),
            ReplicaId::new(2),
            ReplicaId::new(3),
            ReplicaId::new(4),
            ReplicaId::new(5),
        ];
        let ring = HashRing::new(nodes, 50, 3);

        let replicas = ring.get_replicas("test_key");
        assert_eq!(replicas.len(), 3);

        // All replicas should be unique
        let unique: HashSet<_> = replicas.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn test_is_responsible() {
        let nodes = vec![ReplicaId::new(1), ReplicaId::new(2), ReplicaId::new(3)];
        let ring = HashRing::new(nodes, 50, 2);

        let replicas = ring.get_replicas("my_key");

        // Exactly 2 nodes should be responsible
        let responsible_count = (1..=3)
            .filter(|&id| ring.is_responsible("my_key", ReplicaId::new(id)))
            .count();
        assert_eq!(responsible_count, 2);

        // Check consistency with get_replicas
        for r in &replicas {
            assert!(ring.is_responsible("my_key", *r));
        }
    }

    #[test]
    fn test_deterministic_placement() {
        let nodes = vec![ReplicaId::new(1), ReplicaId::new(2), ReplicaId::new(3)];
        let ring1 = HashRing::new(nodes.clone(), 50, 3);
        let ring2 = HashRing::new(nodes, 50, 3);

        // Same key should map to same replicas
        assert_eq!(ring1.get_replicas("key1"), ring2.get_replicas("key1"));
        assert_eq!(ring1.get_replicas("key2"), ring2.get_replicas("key2"));
    }

    #[test]
    fn test_add_remove_node() {
        let mut ring = HashRing::new(vec![ReplicaId::new(1), ReplicaId::new(2)], 50, 2);

        let v1 = ring.version();
        ring.add_node(ReplicaId::new(3));
        assert_eq!(ring.node_count(), 3);
        assert!(ring.version() > v1);

        ring.remove_node(ReplicaId::new(2));
        assert_eq!(ring.node_count(), 2);
        assert!(ring.contains_node(ReplicaId::new(1)));
        assert!(!ring.contains_node(ReplicaId::new(2)));
        assert!(ring.contains_node(ReplicaId::new(3)));
    }

    #[test]
    fn test_get_gossip_targets() {
        let nodes = vec![ReplicaId::new(1), ReplicaId::new(2), ReplicaId::new(3)];
        let ring = HashRing::new(nodes, 50, 3);

        let targets = ring.get_gossip_targets("key", ReplicaId::new(1));

        // Should not include sender
        assert!(
            !targets.contains(&ReplicaId::new(1)) || !ring.is_responsible("key", ReplicaId::new(1))
        );

        // Should be a subset of replicas
        let replicas = ring.get_replicas("key");
        for t in &targets {
            assert!(replicas.contains(t));
        }
    }

    #[test]
    fn test_distribution_balance() {
        let nodes: Vec<_> = (1..=5).map(|i| ReplicaId::new(i)).collect();
        let ring = HashRing::new(nodes, 150, 3);

        // Generate sample keys
        let keys: Vec<String> = (0..1000).map(|i| format!("key_{}", i)).collect();
        let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();

        let stats = ring.get_distribution_stats(&key_refs);

        // With 1000 keys, RF=3, 5 nodes: each node should have ~600 assignments
        // Allow for some variance (within 2 std devs of expected)
        let expected_per_node = (1000 * 3) as f64 / 5.0;
        assert!(
            stats.mean_per_node > expected_per_node * 0.8,
            "Mean {} too low (expected ~{})",
            stats.mean_per_node,
            expected_per_node
        );
        assert!(
            stats.mean_per_node < expected_per_node * 1.2,
            "Mean {} too high (expected ~{})",
            stats.mean_per_node,
            expected_per_node
        );

        // Standard deviation should be reasonable (< 20% of mean)
        assert!(
            stats.std_dev < stats.mean_per_node * 0.2,
            "Distribution too uneven: std_dev={}, mean={}",
            stats.std_dev,
            stats.mean_per_node
        );
    }

    #[test]
    fn test_replication_factor_bounds() {
        // RF > node count should be capped
        let nodes = vec![ReplicaId::new(1), ReplicaId::new(2)];
        let ring = HashRing::new(nodes, 50, 5);

        let replicas = ring.get_replicas("key");
        assert_eq!(replicas.len(), 2); // Capped at node count
    }

    #[test]
    fn test_empty_ring() {
        let ring = HashRing::new(vec![], 50, 3);

        assert!(ring.get_replicas("key").is_empty());
        assert!(!ring.is_responsible("key", ReplicaId::new(1)));
    }

    #[test]
    fn test_single_node() {
        let ring = HashRing::new(vec![ReplicaId::new(1)], 50, 3);

        let replicas = ring.get_replicas("key");
        assert_eq!(replicas.len(), 1);
        assert_eq!(replicas[0], ReplicaId::new(1));
        assert!(ring.is_responsible("key", ReplicaId::new(1)));
    }

    #[test]
    fn test_per_key_replication_factor() {
        let nodes: Vec<_> = (1..=5).map(|i| ReplicaId::new(i)).collect();
        let ring = HashRing::new(nodes, 50, 3); // Default RF=3

        // Normal key gets RF=3
        let normal_replicas = ring.get_replicas("normal_key");
        assert_eq!(normal_replicas.len(), 3);

        // Hot key can get RF=5
        let hot_replicas = ring.get_replicas_with_rf("hot_key", 5);
        assert_eq!(hot_replicas.len(), 5);

        // All replicas should be unique
        let unique: HashSet<_> = hot_replicas.iter().collect();
        assert_eq!(unique.len(), 5);

        // RF > node count is capped
        let over_replicas = ring.get_replicas_with_rf("key", 10);
        assert_eq!(over_replicas.len(), 5); // Capped at 5 nodes
    }

    #[test]
    fn test_is_responsible_with_rf() {
        let nodes: Vec<_> = (1..=5).map(|i| ReplicaId::new(i)).collect();
        let ring = HashRing::new(nodes, 50, 3);

        // With higher RF, more nodes become responsible
        let responsible_rf3: Vec<_> = (1..=5)
            .filter(|&id| ring.is_responsible("key", ReplicaId::new(id)))
            .collect();
        let responsible_rf5: Vec<_> = (1..=5)
            .filter(|&id| ring.is_responsible_with_rf("key", ReplicaId::new(id), 5))
            .collect();

        assert_eq!(responsible_rf3.len(), 3);
        assert_eq!(responsible_rf5.len(), 5);

        // RF3 nodes should be subset of RF5 nodes
        for id in &responsible_rf3 {
            assert!(responsible_rf5.contains(id));
        }
    }
}
