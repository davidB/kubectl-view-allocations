// see [Definitions of the SI units: The binary prefixes](https://physics.nist.gov/cuu/Units/binary.html)
// see [Managing Compute Resources for Containers - Kubernetes](https://kubernetes.io/docs/concepts/configuration/manage-compute-resources-container/)
//TODO rewrite to support exponent, ... see [apimachinery/quantity.go at master · kubernetes/apimachinery](https://github.com/kubernetes/apimachinery/blob/master/pkg/api/resource/quantity.go)

use std::str::FromStr;
use std::cmp::Ordering;
use failure::{Error,format_err};

#[derive(Debug,Clone,Eq,PartialEq, Default)]
struct Scale {
    label: &'static str,
    base: u32,
    pow: i32,
}

// should be sorted in DESC
static SCALES: [Scale;11] = [
    Scale{ label:"Pi", base: 2, pow: 50},
    Scale{ label:"Ti", base: 2, pow: 40},
    Scale{ label:"Gi", base: 2, pow: 30},
    Scale{ label:"Mi", base: 2, pow: 20},
    Scale{ label:"Ki", base: 2, pow: 10},
    Scale{ label:"P", base: 10, pow: 12},
    Scale{ label:"G", base: 10, pow: 9},
    Scale{ label:"M", base: 10, pow: 6},
    Scale{ label:"k", base: 10, pow: 3},
    Scale{ label:"", base: 10, pow: 0},
    Scale{ label:"m", base: 10, pow: -3},
];

impl FromStr for Scale {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        SCALES.iter().find(|v| v.label == s).cloned().ok_or(format_err!("scale not found"))
    }
}

impl From<&Scale> for f64 {
    fn from(v: &Scale) -> f64 {
        if v.pow == 0 || v.base == 0 {
            1.0
        } else {
            (v.base as f64).powf(v.pow as f64)
        }
    }
}

impl PartialOrd for Scale {
    //TODO optimize accuracy with big number
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let v1 = f64::from(self);
        let v2 = f64::from(other);
        if v1 > v2 {
            Some(Ordering::Greater)
        } else if v1 < v2 {
            Some(Ordering::Less)
        } else if v1 == v2 {
            Some(Ordering::Equal)
        } else {
            None
        }
    }
}

#[derive(Debug,Clone, Eq, PartialEq, Default)]
pub struct Qty {
    value: i64,
    scale: Scale,
}

impl From<&Qty> for f64 {
    fn from(v: &Qty) -> f64 {
        (v.value as f64) * f64::from(&v.scale)
    }
}

impl Qty {
    pub fn calc_percentage(&self, base100: &Self) -> f64 {
        if base100.value != 0 {
            f64::from(self) * 100f64 / f64::from(base100)
        } else {
            core::f64::NAN
        }
    }

    pub fn adjust_scale(&self) -> Qty {
        let value = f64::from(self);
        let scale = SCALES.iter().filter(|s| s.base == self.scale.base || s.base == 0)
            .find(|s| f64::from(*s) <= value)
            ;
        match scale {
            Some(scale) => Qty{
                value: (value / f64::from(scale)) as i64,
                scale: scale.clone(),
            },
            None => self.clone(),
        }
    }
}

impl FromStr for Qty {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (num_str, scale_str): (&str, &str) = match s.find(|c: char| !c.is_digit(10)) {
            Some(pos) => (&s[..pos], &s[pos..]),
            None => (s, ""),
        };
        let value = i64::from_str(num_str)?;
        let scale = Scale::from_str(scale_str.trim())?;
        Ok(Qty{value, scale})
    }
}

impl std::fmt::Display for Qty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.value, self.scale.label)
    }
}

impl PartialOrd for Qty {
    //TODO optimize accuracy with big number
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let v1 = f64::from(self);
        let v2 = f64::from(other);
        if v1 > v2 {
            Some(Ordering::Greater)
        } else if v1 < v2 {
            Some(Ordering::Less)
        } else if v1 == v2 {
            Some(Ordering::Equal)
        } else {
            None
        }
    }
}

// impl PartialOrd for Person {
//     fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
//         Some(self.cmp(other))
//     }
// }

// impl PartialEq for Person {
//     fn eq(&self, other: &Self) -> bool {
//         self.height == other.height
//     }
// }

impl std::ops::Add for &Qty {
    type Output = Qty;
    //TODO optimize + use int
    fn add(self, other: Self) -> Qty {
        let v1 = f64::from(self);
        let v2 = f64::from(other);
        Qty{
            value: (v1 + v2) as i64,
            scale: Scale{label: "", base: self.scale.base.min(other.scale.base), pow: 0},
        }
    }
}

impl<'b> std::ops::AddAssign<&'b Qty> for Qty {
    fn add_assign(&mut self, other: &'b Self) {
        let v1 = f64::from(&*self);
        let v2 = f64::from(other);
        *self = Qty{
            value: (v1 + v2) as i64,
            scale: Scale{label: "", base: self.scale.base.min(other.scale.base), pow: 0},
        };
    }
}

impl std::ops::Sub for &Qty {
    type Output = Qty;
    //TODO optimize
    fn sub(self, other: Self) -> Qty {
        let v1 = f64::from(self);
        let v2 = f64::from(other);
        Qty{
            value: (v1 - v2) as i64,
            scale: Scale{label: "", base: self.scale.base.min(other.scale.base), pow: 0},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectral::prelude::*;

    #[test]
    fn test_to_base() -> Result<(), Box<dyn std::error::Error>> {
        assert_that!(f64::from(&Qty::from_str("1k")?)).is_close_to(f64::from(&Qty::from_str("1000000m")?), 0.01);
        assert_that!(Qty::from_str("1Ki")?).is_equal_to(Qty { value: 1, scale: Scale{ label: "Ki", base: 2, pow: 10 } });
        Ok(())
    }

    #[test]
    fn expectation_ok_for_adjust_scale() -> Result<(), Box<dyn std::error::Error>> {
        let cases = vec![
            ("1k", "1k"),
            ("10k", "10k"),
            ("100k", "100k"),
            ("999k", "999k"),
            ("1000k", "1M"),
            ("1999k", "1M"),
            ("1Ki", "1Ki"),
            ("10Ki", "10Ki"),
            ("100Ki", "100Ki"),
            ("1000Ki", "1000Ki"),
            ("1024Ki", "1Mi"),
        ];
        for (input, expected) in cases {
            assert_that!(&Qty::from_str(input)?.adjust_scale()).is_equal_to(&Qty::from_str(expected)?);
        }
        Ok(())
    }

    // #[test]
    // fn test_basic_to_unit_changes() -> Result<(), Box<dyn std::error::Error>> {
    //     assert_that!(Qty::from_str("1k")?.to_unit(SiUnit::from_str("")?)).is_equal_to(Qty::from_str("1000")?);
    //     assert_that!(Qty::from_str("1k")?.to_unit(SiUnit::from_str("m")?)).is_equal_to(Qty::from_str("1000000m")?);
    //     assert_that!(Qty::from_str("1Ki")?.to_unit(SiUnit::from_str("")?)).is_equal_to(Qty::from_str("1024")?);
    //     Ok(())
    // }

    // #[test]
    // fn test_add() -> Result<(), Box<dyn std::error::Error>> {
    //     assert_that!((&Qty::from_str("1Ki")?) + &Qty::from_str("1Ki")?).is_equal_to(Qty::from_str("2Ki")?);
    //     assert_that!((&Qty::from_str("1Ki")?) + &Qty::from_str("1k")?).is_equal_to(Qty::from_str("2024")?);
    //     Ok(())
    // }
}
