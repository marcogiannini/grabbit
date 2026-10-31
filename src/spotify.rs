//! Supporto ai link Spotify.
//!
//! Spotify protegge l'audio con DRM e yt-dlp non lo scarica. Come fanno altri
//! strumenti open source, Grabbit legge soltanto i **metadati pubblici**
//! (titolo e artista) dalla pagina "embed" di Spotify e poi cerca la
//! corrispondenza su YouTube, da cui scarica l'audio. Nessun DRM viene aggirato.
//!
//! Limite: la pagina embed pubblica mostra al massimo ~100 brani per playlist.
use crate::net;
use anyhow::{anyhow, bail, Result};
use serde_json::Value;

pub struct Track {
    pub title: String,
    pub artist: String,
}

pub struct Collection {
    pub name: String,
    pub tracks: Vec<Track>,
}

pub fn is_spotify(s: &str) -> bool {
    let s = s.trim();
    s.contains("open.spotify.com/") || s.starts_with("spotify:")
}

/// Estrae (tipo, id) da `https://open.spotify.com/[intl-it/]track/ID?si=…`
/// oppure da `spotify:track:ID`.
fn parse(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("spotify:") {
        let mut p = rest.split(':');
        return Some((p.next()?.to_string(), p.next()?.to_string()));
    }
    let after = s.split("open.spotify.com/").nth(1)?;
    let path = after.split(['?', '#']).next()?;
    let segs: Vec<&str> = path.split('/').filter(|x| !x.is_empty()).collect();
    let idx = segs
        .iter()
        .position(|x| matches!(*x, "track" | "album" | "playlist"))?;
    Some((segs[idx].to_string(), segs.get(idx + 1)?.to_string()))
}

fn next_data(html: &str) -> Result<Value> {
    let i = html
        .find("id=\"__NEXT_DATA__\"")
        .ok_or_else(|| anyhow!("pagina Spotify in un formato inatteso"))?;
    let rest = &html[i..];
    let start = rest.find('>').ok_or_else(|| anyhow!("JSON non trovato"))? + 1;
    let len = rest[start..]
        .find("</script>")
        .ok_or_else(|| anyhow!("JSON non terminato"))?;
    Ok(serde_json::from_str(&rest[start..start + len])?)
}

fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m
            .get(key)
            .or_else(|| m.values().find_map(|c| find_key(c, key))),
        Value::Array(a) => a.iter().find_map(|c| find_key(c, key)),
        _ => None,
    }
}

fn text<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
        .filter(|s| !s.trim().is_empty())
}

fn clean(s: &str) -> String {
    s.replace('\u{a0}', " ").trim().to_string()
}

pub fn fetch(input: &str) -> Result<Collection> {
    let (kind, id) = parse(input).ok_or_else(|| anyhow!("link Spotify non riconosciuto"))?;
    if !matches!(kind.as_str(), "track" | "album" | "playlist") {
        bail!("tipo di link Spotify non supportato: {kind}");
    }
    let html = net::get_text(&format!("https://open.spotify.com/embed/{kind}/{id}"))?;
    let json = next_data(&html)?;
    let entity = find_key(&json, "entity").ok_or_else(|| anyhow!("dati Spotify non trovati"))?;
    let name = text(entity, &["name", "title"])
        .unwrap_or("Spotify")
        .to_string();

    // album / playlist
    if let Some(list) = entity.get("trackList").and_then(|l| l.as_array()) {
        let tracks: Vec<Track> = list
            .iter()
            .filter_map(|t| {
                Some(Track {
                    title: clean(text(t, &["title", "name"])?),
                    artist: clean(text(t, &["subtitle"]).unwrap_or("")),
                })
            })
            .collect();
        if tracks.is_empty() {
            bail!("nessun brano trovato (playlist privata o vuota?)");
        }
        return Ok(Collection { name, tracks });
    }

    // brano singolo
    let artist = entity
        .get("artists")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| text(x, &["name"]))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Ok(Collection {
        tracks: vec![Track {
            title: clean(&name),
            artist: clean(&artist),
        }],
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links() {
        assert_eq!(
            parse("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc"),
            Some(("track".into(), "4uLU6hMCjMI75M1A2tKUQC".into()))
        );
        assert_eq!(
            parse("https://open.spotify.com/intl-it/album/1DFixLWuPkv3KT3TnV35m3"),
            Some(("album".into(), "1DFixLWuPkv3KT3TnV35m3".into()))
        );
        assert_eq!(
            parse("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
            Some(("playlist".into(), "37i9dQZF1DXcBWIGoYBM5M".into()))
        );
        assert_eq!(parse("https://www.youtube.com/watch?v=x"), None);
        assert!(is_spotify("https://open.spotify.com/track/x"));
        assert!(!is_spotify("https://youtu.be/x"));
    }

    #[test]
    fn extracts_next_data() {
        let html = r#"<html><script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"name":"Song","artists":[{"name":"A"},{"name":"B"}]}}}}}}</script></html>"#;
        let json = next_data(html).unwrap();
        let e = find_key(&json, "entity").unwrap();
        assert_eq!(text(e, &["name"]), Some("Song"));
    }
}
