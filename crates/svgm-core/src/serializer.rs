use crate::ast::{Document, NodeId, NodeKind};

/// SVG elements that must not have self-closing tags in HTML contexts.
/// For pure SVG, self-closing is always valid, but we avoid it for
/// container elements that commonly have children.
const VOID_SVG_ELEMENTS: &[&str] = &[
    "path",
    "rect",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "use",
    "image",
    "stop",
    "animate",
    "animateTransform",
    "animateMotion",
    "set",
    "mpath",
];

/// Serialize a Document to a minified SVG string.
pub fn serialize(doc: &Document) -> String {
    let mut out = String::new();
    for child in doc.children(doc.root) {
        serialize_node(doc, child, &mut out);
    }
    out
}

fn serialize_node(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.node(id);
    if node.removed {
        return;
    }

    match &node.kind {
        NodeKind::Root => {
            for child in doc.children(id) {
                serialize_node(doc, child, out);
            }
        }

        NodeKind::Element(elem) => {
            out.push('<');
            out.push_str(&elem.qualified_name());

            // Namespace declarations
            for ns in &elem.namespaces {
                if ns.prefix.is_empty() {
                    out.push_str(" xmlns=\"");
                } else {
                    out.push_str(" xmlns:");
                    out.push_str(&ns.prefix);
                    out.push_str("=\"");
                }
                push_escaped_attr(out, &ns.uri);
                out.push('"');
            }

            // Attributes
            for attr in &elem.attributes {
                out.push(' ');
                out.push_str(&attr.qualified_name());
                out.push_str("=\"");
                push_escaped_attr(out, &attr.value);
                out.push('"');
            }

            let children: Vec<NodeId> = doc.children(id).collect();

            if children.is_empty() {
                // Self-closing for elements that commonly have no children
                let name = elem.name.as_str();
                if VOID_SVG_ELEMENTS.contains(&name) || !has_potential_children(name) {
                    out.push_str("/>");
                } else {
                    out.push_str("></");
                    out.push_str(&elem.qualified_name());
                    out.push('>');
                }
            } else {
                out.push('>');
                for child in &children {
                    serialize_node(doc, *child, out);
                }
                out.push_str("</");
                out.push_str(&elem.qualified_name());
                out.push('>');
            }
        }

        NodeKind::Text(text) => {
            push_escaped_text(out, text);
        }

        NodeKind::Comment(text) => {
            out.push_str("<!--");
            out.push_str(text);
            out.push_str("-->");
        }

        NodeKind::CData(text) => {
            out.push_str("<![CDATA[");
            out.push_str(text);
            out.push_str("]]>");
        }

        NodeKind::ProcessingInstruction { target, content } => {
            out.push_str("<?");
            out.push_str(target);
            if !content.is_empty() {
                out.push(' ');
                out.push_str(content);
            }
            out.push_str("?>");
        }

        NodeKind::Doctype(text) => {
            if !text.is_empty() {
                out.push_str("<!DOCTYPE ");
                out.push_str(text);
                out.push('>');
            }
        }
    }
}

/// Returns true if an SVG element typically contains children.
fn has_potential_children(name: &str) -> bool {
    matches!(
        name,
        "svg"
            | "g"
            | "defs"
            | "symbol"
            | "clipPath"
            | "mask"
            | "pattern"
            | "linearGradient"
            | "radialGradient"
            | "filter"
            | "marker"
            | "text"
            | "tspan"
            | "textPath"
            | "a"
            | "switch"
            | "foreignObject"
    )
}

fn push_escaped_attr(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn push_escaped_text(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn roundtrip_simple_svg() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="50" height="50"/></svg>"#;
        let doc = parse(input).unwrap();
        let output = serialize(&doc);
        // Roundtrip: parse the output and compare structure
        let doc2 = parse(&output).unwrap();
        assert_structural_eq(&doc, &doc2);
    }

    #[test]
    fn roundtrip_nested_groups() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g id="a"><g id="b"><rect/></g></g></svg>"#;
        let doc = parse(input).unwrap();
        let output = serialize(&doc);
        let doc2 = parse(&output).unwrap();
        assert_structural_eq(&doc, &doc2);
    }

    #[test]
    fn roundtrip_text_content() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><text>Hello world</text></svg>"#;
        let doc = parse(input).unwrap();
        let output = serialize(&doc);
        assert!(output.contains("Hello world"));
        let doc2 = parse(&output).unwrap();
        assert_structural_eq(&doc, &doc2);
    }

    #[test]
    fn text_with_entities() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text>a &amp; b &lt; c</text></svg>"#;
        let doc = parse(input).unwrap();
        let output = serialize(&doc);
        // xmlparser decodes entities, serializer re-encodes them
        assert!(output.contains("a &amp; b &lt; c"));
    }

    #[test]
    fn self_closing_void_elements() {
        let input =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/><circle r="5"/></svg>"#;
        let doc = parse(input).unwrap();
        let output = serialize(&doc);
        assert!(output.contains("<path "));
        assert!(output.contains("/>"));
    }

    #[test]
    fn container_elements_not_self_closing() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g></g><defs></defs></svg>"#;
        let doc = parse(input).unwrap();
        let output = serialize(&doc);
        assert!(output.contains("></g>"));
        assert!(output.contains("></defs>"));
    }

    #[test]
    fn removed_nodes_not_serialized() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- comment --><rect/></svg>"#;
        let mut doc = parse(input).unwrap();
        let svg_id = doc.children(doc.root).next().unwrap();
        let comment_id = doc.children(svg_id).next().unwrap();
        doc.remove(comment_id);
        let output = serialize(&doc);
        assert!(!output.contains("comment"));
        assert!(output.contains("<rect/>"));
    }

    /// Assert two documents have the same tree structure (element names, attributes).
    fn assert_structural_eq(a: &Document, b: &Document) {
        let nodes_a = a.traverse();
        let nodes_b = b.traverse();
        assert_eq!(
            nodes_a.len(),
            nodes_b.len(),
            "different node counts: {} vs {}",
            nodes_a.len(),
            nodes_b.len()
        );
        for (na, nb) in nodes_a.iter().zip(nodes_b.iter()) {
            match (&a.node(*na).kind, &b.node(*nb).kind) {
                (NodeKind::Root, NodeKind::Root) => {}
                (NodeKind::Element(ea), NodeKind::Element(eb)) => {
                    assert_eq!(ea.name, eb.name, "element name mismatch");
                    assert_eq!(
                        ea.attributes.len(),
                        eb.attributes.len(),
                        "attr count mismatch on <{}>",
                        ea.name
                    );
                    for (aa, ab) in ea.attributes.iter().zip(eb.attributes.iter()) {
                        assert_eq!(aa.name, ab.name);
                        assert_eq!(aa.value, ab.value);
                    }
                }
                (NodeKind::Text(ta), NodeKind::Text(tb)) => {
                    assert_eq!(ta, tb);
                }
                (NodeKind::Comment(ca), NodeKind::Comment(cb)) => {
                    assert_eq!(ca, cb);
                }
                _ => {
                    // Same variant is enough for other types
                    assert_eq!(
                        std::mem::discriminant(&a.node(*na).kind),
                        std::mem::discriminant(&b.node(*nb).kind)
                    );
                }
            }
        }
    }
}
