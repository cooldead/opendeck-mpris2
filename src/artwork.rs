use anyhow::{Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use url::Url;

const LIMIT: usize = 8 * 1024 * 1024;

fn data_url(bytes: &[u8]) -> Result<String> {
	let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
		"image/png"
	} else if bytes.starts_with(b"\xff\xd8\xff") {
		"image/jpeg"
	} else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
		"image/gif"
	} else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
		"image/webp"
	} else {
		bail!("Unsupported artwork format");
	};
	ensure!(bytes.len() <= LIMIT, "Artwork exceeds 8 MiB");
	Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

async fn fetch(client: &reqwest::Client, source: &str) -> Result<String> {
	let url = Url::parse(source)?;
	let bytes = match url.scheme() {
		"file" => {
			let path = url
				.to_file_path()
				.map_err(|_| anyhow::anyhow!("Invalid local artwork URL"))?;
			let file = tokio::fs::File::open(path).await?;
			ensure!(
				file.metadata().await?.is_file(),
				"Artwork is not a regular file"
			);
			let mut bytes = Vec::new();
			file.take((LIMIT + 1) as u64)
				.read_to_end(&mut bytes)
				.await?;
			bytes
		}
		"http" | "https" => {
			let mut response = client.get(url).send().await?.error_for_status()?;
			let mut bytes = Vec::new();
			while let Some(chunk) = response.chunk().await? {
				ensure!(bytes.len() + chunk.len() <= LIMIT, "Artwork exceeds 8 MiB");
				bytes.extend_from_slice(&chunk);
			}
			bytes
		}
		"data" => {
			let (header, encoded) = source
				.split_once(',')
				.ok_or_else(|| anyhow::anyhow!("Invalid image data URL"))?;
			ensure!(
				header.ends_with(";base64") && encoded.len() <= LIMIT * 2,
				"Invalid image data URL"
			);
			STANDARD.decode(encoded)?
		}
		_ => bail!("Unsupported artwork URL scheme"),
	};
	data_url(&bytes)
}

