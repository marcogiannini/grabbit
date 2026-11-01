// SPDX-License-Identifier: GPL-3.0-or-later
//! Gestione degli strumenti esterni: yt-dlp, Deno (runtime JS richiesto da
//! yt-dlp per YouTube) e ffmpeg. Vengono scaricati nella cartella dati
//! dell'app e tenuti aggiornati automaticamente.
use crate::net;
use crate::tr;
use anyhow::{anyhow, bail, Context, Result};
use directories::ProjectDirs;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("org", "Grabbit", "Grabbit")
}

pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("grabbit"))
}

pub fn bin_dir() -> PathBuf {
    data_dir().join("bin")
}

pub fn ytdlp_path() -> PathBuf {
    bin_dir().join(format!("yt-dlp{EXE}"))
}

pub fn deno_path() -> PathBuf {
    bin_dir().join(format!("deno{EXE}"))
}

/// `Command` che su Windows non apre una finestra console.
pub fn command<S: AsRef<OsStr>>(program: S) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

#[cfg(unix)]
fn make_executable(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(p)?.permissions();
    perm.set_mode(0o755);
    fs::set_permissions(p, perm)?;
    Ok(())
}
#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

fn pct(done: u64, total: Option<u64>) -> String {
    match total {
        Some(t) if t > 0 => format!("{}%", done * 100 / t),
        _ => format!("{} MB", done / 1_048_576),
    }
}

// ───────────────────────────── yt-dlp ─────────────────────────────

fn ytdlp_asset() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(target_arch = "aarch64") {
        "yt-dlp_linux_aarch64"
    } else {
        "yt-dlp_linux"
    }
}

pub fn local_ytdlp_version() -> Option<String> {
    let p = ytdlp_path();
    if !p.exists() {
        return None;
    }
    let out = command(&p).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

fn latest_ytdlp_version() -> Result<String> {
    let v = net::get_json("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")?;
    v["tag_name"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            anyhow!(tr!(
                "risposta GitHub senza tag_name",
                "GitHub reply without tag_name"
            ))
        })
}

fn install_ytdlp(status: &mut dyn FnMut(String)) -> Result<()> {
    let url = format!(
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}",
        ytdlp_asset()
    );
    let dest = ytdlp_path();
    net::download(&url, &dest, &mut |d, t| {
        status(tr!(
            format!("Scarico yt-dlp… {}", pct(d, t)),
            format!("Downloading yt-dlp… {}", pct(d, t))
        ))
    })?;
    make_executable(&dest)?;
    Ok(())
}

/// Scarica yt-dlp se manca, lo aggiorna se esiste una release più recente.
/// Ritorna la versione installata.
pub fn ensure_ytdlp(status: &mut dyn FnMut(String)) -> Result<String> {
    match local_ytdlp_version() {
        None => {
            install_ytdlp(status).context(tr!("installazione di yt-dlp", "installing yt-dlp"))?;
        }
        Some(local) => {
            status(
                tr!(
                    "Controllo aggiornamenti di yt-dlp…",
                    "Checking for yt-dlp updates…"
                )
                .into(),
            );
            // se il controllo fallisce (offline, rate limit) si usa la versione locale
            if let Ok(latest) = latest_ytdlp_version() {
                if latest != local {
                    status(tr!(
                        format!("Aggiorno yt-dlp {local} → {latest}…"),
                        format!("Updating yt-dlp {local} → {latest}…")
                    ));
                    install_ytdlp(status)
                        .context(tr!("aggiornamento di yt-dlp", "updating yt-dlp"))?;
                }
            }
        }
    }
    local_ytdlp_version().ok_or_else(|| {
        anyhow!(tr!(
            "yt-dlp non si avvia dopo l'installazione",
            "yt-dlp does not start after installation"
        ))
    })
}

// ───────────────────────────── Deno ─────────────────────────────

fn deno_target() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => return None,
    })
}

/// Deno è il runtime JavaScript che yt-dlp usa per risolvere le "sfide" di
/// YouTube. Se manca lo scarichiamo accanto a yt-dlp.
pub fn ensure_deno(status: &mut dyn FnMut(String)) -> Result<PathBuf> {
    let dest = deno_path();
    if dest.exists() {
        return Ok(dest);
    }
    let target = deno_target().ok_or_else(|| {
        anyhow!(tr!(
            "piattaforma non supportata per Deno",
            "platform not supported for Deno"
        ))
    })?;
    let url =
        format!("https://github.com/denoland/deno/releases/latest/download/deno-{target}.zip");
    let tmp = data_dir().join("deno.zip");
    net::download(&url, &tmp, &mut |d, t| {
        status(tr!(
            format!("Scarico il runtime JavaScript (Deno)… {}", pct(d, t)),
            format!("Downloading the JavaScript runtime (Deno)… {}", pct(d, t))
        ))
    })?;
    extract_zip(&tmp, &[&format!("deno{EXE}")], &bin_dir())?;
    let _ = fs::remove_file(&tmp);
    if !dest.exists() {
        bail!(tr!(
            "Deno non trovato nell'archivio",
            "Deno not found in the archive"
        ));
    }
    Ok(dest)
}

// ───────────────────────────── ffmpeg ─────────────────────────────

