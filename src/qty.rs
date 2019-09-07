// see [Definitions of the SI units: The binary prefixes](https://physics.nist.gov/cuu/Units/binary.html)
// see [Managing Compute Resources for Containers - Kubernetes](https://kubernetes.io/docs/concepts/configuration/manage-compute-resources-container/)
//TODO rewrite to support exponent, ... see [apimachinery/quantity.go at master · kubernetes/apimachinery](https://github.com/kubernetes/apimachinery/blob/master/pkg/api/resource/quantity.go)

use std::str::FromStr;
use std::cmp::Ordering;
use failure::{Error,format_err};

#[derive(Debug,Clone,Eq,PartialEq, PartialOrd)]
struct SiUnit {
    label: &'static str,
    pow10: u32,
    pow2: u8,
}

static ALL: [SiUnit;11] = [
    SiUnit{ label:"", pow10: 3, pow2: 0},
    SiUnit{ label:"m", pow10: 0, pow2: 0},
    SiUnit{ label:"k", pow10: 6, pow2: 0},
    SiUnit{ label:"M", pow10: 6, pow2: 0},
    SiUnit{ label:"G", pow10: 6, pow2: 0},
    SiUnit{ label:"P", pow10: 6, pow2: 0},
    SiUnit{ label:"Ki", pow10: 3, pow2: 10},
    SiUnit{ label:"Mi", pow10: 3, pow2: 20},
    SiUnit{ label:"Gi", pow10: 3, pow2: 30},
    SiUnit{ label:"Ti", pow10: 3, pow2: 40},
    SiUnit{ label:"Pi", pow10: 3, pow2: 50},
];

impl Default for SiUnit {
    fn default() -> Self {
        SiUnit{ label:"", pow10: 3, pow2: 0}
    }
}

impl FromStr for SiUnit {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ALL.iter().find(|v| v.label == s).cloned().ok_or(format_err!("unit not found"))
    }
}

impl Ord for SiUnit {
    //TODO optimize accuracy with big number
    fn cmp(&self, other: &Self) -> Ordering {
        let u_self = (1_i64 << self.pow2) * 10_i64.pow(other.pow10);
        let u_other = (1_i64 << other.pow2) * 10_i64.pow(other.pow10);
        u_self.cmp(&u_other)
    }
}

#[derive(Debug,Clone, Eq, PartialOrd, Default)]
pub struct Qty {
    value: i64,
    unit: SiUnit, 
}

impl Qty {
    //TODO manage binary and mixe
    fn to_base(&self) -> Qty {
        Qty{
            value: (self.value << self.unit.pow2) * 10_i64.pow(self.unit.pow10),
            unit: SiUnit{ label:"m", pow10: 0, pow2: 0},
        }
    }
    
    //TODO optimize accuracy with big number
    fn to_unit(&self, unit: SiUnit) -> Qty {
        let b = self.to_base();
        let mut value = b.value;
        value = value >> unit.pow2;
        value = value / 10_i64.pow(unit.pow10);
        Qty {
            value, unit,
        }
        // let v = self.value;
        // if unit.coeff <= self.unit.coeff {
        //     let p = self.unit.coeff - unit.coeff;
        //     Qty {
        //         value: v * 10_i64.pow(p),
        //         unit,
        //     }
        // } else {
        //     let p = unit.coeff - self.unit.coeff;
        //     Qty {
        //         value: v / 10_i64.pow(p),
        //         unit,
        //     }
        // }
    }

    pub fn calc_percentage(&self, base100: &Self) -> f32 {
    if self.value >= 0 && base100.value > 0 {
        self.to_base().value as f32 * 100f32 / base100.to_base().value as f32
    } else {
        core::f32::NAN
    }
}

}

impl FromStr for Qty {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (num_str, unit_str): (&str, &str) = match s.find(|c: char| !c.is_digit(10)) {
            Some(pos) => (&s[..pos], &s[pos..]),
            None => (s, ""),
        };
        let value = i64::from_str(num_str)?;
        let unit = SiUnit::from_str(unit_str)?;
        Ok(Qty{value, unit})
    }
}

impl std::fmt::Display for Qty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.value, self.unit.label)
    }
}

impl PartialEq for Qty {
    fn eq(&self, other: &Self) -> bool {
        self.to_base().value == other.to_base().value
    }
}

impl Ord for Qty {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_base().value.cmp(&other.to_base().value)
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
    //TODO optimize
    fn add(self, other: Self) -> Qty {
        let v1 = self.to_base();
        let v2 = other.to_base();
        let sum = Qty{
            value: v1.value + v2.value,
            unit: v1.unit,
        };
        // sum.to_unit(SiUnit::smallest(&self.unit, &other.unit).clone())
        sum
    }
}

impl<'b> std::ops::AddAssign<&'b Qty> for Qty {
    fn add_assign(&mut self, other: &'b Self) {
        let v1 = self.to_base();
        let v2 = other.to_base();
        *self = Qty{
            value: v1.value + v2.value,
            unit: v1.unit,
        };
    }
}

impl std::ops::Sub for &Qty {
    type Output = Qty;
    //TODO optimize
    fn sub(self, other: Self) -> Qty {
        let v1 = self.to_base();
        let v2 = other.to_base();
        let sum = Qty{
            value: v1.value - v2.value,
            unit: v1.unit,
        };
        // sum.to_unit(SiUnit::smallest(&self.unit, &other.unit).clone())
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectral::prelude::*;

    #[test]
    fn test_to_base() -> Result<(), Box<dyn std::error::Error>> {
        assert_that!(Qty::from_str("1k")?.to_base()).is_equal_to(Qty::from_str("1000000m")?);
        assert_that!(Qty::from_str("1Ki")?.to_base()).is_equal_to(Qty { value: 1024*1000, unit: SiUnit { label: "m", pow10: 0, pow2: 0 } });
        Ok(())
    }

    #[test]
    fn test_basic_to_unit_changes() -> Result<(), Box<dyn std::error::Error>> {
        assert_that!(Qty::from_str("1k")?.to_unit(SiUnit::from_str("")?)).is_equal_to(Qty::from_str("1000")?);
        assert_that!(Qty::from_str("1k")?.to_unit(SiUnit::from_str("m")?)).is_equal_to(Qty::from_str("1000000m")?);
        assert_that!(Qty::from_str("1Ki")?.to_unit(SiUnit::from_str("")?)).is_equal_to(Qty::from_str("1024")?);
        Ok(())
    }

    #[test]
    fn test_add() -> Result<(), Box<dyn std::error::Error>> {
        assert_that!((&Qty::from_str("1Ki")?) + &Qty::from_str("1Ki")?).is_equal_to(Qty::from_str("2Ki")?);
        assert_that!((&Qty::from_str("1Ki")?) + &Qty::from_str("1k")?).is_equal_to(Qty::from_str("2024")?);
        Ok(())
    }
}