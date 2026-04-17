//! C++ public-API extraction via tree-sitter.
//!
//! C++ doesn't have a built-in module/visibility system at the global
//! scope — every top-level declaration in a header IS the API. The
//! v0.1 query captures: free functions, class/struct/enum/union
//! declarations, namespace declarations. Member functions inside
//! classes are not visibility-filtered (private/protected/public
//! sections require tracking the surrounding access specifier, which
//! the v0.1 query doesn't).
//!
//! Captured signatures:
//!   - `function_definition` (free function or class method body) → `fn:<name>`
//!   - `class_specifier` → `class:<name>`
//!   - `struct_specifier` → `struct:<name>`
//!   - `enum_specifier` → `enum:<name>`
//!   - `union_specifier` → `union:<name>`
//!   - `namespace_definition` → `namespace:<name>`
//!
//! Caveats:
//!   - Templates are not specialized in the signature (no template-arg
//!     fingerprinting in v0.1).
//!   - Anonymous namespaces / classes / structs produce no captures.
//!   - Private/protected member functions are over-counted.

use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_cpp::LANGUAGE.into())
}

fn query() -> &'static Query {
    static Q: OnceLock<Query> = OnceLock::new();
    Q.get_or_init(|| {
        Query::new(
            language(),
            r#"
            (function_definition
              declarator: (function_declarator
                declarator: (identifier) @fn))
            (function_definition
              declarator: (function_declarator
                declarator: (qualified_identifier
                  name: (identifier) @fn)))
            (class_specifier name: (type_identifier) @class)
            (struct_specifier name: (type_identifier) @struct)
            (enum_specifier name: (type_identifier) @enum)
            (union_specifier name: (type_identifier) @union)
            (namespace_definition name: (namespace_identifier) @namespace)
            "#,
        )
        .expect("static cpp query compiles")
    })
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let q = query();
    let fn_idx = q.capture_index_for_name("fn")?;
    let class_idx = q.capture_index_for_name("class")?;
    let struct_idx = q.capture_index_for_name("struct")?;
    let enum_idx = q.capture_index_for_name("enum")?;
    let union_idx = q.capture_index_for_name("union")?;
    let namespace_idx = q.capture_index_for_name("namespace")?;

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
            } else if capture.index == struct_idx {
                "struct"
            } else if capture.index == enum_idx {
                "enum"
            } else if capture.index == union_idx {
                "union"
            } else if capture.index == namespace_idx {
                "namespace"
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
    fn parses_classes_and_functions() {
        let src = r#"
class Widget {
public:
    int render();
};

int Widget::render() { return 0; }

int free_function(int x) { return x + 1; }

namespace utils {
    int helper() { return 42; }
}
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"class:Widget".to_string()));
        assert!(items.contains(&"fn:render".to_string()));
        assert!(items.contains(&"fn:free_function".to_string()));
        assert!(items.contains(&"namespace:utils".to_string()));
        assert!(items.contains(&"fn:helper".to_string()));
    }

    #[test]
    fn parses_struct_enum_union() {
        let src = r#"
struct Point { int x; int y; };
enum Color { RED, GREEN, BLUE };
union Value { int i; float f; };
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"struct:Point".to_string()));
        assert!(items.contains(&"enum:Color".to_string()));
        assert!(items.contains(&"union:Value".to_string()));
    }

    #[test]
    fn cosmetic_rewrite_yields_zero_delta() {
        let a = "int foo() { return 0; }";
        let b = "int foo() {\n    // a comment\n    return 0;\n}\n";
        assert_eq!(public_api_delta(a, b, Language::Cpp), 0);
    }

    #[test]
    fn adding_function_counts_as_one() {
        let a = "int foo() { return 0; }";
        let b = "int foo() { return 0; }\nint bar() { return 1; }";
        assert_eq!(public_api_delta(a, b, Language::Cpp), 1);
    }
}
