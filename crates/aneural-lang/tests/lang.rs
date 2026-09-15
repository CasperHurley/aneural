use aneural_core::config::TypeScriptConfig;
use aneural_lang::{ImportKind, Resolved, Resolver, analyze_file, extract_imports, run_query};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .canonicalize()
        .unwrap()
}

fn analyze(rel: &str) -> (PathBuf, Vec<(aneural_lang::ImportRef, Resolved)>) {
    let root = fixtures();
    let file = root.join(rel);
    let src = std::fs::read(&file).unwrap();
    let resolver = Resolver::new(&root, &TypeScriptConfig::default());
    let a = analyze_file(&resolver, &file, &src).unwrap();
    (root, a.imports)
}

fn find<'a>(imports: &'a [(aneural_lang::ImportRef, Resolved)], spec: &str) -> &'a Resolved {
    &imports
        .iter()
        .find(|(i, _)| i.specifier == spec)
        .unwrap_or_else(|| panic!("no import {spec}"))
        .1
}

fn file(root: &Path, rel: &str) -> Resolved {
    Resolved::File {
        path: root.join(rel),
    }
}

#[test]
fn abi_all_grammars_load() {
    aneural_lang::abi_check().unwrap();
}

#[test]
fn lang_for_path_maps_extensions() {
    assert_eq!(
        aneural_lang::lang_for_path(Path::new("a/b.tsx")),
        Some("typescript")
    );
    assert_eq!(aneural_lang::lang_for_path(Path::new("a/b.md")), None);
    assert_eq!(aneural_lang::lang_for_path(Path::new("Makefile")), None);
}

#[test]
fn typescript_extraction_and_resolution() {
    let (root, imports) = analyze("ts-app/src/index.ts");
    let kinds: Vec<(String, ImportKind)> = imports
        .iter()
        .map(|(i, _)| (i.specifier.clone(), i.kind))
        .collect();
    assert!(kinds.contains(&("./types".into(), ImportKind::TypeOnly)));
    assert!(kinds.contains(&("./dyn".into(), ImportKind::Dynamic)));
    assert!(kinds.contains(&("./cjs.cjs".into(), ImportKind::Require)));
    assert!(kinds.contains(&("./lib/thing".into(), ImportKind::ReExport)));
    let util = imports
        .iter()
        .find(|(i, _)| i.specifier == "./util.js" && i.kind == ImportKind::Static)
        .unwrap();
    assert_eq!(util.0.symbols, vec!["helper"]);
    assert_eq!(util.0.line, 1);
    let thing = imports
        .iter()
        .find(|(i, _)| i.specifier == "@/lib/thing")
        .unwrap();
    assert_eq!(thing.0.symbols, vec!["default", "Kind"]);

    // .js suffix → .ts file
    assert_eq!(
        find(&imports, "./util.js"),
        &file(&root, "ts-app/src/util.ts")
    );
    // tsconfig paths alias
    assert_eq!(
        find(&imports, "@/lib/thing"),
        &file(&root, "ts-app/src/lib/thing.ts")
    );
    assert_eq!(
        find(&imports, "./types"),
        &file(&root, "ts-app/src/types.ts")
    );
    assert_eq!(find(&imports, "./dyn"), &file(&root, "ts-app/src/dyn.ts"));
    assert_eq!(
        find(&imports, "./cjs.cjs"),
        &file(&root, "ts-app/src/cjs.cjs")
    );
    // node_modules → external
    assert_eq!(
        find(&imports, "react"),
        &Resolved::External {
            ecosystem: "npm".into(),
            name: "react".into()
        }
    );
    assert_eq!(
        find(&imports, "@scope/pkg"),
        &Resolved::External {
            ecosystem: "npm".into(),
            name: "@scope/pkg".into()
        }
    );
    // missing bare package → external by name
    assert_eq!(
        find(&imports, "lodash"),
        &Resolved::External {
            ecosystem: "npm".into(),
            name: "lodash".into()
        }
    );
    assert_eq!(
        find(&imports, "node:fs"),
        &Resolved::Unresolved {
            reason: "stdlib".into()
        }
    );
    assert!(matches!(
        find(&imports, "@/missing"),
        Resolved::Unresolved { .. }
    ));
}

