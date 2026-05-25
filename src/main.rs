//! lava-chart-cli — render a `(deflava-chart …)` source to a Helm
//! chart directory.
//!
//! Usage:
//!
//!     lava-chart-cli render <chart.tlisp> <out-dir>
//!
//! Emits `out-dir/Chart.yaml`, `out-dir/values.yaml`, and
//! `out-dir/templates/<name>.yaml` per manifest. Overwrites existing
//! files — re-run after every edit to the .tlisp source.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let cmd = args.next();
    let in_path = args.next().map(PathBuf::from);
    let out_dir = args.next().map(PathBuf::from);

    let (in_path, out_dir) = match (cmd.as_deref(), in_path, out_dir) {
        (Some("render"), Some(i), Some(o)) => (i, o),
        _ => {
            eprintln!("usage: lava-chart-cli render <chart.tlisp> <out-dir>");
            return ExitCode::from(2);
        }
    };

    let src = match std::fs::read_to_string(&in_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("read {}: {e}", in_path.display());
            return ExitCode::from(1);
        }
    };

    let charts = match lava_chart::charts_in_source(&src) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("parse {}: {e}", in_path.display());
            return ExitCode::from(1);
        }
    };
    if charts.is_empty() {
        eprintln!("no (deflava-chart …) forms in {}", in_path.display());
        return ExitCode::from(1);
    }
    let chart = &charts[0];

    let rendered = match lava_chart::render_chart(chart) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("render: {e}");
            return ExitCode::from(1);
        }
    };

    if let Err(e) = std::fs::create_dir_all(out_dir.join("templates")) {
        eprintln!("mkdir {}: {e}", out_dir.display());
        return ExitCode::from(1);
    }
    if let Err(e) = std::fs::write(out_dir.join("Chart.yaml"), &rendered.chart_yaml) {
        eprintln!("write Chart.yaml: {e}");
        return ExitCode::from(1);
    }
    if let Err(e) = std::fs::write(out_dir.join("values.yaml"), &rendered.values_yaml) {
        eprintln!("write values.yaml: {e}");
        return ExitCode::from(1);
    }
    for (rel, body) in &rendered.templates {
        let path = out_dir.join(rel);
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("mkdir {}: {e}", parent.display());
                return ExitCode::from(1);
            }
        }
        if let Err(e) = std::fs::write(&path, body) {
            eprintln!("write {}: {e}", path.display());
            return ExitCode::from(1);
        }
    }

    eprintln!(
        "rendered {} → Chart.yaml + values.yaml + {} template(s)",
        in_path.display(),
        rendered.templates.len()
    );
    ExitCode::SUCCESS
}
