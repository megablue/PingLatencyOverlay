//! History compression: the x mapping that packs older samples into bands.
//!
//! An overlay's long axis is `window_seconds × scale` logical pixels, so before
//! this module it could show exactly `window_seconds` of history and no more.
//! Compression cuts that canvas into bands and draws each one at its own
//! density, so the same width reaches much further back: the newest band keeps
//! the density the overlay has with the feature off, and every older band is a
//! fixed multiple denser than the one before it.
//!
//! The canvas itself never grows — the point is that the newest samples are
//! drawn at full resolution inside the width the config already names. The
//! bands, newest to oldest:
//!
//! - the **no-compression band**, drawn at the raw density (`scale` pixels per
//!   second, one pixel per sample at 1×), as wide as `noZoneShare` or
//!   `noZoneMinPx` says;
//! - the **ladder**, one band per compression step, their ratios spread evenly
//!   from 2× up to the configured maximum, all the same width;
//! - the **reserve**, held at the configured maximum ratio.
//!
//! Density is constant inside a band and steps down at every join. The
//! background grid draws exactly that: a cell is 30 px in the no-compression
//! band and `30 / ratio` in a compressed one, so the bands read without a
//! legend.
//!
//! Every number here is in the caller's units. The renderer passes physical
//! pixels and gets physical pixels back; the editor passes the config's own
//! logical axis for its readout. The only unit-free quantities are the band
//! shares, and the px floors are converted to shares against the *logical* axis
//! (`window_seconds × scale`), which is what keeps a floor meaning the same
//! thing at every DPI without anyone having to hand this module a scale factor.
//!
//! Pure by construction: no Win32, no I/O, no GUI crate. It is a sibling of the
//! renderer's other reserves — one statement of the rule, read by the drawing
//! and by the editor's readout.

use crate::config::{OverlayConfig, MAX_HISTORY_COMPRESSION_RATIO, MIN_HISTORY_COMPRESSION_RATIO};

/// The narrowest a ladder band may be drawn, in logical pixels.
///
/// A band thinner than this reads as a stripe rather than as a zone, so the
/// ladder gets as many bands as the canvas can hold at this width and no more.
/// The no-compression band and the reserve have their own config floors.
pub const MIN_COMPRESSION_ZONE_PX: f32 = 10.0;

/// The most bands one mapping can hold: the reserve plus the longest ladder the
/// ratio range can express — whole ratio steps from 2× to the maximum.
pub const MAX_COMPRESSION_ZONES: usize = MAX_HISTORY_COMPRESSION_RATIO as usize;

/// One compressed band of the mapping.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Zone {
    /// How much denser this band is than the raw density.
    ratio: f32,
    /// The band's width, in the caller's pixels.
    width: f32,
    /// Distance from the newest drawn instant where the band begins.
    from: f32,
    /// The x the band begins at, measured the same way `axis` is.
    x: f32,
}

impl Zone {
    /// How much denser this band is drawn than the raw density.
    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    /// The band's width, in the caller's pixels.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// The distance from the newest drawn instant where the band begins.
    pub fn from(&self) -> f32 {
        self.from
    }

    /// The x the band begins at, measured the same way `axis` is.
    pub fn x(&self) -> f32 {
        self.x
    }
}

/// One overlay's resolved history compression.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compression {
    /// The whole long axis the mapping lives in, in the caller's pixels.
    axis: f32,
    /// Width of the no-compression band, in pixels.
    no_zone: f32,
    /// Width of the reserve, in pixels.
    max_zone: f32,
    /// Distance the no-compression band holds.
    no_zone_seconds: f32,
    /// Density of the no-compression band: pixels per unit of distance. This is
    /// the density the same overlay draws at with the feature off.
    base: f32,
    /// The compressed bands, newest to deepest, the reserve last.
    zones: [Zone; MAX_COMPRESSION_ZONES],
    /// How many of `zones` are in use.
    n_zones: usize,
    /// The ratio the reserve holds.
    ratio: f32,
    /// How much history the whole canvas covers.
    span: f32,
}

