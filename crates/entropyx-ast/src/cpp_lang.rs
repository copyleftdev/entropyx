//! C++ public-API extraction via tree-sitter.
//!
//! C++ access control lives on class/struct/union members: each body
//! tracks a "current section" that starts at the type's default access
//! (`class` → private, `struct`/`union` → public) and flips whenever
//! an `access_specifier` (`public:`, `private:`, `protected:`) appears.
//! Only members in the currently-public section are captured.
//!
//! Top-level or namespace-scoped entities (free functions, classes,
//! namespaces, enums) are always public API surface.
//!
//! Captured signatures:
//!   - free `function_definition` → `fn:<name>`
//!   - public member `function_definition` / method declaration → `fn:<name>`
//!   - `class_specifier` → `class:<name>`
//!   - `struct_specifier` → `struct:<name>`
//!   - `enum_specifier` → `enum:<name>`
//!   - `union_specifier` → `union:<name>`
//!   - `namespace_definition` → `namespace:<name>`
//!
//! Caveats:
//!   - Templates aren't specialized in the signature.
//!   - Anonymous namespaces / classes produce no captures.
//!   - Out-of-line qualified definitions (`int Widget::render() {}`)
//!     are emitted as `fn:render` regardless of the declaring class's
//!     access specifier — the original class header isn't re-parsed
//!     here. v0.1 accepts this over-counting.
//!   - Nested class members are filtered by the nested class's own
//!     access state, independent of the outer class.

use std::sync::OnceLock;
use tree_sitter::{Language, Node, Parser};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_cpp::LANGUAGE.into())
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let src = source.as_bytes();
    let mut items = Vec::new();
    walk_public(tree.root_node(), src, &mut items);

    items.sort();
    items.dedup();
    Some(items)
}

/// Walk a node at a scope where everything visible is public (top
/// level, inside a namespace, or inside an access-`public` member area
/// once the caller has already filtered).
fn walk_public(node: Node<'_>, src: &[u8], items: &mut Vec<String>) {
    match node.kind() {
        "class_specifier" => handle_record(node, src, items, "class", false),
        "struct_specifier" => handle_record(node, src, items, "struct", true),
        "union_specifier" => handle_record(node, src, items, "union", true),
        "enum_specifier" => {
            if let Some(name) = type_name(node, src) {
                items.push(format!("enum:{name}"));
            }
        }
        "namespace_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(src) {
                    items.push(format!("namespace:{name}"));
                }
            }
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    walk_public(child, src, items);
                }
            }
        }
        "function_definition" => {
            if let Some(name) = function_definition_name(node, src) {
                items.push(format!("fn:{name}"));
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk_public(child, src, items);
            }
        }
    }
}

fn handle_record(
    node: Node<'_>,
    src: &[u8],
    items: &mut Vec<String>,
    kind: &str,
    default_public: bool,
) {
    if let Some(name) = type_name(node, src) {
        items.push(format!("{kind}:{name}"));
    }
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    let mut section_public = default_public;
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "access_specifier" => {
                if let Ok(text) = child.utf8_text(src) {
                    section_public = text.trim() == "public";
                }
            }
            "function_definition" => {
                if section_public {
                    if let Some(name) = function_definition_name(child, src) {
                        items.push(format!("fn:{name}"));
                    }
                }
            }
            "field_declaration" => {
                if section_public {
                    if let Some(name) = field_function_name(child, src) {
                        items.push(format!("fn:{name}"));
                    }
                }
            }
            "class_specifier" | "struct_specifier" | "union_specifier"
            | "enum_specifier" => {
                if section_public {
                    walk_public(child, src, items);
                }
            }
            _ => {
                if section_public {
                    walk_public(child, src, items);
                }
            }
        }
    }
}

/// Extract the type identifier from a class/struct/union/enum specifier.
fn type_name<'a>(node: Node<'_>, src: &'a [u8]) -> Option<&'a str> {
    let name_node = node.child_by_field_name("name")?;
    name_node.utf8_text(src).ok()
}

