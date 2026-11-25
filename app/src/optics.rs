use eframe::egui::{Color32, ColorImage};

// A small still landscape for the idle viewfinder. The GPU scales this single texture.
pub fn backdrop() -> ColorImage {
    let (width, height) = (768, 384);
    let mut pixels = vec![Color32::BLACK; width * height];
    for x in 0..width {
        let u = x as f32 / width as f32;
        let ridge = |f: f32| (f.sin() + 0.32 * (2.41 * f).sin() + 0.09 * (6.53 * f).sin()) / 1.41;
        let far = 0.55 + 0.10 * ridge(8.0 * u + 0.3) - 0.08 * u;
        let mid = 0.68 + 0.12 * ridge(6.0 * u - 0.7) - 0.10 * u;
        let near = 0.83 + 0.12 * ridge(5.0 * u + 0.6) - 0.16 * u;
        for y in 0..height {
            let v = y as f32 / height as f32;
            let dx = (u - 0.76) * 2.0;
            let dy = v - 0.39;
            let radius = (dx * dx + dy * dy).sqrt();
            let haze = (1.0 - radius * 1.5).clamp(0.0, 1.0);
            let light = (0.08 + 0.51 * v + haze * haze * 0.68).min(1.0);
            let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
                std::array::from_fn::<_, 3, _>(|i| a[i] + (b[i] - a[i]) * t)
            };
            let mut c = mix([22., 44., 53.], [172., 217., 179.], light);
            c = mix(
                c,
                [228., 238., 194.],
                ((0.105 - radius) * height as f32).clamp(0., 1.),
            );
            for (line, top, base) in [
                (far, [98., 147., 136.], [64., 110., 109.]),
                (mid, [53., 100., 98.], [36., 77., 78.]),
                (near, [28., 68., 69.], [15., 41., 48.]),
            ] {
                let depth = v - line;
                if depth > 0.0 {
                    c = mix(
                        c,
                        mix(top, base, (depth * 5.).min(1.)),
                        (depth * height as f32).min(1.),
                    );
                }
            }
            let shade = 0.53 + 0.47 * (u * 1.4 + (1.0 - v) * 0.4).min(1.0);
            let grain = ((x * 73 + y * 137 + (x ^ y) * 31) % 11) as f32 * 0.23 - 1.15;
            let [r, g, b] = c.map(|n| (n * shade + grain) as u8);
            pixels[y * width + x] = Color32::from_rgb(r, g, b);
        }
    }
    ColorImage::new([width, height], pixels)
}
