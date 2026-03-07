use eframe::glow::HasContext;
use egui::{pos2, vec2, Color32, LayerId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use image::{ColorType, DynamicImage, GenericImageView, ImageBuffer, ImageFormat, Rgb, Rgba};
use resvg::FitTo;
use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use usvg::{ScreenSize, ShapeRendering, StrokeMiterlimit, TreeParsing};
use wasm_bindgen::prelude::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");
type SvgSelection = Option<(String, Vec<u8>)>;
type SvgSelectionResult = Result<SvgSelection, String>;
type SvgDialogReceiver = Receiver<SvgSelectionResult>;

#[wasm_bindgen]
extern "C" {
    fn download(fileName: &str, text: &str);
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct AmvApp {
    #[serde(skip)]
    state: Option<AppState>,
    selected_format: usize,
    lock_aspectratio: bool,
    #[serde(skip)]
    formats: Vec<ImageFormat>,
    #[serde(skip)]
    images: Vec<SubImage>,
    #[serde(skip)]
    svg_dialog_rx: Option<SvgDialogReceiver>,
    selected_image: Option<usize>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum DragMode {
    Image,
    CropTop,
    CropBottom,
    CropLeft,
    CropRight,

    CropLeftTop,
    CropRightTop,
    CropLeftBottom,
    CropRightBottom,
}

struct AppState {
    texture: egui::TextureHandle,
    image: image::DynamicImage,
    image_rect: Rect,
    cropping: Rect,
    drag_mode: DragMode,
    last_pos: Pos2,
    t: Pos2,
    back_ground_color: Color32,
    scale: f32,
}

pub enum ImageSource {
    Vector(usvg::Tree),
    Raster(image::DynamicImage),
}

struct SubImage {
    name: String,
    tree: ImageSource,
    texture: egui::TextureHandle,
    image_rect: Rect,
    cropping: Rect,
    original_aspectratio: f32,
    foreground_color: [u8; 3],
}

impl SubImage {
    fn change_forgroundcolor(&mut self, color: [u8; 3], ui: &Ui) -> Result<(), String> {
        if let ImageSource::Vector(tree) = &self.tree {
            let factor = 10;
            self.foreground_color = color;
            let pixmap_size = tree.size.to_screen_size();
            let [w, h] = [pixmap_size.width() * factor, pixmap_size.height() * factor];
            let mut pixmap = tiny_skia::Pixmap::new(w, h)
                .ok_or_else(|| format!("Failed to create SVG Pixmap of size {}x{}", w, h))?;
            resvg::render(
                &tree,
                FitTo::Size(w, h),
                Default::default(),
                pixmap.as_mut(),
            )
            .ok_or_else(|| "Failed to render SVG".to_owned())?;
            let mut image =
                egui::ColorImage::from_rgba_unmultiplied([w as _, h as _], pixmap.data());
            for pixel in image.pixels.iter_mut() {
                *pixel = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], pixel.a());
            }

            self.texture = ui.ctx().load_texture("svg", image, Default::default());
        }

        Ok(())
    }

    fn get_rect(&self, image_rect: &Rect, scale: f32) -> Rect {
        let min = vec2(self.image_rect.min.x, self.image_rect.min.y) * scale;
        let view_rect = Rect {
            min: image_rect.min + min,
            max: image_rect.min + min + self.image_rect.size() * scale,
        };
        return view_rect;
    }

    fn resize_for_aspectratio(&mut self) {
        // r = w / h | * h
        // w = r * h | / r
        // h = w / r

        let currentratio = self.image_rect.width() / self.image_rect.height();
        if currentratio > self.original_aspectratio {
            self.image_rect
                .set_height(self.image_rect.width() / self.original_aspectratio);
        } else if currentratio < self.original_aspectratio {
            self.image_rect
                .set_width(self.original_aspectratio * self.image_rect.height());
        }
    }
}

fn create_sub_image(svg_bytes: &[u8], ui: &Ui, name: &str) -> Result<SubImage, String> {
    if name.ends_with("svg") {
        let opt = usvg::Options {
            resources_dir: None,
            dpi: 96.0,
            // Default font is user-agent dependent so we can use whichever we like.
            font_family: "Times New Roman".to_owned(),
            font_size: 12.0,
            languages: vec!["en".to_string()],
            shape_rendering: usvg::ShapeRendering::GeometricPrecision,
            text_rendering: usvg::TextRendering::GeometricPrecision,
            image_rendering: usvg::ImageRendering::OptimizeQuality,
            default_size: usvg::Size::new(100.0, 100.0).unwrap(),
            image_href_resolver: usvg::ImageHrefResolver::default(),
        };
        let rtree = usvg::Tree::from_data(svg_bytes, &opt).map_err(|err| err.to_string())?;
        let factor = 10;
        let pixmap_size = rtree.size.to_screen_size();
        let [w, h] = [pixmap_size.width() * factor, pixmap_size.height() * factor];
        let mut pixmap = tiny_skia::Pixmap::new(w, h)
            .ok_or_else(|| format!("Failed to create SVG Pixmap of size {}x{}", w, h))?;
        resvg::render(
            &rtree,
            FitTo::Size(w, h),
            Default::default(),
            pixmap.as_mut(),
        )
        .ok_or_else(|| "Failed to render SVG".to_owned())?;
        let mut image = egui::ColorImage::from_rgba_unmultiplied([w as _, h as _], pixmap.data());
        for pixel in image.pixels.iter_mut() {
            *pixel = Color32::from_rgba_unmultiplied(255, 255, 255, pixel.a());
        }

        let texture = ui.ctx().load_texture("svg", image, Default::default());

        let sub_image = SubImage {
            name: name.into(),
            tree: ImageSource::Vector(rtree),
            texture: texture,
            image_rect: Rect {
                min: pos2(0., 0.),
                max: pos2((w / factor) as f32, (h / factor) as f32),
            },
            cropping: Rect {
                min: pos2(0., 0.),
                max: pos2(1., 1.),
            },
            original_aspectratio: (w as f32) / (h as f32),
            foreground_color: [255, 255, 255],
        };
        return Ok(sub_image);
    } else {
        let image: image::DynamicImage = image::load_from_memory(svg_bytes).unwrap();

        let size = [image.width() as _, image.height() as _];
        let image_buffer = image.to_rgba8();
        let pixels = image_buffer.as_flat_samples();
        let texture = ui.ctx().load_texture(
            "image",
            egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice()),
            Default::default(),
        );
        let sub_image = SubImage {
            name: name.into(),
            tree: ImageSource::Raster(image),
            texture: texture,
            image_rect: Rect {
                min: pos2(0., 0.),
                max: pos2((size[0] as f32), (size[1] as f32)),
            },
            cropping: Rect {
                min: pos2(0., 0.),
                max: pos2(1., 1.),
            },
            original_aspectratio: (size[0] as f32) / (size[1] as f32),
            foreground_color: [255, 255, 255],
        };
        return Ok(sub_image);
    }

    return Err("test".to_string());
}

