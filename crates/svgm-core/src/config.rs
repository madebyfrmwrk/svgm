use std::collections::HashMap;

use crate::passes::{self, Pass};

/// Optimization preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    /// Zero-risk-to-rendering: removal, normalization, and whitespace passes only.
    Safe,
    /// Full optimization (default). All passes enabled.
    #[default]
    Default,
}

/// Configuration for the optimization pipeline.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Which preset to use as the base pass set.
    pub preset: Preset,
    /// Numeric precision for rounding passes. If `None`, uses the preset default
    /// (default: 3).
    pub precision: Option<u32>,
    /// Per-pass overrides. `true` enables a pass not in the preset, `false` disables one that is.
    pub pass_overrides: HashMap<String, bool>,
}

impl Config {
    /// Returns the effective numeric precision for this configuration.
    pub fn effective_precision(&self) -> u32 {
        self.precision.unwrap_or(3)
    }
}

// (name, in_safe, in_default) — execution order matters.
const PASS_CATALOG: &[(&str, bool, bool)] = &[
    ("removeDoctype", true, true),
    ("removeProcInst", true, true),
    ("removeComments", true, true),
    ("removeDeprecatedAttrs", true, true),
    ("removeMetadata", true, true),
    ("removeEditorData", true, true),
    ("removeDesc", false, true),
    ("removeEmptyAttrs", true, true),
    ("removeEmptyText", true, true),
    ("removeHiddenElems", true, true),
    ("removeUselessDefs", false, true),
    ("removeUselessStrokeAndFill", false, true),
    ("removeEmptyContainers", true, true),
    ("removeUnusedNamespaces", true, true),
    ("cleanupAttrs", true, true),
    ("inlineStyles", false, true),
    ("minifyStyles", true, true),
    ("cleanupNumericValues", true, true),
    ("convertColors", true, true),
    ("removeUnknownsAndDefaults", true, true),
    ("removeNonInheritableGroupAttrs", false, true),
    ("cleanupEnableBackground", true, true),
    ("convertEllipseToCircle", false, true),
    ("convertShapeToPath", false, true),
    ("moveElemsAttrsToGroup", false, true),
    ("moveGroupAttrsToElems", false, true),
    ("convertTransform", false, true),
    ("collapseGroups", false, true),
    ("cleanupIds", false, true),
    ("convertPathData", false, true),
    ("mergePaths", false, true),
    ("sortAttrs", true, true),
    ("sortDefsChildren", true, true),
    ("minifyWhitespace", true, true),
];

fn is_in_preset(entry: &(&str, bool, bool), preset: Preset) -> bool {
    match preset {
        Preset::Safe => entry.1,
        Preset::Default => entry.2,
    }
}

fn create_pass(name: &str, precision: u32) -> Box<dyn Pass> {
    match name {
        "removeDoctype" => Box::new(passes::remove_doctype::RemoveDoctype),
        "removeProcInst" => Box::new(passes::remove_proc_inst::RemoveProcInst),
        "removeComments" => Box::new(passes::remove_comments::RemoveComments),
        "removeDeprecatedAttrs" => Box::new(passes::remove_deprecated_attrs::RemoveDeprecatedAttrs),
        "removeMetadata" => Box::new(passes::remove_metadata::RemoveMetadata),
        "removeEditorData" => Box::new(passes::remove_editor_data::RemoveEditorData),
        "removeDesc" => Box::new(passes::remove_desc::RemoveDesc),
        "removeEmptyAttrs" => Box::new(passes::remove_empty_attrs::RemoveEmptyAttrs),
        "removeEmptyText" => Box::new(passes::remove_empty_text::RemoveEmptyText),
        "removeHiddenElems" => Box::new(passes::remove_hidden_elems::RemoveHiddenElems),
        "removeUselessDefs" => Box::new(passes::remove_useless_defs::RemoveUselessDefs),
        "removeUselessStrokeAndFill" => {
            Box::new(passes::remove_useless_stroke_and_fill::RemoveUselessStrokeAndFill)
        }
        "removeEmptyContainers" => Box::new(passes::remove_empty_containers::RemoveEmptyContainers),
        "removeUnusedNamespaces" => {
            Box::new(passes::remove_unused_namespaces::RemoveUnusedNamespaces)
        }
        "cleanupAttrs" => Box::new(passes::cleanup_attrs::CleanupAttrs),
        "inlineStyles" => Box::new(passes::inline_styles::InlineStyles),
        "minifyStyles" => Box::new(passes::minify_styles::MinifyStyles),
        "cleanupNumericValues" => {
            Box::new(passes::cleanup_numeric_values::CleanupNumericValues { precision })
        }
        "convertColors" => Box::new(passes::convert_colors::ConvertColors),
        "removeUnknownsAndDefaults" => {
            Box::new(passes::remove_unknowns_and_defaults::RemoveUnknownsAndDefaults)
        }
        "removeNonInheritableGroupAttrs" => {
            Box::new(passes::remove_non_inheritable_group_attrs::RemoveNonInheritableGroupAttrs)
        }
        "cleanupEnableBackground" => {
            Box::new(passes::cleanup_enable_background::CleanupEnableBackground)
        }
        "convertEllipseToCircle" => {
            Box::new(passes::convert_ellipse_to_circle::ConvertEllipseToCircle)
        }
        "convertShapeToPath" => {
            Box::new(passes::convert_shape_to_path::ConvertShapeToPath { precision })
        }
        "moveElemsAttrsToGroup" => {
            Box::new(passes::move_elems_attrs_to_group::MoveElemsAttrsToGroup)
        }
        "moveGroupAttrsToElems" => {
            Box::new(passes::move_group_attrs_to_elems::MoveGroupAttrsToElems)
        }
        "convertTransform" => Box::new(passes::convert_transform::ConvertTransform { precision }),
        "collapseGroups" => Box::new(passes::collapse_groups::CollapseGroups),
        "cleanupIds" => Box::new(passes::cleanup_ids::CleanupIds),
        "convertPathData" => Box::new(passes::convert_path_data::ConvertPathData { precision }),
        "mergePaths" => Box::new(passes::merge_paths::MergePaths),
        "sortAttrs" => Box::new(passes::sort_attrs::SortAttrs),
        "sortDefsChildren" => Box::new(passes::sort_defs_children::SortDefsChildren),
        "minifyWhitespace" => Box::new(passes::minify_whitespace::MinifyWhitespace),
        _ => panic!("unknown pass: {name}"),
    }
}

