#!/usr/bin/env python3
"""Generate adversarial SVG inputs for the svgm robustness sweep.

Usage: scripts/gen_malformed.py OUT_DIR

Writes one .svg per case (truncated documents, entities, garbage path data,
non-finite numbers, deep nesting, huge inputs, non-ASCII everywhere,
malformed stylesheets ...). scripts/sweep.py --corpus malformed calls this.
Add a case with add(name, content) when a new crash class is found.
"""
import os, sys

OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)
NS = 'xmlns="http://www.w3.org/2000/svg"'
XL = 'xmlns:xlink="http://www.w3.org/1999/xlink"'

cases = {}

def add(name, content):
    assert name not in cases, name
    cases[name] = content

def wrap(body, attrs=""):
    return f'<svg {NS} {attrs}>{body}</svg>'

# --- truncated / malformed docs ---
add("trunc_01_lt_svg", "<svg")
add("trunc_02_svg_xmlns_eq", "<svg xmlns=")
add("trunc_03_svg_g_open", "<svg><g>")
add("trunc_04_svg_ns_g_open", f'<svg {NS}><g><rect width="1" height="1"/>')
add("trunc_05_mid_attr", f'<svg {NS}><rect width="1" heig')
add("trunc_06_unclosed_comment", f'<svg {NS}><!-- oops <rect/></svg>')
add("trunc_07_unclosed_cdata", f'<svg {NS}><style><![CDATA[ a{{}} </style></svg>')
add("empty_01_zero_bytes", "")
add("empty_02_whitespace_only", "   \n\t  \n")
add("empty_03_only_xml_decl", '<?xml version="1.0" encoding="UTF-8"?>\n')
add("empty_04_only_comment", '<!-- nothing -->')
add("root_01_non_svg_root", '<html><body><p>hi</p></body></html>')
add("root_02_div_root_with_svg_inside", f'<div><svg {NS}><rect width="1" height="1"/></svg></div>')
add("root_03_multiple_roots", f'<svg {NS}><rect width="1" height="1"/></svg><svg {NS}><circle r="1"/></svg>')
add("root_04_text_after_root", f'<svg {NS}><rect width="1" height="1"/></svg>trailing junk')
add("root_05_pi_after_root", f'<svg {NS}><rect width="1" height="1"/></svg><?foo bar?>')
add("root_06_comment_after_root", f'<svg {NS}><rect width="1" height="1"/></svg><!-- c -->')
add("root_07_extra_close_tag", f'<svg {NS}><rect width="1" height="1"/></svg></svg>')
add("root_08_mismatched_close", f'<svg {NS}><g><rect/></svg></g>')
add("bom_01_utf8_bom", '﻿' + wrap('<rect width="1" height="1"/>'))
add("bom_02_bom_then_decl", '﻿<?xml version="1.0"?>' + wrap('<rect width="1" height="1"/>'))
add("decl_01_standalone_encoding_latin1", '<?xml version="1.0" encoding="ISO-8859-1" standalone="no"?>' + wrap('<text>café</text>'))
add("decl_02_decl_not_first", '\n\n<?xml version="1.0"?>' + wrap('<rect width="1" height="1"/>'))
add("decl_03_decl_after_root", wrap('<rect width="1" height="1"/>') + '<?xml version="1.0"?>')

# --- entities / doctype ---
add("ent_01_doctype_internal_entity", '<!DOCTYPE svg [ <!ENTITY foo "bar"> <!ENTITY ns_svg "http://www.w3.org/2000/svg"> ]>'
    f'<svg xmlns="&ns_svg;"><text>&foo;</text><rect width="1" height="1" class="&foo;"/></svg>')
add("ent_02_undefined_entity_text", wrap('<text>&undefined; &nbsp; &copy;</text>'))
add("ent_03_undefined_entity_attr", wrap('<rect width="1" height="1" data-x="&undefined;"/>'))
add("ent_04_numeric_refs", wrap('<text>&#65;&#x42;&#x1F600;&#38;&#60;&#62;&#34;&#39;</text><rect width="1" height="1" aria-label="&#65;&#x42;&#x26;"/>'))
add("ent_05_bad_numeric_refs", wrap('<text>&#0;&#xD800;&#xFFFFFFFF;&#;&#x;&#-1;&#1114112;</text>'))
add("ent_06_bare_ampersand_text", wrap('<text>a & b</text>'))
add("ent_07_bare_ampersand_attr", wrap('<rect width="1" height="1" data-x="a & b"/>'))
add("ent_08_entity_no_semicolon", wrap('<text>a &amp b &lt c</text>'))
add("ent_09_doctype_external_public", '<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">' + wrap('<rect width="1" height="1"/>'))
add("ent_10_entity_in_entity", '<!DOCTYPE svg [ <!ENTITY a "x"> <!ENTITY b "&a;&a;"> ]>' + wrap('<text>&b;</text>'))
add("ent_11_billion_laughs_small", '<!DOCTYPE svg [ <!ENTITY l0 "ha"> <!ENTITY l1 "&l0;&l0;&l0;&l0;&l0;&l0;&l0;&l0;&l0;&l0;"> <!ENTITY l2 "&l1;&l1;&l1;&l1;&l1;&l1;&l1;&l1;&l1;&l1;"> <!ENTITY l3 "&l2;&l2;&l2;&l2;&l2;&l2;&l2;&l2;&l2;&l2;"> ]>' + wrap('<text>&l3;</text>'))
add("ent_12_lt_in_text_via_entity", wrap('<text>&lt;script&gt;alert(1)&lt;/script&gt;</text>'))
add("ent_13_attr_with_quotes_entities", wrap('<rect width="1" height="1" title="say &quot;hi&quot; &apos;there&apos;"/>'))

