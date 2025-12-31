//! SilkSurfJS CLI - JavaScript Engine
//!
//! Command-line interface for the SilkSurf JavaScript engine.

#[cfg(feature = "cli")]
use clap::Parser;

#[cfg(feature = "cli")]
#[derive(Parser, Debug)]
#[command(name = "silksurf")]
#[command(author = "SilkSurf Project")]
#[command(version)]
#[command(about = "Pure Rust JavaScript Engine", long_about = None)]
struct Args {
    /// JavaScript file to execute
    #[arg(value_name = "FILE")]
    file: Option<std::path::PathBuf>,

    /// Evaluate JavaScript code from command line
    #[arg(short, long)]
    eval: Option<String>,

    /// Start interactive REPL
    #[arg(short, long)]
    interactive: bool,

    /// Print AST instead of executing
    #[arg(long)]
    print_ast: bool,

    /// Print bytecode instead of executing
    #[arg(long)]
    print_bytecode: bool,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[cfg(feature = "cli")]
fn main() {
    let args = Args::parse();

    if args.verbose {
        eprintln!("SilkSurfJS v{}", env!("CARGO_PKG_VERSION"));
    }

    if let Some(code) = args.eval {
        // TODO: Execute code
        eprintln!("Eval: {code}");
    } else if let Some(file) = args.file {
        // TODO: Execute file
        eprintln!("File: {}", file.display());
    } else if args.interactive {
        // TODO: Start REPL
        eprintln!("REPL not yet implemented");
    } else {
        eprintln!("Usage: silksurf [OPTIONS] [FILE]");
        eprintln!("Try 'silksurf --help' for more information.");
    }
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("CLI feature not enabled. Build with: cargo build --features cli");
}
