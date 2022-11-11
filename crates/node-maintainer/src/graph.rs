use std::{
    collections::{HashMap, VecDeque},
    ops::{Index, IndexMut},
};

use kdl::KdlDocument;
use nassun::PackageResolution;
use petgraph::{
    dot::Dot,
    stable_graph::{NodeIndex, StableGraph},
};
use unicase::UniCase;

use crate::{DepType, Edge, Node, NodeMaintainerError, PackageNode, ResolvedTree};

#[derive(Debug, Default)]
pub struct Graph {
    pub(crate) root: NodeIndex,
    pub(crate) inner: StableGraph<Node, Edge>,
}

impl Index<NodeIndex> for Graph {
    type Output = Node;

    fn index(&self, index: NodeIndex) -> &Self::Output {
        &self.inner[index]
    }
}

impl IndexMut<NodeIndex> for Graph {
    fn index_mut(&mut self, index: NodeIndex) -> &mut Self::Output {
        &mut self.inner[index]
    }
}

impl Graph {
    pub fn to_resolved_tree(&self) -> ResolvedTree {
        let root = self.node_pkg_node(self.root, true);
        let packages = self
            .inner
            .node_indices()
            .filter(|idx| *idx != self.root)
            .map(|idx| self.node_pkg_node(idx, false))
            .collect();
        ResolvedTree {
            version: 1,
            root,
            packages,
        }
    }

    pub fn to_kdl(&self) -> KdlDocument {
        self.to_resolved_tree().to_kdl()
    }

    pub fn render(&self) -> String {
        format!(
            "{:?}",
            Dot::new(&self.inner.map(
                |_, mut node| {
                    let resolved = node.package.resolved();
                    let mut label = node.package.name().to_string();
                    while let Some(node_idx) = &node.parent {
                        node = &self.inner[*node_idx];
                        let name = node.package.name();
                        label = format!("{name}/node_modules/{label}");
                    }
                    format!("{resolved:?} @ {label}")
                },
                |_, edge| { format!("{}", edge.requested) }
            ))
        )
    }

    pub(crate) fn find_by_name(
        &self,
        parent: NodeIndex,
        name: &UniCase<String>,
    ) -> Result<Option<NodeIndex>, NodeMaintainerError> {
        let mut parent = self.inner.node_weight(parent);
        while let Some(node) = parent {
            if node.children.contains_key(name) {
                return Ok(Some(node.children[name]));
            }
            parent = node.parent.and_then(|idx| self.inner.node_weight(idx));
        }
        Ok(None)
    }

    fn node_pkg_node(&self, node: NodeIndex, is_root: bool) -> PackageNode {
        let node = &self.inner[node];
        let mut path = VecDeque::new();
        path.push_front(UniCase::new(node.package.name().to_owned()));
        if !is_root {
            let mut parent = node.parent;
            while let Some(parent_idx) = parent {
                if parent_idx == self.root {
                    break;
                }
                path.push_front(UniCase::new(
                    self.inner[parent_idx].package.name().to_owned(),
                ));
                parent = self.inner[parent_idx].parent;
            }
        };
        let resolved = node.package.resolved();
        let version = if let &PackageResolution::Npm { version, .. } = &resolved {
            Some(version.clone())
        } else {
            None
        };

        let (resolved, integrity) = if is_root {
            (None, None)
        } else {
            let integrity = if let &PackageResolution::Npm { integrity, .. } = &resolved {
                integrity.clone()
            } else {
                None
            };
            (Some(resolved.clone()), integrity)
        };
        let mut prod_deps = HashMap::new();
        let mut dev_deps = HashMap::new();
        let mut peer_deps = HashMap::new();
        let mut opt_deps = HashMap::new();
        if !node.dependencies.is_empty() {
            let dependencies = node
                .dependencies
                .iter()
                .map(|(name, edge_idx)| {
                    let edge = &self.inner[*edge_idx];
                    (name, &edge.requested, &edge.dep_type)
                })
                .collect::<Vec<_>>();
            for (name, requested, dep_type) in dependencies {
                use DepType::*;
                let deps = match dep_type {
                    Prod => &mut prod_deps,
                    Dev => &mut dev_deps,
                    Peer => &mut peer_deps,
                    Opt => &mut opt_deps,
                };
                deps.insert(name.clone(), requested.clone());
            }
        }
        PackageNode {
            name: UniCase::new(node.package.name().to_string()),
            is_root,
            path: path.into(),
            resolved,
            version,
            integrity,
            dependencies: prod_deps,
            dev_dependencies: dev_deps,
            peer_dependencies: peer_deps,
            optional_dependencies: opt_deps,
        }
    }
}
