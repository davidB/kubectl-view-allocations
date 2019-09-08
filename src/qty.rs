// see [Binary prefix - Wikipedia](https://en.wikipedia.org/wiki/Binary_prefix)
// see [Managing Compute Resources for Containers - Kubernetes](https://kubernetes.io/docs/concepts/configuration/manage-compute-resources-container/)
//TODO rewrite to support exponent, ... see [apimachinery/quantity.go at master · kubernetes/apimachinery](https://github.com/kubernetes/apimachinery/blob/master/pkg/api/resource/quantity.go)

use std::str::FromStr;
use failure::Error;
use crate::human_format::{Formatter};

#[derive(Debug,Clone, PartialOrd, Default)]
pub struct Qty {
    value: f64,
}

impl Qty {
    pub fn calc_percentage(&self, base100: &Self) -> f64 {
    if self.value >= 0f64 && base100.value > 0f64 {
        self.value * 100f64 / base100.value
    } else {
        core::f64::NAN
    }
    }
}

impl FromStr for Qty {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Formatter's parse failed if no separator
        let value = Formatter::new().with_separator("").parse(s);
        Ok(Qty{value})
    }
}

impl std::fmt::Display for Qty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", Formatter::new().with_separator("").format(self.value))
    }
}

impl PartialEq for Qty {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

// impl PartialOrd for Qty {
//     fn partial_cmp(&self, other: &Self) -> Ordering {
//         if self.value > other.value {
//             Ordering::Greater
//         } else if self.value < other.value {
//             Ordering::Less
//         } else {
//             Ordering::Equal
//         }
//     }
// }

impl std::ops::Add for &Qty {
    type Output = Qty;
    //TODO optimize
    fn add(self, other: Self) -> Qty {
        let sum = Qty{
            value: self.value + other.value,
        };
        // sum.to_unit(SiUnit::smallest(&self.unit, &other.unit).clone())
        sum
    }
}

impl<'b> std::ops::AddAssign<&'b Qty> for Qty {
    fn add_assign(&mut self, other: &'b Self) {
        *self = Qty{
            value: self.value + other.value,
        };
    }
}

impl std::ops::Sub for &Qty {
    type Output = Qty;
    //TODO optimize
    fn sub(self, other: Self) -> Qty {
        Qty{
            value: self.value - other.value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectral::prelude::*;

    #[test]
    fn test_from_str() -> Result<(), Box<dyn std::error::Error>> {
        assert_that!(Qty::from_str("1k")?).is_equal_to(Qty::from_str("1000")?);
        assert_that!(Qty::from_str("1000000m")?).is_equal_to(Qty::from_str("1000")?);
        assert_that!(Qty::from_str("1Ki")?).is_equal_to(Qty::from_str("1024")?);
        Ok(())
    }

    #[test]
    fn test_add() -> Result<(), Box<dyn std::error::Error>> {
        assert_that!((&Qty::from_str("1Ki")?) + &Qty::from_str("1Ki")?).is_equal_to(Qty::from_str("2Ki")?);
        assert_that!((&Qty::from_str("1Ki")?) + &Qty::from_str("1k")?).is_equal_to(Qty::from_str("2024")?);
        Ok(())
    }
}
