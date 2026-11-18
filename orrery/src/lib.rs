//! `view4d-orrery`: where the planets are. Positions come from JPL's
//! approximate Keplerian elements ("Approximate Positions of the Planets",
//! E. M. Standish, Table 1: fitted to 1800–2050), heliocentric, in au, on
//! the ecliptic and equinox of J2000: x towards the equinox, z towards the
//! ecliptic's north pole.

use std::f64::consts::TAU;
use std::time::{SystemTime, UNIX_EPOCH};

mod scene;
pub use scene::{Distances, Orrery};

/// The Julian day of J2000.0, which the elements count from.
pub const J2000: f64 = 2_451_545.0;
/// Julian days in a Julian century, the elements' unit of time.
const CENTURY: f64 = 36_525.0;
/// The Julian day of the Unix epoch.
const UNIX_JD: f64 = 2_440_587.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Planet {
    Mercury,
    Venus,
    /// The Earth and Moon's barycentre, as the elements give it.
    Earth,
    Mars,
    Jupiter,
    Saturn,
    Uranus,
    Neptune,
}

impl Planet {
    /// The planets, from the Sun out.
    pub const ALL: [Planet; 8] = [
        Planet::Mercury,
        Planet::Venus,
        Planet::Earth,
        Planet::Mars,
        Planet::Jupiter,
        Planet::Saturn,
        Planet::Uranus,
        Planet::Neptune,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Planet::Mercury => "Mercury",
            Planet::Venus => "Venus",
            Planet::Earth => "Earth",
            Planet::Mars => "Mars",
            Planet::Jupiter => "Jupiter",
            Planet::Saturn => "Saturn",
            Planet::Uranus => "Uranus",
            Planet::Neptune => "Neptune",
        }
    }

    /// Mean radius, km.
    pub fn radius_km(self) -> f64 {
        match self {
            Planet::Mercury => 2_439.7,
            Planet::Venus => 6_051.8,
            Planet::Earth => 6_371.0,
            Planet::Mars => 3_389.5,
            Planet::Jupiter => 69_911.0,
            Planet::Saturn => 58_232.0,
            Planet::Uranus => 25_362.0,
            Planet::Neptune => 24_622.0,
        }
    }

    /// A colour to draw it in, sRGB.
    pub fn color(self) -> [u8; 3] {
        match self {
            Planet::Mercury => [170, 165, 160],
            Planet::Venus => [230, 210, 160],
            Planet::Earth => [90, 140, 220],
            Planet::Mars => [210, 110, 70],
            Planet::Jupiter => [215, 180, 140],
            Planet::Saturn => [225, 205, 150],
            Planet::Uranus => [160, 215, 225],
            Planet::Neptune => [90, 120, 220],
        }
    }

    /// Its elements and their rates per century (Table 1).
    fn elements(self) -> [[f64; 6]; 2] {
        // a (au), e, I (deg), L (deg), long. of perihelion (deg), long. of node (deg)
        match self {
            Planet::Mercury => [
                [0.38709927, 0.20563593, 7.00497902, 252.25032350, 77.45779628, 48.33076593],
                [0.00000037, 0.00001906, -0.00594749, 149472.67411175, 0.16047689, -0.12534081],
            ],
            Planet::Venus => [
                [0.72333566, 0.00677672, 3.39467605, 181.97909950, 131.60246718, 76.67984255],
                [0.00000390, -0.00004107, -0.00078890, 58517.81538729, 0.00268329, -0.27769418],
            ],
            Planet::Earth => [
                [1.00000261, 0.01671123, -0.00001531, 100.46457166, 102.93768193, 0.0],
                [0.00000562, -0.00004392, -0.01294668, 35999.37244981, 0.32327364, 0.0],
            ],
            Planet::Mars => [
                [1.52371034, 0.09339410, 1.84969142, -4.55343205, -23.94362959, 49.55953891],
                [0.00001847, 0.00007882, -0.00813131, 19140.30268499, 0.44441088, -0.29257343],
            ],
            Planet::Jupiter => [
                [5.20288700, 0.04838624, 1.30439695, 34.39644051, 14.72847983, 100.47390909],
                [-0.00011607, -0.00013253, -0.00183714, 3034.74612775, 0.21252668, 0.20469106],
            ],
            Planet::Saturn => [
                [9.53667594, 0.05386179, 2.48599187, 49.95424423, 92.59887831, 113.66242448],
                [-0.00125060, -0.00050991, 0.00193609, 1222.49362201, -0.41897216, -0.28867794],
            ],
            Planet::Uranus => [
                [19.18916464, 0.04725744, 0.77263783, 313.23810451, 170.95427630, 74.01692503],
                [-0.00196176, -0.00004397, -0.00242939, 428.48202785, 0.40805281, 0.04240589],
            ],
            Planet::Neptune => [
                [30.06992276, 0.00859048, 1.77004347, -55.12002969, 44.96476227, 131.78422574],
                [0.00026291, 0.00005105, 0.00035372, 218.45945325, -0.32241464, -0.00508664],
            ],
        }
    }

    /// Its orbit at Julian day `jd` (TDB; UTC is within a minute and
    /// a half, which nothing here can see).
    pub fn orbit(self, jd: f64) -> Orbit {
        let t = (jd - J2000) / CENTURY;
        let [base, rate] = self.elements();
        let el: [f64; 6] = std::array::from_fn(|i| base[i] + rate[i] * t);
        let [a, e, i, l, peri, node] = el;
        Orbit {
            a,
            e,
            incl: i.to_radians(),
            node: node.to_radians(),
            arg_peri: (peri - node).to_radians(),
            mean_anomaly: (l - peri).to_radians(),
        }
    }

    /// Where it is at Julian day `jd`: au, heliocentric ecliptic J2000.
    pub fn position(self, jd: f64) -> [f64; 3] {
        let o = self.orbit(jd);
        o.at(o.eccentric_anomaly())
    }
}

