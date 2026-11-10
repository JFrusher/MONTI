//! Every supported language, in one table. To add a language:
//!
//! 1. `cargo add tree-sitter-<lang>` in `crates/monti-core`.
//! 2. Add a [`Spec`] to [`SPECS`]. Leave a field empty (`&[]` / `None`) when you are unsure:
//!    the detector that needs it is then simply off for that language.
//! 3. Add `tests/fixtures/<name>/` with `slop.<ext>`, `slop.expect` and `clean.<ext>`
//!    (see CONTRIBUTING.md). The test suite fails for a language without fixtures.
//! 4. Optionally add query rules for it to `rules/default.toml`.
//!
//! Node kind names come from the grammar: print a tree with `monti tree <file>`.

use tree_sitter::Node;

pub struct Spec {
    /// Name used in `languages = [...]` of rule files.
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub grammar: fn() -> tree_sitter::Language,
    /// Function-like node kinds (named functions, methods, lambdas).
    pub functions: &'static [&'static str],
    /// Function kinds that are always methods. Methods may delegate to satisfy an interface,
    /// so pass-through-wrapper skips them.
    pub methods: &'static [&'static str],
    /// A function whose parent or grandparent has one of these kinds is a method.
    pub classes: &'static [&'static str],
    /// Call kinds with an `arguments` field. Empty turns pass-through-wrapper off.
    pub calls: &'static [&'static str],
    /// Enables redundant-null-check.
    pub null_check: Option<NullCheck>,
    /// Enables ghost-abstraction.
    pub ghost: Option<Ghost>,
    /// Enables undefined-name (for dynamic languages, where calling an unknown name is a runtime error).
    pub undefined: Option<Undefined>,
    /// Formatter command `monti fix` runs on changed files (the path is appended). Empty: none.
    pub formatter: &'static [&'static str],
}

pub struct NullCheck {
    /// Spellings of null, e.g. `null`, `None`, `nil`.
    pub nulls: &'static [&'static str],
    /// "Is not null" operators, whitespace removed, e.g. `!=`, `isnot`.
    pub ops: &'static [&'static str],
    /// True when an assigned value can never be null.
    pub non_null: fn(Node, &str) -> bool,
}

pub struct Undefined {
    /// Names callable without a definition or import.
    pub builtins: &'static [&'static str],
    /// Node kind of an import that can bring in any name (`from x import *`); files with one are skipped.
    pub wildcard_import: &'static str,
}

pub struct Ghost {
    /// True for interfaces, abstract classes, traits, protocols.
    pub is_abstraction: fn(Node, &str) -> bool,
    /// The nodes listing what a type implements or extends (for class/impl nodes; empty otherwise).
    pub heritage: fn(Node) -> Vec<Node>,
}

fn kind_in(kinds: &'static [&'static str]) -> impl Fn(Node) -> bool {
    move |n| kinds.contains(&n.kind())
}

fn children_of_kind<'t>(n: Node<'t>, kinds: &[&str]) -> Vec<Node<'t>> {
    let mut cursor = n.walk();
    n.children(&mut cursor).filter(|c| kinds.contains(&c.kind())).collect()
}

fn fields<'t>(n: Node<'t>, names: &[&str]) -> Vec<Node<'t>> {
    names.iter().filter_map(|f| n.child_by_field_name(f)).collect()
}

fn has_child_text(n: Node, src: &str, kinds: &[&str], text: &str) -> bool {
    children_of_kind(n, kinds).iter().any(|c| src[c.byte_range()].contains(text))
}

const JS_FUNCTIONS: &[&str] =
    &["function_declaration", "function_expression", "arrow_function", "method_definition", "generator_function_declaration"];

fn js_non_null(n: Node, _: &str) -> bool {
    kind_in(&[
        "new_expression", "object", "array", "string", "template_string", "number", "true", "false",
        "arrow_function", "function_expression",
    ])(n)
}

const JS_NULL: NullCheck = NullCheck { nulls: &["null", "undefined"], ops: &["!=", "!=="], non_null: js_non_null };

const TS_GHOST: Ghost = Ghost {
    is_abstraction: |n, _| kind_in(&["interface_declaration", "abstract_class_declaration"])(n),
    heritage: |n| match n.kind() {
        "class_declaration" | "abstract_class_declaration" => children_of_kind(n, &["class_heritage"]),
        _ => Vec::new(),
    },
};

const JS_FORMAT: &[&str] = &["prettier", "--write"];

