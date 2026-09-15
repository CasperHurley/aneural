use std::path::Path;
fn dump(lang: &str, path: &str, src: &str) {
    let mut p = aneural_lang::parser_for(lang, Some(Path::new(path))).unwrap();
    let tree = p.parse(src, None).unwrap();
    println!("=== {lang} ({path})\n{}\n", tree.root_node().to_sexp());
}
#[test]
#[ignore]
fn dump_all() {
    dump("typescript", "a.ts", "import x from 'a';\nimport {a, b as c} from './b';\nimport * as ns from 'ns';\nimport 'side';\nimport type {T} from './t';\nimport {type U, v} from './u';\nexport {q, r as s} from './q';\nexport * from './star';\nexport * as w from './w';\nconst m = await import('./dyn');\nconst r = require('./req');\nimport d = require('./d');\n");
    dump("python", "a.py", "import a.b\nimport a as b, c\nfrom .x import y\nfrom ..pkg import z as w, q\nfrom mod import *\n");
    dump("rust", "a.rs", "use crate::a::b;\nuse super::x;\nuse self::y::{z, w as v};\nuse serde::{Serialize, Deserialize};\nuse std::collections::*;\nmod foo;\npub mod bar;\nmod inline { }\nuse {a::b, c};\n");
    dump("go", "a.go", "package main\nimport \"fmt\"\nimport (\n\t\"os\"\n\tf \"github.com/x/y/z\"\n\t_ \"embed\"\n)\n");
    dump("java", "A.java", "package a;\nimport a.b.C;\nimport static a.b.C.d;\nimport a.b.*;\n");
    dump("php", "a.php", "<?php\nnamespace App;\nuse A\\B\\C;\nuse A\\B\\{C, D as E};\nuse function A\\f;\nrequire 'x.php';\nrequire_once('y.php');\ninclude __DIR__ . '/z.php';\n");
    dump("ruby", "a.rb", "require 'x'\nrequire_relative 'y'\nload 'z.rb'\nrequire \"w\"\n");
}
