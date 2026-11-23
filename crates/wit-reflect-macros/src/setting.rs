//! The representation map a crate names in its manifest.
//!
//! ```toml
//! [package.metadata.wit-reflect]
//! representations = "crate::wit::Representations"
//! ```
//!
//! With this setting every `#[derive(WitType)]` of the package's library looks
//! its fields up in that map, without naming it on each item. Reading the
//! manifest needs a TOML parser, so it is compiled only with the
//! `representations` feature; without it no crate has a map setting.

use proc_macro2::TokenStream;
use syn::Path;

/// The map the expanding crate's manifest names, if this crate is the
/// package's library.
///
/// Other targets of the package (binaries, examples, integration tests) are
/// crates of their own, in which `crate::…` names something else.
///
/// # Errors
///
/// Returns a message if the manifest cannot be read or the setting is not a
/// path.
#[cfg(feature = "representations")]
pub(crate) fn crate_map() -> Result<Option<Path>, String> {
    use std::sync::OnceLock;

    // One compiler process expands every derive of one crate, so the manifest
    // is read once.
    static SETTING: OnceLock<Result<Option<String>, String>> = OnceLock::new();

    let setting = SETTING.get_or_init(read_setting).clone()?;
    let Some(written_path) = setting else {
        return Ok(None);
    };
    let map = syn::parse_str::<Path>(&written_path).map_err(|error| {
        format!(
            "`representations = \"{written_path}\"` in `Cargo.toml` is not a path to the \
             crate's map: {error}"
        )
    })?;
    Ok(Some(map))
}

#[cfg(not(feature = "representations"))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature is the same with and without the feature"
)]
pub(crate) fn crate_map() -> Result<Option<Path>, String> {
    Ok(None)
}

/// An item that makes the compiler record the manifest as an input.
///
/// Cargo does not rebuild a crate when only the metadata in its manifest
/// changes. With this item a changed setting is expanded again.
#[cfg(feature = "representations")]
pub(crate) fn tracked_manifest() -> TokenStream {
    quote::quote! {
        const _: &[u8] = ::core::include_bytes!(
            ::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")
        );
    }
}

#[cfg(not(feature = "representations"))]
pub(crate) fn tracked_manifest() -> TokenStream {
    TokenStream::new()
}

/// The setting as written, or `None` if the manifest has none or this crate
/// is not the package's library.
#[cfg(feature = "representations")]
fn read_setting() -> Result<Option<String>, String> {
    use std::env;

    use toml::{Table, Value};

    let Ok(directory) = env::var("CARGO_MANIFEST_DIR") else {
        // Not built by Cargo: there is no manifest to read.
        return Ok(None);
    };
    let manifest_path = format!("{directory}/Cargo.toml");
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {manifest_path}: {error}"))?;
    let manifest: Table = manifest_text
        .parse()
        .map_err(|error| format!("{manifest_path} is not valid TOML: {error}"))?;

    let package = manifest.get("package");
    let setting = package
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("wit-reflect"))
        .and_then(|settings| settings.get("representations"));
    let Some(setting) = setting else {
        return Ok(None);
    };
    let Value::String(written_path) = setting else {
        return Err(format!(
            "`representations` in the `[package.metadata.wit-reflect]` table of \
             {manifest_path} must be a string naming the crate's map"
        ));
    };

    // A binary is compiled with `CARGO_BIN_NAME`, also when it shares the
    // library's name.
    let is_binary = env::var_os("CARGO_BIN_NAME").is_some();
    let library_name = manifest
        .get("lib")
        .and_then(|library| library.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            let package_name = package?.get("name")?.as_str()?;
            Some(package_name.replace('-', "_"))
        });
    let is_library = !is_binary && env::var("CARGO_CRATE_NAME").ok() == library_name;
    Ok(is_library.then(|| written_path.clone()))
}
