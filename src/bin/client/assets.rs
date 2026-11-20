//! Application art and fonts are read from the executable, never a disk directory.
use bevy::{asset::io::embedded::EmbeddedAssetRegistry, prelude::*};

const ASSETS: &[(&str, &[u8])] = &[
    ("board.png", include_bytes!("../../../assets/board.png")),
    ("cover.png", include_bytes!("../../../assets/cover.png")),
    ("marshal.png", include_bytes!("../../../assets/marshal.png")),
    ("general.png", include_bytes!("../../../assets/general.png")),
    ("captain.png", include_bytes!("../../../assets/captain.png")),
    ("cadet.png", include_bytes!("../../../assets/cadet.png")),
    (
        "corporal.png",
        include_bytes!("../../../assets/corporal.png"),
    ),
    (
        "rifleman.png",
        include_bytes!("../../../assets/rifleman.png"),
    ),
    ("scout.png", include_bytes!("../../../assets/scout.png")),
    ("miner.png", include_bytes!("../../../assets/miner.png")),
    ("spy.png", include_bytes!("../../../assets/spy.png")),
    ("mine.png", include_bytes!("../../../assets/mine.png")),
    ("flag.png", include_bytes!("../../../assets/flag.png")),
    ("font.ttf", include_bytes!("../../../assets/font.ttf")),
    (
        "font-bold.ttf",
        include_bytes!("../../../assets/font-bold.ttf"),
    ),
];
pub const FONT_LICENSE: &str = include_str!("../../../assets/FONT-LICENSE.txt");

pub fn path(name: &str) -> String {
    format!("embedded://mas/{name}")
}
pub struct EmbeddedArtPlugin;
impl Plugin for EmbeddedArtPlugin {
    fn build(&self, app: &mut App) {
        let registry = app.world().resource::<EmbeddedAssetRegistry>();
        for &(name, bytes) in ASSETS {
            registry.insert_asset(
                std::path::PathBuf::new(),
                std::path::Path::new(&format!("mas/{name}")),
                bytes,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::{AssetMetaCheck, AssetPlugin},
        image::{CompressedImageFormats, ImageLoader, ImagePlugin},
        text::FontLoader,
    };
    #[test]
    fn every_graphic_and_font_loads_without_an_assets_directory() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: "missing-assets-for-embedded-test".into(),
                meta_check: AssetMetaCheck::Never,
                ..default()
            },
            ImagePlugin::default(),
            EmbeddedArtPlugin,
        ))
        .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE))
        .init_asset::<Font>()
        .init_asset_loader::<FontLoader>();
        let server = app.world().resource::<AssetServer>();
        let images: Vec<Handle<Image>> = ASSETS
            .iter()
            .filter(|(name, _)| name.ends_with(".png"))
            .map(|(name, _)| server.load(path(name)))
            .collect();
        let fonts: Vec<Handle<Font>> = ["font.ttf", "font-bold.ttf"]
            .into_iter()
            .map(|name| server.load(path(name)))
            .collect();
        app.finish();
        app.cleanup();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.update();
            let server = app.world().resource::<AssetServer>();
            if images
                .iter()
                .all(|handle| server.is_loaded_with_dependencies(handle.id()))
                && fonts
                    .iter()
                    .all(|handle| server.is_loaded_with_dependencies(handle.id()))
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Embedded art did not load without filesystem assets"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(images.len(), 13);
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&images[0])
            .unwrap();
        assert_eq!((image.width(), image.height()), (1059, 1485));
        assert!(FONT_LICENSE.contains("Bitstream"));
    }
}
