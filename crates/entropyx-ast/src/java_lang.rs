//! Java public-API extraction via tree-sitter.
//!
//! Java's visibility is explicit through modifier keywords. Our v0.1
//! query captures top-level declarations whose `modifiers` node
//! contains the `"public"` keyword: classes, interfaces, enums,
//! records, and methods. This produces a conservative signature set —
//! a `public class` addition definitely fires, a new public method
//! on an existing class fires.
//!
//! Scope caveats:
//!   - Interface methods are **implicitly** public but carry no
//!     modifier keyword; they are not captured here. API-delta will
//!     miss changes inside interfaces until the query is extended to
//!     recognize "all methods inside interface_declaration are public".
//!   - `protected` (semi-public) is ignored — v0.1 treats only `public`
//!     as API surface.

use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_java::LANGUAGE.into())
}

fn query() -> &'static Query {
    static Q: OnceLock<Query> = OnceLock::new();
    Q.get_or_init(|| {
        Query::new(
            language(),
            r#"
            (class_declaration
              (modifiers "public")
              name: (identifier) @class)
            (interface_declaration
              (modifiers "public")
              name: (identifier) @interface)
            (enum_declaration
              (modifiers "public")
              name: (identifier) @enum)
            (record_declaration
              (modifiers "public")
              name: (identifier) @record)
            (method_declaration
              (modifiers "public")
              name: (identifier) @method)
            "#,
        )
        .expect("static java query compiles")
    })
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let q = query();
    let class_idx = q.capture_index_for_name("class")?;
    let iface_idx = q.capture_index_for_name("interface")?;
    let enum_idx = q.capture_index_for_name("enum")?;
    let record_idx = q.capture_index_for_name("record")?;
    let method_idx = q.capture_index_for_name("method")?;

    let mut cursor = QueryCursor::new();
    let mut items = Vec::new();
    let src_bytes = source.as_bytes();
    let mut matches = cursor.matches(q, tree.root_node(), src_bytes);
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let Ok(name) = capture.node.utf8_text(src_bytes) else {
                continue;
            };
            let kind = if capture.index == class_idx {
                "class"
            } else if capture.index == iface_idx {
                "interface"
            } else if capture.index == enum_idx {
                "enum"
            } else if capture.index == record_idx {
                "record"
            } else if capture.index == method_idx {
                "method"
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
    fn parses_public_class_and_methods() {
        let src = r#"
public class Widget {
    public int render() { return 0; }
    private int helper() { return 1; }
    int packageLocal() { return 2; }
}

class Internal {
    public void alsoPublicButHostIsPrivate() {}
}
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"class:Widget".to_string()));
        assert!(items.contains(&"method:render".to_string()));
        assert!(!items.iter().any(|i| i.contains("helper")));
        assert!(!items.iter().any(|i| i.contains("packageLocal")));
        // Host class `Internal` is package-private → not captured.
        assert!(!items.iter().any(|i| i.contains("Internal")));
        // But we still capture alsoPublicButHostIsPrivate because it
        // literally has `public` on the method. v0.1 accepts this
        // over-counting; callers can dedupe by class scope later.
        assert!(items.contains(&"method:alsoPublicButHostIsPrivate".to_string()));
    }

    #[test]
    fn parses_public_interface_and_enum() {
        let src = r#"
public interface Service {
    void go();
}
public enum State { OPEN, CLOSED }
interface Hidden {}
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"interface:Service".to_string()));
        assert!(items.contains(&"enum:State".to_string()));
        assert!(!items.iter().any(|i| i.contains("Hidden")));
    }

    #[test]
    fn parses_public_records() {
        let src = "public record Point(int x, int y) {}\nrecord Internal() {}";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"record:Point".to_string()));
        assert!(!items.iter().any(|i| i.contains("Internal")));
    }

    #[test]
    fn cosmetic_rewrite_yields_zero_delta() {
        let a = "public class A { public void foo() {} }";
        let b = "public class A {\n    public void foo() {\n        // added comment\n    }\n}\n";
        assert_eq!(public_api_delta(a, b, Language::Java), 0);
    }

    #[test]
    fn adding_public_method_counts_as_one() {
        let a = "public class A { public void foo() {} }";
        let b = "public class A { public void foo() {} public void bar() {} }";
        assert_eq!(public_api_delta(a, b, Language::Java), 1);
    }

    #[test]
    fn adding_public_class_counts_as_one() {
        let a = "public class A {}";
        let b = "public class A {}\npublic class B {}";
        assert_eq!(public_api_delta(a, b, Language::Java), 1);
    }

    #[test]
    fn private_changes_do_not_count() {
        let a = "public class A { private void hidden() {} }";
        let b = "public class A { private void hidden() {} private void alsoHidden() {} }";
        assert_eq!(public_api_delta(a, b, Language::Java), 0);
    }

    #[test]
    fn protected_is_not_captured_in_v01() {
        // `protected` is semi-public but v0.1 only treats `public` as
        // API surface. This test documents the boundary.
        let a = "public class A { protected void a() {} }";
        let b = "public class A { protected void a() {} protected void b() {} }";
        assert_eq!(public_api_delta(a, b, Language::Java), 0);
    }
}
