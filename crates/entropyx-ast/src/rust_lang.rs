//! Rust public-item extraction via `syn`. Richer semantics than
//! tree-sitter (trait items, arg counts, `pub use` targets) because syn
//! is type-aware at the crate-boundary level.

use syn::{Item, Signature, Visibility};

pub fn parse(source: &str) -> Option<Vec<String>> {
    let file = syn::parse_file(source).ok()?;
    let mut out = Vec::new();
    for item in &file.items {
        collect(item, &mut out);
    }
    out.sort();
    out.dedup();
    Some(out)
}

fn collect(item: &Item, out: &mut Vec<String>) {
    match item {
        Item::Fn(f) if is_public(&f.vis) => {
            out.push(fn_sig(&f.sig));
        }
        Item::Struct(s) if is_public(&s.vis) => {
            out.push(format!("struct:{}", s.ident));
        }
        Item::Enum(e) if is_public(&e.vis) => {
            out.push(format!("enum:{}", e.ident));
        }
        Item::Trait(t) if is_public(&t.vis) => {
            out.push(format!("trait:{}", t.ident));
        }
        Item::Const(c) if is_public(&c.vis) => {
            out.push(format!("const:{}", c.ident));
        }
        Item::Static(s) if is_public(&s.vis) => {
            out.push(format!("static:{}", s.ident));
        }
        Item::Type(t) if is_public(&t.vis) => {
            out.push(format!("type:{}", t.ident));
        }
        Item::Use(u) if is_public(&u.vis) => {
            out.push(format!("use:{}", quote_use(&u.tree)));
        }
        Item::Mod(m) if is_public(&m.vis) => {
            out.push(format!("mod:{}", m.ident));
            if let Some((_, items)) = &m.content {
                for nested in items {
                    collect(nested, out);
                }
            }
        }
        Item::Impl(imp) => {
            let ty = type_name(&imp.self_ty);
            for item in &imp.items {
                if let syn::ImplItem::Fn(m) = item
                    && is_public(&m.vis)
                {
                    out.push(format!("impl:{}::{}", ty, fn_sig(&m.sig)));
                }
            }
        }
        _ => {}
    }
}

fn is_public(v: &Visibility) -> bool {
    matches!(v, Visibility::Public(_))
}

fn fn_sig(sig: &Signature) -> String {
    format!("fn:{}/{}", sig.ident, sig.inputs.len())
}

fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(tp) => tp
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        _ => "?".to_string(),
    }
}

fn quote_use(tree: &syn::UseTree) -> String {
    match tree {
        syn::UseTree::Path(p) => format!("{}::{}", p.ident, quote_use(&p.tree)),
        syn::UseTree::Name(n) => n.ident.to_string(),
        syn::UseTree::Rename(r) => format!("{} as {}", r.ident, r.rename),
        syn::UseTree::Glob(_) => "*".to_string(),
        syn::UseTree::Group(g) => {
            let parts: Vec<String> = g.items.iter().map(quote_use).collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Language, public_api_delta};
    use super::parse;

    #[test]
    fn parses_public_fns_and_structs() {
        let src = r#"
            pub fn foo(a: i32, b: i32) -> i32 { a + b }
            fn private_helper() {}
            pub struct Bar;
            pub(crate) struct Hidden;
        "#;
        let items = parse(src).expect("parse");
        assert!(items.contains(&"fn:foo/2".to_string()));
        assert!(items.contains(&"struct:Bar".to_string()));
        assert!(!items.iter().any(|i| i.contains("private_helper")));
        assert!(!items.iter().any(|i| i.contains("Hidden")));
    }

    #[test]
    fn cosmetic_rewrite_yields_zero_delta() {
        let a = "pub fn foo(x: i32) -> i32 { x + 1 }";
        let b = "pub fn foo( x: i32 ) -> i32 {\n    // rename tweaked\n    x + 1\n}";
        assert_eq!(public_api_delta(a, b, Language::Rust), 0);
    }

    #[test]
    fn adding_public_fn_counts_as_one() {
        let a = "pub fn foo() {}";
        let b = "pub fn foo() {}\npub fn bar() {}";
        assert_eq!(public_api_delta(a, b, Language::Rust), 1);
    }

    #[test]
    fn changing_arg_count_counts_as_two() {
        let a = "pub fn foo() {}";
        let b = "pub fn foo(x: i32) {}";
        assert_eq!(public_api_delta(a, b, Language::Rust), 2);
    }

    #[test]
    fn private_item_changes_do_not_count() {
        let a = "pub fn keep() {}\nfn inner() {}";
        let b = "pub fn keep() {}\nfn inner(x: i32, y: i32) -> bool { true }";
        assert_eq!(public_api_delta(a, b, Language::Rust), 0);
    }

    #[test]
    fn recurses_into_public_modules() {
        let a = "pub mod m { pub fn old() {} }";
        let b = "pub mod m { pub fn renamed() {} }";
        assert_eq!(public_api_delta(a, b, Language::Rust), 2);
    }

    #[test]
    fn parse_failure_is_zero_not_crash() {
        let a = "pub fn foo() {}";
        let b = "pub fn foo( this is not valid rust";
        assert_eq!(public_api_delta(a, b, Language::Rust), 0);
        assert!(parse(b).is_none());
    }

    #[test]
    fn impl_methods_are_tracked() {
        let a = "pub struct Foo; impl Foo { pub fn a(&self) {} }";
        let b = "pub struct Foo; impl Foo { pub fn a(&self) {} pub fn b(&self) {} }";
        assert_eq!(public_api_delta(a, b, Language::Rust), 1);
    }
}
