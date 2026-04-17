//! Ruby public-API extraction via tree-sitter.
//!
//! Ruby visibility is tracked by a combination of:
//!
//!   - Name convention — a leading underscore (`_helper`) is treated as
//!     convention-private, matching the Python backend.
//!   - Explicit visibility sections — a bare `private` or `protected`
//!     statement in a class/module body flips visibility for every
//!     subsequent method until a `public` statement flips it back.
//!   - Explicit visibility calls — `private :foo, :bar` marks specific
//!     methods private regardless of section state.
//!
//! Captured: `def`/`singleton_method` whose name is public under the
//! rules above, plus `class` and `module` declarations with non-
//! underscore names. Methods defined at the top level (outside any
//! class/module) are treated as public: they're reachable as methods
//! on `Object` and commonly used as script entry points.

use std::collections::HashSet;
use std::sync::OnceLock;
use tree_sitter::{Language, Node, Parser};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_ruby::LANGUAGE.into())
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let src = source.as_bytes();
    let mut items = Vec::new();
    walk_top(tree.root_node(), src, &mut items);

    items.sort();
    items.dedup();
    Some(items)
}

/// Walk at top-level / non-class scope: capture class/module declarations
/// and any top-level `def`. Recurse into children of unknown nodes so
/// deeply-nested classes still surface.
fn walk_top(node: Node<'_>, src: &[u8], items: &mut Vec<String>) {
    match node.kind() {
        "class" => handle_class_or_module(node, src, items, "class"),
        "module" => handle_class_or_module(node, src, items, "module"),
        "method" | "singleton_method" => {
            if let Some(name) = public_method_name(node, src) {
                items.push(format!("method:{name}"));
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk_top(child, src, items);
            }
        }
    }
}

fn handle_class_or_module(node: Node<'_>, src: &[u8], items: &mut Vec<String>, kind: &str) {
    if let Some(name_node) = node.child_by_field_name("name")
        && let Ok(name) = name_node.utf8_text(src)
        && !name.starts_with('_')
    {
        items.push(format!("{kind}:{name}"));
    }
    if let Some(body) = node.child_by_field_name("body") {
        walk_body(body, src, items);
    }
}

/// Walk a `class`/`module` body, tracking visibility state as we iterate
/// statements. `private`/`protected` statements flip the scope private
/// for subsequent methods; `public` flips it back. `private :name` /
/// `protected :name` mark specific methods private retroactively.
fn walk_body(body: Node<'_>, src: &[u8], items: &mut Vec<String>) {
    let mut section_public = true;
    let mut captured: Vec<(String, usize)> = Vec::new();
    let mut force_private: HashSet<String> = HashSet::new();

    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                if let Ok(text) = child.utf8_text(src) {
                    match text {
                        "private" | "protected" => section_public = false,
                        "public" => section_public = true,
                        _ => {}
                    }
                }
            }
            "call" => {
                if let Some(keyword) = visibility_call_keyword(child, src) {
                    let targets = call_symbol_targets(child, src);
                    if targets.is_empty() {
                        // Bare `private`/`protected` call (no args) acts
                        // like the identifier form: flip section state.
                        section_public = keyword == "public";
                    } else if keyword != "public" {
                        force_private.extend(targets);
                    }
                }
            }
            "method" | "singleton_method" => {
                if !section_public {
                    continue;
                }
                if let Some(name) = public_method_name(child, src) {
                    let idx = captured.len();
                    captured.push((name, idx));
                }
            }
            "class" | "module" => {
                walk_top(child, src, items);
            }
            _ => {
                walk_top(child, src, items);
            }
        }
    }

    for (name, _) in captured {
        if force_private.contains(&name) {
            continue;
        }
        items.push(format!("method:{name}"));
    }
}

fn public_method_name(node: Node<'_>, src: &[u8]) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    let text = name_node.utf8_text(src).ok()?;
    if text.starts_with('_') {
        return None;
    }
    Some(text.to_string())
}

/// If `node` is a `call` like `private`, `protected`, `public`, or
/// `private_class_method`, return the visibility keyword it implies.
fn visibility_call_keyword<'a>(node: Node<'_>, src: &'a [u8]) -> Option<&'a str> {
    let method = node.child_by_field_name("method")?;
    let text = method.utf8_text(src).ok()?;
    match text {
        "private" | "protected" | "public" => Some(text),
        "private_class_method" => Some("private"),
        _ => None,
    }
}

/// Extract `:foo`/`:bar` symbol names from a call's argument_list.
/// Returns an empty vec when the call has no arguments or its arguments
/// aren't bare symbols (e.g. `private def foo` — v0.1 doesn't handle
/// the inline `private def` form yet).
fn call_symbol_targets(node: Node<'_>, src: &[u8]) -> Vec<String> {
    let Some(args) = node.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = args.walk();
    let mut out = Vec::new();
    for child in args.children(&mut cursor) {
        if child.kind() == "simple_symbol"
            && let Ok(text) = child.utf8_text(src)
        {
            out.push(text.trim_start_matches(':').to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{Language, public_api_delta};
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

    #[test]
    fn private_section_excludes_subsequent_methods() {
        let src = r#"
class W
  def public_a
  end

  private

  def private_b
  end

  def private_c
  end
end
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:public_a".to_string()));
        assert!(!items.contains(&"method:private_b".to_string()));
        assert!(!items.contains(&"method:private_c".to_string()));
    }

    #[test]
    fn protected_section_excludes_methods() {
        let src = r#"
class W
  def pub
  end

  protected

  def prot
  end
end
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:pub".to_string()));
        assert!(!items.contains(&"method:prot".to_string()));
    }

    #[test]
    fn public_keyword_flips_visibility_back() {
        let src = r#"
class W
  def a
  end

  private

  def b
  end

  public

  def c
  end
end
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:a".to_string()));
        assert!(!items.contains(&"method:b".to_string()));
        assert!(items.contains(&"method:c".to_string()));
    }

    #[test]
    fn private_with_symbols_marks_specific_methods() {
        let src = r#"
class W
  def a
  end

  def b
  end

  def c
  end

  private :b
end
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:a".to_string()));
        assert!(!items.contains(&"method:b".to_string()));
        assert!(items.contains(&"method:c".to_string()));
    }

    #[test]
    fn visibility_state_resets_across_classes() {
        // Classes have independent visibility scopes — the trailing
        // `private` in `A` must not leak into `B`.
        let src = r#"
class A
  def a
  end

  private

  def hidden
  end
end

class B
  def b
  end
end
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:a".to_string()));
        assert!(!items.contains(&"method:hidden".to_string()));
        assert!(items.contains(&"method:b".to_string()));
    }

    #[test]
    fn private_class_method_marks_singleton_private() {
        let src = r#"
class W
  def self.public_cm
  end

  def self.hidden_cm
  end

  private_class_method :hidden_cm
end
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"method:public_cm".to_string()));
        assert!(!items.contains(&"method:hidden_cm".to_string()));
    }
}