/// `dir(builtins)` of Python 3.13, public names plus `__import__` and `__build_class__`.
const PYTHON_BUILTINS: &[&str] = &[
    "ArithmeticError", "AssertionError", "AttributeError", "BaseException", "BaseExceptionGroup", "BlockingIOError", "BrokenPipeError", "BufferError",
    "BytesWarning", "ChildProcessError", "ConnectionAbortedError", "ConnectionError", "ConnectionRefusedError", "ConnectionResetError", "DeprecationWarning", "EOFError",
    "Ellipsis", "EncodingWarning", "EnvironmentError", "Exception", "ExceptionGroup", "False", "FileExistsError", "FileNotFoundError",
    "FloatingPointError", "FutureWarning", "GeneratorExit", "IOError", "ImportError", "ImportWarning", "IndentationError", "IndexError",
    "InterruptedError", "IsADirectoryError", "KeyError", "KeyboardInterrupt", "LookupError", "MemoryError", "ModuleNotFoundError", "NameError",
    "None", "NotADirectoryError", "NotImplemented", "NotImplementedError", "OSError", "OverflowError", "PendingDeprecationWarning", "PermissionError",
    "ProcessLookupError", "PythonFinalizationError", "RecursionError", "ReferenceError", "ResourceWarning", "RuntimeError", "RuntimeWarning", "StopAsyncIteration",
    "StopIteration", "SyntaxError", "SyntaxWarning", "SystemError", "SystemExit", "TabError", "TimeoutError", "True",
    "TypeError", "UnboundLocalError", "UnicodeDecodeError", "UnicodeEncodeError", "UnicodeError", "UnicodeTranslateError", "UnicodeWarning", "UserWarning",
    "ValueError", "Warning", "ZeroDivisionError", "__build_class__", "__import__", "abs", "aiter", "all",
    "anext", "any", "ascii", "bin", "bool", "breakpoint", "bytearray", "bytes",
    "callable", "chr", "classmethod", "compile", "complex", "copyright", "credits", "delattr",
    "dict", "dir", "divmod", "enumerate", "eval", "exec", "exit", "filter",
    "float", "format", "frozenset", "getattr", "globals", "hasattr", "hash", "help",
    "hex", "id", "input", "int", "isinstance", "issubclass", "iter", "len",
    "license", "list", "locals", "map", "max", "memoryview", "min", "next",
    "object", "oct", "open", "ord", "pow", "print", "property", "quit",
    "range", "repr", "reversed", "round", "set", "setattr", "slice", "sorted",
    "staticmethod", "str", "sum", "super", "tuple", "type", "vars", "zip",
];

