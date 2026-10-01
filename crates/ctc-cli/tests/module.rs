use serde_json::Value;

pub mod common;

use common::{Project, lines};

// module field

fn module_project(template_name: &str, template: &str, source_name: &str, source: &str) -> Project {
    let project = Project::new();
    project.write(&format!(".ctmpl/{template_name}"), template);
    project.write(&format!("src/{source_name}"), source);
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-module",
            "template": format!(".ctmpl/{template_name}"),
            "include": [format!("src/{source_name}")],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    project
}

const FS_TEMPLATE: &str =
    "{{ Import | field(\"module\", \"matches\", \"^(node:)?fs(/promises)?$\") }}\n";

const TS_SOURCE: &str = "import fs from \"fs\";\nimport { readFileSync } from \"node:fs\";\nimport * as p from 'fs/promises';\nimport path from \"path\";\nimport \"node:fs/promises\";\nimport legacy = require(\"fs\");\nconst a = require(\"fs\");\nconst b = require(\"fs\").readFileSync(\"x\");\nconst c = await import(\"fs\");\nimport(\"fs\").then((m) => m);\nexport * from \"fs\";\nexport { x } from \"node:fs\";\nimport type { Stats } from \"fs\";\nrequire(name);\nrequire.resolve(\"fs\");\nexport const ok = 1;\n";

#[test]
fn ts_module_field_finds_every_way_to_load_a_module() {
    let project = module_project("m.ts.ctmpl", FS_TEMPLATE, "a.ts", TS_SOURCE);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(lines(&report), vec![1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
}

#[test]
fn ts_module_field_does_not_flag_other_modules_or_computed_names() {
    let project = module_project(
        "m.ts.ctmpl",
        FS_TEMPLATE,
        "a.ts",
        "import path from \"path\";\nimport { a } from \"./fs\";\nimport { b } from \"fs-extra\";\nconst n = \"fs\";\nrequire(n);\nrequire.resolve(\"fs\");\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!((code, lines(&report)), (0, Vec::<u64>::new()));
}

#[test]
fn ts_module_field_bans_a_module_by_prefix() {
    let project = module_project(
        "m.ts.ctmpl",
        "{{ Import | field(\"module\", \"matches\", \"^jquery\") }}\n",
        "a.ts",
        "import $ from \"jquery\";\nimport x from \"jquery/dist/jquery\";\nimport y from \"lodash\";\n",
    );
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![1, 2]);
}

#[test]
fn ts_module_field_works_with_equal_and_kind() {
    let project = module_project(
        "m.ts.ctmpl",
        "{{ Import | kind(\"ImportDeclaration\") | field(\"module\", \"equal\", \"fs\") }}\n",
        "a.ts",
        "import fs from \"fs\";\nconst a = require(\"fs\");\nimport x from \"node:fs\";\n",
    );
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![1]);
}

#[test]
fn ts_templates_that_name_a_module_still_match_by_structure() {
    let project = module_project(
        "m.ts.ctmpl",
        "import {{ Bound }} from \"jquery\";\n",
        "a.ts",
        "import $ from \"jquery\";\nimport y from \"lodash\";\n",
    );
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![1]);
}

#[test]
fn ts_only_allowed_file_may_load_the_module() {
    let project = Project::new();
    project.write(".ctmpl/fs.ts.ctmpl", FS_TEMPLATE);
    project.write(
        "src/infra/FileSystem.ts",
        "import * as fs from \"node:fs\";\nexport class FileSystem { exists(p: string) { return fs.existsSync(p); } }\n",
    );
    project.write(
        "src/report.ts",
        "import { FileSystem } from \"./infra/FileSystem\";\nimport { readFileSync } from \"fs\";\nexport const r = readFileSync;\n",
    );
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "use-filesystem",
            "template": ".ctmpl/fs.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "exclude": ["src/infra/FileSystem.ts"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["source"]["path"], "src/report.ts");
    assert_eq!(diagnostics[0]["source"]["start"]["line"], 2);
}

#[test]
fn cpp_module_field_holds_the_included_file() {
    let project = module_project(
        "m.cpp.ctmpl",
        "{{ Include | field(\"module\", \"matches\", \"^(iostream|cstdio|legacy/.*)$\") }}\n",
        "a.cpp",
        "#include <iostream>\n#include <vector>\n#include \"legacy/util.h\"\n#include \"cstdio\"\n#include \"own.h\"\nint main() { return 0; }\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(lines(&report), vec![1, 3, 4]);
}

#[test]
fn rust_module_field_holds_the_use_path() {
    let project = module_project(
        "m.rs.ctmpl",
        "{{ Use | field(\"module\", \"matches\", \"^(std::fs|tokio::fs)\") }}\n",
        "a.rs",
        "use std::fs;\nuse std::fs::{self, File};\nuse tokio::fs::read;\nuse std::collections::HashMap;\nuse crate::fs;\nfn main() {}\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(lines(&report), vec![1, 2, 3]);
}

#[test]
fn module_field_needs_a_real_field_name() {
    let project = module_project(
        "m.ts.ctmpl",
        "{{ Import | field(\"modul\", \"matches\", \"x\") }}\n",
        "a.ts",
        "import a from \"a\";\n",
    );
    let (code, report): (i32, Value) = project.run(&[]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CTC2004");
}