async fn pick_svg_from_dialog() -> SvgSelectionResult {
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("SVG files", &["svg", "png", "jpg"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };

    let name = file.file_name();
    let bytes = file.read().await;

    Ok(Some((name, bytes)))
}

fn is_hover_over_subimage(
    image_rect: &Rect,
    ui: &Ui,
    images: &Vec<SubImage>,
    scale: f32,
    radius: f32,
) -> Option<usize> {
    for (i, image) in images.iter().enumerate() {
        let min = vec2(image.image_rect.min.x, image.image_rect.min.y) * scale;
        let view_rect = Rect {
            min: image_rect.min + min - 2. * vec2(radius, radius),
            max: image_rect.min + min + image.image_rect.size() * scale + 2. * vec2(radius, radius),
        };
        if is_hover(ui, view_rect) {
            return Some(i);
        }
    }
    return None;
}

fn get_hover_info(cropping: &Rect, ui: &Ui, radius: f32) -> DragMode {
    let vec = vec2(radius, radius);
    let point_factor = 2.;

    if is_hover_point(ui, cropping.left_top(), radius * point_factor) {
        return DragMode::CropLeftTop;
    }
    if is_hover_point(ui, cropping.left_bottom(), radius * point_factor) {
        return DragMode::CropLeftBottom;
    }
    if is_hover_point(ui, cropping.right_top(), radius * point_factor) {
        return DragMode::CropRightTop;
    }
    if is_hover_point(ui, cropping.right_bottom(), radius * point_factor) {
        return DragMode::CropRightBottom;
    }

    if is_hover(
        ui,
        egui::Rect {
            min: cropping.left_top() - vec,
            max: cropping.right_top() + vec,
        },
    ) {
        return DragMode::CropTop;
    }
    if is_hover(
        ui,
        egui::Rect {
            min: cropping.left_bottom() - vec,
            max: cropping.right_bottom() + vec,
        },
    ) {
        return DragMode::CropBottom;
    }
    if is_hover(
        ui,
        egui::Rect {
            min: cropping.left_top() - vec,
            max: cropping.left_bottom() + vec,
        },
    ) {
        return DragMode::CropLeft;
    }
    if is_hover(
        ui,
        egui::Rect {
            min: cropping.right_top() - vec,
            max: cropping.right_bottom() + vec,
        },
    ) {
        return DragMode::CropRight;
    }

    return DragMode::Image;
}

fn is_hover(ui: &Ui, rect: Rect) -> bool {
    if let Some(hover) = ui.input(|x| x.pointer.hover_pos()) {
        if hover.x >= rect.min.x
            && hover.x <= rect.max.x
            && hover.y >= rect.min.y
            && hover.y <= rect.max.y
        {
            return true;
        }
    }
    return false;
}

fn is_hover_point(ui: &Ui, point: Pos2, radius: f32) -> bool {
    if let Some(hover) = ui.input(|x| x.pointer.hover_pos()) {
        return hover.distance(point) <= radius;
    }
    return false;
}

pub fn calculate_average(img: &DynamicImage) -> [u8; 3] {
    // See: https://stackoverflow.com/a/2541680/6784368

    let (width, height) = img.dimensions();

    let block_size = 5;
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    let mut rgb: [u32; 3] = [0, 0, 0];
    let mut count = 0;

    loop {
        let (x1, y1) = next_coordinates(width, x, y, block_size);

        if y1 > height - 1 {
            break;
        }

        let pixel = img.get_pixel(x1, y1);

        rgb[0] += pixel.0[0] as u32;
        rgb[1] += pixel.0[1] as u32;
        rgb[2] += pixel.0[2] as u32;

        count += 1;
        x = x1;
        y = y1;
    }

    rgb[0] = !!(rgb[0] / count);
    rgb[1] = !!(rgb[1] / count);
    rgb[2] = !!(rgb[2] / count);

    return [rgb[0] as u8, rgb[1] as u8, rgb[2] as u8];
}

fn next_coordinates(width: u32, x: u32, y: u32, block_size: u32) -> (u32, u32) {
    let mut next_x = x;
    let mut next_y = y;
    let w = width - 1;

    if x < w && x + block_size < w {
        next_x += block_size;
    } else {
        next_x = 0;
        next_y = y + block_size;
    }

    (next_x, next_y)
}

fn to_f32(c: &Color32) -> (f32, f32, f32, f32) {
    (
        c.r() as f32 / 255.0,
        c.g() as f32 / 255.0,
        c.b() as f32 / 255.0,
        c.a() as f32 / 255.0,
    )
}

fn difference(c: &Color32, other: &Color32) -> f32 {
    let (r1, g1, b1, a1) = to_f32(c);
    let (r2, g2, b2, a2) = to_f32(other);

    let dr = r1 - r2;
    let dg = g1 - g2;
    let db = b1 - b2;
    let da = a1 - a2;

    (dr * dr + dg * dg + db * db + da * da).sqrt()
}

impl Default for AmvApp {
    fn default() -> Self {
        Self {
            state: None,
            selected_format: 0,
            lock_aspectratio: true,
            formats: vec![ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Qoi],
            images: vec![],
            svg_dialog_rx: None,
            selected_image: None,
        }
    }
}

impl AmvApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // if let Some(storage) = cc.storage {
        //     let app = eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        //     app.
        //     return app;
        // }

        Default::default()
    }

    fn start_svg_dialog(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.svg_dialog_rx = Some(rx);

        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || {
            let result = pollster::block_on(pick_svg_from_dialog());
            let _ = tx.send(result);
        });

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            let result = pick_svg_from_dialog().await;
            let _ = tx.send(result);
        });

        ctx.request_repaint();
    }

    fn take_svg_dialog_result(&mut self) -> Option<SvgSelectionResult> {
        let recv_result = match self.svg_dialog_rx.as_ref() {
            Some(rx) => rx.try_recv(),
            None => return None,
        };

        match recv_result {
            Ok(result) => {
                self.svg_dialog_rx = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.svg_dialog_rx = None;
                Some(Err(
                    "SVG file dialog task disconnected before completing".to_owned()
                ))
            }
        }
    }
}

