pub mod cleanup_attrs;
pub mod cleanup_ids;
pub mod cleanup_numeric_values;
pub mod collapse_groups;
pub mod convert_colors;
pub mod convert_path_data;
pub mod convert_shape_to_path;
pub mod convert_transform;
pub mod merge_paths;
pub mod minify_whitespace;
pub mod remove_comments;
pub mod remove_desc;
pub mod remove_doctype;
pub mod remove_editor_data;
pub mod remove_empty_attrs;
pub mod remove_empty_containers;
pub mod remove_empty_text;
pub mod remove_metadata;
pub mod remove_proc_inst;
pub mod remove_unknowns_and_defaults;
pub mod remove_unused_namespaces;

use crate::ast::Document;

/// Result of running a single optimization pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassResult {
    Changed,
    Unchanged,
}

impl PassResult {
    pub fn changed(self) -> bool {
        self == PassResult::Changed
    }
}

/// An optimization pass that transforms a Document in place.
pub trait Pass {
    fn name(&self) -> &'static str;
    fn run(&self, doc: &mut Document) -> PassResult;
}

/// Returns the default set of safe passes in execution order.
pub fn default_passes() -> Vec<Box<dyn Pass>> {
    vec![
        Box::new(remove_doctype::RemoveDoctype),
        Box::new(remove_proc_inst::RemoveProcInst),
        Box::new(remove_comments::RemoveComments),
        Box::new(remove_metadata::RemoveMetadata),
        Box::new(remove_editor_data::RemoveEditorData),
        Box::new(remove_empty_attrs::RemoveEmptyAttrs),
        Box::new(remove_empty_text::RemoveEmptyText),
        Box::new(remove_empty_containers::RemoveEmptyContainers),
        Box::new(remove_unused_namespaces::RemoveUnusedNamespaces),
        Box::new(cleanup_attrs::CleanupAttrs),
        Box::new(cleanup_numeric_values::CleanupNumericValues::default()),
        Box::new(convert_colors::ConvertColors),
        Box::new(remove_unknowns_and_defaults::RemoveUnknownsAndDefaults),
        Box::new(convert_shape_to_path::ConvertShapeToPath::default()),
        Box::new(convert_transform::ConvertTransform::default()),
        Box::new(collapse_groups::CollapseGroups),
        Box::new(cleanup_ids::CleanupIds),
        Box::new(convert_path_data::ConvertPathData::default()),
        Box::new(merge_paths::MergePaths),
        Box::new(minify_whitespace::MinifyWhitespace),
    ]
}
