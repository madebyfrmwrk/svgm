use std::collections::HashSet;

use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Editor namespace URIs (SVGO `editorNamespaces`), plus the Adobe XMP
/// namespaces that only ever occur inside editor metadata.
const EDITOR_NAMESPACES: &[&str] = &[
    "http://creativecommons.org/ns#",
    "http://inkscape.sourceforge.net/DTD/sodipodi-0.dtd",
    "http://krita.org/namespaces/svg/krita",
    "http://ns.adobe.com/AdobeIllustrator/10.0/",
    "http://ns.adobe.com/AdobeSVGViewerExtensions/3.0/",
    "http://ns.adobe.com/Extensibility/1.0/",
    "http://ns.adobe.com/Flows/1.0/",
    "http://ns.adobe.com/GenericCustomNamespace/1.0/",
    "http://ns.adobe.com/Graphs/1.0/",
    "http://ns.adobe.com/ImageReplacement/1.0/",
    "http://ns.adobe.com/SaveForWeb/1.0/",
    "http://ns.adobe.com/Variables/1.0/",
    "http://ns.adobe.com/XPath/1.0/",
    "http://ns.adobe.com/xap/1.0/",
    "http://ns.adobe.com/xap/1.0/mm/",
    "http://ns.adobe.com/xap/1.0/sType/ResourceRef#",
    "http://purl.org/dc/elements/1.1/",
    "http://schemas.microsoft.com/visio/2003/SVGExtensions/",
    "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
    "http://sodipodi.sourceforge.net/DTD/sodipodi-0.0.dtd",
    "http://taptrix.com/vectorillustrator/svg_extensions",
    "http://www.bohemiancoding.com/sketch/ns",
    "http://www.figma.com/figma/ns",
    "http://www.inkscape.org/namespaces/inkscape",
    "http://www.serif.com/",
    "http://www.vector.evaxdesign.sk",
    "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
    "https://boxy-svg.com",
];

/// Conventional editor prefixes. Consulted only for prefixes that no
/// `xmlns:` declaration in the document binds: an undeclared `inkscape:label`
/// is still editor data, while a declared `xmlns:x="http://www.w3.org/1999/xlink"`
/// (and its `x:href` usages) is not.
const EDITOR_PREFIXES: &[&str] = &[
    "inkscape", "sodipodi", "i", "sketch", "figma", "dc", "cc", "rdf", "x", "a",
];

/// Removes editor-specific namespaces, elements and attributes
/// (SVGO `removeEditorsNSData`). Prefixes are resolved through the document's
/// `xmlns:` declarations by URI, so declarations, elements and attributes
/// always go together and the output stays well-formed.
pub struct RemoveEditorData;

impl Pass for RemoveEditorData {
    fn name(&self) -> &'static str {
        "removeEditorData"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let ids = doc.traverse();

        // Prefixes bound to an editor URI anywhere in the document, and the
        // set of all bound prefixes (which disables the conventional-prefix
        // fallback for them).
        let mut editor_prefixes: HashSet<String> = HashSet::new();
        let mut declared_prefixes: HashSet<String> = HashSet::new();
        for &id in &ids {
            if let NodeKind::Element(ref elem) = doc.node(id).kind {
                for ns in &elem.namespaces {
                    if ns.prefix.is_empty() {
                        continue;
                    }
                    declared_prefixes.insert(ns.prefix.clone());
                    if EDITOR_NAMESPACES.contains(&ns.uri.as_str()) {
                        editor_prefixes.insert(ns.prefix.clone());
                    }
                }
            }
        }
        let is_editor_prefix = |prefix: &str| {
            editor_prefixes.contains(prefix)
                || (!declared_prefixes.contains(prefix) && EDITOR_PREFIXES.contains(&prefix))
        };

        let mut changed = false;

        // Editor elements, e.g. <sodipodi:namedview>, <i:pgf>.
        for &id in &ids {
            let node = doc.node(id);
            if node.removed {
                continue;
            }
            let remove = match node.kind {
                NodeKind::Element(ref elem) => elem.prefix.as_deref().is_some_and(is_editor_prefix),
                _ => false,
            };
            if remove {
                doc.remove(id);
                changed = true;
            }
        }

        // Editor attributes and namespace declarations.
        for &id in &ids {
            let node = doc.node_mut(id);
            if node.removed {
                continue;
            }
            if let NodeKind::Element(ref mut elem) = node.kind {
                let before_attrs = elem.attributes.len();
                elem.attributes.retain(|attr| match attr.prefix {
                    Some(ref prefix) => !is_editor_prefix(prefix),
                    // Illustrator's data-name="Layer 1"
                    None => attr.name != "data-name",
                });
                if elem.attributes.len() != before_attrs {
                    changed = true;
                }

                let before_ns = elem.namespaces.len();
                elem.namespaces.retain(|ns| {
                    ns.prefix.is_empty()
                        || !(editor_prefixes.contains(&ns.prefix)
                            || EDITOR_NAMESPACES.contains(&ns.uri.as_str()))
                });
                if elem.namespaces.len() != before_ns {
                    changed = true;
                }
            }
        }