impl eframe::App for AmvApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.svg_dialog_rx.is_some() {
            ctx.request_repaint();
        }

        let my_frame = egui::containers::Frame {
            inner_margin: egui::Margin::ZERO,
            outer_margin: egui::Margin::ZERO,
            shadow: eframe::epaint::Shadow::NONE,
            fill: Color32::BLACK,
            stroke: egui::Stroke::NONE,
            corner_radius: egui::CornerRadius::ZERO,
        };

        egui::Window::new("Layers").show(ctx, |ui| {
            if let Some(result) = self.take_svg_dialog_result() {
                match result {
                    Ok(Some((name, svg_bytes))) => match create_sub_image(&svg_bytes, ui, &name) {
                        Ok(sub_image) => self.images.insert(0,sub_image),
                        Err(err) => eprintln!("{err}"),
                    },
                    Ok(None) => {}
                    Err(err) => eprintln!("{err}"),
                }
            }

            if ui.button("+").clicked() {
                self.start_svg_dialog(ctx);
            }
            for image in self.images.iter_mut() {
                ui.horizontal(|ui| {
                    ui.label(&image.name);
                    let mut color = image.foreground_color.clone();
                    ui.color_edit_button_srgb(&mut color);
                    if color != image.foreground_color {
                        image.foreground_color = color;
                        image.change_forgroundcolor(color, ui);
                    }
                    if ui.button("auto").clicked() {
                        if let Some(state) = &self.state {
                            let crop = state.image.crop_imm(
                                image.image_rect.min.x as _,
                                image.image_rect.min.y as _,
                                image.image_rect.width() as _,
                                image.image_rect.height() as _,
                            );
                            let avg = calculate_average(&crop);
                            let avg = Color32::from_rgb(avg[0], avg[1], avg[2]);
                            let diff_black = difference(&avg, &Color32::BLACK) - 1.;
                            let diff_white = difference(&avg, &Color32::WHITE);
                            println!("b {} w {}", diff_black, diff_white);
                            let new_color = if diff_black > diff_white {
                                Color32::BLACK
                            } else {
                                Color32::WHITE
                            };
                            let _ = image.change_forgroundcolor(
                                [new_color.r(), new_color.g(), new_color.b()],
                                ui,
                            );
                        }
                    }
                });
            }
        });

        egui::CentralPanel::default()
            .frame(my_frame)
            .show(ctx, |ui| {
                let hover = ui
                    .allocate_response(ctx.content_rect().size(), Sense::click_and_drag())
                    .hover_pos();

                ui.with_layer_id(LayerId::background(), |ui| {
                    let mut state: &mut AppState = self.state.get_or_insert_with(|| {
                        let image_bytes = include_bytes!("../assets/starship.jpg");
                        let image: image::DynamicImage =
                            image::load_from_memory(image_bytes).unwrap();

                        let size = [image.width() as _, image.height() as _];
                        let image_buffer = image.to_rgba8();
                        let pixels = image_buffer.as_flat_samples();
                        let texture = ui.ctx().load_texture(
                            "image2",
                            egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice()),
                            Default::default(),
                        );

                        return AppState {
                            image,
                            texture,
                            image_rect: Rect {
                                min: pos2(0., 0.),
                                max: pos2(size[0] as f32, size[1] as f32),
                            },
                            last_pos: pos2(0., 0.),
                            cropping: Rect {
                                min: pos2(0., 0.),
                                max: pos2(1., 1.),
                            },
                            drag_mode: DragMode::Image,
                            t: pos2(0., 0.),
                            back_ground_color: Color32::WHITE,
                            scale: 1.,
                        };
                    });

                    let radius = 5.0;

                    //let hover = ctx.input(|x| x.pointer.hover_pos());
                    let pressed =
                        ui.input(|x| x.pointer.button_pressed(egui::PointerButton::Primary));
                    let down = ui.input(|x| x.pointer.button_down(egui::PointerButton::Primary));
                    let origin = ctx.input(|x| x.pointer.press_origin());

                    if let Some(hover) = hover {
                        let delta = ui.input(|x| x.zoom_delta());
                        let diff = state.image_rect.min - hover;
                        let new_diff = diff * delta;
                        let change_diff = new_diff - diff;
                        let image_size = state.image_rect.size();
                        state.image_rect.min = state.image_rect.min + change_diff;
                        state.image_rect.max = state.image_rect.min + image_size * delta;
                        state.scale = state.scale * delta;
                    }

                    let mut current_cropping = Rect {
                        min: state.image_rect.min
                            + vec2(
                                state.cropping.min.x * state.image_rect.size().x,
                                state.cropping.min.y * state.image_rect.size().y,
                            ),
                        max: state.image_rect.min
                            + vec2(
                                state.cropping.max.x * state.image_rect.size().x,
                                state.cropping.max.y * state.image_rect.size().y,
                            ),
                    };

                    let hover_image = is_hover_over_subimage(
                        &state.image_rect,
                        ui,
                        &self.images,
                        state.scale,
                        radius,
                    );
                    let selected_rect = if let Some(selected) = self.selected_image {
                        self.images[selected].get_rect(&state.image_rect, state.scale)
                    } else {
                        current_cropping
                    };
                    let crop_hover_info = get_hover_info(&selected_rect, ui, radius);

                    if let Some(hover) = hover {
                        if pressed {
                            state.drag_mode = crop_hover_info;
                            self.selected_image = hover_image;

                            state.last_pos = match &state.drag_mode {
                                DragMode::Image => {
                                    if let Some(selected) = self.selected_image {
                                        let min = self.images[selected].image_rect.min;
                                        state.image_rect.min + vec2(min.x, min.y) * state.scale
                                    } else {
                                        state.last_pos
                                    }
                                }
                                DragMode::CropTop => current_cropping.left_top(),
                                DragMode::CropBottom => current_cropping.right_bottom(),
                                DragMode::CropLeft => current_cropping.left_top(),
                                DragMode::CropLeftTop => current_cropping.left_top(),
                                DragMode::CropLeftBottom => current_cropping.left_bottom(),
                                DragMode::CropRight => current_cropping.right_bottom(),
                                DragMode::CropRightTop => current_cropping.right_top(),
                                DragMode::CropRightBottom => {
                                    if let Some(selected) = self.selected_image {
                                        state.image_rect.min
                                            + vec2(
                                                self.images[selected].image_rect.right_bottom().x,
                                                self.images[selected].image_rect.right_bottom().y,
                                            ) * state.scale
                                    } else {
                                        current_cropping.right_bottom()
                                    }
                                }
                            };
                            state.t = match &state.drag_mode {
                                DragMode::Image => {
                                    if let Some(selected) = self.selected_image {
                                        let min = self.images[selected].image_rect.min;
                                        min
                                    } else {
                                        pos2(0., 0.)
                                    }
                                }
                                DragMode::CropTop => state.cropping.left_top(),
                                DragMode::CropBottom => state.cropping.right_bottom(),
                                DragMode::CropLeft => state.cropping.left_top(),
                                DragMode::CropRight => state.cropping.right_bottom(),
                                DragMode::CropLeftTop => state.cropping.left_top(),
                                DragMode::CropLeftBottom => state.cropping.left_bottom(),
                                DragMode::CropRightTop => state.cropping.right_top(),
                                DragMode::CropRightBottom => {
                                    if let Some(selected) = self.selected_image {
                                        self.images[selected].image_rect.right_bottom()
                                    } else {
                                        state.cropping.right_bottom()
                                    }
                                }
                            };

                            state.last_pos = state.image_rect.min;
                        }
                        if down {
                            if let Some(origin) = origin {
                                let diff = hover - origin;
                                let new_pos = state.last_pos + diff;
                                let image_size = state.image_rect.size();
                                let image_pos = state.image_rect.min;
                                match &state.drag_mode {
                                    DragMode::Image => {
                                        if let Some(selected) = self.selected_image {
                                            let image_size =
                                                self.images[selected].image_rect.size();
                                            let new_pos = new_pos - state.image_rect.min;
                                            let new_pos = vec2(
                                                new_pos.x / state.scale,
                                                new_pos.y / state.scale,
                                            );
                                            self.images[selected].image_rect.min =
                                                state.t + new_pos;
                                            self.images[selected].image_rect.max =
                                                state.t + new_pos + image_size;
                                        } else {
                                            state.image_rect.min = new_pos;
                                            state.image_rect.max = new_pos + image_size;
                                        }
                                    }
                                    DragMode::CropTop => {
                                        state.cropping.min.y =
                                            state.t.y + (new_pos.y - image_pos.y) / image_size.y;
                                        current_cropping.min.y =
                                            image_pos.y + state.cropping.min.y * image_size.y;
                                    }
                                    DragMode::CropBottom => {
                                        state.cropping.max.y =
                                            state.t.y + (new_pos.y - image_pos.y) / image_size.y;
                                        current_cropping.max.y =
                                            image_pos.y + state.cropping.max.y * image_size.y;
                                    }
                                    DragMode::CropLeft => {
                                        state.cropping.min.x =
                                            state.t.x + (new_pos.x - image_pos.x) / image_size.x;
                                        current_cropping.min.x =
                                            image_pos.x + state.cropping.min.x * image_size.x;
                                    }
                                    DragMode::CropRight => {
                                        state.cropping.max.x =
                                            state.t.x + (new_pos.x - image_pos.x) / image_size.x;
                                        current_cropping.max.x =
                                            image_pos.x + state.cropping.max.x * image_size.x;
                                    }
                                    DragMode::CropLeftTop => {
                                        state.cropping.min.x =
                                            state.t.x + (new_pos.x - image_pos.x) / image_size.x;
                                        state.cropping.min.y =
                                            state.t.y + (new_pos.y - image_pos.y) / image_size.y;
                                        current_cropping.min.x =
                                            image_pos.x + state.cropping.min.x * image_size.x;
                                        current_cropping.min.y =
                                            image_pos.y + state.cropping.min.y * image_size.y;
                                    }
                                    DragMode::CropRightTop => {
                                        state.cropping.max.x =
                                            state.t.x + (new_pos.x - image_pos.x) / image_size.x;
                                        state.cropping.min.y =
                                            state.t.y + (new_pos.y - image_pos.y) / image_size.y;
                                        current_cropping.max.x =
                                            image_pos.x + state.cropping.max.x * image_size.x;
                                        current_cropping.min.y =
                                            image_pos.y + state.cropping.min.y * image_size.y;
                                    }
                                    DragMode::CropLeftBottom => {
                                        state.cropping.min.x =
                                            state.t.x + (new_pos.x - image_pos.x) / image_size.x;
                                        state.cropping.max.y =
                                            state.t.y + (new_pos.y - image_pos.y) / image_size.y;
                                        current_cropping.min.x =
                                            image_pos.x + state.cropping.min.x * image_size.x;
                                        current_cropping.max.y =
                                            image_pos.y + state.cropping.max.y * image_size.y;
                                    }
                                    DragMode::CropRightBottom => {
                                        if let Some(selected) = self.selected_image {
                                            let new_pos = new_pos - state.image_rect.min;
                                            let new_pos = vec2(
                                                new_pos.x / state.scale,
                                                new_pos.y / state.scale,
                                            );
                                            self.images[selected].image_rect.max.x =
                                                state.t.x + new_pos.x;
                                            self.images[selected].image_rect.max.y =
                                                state.t.y + new_pos.y;

                                            if self.lock_aspectratio {
                                                self.images[selected].resize_for_aspectratio();
                                            }
                                        } else {
                                            state.cropping.max.x = state.t.x
                                                + (new_pos.x - image_pos.x) / image_size.x;
                                            state.cropping.max.y = state.t.y
                                                + (new_pos.y - image_pos.y) / image_size.y;
                                            current_cropping.max.x =
                                                image_pos.x + state.cropping.max.x * image_size.x;
                                            current_cropping.max.y =
                                                image_pos.y + state.cropping.max.y * image_size.y;
                                        }
                                    }
                                };
                            }
                        }
                    }

                    ui.painter()
                        .rect_filled(current_cropping, 0., state.back_ground_color);

                    // state.image_rect.size()
                    egui::Image::new(&state.texture).paint_at(ui, state.image_rect);

                    for (i, image) in self.images.iter().enumerate().rev() {
                        let min =
                            vec2(image.image_rect.min.x, image.image_rect.min.y) * state.scale;
                        let view_rect = Rect {
                            min: state.image_rect.min + min,
                            max: state.image_rect.min + min + image.image_rect.size() * state.scale,
                        };
                        // , view_rect.size()
                        egui::Image::new(&image.texture).paint_at(ui, view_rect);

                        if self.selected_image == Some(i) || hover_image == Some(i) {
                            draw_crop_rect(ui, &view_rect, crop_hover_info);
                        }
                    }

                    draw_crop_blending(ui, &current_cropping, &state.image_rect);
                    if self.selected_image.is_none() {
                        draw_crop_rect(ui, &current_cropping, crop_hover_info);
                    }
                });
            });

        egui::TopBottomPanel::bottom("my_panel").show(ctx, |ui| {
            let mut texture: &mut AppState = self.state.get_or_insert_with(|| {
                let image_bytes = include_bytes!("../assets/starship.jpg");
                let image: image::DynamicImage = image::load_from_memory(image_bytes).unwrap();

                let size = [image.width() as _, image.height() as _];
                let image_buffer = image.to_rgba8();
                let pixels = image_buffer.as_flat_samples();
                let texture = ui.ctx().load_texture(
                    "image2",
                    egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice()),
                    Default::default(),
                );

                return AppState {
                    image,
                    texture,
                    image_rect: Rect {
                        min: pos2(0., 0.),
                        max: pos2(size[0] as f32, size[1] as f32),
                    },
                    last_pos: pos2(0., 0.),
                    cropping: Rect {
                        min: pos2(0., 0.),
                        max: pos2(1., 1.),
                    },
                    drag_mode: DragMode::Image,
                    t: pos2(0., 0.),
                    back_ground_color: Color32::WHITE,
                    scale: 1.,
                };
            });

            ui.horizontal(|ui| {
                egui::ComboBox::from_label("")
                    .selected_text(
                        self.formats[self.selected_format]
                            .extensions_str()
                            .first()
                            .unwrap_or(&"unkown")
                            .to_string(),
                    )
                    .show_ui(ui, |ui| {
                        for (i, format) in self.formats.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_format,
                                i,
                                format
                                    .extensions_str()
                                    .first()
                                    .unwrap_or(&"unkown")
                                    .to_string(),
                            );
                        }
                    });
                if ui.button("Download").clicked() {
                    let mut crop = crop_image(
                        &texture.image,
                        &texture.cropping,
                        &texture.back_ground_color,
                    );
                    for image in self.images.iter() {
                        if let ImageSource::Vector(tree) = &image.tree {
                            let [w, h] = [
                                image.image_rect.width() as u32,
                                image.image_rect.height() as u32,
                            ];
                            let mut pixmap = tiny_skia::Pixmap::new(w, h)
                                .ok_or_else(|| {
                                    format!("Failed to create SVG Pixmap of size {}x{}", w, h)
                                })
                                .unwrap();
                            resvg::render(
                                &tree,
                                FitTo::Size(w, h),
                                Default::default(),
                                pixmap.as_mut(),
                            )
                            .ok_or_else(|| "Failed to render SVG".to_owned())
                            .unwrap();
                            let mut vec = pixmap.data().to_vec();
                            for i in (0..vec.len()).step_by(4) {
                                vec[i + 0] = image.foreground_color[0];
                                vec[i + 1] = image.foreground_color[1];
                                vec[i + 2] = image.foreground_color[2];
                            }
                            let buffer = ImageBuffer::from_vec(w, h, vec).unwrap();
                            let img = image::DynamicImage::ImageRgba8(buffer);
                            image::imageops::overlay(
                                &mut crop,
                                &img,
                                (image.image_rect.min.x
                                    - texture.cropping.min.x * (texture.image.width() as f32))
                                    as i64,
                                (image.image_rect.min.y
                                    - texture.cropping.min.y * (texture.image.height() as f32))
                                    as i64,
                            );
                        }
                    }
                    export_image(&crop, self.formats[self.selected_format]);
                }

                ui.color_edit_button_srgba(&mut texture.back_ground_color);

                ui.label(VERSION);
            });
        });

        ctx.request_repaint();
    }
}

