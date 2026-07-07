//! 色空間変換（OpenCV の u8 表現に合わせる: H は 0..180、Lab は L*255/100, a/b は +128）

/// RGB u8 → HSV u8（OpenCV 互換: H 0..180）
pub fn rgb_to_hsv(rgb: [u8; 3]) -> [u8; 3] {
    let (r, g, b) = (rgb[0] as f32, rgb[1] as f32, rgb[2] as f32);
    let v = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let diff = v - mn;
    let s = if v > 0.0 { diff * 255.0 / v } else { 0.0 };
    let h = if diff > 0.0 {
        let h = if (v - r).abs() < f32::EPSILON {
            60.0 * (g - b) / diff
        } else if (v - g).abs() < f32::EPSILON {
            120.0 + 60.0 * (b - r) / diff
        } else {
            240.0 + 60.0 * (r - g) / diff
        };
        let h = if h < 0.0 { h + 360.0 } else { h };
        h / 2.0
    } else {
        0.0
    };
    [h.round().min(180.0) as u8, s.round().min(255.0) as u8, v.round() as u8]
}

/// HSV u8（OpenCV 互換）→ RGB u8
pub fn hsv_to_rgb(hsv: [u8; 3]) -> [u8; 3] {
    let h = hsv[0] as f32 * 2.0; // 0..360
    let s = hsv[1] as f32 / 255.0;
    let v = hsv[2] as f32 / 255.0;
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn lab_f(t: f32) -> f32 {
    const D: f32 = 6.0 / 29.0;
    if t > D * D * D {
        t.cbrt()
    } else {
        t / (3.0 * D * D) + 4.0 / 29.0
    }
}

fn lab_f_inv(t: f32) -> f32 {
    const D: f32 = 6.0 / 29.0;
    if t > D {
        t * t * t
    } else {
        3.0 * D * D * (t - 4.0 / 29.0)
    }
}

/// RGB u8 → Lab f32（OpenCV u8 スケール: L 0..255, a/b は 128 中心）
pub fn rgb_to_lab_u8scale(rgb: [u8; 3]) -> [f32; 3] {
    let r = srgb_to_linear(rgb[0] as f32 / 255.0);
    let g = srgb_to_linear(rgb[1] as f32 / 255.0);
    let b = srgb_to_linear(rgb[2] as f32 / 255.0);
    // sRGB D65
    let x = 0.412453 * r + 0.357580 * g + 0.180423 * b;
    let y = 0.212671 * r + 0.715160 * g + 0.072169 * b;
    let z = 0.019334 * r + 0.119193 * g + 0.950227 * b;
    let (xn, yn, zn) = (0.950456, 1.0, 1.088754);
    let fx = lab_f(x / xn);
    let fy = lab_f(y / yn);
    let fz = lab_f(z / zn);
    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let bb = 200.0 * (fy - fz);
    [l * 255.0 / 100.0, a + 128.0, bb + 128.0]
}

/// Lab（OpenCV u8 スケール）→ RGB u8
pub fn lab_u8scale_to_rgb(lab: [f32; 3]) -> [u8; 3] {
    let l = lab[0] * 100.0 / 255.0;
    let a = lab[1] - 128.0;
    let b = lab[2] - 128.0;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let (xn, yn, zn) = (0.950456, 1.0, 1.088754);
    let x = xn * lab_f_inv(fx);
    let y = yn * lab_f_inv(fy);
    let z = zn * lab_f_inv(fz);
    let r = 3.240479 * x - 1.537150 * y - 0.498535 * z;
    let g = -0.969256 * x + 1.875992 * y + 0.041556 * z;
    let bb = 0.055648 * x - 0.204043 * y + 1.057311 * z;
    [
        (linear_to_srgb(r.clamp(0.0, 1.0)) * 255.0).round() as u8,
        (linear_to_srgb(g.clamp(0.0, 1.0)) * 255.0).round() as u8,
        (linear_to_srgb(bb.clamp(0.0, 1.0)) * 255.0).round() as u8,
    ]
}
