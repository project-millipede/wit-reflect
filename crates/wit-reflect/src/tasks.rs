//! Generate a component crate's contract files, or check that they are
//! current.
//!
//! With the contract declared by marks on the items (see
//! [`registry`](crate::registry)), a generator program is one call:
//!
//! ```text
//! // Link the crate whose items are marked; nothing else of it is used here.
//! use owner_crate as _;
//!
//! fn main() -> std::process::ExitCode {
//!     wit_reflect::tasks::run("crates/component/Cargo.toml")
//! }
//! ```
//!
//! The tasks read what no Rust item states from the component crate's
//! manifest:
//!
//! ```toml
//! [package.metadata.wit-reflect]
//! package = "example:session@0.1.0"
//! world = "session"
//! imports = ["example:sockets/connections@0.1.0"]
//! exports = [{ interface = "native", name = "native" }, "binary"]
//!
//! [[package.metadata.wit-reflect.function]]
//! interface = "native"
//! name = "create-session"
//! returns = "session"
//! path = "crate::create_session"
//! docs = "Create a session without a selection."
//! ```
//!
//! | Key | Meaning |
//! | --- | --- |
//! | `package` | The WIT package, `namespace:name@version`. Required. |
//! | `world` | The world's name. Required. |
//! | `imports` | Interfaces of other packages the world imports. |
//! | `exports` | Interfaces of this package the world exports. A table also gives the export its own name. |
//! | `wit`, `bindings`, `forwarding` | Output paths below the manifest's directory. They default to `wit/<world>.wit`, `src/bindings.rs` and `src/forwarding.rs`. |
//! | `function` | A function the component crate implements itself. |
//! | `representations` | Not read here: the crate's representation map, which the derive reads. |
//!
//! A `function` entry exists because the component crate cannot register
//! anything: it compiles only once its generated files exist. The entry gives
//! the function's interface, its WIT `name`, the `path` generated code calls
//! it by, optionally the WIT name of the type it `returns` and its `docs`.
//! It takes no parameters. If the Rust function's signature differs, the
//! component crate does not compile.
//!
//! The three generated files are meant to be committed. [`generate`] rewrites
//! them. [`check`] changes nothing and fails when one of them differs, so a
//! contract change is visible in review.

mod settings;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::error::SchemaErrors;
use crate::forwarding::ForwardingErrors;
use crate::function::Function;
use crate::registry::NothingRegistered;
use crate::schema::Schema;
use crate::types::Type;

pub use settings::SettingsError;
use settings::{FunctionSetting, Settings};