/// Build the pass list for a given configuration.
pub fn passes_for_config(config: &Config) -> Vec<Box<dyn Pass>> {
    let precision = config.effective_precision();
    let mut result = Vec::new();

    for entry in PASS_CATALOG {
        let name = entry.0;
        let enabled = if let Some(&override_val) = config.pass_overrides.get(name) {
            override_val
        } else {
            is_in_preset(entry, config.preset)
        };

        if enabled {
            result.push(create_pass(name, precision));
        }
    }

    result
}

/// Returns the list of all known pass names in execution order.
pub fn all_pass_names() -> Vec<&'static str> {
    PASS_CATALOG.iter().map(|e| e.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass_names(config: &Config) -> Vec<&'static str> {
        passes_for_config(config).iter().map(|p| p.name()).collect()
    }

    #[test]
    fn safe_preset_passes() {
        let config = Config {
            preset: Preset::Safe,
            ..Config::default()
        };
        let names = pass_names(&config);
        assert_eq!(
            names,
            vec![
                "removeDoctype",
                "removeProcInst",
                "removeComments",
                "removeDeprecatedAttrs",
                "removeMetadata",
                "removeEditorData",
                "removeEmptyAttrs",
                "removeEmptyText",
                "removeHiddenElems",
                "removeEmptyContainers",
                "removeUnusedNamespaces",
                "cleanupAttrs",
                "minifyStyles",
                "cleanupNumericValues",
                "convertColors",
                "removeUnknownsAndDefaults",
                "cleanupEnableBackground",
                "sortAttrs",
                "sortDefsChildren",
                "minifyWhitespace",
            ]
        );
        assert_eq!(names.len(), 20);
    }

    #[test]
    fn default_preset_passes() {
        let config = Config::default();
        assert_eq!(config.preset, Preset::Default);
        let names = pass_names(&config);
        assert_eq!(
            names,
            vec![
                "removeDoctype",
                "removeProcInst",
                "removeComments",
                "removeDeprecatedAttrs",
                "removeMetadata",
                "removeEditorData",
                "removeDesc",
                "removeEmptyAttrs",
                "removeEmptyText",
                "removeHiddenElems",
                "removeUselessDefs",
                "removeUselessStrokeAndFill",
                "removeEmptyContainers",
                "removeUnusedNamespaces",
                "cleanupAttrs",
                "inlineStyles",
                "minifyStyles",
                "cleanupNumericValues",
                "convertColors",
                "removeUnknownsAndDefaults",
                "removeNonInheritableGroupAttrs",
                "cleanupEnableBackground",
                "convertEllipseToCircle",
                "convertShapeToPath",
                "moveElemsAttrsToGroup",
                "moveGroupAttrsToElems",
                "convertTransform",
                "collapseGroups",
                "cleanupIds",
                "convertPathData",
                "mergePaths",
                "sortAttrs",
                "sortDefsChildren",
                "minifyWhitespace",
            ]
        );
        assert_eq!(names.len(), 34);
    }

    #[test]
    fn override_enables_pass_not_in_preset() {
        // removeDesc is in Default but not in Safe — enable it via override
        let config = Config {
            preset: Preset::Safe,
            pass_overrides: HashMap::from([("removeDesc".to_string(), true)]),
            ..Config::default()
        };
        let names = pass_names(&config);
        assert!(names.contains(&"removeDesc"));
    }

    #[test]
    fn override_disables_preset_pass() {
        let config = Config {
            preset: Preset::Default,
            pass_overrides: HashMap::from([("collapseGroups".to_string(), false)]),
            ..Config::default()
        };
        let names = pass_names(&config);
        assert!(!names.contains(&"collapseGroups"));
    }

    #[test]
    fn effective_precision_defaults() {
        assert_eq!(
            Config {
                preset: Preset::Safe,
                ..Config::default()
            }
            .effective_precision(),
            3
        );
        assert_eq!(Config::default().effective_precision(), 3);
    }

    #[test]
    fn explicit_precision_overrides_default() {
        let config = Config {
            precision: Some(4),
            ..Config::default()
        };
        assert_eq!(config.effective_precision(), 4);
    }
}
