# AI-SubTrans

**English** | [中文](./README_ZH.md)

<p align="center">
  <img src="docs/卡通Q版鹦鹉吉祥物-512.png" width="160" alt="AI-SubTrans mascot">
</p>

> Your personal AI subtitle assistant: open a video → extract subtitles → one-click translate → merge back into the video, all in one app.

[![Windows 10+](https://img.shields.io/badge/Windows-10+-0078D6)](https://www.microsoft.com/windows)
[![macOS 11+](https://img.shields.io/badge/macOS-11+-000000)](https://www.apple.com/macos)
[![Version](https://img.shields.io/badge/version-1.0.5-green)](https://github.com/terry2010/polly-subtitle-translator/releases)
[![License](https://img.shields.io/badge/License-Apache--2.0-blue)](./LICENSE)

## Table of Contents

- [Who is it for](#who-is-it-for)
- [Subtitles in 3 Steps](#subtitles-in-3-steps)
- [Download & Install](#download--install)
- [Try it for Free](#try-it-for-free)
- [Features](#features)
- [Quick Start](#quick-start)
- [Translation Services](#translation-services)
- [Privacy & Security](#privacy--security)
- [FAQ](#faq)
- [For Developers](#for-developers)
- [License](#license)
- [Acknowledgements](#acknowledgements)

---

## Who is it for

- Watching TV shows, movies, or raw tutorials without satisfactory Chinese subtitles
- Learning a foreign language by comparing original and translated text side by side
- Needing to translate, re-time, or export bilingual subtitles you downloaded
- Wanting to merge subtitles directly back into a video for TV or cloud-drive playback
- Subtitle groups or volunteer translators who need batch processing and consistent character-name translation

## Subtitles in 3 Steps

1. Open a video or subtitle file
2. Extract subtitles and translate with one click
3. Proofread, then export or merge back into the video

> Screenshot placeholder: main UI / translating / playback preview / context menu — to be added.

---

## Download & Install

Go to [Releases](https://github.com/terry2010/polly-subtitle-translator/releases) and download the installer for your platform:

| Platform | Installer |
|---|---|
| Windows 10+ | `AI-SubTrans_1.0.5_x64-setup.exe` |
| macOS 11+ (Apple Silicon) | `AI-SubTrans_1.0.5_aarch64.dmg` |
| macOS 11+ (Intel) | `AI-SubTrans_1.0.5_x64.dmg` |

Launch after installation — the app checks for updates automatically.

> **macOS "cannot verify developer"**: This project does not have an Apple Developer certificate. On first launch, right-click the app → "Open", or go to "System Settings → Privacy & Security" and click "Open Anyway".

> **Windows SmartScreen warning**: Click "More info → Run anyway". The installer is signed with an Ed25519 key; in-app auto-update verifies the signature.

> **macOS note**: macOS builds are available, but the primary testing environment is Windows. System integrations such as context menus are not yet available on macOS; some details are still being refined. Feedback is welcome.

> **Why the warnings?**: Because the author can't afford a code-signing certificate QAQ

### Try it for Free

- **Completely free**: Groq, Ollama, LM Studio (run locally, no cost)
- **Free tier available**: Zhipu GLM-4.7-Flash, SiliconFlow, Hunyuan, ERNIE Bot, Gemini, etc.
- Other services require an API key from their respective websites

---

## Features

### Extract subtitles from video
- Supports mkv, mp4, avi, mov, wmv, flv, ts, m2ts and other common formats
- Auto-lists all subtitle streams in the video, preferring English SDH / plain English
- Automatically skips PGS/VOBSUB and other image-based subtitles to avoid garbled output
- FFmpeg is auto-downloaded on first use — no manual setup required

### One-click translation
- 12 traditional translators: Baidu, Bing, Google, DeepL, Youdao, Caiyun, Niutrans, Tencent, Volcano, Alibaba, Amazon
- 16+ AI large models: OpenAI, Azure OpenAI, DeepSeek, Zhipu GLM, SiliconFlow, Groq, Qwen, Doubao, Hunyuan, Yi, Kimi, ERNIE Bot, Gemini, plus local Ollama / LM Studio / custom endpoints
- Automatic rate limiting, retry on failure, and translation caching to avoid paying twice
- Character-name precision translation (developer mode): auto-scans character names and unifies translations for consistency
- Real-time translation speed and ETA display; cancelable at any time

### Subtitle editor
- Table layout with original / translated text side by side; add/delete rows, find & replace, undo/redo
- Auto-detects bilingual subtitles and splits them into two columns
- Global timeline offset for easy re-timing
- Export to srt / ass / vtt with monolingual, bilingual, and ASS style preview options
- Virtual scrolling keeps things smooth even with tens of thousands of entries

### Watch while proofreading
- Built-in libmpv player with hardware decoding, speed control, volume, and audio-track switching
- Subtitles don't overlay the video — they highlight in a panel below, synced to playback for line-by-line proofreading
- HDR / Dolby Vision videos are detected and flagged automatically

### No embedded subtitles? Search online
- Three sources: OpenSubtitles, SubHD, Zimuku
- OpenSubtitles requires your own API key; SubHD / Zimuku need no key
- Auto-simplifies filenames to improve search hit rate
- Downloaded non-Chinese subtitles can continue through the translation pipeline

### Merge back into video
- After translation, merge subtitles back into the video with one click
- By default merges directly onto the original video (ensure enough disk space); can also save as `*.merged.mkv`
- No re-encoding — completes in seconds

### More niceties
- **Auto-update**: silent check on launch, popup on new version, auto download & install after confirmation
- **Windows context menus**: right-click a video → "AI-SubTrans Quick Translate"; right-click a subtitle → "AI-SubTrans Edit Subtitle"; right-click a folder → "AI-SubTrans Batch Translate"
- **Bilingual UI** (Chinese / English), **light / dark / follow-system** themes
- **Translation proxy**: configure HTTP/SOCKS5 proxy specifically for translation requests (for services like Google that require VPN)
- **Crash diagnostics**: writes a local crash log on abnormal exit; viewable and clearable in Settings

---

## Quick Start

### Scenario 1: Add Chinese subtitles to a foreign-language video

1. Launch AI-SubTrans, click "Open Video" or drag a video into the window.
2. On first use you'll be prompted to download FFmpeg; reopen the video after it finishes.
3. Select a subtitle stream on the left, click "Extract Subtitle".
4. Switch to the "Translate" tab on the right, pick an engine and language (set up the API Key in "Settings → Translation" first), click "Start Translate".
5. Proofread in the table — adjust timeline, find & replace, add/delete rows as needed.
6. Click "Play" to watch the video and check subtitles against it.
7. Click "Export" to save the subtitle file, or "Merge to Video" to produce a video with embedded subtitles.

### Scenario 2: Translate an existing subtitle file

1. Drag an `.srt` / `.ass` / `.vtt` file into the window, or click "Open Subtitle".
2. Bilingual subtitles are auto-detected and split into original / translated columns.
3. Choose a translation engine and click translate.
4. Proofread and export.

### Scenario 3: Windows right-click one-click processing

1. Configure translation services in "Settings → Translation" first.
2. After installation, right-click a video file in File Explorer → "AI-SubTrans Quick Translate".
3. The app runs "extract → translate → merge" in the background.
4. A system notification pops up when done — click it to return to the main UI.

> You can also right-click a subtitle file → "AI-SubTrans Edit Subtitle" to jump straight into the editor, or right-click a folder → "AI-SubTrans Batch Translate" to add it to batch processing.

---

## Translation Services

AI-SubTrans supports 12 traditional translators + 16+ AI large models. **You need to fill in the API Key for each service in "Settings → Translation" before use** (some services offer free tiers — see [Try it for Free](#try-it-for-free) above):

- **Traditional translators**: Baidu, Bing, Google, DeepL, Youdao, Caiyun, Niutrans, Tencent, Volcano, Alibaba, Amazon
- **AI large models** (OpenAI-compatible interface): OpenAI, Azure OpenAI, DeepSeek, Zhipu GLM, SiliconFlow, Groq, Qwen, Doubao, Hunyuan, Yi, Kimi, ERNIE Bot, Gemini, Ollama (local), LM Studio (local), custom OpenAI-compatible endpoints

AI services support multi-instance configuration (multiple API Keys / BaseUrls for the same service). See each provider's website for how to apply.

---

## Privacy & Security

- **Your files are never uploaded**: video and subtitles are processed locally; only subtitle text is sent to the translation service you configured.
- **Sensitive data stored locally**: API keys, proxy passwords, etc. are kept in a local database and never uploaded to any server.
- **Auto-update only sends a version number**: update checks request only the version manifest — no user data is transmitted.
- **Crash logs stay local**: crash info is written to local disk only, never auto-uploaded; you must submit it to the developer manually.
- **On-demand component download**: FFmpeg and libmpv are downloaded from GitHub on first use (with gh-proxy acceleration for users in China); can be deleted and re-downloaded anytime in Settings.

---

## FAQ

**Q: Opening a video prompts me to download FFmpeg?**
A: FFmpeg is the underlying tool for subtitle extraction and merging. It's auto-downloaded on first use. If the network is poor, you can manually switch the download source or retry in Settings.

**Q: Translation shows "insufficient balance"?**
A: Your translation service account has run out of quota — top up on the provider's website. The app does not auto-retry and waste your quota.

**Q: Google / Gemini etc. won't connect?**
A: These services may require a proxy in mainland China. Configure an HTTP/SOCKS5 proxy in "Settings → Advanced" and check "Route translation requests through proxy".

**Q: Character names are inconsistent after translation?**
A: Tap the version number 7 times in "Settings → About" to enable developer mode, then turn on "Character-name precision translation". It auto-scans and unifies character names before translating.

**Q: Where is the merged video?**
A: By default it merges directly onto the original video file (no new file generated), so make sure the disk has enough space. If space is insufficient, you'll be prompted to save as `*.merged.mkv` instead.

**Q: macOS won't open the app?**
A: No Apple Developer certificate was purchased. Right-click the app → "Open" on first launch; or go to "System Settings → Privacy & Security" and click "Open Anyway".

---

## Feedback & Help

- Report bugs or suggestions: [GitHub Issues](https://github.com/terry2010/polly-subtitle-translator/issues)
- View changelog: [Releases](https://github.com/terry2010/polly-subtitle-translator/releases)
- In-app "Settings → About" shows the current version, manual update check, and crash log management.

---

## For Developers

This project is built with Tauri 2 + React 18 + Rust. To build from source:

```bash
# Prerequisites: Node.js 18+, Rust 1.89+, system deps per Tauri docs
npm install
npm run start          # dev mode
npm run build:release  # production build
```

For detailed architecture, IPC interfaces, and module docs, see [docs/开发文档-开发者版.md](./docs/开发文档-开发者版.md) (developer-oriented, not a user guide).

---

## License

AI-SubTrans is open-sourced under [Apache-2.0](./LICENSE).

- Main program: Apache-2.0
- libmpv: LGPL-2.1+ (dynamically loaded at runtime)
- FFmpeg: GPL-2.0+ (invoked as a separate subprocess, does not affect the main program license)

---

## Acknowledgements

- [Tauri](https://tauri.app) — cross-platform desktop framework
- [libmpv](https://mpv.io) / [mpv-winbuild](https://github.com/zhongfly/mpv-winbuild) / [libmpv-darwin-build](https://github.com/media-kit/libmpv-darwin-build) — video playback
- [FFmpeg](https://ffmpeg.org) / [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) — subtitle extraction & merging
- [shadcn/ui](https://ui.shadcn.com) / [Radix UI](https://www.radix-ui.com) / [Tailwind CSS](https://tailwindcss.com) — UI components & styling
- [OpenSubtitles](https://www.opensubtitles.org) / SubHD / Zimuku — online subtitle sources
