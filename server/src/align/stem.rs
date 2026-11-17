//! STEM emissions file decode (ghost-story / Storyteller emit format).
//!
//! Ports `@storyteller-platform/ghost-story` encoding: magic `STEM`, versions
//! 1/2/3, dense float16 rows and sparse top-K.

use std::collections::HashMap;
use std::path::Path;

use super::ctc::{EmissionTimings, Emissions};

const MAGIC: &[u8; 4] = b"STEM";
const VERSION_1_VOCAB_OFFSET: usize = 44;
const OFFSET_MAGIC: usize = 0;
const OFFSET_VERSION: usize = 4;
const OFFSET_ENCODING: usize = 6;
const OFFSET_VOCAB_SIZE: usize = 8;
const OFFSET_NUM_FRAMES: usize = 12;
const OFFSET_BLANK_INDEX: usize = 16;
const OFFSET_SECONDS_PER_CHUNK: usize = 20;
const OFFSET_FRAMES_PER_CHUNK: usize = 28;
const OFFSET_SECONDS_PER_FRAME: usize = 32;
const OFFSET_FRAMES_OFFSET: usize = 40;
const OFFSET_START_OFFSET_SECONDS: usize = 44;
const OFFSET_END_OFFSET_SECONDS: usize = 48;
const OFFSET_TOKENIZER: usize = 52;
const OFFSET_TOP_K: usize = 54;
const OFFSET_UNK_ID: usize = 56;
const OFFSET_VOCAB: usize = 60;

const ENCODING_DENSE: u16 = 1;
const ENCODING_SPARSE: u16 = 2;
pub const TOKENIZER_NONE: u16 = 0;
pub const TOKENIZER_UNIGRAM: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StemEncoding {
    Dense,
    Sparse,
}

#[derive(Debug, Clone)]
pub struct StemHeader {
    pub version: u16,
    pub vocab: HashMap<String, i32>,
    pub blank_index: i32,
    pub classes: usize,
    pub num_frames: usize,
    pub timings: EmissionTimings,
    pub encoding: StemEncoding,
    /// STEM tokenizer kind: 0 = none (char MMS/quartznet), 1 = UNIGRAM (unsupported by align).
    pub tokenizer_kind: u16,
    pub top_k: usize,
    pub frames_offset: usize,
    pub bytes_per_frame: usize,
}

/// IEEE binary16 → f32 (no `half` crate).
fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15);
    let exp = u32::from((bits >> 10) & 0x1f);
    let frac = u32::from(bits & 0x3ff);
    if exp == 0 {
        if frac == 0 {
            return f32::from_bits(sign << 31);
        }
        // subnormal → normalize
        let mut e: i32 = -14;
        let mut m = frac;
        while m & 0x400 == 0 {
            m <<= 1;
            e -= 1;
        }
        m &= 0x3ff;
        return f32::from_bits((sign << 31) | (((e + 127) as u32) << 23) | (m << 13));
    }
    if exp == 31 {
        return f32::from_bits((sign << 31) | (0xff << 23) | (frac << 13));
    }
    f32::from_bits((sign << 31) | ((exp + 127 - 15) << 23) | (frac << 13))
}

