//! Lengths and unit handling.
//!
//! Every dimension in the engine is a [`Length`]: an integer count of
//! nanometres. One nanometre divides both 1/64 inch (396 875 nm) and 0.1 mm
//! exactly, so imperial and metric values round-trip without drift and
//! converting a project between unit systems never changes its geometry.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

pub const NM_PER_MM: i64 = 1_000_000;
pub const NM_PER_INCH: i64 = 25_400_000;
pub const NM_PER_FOOT: i64 = 12 * NM_PER_INCH;

/// Largest length the parser accepts (1 km). Far beyond any room, and small
/// enough that sums and products of lengths stay well inside `i64`.
const MAX_PARSED_NM: i128 = 1_000 * 1_000 * NM_PER_MM as i128;

/// A length in integer nanometres. Serializes as a plain JSON number (serde
/// writes a newtype struct as its inner value).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "number"))]
pub struct Length(i64);

impl Length {
    pub const ZERO: Length = Length(0);

    pub const fn from_nm(nm: i64) -> Self {
        Length(nm)
    }

    pub const fn nm(self) -> i64 {
        self.0
    }

    pub const fn mm(mm: i64) -> Self {
        Length(mm * NM_PER_MM)
    }

    pub const fn inches(inches: i64) -> Self {
        Length(inches * NM_PER_INCH)
    }

    pub const fn feet(feet: i64) -> Self {
        Length(feet * NM_PER_FOOT)
    }

    /// `num/den` inches, e.g. `Length::inch_fraction(7, 2)` is 3 1/2".
    /// Exact for any power-of-two denominator up to 64.
    pub const fn inch_fraction(num: i64, den: i64) -> Self {
        Length(num * NM_PER_INCH / den)
    }

    pub fn as_mm(self) -> f64 {
        self.0 as f64 / NM_PER_MM as f64
    }

    pub fn as_inches(self) -> f64 {
        self.0 as f64 / NM_PER_INCH as f64
    }

    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    /// Rounds to the nearest multiple of `step` (ties round up). A
    /// non-positive `step` returns the length unchanged.
    pub fn round_to(self, step: Length) -> Length {
        if step.0 <= 0 {
            return self;
        }
        Length((self.0 + step.0 / 2).div_euclid(step.0) * step.0)
    }
}

impl fmt::Debug for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}mm", self.as_mm())
    }
}

impl Add for Length {
    type Output = Length;
    fn add(self, rhs: Length) -> Length {
        Length(self.0 + rhs.0)
    }
}

impl AddAssign for Length {
    fn add_assign(&mut self, rhs: Length) {
        self.0 += rhs.0;
    }
}

impl Sub for Length {
    type Output = Length;
    fn sub(self, rhs: Length) -> Length {
        Length(self.0 - rhs.0)
    }
}

impl SubAssign for Length {
    fn sub_assign(&mut self, rhs: Length) {
        self.0 -= rhs.0;
    }
}

impl Neg for Length {
    type Output = Length;
    fn neg(self) -> Length {
        Length(-self.0)
    }
}

impl Mul<i64> for Length {
    type Output = Length;
    fn mul(self, rhs: i64) -> Length {
        Length(self.0 * rhs)
    }
}

impl Div<i64> for Length {
    type Output = Length;
    fn div(self, rhs: i64) -> Length {
        Length(self.0 / rhs)
    }
}

impl Sum for Length {
    fn sum<I: Iterator<Item = Length>>(iter: I) -> Length {
        Length(iter.map(|l| l.0).sum())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum UnitSystem {
    Imperial,
    Metric,
}

/// How lengths are shown to and read from the user. This is a display
/// preference only; stored geometry is always nanometres.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UnitPrefs {
    pub system: UnitSystem,
    /// Smallest imperial fraction shown: 8, 16, 32 or 64 (meaning 1/8" etc).
    /// Ignored for metric.
    pub fraction_denominator: u32,
    /// Show `2' 8 1/2"` rather than `32 1/2"`. Ignored for metric.
    pub feet_and_inches: bool,
}

impl UnitPrefs {
    pub const fn imperial() -> Self {
        UnitPrefs {
            system: UnitSystem::Imperial,
            fraction_denominator: 16,
            feet_and_inches: false,
        }
    }

