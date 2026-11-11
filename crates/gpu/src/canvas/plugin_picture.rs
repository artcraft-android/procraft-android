//! A picture from a plugin file (interface 2): one WGSL function, `picture`, with everything around
//! it written here (the template in `shaders/plugin_picture_2.wgsl`). The app shows it in place of a
//! picture of its own, first the colour panel's Classic square, and draws it again whenever its hue
//! changes, so it is drawn on the GPU: on the CPU in a plugin's code it would take a good part of a
//! second on a tablet each time.
//!
//! It is checked as a filter's function is (`plugin_filter.rs`): nothing to reach, and loops that
//! pay from a budget, so the most a pixel can cost is known. A picture is small, so it is drawn whole
//! at once, after a probe that finds one that runs out of its budget, and refused when even its
//! worst could take too long.

use std::borrow::Cow;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use wgpu::util::DeviceExt;

use super::plugin_filter::{BIG_STACK, COMPILE_LIMIT, FilterCode, PICTURE, read_kind};
use super::{blended_pipeline, read_texture, render_pass, texture};
use crate::{Gpu, GpuError};

/// The largest picture, pixels each way, and the most all its pixels may cost at their worst.
const MOST_SIDE: u32 = 1024;
const MOST_WORK: u64 = 1 << 33;
/// How many pixels each way the probe looks at.
const PROBE_SIDE: u32 = 16;
/// A draw that takes this long isn't one that finished.
const DRAW_LIMIT: Duration = Duration::from_millis(1500);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PictureUniform {
    /// (the hue, unused, unused, unused)
    a: [f32; 4],
    /// (the picture's width and height, unused, unused)
    size: [f32; 4],
}

/// A picture's code that has held up, with what its loops pay written in. Nothing has run.
pub struct PictureCode(FilterCode);

impl PictureCode {
    /// The most a pixel can cost.
    pub fn worst(&self) -> u64 {
        self.0.worst()
    }
}

/// Reads `text`, a picture's code. Why not, in words for the log, if it can't be drawn. Nothing
/// runs, and no GPU is needed.
pub fn read_picture(text: &str) -> Result<PictureCode, String> {
    read_kind(text, &PICTURE).map(PictureCode)
}

/// A picture ready to draw on this device.
pub struct PictureProgram {
    draw: wgpu::RenderPipeline,
    probe: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    worst: u64,
    loops: bool,
}

/// Why a picture wasn't drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PictureStop {
    /// Run out of its budget on a pixel looked at: it never ends, or does too much for a pixel.
    Spent,
    /// Asked for bigger than a picture may be, or bigger than its worst can be drawn in time.
    TooBig,
    /// It took longer than any picture can, so it may have been stopped.
    Cut,
    /// The device was lost.
    Lost,
}

impl Gpu {
    /// Compiles `code` for this device, as [`Gpu::make_filter`] compiles a filter.
    pub fn make_picture(&self, code: PictureCode) -> Result<Arc<PictureProgram>, GpuError> {
        let (done, came) = mpsc::channel();
        let gpu = self.clone();
        let started =
            std::thread::Builder::new().name("plugin picture".into()).stack_size(BIG_STACK).spawn(move || {
                let _ = done.send(gpu.picture_pipelines(code.0));
            });
        started.map_err(|e| GpuError::Failed(e.to_string()))?;
        came.recv_timeout(COMPILE_LIMIT).map_err(|e| GpuError::Failed(format!("compiling a picture: {e}")))?
    }

    fn picture_pipelines(&self, code: FilterCode) -> Result<Arc<PictureProgram>, GpuError> {
        let (worst, loops) = (code.worst(), code.loops());
        self.checked(|| {
            let device = &self.device;
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("plugin picture"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("plugin picture"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("plugin picture"),
                source: wgpu::ShaderSource::Naga(Cow::Owned(code.into_module())),
            });
            let pipeline = |entry: &str| {
                blended_pipeline(device, &pipeline_layout, &module, "plugin picture", ("host_vs", entry), FORMAT, None)
            };
            Arc::new(PictureProgram {
                draw: pipeline("host_main"),
                probe: pipeline("host_probe"),
                layout,
                worst,
                loops,
            })
        })
    }