/// Drill into a `function_definition` to find the function's name,
/// handling both bare identifiers (free functions / inline methods)
/// and qualified identifiers (out-of-line class member definitions).
fn function_definition_name<'a>(node: Node<'_>, src: &'a [u8]) -> Option<&'a str> {
    let declarator = node.child_by_field_name("declarator")?;
    declarator_leaf_name(declarator, src)
}

/// A `field_declaration` wrapping a function_declarator is a method
/// declaration without a body — `int foo();` inside a class body.
fn field_function_name<'a>(node: Node<'_>, src: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_declarator" {
            return declarator_leaf_name(child, src);
        }
    }
    None
}

/// Walk through (possibly nested) function_declarators, dropping
/// pointer/reference wrappers, and return the leaf name identifier.
fn declarator_leaf_name<'a>(node: Node<'_>, src: &'a [u8]) -> Option<&'a str> {
    let mut current = node;
    loop {
        match current.kind() {
            "function_declarator" | "pointer_declarator"
            | "reference_declarator" => {
                current = current.child_by_field_name("declarator")?;
            }
            "identifier" | "field_identifier" => {
                return current.utf8_text(src).ok();
            }
            "qualified_identifier" => {
                let name_node = current.child_by_field_name("name")?;
                return name_node.utf8_text(src).ok();
            }
            "destructor_name" | "operator_name" => {
                return current.utf8_text(src).ok();
            }
            _ => return None,
        }
    }
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

    #[test]
    fn private_class_members_are_not_captured() {
        // `class` defaults to private — the method `hidden` declared
        // before any access specifier must NOT be captured.
        let src = r#"
class Widget {
    int hidden();
public:
    int shown();
};
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"class:Widget".to_string()));
        assert!(items.contains(&"fn:shown".to_string()));
        assert!(
            !items.contains(&"fn:hidden".to_string()),
            "class-default-private member must not leak",
        );
    }

    #[test]
    fn protected_class_members_are_not_captured() {
        let src = r#"
class Widget {
public:
    int pub();
protected:
    int prot();
};
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:pub".to_string()));
        assert!(!items.contains(&"fn:prot".to_string()));
    }

    #[test]
    fn struct_defaults_to_public() {
        // Opposite of class — struct members are public unless a
        // private/protected specifier flips the section.
        let src = r#"
struct Point {
    int x_getter();
private:
    int hidden();
};
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"struct:Point".to_string()));
        assert!(items.contains(&"fn:x_getter".to_string()));
        assert!(!items.contains(&"fn:hidden".to_string()));
    }

    #[test]
    fn private_then_public_then_private_sections() {
        // Access specifiers flip the section repeatedly — order matters.
        let src = r#"
class W {
    int a();
public:
    int b();
private:
    int c();
public:
    int d();
};
"#;
        let items = parse(src).expect("parse");
        assert!(!items.contains(&"fn:a".to_string()));
        assert!(items.contains(&"fn:b".to_string()));
        assert!(!items.contains(&"fn:c".to_string()));
        assert!(items.contains(&"fn:d".to_string()));
    }

    #[test]
    fn inline_body_methods_respect_access() {
        // `function_definition` inside a class body (inline method
        // bodies, not just declarations) must also be filtered.
        let src = r#"
class W {
public:
    int pub_inline() { return 0; }
private:
    int priv_inline() { return 1; }
};
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:pub_inline".to_string()));
        assert!(!items.contains(&"fn:priv_inline".to_string()));
    }

    #[test]
    fn adding_private_method_does_not_count() {
        let a = "class W { public: int pub(); };";
        let b = "class W { public: int pub(); private: int priv(); };";
        assert_eq!(public_api_delta(a, b, Language::Cpp), 0);
    }

    #[test]
    fn adding_public_method_in_class_counts_as_one() {
        let a = "class W { public: int a(); };";
        let b = "class W { public: int a(); int b(); };";
        assert_eq!(public_api_delta(a, b, Language::Cpp), 1);
    }
}
