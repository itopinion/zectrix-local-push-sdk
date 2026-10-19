use crate::{AudioClip, Error, RecordingEnd, Result};
use std::collections::BTreeMap;
use tokio::time::{Duration, Instant};
pub const FRAME: usize = 960;
pub const MAX_SAMPLES: usize = 16000 * 120;
pub fn encode(audio: &AudioClip) -> Result<Vec<Vec<u8>>> {
    if audio.samples.is_empty() {
        return Err(Error::InvalidAudio("没有样本".into()));
    }
    if audio.samples.len() > MAX_SAMPLES {
        return Err(Error::AudioTooLong);
    }
    let mut enc =
        opus::Encoder::new(16000, opus::Channels::Mono, opus::Application::Audio).map_err(err)?;
    enc.set_dtx(false).map_err(err)?;
    audio
        .samples
        .chunks(FRAME)
        .map(|chunk| {
            let mut padded = [0i16; FRAME];
            padded[..chunk.len()].copy_from_slice(chunk);
            enc.encode_vec(&padded, 1180).map_err(err)
        })
        .collect()
}
fn err(e: opus::Error) -> Error {
    Error::InvalidAudio(e.to_string())
}
pub struct Recording {
    pub id: u32,
    pub progress: Instant,
    pub started: Instant,
    decoder: opus::Decoder,
    frames: BTreeMap<u32, Vec<u8>>,
    next: u32,
    pcm: Vec<i16>,
    last: Vec<i16>,
    pub end: Option<(u32, u16, RecordingEnd, Instant)>,
    pub stopped: bool,
}
impl Recording {
    pub fn new(id: u32) -> Result<Self> {
        Ok(Self {
            id,
            progress: Instant::now(),
            started: Instant::now(),
            decoder: opus::Decoder::new(16000, opus::Channels::Mono).map_err(err)?,
            frames: BTreeMap::new(),
            next: 0,
            pcm: Vec::new(),
            last: Vec::new(),
            end: None,
            stopped: false,
        })
    }
    pub fn push(&mut self, seq: u32, data: &[u8]) -> Result<()> {
        if seq < self.next
            || seq >= 2000
            || seq >= self.next + 6
            || self.frames.contains_key(&seq)
            || self.end.as_ref().is_some_and(|e| seq >= e.0)
        {
            return Ok(());
        }
        if data.is_empty()
            || data.len() > 1180
            || opus::packet::get_nb_samples(data, 16000).map_err(err)? != FRAME
            || opus::packet::get_nb_channels(data).map_err(err)? != opus::Channels::Mono
        {
            return Err(Error::InvalidAudio("Opus 帧格式错误".into()));
        }
        self.frames.insert(seq, data.to_vec());
        self.progress = Instant::now();
        self.drain(false)?;
        Ok(())
    }
    fn drain(&mut self, force: bool) -> Result<()> {
        let limit = self.end.as_ref().map(|e| e.0);
        while self.next < 2000 && limit.is_none_or(|n| self.next < n) {
            let present = self.frames.contains_key(&self.next);
            if !present
                && !(force && (limit.is_some() || !self.frames.is_empty())
                    || self.frames.len() >= 6
                    || (!self.frames.is_empty()
                        && self.progress.elapsed() >= Duration::from_millis(180)))
            {
                break;
            }
            if !self.last.is_empty() {
                self.pcm.extend_from_slice(&self.last);
            }
            let data = self.frames.remove(&self.next).unwrap_or_default();
            let mut pcm = [0i16; FRAME];
            let n = self.decoder.decode(&data, &mut pcm, false).map_err(err)?;
            if n != FRAME {
                return Err(Error::InvalidAudio("解码帧长度错误".into()));
            }
            self.last = pcm.to_vec();
            self.next += 1;
        }
        Ok(())
    }
    pub fn ending(&mut self, total: u32, tail: u16, reason: RecordingEnd) -> Result<()> {
        if total > 2000
            || (total == 0 && tail != 0)
            || (total > 0 && !(1..=960).contains(&tail))
            || total < self.next
            || self
                .frames
                .last_key_value()
                .is_some_and(|(&n, _)| n >= total)
        {
            return Err(Error::InvalidAudio("录音结束长度错误".into()));
        }
        if self.end.is_none() {
            self.end = Some((
                total,
                tail,
                if self.stopped {
                    RecordingEnd::Stopped
                } else {
                    reason
                },
                Instant::now() + Duration::from_millis(360),
            ));
        }
        Ok(())
    }
    pub fn ready(&mut self) -> Result<bool> {
        let force = self.end.as_ref().is_some_and(|e| Instant::now() >= e.3);
        self.drain(force)?;
        Ok(self.end.as_ref().is_some_and(|e| self.next == e.0))
    }
    pub fn finish(mut self, fallback: RecordingEnd) -> (Option<AudioClip>, RecordingEnd) {
        let _ = self.drain(true);
        let (tail, reason) = self
            .end
            .as_ref()
            .map(|e| (e.1 as usize, e.2))
            .unwrap_or((self.last.len(), fallback));
        self.pcm
            .extend_from_slice(&self.last[..tail.min(self.last.len())]);
        (
            if self.pcm.is_empty() {
                None
            } else {
                Some(AudioClip { samples: self.pcm })
            },
            reason,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn reordered_audio_and_partial_tail() {
        let samples = (0..1083)
            .map(|i| ((i as f32 * 0.2).sin() * 8000.) as i16)
            .collect();
        let frames = encode(&AudioClip { samples }).unwrap();
        let mut r = Recording::new(9).unwrap();
        r.push(1, &frames[1]).unwrap();
        r.push(0, &frames[0]).unwrap();
        r.ending(2, 123, RecordingEnd::Completed).unwrap();
        assert!(r.ready().unwrap());
        let (a, why) = r.finish(RecordingEnd::Failed);
        assert_eq!(why, RecordingEnd::Completed);
        assert_eq!(a.unwrap().samples.len(), 1083);
    }
    #[tokio::test]
    async fn missing_frame_is_concealed_at_end() {
        let frames = encode(&AudioClip {
            samples: vec![300; 1920],
        })
        .unwrap();
        let mut r = Recording::new(1).unwrap();
        r.push(1, &frames[1]).unwrap();
        r.ending(2, 960, RecordingEnd::Completed).unwrap();
        let (a, _) = r.finish(RecordingEnd::Failed);
        assert_eq!(a.unwrap().samples.len(), 1920);
    }
    #[tokio::test]
    async fn zero_frame_has_no_audio() {
        let mut r = Recording::new(1).unwrap();
        r.ending(0, 0, RecordingEnd::Completed).unwrap();
        assert!(r.ready().unwrap());
        assert!(r.finish(RecordingEnd::Failed).0.is_none());
    }
    #[test]
    fn oversized_audio_is_rejected() {
        assert!(matches!(
            encode(&AudioClip {
                samples: vec![0; MAX_SAMPLES + 1]
            }),
            Err(Error::AudioTooLong)
        ));
    }
}
