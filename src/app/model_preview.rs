use super::*;

pub(super) fn draw_model_preview_panel(
    ui: &mut Ui,
    tag: &TagFile,
    entry: &TagEntry,
    names: &TagNameIndex,
    source: Option<&TagSource>,
    state: &mut ModelPreviewState,
) {
    let is_model = is_model_group(entry.group_tag, names);
    if !is_model {
        return;
    }

    egui::CollapsingHeader::new(RichText::new("Render model").strong().color(text_dark()))
        .id_salt(("model_preview", &entry.key))
        .default_open(true)
        .show(ui, |ui| {
            ensure_model_preview_loaded(tag, entry, names, source, state);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Scale").color(subtle_dark()));
                ui.add(
                    egui::Slider::new(&mut state.scale, 0.05..=5.0)
                        .show_value(false)
                        .clamping(egui::SliderClamping::Always),
                );
                if ui.button("Reset").clicked() {
                    state.yaw = -0.45;
                    state.pitch = 0.25;
                    state.pan = Vec2::ZERO;
                    state.scale = 1.0;
                }
                ui.checkbox(&mut state.show_markers, "Markers");
                ui.checkbox(&mut state.show_wireframe, "Wireframe");
                ui.checkbox(&mut state.show_backfaces, "Backfaces");
                if ui.button("Refresh model").clicked() {
                    state.loaded_key = None;
                    state.data = None;
                    ensure_model_preview_loaded(tag, entry, names, source, state);
                }
            });

            let Some(data_result) = state.data.take() else {
                ui.label(RichText::new("No preview loaded").color(subtle_dark()));
                return;
            };
            let mut restore_data = Some(data_result);
            let data = match restore_data.as_ref().expect("preview data just set") {
                Ok(data) => data,
                Err(error) => {
                    ui.colored_label(Color32::from_rgb(150, 56, 44), error);
                    state.data = restore_data.take();
                    return;
                }
            };

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    let width = ui.available_width().min(470.0).max(280.0);
                    draw_model_viewport(ui, data, state, Vec2::new(width, 300.0));
                    ui.small(
                        RichText::new(format!(
                            "{} vertices, {} triangles",
                            data.preview.vertices.len(),
                            data.preview.indices.len() / 3
                        ))
                        .color(subtle_dark()),
                    );
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    draw_variant_controls(ui, data, state);
                });
            });
            state.data = restore_data.take();
        });
    ui.add_space(8.0);
}

fn ensure_model_preview_loaded(
    model_tag: &TagFile,
    entry: &TagEntry,
    names: &TagNameIndex,
    source: Option<&TagSource>,
    state: &mut ModelPreviewState,
) {
    if state.loaded_key.as_deref() == Some(entry.key.as_str()) && state.data.is_some() {
        return;
    }
    state.loaded_key = Some(entry.key.clone());
    state.data = Some(load_model_preview(model_tag, names, source).map(|data| {
        state.render_model_path = Some(data.render_model_path.clone());
        reset_model_preview_selection(state, &data, None);
        data
    }));
}

fn load_model_preview(
    model_tag: &TagFile,
    names: &TagNameIndex,
    source: Option<&TagSource>,
) -> Result<ModelPreviewData, String> {
    let Some((group_tag, rel_path)) = model_tag.root().read_tag_ref_with_group("render model")
    else {
        return Err("This model tag has no render model reference.".to_owned());
    };
    if rel_path.trim().is_empty() {
        return Err("This model tag has an empty render model reference.".to_owned());
    }
    let Some(TagSource::LooseFolder { root }) = source else {
        return Err("Render model preview requires a loaded loose-folder editing kit.".to_owned());
    };
    let extension = names
        .name_for(group_tag)
        .or_else(|| group_tag_to_extension(group_tag))
        .unwrap_or("render_model");
    let mut normalized = rel_path.replace('/', "\\");
    if let Some(stripped) = normalized.strip_suffix(&format!(".{extension}")) {
        normalized = stripped.to_owned();
    }
    let path = resolve_tag_path(root, &normalized, extension);
    if !path.exists() {
        return Err(format!(
            "Referenced render_model was not found: {}",
            path.display()
        ));
    }
    let render_entry = TagEntry {
        key: format!("file:{}", path.display()),
        display_path: format!("{}.{}", normalized.replace('\\', "/"), extension),
        group_tag,
        group_name: names.name_for(group_tag).map(str::to_owned),
        location: TagEntryLocation::LooseFile(path),
    };
    let render_tag =
        read_entry(source.unwrap(), &render_entry).map_err(|error| error.to_string())?;
    let render_model = RenderModel::from_tag(&render_tag).map_err(|error| error.to_string())?;
    let preview = render_model.to_preview();
    if preview.batches.is_empty() {
        return Err("Referenced render_model has no previewable draw batches.".to_owned());
    }
    let max_preview_edge = preview_edge_limit(preview.bounds_min, preview.bounds_max);
    let draw_triangles = build_model_source_triangles(&preview, max_preview_edge);
    Ok(ModelPreviewData {
        source_key: render_entry.key,
        render_model_path: normalized,
        preview,
        draw_triangles,
        variants: read_model_variants(model_tag),
    })
}

