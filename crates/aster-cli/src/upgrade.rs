//! `aster upgrade`: download a released binary from a deterministic
//! `github.com/.../releases/download/` URL (no GitHub API, so no rate limit),
//! verify its sha256, and swap it in place with `self-replace`.

use anyhow::{Context, Result};
use clap::Args;

const REPO_OWNER: &str = "zfinix";
const REPO_NAME: &str = "aster";

#[derive(Args)]
pub struct UpgradeArgs {
    /// Upgrade to a specific version (e.g. 0.4.0) instead of the latest.
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Reinstall even when the running binary is already current.
    #[arg(long)]
    force: bool,
}

pub async fn run(args: UpgradeArgs) -> Result<()> {
    kill_other_instances();

    // The cliclack steps below only read well on a terminal; piped output
    // stays plain `[aster]` lines.
    let fancy = crate::picker::is_tty() && !crate::json_mode();
    if fancy {
        cliclack::set_theme(crate::init::AsterTheme);
    }
    let step = |msg: &str| {
        if fancy {
            let _ = cliclack::log::step(msg);
        } else {
            eprintln!("[aster] {msg}");
        }
    };

    let tag = match &args.version {
        Some(version) => normalize_tag(version),
        None => {
            step("Resolving latest release…");
            tokio::task::spawn_blocking(latest_cli_tag)
                .await?
                .context("latest release lookup failed")?
        }
    };
    let version = tag
        .strip_prefix("cli-v")
        .with_context(|| format!("unexpected release tag: {tag}"))?
        .to_string();
    let current = env!("CARGO_PKG_VERSION");
    if !args.force && version == current {
        println!("[aster] aster is up to date ({current}).");
        return Ok(());
    }

    step(&format!("Installing aster {version}…"));

    // Blocking HTTP and file I/O, so keep it off the async executor.
    let install_version = version.clone();
    tokio::task::spawn_blocking(move || install(&tag, &install_version))
        .await?
        .context("install failed")?;

    match fancy {
        true => cliclack::outro(format!("Installed aster {version}"))?,
        false => println!("[aster] Installed aster {version}."),
    }
    Ok(())
}

fn install(tag: &str, version: &str) -> Result<()> {
    let fancy = crate::picker::is_tty() && !crate::json_mode();
    let target = target_triple();
    let base = format!(
        "https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/download/{tag}/aster-{version}-{target}"
    );
    let (asset_url, zipped) = if cfg!(windows) {
        (format!("{base}.zip"), true)
    } else {
        (format!("{base}.tar.gz"), false)
    };

    let asset_name = format!(
        "aster-{version}-{target}.{}",
        if zipped { "zip" } else { "tar.gz" }
    );
    match fancy {
        true => cliclack::log::step(format!("Downloading {asset_name}"))?,
        false => eprintln!("[aster] Downloading {asset_url}..."),
    }
    let archive_path = download(&asset_url, "archive").context("asset download failed")?;

    let expected_path = download(&format!("{asset_url}.sha256"), "checksum").ok();
    let expected = expected_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string());
    if let Some(expected) = expected {
        let archive = std::fs::read(&archive_path).context("temp archive read failed")?;
        verify_sha256(&archive, &expected).context("checksum verification failed")?;
        if fancy {
            cliclack::log::success("Checksum verified")?;
        }
    } else if fancy {
        cliclack::log::warning("No checksum published for this release; skipping verification")?;
    } else {
        eprintln!("[aster] No checksum published for this release; skipping verification.");
    }

    let binary = unpack_from_file(&archive_path, &format!("aster-{version}-{target}"), zipped)?;
    self_replace::self_replace(&binary).context("binary replacement failed")?;
    Ok(())
}