#[test]
fn tsx_uses_tsx_grammar() {
    let (root, imports) = analyze("ts-app/src/App.tsx");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        find(&imports, "./index"),
        &file(&root, "ts-app/src/index.ts")
    );
}

#[test]
fn javascript_extraction() {
    let src = b"const a = require('./a');\nimport b from './b';\nexport * from './c';\n";
    let refs = extract_imports("javascript", Path::new("x.js"), src).unwrap();
    assert_eq!(refs.len(), 3);
    assert_eq!(refs[0].kind, ImportKind::Require);
    assert_eq!(refs[2].kind, ImportKind::ReExport);
}

#[test]
fn python_extraction_and_resolution() {
    let (root, imports) = analyze("py/pkg/sub/deep.py");
    assert_eq!(
        find(&imports, "os"),
        &Resolved::Unresolved {
            reason: "stdlib".into()
        }
    );
    assert_eq!(
        find(&imports, "requests"),
        &Resolved::External {
            ecosystem: "pypi".into(),
            name: "requests".into()
        }
    );
    assert_eq!(find(&imports, "..mod"), &file(&root, "py/pkg/mod.py"));
    assert_eq!(find(&imports, "."), &file(&root, "py/pkg/sub/helper.py"));
    assert_eq!(find(&imports, "pkg.mod"), &file(&root, "py/pkg/mod.py"));
    assert_eq!(
        find(&imports, "pkg.sub.helper"),
        &file(&root, "py/pkg/sub/helper.py")
    );
    assert_eq!(find(&imports, "pkg"), &file(&root, "py/pkg/mod.py"));
    let from = imports
        .iter()
        .find(|(i, _)| i.specifier == "..mod")
        .unwrap();
    assert_eq!(from.0.symbols, vec!["f"]);
}

#[test]
fn rust_extraction_and_resolution() {
    let (root, imports) = analyze("rs/src/lib.rs");
    let a = imports
        .iter()
        .find(|(i, _)| i.specifier == "a" && i.kind == ImportKind::Mod)
        .unwrap();
    assert_eq!(a.1, file(&root, "rs/src/a/mod.rs"));
    let util = imports
        .iter()
        .find(|(i, _)| i.specifier == "util" && i.kind == ImportKind::Mod)
        .unwrap();
    assert_eq!(util.1, file(&root, "rs/src/util.rs"));
    // crate::a::b::thing → thing is an item in a/b.rs
    assert_eq!(
        find(&imports, "crate::a::b::thing"),
        &file(&root, "rs/src/a/b.rs")
    );
    assert_eq!(
        find(&imports, "serde::Serialize"),
        &Resolved::External {
            ecosystem: "cargo".into(),
            name: "serde".into()
        }
    );
    assert_eq!(
        find(&imports, "std::fmt"),
        &Resolved::Unresolved {
            reason: "stdlib".into()
        }
    );
    assert_eq!(
        find(&imports, "tokio_util::codec"),
        &Resolved::External {
            ecosystem: "cargo".into(),
            name: "tokio-util".into()
        }
    );

    let (root, imports) = analyze("rs/src/a/b.rs");
    let inner = imports
        .iter()
        .find(|(i, _)| i.kind == ImportKind::Mod)
        .unwrap();
    assert_eq!(inner.1, file(&root, "rs/src/a/b/inner.rs"));
    assert_eq!(
        find(&imports, "super::super::util::x"),
        &file(&root, "rs/src/util.rs")
    );
    assert_eq!(
        find(&imports, "self::inner::y"),
        &file(&root, "rs/src/a/b/inner.rs")
    );
    assert_eq!(
        find(&imports, "crate::util"),
        &file(&root, "rs/src/util.rs")
    );

    let (root, imports) = analyze("rs/src/a/mod.rs");
    assert_eq!(find(&imports, "b"), &file(&root, "rs/src/a/b.rs"));
    assert_eq!(
        find(&imports, "super::util"),
        &file(&root, "rs/src/util.rs")
    );
}

#[test]
fn go_extraction_and_resolution() {
    let (root, imports) = analyze("go/main.go");
    assert_eq!(
        find(&imports, "fmt"),
        &Resolved::Unresolved {
            reason: "stdlib".into()
        }
    );
    assert_eq!(
        find(&imports, "example.com/app/internal/store"),
        &Resolved::Directory {
            path: root.join("go/internal/store")
        }
    );
    assert_eq!(
        find(&imports, "github.com/x/y"),
        &Resolved::External {
            ecosystem: "go".into(),
            name: "github.com/x/y".into()
        }
    );
}