fn draw_crop_blending(ui: &Ui, crop: &Rect, image: &Rect) {
    let color = Color32::from_rgba_premultiplied(0, 0, 0, 210);
    if crop.min.x > image.min.x {
        ui.painter().rect_filled(
            Rect {
                min: image.min,
                max: pos2(crop.min.x, image.max.y),
            },
            0.,
            color,
        );
    }
    if crop.min.y > image.min.y {
        ui.painter().rect_filled(
            Rect {
                min: pos2(crop.min.x, image.min.y),
                max: crop.right_top(),
            },
            0.,
            color,
        );
    }
    if crop.max.x < image.max.x {
        ui.painter().rect_filled(
            Rect {
                min: pos2(crop.max.x, image.min.y),
                max: image.max,
            },
            0.,
            color,
        );
    }
    if crop.max.y < image.max.y {
        ui.painter().rect_filled(
            Rect {
                min: crop.left_bottom(),
                max: pos2(crop.max.x, image.max.y),
            },
            0.,
            color,
        );
    }
}

fn draw_crop_rect(ui: &Ui, crop: &Rect, drag_mode: DragMode) {
    let color = Color32::GREEN;
    let hover_color = Color32::BLUE;
    let stroke = Stroke::new(2., color);
    let hover_stroke = Stroke::new(2., hover_color);
    let radius = 5.0;

    ui.painter().line_segment(
        [crop.left_top(), crop.right_top()],
        if drag_mode == DragMode::CropTop {
            hover_stroke
        } else {
            stroke
        },
    );
    ui.painter().line_segment(
        [crop.left_bottom(), crop.right_bottom()],
        if drag_mode == DragMode::CropBottom {
            hover_stroke
        } else {
            stroke
        },
    );
    ui.painter().line_segment(
        [crop.left_top(), crop.left_bottom()],
        if drag_mode == DragMode::CropLeft {
            hover_stroke
        } else {
            stroke
        },
    );
    ui.painter().line_segment(
        [crop.right_top(), crop.right_bottom()],
        if drag_mode == DragMode::CropRight {
            hover_stroke
        } else {
            stroke
        },
    );
    ui.painter().circle_filled(
        crop.left_top(),
        radius,
        if drag_mode == DragMode::CropLeftTop {
            hover_color
        } else {
            color
        },
    );
    ui.painter().circle_filled(
        crop.left_bottom(),
        radius,
        if drag_mode == DragMode::CropLeftBottom {
            hover_color
        } else {
            color
        },
    );
    ui.painter().circle_filled(
        crop.right_top(),
        radius,
        if drag_mode == DragMode::CropRightTop {
            hover_color
        } else {
            color
        },
    );
    ui.painter().circle_filled(
        crop.right_bottom(),
        radius,
        if drag_mode == DragMode::CropRightBottom {
            hover_color
        } else {
            color
        },
    );
}