/// An orbit at one moment: an ellipse and where on it the planet is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    /// Semi-major axis, au.
    pub a: f64,
    pub e: f64,
    /// Inclination, longitude of the ascending node and argument of
    /// perihelion, radians.
    pub incl: f64,
    pub node: f64,
    pub arg_peri: f64,
    /// Radians, any multiple of a turn.
    pub mean_anomaly: f64,
}

impl Orbit {
    /// Kepler's equation, M = E − e sin E, solved for E by Newton's method.
    pub fn eccentric_anomaly(&self) -> f64 {
        let m = self.mean_anomaly.rem_euclid(TAU);
        let mut ea = if self.e < 0.8 { m } else { std::f64::consts::PI };
        for _ in 0..32 {
            let step = (ea - self.e * ea.sin() - m) / (1.0 - self.e * ea.cos());
            ea -= step;
            if step.abs() < 1e-14 {
                break;
            }
        }
        ea
    }

    /// The point at eccentric anomaly `ea`, as [`Planet::position`].
    pub fn at(&self, ea: f64) -> [f64; 3] {
        // In the orbit's plane, x towards perihelion.
        let x = self.a * (ea.cos() - self.e);
        let y = self.a * (1.0 - self.e * self.e).sqrt() * ea.sin();
        let (so, co) = self.arg_peri.sin_cos();
        let (sn, cn) = self.node.sin_cos();
        let (si, ci) = self.incl.sin_cos();
        [
            (co * cn - so * sn * ci) * x + (-so * cn - co * sn * ci) * y,
            (co * sn + so * cn * ci) * x + (-so * sn + co * cn * ci) * y,
            (so * si) * x + (co * si) * y,
        ]
    }

    /// `n` points evenly round the ellipse (by eccentric anomaly), for
    /// drawing it.
    pub fn path(&self, n: usize) -> Vec<[f64; 3]> {
        (0..n).map(|k| self.at(k as f64 / n as f64 * TAU)).collect()
    }
}

