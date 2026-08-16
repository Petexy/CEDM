//! LineXinBar-compatible wallpaper and Liquid Glass renderer.

pub mod theme;

use crate::displays;
use anyhow::Context;
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer as TextBuffer, Cache, Color as TextColor, Family, FontSystem, Metrics,
    Resolution, Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
    Weight,
};
use std::num::NonZeroU64;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

pub const SOLID_SLOT: u32 = 0;
pub const GLOW_SLOT: u32 = 1;
pub const ARROW_LEFT_SLOT: u32 = 2;
pub const ARROW_DOWN_SLOT: u32 = 3;
pub const ARROW_UP_SLOT: u32 = 4;
pub const ARROW_RIGHT_SLOT: u32 = 5;
pub const KEYBOARD_HIDE_SLOT: u32 = 6;
pub const PAD_SELECT_SLOT: u32 = 7;
pub const PAD_WEST_SLOT: u32 = 8;
pub const SLEEP_SLOT: u32 = 9;
pub const RESTART_SLOT: u32 = 10;
pub const POWER_SLOT: u32 = 11;
pub const USER_SWITCH_SLOT: u32 = 12;
pub const SESSION_SLOT: u32 = 13;
pub const KEYBOARD_SHOW_SLOT: u32 = 14;
/// The first cell an account's own picture goes in; one per enumerated account,
/// in the order they were enumerated.
pub const FACE_SLOT: u32 = 15;
pub const CIRCULAR_CORNER: f32 = 2.0;
pub const SQUIRCLE_CORNER: f32 = 4.0;
/// One cell, and the size every drawing and every portrait is kept at.
///
/// Twice what the glyphs alone would need. An avatar is drawn at the width of
/// the column's own disc, which is around 130 pixels on a 900-line display and
/// twice that on a console plugged into a 4K television — and a face is the one
/// thing in this interface that is a photograph rather than a shape, so it is
/// the one thing that cannot be redrawn at whatever size it is asked for.
const CELL: u32 = 256;
const ATLAS_COLUMNS: u32 = 4;
/// Fifteen drawings and a cell for each account's picture. Eight rows of four
/// leaves seventeen faces, which is more local interactive accounts than a
/// machine with a login screen on a television has; past that an account keeps
/// its initial, which is what every account without a published picture shows
/// anyway.
const ATLAS_ROWS: u32 = 8;
pub const MAX_FACES: usize = (ATLAS_COLUMNS * ATLAS_ROWS - FACE_SLOT) as usize;

/// The square a portrait has to arrive at to go in a cell.
pub const fn face_size() -> u32 {
    CELL
}

/// The cell `index`'s account has its picture in, if the atlas has one for it.
pub const fn face_slot(index: usize) -> Option<u32> {
    if index < MAX_FACES {
        Some(FACE_SLOT + index as u32)
    } else {
        None
    }
}
const BACKDROP_MIPS: u32 = 5;
const MAX_COVERS: usize = 6;
const MAX_GLASS_BATCHES: usize = 12;
const UNCUT: [f32; 4] = [-1.0e9, -1.0e9, 1.0e9, 1.0e9];
const UI_FONT: &str = "Roboto";
const UI_FONT_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
const UI_FONT_BOLD: &[u8] = include_bytes!("../../assets/fonts/Roboto-Bold.ttf");

#[derive(Debug, Clone, Copy)]
pub struct Quad {
    pub rect: [f32; 4],
    pub slot: u32,
    pub color: [f32; 4],
    pub radius: f32,
    pub corner: f32,
    pub border: f32,
    pub thickness: f32,
    pub frost: f32,
    pub gloss: f32,
    pub face_curve: f32,
    pub fade: f32,
    /// The display this pane is standing on, as its rectangle on the surface.
    ///
    /// Glass that finds nothing drawn behind it falls through to the wallpaper
    /// and *evaluates* it, so it has to be asked at the same place, in the same
    /// shape, as the pass that drew that display's wallpaper — which is this
    /// rectangle rather than the whole surface. All zeros means the surface,
    /// which is what a machine with one display has.
    pub display: [f32; 4],
}

impl Default for Quad {
    fn default() -> Self {
        Self {
            rect: [0.0; 4],
            slot: SOLID_SLOT,
            color: [0.0; 4],
            radius: 0.0,
            corner: CIRCULAR_CORNER,
            border: 0.0,
            thickness: 0.0,
            frost: 0.0,
            gloss: 0.0,
            face_curve: 0.0,
            fade: 1.0,
            display: [0.0; 4],
        }
    }
}

impl Quad {
    fn reads_backdrop(self) -> bool {
        self.radius > 0.0 && self.border <= 0.0 && self.thickness > 0.0
    }

