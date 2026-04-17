//! Python public-item extraction via tree-sitter.
//!
//! Python's "public" convention is nominative: identifiers not starting
//! with `_` are the module's API surface. We capture every top-level
//! or class-scoped function and class definition, then filter by that
//! leading-underscore rule.
//!
//! v0.1 captures `def`, `async def`, and `class`. Module-level variable
//! bindings (ALL_CAPS constants, exported singletons) are not yet
//! tracked — add a `(expression_statement (assignment ...))` clause
//! when signal quality demands it.

use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_python::LANGUAGE.into())
}

fn query() -> &'static Query {
    static Q: OnceLock<Query> = OnceLock::new();
    Q.get_or_init(|| {
        Query::new(
            language(),
            "(function_definition name: (identifier) @fn)
             (class_definition name: (identifier) @class)",
        )
        .expect("static python query compiles")
    })
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let q = query();
    let fn_idx = q.capture_index_for_name("fn")?;
    let class_idx = q.capture_index_for_name("class")?;

    let mut cursor = QueryCursor::new();
    let mut items = Vec::new();
    let src_bytes = source.as_bytes();
    let mut matches = cursor.matches(q, tree.root_node(), src_bytes);
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let Ok(name) = capture.node.utf8_text(src_bytes) else {
                continue;
            };
            if name.starts_with('_') {
                continue;
            }
            let kind = if capture.index == fn_idx {
                "fn"
            } else if capture.index == class_idx {
                "class"
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
    fn parses_public_defs_and_classes() {
        let src = r#"
def public_function():
    pass

def _private_function():
    pass

class PublicClass:
    pass

class _InternalClass:
    pass
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:public_function".to_string()));
        assert!(items.contains(&"class:PublicClass".to_string()));
        assert!(!items.iter().any(|i| i.contains("private")));
        assert!(!items.iter().any(|i| i.contains("Internal")));
    }

    #[test]
    fn async_def_also_counts() {
        // tree-sitter-python models `async def` as a `function_definition`
        // too, so our query catches it without a special case.
        let src = "async def fetch():\n    pass\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:fetch".to_string()));
    }

    #[test]
    fn cosmetic_rewrite_yields_zero_delta() {
        let a = "def foo():\n    pass\n";
        let b = "def foo():\n    # a comment\n    pass\n";
        assert_eq!(public_api_delta(a, b, Language::Python), 0);
    }

    #[test]
    fn adding_public_def_counts_as_one() {
        let a = "def foo():\n    pass\n";
        let b = "def foo():\n    pass\n\ndef bar():\n    pass\n";
        assert_eq!(public_api_delta(a, b, Language::Python), 1);
    }

    #[test]
    fn private_additions_do_not_count() {
        let a = "def keep():\n    pass\n";
        let b = "def keep():\n    pass\n\ndef _helper():\n    pass\n";
        assert_eq!(public_api_delta(a, b, Language::Python), 0);
    }

    #[test]
    fn class_methods_are_captured_by_dedup() {
        // Our v0.1 query doesn't scope methods to their class, so two
        // classes with identical public method names collide in the
        // signature set. Acceptable trade-off until per-class scoping lands.
        let src = r#"
class A:
    def shared(self):
        pass

class B:
    def shared(self):
        pass
"#;
        let items = parse(src).expect("parse");
        // Both classes contribute fn:shared; dedup collapses to one.
        let shared_count = items.iter().filter(|s| *s == "fn:shared").count();
        assert_eq!(shared_count, 1);
    }
}
