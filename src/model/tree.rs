//! The hierarchy: the workspace, its packages, each inside the package whose
//! directory holds it, and their modules. A package's node is its crate's root
//! module. A directory that is not a package is not a level.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Workspace,
    Package,
    Module,
}

#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub depth: u32,
    /// Test code, and so is everything inside it (`Tree::is_test`).
    pub test: bool,
    /// For a package node: whether Cargo allows publishing it (`false` for `publish = false`).
    pub publish: bool,
}

#[derive(Debug)]
pub struct Tree {
    nodes: Vec<Node>,
    children: HashMap<(NodeId, NodeKind, String), NodeId>,
}

impl Default for Tree {
    fn default() -> Self {
        Tree::new()
    }
}

impl Tree {
    /// A tree with only the workspace.
    pub fn new() -> Tree {
        let root = Node {
            name: String::new(),
            kind: NodeKind::Workspace,
            parent: None,
            depth: 0,
            test: false,
            publish: true,
        };
        Tree {
            nodes: vec![root],
            children: HashMap::new(),
        }
    }

    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    /// The child of `parent` with this kind and name, made if it is new.
    pub fn child(&mut self, parent: NodeId, kind: NodeKind, name: &str) -> NodeId {
        let key = (parent, kind, name.to_owned());
        if let Some(&id) = self.children.get(&key) {
            return id;
        }
        let id = NodeId(u32::try_from(self.nodes.len()).expect("fewer than 2^32 modules"));
        let parent_node = &self.nodes[parent.index()];
        self.nodes.push(Node {
            name: name.to_owned(),
            kind,
            parent: Some(parent),
            depth: parent_node.depth + 1,
            test: false,
            publish: true,
        });
        self.children.insert(key, id);
        id
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// The root, made the package Cargo calls `name`: what a workspace whose
    /// root is a package has instead of a level above it.
    pub fn root_package(&mut self, name: &str) -> NodeId {
        let root = self.root();
        let node = &mut self.nodes[root.index()];
        node.kind = NodeKind::Package;
        node.name = name.to_owned();
        root
    }

    /// Whether Cargo allows publishing the package `id` is.
    pub fn set_publish(&mut self, id: NodeId, publish: bool) {
        self.nodes[id.index()].publish = publish;
    }

    /// Whether `id` is test code, and so is everything inside it.
    pub fn set_test(&mut self, id: NodeId, test: bool) {
        self.nodes[id.index()].test = test;
    }

    /// Every node, each after its parent.
    pub fn ids(&self) -> impl Iterator<Item = NodeId> + use<> {
        (0..self.nodes.len()).map(|index| NodeId(index as u32))
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    /// `id`, then its parent, up to the root.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(Some(id), |&id| self.parent(id))
    }

    pub fn common_ancestor(&self, mut a: NodeId, mut b: NodeId) -> NodeId {
        while self.node(a).depth > self.node(b).depth {
            a = self.parent(a).expect("a deeper node has a parent");
        }
        while self.node(b).depth > self.node(a).depth {
            b = self.parent(b).expect("a deeper node has a parent");
        }
        while a != b {
            a = self.parent(a).expect("nodes of one tree meet at its root");
            b = self.parent(b).expect("nodes of one tree meet at its root");
        }
        a
    }

    pub fn is_within(&self, id: NodeId, ancestor: NodeId) -> bool {
        self.ancestors(id).any(|a| a == ancestor)
    }

    /// The package `id` is in: itself, or its closest package ancestor.
    pub fn package_of(&self, id: NodeId) -> Option<NodeId> {
        self.ancestors(id)
            .find(|&a| self.node(a).kind == NodeKind::Package)
    }

    pub fn is_test(&self, id: NodeId) -> bool {
        self.ancestors(id).any(|a| self.node(a).test)
    }

    /// `my_crate::kinds::epoll` for a module, `.` for the workspace.
    pub fn display(&self, id: NodeId) -> String {
        let node = self.node(id);
        match node.kind {
            NodeKind::Package => node.name.clone(),
            NodeKind::Module => {
                let parent = self.parent(id).expect("a module has a parent");
                format!("{}::{}", self.display(parent), node.name)
            }
            NodeKind::Workspace => ".".to_owned(),
        }
    }
}

impl NodeId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_whose_root_is_a_package_names_the_root() {
        let mut tree = Tree::new();
        assert_eq!(tree.node(tree.root()).kind, NodeKind::Workspace);
        assert_eq!(tree.root_package("my-crate"), tree.root());
        assert_eq!(tree.node(tree.root()).kind, NodeKind::Package);
        assert_eq!(tree.display(tree.root()), "my-crate");
    }

    #[test]
    fn a_node_says_what_it_is_marked_and_holds_what_it_says_of_what_it_holds() {
        let mut tree = Tree::new();
        let package = tree.child(tree.root(), NodeKind::Package, "p");
        let module = tree.child(package, NodeKind::Module, "sched");
        tree.set_publish(package, false);
        tree.set_test(module, true);
        assert!(!tree.node(package).publish);
        assert!(tree.node(module).test);
        // Test code inside test code is test code, marked or not.
        let fixtures = tree.child(module, NodeKind::Module, "fixtures");
        assert!(tree.is_test(fixtures));
        assert!(!tree.node(fixtures).test);
    }
}
