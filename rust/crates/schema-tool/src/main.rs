//! schema-tool — deterministic JSON Schema check/codegen/validate/canonicalize CLI.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use schema_tool::{canonicalize, check, generate, paths, validate};
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
    /// Validate JSON instances against a schema $id or source path.
    Validate {
        /// Schema $id or relative source path under schemas/source.
        #[arg(long)]
        schema: String,
        /// JSON instance file to validate.
        #[arg(long)]
        instance: PathBuf,
    },
    /// Emit RFC 8785 canonical JSON bytes for a file (stdout, no trailing newline).
    Canonicalize {
        /// JSON file to canonicalize.
        json_file: PathBuf,
        /// Also print lowercase SHA-256 of canonical bytes.
        #[arg(long)]
        hash: bool,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("schema-tool error: {err:#}");
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
        Commands::Validate { schema, instance } => validate::run(&repo_root, &schema, &instance),
        Commands::Canonicalize { json_file, hash } => canonicalize::run(&json_file, hash),
    }
}
