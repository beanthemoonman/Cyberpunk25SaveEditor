use crate::{i18n::Language, weapon_images, App, BG, CYAN, MUTED, YELLOW};
use eframe::egui::{self, Color32, RichText, TextureHandle};
use nightcity_save_editor::weapons::{self, Weapon, WeaponQuality, PLAYER_INVENTORY, STASH};
use std::collections::HashMap;

#[derive(Default)]
pub struct WeaponBrowser {
    search: String,
    kind: String,
    missing_only: bool,
    quality: WeaponQuality,
    textures: HashMap<String, TextureHandle>,
}
fn text<'a>(lang: Language, ru: &'a str, en: &'a str) -> &'a str {
    if lang == Language::Russian {
        ru
    } else {
        en
    }
}
fn kind_label(kind: &str, lang: Language) -> &str {
    let (ru, en) = match kind {
        "Wea_Handgun" => ("Пистолеты", "Pistols"),
        "Wea_Revolver" => ("Револьверы", "Revolvers"),
        "Wea_Shotgun" | "Wea_ShotgunDual" => ("Дробовики", "Shotguns"),
        "Wea_SubmachineGun" => ("Пистолеты-пулемёты", "Submachine guns"),
        "Wea_Rifle" | "Wea_AssaultRifle" => ("Штурмовые винтовки", "Assault rifles"),
        "Wea_PrecisionRifle" => ("Целевые винтовки", "Precision rifles"),
        "Wea_SniperRifle" => ("Снайперские винтовки", "Sniper rifles"),
        "Wea_Katana" => ("Катаны", "Katanas"),
        "Wea_Knife" => ("Ножи", "Knives"),
        "Wea_OneHandedClub" => ("Одноручные дубинки", "One-handed clubs"),
        "Wea_TwoHandedClub" => ("Двуручные дубинки", "Two-handed clubs"),
        "Wea_Hammer" => ("Молоты", "Hammers"),
        "Wea_LongBlade" => ("Клинки", "Blades"),
        _ => (kind, kind),
    };
    text(lang, ru, en)
}
fn shown(w: &Weapon, lang: Language) -> &str {
    text(lang, &w.name, &w.name_en)
}
impl App {
    pub(crate) fn weapons(&mut self, ui: &mut egui::Ui) {
        let lang = self.language;
        egui::Frame::none().fill(Color32::from_rgb(55, 43, 22)).inner_margin(12.).rounding(6.).show(ui, |ui| {
            ui.label(RichText::new(text(lang,
                "ТЕСТОВАЯ ФУНКЦИЯ — добавление оружия сейчас проходит испытания.",
                "EXPERIMENTAL — weapon addition is currently being tested.")).color(YELLOW).strong());
            ui.label(text(lang,
                "Проверка в игре ещё требуется. Добавление не завершает квест и не открывает его этапы. Совместимость с дальнейшим получением оружия не подтверждена.",
                "In-game verification is still required. Adding a weapon does not complete a quest or unlock its stages. Compatibility with later story rewards is unverified."));
        });
        ui.add_space(10.);
        ui.horizontal_wrapped(|ui| {
            ui.label(text(lang, "Тип", "Type"));
            let mut kinds = weapons::catalog()
                .iter()
                .map(|w| w.kind.as_str())
                .collect::<Vec<_>>();
            // Both shotgun variants share one filter, including both item types.
            for k in &mut kinds {
                if *k == "Wea_ShotgunDual" {
                    *k = "Wea_Shotgun";
                }
            }
            kinds.sort_by_key(|k| kind_label(k, lang));
            kinds.dedup();
            egui::ComboBox::from_id_salt("iconic_kind")
                .width(190.)
                .selected_text(if self.weapon_browser.kind.is_empty() {
                    text(lang, "Все типы", "All types")
                } else {
                    kind_label(&self.weapon_browser.kind, lang)
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.weapon_browser.kind,
                        String::new(),
                        text(lang, "Все типы", "All types"),
                    );
                    for kind in kinds {
                        ui.selectable_value(
                            &mut self.weapon_browser.kind,
                            kind.to_owned(),
                            kind_label(kind, lang),
                        );
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.weapon_browser.search)
                    .desired_width(230.)
                    .hint_text(text(
                        lang,
                        "Название, ID или способ получения",
                        "Name, ID or acquisition",
                    )),
            );
            ui.checkbox(
                &mut self.weapon_browser.missing_only,
                text(lang, "Только отсутствующие", "Missing only"),
            );
            if ui.button(lang.text("Сбросить фильтры")).clicked() {
                self.weapon_browser.search.clear();
                self.weapon_browser.kind.clear();
                self.weapon_browser.missing_only = false;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(text(lang, "Ранг нового оружия", "New weapon tier"));
            let label = |quality| match quality {
                WeaponQuality::PlayerLevel => text(lang, "По уровню V (без +)", "V's level (no +)"),
                WeaponQuality::Tier1 => text(lang, "1 — серый", "1 — grey"),
                WeaponQuality::Tier5PlusPlus => text(lang, "5++ — максимум", "5++ — maximum"),
            };
            egui::ComboBox::from_id_salt("iconic_quality")
                .width(210.)
                .selected_text(label(self.weapon_browser.quality))
                .show_ui(ui, |ui| {
                    for quality in WeaponQuality::ALL {
                        ui.selectable_value(&mut self.weapon_browser.quality, quality, label(quality));
                    }
                });
            ui.label(RichText::new(text(lang,
                "Ранг и ступень улучшений задаются вместе. Изменение касается только новых копий.",
                "Tier and upgrade stage are set together. Only new copies are affected.")).small().color(MUTED));
        });
        ui.label(RichText::new(text(lang,
            "Наличие: инвентарь V + тайник. Лимит: 2 для оружия с поддержкой стрельбы с двух рук, 1 для остальных. Новые экземпляры попадают в инвентарь V.",
            "Ownership: V's inventory + stash. Limit: 2 for dual-wield weapons, 1 for others. New copies go to V's inventory.")).small().color(MUTED));
        let search = self.weapon_browser.search.to_lowercase();
        let mut visible = weapons::catalog()
            .iter()
            .filter(|w| {
                let kind_matches = self.weapon_browser.kind.is_empty()
                    || self.weapon_browser.kind == w.kind
                    || self.weapon_browser.kind == "Wea_Shotgun" && w.kind == "Wea_ShotgunDual";
                let haystack = format!(
                    "{} {} {} {} {}",
                    w.name, w.name_en, w.record, w.source, w.source_en
                )
                .to_lowercase();
                kind_matches
                    && search.split_whitespace().all(|s| haystack.contains(s))
                    && (!self.weapon_browser.missing_only || w.owned(&self.items, None) == 0)
            })
            .collect::<Vec<_>>();
        visible.sort_by_key(|w| shown(w, lang));
        ui.label(format!(
            "{}: {} / {}",
            text(lang, "Показано", "Showing"),
            visible.len(),
            weapons::catalog().len()
        ));
        let mut add = None;
        egui::ScrollArea::vertical().id_salt("iconic_list").auto_shrink([false, false]).show(ui, |ui| {
            if visible.is_empty() { ui.label(lang.text("Нет предметов, соответствующих фильтрам")); }
            for w in visible {
                ui.push_id(&w.key, |ui| {
                    egui::Frame::none().fill(Color32::from_rgb(19, 27, 39)).inner_margin(12.).rounding(6.).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui(egui::vec2(136., 95.), |ui| {
                                if !self.weapon_browser.textures.contains_key(&w.image) {
                                    if let Some(bytes) = weapon_images::png(&w.image) {
                                        if let Ok(decoded) = image::load_from_memory_with_format(bytes, image::ImageFormat::Png) {
                                            let rgba = decoded.to_rgba8();
                                            let img = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                                            let texture = ui.ctx().load_texture(format!("iconic_{}", w.image), img, egui::TextureOptions::LINEAR);
                                            self.weapon_browser.textures.insert(w.image.clone(), texture);
                                        }
                                    }
                                }
                                if let Some(texture) = self.weapon_browser.textures.get(&w.image) {
                                    ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(132., 88.)).maintain_aspect_ratio(true));
                                }
                            });
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(shown(w, lang)).size(19.).color(YELLOW).strong());
                                    ui.label(RichText::new(kind_label(&w.kind, lang)).small().color(MUTED));
                                    if w.dual_wield { ui.label(RichText::new(text(lang, "С двух рук · до 2", "Dual wield · up to 2")).small().color(CYAN)); }
                                });
                                ui.label(text(lang, &w.description, &w.description_en));
                                ui.label(RichText::new(format!("{}: {}", text(lang, "Как получить", "How to obtain"), text(lang, &w.source, &w.source_en))).small().color(MUTED));
                                let in_v = w.owned(&self.items, Some(PLAYER_INVENTORY));
                                let stash = w.owned(&self.items, Some(STASH));
                                let group_owned = w.group_owned(&self.items);
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(format!("{}: {} · {}: {}", text(lang, "Инвентарь", "Inventory"), in_v, text(lang, "Тайник", "Stash"), stash))
                                        .color(if in_v + stash > 0 { CYAN } else { MUTED }));
                                    ui.label(format!("{}: {}/{}", text(lang, "Всего", "Total"), group_owned, w.limit));
                                    if ui.add_enabled(self.save.is_some() && group_owned < w.limit,
                                        egui::Button::new(text(lang, "Добавить в инвентарь", "Add to inventory")).fill(BG)).clicked() { add = Some((w.key.as_str(), 1)); }
                                    if w.limit == 2 && group_owned == 0 && ui.button(text(lang, "Добавить 2", "Add 2")).clicked() { add = Some((w.key.as_str(), 2)); }
                                    if group_owned >= w.limit { ui.label(RichText::new(text(lang, "Лимит достигнут", "Limit reached")).small().color(MUTED)); }
                                });
                                ui.label(RichText::new(text(lang, "Квест после добавления: не проверено", "Quest after addition: unverified")).small().color(YELLOW));
                                if w.group == "skippy" {
                                    ui.label(RichText::new(text(lang,
                                        "Оба варианта делят лимит. Реплики и квест зависят от сюжетных флагов; добавление версии с ИИ не гарантирует разговоры. Флаги не меняются.",
                                        "Both variants share the limit. Dialogue and quest behavior depend on story flags; adding the AI version does not guarantee speech. Flags are retained.")).small().color(YELLOW));
                                } else if w.quest_sensitive {
                                    ui.label(RichText::new(text(lang,
                                        "Есть квестовая привязка. Поведение при дальнейшем прохождении нужно проверить в игре.",
                                        "Has a quest binding. Behavior during later quest progression needs in-game testing.")).small().color(YELLOW));
                                }
                                ui.label(RichText::new(&w.record).monospace().small().color(MUTED));
                            });
                        });
                    });
                    ui.add_space(8.);
                });
            }
        });
        if let Some((key, count)) = add {
            let quality = self.weapon_browser.quality;
            self.edit(
                "Оружие добавлено (тестовая функция)",
                |s| {
                    for _ in 0..count {
                        s.add_iconic_weapon_with_quality(key, quality)?;
                    }
                    Ok(())
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nightcity_save_editor::save::Save;
    #[test]
    fn catalog_images_filters_and_languages_are_read_only() {
        for w in weapons::catalog() {
            let png = weapon_images::png(&w.image).expect("Weapon artwork missing");
            let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).unwrap();
            assert!(image.width() > 0 && image.height() > 0);
            assert!(!kind_label(&w.kind, Language::English).starts_with("Wea_"));
        }
        if !std::path::Path::new("saveExample/sav.dat").exists() {
            return;
        }
        let save = Save::load(std::path::Path::new("saveExample")).unwrap();
        let before = save.data.clone();
        let mut app = App {
            clean_data: before.clone(),
            save: Some(save),
            ..Default::default()
        };
        app.refresh();
        let ctx = egui::Context::default();
        for language in [Language::English, Language::Russian] {
            app.language = language;
            for width in [720., 1000.] {
                for search in ["", "скиппи", "Kongou", "no-such-weapon"] {
                    app.weapon_browser.search = search.into();
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 680.),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| app.weapons(ui));
                        },
                    );
                }
            }
        }
        assert_eq!(app.save.as_ref().unwrap().data, before);
        assert!(!app.dirty);
        assert!(app.undo.is_empty());
        // The add action uses App::edit, retaining the normal undo/redo contract.
        app.edit(
            "Оружие добавлено (тестовая функция)",
            |s| s.add_iconic_weapon("Preset_Liberty_Yorinobu"),
        );
        assert!(!app.error, "{}", app.status);
        assert_eq!(app.undo.len(), 1);
        assert!(app.dirty);
        assert_eq!(app.undo[0].data, before);
        Save::parse(app.save.as_ref().unwrap().encode().unwrap()).unwrap();
    }
}
