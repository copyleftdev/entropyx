//! Ruby public-API extraction via tree-sitter.
//!
//! Ruby visibility is largely cultural: methods named with a leading
//! underscore are convention-private; classes/modules with non-
//! underscore names are public. The `private`/`protected` keywords
//! affect runtime accessibility but aren't always at the declaration
//! level — for v0.1 we use the underscore-prefix heuristic, matching
//! the Python backend's approach.
//!
//! Captured: `def name`, `class Name`, `module Name` whose name
//! identifier doesn't start with `_`. The `def self.name` (singleton/
//! class method) form is also captured because the inner `name` is
//! still a method_definition node in tree-sitter-ruby.

use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_ruby::LANGUAGE.into())
}

fn query() -> &'static Query {
    static Q: OnceLock<Query> = OnceLock::new();
    Q.get_or_init(|| {
        Query::new(
            language(),
            "(method name: (identifier) @method)
             (singleton_method name: (identifier) @method)
             (class name: (constant) @class)
             (module name: (constant) @module)",
        )
        .expect("static ruby query compiles")
    })
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let q = query();
    let method_idx = q.capture_index_for_name("method")?;
    let class_idx = q.capture_index_for_name("class")?;
    let module_idx = q.capture_index_for_name("module")?;

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
            let kind = if capture.index == method_idx {
                "method"
            } else if capture.index == class_idx {
                "class"
            } else if capture.index == module_idx {
                "module"
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
    fn parses_methods_and_classes() {
        let src = r#"
class Widget
  def render
    "ok"
  end

  def _internal
    nil
  end
end

module Helpers
  def self.greet(name)
    "hi #{name}"
  end
end

class _Hidden
end
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"class:Widget".to_string()));
        assert!(items.contains(&"method:render".to_string()));
        assert!(items.contains(&"module:Helpers".to_string()));
        assert!(items.contains(&"method:greet".to_string()));
        assert!(!items.iter().any(|i| i.contains("_internal")));
        assert!(!items.iter().any(|i| i.contains("_Hidden")));
    }

    #[test]
    fn cosmetic_rewrite_yields_zero_delta() {
        let a = "def foo\n  1\nend\n";
        let b = "def foo\n  # added a comment\n  1\nend\n";
        assert_eq!(public_api_delta(a, b, Language::Ruby), 0);
    }

    #[test]
    fn adding_public_method_counts_as_one() {
        let a = "def foo\nend\n";
        let b = "def foo\nend\n\ndef bar\nend\n";
        assert_eq!(public_api_delta(a, b, Language::Ruby), 1);
    }

    #[test]
    fn underscore_prefixed_does_not_count() {
        let a = "def keep\nend\n";
        let b = "def keep\nend\n\ndef _helper\nend\n";
        assert_eq!(public_api_delta(a, b, Language::Ruby), 0);
    }
}
