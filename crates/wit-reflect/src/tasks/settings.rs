//! The contract settings in a component crate's `Cargo.toml`.
//!
//! They hold what no Rust item of a linked crate can state: the package and
//! world, the interfaces the world imports and exports, and functions the
//! component crate implements itself. Every problem in the table is reported
//! together.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::package_name::PackageName;

/// Why the contract settings of a manifest cannot be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsError {
    /// The manifest cannot be read.
    #[error("cannot read {}: {source}", .manifest.display())]
    Read {
        /// The manifest's path.
        manifest: PathBuf,
        /// The failure reported by the filesystem.
        source: io::Error,
    },

    /// The manifest is not valid TOML.
    #[error("{} is not valid TOML: {source}", .manifest.display())]
    Syntax {
        /// The manifest's path.
        manifest: PathBuf,
        /// The parser's report.
        source: toml::de::Error,
    },

    /// The manifest has no `[package.metadata.wit-reflect]` table.
    #[error("{} has no `[package.metadata.wit-reflect]` table", .manifest.display())]
    Missing {
        /// The manifest's path.
        manifest: PathBuf,
    },

    /// The settings name something the contract cannot use.
    #[error(
        "the contract settings in {} are invalid:{}",
        .manifest.display(),
        listed(.problems)
    )]
    Invalid {
        /// The manifest's path.
        manifest: PathBuf,
        /// Every problem found, one sentence each.
        problems: Vec<String>,
    },
}

/// One line per problem, so a generator's output lists them all.
fn listed(problems: &[String]) -> String {
    let mut listing = String::new();
    for problem in problems {
        listing.push_str("\n- ");
        listing.push_str(problem);
    }
    listing
}

/// The `[package.metadata.wit-reflect]` table of one manifest.
#[derive(Debug)]
pub(super) struct Settings {
    pub(super) package: PackageName,
    pub(super) world: String,
    /// Interfaces of other packages, as `namespace:package/interface@version`.
    pub(super) imports: Vec<String>,
    pub(super) exports: Vec<Export>,
    pub(super) functions: Vec<FunctionSetting>,
    pub(super) wit: PathBuf,
    pub(super) bindings: PathBuf,
    pub(super) forwarding: PathBuf,
}

/// One exported interface of the world.
#[derive(Debug)]
pub(super) struct Export {
    pub(super) interface: String,
    /// The name the world gives the export, when it is not the interface's.
    pub(super) name: Option<String>,
}

/// A function the component crate implements itself.
///
/// Its crate compiles only once the contract's files exist, so the function
/// cannot register itself.
#[derive(Debug)]
pub(super) struct FunctionSetting {
    pub(super) interface: String,
    pub(super) name: String,
    /// The WIT name of the type it returns. `None` if it returns nothing.
    pub(super) returns: Option<String>,
    /// The path generated forwarding code calls it by.
    pub(super) path: String,
    pub(super) docs: Option<String>,
}

impl Settings {
    /// Read the settings of the manifest at `manifest`.
    ///
    /// Output paths are resolved against the manifest's directory.
    pub(super) fn read(manifest: &Path) -> Result<Self, SettingsError> {
        let manifest_text = fs::read_to_string(manifest).map_err(|source| SettingsError::Read {
            manifest: manifest.to_owned(),
            source,
        })?;
        let manifest_table: Table =
            manifest_text
                .parse()
                .map_err(|source| SettingsError::Syntax {
                    manifest: manifest.to_owned(),
                    source,
                })?;
        let settings_table = manifest_table
            .get("package")
            .and_then(|package| package.get("metadata"))
            .and_then(|metadata| metadata.get("wit-reflect"))
            .and_then(Value::as_table)
            .ok_or_else(|| SettingsError::Missing {
                manifest: manifest.to_owned(),
            })?;

        let directory = manifest.parent().unwrap_or(Path::new(""));
        Self::parse(settings_table, directory).map_err(|problems| SettingsError::Invalid {
            manifest: manifest.to_owned(),
            problems,
        })
    }

    fn parse(table: &Table, directory: &Path) -> Result<Self, Vec<String>> {
        let mut problems = Vec::new();
        let mut entries = Entries::of(table, "the settings", &mut problems);

        let package = entries.required_string("package").and_then(|text| {
            let parse_result = text.parse::<PackageName>();
            match parse_result {
                Ok(package) => Some(package),
                Err(error) => {
                    entries.report(format!("`package` is not a package name: {error}"));
                    None
                }
            }
        });
        let world = entries.required_string("world");
        let imports = entries.strings("imports");
        let export_values = entries.array("exports");
        let function_values = entries.array("function");
        let wit = entries.string("wit");
        let bindings = entries.string("bindings");
        let forwarding = entries.string("forwarding");
        // Read by the derive, not by the tasks: the crate's representation map.
        entries.string("representations");
        entries.finish();

        let exports = parse_exports(export_values, &mut problems);
        let functions = parse_functions(function_values, &mut problems);
        match (package, world) {
            (Some(package), Some(world)) if problems.is_empty() => {
                let default_wit = format!("wit/{world}.wit");
                Ok(Self {
                    package,
                    world: world.to_owned(),
                    imports,
                    exports,
                    functions,
                    wit: directory.join(wit.unwrap_or(&default_wit)),
                    bindings: directory.join(bindings.unwrap_or("src/bindings.rs")),
                    forwarding: directory.join(forwarding.unwrap_or("src/forwarding.rs")),
                })
            }
            _ => Err(problems),
        }
    }
}

