mod config_file;

use clap::{Parser, ValueEnum};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use svgm_core::Config;

#[derive(Clone, Copy, ValueEnum)]
enum CliPreset {
    Safe,
    Balanced,
    Aggressive,
}

impl From<CliPreset> for svgm_core::Preset {
    fn from(p: CliPreset) -> Self {
        match p {
            CliPreset::Safe => svgm_core::Preset::Safe,
            CliPreset::Balanced => svgm_core::Preset::Balanced,
            CliPreset::Aggressive => svgm_core::Preset::Aggressive,
        }
    }
}

#[derive(Parser)]
#[command(
    name = "svgm",
    version,
    about = "SVG minimizer — fast, safe, fixed-point convergence SVG optimizer",
    after_help = "When piped (stdout is not a terminal), output goes to stdout automatically.\n\
                  Config: place svgm.config.toml in your project root for per-project settings."
)]
struct Cli {
    /// SVG file(s) to optimize, or a directory with -r
    #[arg(required = true)]
    input: Vec<PathBuf>,

    /// Write output to PATH instead of overwriting in place
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Optimization preset [default: balanced]
    #[arg(
        long,
        value_enum,
        long_help = "Optimization preset [default: balanced]\n\n  \
                     safe:       removal and normalization only (17 passes)\n  \
                     balanced:   full optimization (24 passes)\n  \
                     aggressive: full optimization, lower precision"
    )]
    preset: Option<CliPreset>,

    /// Decimal digits for numeric rounding [default: 3, or 2 with aggressive]
    #[arg(long, value_name = "N")]
    precision: Option<u32>,

    /// Path to config file (default: auto-discover svgm.config.toml)
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Skip config file auto-discovery
    #[arg(long)]
    no_config: bool,

    /// Recursively optimize all SVGs in a directory
    #[arg(short, long)]
    recursive: bool,

    /// Write result to stdout instead of overwriting the file
    #[arg(long)]
    stdout: bool,

    /// Show per-pass breakdown
    #[arg(long)]
    stats: bool,

    /// Preview size reduction without writing any files
    #[arg(long)]
    dry_run: bool,

    /// Suppress all output except errors
    #[arg(short, long)]
    quiet: bool,
}

fn build_config(cli: &Cli) -> Config {
    let start_dir = cli
        .input
        .first()
        .and_then(|p| {
            if p.is_dir() {
                Some(p.as_path())
            } else {
                p.parent()
            }
        })
        .unwrap_or(Path::new("."));

    let explicit = cli.config.as_deref();

    // Load config file (unless --no-config)
    let mut config = if cli.no_config {
        Config::default()
    } else {
        match config_file::find_config(explicit, start_dir) {
            Some(path) => match config_file::load_config(&path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{} {}: {}", style("error:").red().bold(), path.display(), e);
                    std::process::exit(1);
                }
            },
            None => {
                if explicit.is_some() {
                    eprintln!(
                        "{} config file not found: {}",
                        style("error:").red().bold(),
                        cli.config.as_ref().unwrap().display()
                    );
                    std::process::exit(1);
                }
                Config::default()
            }
        }
    };

    // CLI flags override config file
    if let Some(preset) = cli.preset {
        config.preset = preset.into();
    }
    if let Some(precision) = cli.precision {
        config.precision = Some(precision);
    }

    config
}