# --- CDATA / comments / PIs ---
add("cdata_01_in_style", wrap('<style><![CDATA[ .a{fill:red} .b > .c{stroke:blue} ]]></style><rect class="a" width="1" height="1"/><rect class="b" width="1" height="1"/>'))
add("cdata_02_in_text", wrap('<text><![CDATA[ 1 < 2 && 3 > 2 ]]></text>'))
add("cdata_03_in_script", wrap('<script><![CDATA[ if (a < b && c > d) { x = "]]"; } ]]></script><rect width="1" height="1"/>'))
add("cdata_04_in_attr_like", wrap('<rect width="1" height="1" data-x="<![CDATA[x]]>"/>'))
add("cdata_05_nested_like_text", wrap('<text><![CDATA[ <![CDATA[ inner ]]></text>'))
add("cdata_06_text_contains_cdata_end", wrap('<text>a ]]&gt; b</text>'))
add("cdata_07_empty_cdata_in_style", wrap('<style><![CDATA[]]></style><rect width="1" height="1"/>'))
add("cdata_08_cdata_at_root_level", f'<![CDATA[x]]><svg {NS}><rect width="1" height="1"/></svg>')
add("comment_01_double_dash", wrap('<!-- a -- b --><rect width="1" height="1"/>'))
add("comment_02_ends_with_dash", wrap('<!-- a ---><rect width="1" height="1"/>'))
add("comment_03_in_text_element", wrap('<text>a<!--x-->b</text>'))
add("comment_04_in_style", wrap('<style>/* css */ .a{fill:red}<!-- html --></style><rect class="a" width="1" height="1"/>'))
add("comment_05_before_decl", '<!-- c --><?xml version="1.0"?>' + wrap('<rect width="1" height="1"/>'))
add("pi_01_inside_root", wrap('<?xml-stylesheet href="a.css"?><rect width="1" height="1"/>'))
add("pi_02_before_root", f'<?xml-stylesheet href="a.css" type="text/css"?><svg {NS}><rect width="1" height="1"/></svg>')
add("pi_03_weird_pi", wrap('<?php echo "<"; ?><rect width="1" height="1"/>'))

# --- nesting / size stress ---
def nested(depth):
    return f'<svg {NS}>' + '<g>' * depth + '<rect width="1" height="1"/>' + '</g>' * depth + '</svg>'
add("deep_01_depth_500", nested(500))
add("deep_02_depth_5000", nested(5000))
add("deep_03_depth_50000", nested(50000))
add("deep_04_depth_5000_with_attrs", f'<svg {NS}>' + '<g fill="red" transform="translate(1,1)">' * 5000 + '<rect width="1" height="1"/>' + '</g>' * 5000 + '</svg>')
add("deep_05_depth_5000_svg_nested", f'<svg {NS}>' + '<svg x="1">' * 5000 + '<rect width="1" height="1"/>' + '</svg>' * 5000 + '</svg>')
add("deep_06_depth_5000_unclosed", f'<svg {NS}>' + '<g>' * 5000)
add("big_01_1mb_class_attr", wrap('<rect width="1" height="1" class="' + 'a' * (1024 * 1024) + '"/>'))
add("big_02_1mb_style_attr", wrap('<rect width="1" height="1" style="' + 'fill:red;' * 120000 + '"/>'))
add("big_03_1mb_path_d", wrap('<path d="M0 0' + ' L1.123456 2.654321' * 60000 + 'Z"/>'))
add("big_04_100k_siblings", wrap(''.join(f'<rect x="{i}" width="1" height="1"/>' for i in range(100000))))
add("big_05_100k_paths_mergeable", wrap(''.join(f'<path d="M{i} 0h1v1h-1z"/>' for i in range(100000))))
add("big_06_10k_groups_single_child_chain", wrap(''.join(f'<g><rect x="{i}" width="1" height="1"/></g>' for i in range(10000))))
add("big_07_1mb_comment", wrap('<!--' + 'x' * (1024 * 1024) + '--><rect width="1" height="1"/>'))
add("big_08_1mb_text_node", wrap('<text>' + 'lorem ipsum ' * 90000 + '</text>'))
add("big_09_10k_ids_referenced", wrap('<defs>' + ''.join(f'<linearGradient id="g{i}"><stop offset="0"/></linearGradient>' for i in range(10000)) + '</defs>' + ''.join(f'<rect width="1" height="1" fill="url(#g{i})"/>' for i in range(10000))))
add("big_10_10k_css_rules", wrap('<style>' + ''.join(f'.c{i}{{fill:#{i%4096:03x}}}' for i in range(10000)) + '</style>' + ''.join(f'<rect class="c{i}" width="1" height="1"/>' for i in range(2000))))
add("big_11_10k_empty_rules", wrap('<style>' + 'a{}' * 10000 + '</style><rect width="1" height="1"/>'))
add("big_12_1mb_transform_chain", wrap('<rect width="1" height="1" transform="' + 'translate(1,1) ' * 60000 + '"/>'))
add("big_13_100k_attrs_on_one_elem", wrap('<rect width="1" height="1" ' + ' '.join(f'data-a{i}="{i}"' for i in range(100000)) + '/>'))
add("big_14_path_100k_subpaths", wrap('<path d="' + ''.join(f'M{i} 0h1v1z' for i in range(100000)) + '"/>'))
add("big_15_wide_use_fanout", wrap('<defs><g id="a">' + '<rect width="1" height="1"/>' * 100 + '</g>' + ''.join(f'<g id="b{i}">' + ''.join(f'<use href="#{"a" if i == 0 else "b"+str(i-1)}"/>' for _ in range(10)) + '</g>' for i in range(1, 30)) + '</defs><use href="#b29"/>'))

