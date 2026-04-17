//! Go public-item extraction via tree-sitter.
//!
//! Go's export rule is "identifier begins with uppercase letter", which
//! makes the grammar's own structure insufficient on its own — we still
//! need a case check on every captured name. The query pulls top-level
//! function declarations, method declarations, and type specs; the
//! filter keeps only the uppercase-starting ones.
//!
//! v0.1 omits arg-count in the signature shape (unlike the Rust backend)
//! — adding/removing Go functions is the dominant API change mode and
//! argument fidelity can follow once we need signature-delta sensitivity.

use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

fn language() -> &'static Language {
    static LANG: OnceLock<Language> = OnceLock::new();
    LANG.get_or_init(|| tree_sitter_go::LANGUAGE.into())
}

fn query() -> &'static Query {
    static Q: OnceLock<Query> = OnceLock::new();
    Q.get_or_init(|| {
        Query::new(
            language(),
            "(function_declaration name: (identifier) @fn)
             (method_declaration name: (field_identifier) @method)
             (type_spec name: (type_identifier) @type)",
        )
        .expect("static go query compiles")
    })
}

pub fn parse(source: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(language()).ok()?;
    let tree = parser.parse(source, None)?;

    let q = query();
    let fn_idx = q.capture_index_for_name("fn")?;
    let method_idx = q.capture_index_for_name("method")?;
    let type_idx = q.capture_index_for_name("type")?;

    let mut cursor = QueryCursor::new();
    let mut items = Vec::new();
    let src_bytes = source.as_bytes();
    let mut matches = cursor.matches(q, tree.root_node(), src_bytes);
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let Ok(name) = capture.node.utf8_text(src_bytes) else {
                continue;
            };
            if !name
                .chars()
                .next()
                .map_or(false, |c| c.is_ascii_uppercase())
            {
                continue;
            }
            let kind = if capture.index == fn_idx {
                "fn"
            } else if capture.index == method_idx {
                "method"
            } else if capture.index == type_idx {
                "type"
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
    fn parses_exported_functions() {
        let src = r#"
package main

func Exported() {}
func private() {}
func ExportedWithArgs(a int, b string) bool { return true }
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:Exported".to_string()));
        assert!(items.contains(&"fn:ExportedWithArgs".to_string()));
        assert!(!items.iter().any(|i| i.contains("private")));
    }

    #[test]
    fn parses_methods_and_types() {
        let src = r#"
package main

type Widget struct{}
type private struct{}

func (w *Widget) Render() string { return "" }
func (w *Widget) internal() {}
"#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"type:Widget".to_string()));
        assert!(items.contains(&"method:Render".to_string()));
        assert!(!items.iter().any(|i| i.contains("private")));
        assert!(!items.iter().any(|i| i.contains("internal")));
    }

    #[test]
    fn cosmetic_rewrite_yields_zero_delta() {
        let a = "package main\nfunc Foo() {}";
        let b = "package main\n\nfunc Foo() { // added a comment\n}\n";
        assert_eq!(public_api_delta(a, b, Language::Go), 0);
    }

    #[test]
    fn adding_exported_fn_counts_as_one() {
        let a = "package main\nfunc Foo() {}";
        let b = "package main\nfunc Foo() {}\nfunc Bar() {}";
        assert_eq!(public_api_delta(a, b, Language::Go), 1);
    }

    #[test]
    fn renaming_exported_fn_counts_as_two() {
        let a = "package main\nfunc Foo() {}";
        let b = "package main\nfunc Renamed() {}";
        assert_eq!(public_api_delta(a, b, Language::Go), 2);
    }

    #[test]
    fn private_additions_do_not_count() {
        let a = "package main\nfunc Keep() {}";
        let b = "package main\nfunc Keep() {}\nfunc inner() {}";
        assert_eq!(public_api_delta(a, b, Language::Go), 0);
    }

    #[test]
    fn parse_failure_is_handled() {
        // Go parsers are generally tolerant, so we won't get None easily;
        // but malformed content must at least not panic.
        let src = "this is not go code at all {{{";
        // `parse` returns Some([]) or Some(nothing) — tree-sitter is
        // permissive. The contract is "don't panic and produce a vec".
        let items = parse(src);
        assert!(items.is_some());
    }
}
