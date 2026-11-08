use std::sync::OnceLock;

use syntect::parsing::{SyntaxDefinition, SyntaxSet};

const ADDITIONAL_SYNTAXES: &[(&str, &str, &str)] = &[
    (
        "PowerShell",
        include_str!("../assets/syntaxes/PowerShell.sublime-syntax"),
        "$name = Get-Item \"hello\"\n",
    ),
    (
        "Nushell",
        include_str!("../assets/syntaxes/Nushell.sublime-syntax"),
        "let name = \"hello\"\n",
    ),
    (
        "Just",
        include_str!("../assets/syntaxes/Just.sublime-syntax"),
        "name := \"hello\"\n",
    ),
    (
        "JSON5",
        include_str!("../assets/syntaxes/JSON5.sublime-syntax"),
        "{name: 'hello', enabled: true}\n",
    ),
    (
        "CUE",
        include_str!("../assets/syntaxes/CUE.sublime-syntax"),
        "package hello\n",
    ),
    (
        "HOCON",
        include_str!("../assets/syntaxes/HOCON.sublime-syntax"),
        "name = \"hello\"\n",
    ),
    (
        "KDL",
        include_str!("../assets/syntaxes/KDL.sublime-syntax"),
        "node name=\"hello\"\n",
    ),
    (
        "Prisma",
        include_str!("../assets/syntaxes/Prisma.sublime-syntax"),
        "model User {\n",
    ),
    (
        "Gleam",
        include_str!("../assets/syntaxes/Gleam.sublime-syntax"),
        "pub fn hello() {\n",
    ),
    (
        "Caddyfile",
        include_str!("../assets/syntaxes/Caddyfile.sublime-syntax"),
        "reverse_proxy localhost:8080\n",
    ),
    (
        "Requirements",
        include_str!("../assets/syntaxes/Requirements.sublime-syntax"),
        "requests>=2.0\n",
    ),
    (
        "Bicep",
        include_str!("../assets/syntaxes/Bicep.sublime-syntax"),
        "param location string = 'eastus'\n",
    ),
    (
        "Meson",
        include_str!("../assets/syntaxes/Meson.sublime-syntax"),
        "project('hello', 'c')\n",
    ),
    (
        "WebAssembly",
        include_str!("../assets/syntaxes/WebAssembly.sublime-syntax"),
        "(module (func $answer (result i32) i32.const 42))\n",
    ),
    (
        "WebIDL",
        include_str!("../assets/syntaxes/WebIDL.sublime-syntax"),
        "interface Example { attribute DOMString name; };\n",
    ),
    (
        "Thrift",
        include_str!("../assets/syntaxes/Thrift.sublime-syntax"),
        "struct User { 1: required string name }\n",
    ),
    (
        "FlatBuffers",
        include_str!("../assets/syntaxes/FlatBuffers.sublime-syntax"),
        "table User { name:string; }\n",
    ),
    (
        "Mermaid",
        include_str!("../assets/syntaxes/Mermaid.sublime-syntax"),
        "flowchart TD\n",
    ),
    (
        "PlantUML",
        include_str!("../assets/syntaxes/PlantUML.sublime-syntax"),
        "@startuml\n",
    ),
    (
        "D2",
        include_str!("../assets/syntaxes/D2.sublime-syntax"),
        "a -> b: hello\n",
    ),
    (
        "Haxe",
        include_str!("../assets/syntaxes/Haxe.sublime-syntax"),
        "class Main { static function main() {} }\n",
    ),
    (
        "CUDA",
        include_str!("../assets/syntaxes/CUDA.sublime-syntax"),
        "__global__ void add(float *x) { x[threadIdx.x] += 1; }\n",
    ),
    (
        "HLSL",
        include_str!("../assets/syntaxes/HLSL.sublime-syntax"),
        "float4 main() : SV_Target { return float4(1, 0, 0, 1); }\n",
    ),
    (
        "Metal",
        include_str!("../assets/syntaxes/Metal.sublime-syntax"),
        "kernel void add(device float *x [[buffer(0)]]) {}\n",
    ),
    (
        "GDShader",
        include_str!("../assets/syntaxes/GDShader.sublime-syntax"),
        "shader_type spatial;\n",
    ),
    (
        "Luau",
        include_str!("../assets/syntaxes/Luau.sublime-syntax"),
        "export type Name = string\n",
    ),
    (
        "Mojo",
        include_str!("../assets/syntaxes/Mojo.sublime-syntax"),
        "fn main():\n",
    ),
    (
        "Gherkin",
        include_str!("../assets/syntaxes/Gherkin.sublime-syntax"),
        "Feature: Greeting\n",
    ),
    (
        "YARA",
        include_str!("../assets/syntaxes/YARA.sublime-syntax"),
        "rule example { condition: true }\n",
    ),
    (
        "Pkl",
        include_str!("../assets/syntaxes/Pkl.sublime-syntax"),
        "name: String = \"world\"\n",
    ),
    (
        "Dhall",
        include_str!("../assets/syntaxes/Dhall.sublime-syntax"),
        "let name = \"world\" in name\n",
    ),
    (
        "Nickel",
        include_str!("../assets/syntaxes/Nickel.sublime-syntax"),
        "let name = \"world\" in { name }\n",
    ),
    (
        "ReScript",
        include_str!("../assets/syntaxes/ReScript.sublime-syntax"),
        "let name: string = \"world\"\n",
    ),
    (
        "ABAP",
        include_str!("../assets/syntaxes/ABAP.sublime-syntax"),
        "DATA name TYPE string VALUE 'world'.\n",
    ),
    (
        "AutoHotkey",
        include_str!("../assets/syntaxes/AutoHotkey.sublime-syntax"),
        "MsgBox \"hello\"\n",
    ),
    (
        "OpenSCAD",
        include_str!("../assets/syntaxes/OpenSCAD.sublime-syntax"),
        "module box() { cube([1, 2, 3]); }\n",
    ),
    (
        "Stan",
        include_str!("../assets/syntaxes/Stan.sublime-syntax"),
        "parameters { real mu; }\n",
    ),
    (
        "TypeSpec",
        include_str!("../assets/syntaxes/TypeSpec.sublime-syntax"),
        "model User { name: string; }\n",
    ),
    (
        "G-code",
        include_str!("../assets/syntaxes/G-code.sublime-syntax"),
        "G1 X10.5 Y20 F1200\n",
    ),
    (
        "RON",
        include_str!("../assets/syntaxes/RON.sublime-syntax"),
        "Config(name: \"world\", enabled: true)\n",
    ),
    (
        "Smithy",
        include_str!("../assets/syntaxes/Smithy.sublime-syntax"),
        "structure User { name: String }\n",
    ),
    (
        "KCL",
        include_str!("../assets/syntaxes/KCL.sublime-syntax"),
        "schema User:\n",
    ),
    (
        "Hurl",
        include_str!("../assets/syntaxes/Hurl.sublime-syntax"),
        "GET https://example.invalid\n",
    ),
    (
        "Procfile",
        include_str!("../assets/syntaxes/Procfile.sublime-syntax"),
        "web: server --port $PORT\n",
    ),
    (
        "CODEOWNERS",
        include_str!("../assets/syntaxes/CODEOWNERS.sublime-syntax"),
        "/src/ @example/team\n",
    ),
    (
        "Cisco",
        include_str!("../assets/syntaxes/Cisco.sublime-syntax"),
        "interface GigabitEthernet0/1\n",
    ),
    (
        "Junos",
        include_str!("../assets/syntaxes/Junos.sublime-syntax"),
        "set system host-name edge\n",
    ),
    (
        "RouterOS",
        include_str!("../assets/syntaxes/RouterOS.sublime-syntax"),
        ":local name \"edge\"\n",
    ),
    (
        "BIRD",
        include_str!("../assets/syntaxes/BIRD.sublime-syntax"),
        "protocol bgp upstream {\n",
    ),
    (
        "YANG",
        include_str!("../assets/syntaxes/YANG.sublime-syntax"),
        "module example {\n",
    ),
    (
        "DNS Zone",
        include_str!("../assets/syntaxes/DNS-Zone.sublime-syntax"),
        "$TTL 3600\n",
    ),
    (
        "MoonBit",
        include_str!("../assets/syntaxes/MoonBit.sublime-syntax"),
        "fn main { println(\"hello\") }\n",
    ),
    (
        "Cangjie",
        include_str!("../assets/syntaxes/Cangjie.sublime-syntax"),
        "func greet(): String { return \"hello\" }\n",
    ),
    (
        "Wren",
        include_str!("../assets/syntaxes/Wren.sublime-syntax"),
        "class Greeter {}\n",
    ),
    (
        "Rhai",
        include_str!("../assets/syntaxes/Rhai.sublime-syntax"),
        "let greeting = \"hello\";\n",
    ),
    (
        "Fennel",
        include_str!("../assets/syntaxes/Fennel.sublime-syntax"),
        "(local greeting \"hello\")\n",
    ),
    (
        "Janet",
        include_str!("../assets/syntaxes/Janet.sublime-syntax"),
        "(def greeting \"hello\")\n",
    ),
    (
        "Starlark",
        include_str!("../assets/syntaxes/Starlark.sublime-syntax"),
        "load(\"//:rules.bzl\", \"rule\")\n",
    ),
    (
        "GN",
        include_str!("../assets/syntaxes/GN.sublime-syntax"),
        "import(\"//build/config.gni\")\n",
    ),
    (
        "Nextflow",
        include_str!("../assets/syntaxes/Nextflow.sublime-syntax"),
        "process GREET {\n",
    ),
    (
        "Kusto",
        include_str!("../assets/syntaxes/Kusto.sublime-syntax"),
        "Events | where count > 1\n",
    ),
    (
        "SurrealQL",
        include_str!("../assets/syntaxes/SurrealQL.sublime-syntax"),
        "SELECT * FROM person;\n",
    ),
    (
        "SPARQL",
        include_str!("../assets/syntaxes/SPARQL.sublime-syntax"),
        "SELECT ?name WHERE { ?s ?p ?name }\n",
    ),
    (
        "Turtle",
        include_str!("../assets/syntaxes/Turtle.sublime-syntax"),
        "@prefix ex: <https://example.invalid/> .\n",
    ),
    (
        "Vala",
        include_str!("../assets/syntaxes/Vala.sublime-syntax"),
        "public class Greeter : Object {}\n",
    ),
    (
        "Modelica",
        include_str!("../assets/syntaxes/Modelica.sublime-syntax"),
        "model Example\n",
    ),
    (
        "VCL",
        include_str!("../assets/syntaxes/VCL.sublime-syntax"),
        "vcl 4.1;\n",
    ),
    (
        "ANTLR",
        include_str!("../assets/syntaxes/ANTLR.sublime-syntax"),
        "grammar Example;\n",
    ),
    (
        "TLA+",
        include_str!("../assets/syntaxes/TLA.sublime-syntax"),
        "---- MODULE Example ----\n",
    ),
    (
        "WIT",
        include_str!("../assets/syntaxes/WIT.sublime-syntax"),
        "package example:greeting;\n",
    ),
    (
        "SAS",
        include_str!("../assets/syntaxes/SAS.sublime-syntax"),
        "data example;\n",
    ),
    (
        "Stata",
        include_str!("../assets/syntaxes/Stata.sublime-syntax"),
        "generate example = 42\n",
    ),
    (
        "AutoIt",
        include_str!("../assets/syntaxes/AutoIt.sublime-syntax"),
        "Local $greeting = \"hello\"\n",
    ),
    (
        "Visual Basic .NET",
        include_str!("../assets/syntaxes/Visual-Basic.sublime-syntax"),
        "Public Class Greeter\n",
    ),
    (
        "Move",
        include_str!("../assets/syntaxes/Move.sublime-syntax"),
        "module example::greeting {\n",
    ),
    (
        "Cairo",
        include_str!("../assets/syntaxes/Cairo.sublime-syntax"),
        "fn greeting() -> felt252 { 42 }\n",
    ),
    (
        "Noir",
        include_str!("../assets/syntaxes/Noir.sublime-syntax"),
        "fn main(x: Field) { assert(x == 42); }\n",
    ),
    (
        "Ballerina",
        include_str!("../assets/syntaxes/Ballerina.sublime-syntax"),
        "public function main() {}\n",
    ),
    (
        "Astro",
        include_str!("../assets/syntaxes/Astro.sublime-syntax"),
        "{true}\n",
    ),
    (
        "MDX",
        include_str!("../assets/syntaxes/MDX.sublime-syntax"),
        "{true}\n",
    ),
    (
        "EJS",
        include_str!("../assets/syntaxes/EJS.sublime-syntax"),
        "<%= true %>\n",
    ),
    (
        "Liquid",
        include_str!("../assets/syntaxes/Liquid.sublime-syntax"),
        "{% assign name = \"hello\" %}\n",
    ),
    (
        "Handlebars",
        include_str!("../assets/syntaxes/Handlebars.sublime-syntax"),
        "{{#if ready}}\n",
    ),
    (
        "Nunjucks",
        include_str!("../assets/syntaxes/Nunjucks.sublime-syntax"),
        "{% if ready %}\n",
    ),
    (
        "CodeQL",
        include_str!("../assets/syntaxes/CodeQL.sublime-syntax"),
        "from int n where n = 42 select n\n",
    ),
    (
        "Cypher",
        include_str!("../assets/syntaxes/Cypher.sublime-syntax"),
        "MATCH (n:Person) RETURN n\n",
    ),
    (
        "EdgeQL",
        include_str!("../assets/syntaxes/EdgeQL.sublime-syntax"),
        "select User filter .name = 'hello';\n",
    ),
    (
        "Cap'n Proto",
        include_str!("../assets/syntaxes/Capn-Proto.sublime-syntax"),
        "struct User { name @0 :Text; }\n",
    ),
    (
        "AIDL",
        include_str!("../assets/syntaxes/AIDL.sublime-syntax"),
        "interface Greeter { String greet(); }\n",
    ),
    (
        "P4",
        include_str!("../assets/syntaxes/P4.sublime-syntax"),
        "header ethernet_t { bit<48> dst; }\n",
    ),
    (
        "MLIR",
        include_str!("../assets/syntaxes/MLIR.sublime-syntax"),
        "%value = arith.constant 42 : i32\n",
    ),
    (
        "Slang",
        include_str!("../assets/syntaxes/Slang.sublime-syntax"),
        "float4 main() { return float4(1); }\n",
    ),
    (
        "ISPC",
        include_str!("../assets/syntaxes/ISPC.sublime-syntax"),
        "export void fill(uniform float data[]) {}\n",
    ),
    (
        "Hy",
        include_str!("../assets/syntaxes/Hy.sublime-syntax"),
        "(defn greet [] (print \"hello\"))\n",
    ),
    (
        "Scilab",
        include_str!("../assets/syntaxes/Scilab.sublime-syntax"),
        "function y = square(x)\n",
    ),
    (
        "XQuery",
        include_str!("../assets/syntaxes/XQuery.sublime-syntax"),
        "for $x in (1, 2) return $x\n",
    ),
    (
        "Dafny",
        include_str!("../assets/syntaxes/Dafny.sublime-syntax"),
        "method Main() { assert true; }\n",
    ),
    (
        "Agda",
        include_str!("../assets/syntaxes/Agda.sublime-syntax"),
        "module Main where\n",
    ),
    (
        "Koka",
        include_str!("../assets/syntaxes/Koka.sublime-syntax"),
        "fun main() { println(\"hello\") }\n",
    ),
    (
        "Pony",
        include_str!("../assets/syntaxes/Pony.sublime-syntax"),
        "actor Main\n",
    ),
];