# --- path d edge cases ---
pd = [
    ("e_alone", "M0 0 e"), ("E_upper_alone", "M0 0 E"), ("1e", "M1e 1e"), ("1e_plus", "M1e+ 2"), ("1e999", "M0 0 L1e999 1e999"),
    ("1e-999", "M0 0 L1e-999 1e-999"), ("inf", "M0 0 L inf inf"), ("nan", "M0 0 L nan nan"), ("NaN_mixedcase", "M0 0 L NaN NaN"),
    ("Infinity", "M0 0 L Infinity -Infinity"), ("minus_alone", "M0 0 L - -"), ("dot_alone", "M0 0 L . ."), ("dotdot", "M0 0 L .. .."),
    ("plusminus", "M0 0 L +- 1"), ("1.2.3", "M1.2.3 4"), ("arc_flag_2", "M0 0 A5 5 0 2 0 10 10"), ("arc_flag_9", "M0 0 A5 5 0 9 1 10 10"),
    ("arc_flag_01_packed", "M0 0 A5 5 0 0110 10"), ("arc_flag_packed_all", "M0 0a5 5 0 1110 10"), ("arc_neg_radius", "M0 0 A-5 -5 0 0 1 10 10"),
    ("arc_zero_radius", "M0 0 A0 0 0 0 1 10 10"), ("arc_zero_rx_only", "M0 0 A0 5 0 0 1 10 10"), ("arc_same_endpoint", "M5 5 A5 5 0 0 1 5 5"),
    ("arc_tiny_radius_big_chord", "M0 0 A1 1 0 0 1 100 100"), ("arc_huge_rotation", "M0 0 A5 5 1e10 0 1 10 10"),
    ("missing_args_L", "M0 0 L 1"), ("missing_args_C", "M0 0 C 1 2 3"), ("missing_args_A", "M0 0 A 5 5 0 0"), ("only_Z", "Z"),
    ("mZ", "mZ"), ("M_only", "M"), ("M0", "M0"), ("trailing_comma", "M0,0 L1,1,"), ("leading_comma", "M,0,0"), ("double_comma", "M0,,0 L1,1"),
    ("unicode_digits", "M٠ ٠ L１ １"), ("fullwidth_cmd", "Ｍ0 0 L1 1"), ("huge_coords", "M1e308 1e308 L-1e308 -1e308 L1.7976931348623157e308 0"),
    ("tiny_coords", "M1e-308 1e-308 L5e-324 5e-324"), ("neg_zero", "M-0 -0 L-0.0 -0.0 h-0 v-0"), ("hex_looking", "M0x10 0x20 L 1 1"),
    ("lowercase_z_then_coords", "M0 0 L1 1 z 2 2"), ("m_implicit_lineto", "m0 0 1 1 2 2 3 3"), ("M_implicit_after_z", "M0 0 1 1 z 2 2"),
    ("H_V_only", "H10 V10"), ("relative_first", "m10 10 l5 5"), ("empty_d", ""), ("whitespace_d", "   "), ("garbage_d", "hello world"),
    ("cmd_no_space_numbers", "M0.5.5.5.5L1-1-1-1"), ("exp_uppercase_E", "M1E2 1E-2 L1.5E+3 2"), ("many_decimals", "M0.123456789012345678901234567890 0.999999999999999999999"),
    ("deg_curve_all_same", "M5 5 C5 5 5 5 5 5"), ("deg_curve_collinear_backtrack", "M0 0 C10 0 -10 0 5 0"), ("q_deg", "M0 0 Q0 0 0 0 T0 0 T0 0"),
    ("s_after_m", "M0 0 S10 10 20 20"), ("t_after_m", "M0 0 T10 10"), ("zero_length_line_chain", "M5 5 L5 5 L5 5 L5 5 Z"),
    ("huge_count_zeros", "M0 0 " + "L0 0 " * 5000 + "Z"), ("nul_byte_like", "M0 0 L1 1\x0c\x0bL2 2"), ("tabs_newlines", "M0\t0\nL1\r\n1"),
    ("cmd_only_letters", "MLHVCSQTAZ"), ("number_then_e_letter_next_cmd", "M0 0L1 1e5 5"), ("plus_prefixed", "M+1 +1 L+2 +2"),
    ("arc_flags_no_sep_neg", "M0 0a5 5 0 01-10-10"), ("overflow_precision_relative", "M1e15 1e15 l0.001 0.001"),
    ("big_minus_small", "M100000.0005 0 L100000.0006 0"),
]
for n, d in pd:
    add(f"path_{n}", wrap(f'<path d="{d}" fill="red"/>'))
add("path_mixed_in_g_merge", wrap('<path d="M0 0h1v1h-1z"/><path d="M0 0 L1e999 0"/><path d="M0 0 L nan 0"/>'))
add("path_shape_to_path_weird", wrap('<rect x="1e999" y="nan" width="1e-999" height="-5"/><circle cx="inf" cy="1" r="1e308"/><polygon points="0,0 1e999,1 nan,2 3"/><polyline points="1 2 3"/><line x1="1e308" y1="-1e308" x2="1e308" y2="1e308"/>'))

# --- transform edge cases ---
tr = [
    ("unknown_fn", "foo(1,2)"), ("unbalanced_open", "translate(1,2"), ("unbalanced_close", "translate(1,2))"), ("rotate_open", "rotate("),
    ("matrix_5_args", "matrix(1,0,0,1,5)"), ("matrix_7_args", "matrix(1,0,0,1,5,5,5)"), ("euro_comma", "translate(1,5 2,5)"),
    ("euro_comma_matrix", "matrix(1,0,0,1,10,5,20,5)"), ("nested_fn", "translate(rotate(45))"), ("scale_0", "scale(0)"),
    ("scale_0_0", "scale(0,0)"), ("1e999", "translate(1e999,0)"), ("nan", "translate(nan,nan)"), ("inf", "scale(inf)"),
    ("empty", ""), ("spaces_only", "   "), ("no_args", "translate()"), ("rotate_cxcy_2args", "rotate(45,10)"),
    ("skew_90", "skewX(90)"), ("skew_270", "skewY(270) skewX(89.9999999)"), ("rotate_360000", "rotate(360000.0000001)"),
    ("singular_matrix", "matrix(1,2,2,4,0,0)"), ("tiny_scale", "scale(1e-10)"), ("huge_scale", "scale(1e200) scale(1e200)"),
    ("neg_zero", "translate(-0,-0) scale(-0)"), ("many_identity", "translate(0,0) " * 1000), ("uppercase_fn", "TRANSLATE(1,2)"),
    ("missing_comma_space", "translate(1 2)rotate(45)scale(2)"), ("unicode_minus", "translate(−5,−5)"), ("plus_sign", "translate(+5,+5)"),
    ("trailing_garbage", "translate(1,2) xyz"), ("leading_garbage", "xyz translate(1,2)"), ("semicolon_sep", "translate(1,2);rotate(45)"),
    ("dot_only_args", "translate(.,.)"), ("exp_args", "translate(1e3,1E-3) scale(1e)"),
]
for n, t in tr:
    add(f"transform_{n}", wrap(f'<g transform="{t}"><rect width="10" height="10"/></g><path transform="{t}" d="M0 0h1v1z"/>'))