    pub const fn metric() -> Self {
        UnitPrefs {
            system: UnitSystem::Metric,
            fraction_denominator: 16,
            feet_and_inches: false,
        }
    }

    pub const fn for_system(system: UnitSystem) -> Self {
        match system {
            UnitSystem::Imperial => Self::imperial(),
            UnitSystem::Metric => Self::metric(),
        }
    }
}

/// Formats a length for display, e.g. `32 1/2"`, `2' 8 1/2"` or `826 mm`.
pub fn format_length(len: Length, prefs: &UnitPrefs) -> String {
    let sign = if len.0 < 0 { "-" } else { "" };
    let nm = len.0.unsigned_abs() as u128;
    match prefs.system {
        UnitSystem::Metric => {
            // Nearest 0.1 mm; the decimal is dropped when it is zero.
            let tenths = (nm + 50_000) / 100_000;
            if tenths.is_multiple_of(10) {
                format!("{sign}{} mm", tenths / 10)
            } else {
                format!("{sign}{}.{} mm", tenths / 10, tenths % 10)
            }
        }
        UnitSystem::Imperial => {
            let den = match prefs.fraction_denominator {
                d @ (1 | 2 | 4 | 8 | 16 | 32 | 64) => d as u128,
                _ => 16,
            };
            let per_inch = NM_PER_INCH as u128;
            let steps = (nm * den + per_inch / 2) / per_inch;
            let whole = steps / den;
            let frac = fraction(steps % den, den);
            if prefs.feet_and_inches && whole >= 12 {
                let inches = mixed(whole % 12, frac);
                format!("{sign}{}' {inches}\"", whole / 12)
            } else {
                format!("{sign}{}\"", mixed(whole, frac))
            }
        }
    }
}

/// A reduced fraction, or `None` when the numerator is zero.
fn fraction(num: u128, den: u128) -> Option<(u128, u128)> {
    if num == 0 {
        return None;
    }
    let g = gcd(num, den);
    Some((num / g, den / g))
}

fn mixed(whole: u128, frac: Option<(u128, u128)>) -> String {
    match (whole, frac) {
        (w, None) => w.to_string(),
        (0, Some((n, d))) => format!("{n}/{d}"),
        (w, Some((n, d))) => format!("{w} {n}/{d}"),
    }
}

fn gcd(a: u128, b: u128) -> u128 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseLengthError {
    Empty,
    Negative,
    TooLarge,
    Invalid(String),
}

impl fmt::Display for ParseLengthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseLengthError::Empty => write!(f, "enter a length"),
            ParseLengthError::Negative => write!(f, "a length cannot be negative"),
            ParseLengthError::TooLarge => write!(f, "that length is too large"),
            ParseLengthError::Invalid(input) => write!(
                f,
                "could not read \"{input}\" as a length (try 32 1/2, 2' 8\", or 826mm)"
            ),
        }
    }
}

impl std::error::Error for ParseLengthError {}

/// Parses a length typed by a user.
///
/// An explicit unit always wins, so `826mm` and `32 1/2"` mean the same thing
/// whatever the preference is. A bare number is read as inches when
/// `default` is imperial and millimetres when it is metric.
///
/// Accepted forms include `32 1/2`, `32-1/2"`, `32.5in`, `2' 8 1/2"`,
/// `2'-8"`, `8ft`, `826mm`, `82.6cm` and `2.4m`.
pub fn parse_length(input: &str, default: UnitSystem) -> Result<Length, ParseLengthError> {
    let s = normalize(input);
    let invalid = || ParseLengthError::Invalid(input.trim().to_owned());
    if s.is_empty() {
        return Err(ParseLengthError::Empty);
    }
    if s.starts_with('-') {
        return Err(ParseLengthError::Negative);
    }

    let metric_units = [
        ("mm", NM_PER_MM),
        ("cm", 10 * NM_PER_MM),
        ("m", 1000 * NM_PER_MM),
    ];
    for (suffix, unit) in metric_units {
        if let Some(number) = s.strip_suffix(suffix) {
            let value = decimal(number.trim()).ok_or_else(invalid)?;
            return to_length(value, unit);
        }
    }

    let imperial_marked = s.contains('\'') || s.contains('"');
    if imperial_marked || default == UnitSystem::Imperial {
        let inches = imperial_inches(&s).ok_or_else(invalid)?;
        to_length(inches, NM_PER_INCH)
    } else {
        let value = decimal(&s).ok_or_else(invalid)?;
        to_length(value, NM_PER_MM)
    }
}

