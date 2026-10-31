//! Aggiornamento automatico di Grabbit stesso.
//!
//! Legge l'ultima release del repository indicato in `Cargo.toml`
//! (campo `repository`) e, se la versione è più recente, scarica il file
//! giusto per la piattaforma e sostituisce l'eseguibile in uso.
//!
//! Nomi dei file che devi allegare a ogni release GitHub:
//!   • Windows : grabbit-windows-x86_64.exe
//!   • Linux   : grabbit-linux-x86_64.AppImage   (e -aarch64)
//!   • macOS   : grabbit-macos-aarch64 / grabbit-macos-x86_64  (binario nudo)
use crate::{net, tools};
use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page_url: String,
    pub asset_url: Option<String>,
}

/// "owner/repo" ricavato da Cargo.toml, `None` finché c'è il segnaposto OWNER.
fn repo() -> Option<String> {
    let url = env!("CARGO_PKG_REPOSITORY")
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let r = url.split("github.com/").nth(1)?.to_string();
    (!r.starts_with("OWNER/")).then_some(r)
}

fn asset_name() -> Option<String> {
    let arch = std::env::consts::ARCH; // x86_64 | aarch64
    Some(match std::env::consts::OS {
        "windows" => format!("grabbit-windows-{arch}.exe"),
        "linux" => format!("grabbit-linux-{arch}.AppImage"),
        "macos" => format!("grabbit-macos-{arch}"),
        _ => return None,
    })
}

/// Su Linux l'aggiornamento in-place funziona solo da AppImage.
pub fn supported() -> bool {
    if cfg!(target_os = "linux") {
        std::env::var_os("APPIMAGE").is_some()
    } else {
        true
    }
}

pub fn check() -> Result<Option<Release>> {
    let Some(repo) = repo() else { return Ok(None) };
    let v = match net::get_json(&format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    )) {
        Ok(v) => v,
        Err(e) => {
            // 404 = il repository non ha ancora nessuna release
            let not_found = e.chain().any(|c| {
                matches!(
                    c.downcast_ref::<ureq::Error>(),
                    Some(ureq::Error::StatusCode(404))
                )
            });
            return if not_found { Ok(None) } else { Err(e) };
        }
    };
    parse_release(&v, CURRENT)
}

