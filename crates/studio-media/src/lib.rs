//! 音频工具：PCM 切块、拼接、WAV 封装。
//!
//! TTS 场景下引擎一次可能吐出几百 KB 的 PCM，直接下发会让客户端首包延迟变大；
//! 这里按固定时长切块，既平滑又便于流式发送。

/// 按固定时长切分 PCM（16bit 小端）。
///
/// 返回的每个块都是完整采样边界，不会截断采样。
pub fn chunk_pcm(pcm: &[u8], sample_rate: u32, channels: u16, frame_ms: u32) -> Vec<Vec<u8>> {
    if pcm.is_empty() {
        return Vec::new();
    }

    let channels = channels.max(1) as usize;
    let bytes_per_sample = 2 * channels;
    let frame_samples = (sample_rate as usize * frame_ms as usize / 1000).max(1);
    let frame_bytes = frame_samples * bytes_per_sample;

    pcm.chunks(frame_bytes)
        .map(|chunk| {
            // 对齐到采样边界，避免破坏 16bit 采样
            let aligned = chunk.len() / bytes_per_sample * bytes_per_sample;
            chunk[..aligned].to_vec()
        })
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

/// 拼接 PCM 块。
pub fn concat_pcm(chunks: &[Vec<u8>]) -> Vec<u8> {
    let total: usize = chunks.iter().map(|c| c.len()).sum();
    let mut out = Vec::with_capacity(total);
    for chunk in chunks {
        out.extend_from_slice(chunk);
    }
    out
}

/// 生成单声道 16bit WAV 文件内容（用于调试与测试）。
pub fn wav_mono16(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);

    out
}

/// 估算 PCM 时长（毫秒）。
pub fn duration_ms(pcm_len: usize, sample_rate: u32, channels: u16) -> f64 {
    if sample_rate == 0 {
        return 0.0;
    }
    let bytes_per_sample = 2 * channels.max(1) as u64;
    let samples = pcm_len as u64 / bytes_per_sample;
    samples as f64 * 1000.0 / sample_rate as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_are_sample_aligned() {
        // 16000Hz、20ms = 320 采样 = 640 字节
        let pcm = vec![1u8; 640 * 2 + 100];
        let chunks = chunk_pcm(&pcm, 16_000, 1, 20);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 640);
        assert_eq!(chunks[1].len(), 640);
        assert_eq!(chunks[2].len(), 100);
    }

    #[test]
    fn empty_input_produces_no_chunks() {
        assert!(chunk_pcm(&[], 16_000, 1, 20).is_empty());
    }

    #[test]
    fn concat_roundtrip() {
        let chunks = vec![vec![1u8, 2], vec![3u8, 4, 5, 6]];
        assert_eq!(concat_pcm(&chunks), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn wav_header_size() {
        let wav = wav_mono16(&[0u8; 100], 22_050);
        assert_eq!(wav.len(), 144);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
    }

    #[test]
    fn duration_math() {
        // 16000Hz 单声道 16bit：32000 字节 = 1000ms
        assert!((duration_ms(32_000, 16_000, 1) - 1000.0).abs() < 0.001);
    }
}
