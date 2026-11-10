//! Media Foundation sink writer: hardware H.264 + AAC into fragmented MP4.

use super::Ctx;
use crate::{Error, Result, Settings};
use std::path::Path;
use windows::core::{Interface, HSTRING};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BOX,
};
use windows::Win32::Media::MediaFoundation::*;

pub(crate) const AUDIO_RATE: u32 = 48_000;
pub(crate) const AUDIO_CHANNELS: u32 = 2;
/// Bytes per audio frame of the PCM the encoder is fed (16-bit stereo).
pub(crate) const AUDIO_BLOCK: u32 = AUDIO_CHANNELS * 2;

/// How many frames may be queued in the encoder before new ones are dropped.
const SAMPLE_POOL: u32 = 12;

pub(crate) struct Encoder {
    writer: IMFSinkWriter,
    allocator: IMFVideoSampleAllocatorEx,
    context: ID3D11DeviceContext,
    video_stream: u32,
    audio_stream: Option<u32>,
    width: u32,
    height: u32,
    // Kept alive for the lifetime of the writer.
    _manager: IMFDXGIDeviceManager,
}

// The sink writer, the sample allocator and a multithread-protected D3D
// context are all documented as callable from any thread.
unsafe impl Send for Encoder {}
unsafe impl Sync for Encoder {}

fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn attributes(capacity: u32) -> Result<IMFAttributes> {
    let mut attrs = None;
    unsafe { MFCreateAttributes(&mut attrs, capacity) }.ctx("create attributes")?;
    Ok(attrs.expect("MFCreateAttributes succeeded without a value"))
}