impl Compression {
    /// The long axis this mapping was resolved against.
    pub fn axis(&self) -> f32 {
        self.axis
    }

    /// Width of the no-compression band, in pixels.
    pub fn no_zone(&self) -> f32 {
        self.no_zone
    }

    /// Width of the reserve, in pixels.
    pub fn max_zone(&self) -> f32 {
        self.max_zone
    }

    /// Distance the no-compression band holds: `no_zone / base`.
    ///
    /// It is `noZoneMinPx / scale` seconds while the floor decides the band and
    /// more when the share does, and it is not the configured window: with a
    /// canvas that never grows, the raw band can only hold what fits at the raw
    /// density.
    pub fn no_zone_seconds(&self) -> f32 {
        self.no_zone_seconds
    }

    /// Density of the no-compression band: pixels per second, or per slot.
    pub fn base(&self) -> f32 {
        self.base
    }

    /// The ratio the reserve holds.
    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    /// The compressed bands, newest to deepest, the reserve last.
    pub fn zones(&self) -> &[Zone] {
        &self.zones[..self.n_zones]
    }

    /// How many bands there are, the reserve included.
    pub fn zone_count(&self) -> usize {
        self.n_zones
    }

    /// How much history the whole canvas covers, in the distance's own units.
    pub fn span(&self) -> f32 {
        self.span
    }

    /// The x a sample at `distance` from the newest drawn instant is drawn at.
    ///
    /// Distance is seconds in smooth mode and slots in index mode; either way
    /// zero is the newest instant the frame presents. A negative distance is a
    /// sample newer than that — the line pass keeps one older sample and the
    /// reveal tip interpolates inside the newest segment, and both want the
    /// linear extrapolation the first branch gives them. A distance past the
    /// span extrapolates the reserve's own density, which is the same thing at
    /// the other end.
    ///
    /// The mapping is strictly decreasing and piecewise linear: density is
    /// constant inside a band and steps down by the band's ratio at each join.
    pub fn x_of(&self, distance: f32) -> f32 {
        let distance = if distance.is_finite() { distance } else { 0.0 };
        if distance <= self.no_zone_seconds {
            return self.axis - distance * self.base;
        }
        // The last band that begins behind this distance holds it. `saturating`
        // is belt and braces: the first band begins exactly at the end of the
        // no-compression band, so the search always finds one.
        let index = self.zones().partition_point(|zone| zone.from <= distance);
        let zone = &self.zones[index.saturating_sub(1)];
        zone.x - (distance - zone.from) * self.base / zone.ratio
    }
}

/// The band widths one config resolves to, as shares of the drawn axis, plus
/// the ladder's ratios.
struct Bands {
    /// Share of the drawn axis the no-compression band takes.
    no: f32,
    /// Share of the drawn axis the reserve takes, plus any middle the ladder has
    /// no room for.
    max: f32,
    /// The ladder's ratios, newest to deepest. Only the first `n_middle` are
    /// used.
    ratios: [f32; MAX_COMPRESSION_ZONES],
    /// How many ladder bands there are.
    n_middle: usize,
    /// The deepest ratio, held by the reserve.
    ratio: f32,
}