fn main() {
    let cli = Cli::parse();
    let config = build_config(&cli);

    if cli.recursive {
        // -r requires exactly one input and it must be a directory
        if cli.input.len() != 1 {
            eprintln!(
                "{} -r requires exactly one directory input",
                style("error:").red().bold()
            );
            std::process::exit(1);
        }
        if !cli.input[0].is_dir() {
            eprintln!(
                "{} {} is not a directory",
                style("error:").red().bold(),
                cli.input[0].display()
            );
            std::process::exit(1);
        }
        if cli.stdout {
            eprintln!(
                "{} cannot use --stdout with -r",
                style("error:").red().bold()
            );
            std::process::exit(1);
        }

        let source = fs::canonicalize(&cli.input[0]).unwrap_or_else(|_| cli.input[0].clone());

        // Check output directory is not inside (or equal to) source directory
        if let Some(out) = &cli.output {
            // Resolve the output path through its nearest existing ancestor
            // to handle macOS /var → /private/var symlinks
            let out_abs = if out.exists() {
                fs::canonicalize(out).unwrap_or_else(|_| out.clone())
            } else {
                let mut ancestor = out.as_path();
                loop {
                    match ancestor.parent() {
                        Some(p) if !p.as_os_str().is_empty() => {
                            if p.exists() {
                                let base = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
                                break base.join(out.strip_prefix(p).unwrap());
                            }
                            ancestor = p;
                        }
                        _ => break out.clone(),
                    }
                }
            };
            if out_abs.starts_with(&source) || source.starts_with(&out_abs) {
                eprintln!(
                    "{} output directory must not overlap with source directory",
                    style("error:").red().bold()
                );
                std::process::exit(1);
            }
        }

        let files = match collect_svg_files(&cli.input[0]) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "{} {}: {}",
                    style("error:").red().bold(),
                    cli.input[0].display(),
                    e
                );
                std::process::exit(1);
            }
        };

        if files.is_empty() {
            if !cli.quiet {
                eprintln!("  0 SVG files found");
            }
            return;
        }

        let base_dir = cli.output.as_ref().map(|_| cli.input[0].as_path());
        run_directory_mode(&cli, &config, &files, base_dir);
    } else {
        // File mode validations
        for input in &cli.input {
            if input.is_dir() {
                eprintln!(
                    "{} {} is a directory, use -r to process recursively",
                    style("error:").red().bold(),
                    input.display()
                );
                std::process::exit(1);
            }
        }

        if cli.input.len() > 1 && cli.output.is_some() {
            eprintln!(
                "{} cannot use -o with multiple input files",
                style("error:").red().bold()
            );
            std::process::exit(1);
        }

        if cli.input.len() > 1 && cli.stdout {
            eprintln!(
                "{} cannot use --stdout with multiple input files",
                style("error:").red().bold()
            );
            std::process::exit(1);
        }

        let files: Vec<PathBuf> = cli.input.clone();
        run_file_mode(&cli, &config, &files);
    }
}