fn download(url: &str, name: &str) -> Result<std::path::PathBuf> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .context("http client build failed")?;
    let response = client.get(url).send()?.error_for_status()?;

    let total = response.content_length();
    let path = std::env::temp_dir().join(format!("aster-download-{}-{name}", std::process::id()));
    let mut file = std::fs::File::create(&path).context("temp file creation failed")?;

    // Only real downloads get a bar; a 65 B checksum fetch renders as nothing.
    let show_bar = crate::picker::is_tty() && total.is_some_and(|t| t > 1 << 20);
    let bar = show_bar.then(|| {
        std::sync::Arc::new(DownloadBar {
            term: console::Term::stderr(),
            total: total.unwrap_or_default(),
            last: std::sync::Mutex::new((std::time::Instant::now(), 0)),
        })
    });
    let shared = std::sync::Arc::new(SharedProgress {
        downloaded: std::sync::atomic::AtomicU64::new(0),
        bar,
    });

    const CHUNKS: u64 = 16;
    let parallel = total.is_some_and(|t| t > 1 << 20)
        && response
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .is_some_and(|v| v.to_str().is_ok_and(|v| v.contains("bytes")));

    let result = if parallel {
        drop(response);
        let total = total.unwrap_or_default();
        file.set_len(total).context("temp file sizing failed")?;
        let chunk_size = total.div_ceil(CHUNKS);
        let mut handles = Vec::new();
        for chunk in 0..CHUNKS {
            let start = chunk * chunk_size;
            let end = (start + chunk_size).min(total);
            if start >= end {
                break;
            }
            let client = client.clone();
            let url = url.to_string();
            let path = path.clone();
            let progress = shared.clone();
            handles.push(
                std::thread::Builder::new()
                    .name(format!("aster-dl-{chunk}"))
                    .spawn(move || -> Result<()> {
                        let mut resp = client
                            .get(&url)
                            .header(reqwest::header::RANGE, format!("bytes={start}-{}", end - 1))
                            .send()?
                            .error_for_status()?;
                        let mut file = std::fs::OpenOptions::new()
                            .write(true)
                            .open(&path)
                            .context("temp file open failed")?;
                        use std::io::Seek;
                        file.seek(std::io::SeekFrom::Start(start))
                            .context("temp file seek failed")?;
                        let mut reader = ChunkReader {
                            inner: &mut resp,
                            remaining: end - start,
                            progress: &progress,
                        };
                        std::io::copy(&mut reader, &mut file).context("download interrupted")?;
                        Ok(())
                    })
                    .context("download thread spawn failed")?,
            );
        }
        let joined: Result<Vec<_>> = handles
            .into_iter()
            .map(|h| {
                h.join()
                    .map_err(|_| anyhow::anyhow!("download thread panicked"))?
            })
            .collect();
        joined.map(|_| ())
    } else {
        let mut reader = ProgressReader {
            inner: response,
            progress: shared.as_ref(),
        };
        std::io::copy(&mut reader, &mut file)
            .map(|_| ())
            .context("download interrupted")
    };
    if let Some(bar) = shared.bar.as_ref() {
        bar.finish(shared.downloaded.load(std::sync::atomic::Ordering::Relaxed));
    } else if total.is_some() {
        eprintln!();
    }
    result?;
    Ok(path)
}

struct SharedProgress {
    downloaded: std::sync::atomic::AtomicU64,
    bar: Option<std::sync::Arc<DownloadBar>>,
}

impl SharedProgress {
    fn add(&self, n: u64) {
        use std::sync::atomic::Ordering;
        let downloaded = self.downloaded.fetch_add(n, Ordering::Relaxed) + n;
        if let Some(bar) = &self.bar {
            bar.draw(downloaded);
        }
    }
}

struct DownloadBar {
    term: console::Term,
    total: u64,
    last: std::sync::Mutex<(std::time::Instant, u64)>,
}

impl DownloadBar {
    const WIDTH: usize = 24;
    const ORANGE: (u8, u8, u8) = (0xf2, 0x76, 0x4f);

    fn line(&self, downloaded: u64, message: &str) -> String {
        let frac = if self.total == 0 {
            1.0
        } else {
            (downloaded as f64 / self.total as f64).min(1.0)
        };
        let filled = (frac * Self::WIDTH as f64).round() as usize;
        let fill = console::style("█".repeat(filled)).true_color(
            Self::ORANGE.0,
            Self::ORANGE.1,
            Self::ORANGE.2,
        );
        let rest = console::style("░".repeat(Self::WIDTH - filled)).dim();
        let pct = format!("{:>3.0}%", frac * 100.0);
        format!(
            "{pct} {fill}{rest} {:>7}/{:<7} {message:<12}",
            format_bytes(downloaded),
            format_bytes(self.total),
        )
    }

    fn draw(&self, downloaded: u64) {
        let mut last = self.last.lock().unwrap();
        let done = downloaded >= self.total;
        if !done && last.0.elapsed() < std::time::Duration::from_millis(80) {
            return;
        }
        *last = (std::time::Instant::now(), downloaded);
        let _ = self
            .term
            .write_str(&format!("\r{}", self.line(downloaded, "Downloading")));
    }

    fn finish(&self, downloaded: u64) {
        let _ = self.term.write_str(&format!(
            "\r{}\n",
            console::style(self.line(downloaded, "Downloaded")).bold()
        ));
    }
}

struct ChunkReader<'a, R: std::io::Read> {
    inner: R,
    remaining: u64,
    progress: &'a SharedProgress,
}

