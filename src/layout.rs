use std::collections::{HashMap, HashSet};

/// Find complete contiguous artwork squares, preferring a 3×3 square over 2×2.
/// `None` means the coordinate set is not a complete tiling; callers can keep
/// showing each artwork action as an ordinary single-button cover.
pub fn detect(coords: &HashSet<(u8, u8)>) -> Option<HashMap<(u8, u8), (u8, u8)>> {
	let mut remaining = coords.clone();
	let mut assignments = HashMap::new();
	while let Some(&(top, left)) = remaining.iter().min() {
		let grid = [3u8, 2].into_iter().find(|&grid| {
			(top..top + grid)
				.all(|row| (left..left + grid).all(|col| remaining.contains(&(row, col))))
		})?;
		for row in 0..grid {
			for col in 0..grid {
				let coord = (top + row, left + col);
				remaining.remove(&coord);
				assignments.insert(coord, (grid, row * grid + col));
			}
		}
	}
	Some(assignments)
}

#[cfg(test)]
mod tests {
	use super::*;
	fn rect(rows: u8, cols: u8) -> HashSet<(u8, u8)> {
		(0..rows)
			.flat_map(|r| (0..cols).map(move |c| (r, c)))
			.collect()
	}
	#[test]
	fn detects_each_tile_by_deck_position() {
		let two = detect(&rect(2, 2)).unwrap();
		assert_eq!(two.len(), 4);
		assert_eq!(two[&(0, 0)], (2, 0));
		assert_eq!(two[&(0, 1)], (2, 1));
		assert_eq!(two[&(1, 0)], (2, 2));
		assert_eq!(two[&(1, 1)], (2, 3));
		let three = detect(&rect(3, 3)).unwrap();
		assert_eq!(three.len(), 9);
		assert_eq!(three[&(2, 2)], (3, 8));
	}
	#[test]
	fn supports_adjacent_grids_and_rejects_incomplete_groups() {
		let adjacent = detect(&rect(2, 4)).unwrap();
		assert_eq!(adjacent.len(), 8);
		assert_eq!(adjacent[&(0, 2)], (2, 0));
		assert_eq!(adjacent[&(1, 3)], (2, 3));
		assert!(detect(&rect(2, 3)).is_none());
		assert!(detect(&HashSet::from([(2, 4)])).is_none());
		assert!(detect(&HashSet::new()).unwrap().is_empty());
	}
}