fn run_file_mode(cli: &Cli, config: &Config, files: &[PathBuf]) {
    let multi_file = files.len() > 1;
    let mut exit_code = 0;

    for file in files {
        if let Err(e) = process_file(cli, config, file, multi_file) {
            eprintln!("{} {}: {}", style("error:").red().bold(), file.display(), e);
            exit_code = 1;
        }
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn run_directory_mode(cli: &Cli, config: &Config, files: &[PathBuf], base_dir: Option<&Path>) {
    let total = files.len() as u64;
    let mut total_input = 0usize;
    let mut total_output = 0usize;
    let mut errors = 0usize;

    let bar = if !cli.quiet {
        let b = ProgressBar::new(total);
        b.set_style(
            ProgressStyle::with_template("  {bar:24} {pos}/{len} files")
                .unwrap()
                .progress_chars("█░░"),
        );
        Some(b)
    } else {
        None
    };

    let start = Instant::now();

    for file in files {
        // Compute output path if -o is set
        let output_path = match (&cli.output, base_dir) {
            (Some(out_dir), Some(base)) => {
                let relative = file.strip_prefix(base).unwrap();
                let target = out_dir.join(relative);
                if let Some(parent) = target.parent()
                    && let Err(e) = fs::create_dir_all(parent)
                {
                    eprintln!(
                        "{} {}: {}",
                        style("error:").red().bold(),
                        parent.display(),
                        e
                    );
                    errors += 1;
                    if let Some(b) = &bar {
                        b.inc(1);
                    }
                    continue;
                }
                Some(target)
            }
            _ => None,
        };

        match optimize_file(file, config) {
            Ok((input_size, output_data, _iterations)) => {
                total_input += input_size;
                total_output += output_data.len();

                if !cli.dry_run {
                    let target = output_path.as_deref().unwrap_or(file.as_path());
                    if let Err(e) = fs::write(target, &output_data) {
                        eprintln!(
                            "{} {}: {}",
                            style("error:").red().bold(),
                            target.display(),
                            e
                        );
                        errors += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("{} {}: {}", style("error:").red().bold(), file.display(), e);
                errors += 1;
            }
        }

        if let Some(b) = &bar {
            b.inc(1);
        }
    }

    let elapsed = start.elapsed();

    if let Some(b) = &bar {
        b.finish_and_clear();
    }

    if !cli.quiet {
        let optimized = files.len() - errors;
        let file_word = if optimized == 1 { "file" } else { "files" };
        let reduction = if total_input > 0 {
            ((total_input - total_output) as f64 / total_input as f64) * 100.0
        } else {
            0.0
        };

        let reduction_str = format!("{:.1}% smaller", reduction);
        let colored_reduction = if reduction > 0.0 {
            style(reduction_str).green()
        } else {
            style(reduction_str).yellow()
        };

        eprintln!();
        eprintln!("  {} {} optimized", style(optimized).bold(), file_word);
        eprintln!(
            "  {} → {} ({})  {}",
            style(format_bytes(total_input)).dim(),
            style(format_bytes(total_output)).white().bold(),
            colored_reduction,
            style(format_duration(elapsed)).dim(),
        );
        if errors > 0 {
            eprintln!(
                "  {} {} failed",
                style(errors).red().bold(),
                if errors == 1 { "file" } else { "files" }
            );
        }
        eprintln!();
    }

    if errors > 0 {
        std::process::exit(1);
    }
}

fn optimize_file(
    path: &Path,
    config: &Config,
) -> Result<(usize, String, usize), Box<dyn std::error::Error>> {
    let input = fs::read_to_string(path)?;
    let input_size = input.len();
    let result = svgm_core::optimize_with_config(&input, config)?;
    Ok((input_size, result.data, result.iterations))
}

fn process_file(
    cli: &Cli,
    config: &Config,
    input_path: &Path,
    multi_file: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = fs::read_to_string(input_path)?;
    let input_size = input.len();

    // Determine where output goes:
    // --stdout flag → stdout
    // -o path → that path
    // single file + piped (not a terminal) → stdout
    // otherwise → overwrite input file in place
    let write_to_stdout =
        cli.stdout || (cli.output.is_none() && !multi_file && !io::stdout().is_terminal());

    // Show spinner unless quiet or writing to stdout
    let spinner = if !cli.quiet && !write_to_stdout {
        let sp = ProgressBar::new_spinner();
        sp.set_style(
            ProgressStyle::with_template("  {spinner} Optimizing {msg}...")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]),
        );
        sp.set_message(
            input_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        );
        sp.enable_steady_tick(std::time::Duration::from_millis(80));
        Some(sp)
    } else {
        None
    };

    let start = Instant::now();
    let result = svgm_core::optimize_with_config(&input, config)?;
    let elapsed = start.elapsed();
    let output_size = result.data.len();

    if let Some(sp) = &spinner {
        sp.finish_and_clear();
    }

    if cli.dry_run {
        if !cli.quiet {
            print_summary(
                input_path,
                input_size,
                output_size,
                elapsed,
                result.iterations,
            );
        }
        return Ok(());
    }

    if write_to_stdout {
        io::stdout().write_all(result.data.as_bytes())?;
    } else {
        let output_path = cli.output.as_deref().unwrap_or(input_path);
        fs::write(output_path, &result.data)?;
        if !cli.quiet {
            print_summary(
                input_path,
                input_size,
                output_size,
                elapsed,
                result.iterations,
            );
        }
    }

    Ok(())
}

fn collect_svg_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_svg_files_inner(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_svg_files_inner(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_svg_files_inner(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn print_summary(
    path: &Path,
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
