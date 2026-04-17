//! TypeScript public-item extraction via tree-sitter.
//!
//! TypeScript's public surface is explicit: declarations preceded by
//! the `export` keyword. We use the TSX grammar (a superset of TS)
//! for both `.ts` and `.tsx` so a single module handles both.
//!
//! v0.1 captures:
//!   - `export function name(...)`
//!   - `export class Name`
//!   - `export interface Name`
//!   - `export type Name = ...`
//!   - `export const|let name = ...` (lexical declarations)
//!
//! Not yet captured: default exports (`export default ...`), re-exports
//! (`export { x } from '...'`), namespace exports. These matter less
//! for API-delta magnitude and can be added as the signal quality
//! demands.

use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    // TSX parses TypeScript too; using it as the single grammar keeps
    // the backend unified across .ts and .tsx files.
    LANG.get_or_init(|| tree_sitter_typescript::LANGUAGE_TSX.into())
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
              (class_declaration name: (type_identifier) @class))
            (export_statement
              (interface_declaration name: (type_identifier) @interface))
            (export_statement
              (type_alias_declaration name: (type_identifier) @type))
            (export_statement
              (lexical_declaration
                (variable_declarator name: (identifier) @const)))
            "#,
        )
        .expect("static typescript query compiles")
    })
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let q = query();
    let fn_idx = q.capture_index_for_name("fn")?;
    let class_idx = q.capture_index_for_name("class")?;
    let interface_idx = q.capture_index_for_name("interface")?;
    let type_idx = q.capture_index_for_name("type")?;
    let const_idx = q.capture_index_for_name("const")?;

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
            } else if capture.index == interface_idx {
                "interface"
            } else if capture.index == type_idx {
                "type"
            } else if capture.index == const_idx {
                "const"
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
    use super::super::{Language, public_api_delta};
    use super::parse;

    #[test]
    fn parses_exported_functions_and_classes() {
        let src = r#"
export function greet(name: string): string {
    return `hello ${name}`;
}

function internal(): void {}

export class Widget {
    render(): string { return ""; }
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
    fn parses_exported_interfaces_and_type_aliases() {
        let src = r#"
export interface User {
    id: number;
    name: string;
}

export type UserId = string | number;

interface Private {}
type Internal = unknown;
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"interface:User".to_string()));
        assert!(items.contains(&"type:UserId".to_string()));
        assert!(!items.iter().any(|i| i.contains("Private")));
        assert!(!items.iter().any(|i| i.contains("Internal")));
    }

    #[test]
    fn exported_const_is_captured() {
        let src = "export const CONFIG = { port: 3000 };\nconst internal = 42;\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"const:CONFIG".to_string()));
        assert!(!items.iter().any(|i| i.contains("internal")));
    }

    #[test]
    fn cosmetic_rewrite_yields_zero_delta() {
        let a = "export function foo(): void {}\n";
        let b = "export function foo(): void {\n    // no-op\n}\n";
        assert_eq!(public_api_delta(a, b, Language::TypeScript), 0);
    }

    #[test]
    fn adding_exported_fn_counts_as_one() {
        let a = "export function foo() {}\n";
        let b = "export function foo() {}\nexport function bar() {}\n";
        assert_eq!(public_api_delta(a, b, Language::TypeScript), 1);
    }

    #[test]
    fn private_additions_do_not_count() {
        let a = "export function keep() {}\n";
        let b = "export function keep() {}\nfunction inner() {}\n";
        assert_eq!(public_api_delta(a, b, Language::TypeScript), 0);
    }
}
