//! schema-tool — deterministic JSON Schema check/codegen/validate/canonicalize CLI.

use anyhow::{Context, Result};
use clap::{ArgGroup, Parser, Subcommand};
use schema_tool::canonicalize::{
    canonicalize_selected_json, write_stdout, CanonicalOutputMode, CanonicalizeRequest,
};
use schema_tool::json_pointer::JsonPointer;
use schema_tool::validate::{render_success, validate_selected_request, ValidateSelectedRequest};
use schema_tool::{check, generate, paths};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "schema-tool",
    about = "Shittim JSON Schema generator and contract checker"
)]
struct Cli {
    /// Repository root. Defaults to discovery from CWD / executable location.
    #[arg(long, global = true)]
    repo_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate contract artifacts from schemas/source (currently Rust only).
    Generate,
    /// Validate schemas, refs, manifest and generation drift without writing.
    Check,
    /// Validate a JSON value against a schema $id or source path.
    Validate {
        /// Schema $id or relative source path under schemas/source.
        #[arg(long)]
        schema: String,
        /// JSON instance file to validate.
        #[arg(long)]
        instance: PathBuf,
        /// Strict RFC 6901 pointer selecting the value to validate.
        #[arg(long, default_value = "")]
        pointer: String,
    },
    /// Emit RFC 8785 canonical JSON for a selected value (stdout, no newline).
    #[command(group(
        ArgGroup::new("canonical_output")
            .args(["hex", "hash"])
            .multiple(false)
    ))]
    Canonicalize {
        /// JSON file to canonicalize.
        json_file: PathBuf,
        /// Strict RFC 6901 pointer selecting the value to canonicalize.
        #[arg(long, default_value = "")]
        pointer: String,
        /// Print lowercase hex of canonical UTF-8 bytes.
        #[arg(long)]
        hex: bool,
        /// Print lowercase SHA-256 of canonical UTF-8 bytes.
        #[arg(long)]
        hash: bool,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("schema-tool error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let repo_root = match cli.repo_root {
        Some(path) => path
            .canonicalize()
            .with_context(|| format!("resolve --repo-root {}", path.display()))?,
        None => paths::discover_repo_root()?,
    };

    match cli.command {
        Commands::Generate => generate::run(&repo_root),
        Commands::Check => check::run(&repo_root),
        Commands::Validate {
            schema,
            instance,
            pointer,
        } => {
            let request =
                ValidateSelectedRequest::new(schema, instance, JsonPointer::parse(&pointer)?);
            let result = validate_selected_request(&repo_root, &request)?;
            println!("{}", render_success(&result));
            Ok(())
        }
        Commands::Canonicalize {
            json_file,
            pointer,
            hex,
            hash,
        } => {
            let output_mode = if hex {
                CanonicalOutputMode::Hex
            } else if hash {
                CanonicalOutputMode::Hash
            } else {
                CanonicalOutputMode::Bytes
            };
            let request =
                CanonicalizeRequest::new(json_file, JsonPointer::parse(&pointer)?, output_mode);
            let result = canonicalize_selected_json(&request)?;
            write_stdout(&result)
        }
    }
}