    fn overlaps(self, other: Self) -> bool {
        let [x, y, w, h] = self.rect;
        let [ox, oy, ow, oh] = other.rect;
        x < ox + ow && ox < x + w && y < oy + oh && oy < y + h
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub content: String,
    pub rect: [f32; 4],
    pub size: f32,
    pub color: [f32; 4],
    pub bold: bool,
    pub align: TextAlign,
    /// A scissor over the run, in the same pixels as `rect`, or the whole of it.
    ///
    /// It cuts the drawing and never the layout: the run is still laid out in
    /// `rect`, at its own alignment, so every glyph stays exactly where it was
    /// and only the part of it that reaches the display differs. That is what
    /// lets a panel take away the half of a label it covers while the half
    /// beside it stays put — which is the only way a panel can be *in front of*
    /// text at all here, text being one pass after every quad.
    pub clip: Option<[f32; 4]>,
}

#[derive(Debug, Default)]
pub struct Scene {
    pub quads: Vec<Quad>,
    pub texts: Vec<Text>,
    /// The displays this frame is drawn on, each as its rectangle on the
    /// surface. The wallpaper is drawn once for each of them, in that
    /// display's own shape — a picture per screen rather than one picture
    /// stretched over a row of them.
    ///
    /// Empty is the whole surface as one display, which is what a scene
    /// assembled without asking about displays gets.
    pub displays: Vec<[f32; 4]>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Instance {
    rect: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
    shape: [f32; 4],
    material: [f32; 4],
    corner: f32,
    face_curve: f32,
    drain: f32,
    cut: [f32; 4],
    display: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    resolution: [f32; 2],
    time: f32,
    blur: f32,
    window_rect: [f32; 4],
    params: [f32; 4],
    sky: [[f32; 4]; 4],
    accent: [[f32; 4]; 3],
    glow: [f32; 4],
    covers: [[f32; 4]; MAX_COVERS],
    hero: [f32; 4],
}

#[derive(Debug, Clone, Copy)]
struct Batch {
    end: usize,
    reads: bool,
}

/// The textures a frame is built in.
///
/// Two of them, for the reason the shell has two Wayland surfaces: the
/// wallpaper is a *function*, and glass is written to ask it what it looks like
/// softened rather than to blur a picture of it. `wall` holds the wallpaper as
/// it is drawn; `scene` holds only what the greeter puts on top of it, over
/// nothing, and that is what the panes are handed as their backdrop. Where they
/// find it empty they fall through to the wallpaper function itself, at
/// whatever softness their frost asks for — which is how a frosted pane over a
/// smooth gradient comes out looking frosted at all. Blurring a picture of a
/// smooth gradient returns the gradient.
///
/// The two meet once, in the compositing pass, and everything after that —
/// the text, the copy the display gets, the copy `--shot` writes — is `wall`.
struct Offscreen {
    wall: wgpu::Texture,
    wall_view: wgpu::TextureView,
    wall_source: wgpu::BindGroup,
    scene: wgpu::Texture,
    scene_view: wgpu::TextureView,
    scene_source: wgpu::BindGroup,
    backdrop: wgpu::Texture,
    backdrop_source: wgpu::BindGroup,
    rungs: Vec<wgpu::TextureView>,
    rung_sources: Vec<wgpu::BindGroup>,
}

pub struct Renderer {
    _window: Arc<Window>,
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    _adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    background_pipeline: wgpu::RenderPipeline,
    quad_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    compose_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    sample_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    atlas_bind_group: wgpu::BindGroup,
    scenery_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    /// One rectangle per display, in the order the frame names them: the
    /// vertex stream the wallpaper pass is drawn from.
    display_buffer: wgpu::Buffer,
    offscreen: Offscreen,
    font_system: FontSystem,
    swash_cache: SwashCache,
    _text_cache: Cache,
    text_atlas: TextAtlas,
    viewport: Viewport,
    text_renderer: TextRenderer,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, faces: &[Option<Vec<u8>>]) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })
            .await
            .context("no suitable GPU adapter")?;
        tracing::info!(adapter = adapter.get_info().name, "using GPU");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("cedm"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .context("could not open GPU device")?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| *format == wgpu::TextureFormat::Bgra8UnormSrgb)
            .or_else(|| {
                capabilities
                    .formats
                    .iter()
                    .copied()
                    .find(wgpu::TextureFormat::is_srgb)
            })
            .or_else(|| capabilities.formats.first().copied())
            .context("surface offers no formats")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Srgb,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<Globals>() as u64),
                },
                count: None,
            }],
        });
        let sample_layout =
            texture_layout(&device, "sample layout", wgpu::TextureViewDimension::D2);
        let atlas_layout = texture_layout(&device, "atlas layout", wgpu::TextureViewDimension::D2);
        let scenery_layout = texture_layout(
            &device,
            "scenery layout",
            wgpu::TextureViewDimension::D2Array,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear clamp sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let (atlas, atlas_bind_group) = atlas(&device, &queue, &atlas_layout, &sampler, faces);
        let (scenery, scenery_bind_group) = scenery(&device, &scenery_layout, &sampler);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("LineXinBar shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders.wgsl").into()),
        });
        let background_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("background layout"),
            bind_group_layouts: &[Some(&globals_layout), None, None, Some(&scenery_layout)],
            immediate_size: 0,
        });
        // One instance per display, each carrying the rectangle of the surface
        // that display owns. The wallpaper is a function of where you are on a
        // *screen*, so the pass that draws it has to be told which screen it is
        // drawing, and a row of monitors is a row of instances rather than one
        // picture drawn wide.
        let display_attributes = wgpu::vertex_attr_array![0 => Float32x4];
        let background_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 4]>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &display_attributes,
        })];
        let background_pipeline = pipeline(
            &device,
            "background",
            &background_layout,
            &shader,
            "vs_background",
            "fs_background",
            format,
            &background_buffers,
            None,
        );
        let quad_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad layout"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&atlas_layout),
                Some(&sample_layout),
                Some(&scenery_layout),
            ],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
            4 => Float32x4, 5 => Float32, 6 => Float32, 7 => Float32, 8 => Float32x4,
            9 => Float32x4,
        ];
        let quad_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &attributes,
        })];
        let quad_pipeline = pipeline(
            &device,
            "glass quads",
            &quad_layout,
            &shader,
            "vs_quad",
            "fs_quad",
            format,
            &quad_buffers,
            // Straight colour in, premultiplied out. The quads are drawn over
            // nothing rather than over the wallpaper, so the target has to end
            // up holding what was drawn *and* how much of it there is: colour
            // weighted by its own coverage, alpha accumulated the way coverage
            // accumulates. Plain alpha blending answers the first and not the
            // second, which leaves every pane's own alpha reading lower than
            // the ink it carries — panes that then vanish where they were
            // supposed to be opaque.
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
        );
        let offscreen_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("offscreen shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../offscreen.wgsl").into()),
        });
        let offscreen_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("offscreen layout"),
            bind_group_layouts: &[Some(&sample_layout)],
            immediate_size: 0,
        });
        let downsample_pipeline = pipeline(
            &device,
            "downsample",
            &offscreen_layout,
            &offscreen_shader,
            "vs_fullscreen",
            "fs_downsample",
            format,
            &[],
            None,
        );
        // What the greeter drew, laid over the wallpaper. Premultiplied,
        // because that is what the pass above left in the scene.
        let compose_pipeline = pipeline(
            &device,
            "compose",
            &offscreen_layout,
            &offscreen_shader,
            "vs_fullscreen",
            "fs_blit",
            format,
            &[],
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let blit_pipeline = pipeline(
            &device,
            "blit",
            &offscreen_layout,
            &offscreen_shader,
            "vs_fullscreen",
            "fs_blit",
            format,
            &[],
            None,
        );

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });
        let instance_capacity = 256;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (instance_capacity * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let display_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("displays"),
            size: (displays::MAX * std::mem::size_of::<[f32; 4]>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let offscreen = Offscreen::new(
            &device,
            &sample_layout,
            &sampler,
            format,
            config.width,
            config.height,
        );

        let mut font_system = FontSystem::new();
        for face in [UI_FONT_REGULAR, UI_FONT_BOLD] {
            font_system.db_mut().load_font_data(face.to_vec());
        }
        let swash_cache = SwashCache::new();
        let text_cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &text_cache);
        let mut text_atlas = TextAtlas::new(&device, &queue, &text_cache, format);
        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );

        // Keep textures alive through their bind groups.
        let _ = (atlas, scenery);
        Ok(Self {
            _window: window,
            _instance: instance,
            surface,
            _adapter: adapter,
            device,
            queue,
            config,
            background_pipeline,
            quad_pipeline,
            downsample_pipeline,
            compose_pipeline,
            blit_pipeline,
            sample_layout,
            sampler,
            globals_buffer,
            globals_bind_group,
            atlas_bind_group,
            scenery_bind_group,
            instance_buffer,
            instance_capacity,
            display_buffer,
            offscreen,
            font_system,
            swash_cache,
            _text_cache: text_cache,
            text_atlas,
            viewport,
            text_renderer,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.offscreen = Offscreen::new(
            &self.device,
            &self.sample_layout,
            &self.sampler,
            self.config.format,
            width,
            height,
        );
    }

    pub fn render(&mut self, scene: &Scene, time: f32) -> anyhow::Result<()> {
        let shown = theme::theme();
        self.queue.write_buffer(
            &self.globals_buffer,
            0,
            bytemuck::bytes_of(&Globals {
                resolution: [self.config.width as f32, self.config.height as f32],
                time,
                blur: 0.0,
                window_rect: [0.0; 4],
                params: [0.0; 4],
                sky: shown.sky.map(|color| color.a(1.0)),
                accent: [
                    shown.accent.a(1.0),
                    shown.accent_soft.a(1.0),
                    shown.accent_deep.a(1.0),
                ],
                glow: shown.glow.a(1.0),
                covers: [[0.0; 4]; MAX_COVERS],
                hero: [0.0; 4],
            }),
        );
        let instances = scene.quads.iter().map(instance).collect::<Vec<_>>();
        self.ensure_instance_capacity(instances.len());
        if !instances.is_empty() {
            self.queue
                .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        }
        // A frame that names no display is one screen, and it is this whole
        // surface: the wallpaper has to be drawn somewhere, and the only honest
        // answer to "on which display" from a scene that was not asked is "all
        // of it", which is what a machine with one monitor means anyway.
        let whole = [[
            0.0,
            0.0,
            self.config.width as f32,
            self.config.height as f32,
        ]];
        let displays = match scene.displays.len() {
            0 => &whole[..],
            _ => &scene.displays[..scene.displays.len().min(displays::MAX)],
        };
        self.queue
            .write_buffer(&self.display_buffer, 0, bytemuck::cast_slice(displays));
        self.prepare_text(&scene.texts)?;

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(())
            }
            other => anyhow::bail!("could not acquire frame: {other:?}"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("CEDM frame"),
            });
        {
            let mut pass = onto(
                &mut encoder,
                &self.offscreen.wall_view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                "wallpaper",
            );
            pass.set_pipeline(&self.background_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(3, &self.scenery_bind_group, &[]);
            pass.set_vertex_buffer(0, self.display_buffer.slice(..));
            // Two triangles the size of one display, once per display. A strip
            // of surface that belongs to no display — the space under a shorter
            // monitor standing beside a taller one — is drawn on by none of
            // them and stays the black this target was cleared to, which is
            // what is in front of the user there.
            pass.draw(0..6, 0..displays.len() as u32);
        }
        // Emptied before anything reads it: the first run of panes may want a
        // snapshot before a single quad has been drawn, and what it must find
        // there is nothing rather than the frame before this one.
        onto(
            &mut encoder,
            &self.offscreen.scene_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "scene",
        );
        // The greeter's own drawing, over nothing. A pane reading the frame
        // behind it therefore finds the panes it is resting on and, wherever
        // there are none, an emptiness it answers by evaluating the wallpaper —
        // at the softness its frost asks for, which is the whole point.
        let stride = std::mem::size_of::<Instance>() as u64;
        let mut drawn = 0;
        for batch in glass_batches(&scene.quads, MAX_GLASS_BATCHES) {
            if batch.reads {
                self.snapshot(&mut encoder);
            }
            if batch.end == drawn {
                continue;
            }
            let mut pass = onto(
                &mut encoder,
                &self.offscreen.scene_view,
                wgpu::LoadOp::Load,
                "glass",
            );
            pass.set_pipeline(&self.quad_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);
            pass.set_bind_group(2, &self.offscreen.backdrop_source, &[]);
            pass.set_bind_group(3, &self.scenery_bind_group, &[]);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(drawn as u64 * stride..));
            pass.draw(0..6, 0..(batch.end - drawn) as u32);
            drawn = batch.end;
        }
        {
            let mut pass = onto(
                &mut encoder,
                &self.offscreen.wall_view,
                wgpu::LoadOp::Load,
                "compose",
            );
            pass.set_pipeline(&self.compose_pipeline);
            pass.set_bind_group(0, &self.offscreen.scene_source, &[]);
            pass.draw(0..3, 0..1);
        }
        // Onto the joined frame rather than into the scene: text is the one
        // thing drawn straight to the display, and its blending is the library's
        // own — correct over something, and thinning its own antialiased edges
        // over nothing.
        {
            let mut pass = onto(
                &mut encoder,
                &self.offscreen.wall_view,
                wgpu::LoadOp::Load,
                "text",
            );
            self.text_renderer
                .render(&self.text_atlas, &self.viewport, &mut pass)?;
        }
        {
            let mut pass = onto(
                &mut encoder,
                &view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                "present",
            );
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.offscreen.wall_source, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        self.text_atlas.trim();
        Ok(())
    }

    /// Read the frame that was last rendered back off the GPU, as `RGBA8`.
    ///
    /// The composed frame already exists as a texture — [`Self::render`] draws
    /// the whole screen into it and only the final pass blits that to the
    /// window — so this is a copy rather than a second rendering, and what it
    /// returns is exactly the pixels that were presented.
    ///
    /// For looking at what the greeter draws without owning a seat to draw it
    /// on: design review, and the golden frames the handover will eventually
    /// be tested with. It is not on any login path.
    pub fn capture(&mut self) -> anyhow::Result<(u32, u32, Vec<u8>)> {
        let (width, height) = self.size();
        // Copies out of a texture are addressed in rows of 256 bytes, so the
        // buffer is padded and the padding is dropped on the way out.
        let unpadded = width as usize * 4;
        let padded = unpadded.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: (padded * height as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("capture"),
            });
        encoder.copy_texture_to_buffer(
            whole(&self.offscreen.wall),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver
            .recv()
            .context("the GPU never answered the capture")?
            .context("could not map the captured frame")?;

        let mapped = slice
            .get_mapped_range()
            .map_err(|error| anyhow::anyhow!("could not read the captured frame: {error:?}"))?;
        let mut pixels = Vec::with_capacity(unpadded * height as usize);
        for row in 0..height as usize {
            let start = row * padded;
            pixels.extend_from_slice(&mapped[start..start + unpadded]);
        }
        drop(mapped);
        buffer.unmap();

        // The scene texture is `Bgra8UnormSrgb` on every adapter this runs on,
        // but the format is chosen from what the surface offers, so the two
        // orders are both possible and the caller is owed one of them.
        if !matches!(
            self.config.format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
        ) {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Ok((width, height, pixels))
    }

    fn ensure_instance_capacity(&mut self, needed: usize) {
        if needed <= self.instance_capacity {
            return;
        }
        self.instance_capacity = needed.next_power_of_two();
        self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (self.instance_capacity * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }

    fn prepare_text(&mut self, texts: &[Text]) -> anyhow::Result<()> {
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );
        let mut buffers = Vec::with_capacity(texts.len());
        for text in texts {
            let mut buffer = TextBuffer::new(
                &mut self.font_system,
                Metrics::new(text.size, text.size * 1.25),
            );
            buffer.set_size(Some(text.rect[2]), Some(text.rect[3]));
            let attrs = Attrs::new()
                .family(Family::Name(UI_FONT))
                .weight(if text.bold {
                    Weight::BOLD
                } else {
                    Weight::NORMAL
                });
            buffer.set_text(&text.content, &attrs, Shaping::Advanced, None);
            let align = match text.align {
                TextAlign::Left => None,
                TextAlign::Center => Some(glyphon::cosmic_text::Align::Center),
                TextAlign::Right => Some(glyphon::cosmic_text::Align::Right),
            };
            for line in &mut buffer.lines {
                line.set_align(align);
            }
            buffer.shape_until_scroll(&mut self.font_system, false);
            buffers.push(buffer);
        }
        let areas = buffers
            .iter()
            .zip(texts)
            .map(|(buffer, text)| TextArea {
                buffer,
                left: text.rect[0],
                top: text.rect[1],
                scale: 1.0,
                bounds: {
                    let [x, y, w, h] = match text.clip {
                        Some(clip) => intersection(text.rect, clip),
                        None => text.rect,
                    };
                    TextBounds {
                        left: x.floor() as i32,
                        top: y.floor() as i32,
                        right: (x + w).ceil() as i32,
                        bottom: (y + h).ceil() as i32,
                    }
                },
                default_color: TextColor::rgba(
                    (text.color[0] * 255.0) as u8,
                    (text.color[1] * 255.0) as u8,
                    (text.color[2] * 255.0) as u8,
                    (text.color[3] * 255.0) as u8,
                ),
                custom_glyphs: &[],
            })
            .collect::<Vec<_>>();
        self.text_renderer.prepare(
            &self.device,
            &self.queue,
            &mut self.font_system,
            &mut self.text_atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        )?;
        Ok(())
    }

    fn snapshot(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_texture(
            whole(&self.offscreen.scene),
            whole(&self.offscreen.backdrop),
            wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
        );
        for rung in 1..self.offscreen.rungs.len() {
            let mut pass = onto(
                encoder,
                &self.offscreen.rungs[rung],
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                "blur",
            );
            pass.set_pipeline(&self.downsample_pipeline);
            pass.set_bind_group(0, &self.offscreen.rung_sources[rung - 1], &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

impl Offscreen {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let extent = wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        };
        let plain = |label| wgpu::TextureDescriptor {
            label: Some(label),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let wall = device.create_texture(&plain("wall"));
        let scene = device.create_texture(&plain("scene"));
        let levels = BACKDROP_MIPS.min(width.min(height).max(2).ilog2().max(1));
        let backdrop = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("backdrop"),
            size: extent,
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bind = |view: &wgpu::TextureView, label| {
            texture_bind_group(device, layout, sampler, view, label)
        };
        let rungs = (0..levels)
            .map(|level| {
                backdrop.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("backdrop rung"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect::<Vec<_>>();
        let rung_sources = rungs
            .iter()
            .map(|view| bind(view, "backdrop rung"))
            .collect();
        let wall_view = wall.create_view(&Default::default());
        let wall_source = bind(&wall_view, "wall source");
        let scene_view = scene.create_view(&Default::default());
        let scene_source = bind(&scene_view, "scene source");
        let backdrop_source = bind(
            &backdrop.create_view(&Default::default()),
            "backdrop source",
        );
        Self {
            wall,
            wall_view,
            wall_source,
            scene,
            scene_view,
            scene_source,
            backdrop,
            backdrop_source,
            rungs,
            rung_sources,
        }
    }
}

fn texture_layout(
    device: &wgpu::Device,
    label: &str,
    dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: dimension,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn pipeline<'a>(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    vertex: &str,
    fragment: &str,
    format: wgpu::TextureFormat,
    buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex),
            buffers,
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// The greeter's own marks, in the atlas cells they are rasterised into.
///
/// Built in rather than looked up because a login screen runs before any
/// session does, on a machine that may have no icon theme past hicolor and no
/// desktop at all to have installed one.
///
/// Seven of them are `lxb-desktop`'s files unchanged — the four arrow caps, the
/// two controller hints and the keyboard's own close key — and three more are
/// its drawings under this repository's names, byte for byte from `<svg` on.
/// The rest are drawn here in the same material. See the guard below, which is
/// the shell's, and which is what keeps a new one from arriving flat.
const GLYPHS: [(u32, &[u8]); 13] = [
    (
        ARROW_LEFT_SLOT,
        include_bytes!("../../assets/glyphs/arrow-left.svg").as_slice(),
    ),
    (
        ARROW_DOWN_SLOT,
        include_bytes!("../../assets/glyphs/arrow-down.svg").as_slice(),
    ),
    (
        ARROW_UP_SLOT,
        include_bytes!("../../assets/glyphs/arrow-up.svg").as_slice(),
    ),
    (
        ARROW_RIGHT_SLOT,
        include_bytes!("../../assets/glyphs/arrow-right.svg").as_slice(),
    ),
    (
        KEYBOARD_HIDE_SLOT,
        include_bytes!("../../assets/glyphs/keyboard-hide.svg").as_slice(),
    ),
    (
        PAD_SELECT_SLOT,
        include_bytes!("../../assets/glyphs/pad-select.svg").as_slice(),
    ),
    (
        PAD_WEST_SLOT,
        include_bytes!("../../assets/glyphs/pad-west.svg").as_slice(),
    ),
    (
        SLEEP_SLOT,
        include_bytes!("../../assets/glyphs/sleep.svg").as_slice(),
    ),
    (
        RESTART_SLOT,
        include_bytes!("../../assets/glyphs/restart.svg").as_slice(),
    ),
    (
        POWER_SLOT,
        include_bytes!("../../assets/glyphs/power.svg").as_slice(),
    ),
    (
        USER_SWITCH_SLOT,
        include_bytes!("../../assets/glyphs/user-switch.svg").as_slice(),
    ),
    (
        SESSION_SLOT,
        include_bytes!("../../assets/glyphs/session.svg").as_slice(),
    ),
    (
        KEYBOARD_SHOW_SLOT,
        include_bytes!("../../assets/glyphs/keyboard-show.svg").as_slice(),
    ),
];

/// Build the atlas: the two painted cells, the drawings, and one cell for each
/// account that has a picture.
///
/// `faces` is straight `RGBA` at [`CELL`] square, in account order — see
/// [`crate::faces`], which is where they come from and why the greeter is
/// allowed to have them. It is written once, here, because the accounts are
/// enumerated once at start and do not change while a login screen is up.
fn atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    faces: &[Option<Vec<u8>>],
) -> (wgpu::Texture, wgpu::BindGroup) {
    let width = CELL * ATLAS_COLUMNS;
    let height = CELL * ATLAS_ROWS;
    let mut pixels = vec![0_u8; (width * height * 4) as usize];
    for y in 0..CELL {
        for x in 0..CELL {
            let offset = ((y * width + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            let half = (CELL as f32 - 1.0) * 0.5;
            let dx = (x as f32 - half) / half;
            let dy = (y as f32 - half) / half;
            let radius = (dx * dx + dy * dy).sqrt();
            let falloff = (-5.5 * radius * radius).exp();
            let edge = ((1.0 - radius) / 0.12).clamp(0.0, 1.0);
            let glow = ((y * width + CELL + x) * 4) as usize;
            pixels[glow..glow + 4].copy_from_slice(&[
                255,
                255,
                255,
                (falloff * edge * 255.0) as u8,
            ]);
        }
    }
    let mut into_cell = |slot: u32, cell: &[u8]| {
        let cell_x = (slot % ATLAS_COLUMNS) * CELL;
        let cell_y = (slot / ATLAS_COLUMNS) * CELL;
        for y in 0..CELL {
            let source = (y * CELL * 4) as usize;
            let destination = (((cell_y + y) * width + cell_x) * 4) as usize;
            pixels[destination..destination + (CELL * 4) as usize]
                .copy_from_slice(&cell[source..source + (CELL * 4) as usize]);
        }
    };
    for (slot, svg) in GLYPHS {
        if let Some(icon) = rasterise_svg(svg, CELL) {
            into_cell(slot, &icon);
        }
    }
    for (index, face) in faces.iter().enumerate().take(MAX_FACES) {
        if let Some(face) = face {
            if face.len() == (CELL * CELL * 4) as usize {
                into_cell(FACE_SLOT + index as u32, face);
            }
        }
    }
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("CEDM atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &pixels,
    );
    let view = texture.create_view(&Default::default());
    let group = texture_bind_group(device, layout, sampler, &view, "atlas");
    (texture, group)
}

fn rasterise_svg(data: &[u8], size: u32) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let tree_size = tree.size();
    let scale = (size as f32 / tree_size.width()).min(size as f32 / tree_size.height());
    let dx = (size as f32 - tree_size.width() * scale) * 0.5;
    let dy = (size as f32 - tree_size.height() * scale) * 0.5;
    let transform = tiny_skia::Transform::from_translate(dx, dy).pre_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.take();
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        for channel in &mut pixel[..3] {
            *channel = (*channel as u32 * 255 + alpha / 2)
                .checked_div(alpha)
                .unwrap_or(0)
                .min(255) as u8;
        }
    }
    Some(rgba)
}

fn scenery(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("empty scenery"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let group = texture_bind_group(device, layout, sampler, &view, "empty scenery");
    (texture, group)
}

fn texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn instance(quad: &Quad) -> Instance {
    let uv = if quad.slot == SOLID_SLOT {
        [
            0.5 / ATLAS_COLUMNS as f32,
            0.5 / ATLAS_ROWS as f32,
            0.5 / ATLAS_COLUMNS as f32,
            0.5 / ATLAS_ROWS as f32,
        ]
    } else {
        let column = quad.slot % ATLAS_COLUMNS;
        let row = quad.slot / ATLAS_COLUMNS;
        let step_x = 1.0 / ATLAS_COLUMNS as f32;
        let step_y = 1.0 / ATLAS_ROWS as f32;
        let inset_x = step_x * (1.5 / CELL as f32);
        let inset_y = step_y * (1.5 / CELL as f32);
        [
            column as f32 * step_x + inset_x,
            row as f32 * step_y + inset_y,
            (column + 1) as f32 * step_x - inset_x,
            (row + 1) as f32 * step_y - inset_y,
        ]
    };
    Instance {
        rect: quad.rect,
        uv,
        color: quad.color,
        shape: [quad.radius, quad.border, 0.0, quad.frost],
        material: [quad.thickness, 0.0, quad.gloss, quad.fade],
        corner: quad.corner,
        face_curve: quad.face_curve,
        drain: 0.0,
        // Cut to the display it stands on, which is what a display does to
        // what runs past it. On a machine with one screen that is the edge of
        // the framebuffer and the cut changes nothing; on a row of monitors it
        // is the difference between the glow under an avatar ending at the edge
        // of its own screen and four pixels of it appearing on the next one.
        cut: cut_to(quad.display),
        display: quad.display,
    }
}

/// The rectangle a pane is cut to, as its two corners: a display's own bounds,
/// or a box larger than any display for a pane that was never told which one it
/// is on.
fn cut_to([x, y, w, h]: [f32; 4]) -> [f32; 4] {
    if w > 0.0 && h > 0.0 {
        [x, y, x + w, y + h]
    } else {
        UNCUT
    }
}

/// What two rectangles have in common, which may be nothing: a width or a
/// height of zero or less means they do not meet at all.
pub fn intersection([ax, ay, aw, ah]: [f32; 4], [bx, by, bw, bh]: [f32; 4]) -> [f32; 4] {
    let x = ax.max(bx);
    let y = ay.max(by);
    [x, y, (ax + aw).min(bx + bw) - x, (ay + ah).min(by + bh) - y]
}

fn glass_batches(quads: &[Quad], limit: usize) -> Vec<Batch> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut reads = false;
    for (index, quad) in quads.iter().copied().enumerate() {
        if !quad.reads_backdrop() {
            continue;
        }
        let covered = quads[start..index]
            .iter()
            .copied()
            .any(|under| under.overlaps(quad));
        if covered && batches.len() + 1 < limit {
            batches.push(Batch { end: index, reads });
            start = index;
        }
        reads = true;
    }
    batches.push(Batch {
        end: quads.len(),
        reads,
    });
    batches
}

fn onto<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    label: &'a str,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn whole(texture: &wgpu::Texture) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The greeter's own marks have to be in the binary, have to draw
    /// something, and have to be made of the same material as each other —
    /// because nothing at runtime will notice if they are not. A glyph that
    /// fails to rasterise leaves an empty circle, which looks like a layout
    /// that meant to leave one; a flat one among moulded ones looks like the
    /// control that has not finished loading.
    ///
    /// `lxb-desktop`'s own guard over the same set of drawings, and it is the
    /// reason the shared ones can be copied across without being checked by
    /// eye each time.
    #[test]
    fn every_glyph_ships_neutral_and_draws_something() {
        assert_eq!(
            GLYPHS.len(),
            13,
            "four arrow caps, two controller hints, the keyboard's close key \
             and the button that raises it, the three machine actions, the \
             route to an account that was not listed, and the session badge"
        );
        let mut drawings = Vec::new();
        for (slot, svg) in GLYPHS {
            let rgba = rasterise_svg(svg, CELL).unwrap_or_else(|| {
                panic!("slot {slot} did not rasterise");
            });
            assert_eq!(rgba.len() as u32, CELL * CELL * 4);

            // Ink, not an empty square. A drawing that misses its viewBox
            // rasterises perfectly happily to nothing at all.
            let ink = rgba.chunks_exact(4).filter(|px| px[3] > 128).count();
            let share = ink as f32 / (CELL * CELL) as f32;
            assert!(
                (0.05..0.60).contains(&share),
                "slot {slot} covers {share:.3} of its cell"
            );

            for pixel in rgba.chunks_exact(4).filter(|px| px[3] == 255) {
                let [r, g, b] = [pixel[0], pixel[1], pixel[2]];
                // Neutral, so the colour the layout asks for is the colour it
                // gets: the atlas multiplies the quad's colour into the texel,
                // and a glyph with a hue of its own could only ever come out
                // muddier than the label beside it.
                //
                // Not the same as *white*. The moulded ones are shaded — one
                // lamp above the drawing, a shadow under it — and a grey under
                // a white tint is still that grey. What must not vary is the
                // balance between the channels.
                assert!(
                    r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1 && r.abs_diff(b) <= 1,
                    "slot {slot} has a colour of its own: {:?}",
                    &pixel[..3]
                );
                // And never so dark that it reads as a hole in the glyph.
                // Shadow on a white shell is a dimmer white; a shadow dark
                // enough to read as a hole has stopped being shading.
                assert!(r >= 128, "slot {slot} has a pixel at {r}, which is not ink");
            }
            drawings.push((slot, rgba));
        }

        // One cell each, and no two the same. Four arrow caps that rasterised
        // alike would point the wrong way three times out of four, and the two
        // keyboard marks differ by the direction of one triangle.
        let mut slots: Vec<u32> = drawings.iter().map(|(slot, _)| *slot).collect();
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), GLYPHS.len(), "two glyphs share a cell");
        assert!(
            slots.iter().all(|slot| *slot < ATLAS_COLUMNS * ATLAS_ROWS),
            "a glyph is outside the atlas"
        );
        for (index, (slot, rgba)) in drawings.iter().enumerate() {
            for (other, pixels) in &drawings[index + 1..] {
                assert_ne!(rgba, pixels, "slots {slot} and {other} draw the same");
            }
        }
    }

    /// Nothing drawn on one display reaches the display beside it.
    ///
    /// Panes are not all inside the thing they belong to: the light under an
    /// avatar is drawn wider than the avatar, and the glow at the head of the
    /// column reaches past its glass. Against the edge of a single screen the
    /// framebuffer takes care of that. On a surface that spans a row of
    /// monitors there is no edge there — the next display is — so the pane is
    /// cut to its own display, which is what an edge does to what runs past it.
    #[test]
    fn a_pane_is_cut_to_the_display_it_stands_on() {
        let second = [2560.0, 0.0, 1920.0, 1080.0];
        let spilling = instance(&Quad {
            // Reaching back over the seam onto the monitor to its left.
            rect: [2540.0, 300.0, 200.0, 200.0],
            display: second,
            ..Quad::default()
        });
        assert_eq!(spilling.cut, [2560.0, 0.0, 4480.0, 1080.0]);

        // And a frame that never mentioned a display is one screen, where
        // nothing is cutting anything.
        assert_eq!(instance(&Quad::default()).cut, UNCUT);
    }
}
