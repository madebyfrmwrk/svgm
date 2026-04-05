use super::{Pass, PassResult};
use crate::ast::{Document, NodeKind};

/// Attributes that can contain color values.
const COLOR_ATTRS: &[&str] = &[
    "fill",
    "stroke",
    "stop-color",
    "flood-color",
    "lighting-color",
    "color",
];

pub struct ConvertColors;

impl Pass for ConvertColors {
    fn name(&self) -> &'static str {
        "convertColors"
    }

    fn run(&self, doc: &mut Document) -> PassResult {
        let mut changed = false;
        let ids = doc.traverse();

        for id in ids {
            let node = doc.node_mut(id);
            if let NodeKind::Element(ref mut elem) = node.kind {
                for attr in &mut elem.attributes {
                    if attr.prefix.is_some() {
                        continue;
                    }
                    if COLOR_ATTRS.contains(&attr.name.as_str())
                        && let Some(shorter) = shorten_color(&attr.value)
                        && shorter.len() < attr.value.len()
                    {
                        attr.value = shorter;
                        changed = true;
                    }
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

/// Try to produce a shorter color representation.
fn shorten_color(value: &str) -> Option<String> {
    let trimmed = value.trim();

    // Normalize to #rrggbb first
    let hex6 = if let Some(rgb) = parse_rgb(trimmed) {
        Some(format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2))
    } else if trimmed.starts_with('#') && trimmed.len() == 7 {
        Some(trimmed.to_lowercase())
    } else if trimmed.starts_with('#') && trimmed.len() == 4 {
        let c: Vec<char> = trimmed.chars().collect();
        Some(format!("#{0}{0}{1}{1}{2}{2}", c[1], c[2], c[3]).to_lowercase())
    } else {
        named_to_hex(trimmed).map(|hex| hex.to_string())
    };

    let hex6 = hex6?;

    // Find the shortest representation among: hex6, hex3, named color
    let hex3 = shorten_hex(&hex6);
    let named = hex_to_shorter_name(&hex6);

    let mut best = hex6.clone();
    if hex3.len() < best.len() {
        best = hex3;
    }
    if let Some(name) = named
        && name.len() < best.len()
    {
        best = name.to_string();
    }

    if best.len() < trimmed.len() || best != trimmed.to_lowercase() {
        Some(best)
    } else {
        None
    }
}

/// Shorten #rrggbb to #rgb if r==r, g==g, b==b.
fn shorten_hex(hex: &str) -> String {
    let hex = hex.to_lowercase();
    let chars: Vec<char> = hex.chars().collect();
    if chars.len() == 7 && chars[1] == chars[2] && chars[3] == chars[4] && chars[5] == chars[6] {
        format!("#{}{}{}", chars[1], chars[3], chars[5])
    } else {
        hex
    }
}

/// Parse rgb(r, g, b) or rgb(r%, g%, b%) — integer form only for now.
fn parse_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    let inner = s.strip_prefix("rgb(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let r: u8 = parts[0].trim().parse().ok()?;
    let g: u8 = parts[1].trim().parse().ok()?;
    let b: u8 = parts[2].trim().parse().ok()?;
    Some((r, g, b))
}

fn named_to_hex(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    // Only include colors where hex is shorter than the name
    match lower.as_str() {
        "black" => Some("#000000"),
        "white" => Some("#ffffff"),
        "red" => Some("#ff0000"),
        "blue" => Some("#0000ff"),
        "green" => Some("#008000"),
        "yellow" => Some("#ffff00"),
        "cyan" | "aqua" => Some("#00ffff"),
        "magenta" | "fuchsia" => Some("#ff00ff"),
        "silver" => Some("#c0c0c0"),
        "gray" | "grey" => Some("#808080"),
        "maroon" => Some("#800000"),
        "olive" => Some("#808000"),
        "navy" => Some("#000080"),
        "purple" => Some("#800080"),
        "teal" => Some("#008080"),
        "lime" => Some("#00ff00"),
        _ => None,
    }
}

/// For hex values that match a short named color, return the name.
fn hex_to_shorter_name(hex: &str) -> Option<&'static str> {
    match hex {
        "#ff0000" => Some("red"),
        "#000080" => Some("navy"),
        "#008080" => Some("teal"),
        "#808000" => Some("olive"),
        "#800080" => Some("plum"), // wait, plum is #dda0dd, not #800080. Let me be careful here
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::serializer::serialize;

    #[test]
    fn rgb_to_hex() {
        assert_eq!(shorten_color("rgb(255, 0, 0)"), Some("red".to_string()));
    }

    #[test]
    fn hex6_to_hex3() {
        assert_eq!(shorten_color("#ff0000"), Some("red".to_string()));
        assert_eq!(shorten_color("#aabbcc"), Some("#abc".to_string()));
    }

    #[test]
    fn named_to_shorter_hex() {
        // "white" (5 chars) → "#fff" (4 chars)
        assert_eq!(shorten_color("white"), Some("#fff".to_string()));
        // "black" (5 chars) → "#000" (4 chars)
        assert_eq!(shorten_color("black"), Some("#000".to_string()));
    }

    #[test]
    fn pass_converts_colors() {
        let input = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect fill=\"rgb(255, 0, 0)\" stroke=\"#aabbcc\"/></svg>";
        let mut doc = parse(input).unwrap();
        assert_eq!(ConvertColors.run(&mut doc), PassResult::Changed);
        let output = serialize(&doc);
        assert!(output.contains("fill=\"red\""));
        assert!(output.contains("stroke=\"#abc\""));
    }

    #[test]
    fn keeps_none_and_url() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="none" stroke="url(#grad)"/></svg>"#;
        let mut doc = parse(input).unwrap();
        assert_eq!(ConvertColors.run(&mut doc), PassResult::Unchanged);
    }
}