    /// `program`'s picture for `hue` (0..1), `side` pixels square: premultiplied RGBA rows from the
    /// top. Waits for it.
    pub fn draw_picture(&self, program: &PictureProgram, hue: f32, side: u32) -> Result<Vec<u8>, PictureStop> {
        let pixels = u64::from(side) * u64::from(side);
        if side == 0 || side > MOST_SIDE || pixels.saturating_mul(program.worst.max(1)) > MOST_WORK {
            return Err(PictureStop::TooBig);
        }
        if self.is_lost() {
            return Err(PictureStop::Lost);
        }
        let device = &self.device;
        let hue = if hue.is_finite() { hue.rem_euclid(1.0) } else { 0.0 };
        let uniform = PictureUniform { a: [hue, 0.0, 0.0, 0.0], size: [side as f32, side as f32, 0.0, 0.0] };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("plugin picture"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("plugin picture"),
            layout: &program.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        // draws `pipeline` into `target` and waits: whether it finished in time and the device stands
        let draw = |pipeline: &wgpu::RenderPipeline, target: &wgpu::Texture| -> Result<(), PictureStop> {
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("plugin picture") });
            let view = target.create_view(&wgpu::TextureViewDescriptor::default());
            {
                let mut pass =
                    render_pass(&mut encoder, "plugin picture", &view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
            let sent = Instant::now();
            self.queue.submit([encoder.finish()]);
            let waited = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(DRAW_LIMIT) });
            if self.is_lost() {
                return Err(PictureStop::Lost);
            }
            if waited.is_err() || sent.elapsed() >= DRAW_LIMIT {
                return Err(PictureStop::Cut);
            }
            Ok(())
        };
        let read = |target: &wgpu::Texture, side: u32| {
            read_texture(self, target, side, side)
                .map_err(|_| if self.is_lost() { PictureStop::Lost } else { PictureStop::Cut })
        };
        if program.loops {
            let probe = texture(device, "plugin picture probe", PROBE_SIDE, PROBE_SIDE, FORMAT);
            draw(&program.probe, &probe)?;
            if read(&probe, PROBE_SIDE)?.as_chunks::<4>().0.iter().any(|[red, ..]| *red > 127) {
                return Err(PictureStop::Spent);
            }
        }
        let target = texture(device, "plugin picture", side, side, FORMAT);
        draw(&program.draw, &target)?;
        read(&target, side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu() -> Option<Gpu> {
        Gpu::headless().map_err(|e| eprintln!("skipped: {e}")).ok()
    }

    #[test]
    fn a_picture_reads_with_what_it_is_given_and_nothing_else() {
        assert!(read_picture("fn picture(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(at, hue(), 1.0); }").is_ok());
        let sized = "fn picture(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(at * picture_size() / 8.0, 0.0, 1.0); }";
        assert!(read_picture(sized).is_ok());
        // not what a filter is given, nor anything of the app's
        let layer = "fn picture(at: vec2<f32>) -> vec4<f32> { return layer_at(at); }";
        assert!(read_picture(layer).is_err());
        let reaches = "fn picture(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(host_u.a.x); }";
        assert!(read_picture(reaches).is_err());
        assert!(
            read_picture("fn pixel(c: vec4<f32>, at: vec2<f32>) -> vec4<f32> { return c; }").is_err(),
            "no picture"
        );
        let resource = "@group(0) @binding(1) var t: texture_2d<f32>;\nfn picture(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(1.0); }";
        assert!(read_picture(resource).is_err());
    }

    #[test]
    fn a_picture_is_drawn_for_its_hue_premultiplied() {
        let Some(gpu) = gpu() else { return };
        let text = "fn picture(at: vec2<f32>) -> vec4<f32> { return vec4<f32>(at.x, 1.0, hue(), 0.5); }";
        let program = gpu.make_picture(read_picture(text).unwrap()).unwrap();
        let rgba = gpu.draw_picture(&program, 1.5, 8).unwrap();
        assert_eq!(rgba.len(), 8 * 8 * 4);
        // the first pixel's centre is an eighth of a half across; the hue is taken round to .5
        let first = &rgba[..4];
        assert!((first[0] as i32 - (0.0625_f32 * 0.5 * 255.0).round() as i32).abs() <= 1, "{first:?}");
        assert!((first[1] as i32 - 128).abs() <= 1, "{first:?}");
        assert!((first[2] as i32 - 64).abs() <= 1, "{first:?}");
        assert!((first[3] as i32 - 128).abs() <= 1, "{first:?}");
        // too big a picture isn't drawn
        assert_eq!(gpu.draw_picture(&program, 0.0, MOST_SIDE + 1).err(), Some(PictureStop::TooBig));
        assert_eq!(gpu.draw_picture(&program, 0.0, 0).err(), Some(PictureStop::TooBig));
    }

    #[test]
    fn a_picture_that_never_ends_is_found_before_it_is_drawn() {
        let Some(gpu) = gpu() else { return };
        let text =
            "fn picture(at: vec2<f32>) -> vec4<f32> { var x = at.x; loop { x = x + 1.0; } return vec4<f32>(x); }";
        let program = gpu.make_picture(read_picture(text).unwrap()).unwrap();
        assert_eq!(gpu.draw_picture(&program, 0.0, 64).err(), Some(PictureStop::Spent));
    }
}