#[test]
fn java_extraction_and_resolution() {
    let (root, imports) = analyze("java/src/main/java/com/acme/App.java");
    assert_eq!(
        find(&imports, "com.acme.util.Strings"),
        &file(&root, "java/src/main/java/com/acme/util/Strings.java")
    );
    assert_eq!(
        find(&imports, "com.acme.model"),
        &Resolved::Directory {
            path: root.join("java/src/main/java/com/acme/model")
        }
    );
    let stat = imports
        .iter()
        .find(|(i, _)| i.symbols == vec!["trim"])
        .unwrap();
    assert_eq!(stat.0.specifier, "com.acme.util.Strings");
    assert_eq!(
        find(&imports, "java.util.List"),
        &Resolved::Unresolved {
            reason: "stdlib".into()
        }
    );
    assert_eq!(
        find(&imports, "org.apache.commons.lang3.StringUtils"),
        &Resolved::External {
            ecosystem: "maven".into(),
            name: "org.apache".into()
        }
    );
}

#[test]
fn php_extraction_and_resolution() {
    let (root, imports) = analyze("php/src/Http/Kernel.php");
    assert_eq!(
        find(&imports, "App\\Support\\Helper"),
        &file(&root, "php/src/Support/Helper.php")
    );
    assert!(
        imports
            .iter()
            .filter(|(i, _)| i.specifier == "App\\Support\\Helper")
            .count()
            >= 2,
        "group import expands"
    );
    assert!(matches!(
        find(&imports, "App\\Support\\Missing"),
        Resolved::Unresolved { .. } | Resolved::External { .. }
    ));
    assert_eq!(
        find(&imports, "Illuminate\\Foundation\\Application"),
        &Resolved::External {
            ecosystem: "packagist".into(),
            name: "illuminate/foundation".into()
        }
    );
    assert_eq!(
        find(&imports, "/../../bootstrap.php"),
        &file(&root, "php/src/Http/../../bootstrap.php")
    );
    assert_eq!(
        find(&imports, "Local.php"),
        &file(&root, "php/src/Http/Local.php")
    );
}

#[test]
fn ruby_extraction_and_resolution() {
    let (root, imports) = analyze("ruby/lib/app.rb");
    assert_eq!(
        find(&imports, "app/config"),
        &file(&root, "ruby/lib/app/config.rb")
    );
    assert_eq!(
        find(&imports, "app/util"),
        &file(&root, "ruby/lib/app/util.rb")
    );
    assert_eq!(
        find(&imports, "json"),
        &Resolved::Unresolved {
            reason: "stdlib".into()
        }
    );
    assert_eq!(
        find(&imports, "rails/all"),
        &Resolved::External {
            ecosystem: "rubygems".into(),
            name: "rails".into()
        }
    );
    assert_eq!(
        find(&imports, "user_helper"),
        &file(&root, "ruby/app/helpers/user_helper.rb")
    );
    let rel = imports
        .iter()
        .find(|(i, _)| i.specifier == "app/util")
        .unwrap();
    assert_eq!(rel.0.kind, ImportKind::Include);
}

#[test]
fn extraction_survives_syntax_errors() {
    let src =
        b"import a from './a';\nfunction f() { return }}\nconst x = ;\nimport b from './b';\n";
    let refs = extract_imports("typescript", Path::new("x.ts"), src).unwrap();
    let specs: Vec<&str> = refs.iter().map(|r| r.specifier.as_str()).collect();
    assert!(specs.contains(&"./a"));
    assert!(specs.contains(&"./b"));
}

#[test]
fn run_query_returns_captures_per_match() {
    let src = b"// claude: refactor this\nconst x = 1; // TODO later\n// claude: and this\n";
    let matches = run_query(
        "typescript",
        None,
        r#"((comment) @c (#match? @c "claude:"))"#,
        src,
    )
    .unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0][0].name, "c");
    assert_eq!(matches[0][0].line, 1);
    assert_eq!(matches[1][0].text, "// claude: and this");
    assert!(run_query("typescript", None, "(nonsense_node) @x", src).is_err());
}
