# Grabbit

Scarica **video** e **musica** da YouTube e Spotify con un'interfaccia semplice.
Scritto in Rust (leggero: l'AppImage pesa circa 4 MB), si appoggia a
[yt-dlp](https://github.com/yt-dlp/yt-dlp) e funziona su Windows, Linux e macOS.

## Cosa fa

- **Video**: MP4 / MKV / WebM, qualità massima a scelta (fino a 4K).
- **Audio**: MP3, M4A (AAC), Opus, OGG Vorbis, FLAC, WAV, con scelta del **bitrate**.
- Cartella di destinazione a scelta, più link in un colpo solo (uno per riga), playlist.
- Copertina e metadati incorporati nei file.
- Coda con avanzamento in tempo reale e pulsante **Annulla**.
- **Si prepara da solo**: al primo avvio scarica yt-dlp, ffmpeg e Deno
  (il runtime JavaScript che yt-dlp richiede per YouTube); a ogni avvio
  controlla se esiste un yt-dlp più recente e lo aggiorna.
- **Si aggiorna da solo**: a ogni avvio guarda l'ultima release GitHub di
  questo repository e, se è più recente, installa la nuova versione.

### Come funziona Spotify

Spotify cifra l'audio (DRM) e yt-dlp non lo scarica. Grabbit legge soltanto i
**dati pubblici** del link (titolo e artista di brano, album o playlist), poi
**cerca lo stesso brano su YouTube** e scarica l'audio da lì. Quindi:

- il risultato è l'audio della versione YouTube, non il file originale Spotify;
- una playlist può mostrare al massimo circa 100 brani;
- se Spotify cambia la struttura delle sue pagine pubbliche, la lettura dei link
  può smettere di funzionare finché non si aggiorna Grabbit.

> Usa Grabbit solo per contenuti che hai il diritto di scaricare e nel rispetto
> del copyright e dei termini di servizio delle piattaforme.

## Installazione

| Sistema | File dalla pagina *Releases* | Note |
|---|---|---|
| Windows | `grabbit-windows-x86_64.exe` | doppio clic; SmartScreen può avvisare perché l'eseguibile non è firmato |
| Linux | `grabbit-linux-x86_64.AppImage` | `chmod +x` e doppio clic (l'auto-aggiornamento funziona solo da AppImage) |
| macOS | `Grabbit-macos-aarch64.app.zip` (Apple Silicon) oppure `…-x86_64.app.zip` (Intel) | la prima volta: tasto destro → Apri, perché l'app non è notarizzata |

Dati e strumenti scaricati stanno in:
Windows `%APPDATA%\Grabbit\Grabbit\data`, Linux `~/.local/share/grabbit`,
macOS `~/Library/Application Support/org.Grabbit.Grabbit`.

## Pubblicare una nuova versione (e farla arrivare agli utenti)

1. In `Cargo.toml` imposta `repository = "https://github.com/TUO-UTENTE/grabbit"`
   (una volta sola: da qui l'app sa dove cercare gli aggiornamenti).
2. Per ogni release: alza `version` in `Cargo.toml` (es. `0.2.0`), fai commit, poi
   ```bash
   git tag v0.2.0
   git push --tags
   ```
3. La GitHub Action `.github/workflows/release.yml` compila tutto e allega alla
   release questi file, **con esattamente questi nomi** (li cerca l'updater):

   - `grabbit-windows-x86_64.exe`
   - `grabbit-linux-x86_64.AppImage` (e `-aarch64`)
   - `grabbit-macos-aarch64` e `grabbit-macos-x86_64` (binari nudi, per l'aggiornamento)
   - `Grabbit-macos-*.app.zip` (per la prima installazione su Mac)

4. Al successivo avvio le app installate trovano la release e si aggiornano
   (nelle opzioni avanzate si può scegliere tra aggiornamento automatico e a conferma).

Il tag deve coincidere con la versione in `Cargo.toml`: la CI lo verifica.

## Ottenere l'eseguibile Windows

- **Senza installare nulla**: su GitHub vai in *Actions → Windows exe → Run workflow*;
  dopo qualche minuto scarichi `grabbit-windows` da *Artifacts*.
- **Sul tuo PC Windows**: installa Rust da <https://rustup.rs> e fai doppio clic su
  `packaging\windows\build.bat` (crea `grabbit-windows-x86_64.exe`).

## Compilare da sorgente

```bash
cargo run --release                      # avvio
cargo test                               # test unitari
packaging/linux/build-appimage.sh        # crea l'AppImage (Linux)
python3 scripts/make_icon.py             # rigenera le icone (serve Pillow)
```

Serve Rust 1.85+. Su Linux non servono librerie di sviluppo particolari.

Prove con yt-dlp reale (facoltative):

```bash
GRABBIT_YTDLP=/percorso/yt-dlp cargo test -- --ignored
```

## Struttura

| File | Compito |
|---|---|
| `src/app.rs` | interfaccia grafica (egui) |
| `src/downloader.rs` | costruisce i comandi yt-dlp, legge l'avanzamento, annulla |
| `src/tools.rs` | scarica/aggiorna yt-dlp, Deno, ffmpeg |
| `src/spotify.rs` | legge titolo/artista dai link Spotify |
| `src/selfupdate.rs` | aggiornamento automatico dell'app |
| `src/config.rs` | impostazioni salvate |

## Problemi comuni

- **«Sign in to confirm you're not a bot» / video con restrizioni**: in
  *Opzioni avanzate* scegli il browser da cui prendere i cookie.
- **macOS**: ffmpeg viene scaricato in versione Intel (su Apple Silicon usa
  Rosetta); se hai Homebrew (`brew install ffmpeg`) viene usato quello.
- **Linux senza AppImage** (binario avviato a mano): funziona, ma l'app non
  può sostituire se stessa; mostra solo il link alla nuova release.

## Licenza

MIT, vedi `LICENSE`.
