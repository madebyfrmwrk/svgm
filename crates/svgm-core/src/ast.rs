use std::fmt;

/// Index into the document's node arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Arena-based SVG/XML document.
pub struct Document {
    pub nodes: Vec<Node>,
    pub root: NodeId,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let root_node = Node {
            kind: NodeKind::Root,
            parent: None,
            children: Vec::new(),
            removed: false,
        };
        Self {
            nodes: vec![root_node],
            root: NodeId(0),
        }
    }

    /// Allocate a new node in the arena and return its id.
    pub fn alloc(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            kind,
            parent: None,
            children: Vec::new(),
            removed: false,
        });
        id
    }

    /// Append `child` as the last child of `parent`.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[child.index()].parent = Some(parent);
        self.nodes[parent.index()].children.push(child);
    }

    /// Get a reference to a node.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Get a mutable reference to a node.
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    /// Mark a node (and all its descendants) as removed.
    pub fn remove(&mut self, id: NodeId) {
        self.nodes[id.index()].removed = true;
        let children: Vec<NodeId> = self.nodes[id.index()].children.clone();
        for child in children {
            self.remove(child);
        }
    }

    /// Iterate over the direct children of a node, skipping removed nodes.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes[id.index()]
            .children
            .iter()
            .copied()
            .filter(|&child| !self.nodes[child.index()].removed)
    }

    /// Walk the entire tree depth-first, yielding non-removed node ids.
    pub fn traverse(&self) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.traverse_recursive(self.root, &mut result);
        result
    }

    fn traverse_recursive(&self, id: NodeId, result: &mut Vec<NodeId>) {
        if self.nodes[id.index()].removed {
            return;
        }
        result.push(id);
        for &child in &self.nodes[id.index()].children {
            self.traverse_recursive(child, result);
        }
    }
}

pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub removed: bool,
}

pub enum NodeKind {
    Root,
    Element(Element),
    Text(String),
    Comment(String),
    CData(String),
    ProcessingInstruction { target: String, content: String },
    Doctype(String),
}

pub struct Element {
    pub name: String,
    pub prefix: Option<String>,
    pub attributes: Vec<Attribute>,
    pub namespaces: Vec<Namespace>,
}

impl Element {
    /// Get the value of an attribute by local name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name == name && a.prefix.is_none())
            .map(|a| a.value.as_str())
    }

    /// Get the qualified name (prefix:name or just name).
    pub fn qualified_name(&self) -> String {
        match &self.prefix {
            Some(prefix) => format!("{prefix}:{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Attribute {
    pub prefix: Option<String>,
    pub name: String,
    pub value: String,
}

impl Attribute {
    /// Get the qualified name (prefix:name or just name).
    pub fn qualified_name(&self) -> String {
        match &self.prefix {
            Some(prefix) => format!("{prefix}:{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Namespace {
    pub prefix: String,
    pub uri: String,
}