impl Encoder {
    pub fn new(
        device: &ID3D11Device,
        output: &Path,
        width: u32,
        height: u32,
        settings: &Settings,
        with_audio: bool,
    ) -> Result<Self> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL).ctx("start Media Foundation")?;

            // Let the encoder read our textures directly.
            let mut token = 0u32;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager).ctx("create device manager")?;
            let manager = manager.expect("device manager");
            manager.ResetDevice(device, token).ctx("attach D3D device to encoder")?;

            let attrs = attributes(6)?;
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1).ctx("writer attributes")?;
            attrs.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1).ctx("writer attributes")?;
            attrs.SetUINT32(&MF_LOW_LATENCY, 1).ctx("writer attributes")?;
            attrs.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &manager).ctx("writer attributes")?;
            // Fragmented MP4: everything written so far survives a crash.
            attrs.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_FMPEG4).ctx("writer attributes")?;

            let url = HSTRING::from(output.as_os_str());
            let writer = MFCreateSinkWriterFromURL(&url, None, &attrs).ctx("create output file")?;

            let fps = settings.fps.max(1);

            // --- video: what goes into the file
            let out = MFCreateMediaType().ctx("video output type")?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ctx("video output type")?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).ctx("video output type")?;
            out.SetUINT32(&MF_MT_AVG_BITRATE, settings.bitrate_kbps.saturating_mul(1000)).ctx("video output type")?;
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).ctx("video output type")?;
            out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32).ctx("video output type")?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height)).ctx("video output type")?;
            out.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).ctx("video output type")?;
            out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).ctx("video output type")?;
            let video_stream = writer.AddStream(&out).ctx("add video stream")?;

            // --- video: what we feed in (BGRA textures from the capture)
            let input = MFCreateMediaType().ctx("video input type")?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ctx("video input type")?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_ARGB32).ctx("video input type")?;
            input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).ctx("video input type")?;
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4).ctx("video input type")?;
            input.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1).ctx("video input type")?;
            input.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height)).ctx("video input type")?;
            input.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).ctx("video input type")?;
            input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).ctx("video input type")?;

            // Encoder tuning. A fixed keyframe interval makes lossless cuts land
            // within `keyframe_interval_secs` of where they were asked for.
            let enc = attributes(4)?;
            let gop = fps * settings.keyframe_interval_secs.max(1);
            enc.SetUINT32(&CODECAPI_AVEncMPVGOPSize, gop).ctx("encoder settings")?;
            enc.SetUINT32(&CODECAPI_AVEncMPVDefaultBPictureCount, 0).ctx("encoder settings")?;
            enc.SetUINT32(&CODECAPI_AVEncCommonRateControlMode, eAVEncCommonRateControlMode_CBR.0 as u32)
                .ctx("encoder settings")?;
            writer
                .SetInputMediaType(video_stream, &input, &enc)
                .ctx("configure video encoder (no hardware H.264 encoder accepted the format)")?;

            // --- audio
            let audio_stream = if with_audio {
                let aout = MFCreateMediaType().ctx("audio output type")?;
                aout.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).ctx("audio output type")?;
                aout.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).ctx("audio output type")?;
                aout.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).ctx("audio output type")?;
                aout.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE).ctx("audio output type")?;
                aout.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS).ctx("audio output type")?;
                aout.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000).ctx("audio output type")?; // 192 kbps
                let stream = writer.AddStream(&aout).ctx("add audio stream")?;

                let ain = MFCreateMediaType().ctx("audio input type")?;
                ain.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).ctx("audio input type")?;
                ain.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).ctx("audio input type")?;
                ain.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).ctx("audio input type")?;
                ain.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE).ctx("audio input type")?;
                ain.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS).ctx("audio input type")?;
                ain.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, AUDIO_BLOCK).ctx("audio input type")?;
                ain.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AUDIO_RATE * AUDIO_BLOCK).ctx("audio input type")?;
                ain.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1).ctx("audio input type")?;
                writer.SetInputMediaType(stream, &ain, None).ctx("configure audio encoder")?;
                Some(stream)
            } else {
                None
            };

            // A fixed pool of GPU textures to hand frames to the encoder in.
            // When the encoder falls behind the pool runs dry and frames are
            // dropped instead of memory growing without bound.
            let mut allocator: Option<IMFVideoSampleAllocatorEx> = None;
            MFCreateVideoSampleAllocatorEx(&IMFVideoSampleAllocatorEx::IID, &mut allocator as *mut _ as *mut _)
                .ctx("create frame pool")?;
            let allocator = allocator.ok_or_else(|| Error::Other("no frame pool".into()))?;
            allocator.SetDirectXManager(&manager).ctx("create frame pool")?;
            let pool_attrs = attributes(2)?;
            pool_attrs
                .SetUINT32(&MF_SA_D3D11_BINDFLAGS, (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32)
                .ctx("create frame pool")?;
            allocator
                .InitializeSampleAllocatorEx(2, SAMPLE_POOL, &pool_attrs, &input)
                .ctx("create frame pool")?;

            writer.BeginWriting().ctx("begin writing")?;

            let context = device.GetImmediateContext().ctx("get D3D context")?;
            Ok(Encoder { writer, allocator, context, video_stream, audio_stream, width, height, _manager: manager })
        }
    }

    /// Queue one captured frame. `src_w`/`src_h` is the valid area of `texture`.
    /// Returns `Ok(false)` if the frame was dropped because the encoder is behind.
    pub fn write_video(&self, texture: &ID3D11Texture2D, src_w: u32, src_h: u32, pts: i64, duration: i64) -> Result<bool> {
        unsafe {
            let sample = match self.allocator.AllocateSample() {
                Ok(s) => s,
                Err(e) if e.code() == MF_E_SAMPLEALLOCATOR_EMPTY => return Ok(false),
                Err(e) => return Err(Error::Windows { context: "allocate frame", source: e }),
            };
            let buffer = sample.GetBufferByIndex(0).ctx("frame buffer")?;
            let dxgi: IMFDXGIBuffer = buffer.cast().ctx("frame buffer")?;
            let mut dst: Option<ID3D11Texture2D> = None;
            dxgi.GetResource(&ID3D11Texture2D::IID, &mut dst as *mut _ as *mut _).ctx("frame texture")?;
            let dst = dst.ok_or_else(|| Error::Other("encoder frame has no texture".into()))?;
            let subresource = dxgi.GetSubresourceIndex().ctx("frame texture")?;

            // Copy the overlapping area; if the window shrank the rest keeps
            // whatever the pooled texture held, if it grew the frame is cropped.
            let region = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: src_w.min(self.width),
                bottom: src_h.min(self.height),
                back: 1,
            };
            self.context.CopySubresourceRegion(&dst, subresource, 0, 0, 0, texture, 0, Some(&region));

            let len = buffer.cast::<IMF2DBuffer>().and_then(|b| b.GetContiguousLength()).ctx("frame length")?;
            buffer.SetCurrentLength(len).ctx("frame length")?;
            sample.SetSampleTime(pts).ctx("frame time")?;
            sample.SetSampleDuration(duration).ctx("frame time")?;
            self.writer.WriteSample(self.video_stream, &sample).ctx("write video frame")?;
            Ok(true)
        }
    }

    /// Queue interleaved 16-bit stereo PCM starting at `pts` (100 ns units).
    pub fn write_audio(&self, pcm: &[u8], pts: i64) -> Result<()> {
        let Some(stream) = self.audio_stream else { return Ok(()) };
        if pcm.is_empty() {
            return Ok(());
        }
        unsafe {
            let buffer = MFCreateMemoryBuffer(pcm.len() as u32).ctx("audio buffer")?;
            let mut ptr = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None).ctx("audio buffer")?;
            std::ptr::copy_nonoverlapping(pcm.as_ptr(), ptr, pcm.len());
            buffer.Unlock().ctx("audio buffer")?;
            buffer.SetCurrentLength(pcm.len() as u32).ctx("audio buffer")?;

            let sample = MFCreateSample().ctx("audio sample")?;
            sample.AddBuffer(&buffer).ctx("audio sample")?;
            let frames = pcm.len() as i64 / AUDIO_BLOCK as i64;
            sample.SetSampleTime(pts).ctx("audio sample")?;
            sample.SetSampleDuration(frames * 10_000_000 / AUDIO_RATE as i64).ctx("audio sample")?;
            self.writer.WriteSample(stream, &sample).ctx("write audio")?;
        }
        Ok(())
    }

    /// Flush the encoders and close the file.
    pub fn finish(&self) -> Result<()> {
        unsafe { self.writer.Finalize() }.ctx("finalize file")
    }
}