#[cfg(test)]
pub(crate) fn bundled_syntax_samples() -> impl Iterator<Item = (&'static str, &'static str)> {
    ADDITIONAL_SYNTAXES
        .iter()
        .map(|(name, _, example)| (*name, *example))
}

pub(crate) fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut builder = two_face::syntax::extra_newlines().into_builder();
        builder.add(
            SyntaxDefinition::load_from_str(
                include_str!("../assets/syntaxes/C-family.sublime-syntax"),
                true,
                None,
            )
            .expect("invalid bundled C-family token definitions"),
        );
        for (name, source, _) in ADDITIONAL_SYNTAXES {
            builder.add(
                SyntaxDefinition::load_from_str(source, true, None)
                    .unwrap_or_else(|error| panic!("invalid bundled {name} syntax: {error}")),
            );
        }
        builder.build()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntect::parsing::{ParseState, ScopeStack};

    fn assert_token_scope(language: &str, source: &str, token: &str, expected: &str) {
        let registry = syntax_set();
        let syntax = registry.find_syntax_by_name(language).unwrap();
        let mut state = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        let target = source.find(token).unwrap();
        let mut line_start = 0;
        for line in syntect::util::LinesWithEndings::from(source) {
            let operations = state.parse_line(line, registry).unwrap();
            if target < line_start + line.len() {
                for (offset, operation) in operations {
                    if line_start + offset > target {
                        break;
                    }
                    stack.apply(&operation).unwrap();
                }
                assert!(
                    stack
                        .scopes
                        .iter()
                        .any(|scope| scope.to_string().starts_with(expected)),
                    "{language}: {token:?} should be {expected}, found {stack:?}"
                );
                return;
            }
            for (_, operation) in operations {
                stack.apply(&operation).unwrap();
            }
            line_start += line.len();
        }
        panic!("missing token {token:?} in {language}");
    }

    #[test]
    fn bundled_grammar_includes_resolve_without_new_missing_contexts() {
        let baseline = two_face::syntax::extra_newlines().find_unlinked_contexts();
        let registry = syntax_set();
        let unresolved = registry.find_unlinked_contexts();
        assert!(
            unresolved.is_subset(&baseline),
            "new unresolved syntax includes: {:?}",
            unresolved.difference(&baseline).collect::<Vec<_>>()
        );
    }

    #[test]
    fn bundled_syntaxes_parse_real_tokens_with_the_runtime_regex_backend() {
        let registry = syntax_set();
        for (name, example) in bundled_syntax_samples() {
            let syntax = registry.find_syntax_by_name(name).unwrap();
            let operations = ParseState::new(syntax)
                .parse_line(example, registry)
                .unwrap();
            let mut scopes = ScopeStack::new();
            let mut token_scope = false;
            for (_, operation) in operations {
                scopes.apply(&operation).unwrap();
                token_scope |= scopes.scopes.len() > 1;
            }

            assert!(token_scope, "{name} did not highlight its example");
        }
    }

    #[test]
    fn every_bundled_syntax_file_is_registered() {
        let expected: std::collections::BTreeSet<_> = std::fs::read_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/syntaxes"),
        )
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sublime-syntax"))
        .map(|path| {
            let source = std::fs::read_to_string(path).unwrap();
            SyntaxDefinition::load_from_str(&source, true, None)
                .unwrap()
                .name
        })
        .collect();
        let actual: std::collections::BTreeSet<_> = syntax_set()
            .syntaxes()
            .iter()
            .map(|syntax| syntax.name.clone())
            .collect();
        assert!(
            expected.is_subset(&actual),
            "unregistered grammars: {:?}",
            expected.difference(&actual).collect::<Vec<_>>()
        );
    }

    #[test]
    fn query_interface_and_computation_grammars_have_distinct_token_scopes() {
        for (language, source, token, scope) in [
            ("CodeQL", "from int n select n\n", "from", "keyword.control"),
            ("CodeQL", "// select ignored\n", "select", "comment.line"),
            ("Cypher", "mAtCh (n) RETURN n\n", "mAtCh", "keyword.control"),
            ("Cypher", "RETURN $name\n", "$name", "variable.parameter"),
            ("Cypher", "RETURN 'MATCH'\n", "MATCH", "string.quoted"),
            ("EdgeQL", "select User;\n", "select", "keyword.control"),
            (
                "EdgeQL",
                "select $$a ' quote$$;\n",
                "quote",
                "string.quoted.other.dollar",
            ),
            (
                "EdgeQL",
                "select $tag$a $ quote$tag$;\n",
                "quote",
                "string.quoted.other.dollar",
            ),
            (
                "Cap'n Proto",
                "struct User {}\n",
                "struct",
                "keyword.declaration",
            ),
            (
                "Cap'n Proto",
                "name @0 :Text;\n",
                "@0",
                "constant.numeric.ordinal",
            ),
            (
                "AIDL",
                "oneway interface Greeter {}\n",
                "oneway",
                "keyword.declaration",
            ),
            (
                "AIDL",
                "@nullable String name;\n",
                "@nullable",
                "variable.annotation",
            ),
            (
                "P4",
                "header ethernet_t { bit<48> dst; }\n",
                "header",
                "keyword.control",
            ),
            (
                "P4",
                "bit<8> value = 8w0xff;\n",
                "8w0xff",
                "constant.numeric",
            ),
            (
                "MLIR",
                "%value = arith.constant 42 : i32\n",
                "%value",
                "variable.other.ssa",
            ),
            (
                "MLIR",
                "%value = arith.constant 42 : i32\n",
                "arith.constant",
                "keyword.other.operation",
            ),
            (
                "Slang",
                "interface IShader {}\n",
                "interface",
                "keyword.control",
            ),
            ("Slang", "float4 color;\n", "float4", "storage.type"),
            (
                "ISPC",
                "foreach (i = 0 ... count) {}\n",
                "foreach",
                "keyword.control",
            ),
            (
                "ISPC",
                "int i = programIndex;\n",
                "programIndex",
                "variable.language",
            ),
            ("Hy", "(defn greet [] True)\n", "defn", "keyword.control"),
            ("Hy", "; defn ignored\n", "defn", "comment.line"),
            (
                "Scilab",
                "function y = square(x)\n",
                "function",
                "keyword.control",
            ),
            ("Scilab", "x = A'; y = %pi;\n", "%pi", "constant.language"),
            ("Scilab", "disp('a''b')\n", "b", "string.quoted"),
            (
                "XQuery",
                "for $x in (1,2) return $x\n",
                "$x",
                "variable.other",
            ),
            (
                "XQuery",
                "(: outer (: inner :) still :)\n",
                "still",
                "comment.block",
            ),
            (
                "Dafny",
                "method Main() requires true {}\n",
                "requires",
                "keyword.control",
            ),
            (
                "Dafny",
                "/* outer /* inner */ still */\n",
                "still",
                "comment.block",
            ),
            ("Agda", "module Main where\n", "module", "keyword.control"),
            (
                "Agda",
                "{- outer {- inner -} still -}\n",
                "still",
                "comment.block",
            ),
            (
                "Koka",
                "effect ask { fun question(): string }\n",
                "effect",
                "keyword.control",
            ),
            (
                "Koka",
                "/* outer /* inner */ still */\n",
                "still",
                "comment.block",
            ),
            ("Pony", "actor Main\n", "actor", "keyword.control"),
            (
                "Pony",
                "\"\"\"\nactor text\n\"\"\"\n",
                "actor",
                "string.quoted.triple",
            ),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }

    #[test]
    fn active_language_grammars_have_language_specific_token_scopes() {
        for (language, source, token, scope) in [
            (
                "Cisco",
                "interface GigabitEthernet0/1\n",
                "interface",
                "keyword.control",
            ),
            (
                "Cisco",
                "interface GigabitEthernet0/1\n",
                "GigabitEthernet0/1",
                "entity.name.type.interface",
            ),
            (
                "Cisco",
                " ip address 192.0.2.1 255.255.255.0\n",
                "192.0.2.1",
                "constant.numeric.address",
            ),
            (
                "Cisco",
                "! interface ignored\n",
                "interface",
                "comment.line",
            ),
            (
                "Cisco",
                "banner motd #\ninterface in banner\n#\ninterface Loopback0\n",
                "interface in banner",
                "string.unquoted.banner",
            ),
            (
                "Junos",
                "set interfaces ge-0/0/0 unit 0\n",
                "ge-0/0/0",
                "entity.name.type.interface",
            ),
            (
                "Junos",
                "/* set ignored */\nset system host-name edge\n",
                "set ignored",
                "comment.block",
            ),
            (
                "RouterOS",
                ":local name \"hello\"\n",
                ":local",
                "keyword.control",
            ),
            ("RouterOS", ":put $name\n", "$name", "variable.other"),
            (
                "RouterOS",
                ":put \"if # not a comment\"\n",
                "#",
                "string.quoted",
            ),
            (
                "BIRD",
                "protocol bgp upstream { local as 64500; }\n",
                "protocol",
                "keyword.control",
            ),
            ("BIRD", "# protocol ignored\n", "protocol", "comment.line"),
            (
                "YANG",
                "module example { leaf count { type uint32; } }\n",
                "uint32",
                "storage.type",
            ),
            (
                "YANG",
                "description \"leaf is text\";\n",
                "leaf",
                "string.quoted",
            ),
            (
                "DNS Zone",
                "@ 3600 IN A 192.0.2.1 ; A record\n",
                "IN",
                "storage.type",
            ),
            (
                "DNS Zone",
                "@ IN TXT \"a;b\" ; note\n",
                ";b",
                "string.quoted",
            ),
            (
                "MoonBit",
                "fn main { let count : Int = 42 }\n",
                "fn",
                "keyword.control",
            ),
            (
                "MoonBit",
                "let name = \"fn is text\"\n",
                "fn",
                "string.quoted",
            ),
            (
                "Cangjie",
                "func greet(): String { return \"hello\" }\n",
                "String",
                "storage.type",
            ),
            (
                "Cangjie",
                "/* outer /* nested */ still */\nlet n = 42\n",
                "still",
                "comment.block",
            ),
            (
                "Wren",
                "class Greeter { construct new() {} }\n",
                "construct",
                "keyword.control",
            ),
            (
                "Wren",
                "/* outer /* inner */ still */\nvar n = 42\n",
                "still",
                "comment.block",
            ),
            (
                "Rhai",
                "fn greet() { let n = 42; }\n",
                "fn",
                "keyword.control",
            ),
            (
                "Rhai",
                "/* outer /* inner */ still */\nlet n = 42;\n",
                "still",
                "comment.block",
            ),
            (
                "Fennel",
                "(local name :greeting)\n",
                "local",
                "keyword.control",
            ),
            ("Fennel", "; local ignored\n", "local", "comment.line"),
            ("Janet", "(def name :greeting)\n", "def", "keyword.control"),
            (
                "Janet",
                "(def name ``hello ` world``)\n",
                "world",
                "string.quoted.other.long",
            ),
            (
                "Starlark",
                "load(\"//:rules.bzl\", \"rule\")\n",
                "load",
                "keyword.control",
            ),
            (
                "Starlark",
                "name = \"# not a comment\"\n",
                "#",
                "string.quoted",
            ),
            (
                "GN",
                "template(\"greet\") {}\n",
                "template",
                "keyword.control",
            ),
            (
                "GN",
                "name = \"$target_name\"\n",
                "$target_name",
                "variable.other",
            ),
            (
                "Nextflow",
                "process GREET { cpus 2 }\n",
                "process",
                "keyword.control",
            ),
            (
                "Nextflow",
                "script:\n\"\"\"\necho hello\n\"\"\"\n",
                "echo",
                "string.quoted.triple",
            ),
            (
                "Kusto",
                "Events | summarize count() by name\n",
                "summarize",
                "keyword.control",
            ),
            (
                "Kusto",
                "let name = @\"a\"\"b\";\n",
                "b",
                "string.quoted.double.verbatim",
            ),
            (
                "SurrealQL",
                "SELECT * FROM person WHERE age > 42;\n",
                "SELECT",
                "keyword.control",
            ),
            (
                "SurrealQL",
                "LET $name = 'hello';\n",
                "$name",
                "variable.other",
            ),
            (
                "SPARQL",
                "SELECT ?name WHERE { ?s ?p ?name }\n",
                "SELECT",
                "keyword.control",
            ),
            (
                "SPARQL",
                "SELECT ?name WHERE { ?s ?p ?name }\n",
                "?name",
                "variable.other",
            ),
            (
                "Turtle",
                "@prefix ex: <https://example.invalid/> .\n",
                "@prefix",
                "keyword.control",
            ),
            ("Turtle", "ex:s ex:p \"# text\" .\n", "#", "string.quoted"),
            (
                "Vala",
                "public class Greeter : Object { string name; }\n",
                "string",
                "storage.type",
            ),
            (
                "Vala",
                "string name = \"class is text\";\n",
                "class",
                "string.quoted",
            ),
            (
                "Modelica",
                "model Example\n parameter Real value = 42;\nend Example;\n",
                "parameter",
                "keyword.control",
            ),
            (
                "Modelica",
                "model Example\n parameter Real value = 42;\nend Example;\n",
                "Real",
                "storage.type",
            ),
            (
                "VCL",
                "sub vcl_recv { set req.http.host = \"example.invalid\"; }\n",
                "req.http.host",
                "variable.other.member",
            ),
            (
                "VCL",
                "set req.http.host = {\"multi\nline\"};\n",
                "line",
                "string.quoted.other.long",
            ),
            (
                "ANTLR",
                "grammar Example;\nstart: HELLO EOF;\n",
                "grammar",
                "keyword.declaration",
            ),
            (
                "ANTLR",
                "grammar Example;\nstart: HELLO EOF;\n",
                "start",
                "entity.name.function.rule",
            ),
            (
                "TLA+",
                "---- MODULE Example ----\nVARIABLE count\n",
                "VARIABLE",
                "keyword.control",
            ),
            (
                "TLA+",
                "(* outer (* inner *) still *)\nVARIABLE count\n",
                "still",
                "comment.block",
            ),
            (
                "WIT",
                "interface example { greet: func(name: string); }\n",
                "func",
                "keyword.declaration",
            ),
            (
                "WIT",
                "interface example { greet: func(name: string); }\n",
                "string",
                "storage.type",
            ),
            (
                "SAS",
                "%let name = hello;\n",
                "%let",
                "keyword.control.macro",
            ),
            ("SAS", "name = 'can''t';\n", "t", "string.quoted"),
            (
                "Stata",
                "generate name = 42\n",
                "generate",
                "keyword.control",
            ),
            (
                "Stata",
                "display `\"a \"quoted\" value\"'\n",
                "quoted",
                "string.quoted.other.compound",
            ),
            (
                "AutoIt",
                "Local $name = \"hello\"\n",
                "$name",
                "variable.other",
            ),
            (
                "AutoIt",
                "#cs\nLocal ignored\n#ce\n",
                "Local",
                "comment.block",
            ),
            (
                "Visual Basic .NET",
                "Public Class Greeter\n",
                "Class",
                "keyword.control",
            ),
            (
                "Visual Basic .NET",
                "Dim name As String = \"say \"\"hello\"\"\"\n",
                "hello",
                "string.quoted",
            ),
            (
                "Move",
                "module example::greeting { public fun greet() {} }\n",
                "fun",
                "keyword.control",
            ),
            ("Move", "const VALUE: u64 = 42;\n", "u64", "storage.type"),
            (
                "Cairo",
                "fn greet() -> felt252 { 42 }\n",
                "felt252",
                "storage.type",
            ),
            ("Cairo", "let name = 'hello';\n", "hello", "string.quoted"),
            (
                "Noir",
                "fn main(x: Field) { assert(x == 42); }\n",
                "Field",
                "storage.type",
            ),
            ("Noir", "// fn ignored\n", "fn", "comment.line"),
            (
                "Ballerina",
                "public function main() { string name = \"hello\"; }\n",
                "function",
                "keyword.control",
            ),
            (
                "Ballerina",
                "string name = `hello`;\n",
                "hello",
                "string.quoted.other.template",
            ),
            (
                "Astro",
                "---\nconst name = 'hello';\n---\n<p>{name}</p>\n",
                "const",
                "storage",
            ),
            ("Astro", "{true}\n", "true", "constant.language"),
            ("MDX", "export const name = 'hello';\n", "const", "storage"),
            ("MDX", "{true}\n", "true", "constant.language"),
            ("EJS", "<% if (ready) { %>\n", "if", "keyword.control"),
            ("EJS", "<%# if ignored %>\n", "if", "comment.block"),
            (
                "Liquid",
                "{% assign name = \"hello\" %}\n",
                "assign",
                "keyword.control",
            ),
            (
                "Liquid",
                "{% comment %}if ignored{% endcomment %}\n",
                "if",
                "comment.block",
            ),
            ("Handlebars", "{{#if ready}}\n", "if", "keyword.control"),
            (
                "Handlebars",
                "{{!-- if ignored --}}\n",
                "if",
                "comment.block",
            ),
            ("Nunjucks", "{% if ready %}\n", "if", "keyword.control"),
            ("Nunjucks", "{# if ignored #}\n", "if", "comment.block"),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }

    #[test]
    fn template_embedded_contexts_return_to_markup() {
        for (language, source, token, scope) in [
            (
                "EJS",
                "<% if (ready) { %><article>hello</article><% } %>\n",
                "article",
                "entity.name.tag",
            ),
            (
                "Astro",
                "---\nconst settings = { name: \"hello\" };\n---\n<article>{settings.name}</article>\n",
                "article",
                "entity.name.tag",
            ),
            (
                "Astro",
                "{ { name: \"}\" } }\n<article />\n",
                "article",
                "entity.name.tag",
            ),
            (
                "Liquid",
                "{% raw %}{{ if }}{% endraw %}<article />\n",
                "article",
                "entity.name.tag",
            ),
            (
                "Handlebars",
                "{{#if ready}}<article />{{/if}}\n",
                "article",
                "entity.name.tag",
            ),
            (
                "Nunjucks",
                "{% if ready %}<article />{% endif %}\n",
                "article",
                "entity.name.tag",
            ),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }

    #[test]
    fn configuration_and_http_languages_highlight_semantic_tokens() {
        for (language, source, token, scope) in [
            (
                "RON",
                "Config(name: \"hello\", enabled: true) // Config\n",
                "Config",
                "entity.name.type",
            ),
            (
                "RON",
                "Config(name: \"hello\", enabled: true) // Config\n",
                "name",
                "variable.other.member",
            ),
            (
                "RON",
                "Config(name: \"hello\", enabled: true) // Config\n",
                "hello",
                "string.quoted",
            ),
            (
                "RON",
                "Config(name: \"hello\", enabled: true) // Config\n",
                "true",
                "constant.language",
            ),
            (
                "RON",
                "Config(name: \"hello\", enabled: true) // Config\n",
                "// Config",
                "comment.line",
            ),
            (
                "RON",
                "/* outer /* nested */ still */\nConfig(count: 42)\n",
                "still",
                "comment.block",
            ),
            (
                "RON",
                "/* outer /* nested */ still */\nConfig(count: 42)\n",
                "Config",
                "entity.name.type",
            ),
            (
                "RON",
                "Config(name: r##\"hello \" world\"##, count: 42)\n",
                "world",
                "string.quoted.double.raw",
            ),
            (
                "RON",
                "Config(name: r##\"hello \" world\"##, count: 42)\n",
                "42",
                "constant.numeric",
            ),
            (
                "Smithy",
                "@documentation(\"hello\")\nstructure User { name: String } // User\n",
                "@documentation",
                "entity.name.function.trait",
            ),
            (
                "Smithy",
                "@documentation(\"hello\")\nstructure User { name: String } // User\n",
                "structure",
                "keyword.declaration",
            ),
            (
                "Smithy",
                "@documentation(\"hello\")\nstructure User { name: String } // User\n",
                "String",
                "storage.type",
            ),
            (
                "Smithy",
                "@documentation(\"hello\")\nstructure User { name: String } // User\n",
                "hello",
                "string.quoted",
            ),
            (
                "Smithy",
                "@documentation(\"hello\")\nstructure User { name: String } // User\n",
                "// User",
                "comment.line",
            ),
            (
                "Smithy",
                "@documentation(\"\"\"\n// text\n\"\"\")\nstructure User {}\n",
                "// text",
                "string.quoted.triple",
            ),
            (
                "Smithy",
                "@documentation(\"\"\"\n// text\n\"\"\")\nstructure User {}\n",
                "structure",
                "keyword.declaration",
            ),
            (
                "KCL",
                "schema User:\n    name: str = \"hello\" # name\n",
                "schema",
                "keyword.control",
            ),
            (
                "KCL",
                "schema User:\n    name: str = \"hello\" # name\n",
                "str",
                "storage.type",
            ),
            (
                "KCL",
                "schema User:\n    name: str = \"hello\" # name\n",
                "hello",
                "string.quoted",
            ),
            (
                "KCL",
                "schema User:\n    name: str = \"hello\" # name\n",
                "# name",
                "comment.line",
            ),
            (
                "KCL",
                "name = \"hello ${ {key = 'world'}.key }\"\ncount = 42\n",
                "42",
                "constant.numeric",
            ),
            (
                "KCL",
                "name = \"\"\"\n# not a comment\n\"\"\"\nschema User:\n    name: str\n",
                "# not",
                "string.quoted.triple",
            ),
            (
                "KCL",
                "name = \"\"\"\n# not a comment\n\"\"\"\nschema User:\n    name: str\n",
                "schema",
                "keyword.control",
            ),
            (
                "Hurl",
                "GET https://example.invalid\nHTTP 200\n[Asserts]\njsonpath \"$.name\" == \"hello\" # assertion\n",
                "GET",
                "keyword.control.http-method",
            ),
            (
                "Hurl",
                "GET https://example.invalid\nHTTP 200\n[Asserts]\njsonpath \"$.name\" == \"hello\" # assertion\n",
                "HTTP",
                "keyword.control.http-status",
            ),
            (
                "Hurl",
                "GET https://example.invalid\nHTTP 200\n[Asserts]\njsonpath \"$.name\" == \"hello\" # assertion\n",
                "[Asserts]",
                "keyword.declaration.section",
            ),
            (
                "Hurl",
                "GET https://example.invalid\nHTTP 200\n[Asserts]\njsonpath \"$.name\" == \"hello\" # assertion\n",
                "jsonpath",
                "support.function",
            ),
            (
                "Hurl",
                "GET https://example.invalid\nHTTP 200\n[Asserts]\njsonpath \"$.name\" == \"hello\" # assertion\n",
                "hello",
                "string.quoted",
            ),
            (
                "Hurl",
                "GET https://example.invalid\nHTTP 200\n[Asserts]\njsonpath \"$.name\" == \"hello\" # assertion\n",
                "# assertion",
                "comment.line",
            ),
            (
                "Hurl",
                "GET {{base_url}}/users\nHTTP 200\n",
                "base_url",
                "variable.other.template",
            ),
            (
                "Hurl",
                "POST https://example.invalid\n```\n# body\n```\nHTTP 201\n",
                "# body",
                "string.quoted.triple",
            ),
            (
                "Hurl",
                "POST https://example.invalid\n```\n# body\n```\nHTTP 201\n",
                "HTTP",
                "keyword.control.http-status",
            ),
            (
                "Procfile",
                "web: echo \"hello\" # command\nworker: queue\n",
                "web",
                "entity.name.function.process",
            ),
            (
                "Procfile",
                "web: echo \"hello\" # command\nworker: queue\n",
                "hello",
                "string.quoted",
            ),
            (
                "Procfile",
                "web: echo \"hello\" # command\nworker: queue\n",
                "# command",
                "comment.line",
            ),
            (
                "Procfile",
                "web: echo \"hello\" # command\nworker: queue\n",
                "worker",
                "entity.name.function.process",
            ),
            (
                "CODEOWNERS",
                "# owners\n/src/ @example/team\n*.md docs@example.invalid\n",
                "# owners",
                "comment.line",
            ),
            (
                "CODEOWNERS",
                "# owners\n/src/ @example/team\n*.md docs@example.invalid\n",
                "/src/",
                "string.unquoted.path",
            ),
            (
                "CODEOWNERS",
                "# owners\n/src/ @example/team\n*.md docs@example.invalid\n",
                "@example/team",
                "variable.other.owner",
            ),
            (
                "CODEOWNERS",
                "# owners\n/src/ @example/team\n*.md docs@example.invalid\n",
                "docs@example.invalid",
                "variable.other.owner",
            ),
            (
                "CODEOWNERS",
                "/a\\ b/ @example/team\n",
                "/a\\ b/",
                "string.unquoted.path",
            ),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }

    #[test]
    fn engineering_and_automation_languages_have_real_token_scopes() {
        for (language, source, token, scope) in [
            (
                "ABAP",
                "data name type string value 'hello'. \" data\n",
                "data",
                "keyword.control",
            ),
            (
                "ABAP",
                "data name type string value 'hello'. \" data\n",
                "string",
                "storage.type",
            ),
            (
                "ABAP",
                "data name type string value 'hello'. \" data\n",
                "hello",
                "string.quoted",
            ),
            (
                "ABAP",
                "data name type string value 'hello'. \" data\n",
                "\" data",
                "comment.line",
            ),
            (
                "ABAP",
                "* DATA is a comment\nDATA count TYPE i VALUE 42.\n",
                "* DATA",
                "comment.line",
            ),
            (
                "ABAP",
                "DATA name TYPE string VALUE 'it''s text'.\nWRITE name.\n",
                "s text",
                "string.quoted",
            ),
            (
                "ABAP",
                "DATA name TYPE string VALUE 'it''s text'.\nWRITE name.\n",
                "WRITE",
                "keyword.control",
            ),
            (
                "AutoHotkey",
                "if A_IsAdmin {\nMsgBox \"hello\" ; MsgBox\n}\n",
                "if",
                "keyword.control",
            ),
            (
                "AutoHotkey",
                "if A_IsAdmin {\nMsgBox \"hello\" ; MsgBox\n}\n",
                "A_IsAdmin",
                "variable.language",
            ),
            (
                "AutoHotkey",
                "if A_IsAdmin {\nMsgBox \"hello\" ; MsgBox\n}\n",
                "MsgBox",
                "support.function",
            ),
            (
                "AutoHotkey",
                "if A_IsAdmin {\nMsgBox \"hello\" ; MsgBox\n}\n",
                "hello",
                "string.quoted",
            ),
            (
                "AutoHotkey",
                "if A_IsAdmin {\nMsgBox \"hello\" ; MsgBox\n}\n",
                "; MsgBox",
                "comment.line",
            ),
            (
                "AutoHotkey",
                "^j::MsgBox \"hello\"\n",
                "^j::",
                "entity.name.function.hotkey",
            ),
            (
                "AutoHotkey",
                "/* comment\nMsgBox\n*/\nSleep 42\n",
                "MsgBox",
                "comment.block",
            ),
            (
                "AutoHotkey",
                "/* comment\nMsgBox\n*/\nSleep 42\n",
                "Sleep",
                "support.function",
            ),
            (
                "OpenSCAD",
                "module box() { cube([1, 2, 3]); echo(\"hello\"); } // cube\n",
                "module",
                "keyword.control",
            ),
            (
                "OpenSCAD",
                "module box() { cube([1, 2, 3]); echo(\"hello\"); } // cube\n",
                "cube",
                "support.function",
            ),
            (
                "OpenSCAD",
                "module box() { cube([1, 2, 3]); echo(\"hello\"); } // cube\n",
                "hello",
                "string.quoted",
            ),
            (
                "OpenSCAD",
                "module box() { cube([1, 2, 3]); echo(\"hello\"); } // cube\n",
                "// cube",
                "comment.line",
            ),
            (
                "OpenSCAD",
                "use <library.scad>\n$fn = 42;\n",
                "library.scad",
                "string.unquoted.import",
            ),
            (
                "OpenSCAD",
                "use <library.scad>\n$fn = 42;\n",
                "$fn",
                "variable.language",
            ),
            (
                "Stan",
                "parameters { real mu; }\nmodel { mu ~ normal(0, 1); print(\"hello\"); } // model\n",
                "parameters",
                "keyword.control",
            ),
            (
                "Stan",
                "parameters { real mu; }\nmodel { mu ~ normal(0, 1); print(\"hello\"); } // model\n",
                "real",
                "storage.type",
            ),
            (
                "Stan",
                "parameters { real mu; }\nmodel { mu ~ normal(0, 1); print(\"hello\"); } // model\n",
                "normal",
                "support.function",
            ),
            (
                "Stan",
                "parameters { real mu; }\nmodel { mu ~ normal(0, 1); print(\"hello\"); } // model\n",
                "hello",
                "string.quoted",
            ),
            (
                "Stan",
                "parameters { real mu; }\nmodel { mu ~ normal(0, 1); print(\"hello\"); } // model\n",
                "// model",
                "comment.line",
            ),
            (
                "Stan",
                "target += normal_lpdf(mu | 0, 1);\n",
                "normal_lpdf",
                "support.function.distribution",
            ),
            (
                "TypeSpec",
                "@doc(\"hello\") model User { name: string; } // model\n",
                "@doc",
                "entity.name.function.decorator",
            ),
            (
                "TypeSpec",
                "@doc(\"hello\") model User { name: string; } // model\n",
                "model",
                "keyword.declaration",
            ),
            (
                "TypeSpec",
                "@doc(\"hello\") model User { name: string; } // model\n",
                "string",
                "storage.type",
            ),
            (
                "TypeSpec",
                "@doc(\"hello\") model User { name: string; } // model\n",
                "hello",
                "string.quoted",
            ),
            (
                "TypeSpec",
                "@doc(\"hello\") model User { name: string; } // model\n",
                "// model",
                "comment.line",
            ),
            (
                "TypeSpec",
                "@doc(\"\"\"\n// text\n\"\"\")\nmodel User {}\n",
                "// text",
                "string.quoted.triple",
            ),
            (
                "TypeSpec",
                "@doc(\"\"\"\n// text\n\"\"\")\nmodel User {}\n",
                "model",
                "keyword.declaration",
            ),
            (
                "G-code",
                "G1X10.5Y20F1200 ; move\n",
                "G1",
                "keyword.control",
            ),
            (
                "G-code",
                "G1X10.5Y20F1200 ; move\n",
                "X",
                "variable.parameter",
            ),
            (
                "G-code",
                "G1X10.5Y20F1200 ; move\n",
                "10.5",
                "constant.numeric",
            ),
            (
                "G-code",
                "G1X10.5Y20F1200 ; move\n",
                "; move",
                "comment.line",
            ),
            (
                "G-code",
                "(G1 is a comment)\nM117 \"hello\"\n",
                "G1",
                "comment.block",
            ),
            (
                "G-code",
                "(G1 is a comment)\nM117 \"hello\"\n",
                "hello",
                "string.quoted",
            ),
            (
                "G-code",
                "#<depth> = -1.5\nG1 Z#<depth>\n",
                "#<depth>",
                "variable.other",
            ),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }

    #[test]
    fn shader_and_modern_language_scopes_are_not_plain_text_aliases() {
        for (language, source, token, scope) in [
            (
                "CUDA",
                "__global__ void add() { int x = threadIdx.x; }\n",
                "__global__",
                "storage.modifier",
            ),
            (
                "CUDA",
                "__global__ void add() { int x = threadIdx.x; }\n",
                "threadIdx",
                "variable.language",
            ),
            (
                "CUDA",
                "// __global__\nconst char *name = \"hello\";\n",
                "// __global__",
                "comment.line",
            ),
            (
                "CUDA",
                "// __global__\nconst char *name = \"hello\";\n",
                "hello",
                "string.quoted",
            ),
            (
                "HLSL",
                "float4 main() : SV_Target { return float4(1,0,0,1); }\n",
                "float4",
                "storage.type",
            ),
            (
                "HLSL",
                "float4 main() : SV_Target { return float4(1,0,0,1); }\n",
                "SV_Target",
                "variable.language",
            ),
            (
                "HLSL",
                "// float4\nconst char *name = \"hello\";\n",
                "// float4",
                "comment.line",
            ),
            (
                "HLSL",
                "// float4\nconst char *name = \"hello\";\n",
                "hello",
                "string.quoted",
            ),
            (
                "Metal",
                "kernel void add(device float *x [[buffer(0)]]) {}\n",
                "kernel",
                "storage.modifier",
            ),
            (
                "Metal",
                "kernel void add(device float *x [[buffer(0)]]) {}\n",
                "buffer",
                "entity.other.attribute-name",
            ),
            (
                "Metal",
                "// kernel\nconstant char *name = \"hello\";\n",
                "// kernel",
                "comment.line",
            ),
            (
                "Metal",
                "// kernel\nconstant char *name = \"hello\";\n",
                "hello",
                "string.quoted",
            ),
            (
                "GDShader",
                "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(1.0); }\n",
                "shader_type",
                "keyword.declaration",
            ),
            (
                "GDShader",
                "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(1.0); }\n",
                "ALBEDO",
                "variable.language",
            ),
            (
                "GDShader",
                "// ALBEDO\nuniform sampler2D tex;\n",
                "// ALBEDO",
                "comment.line",
            ),
            (
                "GDShader",
                "#include \"common.gdshaderinc\"\n",
                "common",
                "string.quoted",
            ),
            (
                "Luau",
                "export type Name = string\nlocal name = \"hello\" -- type\n",
                "export",
                "keyword.declaration",
            ),
            (
                "Luau",
                "export type Name = string\nlocal name = \"hello\" -- type\n",
                "string",
                "storage.type",
            ),
            (
                "Luau",
                "export type Name = string\nlocal name = \"hello\" -- type\n",
                "hello",
                "string.quoted",
            ),
            (
                "Luau",
                "export type Name = string\nlocal name = \"hello\" -- type\n",
                "-- type",
                "comment.line",
            ),
            (
                "Mojo",
                "fn greet(name: String):\n    print(\"hello\") # fn\n",
                "fn",
                "keyword.declaration",
            ),
            (
                "Mojo",
                "fn greet(name: String):\n    print(\"hello\") # fn\n",
                "String",
                "storage.type",
            ),
            (
                "Mojo",
                "fn greet(name: String):\n    print(\"hello\") # fn\n",
                "hello",
                "string.quoted",
            ),
            (
                "Mojo",
                "fn greet(name: String):\n    print(\"hello\") # fn\n",
                "# fn",
                "comment.line",
            ),
            (
                "Gherkin",
                "Feature: Greeting\n  Given a user named \"hello\"\n",
                "Feature",
                "keyword.declaration",
            ),
            (
                "Gherkin",
                "Feature: Greeting\n  Given a user named \"hello\"\n",
                "Given",
                "keyword.control",
            ),
            (
                "Gherkin",
                "Feature: Greeting\n  Given a user named \"hello\"\n",
                "hello",
                "string.quoted",
            ),
            (
                "Gherkin",
                "# Feature\nFeature: Greeting\n",
                "# Feature",
                "comment.line",
            ),
            (
                "YARA",
                "rule Example { strings: $text = \"hello\" condition: $text }\n",
                "rule",
                "keyword.declaration",
            ),
            (
                "YARA",
                "rule Example { strings: $text = \"hello\" condition: $text }\n",
                "$text",
                "variable.other",
            ),
            (
                "YARA",
                "rule Example { strings: $text = \"hello\" condition: $text }\n",
                "hello",
                "string.quoted",
            ),
            (
                "YARA",
                "// rule\nrule Example { condition: true }\n",
                "// rule",
                "comment.line",
            ),
            (
                "Pkl",
                "name: String = \"hello\" // name\n",
                "String",
                "storage.type",
            ),
            (
                "Pkl",
                "name: String = \"hello\" // name\n",
                "hello",
                "string.quoted",
            ),
            (
                "Pkl",
                "name: String = \"hello\" // name\n",
                "// name",
                "comment.line",
            ),
            (
                "Pkl",
                "when (true) { count = 42 }\n",
                "when",
                "keyword.control",
            ),
            (
                "Dhall",
                "let name : Text = \"hello\" in name\n",
                "let",
                "keyword.control",
            ),
            (
                "Dhall",
                "let name : Text = \"hello\" in name\n",
                "Text",
                "storage.type",
            ),
            (
                "Dhall",
                "let name : Text = \"hello\" in name\n",
                "hello",
                "string.quoted",
            ),
            (
                "Dhall",
                "-- let\nNatural/show 42\n",
                "-- let",
                "comment.line",
            ),
            (
                "Dhall",
                "-- let\nNatural/show 42\n",
                "Natural/show",
                "support.function",
            ),
            (
                "Nickel",
                "let name | String = \"hello\" in { name } # name\n",
                "let",
                "keyword.control",
            ),
            (
                "Nickel",
                "let name | String = \"hello\" in { name } # name\n",
                "String",
                "storage.type",
            ),
            (
                "Nickel",
                "let name | String = \"hello\" in { name } # name\n",
                "hello",
                "string.quoted",
            ),
            (
                "Nickel",
                "let name | String = \"hello\" in { name } # name\n",
                "# name",
                "comment.line",
            ),
            (
                "ReScript",
                "let name: string = \"hello\" // let\n",
                "let",
                "keyword.control",
            ),
            (
                "ReScript",
                "let name: string = \"hello\" // let\n",
                "string",
                "storage.type",
            ),
            (
                "ReScript",
                "let name: string = \"hello\" // let\n",
                "hello",
                "string.quoted",
            ),
            (
                "ReScript",
                "let name: string = \"hello\" // let\n",
                "// let",
                "comment.line",
            ),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }

    #[test]
    fn modern_language_multiline_states_close_correctly() {
        for (language, source, token, scope) in [
            (
                "CUDA",
                "/* __global__\nthreadIdx */\n__global__ void add() {}\n",
                "threadIdx",
                "comment.block",
            ),
            (
                "CUDA",
                "/* __global__\nthreadIdx */\n__global__ void add() {}\n",
                "__global__ void",
                "storage.modifier",
            ),
            (
                "Luau",
                "--[[type\nstring]]\nexport type Name = string\n",
                "string]]",
                "comment.block",
            ),
            (
                "Luau",
                "--[[type\nstring]]\nexport type Name = string\n",
                "export",
                "keyword.declaration",
            ),
            (
                "Luau",
                "local text = `hello {string.upper(\"world\")}`\nlocal count = 42\n",
                "42",
                "constant.numeric",
            ),
            (
                "Mojo",
                "var name = \"\"\"\n# not a comment\n\"\"\"\nfn main():\n    pass\n",
                "# not",
                "string.quoted",
            ),
            (
                "Mojo",
                "var name = \"\"\"\n# not a comment\n\"\"\"\nfn main():\n    pass\n",
                "fn",
                "keyword.declaration",
            ),
            (
                "Gherkin",
                "Feature: Greeting\nScenario: hello\nGiven text\n\"\"\"\n# content\n\"\"\"\nThen done\n",
                "# content",
                "string.quoted",
            ),
            (
                "Gherkin",
                "Feature: Greeting\nScenario: hello\nGiven text\n\"\"\"\n# content\n\"\"\"\nThen done\n",
                "Then",
                "keyword.control",
            ),
            (
                "Pkl",
                "/* outer /* nested */ still */\nname: String = \"hello\"\n",
                "still",
                "comment.block",
            ),
            (
                "Pkl",
                "/* outer /* nested */ still */\nname: String = \"hello\"\n",
                "String",
                "storage.type",
            ),
            (
                "Dhall",
                "{- outer {- nested -} still -}\nlet count = 42 in count\n",
                "still",
                "comment.block",
            ),
            (
                "Dhall",
                "{- outer {- nested -} still -}\nlet count = 42 in count\n",
                "let",
                "keyword.control",
            ),
            (
                "ReScript",
                "/* outer /* nested */ still */\nlet count = 42\n",
                "still",
                "comment.block",
            ),
            (
                "ReScript",
                "/* outer /* nested */ still */\nlet count = 42\n",
                "let",
                "keyword.control",
            ),
            (
                "Nickel",
                "let name = \"hello %{ {value = \"world\"}.value }\" in 42\n",
                "42",
                "constant.numeric",
            ),
            (
                "Pkl",
                "name = \"hello \\(f(g(h(42))))\"\ncount = 7\n",
                "7",
                "constant.numeric",
            ),
            (
                "Luau",
                "--[==[ outer ]] still ]==]\nexport type Name = string\n",
                "still",
                "comment.block",
            ),
            (
                "Luau",
                "--[==[ outer ]] still ]==]\nexport type Name = string\n",
                "export",
                "keyword.declaration",
            ),
            (
                "CUDA",
                "const char *name = R\"tag(__global__ \"still string\")tag\";\n__global__ void add() {}\n",
                "__global__ \"still",
                "string.quoted.double.raw",
            ),
            (
                "CUDA",
                "const char *name = R\"tag(__global__ \"still string\")tag\";\n__global__ void add() {}\n",
                "__global__ void",
                "storage.modifier",
            ),
            (
                "Gherkin",
                "# language: zh-CN\n功能: 问候\n  场景: 用户\n    假如 一个用户\n",
                "功能",
                "keyword.declaration",
            ),
            (
                "Gherkin",
                "# language: zh-CN\n功能: 问候\n  场景: 用户\n    假如 一个用户\n",
                "假如",
                "keyword.control",
            ),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }

    #[test]
    fn additional_language_tokens_have_semantic_scopes() {
        for (language, source, token, scope) in [
            (
                "Bicep",
                "param name string = 'world' // param\n",
                "param",
                "keyword.control",
            ),
            ("Bicep", "var name = 'world'\n", "world", "string.quoted"),
            (
                "Bicep",
                "/* param */\nparam count int = 42\n",
                "param */",
                "comment.block",
            ),
            (
                "Bicep",
                "/* param */\nparam count int = 42\n",
                "42",
                "constant.numeric",
            ),
            (
                "Meson",
                "project('hello', 'c') # project\n",
                "project",
                "support.function",
            ),
            (
                "Meson",
                "project('hello', 'c') # project\n",
                "hello",
                "string.quoted",
            ),
            ("Meson", "# if\nif true\nendif\n", "if\n", "comment.line"),
            (
                "Meson",
                "# if\nif true\nendif\n",
                "endif",
                "keyword.control",
            ),
            (
                "WebAssembly",
                "(func $add (param i32) i32.const 42)\n",
                "i32.const",
                "support.function",
            ),
            (
                "WebAssembly",
                "(func $add (param i32) i32.const 42)\n",
                "$add",
                "variable.other",
            ),
            (
                "WebAssembly",
                "(func (param i32) local.get 0)\n",
                "local.get",
                "support.function",
            ),
            (
                "WebAssembly",
                "(data \"hello\") ;; func\n",
                "hello",
                "string.quoted",
            ),
            (
                "WebAssembly",
                "(data \"hello\") ;; func\n",
                ";; func",
                "comment.line",
            ),
            (
                "WebIDL",
                "interface Example { attribute DOMString name; };\n",
                "interface",
                "keyword.declaration",
            ),
            (
                "WebIDL",
                "interface Example { attribute DOMString name; };\n",
                "DOMString",
                "storage.type",
            ),
            (
                "WebIDL",
                "// interface\nconst long count = 42;\n",
                "// interface",
                "comment.line",
            ),
            (
                "WebIDL",
                "// interface\nconst long count = 42;\n",
                "42",
                "constant.numeric",
            ),
            (
                "Thrift",
                "struct User { 1: required string name = 'world' }\n",
                "struct",
                "keyword.declaration",
            ),
            (
                "Thrift",
                "struct User { 1: required string name = 'world' }\n",
                "string",
                "storage.type",
            ),
            (
                "Thrift",
                "struct User { 1: required string name = 'world' }\n",
                "world",
                "string.quoted",
            ),
            (
                "Thrift",
                "# struct\nstruct User {}\n",
                "# struct",
                "comment.line",
            ),
            (
                "FlatBuffers",
                "table User { name:string; }\n",
                "table",
                "keyword.declaration",
            ),
            (
                "FlatBuffers",
                "table User { name:string; }\n",
                "string",
                "storage.type",
            ),
            (
                "FlatBuffers",
                "file_identifier \"USER\"; // table\n",
                "USER",
                "string.quoted",
            ),
            (
                "FlatBuffers",
                "file_identifier \"USER\"; // table\n",
                "// table",
                "comment.line",
            ),
            (
                "Mermaid",
                "flowchart TD\n  a --> b\n",
                "flowchart",
                "keyword.declaration",
            ),
            (
                "Mermaid",
                "flowchart TD\n  a --> b\n",
                "-->",
                "keyword.operator",
            ),
            (
                "Mermaid",
                "a[\"hello\"] %% graph\n",
                "hello",
                "string.quoted",
            ),
            (
                "Mermaid",
                "a[\"hello\"] %% graph\n",
                "%% graph",
                "comment.line",
            ),
            (
                "PlantUML",
                "@startuml\nactor User\n@enduml\n",
                "@startuml",
                "keyword.declaration",
            ),
            (
                "PlantUML",
                "@startuml\nactor User\n@enduml\n",
                "actor",
                "keyword.control",
            ),
            (
                "PlantUML",
                "Alice -> Bob: \"hello\"\n",
                "hello",
                "string.quoted",
            ),
            (
                "PlantUML",
                "' actor\nactor User\n",
                "' actor",
                "comment.line",
            ),
            ("D2", "a -> b: \"hello\"\n", "->", "keyword.operator"),
            ("D2", "a -> b: \"hello\"\n", "hello", "string.quoted"),
            (
                "D2",
                "a: {shape: rectangle}\n",
                "shape",
                "support.type.property-name",
            ),
            ("D2", "# shape\nwidth: 42\n", "# shape", "comment.line"),
            (
                "Haxe",
                "class Main { var name:String = 'hello'; }\n",
                "class",
                "keyword.control",
            ),
            (
                "Haxe",
                "class Main { var name:String = 'hello'; }\n",
                "String",
                "storage.type",
            ),
            (
                "Haxe",
                "class Main { var name:String = 'hello'; }\n",
                "hello",
                "string.quoted",
            ),
            (
                "Haxe",
                "// class\nvar count = 42;\n",
                "// class",
                "comment.line",
            ),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }

    #[test]
    fn multiline_comments_and_interpolation_preserve_following_tokens() {
        for (language, source, token, scope) in [
            (
                "WebAssembly",
                "(; outer\n(; nested ;)\nstill comment ;)\n(func)\n",
                "still comment",
                "comment.block",
            ),
            (
                "WebAssembly",
                "(; outer\n(; nested ;)\nstill comment ;)\n(func)\n",
                "func",
                "keyword.declaration",
            ),
            (
                "Bicep",
                "var message = 'hello ${toUpper('world')}'\nparam count int = 42\n",
                "toUpper",
                "variable.function",
            ),
            (
                "Bicep",
                "var message = 'hello ${toUpper('world')}'\nparam count int = 42\n",
                "42",
                "constant.numeric",
            ),
            (
                "Bicep",
                "var raw = '''\n// not a comment\n'''\nparam count int = 42\n",
                "// not",
                "string.quoted.triple",
            ),
            (
                "Bicep",
                "var raw = '''\n// not a comment\n'''\nparam count int = 42\n",
                "42",
                "constant.numeric",
            ),
            (
                "Meson",
                "text = '''\n# not a comment\n'''\ncount = 42\n",
                "# not",
                "string.quoted.triple",
            ),
            (
                "Meson",
                "text = '''\n# not a comment\n'''\ncount = 42\n",
                "42",
                "constant.numeric",
            ),
            (
                "Haxe",
                "var text = '${{x: \"}\"}.x}'; var count = 42;\n",
                "42",
                "constant.numeric",
            ),
            (
                "PlantUML",
                "/' multi\nactor User\n'/\nactor Other\n",
                "actor User",
                "comment.block",
            ),
            (
                "PlantUML",
                "/' multi\nactor User\n'/\nactor Other\n",
                "actor Other",
                "keyword.control",
            ),
            (
                "D2",
                "a: |md\n# heading\n|\na -> b\n",
                "# heading",
                "string.unquoted.block",
            ),
            (
                "D2",
                "a: |md\n# heading\n|\na -> b\n",
                "->",
                "keyword.operator",
            ),
        ] {
            assert_token_scope(language, source, token, scope);
        }
    }
}
