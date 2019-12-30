//! Types for modeling and representing item metadata.

use std::collections::BTreeMap;
use std::convert::TryFrom;

use rust_decimal::Decimal;

use crate::util::Number;

/// Errors that can occur when working with `Value`.
#[derive(Debug, Copy, Clone, PartialEq, Hash)]
pub enum Error {
    Convert(ValueKind),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Self::Convert(ref source) => write!(f, "cannot convert value of kind {} into target type", source.as_ref()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

pub type Integer = i64;
pub type Boolean = bool;
pub type Sequence = Vec<Value>;
pub type Mapping = BTreeMap<String, Value>;

/// Represents the types of data that can be used as metadata.
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Hash, Deserialize, EnumDiscriminants)]
#[serde(untagged)]
#[strum_discriminants(name(ValueKind), derive(Hash, AsRefStr))]
pub enum Value {
    Null,
    String(String),
    Sequence(Sequence),
    Mapping(Mapping),
    Integer(Integer),
    Boolean(Boolean),
    Decimal(Decimal),
}

impl Value {
    /// Given a list of keys, looks up the subvalue at that key path of this value.
    /// This only works if this value is a mapping.
    pub fn get_key_path<S: AsRef<str>>(&self, key_path: &[S]) -> Option<&Self> {
        let mut curr_val = self;

        for key in key_path {
            // See if the current meta value is indeed a mapping.
            match curr_val {
                Self::Mapping(map) => {
                    // See if the current key in the key path is found in this mapping.
                    // If it is, set it as the new current value.
                    curr_val = map.get(key.as_ref())?;
                },

                // An attempt was made to get the key of a non-mapping, short circuit.
                _ => return None,
            }
        }

        // The remaining current value is what is needed to return.
        Some(curr_val)
    }
}

