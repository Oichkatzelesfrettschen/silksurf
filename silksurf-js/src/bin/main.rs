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

    /// Print AST instead of executing (requires legacy-vm feature)
    #[arg(long)]
    print_ast: bool,

    /// Print bytecode instead of executing (requires legacy-vm feature)
    #[arg(long)]
    print_bytecode: bool,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[cfg(feature = "cli")]
fn run_script(src: &str, verbose: bool) -> Result<(), String> {
    let mut ctx = silksurf_js::SilkContext::new();
    if verbose {
        eprintln!("Evaluating {} bytes", src.len());
    }
    ctx.eval(src)?;
    ctx.run_pending_jobs();
    Ok(())
}

#[cfg(feature = "cli")]
fn run_repl() {
    use std::io::{self, BufRead, Write};
    eprintln!("SilkSurf JS REPL -- Ctrl-D to exit");
    let mut ctx = silksurf_js::SilkContext::new();
    loop {
        print!("> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        match io::stdin().lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("error: {e}");
                break;
            }
        }
        let src = line.trim();
        if src.is_empty() {
            continue;
        }
        if let Err(e) = ctx.eval(src) {
            eprintln!("error: {e}");
        }
        ctx.run_pending_jobs();
    }
}

#[cfg(feature = "cli")]
fn main() {
    let args = Args::parse();

    if args.print_ast || args.print_bytecode {
        eprintln!("error: --print-ast and --print-bytecode require the legacy-vm feature");
        std::process::exit(1);
    }

    if args.verbose {
        eprintln!("SilkSurfJS v{}", env!("CARGO_PKG_VERSION"));
    }

    if let Some(code) = args.eval {
        if let Err(e) = run_script(&code, args.verbose) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    } else if let Some(file) = args.file {
        let src = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read {}: {e}", file.display());
                std::process::exit(1);
            }
        };
        if let Err(e) = run_script(&src, args.verbose) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    } else if args.interactive {
        run_repl();
    } else {
        eprintln!("Usage: silksurf [OPTIONS] [FILE]");
        eprintln!("Try 'silksurf --help' for more information.");
    }
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("CLI feature not enabled. Build with: cargo build --features cli");
}