/// Resolve the bands one config asks for, or `None` when compression does not
/// apply at all.
///
/// One statement of the rule, read by `geometry` and by the editor's readout
/// through it: the shares decide how the canvas is cut up, and the ladder's
/// shape follows from the shares and the ratio.
fn bands(config: &OverlayConfig) -> Option<Bands> {
    if !config.history_compression {
        return None;
    }
    // The axis as configured, before DPI: the floors are logical pixels and the
    // threshold means the same thing on every display.
    let logical_axis = (config.window_seconds.max(1) as f64 * config.scale.max(1) as f64) as f32;
    if logical_axis < config.history_compression_min_axis_px as f32 {
        return None;
    }
    let ratio = (config.history_compression_ratio as f32).clamp(
        MIN_HISTORY_COMPRESSION_RATIO as f32,
        MAX_HISTORY_COMPRESSION_RATIO as f32,
    );
    let mut no = config.history_compression_no_zone_share as f32 / 100.0;
    let mut max = config.history_compression_max_zone_share as f32 / 100.0;
    // A floor is a share of the logical axis, so it needs no DPI scale here:
    // the physical axis is the logical one times that scale, which is exactly
    // what makes the floor come out as its logical value times the scale.
    no = no.max(config.history_compression_no_zone_min_px as f32 / logical_axis);
    max = max.max(config.history_compression_max_zone_min_px as f32 / logical_axis);
    // Two bands that between them ask for more than the canvas get scaled down
    // together, keeping their proportions: a canvas too narrow for its own
    // floors still draws something sane.
    let total = no + max;
    if total > 1.0 {
        no /= total;
        max /= total;
    }
    let middle = 1.0 - no - max;
    let middle_px = middle * logical_axis;
    // How many ladder bands fit, and how fine the ratios can step: a step below
    // 1× is a band nobody can see, so the ladder never holds more than one band
    // per whole ratio from 2 up to the maximum.
    let fit = (middle_px / MIN_COMPRESSION_ZONE_PX).floor() as usize;
    // Never more bands than whole ratio steps: a step below 1x is a band nobody
    // can see, so the ladder never holds more than one band per whole ratio.
    let n_middle = fit.min((ratio as usize).saturating_sub(1));
    if n_middle == 0 {
        // No room for a ladder: one step, raw straight to the maximum, and the
        // reserve takes the whole middle rather than leaving it blank.
        max += middle;
    }
    let mut ratios = [0.0f32; MAX_COMPRESSION_ZONES];
    for (index, slot) in ratios.iter_mut().enumerate().take(n_middle) {
        *slot = if n_middle <= 1 {
            ratio
        } else {
            2.0 + index as f32 * (ratio - 2.0) / (n_middle as f32 - 1.0)
        };
    }
    Some(Bands {
        no,
        max,
        ratios,
        n_middle,
        ratio,
    })
}