/// Lowercases and rewrites every spelling of feet and inches to `'` and `"`.
fn normalize(input: &str) -> String {
    let mut s = input.trim().to_lowercase();
    for (from, to) in [
        ("\u{2032}", "'"),  // prime
        ("\u{2019}", "'"),  // right single quote
        ("\u{2033}", "\""), // double prime
        ("\u{201c}", "\""), // left double quote
        ("\u{201d}", "\""), // right double quote
        ("''", "\""),
        ("inches", "\""),
        ("inch", "\""),
        ("in.", "\""),
        ("in", "\""),
        ("feet", "'"),
        ("foot", "'"),
        ("ft.", "'"),
        ("ft", "'"),
    ] {
        if s.contains(from) {
            s = s.replace(from, to);
        }
    }
    s.trim().to_owned()
}

/// A non-negative rational number.
#[derive(Clone, Copy)]
struct Ratio {
    num: i128,
    den: i128,
}

impl Ratio {
    fn add(self, other: Ratio) -> Ratio {
        Ratio {
            num: self.num * other.den + other.num * self.den,
            den: self.den * other.den,
        }
    }

    fn scale(self, by: i128) -> Ratio {
        Ratio {
            num: self.num * by,
            den: self.den,
        }
    }
}

fn to_length(value: Ratio, unit_nm: i64) -> Result<Length, ParseLengthError> {
    // Round half up to the nearest nanometre.
    let nm = (value.num * unit_nm as i128 * 2 + value.den) / (value.den * 2);
    if nm > MAX_PARSED_NM {
        return Err(ParseLengthError::TooLarge);
    }
    Ok(Length(nm as i64))
}

/// Parses `[feet'] [inches["]]` into a number of inches.
fn imperial_inches(s: &str) -> Option<Ratio> {
    let (feet, rest) = match s.split_once('\'') {
        Some((feet, rest)) => (Some(feet.trim()), rest.trim()),
        None => (None, s),
    };
    let rest = rest.strip_suffix('"').unwrap_or(rest).trim();
    if rest.contains('"') || rest.contains('\'') {
        return None;
    }
    // Architectural style puts a dash between feet and inches: 2'-8".
    let rest = match feet {
        Some(_) => rest.strip_prefix('-').unwrap_or(rest).trim(),
        None => rest,
    };

    match (feet, rest.is_empty()) {
        (Some(feet), true) => Some(mixed_number(feet)?.scale(12)),
        (Some(feet), false) => Some(mixed_number(feet)?.scale(12).add(mixed_number(rest)?)),
        (None, false) => mixed_number(rest),
        (None, true) => None,
    }
}

/// Parses `32`, `32.5`, `1/2`, `32 1/2` or `32-1/2`.
fn mixed_number(s: &str) -> Option<Ratio> {
    let tokens: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|t| !t.is_empty())
        .collect();
    match tokens.as_slice() {
        [single] if single.contains('/') => simple_fraction(single),
        [single] => decimal(single),
        [whole, frac] => {
            let whole = Ratio {
                num: integer(whole)?,
                den: 1,
            };
            Some(whole.add(simple_fraction(frac)?))
        }
        _ => None,
    }
}

fn simple_fraction(s: &str) -> Option<Ratio> {
    let (num, den) = s.split_once('/')?;
    let (num, den) = (integer(num)?, integer(den)?);
    (den != 0).then_some(Ratio { num, den })
}