fn read_model_variants(tag: &TagFile) -> Vec<ModelVariantPreview> {
    let Some(variants) = tag.root().field_path("variants").and_then(|f| f.as_block()) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(variants.len());
    for index in 0..variants.len() {
        let Some(variant) = variants.element(index) else {
            continue;
        };
        let name =
            read_named_string(&variant, "name").unwrap_or_else(|| format!("variant {index}"));
        let mut regions = HashMap::new();
        if let Some(region_block) = variant.field("regions").and_then(|f| f.as_block()) {
            for region_index in 0..region_block.len() {
                let Some(region) = region_block.element(region_index) else {
                    continue;
                };
                let Some(region_name) = read_named_string(&region, "region name") else {
                    continue;
                };
                let permutation = region
                    .field("permutations")
                    .and_then(|f| f.as_block())
                    .and_then(|perms| perms.element(0))
                    .and_then(|perm| read_named_string(&perm, "permutation name"));
                if let Some(permutation) = permutation {
                    regions.insert(region_name, permutation);
                }
            }
        }
        out.push(ModelVariantPreview { name, regions });
    }
    out
}

fn read_named_string(tag_struct: &TagStruct<'_>, prefix: &str) -> Option<String> {
    for field in tag_struct.fields() {
        let name = field.name();
        if name.starts_with(prefix) {
            if let Some(TagFieldData::StringId(id) | TagFieldData::OldStringId(id)) = field.value()
            {
                if !id.string.is_empty() {
                    return Some(id.string);
                }
            }
        }
    }
    None
}

fn reset_model_preview_selection(
    state: &mut ModelPreviewState,
    data: &ModelPreviewData,
    variant: Option<usize>,
) {
    state.selected_variant = variant;
    state.region_selections.clear();
    for region in &data.preview.regions {
        let default_perm = region.permutations.first().cloned().unwrap_or_default();
        let permutation = variant
            .and_then(|idx| data.variants.get(idx))
            .and_then(|v| v.regions.get(&region.name))
            .cloned()
            .filter(|name| region.permutations.iter().any(|p| p == name))
            .unwrap_or(default_perm);
        state.region_selections.insert(
            region.name.clone(),
            ModelRegionSelection {
                enabled: !region.permutations.is_empty(),
                permutation,
            },
        );
    }
}

fn draw_variant_controls(ui: &mut Ui, data: &ModelPreviewData, state: &mut ModelPreviewState) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Variant").color(subtle_dark()));
        let selected = state
            .selected_variant
            .and_then(|idx| data.variants.get(idx))
            .map(|variant| variant.name.as_str())
            .unwrap_or("<None>");
        egui::ComboBox::from_id_salt(("model_preview_variant", &data.source_key))
            .selected_text(selected)
            .width(180.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(state.selected_variant.is_none(), "<None>")
                    .clicked()
                {
                    reset_model_preview_selection(state, data, None);
                }
                for index in 0..data.variants.len() {
                    if ui
                        .selectable_label(
                            state.selected_variant == Some(index),
                            &data.variants[index].name,
                        )
                        .clicked()
                    {
                        reset_model_preview_selection(state, data, Some(index));
                    }
                }
            });
    });
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .id_salt(("model_preview_regions", &data.source_key))
        .max_height(230.0)
        .show(ui, |ui| {
            for region in &data.preview.regions {
                let selection = state
                    .region_selections
                    .entry(region.name.clone())
                    .or_insert_with(|| ModelRegionSelection {
                        enabled: !region.permutations.is_empty(),
                        permutation: region.permutations.first().cloned().unwrap_or_default(),
                    });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut selection.enabled, "");
                    ui.label(RichText::new(&region.name).color(text_dark()).strong());
                    for permutation in &region.permutations {
                        let selected = selection.permutation == *permutation;
                        let response = ui.selectable_label(selected, permutation);
                        if response.clicked() {
                            selection.permutation = permutation.clone();
                            selection.enabled = true;
                        }
                    }
                });
            }
        });

    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        for label in [
            "Create new variant from selection...",
            "Update existing variant from selection...",
            "Drop Variant",
            "Drop Permutation",
            "Update",
        ] {
            ui.add_enabled(false, egui::Button::new(label))
                .on_hover_text("Variant writing is deferred to the model preview V2 pass.");
        }
    });
}