/// Resolve the compression an overlay draws with, or `None` for "exactly as
/// it would be drawn with the feature off".
///
/// `None` covers the setting being off, a long axis below the configured
/// minimum, and a degenerate geometry. Every caller treats it the same way, so
/// a graph that is too short to compress is a graph that is drawn the old way
/// rather than one drawn badly.
///
/// `axis_px` is the long axis in the caller's pixels: the renderer's
/// `axis_long`, or the editor's `window_seconds × scale` for a readout. The
/// canvas is that size and stays that size — the bands divide it, they never
/// widen it.
pub fn geometry(config: &OverlayConfig, axis_px: f32) -> Option<Compression> {
    if !axis_px.is_finite() || axis_px <= 0.0 {
        return None;
    }
    let bands = bands(config)?;
    // The raw density: the canvas covers `window_seconds` at `scale` pixels per
    // second, so a drawn axis of `logical_axis × dpi` comes out at `scale × dpi`
    // without this module being handed a scale factor. A canvas clamped by the
    // render dimension ends up below that, and every band shrinks with it.
    let logical_axis = (config.window_seconds.max(1) as f64 * config.scale.max(1) as f64) as f32;
    let base = axis_px / logical_axis * config.scale.max(1) as f32;
    if base <= 0.0 || !base.is_finite() {
        return None;
    }
    let no_zone = (bands.no * axis_px).max(1.0);
    let max_zone = bands.max * axis_px;
    let no_zone_seconds = no_zone / base;
    if no_zone_seconds <= 0.0 || !no_zone_seconds.is_finite() {
        return None;
    }
    let middle_px = (axis_px - no_zone - max_zone).max(0.0);
    let width = if bands.n_middle > 0 {
        middle_px / bands.n_middle as f32
    } else {
        0.0
    };
    let mut zones = [Zone::default(); MAX_COMPRESSION_ZONES];
    let mut from = no_zone_seconds;
    let mut x = axis_px - no_zone;
    let mut span = no_zone_seconds;
    let mut n_zones = 0;
    for index in 0..bands.n_middle {
        let held = width * bands.ratios[index] / base;
        zones[n_zones] = Zone {
            ratio: bands.ratios[index],
            width,
            from,
            x,
        };
        n_zones += 1;
        from += held;
        x -= width;
        span += held;
    }
    // The reserve, held at the maximum ratio. When the middle is empty it is
    // also the only compressed band, and when the ladder's last ratio is already
    // the maximum it continues that band at the same density.
    let held = max_zone * bands.ratio / base;
    zones[n_zones] = Zone {
        ratio: bands.ratio,
        width: max_zone,
        from,
        x,
    };
    n_zones += 1;
    span += held;
    Some(Compression {
        axis: axis_px,
        no_zone,
        max_zone,
        no_zone_seconds,
        base,
        zones,
        n_zones,
        ratio: bands.ratio,
        span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compressed overlay at the schema's defaults, on a 120 px logical axis.
    fn compressed() -> OverlayConfig {
        let mut config = OverlayConfig::new();
        config.window_seconds = 60;
        config.scale = 2;
        config.history_compression = true;
        config
    }

    /// The same overlay with both band shares left to their floors.
    fn floored(mut config: OverlayConfig) -> OverlayConfig {
        config.history_compression_no_zone_share = 0;
        config.history_compression_max_zone_share = 0;
        config
    }

    /// Pixels per unit of distance over a short interval — the density the
    /// graph is drawn at there. Negative, because x falls as distance grows.
    fn density(compression: &Compression, distance: f32) -> f32 {
        const STEP: f32 = 0.05;
        (compression.x_of(distance - STEP) - compression.x_of(distance + STEP)) / (2.0 * STEP)
    }

    /// How much distance a band holds, in the mapping's own units.
    fn held(compression: &Compression, zone: &Zone) -> f32 {
        zone.width() * zone.ratio() / compression.base()
    }

    #[test]
    fn compression_off_is_no_mapping_at_all() {
        let mut config = compressed();
        config.history_compression = false;
        assert_eq!(geometry(&config, 400.0), None);
    }

    #[test]
    fn the_gate_keeps_a_short_axis_uncompressed() {
        let mut config = compressed();
        config.scale = 1;
        config.window_seconds = 119;
        assert_eq!(
            geometry(&config, 119.0),
            None,
            "one pixel short of the gate"
        );
        config.window_seconds = 120;
        assert!(geometry(&config, 120.0).is_some(), "at the gate");
    }

    #[test]
    fn the_gate_reads_the_configured_axis_not_the_drawn_one() {
        // 60 s at 2x is 120 logical pixels: the canvas is over the gate even
        // though the physical axis a test hands in is smaller.
        let config = compressed();
        assert!(geometry(&config, 40.0).is_some());
        let mut short = compressed();
        short.window_seconds = 30;
        short.scale = 3;
        assert_eq!(geometry(&short, 400.0), None);
    }

    /// The ladder the whole feature exists for: a 120 px canvas with the 30 px
    /// floors is a 30 px no-compression band, six 10 px bands stepping 2, 4, 6,
    /// 8, 10, 12, and a 30 px reserve at 12.
    #[test]
    fn the_ladder_steps_evenly_from_two_to_the_ratio() {
        let mut config = compressed();
        config.scale = 1;
        config.window_seconds = 120;
        config.history_compression_ratio = 12;
        let config = floored(config);
        let compression = geometry(&config, 120.0).expect("compression");

        assert!((compression.no_zone() - 30.0).abs() < 0.01);
        assert!((compression.max_zone() - 30.0).abs() < 0.01);
        let ladder = &compression.zones()[..compression.zone_count() - 1];
        let ratios: Vec<f32> = ladder.iter().map(|zone| zone.ratio()).collect();
        assert_eq!(ratios, vec![2.0, 4.0, 6.0, 8.0, 10.0, 12.0]);
        for zone in ladder {
            assert!(
                (zone.width() - 10.0).abs() < 0.01,
                "ladder band is {} px",
                zone.width()
            );
        }
        let reserve = compression.zones()[compression.zone_count() - 1];
        assert_eq!(reserve.ratio(), 12.0, "the reserve holds the maximum");
        assert!((reserve.width() - 30.0).abs() < 0.01);
        // 30 s of raw history, the ladder's 20+40+60+80+100+120 s, and 30 px of
        // 12x reserve holding 360 s.
        assert!((compression.span() - 810.0).abs() < 0.01);
    }

    /// The count is dynamic in the canvas and bounded by the ratio: a wider
    /// canvas holds more bands until the ratio steps would fall below 1x.
    #[test]
    fn the_band_count_follows_the_canvas_and_the_ratio() {
        let narrow = floored(compressed());
        let narrow = geometry(&narrow, 120.0).expect("compression");

        let mut wide = compressed();
        wide.window_seconds = 600;
        let wide = floored(wide);
        let wide = geometry(&wide, 1200.0).expect("compression");

        assert!(
            wide.zone_count() > narrow.zone_count(),
            "a wider canvas has to hold more bands: {} against {}",
            wide.zone_count(),
            narrow.zone_count()
        );
        assert!(wide.zone_count() <= MAX_HISTORY_COMPRESSION_RATIO as usize);
        // Every ladder step is a whole ratio. The reserve may continue the last
        // ladder band when the ladder already reaches the maximum.
        let ladder = &wide.zones()[..wide.zone_count() - 1];
        for pair in ladder.windows(2) {
            assert!(
                pair[1].ratio() - pair[0].ratio() >= 1.0 - 1e-4,
                "a ratio step below 1x: {} then {}",
                pair[0].ratio(),
                pair[1].ratio()
            );
        }
        for zone in &wide.zones()[..wide.zone_count() - 1] {
            assert!(
                zone.width() >= MIN_COMPRESSION_ZONE_PX - 0.01,
                "ladder band is {} px",
                zone.width()
            );
        }
    }

    /// No room for a ladder is a legitimate answer: one step, raw straight to
    /// the maximum, with the reserve taking the whole middle.
    #[test]
    fn a_narrow_canvas_falls_back_to_the_deepest_band() {
        let mut config = compressed();
        config.scale = 1;
        config.window_seconds = 120;
        config.history_compression_no_zone_share = 0;
        config.history_compression_max_zone_share = 70;
        config.history_compression_max_zone_min_px = 0;
        let compression = geometry(&config, 120.0).expect("compression");

        assert_eq!(compression.zone_count(), 1, "just the reserve");
        assert_eq!(compression.zones()[0].ratio(), compression.ratio());
        assert!(
            (compression.max_zone() - 90.0).abs() < 0.01,
            "the reserve takes what the no-compression band does not: {}",
            compression.max_zone()
        );
    }

    /// The newest band is drawn at exactly the density the same overlay has
    /// with the feature off: the canvas covers its window at that density.
    #[test]
    fn the_no_compression_band_holds_the_raw_density() {
        let compression = geometry(&compressed(), 240.0).expect("compression");

        assert!((compression.no_zone() - 120.0).abs() < 0.01);
        assert!(
            (compression.base() - 4.0).abs() < 0.01,
            "240 px of canvas over a 60 s window is 4 px/s"
        );
        assert!((compression.no_zone_seconds() - 30.0).abs() < 0.01);
        // It ends exactly where the first band begins...
        assert!(
            (compression.x_of(compression.no_zone_seconds())
                - (compression.axis() - compression.no_zone()))
            .abs()
                < 0.01
        );
        // ...and holds one density the whole way across.
        assert!(
            (density(&compression, compression.no_zone_seconds() * 0.5).abs() - compression.base())
                .abs()
                < 0.01
        );
    }

    /// Every band holds one density, each is its ratio denser than the raw one,
    /// and the density steps — rather than falls smoothly — at a join.
    #[test]
    fn the_density_is_constant_inside_a_band() {
        let compression = geometry(&compressed(), 240.0).expect("compression");

        for zone in compression.zones() {
            let middle = zone.from() + held(&compression, zone) * 0.5;
            let drawn = density(&compression, middle).abs();
            let expected = compression.base() / zone.ratio();
            assert!(
                (drawn - expected).abs() / expected < 0.01,
                "the band at {}x drew {drawn}, expected {expected}",
                zone.ratio()
            );
        }
        let first = compression.zones()[0];
        let before = density(&compression, first.from() - 0.1).abs();
        let after = density(&compression, first.from() + 0.1).abs();
        assert!(
            (before / after - first.ratio()).abs() < 0.05,
            "the join did not step by {}x: {before} then {after}",
            first.ratio()
        );
    }

    #[test]
    fn the_mapping_is_monotonic() {
        let compression = geometry(&compressed(), 240.0).expect("compression");
        let end = compression.span() + 100.0;
        let mut previous = compression.x_of(-10.0);
        let mut distance = -9.9;
        while distance < end {
            let x = compression.x_of(distance);
            assert!(x < previous, "x rose at {distance}: {previous} then {x}");
            previous = x;
            distance += 0.1;
        }
    }

    /// The bands tile the canvas, and the span is exactly the history they hold
    /// between them — which is more than the window, or the feature would be
    /// pointless.
    #[test]
    fn the_bands_tile_the_canvas_and_hold_the_span() {
        let compression = geometry(&compressed(), 240.0).expect("compression");

        let mut tiled = compression.no_zone();
        for zone in compression.zones() {
            tiled += zone.width();
        }
        assert!(
            (tiled - compression.axis()).abs() < 0.01,
            "the bands cover {tiled} of {} px",
            compression.axis()
        );
        let mut total = compression.no_zone_seconds();
        for zone in compression.zones() {
            total += held(&compression, zone);
        }
        assert!((compression.span() - total).abs() < 0.01);
        assert!(
            compression.span() > compression.no_zone_seconds() * 2.0,
            "span {} against a {} s raw band",
            compression.span(),
            compression.no_zone_seconds()
        );
    }

    #[test]
    fn the_span_is_independent_of_the_dpi_scale() {
        // The same overlay at 100% and at 200%: the canvas doubles and so does
        // every band, so the history it covers is the same.
        let config = compressed();
        let small = geometry(&config, 120.0).expect("compression");
        let large = geometry(&config, 240.0).expect("compression");
        assert!((small.span() - large.span()).abs() < 0.01);
    }

    #[test]
    fn the_band_floors_hold_on_a_short_canvas() {
        let config = floored(compressed());
        let compression = geometry(&config, 120.0).expect("compression");
        assert!((compression.no_zone() - 30.0).abs() < 0.01);
        assert!(
            (compression.max_zone() - 30.0).abs() < 0.01,
            "the reserve keeps its own floor: {}",
            compression.max_zone()
        );
        for zone in &compression.zones()[..compression.zone_count() - 1] {
            assert!((zone.width() - 10.0).abs() < 0.01);
        }
    }

    #[test]
    fn the_floors_give_way_when_the_canvas_cannot_hold_them() {
        let mut config = compressed();
        config.scale = 1;
        config.window_seconds = 120;
        // Two 500 px floors cannot both fit in a 120 px canvas: they are scaled
        // down together, and the reserve takes the whole middle as well.
        config.history_compression_no_zone_min_px = 500;
        config.history_compression_max_zone_min_px = 500;
        let compression = geometry(&config, 120.0).expect("compression");
        assert!((compression.no_zone() - 60.0).abs() < 0.01);
        assert!((compression.max_zone() - 60.0).abs() < 0.01);
        assert_eq!(compression.zone_count(), 1);
    }
}
