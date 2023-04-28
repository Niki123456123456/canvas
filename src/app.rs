use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use image::ColorType;
use std::io::Cursor;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    fn download(fileName: &str, text: &str);
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // #[serde(skip)]
pub struct AmvApp {
    #[serde(skip)]
    texture: Option<ImgData>,
}
#[derive(Clone)]
enum DragMode {
    Image,
    CropTop,
    CropBottom,
    CropLeft,
    CropRight,
}

struct ImgData {
    texture: egui::TextureHandle,
    image: image::DynamicImage,
    image_size: Vec2,
    image_pos: Pos2,
    last_pos: Pos2,
    cropping: Rect,
    drag_mode: DragMode,
}

#[derive(Clone)]
struct CropHoverInfo {
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
}

impl Into<DragMode> for CropHoverInfo {
    fn into(self) -> DragMode {
       if self.top {
        return DragMode::CropTop;
       }
       if self.bottom {
        return DragMode::CropBottom;
       }
       if self.left {
        return DragMode::CropLeft;
       }
       if self.right {
        return DragMode::CropRight;
       }
       return DragMode::Image;
    }
}

fn get_hover_info(currentClipping: &Rect, ui: &Ui) -> CropHoverInfo {
    let vec = vec2(5., 5.);
    let info = CropHoverInfo {
        top: is_hover(ui, egui::Rect {
            min: currentClipping.left_top() - vec,
            max: currentClipping.right_top() + vec,
        }),
        bottom: is_hover(ui, egui::Rect {
            min: currentClipping.left_bottom() - vec,
            max: currentClipping.right_bottom() + vec,
        }),
        left: is_hover(ui, egui::Rect {
            min: currentClipping.left_top() - vec,
            max: currentClipping.left_bottom() + vec,
        }),
        right: is_hover(ui, egui::Rect {
            min: currentClipping.right_top() - vec,
            max: currentClipping.right_bottom() + vec,
        }),
    };
    return info;
}

fn is_hover(ui: &Ui, rect: Rect) -> bool {
    ui.interact(rect, ui.next_auto_id(), Sense::hover())
        .hovered()
}

impl Default for AmvApp {
    fn default() -> Self {
        Self { texture: None }
    }
}

impl AmvApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Default::default()
    }
}

impl eframe::App for AmvApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::Area::new("area")
            .fixed_pos(Pos2::new(0., 0.))
            .show(ctx, |ui| {
                ui.allocate_at_least(ctx.screen_rect().size(), Sense::click());
                let mut texture: &mut ImgData = self.texture.get_or_insert_with(|| {
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

                    return ImgData {
                        image,
                        texture,
                        image_size: Vec2::new(500., 500.),
                        image_pos: pos2(0., 0.),
                        last_pos: pos2(0., 0.),
                        cropping: Rect {
                            min: pos2(0., 0.),
                            max: pos2(1., 1.),
                        },
                        drag_mode: DragMode::Image
                    };
                });

                let hover = ctx.input(|x| x.pointer.hover_pos());
                let pressed = ui.input(|x| x.pointer.button_pressed(egui::PointerButton::Primary));
                let down = ui.input(|x| x.pointer.button_down(egui::PointerButton::Primary));
                let origin = ctx.input(|x| x.pointer.press_origin());
                let mut currentClipping = Rect {
                    min: texture.image_pos
                        + vec2(
                            texture.cropping.min.x * texture.image_size.x,
                            texture.cropping.min.y * texture.image_size.y,
                        ),
                    max: texture.image_pos
                        + vec2(
                            texture.cropping.max.x * texture.image_size.x,
                            texture.cropping.max.y * texture.image_size.y,
                        ),
                };
                let crop_hover_info = get_hover_info(&currentClipping, ui);
                

                if let Some(hover) = hover {
                    if pressed {
                        texture.drag_mode = crop_hover_info.clone().into();
                            texture.last_pos = 
                            match &texture.drag_mode {
                                DragMode::Image => texture.last_pos,
                                DragMode::CropTop => currentClipping.left_top(),
                                DragMode::CropBottom => currentClipping.right_bottom(),
                                DragMode::CropLeft => currentClipping.left_top(),
                                DragMode::CropRight => currentClipping.right_bottom(),
                            };

                        texture.last_pos = texture.image_pos;
                    }
                    if down {
                        if let Some(origin) = origin {
                            let diff = hover - origin;
                            match &texture.drag_mode {
                                DragMode::Image => {
                                    texture.image_pos = texture.last_pos + diff;
                                },
                                DragMode::CropTop => {
                                    let newTop = texture.last_pos + diff;
                                    texture.cropping.min.y = (newTop.y - texture.image_pos.y) / texture.image_size.y;
                                    currentClipping.min.y = newTop.y;
                                },
                                DragMode::CropBottom => {},
                                DragMode::CropLeft => {},
                                DragMode::CropRight => {},
                            };
                            
                        }
                    }
                    let delta = ui.input(|x| x.zoom_delta());
                    let diff = texture.image_pos - hover;
                    let new_diff = diff * delta;
                    let change_diff = new_diff - diff;
                    texture.image_pos = texture.image_pos + change_diff;

                    texture.image_size = texture.image_size * delta;
                }

                

                egui::Image::new(&texture.texture, texture.image_size).paint_at(
                    ui,
                    egui::Rect {
                        min: texture.image_pos,
                        max: texture.image_pos + texture.image_size,
                    },
                );

                let stroke = Stroke::new(2., Color32::GREEN);
                let hover_stroke = Stroke::new(2., Color32::BLUE);

                ui.painter().line_segment(
                    [currentClipping.left_top(), currentClipping.right_top()],
                    if crop_hover_info.top {hover_stroke} else {stroke},
                );
                ui.painter().line_segment(
                    [
                        currentClipping.left_bottom(),
                        currentClipping.right_bottom(),
                    ],
                    if crop_hover_info.bottom {hover_stroke} else {stroke},
                );
                ui.painter().line_segment(
                    [currentClipping.left_top(), currentClipping.left_bottom()],
                    if crop_hover_info.left {hover_stroke} else {stroke},
                );
                ui.painter().line_segment(
                    [currentClipping.right_top(), currentClipping.right_bottom()],
                    if crop_hover_info.right {hover_stroke} else {stroke},
                );
            });

        egui::TopBottomPanel::bottom("my_panel").show(ctx, |ui| {
            let texture: &ImgData = self.texture.as_ref().unwrap();
            if ui.button("Download").clicked() {
                let mut vec = Vec::new();
                let mut c = Cursor::new(&mut vec);
                texture
                    .image
                    .write_to(&mut c, image::ImageOutputFormat::Png)
                    .unwrap();
                let base64 = base64::encode(vec);
                download("test.png", &base64);
            }
        });
    }
}
