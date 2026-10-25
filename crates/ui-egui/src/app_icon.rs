//! The program's own icon, drawn in code: a closed envelope on a rounded blue square. The window
//! shows it and `cargo xtask icon` writes it into `apps/openmailclient/assets/openmailclient.ico`,
//! which the installer and the program file carry. It uses nothing but this file, so the xtask
//! can include it as it is.

/// The largest icon drawn; Windows asks for 256 at most.
pub const MAX_SIZE: u32 = 1024;

/// The icon at `size` by `size` pixels, as RGBA rows from the top, edges smoothed.
pub fn rgba(size: u32) -> Vec<u8> {
    let size = size.clamp(1, MAX_SIZE);
    let n = size as usize;
    let mut out = vec![0u8; n * n * 4];
    let px = 1.0 / size as f32;
    // How much of a pixel a shape `d` away from its edge covers.
    let cover = |d: f32| (0.5 - d / px).clamp(0.0, 1.0);
    let top = [0x3a as f32, 0x86 as f32, 0xd8 as f32];
    let bottom = [0x1c as f32, 0x55 as f32, 0xa3 as f32];
    // Small icons get a thicker fold so it still shows at 16 pixels.
    let stroke = (0.035f32).max(1.3 * px);
    for (i, pixel) in out.chunks_exact_mut(4).enumerate() {
        let x = ((i % n) as f32 + 0.5) * px;
        let y = ((i / n) as f32 + 0.5) * px;
        let back = cover(rounded_box(x, y, 0.5, 0.5, 0.47, 0.47, 0.2));
        if back <= 0.0 {
            continue;
        }
        let blue: Vec<f32> = top.iter().zip(bottom).map(|(t, b)| t + (b - t) * y).collect();
        let mut colour = [blue[0], blue[1], blue[2]];
        let paper = cover(rounded_box(x, y, 0.5, 0.53, 0.3, 0.21, 0.04));
        let fold = cover(segment(x, y, (0.215, 0.35), (0.5, 0.575)).min(segment(x, y, (0.785, 0.35), (0.5, 0.575))) - stroke / 2.0);
        let white = paper * (1.0 - fold);
        for (c, b) in colour.iter_mut().zip(&blue) {
            *c = *c + (255.0 - *c) * white;
            // The fold is the blue a little deeper, as a shadow on the paper.
            *c -= (*c - b * 0.85) * fold * paper;
        }
        pixel[0] = colour[0].round().clamp(0.0, 255.0) as u8;
        pixel[1] = colour[1].round().clamp(0.0, 255.0) as u8;
        pixel[2] = colour[2].round().clamp(0.0, 255.0) as u8;
        pixel[3] = (back * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// How far (x, y) is outside a box centred on (cx, cy) with half sizes (hx, hy) and corners of
/// radius r; less than nothing inside it.
fn rounded_box(x: f32, y: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (x - cx).abs() - hx + r;
    let qy = (y - cy).abs() - hy + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// How far (x, y) is from the line from `a` to `b`.
fn segment(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py) = (x - a.0, y - a.1);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let along = ((px * dx + py * dy) / (dx * dx + dy * dy).max(f32::EPSILON)).clamp(0.0, 1.0);
    ((px - dx * along).powi(2) + (py - dy * along).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_is_drawn_at_every_size_asked() {
        for size in [0, 1, 16, 32, 256, 5000] {
            let got = super::rgba(size);
            let side = size.clamp(1, super::MAX_SIZE) as usize;
            assert_eq!(got.len(), side * side * 4);
        }
        let icon = super::rgba(64);
        // The corners are clear and the middle is the white paper.
        assert_eq!(icon[3], 0);
        let middle = (40 * 64 + 32) * 4;
        assert!(icon[middle] > 200 && icon[middle + 3] == 255, "{:?}", &icon[middle..middle + 4]);
    }
}
