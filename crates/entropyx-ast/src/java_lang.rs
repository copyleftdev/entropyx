//! Java public-API extraction via tree-sitter.
//!
//! Java's visibility is explicit through modifier keywords for classes
//! and class members, with one wrinkle: methods declared inside an
//! `interface_declaration` are *implicitly* public, with no modifier
//! keyword. The query handles both:
//!
//!   - `class_declaration / interface_declaration / enum_declaration /
//!     record_declaration / method_declaration` with a `(modifiers
//!     "public")` child — top-level declarations marked explicitly.
//!   - `method_declaration` whose ancestor is an `interface_declaration`
//!     — implicitly public; modifier check skipped.
//!
//! Dedup collapses any double-counts (e.g. an interface method that
//! also writes `public` explicitly is captured once).
//!
//! Scope caveat: `protected` (semi-public) is ignored — v0.1 treats
//! only `public` (or interface-implicit-public) as API surface.

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
            ;; Methods inside an interface body — implicitly public
            ;; unless explicitly private (Java 9+). The `private`
            ;; check happens in Rust because tree-sitter queries can't
            ;; assert absence of a sub-token.
            (interface_declaration
              body: (interface_body
                (method_declaration
                  name: (identifier) @iface_method)))
            "#,
        )
        .expect("static java query compiles")
    })
}

/// Walk up from a captured identifier to the surrounding
/// `method_declaration`, then inspect its `modifiers` child for the
/// `private` token. Returns true when the method is explicitly private.
fn method_has_private_modifier(
    name_node: tree_sitter::Node<'_>,
    src: &[u8],
) -> bool {
    let Some(method) = name_node.parent() else {
        return false;
    };
    let mut walker = method.walk();
    for child in method.children(&mut walker) {
        if child.kind() == "modifiers" {
            if let Ok(text) = child.utf8_text(src) {
                if text.split_whitespace().any(|w| w == "private") {
                    return true;
                }
            }
        }
    }
    false
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
    let iface_method_idx = q.capture_index_for_name("iface_method")?;

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
            } else if capture.index == iface_method_idx {
                // Java 9+: interface methods can be explicitly `private`
                // (helpers for default methods). Skip those.
                if method_has_private_modifier(capture.node, src_bytes) {
                    continue;
                }
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
        // Interface methods are implicitly public — must be captured.
        assert!(items.contains(&"method:go".to_string()));
        assert!(!items.iter().any(|i| i.contains("Hidden")));
    }

    #[test]
    fn interface_method_with_explicit_public_modifier_is_not_double_counted() {
        // `public` on an interface method is redundant but legal Java.
        // The same name shouldn't appear twice in the captured items.
        let src = r#"
public interface Service {
    public void go();
}
"#;
        let items = parse(src).expect("parse");
        let go_count = items.iter().filter(|s| *s == "method:go").count();
        assert_eq!(go_count, 1, "explicit public + implicit interface dedup");
    }

    #[test]
    fn default_methods_in_interfaces_are_captured() {
        // Java 8+ `default` methods have bodies and are also implicitly
        // public — they're part of the interface's public API surface.
        let src = r#"
public interface Greeter {
    default String hello() { return "hi"; }
    String required();
}
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:hello".to_string()));
        assert!(items.contains(&"method:required".to_string()));
    }

    #[test]
    fn private_methods_in_interfaces_are_excluded() {
        // Java 9+: interface methods can be explicitly `private`
        // (helpers for default methods). Those are NOT public API
        // and must not be captured.
        let src = r#"
public interface I {
    default void public_helper() { realWork(); }
    private void realWork() {}
}
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:public_helper".to_string()));
        assert!(
            !items.contains(&"method:realWork".to_string()),
            "private interface method must not be captured",
        );
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