/// An export is an interface name, or a table that also gives the export's
/// own name.
fn parse_exports(values: &[Value], problems: &mut Vec<String>) -> Vec<Export> {
    let mut exports = Vec::with_capacity(values.len());
    for (position, value) in values.iter().enumerate() {
        match value {
            Value::String(interface) => exports.push(Export {
                interface: interface.clone(),
                name: None,
            }),
            Value::Table(table) => {
                let subject = format!("entry {} of `exports`", position + 1);
                let mut entries = Entries::of(table, &subject, problems);
                let interface = entries.required_string("interface");
                let name = entries.string("name");
                entries.finish();
                if let Some(interface) = interface {
                    exports.push(Export {
                        interface: interface.to_owned(),
                        name: name.map(str::to_owned),
                    });
                }
            }
            _ => problems.push(format!(
                "entry {} of `exports` must be an interface name or a table \
                 with `interface` and `name`",
                position + 1
            )),
        }
    }
    exports
}

fn parse_functions(values: &[Value], problems: &mut Vec<String>) -> Vec<FunctionSetting> {
    let mut functions = Vec::with_capacity(values.len());
    for (position, value) in values.iter().enumerate() {
        let subject = format!("`[[function]]` entry {}", position + 1);
        let Value::Table(table) = value else {
            problems.push(format!("{subject} must be a table"));
            continue;
        };
        let mut entries = Entries::of(table, &subject, problems);
        let interface = entries.required_string("interface");
        let name = entries.required_string("name");
        let path = entries.required_string("path");
        let returns = entries.string("returns");
        let docs = entries.string("docs");
        entries.finish();
        if let (Some(interface), Some(name), Some(path)) = (interface, name, path) {
            functions.push(FunctionSetting {
                interface: interface.to_owned(),
                name: name.to_owned(),
                returns: returns.map(str::to_owned),
                path: path.to_owned(),
                docs: docs.map(str::to_owned),
            });
        }
    }
    functions
}

/// One table while its keys are read.
///
/// It remembers which keys were asked for, so [`Entries::finish`] can report
/// the ones nothing understands, such as a misspelled key.
struct Entries<'table, 'problems> {
    table: &'table Table,
    /// How a message names the table.
    subject: &'problems str,
    read_keys: Vec<&'static str>,
    problems: &'problems mut Vec<String>,
}

impl<'table, 'problems> Entries<'table, 'problems> {
    fn of(
        table: &'table Table,
        subject: &'problems str,
        problems: &'problems mut Vec<String>,
    ) -> Self {
        Self {
            table,
            subject,
            read_keys: Vec::new(),
            problems,
        }
    }

    fn report(&mut self, problem: String) {
        self.problems.push(problem);
    }

    fn value(&mut self, key: &'static str) -> Option<&'table Value> {
        self.read_keys.push(key);
        self.table.get(key)
    }

    /// The string at `key`, if the key is present.
    fn string(&mut self, key: &'static str) -> Option<&'table str> {
        let value = self.value(key)?;
        let text = value.as_str();
        if text.is_none() {
            self.report(format!("`{key}` in {} must be a string", self.subject));
        }
        text
    }

    fn required_string(&mut self, key: &'static str) -> Option<&'table str> {
        if !self.table.contains_key(key) {
            self.read_keys.push(key);
            self.report(format!("`{key}` is missing in {}", self.subject));
            return None;
        }
        self.string(key)
    }

    /// The array at `key`. A missing key is an empty array.
    fn array(&mut self, key: &'static str) -> &'table [Value] {
        let Some(value) = self.value(key) else {
            return &[];
        };
        match value.as_array() {
            Some(values) => values,
            None => {
                self.report(format!("`{key}` in {} must be an array", self.subject));
                &[]
            }
        }
    }

    /// The strings of the array at `key`.
    fn strings(&mut self, key: &'static str) -> Vec<String> {
        let mut strings = Vec::new();
        for (position, value) in self.array(key).iter().enumerate() {
            match value.as_str() {
                Some(text) => strings.push(text.to_owned()),
                None => self.report(format!(
                    "entry {} of `{key}` in {} must be a string",
                    position + 1,
                    self.subject
                )),
            }
        }
        strings
    }

    /// Report every key of the table that was never asked for.
    fn finish(self) {
        for key in self.table.keys() {
            if !self.read_keys.contains(&key.as_str()) {
                self.problems
                    .push(format!("`{key}` in {} is not a known key", self.subject));
            }
        }
    }
}