/// The Julian day of `t`.
pub fn julian_day(t: SystemTime) -> f64 {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64(),
        Err(e) => -e.duration().as_secs_f64(),
    };
    UNIX_JD + secs / 86_400.0
}

/// The calendar date and time (UTC, proleptic Gregorian) of Julian day
/// `jd`: year, month, day, hour, minute.
pub fn calendar(jd: f64) -> (i64, u32, u32, u32, u32) {
    // Fliegel and Van Flandern, on the day starting at midnight.
    let jd = jd + 0.5;
    let day = jd.floor();
    let mins = ((jd - day) * 1440.0).round() as u32;
    let (day, mins) = if mins == 1440 { (day + 1.0, 0) } else { (day, mins) };
    let mut l = day as i64 + 68_569;
    let n = 4 * l / 146_097;
    l -= (146_097 * n + 3) / 4;
    let i = 4000 * (l + 1) / 1_461_001;
    l = l - 1461 * i / 4 + 31;
    let j = 80 * l / 2447;
    let d = l - 2447 * j / 80;
    l = j / 11;
    let m = j + 2 - 12 * l;
    let y = 100 * (n - 49) + i + l;
    (y, m as u32, d as u32, mins / 60, mins % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Longitude and latitude (arcsec) and distance (km) of `p`.
    fn spherical([x, y, z]: [f64; 3]) -> (f64, f64, f64) {
        let r = (x * x + y * y + z * z).sqrt();
        let arcsec = 180.0 / std::f64::consts::PI * 3600.0;
        (y.atan2(x) * arcsec, (z / r).asin() * arcsec, r * 149_597_870.7)
    }

    /// JPL Horizons: ecliptic J2000 positions, au, of each planet's system
    /// barycentre (the Earth's: the Earth and Moon's), at 2000-01-01 12:00
    /// and 2025-06-01 00:00 TDB. From the Sun's centre, but Uranus and
    /// Neptune from the solar system's barycentre: their elements follow
    /// that, and the Sun's wobble about it (0.005–0.01 au, as far as 60″
    /// seen from Neptune) is four times JPL's error for Neptune. (The
    /// inner planets are hundreds of times out from the barycentre.)
    // As Horizons prints them, a digit past what an f64 holds.
    #[allow(clippy::excessive_precision)]
    const HORIZONS: [(Planet, [f64; 3], [f64; 3]); 8] = [
        (
            Planet::Mercury,
            [-1.300936053754522E-01, -4.472876181353563E-01, -2.459830695805179E-02],
            [5.207750179288951E-02, 3.024613477313662E-01, 1.994115596242635E-02],
        ),
        (
            Planet::Venus,
            [-7.183022963453890E-01, -3.265430819980606E-02, 4.101418202684621E-02],
            [2.952293498160070E-01, -6.649367236232667E-01, -2.616988963434095E-02],
        ),
        (
            Planet::Earth,
            [-1.771587841839055E-01, 9.672193524609504E-01, -1.139275508446145E-06],
            [-3.398144910415934E-01, -9.553159949651717E-01, 5.869673718973563E-05],
        ),
        (
            Planet::Mars,
            [1.390715921746351E+00, -1.341631815101244E-02, -3.446766277581799E-02],
            [-1.651258523907133E+00, 1.177966746251616E-01, 4.296152758140306E-02],
        ),
        (
            Planet::Jupiter,
            [4.001177161126058E+00, 2.938576106966103E+00, -1.017852766337440E-01],
            [-9.009424222141207E-02, 5.133533464262957E+00, -1.930881210921232E-02],
        ),
        (
            Planet::Saturn,
            [6.406408859532809E+00, 6.569989616912557E+00, -3.690764262448534E-01],
            [9.532091188770105E+00, -9.322444333894778E-01, -3.631968439264363E-01],
        ),
        (
            Planet::Uranus,
            [1.442471943522018E+01, -1.373711746626653E+01, -2.379353892930592E-01],
            [1.059970104243165E+01, 1.639177270688362E+01, -7.644267827894669E-02],
        ),
        (
            Planet::Neptune,
            [1.680490978889128E+01, -2.499455888678026E+01, 1.274291784264387E-01],
            [2.987742674358717E+01, -1.623262589762108E-01, -6.852129404481253E-01],
        ),
    ];

    /// JPL's nominal errors of Table 1 over 1800–2050: longitude and
    /// latitude in arcsec, distance in 1000 km. Nominal, not worst: Saturn's
    /// distance at J2000 is 1.15 times it, so the test allows half as much
    /// again.
    fn accuracy(p: Planet) -> (f64, f64, f64) {
        match p {
            Planet::Mercury => (15.0, 1.0, 1.0),
            Planet::Venus => (20.0, 1.0, 4.0),
            Planet::Earth => (20.0, 8.0, 6.0),
            Planet::Mars => (40.0, 2.0, 25.0),
            Planet::Jupiter => (400.0, 10.0, 600.0),
            Planet::Saturn => (600.0, 25.0, 1500.0),
            Planet::Uranus => (50.0, 2.0, 1000.0),
            Planet::Neptune => (10.0, 1.0, 200.0),
        }
    }

    #[test]
    fn positions_are_as_jpl_gives_them() {
        let june_2025 = 2_460_827.5;
        for (planet, at_j2000, at_2025) in HORIZONS {
            for (jd, want) in [(J2000, at_j2000), (june_2025, at_2025)] {
                let (lon, lat, r) = spherical(planet.position(jd));
                let (wlon, wlat, wr) = spherical(want);
                let dlon = (lon - wlon + 648_000.0).rem_euclid(1_296_000.0) - 648_000.0;
                let (tlon, tlat, tr) = accuracy(planet);
                let (tlon, tlat, tr) = (tlon * 1.5, tlat * 1.5, tr * 1.5);
                let errors = (dlon.abs(), (lat - wlat).abs(), (r - wr).abs() / 1000.0);
                assert!(
                    errors.0 <= tlon && errors.1 <= tlat && errors.2 <= tr,
                    "{} at JD {jd}: off by {errors:?} (arcsec, arcsec, 1000 km), JPL allows {:?}",
                    planet.name(),
                    (tlon, tlat, tr)
                );
            }
        }
    }

    #[test]
    fn kepler() {
        for e in [0.0, 0.2, 0.9] {
            for m in [0.0, 0.5, 3.0, 6.0, -1.0, 20.0] {
                let o = Orbit { a: 1.0, e, incl: 0.0, node: 0.0, arg_peri: 0.0, mean_anomaly: m };
                let ea = o.eccentric_anomaly();
                let back = ea - e * ea.sin();
                assert!((back - m.rem_euclid(TAU)).abs() < 1e-12, "e {e}, M {m}");
            }
        }
        // A circle in the ecliptic: perihelion along x, a quarter turn on y.
        let o = Orbit { a: 2.0, e: 0.0, incl: 0.0, node: 0.0, arg_peri: 0.0, mean_anomaly: 0.0 };
        let q = o.at(TAU / 4.0);
        assert!(q[0].abs() < 1e-12 && (q[1] - 2.0).abs() < 1e-12 && q[2] == 0.0);
        assert_eq!(o.path(4).len(), 4);
    }

    #[test]
    fn dates() {
        assert_eq!(julian_day(UNIX_EPOCH), UNIX_JD);
        assert_eq!(calendar(J2000), (2000, 1, 1, 12, 0));
        assert_eq!(calendar(2_460_827.5), (2025, 6, 1, 0, 0));
        assert_eq!(calendar(UNIX_JD + 0.75), (1970, 1, 1, 18, 0));
        assert_eq!(calendar(2_451_544.4999999), (2000, 1, 1, 0, 0), "a moment before midnight rounds up");
    }
}