add("transform_gradient_weird", wrap('<defs><linearGradient id="g" gradientTransform="rotate(45 scale(0)"><stop offset="0"/></linearGradient><pattern id="p" patternTransform="matrix(nan,0,0,1,0,0)" width="1" height="1"><rect width="1" height="1"/></pattern></defs><rect fill="url(#g)" stroke="url(#p)" width="1" height="1"/>'))
add("transform_on_text_with_tspan", wrap('<g transform="translate(1e999,5)"><text x="1" y="1">a<tspan x="2">b</tspan></text></g>'))
add("transform_nested_svg_elements", wrap(f'<g transform="rotate(45)"><svg {NS} x="10" y="10" width="50" height="50" viewBox="0 0 10 10"><g transform="scale(2)"><svg x="1" y="1"><rect width="1" height="1"/></svg></g></svg></g>'))

# --- colors ---
colors = ["#ggg", "#12345", "#1234567", "#", "##fff", "rgb(300%,-1,1e9)", "rgb(1 2 3 / 50%)", "rgba(1,2,3,0.5)", "rgb(1,2)", "rgb(,,)", "rgb()",
          "hsl(120,100%,50%)", "hsl(120 100% 50% / .5)", "hwb(0 0% 0%)", "color(display-p3 1 0 0)", "lab(50% 40 59.5)", "currentColor", "CURRENTCOLOR",
          "url(#x) rgba(0,0,0,.5)", "url(#x) red", "url(#x)", "url( #x )", "url('#x')", 'url(&quot;#x&quot;)', "none ", " transparent", "inherit",
          "ＲＥＤ", "rëd", "Red", "RED", "rgb(0,0,0", "rgb(1.5,2.5,3.5)", "rgb(100%,100%,100%)", "rgb(50.5%,0%,0%)", "#FFFFFF80", "#ffff", "#000000ff",
          "context-fill", "context-stroke", "var(--x)", "var(--x, red)", "light-dark(red, blue)", "rgb(-1e999,nan,inf)", "rgb(+1,+2,+3)", "rgb( 1 , 2 , 3 )",
          "ActiveBorder", "ButtonText", "aqua", "cyan", "rebeccapurple", "transparent"]
for i, c in enumerate(colors):
    add(f"color_{i:02d}", wrap(f'<rect fill="{c}" stroke="{c}" stop-color="{c}" width="1" height="1" style="fill:{c};stroke:{c}"/><linearGradient id="x"><stop stop-color="{c}" offset="0"/></linearGradient>'))

# --- style attribute edge cases ---
styles = ["fill:red;;;;", "fill:red;;stroke:blue;;", ":", ";", ":;:;", "fill", "fill:", ":red", "fill:url(", "fill:url(#a", "fill:url(#a)   ", "fill : red !important",
          "fill:red!important;stroke:blue !IMPORTANT", "fill:red}", "fill:red;{", "fill:red/*unterminated", "fill:red;/* ok */stroke:blue", "fill:\"red\"", "fill:'red",
          "fill:red;;fill:blue", "FILL:RED", "fill:red\n;stroke:\tblue", "--x:red;fill:var(--x)", "fill:rgb(1,2,3;stroke:red", "font-family:\"a;b\",serif;fill:red",
          "content:';';fill:red", "fill:red;stroke-width:1e999", "transform:translate(1px,1px) rotate(45deg)", "opacity:1;fill-opacity:1;stroke:none;stroke-width:5",
          "fill:red;" * 1000, "fill:url(#a)red", "fill:#", "stroke-dasharray:1,,2", "fill:red;" + "x" * 100000]
for i, s in enumerate(styles):
    add(f"styleattr_{i:02d}", wrap(f'<rect style="{s}" width="1" height="1"/><g style="{s}"><path d="M0 0h1" style="{s}"/></g>'))
add("styleattr_unicode_class_emoji_id", wrap('<style>.类名{fill:red}.😀{stroke:blue}#🚀{opacity:.5}#a\\:b{fill:green}.a\\.b{fill:pink}</style><rect class="类名 😀 a.b" id="🚀" width="1" height="1"/><rect id="a:b" width="1" height="1"/>'))

