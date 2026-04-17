//! JavaScript public-API extraction via tree-sitter.
//!
//! Captures both ES-module and CommonJS export patterns:
//!
//! ES modules:
//!   - `export function name() {}`
//!   - `export class Name {}`
//!   - `export const|let|var name = ...`
//!
//! CommonJS (signatures emitted with a `cjs:` prefix to distinguish
//! from ESM `fn:` / `class:` / `const:`):
//!   - `module.exports = { foo, bar };` → `cjs:foo`, `cjs:bar`
//!   - `module.exports = SomeIdent;` → `cjs:SomeIdent`
//!   - `exports.foo = ...;` → `cjs:foo`
//!   - `module.exports.bar = ...;` → `cjs:bar`
//!
//! Not yet captured: `module.exports = function namedFn() {}` (an
//! anonymous-or-named function expression on the right-hand side
//! is treated as an opaque value; the binding name isn't extracted).

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
            ;; ES modules
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

            ;; CommonJS — module.exports = { foo, bar } or { foo: ..., bar: ... }
            (assignment_expression
              left: (member_expression
                object: (identifier) @_module
                property: (property_identifier) @_exports)
              right: (object
                (shorthand_property_identifier) @cjs)
              (#eq? @_module "module")
              (#eq? @_exports "exports"))
            (assignment_expression
              left: (member_expression
                object: (identifier) @_module
                property: (property_identifier) @_exports)
              right: (object
                (pair key: (property_identifier) @cjs))
              (#eq? @_module "module")
              (#eq? @_exports "exports"))

            ;; CommonJS — module.exports = SomeIdent
            (assignment_expression
              left: (member_expression
                object: (identifier) @_module
                property: (property_identifier) @_exports)
              right: (identifier) @cjs
              (#eq? @_module "module")
              (#eq? @_exports "exports"))

            ;; CommonJS — module.exports = function namedFn() {}
            (assignment_expression
              left: (member_expression
                object: (identifier) @_module
                property: (property_identifier) @_exports)
              right: (function_expression
                name: (identifier) @cjs)
              (#eq? @_module "module")
              (#eq? @_exports "exports"))

            ;; CommonJS — module.exports = function () {}  (anonymous)
            (assignment_expression
              left: (member_expression
                object: (identifier) @_module
                property: (property_identifier) @_exports)
              right: (function_expression !name) @cjs_default
              (#eq? @_module "module")
              (#eq? @_exports "exports"))

            ;; CommonJS — module.exports = () => {}  (anonymous arrow)
            (assignment_expression
              left: (member_expression
                object: (identifier) @_module
                property: (property_identifier) @_exports)
              right: (arrow_function) @cjs_default
              (#eq? @_module "module")
              (#eq? @_exports "exports"))

            ;; CommonJS — exports.foo = ...
            (assignment_expression
              left: (member_expression
                object: (identifier) @_exports_obj
                property: (property_identifier) @cjs)
              (#eq? @_exports_obj "exports"))

            ;; CommonJS — module.exports.foo = ...
            (assignment_expression
              left: (member_expression
                object: (member_expression
                  object: (identifier) @_module
                  property: (property_identifier) @_exports)
                property: (property_identifier) @cjs)
              (#eq? @_module "module")
              (#eq? @_exports "exports"))
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
    let cjs_idx = q.capture_index_for_name("cjs")?;
    let cjs_default_idx = q.capture_index_for_name("cjs_default")?;

    let mut cursor = QueryCursor::new();
    let mut items = Vec::new();
    let src_bytes = source.as_bytes();
    let mut matches = cursor.matches(q, tree.root_node(), src_bytes);
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let Ok(name) = capture.node.utf8_text(src_bytes) else {
                continue;
            };
            let (kind, name_str) = if capture.index == fn_idx {
                ("fn", name.to_string())
            } else if capture.index == class_idx {
                ("class", name.to_string())
            } else if capture.index == const_idx {
                ("const", name.to_string())
            } else if capture.index == var_idx {
                ("var", name.to_string())
            } else if capture.index == cjs_idx {
                ("cjs", name.to_string())
            } else if capture.index == cjs_default_idx {
                // Anonymous module.exports — emit a stable sentinel so
                // additions/removals register but the binding name
                // (which doesn't exist) doesn't pollute identity.
                ("cjs", "default".to_string())
            } else {
                // Predicate-helper captures (`@_module`, `@_exports`)
                // are filtered out by tree-sitter when the `#eq?`
                // doesn't match — but they still appear in successful
                // matches. Skip anything not in our named-emit set.
                continue;
            };
            items.push(format!("{kind}:{name_str}"));
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
    fn module_exports_object_literal_captures_each_key() {
        let a = "function hello() {}\nmodule.exports = { hello };\n";
        let b = "function hello() {}\nfunction world() {}\nmodule.exports = { hello, world };\n";
        // a has cjs:hello; b adds cjs:world → delta=1.
        assert_eq!(public_api_delta(a, b, Language::JavaScript), 1);
        let items_b = parse(b).expect("parse");
        assert!(items_b.contains(&"cjs:hello".to_string()));
        assert!(items_b.contains(&"cjs:world".to_string()));
    }

    #[test]
    fn module_exports_object_with_pair_keys_captured() {
        // `module.exports = { foo: ..., bar: 42 }` — each key counts.
        let src = "module.exports = { greet: () => {}, version: 'v1' };\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"cjs:greet".to_string()));
        assert!(items.contains(&"cjs:version".to_string()));
    }

    #[test]
    fn module_exports_single_identifier_captured() {
        let src = "function go() {}\nmodule.exports = go;\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"cjs:go".to_string()));
    }

    #[test]
    fn exports_dot_name_is_captured() {
        let src = "exports.foo = function () {};\nexports.bar = 42;\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"cjs:foo".to_string()));
        assert!(items.contains(&"cjs:bar".to_string()));
    }

    #[test]
    fn module_exports_dot_name_is_captured() {
        let src = "module.exports.alpha = 1;\nmodule.exports.beta = 2;\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"cjs:alpha".to_string()));
        assert!(items.contains(&"cjs:beta".to_string()));
    }

    #[test]
    fn module_exports_named_function_expression_captures_name() {
        let src = "module.exports = function namedFn(x) { return x; };\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"cjs:namedFn".to_string()));
    }

    #[test]
    fn module_exports_anonymous_function_emits_cjs_default() {
        let src = "module.exports = function () { return 42; };\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"cjs:default".to_string()));
    }

    #[test]
    fn module_exports_arrow_function_emits_cjs_default() {
        let src = "module.exports = () => 42;\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"cjs:default".to_string()));
    }

    #[test]
    fn mixed_es_and_cjs_exports_both_appear() {
        let src = "export function esmFn() {}\nmodule.exports.cjsName = 1;\n";
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:esmFn".to_string()));
        assert!(items.contains(&"cjs:cjsName".to_string()));
    }
}
