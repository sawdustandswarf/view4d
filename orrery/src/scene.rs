//! The orrery as a scene: the Sun, the planets where they are, and their
//! orbits, seen from above the ecliptic by default.
//!
//! No one scale shows it all: at true distances the inner planets crowd
//! the Sun, and at true sizes every planet is far smaller than a pixel. So
//! distances are mapped ([`Distances`]) and the bodies drawn big: a size
//! every planet has, and more by the square root of its radius, against
//! the size of the whole.

use std::time::{Instant, SystemTime};

use view4d_scene::shapes::sphere;
use view4d_scene::{Aabb, Label, Lines, Mesh, Scene, Vec3};

use crate::{Planet, calendar, julian_day};

/// How distances from the Sun are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distances {
    /// In au, as they are.
    True,
    /// By the logarithm: each planet's orbit clear of the last.
    Log,
    /// Each planet's orbit one step further out than the last.
    Even,
}

impl Distances {
    /// The next, round and round.
    pub fn next(self) -> Distances {
        match self {
            Distances::Log => Distances::Even,
            Distances::Even => Distances::True,
            Distances::True => Distances::Log,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Distances::True => "true",
            Distances::Log => "log",
            Distances::Even => "even",
        }
    }
}

/// How fast time can go: days a second, and what to call it.
const RATES: [(f64, &str); 8] = [
    (1.0 / 86_400.0, "real time"),
    (1.0 / 1440.0, "a minute a second"),
    (1.0 / 24.0, "an hour a second"),
    (1.0, "a day a second"),
    (7.0, "a week a second"),
    (30.0, "a month a second"),
    (365.25, "a year a second"),
    (3652.5, "ten years a second"),
];
/// Julian days of 1800-01-01 and 2050-01-01, the years the elements fit.
const FITTED: (f64, f64) = (2_378_496.5, 2_469_807.5);

/// The orrery's own time: a moment, and how fast it moves on from it.
#[derive(Debug, Clone, Copy)]
struct Clock {
    /// When it was last set, and the Julian day it was set to.
    set_at: Instant,
    set_to: f64,
    /// Into [`RATES`].
    rate: usize,
    paused: bool,
}

impl Clock {
    /// The Julian day at `now`.
    fn jd(&self, now: Instant) -> f64 {
        if self.paused {
            return self.set_to;
        }
        self.set_to + now.saturating_duration_since(self.set_at).as_secs_f64() * RATES[self.rate].0
    }
}

/// Where [`Distances::Log`] turns from nearly even to logarithmic, au.
const LOG_KNEE: f64 = 0.2;
/// Points on each orbit's line.
const ORBIT_POINTS: usize = 180;
/// The Sun's radius, the radius every planet has, and what Jupiter's size
/// adds to it, as fractions of Neptune's distance as drawn; and none more
/// than these fractions of Mercury's, so the Sun doesn't swallow it at
/// true distances.
const SUN_SIZE: f32 = 0.06;
const PLANET_SIZE: f32 = 0.014;
const JUPITER_SIZE: f32 = 0.03;
const SUN_MAX: f32 = 0.4;
const PLANET_MAX: f32 = 0.25;
const SUN_COLOR: [u8; 3] = [255, 205, 90];
/// How far towards white a planet's name is, to read on the dark.
const LABEL_LIGHT: f32 = 0.4;
/// How much of a planet's colour its orbit is drawn in.
const ORBIT_SHADE: f32 = 0.5;

#[derive(Debug)]
pub struct Orrery {
    /// The moment shown, a Julian day.
    jd: f64,
    clock: Clock,
    distances: Distances,
    meshes: Vec<Mesh>,
    lines: Vec<Lines>,
    /// The planets' names, or none (`l`).
    labels: Vec<Label>,
    named: bool,
    bounds: Aabb,
}

impl Orrery {
    /// The planets now, in real time.
    pub fn now() -> Orrery {
        let mut o = Orrery::at(julian_day(SystemTime::now()));
        o.clock.paused = false;
        o
    }

    /// The planets at Julian day `jd`, paused.
    pub fn at(jd: f64) -> Orrery {
        let clock = Clock { set_at: Instant::now(), set_to: jd, rate: 0, paused: true };
        let mut o = Orrery {
            jd,
            clock,
            distances: Distances::Log,
            meshes: Vec::new(),
            lines: Vec::new(),
            labels: Vec::new(),
            named: true,
            bounds: Aabb::EMPTY,
        };
        o.build();
        o
    }

    pub fn jd(&self) -> f64 {
        self.jd
    }

    /// Move on to `now` by its clock; true if what is shown changed (every
    /// time while it runs faster than real time, else when the minute does).
    pub fn update(&mut self, now: Instant) -> bool {
        let jd = self.clock.jd(now);
        let shown = calendar(self.jd);
        self.jd = jd;
        if !self.animates() && calendar(jd) == shown {
            return false;
        }
        self.build();
        true
    }

