use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Serialize, Deserialize)]
struct FileResult {
    file: String,
    input_bytes: usize,
    output_bytes: usize,
    reduction_pct: f64,
    time_us: u128,
    iterations: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct BenchReport {
    files: Vec<FileResult>,
    total_input_bytes: usize,
    total_output_bytes: usize,
    total_reduction_pct: f64,
    total_time_us: u128,
}

#[derive(Debug, Serialize, Deserialize)]
struct Baseline {
    files: HashMap<String, BaselineEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BaselineEntry {
    output_bytes: usize,
}

fn main() {
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/corpus");
    let baseline_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/baseline.json");

    if !corpus_dir.exists() {
        eprintln!("corpus directory not found: {}", corpus_dir.display());
        std::process::exit(1);
    }

    // Collect SVG files
    let mut svg_files: Vec<_> = fs::read_dir(&corpus_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "svg")
        })
        .collect();
    svg_files.sort_by_key(|e| e.file_name());

    if svg_files.is_empty() {
        eprintln!("no SVG files found in corpus");
        std::process::exit(1);
    }

    // Load baseline if exists
    let baseline: Option<Baseline> = if baseline_path.exists() {
        let data = fs::read_to_string(&baseline_path).unwrap();
        Some(serde_json::from_str(&data).unwrap())
    } else {
        None
    };

    let mut results = Vec::new();
    let mut total_input: usize = 0;
    let mut total_output: usize = 0;
    let mut total_time: u128 = 0;
    let mut regressions = Vec::new();

    // Header
    println!(
        "{:<35} {:>8} {:>8} {:>7} {:>8} {:>4}",
        "File", "Input", "Output", "Saved", "Time", "Iter"
    );
    println!("{}", "-".repeat(75));

    for entry in &svg_files {
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let input = fs::read_to_string(&path).unwrap();
        let input_bytes = input.len();

        // Warm up
        let _ = svgm_core::optimize(&input);

        // Timed run (best of 3)
        let mut best_time = u128::MAX;
        let mut output_data = String::new();
        let mut iters = 0;

        for _ in 0..3 {
            let start = Instant::now();
            let result = svgm_core::optimize(&input).unwrap();
            let elapsed = start.elapsed().as_micros();
            if elapsed < best_time {
                best_time = elapsed;
                output_data = result.data;
                iters = result.iterations;
            }
        }

        let output_bytes = output_data.len();
        let reduction = if input_bytes > 0 {
            (1.0 - output_bytes as f64 / input_bytes as f64) * 100.0
        } else {
            0.0
        };

        // Check for regression against baseline
        if let Some(ref bl) = baseline {
            if let Some(bl_entry) = bl.files.get(&name) {
                if output_bytes > bl_entry.output_bytes {
                    regressions.push(format!(
                        "{}: output {} bytes > baseline {} bytes (+{} bytes)",
                        name,
                        output_bytes,
                        bl_entry.output_bytes,
                        output_bytes - bl_entry.output_bytes
                    ));
                }
            }
        }

        println!(
            "{:<35} {:>8} {:>8} {:>6.1}% {:>5}us {:>4}",
            name, input_bytes, output_bytes, reduction, best_time, iters
        );

        total_input += input_bytes;
        total_output += output_bytes;
        total_time += best_time;

        results.push(FileResult {
            file: name,
            input_bytes,
            output_bytes,
            reduction_pct: (reduction * 10.0).round() / 10.0,
            time_us: best_time,
            iterations: iters,
        });
    }

    let total_reduction = if total_input > 0 {
        (1.0 - total_output as f64 / total_input as f64) * 100.0
    } else {
        0.0
    };

    println!("{}", "-".repeat(75));
    println!(
        "{:<35} {:>8} {:>8} {:>6.1}% {:>5}us",
        "TOTAL", total_input, total_output, total_reduction, total_time
    );

    // Build report
    let report = BenchReport {
        files: results,
        total_input_bytes: total_input,
        total_output_bytes: total_output,
        total_reduction_pct: (total_reduction * 10.0).round() / 10.0,
        total_time_us: total_time,
    };

    // Write JSON report
    let report_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/report.json");
    let json = serde_json::to_string_pretty(&report).unwrap();
    fs::write(&report_path, &json).unwrap();
    eprintln!("\nReport written to {}", report_path.display());

    // Report regressions
    if !regressions.is_empty() {
        eprintln!("\nREGRESSIONS DETECTED:");
        for r in &regressions {
            eprintln!("  {}", r);
        }
        std::process::exit(1);
    }
}