/// Why the contract files could not be generated or are not current.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TaskError {
    /// The manifest's contract settings cannot be used.
    #[error(transparent)]
    Settings(#[from] SettingsError),

    /// The program links no crate with marked items.
    #[error(transparent)]
    Registry(#[from] NothingRegistered),

    /// The declared contract is not valid.
    #[error(transparent)]
    Schema(#[from] SchemaErrors),

    /// The component's Rust files cannot be generated from the contract.
    #[error(transparent)]
    Forwarding(#[from] ForwardingErrors),

    /// A generated file cannot be read or written.
    #[error("cannot {action} {}: {source}", .path.display())]
    File {
        /// `read` or `write`.
        action: &'static str,
        /// The generated file.
        path: PathBuf,
        /// The failure reported by the filesystem.
        source: io::Error,
    },

    /// Committed files differ from what the declarations generate.
    #[error(
        "generated contract files are stale: {}; review the change, then regenerate them",
        .paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
    )]
    Stale {
        /// The files that differ or are missing.
        paths: Vec<PathBuf>,
    },
}

/// Rewrite the contract files of the component crate whose manifest is at
/// `manifest`.
///
/// Returns the files whose contents changed. A file that is already current
/// is left untouched.
///
/// # Errors
///
/// Returns a [`TaskError`] if the settings or the declared contract are
/// invalid, if nothing is registered, or if a file cannot be read or written.
pub fn generate(manifest: impl AsRef<Path>) -> Result<Vec<PathBuf>, TaskError> {
    let mut written_paths = Vec::new();
    for file in generated_files(manifest.as_ref())? {
        if file.is_current()? {
            continue;
        }
        if let Some(directory) = file.path.parent() {
            fs::create_dir_all(directory).map_err(|source| file.error("write", source))?;
        }
        fs::write(&file.path, &file.contents).map_err(|source| file.error("write", source))?;
        written_paths.push(file.path);
    }
    Ok(written_paths)
}

/// Check that the contract files of the component crate whose manifest is at
/// `manifest` are what the declarations generate. Nothing is written.
///
/// A check never repairs: a stale file means a Rust declaration or a setting
/// changed, and that change belongs in review.
///
/// # Errors
///
/// Returns [`TaskError::Stale`] naming every file that differs or is missing,
/// or another [`TaskError`] under the conditions of [`generate`].
pub fn check(manifest: impl AsRef<Path>) -> Result<(), TaskError> {
    let mut stale_paths = Vec::new();
    for file in generated_files(manifest.as_ref())? {
        if !file.is_current()? {
            stale_paths.push(file.path);
        }
    }
    if stale_paths.is_empty() {
        Ok(())
    } else {
        Err(TaskError::Stale { paths: stale_paths })
    }
}

/// Run [`generate`], or [`check`] if the program was started with `--check`,
/// and report the outcome on the standard streams.
///
/// This is the whole body of a generator program's `main`.
#[must_use]
pub fn run(manifest: impl AsRef<Path>) -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match arguments.as_slice() {
        [] => generate(manifest).map(|written_paths| {
            for path in &written_paths {
                println!("wrote {}", path.display());
            }
            if written_paths.is_empty() {
                println!("generated contract files are current");
            }
        }),
        [flag] if flag == "--check" => check(manifest).map(|()| {
            println!("generated contract files are current");
        }),
        _ => {
            eprintln!("usage: [--check]");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// One generated file: where it belongs and what it must contain.
struct GeneratedFile {
    path: PathBuf,
    contents: String,
}

impl GeneratedFile {
    /// Whether the file on disk already holds the generated contents. A
    /// missing file is not current.
    fn is_current(&self) -> Result<bool, TaskError> {
        let read_result = fs::read_to_string(&self.path);
        match read_result {
            Ok(committed_contents) => Ok(committed_contents == self.contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(self.error("read", source)),
        }
    }

    fn error(&self, action: &'static str, source: io::Error) -> TaskError {
        TaskError::File {
            action,
            path: self.path.clone(),
            source,
        }
    }
}

/// The WIT package, the bindings file and the forwarding file, in that order.
fn generated_files(manifest: &Path) -> Result<Vec<GeneratedFile>, TaskError> {
    let settings = Settings::read(manifest)?;
    let schema = declared_schema(&settings, manifest)?;
    let package = schema.resolve()?;
    let bindings = package.bindings(&settings.world)?;
    let forwarding = package.forwarding(&settings.world)?;
    Ok(vec![
        GeneratedFile {
            path: settings.wit,
            contents: package.to_string(),
        },
        GeneratedFile {
            path: settings.bindings,
            contents: bindings,
        },
        GeneratedFile {
            path: settings.forwarding,
            contents: forwarding,
        },
    ])
}

/// The registered declarations, completed with the settings.
fn declared_schema(settings: &Settings, manifest: &Path) -> Result<Schema, TaskError> {
    let mut schema = Schema::from_registry(settings.package.clone())?;

    let mut problems = Vec::new();
    for function in &settings.functions {
        match function_signature(function, &schema) {
            Ok(signature) => {
                // The model holds paths as `&'static str`, because every other
                // one is a literal of the declaring crate. Settings are read
                // once by a generator program, so these strings are leaked.
                let rust_path: &'static str = String::leak(function.path.clone());
                schema.declare_function(&function.interface, signature, rust_path);
            }
            Err(problem) => problems.push(problem),
        }
    }
    if !problems.is_empty() {
        return Err(TaskError::Settings(SettingsError::Invalid {
            manifest: manifest.to_owned(),
            problems,
        }));
    }

    let mut world = schema.world(&settings.world);
    for import in &settings.imports {
        world.import(import);
    }
    for export in &settings.exports {
        match &export.name {
            Some(name) => world.export_as(name, &export.interface),
            None => world.export(&export.interface),
        };
    }
    Ok(schema)
}

/// The signature a `function` entry states, or what is wrong with the entry.
fn function_signature(function: &FunctionSetting, schema: &Schema) -> Result<Function, String> {
    let result = match &function.returns {
        // The unit type: the function returns nothing.
        None => Type::Tuple(Vec::new()),
        Some(type_name) => {
            let mut types = schema.types_named(type_name);
            match (types.pop(), types.is_empty()) {
                (Some(only_type), true) => only_type,
                (None, _) => {
                    return Err(format!(
                        "function `{}` returns `{type_name}`, but no registered type has that name",
                        function.name
                    ));
                }
                (Some(_), false) => {
                    return Err(format!(
                        "function `{}` returns `{type_name}`, which names more than one \
                         registered type",
                        function.name
                    ));
                }
            }
        }
    };

    // Leaked for the reason given in `declared_schema`.
    let name: &'static str = String::leak(function.name.clone());
    let signature = Function::new(name, Vec::new(), result);
    Ok(match &function.docs {
        Some(docs) => signature.with_docs(String::leak(docs.clone())),
        None => signature,
    })
}
