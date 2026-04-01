use std::{fs, io, path::Path, sync::OnceLock};

use ffmpeg::{codec, filter, format, frame, media};
use ffmpeg_next as ffmpeg;
use thiserror::Error;

use crate::{
    signal_pcm::{SignalLength, SignalPcm, signal},
    signal_spec::{ResolvedSignalSpec, SignalFormat},
};

#[derive(Debug)]
pub(crate) struct EncodedAudio {
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: &'static str,
}

#[derive(Debug, Error)]
pub(crate) enum SignalEncodeError {
    #[error("signal length mode is not supported for `{0}`")]
    UnsupportedLength(&'static str),
    #[error("ffmpeg initialization failed: {0}")]
    Init(String),
    #[error("no output codec is registered for `{0}`")]
    MissingOutputCodec(&'static str),
    #[error("no encoder is available for `{0}`")]
    MissingEncoder(&'static str),
    #[error("failed to access encoded signal artifact: {0}")]
    Io(#[source] io::Error),
    #[error("ffmpeg encode failed: {0}")]
    Ffmpeg(#[source] ffmpeg::Error),
}

impl SignalEncodeError {
    pub(crate) const fn is_bad_request(&self) -> bool {
        matches!(self, Self::UnsupportedLength(_))
    }
}

impl From<io::Error> for SignalEncodeError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ffmpeg::Error> for SignalEncodeError {
    fn from(error: ffmpeg::Error) -> Self {
        Self::Ffmpeg(error)
    }
}

struct EncodeTarget {
    ext: &'static str,
    mime: &'static str,
    bit_rate: Option<usize>,
    option_pairs: &'static [(&'static str, &'static str)],
}

impl EncodeTarget {
    fn from_format(format: SignalFormat) -> Self {
        match format {
            SignalFormat::Wav => unreachable!("WAV uses the dedicated route path"),
            SignalFormat::Mp3 => Self {
                ext: "mp3",
                mime: "audio/mpeg",
                bit_rate: Some(128_000),
                option_pairs: &[("b", "128k")],
            },
            SignalFormat::Flac => Self {
                ext: "flac",
                mime: "audio/flac",
                bit_rate: None,
                option_pairs: &[("compression_level", "5")],
            },
            SignalFormat::Aac => Self {
                ext: "aac",
                mime: "audio/aac",
                bit_rate: Some(128_000),
                option_pairs: &[("b", "128k")],
            },
            SignalFormat::M4a => Self {
                ext: "m4a",
                mime: "audio/mp4",
                bit_rate: Some(128_000),
                option_pairs: &[("b", "128k")],
            },
        }
    }
}

pub(crate) fn encode_signal<S: signal::SignalFn>(
    signal: S,
    spec: &ResolvedSignalSpec,
    format: SignalFormat,
) -> Result<EncodedAudio, SignalEncodeError> {
    if !matches!(spec.length, SignalLength::Finite { .. }) {
        let ext = EncodeTarget::from_format(format).ext;
        return Err(SignalEncodeError::UnsupportedLength(ext));
    }

    ensure_ffmpeg_initialized()?;

    let target = EncodeTarget::from_format(format);
    let temp_dir = tempfile::tempdir()?;
    let output_path = temp_dir.path().join(format!("signal.{}", target.ext));

    let pcm = SignalPcm::new(signal, spec.sample_rate, spec.channels, spec.length);
    encode_direct_pcm(&pcm, &output_path, &target)?;

    Ok(EncodedAudio {
        bytes: fs::read(&output_path)?,
        content_type: target.mime,
    })
}

fn ensure_ffmpeg_initialized() -> Result<(), SignalEncodeError> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();

    match INIT.get_or_init(|| ffmpeg::init().map_err(|error| error.to_string())) {
        Ok(()) => Ok(()),
        Err(message) => Err(SignalEncodeError::Init(message.clone())),
    }
}

fn encode_direct_pcm<S: signal::SignalFn>(
    pcm: &SignalPcm<S>,
    output_path: &Path,
    target: &EncodeTarget,
) -> Result<(), SignalEncodeError> {
    let mut octx = format::output(output_path)?;
    let mut encoder = DirectEncoder::new(
        &mut octx,
        output_path,
        target,
        pcm.sample_rate(),
        pcm.channels(),
    )?;

    octx.write_header()?;

    let chunk_frames = 1024;
    let bytes_per_frame = pcm.channels() as usize * size_of::<i16>();
    let mut offset = 0;
    let mut pts = 0;

    let mut buf = vec![0u8; chunk_frames * bytes_per_frame];

    while offset < pcm.total_byte_len().unwrap_or(0) {
        let remaining_bytes = pcm.total_byte_len().unwrap_or(0) - offset;
        let read_bytes = remaining_bytes.min(buf.len());
        let read = pcm.read_pcm_at(offset, &mut buf[..read_bytes]);
        if read == 0 {
            break;
        }

        let frames_read = read / bytes_per_frame;

        let mut frame = frame::Audio::new(
            format::Sample::I16(format::sample::Type::Packed),
            frames_read,
            ffmpeg::ChannelLayout::default(i32::from(pcm.channels())),
        );
        frame.set_rate(pcm.sample_rate());
        frame.set_pts(Some(pts as i64));

        frame.data_mut(0)[..read].copy_from_slice(&buf[..read]);

        encoder.send_frame_to_filter(&frame)?;
        encoder.receive_and_process_filtered_frames(&mut octx)?;

        offset += read;
        pts += frames_read;
    }

    encoder.flush_filter()?;
    encoder.receive_and_process_filtered_frames(&mut octx)?;

    encoder.send_eof_to_encoder()?;
    encoder.receive_and_process_encoded_packets(&mut octx)?;

    octx.write_trailer()?;
    Ok(())
}

fn build_direct_filter(
    encoder: &codec::encoder::Audio,
    sample_rate: u32,
    channels: u16,
) -> Result<filter::Graph, ffmpeg::Error> {
    let mut graph = filter::Graph::new();
    let input_channel_layout = ffmpeg::ChannelLayout::default(i32::from(channels));
    let args = format!(
        "time_base=1/{}:sample_rate={}:sample_fmt={}:channel_layout=0x{:x}",
        sample_rate,
        sample_rate,
        format::Sample::I16(format::sample::Type::Packed).name(),
        input_channel_layout.bits()
    );

    graph.add(
        &filter::find("abuffer").ok_or(ffmpeg::Error::Bug)?,
        "in",
        &args,
    )?;
    graph.add(
        &filter::find("abuffersink").ok_or(ffmpeg::Error::Bug)?,
        "out",
        "",
    )?;

    let aformat_args = format!(
        "aformat=sample_fmts={}:sample_rates={}:channel_layouts=0x{:x}",
        encoder.format().name(),
        encoder.rate(),
        encoder.channel_layout().bits()
    );
    graph
        .output("in", 0)?
        .input("out", 0)?
        .parse(&aformat_args)?;
    graph.validate()?;

    if let Some(codec) = encoder.codec()
        && !codec
            .capabilities()
            .contains(codec::capabilities::Capabilities::VARIABLE_FRAME_SIZE)
    {
        graph
            .get("out")
            .ok_or(ffmpeg::Error::Bug)?
            .sink()
            .set_frame_size(encoder.frame_size());
    }

    Ok(graph)
}

struct DirectEncoder {
    filter: filter::Graph,
    encoder: codec::encoder::Audio,
}

impl DirectEncoder {
    fn new(
        octx: &mut format::context::Output,
        output_path: &Path,
        target: &EncodeTarget,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Self, SignalEncodeError> {
        let codec_id = octx.format().codec(output_path, media::Type::Audio);
        let codec = ffmpeg::encoder::find(codec_id)
            .ok_or(SignalEncodeError::MissingOutputCodec(target.ext))?
            .audio()
            .map_err(|_| SignalEncodeError::MissingEncoder(target.ext))?;
        let global_header = octx
            .format()
            .flags()
            .contains(format::flag::Flags::GLOBAL_HEADER);

        let mut output = octx.add_stream(codec)?;
        let context = codec::context::Context::from_parameters(output.parameters())?;
        let mut encoder = context.encoder().audio()?;

        let input_channel_layout = ffmpeg::ChannelLayout::default(i32::from(channels));
        let channel_layout = codec
            .channel_layouts()
            .map_or(ffmpeg::channel_layout::ChannelLayout::STEREO, |layouts| {
                layouts.best(input_channel_layout.channels())
            });

        if global_header {
            encoder.set_flags(codec::flag::Flags::GLOBAL_HEADER);
        }

        encoder.set_rate(sample_rate as i32);
        encoder.set_channel_layout(channel_layout);

        encoder.set_format(
            codec
                .formats()
                .ok_or(ffmpeg::Error::InvalidData)?
                .next()
                .ok_or(ffmpeg::Error::InvalidData)?,
        );
        if let Some(bit_rate) = target.bit_rate {
            encoder.set_bit_rate(bit_rate);
            encoder.set_max_bit_rate(bit_rate);
        }
        encoder.set_time_base((1, sample_rate as i32));
        output.set_time_base((1, sample_rate as i32));

        let mut options = ffmpeg::Dictionary::new();
        for (key, value) in target.option_pairs {
            options.set(key, value);
        }
        let encoder = encoder.open_as_with(codec, options)?;
        output.set_parameters(&encoder);

        let filter = build_direct_filter(&encoder, sample_rate, channels)?;

        Ok(Self { filter, encoder })
    }

    fn send_frame_to_filter(&mut self, frame: &frame::Audio) -> Result<(), ffmpeg::Error> {
        self.filter
            .get("in")
            .ok_or(ffmpeg::Error::Bug)?
            .source()
            .add(frame)
    }

    fn flush_filter(&mut self) -> Result<(), ffmpeg::Error> {
        self.filter
            .get("in")
            .ok_or(ffmpeg::Error::Bug)?
            .source()
            .flush()
    }

    fn receive_and_process_filtered_frames(
        &mut self,
        octx: &mut format::context::Output,
    ) -> Result<(), ffmpeg::Error> {
        let mut filtered = frame::Audio::empty();
        while self
            .filter
            .get("out")
            .ok_or(ffmpeg::Error::Bug)?
            .sink()
            .frame(&mut filtered)
            .is_ok()
        {
            self.encoder.send_frame(&filtered)?;
            self.receive_and_process_encoded_packets(octx)?;
        }

        Ok(())
    }

    fn send_eof_to_encoder(&mut self) -> Result<(), ffmpeg::Error> {
        self.encoder.send_eof()
    }

    fn receive_and_process_encoded_packets(
        &mut self,
        octx: &mut format::context::Output,
    ) -> Result<(), ffmpeg::Error> {
        let mut encoded = ffmpeg::Packet::empty();
        let stream_time_base = octx.stream(0).unwrap().time_base();
        while self.encoder.receive_packet(&mut encoded).is_ok() {
            if encoded.size() > 0 {
                encoded.set_stream(0);
                encoded.rescale_ts(self.encoder.time_base(), stream_time_base);
                encoded.write_interleaved(octx)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_format_mapping_matches_runtime_contract() {
        let cases = [
            (SignalFormat::Mp3, "mp3", "audio/mpeg", Some(128_000)),
            (SignalFormat::Flac, "flac", "audio/flac", None),
            (SignalFormat::Aac, "aac", "audio/aac", Some(128_000)),
            (SignalFormat::M4a, "m4a", "audio/mp4", Some(128_000)),
        ];

        for (format, ext, mime, bit_rate) in cases {
            let target = EncodeTarget::from_format(format);

            assert_eq!(target.ext, ext);
            assert_eq!(target.mime, mime);
            assert_eq!(target.bit_rate, bit_rate);
            assert!(!target.option_pairs.is_empty());
        }
    }
}