fn export_image(image: &image::DynamicImage, format: image::ImageFormat) {
    let mut vec = Vec::new();
    let mut c = Cursor::new(&mut vec);
    image.write_to(&mut c, format).unwrap();
    let base64 = base64::encode(vec);

    let suffix = format.extensions_str().first().unwrap();
    unsafe {
        download(&format!("image.{}", suffix), &base64);
    }
}

fn crop_image(image: &DynamicImage, cropping: &Rect, back_ground_color: &Color32) -> DynamicImage {
    let width = (image.width() as f32);
    let height = (image.height() as f32);
    if cropping.min.x < 0. || cropping.min.y < 0. || cropping.max.x > 1. || cropping.max.y < 1. {
        let x = (width * cropping.min.x) as i32;
        let y = (height * cropping.min.y) as i32;
        let w = (width * cropping.max.x) as i32 - x;
        let h = (height * cropping.max.y) as i32 - y;

        let mut img = create_image(w as u32, h as u32, back_ground_color);

        let crop = image.crop_imm(
            x.max(0) as u32,
            y.max(0) as u32,
            image.width().min(w as u32),
            image.height().min(h as u32),
        );
        image::imageops::overlay(
            &mut img,
            &crop,
            (x.max(0) - x) as i64,
            (y.max(0) - y) as i64,
        );
        return img;
    } else {
        let x = (width * cropping.min.x) as u32;
        let y = (height * cropping.min.y) as u32;
        let w = (width * cropping.max.x) as u32 - x;
        let h = (height * cropping.max.y) as u32 - y;
        let crop = image.crop_imm(x, y, w, h);
        return crop;
    }
}

fn create_image(width: u32, height: u32, back_ground_color: &Color32) -> DynamicImage {
    let mut imgbuf = ImageBuffer::<Rgba<u8>, _>::new(width, height);

    for (_x, _y, pixel) in imgbuf.enumerate_pixels_mut() {
        *pixel = image::Rgba([
            back_ground_color.r(),
            back_ground_color.g(),
            back_ground_color.b(),
            back_ground_color.a(),
        ]);
    }

    DynamicImage::ImageRgba8(imgbuf)
}