fn draw_model_viewport(
    ui: &mut Ui,
    data: &ModelPreviewData,
    state: &mut ModelPreviewState,
    desired_size: Vec2,
) {
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(228, 238, 244));
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, foundation_input_edge()));

    if response.dragged() {
        let delta = response.drag_delta();
        if ui.input(|i| i.modifiers.shift) {
            state.pan += delta;
        } else {
            state.yaw += delta.x * 0.01;
            state.pitch = (state.pitch + delta.y * 0.01).clamp(-1.45, 1.45);
        }
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll.abs() > f32::EPSILON {
            state.scale = (state.scale * (scroll / 450.0).exp()).clamp(0.05, 5.0);
        }
    }

    let camera = PreviewCamera::new(data, state, rect);
    collect_visible_triangles_into(
        data,
        &state.region_selections,
        state.show_backfaces,
        &camera,
        &mut state.projected_triangles,
    );
    state
        .projected_triangles
        .sort_by(|a, b| b.depth.total_cmp(&a.depth));

    let mut mesh = egui::epaint::Mesh::default();
    mesh.vertices.reserve(state.projected_triangles.len() * 3);
    mesh.indices.reserve(state.projected_triangles.len() * 3);
    for tri in &state.projected_triangles {
        let start = mesh.vertices.len() as u32;
        for point in tri.points {
            mesh.colored_vertex(point, tri.fill);
        }
        mesh.add_triangle(start, start + 1, start + 2);
    }
    painter.add(egui::Shape::mesh(mesh));

    let wire_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(20, 35, 45, 110));
    let wire_edge_limit = camera.screen_radius() * 0.55;
    if state.show_wireframe {
        for tri in &state.projected_triangles {
            draw_wireframe_edges(&painter, tri.points, wire_edge_limit, wire_stroke);
        }
    }

    if state.show_markers {
        for marker in &data.preview.markers {
            let projected = camera.project(marker.position);
            let color = Color32::from_rgb(30, 112, 165);
            painter.circle_filled(projected.pos, 4.0, color);
            painter.text(
                projected.pos + Vec2::new(6.0, -6.0),
                Align2::LEFT_BOTTOM,
                &marker.name,
                FontId::proportional(10.0),
                color,
            );
        }
    }
}

fn collect_visible_triangles_into(
    data: &ModelPreviewData,
    region_selections: &HashMap<String, ModelRegionSelection>,
    show_backfaces: bool,
    camera: &PreviewCamera,
    out: &mut Vec<ModelProjectedTriangle>,
) {
    out.clear();
    out.reserve(data.draw_triangles.len());
    for triangle in &data.draw_triangles {
        let Some(batch) = data.preview.batches.get(triangle.batch_index) else {
            continue;
        };
        let Some(selection) = region_selections.get(&batch.region_name) else {
            continue;
        };
        if !selection.enabled || selection.permutation != batch.permutation_name {
            continue;
        }
        let pa = camera.project(triangle.positions[0]);
        let pb = camera.project(triangle.positions[1]);
        let pc = camera.project(triangle.positions[2]);
        if !show_backfaces && projected_signed_area(pa.pos, pb.pos, pc.pos) >= -0.25 {
            continue;
        }
        if projected_max_edge(pa.pos, pb.pos, pc.pos) > camera.screen_radius() * 0.9 {
            continue;
        }
        if !camera.rect.intersects(egui::Rect::from_min_max(
            egui::pos2(
                pa.pos.x.min(pb.pos.x).min(pc.pos.x),
                pa.pos.y.min(pb.pos.y).min(pc.pos.y),
            ),
            egui::pos2(
                pa.pos.x.max(pb.pos.x).max(pc.pos.x),
                pa.pos.y.max(pb.pos.y).max(pc.pos.y),
            ),
        )) {
            continue;
        }
        out.push(ModelProjectedTriangle {
            points: [pa.pos, pb.pos, pc.pos],
            depth: (pa.depth + pb.depth + pc.depth) / 3.0,
            fill: triangle.fill,
        });
    }
}

fn draw_wireframe_edges(
    painter: &egui::Painter,
    points: [egui::Pos2; 3],
    max_edge: f32,
    stroke: Stroke,
) {
    for (a, b) in [
        (points[0], points[1]),
        (points[1], points[2]),
        (points[2], points[0]),
    ] {
        if screen_edge_length(a, b) <= max_edge {
            painter.line_segment([a, b], stroke);
        }
    }
}

