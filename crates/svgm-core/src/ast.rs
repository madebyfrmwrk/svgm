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
#[derive(Clone)]
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

    /// Mark only `id` as removed, leaving its children untouched. The caller
    /// must already have moved the children elsewhere ([`Document::reparent`])
    /// or removed them: a removed node's descendants are unreachable from the
    /// tree but still count in [`Document::byte_estimate`].
    pub fn remove_shallow(&mut self, id: NodeId) {
        self.nodes[id.index()].removed = true;
    }

    /// Detach `child` from its current parent (if any) and insert it into
    /// `new_parent`'s child list at `index` (clamped to the end). Removed
    /// siblings count toward `index`, exactly as in the raw `children` vec.
    pub fn reparent(&mut self, child: NodeId, new_parent: NodeId, index: usize) {
        if let Some(old_parent) = self.nodes[child.index()].parent {
            let siblings = &mut self.nodes[old_parent.index()].children;
            if let Some(pos) = siblings.iter().position(|&c| c == child) {
                siblings.remove(pos);
            }
        }
        let children = &mut self.nodes[new_parent.index()].children;
        let index = index.min(children.len());
        children.insert(index, child);
        self.nodes[child.index()].parent = Some(new_parent);
    }

    /// Iterate over the direct children of a node, skipping removed nodes.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes[id.index()]
            .children
            .iter()
            .copied()
            .filter(|&child| !self.nodes[child.index()].removed)
    }

    /// Cheap estimate of the serialized size in bytes, counting only
    /// non-removed nodes. Used by the optimizer to detect a pass that grows
    /// the document instead of shrinking it. No allocation.
    pub fn byte_estimate(&self) -> usize {
        let mut total = 0usize;
        for node in &self.nodes {
            if node.removed {
                continue;
            }
            total += match &node.kind {
                NodeKind::Root => 0,
                NodeKind::Element(elem) => {
                    let mut n = elem.name.len() * 2 + 5; // <name></name>
                    if let Some(p) = &elem.prefix {
                        n += (p.len() + 1) * 2;
                    }
                    for a in &elem.attributes {
                        n += a.name.len() + a.value.len() + 4; // ` name="value"`
                        if let Some(p) = &a.prefix {
                            n += p.len() + 1;
                        }
                    }
                    for ns in &elem.namespaces {
                        n += ns.prefix.len() + ns.uri.len() + 10; // ` xmlns:p="uri"`
                    }
                    n
                }
                NodeKind::Text(t) => t.len(),
                NodeKind::Comment(t) => t.len() + 7,
                NodeKind::CData(t) => t.len() + 12,
                NodeKind::ProcessingInstruction { target, content } => {
                    target.len() + content.len() + 5
                }
                NodeKind::Doctype(t) => t.len() + 11,
            };
        }
        total
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

#[derive(Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub removed: bool,
}

#[derive(Clone)]
pub enum NodeKind {
    Root,
    Element(Element),
    Text(String),
    Comment(String),
    CData(String),
    ProcessingInstruction { target: String, content: String },
    Doctype(String),
}

#[derive(Clone)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn elem(name: &str) -> NodeKind {
        NodeKind::Element(Element {
            name: name.to_string(),
            prefix: None,
            attributes: Vec::new(),
            namespaces: Vec::new(),
        })
    }

    fn names(doc: &Document, parent: NodeId) -> Vec<String> {
        doc.children(parent)
            .map(|c| match &doc.node(c).kind {
                NodeKind::Element(e) => e.name.clone(),
                _ => "#text".to_string(),
            })
            .collect()
    }

    #[test]
    fn remove_shallow_leaves_children_untouched() {
        let mut doc = Document::new();
        let g = doc.alloc(elem("g"));
        let path = doc.alloc(elem("path"));
        doc.append_child(doc.root, g);
        doc.append_child(g, path);

        doc.remove_shallow(g);
        assert!(doc.node(g).removed);
        assert!(!doc.node(path).removed);
        assert_eq!(doc.node(g).children, vec![path]);
        // The subtree is unreachable from the root.
        assert_eq!(doc.traverse(), vec![doc.root]);

        // Recursive removal, by contrast, marks the whole subtree.
        let mut doc2 = Document::new();
        let g2 = doc2.alloc(elem("g"));
        let p2 = doc2.alloc(elem("path"));
        doc2.append_child(doc2.root, g2);
        doc2.append_child(g2, p2);
        doc2.remove(g2);
        assert!(doc2.node(p2).removed);
    }

    #[test]
    fn reparent_moves_between_parents_at_index() {
        let mut doc = Document::new();
        let defs = doc.alloc(elem("defs"));
        let g = doc.alloc(elem("g"));
        let a = doc.alloc(elem("a"));
        let b = doc.alloc(elem("b"));
        let c = doc.alloc(elem("c"));
        doc.append_child(doc.root, defs);
        doc.append_child(defs, a);
        doc.append_child(defs, g);
        doc.append_child(g, b);
        doc.append_child(g, c);

        // Insert before <g>, which sits at index 1.
        doc.reparent(c, defs, 1);
        assert_eq!(names(&doc, defs), vec!["a", "c", "g"]);
        assert_eq!(names(&doc, g), vec!["b"]);
        assert_eq!(doc.node(c).parent, Some(defs));
        assert_eq!(doc.node(b).parent, Some(g));

        // An out-of-range index appends.
        doc.reparent(b, defs, 99);
        assert_eq!(names(&doc, defs), vec!["a", "c", "g", "b"]);
        assert!(doc.children(g).next().is_none());

        // Moving within the same parent reorders.
        doc.reparent(b, defs, 0);
        assert_eq!(names(&doc, defs), vec!["b", "a", "c", "g"]);
        assert_eq!(doc.traverse().len(), 6);
    }

    #[test]
    fn reparent_then_remove_keeps_moved_nodes() {
        let mut doc = Document::new();
        let defs = doc.alloc(elem("defs"));
        let g = doc.alloc(elem("g"));
        let path = doc.alloc(elem("path"));
        doc.append_child(doc.root, defs);
        doc.append_child(defs, g);
        doc.append_child(g, path);

        doc.reparent(path, defs, 0);
        doc.remove(g);
        assert!(!doc.node(path).removed);
        assert_eq!(names(&doc, defs), vec!["path"]);
    }
}
