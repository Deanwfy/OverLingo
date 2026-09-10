use tauri::image::Image;

/// Menu bar icons are sized in points, so a Retina display needs twice the pixels. macOS
/// scales the image to the bar's height, so the canvas is cropped to the glyph: padding
/// baked into the image would only shrink the glyph next to everyone else's.
const ICON_PX_H: u32 = 36;
const ICON_PX_W: u32 = 45;
/// The glyph is authored on an 18-unit grid regardless of the pixel resolution; the canvas
/// shows this window of it, sized so the glyph keeps a hairline of margin.
const ICON_REGION: RoundRect = RoundRect {
    x: 0.25,
    y: 2.05,
    w: 17.5,
    h: 13.9,
    r: 0.0,
};
const SUBSAMPLES: u32 = 4;

#[derive(Clone, Copy)]
struct RoundRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
}

impl RoundRect {
    const fn new(x: f32, y: f32, w: f32, h: f32, r: f32) -> Self {
        Self { x, y, w, h, r }
    }

    fn grown(&self, by: f32) -> Self {
        Self {
            x: self.x - by,
            y: self.y - by,
            w: self.w + by * 2.0,
            h: self.h + by * 2.0,
            r: self.r + by,
        }
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        if x < self.x || x > self.x + self.w || y < self.y || y > self.y + self.h {
            return false;
        }
        let dx = (self.x + self.r - x)
            .max(x - (self.x + self.w - self.r))
            .max(0.0);
        let dy = (self.y + self.r - y)
            .max(y - (self.y + self.h - self.r))
            .max(0.0);
        dx * dx + dy * dy <= self.r * self.r
    }
}

struct Layer {
    fill: bool,
    shape: RoundRect,
    /// Limits the layer to one region, which is how a ring becomes an arc.
    clip: Option<RoundRect>,
}

impl Layer {
    const fn fill(shape: RoundRect) -> Self {
        Self {
            fill: true,
            shape,
            clip: None,
        }
    }

    const fn erase(shape: RoundRect) -> Self {
        Self {
            fill: false,
            shape,
            clip: None,
        }
    }

    const fn within(self, clip: RoundRect) -> Self {
        Self {
            clip: Some(clip),
            ..self
        }
    }

    fn covers(&self, x: f32, y: f32) -> bool {
        self.clip.is_none_or(|clip| clip.contains(x, y)) && self.shape.contains(x, y)
    }
}

/// A circle, for the arc the running badge is cut from.
const fn disc(cx: f32, cy: f32, radius: f32) -> RoundRect {
    RoundRect {
        x: cx - radius,
        y: cy - radius,
        w: radius * 2.0,
        h: radius * 2.0,
        r: radius,
    }
}

/// Two equally sized speech bubbles (x, y, w, h, radius on the 18-unit grid) with text
/// lines punched out, plus the arcs a running session adds.
fn glyph_layers(running: bool) -> Vec<Layer> {
    let upper = RoundRect::new(1.0, 2.8, 12.0, 7.5, 2.4);
    let lower = RoundRect::new(5.0, 7.7, 12.0, 7.5, 2.4);
    let mut layers = Vec::new();
    if running {
        // Quarter rings sweeping out of the bubbles along the free diagonal. Each origin
        // sits inside a bubble, so only the part clear of the glyph shows, and the pair
        // reads as sound leaving both ends of the stack.
        layers.extend(arc(11.5, 8.5, Corner::TopRight));
        layers.extend(arc(6.5, 9.5, Corner::BottomLeft));
    }
    // Each bubble clears an oversized copy of itself first, so its outline survives macOS
    // flattening the icon to one colour whatever it overlaps.
    layers.extend([
        Layer::erase(upper.grown(0.7)),
        Layer::fill(upper),
        Layer::erase(RoundRect::new(3.6, 4.5, 6.4, 1.1, 0.55)),
        Layer::erase(lower.grown(0.7)),
        Layer::fill(lower),
        Layer::erase(RoundRect::new(7.6, 9.8, 6.4, 1.1, 0.55)),
        Layer::erase(RoundRect::new(9.6, 12.2, 3.4, 1.1, 0.55)),
    ]);
    layers
}

enum Corner {
    TopRight,
    BottomLeft,
}

const ARC_RADIUS: f32 = 5.4;
const ARC_STROKE: f32 = 1.15;

/// One quarter ring. The sweep stops short of the far bubble so the arc ends in open space
/// instead of running into an outline.
fn arc(x: f32, y: f32, corner: Corner) -> [Layer; 2] {
    let sweep = match corner {
        Corner::TopRight => RoundRect::new(x, y - 9.0, 9.0, 8.2, 0.0),
        Corner::BottomLeft => RoundRect::new(x - 9.0, y + 0.8, 9.0, 8.2, 0.0),
    };
    [
        Layer::fill(disc(x, y, ARC_RADIUS)).within(sweep),
        Layer::erase(disc(x, y, ARC_RADIUS - ARC_STROKE)).within(sweep),
    ]
}

pub fn status_icon(running: bool) -> Image<'static> {
    let layers = glyph_layers(running);
    let scale_x = ICON_REGION.w / ICON_PX_W as f32;
    let scale_y = ICON_REGION.h / ICON_PX_H as f32;
    let step = 1.0 / SUBSAMPLES as f32;
    let mut rgba = vec![0u8; (ICON_PX_W * ICON_PX_H * 4) as usize];

    for y in 0..ICON_PX_H {
        for x in 0..ICON_PX_W {
            let mut hits = 0u32;
            for sy in 0..SUBSAMPLES {
                for sx in 0..SUBSAMPLES {
                    let px = ICON_REGION.x + (x as f32 + (sx as f32 + 0.5) * step) * scale_x;
                    let py = ICON_REGION.y + (y as f32 + (sy as f32 + 0.5) * step) * scale_y;
                    let covered = layers.iter().fold(false, |on, layer| {
                        if layer.covers(px, py) {
                            layer.fill
                        } else {
                            on
                        }
                    });
                    if covered {
                        hits += 1;
                    }
                }
            }
            if hits > 0 {
                let alpha = (hits * 255 / (SUBSAMPLES * SUBSAMPLES)) as u8;
                let index = ((y * ICON_PX_W + x) * 4) as usize;
                rgba[index..index + 4].copy_from_slice(&[0, 0, 0, alpha]);
            }
        }
    }

    Image::new_owned(rgba, ICON_PX_W, ICON_PX_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_template_icon_pixels() {
        for running in [false, true] {
            let icon = status_icon(running);
            assert_eq!(icon.width(), ICON_PX_W);
            assert_eq!(icon.height(), ICON_PX_H);
            let alpha: Vec<u8> = icon.rgba().iter().skip(3).step_by(4).copied().collect();
            assert!(alpha.contains(&255));
            assert!(alpha.iter().any(|&value| value > 0 && value < 255));
        }
    }

    /// The two states have to be told apart at menu-bar size, not just differ in code.
    #[test]
    fn the_running_icon_is_visibly_different() {
        let idle = status_icon(false);
        let running = status_icon(true);
        let changed = idle
            .rgba()
            .iter()
            .zip(running.rgba().iter())
            .filter(|(left, right)| left != right)
            .count();
        // Guards against the badge vanishing, not against it being small: the arc is a
        // thin stroke that still reads at menu-bar scale.
        assert!(changed > 24, "changed={changed}");
    }
}
