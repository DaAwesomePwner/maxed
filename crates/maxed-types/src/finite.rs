//! Reject nonfinite numbers instead of silently serializing them as JSON null.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! finite_number {
    ($module:ident, $number:ty) => {
        pub(crate) mod $module {
            use super::*;

            pub fn serialize<S: Serializer>(
                value: &$number,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                if !value.is_finite() {
                    return Err(serde::ser::Error::custom("expected a finite number"));
                }
                value.serialize(serializer)
            }

            pub fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<$number, D::Error> {
                let value = <$number>::deserialize(deserializer)?;
                if !value.is_finite() {
                    return Err(serde::de::Error::custom("expected a finite number"));
                }
                Ok(value)
            }
        }
    };
}

finite_number!(f64_value, f64);
finite_number!(f32_value, f32);

pub(crate) mod vec_f64 {
    use super::*;

    pub fn serialize<S: Serializer>(values: &[f64], serializer: S) -> Result<S::Ok, S::Error> {
        if values.iter().any(|value| !value.is_finite()) {
            return Err(serde::ser::Error::custom("expected finite numbers"));
        }
        values.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<f64>, D::Error> {
        let values = Vec::<f64>::deserialize(deserializer)?;
        if values.iter().any(|value| !value.is_finite()) {
            return Err(serde::de::Error::custom("expected finite numbers"));
        }
        Ok(values)
    }
}

pub(crate) mod optional_f64 {
    use super::*;

    pub fn serialize<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) if !value.is_finite() => {
                Err(serde::ser::Error::custom("expected a finite number"))
            }
            _ => value.serialize(serializer),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<f64>, D::Error> {
        let value = Option::<f64>::deserialize(deserializer)?;
        if value.is_some_and(|value| !value.is_finite()) {
            return Err(serde::de::Error::custom("expected a finite number"));
        }
        Ok(value)
    }
}