impl<R: std::io::Read> std::io::Read for ChunkReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let cap = buf.len().min(self.remaining as usize);
        let n = self.inner.read(&mut buf[..cap])?;
        self.remaining -= n as u64;
        self.progress.add(n as u64);
        Ok(n)
    }
}

fn format_bytes(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MiB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KiB", b as f64 / (1 << 10) as f64),
        b => format!("{b} B"),
    }
}

struct ProgressReader<'a, R: std::io::Read> {
    inner: R,
    progress: &'a SharedProgress,
}

impl<R: std::io::Read> std::io::Read for ProgressReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.progress.add(n as u64);
        Ok(n)
    }
}

fn verify_sha256(bytes: &[u8], expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    let actual = format!("{:x}", Sha256::digest(bytes));
    if !expected.eq_ignore_ascii_case(&actual) {
        anyhow::bail!("checksum mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

fn unpack_from_file(
    archive_path: &std::path::Path,
    dir_name: &str,
    zipped: bool,
) -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(dir_name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).context("temp dir creation failed")?;

    if zipped {
        let archive = std::fs::read(archive_path).context("temp archive read failed")?;
        unpack_zip(&archive, &dir)?;
    } else {
        let file = std::fs::File::open(archive_path).context("temp archive open failed")?;
        let reader = std::io::BufReader::new(file);
        unpack_tar_gz_stream(reader, &dir)?;
    }
    find_binary(&dir)
}

fn find_binary(dir: &std::path::Path) -> Result<std::path::PathBuf> {
    let name = bin_name();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries =
            std::fs::read_dir(&current).with_context(|| format!("read {}", current.display()))?;
        while let Some(entry) = entries.next().transpose().context("temp dir read failed")? {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == name) {
                return Ok(path);
            }
        }
    }
    anyhow::bail!("archive did not contain {name}")
}

fn unpack_tar_gz_stream<R: std::io::Read>(reader: R, dir: &std::path::Path) -> Result<()> {
    let gz = flate2::read::GzDecoder::new(reader);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(dir).context("tar extraction failed")?;
    Ok(())
}

#[cfg(windows)]
fn unpack_zip(archive: &[u8], dir: &std::path::Path) -> Result<()> {
    let zip_path = dir.join("aster.zip");
    std::fs::write(&zip_path, archive).context("temp archive write failed")?;
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Expand-Archive",
            "-Force",
            &format!("'{}'", zip_path.display()),
            &format!("'{}'", dir.display()),
        ])
        .status()
        .context("Expand-Archive failed")?;
    if !status.success() {
        anyhow::bail!("Expand-Archive exited with {status}");
    }
    let _ = std::fs::remove_file(&zip_path);
    Ok(())
}

#[cfg(unix)]
fn unpack_zip(_: &[u8], _: &std::path::Path) -> Result<()> {
    anyhow::bail!("zip archives are only published for Windows releases")
}

fn bin_name() -> &'static str {
    if cfg!(windows) { "aster.exe" } else { "aster" }
}

fn target_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        _ => "unknown",
    }
}

fn kill_other_instances() {
    let our_pid = std::process::id();
    let pids = match running_aster_pids() {
        Ok(pids) => pids,
        Err(e) => {
            eprintln!("[aster] Could not list running instances: {e}");
            return;
        }
    };
    for pid in pids {
        if pid == our_pid {
            continue;
        }
        eprintln!("[aster] Stopping aster (pid {pid})...");
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

#[cfg(unix)]
fn running_aster_pids() -> Result<Vec<u32>> {
    let out = std::process::Command::new("pgrep")
        .args(["-x", "aster"])
        .output()
        .context("pgrep aster")?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().filter_map(|l| l.trim().parse().ok()).collect())
}

#[cfg(windows)]
fn running_aster_pids() -> Result<Vec<u32>> {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq aster.exe", "/FO", "CSV", "/NH"])
        .output()
        .context("tasklist aster.exe")?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter_map(|l| l.split(',').nth(1))
        .filter_map(|s| s.trim_matches('"').parse().ok())
        .collect())
}

fn normalize_tag(version: &str) -> String {
    match version {
        v if v.starts_with("cli-v") => v.to_string(),
        v if v.starts_with('v') => format!("cli-{v}"),
        v => format!("cli-v{v}"),
    }
}

fn latest_cli_tag() -> Result<String> {
    let latest = format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let final_url = reqwest::blocking::get(latest)?.url().clone();
    let tag = final_url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .context("releases/latest redirect carried no tag")?;
    if !tag.starts_with("cli-v") {
        anyhow::bail!("unexpected latest release tag: {tag}");
    }
    Ok(tag.to_string())
}

#[cfg(test)]
#[path = "tests/upgrade_test.rs"]
mod tests;