# --- <style> element edge cases ---
sty = [
    ("unclosed_brace", ".a{fill:red"), ("extra_close_brace", ".a{fill:red}}}"), ("media_nested", "@media (min-width:1px){.a{fill:red}@media print{.a{fill:blue}}}"),
    ("import", "@import url(foo.css);.a{fill:red}"), ("import_only", "@import 'x.css';"), ("unicode", ".a{fill:red;font-family:'日本語'}"),
    ("10k_empty_rules", "a{}" * 10000), ("escaped_selector", ".a\\:b{fill:red}.c\\#d{stroke:blue}#e\\.f{opacity:.5}"), ("comment_only", "/* only */"),
    ("comment_unterminated", "/* oops .a{fill:red}"), ("comment_inside_decl", ".a{fill:/*x*/red/*y*/;stroke:blue}"), ("string_with_braces", ".a{content:'{}';fill:red}"),
    ("string_with_semicolons", ".a{font-family:'a;b;c';fill:red}"), ("attr_selector", "rect[fill='red']{stroke:blue}rect[class~=a]{fill:green}"),
    ("pseudo", ".a:hover{fill:red}.a::before{content:'x'}rect:nth-child(2n+1){fill:blue}"), ("important", ".a{fill:red!important}"),
    ("keyframes", "@keyframes spin{from{transform:rotate(0)}to{transform:rotate(360deg)}}.a{animation:spin 1s infinite}"), ("font_face", "@font-face{font-family:x;src:url(x.woff)}.a{font-family:x}"),
    ("charset", "@charset \"utf-8\";.a{fill:red}"), ("namespace_rule", "@namespace svg url(http://www.w3.org/2000/svg);svg|rect{fill:red}"),
    ("supports", "@supports (fill:red){.a{fill:red}}"), ("selector_only", ".a"), ("brace_only", "{}"), ("semicolons_only", ";;;;"),
    ("nested_css", ".a{fill:red;&:hover{fill:blue}}"), ("universal_and_combinators", "*{fill:red}svg > g + rect ~ circle{stroke:blue}g rect{opacity:.5}"),
    ("id_and_class_same_name", "#a{fill:red}.a{fill:blue}a{fill:green}"), ("duplicate_rules", ".a{fill:red}.a{fill:blue}.a{fill:red}"),
    ("url_with_parens", ".a{fill:url(\"data:image/svg+xml,<svg xmlns='x'></svg>\")}"), ("cdata_marker_text", "<![CDATA[.a{fill:red}]]>"),
    ("html_comment_markers", "<!-- .a{fill:red} -->"), ("lt_gt_raw", ".a > .b{fill:red}"), ("huge_selector_list", ",".join(f".c{i}" for i in range(5000)) + "{fill:red}"),
    ("css_var_fallbacks", ":root{--c:red}.a{fill:var(--c,blue)}"), ("rule_with_type_attr", ".a{fill:red}"),
]
for n, s in sty:
    body = f'<style>{s}</style><g class="a"><rect class="a b c0 c1" id="a" width="1" height="1"/><circle class="a" r="1"/></g>'
    add(f"styleelem_{n}", wrap(body))
add("styleelem_type_attr_not_css", wrap('<style type="text/less">.a{fill:@c}</style><style media="print">.a{fill:red}</style><rect class="a" width="1" height="1"/>'))
add("styleelem_multiple_merge_with_media_attr", wrap('<style>.a{fill:red}</style><style media="(max-width:1px)">.a{fill:blue}</style><rect class="a" width="1" height="1"/>'))
add("styleelem_lt_amp_raw_in_style", wrap('<style>.a{fill:red}/* a < b & c */</style><rect class="a" width="1" height="1"/>'))

# --- viewBox weirdness ---
for i, vb in enumerate(["0 0 10", "0 0 10 10 10", "a b c d", "0 0 -10 -10", "0 0 0 0", "", "  ", "0,0,10,10", "0 0 1e999 1e-999", "0 0 nan inf", "0 0 10px 10px", "-0 -0 10.0000 10.0000", "0 0 10 10,", "0 0 .5 .5"]):
    add(f"viewbox_{i:02d}", f'<svg {NS} viewBox="{vb}" width="10" height="10"><rect width="1" height="1"/></svg>')

# --- numeric attrs with units ---
for i, v in enumerate(["1e3px", "50%", "1Q", "1em", "1ex", "1rem", "1vw", "1pt", "1pc", "1mm", "1cm", "1in", "auto", "1PX", "1 px", "px", "1e999px", "-0px", ".5px", "1.px", "1e-7", "0.0000001", "100000000000000000000", "1,5", "1.5.5", "1e3%", "1.0e+3px", "١٢٣", "NaN", "Infinity", "-", "+1", "0x10", "1_000"]):
    add(f"units_{i:02d}", f'<svg {NS} width="{v}" height="{v}" x="{v}" y="{v}"><rect x="{v}" y="{v}" width="{v}" height="{v}" rx="{v}" stroke-width="{v}" opacity="{v}" stroke-miterlimit="{v}"/><circle r="{v}" cx="{v}" cy="{v}"/><text x="{v}" font-size="{v}">t</text><stop offset="{v}"/></svg>')
add("dims_zero", f'<svg {NS} width="0" height="0"><rect width="0" height="0"/><circle r="0"/><ellipse rx="0" ry="5"/><line x1="1" y1="1" x2="1" y2="1"/></svg>')
add("dims_negative", f'<svg {NS} width="-10" height="-10" viewBox="0 0 -10 -10"><rect width="-1" height="-1"/><circle r="-1"/><ellipse rx="-1" ry="-1"/><rect width="5" height="5" rx="-1" ry="-1"/></svg>')
add("dims_auto", f'<svg {NS} width="auto" height="auto"><rect width="auto" height="auto"/><image width="auto" height="auto" href="a.png"/></svg>')
add("dims_percent_only", f'<svg {NS} width="100%" height="100%"><rect width="100%" height="100%" x="0%" y="0.0%"/><rect width="100.5%" height="99.999%"/></svg>')

