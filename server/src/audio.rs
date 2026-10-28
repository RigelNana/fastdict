//! Browser playback for legacy Ogg/Speex resources. Original MDD bytes stay intact.
use anyhow::{Context, Result, bail, ensure};
use axum::body::Bytes;
use lru::LruCache;
use std::{fs, num::NonZeroUsize, process::{Command, Stdio}, time::{Duration, Instant}};

const CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_INPUT: usize = 2 * 1024 * 1024;
const MAX_OUTPUT: u64 = 8 * 1024 * 1024;

pub struct AudioCache { entries: LruCache<blake3::Hash, Bytes>, bytes: usize }
impl Default for AudioCache {
    fn default() -> Self { Self { entries: LruCache::new(NonZeroUsize::new(64).unwrap()), bytes: 0 } }
}
impl AudioCache {
    // Call on the blocking pool, under the app's audio mutex. This bounds decoder
    // concurrency without delaying ordinary MP3, CSS or dictionary requests.
    pub fn decode(&mut self, source: &[u8]) -> Result<Bytes> {
        ensure!(source.len() <= MAX_INPUT, "Speex 音频超过 2 MiB 转换限制");
        let key = blake3::hash(source);
        if let Some(bytes) = self.entries.get(&key) { return Ok(bytes.clone()); }
        let bytes = decode(source)?;
        while self.bytes + bytes.len() > CACHE_BYTES || self.entries.len() == self.entries.cap().get() {
            if let Some((_, old)) = self.entries.pop_lru() { self.bytes -= old.len(); } else { break; }
        }
        self.bytes += bytes.len();self.entries.put(key, bytes.clone());Ok(bytes)
    }
}
pub fn is_speex(bytes: &[u8]) -> bool {
    // Ogg's first page contains the codec identification packet. Detect its
    // actual content, including dictionaries that call their Speex files .ogg.
    bytes.starts_with(b"OggS") && bytes.get(..128.min(bytes.len())).is_some_and(|header| header.windows(8).any(|part| part == b"Speex   "))
}
fn decode(source: &[u8]) -> Result<Bytes> {
    ensure!(is_speex(source), "不是有效的 Speex 音频");
    let dir = tempfile::tempdir()?;
    let input = dir.path().join("source.spx");let output = dir.path().join("decoded.wav");
    fs::write(&input, source)?;
    let mut child = Command::new("speexdec").arg(&input).arg(&output)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        .context("服务缺少 Speex 音频解码器（speexdec）")?;
    let start = Instant::now();
    let status = loop {
        if start.elapsed() > Duration::from_secs(5) || fs::metadata(&output).is_ok_and(|m| m.len() > MAX_OUTPUT) {
            let _ = child.kill();let _ = child.wait();bail!("Speex 音频转换超出限制");
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => { let _ = child.kill();let _ = child.wait();return Err(error.into()); }
        }
    };
    ensure!(status.success(), "Speex 音频转换失败");
    ensure!(fs::metadata(&output)?.len() <= MAX_OUTPUT, "转换后音频过大");
    let wav = fs::read(output)?;
    ensure!(wav.len() > 44 && wav.starts_with(b"RIFF") && &wav[8..12] == b"WAVE", "Speex 音频转换结果无效");
    Ok(Bytes::from(wav))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn speex_is_decoded_and_cached_without_changing_the_source() {
        let source = include_bytes!("../tests/fixtures/tone.spx");
        assert!(is_speex(source));assert!(!is_speex(b"OggS Vorbis"));
        let mut cache = AudioCache::default();
        let wav = cache.decode(source).unwrap();
        assert!(wav.starts_with(b"RIFF"));assert!(wav.len() > source.len());
        let again = cache.decode(source).unwrap();assert_eq!(wav.as_ptr(), again.as_ptr());
        assert_eq!(cache.entries.len(), 1);assert_eq!(cache.bytes, wav.len());
        assert!(cache.decode(b"not audio").is_err());
        assert!(is_speex(source));
    }
}