        if changed {
            PassResult::Changed
        } else {
            PassResult::Unchanged
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    fn run(input: &str) -> (PassResult, String) {
        let mut doc = parse(input).unwrap();
        let result = RemoveEditorData.run(&mut doc);
        (result, serialize(&doc))
    }

    #[test]
    fn removes_inkscape_elements_and_attrs() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" inkscape:version="1.0"><rect inkscape:label="bg"/></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("inkscape"));
    }

    #[test]
    fn removes_sketch_namespace() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sketch="http://www.bohemiancoding.com/sketch/ns"><rect sketch:type="MSPage"/></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert!(!output.contains("sketch"));
    }

    #[test]
    fn svgo_fixture_01_sodipodi() {
        // SVGO removeEditorsNSData.01
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"><sodipodi:namedview>...</sodipodi:namedview><path d="..." sodipodi:nodetypes="cccc"/></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="..."/></svg>"#
        );
    }

    #[test]
    fn svgo_fixture_02_sodipodi_inkscape_dtd_uri() {
        // SVGO removeEditorsNSData.02 — the inkscape.sourceforge.net
        // sodipodi URI (previously mis-typed as inkscape-0.dtd here).
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sodipodi="http://inkscape.sourceforge.net/DTD/sodipodi-0.dtd"><sodipodi:namedview>...</sodipodi:namedview><path d="..." sodipodi:nodetypes="cccc"/></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><path d="..."/></svg>"#
        );
    }

    #[test]
    fn keeps_x_prefix_bound_to_xlink() {
        // Illustrator binds xmlns:x to its Extensibility namespace, but a
        // document is free to bind `x` to XLink instead. Removing x:href
        // would break <use> and gradient references.
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:x="http://www.w3.org/1999/xlink"><defs><linearGradient id="g" x:href="#h"/></defs><use x:href="#r"/></svg>"##;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged);
        assert!(
            output.contains(r#"xmlns:x="http://www.w3.org/1999/xlink""#),
            "{output}"
        );
        assert_eq!(output.matches("x:href=").count(), 2, "{output}");
    }

    #[test]
    fn keeps_conventional_prefixes_bound_to_custom_uris() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:a="http://example.com/a" xmlns:i="http://example.com/i" xmlns:dc="http://example.com/dc"><a:meta i:x="1"/><rect dc:title="t" a:role="keep"/></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }

    #[test]
    fn removes_editor_namespaces_under_non_canonical_prefixes() {
        // The prefix is irrelevant; the bound URI decides. Declaration,
        // elements and attributes go together so the output stays
        // well-formed.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:ink="http://www.inkscape.org/namespaces/inkscape" xmlns:sp="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd" ink:version="1.2"><sp:namedview id="base"/><g ink:groupmode="layer"><path sp:nodetypes="cc" d="M0 0"/></g></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g><path d="M0 0"/></g></svg>"#
        );
    }

    #[test]
    fn removes_newly_listed_editor_namespaces() {
        // serif (Affinity), boxy-svg, visio, krita: in SVGO's list but
        // previously missing here.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:serif="http://www.serif.com/" xmlns:bx="https://boxy-svg.com" xmlns:v="http://schemas.microsoft.com/visio/2003/SVGExtensions/" xmlns:krita="http://krita.org/namespaces/svg/krita"><v:documentProperties v:langID="1033"/><bx:grid x="0" y="0"/><rect serif:id="Shape" krita:marker="1"/></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#
        );
    }

    #[test]
    fn undeclared_conventional_prefix_is_still_removed() {
        // No xmlns:inkscape anywhere (ill-formed, but seen in the wild):
        // the conventional prefix list still applies.
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect inkscape:label="bg" width="1"/></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="1"/></svg>"#
        );
    }

    #[test]
    fn declaration_on_nested_element_is_honoured() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><g xmlns:fig="http://www.figma.com/figma/ns" fig:type="frame"><rect fig:id="1"/></g></svg>"#;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Changed);
        assert_eq!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g><rect/></g></svg>"#
        );
    }

    #[test]
    fn unchanged_without_editor_data() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="#a"/></svg>"##;
        let (result, output) = run(input);
        assert_eq!(result, PassResult::Unchanged);
        assert_eq!(output, input);
    }
}
