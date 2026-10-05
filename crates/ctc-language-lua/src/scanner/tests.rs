use std::path::Path;

use super::scan_placeholders;

fn captures(source: &str) -> Vec<&str> {
    scan_placeholders(source, Path::new("rule.lua.ctmpl"))
        .unwrap()
        .into_iter()
        .map(|range| &source[range])
        .collect()
}

#[test]
fn ignores_lua_comments_and_strings() {
    let source = r#"#!/usr/bin/env lua {{ shebang }}
-- {{ line }}
--[[ {{ block }} ]]
--[==[ ]] {{ still a comment }} ]==]
local a = "{{ double }}"
local b = '{{ single }}'
local c = [[ {{ long }} ]]
local d = [=[ ]] {{ long level one }} ]=]
local e = "\"{{ escaped quote }}"
{{ Capture }}
"#;
    assert_eq!(captures(source), vec!["{{ Capture }}"]);
}

#[test]
fn reads_placeholders_inside_code_and_filters() {
    let source = "local {{ Name | matches(\"--x\") }} = {{ Value }} -- {{ no }}\n";
    assert_eq!(
        captures(source),
        vec!["{{ Name | matches(\"--x\") }}", "{{ Value }}"]
    );
}

#[test]
fn keeps_scanning_after_a_raw_line_break_in_a_string() {
    let source = "local s = \"open\n{{ Capture }}\n";
    assert_eq!(captures(source), vec!["{{ Capture }}"]);
}

#[test]
fn treats_a_leading_byte_order_mark_as_white_space() {
    let source = "\u{feff}{{ Capture }}\n";
    assert_eq!(captures(source), vec!["{{ Capture }}"]);
}

#[test]
fn reports_an_unclosed_placeholder() {
    let diagnostics =
        scan_placeholders("local x = {{ Name\n", Path::new("rule.lua.ctmpl")).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2001");
}

#[test]
fn reads_a_nested_table_constructor_as_a_placeholder_candidate() {
    let source = "local t = {{1, 2}, {3}}\n";
    assert_eq!(captures(source), vec!["{{1, 2}, {3}}"]);
}
