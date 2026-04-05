use crate::ast::{Document, NodeKind};
use super::{Pass, PassResult};

/// Editor namespace URIs to remove.
const EDITOR_NAMESPACES: &[&str] = &[
    "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
    "http://inkscape.sourceforge.net/DTD/inkscape-0.dtd",
    "http://www.inkscape.org/namespaces/inkscape",
    "http://sodipodi.sourceforge.net/DTD/sodipodi-0.0.dtd",
    "http://ns.adobe.com/AdobeIllustrator/10.0/",
    "http://ns.adobe.com/AdobeSVGViewerExtensions/3.0/",
    "http://ns.adobe.com/Extensibility/1.0/",
    "http://ns.adobe.com/Flows/1.0/",
    "http://ns.adobe.com/ImageReplacement/1.0/",
    "http://ns.adobe.com/GenericCustomNamespace/1.0/",
    "http://ns.adobe.com/SaveForWeb/1.0/",
    "http://ns.adobe.com/Variables/1.0/",
    "http://ns.adobe.com/xap/1.0/",
    "http://ns.adobe.com/xap/1.0/mm/",
    "http://ns.adobe.com/xap/1.0/sType/ResourceRef#",
    "http://www.bohemiancoding.com/sketch/ns",
    "http://www.figma.com/figma/ns",
    "http://creativecommons.org/ns#",
    "http://purl.org/dc/elements/1.1/",
    "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
];

/// Editor-specific namespace prefixes.
const EDITOR_PREFIXES: &[&str] = &[
    "inkscape", "sodipodi", "i", "sketch", "figma",
    "dc", "cc", "rdf", "x", "a",
];

pub struct RemoveEditorData;

impl Pass for RemoveEditorData {
    fn name(&self) -> &'static str {
        "removeEditorData"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node(id);
            if let NodeKind::Element(ref elem) = node.kind {
                // Remove elements with editor namespace prefixes
                if let Some(ref prefix) = elem.prefix
                    && EDITOR_PREFIXES.contains(&prefix.as_str()) {
                        doc.remove(id);
                        changed = true;
                        continue;
                    }

                // Remove editor-specific elements by name
                if matches!(elem.name.as_str(),
                    "namedview" | "sodipodi:namedview" | "inkscape:perspective"
                ) {
                    doc.remove(id);
                    changed = true;
                    continue;
                }
            }
        }

        // Second pass: remove editor attributes and namespace declarations from elements
        let ids = doc.traverse();
        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                let before_attrs = elem.attributes.len();
                elem.attributes.retain(|attr| {
                    if let Some(ref prefix) = attr.prefix {
                        !EDITOR_PREFIXES.contains(&prefix.as_str())
                    } else {
                        // Remove common editor-specific attributes without prefix
                        !matches!(attr.name.as_str(),
                            "data-name" // Illustrator
                        )
                    }
                });
                if elem.attributes.len() != before_attrs {
                    changed = true;
                }

                let before_ns = elem.namespaces.len();
                elem.namespaces.retain(|ns| {
                    !EDITOR_NAMESPACES.contains(&ns.uri.as_str())
                });
                if elem.namespaces.len() != before_ns {
                    changed = true;
                }
            }
        }

        if changed { PassResult::Changed } else { PassResult::Unchanged }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn removes_inkscape_elements_and_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" inkscape:version="1.0"><rect inkscape:label="bg"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEditorData.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("inkscape"));
    }

    #[test]
    fn removes_sketch_namespace() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sketch="http://www.bohemiancoding.com/sketch/ns"><rect sketch:type="MSPage"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(RemoveEditorData.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(!output.contains("sketch"));
    }
}