/// Cartella che contiene ffmpeg: prima la nostra, poi il PATH di sistema.
pub fn find_ffmpeg_dir() -> Option<PathBuf> {
    let ours = bin_dir();
    if ours.join(format!("ffmpeg{EXE}")).is_file() {
        return Some(ours);
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    // le app grafiche su macOS non ereditano il PATH della shell
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    dirs.into_iter()
        .find(|d| d.join(format!("ffmpeg{EXE}")).is_file())
}

pub fn ensure_ffmpeg(status: &mut dyn FnMut(String)) -> Result<PathBuf> {
    if let Some(d) = find_ffmpeg_dir() {
        return Ok(d);
    }
    let dest = bin_dir();
    fs::create_dir_all(&dest)?;
    let tmp = data_dir().join("ffmpeg-download");
    let mut progress = |d: u64, t: Option<u64>| {
        status(tr!(
            format!("Scarico ffmpeg… {}", pct(d, t)),
            format!("Downloading ffmpeg… {}", pct(d, t))
        ))
    };

    #[cfg(windows)]
    {
        let url = "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip";
        net::download(url, &tmp, &mut progress)?;
        extract_zip(&tmp, &["ffmpeg.exe", "ffprobe.exe"], &dest)?;
    }
    #[cfg(target_os = "macos")]
    {
        // build statiche di evermeet.cx (x86_64; su Apple Silicon usa Rosetta)
        net::download(
            "https://evermeet.cx/ffmpeg/getrelease/zip",
            &tmp,
            &mut progress,
        )?;
        extract_zip(&tmp, &["ffmpeg"], &dest)?;
    }
    #[cfg(target_os = "linux")]
    {
        let arch = if cfg!(target_arch = "aarch64") {
            "linuxarm64"
        } else {
            "linux64"
        };
        let url = format!(
            "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-{arch}-gpl.tar.xz"
        );
        net::download(&url, &tmp, &mut progress)?;
        status(tr!("Estraggo ffmpeg…", "Extracting ffmpeg…").into());
        extract_tar_xz(&tmp, &dest)?;
    }
    let _ = fs::remove_file(&tmp);
    find_ffmpeg_dir().ok_or_else(|| {
        anyhow!(tr!(
            "ffmpeg non trovato dopo l'estrazione",
            "ffmpeg not found after extraction"
        ))
    })
}

// ─────────────────────────── estrazione archivi ───────────────────────────

fn extract_zip(zip_path: &Path, wanted: &[&str], dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let entry_name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
        let base = Path::new(&entry_name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if wanted.contains(&base.as_str()) {
            let out_path = dest.join(&base);
            let mut out = File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out)?;
            drop(out);
            make_executable(&out_path)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn extract_tar_xz(path: &Path, dest: &Path) -> Result<()> {
    let file = std::io::BufReader::new(File::open(path)?);
    let mut archive = tar::Archive::new(xz2::read::XzDecoder::new(file));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let (name, in_bin) = {
            let p = entry.path()?;
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let in_bin = p
                .parent()
                .and_then(|d| d.file_name())
                .map(|n| n == "bin")
                .unwrap_or(false);
            (name, in_bin)
        };
        if in_bin && (name == "ffmpeg" || name == "ffprobe") {
            let out = dest.join(&name);
            entry.unpack(&out)?;
            make_executable(&out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("grabbit-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn extracts_wanted_files_from_zip_in_nested_dirs() {
        let dir = tmpdir("zip");
        let zip_path = dir.join("a.zip");
        {
            let mut w = zip::ZipWriter::new(File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("ffmpeg-master/bin/ffmpeg.exe", opts).unwrap();
            w.write_all(b"FF").unwrap();
            w.start_file("ffmpeg-master/bin/ffprobe.exe", opts).unwrap();
            w.write_all(b"FP").unwrap();
            w.start_file("ffmpeg-master/doc/readme.txt", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let out = dir.join("out");
        extract_zip(&zip_path, &["ffmpeg.exe", "ffprobe.exe"], &out).unwrap();
        assert_eq!(fs::read(out.join("ffmpeg.exe")).unwrap(), b"FF");
        assert_eq!(fs::read(out.join("ffprobe.exe")).unwrap(), b"FP");
        assert!(!out.join("readme.txt").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn extracts_ffmpeg_from_tar_xz() {
        let dir = tmpdir("tarxz");
        let archive = dir.join("a.tar.xz");
        {
            let enc = xz2::write::XzEncoder::new(File::create(&archive).unwrap(), 1);
            let mut tb = tar::Builder::new(enc);
            for (name, data) in [
                ("ff-master/bin/ffmpeg", &b"FF"[..]),
                ("ff-master/bin/ffprobe", b"FP"),
                ("ff-master/LICENSE", b"L"),
            ] {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                tb.append_data(&mut h, name, data).unwrap();
            }
            tb.into_inner().unwrap().finish().unwrap();
        }
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        extract_tar_xz(&archive, &out).unwrap();
        assert_eq!(fs::read(out.join("ffmpeg")).unwrap(), b"FF");
        assert!(out.join("ffprobe").exists());
        assert!(!out.join("LICENSE").exists());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(out.join("ffmpeg"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0o111
        );
    }
}
