use std::collections::HashSet;
use std::sync::Arc;

pub struct FontManager {
    system_families: Vec<String>,
    loaded_variants: HashSet<String>,
    font_defs: egui::FontDefinitions,
    initialized: bool,
    needs_apply: bool,
    // Fonts loaded this frame - not yet in the atlas (set_fonts takes effect next frame)
    newly_loaded: HashSet<String>,
}

impl FontManager {
    pub fn new() -> Self {
        Self {
            system_families: Vec::new(),
            loaded_variants: HashSet::new(),
            font_defs: egui::FontDefinitions::default(),
            initialized: false,
            needs_apply: false,
            newly_loaded: HashSet::new(),
        }
    }

    /// Call at the start of each frame. Fonts loaded last frame are now in the atlas.
    pub fn begin_frame(&mut self) {
        self.newly_loaded.clear();
    }

    /// Call at the end of each frame. Applies pending font changes so the atlas
    /// rebuilds on the next frame's begin_frame.
    pub fn end_frame(&mut self, ctx: &egui::Context) {
        if self.needs_apply {
            ctx.set_fonts(self.font_defs.clone());
            self.needs_apply = false;
        }
    }

    fn initialize(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        use font_kit::source::SystemSource;
        match SystemSource::new().all_families() {
            Ok(mut families) => {
                families.sort_by_key(|a| a.to_lowercase());
                families.dedup();
                self.system_families = families;
            }
            Err(e) => {
                log::error!("Failed to enumerate system fonts: {}", e);
            }
        }
    }

    pub fn families(&mut self) -> &[String] {
        self.initialize();
        &self.system_families
    }

    pub fn ensure_loaded(&mut self, family: &str, bold: bool, italic: bool) {
        let key = variant_key(family, bold, italic);
        if self.loaded_variants.contains(&key) {
            return;
        }

        use font_kit::properties::Style;
        use font_kit::source::SystemSource;

        let source = SystemSource::new();

        // Try the given name first, then known aliases if that fails
        let names_to_try = lookup_names(family);
        let mut family_handle = None;
        for name in &names_to_try {
            if let Ok(h) = source.select_family_by_name(name) {
                family_handle = Some(h);
                break;
            }
        }

        let family_handle = match family_handle {
            Some(h) => h,
            None => {
                log::warn!("Font family not found: {} (tried {:?})", family, names_to_try);
                self.loaded_variants.insert(key);
                return;
            }
        };

        let fonts = family_handle.fonts();

        // Find the best variant match (bold/italic)
        let mut best_data: Option<Vec<u8>> = None;
        let mut fallback_data: Option<Vec<u8>> = None;

        for handle in fonts {
            if let Ok(font) = handle.load() {
                let props = font.properties();
                let weight_ok = if bold {
                    props.weight.0 >= 600.0
                } else {
                    props.weight.0 < 600.0
                };
                let style_ok = if italic {
                    matches!(props.style, Style::Italic | Style::Oblique)
                } else {
                    props.style == Style::Normal
                };

                if let Some(data) = font.copy_font_data() {
                    if weight_ok && style_ok {
                        best_data = Some((*data).clone());
                        break;
                    }
                    if fallback_data.is_none() {
                        fallback_data = Some((*data).clone());
                    }
                }
            }
        }

        let data = best_data.or(fallback_data);
        if let Some(data_vec) = data {
            self.font_defs.font_data.insert(
                key.clone(),
                egui::FontData::from_owned(data_vec).into(),
            );

            // Build fallback chain: our font first, then default proportional fonts
            let default_chain: Vec<String> = self
                .font_defs
                .families
                .get(&egui::FontFamily::Proportional)
                .cloned()
                .unwrap_or_default();
            let mut chain = vec![key.clone()];
            chain.extend(default_chain);

            self.font_defs.families.insert(
                egui::FontFamily::Name(Arc::from(key.as_str())),
                chain,
            );
            self.loaded_variants.insert(key.clone());
            self.newly_loaded.insert(key);
            self.needs_apply = true;
            log::info!("Loaded font: {} (bold={}, italic={})", family, bold, italic);
        } else {
            log::warn!("No font data available for: {} (bold={}, italic={})", family, bold, italic);
            self.loaded_variants.insert(key);
        }
    }

    pub fn font_id(&self, family: &str, size: f32, bold: bool, italic: bool) -> egui::FontId {
        let key = variant_key(family, bold, italic);
        // Only return custom FontFamily if the font is loaded, has data, AND
        // wasn't loaded this frame (atlas hasn't rebuilt yet)
        if self.loaded_variants.contains(&key)
            && self.font_defs.font_data.contains_key(&key)
            && !self.newly_loaded.contains(&key)
        {
            egui::FontId::new(size, egui::FontFamily::Name(Arc::from(key.as_str())))
        } else {
            egui::FontId::proportional(size)
        }
    }
}

/// Map a font family name to a list of names to try via select_family_by_name.
/// Handles fontconfig aliases that font-kit doesn't resolve automatically.
fn lookup_names(family: &str) -> Vec<&str> {
    match family {
        "Sans" | "sans" => vec!["sans-serif", "Sans", "DejaVu Sans"],
        "Serif" | "serif" => vec!["serif", "Serif", "DejaVu Serif"],
        "Monospace" | "monospace" | "Mono" => vec!["monospace", "Monospace", "DejaVu Sans Mono"],
        _ => vec![family],
    }
}

fn variant_key(family: &str, bold: bool, italic: bool) -> String {
    match (bold, italic) {
        (false, false) => family.to_string(),
        (true, false) => format!("{}-bold", family),
        (false, true) => format!("{}-italic", family),
        (true, true) => format!("{}-bold-italic", family),
    }
}