#[cfg(test)]
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl TryFrom<Value> for String {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::String(s) => Ok(s),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl<'k> TryFrom<&'k Value> for &'k str {
    type Error = Error;

    fn try_from(value: &'k Value) -> Result<Self, Self::Error> {
        match value {
            &Value::String(ref s) => Ok(s),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl From<Integer> for Value {
    fn from(i: Integer) -> Self {
        Self::Integer(i)
    }
}

impl TryFrom<Value> for Integer {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Integer(i) => Ok(i),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl<'k> TryFrom<&'k Value> for Integer {
    type Error = Error;

    fn try_from(value: &'k Value) -> Result<Self, Self::Error> {
        match value {
            &Value::Integer(i) => Ok(i),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl From<Boolean> for Value {
    fn from(b: Boolean) -> Self {
        Self::Boolean(b)
    }
}

impl TryFrom<Value> for Boolean {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Boolean(b) => Ok(b),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl<'k> TryFrom<&'k Value> for Boolean {
    type Error = Error;

    fn try_from(value: &'k Value) -> Result<Self, Self::Error> {
        match value {
            &Value::Boolean(b) => Ok(b),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl From<Decimal> for Value {
    fn from(d: Decimal) -> Self {
        Self::Decimal(d)
    }
}

impl TryFrom<Value> for Decimal {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Decimal(d) => Ok(d),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl<'k> TryFrom<&'k Value> for Decimal {
    type Error = Error;

    fn try_from(value: &'k Value) -> Result<Self, Self::Error> {
        match value {
            &Value::Decimal(d) => Ok(d),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl From<Sequence> for Value {
    fn from(s: Sequence) -> Self {
        Self::Sequence(s)
    }
}

impl TryFrom<Value> for Sequence {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Sequence(s) => Ok(s),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl From<Mapping> for Value {
    fn from(m: Mapping) -> Self {
        Self::Mapping(m)
    }
}

impl TryFrom<Value> for Mapping {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Mapping(m) => Ok(m),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl From<Number> for Value {
    fn from(nl: Number) -> Value {
        match nl {
            Number::Integer(i) => Self::from(i),
            Number::Decimal(d) => Self::from(d),
        }
    }
}

impl TryFrom<Value> for Number {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Integer(i) => Ok(Self::from(i)),
            Value::Decimal(d) => Ok(Self::from(d)),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

impl<'k> TryFrom<&'k Value> for Number {
    type Error = Error;

    fn try_from(value: &'k Value) -> Result<Self, Self::Error> {
        match value {
            &Value::Integer(i) => Ok(Self::Integer(i)),
            &Value::Decimal(d) => Ok(Self::Decimal(d)),
            _ => Err(Error::Convert(value.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize() {
        let inputs_and_expected = vec![
            ("null", Value::Null),
            (r#""string""#, Value::String(String::from("string"))),
            ("27", Value::Integer(27)),
            ("-27", Value::Integer(-27)),
            ("3.1415", Value::Decimal(dec!(3.1415))),
            ("-3.1415", Value::Decimal(dec!(-3.1415))),
            ("true", Value::Boolean(true)),
            ("false", Value::Boolean(false)),
            (
                r#"[null, "string", 27, true]"#,
                Value::Sequence(vec![
                    Value::Null,
                    Value::String(String::from("string")),
                    Value::Integer(27),
                    Value::Boolean(true),
                ]),
            ),
            (
                r#"{"key_a": "string", "key_b": -27, "key_c": false}"#,
                Value::Mapping(btreemap![
                    String::from("key_a") => Value::String(String::from("string")),
                    String::from("key_b") => Value::Integer(-27),
                    String::from("key_c") => Value::Boolean(false),
                ]),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = serde_json::from_str::<Value>(&input).unwrap();
            assert_eq!(expected, produced);
        }

        let inputs_and_expected = vec![
            ("null", Value::Null),
            ("~", Value::Null),
            (r#""string""#, Value::String(String::from("string"))),
            ("string", Value::String(String::from("string"))),
            ("27", Value::Integer(27)),
            ("-27", Value::Integer(-27)),
            ("3.1415", Value::Decimal(dec!(3.1415))),
            ("-3.1415", Value::Decimal(dec!(-3.1415))),
            ("true", Value::Boolean(true)),
            ("false", Value::Boolean(false)),
            (
                r#"[null, "string", 27, true]"#,
                Value::Sequence(vec![
                    Value::Null,
                    Value::String(String::from("string")),
                    Value::Integer(27),
                    Value::Boolean(true),
                ]),
            ),
            (
                "- null\n- string\n- 27\n- true",
                Value::Sequence(vec![
                    Value::Null,
                    Value::String(String::from("string")),
                    Value::Integer(27),
                    Value::Boolean(true),
                ]),
            ),
            (
                r#"{"key_a": "string", "key_b": -27, "key_c": false}"#,
                Value::Mapping(btreemap![
                    String::from("key_a") => Value::String(String::from("string")),
                    String::from("key_b") => Value::Integer(-27),
                    String::from("key_c") => Value::Boolean(false),
                ]),
            ),
            (
                "key_a: string\nkey_b: -27\nkey_c: false",
                Value::Mapping(btreemap![
                    String::from("key_a") => Value::String(String::from("string")),
                    String::from("key_b") => Value::Integer(-27),
                    String::from("key_c") => Value::Boolean(false),
                ]),
            ),
        ];

        for (input, expected) in inputs_and_expected {
            let produced = serde_yaml::from_str::<Value>(&input).unwrap();
            assert_eq!(expected, produced);
        }
    }

    #[test]
    fn test_get_key_path() {
        let key_str_a = "key_a";
        let key_str_b = "key_b";
        let key_str_c = "key_c";
        let key_str_x = "key_x";

        let val_nil = Value::Null;
        let val_str_a = Value::from("val_a");
        let val_str_b = Value::from("val_b");
        let val_str_c = Value::from("val_c");
        let val_seq_a = Value::from(vec![
            val_str_a.clone(), val_str_a.clone(), val_str_a.clone(),
        ]);
        let val_seq_b = Value::from(vec![
            val_str_b.clone(), val_str_b.clone(), val_str_b.clone(),
        ]);
        let val_seq_c = Value::from(vec![
            val_str_c.clone(), val_str_c.clone(), val_str_c.clone(),
        ]);
        let val_map_a = Value::from(btreemap![
            key_str_a.to_string() => val_str_a.clone(),
            key_str_b.to_string() => val_str_b.clone(),
            key_str_c.to_string() => val_str_c.clone(),
        ]);
        let val_map_b = Value::from(btreemap![
            key_str_a.to_string() => val_seq_a.clone(),
            key_str_b.to_string() => val_seq_b.clone(),
            key_str_c.to_string() => val_seq_c.clone(),
        ]);
        let val_map_c = Value::from(btreemap![
            key_str_a.to_string() => val_nil.clone(),
            key_str_b.to_string() => val_nil.clone(),
            key_str_c.to_string() => val_nil.clone(),
        ]);
        let val_map_d = Value::from(btreemap![
            key_str_a.to_string() => val_map_a.clone(),
            key_str_b.to_string() => val_map_b.clone(),
            key_str_c.to_string() => val_map_c.clone(),
        ]);

        let inputs_and_expected = vec![

            // An empty key path always returns the original value.
            ((&val_nil, vec![]), Some(&val_nil)),
            ((&val_str_a, vec![]), Some(&val_str_a)),
            ((&val_seq_a, vec![]), Some(&val_seq_a)),
            ((&val_map_a, vec![]), Some(&val_map_a)),

            // A non-empty key path returns no value on non-maps.
            ((&val_nil, vec![key_str_a]), None),
            ((&val_str_a, vec![key_str_a]), None),
            ((&val_seq_a, vec![key_str_a]), None),

            // If the key is not found in a mapping, nothing is returned.
            ((&val_map_a, vec![key_str_x]), None),
            ((&val_map_d, vec![key_str_a, key_str_x]), None),

            // Positive test cases.
            ((&val_map_a, vec![key_str_a]), Some(&val_str_a)),
            ((&val_map_b, vec![key_str_a]), Some(&val_seq_a)),
            ((&val_map_c, vec![key_str_a]), Some(&val_nil)),
            ((&val_map_d, vec![key_str_a]), Some(&val_map_a)),
            ((&val_map_a, vec![key_str_b]), Some(&val_str_b)),
            ((&val_map_b, vec![key_str_b]), Some(&val_seq_b)),
            ((&val_map_c, vec![key_str_b]), Some(&val_nil)),
            ((&val_map_d, vec![key_str_b]), Some(&val_map_b)),
            ((&val_map_a, vec![key_str_c]), Some(&val_str_c)),
            ((&val_map_b, vec![key_str_c]), Some(&val_seq_c)),
            ((&val_map_c, vec![key_str_c]), Some(&val_nil)),
            ((&val_map_d, vec![key_str_c]), Some(&val_map_c)),

            // Nested positive test cases.
            ((&val_map_d, vec![key_str_a, key_str_a]), Some(&val_str_a)),
            ((&val_map_d, vec![key_str_b, key_str_b]), Some(&val_seq_b)),
            ((&val_map_d, vec![key_str_c, key_str_c]), Some(&val_nil)),

        ];

        for (input, expected) in inputs_and_expected {
            let (val, key_path) = input;
            let produced = val.get_key_path(&key_path);
            assert_eq!(expected, produced);
        }
    }
}