fn projected_signed_area(a: egui::Pos2, b: egui::Pos2, c: egui::Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn projected_max_edge(a: egui::Pos2, b: egui::Pos2, c: egui::Pos2) -> f32 {
    screen_edge_length(a, b)
        .max(screen_edge_length(b, c))
        .max(screen_edge_length(c, a))
}

fn screen_edge_length(a: egui::Pos2, b: egui::Pos2) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn preview_edge_limit(min: [f32; 3], max: [f32; 3]) -> f32 {
    let dx = max[0] - min[0];
    let dy = max[1] - min[1];
    let dz = max[2] - min[2];
    let diagonal = (dx * dx + dy * dy + dz * dz).sqrt().max(0.001);
    diagonal * 0.45
}

fn triangle_max_edge(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    edge_length(a, b)
        .max(edge_length(b, c))
        .max(edge_length(c, a))
}

fn edge_length(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn build_model_source_triangles(
    preview: &RenderModelPreview,
    max_preview_edge: f32,
) -> Vec<ModelSourceTriangle> {
    let mut out = Vec::with_capacity(preview.indices.len() / 3);
    for (batch_index, batch) in preview.batches.iter().enumerate() {
        let start = batch.index_start as usize;
        let end = start
            .saturating_add(batch.index_count as usize)
            .min(preview.indices.len());
        let fill = material_color(batch.material_index);
        for chunk in preview.indices[start..end].chunks_exact(3) {
            let Some(a) = preview
                .vertices
                .get(chunk[0] as usize)
                .map(|vertex| vertex.position)
            else {
                continue;
            };
            let Some(b) = preview
                .vertices
                .get(chunk[1] as usize)
                .map(|vertex| vertex.position)
            else {
                continue;
            };
            let Some(c) = preview
                .vertices
                .get(chunk[2] as usize)
                .map(|vertex| vertex.position)
            else {
                continue;
            };
            let max_edge = triangle_max_edge(a, b, c);
            if max_edge > max_preview_edge {
                continue;
            }
            out.push(ModelSourceTriangle {
                batch_index,
                positions: [a, b, c],
                fill,
            });
        }
    }
    out
}

fn material_color(index: u16) -> Color32 {
    const COLORS: &[(u8, u8, u8)] = &[
        (132, 168, 188),
        (176, 166, 128),
        (142, 182, 150),
        (180, 136, 134),
        (150, 145, 190),
        (186, 154, 104),
        (126, 174, 176),
    ];
    let (r, g, b) = COLORS[index as usize % COLORS.len()];
    Color32::from_rgb(r, g, b)
}

struct ProjectedPoint {
    pos: egui::Pos2,
    depth: f32,
}

struct PreviewCamera {
    rect: egui::Rect,
    center: [f32; 3],
    radius: f32,
    yaw: f32,
    pitch: f32,
    scale: f32,
    pan: Vec2,
}

impl PreviewCamera {
    fn new(data: &ModelPreviewData, state: &ModelPreviewState, rect: egui::Rect) -> Self {
        let min = data.preview.bounds_min;
        let max = data.preview.bounds_max;
        let center = [
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ];
        let extent = [
            (max[0] - min[0]).abs(),
            (max[1] - min[1]).abs(),
            (max[2] - min[2]).abs(),
        ];
        let radius =
            ((extent[0] * extent[0] + extent[1] * extent[1] + extent[2] * extent[2]).sqrt() * 0.5)
                .max(0.001);
        Self {
            rect,
            center,
            radius,
            yaw: state.yaw,
            pitch: state.pitch,
            scale: state.scale,
            pan: state.pan,
        }
    }

    fn project(&self, point: [f32; 3]) -> ProjectedPoint {
        let mut x = (point[0] - self.center[0]) * self.scale;
        let mut y = (point[1] - self.center[1]) * self.scale;
        let mut z = (point[2] - self.center[2]) * self.scale;
        let (sy, cy) = self.yaw.sin_cos();
        let yaw_x = x * cy - y * sy;
        let yaw_y = x * sy + y * cy;
        x = yaw_x;
        y = yaw_y;
        let (sp, cp) = self.pitch.sin_cos();
        let pitch_y = y * cp - z * sp;
        let pitch_z = y * sp + z * cp;
        y = pitch_y;
        z = pitch_z;
        let fit = self.rect.width().min(self.rect.height()) / (self.radius * 2.2).max(0.001);
        let screen = self.rect.center() + self.pan + Vec2::new(x * fit, -z * fit);
        ProjectedPoint {
            pos: screen,
            depth: y,
        }
    }

    fn screen_radius(&self) -> f32 {
        let fit = self.rect.width().min(self.rect.height()) / (self.radius * 2.2).max(0.001);
        self.radius * self.scale * fit
    }
}