pub struct Cache {
	client: reqwest::Client,
	key: String,
	image: Option<String>,
	checked: Instant,
}
impl Cache {
	pub fn new() -> Self {
		Self {
			client: reqwest::Client::builder()
				.timeout(Duration::from_secs(3))
				.build()
				.expect("HTTP client"),
			key: String::new(),
			image: None,
			checked: Instant::now(),
		}
	}
	pub async fn get(&mut self, player: &str, track: &str, source: &str) -> Option<String> {
		if source.is_empty() {
			self.key.clear();
			self.image = None;
			return None;
		}
		let mut key = format!("{player}\n{track}\n{source}");
		if let Ok(url) = Url::parse(source)
			&& let Ok(path) = url.to_file_path()
			&& let Ok(meta) = tokio::fs::metadata(path).await
		{
			key.push_str(&format!("{:?}:{}", meta.modified().ok(), meta.len()));
		}
		if key != self.key
			|| (self.image.is_none() && self.checked.elapsed() > Duration::from_secs(5))
		{
			self.key = key;
			self.checked = Instant::now();
			self.image =
				match tokio::time::timeout(Duration::from_secs(4), fetch(&self.client, source))
					.await
				{
					Ok(Ok(image)) => Some(image),
					result => {
						log::debug!("Artwork unavailable: {result:?}");
						None
					}
				};
		}
		self.image.clone()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[tokio::test]
	async fn local_encoded_path_refresh_and_missing_art() {
		let path = std::env::temp_dir().join(format!("mpris artwork {}.png", std::process::id()));
		let png = include_bytes!("../assets/icons/playpause.png");
		tokio::fs::write(&path, png).await.unwrap();
		let source = Url::from_file_path(&path).unwrap().to_string();
		assert!(source.contains("%20"));
		let mut cache = Cache::new();
		let image = cache.get("strawberry", "track1", &source).await.unwrap();
		assert_eq!(image, data_url(png).unwrap());
		assert_eq!(
			cache.get("strawberry", "track1", &image).await.unwrap(),
			image
		);
		tokio::fs::write(&path, b"not an image").await.unwrap();
		assert!(cache.get("strawberry", "track1", &source).await.is_none());
		assert!(cache.get("strawberry", "track2", "").await.is_none());
		tokio::fs::remove_file(&path).await.unwrap();
		assert!(cache.get("strawberry", "track3", &source).await.is_none());
	}
}

/// Crop once to a square, then slice a common canvas so adjacent edges line up.
fn split_tiles(source: &str, grid: u8) -> Result<Vec<String>> {
	use image::{ImageFormat, ImageReader, imageops::FilterType};
	use std::io::Cursor;
	ensure!(matches!(grid, 2 | 3), "Unsupported artwork grid");
	let (_, encoded) = source
		.split_once(',')
		.ok_or_else(|| anyhow::anyhow!("Invalid artwork"))?;
	let bytes = STANDARD.decode(encoded)?;
	let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
	let mut limits = image::Limits::default();
	limits.max_image_width = Some(8192);
	limits.max_image_height = Some(8192);
	limits.max_alloc = Some(128 * 1024 * 1024);
	reader.limits(limits);
	let image = reader.decode()?;
	let side = image.width().min(image.height());
	ensure!(side > 0, "Empty artwork");
	let square = image.crop_imm(
		(image.width() - side) / 2,
		(image.height() - side) / 2,
		side,
		side,
	);
	const TILE: u32 = 144;
	let canvas = square.resize_exact(
		TILE * u32::from(grid),
		TILE * u32::from(grid),
		FilterType::Lanczos3,
	);
	let mut tiles = Vec::new();
	for row in 0..u32::from(grid) {
		for column in 0..u32::from(grid) {
			let tile = canvas.crop_imm(column * TILE, row * TILE, TILE, TILE);
			let mut png = Cursor::new(Vec::new());
			tile.write_to(&mut png, ImageFormat::Png)?;
			tiles.push(data_url(png.get_ref())?);
		}
	}
	Ok(tiles)
}

#[derive(Default)]
pub struct TileCache {
	source: Option<String>,
	layouts: std::collections::HashMap<u8, Option<Vec<String>>>,
}
impl TileCache {
	pub fn update(&mut self, source: Option<String>) {
		if self.source != source {
			self.source = source;
			self.layouts.clear();
		}
	}
	pub async fn tile(&mut self, grid: u8, position: u8) -> Option<String> {
		let source = self.source.as_ref()?;
		if !matches!(grid, 2 | 3) {
			return Some(source.clone());
		}
		if let std::collections::hash_map::Entry::Vacant(entry) = self.layouts.entry(grid) {
			let source = source.clone();
			let tiles = match tokio::task::spawn_blocking(move || split_tiles(&source, grid)).await
			{
				Ok(Ok(tiles)) => Some(tiles),
				result => {
					log::warn!("Unable to split album artwork: {result:?}");
					None
				}
			};
			entry.insert(tiles);
		}
		self.layouts
			.get(&grid)?
			.as_ref()?
			.get(usize::from(position.min(grid * grid - 1)))
			.cloned()
	}
}

#[cfg(test)]
mod tile_tests {
	use super::*;
	use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
	use std::io::Cursor;
	fn encoded(image: RgbImage) -> String {
		let mut bytes = Cursor::new(Vec::new());
		DynamicImage::ImageRgb8(image)
			.write_to(&mut bytes, ImageFormat::Png)
			.unwrap();
		data_url(bytes.get_ref()).unwrap()
	}
	fn decode(source: &str) -> RgbImage {
		image::load_from_memory(&STANDARD.decode(source.split_once(',').unwrap().1).unwrap())
			.unwrap()
			.to_rgb8()
	}
	#[test]
	fn grids_reassemble_without_missing_or_reordered_pixels() {
		for grid in [2, 3] {
			let side = 144 * u32::from(grid);
			let original = RgbImage::from_fn(side, side, |x, y| {
				Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
			});
			let tiles = split_tiles(&encoded(original.clone()), grid).unwrap();
			assert_eq!(tiles.len(), usize::from(grid * grid));
			for (index, tile) in tiles.iter().enumerate() {
				let decoded = decode(tile);
				assert_eq!(decoded.dimensions(), (144, 144));
				for (x, y, pixel) in decoded.enumerate_pixels() {
					let column = index as u32 % u32::from(grid);
					let row = index as u32 / u32::from(grid);
					assert_eq!(pixel, original.get_pixel(column * 144 + x, row * 144 + y));
				}
			}
		}
	}
	#[tokio::test]
	async fn crop_cache_layout_switch_track_change_and_missing_cover() {
		let source = encoded(RgbImage::from_fn(600, 300, |x, _| {
			if (150..450).contains(&x) {
				Rgb([255, 0, 0])
			} else {
				Rgb([0, 0, 255])
			}
		}));
		let mut cache = TileCache::default();
		cache.update(Some(source.clone()));
		assert_eq!(cache.tile(1, 0).await, Some(source));
		for grid in [2, 3] {
			for position in 0..grid * grid {
				assert!(
					decode(&cache.tile(grid, position).await.unwrap())
						.pixels()
						.all(|p| *p == Rgb([255, 0, 0]))
				);
			}
		}
		cache.update(Some(encoded(RgbImage::from_pixel(
			144,
			144,
			Rgb([0, 255, 0]),
		))));
		assert_eq!(
			*decode(&cache.tile(3, 8).await.unwrap()).get_pixel(0, 0),
			Rgb([0, 255, 0])
		);
		cache.update(None);
		assert!(cache.tile(2, 0).await.is_none());
		cache.update(Some("data:image/png;base64,YmFk".into()));
		assert!(cache.tile(3, 0).await.is_none());
	}
}
