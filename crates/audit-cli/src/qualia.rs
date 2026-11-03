//! Integration with the QualiaDB toolchain (`qualia-cli`) for validating and
//! converting emitted Turtle graphs into `.q42` volumes.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Locate the `qualia-cli` binary: explicit flag → `QUALIA_CLI` env →
/// `tools/qualia-cli(.exe)` next to CWD or the audit binary → PATH.
pub fn find_cli(explicit: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        if p.exists() {
            return Ok(p.clone());
        }
        bail!("--qualia-cli path does not exist: {}", p.display());
    }
    if let Ok(p) = std::env::var("QUALIA_CLI") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Ok(p);
        }
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from("tools/qualia-cli.exe"),
        PathBuf::from("tools/qualia-cli"),
    ];
    if let Some(d) = exe_dir {
        candidates.push(d.join("qualia-cli.exe"));
        candidates.push(d.join("qualia-cli"));
    }
    for c in candidates {
        if c.exists() {
            return Ok(c);
        }
    }
    // Fall back to PATH lookup.
    for name in ["qualia-cli.exe", "qualia-cli"] {
        if let Ok(out) = Command::new(name).arg("--version").output() {
            if out.status.success() {
                return Ok(PathBuf::from(name));
            }
        }
    }
    bail!(
        "qualia-cli not found. Set QUALIA_CLI, pass --qualia-cli, or drop the binary in tools/.\n\
         Download: https://github.com/mediaprophet/qualiaDB/releases"
    )
}

/// Run `qualia-cli q42 audit-source` on a Turtle file; prints its report.
pub fn audit_source(cli: &Path, ttl: &Path) -> Result<()> {
    let status = Command::new(cli)
        .args(["q42", "audit-source"])
        .arg(ttl)
        .status()
        .with_context(|| format!("running {}", cli.display()))?;
    if !status.success() {
        bail!("qualia-cli q42 audit-source failed on {}", ttl.display());
    }
    Ok(())
}

/// Run `qualia-cli import <ttl> <q42>`.
pub fn import(cli: &Path, ttl: &Path, q42: &Path) -> Result<()> {
    let status = Command::new(cli)
        .arg("import")
        .arg(ttl)
        .arg(q42)
        .status()
        .with_context(|| format!("running {}", cli.display()))?;
    if !status.success() {
        bail!("qualia-cli import failed for {}", ttl.display());
    }
    Ok(())
}

/// Run `qualia-cli query sparql <q42> <query>`, streaming output.
/// Strips `#` comment lines — qualia-cli's parser rejects them.
pub fn sparql(cli: &Path, q42: &Path, query: &str) -> Result<()> {
    let stripped: String = query
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let status = Command::new(cli)
        .args(["query", "sparql"])
        .arg(q42)
        .arg(&stripped)
        .status()
        .with_context(|| format!("running {}", cli.display()))?;
    if !status.success() {
        bail!("qualia-cli query sparql failed");
    }
    Ok(())
}

/// Concatenate Turtle files into one document. Repeated `@prefix` lines are
/// legal Turtle, so plain concatenation is safe.
pub fn concat_ttl(inputs: &[PathBuf], dest: &Path) -> Result<usize> {
    let mut out = String::new();
    for p in inputs {
        let text = std::fs::read_to_string(p)
            .with_context(|| format!("reading {}", p.display()))?;
        out.push_str(&text);
        if !text.ends_with('\n') {
            out.push('\n');
        }
    }
    std::fs::write(dest, &out).with_context(|| format!("writing {}", dest.display()))?;
    Ok(inputs.len())
}