/// Digits only, short enough that no later arithmetic can overflow.
fn integer(s: &str) -> Option<i128> {
    if s.is_empty() || s.len() > 9 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Parses `12`, `12.5`, `.5` or `12.` exactly, without going through floats.
fn decimal(s: &str) -> Option<Ratio> {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    let whole = if whole.is_empty() { 0 } else { integer(whole)? };
    let frac_value = if frac.is_empty() { 0 } else { integer(frac)? };
    let den = 10i128.pow(frac.len() as u32);
    Some(Ratio {
        num: whole * den + frac_value,
        den,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imp(s: &str) -> Result<Length, ParseLengthError> {
        parse_length(s, UnitSystem::Imperial)
    }

    fn met(s: &str) -> Result<Length, ParseLengthError> {
        parse_length(s, UnitSystem::Metric)
    }

    #[test]
    fn sixty_fourths_and_tenths_of_mm_are_exact() {
        assert_eq!(Length::inch_fraction(1, 64).nm(), 396_875);
        assert_eq!(Length::inch_fraction(1, 64) * 64, Length::inches(1));
        assert_eq!(Length::inches(1), Length::from_nm(25_400_000));
        assert_eq!(Length::mm(254), Length::inches(10));
    }

    #[test]
    fn parses_bare_numbers_in_the_default_system() {
        assert_eq!(imp("32"), Ok(Length::inches(32)));
        assert_eq!(met("826"), Ok(Length::mm(826)));
        assert_eq!(imp("32.5"), Ok(Length::inch_fraction(65, 2)));
        assert_eq!(met("826.5"), Ok(Length::from_nm(826_500_000)));
        assert_eq!(imp(".5"), Ok(Length::inch_fraction(1, 2)));
    }

    #[test]
    fn parses_imperial_fractions() {
        let expected = Ok(Length::inch_fraction(65, 2));
        assert_eq!(imp("32 1/2"), expected);
        assert_eq!(imp("32-1/2"), expected);
        assert_eq!(imp("32 1/2\""), expected);
        assert_eq!(imp("  32   1/2 in "), expected);
        assert_eq!(imp("65/2"), expected);
        assert_eq!(imp("3/4"), Ok(Length::inch_fraction(3, 4)));
        assert_eq!(imp("5 13/64"), Ok(Length::inch_fraction(5 * 64 + 13, 64)));
    }

    #[test]
    fn parses_feet_and_inches() {
        let expected = Ok(Length::inch_fraction(65, 2));
        assert_eq!(imp("2' 8 1/2\""), expected);
        assert_eq!(imp("2'8-1/2\""), expected);
        assert_eq!(imp("2'-8 1/2\""), expected);
        assert_eq!(imp("2 ft 8 1/2 in"), expected);
        assert_eq!(imp("2 feet 8.5 inches"), expected);
        assert_eq!(imp("8'"), Ok(Length::feet(8)));
        assert_eq!(imp("8ft"), Ok(Length::feet(8)));
        assert_eq!(imp("8.5'"), Ok(Length::inches(102)));
        assert_eq!(imp("12' 0\""), Ok(Length::feet(12)));
        assert_eq!(imp("32''"), Ok(Length::inches(32)));
        assert_eq!(imp("2\u{2019} 8\u{201d}"), Ok(Length::inches(32)));
    }

    #[test]
    fn explicit_units_override_the_default() {
        assert_eq!(imp("826mm"), Ok(Length::mm(826)));
        assert_eq!(imp("82.6 cm"), Ok(Length::mm(826)));
        assert_eq!(imp("2.4m"), Ok(Length::mm(2400)));
        assert_eq!(met("32 1/2\""), Ok(Length::inch_fraction(65, 2)));
        assert_eq!(met("8'"), Ok(Length::feet(8)));
        assert_eq!(met("10 in"), Ok(Length::mm(254)));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(imp(""), Err(ParseLengthError::Empty));
        assert_eq!(imp("   "), Err(ParseLengthError::Empty));
        assert_eq!(imp("-3"), Err(ParseLengthError::Negative));
        assert_eq!(met("-3mm"), Err(ParseLengthError::Negative));
        assert_eq!(met("999999999m"), Err(ParseLengthError::TooLarge));
        for bad in [
            "abc",
            "1/0",
            "1/",
            "/2",
            "3 4 5",
            "1.2.3",
            "\"",
            "'",
            "3\" 4\"",
            "2' 3' 4\"",
            "1/2/3",
            "3 4",
            "1e3",
            "mm",
            "1,5",
        ] {
            assert!(
                matches!(imp(bad), Err(ParseLengthError::Invalid(_))),
                "{bad:?} should be invalid, got {:?}",
                imp(bad)
            );
        }
        // A bare fraction has no meaning in millimetres.
        assert!(matches!(met("32 1/2"), Err(ParseLengthError::Invalid(_))));
    }

    #[test]
    fn formats_imperial() {
        let p = UnitPrefs::imperial();
        assert_eq!(format_length(Length::inch_fraction(65, 2), &p), "32 1/2\"");
        assert_eq!(format_length(Length::inches(32), &p), "32\"");
        assert_eq!(format_length(Length::inch_fraction(3, 4), &p), "3/4\"");
        assert_eq!(format_length(Length::ZERO, &p), "0\"");
        assert_eq!(format_length(Length::inch_fraction(-3, 4), &p), "-3/4\"");
        // 13.2222" rounds to the nearest sixteenth.
        assert_eq!(format_length(Length::inches(119) / 9, &p), "13 1/4\"");
        // Rounding can carry into the next whole inch.
        assert_eq!(format_length(Length::inch_fraction(639, 64), &p), "10\"");
    }

    #[test]
    fn formats_imperial_precision_and_feet() {
        let mut p = UnitPrefs::imperial();
        let len = Length::inch_fraction(32 * 32 + 5, 32); // 32 5/32"
        assert_eq!(format_length(len, &p), "32 3/16\"");
        p.fraction_denominator = 32;
        assert_eq!(format_length(len, &p), "32 5/32\"");
        p.fraction_denominator = 8;
        assert_eq!(format_length(len, &p), "32 1/8\"");
        p.fraction_denominator = 7; // not a valid setting; falls back to 1/16
        assert_eq!(format_length(len, &p), "32 3/16\"");

        p = UnitPrefs::imperial();
        p.feet_and_inches = true;
        assert_eq!(
            format_length(Length::inch_fraction(65, 2), &p),
            "2' 8 1/2\""
        );
        assert_eq!(format_length(Length::feet(8), &p), "8' 0\"");
        assert_eq!(format_length(Length::inches(11), &p), "11\"");
        assert_eq!(
            format_length(Length::feet(1) + Length::inch_fraction(1, 2), &p),
            "1' 1/2\""
        );
    }

    #[test]
    fn formats_metric() {
        let p = UnitPrefs::metric();
        assert_eq!(format_length(Length::mm(826), &p), "826 mm");
        assert_eq!(format_length(Length::from_nm(826_500_000), &p), "826.5 mm");
        assert_eq!(format_length(Length::from_nm(826_960_000), &p), "827 mm");
        assert_eq!(format_length(Length::inches(1), &p), "25.4 mm");
        assert_eq!(format_length(Length::ZERO, &p), "0 mm");
    }

    #[test]
    fn format_then_parse_round_trips() {
        let mut feet = UnitPrefs::imperial();
        feet.feet_and_inches = true;
        for sixteenths in [0, 1, 7, 16, 200, 1_552, 2_305] {
            let len = Length::inch_fraction(sixteenths, 16);
            for prefs in [UnitPrefs::imperial(), feet] {
                let text = format_length(len, &prefs);
                assert_eq!(imp(&text), Ok(len), "{text}");
                assert_eq!(met(&text), Ok(len), "{text}");
            }
        }
        for tenths in [0, 1, 255, 8_265, 24_000] {
            let len = Length::from_nm(tenths * 100_000);
            let text = format_length(len, &UnitPrefs::metric());
            assert_eq!(met(&text), Ok(len), "{text}");
            assert_eq!(imp(&text), Ok(len), "{text}");
        }
    }

    #[test]
    fn rounds_to_a_step() {
        let sixteenth = Length::inch_fraction(1, 16);
        assert_eq!(
            (Length::inches(119) / 9).round_to(sixteenth),
            Length::inch_fraction(53, 4)
        );
        assert_eq!(Length::mm(10).round_to(Length::ZERO), Length::mm(10));
        assert_eq!(
            Length::from_nm(1_500_000).round_to(Length::mm(1)),
            Length::mm(2)
        );
    }

    #[test]
    fn serializes_as_a_plain_number() {
        assert_eq!(serde_json::to_string(&Length::mm(1)).unwrap(), "1000000");
        assert_eq!(
            serde_json::from_str::<Length>("25400000").unwrap(),
            Length::inches(1)
        );
    }
}
