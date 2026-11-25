use eframe::egui::{Color32, ColorImage};

// Draw a small optical assembly once, in linear layers from the housing to the glass.
pub fn lens(size: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(size * size);
    for row in 0..size {
        for column in 0..size {
            let x = (column as f32 + 0.5) * 2.0 / size as f32 - 1.0;
            let y = (row as f32 + 0.5) * 2.0 / size as f32 - 1.0;
            let r = x.hypot(y);
            let angle = y.atan2(x);
            let light = (0.5 - 0.35 * (x + y) / r.max(0.001)).clamp(0.0, 1.0);
            let band = |radius: f32, spread: f32| (-((r - radius) / spread).powi(2)).exp();
            let glint = |cx: f32, cy: f32, w: f32, h: f32| {
                (-((x - cx) / w).powi(2) - ((y - cy) / h).powi(2)).exp()
            };
            let mut rgba = if r > 0.91 {
                let alpha = 65.0 * (1.0 - (r - 0.91) / 0.09).clamp(0.0, 1.0).powi(2);
                [2.0, 4.0, 12.0, alpha]
            } else if r > 0.62 {
                let grooves = (r * 650.0).cos() * 3.0;
                let metal = 22.0
                    + 38.0 * light
                    + grooves
                    + (band(0.897, 0.011) * 115.0
                        + band(0.78, 0.009) * 75.0
                        + band(0.65, 0.008) * 89.0)
                        * (0.2 + 0.8 * light)
                    - band(0.83, 0.025) * 20.0;
                [metal * 0.89, metal * 0.96, metal + 6.0, 255.0]
            } else {
                let rim = band(0.58, 0.024) + band(0.435, 0.016) * 0.65;
                let blue = glint(-0.22, -0.24, 0.32, 0.18);
                let violet = glint(0.25, 0.22, 0.25, 0.19);
                let teal = glint(-0.18, 0.22, 0.17, 0.13);
                let iris = if (0.18..0.42).contains(&r) {
                    ((angle * 7.0 + r * 14.0).cos() * 0.5 + 0.5) * 8.0
                } else {
                    0.0
                };
                let spec = glint(-0.23, -0.31, 0.12, 0.038) * 85.0;
                [
                    8.0 + 41.0 * blue + 77.0 * violet + rim * 21.0 + spec + iris,
                    13.0 + 80.0 * blue + 21.0 * violet + 48.0 * teal + rim * 44.0 + spec + iris,
                    24.0 + 130.0 * blue + 95.0 * violet + 38.0 * teal + rim * 72.0 + spec + iris,
                    255.0,
                ]
            };
            // Antialias the outer metal edge against the soft shadow.
            if (0.906..=0.91).contains(&r) {
                rgba[3] = 65.0 + 190.0 * (0.91 - r) / 0.004;
            }
            pixels.push(Color32::from_rgba_unmultiplied(
                rgba[0] as u8,
                rgba[1] as u8,
                rgba[2] as u8,
                rgba[3] as u8,
            ));
        }
    }
    ColorImage::new([size, size], pixels)
}

pub fn backdrop() -> ColorImage {
    let (width, height) = (256, 192);
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let u = x as f32 / width as f32;
            let v = y as f32 / height as f32;
            let glow = (-((u - 0.43) / 0.50).powi(2) - ((v - 0.31) / 0.72).powi(2)).exp();
            let blue = (-((u - 0.94) / 0.53).powi(2) - ((v - 0.05) / 0.56).powi(2)).exp();
            pixels.push(Color32::from_rgb(
                (16.0 + 24.0 * glow + 8.0 * blue) as u8,
                (19.0 + 24.0 * glow + 10.0 * blue) as u8,
                (27.0 + 28.0 * glow + 23.0 * blue) as u8,
            ));
        }
    }
    ColorImage::new([width, height], pixels)
}