fn read_u16(bytes: &[u8], off: usize) -> Result<u16, String> {
    let b = bytes
        .get(off..off + 2)
        .ok_or_else(|| format!("STEM truncated at u16 offset {off}"))?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, String> {
    let b = bytes
        .get(off..off + 4)
        .ok_or_else(|| format!("STEM truncated at u32 offset {off}"))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_f32(bytes: &[u8], off: usize) -> Result<f32, String> {
    let b = bytes
        .get(off..off + 4)
        .ok_or_else(|| format!("STEM truncated at f32 offset {off}"))?;
    Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_f64(bytes: &[u8], off: usize) -> Result<f64, String> {
    let b = bytes
        .get(off..off + 8)
        .ok_or_else(|| format!("STEM truncated at f64 offset {off}"))?;
    Ok(f64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

fn read_f16(bytes: &[u8], off: usize) -> Result<f32, String> {
    Ok(f16_to_f32(read_u16(bytes, off)?))
}

pub fn decode_stem_header(bytes: &[u8]) -> Result<StemHeader, String> {
    if bytes.len() < VERSION_1_VOCAB_OFFSET {
        return Err(format!(
            "Failed to decode emissions. Need at least {VERSION_1_VOCAB_OFFSET} bytes for header"
        ));
    }
    if &bytes[OFFSET_MAGIC..OFFSET_VERSION] != MAGIC {
        let magic = String::from_utf8_lossy(&bytes[OFFSET_MAGIC..OFFSET_VERSION]);
        return Err(format!(
            "Failed to decode emissions. Magic bytes decoded to {magic}; expected STEM"
        ));
    }

    let version = read_u16(bytes, OFFSET_VERSION)?;
    if version != 1 && version != 2 && version != 3 {
        return Err(format!(
            "Failed to decode emissions. Found version {version}; expected 1, 2, or 3"
        ));
    }

    let encoding_id = read_u16(bytes, OFFSET_ENCODING)?;
    let encoding = match encoding_id {
        ENCODING_DENSE => StemEncoding::Dense,
        ENCODING_SPARSE => StemEncoding::Sparse,
        other => {
            return Err(format!(
                "Failed to decode emissions. Found encoding {other}; expected 1 or 2"
            ));
        }
    };

    let vocab_size = read_u32(bytes, OFFSET_VOCAB_SIZE)? as usize;
    let num_frames = read_u32(bytes, OFFSET_NUM_FRAMES)? as usize;
    let blank_index = read_u32(bytes, OFFSET_BLANK_INDEX)? as i32;
    let seconds_per_chunk = read_f64(bytes, OFFSET_SECONDS_PER_CHUNK)?;
    let frames_per_chunk = read_u32(bytes, OFFSET_FRAMES_PER_CHUNK)? as usize;
    let seconds_per_frame = read_f64(bytes, OFFSET_SECONDS_PER_FRAME)?;
    let frames_offset = read_u32(bytes, OFFSET_FRAMES_OFFSET)? as usize;

    let mut start_offset_seconds = 0.0f64;
    let mut end_offset_seconds = 0.0f64;
    let mut tokenizer_kind = TOKENIZER_NONE;
    let mut top_k = 0usize;
    let mut vocab_offset = VERSION_1_VOCAB_OFFSET;

    if version >= 2 {
        if bytes.len() < OFFSET_VOCAB {
            return Err("Failed to decode emissions. Truncated v2/v3 header".into());
        }
        if version == 2 {
            // v2 stores a single f64 start offset; end mirrors start.
            start_offset_seconds = read_f64(bytes, OFFSET_START_OFFSET_SECONDS)?;
            end_offset_seconds = start_offset_seconds;
        } else {
            start_offset_seconds = f64::from(read_f32(bytes, OFFSET_START_OFFSET_SECONDS)?);
            end_offset_seconds = f64::from(read_f32(bytes, OFFSET_END_OFFSET_SECONDS)?);
        }
        tokenizer_kind = read_u16(bytes, OFFSET_TOKENIZER)?;
        top_k = read_u16(bytes, OFFSET_TOP_K)? as usize;
        let _unk_id = read_u32(bytes, OFFSET_UNK_ID)?;
        vocab_offset = OFFSET_VOCAB;
    }

    if encoding == StemEncoding::Sparse && top_k == 0 {
        return Err("Failed to decode emissions. Sparse encoding without a top-k".into());
    }

    let scores_offset = if tokenizer_kind == TOKENIZER_NONE {
        frames_offset
    } else {
        frames_offset.saturating_sub(vocab_size * 4)
    };

    if scores_offset < vocab_offset || bytes.len() < scores_offset {
        return Err("Failed to decode emissions. Vocab region truncated".into());
    }

    let vocab_data = &bytes[vocab_offset..scores_offset];
    let mut vocab = HashMap::new();
    let mut o = 0usize;
    while o + 6 <= vocab_data.len() {
        let index = read_u32(vocab_data, o)? as i32;
        let length = read_u16(vocab_data, o + 4)? as usize;
        if o + 6 + length > vocab_data.len() {
            return Err("Failed to decode emissions. Vocab token truncated".into());
        }
        let token = String::from_utf8_lossy(&vocab_data[o + 6..o + 6 + length]).into_owned();
        vocab.insert(token, index);
        o += 6 + length;
    }

    if vocab.len() != vocab_size {
        return Err(format!(
            "Failed to decode emissions. Found vocab with size {}; expected {vocab_size}",
            vocab.len()
        ));
    }

    if tokenizer_kind != TOKENIZER_NONE && tokenizer_kind != TOKENIZER_UNIGRAM {
        return Err(format!(
            "Failed to decode emissions. Found tokenizer {tokenizer_kind}; expected 0 or 1"
        ));
    }
    // Unigram scores sit between vocab and frames; align does not need them yet.

    let bytes_per_frame = match encoding {
        StemEncoding::Sparse => top_k * 4,
        StemEncoding::Dense => vocab_size * 2,
    };

    Ok(StemHeader {
        version,
        vocab,
        blank_index,
        classes: vocab_size,
        num_frames,
        timings: EmissionTimings {
            seconds_per_chunk,
            seconds_per_frame,
            frames_per_chunk,
            start_offset_seconds,
            end_offset_seconds,
        },
        encoding,
        tokenizer_kind,
        top_k,
        frames_offset,
        bytes_per_frame,
    })
}

/// Storyteller `align.ts`: subword STEM vocab is not supported by the CTC aligner.
pub fn reject_subword_tokenizer(tokenizer_kind: u16) -> Result<(), String> {
    if tokenizer_kind == TOKENIZER_NONE {
        return Ok(());
    }
    Err(
        "These emissions come from a model with a subword vocabulary, which the aligner does not support yet. Use the quartznet15x5 or mms model."
            .into(),
    )
}

pub fn frame_byte_range(header: &StemHeader, from: usize, to: usize) -> (usize, usize) {
    let first = from.min(header.num_frames);
    let last = to.max(first).min(header.num_frames);
    (
        header.frames_offset + first * header.bytes_per_frame,
        header.frames_offset + last * header.bytes_per_frame,
    )
}

fn decode_dense_frames(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err(format!(
            "Failed to decode emissions. Dense payload length {} not multiple of 2",
            bytes.len()
        ));
    }
    let count = bytes.len() / 2;
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        values.push(read_f16(bytes, i * 2)?);
    }
    Ok(values)
}

fn decode_sparse_frames(bytes: &[u8], classes: usize, top_k: usize) -> Result<Vec<f32>, String> {
    let frame_bytes = top_k * 4;
    if frame_bytes == 0 || !bytes.len().is_multiple_of(frame_bytes) {
        return Err(format!(
            "Failed to decode emissions. Sparse payload length {} not multiple of {frame_bytes}",
            bytes.len()
        ));
    }
    let frames = bytes.len() / frame_bytes;
    let mut values = vec![0f32; frames * classes];
    for frame in 0..frames {
        let base = frame * frame_bytes;
        let row = frame * classes;
        let floor = read_f16(bytes, base + top_k * 2 + (top_k - 1) * 2)?;
        for c in 0..classes {
            values[row + c] = floor;
        }
        for i in 0..top_k {
            let idx = read_u16(bytes, base + i * 2)? as usize;
            let score = read_f16(bytes, base + top_k * 2 + i * 2)?;
            if idx < classes {
                values[row + idx] = score;
            }
        }
    }
    Ok(values)
}

pub fn decode_stem_frames(header: &StemHeader, frame_bytes: &[u8]) -> Result<Emissions, String> {
    let bpf = header.bytes_per_frame;
    if bpf == 0 || frame_bytes.is_empty() {
        return Ok(Emissions {
            values: Vec::new(),
            frames: 0,
            classes: header.classes,
        });
    }
    if !frame_bytes.len().is_multiple_of(bpf) {
        return Err(format!(
            "Failed to decode emissions. Got {} bytes; expected a multiple of {bpf}",
            frame_bytes.len()
        ));
    }
    let values = match header.encoding {
        StemEncoding::Sparse => decode_sparse_frames(frame_bytes, header.classes, header.top_k)?,
        StemEncoding::Dense => decode_dense_frames(frame_bytes)?,
    };
    Ok(Emissions {
        values,
        frames: frame_bytes.len() / bpf,
        classes: header.classes,
    })
}

pub fn decode_stem_bytes(bytes: &[u8]) -> Result<(StemHeader, Emissions), String> {
    let header = decode_stem_header(bytes)?;
    let (start, end) = frame_byte_range(&header, 0, header.num_frames);
    if end > bytes.len() {
        return Err(format!(
            "Failed to decode emissions. Frames end at {end}; file has {} bytes",
            bytes.len()
        ));
    }
    let emissions = decode_stem_frames(&header, &bytes[start..end])?;
    Ok((header, emissions))
}

pub fn decode_stem_file(path: impl AsRef<Path>) -> Result<(StemHeader, Emissions), String> {
    let bytes = std::fs::read(path.as_ref()).map_err(|e| e.to_string())?;
    decode_stem_bytes(&bytes)
}

/// Decode `[from_frame, to_frame)` from a full STEM file buffer.
pub fn decode_stem_frame_range(
    header: &StemHeader,
    file_bytes: &[u8],
    from_frame: usize,
    to_frame: usize,
) -> Result<Emissions, String> {
    let (start, end) = frame_byte_range(header, from_frame, to_frame);
    if end > file_bytes.len() {
        return Err(format!(
            "Failed to decode emissions. Frame range ends at {end}; file has {} bytes",
            file_bytes.len()
        ));
    }
    decode_stem_frames(header, &file_bytes[start..end])
}

/// Storyteller `EmissionsReader` / `emit/fs.ts` `BLOCK_FRAMES`.
pub const BLOCK_FRAMES: usize = 1000;

/// ST `EmissionsReader.blocks` for one STEM file: yield ≤[`BLOCK_FRAMES`] decoded
/// chunks. Full-file [`decode_stem_bytes`] / [`decode_stem_file`] stay available.
pub fn stem_blocks<'a>(header: &'a StemHeader, file_bytes: &'a [u8]) -> StemBlocks<'a> {
    StemBlocks {
        header,
        file_bytes,
        from: 0,
    }
}

/// Iterator of [`stem_blocks`] chunks.
pub struct StemBlocks<'a> {
    header: &'a StemHeader,
    file_bytes: &'a [u8],
    from: usize,
}

impl Iterator for StemBlocks<'_> {
    type Item = Result<Emissions, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.from >= self.header.num_frames {
            return None;
        }
        let to = (self.from + BLOCK_FRAMES).min(self.header.num_frames);
        let from = self.from;
        self.from = to;
        Some(decode_stem_frame_range(
            self.header,
            self.file_bytes,
            from,
            to,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip helper: f32 → IEEE binary16 bits (for synthetic STEM fixtures).
    fn f32_to_f16_bits(v: f32) -> u16 {
        let bits = v.to_bits();
        let sign = (bits >> 16) & 0x8000;
        let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
        let mut frac = bits & 0x7fffff;
        if exp <= 0 {
            if exp < -10 {
                return sign as u16;
            }
            frac |= 0x800000;
            let shift = (1 - exp) as u32;
            let half = (frac >> (shift + 13)) as u16;
            return (sign as u16) | half;
        }
        if exp >= 31 {
            return (sign as u16) | 0x7c00;
        }
        let half_frac = (frac >> 13) as u16;
        (sign as u16) | ((exp as u16) << 10) | half_frac
    }

    fn write_u16(buf: &mut Vec<u8>, v: u16) {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_u32(buf: &mut Vec<u8>, v: u32) {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_f32(buf: &mut Vec<u8>, v: f32) {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_f64(buf: &mut Vec<u8>, v: f64) {
        buf.extend_from_slice(&v.to_le_bytes());
    }

    fn build_dense_v3(vocab: &[(&str, i32)], frames: &[[f32; 3]], blank: i32) -> Vec<u8> {
        assert_eq!(vocab.len(), 3);
        let vocab_size = vocab.len() as u32;
        let num_frames = frames.len() as u32;

        let mut vocab_blob = Vec::new();
        for &(tok, idx) in vocab {
            write_u32(&mut vocab_blob, idx as u32);
            let tb = tok.as_bytes();
            write_u16(&mut vocab_blob, tb.len() as u16);
            vocab_blob.extend_from_slice(tb);
        }
        while vocab_blob.len() % 4 != 0 {
            vocab_blob.push(0);
        }
        let frames_offset = (OFFSET_VOCAB + vocab_blob.len()) as u32;

        let mut out = Vec::new();
        out.extend_from_slice(b"STEM");
        write_u16(&mut out, 3);
        write_u16(&mut out, ENCODING_DENSE);
        write_u32(&mut out, vocab_size);
        write_u32(&mut out, num_frames);
        write_u32(&mut out, blank as u32);
        write_f64(&mut out, 1.0); // seconds_per_chunk
        write_u32(&mut out, 50); // frames_per_chunk
        write_f64(&mut out, 0.02); // seconds_per_frame
        write_u32(&mut out, frames_offset);
        write_f32(&mut out, 0.0); // start
        write_f32(&mut out, 0.0); // end
        write_u16(&mut out, TOKENIZER_NONE);
        write_u16(&mut out, 0); // topK
        write_u32(&mut out, 0); // unkId
        assert_eq!(out.len(), OFFSET_VOCAB);
        out.extend_from_slice(&vocab_blob);
        assert_eq!(out.len(), frames_offset as usize);

        for row in frames {
            for &v in row {
                write_u16(&mut out, f32_to_f16_bits(v));
            }
        }
        out
    }

    #[test]
    fn dense_v3_round_trip() {
        let vocab = [("-", 0), ("a", 1), ("b", 2)];
        let frames = [[0.1f32, 0.9, 0.0], [0.9, 0.05, 0.0], [0.0, 0.1, 0.8]];
        let bytes = build_dense_v3(&vocab, &frames, 0);
        let (header, emissions) = decode_stem_bytes(&bytes).expect("decode");
        assert_eq!(header.version, 3);
        assert_eq!(header.encoding, StemEncoding::Dense);
        assert_eq!(header.classes, 3);
        assert_eq!(header.num_frames, 3);
        assert_eq!(header.blank_index, 0);
        assert_eq!(header.tokenizer_kind, TOKENIZER_NONE);
        assert_eq!(header.vocab.get("a"), Some(&1));
        assert_eq!(emissions.frames, 3);
        assert_eq!(emissions.classes, 3);
        assert!((emissions.values[1] - 0.9).abs() < 0.01);
        assert!((emissions.values[8] - 0.8).abs() < 0.02);
        assert!(reject_subword_tokenizer(header.tokenizer_kind).is_ok());

        let slice = decode_stem_frame_range(&header, &bytes, 1, 2).expect("range");
        assert_eq!(slice.frames, 1);
        assert!((slice.values[0] - 0.9).abs() < 0.01);
    }

    #[test]
    fn stem_blocks_concat_matches_full_decode() {
        let vocab = [("-", 0), ("a", 1), ("b", 2)];
        // 3 frames << BLOCK_FRAMES → one block; still exercises the iterator.
        let frames = [[0.1f32, 0.9, 0.0], [0.9, 0.05, 0.0], [0.0, 0.1, 0.8]];
        let bytes = build_dense_v3(&vocab, &frames, 0);
        let (header, full) = decode_stem_bytes(&bytes).expect("full");
        let mut values = Vec::new();
        let mut n = 0usize;
        for block in stem_blocks(&header, &bytes) {
            let e = block.expect("block");
            assert_eq!(e.classes, full.classes);
            values.extend_from_slice(&e.values);
            n += e.frames;
        }
        assert_eq!(n, full.frames);
        assert_eq!(values.len(), full.values.len());
        for (a, b) in values.iter().zip(full.values.iter()) {
            assert!((a - b).abs() < 1e-3);
        }
    }

    #[test]
    fn stem_blocks_chunks_at_block_frames() {
        let vocab = [("-", 0), ("a", 1), ("b", 2)];
        let mut frames = Vec::with_capacity(BLOCK_FRAMES + 2);
        for i in 0..(BLOCK_FRAMES + 2) {
            let v = (i % 3) as f32 * 0.1;
            frames.push([v, 1.0 - v, 0.0]);
        }
        let bytes = build_dense_v3(&vocab, &frames, 0);
        let (header, _) = decode_stem_bytes(&bytes).expect("full");
        let sizes: Vec<usize> = stem_blocks(&header, &bytes)
            .map(|b| b.expect("block").frames)
            .collect();
        assert_eq!(sizes, vec![BLOCK_FRAMES, 2]);
    }

    #[test]
    fn reject_subword_tokenizer_blocks_unigram() {
        assert!(reject_subword_tokenizer(TOKENIZER_NONE).is_ok());
        let err = reject_subword_tokenizer(TOKENIZER_UNIGRAM).unwrap_err();
        assert!(err.contains("subword vocabulary"), "{err}");
        assert!(err.contains("mms"), "{err}");
    }

    #[test]
    fn f16_round_trip_common_values() {
        for v in [0.0f32, 1.0, -1.0, 0.5, -3.0, 0.125] {
            let back = f16_to_f32(f32_to_f16_bits(v));
            assert!((back - v).abs() < 1e-3, "{v} -> {back}");
        }
    }
}
