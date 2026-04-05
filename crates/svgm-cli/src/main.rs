use clap::Parser;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "svgm", version, about = "SVG minimizer — fast, safe, single-pass SVG optimizer")]
struct Cli {
    /// Input SVG file(s)
    #[arg(required = true)]
    input: Vec<PathBuf>,

    /// Output file (default: overwrite in place; prints to stdout if piped)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Print to stdout instead of overwriting
    #[arg(long)]
    stdout: bool,

    /// Show per-pass breakdown
    #[arg(long)]
    stats: bool,

    /// Show what would change without writing
    #[arg(long)]
    dry_run: bool,

    /// Suppress all output except errors
    #[arg(short, long)]
    quiet: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.input.len() > 1 && cli.output.is_some() {
        eprintln!(
            "{} cannot use -o with multiple input files",
            style("error:").red().bold()
        );
        std::process::exit(1);
    }

    let mut exit_code = 0;
    for input_path in &cli.input {
        if let Err(e) = process_file(&cli, input_path) {
            eprintln!(
                "{} {}: {}",
                style("error:").red().bold(),
                input_path.display(),
                e
            );
            exit_code = 1;
        }
    }

    std::process::exit(exit_code);
}

fn process_file(cli: &Cli, input_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let input = fs::read_to_string(input_path)?;
    let input_size = input.len();

    // Determine where output goes:
    // --stdout flag → stdout
    // -o path → that path
    // piped (not a terminal) → stdout
    // otherwise → overwrite input file in place (SVGO default behavior)
    let write_to_stdout = cli.stdout || (cli.output.is_none() && !io::stdout().is_terminal());

    // Show spinner unless quiet or writing to stdout
    let spinner = if !cli.quiet && !write_to_stdout {
        let sp = ProgressBar::new_spinner();
        sp.set_style(
            ProgressStyle::with_template("  {spinner} Optimizing {msg}...")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]),
        );
        sp.set_message(input_path.file_name().unwrap().to_string_lossy().to_string());
        sp.enable_steady_tick(std::time::Duration::from_millis(80));
        Some(sp)
    } else {
        None
    };

    let start = Instant::now();
    let result = svgm_core::optimize(&input)?;
    let elapsed = start.elapsed();
    let output_size = result.data.len();

    if let Some(sp) = &spinner {
        sp.finish_and_clear();
    }

    if cli.dry_run {
        if !cli.quiet {
            print_summary(input_path, input_size, output_size, elapsed, result.iterations);
        }
        return Ok(());
    }

    if write_to_stdout {
        io::stdout().write_all(result.data.as_bytes())?;
    } else {
        let output_path = cli.output.as_ref().unwrap_or(input_path);
        fs::write(output_path, &result.data)?;
        if !cli.quiet {
            print_summary(input_path, input_size, output_size, elapsed, result.iterations);
        }
    }

    Ok(())
}

fn print_summary(
    path: &std::path::Path,
    input_size: usize,
    output_size: usize,
    elapsed: std::time::Duration,
    iterations: usize,
) {
    let filename = path.file_name().unwrap().to_string_lossy();
    let reduction = if input_size > 0 {
        ((input_size - output_size) as f64 / input_size as f64) * 100.0
    } else {
        0.0
    };

    let input_fmt = format_bytes(input_size);
    let output_fmt = format_bytes(output_size);
    let elapsed_fmt = format_duration(elapsed);

    let reduction_str = format!("{:.1}% smaller", reduction);
    let colored_reduction = if reduction > 0.0 {
        style(reduction_str).green()
    } else {
        style(reduction_str).yellow()
    };

    let pass_word = if iterations == 1 { "pass" } else { "passes" };

    eprintln!();
    eprintln!("  {}", style(&*filename).bold());
    eprintln!(
        "  {} → {} ({})  {}  {} {}",
        style(input_fmt).dim(),
        style(output_fmt).white().bold(),
        colored_reduction,
        style(elapsed_fmt).dim(),
        iterations,
        style(pass_word).dim(),
    );
    eprintln!();
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn format_duration(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms >= 1000 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}ms", ms)
    }
}
