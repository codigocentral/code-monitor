use std::{env, fs};
use syn::{
    spanned::Spanned,
    visit::{self, Visit},
    Attribute, Meta,
};
#[derive(Default)]
struct Scope {
    ranges: Vec<[usize; 2]>,
}
fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && match &a.meta {
                Meta::List(list) => list.tokens.to_string() == "test",
                _ => false,
            }
    })
}
impl<'ast> Visit<'ast> for Scope {
    fn visit_item_mod(&mut self, n: &'ast syn::ItemMod) {
        if is_test(&n.attrs) {
            self.ranges
                .push([n.span().start().line, n.span().end().line]);
        } else {
            visit::visit_item_mod(self, n);
        }
    }
    fn visit_item_fn(&mut self, n: &'ast syn::ItemFn) {
        if is_test(&n.attrs) {
            self.ranges
                .push([n.span().start().line, n.span().end().line]);
        } else {
            visit::visit_item_fn(self, n);
        }
    }
    fn visit_impl_item_fn(&mut self, n: &'ast syn::ImplItemFn) {
        if is_test(&n.attrs) {
            self.ranges
                .push([n.span().start().line, n.span().end().line]);
        } else {
            visit::visit_impl_item_fn(self, n);
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files = serde_json::Map::new();
    for path in env::args().skip(1) {
        let code = fs::read_to_string(&path)?;
        let parsed = syn::parse_file(&code)?;
        let mut scope = Scope::default();
        scope.visit_file(&parsed);
        scope.ranges.sort_unstable();
        files.insert(path, serde_json::to_value(scope.ranges)?);
    }
    println!("{}", serde_json::to_string_pretty(&files)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_nested_test_modules_and_helper_methods_without_hiding_production() {
        let source="fn live() {let _ = r#\" } { \"#;}\n#[cfg(test)]\nmod tests {fn unit() {}}\nstruct Data;\nimpl Data {\n#[cfg(test)]\nfn helper() {}\nfn real() {}\n}\n";
        let mut scope = Scope::default();
        scope.visit_file(&syn::parse_file(source).unwrap());
        assert_eq!(scope.ranges, vec![[2, 3], [6, 7]]);
    }
}