/// Note da mostrare nell'app: tolgono la riga di credito che sta solo nella pagina GitHub.
fn clean_notes(body: &str) -> String {
    body.lines()
        .filter(|l| !l.contains("è stato scritto con l'aiuto di"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Confronta la release GitHub con la versione in uso.
fn parse_release(v: &serde_json::Value, current: &str) -> Result<Option<Release>> {
    let tag = v["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("release senza tag"))?;
    let latest = semver::Version::parse(tag.trim_start_matches('v'))?;
    if latest <= semver::Version::parse(current)? {
        return Ok(None);
    }
    let want = asset_name();
    let asset_url = v["assets"]
        .as_array()
        .and_then(|list| list.iter().find(|a| a["name"].as_str() == want.as_deref()))
        .and_then(|a| a["browser_download_url"].as_str())
        .map(String::from);
    Ok(Some(Release {
        version: latest.to_string(),
        notes: clean_notes(v["body"].as_str().unwrap_or("")),
        page_url: v["html_url"].as_str().unwrap_or("").to_string(),
        asset_url,
    }))
}

#[cfg(unix)]
fn make_executable(p: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(p)?.permissions();
    perm.set_mode(0o755);
    Ok(fs::set_permissions(p, perm)?)
}
#[cfg(not(unix))]
fn make_executable(_: &std::path::Path) -> Result<()> {
    Ok(())
}

pub fn apply(rel: &Release, status: &mut dyn FnMut(String)) -> Result<()> {
    let url = rel
        .asset_url
        .as_ref()
        .ok_or_else(|| anyhow!("la release non contiene il file per questo sistema"))?;
    let name = asset_name().ok_or_else(|| anyhow!("sistema non supportato"))?;
    let tmp = tools::data_dir().join("update").join(name);
    net::download(url, &tmp, &mut |d, t| {
        let p = t.filter(|t| *t > 0).map(|t| d * 100 / t).unwrap_or(0);
        status(format!("Scarico Grabbit {}… {p}%", rel.version));
    })?;
    make_executable(&tmp)?;

    #[cfg(target_os = "linux")]
    {
        let appimage = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("l'aggiornamento automatico richiede la versione AppImage"))?;
        // si copia accanto all'AppImage e poi si rinomina (atomico, anche se in uso)
        let staged = appimage.with_extension("new");
        fs::copy(&tmp, &staged)?;
        make_executable(&staged)?;
        fs::rename(&staged, &appimage)?;
        let _ = fs::remove_file(&tmp);
    }
    #[cfg(not(target_os = "linux"))]
    {
        self_replace::self_replace(&tmp)?;
        let _ = fs::remove_file(&tmp);
    }
    Ok(())
}

/// Riavvia l'app (dopo un aggiornamento).
pub fn restart() -> ! {
    let exe: Option<PathBuf> = if cfg!(target_os = "linux") {
        std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
    } else {
        std::env::current_exe().ok()
    };
    if let Some(e) = exe {
        let _ = std::process::Command::new(e).spawn();
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release_json(tag: &str) -> serde_json::Value {
        json!({
            "tag_name": tag, "body": "novità", "html_url": "https://example.org/r",
            "assets": [
                {"name": "qualcosa-altro", "browser_download_url": "https://example.org/other"},
                {"name": asset_name().unwrap(), "browser_download_url": "https://example.org/asset"}
            ]
        })
    }

    #[test]
    fn newer_release_picks_platform_asset() {
        let r = parse_release(&release_json("v9.9.9"), "0.1.0")
            .unwrap()
            .unwrap();
        assert_eq!(r.version, "9.9.9");
        assert_eq!(r.asset_url.as_deref(), Some("https://example.org/asset"));
        assert_eq!(r.notes, "novità");
    }

    #[test]
    fn credit_line_is_hidden_in_app_notes() {
        let body = "Questo programma è stato scritto con l'aiuto di [Claude](https://claude.ai), l'assistente AI di Anthropic.\n\n## Cosa cambia\n* Tema scuro";
        assert_eq!(clean_notes(body), "## Cosa cambia\n* Tema scuro");
        assert_eq!(
            clean_notes("solo credito: è stato scritto con l'aiuto di X"),
            ""
        );
        assert_eq!(clean_notes("novità"), "novità");
    }

    #[test]
    fn same_or_older_release_is_ignored() {
        assert!(parse_release(&release_json("v0.1.0"), "0.1.0")
            .unwrap()
            .is_none());
        assert!(parse_release(&release_json("v0.0.9"), "0.1.0")
            .unwrap()
            .is_none());
        assert!(parse_release(&release_json("0.1.1"), "0.1.0")
            .unwrap()
            .is_some()); // tag senza "v"
        assert!(parse_release(&release_json("v0.10.0"), "0.9.0")
            .unwrap()
            .is_some()); // non è un confronto di stringhe
    }

    #[test]
    fn release_without_matching_asset() {
        let v = json!({"tag_name": "v2.0.0", "assets": []});
        let r = parse_release(&v, "0.1.0").unwrap().unwrap();
        assert!(r.asset_url.is_none());
    }

    #[test]
    fn placeholder_repository_disables_checks() {
        assert!(repo().is_none()); // finché in Cargo.toml c'è OWNER
    }

    /// GRABBIT_TEST_ASSET_URL=http://127.0.0.1:8767/grabbit-linux-x86_64.AppImage cargo test apply_ -- --ignored
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn apply_replaces_appimage() {
        use std::os::unix::fs::PermissionsExt;
        let Ok(url) = std::env::var("GRABBIT_TEST_ASSET_URL") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("grabbit-upd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("Grabbit.AppImage");
        fs::write(&fake, b"VECCHIA").unwrap();
        std::env::set_var("APPIMAGE", &fake);
        let rel = Release {
            version: "9.9.9".into(),
            notes: String::new(),
            page_url: String::new(),
            asset_url: Some(url),
        };
        let mut msgs = Vec::new();
        apply(&rel, &mut |s| msgs.push(s)).unwrap();
        assert_eq!(fs::read(&fake).unwrap(), b"NUOVA-VERSIONE");
        assert_eq!(
            fs::metadata(&fake).unwrap().permissions().mode() & 0o111,
            0o111
        );
        assert!(
            !dir.join("Grabbit.new").exists(),
            "file temporaneo lasciato in giro"
        );
        assert!(!msgs.is_empty());
    }
}