pub static SPECS: &[Spec] = &[
    Spec {
        name: "typescript",
        extensions: &["ts", "mts", "cts"],
        grammar: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        functions: JS_FUNCTIONS,
        methods: &["method_definition"],
        classes: &[],
        calls: &["call_expression"],
        null_check: Some(JS_NULL),
        ghost: Some(TS_GHOST),
        undefined: None,
        formatter: JS_FORMAT,
    },
    Spec {
        name: "tsx",
        extensions: &["tsx"],
        grammar: || tree_sitter_typescript::LANGUAGE_TSX.into(),
        functions: JS_FUNCTIONS,
        methods: &["method_definition"],
        classes: &[],
        calls: &["call_expression"],
        null_check: Some(JS_NULL),
        ghost: Some(TS_GHOST),
        undefined: None,
        formatter: JS_FORMAT,
    },
    Spec {
        name: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        grammar: || tree_sitter_javascript::LANGUAGE.into(),
        functions: JS_FUNCTIONS,
        methods: &["method_definition"],
        classes: &[],
        calls: &["call_expression"],
        null_check: Some(JS_NULL),
        ghost: None,
        undefined: None,
        formatter: JS_FORMAT,
    },
    Spec {
        name: "python",
        extensions: &["py"],
        grammar: || tree_sitter_python::LANGUAGE.into(),
        functions: &["function_definition"],
        methods: &[],
        classes: &["class_definition"],
        calls: &["call"],
        null_check: Some(NullCheck {
            nulls: &["None"],
            ops: &["isnot", "!="],
            non_null: |n, _| {
                kind_in(&[
                    "list", "dictionary", "set", "tuple", "string", "integer", "float", "true", "false",
                    "list_comprehension", "dictionary_comprehension", "set_comprehension", "lambda",
                ])(n)
            },
        }),
        ghost: Some(Ghost {
            is_abstraction: |n, src| {
                n.kind() == "class_definition"
                    && n.child_by_field_name("superclasses").is_some_and(|s| {
                        src[s.byte_range()].split(|c: char| !c.is_alphanumeric()).any(|t| matches!(t, "ABC" | "ABCMeta"))
                    })
            },
            heritage: |n| if n.kind() == "class_definition" { fields(n, &["superclasses"]) } else { Vec::new() },
        }),
        undefined: Some(Undefined { builtins: PYTHON_BUILTINS, wildcard_import: "wildcard_import" }),
        formatter: &["ruff", "format"],
    },
    Spec {
        name: "rust",
        extensions: &["rs"],
        grammar: || tree_sitter_rust::LANGUAGE.into(),
        functions: &["function_item"],
        methods: &[],
        classes: &["impl_item", "trait_item"],
        calls: &["call_expression"],
        null_check: None,
        ghost: Some(Ghost {
            is_abstraction: |n, _| n.kind() == "trait_item",
            heritage: |n| if n.kind() == "impl_item" { fields(n, &["trait"]) } else { Vec::new() },
        }),
        undefined: None,
        formatter: &["rustfmt", "--edition", "2024"],
    },
    Spec {
        name: "go",
        extensions: &["go"],
        grammar: || tree_sitter_go::LANGUAGE.into(),
        functions: &["function_declaration", "method_declaration", "func_literal"],
        methods: &["method_declaration"],
        classes: &[],
        calls: &["call_expression"],
        null_check: Some(NullCheck {
            nulls: &["nil"],
            ops: &["!="],
            non_null: |n, src| match n.kind() {
                "composite_literal" | "func_literal" | "interpreted_string_literal" | "raw_string_literal"
                | "int_literal" | "float_literal" => true,
                "unary_expression" => n.child_by_field_name("operand").is_some_and(|o| o.kind() == "composite_literal"),
                "call_expression" => {
                    n.child_by_field_name("function").is_some_and(|f| matches!(&src[f.byte_range()], "new" | "make"))
                }
                _ => false,
            },
        }),
        // Go interfaces are satisfied implicitly, so implementors cannot be counted syntactically.
        ghost: None,
        undefined: None,
        formatter: &["gofmt", "-w"],
    },
    Spec {
        name: "c",
        // `.h` is read as C++: its grammar also parses plain C headers, the reverse fails.
        extensions: &["c"],
        grammar: || tree_sitter_c::LANGUAGE.into(),
        functions: &["function_definition"],
        methods: &[],
        classes: &[],
        calls: &["call_expression"],
        // malloc and friends can return NULL, so no C value is known non-null.
        null_check: None,
        ghost: None,
        undefined: None,
        formatter: &["clang-format", "-i"],
    },
    Spec {
        name: "cpp",
        extensions: &["cpp", "cc", "cxx", "h", "hpp", "hh", "hxx"],
        grammar: || tree_sitter_cpp::LANGUAGE.into(),
        functions: &["function_definition", "lambda_expression"],
        methods: &[],
        classes: &["class_specifier", "struct_specifier"],
        calls: &["call_expression"],
        null_check: Some(NullCheck {
            nulls: &["nullptr", "NULL"],
            ops: &["!="],
            non_null: |n, _| kind_in(&["new_expression", "string_literal", "lambda_expression"])(n),
        }),
        ghost: None,
        undefined: None,
        formatter: &["clang-format", "-i"],
    },
    Spec {
        name: "java",
        extensions: &["java"],
        grammar: || tree_sitter_java::LANGUAGE.into(),
        functions: &["method_declaration", "constructor_declaration", "lambda_expression"],
        methods: &["method_declaration", "constructor_declaration"],
        classes: &[],
        calls: &["method_invocation"],
        null_check: Some(NullCheck {
            nulls: &["null"],
            ops: &["!="],
            non_null: |n, _| {
                kind_in(&["object_creation_expression", "array_creation_expression", "string_literal", "lambda_expression"])(n)
            },
        }),
        ghost: Some(Ghost {
            is_abstraction: |n, src| {
                n.kind() == "interface_declaration"
                    || (n.kind() == "class_declaration" && has_child_text(n, src, &["modifiers"], "abstract"))
            },
            heritage: |n| if n.kind() == "class_declaration" { fields(n, &["interfaces", "superclass"]) } else { Vec::new() },
        }),
        undefined: None,
        formatter: &[],
    },
    Spec {
        name: "csharp",
        extensions: &["cs"],
        grammar: || tree_sitter_c_sharp::LANGUAGE.into(),
        functions: &["method_declaration", "local_function_statement", "lambda_expression"],
        methods: &["method_declaration"],
        classes: &[],
        calls: &["invocation_expression"],
        null_check: None,
        ghost: Some(Ghost {
            is_abstraction: |n, src| {
                n.kind() == "interface_declaration"
                    || (n.kind() == "class_declaration" && has_child_text(n, src, &["modifier"], "abstract"))
            },
            heritage: |n| if n.kind() == "class_declaration" { children_of_kind(n, &["base_list"]) } else { Vec::new() },
        }),
        undefined: None,
        formatter: &[],
    },
    Spec {
        name: "kotlin",
        extensions: &["kt", "kts"],
        grammar: || tree_sitter_kotlin_ng::LANGUAGE.into(),
        functions: &["function_declaration", "lambda_literal", "anonymous_function"],
        methods: &[],
        classes: &["class_declaration", "object_declaration"],
        calls: &[],
        null_check: None,
        ghost: None,
        undefined: None,
        formatter: &["ktlint", "-F"],
    },
    Spec {
        name: "swift",
        extensions: &["swift"],
        grammar: || tree_sitter_swift::LANGUAGE.into(),
        functions: &["function_declaration", "lambda_literal"],
        methods: &[],
        classes: &["class_declaration", "protocol_declaration"],
        calls: &[],
        null_check: None,
        ghost: Some(Ghost {
            is_abstraction: |n, _| n.kind() == "protocol_declaration",
            heritage: |n| {
                if n.kind() == "class_declaration" { children_of_kind(n, &["inheritance_specifier"]) } else { Vec::new() }
            },
        }),
        undefined: None,
        formatter: &[],
    },
    Spec {
        name: "scala",
        extensions: &["scala", "sc"],
        grammar: || tree_sitter_scala::LANGUAGE.into(),
        functions: &["function_definition", "lambda_expression"],
        methods: &[],
        classes: &["class_definition", "object_definition", "trait_definition"],
        calls: &["call_expression"],
        null_check: None,
        ghost: Some(Ghost {
            is_abstraction: |n, _| n.kind() == "trait_definition",
            heritage: |n| match n.kind() {
                "class_definition" | "object_definition" => fields(n, &["extend"]),
                _ => Vec::new(),
            },
        }),
        undefined: None,
        formatter: &[],
    },
    Spec {
        name: "dart",
        extensions: &["dart"],
        grammar: || tree_sitter_dart::LANGUAGE.into(),
        functions: &["function_declaration", "method_declaration", "function_expression"],
        methods: &["method_declaration"],
        classes: &[],
        calls: &["call_expression"],
        null_check: None,
        ghost: None,
        undefined: None,
        formatter: &["dart", "format"],
    },
    Spec {
        name: "php",
        extensions: &["php"],
        grammar: || tree_sitter_php::LANGUAGE_PHP.into(),
        functions: &["function_definition", "method_declaration", "anonymous_function", "arrow_function"],
        methods: &["method_declaration"],
        classes: &[],
        calls: &["function_call_expression"],
        null_check: Some(NullCheck {
            nulls: &["null"],
            ops: &["!==", "!="],
            non_null: |n, _| kind_in(&["object_creation_expression", "array_creation_expression", "string", "encapsed_string"])(n),
        }),
        ghost: Some(Ghost {
            is_abstraction: |n, src| {
                n.kind() == "interface_declaration"
                    || (n.kind() == "class_declaration" && has_child_text(n, src, &["abstract_modifier"], "abstract"))
            },
            heritage: |n| {
                if n.kind() == "class_declaration" {
                    children_of_kind(n, &["class_interface_clause", "base_clause"])
                } else {
                    Vec::new()
                }
            },
        }),
        undefined: None,
        formatter: &[],
    },
    Spec {
        name: "ruby",
        extensions: &["rb"],
        grammar: || tree_sitter_ruby::LANGUAGE.into(),
        functions: &["method", "singleton_method", "lambda"],
        methods: &[],
        classes: &["class", "module"],
        calls: &["call"],
        null_check: None,
        ghost: None,
        undefined: None,
        formatter: &[],
    },
    Spec {
        name: "lua",
        extensions: &["lua"],
        grammar: || tree_sitter_lua::LANGUAGE.into(),
        functions: &["function_declaration", "function_definition"],
        methods: &[],
        classes: &[],
        calls: &["function_call"],
        null_check: None,
        ghost: None,
        undefined: None,
        formatter: &["stylua"],
    },
    Spec {
        name: "gdscript",
        extensions: &["gd"],
        grammar: || tree_sitter_gdscript::LANGUAGE.into(),
        functions: &["function_definition", "lambda"],
        // A .gd file is a class: its functions are methods (and often engine callbacks).
        methods: &["function_definition"],
        classes: &[],
        calls: &["call"],
        null_check: Some(NullCheck {
            nulls: &["null"],
            ops: &["!="],
            non_null: |n, _| kind_in(&["array", "dictionary", "string", "integer", "float", "true", "false"])(n),
        }),
        ghost: None,
        undefined: None,
        formatter: &["gdformat"],
    },
    Spec {
        name: "bash",
        extensions: &["sh", "bash"],
        grammar: || tree_sitter_bash::LANGUAGE.into(),
        functions: &["function_definition"],
        methods: &[],
        classes: &[],
        calls: &[],
        null_check: None,
        ghost: None,
        undefined: None,
        formatter: &["shfmt", "-w"],
    },
    Spec {
        name: "matlab",
        extensions: &["m"],
        grammar: || tree_sitter_matlab::LANGUAGE.into(),
        functions: &["function_definition", "lambda"],
        methods: &[],
        classes: &["class_definition"],
        calls: &[],
        null_check: None,
        ghost: None,
        undefined: None,
        formatter: &[],
    },
];
