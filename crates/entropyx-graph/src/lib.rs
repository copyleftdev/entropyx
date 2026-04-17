//! Graph primitives for entropyx.
//!
//! v0.1 provides the **co-change graph** — nodes are file paths, edges
//! carry a weight equal to the number of commits in which both files
//! changed together. From this graph we compute:
//!
//!   - `weighted_degree(node)` — the sum of incident edge weights. Used
//!     as the degree half of `scan`'s C_s computation (the other half
//!     is betweenness — their max feeds the composite).
//!   - `betweenness_centrality()` — Brandes' algorithm (unweighted on
//!     the topology). Captures "bridge files": nodes whose removal
//!     would disconnect otherwise-isolated subgraphs. High betweenness
//!     = systemic blast radius. Normalized to `[0, 1]`.
//!
//! Future work: lineage graph (file identity across renames), ownership
//! bipartite graph (author ↔ file), weighted shortest-path betweenness,
//! community detection.

use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Default)]
pub struct CoChangeGraph {
    nodes: Vec<String>,
    node_index: BTreeMap<String, usize>,
    adj: Vec<BTreeMap<usize, u64>>,
}

impl CoChangeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a co-change graph from per-commit file-path lists. Each
    /// commit contributes a clique on its files — every pair increments
    /// the edge weight by 1. Deduplication inside a commit is the
    /// caller's responsibility.
    pub fn from_commit_paths<S: AsRef<str>>(per_commit: &[Vec<S>]) -> Self {
        let mut g = Self::new();
        for commit in per_commit {
            let idxs: Vec<usize> = commit.iter().map(|p| g.intern(p.as_ref())).collect();
            for i in 0..idxs.len() {
                for j in (i + 1)..idxs.len() {
                    g.bump_edge(idxs[i], idxs[j]);
                }
            }
        }
        g
    }

    fn intern(&mut self, path: &str) -> usize {
        if let Some(&i) = self.node_index.get(path) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(path.to_string());
        self.node_index.insert(path.to_string(), i);
        self.adj.push(BTreeMap::new());
        i
    }

    fn bump_edge(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        *self.adj[a].entry(b).or_insert(0) += 1;
        *self.adj[b].entry(a).or_insert(0) += 1;
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        // Each undirected edge shows up in two adjacency lists.
        self.adj.iter().map(|a| a.len()).sum::<usize>() / 2
    }

    pub fn nodes(&self) -> &[String] {
        &self.nodes
    }

    pub fn contains(&self, path: &str) -> bool {
        self.node_index.contains_key(path)
    }

    pub fn weighted_degree(&self, path: &str) -> u64 {
        match self.node_index.get(path) {
            Some(&i) => self.adj[i].values().sum(),
            None => 0,
        }
    }

    /// Brandes' algorithm for (unweighted) betweenness centrality.
    /// Returns a map from file path to its normalized betweenness in
    /// `[0, 1]`. Graphs with fewer than 3 nodes produce all-zero
    /// output (no "through" paths possible).
    ///
    /// Complexity: O(V·(V + E)). For V=1000, E=10k that's fine.
    pub fn betweenness_centrality(&self) -> BTreeMap<String, f64> {
        let n = self.nodes.len();
        let mut cb = vec![0.0_f64; n];

        for s in 0..n {
            let mut stack: Vec<usize> = Vec::new();
            let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
            let mut sigma: Vec<u64> = vec![0; n];
            let mut dist: Vec<i64> = vec![-1; n];
            sigma[s] = 1;
            dist[s] = 0;
            let mut queue: VecDeque<usize> = VecDeque::new();
            queue.push_back(s);
            while let Some(v) = queue.pop_front() {
                stack.push(v);
                for &w in self.adj[v].keys() {
                    if dist[w] < 0 {
                        dist[w] = dist[v] + 1;
                        queue.push_back(w);
                    }
                    if dist[w] == dist[v] + 1 {
                        sigma[w] += sigma[v];
                        preds[w].push(v);
                    }
                }
            }
            let mut delta: Vec<f64> = vec![0.0; n];
            while let Some(w) = stack.pop() {
                for &v in &preds[w] {
                    delta[v] += (sigma[v] as f64 / sigma[w] as f64) * (1.0 + delta[w]);
                }
                if w != s {
                    cb[w] += delta[w];
                }
            }
        }

        // Brandes double-counts on undirected graphs (each pair considered
        // from both sides). Normalize by (n-1)(n-2)/2 to land in [0, 1]
        // for the unweighted-undirected case.
        let denom = if n > 2 {
            ((n - 1) * (n - 2)) as f64
        } else {
            0.0
        };
        let mut out = BTreeMap::new();
        for (i, path) in self.nodes.iter().enumerate() {
            let normed = if denom > 0.0 { cb[i] / denom } else { 0.0 };
            out.insert(path.clone(), normed);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(paths: &[&[&str]]) -> CoChangeGraph {
        let owned: Vec<Vec<String>> = paths
            .iter()
            .map(|c| c.iter().map(|s| s.to_string()).collect())
            .collect();
        CoChangeGraph::from_commit_paths(&owned)
    }

    #[test]
    fn empty_input_yields_empty_graph() {
        let g: CoChangeGraph = CoChangeGraph::from_commit_paths(&[] as &[Vec<String>]);
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
        assert!(g.betweenness_centrality().is_empty());
    }

    #[test]
    fn solo_commits_have_no_edges() {
        let g = built(&[&["a"], &["b"], &["a"]]);
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 0);
        assert_eq!(g.weighted_degree("a"), 0);
        assert_eq!(g.weighted_degree("b"), 0);
    }

    #[test]
    fn clique_weights_accumulate() {
        // Three commits, each a 3-file clique on {a,b,c}.
        // Each pair fires thrice → edge weight 3. Weighted degree of
        // any node: 2 partners × 3 commits = 6.
        let g = built(&[&["a", "b", "c"], &["a", "b", "c"], &["a", "b", "c"]]);
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.edge_count(), 3);
        assert_eq!(g.weighted_degree("a"), 6);
        assert_eq!(g.weighted_degree("b"), 6);
        assert_eq!(g.weighted_degree("c"), 6);
    }

    #[test]
    fn path_graph_puts_betweenness_on_middle() {
        // a - b - c as two 2-cliques.
        let g = built(&[&["a", "b"], &["b", "c"]]);
        let bc = g.betweenness_centrality();
        // Only shortest path a↔c goes through b. Normalized for n=3:
        //   raw cb[b] = 2 (one from source a, one from source c)
        //   denom = (n-1)(n-2) = 2
        //   bc[b] = 1.0
        assert_eq!(bc["a"], 0.0);
        assert_eq!(bc["c"], 0.0);
        assert!((bc["b"] - 1.0).abs() < 1e-12, "bc[b] = {}", bc["b"]);
    }

    #[test]
    fn star_graph_puts_betweenness_on_hub() {
        // Hub `h` connected to leaves x, y, z via three 2-cliques.
        let g = built(&[&["h", "x"], &["h", "y"], &["h", "z"]]);
        let bc = g.betweenness_centrality();
        // Every leaf-leaf shortest path passes through h.
        //   3 leaf pairs (xy, xz, yz) → raw cb[h] = 2·3 = 6
        //   denom = (n-1)(n-2) = 3·2 = 6
        //   bc[h] = 1.0
        assert!((bc["h"] - 1.0).abs() < 1e-12, "bc[h] = {}", bc["h"]);
        for leaf in ["x", "y", "z"] {
            assert_eq!(bc[leaf], 0.0);
        }
    }

    #[test]
    fn triangle_has_uniform_zero_betweenness() {
        // Every pair is directly connected — no bridging needed.
        let g = built(&[&["a", "b", "c"]]);
        let bc = g.betweenness_centrality();
        for node in ["a", "b", "c"] {
            assert_eq!(bc[node], 0.0);
        }
    }

    #[test]
    fn tiny_graphs_give_zeroes() {
        let g = built(&[&["a", "b"]]);
        let bc = g.betweenness_centrality();
        assert_eq!(bc["a"], 0.0);
        assert_eq!(bc["b"], 0.0);
    }

    #[test]
    fn betweenness_order_invariant_in_commit_shuffling() {
        let a = built(&[&["a", "b"], &["b", "c"], &["c", "d"]]);
        let b = built(&[&["c", "d"], &["a", "b"], &["b", "c"]]);
        let ca = a.betweenness_centrality();
        let cb = b.betweenness_centrality();
        assert_eq!(ca, cb);
    }
}