# --- id/href edge ---
add("href_01_empty_fragment", wrap('<defs><g id="a"><rect width="1" height="1"/></g></defs><use href="#"/><use xlink:href="#" xmlns:xlink="http://www.w3.org/1999/xlink"/><rect fill="url(#)" width="1" height="1"/>'))
add("href_02_url_no_hash", wrap('<defs><linearGradient id="a"><stop offset="0"/></linearGradient></defs><rect fill="url(a)" width="1" height="1"/><rect fill="url(#a)" width="1" height="1"/><rect fill="url(  #a  )" width="1" height="1"/>'))
add("href_03_url_quoted", wrap('<defs><linearGradient id="a"><stop offset="0"/></linearGradient><linearGradient id="b"><stop offset="0"/></linearGradient></defs><rect fill="url(\'#a\')" width="1" height="1"/><rect fill="url(&quot;#b&quot;)" width="1" height="1"/>'))
add("href_04_circular_use", wrap(f'{XL}<defs><g id="a"><use xlink:href="#b"/></g><g id="b"><use xlink:href="#a"/></g></defs><use xlink:href="#a"/>'))
add("href_05_use_self_reference", wrap('<use id="a" href="#a"/><use id="b" href="#b" width="1" height="1"/>'))
add("href_06_symbol_inside_symbol", wrap('<symbol id="a"><symbol id="b"><rect width="1" height="1"/></symbol><use href="#b"/></symbol><use href="#a"/>'))
add("href_07_id_with_spaces_and_unicode", wrap('<defs><linearGradient id="my id"><stop offset="0"/></linearGradient><linearGradient id="日本"><stop offset="0"/></linearGradient><linearGradient id="a b"><stop offset="0"/></linearGradient></defs><rect fill="url(#my id)" width="1" height="1"/><rect fill="url(#日本)" width="1" height="1"/>'))
add("href_08_id_numeric_start_and_dup", wrap('<defs><linearGradient id="1"><stop offset="0"/></linearGradient><linearGradient id="1"><stop offset="0" stop-color="red"/></linearGradient><linearGradient id="-"><stop offset="0"/></linearGradient></defs><rect fill="url(#1)" width="1" height="1"/><rect fill="url(#-)" width="1" height="1"/>'))
add("href_09_id_ref_in_style_and_script", wrap('<style>#longid{fill:red} .x{fill:url(#grad1)}</style><script>document.getElementById("longid2")</script><defs><linearGradient id="grad1"><stop offset="0"/></linearGradient></defs><rect id="longid" class="x" width="1" height="1"/><rect id="longid2" width="1" height="1"/>'))
add("href_10_href_absolute_url_with_fragment", wrap('<use href="http://example.com/x.svg#a"/><use href="other.svg#b"/><rect fill="url(other.svg#g)" width="1" height="1"/><rect fill="url(#g)" width="1" height="1"/>'))
add("href_11_ref_from_begin_attr_and_mpath", wrap('<circle id="c" r="1"><animate id="an" attributeName="r" begin="t.click; an2.end" dur="1s"/><animateMotion><mpath href="#p"/></animateMotion></circle><path id="p" d="M0 0h1"/><rect id="t" width="1" height="1"/><animate id="an2" href="#c" attributeName="r" dur="1s"/>'))
add("href_12_gradient_href_chain_cycle", wrap('<defs><linearGradient id="a" href="#b"/><linearGradient id="b" href="#a"/></defs><rect fill="url(#a)" width="1" height="1"/>'))
add("href_13_clip_mask_filter_marker_refs", wrap('<defs><clipPath id="cp"><rect width="1" height="1"/></clipPath><mask id="m"><rect width="1" height="1"/></mask><filter id="f"><feGaussianBlur stdDeviation="1"/></filter><marker id="mk"><circle r="1"/></marker></defs><path d="M0 0h1" clip-path="url(#cp)" mask="url(#m)" filter="url(#f)" marker-start="url(#mk)" marker-mid="url(#mk)" marker-end="url(#mk)" style="clip-path:url(#cp);marker:url(#mk)"/>'))
add("href_14_fill_url_fallback_missing_id", wrap('<rect fill="url(#missing) red" stroke="url(#missing2)" width="1" height="1"/>'))
add("href_15_id_only_whitespace_or_empty", wrap('<rect id="" width="1" height="1"/><rect id=" " width="1" height="1"/><rect id="a" width="1" height="1"/><rect id="a" width="1" height="1"/>'))

# --- namespaces ---
add("ns_01_xlink_no_decl", f'<svg {NS}><use xlink:href="#a"/><rect id="a" width="1" height="1"/></svg>')
add("ns_02_unknown_prefix_undeclared", f'<svg {NS}><foo:bar/><rect foo:baz="1" width="1" height="1"/></svg>')
add("ns_03_unknown_ns_declared_used", f'<svg {NS} xmlns:foo="http://example.com/foo"><foo:bar><rect width="1" height="1"/></foo:bar><rect foo:baz="1" width="1" height="1"/></svg>')
add("ns_04_svg_prefixed", f'<svg:svg xmlns:svg="http://www.w3.org/2000/svg"><svg:rect width="1" height="1"/></svg:svg>')
add("ns_05_no_xmlns_at_all", '<svg><rect width="1" height="1"/></svg>')
add("ns_06_wrong_default_ns", '<svg xmlns="http://www.w3.org/1999/xhtml"><rect width="1" height="1"/></svg>')
add("ns_07_xmlns_redeclared_on_child", f'<svg {NS}><g xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="#a"/></g><rect id="a" width="1" height="1"/></svg>')
add("ns_08_xml_space_preserve", f'<svg {NS} xml:space="preserve"><text xml:space="preserve">  a   b  </text><text>  c   d  </text><text xml:space="default">  e   f  </text></svg>')
add("ns_09_xml_lang_base", f'<svg {NS} xml:lang="en" xml:base="http://x/"><text xml:lang="fr">a</text></svg>')
add("ns_10_xmlns_xml_explicit", f'<svg {NS} xmlns:xml="http://www.w3.org/XML/1998/namespace"><text xml:space="preserve"> a </text></svg>')
add("ns_11_default_ns_empty", f'<svg {NS}><g xmlns=""><rect width="1" height="1"/></g></svg>')
add("ns_12_editor_ns_with_data_elements", f'<svg {NS} xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" xmlns:sketch="http://www.bohemiancoding.com/sketch/ns"><sodipodi:namedview/><rect inkscape:label="x" sketch:type="y" width="1" height="1"/><text>inkscape:label</text></svg>')
add("ns_13_xlink_href_and_href_both", f'<svg {NS} {XL}><use xlink:href="#a" href="#b"/><rect id="a" width="1" height="1"/><rect id="b" width="1" height="1"/></svg>')
add("ns_14_attr_with_colon_in_value_and_xmlns_prefix_name", f'<svg {NS} xmlns:a="urn:a" xmlns:xmlns="urn:x"><a:rect a:x="a:b" width="1" height="1"/></svg>')