    /// Apply a time or scale key at `now`: Space pauses, `+` and `-` go
    /// faster and slower, `.` and `,` step a day, `>` and `<` a year, `n`
    /// is now (in real time), `d` the next distance scale, `l` the names.
    pub fn key_at(&mut self, key: char, now: Instant) -> bool {
        let jd = self.clock.jd(now);
        let c = &mut self.clock;
        match key {
            ' ' => c.paused = !c.paused,
            '+' | '=' => c.rate = (c.rate + 1).min(RATES.len() - 1),
            '-' | '_' => c.rate = c.rate.saturating_sub(1),
            '.' => c.set_to = jd + 1.0,
            ',' => c.set_to = jd - 1.0,
            '>' => c.set_to = jd + 365.25,
            '<' => c.set_to = jd - 365.25,
            'n' => {
                *c = Clock { set_at: now, set_to: julian_day(SystemTime::now()), rate: 0, paused: false };
            }
            'd' => self.distances = self.distances.next(),
            'l' => self.named = !self.named,
            _ => return false,
        }
        // Steps move from the moment shown; everything goes on from now.
        if !matches!(key, '.' | ',' | '>' | '<' | 'n') {
            self.clock.set_to = jd;
        }
        self.clock.set_at = now;
        self.jd = self.clock.jd(now);
        self.build();
        true
    }

    /// `r` au from the Sun, as drawn.
    fn distance(&self, r: f64) -> f64 {
        match self.distances {
            Distances::True => r,
            Distances::Log => (r / LOG_KNEE).ln_1p(),
            Distances::Even => {
                let a: Vec<f64> = Planet::ALL.iter().map(|p| p.orbit(self.jd).a).collect();
                match a.iter().position(|&ak| r < ak) {
                    Some(0) => r / a[0],
                    Some(k) => k as f64 + (r - a[k - 1]) / (a[k] - a[k - 1]),
                    None => {
                        let n = a.len();
                        n as f64 + (r - a[n - 1]) / (a[n - 1] - a[n - 2])
                    }
                }
            }
        }
    }

    /// Heliocentric `p`, au, as drawn: the same direction, its distance
    /// mapped.
    fn place(&self, [x, y, z]: [f64; 3]) -> Vec3 {
        let r = (x * x + y * y + z * z).sqrt();
        let k = if r > 0.0 { self.distance(r) / r } else { 0.0 };
        Vec3::new((x * k) as f32, (y * k) as f32, (z * k) as f32)
    }

    /// Everything, again, at `self.jd`.
    fn build(&mut self) {
        let extent = self.distance(Planet::Neptune.orbit(self.jd).a) as f32;
        let inner = self.distance(Planet::Mercury.orbit(self.jd).a) as f32;
        let jupiter = Planet::Jupiter.radius_km();
        self.meshes = vec![sphere(Vec3::ZERO, (SUN_SIZE * extent).min(SUN_MAX * inner), SUN_COLOR)];
        self.lines.clear();
        self.labels.clear();
        self.bounds = Aabb::EMPTY;
        for p in Planet::ALL {
            let orbit = p.orbit(self.jd);
            let at = self.place(orbit.at(orbit.eccentric_anomaly()));
            let size = PLANET_SIZE + JUPITER_SIZE * (p.radius_km() / jupiter).sqrt() as f32;
            let radius = (size * extent).min(PLANET_MAX * inner);
            self.meshes.push(sphere(at, radius, p.color()));
            if self.named {
                // Beside it, past its edge as seen from anywhere.
                let at = at + Vec3::new(0.0, 0.0, radius);
                let color = p.color().map(|c| (c as f32 + (255.0 - c as f32) * LABEL_LIGHT) as u8);
                self.labels.push(Label { at, text: p.name().into(), color });
            }
            let path: Vec<Vec3> = orbit.path(ORBIT_POINTS).into_iter().map(|q| self.place(q)).collect();
            let segments = (0..path.len()).map(|i| [path[i], path[(i + 1) % path.len()]]).collect();
            path.iter().for_each(|&q| self.bounds.add(q));
            let color = p.color().map(|c| (c as f32 * ORBIT_SHADE) as u8);
            self.lines.push(Lines { segments, color });
        }
        for m in &self.meshes {
            self.bounds = self.bounds.union(m.bounds());
        }
    }
}

impl Scene for Orrery {
    fn meshes(&self) -> &[Mesh] {
        &self.meshes
    }

    fn lines(&self) -> &[Lines] {
        &self.lines
    }

    fn bounds(&self) -> Aabb {
        self.bounds
    }

    /// The moment, to the minute, how fast it goes, and the scale.
    fn status(&self) -> String {
        let (y, mo, d, h, mi) = calendar(self.jd);
        let rate = if self.clock.paused { "paused" } else { RATES[self.clock.rate].1 };
        format!("{y}-{mo:02}-{d:02} {h:02}:{mi:02} UTC  {rate}  distances {}", self.distances.name())
    }

