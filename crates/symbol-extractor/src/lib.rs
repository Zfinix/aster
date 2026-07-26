#![forbid(unsafe_code)]

use std::sync::LazyLock;

use aster_models::{Language, Symbol};
use tree_sitter_tags::{TagsConfiguration, TagsContext};

const SNIPPET_MAX_CHARS: usize = 200;

pub fn extract_symbols(content: &str, file_path: &str) -> Vec<Symbol> {
    let Some(config) = config_for_path(file_path) else {
        return Vec::new();
    };
    let source = content.as_bytes();
    let mut context = TagsContext::new();
    let Ok((tags, _)) = context.generate_tags(config, source, None) else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    for tag in tags.flatten() {
        if !tag.is_definition {
            continue;
        }
        let name = String::from_utf8_lossy(&source[tag.name_range]).into_owned();
        if name.is_empty() {
            continue;
        }
        symbols.push(Symbol {
            file_path: file_path.to_string(),
            symbol_name: Some(name),
            symbol_kind: Some(config.syntax_type_name(tag.syntax_type_id).to_string()),
            start_line: Some(tag.span.start.row as i32 + 1),
            end_line: Some(tag.span.end.row as i32 + 1),
            code_snippet: Some(snippet(&source[tag.range])),
        });
    }
    symbols
}

fn snippet(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .take(SNIPPET_MAX_CHARS)
        .collect()
}

fn config_for_path(file_path: &str) -> Option<&'static TagsConfiguration> {
    match Language::from_path(std::path::Path::new(file_path))? {
        Language::Rust => RUST.as_ref(),
        Language::Python => PYTHON.as_ref(),
        Language::JavaScript => JAVASCRIPT.as_ref(),
        Language::TypeScript => TYPESCRIPT.as_ref(),
        Language::Tsx => TSX.as_ref(),
        Language::Go => GO.as_ref(),
        Language::Java => JAVA.as_ref(),
        Language::C => C.as_ref(),
        Language::Cpp => CPP.as_ref(),
        Language::Ruby => RUBY.as_ref(),
        Language::CSharp => C_SHARP.as_ref(),
        Language::Php => PHP.as_ref(),
        Language::Swift => SWIFT.as_ref(),
        Language::Kotlin => KOTLIN.as_ref(),
        Language::Scala => SCALA.as_ref(),
    }
}

macro_rules! tags_config {
    ($name:ident, $language:expr, $query:expr) => {
        static $name: LazyLock<Option<TagsConfiguration>> =
            LazyLock::new(|| TagsConfiguration::new($language.into(), $query, "").ok());
    };
}

tags_config!(
    RUST,
    tree_sitter_rust::LANGUAGE,
    tree_sitter_rust::TAGS_QUERY
);
tags_config!(
    PYTHON,
    tree_sitter_python::LANGUAGE,
    tree_sitter_python::TAGS_QUERY
);
tags_config!(
    JAVASCRIPT,
    tree_sitter_javascript::LANGUAGE,
    tree_sitter_javascript::TAGS_QUERY
);
tags_config!(
    TYPESCRIPT,
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
    tree_sitter_typescript::TAGS_QUERY
);
tags_config!(
    TSX,
    tree_sitter_typescript::LANGUAGE_TSX,
    tree_sitter_typescript::TAGS_QUERY
);
tags_config!(GO, tree_sitter_go::LANGUAGE, tree_sitter_go::TAGS_QUERY);
tags_config!(
    JAVA,
    tree_sitter_java::LANGUAGE,
    tree_sitter_java::TAGS_QUERY
);
tags_config!(C, tree_sitter_c::LANGUAGE, tree_sitter_c::TAGS_QUERY);
tags_config!(CPP, tree_sitter_cpp::LANGUAGE, tree_sitter_cpp::TAGS_QUERY);
tags_config!(
    RUBY,
    tree_sitter_ruby::LANGUAGE,
    tree_sitter_ruby::TAGS_QUERY
);
tags_config!(
    C_SHARP,
    tree_sitter_c_sharp::LANGUAGE,
    tree_sitter_c_sharp::TAGS_QUERY
);
tags_config!(
    PHP,
    tree_sitter_php::LANGUAGE_PHP,
    tree_sitter_php::TAGS_QUERY
);
tags_config!(
    SWIFT,
    tree_sitter_swift::LANGUAGE,
    tree_sitter_swift::TAGS_QUERY
);

// tree-sitter-kotlin-ng and tree-sitter-scala ship no tags query, so these are
// authored against each grammar's node types.
tags_config!(KOTLIN, tree_sitter_kotlin_ng::LANGUAGE, KOTLIN_TAGS_QUERY);
tags_config!(SCALA, tree_sitter_scala::LANGUAGE, SCALA_TAGS_QUERY);

const KOTLIN_TAGS_QUERY: &str = r#"
(class_declaration name: (identifier) @name) @definition.class
(object_declaration name: (identifier) @name) @definition.object
(companion_object name: (identifier) @name) @definition.object
(function_declaration name: (identifier) @name) @definition.function
(property_declaration (variable_declaration (identifier) @name)) @definition.property
(enum_entry (identifier) @name) @definition.constant
(type_alias type: (identifier) @name) @definition.type
"#;

const SCALA_TAGS_QUERY: &str = r#"
(class_definition name: (identifier) @name) @definition.class
(object_definition name: (identifier) @name) @definition.object
(trait_definition name: (identifier) @name) @definition.interface
(enum_definition name: (identifier) @name) @definition.enum
(function_definition name: (identifier) @name) @definition.function
(val_definition pattern: (identifier) @name) @definition.variable
(var_definition pattern: (identifier) @name) @definition.variable
(val_declaration name: (identifier) @name) @definition.variable
(var_declaration name: (identifier) @name) @definition.variable
(type_definition name: (type_identifier) @name) @definition.type
(given_definition name: (identifier) @name) @definition.variable
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn names(content: &str, path: &str) -> Vec<String> {
        extract_symbols(content, path)
            .into_iter()
            .filter_map(|s| s.symbol_name)
            .collect()
    }

    #[test]
    fn extracts_kotlin_definitions() {
        let src = r#"
class Greeter(val name: String) {
    fun greet(): String = "hi"
    val count = 0
}
object Registry
enum class Color { RED, GREEN }
typealias Handler = (Int) -> Unit
"#;
        let got = names(src, "a.kt");
        for want in ["Greeter", "greet", "count", "Registry", "Color", "Handler"] {
            assert!(got.contains(&want.to_string()), "missing {want} in {got:?}");
        }
    }

    #[test]
    fn extracts_scala_definitions() {
        let src = r#"
class Greeter(name: String) {
  def greet: String = "hi"
  val count = 0
}
object Registry
trait Named { def name: String }
type Handler = Int => Unit
"#;
        let got = names(src, "a.scala");
        for want in ["Greeter", "greet", "count", "Registry", "Named", "Handler"] {
            assert!(got.contains(&want.to_string()), "missing {want} in {got:?}");
        }
    }
}
