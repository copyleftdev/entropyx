//! Semantic-delta engines (RFC-005) — feeds `S_n` of the composite.
//!
//! Language-dispatching front: every language exports a `parse` function
//! that yields a sorted, deduplicated list of *public API signatures* —
//! the shape that `public_api_delta_from_items` diffs. Adding a language
//! means adding a module + a `Language` variant + a match arm; no
//! caller-side changes.

mod go_lang;
mod java_lang;
mod javascript_lang;
mod python_lang;
mod rust_lang;
mod typescript_lang;

/// Supported source languages. Rust uses `syn` for exact semantic
/// fidelity; other languages use `tree-sitter` grammars with
/// language-specific "public item" queries.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Go,
    Python,
    TypeScript,
    Java,
    JavaScript,
}

/// Map a file path to a supported `Language` by extension. `.tsx` maps
/// to TypeScript (the TSX grammar is a superset and parses .ts cleanly).
/// Returns `None` for unsupported extensions — those files contribute
/// nothing to `S_n`.
pub fn language_from_path(path: &str) -> Option<Language> {
    if path.ends_with(".rs") {
        Some(Language::Rust)
    } else if path.ends_with(".go") {
        Some(Language::Go)
    } else if path.ends_with(".py") {
        Some(Language::Python)
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        Some(Language::TypeScript)
    } else if path.ends_with(".java") {
        Some(Language::Java)
    } else if path.ends_with(".js")
        || path.ends_with(".jsx")
        || path.ends_with(".mjs")
        || path.ends_with(".cjs")
    {
        Some(Language::JavaScript)
    } else {
        None
    }
}

/// Extract a sorted, deduplicated list of public API signatures from a
/// source string. Returns `None` on parse failure — callers treat
/// unparseable files as "no observable drift" rather than crashing.
///
/// Signature shape is language-independent but stable within a language:
/// `fn:<name>/<arg-count>` for Rust/Go functions, `type:<name>` for
/// both, etc. Cross-language delta comparison is not meaningful; keep
/// the same language for old/new pairs.
pub fn parse_public_items(source: &str, lang: Language) -> Option<Vec<String>> {
    match lang {
        Language::Rust => rust_lang::parse(source),
        Language::Go => go_lang::parse(source),
        Language::Python => python_lang::parse(source),
        Language::TypeScript => typescript_lang::parse(source),
        Language::Java => java_lang::parse(source),
        Language::JavaScript => javascript_lang::parse(source),
    }
}

/// Count of distinct public API signatures that differ between two
/// pre-parsed, sorted item lists. The hot primitive for callers that
/// cache `parse_public_items` output by blob SHA.
///
/// Inputs must be sorted (which every `parse` backend guarantees).
pub fn public_api_delta_from_items(a: &[String], b: &[String]) -> u32 {
    let mut i = 0;
    let mut j = 0;
    let mut diff: u32 = 0;
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => {
                diff += 1;
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                diff += 1;
                j += 1;
            }
        }
    }
    diff += (a.len() - i) as u32;
    diff += (b.len() - j) as u32;
    diff
}

/// Count of distinct public API signatures that differ between `old`
/// and `new` source strings. Convenience wrapper for callers that
/// don't cache parsed results.
pub fn public_api_delta(old: &str, new: &str, lang: Language) -> u32 {
    let (Some(a), Some(b)) = (
        parse_public_items(old, lang),
        parse_public_items(new, lang),
    ) else {
        return 0;
    };
    public_api_delta_from_items(&a, &b)
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn language_routing_by_extension() {
        assert_eq!(language_from_path("src/lib.rs"), Some(Language::Rust));
        assert_eq!(language_from_path("pkg/foo.go"), Some(Language::Go));
        assert_eq!(language_from_path("app/main.py"), Some(Language::Python));
        assert_eq!(language_from_path("src/index.ts"), Some(Language::TypeScript));
        assert_eq!(language_from_path("src/App.tsx"), Some(Language::TypeScript));
        assert_eq!(language_from_path("src/Main.java"), Some(Language::Java));
        assert_eq!(language_from_path("src/index.js"), Some(Language::JavaScript));
        assert_eq!(language_from_path("src/App.jsx"), Some(Language::JavaScript));
        assert_eq!(language_from_path("src/entry.mjs"), Some(Language::JavaScript));
        assert_eq!(language_from_path("src/legacy.cjs"), Some(Language::JavaScript));
        assert_eq!(language_from_path("README.md"), None);
        assert_eq!(language_from_path(""), None);
    }

    #[test]
    fn cross_language_parsing_is_independent() {
        // Same string may parse in one language but not another; the
        // dispatcher keeps them isolated.
        let rust = "pub fn foo() {}";
        let go = "package main\nfunc Foo() {}";
        assert!(parse_public_items(rust, Language::Rust).is_some());
        assert!(parse_public_items(go, Language::Go).is_some());
    }
}