# --- nested svg / foreignObject / script / text ---
add("nested_svg_01_transforms", f'<svg {NS} viewBox="0 0 100 100"><svg x="10" y="10" width="50" height="50" viewBox="0 0 10 10" transform="rotate(45)"><g transform="scale(2)"><rect width="1" height="1"/></g></svg><svg x="0" y="0"><rect width="1" height="1"/></svg></svg>')
add("nested_svg_02_empty_inner", f'<svg {NS}><svg/><svg></svg><svg width="0" height="0"><rect width="1" height="1"/></svg></svg>')
add("foreign_01_html", wrap('<foreignObject width="100" height="100"><div xmlns="http://www.w3.org/1999/xhtml" style="color:red;  font-size: 12px;"><p>Hello <b>world</b> &amp; <br/> friends</p><input type="text" value=""/><span class="x"/></div></foreignObject>'))
add("foreign_02_html_with_svg_inside", wrap(f'<foreignObject width="100" height="100"><div xmlns="http://www.w3.org/1999/xhtml"><svg {NS}><g><rect width="1" height="1" opacity="1" fill="black"/></g></svg></div></foreignObject>'))
add("foreign_03_html_with_style_and_script", wrap('<foreignObject width="100" height="100"><div xmlns="http://www.w3.org/1999/xhtml"><style>p{color:red}</style><script>var a = 1;</script><p id="unused" class="unused">x</p></div></foreignObject>'))
add("foreign_04_empty_html_containers", wrap('<foreignObject width="100" height="100"><div xmlns="http://www.w3.org/1999/xhtml"><div></div><span></span><p>  </p></div></foreignObject>'))
add("script_01_lt_amp_raw", wrap('<script>if (a &lt; b &amp;&amp; c) { d = "&lt;rect/&gt;"; }</script><rect width="1" height="1"/>'))
add("script_02_cdata_with_ids", wrap('<script><![CDATA[ document.querySelector("#target"); var x = 1 < 2 && 3 > 1; ]]></script><rect id="target" width="1" height="1"/><rect id="other" width="1" height="1"/>'))
add("script_03_onclick_handlers", wrap('<rect id="r" onclick="alert(\'#r\')" onload="this.setAttribute(\'fill\',\'red\')" width="1" height="1" fill="black" opacity="1"/>'))
add("script_04_href_javascript", wrap('<a href="javascript:void(0)" xlink:href="javascript:alert(1)" xmlns:xlink="http://www.w3.org/1999/xlink"><rect width="1" height="1"/></a>'))
add("script_05_empty_script_and_style", wrap('<script></script><script/><style></style><style/><rect width="1" height="1"/>'))
add("text_01_mixed_entities", wrap('<text>&amp;&lt;&gt;&quot;&apos;&#169;&#xA9; &amp;amp; &amp;#38;</text>'))
add("text_02_whitespace_significant", wrap('<text>  Hello   <tspan> world </tspan>  !  </text>'))
add("text_03_nested_tspan_empty", wrap('<text><tspan><tspan></tspan></tspan></text><text> </text><text><tspan> </tspan></text><text>x<tspan/></text>'))
add("text_04_textpath", wrap('<defs><path id="p" d="M0 0 C1 1 2 2 3 3"/></defs><text><textPath href="#p">curve</textPath></text>'))
add("text_05_only_whitespace_xml_space", wrap('<text xml:space="preserve">   </text><text xml:space="preserve"><tspan>   </tspan></text>'))
add("text_06_control_chars", wrap('<text>a\u0001b\u0008c\u000bd\u001fe\u007f</text>'))
add("text_07_rtl_and_combining", wrap('<text>مرحبا é ‍ ‏ ﻿</text>'))
add("text_08_in_g_no_text_elem", wrap('<g>loose text</g><g>  </g><defs>loose</defs>'))
add("text_09_text_in_title_desc", wrap('<title>  Title  </title><desc>Created with Sketch.</desc><desc>Real description</desc><desc></desc><rect width="1" height="1"/>'))
add("text_10_switch_systemLanguage", wrap('<switch><text systemLanguage="en">en</text><text systemLanguage="fr">fr</text><text>default</text></switch>'))