    fn error(&self) -> Option<&str> {
        let fitted = FITTED.0..FITTED.1;
        (!fitted.contains(&self.jd)).then_some("outside 1800-2050, where the elements fit: positions drift")
    }

    fn key(&mut self, key: char) -> bool {
        self.key_at(key, Instant::now())
    }

    fn poll(&mut self) -> bool {
        self.update(Instant::now())
    }

    fn animates(&self) -> bool {
        !self.clock.paused && self.clock.rate > 0
    }

    fn labels(&self) -> &[Label] {
        &self.labels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::J2000;

    #[test]
    fn the_sun_planets_and_orbits() {
        let o = Orrery::at(J2000);
        assert_eq!((o.meshes().len(), o.lines().len()), (9, 8), "the Sun and eight planets; eight orbits");
        assert_eq!(o.status(), "2000-01-01 12:00 UTC  paused  distances log");
        // The Earth where it is, in the direction it really is.
        let e = Planet::Earth.position(J2000);
        let drawn = o.meshes()[3].bounds().center();
        let want = Vec3::new(e[0] as f32, e[1] as f32, e[2] as f32).normalized();
        assert!(drawn.normalized().dot(want) > 0.9999, "{drawn:?}");
        // Bigger planets are drawn bigger, and the Sun biggest.
        let size = |i: usize| o.meshes()[i].bounds().size().x;
        assert!(size(0) > size(5) && size(5) > size(3) && size(3) > size(1));
        assert!(o.bounds().size().x > 2.0 * o.distance(29.0) as f32);
    }

    #[test]
    fn time_goes_as_told() {
        use std::time::Duration;
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut o = Orrery::at(J2000);
        assert!(!o.update(s(10)) && o.jd() == J2000, "paused");
        assert!(!o.animates());
        o.key_at(' ', s(10));
        o.key_at('+', s(10));
        o.key_at('+', s(10));
        o.key_at('+', s(10));
        assert!(o.animates());
        assert!(o.status().contains("a day a second"), "{}", o.status());
        assert!(o.update(s(13)));
        assert!((o.jd() - (J2000 + 3.0)).abs() < 1e-9, "three days in three seconds");
        o.key_at(' ', s(13));
        o.key_at('>', s(20));
        assert!((o.jd() - (J2000 + 3.0 + 365.25)).abs() < 1e-9, "paused, then a year on");
        o.key_at(',', s(20));
        assert!(o.status().contains("paused") && !o.animates());
        assert!((o.jd() - (J2000 + 2.0 + 365.25)).abs() < 1e-9);
        for _ in 0..20 {
            o.key_at('-', s(20));
        }
        o.key_at(' ', s(20));
        assert!(!o.animates(), "real time: drawn when the minute changes");
        assert!(o.status().contains("real time"));
        o.key_at('n', s(20));
        assert!((o.jd() - julian_day(SystemTime::now())).abs() < 1e-3, "now");
        assert!(!o.key_at('x', s(20)));
    }

    #[test]
    fn names() {
        let mut o = Orrery::at(J2000);
        let names: Vec<&str> = o.labels().iter().map(|l| l.text.as_str()).collect();
        assert_eq!(names, ["Mercury", "Venus", "Earth", "Mars", "Jupiter", "Saturn", "Uranus", "Neptune"]);
        assert!(o.key_at('l', Instant::now()) && o.labels().is_empty());
    }

    #[test]
    fn outside_the_fitted_years() {
        assert_eq!(Orrery::at(J2000).error(), None);
        assert!(Orrery::at(FITTED.1 + 1.0).error().is_some());
        assert!(Orrery::at(FITTED.0 - 1.0).error().is_some());
    }

    #[test]
    fn the_sun_never_swallows_mercury() {
        let mut o = Orrery::at(J2000);
        for _ in 0..3 {
            o.key_at('d', Instant::now());
            let sun = o.meshes()[0].bounds().size().x / 2.0;
            let mercury = o.meshes()[1].bounds().center().length();
            let r = o.meshes()[1].bounds().size().x / 2.0;
            assert!(sun + r < mercury, "{:?}: the Sun {sun}, Mercury {r} at {mercury}", o.distances);
        }
    }

    #[test]
    fn distances_keep_order() {
        let mut o = Orrery::at(J2000);
        for d in [Distances::True, Distances::Log, Distances::Even] {
            o.distances = d;
            let rs: Vec<f64> = [0.0, 0.1, 0.39, 0.5, 1.0, 5.0, 29.0, 30.07, 40.0].map(|r| o.distance(r)).to_vec();
            assert!(rs.windows(2).all(|w| w[0] < w[1]), "{d:?}: {rs:?}");
            assert_eq!(rs[0], 0.0);
        }
        o.distances = Distances::Even;
        let a = Planet::Earth.orbit(J2000).a;
        assert!((o.distance(a) - 3.0).abs() < 1e-9, "the Earth's orbit is the third");
    }
}
