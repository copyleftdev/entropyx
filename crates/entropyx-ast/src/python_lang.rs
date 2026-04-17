//! Python public-item extraction via tree-sitter.
//!
//! Python's "public" convention is nominative: identifiers not starting
//! with `_` are the module's API surface. We capture top-level `def`,
//! top-level `class`, and class-scoped methods, filtering by the
//! leading-underscore rule.
//!
//! Methods are emitted with their enclosing class path so two classes
//! with the same method name don't collide:
//!
//!   - Module-level `def foo` → `fn:foo`
//!   - Module-level `class Bar` → `class:Bar`
//!   - Nested `class Bar.Inner` → `class:Bar.Inner`
//!   - Method on `Bar` → `method:Bar.foo`
//!   - Method on `Bar.Inner` → `method:Bar.Inner.foo`
//!
//! Functions nested inside other functions (closures) are treated as
//! implementation details and not captured. If any class in the
//! enclosing path starts with `_`, everything inside that class is
//! skipped — a private class's members aren't public API.
//!
//! v0.1 still omits module-level variable bindings (ALL_CAPS constants,
//! singletons). Add a `(assignment ...)` clause when signal demands it.

use std::sync::OnceLock;
use tree_sitter::{Language, Node, Parser};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_python::LANGUAGE.into())
}

#[derive(Clone, Copy)]
enum Scope {
    Module,
    Class,
    Function,
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let src = source.as_bytes();
    let mut items = Vec::new();
    let mut path: Vec<String> = Vec::new();
    walk(tree.root_node(), src, &mut items, &mut path, Scope::Module);

    items.sort();
    items.dedup();
    Some(items)
}

fn walk(
    node: Node<'_>,
    src: &[u8],
    items: &mut Vec<String>,
    class_path: &mut Vec<String>,
    scope: Scope,
) {
    match node.kind() {
        "class_definition" => handle_class(node, src, items, class_path),
        "function_definition" => handle_function(node, src, items, class_path, scope),
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk(child, src, items, class_path, scope);
            }
        }
    }
}

fn handle_class(node: Node<'_>, src: &[u8], items: &mut Vec<String>, class_path: &mut Vec<String>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Ok(name) = name_node.utf8_text(src) else {
        return;
    };
    if name.starts_with('_') {
        // Private class — don't emit, don't recurse. Members of a
        // private class aren't part of the public API.
        return;
    }

    let qualified = qualify(class_path, name);
    items.push(format!("class:{qualified}"));

    class_path.push(name.to_string());
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            walk(child, src, items, class_path, Scope::Class);
        }
    }
    class_path.pop();
}

fn handle_function(
    node: Node<'_>,
    src: &[u8],
    items: &mut Vec<String>,
    class_path: &mut Vec<String>,
    scope: Scope,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Ok(name) = name_node.utf8_text(src) else {
        return;
    };

    let public_name = !name.starts_with('_');

    match scope {
        Scope::Module if public_name => {
            items.push(format!("fn:{name}"));
        }
        Scope::Class if public_name => {
            items.push(format!("method:{}", qualify(class_path, name)));
        }
        _ => {}
    }

    // Recurse into the body so nested classes still surface, but mark
    // the scope Function so inner `def` closures are skipped.
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            walk(child, src, items, class_path, Scope::Function);
        }
    }
}

fn qualify(class_path: &[String], name: &str) -> String {
    if class_path.is_empty() {
        name.to_string()
    } else {
        format!("{}.{name}", class_path.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Language, public_api_delta};
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
        // too, so our walker catches it without a special case.
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
    fn methods_are_scoped_by_class() {
        // Two classes with the same method name no longer collide —
        // each method's identity is qualified by its class.
        let src = r#"
class A:
    def shared(self):
        pass

class B:
    def shared(self):
        pass
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:A.shared".to_string()));
        assert!(items.contains(&"method:B.shared".to_string()));
        assert!(
            !items.iter().any(|i| i == "fn:shared"),
            "class-scoped methods should not leak as top-level fns",
        );
    }

    #[test]
    fn nested_classes_carry_full_path() {
        let src = r#"
class Outer:
    class Inner:
        def foo(self):
            pass
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"class:Outer".to_string()));
        assert!(items.contains(&"class:Outer.Inner".to_string()));
        assert!(items.contains(&"method:Outer.Inner.foo".to_string()));
    }

    #[test]
    fn private_class_members_are_not_captured() {
        // A leading-underscore class is private by convention; its
        // members are not public API regardless of their names.
        let src = r#"
class _Hidden:
    def seemingly_public(self):
        pass
"#;
        let items = parse(src).expect("parse");
        assert!(items.is_empty(), "private class contents must not leak");
    }

    #[test]
    fn private_method_inside_public_class_is_skipped() {
        let src = r#"
class Widget:
    def render(self):
        pass

    def _helper(self):
        pass
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"class:Widget".to_string()));
        assert!(items.contains(&"method:Widget.render".to_string()));
        assert!(!items.contains(&"method:Widget._helper".to_string()));
    }

    #[test]
    fn inner_closures_are_not_captured() {
        // Functions nested inside other functions are implementation
        // details, not API surface.
        let src = r#"
def outer():
    def inner():
        pass
    return inner
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:outer".to_string()));
        assert!(!items.iter().any(|i| i.contains("inner")));
    }

    #[test]
    fn renaming_method_to_same_class_counts_as_one_change() {
        // A method in class A renamed is one addition + one removal.
        let a = "class A:\n    def foo(self):\n        pass\n";
        let b = "class A:\n    def bar(self):\n        pass\n";
        assert_eq!(public_api_delta(a, b, Language::Python), 2);
    }

    #[test]
    fn same_method_name_in_two_classes_count_independently() {
        // Adding a new class that happens to share a method name with
        // an existing one must still register as new surface.
        let a = "class A:\n    def run(self):\n        pass\n";
        let b = "class A:\n    def run(self):\n        pass\n\nclass B:\n    def run(self):\n        pass\n";
        // +class:B, +method:B.run → delta = 2
        assert_eq!(public_api_delta(a, b, Language::Python), 2);
    }
}