# --- misc semantic traps ---
add("misc_01_attr_dup_name", wrap('<rect width="1" width="2" height="1"/>'))
add("misc_02_attr_no_value", wrap('<rect width="1" height="1" hidden/>'))
add("misc_03_attr_single_quotes_with_double_inside", wrap('<rect width="1" height="1" title=\'say "hi"\'/>'))
add("misc_04_attr_newlines_in_value", wrap('<path d="M0 0\n\nL1 1\r\nL2 2" class="a\nb\tc" fill="  red  "/>'))
add("misc_05_elem_names_uppercase_and_weird", wrap('<RECT width="1" height="1"/><Rect width="1" height="1"/><linearGRADIENT id="g"><STOP offset="0"/></linearGRADIENT><rect fill="url(#g)" width="1" height="1"/>'))
add("misc_06_unknown_elements_with_children", wrap('<foo><bar><rect width="1" height="1"/></bar></foo><foo/>'))
add("misc_07_animation_targets", wrap('<rect id="r" width="1" height="1" fill="black" opacity="1"><animate attributeName="opacity" from="1" to="0" dur="1s"/><set attributeName="fill" to="red"/><animateTransform attributeName="transform" type="rotate" from="0" to="360" dur="1s"/></rect><g id="h" display="none"><animate attributeName="display" to="inline" dur="1s"/><rect width="1" height="1"/></g><rect width="0" height="0"><animate attributeName="width" to="10" dur="1s"/></rect>'))
add("misc_08_opacity_visibility_hidden_kept", wrap('<rect opacity="0" width="1" height="1"/><rect visibility="hidden" width="1" height="1"/><g display="none"><rect width="1" height="1"/></g><rect display="none" id="keep" width="1" height="1"/>'))
add("misc_09_mixed_case_attrs", wrap('<rect WIDTH="1" Height="1" viewbox="0 0 1 1" Fill="red" FILL-OPACITY="1" stroke-Width="2"/>'))
add("misc_10_emoji_everywhere", wrap('<g id="😀" class="🎉" transform="translate(1,1)"><text font-family="😀">😀🎉</text><rect id="x" fill="url(#😀)" width="1" height="1"/></g>'))
add("misc_11_long_nested_tspans_depth", wrap('<text>' + '<tspan>' * 3000 + 'x' + '</tspan>' * 3000 + '</text>'))
add("misc_12_zero_width_lines_and_markers", wrap('<defs><marker id="m"><circle r="1"/></marker></defs><line x1="0" y1="0" x2="0" y2="0" marker-end="url(#m)"/><path d="M5 5" marker-start="url(#m)"/><path d="M5 5Z" stroke="red" stroke-linecap="round"/><polyline points="1,1" stroke="red"/>'))
add("misc_13_percent_in_gradients_and_offsets", wrap('<linearGradient id="g" x1="0%" y1="-10%" x2="100.0%" y2="150%"><stop offset="0%"/><stop offset="50.000%"/><stop offset="100%" stop-color="red"/><stop offset="1.5"/><stop offset="-1"/></linearGradient><rect fill="url(#g)" width="1" height="1"/>'))
add("misc_14_stroke_dasharray_oddities", wrap('<path d="M0 0h10" stroke="red" stroke-dasharray="1,,2" stroke-dashoffset="1e999" stroke-dasharray-x="1"/><path d="M0 0h10" stroke="red" stroke-dasharray="none"/><path d="M0 0h10" stroke="red" stroke-dasharray="0" stroke-width="0"/>'))
add("misc_15_use_with_transform_and_xy", wrap('<defs><rect id="r" width="1" height="1"/></defs><use href="#r" x="1e999" y="nan" transform="translate(1,2)" width="0" height="0"/>'))
add("misc_16_attr_values_with_gt_lt_in_them", wrap('<rect width="1" height="1" data-x="a&gt;b&lt;c" aria-label="&lt;3"/><text data-y="1 &gt; 0">x</text>'))
add("misc_17_image_with_data_uri", wrap('<image width="1" height="1" href="data:image/svg+xml;utf8,&lt;svg xmlns=&quot;http://www.w3.org/2000/svg&quot;&gt;&lt;/svg&gt;" xlink:href="data:image/png;base64,iVBORw0KGgo=" xmlns:xlink="http://www.w3.org/1999/xlink"/>'))
add("misc_18_pattern_mask_clippath_empty", wrap('<defs><pattern id="p"/><mask id="m"/><clipPath id="c"/><filter id="f"/><linearGradient id="g"/><marker id="k"/><symbol id="s"/></defs><rect fill="url(#p)" mask="url(#m)" clip-path="url(#c)" filter="url(#f)" stroke="url(#g)" marker-end="url(#k)" width="1" height="1"/><use href="#s"/>'))
add("misc_19_invalid_utf8_like_surrogates_text", wrap('<text>😀 ok</text>'))
add("misc_20_crlf_everywhere", f'<svg\r\n{NS}\r\n>\r\n<rect\r\nwidth="1"\r\nheight="1"\r\n/>\r\n</svg>\r\n')
add("misc_21_tabs_and_formfeeds_between_attrs", f'<svg {NS}\t><rect\twidth="1"\theight="1"\t/>\t</svg>')
add("misc_22_very_long_id_chain_cleanup", wrap('<defs>' + ''.join(f'<linearGradient id="{"x"*(i+1)}"><stop offset="0"/></linearGradient>' for i in range(60)) + '</defs>' + ''.join(f'<rect fill="url(#{"x"*(i+1)})" width="1" height="1"/>' for i in range(60))))
add("misc_23_group_with_only_comment_child", wrap('<g><!-- c --></g><g><?pi x?></g><g><![CDATA[ ]]></g><defs><!-- c --></defs>'))
add("misc_24_svg_root_attrs_weird", f'<svg {NS} version="2.0" baseProfile="tiny" x="0" y="0" width="100" height="100" preserveAspectRatio="xMidYMid  meet" zoomAndPan="magnify" contentScriptType="text/ecmascript" style="" class="" id=""><rect width="1" height="1"/></svg>')
add("misc_25_whitespace_only_attr_values", wrap('<rect width=" " height="\n" fill="\t" style=" " class="  " transform="  " d="  "/>'))

# --- malformed / tricky stylesheets (structural-CSS classifier, #47) ---
add("css_01_unclosed_rule", wrap('<style>.a{fill:red</style><g><rect class="a" width="1" height="1"/></g>'))
add("css_02_stray_close_brace", wrap('<style>}}.a{fill:red}}</style><g><rect class="a" width="1" height="1"/></g>'))
add("css_03_import", wrap('<style>@import url(x.css);.a{fill:red}</style><g><rect class="a" width="1" height="1"/></g>'))
add("css_04_nesting", wrap('<style>.a{fill:red;&amp; rect{fill:blue}}</style><g class="a"><rect width="1" height="1"/></g>'))
add("css_05_escapes", wrap('<style>.\\31 a\\{fill:red}.b\\</style><g><rect class="1a{" width="1" height="1"/></g>'))
add("css_06_brace_in_string", wrap('<style>.a{content:"}g rect{"}</style><g><ellipse class="a" rx="1" ry="1"/></g>'))
add("css_07_geometry_rule", wrap('<style>.a{width:20px;rx:3px}.a:hover{fill:red}</style><g opacity=".5"><rect class="a" width="1" height="1"/><ellipse class="a" rx="1" ry="1"/></g>'))
add("css_08_unicode_selectors", wrap('<style>.é:hover{fill:red}#ü:target{fill:blue}</style><g><rect class="é" id="ü" width="1" height="1"/></g>'))
add("css_09_lone_colons", wrap('<style>.a:{fill:red}.b::{fill:red}:{}</style><g><rect class="a b" width="1" height="1"/></g>'))
add("css_10_deep_media", wrap('<style>' + '@media all{' * 500 + '.a{fill:red}' + '}' * 500 + '</style><g><rect class="a" width="1" height="1"/></g>'))

for name, content in cases.items():
    with open(os.path.join(OUT, name + ".svg"), "w", encoding="utf-8", errors="surrogatepass") as f:
        f.write(content)
print(len(cases), "files written")
