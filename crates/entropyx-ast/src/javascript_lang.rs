//! JavaScript public-API extraction via tree-sitter.
//!
//! JavaScript's API surface is explicit ES-module exports. v0.1
//! captures `export function`, `export class`, and
//! `export const|let|var`. CommonJS (`module.exports = ...`,
//! `exports.name = ...`) is not yet tracked — add a query clause when
//! legacy codebases demand it.
//!
//! The grammar is very close to TypeScript's (TS is a superset), so
//! the query mirrors the TS one minus `interface_declaration` and
//! `type_alias_declaration`, which don't exist in plain JS.

use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_javascript::LANGUAGE.into())
}

fn query() -> &'static Query {
    static Q: OnceLock<Query> = OnceLock::new();
    Q.get_or_init(|| {
        Query::new(
            language(),
            r#"
            (export_statement
              (function_declaration name: (identifier) @fn))
            (export_statement
              (class_declaration name: (identifier) @class))
            (export_statement
              (lexical_declaration
                (variable_declarator name: (identifier) @const)))
            (export_statement
              (variable_declaration
                (variable_declarator name: (identifier) @var)))
            "#,
        )
        .expect("static javascript query compiles")
    })
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let q = query();
    let fn_idx = q.capture_index_for_name("fn")?;
    let class_idx = q.capture_index_for_name("class")?;
    let const_idx = q.capture_index_for_name("const")?;
    let var_idx = q.capture_index_for_name("var")?;

    let mut cursor = QueryCursor::new();
    let mut items = Vec::new();
    let src_bytes = source.as_bytes();
    let mut matches = cursor.matches(q, tree.root_node(), src_bytes);
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let Ok(name) = capture.node.utf8_text(src_bytes) else {
                continue;
            };
            let kind = if capture.index == fn_idx {
                "fn"
            } else if capture.index == class_idx {
                "class"
            } else if capture.index == const_idx {
                "const"
            } else if capture.index == var_idx {
                "var"
            } else {
                continue;
            };
            items.push(format!("{kind}:{name}"));
        }
    }

    items.sort();
    items.dedup();
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::super::{public_api_delta, Language};
    use super::parse;

    #[test]
    fn parses_exported_functions_and_classes() {
        let src = r#"
export function greet(name) { return `hello ${name}`; }
function internal() {}
export class Widget {
    render() { return ""; }
}
class Hidden {}
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:greet".to_string()));
        assert!(items.contains(&"class:Widget".to_string()));
        assert!(!items.iter().any(|i| i.contains("internal")));
        assert!(!items.iter().any(|i| i.contains("Hidden")));
    }

    #[test]
    fn exported_const_and_let_are_captured() {
        let src = "export const CONFIG = { port: 3000 };\nexport let counter = 0;\nconst internal = 42;\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"const:CONFIG".to_string()));
        assert!(items.contains(&"const:counter".to_string()));
        assert!(!items.iter().any(|i| i.contains("internal")));
    }

    #[test]
    fn cosmetic_rewrite_yields_zero_delta() {
        let a = "export function foo() {}\n";
        let b = "export function foo() {\n    // no-op\n}\n";
        assert_eq!(public_api_delta(a, b, Language::JavaScript), 0);
    }

    #[test]
    fn adding_exported_fn_counts_as_one() {
        let a = "export function foo() {}\n";
        let b = "export function foo() {}\nexport function bar() {}\n";
        assert_eq!(public_api_delta(a, b, Language::JavaScript), 1);
    }

    #[test]
    fn private_additions_do_not_count() {
        let a = "export function keep() {}\n";
        let b = "export function keep() {}\nfunction inner() {}\n";
        assert_eq!(public_api_delta(a, b, Language::JavaScript), 0);
    }

    #[test]
    fn module_exports_commonjs_not_yet_tracked() {
        // Document v0.1 limitation: CJS-style exports are not
        // captured. When legacy codebases demand it, extend the query
        // with a clause for `assignment_expression` targeting
        // `module.exports` or `exports.foo`.
        let a = "function hello() {}\nmodule.exports = { hello };\n";
        let b = "function hello() {}\nfunction world() {}\nmodule.exports = { hello, world };\n";
        assert_eq!(
            public_api_delta(a, b, Language::JavaScript),
            0,
            "CJS exports not yet tracked (v0.1 limitation)",
        );
    }
}
