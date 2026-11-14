//! The folder upload's block in the uploader's confirmation dialog, and the
//! receipt's notice.

use super::plan::{Preview, Receipt, Skipped};
use crate::theme::Palette;
use aurora_net::inventory::upload::{Kind, Prices};
use egui::RichText;
use std::sync::Arc;
use uuid::Uuid;

/// Names listed in a line before "and N more".
const LISTED: usize = 5;

/// What the confirmation needs from the run, published once per frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub destination: Uuid,
    /// The disk folder's name.
    pub folder: String,
    pub skipped: Skipped,
}

fn key() -> egui::Id {
    egui::Id::unique("folderstorm_folder_upload_context")
}

/// Shows the block while a folder upload is in the uploader, hides it after.
pub fn publish(ctx: &egui::Context, context: Option<Context>) {
    ctx.data_mut(|data| {
        if let Some(context) = context {
            data.insert_temp(key(), Arc::new(context));
        } else {
            data.remove::<Arc<Context>>(key());
        }
    });
}

/// The run's context, when a folder upload is in the uploader.
pub fn context(ctx: &egui::Context) -> Option<Arc<Context>> {
    ctx.data(|data| data.get_temp::<Arc<Context>>(key()))
}

/// The preview of the uploader's current batch.
pub fn preview(imports: &crate::imports::Imports, world: &crate::world::World, destination: Uuid, prices: &Prices) -> Preview {
    let existing: Vec<String> = world
        .inventory
        .folders
        .get(&destination)
        .map(|folder| {
            folder
                .children
                .iter()
                .filter_map(|id| world.inventory.folders.get(id))
                .map(|child| child.info.name.clone())
                .collect()
        })
        .unwrap_or_default();
    Preview::build(&super::rows(imports), &existing, imports.total(prices), world.balance)
}

/// `names` cut to [`LISTED`], with "and N more".
pub fn list(names: &[String]) -> String {
    let mut text = names.iter().take(LISTED).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > LISTED {
        text.push(' ');
        text.push_str(&aurora_i18n::tr!("fs-folder-upload-more", count = names.len() - LISTED));
    }
    text
}

fn kind_label(kind: Kind, count: usize) -> String {
    match kind {
        Kind::Texture | Kind::Snapshot => aurora_i18n::tr!("fs-folder-upload-textures", count = count),
        Kind::Sound => aurora_i18n::tr!("fs-folder-upload-sounds", count = count),
        Kind::Animation => aurora_i18n::tr!("fs-folder-upload-animations", count = count),
        Kind::Material => aurora_i18n::tr!("fs-folder-upload-materials", count = count),
        Kind::Mesh => aurora_i18n::tr!("fs-folder-upload-meshes", count = count),
    }
}

/// The block's lines and whether each one is a warning, for the dialog and
/// the tests.
pub fn lines(context: &Context, preview: &Preview, destination_path: &str) -> Vec<(String, Tone)> {
    let mut lines = vec![
        (
            aurora_i18n::tr!(
                "fs-folder-upload-from",
                folder = context.folder.as_str(),
                destination = destination_path
            ),
            Tone::Strong,
        ),
        (
            aurora_i18n::tr!("fs-folder-upload-files", files = preview.files, folders = preview.new_folders),
            Tone::Normal,
        ),
    ];
    if !preview.by_kind.is_empty() {
        let kinds: Vec<String> = preview.by_kind.iter().map(|(kind, count)| kind_label(*kind, *count)).collect();
        lines.push((kinds.join(" · "), Tone::Normal));
    }
    let skipped = &context.skipped;
    if !skipped.files.is_empty() {
        lines.push((
            aurora_i18n::tr!(
                "fs-folder-upload-skipped",
                count = skipped.files.len(),
                names = list(&skipped.files)
            ),
            Tone::Warn,
        ));
    }
    if skipped.empty_folders > 0 {
        lines.push((
            aurora_i18n::tr!("fs-folder-upload-empty", count = skipped.empty_folders),
            Tone::Normal,
        ));
    }
    if skipped.links > 0 {
        lines.push((aurora_i18n::tr!("fs-folder-upload-links", count = skipped.links), Tone::Normal));
    }
    if skipped.truncated {
        lines.push((aurora_i18n::tr!("fs-folder-upload-truncated").into(), Tone::Warn));
    }
    if !preview.existing_names.is_empty() {
        lines.push((
            aurora_i18n::tr!(
                "fs-folder-upload-existing",
                destination = destination_path,
                names = list(&preview.existing_names)
            ),
            Tone::Warn,
        ));
    }
    if !preview.shared_names.is_empty() {
        lines.push((
            aurora_i18n::tr!("fs-folder-upload-shared", names = list(&preview.shared_names)),
            Tone::Normal,
        ));
    }
    if preview.errors > 0 {
        lines.push((aurora_i18n::tr!("fs-folder-upload-errors", count = preview.errors), Tone::Danger));
    }
    match (preview.cost, preview.balance, preview.balance_after()) {
        (Some(cost), Some(balance), _) if !preview.affordable() => lines.push((
            aurora_i18n::tr!("fs-folder-upload-short", cost = cost, balance = balance),
            Tone::Danger,
        )),
        (Some(cost), Some(balance), Some(after)) if cost > 0 => lines.push((
            aurora_i18n::tr!("fs-folder-upload-balance", balance = balance, after = after),
            Tone::Normal,
        )),
        _ => {}
    }
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Strong,
    Normal,
    Warn,
    Danger,
}

/// Folderstorm's block in the uploader's confirmation (`imports/ui.rs`),
/// while a folder upload is in the uploader. The total L$ line above it is
/// the uploader's.
pub fn confirm_details(ui: &mut egui::Ui, p: &Palette, imports: &crate::imports::Imports, world: &crate::world::World, prices: &Prices) {
    let Some(context) = context(ui.ctx()) else {
        return;
    };
    let preview = preview(imports, world, context.destination, prices);
    let destination = crate::ui::inventory::inventory_path(&world.inventory, context.destination);
    crate::ui::widgets::divider(ui, p);
    for (text, tone) in lines(&context, &preview, &destination) {
        let text = RichText::new(text);
        let text = match tone {
            Tone::Strong => text.strong(),
            Tone::Normal => text,
            Tone::Warn => text.color(p.warn),
            Tone::Danger => text.color(p.danger),
        };
        ui.add(egui::Label::new(text).wrap());
    }
    crate::ui::widgets::divider(ui, p);
}

/// The receipt's notice.
pub fn receipt_text(folder: &str, receipt: &Receipt) -> String {
    let mut text: String = aurora_i18n::tr!(
        "fs-folder-upload-receipt",
        folder = folder,
        uploaded = receipt.uploaded.len(),
        failed = receipt.failed.len(),
        cancelled = receipt.cancelled.len(),
        spent = receipt.spent
    );
    if !receipt.failed.is_empty() {
        let names: Vec<String> = receipt.failed.iter().map(|(name, _)| name.clone()).collect();
        text.push(' ');
        text.push_str(&aurora_i18n::tr!("fs-folder-upload-receipt-failed", names = list(&names)));
    }
    text
}

/// The disk folder's name for the texts.
pub fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
